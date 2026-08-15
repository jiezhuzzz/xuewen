//! The one error type web handlers return. Every variant serializes to the
//! API's single error body shape — `{"error": "<message>"}` — so no handler
//! can invent a divergent wire format, and `Internal` is the one place a
//! db/service failure is logged (attach the operation name with
//! `anyhow::Context` so the log line still says what failed).

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;

pub enum ApiError {
    /// 404 with the generic "not found" body.
    NotFound,
    /// 404 with an actionable message (e.g. identify's title-search hint).
    NotFoundMsg(String),
    /// 400.
    BadRequest(String),
    /// 409; `id` names the winning paper when the conflict has one (identify).
    Conflict { message: String, id: Option<String> },
    /// 422 — a well-formed request whose content fails validation.
    Unprocessable(String),
    /// 503 — the service this endpoint needs isn't configured.
    Unavailable(&'static str),
    /// 502 — an upstream fetch/provider call failed.
    BadGateway(&'static str),
    /// 500, logged here rather than at every call site.
    Internal(anyhow::Error),
}

impl ApiError {
    /// Map a failed db write: a UNIQUE violation is the caller's 409 with
    /// `message`; anything else is a 500 logged under `ctx`.
    pub fn from_db_conflict(e: anyhow::Error, ctx: &'static str, message: &'static str) -> Self {
        if crate::db::is_unique_violation(&e) {
            ApiError::Conflict {
                message: message.to_string(),
                id: None,
            }
        } else {
            ApiError::Internal(e.context(ctx))
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        ApiError::Internal(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, body) = match self {
            ApiError::NotFound => (StatusCode::NOT_FOUND, json!({"error": "not found"})),
            ApiError::NotFoundMsg(m) => (StatusCode::NOT_FOUND, json!({ "error": m })),
            ApiError::BadRequest(m) => (StatusCode::BAD_REQUEST, json!({ "error": m })),
            ApiError::Conflict { message, id } => (
                StatusCode::CONFLICT,
                match id {
                    Some(id) => json!({"error": message, "id": id}),
                    None => json!({ "error": message }),
                },
            ),
            ApiError::Unprocessable(m) => (StatusCode::UNPROCESSABLE_ENTITY, json!({ "error": m })),
            ApiError::Unavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, json!({ "error": m })),
            ApiError::BadGateway(m) => (StatusCode::BAD_GATEWAY, json!({ "error": m })),
            ApiError::Internal(e) => {
                tracing::error!("{e:#}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    json!({"error": "internal error"}),
                )
            }
        };
        (status, Json(body)).into_response()
    }
}
