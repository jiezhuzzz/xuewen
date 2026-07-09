use axum::extract::multipart::MultipartError;
use axum::extract::{Multipart, Path, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use tower::ServiceExt;
use tower_http::services::ServeFile;
use uuid::Uuid;

use super::dto::{Candidate, PaperDetail, PaperSummary, Stats};
use super::AppState;
use crate::db;
use crate::pipeline::{IdentifyOutcome, Outcome};

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
        if let Err(e) = tokio::fs::create_dir_all(&ingest.staging_dir).await {
            tracing::error!("import staging dir: {e}");
            return internal_error();
        }
        if let Err(e) = tokio::fs::write(&staged, data.as_ref()).await {
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
                let _ = tokio::fs::remove_file(&staged).await;
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
    let under_root = {
        let (p, root) = (path.clone(), app.library_root.clone());
        tokio::task::spawn_blocking(move || {
            match (std::fs::canonicalize(&p), std::fs::canonicalize(&root)) {
                (Ok(file), Ok(root)) => file.starts_with(&root),
                _ => false, // missing file or unresolvable path
            }
        })
        .await
        .inspect_err(|e| tracing::error!("canonicalize check panicked: {e}"))
        .unwrap_or(false)
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

#[derive(Deserialize)]
pub struct IdentifyQuery {
    pub q: Option<String>,
}

/// Ungated candidate search for manual identify (the user is the gate).
pub async fn identify_search(
    State(app): State<AppState>,
    Query(p): Query<IdentifyQuery>,
) -> Response {
    let Some(ingest) = &app.ingest else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "identify not configured"})),
        )
            .into_response();
    };
    let Some(q) = p.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return bad_request("missing query");
    };
    let cands = ingest.ctx.resolver.search_candidates(q).await;
    let out: Vec<Candidate> = cands.iter().map(Candidate::from).collect();
    Json(out).into_response()
}

#[derive(Deserialize)]
pub struct IdentifyBody {
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub candidate: Option<Candidate>,
}

/// Apply a user-confirmed match: fetch authoritative metadata for a DOI or
/// arXiv id (or take a picked search candidate as-is), overwrite the paper's
/// metadata, and re-file. The user's confirmation replaces the confidence gate.
pub async fn identify_paper(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<IdentifyBody>,
) -> Response {
    let Some(ingest) = &app.ingest else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "identify not configured"})),
        )
            .into_response();
    };
    let selectors =
        body.doi.is_some() as u8 + body.arxiv_id.is_some() as u8 + body.candidate.is_some() as u8;
    if selectors != 1 {
        return bad_request("provide exactly one of doi, arxiv_id, candidate");
    }

    // Read through the ctx pool: same handle the apply path writes with
    // (matches the pool-locality convention set in import_paper).
    let mut paper = match db::get_by_id(&ingest.ctx.pool, &id).await {
        Ok(Some(p)) => p,
        Ok(None) => return not_found(),
        Err(e) => {
            tracing::error!("identify lookup: {e}");
            return internal_error();
        }
    };
    if paper.deleted_at.is_some() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "paper is in the trash"})),
        )
            .into_response();
    }

    let md = if let Some(c) = body.candidate {
        Some(c.into_metadata())
    } else if let Some(doi) = &body.doi {
        ingest
            .ctx
            .resolver
            .resolve(&crate::models::Identifier::Doi(doi.clone()), None)
            .await
    } else if let Some(axv) = &body.arxiv_id {
        ingest
            .ctx
            .resolver
            .resolve(&crate::models::Identifier::Arxiv(axv.clone()), None)
            .await
    } else {
        unreachable!("selector count checked above")
    };
    let Some(md) = md else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "identifier not found"})),
        )
            .into_response();
    };

    let (md_doi, md_arxiv) = (md.doi.clone(), md.arxiv_id.clone());
    match ingest.ctx.apply_match(&mut paper, md).await {
        Ok(IdentifyOutcome::Applied) => Json(PaperDetail::from(&paper)).into_response(),
        Ok(IdentifyOutcome::SameWork(other)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": format!("same work as {other}"), "id": other})),
        )
            .into_response(),
        Err(e) => {
            // Lost a race: something claimed this identifier between the guard and
            // the update. Report it as the conflict it is, mirroring ingest.
            if db::is_unique_violation(&e) {
                if let Ok(Some(existing)) =
                    db::find_by_identifier(&ingest.ctx.pool, md_doi.as_deref(), md_arxiv.as_deref())
                        .await
                {
                    return (
                        StatusCode::CONFLICT,
                        Json(serde_json::json!({
                            "error": format!("same work as {}", existing.id),
                            "id": existing.id,
                        })),
                    )
                        .into_response();
                }
            }
            tracing::error!("identify apply: {e}");
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
