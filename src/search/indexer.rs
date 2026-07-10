use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use crate::search::{chunker, fts, planner, store, vector, SearchService};

#[derive(Debug, Default)]
pub struct SweepSummary {
    pub indexed: usize,
    pub deindexed: usize,
    pub failed: usize,
}

/// One full pass: remove tombstones, (re)index every stale paper.
/// Individual paper failures are recorded (with backoff) and never abort
/// the sweep.
pub async fn sweep(svc: &SearchService, library_root: &Path) -> Result<SweepSummary> {
    let papers = svc.paper_states().await?;
    let rows = store::all_index_rows(&svc.pool).await?;
    let plan = planner::plan(
        &papers,
        &rows,
        svc.embedder.as_ref().map(|e| e.model()),
        chrono::Utc::now(),
    );
    let mut summary = SweepSummary::default();

    for paper_id in &plan.deindex {
        match deindex_paper(svc, paper_id).await {
            Ok(()) => summary.deindexed += 1,
            Err(e) => {
                tracing::warn!("deindex {paper_id}: {e}");
                summary.failed += 1;
            }
        }
    }
    for work in &plan.index {
        match index_paper(svc, library_root, work).await {
            Ok(()) => summary.indexed += 1,
            Err(e) => {
                tracing::warn!("index {}: {e}", work.paper_id);
                store::record_error(&svc.pool, &work.paper_id, &e.to_string())
                    .await
                    .ok();
                summary.failed += 1;
            }
        }
    }
    Ok(summary)
}

async fn index_paper(svc: &SearchService, library_root: &Path, work: &planner::Work) -> Result<()> {
    let Some(paper) = crate::db::get_by_id(&svc.pool, &work.paper_id).await? else {
        return Ok(()); // purged since the plan was computed; tombstone next sweep
    };

    let chunks = if work.fts {
        // Full re-extract + re-chunk + Tantivy doc.
        let pdf_path = library_root.join(&paper.rel_path);
        let text = tokio::task::spawn_blocking(move || crate::pdf::extract_text_all(&pdf_path))
            .await
            .context("pdftotext task panicked")??;
        let chunks = chunker::chunk_paper(
            paper.meta.title.as_deref(),
            paper.meta.abstract_text.as_deref(),
            &text,
        );
        store::replace_chunks(
            &svc.pool,
            &paper.id,
            &chunks,
            &paper.content_hash,
            &store::meta_hash(&paper),
        )
        .await?;
        let body: String = chunks
            .iter()
            .filter(|c| c.seq >= 1)
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        svc.fts.upsert(&fts::PaperDoc {
            id: paper.id.clone(),
            title: paper.meta.title.clone().unwrap_or_default(),
            authors: paper.meta.authors.0.join(" ; "),
            venue: paper.meta.venue.clone().unwrap_or_default(),
            abstract_text: paper.meta.abstract_text.clone().unwrap_or_default(),
            body,
        })?;
        store::mark_fts_done(&svc.pool, &paper.id).await?;
        chunks
    } else {
        store::chunks_for_paper(&svc.pool, &paper.id).await?
    };

    if work.vectors {
        let Some(embedder) = &svc.embedder else {
            return Ok(()); // planner only schedules vectors when configured
        };
        if !chunks.is_empty() {
            let texts: Vec<String> = chunks.iter().map(|c| c.text.clone()).collect();
            let vectors = embedder.embed(&texts).await?;
            svc.vectors.ensure_collection().await?;
            let points: Vec<vector::ChunkPoint> = chunks
                .iter()
                .zip(vectors)
                .map(|(c, v)| vector::ChunkPoint {
                    paper_id: paper.id.clone(),
                    seq: c.seq,
                    page: c.page,
                    vector: v,
                })
                .collect();
            svc.vectors.upsert(&points).await?;
        }
        store::mark_vectors_done(&svc.pool, &paper.id, embedder_model(svc)).await?;
    }
    Ok(())
}

fn embedder_model(svc: &SearchService) -> &str {
    svc.embedder.as_ref().map(|e| e.model()).unwrap_or_default()
}

async fn deindex_paper(svc: &SearchService, paper_id: &str) -> Result<()> {
    svc.fts.delete(paper_id)?;
    if svc.embedder.is_some() {
        // Qdrant cleanup only matters when vectors were ever written; a dead
        // Qdrant here must not wedge the tombstone forever.
        if let Err(e) = svc.vectors.delete_paper(paper_id).await {
            tracing::warn!("qdrant delete {paper_id}: {e} (index row removed anyway; \
                            orphan points are overwritten if the paper returns)");
        }
    }
    store::remove_index_entry(&svc.pool, paper_id).await?;
    Ok(())
}

/// Indexer loop: sweep, then sleep until woken or the tick elapses.
pub async fn run(svc: Arc<SearchService>, library_root: PathBuf, tick: Duration) {
    loop {
        match sweep(&svc, &library_root).await {
            Ok(s) if s.indexed + s.deindexed + s.failed > 0 => {
                tracing::info!(
                    "search index sweep: {} indexed, {} removed, {} failed",
                    s.indexed,
                    s.deindexed,
                    s.failed
                );
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("search index sweep failed: {e}"),
        }
        svc.wait_work(tick).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Authors, Paper, PaperMeta, PaperStatus};
    use crate::search::{embedder, fts, store, vector, SearchService};
    use printpdf::{BuiltinFont, Mm, PdfDocument};
    use serde_json::json;
    use std::io::BufWriter;
    use std::path::Path;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn write_pdf(path: &Path, line: &str) {
        let (doc, page1, layer1) = PdfDocument::new("t", Mm(210.0), Mm(297.0), "L1");
        let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        doc.get_page(page1)
            .get_layer(layer1)
            .use_text(line, 12.0, Mm(15.0), Mm(280.0), &font);
        doc.save(&mut BufWriter::new(std::fs::File::create(path).unwrap())).unwrap();
    }

    struct Fixture {
        svc: std::sync::Arc<SearchService>,
        library_root: std::path::PathBuf,
        _dirs: Vec<tempfile::TempDir>,
    }

    /// Temp SQLite + temp Tantivy + wiremock Qdrant/embeddings (when given).
    async fn fixture(server: Option<&MockServer>) -> Fixture {
        let db_dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", db_dir.path().join("t.db").display());
        let pool = crate::db::connect(&url).await.unwrap();
        let idx_dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(idx_dir.path()).unwrap();
        let lib_dir = tempfile::tempdir().unwrap();
        let library_root = lib_dir.path().to_path_buf();
        let (vectors, embed) = match server {
            Some(s) => (
                vector::QdrantStore::new(&s.uri(), "xuewen", 4).unwrap(),
                Some(embedder::Embedder::for_tests(&format!("{}/v1", s.uri()), "m1", 4)),
            ),
            None => (vector::QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap(), None),
        };
        let svc = SearchService::open_with(pool, fts_idx, vectors, embed);
        Fixture { svc, library_root, _dirs: vec![db_dir, idx_dir, lib_dir] }
    }

    async fn insert_paper_with_pdf(f: &Fixture, id: &str, title: &str, body_line: &str) {
        let rel = format!("{id}.pdf");
        write_pdf(&f.library_root.join(&rel), body_line);
        let p = Paper {
            id: id.into(),
            content_hash: format!("hash-{id}"),
            rel_path: rel,
            cite_key: None,
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some(title.into()),
                abstract_text: Some("An abstract.".into()),
                authors: Authors(vec!["Ada Lovelace".into()]),
                venue: None,
                year: Some(2026),
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::Resolved,
            },
        };
        crate::db::insert_paper(&f.svc.pool, &p).await.unwrap();
    }

    #[tokio::test]
    async fn sweep_indexes_fts_even_without_embedder() {
        let f = fixture(None).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "the body mentions dictionaries").await;

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed, 1);

        let hits = f.svc.fts.search("dictionaries", &fts::FieldSel::all(), 10).unwrap();
        assert_eq!(hits.len(), 1, "body text searchable after sweep");
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert!(rows[0].fts_indexed_at.is_some());
        assert!(rows[0].vectors_indexed_at.is_none(), "no embedder -> no vector stamp");

        // Second sweep is a no-op.
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed + s.deindexed + s.failed, 0);
    }

    #[tokio::test]
    async fn sweep_embeds_and_upserts_vectors_when_configured() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/v1/embeddings"))
            .respond_with(|req: &wiremock::Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let n = body["input"].as_array().map(|a| a.len()).unwrap_or(1);
                let data: Vec<_> = (0..n)
                    .map(|i| json!({"index": i, "embedding": [0.1, 0.2, 0.3, 0.4]}))
                    .collect();
                ResponseTemplate::new(200).set_body_json(json!({"data": data}))
            })
            .mount(&server).await;
        Mock::given(method("GET")).and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 4, "distance": "Cosine"}}}}
            })))
            .mount(&server).await;
        Mock::given(method("PUT")).and(path("/collections/xuewen/points"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
            .expect(1..)
            .mount(&server).await;

        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed, 1);
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert!(rows[0].vectors_indexed_at.is_some());
        assert_eq!(rows[0].embed_model.as_deref(), Some("m1"));
    }

    #[tokio::test]
    async fn embedding_failure_keeps_fts_and_records_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server).await;

        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.failed, 1);
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert!(rows[0].fts_indexed_at.is_some(), "FTS tier survived");
        assert!(rows[0].vectors_indexed_at.is_none());
        assert!(rows[0].last_error.is_some());
        assert_eq!(rows[0].attempts, 1);
    }

    #[tokio::test]
    async fn trashed_paper_is_deindexed_everywhere() {
        let f = fixture(None).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        sweep_in(&f).await.unwrap();
        assert_eq!(f.svc.fts.search("fuzzing", &fts::FieldSel::all(), 10).unwrap().len(), 1);

        crate::db::soft_delete(&f.svc.pool, "p1").await.unwrap();
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.deindexed, 1);
        assert!(f.svc.fts.search("fuzzing", &fts::FieldSel::all(), 10).unwrap().is_empty());
        assert!(store::all_index_rows(&f.svc.pool).await.unwrap().is_empty());
        assert!(store::chunks_for_paper(&f.svc.pool, "p1").await.unwrap().is_empty());
        // Qdrant delete for a no-embedder service is skipped, not an error.
    }

    // Helper used by every test: sweep against the fixture's library root.
    async fn sweep_in(f: &Fixture) -> anyhow::Result<SweepSummary> {
        sweep(&f.svc, &f.library_root).await
    }
}
