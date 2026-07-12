//! Background generation of per-paper summaries. A periodic sweep fills the
//! `paper_summaries` table for library papers that lack one, when `[ai.summary]`
//! is configured. Sibling to the search indexer's sweep loop.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use sqlx::SqlitePool;

use crate::config::Config;
use crate::summary::{generate_summary, store, Summarizer};

/// How many papers one sweep pass summarizes before yielding.
const BATCH: i64 = 8;
/// Give up on a paper after this many failed generations.
const MAX_ATTEMPTS: i64 = 5;
/// Wait this long before retrying a failed paper.
const RETRY_BACKOFF_MINS: i64 = 30;

pub struct SummaryService {
    pool: SqlitePool,
    summarizer: Summarizer,
    library_root: PathBuf,
}

impl SummaryService {
    /// `None` when `[ai.summary]` is absent or no model/key resolves.
    pub fn from_config(pool: SqlitePool, cfg: &Config) -> Option<Arc<Self>> {
        let use_ = cfg.ai.summary.as_ref()?;
        let summarizer = Summarizer::from_resolved(&cfg.ai.resolve(use_))?; // warns handled by caller check
        Some(Arc::new(Self { pool, summarizer, library_root: cfg.library_root.clone() }))
    }

    /// DI constructor for tests.
    pub fn for_tests(pool: SqlitePool, summarizer: Summarizer, library_root: PathBuf) -> Arc<Self> {
        Arc::new(Self { pool, summarizer, library_root })
    }

    /// One pass: summarize up to `BATCH` papers that lack a summary. Best-effort
    /// per paper — one paper's failure is logged and skipped, never aborting the
    /// batch (mirrors the search indexer). Returns the number written.
    pub async fn sweep(&self) -> Result<usize> {
        let retry_before = (chrono::Utc::now() - chrono::Duration::minutes(RETRY_BACKOFF_MINS)).to_rfc3339();
        let ids = store::due_ids(&self.pool, BATCH, MAX_ATTEMPTS, &retry_before).await?;
        let mut written = 0;
        for id in ids {
            match self.summarize_one(&id).await {
                Ok(true) => written += 1,
                Ok(false) => {}
                Err(e) => tracing::warn!("summary generation for {id}: {e}"),
            }
        }
        Ok(written)
    }

    /// Summarize one paper. `Ok(true)` = stored; `Ok(false)` = skipped (purged)
    /// or the model produced nothing (a failure was recorded for backoff);
    /// `Err` = a hard DB failure for THIS paper (caller logs; NOT counted as a
    /// generation failure, so it isn't backed off).
    pub async fn summarize_one(&self, id: &str) -> Result<bool> {
        let Some(paper) = crate::db::get_by_id(&self.pool, id).await? else {
            return Ok(false); // purged since selection ran
        };
        let pdf_path = self.library_root.join(&paper.rel_path);
        let full_text = match tokio::task::spawn_blocking(move || crate::pdf::extract_text_all(&pdf_path)).await {
            Ok(Ok(t)) => Some(t),
            Ok(Err(e)) => {
                tracing::warn!("pdf extraction failed for {id}: {e}; summarizing from abstract only");
                None
            }
            Err(e) => {
                tracing::warn!("pdf extraction task panicked for {id}: {e}; summarizing from abstract only");
                None
            }
        };
        let title = paper.meta.title.as_deref().unwrap_or_default();
        let abstract_text = paper.meta.abstract_text.as_deref().unwrap_or_default();
        match generate_summary(&self.summarizer, title, abstract_text, full_text.as_deref()).await {
            Some(summary) => {
                store::upsert(&self.pool, &paper.id, &summary, self.model()).await?;
                store::clear_failure(&self.pool, Some(&paper.id)).await?;
                Ok(true)
            }
            None => {
                store::record_failure(&self.pool, &paper.id).await?;
                Ok(false)
            }
        }
    }

    fn model(&self) -> &str {
        self.summarizer.model()
    }
}

/// Summary loop: sweep a batch, then sleep for `tick`. New papers are picked up
/// on the next tick (no wake channel — the delay is bounded and cheap).
pub async fn run(svc: Arc<SummaryService>, tick: Duration) {
    loop {
        match svc.sweep().await {
            Ok(n) if n > 0 => tracing::info!("summary sweep: {n} generated"),
            Ok(_) => {}
            Err(e) => tracing::warn!("summary sweep failed: {e}"),
        }
        tokio::time::sleep(tick).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Authors, Paper, PaperMeta, PaperStatus};
    use printpdf::{BuiltinFont, Mm, PdfDocument};
    use serde_json::json;
    use std::io::BufWriter;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn write_pdf(path: &std::path::Path, line: &str) {
        let (doc, p, l) = PdfDocument::new("t", Mm(210.0), Mm(297.0), "L1");
        let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        doc.get_page(p).get_layer(l).use_text(line, 12.0, Mm(15.0), Mm(280.0), &font);
        doc.save(&mut BufWriter::new(std::fs::File::create(path).unwrap())).unwrap();
    }

    fn chat_reply(text: &str) -> serde_json::Value {
        json!({"choices": [{"message": {"role": "assistant", "content": text}}]})
    }

    #[tokio::test]
    async fn sweep_generates_and_stores_then_is_idempotent() {
        let server = MockServer::start().await;
        let summary_json = json!({
            "tldr": "TL;DR.", "problem": "P.", "approach": "A.",
            "results": "R.", "limitations": "L."
        })
        .to_string();
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(&summary_json)))
            .expect(1) // exactly one paper summarized, then no more calls
            .mount(&server)
            .await;

        let db_dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", db_dir.path().join("t.db").display());
        let pool = crate::db::connect(&url).await.unwrap();
        let lib = tempfile::tempdir().unwrap();

        let paper = Paper {
            id: "p1".into(),
            content_hash: "h".into(),
            rel_path: "p1.pdf".into(),
            cite_key: None,
            added_at: "2026-07-12T00:00:00Z".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some("Title".into()),
                abstract_text: Some("Abstract.".into()),
                authors: Authors(vec!["Ada".into()]),
                venue: None, year: Some(2026), doi: None, arxiv_id: None,
                dblp_key: None, url: None, source: None,
                status: PaperStatus::Resolved,
            },
        };
        write_pdf(&lib.path().join("p1.pdf"), "the body");
        crate::db::insert_paper(&pool, &paper).await.unwrap();

        let svc = SummaryService::for_tests(
            pool.clone(),
            Summarizer::for_tests(&format!("{}/v1", server.uri()), "m"),
            lib.path().to_path_buf(),
        );

        assert_eq!(svc.sweep().await.unwrap(), 1);
        assert_eq!(
            crate::summary::store::get(&pool, "p1").await.unwrap().unwrap().tldr,
            "TL;DR."
        );
        // Nothing left to do -> no more LLM calls (the .expect(1) mock enforces this).
        assert_eq!(svc.sweep().await.unwrap(), 0);
    }

    #[tokio::test]
    async fn sweep_falls_back_to_abstract_when_pdf_missing() {
        let server = MockServer::start().await;
        let summary_json = json!({
            "tldr": "TL;DR.", "problem": "P.", "approach": "A.",
            "results": "R.", "limitations": "L."
        })
        .to_string();
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(&summary_json)))
            .expect(1)
            .mount(&server)
            .await;

        let db_dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", db_dir.path().join("t.db").display());
        let pool = crate::db::connect(&url).await.unwrap();
        let lib = tempfile::tempdir().unwrap();

        let paper = Paper {
            id: "p1".into(),
            content_hash: "h".into(),
            rel_path: "missing.pdf".into(),
            cite_key: None,
            added_at: "2026-07-12T00:00:00Z".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some("Title".into()),
                abstract_text: Some("Abstract.".into()),
                authors: Authors(vec!["Ada".into()]),
                venue: None, year: Some(2026), doi: None, arxiv_id: None,
                dblp_key: None, url: None, source: None,
                status: PaperStatus::Resolved,
            },
        };
        // Deliberately do NOT write a PDF at lib/missing.pdf.
        crate::db::insert_paper(&pool, &paper).await.unwrap();

        let svc = SummaryService::for_tests(
            pool.clone(),
            Summarizer::for_tests(&format!("{}/v1", server.uri()), "m"),
            lib.path().to_path_buf(),
        );

        assert_eq!(svc.sweep().await.unwrap(), 1);
        assert!(crate::summary::store::get(&pool, "p1").await.unwrap().is_some());
    }

    #[tokio::test]
    async fn sweep_backs_off_a_failing_paper() {
        let server = MockServer::start().await;
        // Non-JSON 200 body -> parse_summary fails on every attempt, so
        // generate_summary returns None after the full-text + abstract attempts.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply("not json")))
            .expect(2) // full-text + abstract attempts, for ONE paper, ONCE
            .mount(&server)
            .await;

        let db_dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", db_dir.path().join("t.db").display());
        let pool = crate::db::connect(&url).await.unwrap();
        let lib = tempfile::tempdir().unwrap();

        let paper = Paper {
            id: "p1".into(),
            content_hash: "h".into(),
            rel_path: "p1.pdf".into(),
            cite_key: None,
            added_at: "2026-07-12T00:00:00Z".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some("Title".into()),
                abstract_text: Some("Abstract.".into()),
                authors: Authors(vec!["Ada".into()]),
                venue: None, year: Some(2026), doi: None, arxiv_id: None,
                dblp_key: None, url: None, source: None,
                status: PaperStatus::Resolved,
            },
        };
        write_pdf(&lib.path().join("p1.pdf"), "the body");
        crate::db::insert_paper(&pool, &paper).await.unwrap();

        let svc = SummaryService::for_tests(
            pool.clone(),
            Summarizer::for_tests(&format!("{}/v1", server.uri()), "m"),
            lib.path().to_path_buf(),
        );

        // First sweep: fails, records the failure.
        assert_eq!(svc.sweep().await.unwrap(), 0);
        // Second, immediate sweep: no summary generated, and (enforced by the
        // .expect(2) mock above) no further chat calls -- the paper was NOT
        // retried because it's within the 30-minute backoff window.
        assert_eq!(svc.sweep().await.unwrap(), 0);

        let recent_cutoff =
            (chrono::Utc::now() - chrono::Duration::minutes(30)).to_rfc3339();
        assert_eq!(
            crate::summary::store::due_ids(&pool, 10, MAX_ATTEMPTS, &recent_cutoff)
                .await
                .unwrap(),
            Vec::<String>::new()
        );
    }

    #[tokio::test]
    async fn summarize_one_clears_failure_row_on_success() {
        let db_dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", db_dir.path().join("t.db").display());
        let pool = crate::db::connect(&url).await.unwrap();
        let lib = tempfile::tempdir().unwrap();

        let paper = Paper {
            id: "p1".into(),
            content_hash: "h".into(),
            rel_path: "p1.pdf".into(),
            cite_key: None,
            added_at: "2026-07-12T00:00:00Z".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some("Title".into()),
                abstract_text: Some("Abstract.".into()),
                authors: Authors(vec!["Ada".into()]),
                venue: None, year: Some(2026), doi: None, arxiv_id: None,
                dblp_key: None, url: None, source: None,
                status: PaperStatus::Resolved,
            },
        };
        write_pdf(&lib.path().join("p1.pdf"), "the body");
        crate::db::insert_paper(&pool, &paper).await.unwrap();

        // Simulate a prior failed attempt, without calling the model.
        crate::summary::store::record_failure(&pool, "p1").await.unwrap();
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM summary_failures WHERE paper_id = ?")
            .bind("p1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);

        let server = MockServer::start().await;
        let summary_json = json!({
            "tldr": "TL;DR.", "problem": "P.", "approach": "A.",
            "results": "R.", "limitations": "L."
        })
        .to_string();
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(&summary_json)))
            .expect(1)
            .mount(&server)
            .await;

        let svc = SummaryService::for_tests(
            pool.clone(),
            Summarizer::for_tests(&format!("{}/v1", server.uri()), "m"),
            lib.path().to_path_buf(),
        );

        assert!(svc.summarize_one("p1").await.unwrap());
        assert!(crate::summary::store::get(&pool, "p1").await.unwrap().is_some());

        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM summary_failures WHERE paper_id = ?")
            .bind("p1")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
