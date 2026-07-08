mod api;
mod assets;
mod dto;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use axum::Router;
use sqlx::SqlitePool;

use crate::pipeline::Libraries;
use crate::resolve::grobid::Grobid;
use crate::resolve::Resolver;

/// Everything the web import handler needs to run the ingest pipeline. Held
/// behind an `Arc` in `AppState` because `Resolver`/`Grobid` are not `Clone`.
pub struct Ingest {
    pub resolver: Resolver,
    pub grobid: Option<Grobid>,
    pub dirs: Libraries,
    /// Where uploaded bytes are written before ingest (`inbox_dir/_uploads`).
    pub staging_dir: PathBuf,
}

/// Shared state for the web handlers.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub library_root: PathBuf,
    /// Present only when the server was started to allow uploads (`serve`).
    pub ingest: Option<Arc<Ingest>>,
}

/// Assemble the read-only web router (no import). Used directly by tests.
pub fn build_router(pool: SqlitePool, library_root: PathBuf) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: None,
    })
}

/// Assemble the full web router, including the import endpoint.
pub fn build_router_with_ingest(
    pool: SqlitePool,
    library_root: PathBuf,
    ingest: Arc<Ingest>,
) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: Some(ingest),
    })
}

fn router_with(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/papers",
            get(api::list_papers)
                .post(api::import_paper)
                .layer(DefaultBodyLimit::max(50 * 1024 * 1024)),
        )
        .route(
            "/api/papers/{id}",
            get(api::get_paper).delete(api::delete_paper),
        )
        .route("/api/stats", get(api::stats))
        .route("/papers/{id}/pdf", get(api::pdf))
        .fallback(assets::static_handler)
        .with_state(state)
}

/// Bind `host:port` and serve the router until the process is stopped.
pub async fn serve(
    host: &str,
    port: u16,
    pool: SqlitePool,
    library_root: PathBuf,
    ingest: Arc<Ingest>,
) -> Result<()> {
    let app = build_router_with_ingest(pool, library_root, ingest);
    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("xuewen serving on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
