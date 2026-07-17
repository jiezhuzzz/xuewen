//! Chat endpoints. POST streams SSE: `delta` events, then `done` (or
//! `error`). Persistence is all-or-nothing after the stream completes.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::Json;
use futures_util::StreamExt;
use serde_json::json;

use super::AppState;
use crate::chat::{context, store};
use crate::db;
use crate::llm::{ChatMessage, LlmClient};

#[derive(serde::Deserialize)]
pub struct ChatRequest {
    pub model_id: String,
    pub message: String,
}

fn sse_event(name: &str, data: serde_json::Value) -> Result<Event, std::convert::Infallible> {
    Ok(Event::default().event(name).data(data.to_string()))
}

pub async fn models(State(app): State<AppState>) -> Response {
    match &app.chat {
        None => Json(json!({ "available": false, "models": [] })).into_response(),
        Some(c) => {
            let models: Vec<_> = c
                .models
                .iter()
                .enumerate()
                .map(|(i, m)| json!({ "id": i.to_string(), "label": m.label }))
                .collect();
            Json(json!({ "available": true, "models": models })).into_response()
        }
    }
}

/// Look up a live (non-deleted) paper or answer 404/500. Mirrors the guard
/// pattern of the other paper endpoints in api.rs.
async fn live_paper(app: &AppState, id: &str) -> Result<crate::models::Paper, Response> {
    match db::get_by_id(&app.pool, id).await {
        Ok(Some(p)) if p.deleted_at.is_none() => Ok(p),
        Ok(_) => Err(StatusCode::NOT_FOUND.into_response()),
        Err(e) => {
            tracing::error!("chat paper lookup: {e}");
            Err(StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
    }
}

pub async fn history(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    let paper = match live_paper(&app, &id).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match store::list(&app.pool, &paper.id).await {
        Ok(rows) => Json(rows).into_response(),
        Err(e) => {
            tracing::error!("chat history: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn clear(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    let paper = match live_paper(&app, &id).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    match store::clear(&app.pool, &paper.id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("chat clear: {e}");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

pub async fn send(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<ChatRequest>,
) -> Response {
    let Some(chat) = app.chat.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": {"code": 503, "message": "chat is not configured"}})),
        )
            .into_response();
    };
    let paper = match live_paper(&app, &id).await {
        Ok(p) => p,
        Err(r) => return r,
    };
    let model = req
        .model_id
        .parse::<usize>()
        .ok()
        .and_then(|i| chat.models.get(i).cloned());
    let Some(model) = model else {
        return (StatusCode::BAD_REQUEST, "unknown model_id").into_response();
    };
    let user_msg = req.message.trim().to_string();
    if user_msg.is_empty() {
        return (StatusCode::BAD_REQUEST, "empty message").into_response();
    }
    let history = match store::list(&app.pool, &paper.id).await {
        Ok(rows) => rows,
        Err(e) => {
            tracing::error!("chat history for send: {e}");
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        }
    };

    let text = chat.paper_text(&app.library_root, &paper).await;
    let system = context::system_prompt(
        &paper,
        text.as_deref().map(|s| s.as_str()),
        chat.max_context_chars,
    );
    let mut messages = vec![ChatMessage {
        role: "system",
        content: system,
    }];
    for row in &history {
        messages.push(ChatMessage {
            role: if row.role == "user" {
                "user"
            } else {
                "assistant"
            },
            content: row.content.clone(),
        });
    }
    messages.push(ChatMessage {
        role: "user",
        content: user_msg.clone(),
    });

    let client = LlmClient::new(&model.base_url, &model.model, model.api_key.clone())
        .with_reasoning_effort(model.reasoning_effort.clone());
    let (pool, paper_id, label) = (app.pool.clone(), paper.id.clone(), model.label.clone());

    let stream = async_stream::stream! {
        let upstream = match client.stream(&messages).await {
            Ok(s) => s,
            Err(e) => {
                yield sse_event("error", json!({ "message": e.to_string() }));
                return;
            }
        };
        futures_util::pin_mut!(upstream);
        let mut full = String::new();
        while let Some(item) = upstream.next().await {
            match item {
                Ok(delta) => {
                    full.push_str(&delta);
                    yield sse_event("delta", json!({ "text": delta }));
                }
                Err(e) => {
                    yield sse_event("error", json!({ "message": e.to_string() }));
                    return;
                }
            }
        }
        if full.is_empty() {
            yield sse_event(
                "error",
                json!({ "message": "the model returned an empty reply" }),
            );
            return;
        }
        // Client disconnects drop this stream before we get here, so
        // nothing is persisted for aborted generations.
        match store::insert_exchange(&pool, &paper_id, &user_msg, &full, &label, None).await {
            Ok(assistant_id) => yield sse_event("done", json!({ "id": assistant_id })),
            Err(e) => yield sse_event("error", json!({ "message": format!("saving the exchange failed: {e}") })),
        }
    };
    Sse::new(stream)
        .keep_alive(KeepAlive::default())
        .into_response()
}
