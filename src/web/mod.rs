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
    /// Present when Agent Ask is configured (`serve`). `None` -> chat
    /// endpoints answer 503 / available:false.
    pub agent: Option<Arc<crate::agent::AgentService>>,
    /// Always present: heuristic parsing needs no config; [ai.citations]
    /// adds the LLM fallback for entries heuristics can't parse.
    pub citations: Arc<crate::citations::CitationsService>,
    /// Present when translate-on-selection is configured (`serve`). `None`
    /// -> /api/translate answers 503, /api/settings reports disabled.
    pub translate: Option<Arc<crate::translate::TranslateService>>,
    /// UI-facing preferences (e.g. abstract folding). Defaulted in
    /// read-only/test routers; set from config in `serve`.
    pub ui: crate::config::UiConfig,
}

impl AppState {
    /// Base state with every optional service off (heuristics-only
    /// citations, no ingest/search/daily/agent, default UI prefs). The
    /// `build_router*` helpers below flip on just what they need.
    fn base(pool: SqlitePool, library_root: PathBuf) -> Self {
        let citations = crate::citations::CitationsService::heuristic_only(pool.clone());
        Self {
            pool,
            library_root,
            ingest: None,
            proxy_login_url: None,
            search: None,
            daily: None,
            agent: None,
            citations,
            translate: None,
            ui: crate::config::UiConfig::default(),
        }
    }

    /// Nudge the background indexer after a mutation. No-op without search.
    pub fn wake_search(&self) {
        if let Some(s) = &self.search {
            s.wake();
        }
    }
}

/// Assemble the read-only web router (no import). Used directly by tests.
pub fn build_router(pool: SqlitePool, library_root: PathBuf) -> Router {
    router_with(AppState::base(pool, library_root))
}

/// Assemble the full web router, including the import endpoint.
pub fn build_router_with_ingest(
    pool: SqlitePool,
    library_root: PathBuf,
    ingest: Arc<Ingest>,
) -> Router {
    let mut state = AppState::base(pool, library_root);
    state.ingest = Some(ingest);
    router_with(state)
}

/// Full router with import + a configured proxy prefix. Used by tests.
pub fn build_router_with_ingest_proxy(
    pool: SqlitePool,
    library_root: PathBuf,
    ingest: Arc<Ingest>,
    proxy_login_url: Option<String>,
) -> Router {
    let mut state = AppState::base(pool, library_root);
    state.ingest = Some(ingest);
    state.proxy_login_url = proxy_login_url;
    router_with(state)
}

/// Read-only router plus a live search service. Used by tests.
pub fn build_router_with_search(
    pool: SqlitePool,
    library_root: PathBuf,
    search: Arc<crate::search::SearchService>,
) -> Router {
    let mut state = AppState::base(pool, library_root);
    state.search = Some(search);
    router_with(state)
}

/// Read-only router plus a daily-recommendations service. Used by tests.
pub fn build_router_with_daily(
    pool: SqlitePool,
    library_root: PathBuf,
    daily: Arc<crate::daily::DailyService>,
) -> Router {
    let mut state = AppState::base(pool, library_root);
    state.daily = Some(daily);
    router_with(state)
}

/// Read-only router plus a configured agent service. Used by tests.
pub fn build_router_with_agent(
    pool: SqlitePool,
    library_root: PathBuf,
    agent: Arc<crate::agent::AgentService>,
) -> Router {
    let mut state = AppState::base(pool, library_root);
    state.agent = Some(agent);
    router_with(state)
}

/// Test router with the citations service wired (everything else off).
pub fn build_router_with_citations(
    pool: SqlitePool,
    library_root: PathBuf,
    citations: Arc<crate::citations::CitationsService>,
) -> Router {
    let mut state = AppState::base(pool, library_root);
    state.citations = citations;
    router_with(state)
}

/// Read-only router plus a configured translate service. Used by tests.
pub fn build_router_with_translate(
    pool: SqlitePool,
    library_root: PathBuf,
    translate: Arc<crate::translate::TranslateService>,
) -> Router {
    let mut state = AppState::base(pool, library_root);
    state.translate = Some(translate);
    router_with(state)
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
        .route(
            "/api/papers/{id}/restore",
            axum::routing::post(api::restore_paper),
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
        .route("/api/tags", get(api::list_tags))
        .route(
            "/api/tags/{id}",
            axum::routing::patch(api::rename_tag).delete(api::delete_tag),
        )
        .route(
            "/api/papers/{paper_id}/tags",
            axum::routing::put(api::add_paper_tag),
        )
        .route(
            "/api/papers/{paper_id}/tags/{tag_id}",
            axum::routing::delete(api::remove_paper_tag),
        )
        .route(
            "/api/papers/{id}/star",
            axum::routing::put(api::star_paper).delete(api::unstar_paper),
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
        .route(
            "/api/papers/{id}/code",
            get(api::get_paper_code)
                .put(api::set_paper_code)
                .delete(api::delete_paper_code),
        )
        .route("/api/translate", axum::routing::post(api::translate))
        .fallback(assets::static_handler)
        .with_state(state)
}

/// Bind `host:port` and serve the router until the process is stopped.
#[allow(clippy::too_many_arguments)]
pub async fn serve(
    host: &str,
    port: u16,
    pool: SqlitePool,
    library_root: PathBuf,
    ingest: Arc<Ingest>,
    proxy_login_url: Option<String>,
    search: Option<Arc<crate::search::SearchService>>,
    daily: Option<Arc<crate::daily::DailyService>>,
    agent: Option<Arc<crate::agent::AgentService>>,
    citations: Arc<crate::citations::CitationsService>,
    translate: Option<Arc<crate::translate::TranslateService>>,
    ui: crate::config::UiConfig,
) -> Result<()> {
    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("xuewen serving on http://{addr}");
    serve_on(
        listener,
        AppState {
            pool,
            library_root,
            ingest: Some(ingest),
            proxy_login_url,
            search,
            daily,
            agent,
            citations,
            translate,
            ui,
        },
    )
    .await
}

/// Serve the full router on a listener the caller has already bound —
/// lets the caller bind port 0 and learn the real port from
/// `listener.local_addr()` before starting the server. Shuts down
/// gracefully on SIGINT/SIGTERM (see `shutdown_signal`) — the container
/// runs this as PID 1, where an unhandled signal means a hung stop.
pub async fn serve_on(listener: tokio::net::TcpListener, state: AppState) -> Result<()> {
    let app = router_with(state);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

/// Resolve on SIGINT or SIGTERM. The container runs the server as PID 1,
/// which gets no default signal disposition — without an explicit handler
/// `docker stop` and Kubernetes rollouts hang the full grace period and
/// end in SIGKILL.
async fn shutdown_signal() {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install the Ctrl-C handler");
    };
    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install the SIGTERM handler")
            .recv()
            .await;
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
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
