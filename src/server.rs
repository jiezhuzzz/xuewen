//! Server assembly shared by the CLI (`xuewen serve`) and the desktop app:
//! build every optional service from config, spawn their background loops,
//! and serve the web router on a caller-provided listener.

use anyhow::Result;
use sqlx::SqlitePool;
use std::future::Future;
use std::sync::Arc;

use crate::config::Config;
use crate::daily::{self, DailyService};
use crate::http::RetryPolicy;
use crate::pipeline::IngestCtx;
use crate::search::{indexer, SearchService};
use crate::web;

/// Every service the web router takes, built from one `Config`.
pub struct Services {
    pub ingest: Arc<web::Ingest>,
    pub search: Option<Arc<SearchService>>,
    pub daily: Option<Arc<DailyService>>,
    pub agent: Option<Arc<crate::agent::AgentService>>,
    pub citations: Arc<crate::citations::CitationsService>,
    pub annotations: Arc<crate::annotations::AnnotationsService>,
    pub translate: Option<Arc<crate::translate::TranslateService>>,
    pub preview: Arc<crate::preview::PreviewService>,
}

/// Build all services and spawn their background loops (indexer, daily
/// scheduler, summary worker). Interactive retry policy: uploads answer
/// synchronously, so keep resolver retries short.
pub async fn spawn_services(cfg: &Config, pool: SqlitePool) -> Result<Services> {
    let ctx = IngestCtx::from_config(cfg, pool.clone(), RetryPolicy::interactive())?;
    // Built once: the Fetcher's reqwest clients hold connection pools that
    // per-request construction would throw away.
    let fetcher = crate::import::Fetcher::new(cfg.proxy.as_ref().map(|p| p.login_url.clone()))?;
    let ingest = Arc::new(web::Ingest { ctx, fetcher });
    let search = match SearchService::open(pool.clone(), &cfg.search, &cfg.ai).await {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!("search disabled: {e}");
            None
        }
    };
    if let Some(s) = &search {
        tokio::spawn(indexer::run(
            s.clone(),
            cfg.library_root.clone(),
            std::time::Duration::from_secs(30),
        ));
    }
    let daily = DailyService::from_config(cfg, pool.clone())?;
    if let Some(d) = &daily {
        tokio::spawn(daily::scheduler::run(d.clone()));
    }
    if let Some(s) = crate::summary::SummaryService::from_config(pool.clone(), cfg) {
        tokio::spawn(crate::summary::run(s, std::time::Duration::from_secs(60)));
    }
    let agent = crate::agent::AgentService::from_config(&cfg.ai.agent);
    match &agent {
        None => tracing::info!("agent ask disabled (no [ai.agent] backends)"),
        Some(a) => {
            for p in a.preflight().await {
                tracing::warn!("agent ask: {p}");
            }
        }
    }
    // Unconditional (rows and staging dirs can predate an [ai.agent] removal):
    // resolve 'cloning' rows a crash stranded and sweep orphaned staging dirs.
    crate::agent::code::reconcile_startup(&pool, &cfg.library_root).await;
    let citations = crate::citations::CitationsService::from_config(pool.clone(), cfg);
    // No config to read: annotations are plain storage, always on.
    let annotations = Arc::new(crate::annotations::AnnotationsService::new(pool.clone()));
    let translate = crate::translate::TranslateService::from_config(cfg).map(Arc::new);
    // No config to gate, only a place to cache: previews are always on.
    let preview = Arc::new(crate::preview::PreviewService::new(
        cfg.preview.cache_dir.clone(),
    ));
    Ok(Services {
        ingest,
        search,
        daily,
        agent,
        citations,
        annotations,
        translate,
        preview,
    })
}

/// Serve the router on `listener`. Non-async on purpose: everything is
/// cloned out of `cfg` up front so the returned future is `'static` and
/// can be `tokio::spawn`ed by a caller that keeps using `cfg`.
pub fn serve_on(
    listener: tokio::net::TcpListener,
    pool: SqlitePool,
    cfg: &Config,
    services: Services,
) -> impl Future<Output = Result<()>> {
    let state = web::AppState {
        pool,
        library_root: cfg.library_root.clone(),
        ingest: Some(services.ingest),
        search: services.search,
        daily: services.daily,
        agent: services.agent,
        citations: services.citations,
        annotations: services.annotations,
        translate: services.translate,
        preview: services.preview,
        ui: cfg.ui.clone(),
    };
    web::serve_on(listener, state)
}
