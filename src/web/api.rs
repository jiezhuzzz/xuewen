use axum::extract::{Path, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::dto::{PaperDetail, PaperSummary, Stats};
use super::AppState;
use crate::db;

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
