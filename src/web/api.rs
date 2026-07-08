use axum::extract::multipart::MultipartError;
use axum::extract::{Multipart, Path, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use tower::ServiceExt;
use tower_http::services::ServeFile;
use uuid::Uuid;

use super::dto::{PaperDetail, PaperSummary, Stats};
use super::AppState;
use crate::db;
use crate::pipeline::Outcome;

#[derive(Deserialize)]
pub struct ListParams {
    pub q: Option<String>,
    pub status: Option<String>,
    pub sort: Option<String>,
}

pub async fn list_papers(State(app): State<AppState>, Query(p): Query<ListParams>) -> Response {
    match db::list_papers(
        &app.pool,
        p.q.as_deref(),
        p.status.as_deref(),
        p.sort.as_deref(),
    )
    .await
    {
        Ok(papers) => {
            let out: Vec<PaperSummary> = papers.iter().map(PaperSummary::from).collect();
            Json(out).into_response()
        }
        Err(e) => {
            tracing::error!("list_papers: {e}");
            internal_error()
        }
    }
}

pub async fn get_paper(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    match db::get_by_id(&app.pool, &id).await {
        Ok(Some(p)) => Json(PaperDetail::from(&p)).into_response(),
        Ok(None) => not_found(),
        Err(e) => {
            tracing::error!("get_paper: {e}");
            internal_error()
        }
    }
}

/// Soft-delete a paper (web mutation): flag it deleted; the file is untouched.
/// Purge (permanent removal) is CLI-only. Idempotent on an already-trashed paper.
pub async fn delete_paper(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    match db::get_by_id(&app.pool, &id).await {
        Ok(Some(_)) => match db::soft_delete(&app.pool, &id).await {
            Ok(_) => Json(serde_json::json!({ "deleted": true })).into_response(),
            Err(e) => {
                tracing::error!("delete_paper: {e}");
                internal_error()
            }
        },
        Ok(None) => not_found(),
        Err(e) => {
            tracing::error!("delete_paper lookup: {e}");
            internal_error()
        }
    }
}

/// Import a single uploaded PDF: validate, stage into `inbox_dir/_uploads`, and
/// run the ingest pipeline. One PDF per request (the frontend uploads files one
/// at a time). Returns `ingested` (with title/status), `duplicate`, or an error.
pub async fn import_paper(State(app): State<AppState>, mut multipart: Multipart) -> Response {
    let ingest = match &app.ingest {
        Some(i) => i.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "import not configured"})),
            )
                .into_response()
        }
    };

    // Take the first file part; skip any non-file fields.
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => return bad_request("no file"),
            Err(e) => return multipart_error(e),
        };
        let Some(filename) = field.file_name().map(|s| s.to_string()) else {
            continue;
        };
        let data = match field.bytes().await {
            Ok(b) => b,
            Err(e) => return multipart_error(e),
        };
        if !data.starts_with(b"%PDF") {
            return bad_request("not a PDF");
        }

        // Stage the bytes under a sanitized, collision-safe name.
        let stem = std::path::Path::new(&filename)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("upload.pdf");
        let staged = ingest
            .staging_dir
            .join(format!("{}-{stem}", Uuid::now_v7()));
        if let Err(e) = std::fs::create_dir_all(&ingest.staging_dir) {
            tracing::error!("import staging dir: {e}");
            return internal_error();
        }
        if let Err(e) = std::fs::write(&staged, data.as_ref()) {
            tracing::error!("import stage write: {e}");
            return internal_error();
        }

        return match ingest.ctx.ingest_file(&staged).await {
            Ok(Outcome::Ingested(id)) => {
                // Look up the fresh row so the UI can show title + resolved/needs_review.
                let (title, status) = match db::get_by_id(&ingest.ctx.pool, &id).await {
                    Ok(Some(p)) => (serde_json::json!(p.meta.title), p.meta.status),
                    _ => (
                        serde_json::Value::Null,
                        crate::models::PaperStatus::NeedsReview,
                    ),
                };
                Json(serde_json::json!({
                    "outcome": "ingested",
                    "id": id,
                    "title": title,
                    "status": status,
                }))
                .into_response()
            }
            Ok(Outcome::Duplicate) => {
                Json(serde_json::json!({"outcome": "duplicate"})).into_response()
            }
            Ok(Outcome::SameWork(id)) => {
                Json(serde_json::json!({"outcome": "same_work", "id": id})).into_response()
            }
            Ok(Outcome::InTrash(id)) => {
                Json(serde_json::json!({"outcome": "in_trash", "id": id})).into_response()
            }
            Err(e) => {
                tracing::error!("import ingest: {e}");
                // On error the pipeline did not move the original — clean it up.
                let _ = std::fs::remove_file(&staged);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "import failed"})),
                )
                    .into_response()
            }
        };
    }
}

pub async fn stats(State(app): State<AppState>) -> Response {
    match db::stats(&app.pool).await {
        Ok((total, resolved, needs_review)) => Json(Stats {
            total: total as usize,
            resolved: resolved as usize,
            needs_review: needs_review as usize,
        })
        .into_response(),
        Err(e) => {
            tracing::error!("stats: {e}");
            internal_error()
        }
    }
}

/// Stream a paper's PDF from the library. Range-aware (via `ServeFile`) and
/// path-safe: the resolved file must live under `library_root`.
pub async fn pdf(State(app): State<AppState>, Path(id): Path<String>, req: Request) -> Response {
    let paper = match db::get_by_id(&app.pool, &id).await {
        Ok(Some(p)) => p,
        Ok(None) => return not_found(),
        Err(e) => {
            tracing::error!("pdf lookup: {e}");
            return internal_error();
        }
    };
    let path = app.library_root.join(&paper.rel_path);
    // Defense in depth: the canonical file must live under the library root.
    let under_root = match (
        std::fs::canonicalize(&path),
        std::fs::canonicalize(&app.library_root),
    ) {
        (Ok(file), Ok(root)) => file.starts_with(&root),
        _ => false, // missing file or unresolvable path
    };
    if !under_root {
        return not_found();
    }
    match ServeFile::new(&path).oneshot(req).await {
        Ok(resp) => resp.map(axum::body::Body::new).into_response(),
        Err(e) => {
            tracing::error!("serve pdf: {e}");
            internal_error()
        }
    }
}

pub(super) fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "not found"})),
    )
        .into_response()
}

pub(super) fn internal_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "internal error"})),
    )
        .into_response()
}

pub(super) fn bad_request(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
        .into_response()
}

/// Map a multipart read error to its proper status (e.g. 413 when the body
/// exceeds the limit) with a JSON body.
fn multipart_error(e: MultipartError) -> Response {
    let status = e.status();
    (
        status,
        Json(serde_json::json!({
            "error": status.canonical_reason().unwrap_or("upload error").to_lowercase()
        })),
    )
        .into_response()
}
