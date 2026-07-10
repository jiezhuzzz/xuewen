pub mod feed;
pub mod job;
pub mod score;
pub mod store;
pub mod tldr;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::{Config, DailyConfig};
use crate::resolve::http::{HttpClient, RetryPolicy};
use crate::search::embedder::Embedder;
use crate::search::vector::QdrantStore;

pub const ARXIV_FEED_BASE: &str = "https://rss.arxiv.org/atom";
pub const ARXIV_PDF_BASE: &str = "https://arxiv.org/pdf";
pub const ARXIV_ABS_BASE: &str = "https://arxiv.org/abs";

/// Daily arXiv recommendations. Owns its own HTTP clients (all stateless)
/// so it stays independent of `SearchService`.
pub struct DailyService {
    pub cfg: DailyConfig,
    pub pool: SqlitePool,
    /// Feed fetches: retried like the resolvers.
    pub(crate) http: HttpClient,
    /// PDF downloads (bytes; single attempt — the TL;DR chain absorbs failures).
    pub(crate) plain_http: reqwest::Client,
    pub(crate) embedder: Embedder,
    pub(crate) vectors: QdrantStore,
    pub(crate) chat: tldr::ChatClient,
    pub(crate) feed_base: String,
    pub(crate) pdf_base: String,
    running: AtomicBool,
}

impl DailyService {
    /// `Ok(None)` when the feature is off: no `[daily]` section, no
    /// `[search.embedding]`, or a missing API key (each case warns).
    /// `Err` only on invalid `[daily]` values.
    pub fn from_config(cfg: &Config, pool: SqlitePool) -> anyhow::Result<Option<Arc<Self>>> {
        let Some(daily) = &cfg.daily else { return Ok(None) };
        if daily.categories.is_empty() {
            anyhow::bail!("[daily].categories must not be empty");
        }
        let Some(embed_cfg) = &cfg.search.embedding else {
            tracing::warn!("[daily] set but [search.embedding] missing — daily papers disabled");
            return Ok(None);
        };
        let Some(embedder) = Embedder::from_config(embed_cfg) else {
            return Ok(None); // warned inside
        };
        let Some(chat) = tldr::ChatClient::from_config(&daily.llm) else {
            return Ok(None); // warned inside
        };
        let vectors = QdrantStore::new(
            &cfg.search.qdrant_url,
            &cfg.search.qdrant_collection,
            embed_cfg.dims,
        )?;
        Ok(Some(Arc::new(Self {
            cfg: daily.clone(),
            pool,
            http: HttpClient::new(reqwest::Client::new(), RetryPolicy::production()),
            plain_http: reqwest::Client::new(),
            embedder,
            vectors,
            chat,
            feed_base: ARXIV_FEED_BASE.to_string(),
            pdf_base: ARXIV_PDF_BASE.to_string(),
            running: AtomicBool::new(false),
        })))
    }

    /// DI constructor: every remote endpoint is overridable. Test support only.
    pub fn for_tests(
        cfg: DailyConfig,
        pool: SqlitePool,
        embedder: Embedder,
        vectors: QdrantStore,
        chat: tldr::ChatClient,
        feed_base: &str,
        pdf_base: &str,
    ) -> Arc<Self> {
        Arc::new(Self {
            cfg,
            pool,
            http: HttpClient::new(reqwest::Client::new(), RetryPolicy::fast_for_tests()),
            plain_http: reqwest::Client::new(),
            embedder,
            vectors,
            chat,
            feed_base: feed_base.trim_end_matches('/').to_string(),
            pdf_base: pdf_base.trim_end_matches('/').to_string(),
            running: AtomicBool::new(false),
        })
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn try_begin(&self) -> bool {
        self.running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Run for `batch_date` unless a run is already in flight (then `None`).
    pub async fn run_guarded(&self, batch_date: &str) -> Option<store::DailyRun> {
        if !self.try_begin() {
            return None;
        }
        let _guard = RunFlagGuard(&self.running);
        Some(job::run_once(self, batch_date).await)
    }

    /// Guarded run on a background task; `false` if one was in flight.
    /// The guard is taken synchronously, so a caller seeing `true` knows
    /// the very next `spawn_run`/`run_guarded` will refuse.
    pub fn spawn_run(self: &Arc<Self>, batch_date: String) -> bool {
        if !self.try_begin() {
            return false;
        }
        let svc = self.clone();
        tokio::spawn(async move {
            let _guard = RunFlagGuard(&svc.running);
            let run = job::run_once(&svc, &batch_date).await;
            tracing::info!(
                "daily run {}: {} ({} candidates)",
                run.batch_date,
                run.status,
                run.papers_found
            );
        });
        true
    }
}

/// Clears the running flag on scope exit — including panics and
/// cancellation — so a wedged run can never permanently refuse new runs.
struct RunFlagGuard<'a>(&'a AtomicBool);

impl Drop for RunFlagGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
