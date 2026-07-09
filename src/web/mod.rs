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

use crate::pipeline::IngestCtx;

/// Everything the web import handler needs to run the ingest pipeline. Held
/// behind an `Arc` in `AppState` because `IngestCtx` is not `Clone` (its
/// `Resolver`/`Grobid` are not).
pub struct Ingest {
    pub ctx: IngestCtx,
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
    /// EZproxy login prefix (from `[proxy].login_url`); `None` disables proxy fetch.
    pub proxy_login_url: Option<String>,
}

/// Assemble the read-only web router (no import). Used directly by tests.
pub fn build_router(pool: SqlitePool, library_root: PathBuf) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: None,
        proxy_login_url: None,
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
        proxy_login_url: None,
    })
}

/// Full router with import + a configured proxy prefix. Used by `serve`.
pub fn build_router_with_ingest_proxy(
    pool: SqlitePool,
    library_root: PathBuf,
    ingest: Arc<Ingest>,
    proxy_login_url: Option<String>,
) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: Some(ingest),
        proxy_login_url,
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
        .route("/api/papers/export", get(api::export_papers))
        .route(
            "/api/papers/{id}",
            get(api::get_paper).delete(api::delete_paper),
        )
        .route("/api/papers/{id}/export", get(api::export_paper))
        .route("/api/stats", get(api::stats))
        .route("/api/identify/search", get(api::identify_search))
        .route(
            "/api/papers/{id}/identify",
            axum::routing::post(api::identify_paper),
        )
        .route("/papers/{id}/pdf", get(api::pdf))
        .route("/api/import", axum::routing::post(api::import_url))
        .route("/api/settings", get(api::get_settings))
        .route(
            "/api/settings/proxy-cookie",
            axum::routing::put(api::set_proxy_cookie).delete(api::clear_proxy_cookie),
        )
        .route(
            "/api/projects",
            get(api::list_projects).post(api::create_project),
        )
        .route(
            "/api/projects/{id}",
            axum::routing::patch(api::update_project).delete(api::delete_project),
        )
        .route(
            "/api/papers/{paper_id}/projects/{project_id}",
            axum::routing::put(api::add_paper_to_project).delete(api::remove_paper_from_project),
        )
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
    proxy_login_url: Option<String>,
) -> Result<()> {
    let app = build_router_with_ingest_proxy(pool, library_root, ingest, proxy_login_url);
    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("xuewen serving on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

/// Whether `host` is a loopback bind (safe to serve without auth). Non-IP
/// hostnames other than "localhost" are conservatively treated as remote.
pub fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::is_loopback_host;

    #[test]
    fn classifies_loopback_hosts() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.1.2.3"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.10"));
        assert!(!is_loopback_host("example.com"));
    }
}
