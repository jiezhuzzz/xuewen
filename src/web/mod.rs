mod api;
mod assets;
mod dto;

use std::path::PathBuf;

use anyhow::Result;
use axum::routing::get;
use axum::Router;
use sqlx::SqlitePool;

/// Shared state for the web handlers.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub library_root: PathBuf,
}

/// Assemble the read-only web router (pure — used directly by tests).
pub fn build_router(pool: SqlitePool, library_root: PathBuf) -> Router {
    let state = AppState { pool, library_root };
    Router::new()
        .route("/api/papers", get(api::list_papers))
        .route("/api/papers/{id}", get(api::get_paper))
        .route("/api/stats", get(api::stats))
        .route("/papers/{id}/pdf", get(api::pdf))
        .fallback(assets::static_handler)
        .with_state(state)
}

/// Bind `host:port` and serve the router until the process is stopped.
pub async fn serve(host: &str, port: u16, pool: SqlitePool, library_root: PathBuf) -> Result<()> {
    let app = build_router(pool, library_root);
    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("xuewen serving on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
