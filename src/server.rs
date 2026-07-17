//! Server assembly shared by the CLI (`xuewen serve`) and the desktop app:
//! build every optional service from config, spawn their background loops,
//! and serve the web router on a caller-provided listener.

use anyhow::Result;
use sqlx::SqlitePool;
use std::future::Future;
use std::sync::Arc;

use crate::config::Config;
use crate::daily::{self, DailyService};
use crate::pipeline::{IngestCtx, Libraries};
use crate::resolve::grobid::Grobid;
use crate::resolve::http::RetryPolicy;
use crate::resolve::Resolver;
use crate::search::{indexer, SearchService};
use crate::web;

/// Every service the web router takes, built from one `Config`.
pub struct Services {
    pub ingest: Arc<web::Ingest>,
    pub search: Option<Arc<SearchService>>,
    pub daily: Option<Arc<DailyService>>,
    pub agent: Option<Arc<crate::agent::AgentService>>,
    pub citations: Arc<crate::citations::CitationsService>,
    pub translate: Option<Arc<crate::translate::TranslateService>>,
}

/// Build all services and spawn their background loops (indexer, daily
/// scheduler, summary worker). Interactive retry policy: uploads answer
/// synchronously, so keep resolver retries short.
pub async fn spawn_services(cfg: &Config, pool: SqlitePool) -> Result<Services> {
    let resolver =
        Resolver::new_with_policy(cfg.contact_email.as_deref(), RetryPolicy::interactive())?;
    let grobid = cfg.grobid_url.as_deref().map(Grobid::new).transpose()?;
    let ctx = IngestCtx {
        pool: pool.clone(),
        dirs: Libraries {
            library_root: cfg.library_root.clone(),
            processed_dir: cfg.inbox_dir.join("_processed"),
        },
        resolver,
        grobid,
    };
    let ingest = Arc::new(web::Ingest {
        ctx,
        staging_dir: cfg.inbox_dir.join("_uploads"),
    });
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
    let citations = crate::citations::CitationsService::from_config(pool.clone(), cfg);
    let translate = crate::translate::TranslateService::from_config(cfg).map(Arc::new);
    Ok(Services {
        ingest,
        search,
        daily,
        agent,
        citations,
        translate,
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
        proxy_login_url: cfg.proxy.as_ref().map(|p| p.login_url.clone()),
        search: services.search,
        daily: services.daily,
        agent: services.agent,
        citations: services.citations,
        translate: services.translate,
        ui: cfg.ui.clone(),
    };
    web::serve_on(listener, state)
}
