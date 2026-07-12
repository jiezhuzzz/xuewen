//! Background generation of per-paper summaries. A periodic sweep fills the
//! `paper_summaries` table for library papers that lack one, when `[summary]`
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

pub struct SummaryService {
    pool: SqlitePool,
    summarizer: Summarizer,
    language: String,
    library_root: PathBuf,
}

impl SummaryService {
    /// `None` when `[summary]` is absent or no API key resolves.
    pub fn from_config(pool: SqlitePool, cfg: &Config) -> Option<Arc<Self>> {
        let sc = cfg.summary.as_ref()?;
        let summarizer = Summarizer::from_summary(sc)?; // warns on missing key
        Some(Arc::new(Self {
            pool,
            summarizer,
            language: sc.language.clone(),
            library_root: cfg.library_root.clone(),
        }))
    }

    /// DI constructor for tests.
    pub fn for_tests(
        pool: SqlitePool,
        summarizer: Summarizer,
        language: String,
        library_root: PathBuf,
    ) -> Arc<Self> {
        Arc::new(Self { pool, summarizer, language, library_root })
    }

    /// One pass: summarize up to `BATCH` papers that lack a summary. Best-effort
    /// per paper (failures are logged and simply retried next sweep). Returns
    /// the number of summaries written.
    pub async fn sweep(&self) -> Result<usize> {
        let ids = store::missing_ids(&self.pool, BATCH).await?;
        let mut written = 0;
        for id in ids {
            let Some(paper) = crate::db::get_by_id(&self.pool, &id).await? else {
                continue; // purged since missing_ids ran
            };
            let pdf_path = self.library_root.join(&paper.rel_path);
            let full_text = tokio::task::spawn_blocking(move || crate::pdf::extract_text_all(&pdf_path))
                .await
                .ok()
                .and_then(|r| r.ok());
            let title = paper.meta.title.clone().unwrap_or_default();
            let abstract_text = paper.meta.abstract_text.clone().unwrap_or_default();
            if let Some(summary) = generate_summary(
                &self.summarizer,
                &self.language,
                &title,
                &abstract_text,
                full_text.as_deref(),
            )
            .await
            {
                store::upsert(&self.pool, &paper.id, &summary, self.model()).await?;
                written += 1;
            }
        }
        Ok(written)
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
            "English".into(),
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
}
