mod api;
mod assets;
mod chat;
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
    /// Present when a search index/service was opened (serve). `None` in
    /// read-only test routers -> /api/search answers 503.
    pub search: Option<Arc<crate::search::SearchService>>,
    /// Present when daily arXiv recommendations are configured (`serve`).
    /// `None` -> /api/daily answers 503.
    pub daily: Option<Arc<crate::daily::DailyService>>,
    /// Present when paper chat is configured (`serve`). `None` -> chat
    /// endpoints answer 503 / available:false.
    pub chat: Option<Arc<crate::chat::ChatService>>,
    /// Present when [ai.citations] is configured (`serve`). `None` ->
    /// POST /api/papers/{id}/citations answers 503.
    pub citations: Option<Arc<crate::citations::CitationsService>>,
    /// UI-facing preferences (e.g. abstract folding). Defaulted in
    /// read-only/test routers; set from config in `serve`.
    pub ui: crate::config::UiConfig,
}

impl AppState {
    /// Nudge the background indexer after a mutation. No-op without search.
    pub fn wake_search(&self) {
        if let Some(s) = &self.search {
            s.wake();
        }
    }
}

/// Assemble the read-only web router (no import). Used directly by tests.
pub fn build_router(pool: SqlitePool, library_root: PathBuf) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: None,
        proxy_login_url: None,
        search: None,
        daily: None,
        chat: None,
        citations: None,
        ui: crate::config::UiConfig::default(),
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
        search: None,
        daily: None,
        chat: None,
        citations: None,
        ui: crate::config::UiConfig::default(),
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
        search: None,
        daily: None,
        chat: None,
        citations: None,
        ui: crate::config::UiConfig::default(),
    })
}

/// Read-only router plus a live search service. Used by tests.
pub fn build_router_with_search(
    pool: SqlitePool,
    library_root: PathBuf,
    search: Arc<crate::search::SearchService>,
) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: None,
        proxy_login_url: None,
        search: Some(search),
        daily: None,
        chat: None,
        citations: None,
        ui: crate::config::UiConfig::default(),
    })
}

/// Read-only router plus a daily-recommendations service. Used by tests.
pub fn build_router_with_daily(
    pool: SqlitePool,
    library_root: PathBuf,
    daily: Arc<crate::daily::DailyService>,
) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: None,
        proxy_login_url: None,
        search: None,
        daily: Some(daily),
        chat: None,
        citations: None,
        ui: crate::config::UiConfig::default(),
    })
}

/// Read-only router plus a configured chat service. Used by tests.
pub fn build_router_with_chat(
    pool: SqlitePool,
    library_root: PathBuf,
    chat: Arc<crate::chat::ChatService>,
) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: None,
        proxy_login_url: None,
        search: None,
        daily: None,
        chat: Some(chat),
        citations: None,
        ui: crate::config::UiConfig::default(),
    })
}

/// Test router with the citations service wired (everything else off).
pub fn build_router_with_citations(
    pool: SqlitePool,
    library_root: PathBuf,
    citations: Arc<crate::citations::CitationsService>,
) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: None,
        proxy_login_url: None,
        search: None,
        daily: None,
        chat: None,
        citations: Some(citations),
        ui: crate::config::UiConfig::default(),
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
        .route("/api/search", get(api::search_papers))
        .route("/api/search/status", get(api::search_status))
        .route("/api/daily", get(api::daily_papers))
        .route("/api/daily/run", axum::routing::post(api::run_daily))
        .route("/api/chat/models", get(chat::models))
        .route(
            "/api/papers/{id}/chat",
            get(chat::history).post(chat::send).delete(chat::clear),
        )
        .route(
            "/api/papers/{id}/citations",
            axum::routing::post(api::parse_citations),
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
    search: Option<Arc<crate::search::SearchService>>,
    daily: Option<Arc<crate::daily::DailyService>>,
    chat: Option<Arc<crate::chat::ChatService>>,
    citations: Option<Arc<crate::citations::CitationsService>>,
    ui: crate::config::UiConfig,
) -> Result<()> {
    let app = router_with(AppState {
        pool,
        library_root,
        ingest: Some(ingest),
        proxy_login_url,
        search,
        daily,
        chat,
        citations,
        ui,
    });
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
