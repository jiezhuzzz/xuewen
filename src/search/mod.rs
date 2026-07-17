pub mod chunker;
pub mod embedder;
pub mod fts;
pub mod fusion;
pub mod indexer;
pub mod planner;
pub mod store;
pub mod vector;

use anyhow::Result;
use sqlx::SqlitePool;
use std::sync::Arc;
use std::time::Duration;

use crate::config::SearchConfig;
use crate::models::Paper;

const KEYWORD_LIMIT: usize = 100;
const SEMANTIC_LIMIT: usize = 50;
const RRF_K: f32 = 60.0;
const SEMANTIC_SNIPPET_CHARS: usize = 200;

pub struct SearchRequest {
    pub q: String,
    pub fields: fts::FieldSel,
    pub keyword: bool,
    pub semantic: bool,
    pub status: Option<String>,
    pub project: Option<String>,
    pub tag: Option<String>,
    pub starred: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct SemanticState {
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MatchInfo {
    /// "keyword" | "semantic" | "both"
    pub engine: String,
    pub field: String,
    /// HTML-safe (escaped text, <mark> highlights only).
    pub snippet: String,
    pub page: Option<i64>,
}

pub struct SearchOutcome {
    pub semantic: SemanticState,
    pub results: Vec<(Paper, MatchInfo)>,
}

#[derive(Debug, Clone, Copy)]
pub struct TierCounts {
    pub indexed: i64,
    pub pending: i64,
    pub failed: i64,
}

#[derive(Debug, Clone)]
pub struct IndexStatus {
    pub fts: TierCounts,
    pub vectors: TierCounts,
    pub semantic_available: bool,
    pub reason: Option<String>,
}

/// Owns the three search backends. SQLite remains the source of truth;
/// Tantivy and Qdrant are derived and rebuildable.
pub struct SearchService {
    pub pool: SqlitePool,
    pub fts: fts::FtsIndex,
    pub vectors: vector::QdrantStore,
    pub embedder: Option<embedder::Embedder>,
    notify: tokio::sync::Notify,
}

impl SearchService {
    pub async fn open(
        pool: SqlitePool,
        cfg: &SearchConfig,
        ai: &crate::config::AiConfig,
    ) -> Result<Arc<Self>> {
        let (fts_idx, created) = fts::FtsIndex::open(&cfg.index_dir)?;
        if created {
            store::clear_stamps(&pool, true, false).await?;
        }
        let (embedder, dims) = match &ai.embedding {
            Some(e) => {
                let r = ai.resolve(&e.endpoint);
                let model = e.model();
                (
                    embedder::Embedder::from_resolved(&r, &model, e.dims),
                    e.dims,
                )
            }
            None => (None, 1536),
        };
        let vectors = vector::QdrantStore::new(&cfg.qdrant_url, &cfg.qdrant_collection, dims)?;
        Ok(Arc::new(Self {
            pool,
            fts: fts_idx,
            vectors,
            embedder,
            notify: tokio::sync::Notify::new(),
        }))
    }

    /// Dependency-injection constructor for tests.
    pub fn open_with(
        pool: SqlitePool,
        fts: fts::FtsIndex,
        vectors: vector::QdrantStore,
        embedder: Option<embedder::Embedder>,
    ) -> Arc<Self> {
        Arc::new(Self {
            pool,
            fts,
            vectors,
            embedder,
            notify: tokio::sync::Notify::new(),
        })
    }

    /// Nudge the indexer to sweep now (harmless if nothing is stale).
    pub fn wake(&self) {
        self.notify.notify_one();
    }

    /// Wait for a wake() or the periodic tick, whichever comes first.
    pub async fn wait_work(&self, tick: Duration) {
        tokio::select! {
            _ = self.notify.notified() => {}
            _ = tokio::time::sleep(tick) => {}
        }
    }

    fn semantic_config_state(&self) -> SemanticState {
        match &self.embedder {
            Some(_) => SemanticState {
                available: true,
                reason: None,
            },
            None => SemanticState {
                available: false,
                reason: Some(
                    "embedding API not configured (set [ai.embedding] and an API key)".into(),
                ),
            },
        }
    }

    pub async fn search(&self, req: &SearchRequest) -> Result<SearchOutcome> {
        let q = req.q.trim();
        let mut semantic = self.semantic_config_state();
        if req.fields.authors_only() && semantic.available {
            semantic = SemanticState {
                available: false,
                reason: Some("semantic search does not apply to an authors-only query".into()),
            };
        }

        let keyword_hits = if req.keyword {
            self.fts.search(q, &req.fields, KEYWORD_LIMIT)?
        } else {
            Vec::new()
        };

        // Best chunk per paper, in Qdrant score order.
        let mut semantic_best: Vec<vector::VecHit> = Vec::new();
        if req.semantic && semantic.available && !q.is_empty() {
            match self.semantic_search(q, &req.fields).await {
                Ok(hits) => {
                    let mut seen = std::collections::HashSet::new();
                    for h in hits {
                        if seen.insert(h.paper_id.clone()) {
                            semantic_best.push(h);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("semantic search failed: {e}");
                    semantic = SemanticState {
                        available: false,
                        reason: Some(e.to_string()),
                    };
                }
            }
        }

        let keyword_ids: Vec<String> = keyword_hits.iter().map(|h| h.paper_id.clone()).collect();
        let semantic_ids: Vec<String> = semantic_best.iter().map(|h| h.paper_id.clone()).collect();
        let fused: Vec<String> = match (keyword_ids.is_empty(), semantic_ids.is_empty()) {
            (false, true) => keyword_ids.clone(),
            (true, false) => semantic_ids.clone(),
            _ => fusion::rrf(&[keyword_ids.clone(), semantic_ids.clone()], RRF_K)
                .into_iter()
                .map(|(id, _)| id)
                .collect(),
        };

        let papers = store::papers_by_ids_ordered(
            &self.pool,
            &fused,
            req.status.as_deref(),
            req.project.as_deref(),
            req.tag.as_deref(),
            req.starred,
        )
        .await?;

        let kw_by_id: std::collections::HashMap<&str, &fts::FtsHit> = keyword_hits
            .iter()
            .map(|h| (h.paper_id.as_str(), h))
            .collect();
        let sem_by_id: std::collections::HashMap<&str, &vector::VecHit> = semantic_best
            .iter()
            .map(|h| (h.paper_id.as_str(), h))
            .collect();

        let mut results = Vec::with_capacity(papers.len());
        for p in papers {
            let kw = kw_by_id.get(p.id.as_str());
            let sem = sem_by_id.get(p.id.as_str());
            let info = match (kw, sem) {
                (Some(k), Some(_)) => MatchInfo {
                    engine: "both".into(),
                    field: k.field.clone(),
                    snippet: k.snippet_html.clone(),
                    page: None,
                },
                (Some(k), None) => MatchInfo {
                    engine: "keyword".into(),
                    field: k.field.clone(),
                    snippet: k.snippet_html.clone(),
                    page: None,
                },
                (None, Some(s)) => self.semantic_match_info(s).await,
                (None, None) => continue, // cannot happen: fused ⊆ union
            };
            results.push((p, info));
        }
        Ok(SearchOutcome { semantic, results })
    }

    async fn semantic_search(&self, q: &str, sel: &fts::FieldSel) -> Result<Vec<vector::VecHit>> {
        let embedder = self.embedder.as_ref().expect("caller checked availability");
        let vecs = embedder.embed(&[q.to_string()]).await?;
        let filter = match (sel.title || sel.abstract_text, sel.body) {
            (true, true) => vector::SeqFilter::All,
            (false, true) => vector::SeqFilter::OnlyBody,
            (true, false) => vector::SeqFilter::OnlySummary,
            (false, false) => vector::SeqFilter::All, // authors-only never reaches here
        };
        self.vectors.search(&vecs[0], SEMANTIC_LIMIT, filter).await
    }

    /// Snippet for a semantic-only hit: the matching chunk's text (escaped, trimmed).
    async fn semantic_match_info(&self, hit: &vector::VecHit) -> MatchInfo {
        let (field, page) = if hit.seq == 0 {
            ("abstract", None)
        } else {
            ("body", hit.page)
        };
        let text = store::chunk_text(&self.pool, &hit.paper_id, hit.seq)
            .await
            .ok()
            .flatten()
            .map(|c| c.text)
            .unwrap_or_default();
        let trimmed: String = text.chars().take(SEMANTIC_SNIPPET_CHARS).collect();
        let ellipsis = if text.chars().count() > SEMANTIC_SNIPPET_CHARS {
            "…"
        } else {
            ""
        };
        MatchInfo {
            engine: "semantic".into(),
            field: field.into(),
            snippet: format!("{}{}", fts::html_escape(&trimmed), ellipsis),
            page,
        }
    }

    /// Live papers as planner input (meta hashes computed here).
    pub async fn paper_states(&self) -> Result<Vec<planner::PaperState>> {
        let papers = sqlx::query_as::<_, Paper>("SELECT * FROM papers")
            .fetch_all(&self.pool)
            .await?;
        Ok(papers
            .iter()
            .map(|p| planner::PaperState {
                id: p.id.clone(),
                content_hash: p.content_hash.clone(),
                meta_hash: store::meta_hash(p),
                trashed: p.deleted_at.is_some(),
            })
            .collect())
    }

    pub async fn status(&self) -> Result<IndexStatus> {
        let papers = self.paper_states().await?;
        let rows = store::all_index_rows(&self.pool).await?;
        let by_id: std::collections::HashMap<&str, &store::IndexRow> =
            rows.iter().map(|r| (r.paper_id.as_str(), r)).collect();
        let model = self.embedder.as_ref().map(|e| e.model());

        // A tier is "indexed" only when its stamp is set AND the stored
        // hashes still match the paper (a stale stamp is pending work).
        let (mut fts_indexed, mut vec_indexed, mut live_n) = (0i64, 0i64, 0i64);
        for p in papers.iter().filter(|p| !p.trashed) {
            live_n += 1;
            if let Some(r) = by_id.get(p.id.as_str()) {
                let content_ok = r.content_hash == p.content_hash && r.meta_hash == p.meta_hash;
                if content_ok && r.fts_indexed_at.is_some() {
                    fts_indexed += 1;
                }
                if content_ok && r.vectors_indexed_at.is_some() && r.embed_model.as_deref() == model
                {
                    vec_indexed += 1;
                }
            }
        }
        let failed = rows.iter().filter(|r| r.last_error.is_some()).count() as i64;
        let sem = self.semantic_config_state();
        // Without an embedder the vectors tier is idle, not "all indexed".
        let vectors = if self.embedder.is_some() {
            TierCounts {
                indexed: vec_indexed,
                pending: live_n - vec_indexed,
                failed,
            }
        } else {
            TierCounts {
                indexed: 0,
                pending: 0,
                failed,
            }
        };
        Ok(IndexStatus {
            fts: TierCounts {
                indexed: fts_indexed,
                pending: live_n - fts_indexed,
                failed,
            },
            vectors,
            semantic_available: sem.available,
            reason: sem.reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Authors, Paper, PaperMeta, PaperStatus};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    async fn pool() -> sqlx::SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let p = crate::db::connect(&url).await.unwrap();
        std::mem::forget(dir);
        p
    }

    fn paper(id: &str, title: &str) -> Paper {
        Paper {
            id: id.into(),
            content_hash: format!("hash-{id}"),
            rel_path: format!("{id}.pdf"),
            cite_key: None,
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            meta: PaperMeta {
                title: Some(title.into()),
                abstract_text: None,
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
        }
    }

    /// Service with keyword tier real (temp Tantivy), semantic unavailable.
    async fn keyword_only_service(pool: sqlx::SqlitePool) -> std::sync::Arc<SearchService> {
        let dir = tempfile::tempdir().unwrap();
        let (fts, _) = fts::FtsIndex::open(dir.path()).unwrap();
        std::mem::forget(dir);
        let vectors = vector::QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap();
        SearchService::open_with(pool, fts, vectors, None)
    }

    #[tokio::test]
    async fn keyword_search_returns_papers_with_snippets_in_rank_order() {
        let pool = pool().await;
        for (id, title) in [("a", "Fuzzing Firmware"), ("b", "Sorting Networks")] {
            crate::db::insert_paper(&pool, &paper(id, title))
                .await
                .unwrap();
        }
        let svc = keyword_only_service(pool).await;
        svc.fts
            .upsert(&fts::PaperDoc {
                id: "a".into(),
                title: "Fuzzing Firmware".into(),
                authors: "Ada Lovelace".into(),
                venue: String::new(),
                abstract_text: String::new(),
                body: "we fuzz routers".into(),
            })
            .unwrap();
        svc.fts
            .upsert(&fts::PaperDoc {
                id: "b".into(),
                title: "Sorting Networks".into(),
                authors: "Ada Lovelace".into(),
                venue: String::new(),
                abstract_text: String::new(),
                body: "batcher merge".into(),
            })
            .unwrap();

        let out = svc
            .search(&SearchRequest {
                q: "fuzzing".into(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();

        assert!(!out.semantic.available); // no embedder configured
        assert!(out.semantic.reason.is_some());
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].0.id, "a");
        assert_eq!(out.results[0].1.engine, "keyword");
        assert!(out.results[0].1.snippet.contains("<mark>"));
    }

    #[tokio::test]
    async fn trashed_papers_are_filtered_at_hydration() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "Fuzzing Firmware"))
            .await
            .unwrap();
        crate::db::soft_delete(&pool, "a").await.unwrap();
        let svc = keyword_only_service(pool).await;
        svc.fts
            .upsert(&fts::PaperDoc {
                id: "a".into(),
                title: "Fuzzing Firmware".into(),
                authors: String::new(),
                venue: String::new(),
                abstract_text: String::new(),
                body: String::new(),
            })
            .unwrap();
        let out = svc
            .search(&SearchRequest {
                q: "fuzzing".into(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: false,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();
        assert!(
            out.results.is_empty(),
            "trashed paper leaked through hydration"
        );
    }

    #[tokio::test]
    async fn hybrid_search_fuses_and_marks_both() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "Fuzzing Firmware"))
            .await
            .unwrap();
        // Chunk for the semantic snippet lookup.
        crate::search::store::replace_chunks(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 1,
                page: Some(7),
                text: "router fuzz harness details".into(),
            }],
            "hash-a",
            "mh",
        )
        .await
        .unwrap();

        // Wiremock plays both Qdrant and the embedding API.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"index": 0, "embedding": [0.1, 0.2, 0.3, 0.4]}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/collections/xuewen/points/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": [{"id": "x", "score": 0.9, "payload": {"paper_id": "a", "seq": 1, "page": 7}}]
            })))
            .mount(&server).await;

        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        std::mem::forget(dir);
        fts_idx
            .upsert(&fts::PaperDoc {
                id: "a".into(),
                title: "Fuzzing Firmware".into(),
                authors: String::new(),
                venue: String::new(),
                abstract_text: String::new(),
                body: "we fuzz routers".into(),
            })
            .unwrap();
        let vectors = vector::QdrantStore::new(&server.uri(), "xuewen", 4).unwrap();
        let embedder = embedder::Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let svc = SearchService::open_with(pool, fts_idx, vectors, Some(embedder));

        let out = svc
            .search(&SearchRequest {
                q: "fuzzing".into(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();

        assert!(out.semantic.available);
        assert_eq!(out.results.len(), 1);
        assert_eq!(out.results[0].1.engine, "both");
        assert!(
            out.results[0].1.snippet.contains("<mark>"),
            "keyword snippet preferred"
        );
    }

    #[tokio::test]
    async fn semantic_only_hit_uses_chunk_text_snippet() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "Some Paper"))
            .await
            .unwrap();
        crate::search::store::replace_chunks(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 2,
                page: Some(3),
                text: "novel <escaping> content".into(),
            }],
            "hash-a",
            "mh",
        )
        .await
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"index": 0, "embedding": [0.1, 0.2, 0.3, 0.4]}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/collections/xuewen/points/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": [{"id": "x", "score": 0.9, "payload": {"paper_id": "a", "seq": 2, "page": 3}}]
            })))
            .mount(&server).await;

        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        std::mem::forget(dir);
        let vectors = vector::QdrantStore::new(&server.uri(), "xuewen", 4).unwrap();
        let embedder = embedder::Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let svc = SearchService::open_with(pool, fts_idx, vectors, Some(embedder));

        let out = svc
            .search(&SearchRequest {
                q: "different words entirely".into(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();

        assert_eq!(out.results.len(), 1);
        let m = &out.results[0].1;
        assert_eq!(m.engine, "semantic");
        assert_eq!(m.field, "body");
        assert_eq!(m.page, Some(3));
        assert!(
            m.snippet.contains("&lt;escaping&gt;"),
            "chunk text must be HTML-escaped: {}",
            m.snippet
        );
    }

    #[tokio::test]
    async fn semantic_failure_degrades_with_reason() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "Fuzzing Firmware"))
            .await
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        std::mem::forget(dir);
        fts_idx
            .upsert(&fts::PaperDoc {
                id: "a".into(),
                title: "Fuzzing Firmware".into(),
                authors: String::new(),
                venue: String::new(),
                abstract_text: String::new(),
                body: String::new(),
            })
            .unwrap();
        // Embedder points at a dead port -> semantic path errors.
        let vectors = vector::QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap();
        let embedder = embedder::Embedder::for_tests("http://127.0.0.1:1/v1", "m", 4);
        let svc = SearchService::open_with(pool, fts_idx, vectors, Some(embedder));

        let out = svc
            .search(&SearchRequest {
                q: "fuzzing".into(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();

        assert!(!out.semantic.available);
        assert!(out.semantic.reason.is_some());
        assert_eq!(out.results.len(), 1, "keyword results still returned");
    }

    #[tokio::test]
    async fn authors_only_selection_skips_semantic() {
        let pool = pool().await;
        let svc = keyword_only_service(pool).await;
        let out = svc
            .search(&SearchRequest {
                q: "lovelace".into(),
                fields: fts::FieldSel {
                    title: false,
                    authors: true,
                    abstract_text: false,
                    body: false,
                },
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();
        // Semantic was requested but is meaningless for authors-only.
        assert!(!out.semantic.available);
    }

    #[tokio::test]
    async fn status_counts_pending_and_failed() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "T"))
            .await
            .unwrap();
        let svc = keyword_only_service(pool).await;
        let st = svc.status().await.unwrap();
        assert_eq!(st.fts.pending, 1); // never indexed
        assert_eq!(st.fts.failed, 0);
        assert!(!st.semantic_available);
    }

    #[tokio::test]
    async fn authors_only_disables_semantic_even_with_embedder() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "Some Paper"))
            .await
            .unwrap();
        crate::search::store::replace_chunks(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 0,
                page: None,
                text: "Ada content".into(),
            }],
            "hash-a",
            "mh",
        )
        .await
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"index": 0, "embedding": [0.1, 0.2, 0.3, 0.4]}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": []
            })))
            .mount(&server)
            .await;

        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        std::mem::forget(dir);
        let vectors = vector::QdrantStore::new(&server.uri(), "xuewen", 4).unwrap();
        let embedder = embedder::Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let svc = SearchService::open_with(pool, fts_idx, vectors, Some(embedder));

        let out = svc
            .search(&SearchRequest {
                q: "ada".into(),
                fields: fts::FieldSel {
                    title: false,
                    authors: true,
                    abstract_text: false,
                    body: false,
                },
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();

        assert!(
            !out.semantic.available,
            "semantic should be disabled for authors-only"
        );
        assert_eq!(
            out.semantic.reason.as_deref(),
            Some("semantic search does not apply to an authors-only query"),
            "reason should explain authors-only disables semantic"
        );
    }

    #[tokio::test]
    async fn status_counts_failed_rows() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "T"))
            .await
            .unwrap();
        crate::search::store::replace_chunks(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 0,
                page: None,
                text: "some chunk".into(),
            }],
            "hash-a",
            "mh",
        )
        .await
        .unwrap();
        crate::search::store::record_error(&pool, "a", "boom")
            .await
            .unwrap();

        let svc = keyword_only_service(pool).await;
        let st = svc.status().await.unwrap();
        assert_eq!(st.fts.failed, 1, "status should count papers with errors");
    }

    #[tokio::test]
    async fn semantic_snippet_truncates_long_chunks_with_ellipsis() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "Some Paper"))
            .await
            .unwrap();
        let long_text = "x".repeat(250);
        crate::search::store::replace_chunks(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 2,
                page: Some(3),
                text: long_text,
            }],
            "hash-a",
            "mh",
        )
        .await
        .unwrap();

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [{"index": 0, "embedding": [0.1, 0.2, 0.3, 0.4]}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("POST")).and(path("/collections/xuewen/points/search"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": [{"id": "x", "score": 0.9, "payload": {"paper_id": "a", "seq": 2, "page": 3}}]
            })))
            .mount(&server).await;

        let dir = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(dir.path()).unwrap();
        std::mem::forget(dir);
        let vectors = vector::QdrantStore::new(&server.uri(), "xuewen", 4).unwrap();
        let embedder = embedder::Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let svc = SearchService::open_with(pool, fts_idx, vectors, Some(embedder));

        let out = svc
            .search(&SearchRequest {
                q: "different words entirely".into(),
                fields: fts::FieldSel::all(),
                keyword: true,
                semantic: true,
                status: None,
                project: None,
                tag: None,
                starred: None,
            })
            .await
            .unwrap();

        assert_eq!(out.results.len(), 1);
        let m = &out.results[0].1;
        assert_eq!(m.engine, "semantic");
        assert!(
            m.snippet.ends_with("…"),
            "snippet should end with ellipsis: {}",
            m.snippet
        );
        let text_before_ellipsis = m.snippet.trim_end_matches('…');
        assert_eq!(
            text_before_ellipsis.chars().count(),
            200,
            "text before ellipsis should be exactly 200 chars"
        );
    }

    #[tokio::test]
    async fn status_counts_backed_off_failures_as_pending() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("a", "T"))
            .await
            .unwrap();
        crate::search::store::replace_chunks(
            &pool,
            "a",
            &[crate::search::chunker::Chunk {
                seq: 0,
                page: None,
                text: "T".into(),
            }],
            "hash-a",
            "mh",
        )
        .await
        .unwrap();
        crate::search::store::record_error(&pool, "a", "boom")
            .await
            .unwrap();
        let svc = keyword_only_service(pool).await;
        let st = svc.status().await.unwrap();
        assert_eq!(
            st.fts.pending, 1,
            "failed+backed-off paper is still pending"
        );
        assert_eq!(st.fts.indexed, 0);
        assert_eq!(st.fts.failed, 1);
        assert_eq!(st.vectors.indexed, 0);
        assert_eq!(st.vectors.pending, 0, "no embedder -> vectors tier idle");
    }
}
