use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

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
