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
            Ok(true) => summary.indexed += 1,
            Ok(false) => {}
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

async fn index_paper(
    svc: &SearchService,
    library_root: &Path,
    work: &planner::Work,
) -> Result<bool> {
    let Some(paper) = crate::db::get_by_id(&svc.pool, &work.paper_id).await? else {
        return Ok(false); // purged since the plan was computed; tombstone next sweep
    };

    // Deriving the text and writing the Tantivy doc are separate steps: a note
    // edit needs the second without the first, and `pdftotext` is by far the
    // most expensive thing in this function.
    let chunks = if work.reextract {
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
        chunks
    } else {
        store::chunks_for_paper(&svc.pool, &paper.id).await?
    };

    if work.fts {
        let body: String = chunks
            .iter()
            .filter(|c| c.seq >= 1)
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join("\n\n");
        let notes = crate::annotations::store::notes_blob(&svc.pool, &paper.id).await?;
        let notes_hash = store::notes_hash(&notes);
        svc.fts.upsert(&fts::PaperDoc {
            id: paper.id.clone(),
            title: paper.meta.title.clone().unwrap_or_default(),
            authors: paper.meta.authors.0.join(" ; "),
            venue: paper.meta.venue.clone().unwrap_or_default(),
            abstract_text: paper.meta.abstract_text.clone().unwrap_or_default(),
            body,
            notes,
        })?;
        store::mark_fts_done(&svc.pool, &paper.id, &notes_hash).await?;
    }

    if work.vectors {
        let Some(embedder) = &svc.embedder else {
            return Ok(true); // planner only schedules vectors when configured
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
    Ok(true)
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
            tracing::warn!(
                "qdrant delete {paper_id}: {e} (index row removed anyway; \
                            orphan points are overwritten if the paper returns)"
            );
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
    use printpdf::*;
    use serde_json::json;
    use std::path::Path;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn write_pdf(path: &Path, line: &str) {
        let mut doc = PdfDocument::new("t");
        let ops = vec![
            Op::StartTextSection,
            Op::SetFont {
                font: PdfFontHandle::Builtin(BuiltinFont::Helvetica),
                size: Pt(12.0),
            },
            Op::SetTextCursor {
                pos: Point::new(Mm(15.0), Mm(280.0)),
            },
            Op::ShowText {
                items: vec![TextItem::Text(line.to_string())],
            },
            Op::EndTextSection,
        ];
        let page = PdfPage::new(Mm(210.0), Mm(297.0), ops);
        let bytes = doc
            .with_pages(vec![page])
            .save(&PdfSaveOptions::default(), &mut Vec::new());
        std::fs::write(path, bytes).unwrap();
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
                Some(embedder::Embedder::for_tests(
                    &format!("{}/v1", s.uri()),
                    "m1",
                    4,
                )),
            ),
            None => (
                vector::QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap(),
                None,
            ),
        };
        let svc = SearchService::open_with(pool, fts_idx, vectors, embed);
        Fixture {
            svc,
            library_root,
            _dirs: vec![db_dir, idx_dir, lib_dir],
        }
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
            starred: false,
            name: None,
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
        insert_paper_with_pdf(
            &f,
            "p1",
            "Fuzzing Firmware",
            "the body mentions dictionaries",
        )
        .await;

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed, 1);

        let hits = f
            .svc
            .fts
            .search("dictionaries", &fts::FieldSel::all(), 10)
            .unwrap();
        assert_eq!(hits.len(), 1, "body text searchable after sweep");
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert!(rows[0].fts_indexed_at.is_some());
        assert!(
            rows[0].vectors_indexed_at.is_none(),
            "no embedder -> no vector stamp"
        );

        // Second sweep is a no-op.
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed + s.deindexed + s.failed, 0);
    }

    /// Attach a note to a paper, the way the annotations API does.
    async fn add_note(f: &Fixture, paper_id: &str, id: &str, note: &str) {
        crate::annotations::store::upsert(
            &f.svc.pool,
            paper_id,
            id,
            &crate::annotations::NewAnnotation {
                page_index: 0,
                kind: crate::annotations::AnnotationKind::Highlight,
                color: crate::annotations::AnnotationColor::Amber,
                quoted_text: Some("some quoted sentence".into()),
                note: Some(note.into()),
                payload: json!({ "annotation": { "type": 9 } }),
            },
            "2026-08-14T00:00:00Z",
        )
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn a_new_note_reindexes_fts_without_touching_the_pdf() {
        let f = fixture(None).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "the body words").await;
        assert_eq!(sweep_in(&f).await.unwrap().indexed, 1);
        assert!(f
            .svc
            .fts
            .search("bisimulation", &fts::FieldSel::all(), 10)
            .unwrap()
            .is_empty());

        // Delete the PDF: a re-extract would now fail loudly, so a green
        // sweep proves the notes path reused the chunks already in SQLite.
        std::fs::remove_file(f.library_root.join("p1.pdf")).unwrap();
        add_note(&f, "p1", "a1", "this is really a bisimulation argument").await;

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed, 1);
        assert_eq!(s.failed, 0, "the PDF must not be re-read for a note edit");

        let mut sel = fts::FieldSel::none();
        sel.notes = true;
        let hits = f.svc.fts.search("bisimulation", &sel, 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].paper_id, "p1");
        // The body survived the rewrite — it came from the stored chunks.
        assert_eq!(
            f.svc
                .fts
                .search("body", &fts::FieldSel::all(), 10)
                .unwrap()
                .len(),
            1
        );

        // Third sweep is a no-op: the notes hash is stamped.
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.indexed + s.deindexed + s.failed, 0);
        assert_eq!(f.svc.status().await.unwrap().fts.pending, 0);
    }

    #[tokio::test]
    async fn an_unstamped_note_shows_up_as_pending() {
        let f = fixture(None).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "the body words").await;
        sweep_in(&f).await.unwrap();
        add_note(&f, "p1", "a1", "worth revisiting").await;
        assert_eq!(
            f.svc.status().await.unwrap().fts.pending,
            1,
            "a note nobody has indexed yet is pending FTS work"
        );
    }

    #[tokio::test]
    async fn sweep_embeds_and_upserts_vectors_when_configured() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(|req: &wiremock::Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let n = body["input"].as_array().map(|a| a.len()).unwrap_or(1);
                let data: Vec<_> = (0..n)
                    .map(|i| json!({"index": i, "embedding": [0.1, 0.2, 0.3, 0.4]}))
                    .collect();
                ResponseTemplate::new(200).set_body_json(json!({"data": data}))
            })
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 4, "distance": "Cosine"}}}}
            })))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen/points"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
            .expect(1..)
            .mount(&server)
            .await;

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
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(400))
            .mount(&server)
            .await;

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
    async fn extraction_failure_on_new_paper_records_error_and_backs_off() {
        let f = fixture(None).await;
        // A paper row whose PDF was never written (missing/corrupt file):
        // extraction fails before replace_chunks ever creates a search_index
        // row, so record_error must upsert one rather than no-op.
        let p = Paper {
            id: "p1".into(),
            content_hash: "hash-p1".into(),
            rel_path: "does-not-exist.pdf".into(),
            cite_key: None,
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: PaperMeta {
                title: Some("Fuzzing Firmware".into()),
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

        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.failed, 1);
        let rows = store::all_index_rows(&f.svc.pool).await.unwrap();
        assert_eq!(rows.len(), 1, "record_error must create the missing row");
        assert_eq!(rows[0].attempts, 1);
        assert!(rows[0].last_error.is_some());
        assert_eq!(f.svc.status().await.unwrap().fts.failed, 1);

        // Second sweep right away: backoff suppresses the retry.
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(
            s.indexed + s.deindexed + s.failed,
            0,
            "backoff should suppress an immediate retry"
        );
    }

    #[tokio::test]
    async fn trashed_paper_is_deindexed_everywhere() {
        let f = fixture(None).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        sweep_in(&f).await.unwrap();
        assert_eq!(
            f.svc
                .fts
                .search("fuzzing", &fts::FieldSel::all(), 10)
                .unwrap()
                .len(),
            1
        );

        crate::db::soft_delete(&f.svc.pool, "p1").await.unwrap();
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.deindexed, 1);
        assert!(f
            .svc
            .fts
            .search("fuzzing", &fts::FieldSel::all(), 10)
            .unwrap()
            .is_empty());
        assert!(store::all_index_rows(&f.svc.pool).await.unwrap().is_empty());
        assert!(store::chunks_for_paper(&f.svc.pool, "p1")
            .await
            .unwrap()
            .is_empty());
        // Qdrant delete for a no-embedder service is skipped, not an error.
    }

    #[tokio::test]
    async fn qdrant_delete_failure_does_not_wedge_tombstone() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(|req: &wiremock::Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let n = body["input"].as_array().map(|a| a.len()).unwrap_or(1);
                let data: Vec<_> = (0..n)
                    .map(|i| json!({"index": i, "embedding": [0.1, 0.2, 0.3, 0.4]}))
                    .collect();
                ResponseTemplate::new(200).set_body_json(json!({"data": data}))
            })
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/collections/xuewen"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"config": {"params": {"vectors": {"size": 4, "distance": "Cosine"}}}}
            })))
            .mount(&server)
            .await;
        Mock::given(method("PUT"))
            .and(path("/collections/xuewen/points"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"result": {}})))
            .mount(&server)
            .await;
        // The delete endpoint is down: the tombstone must still clear.
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/delete"))
            .respond_with(ResponseTemplate::new(500))
            .expect(1)
            .mount(&server)
            .await;

        let f = fixture(Some(&server)).await;
        insert_paper_with_pdf(&f, "p1", "Fuzzing Firmware", "body words").await;
        sweep_in(&f).await.unwrap();

        crate::db::soft_delete(&f.svc.pool, "p1").await.unwrap();
        let s = sweep_in(&f).await.unwrap();
        assert_eq!(s.deindexed, 1, "tombstone cleared despite qdrant failure");
        assert!(store::all_index_rows(&f.svc.pool).await.unwrap().is_empty());
        assert!(f
            .svc
            .fts
            .search("fuzzing", &fts::FieldSel::all(), 10)
            .unwrap()
            .is_empty());
    }

    // Helper used by every test: sweep against the fixture's library root.
    async fn sweep_in(f: &Fixture) -> anyhow::Result<SweepSummary> {
        sweep(&f.svc, &f.library_root).await
    }
}
