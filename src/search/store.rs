use anyhow::Result;
use sha2::{Digest, Sha256};
use sqlx::{QueryBuilder, SqlitePool};

use crate::models::Paper;
use crate::search::chunker::Chunk;

/// State of a paper's derived search indexes. May outlive its paper (tombstone).
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct IndexRow {
    pub paper_id: String,
    pub content_hash: String,
    pub meta_hash: String,
    pub chunk_count: i64,
    pub fts_indexed_at: Option<String>,
    pub vectors_indexed_at: Option<String>,
    pub embed_model: Option<String>,
    pub last_error: Option<String>,
    pub attempts: i64,
    pub last_attempt_at: Option<String>,
}

/// Hash of the metadata that feeds the search indexes. Comparing this against
/// the stored value is how identify/refresh edits are detected without any
/// event plumbing in the mutation paths.
pub fn meta_hash(p: &Paper) -> String {
    let mut h = Sha256::new();
    for part in [
        p.meta.title.as_deref().unwrap_or(""),
        p.meta.abstract_text.as_deref().unwrap_or(""),
        p.meta.venue.as_deref().unwrap_or(""),
    ] {
        h.update(part.as_bytes());
        h.update([0x1f]);
    }
    h.update(p.meta.year.map(|y| y.to_string()).unwrap_or_default().as_bytes());
    h.update([0x1f]);
    h.update(serde_json::to_string(&p.meta.authors).unwrap_or_default().as_bytes());
    hex::encode(h.finalize())
}

pub async fn all_index_rows(pool: &SqlitePool) -> Result<Vec<IndexRow>> {
    let rows = sqlx::query_as::<_, IndexRow>("SELECT * FROM search_index")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

/// Replace a paper's chunks and reset its index row (stamps cleared: both
/// tiers must re-index the new content). One transaction.
pub async fn replace_chunks(
    pool: &SqlitePool,
    paper_id: &str,
    chunks: &[Chunk],
    content_hash: &str,
    meta_hash: &str,
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM chunks WHERE paper_id = ?")
        .bind(paper_id)
        .execute(&mut *tx)
        .await?;
    for c in chunks {
        sqlx::query("INSERT INTO chunks (paper_id, seq, page, text) VALUES (?,?,?,?)")
            .bind(paper_id)
            .bind(c.seq)
            .bind(c.page)
            .bind(&c.text)
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query(
        "INSERT INTO search_index (paper_id, content_hash, meta_hash, chunk_count) \
         VALUES (?,?,?,?) \
         ON CONFLICT(paper_id) DO UPDATE SET \
           content_hash = excluded.content_hash, meta_hash = excluded.meta_hash, \
           chunk_count = excluded.chunk_count, \
           fts_indexed_at = NULL, vectors_indexed_at = NULL",
    )
    .bind(paper_id)
    .bind(content_hash)
    .bind(meta_hash)
    .bind(chunks.len() as i64)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(())
}

pub async fn chunks_for_paper(pool: &SqlitePool, paper_id: &str) -> Result<Vec<Chunk>> {
    let rows: Vec<(i64, Option<i64>, String)> =
        sqlx::query_as("SELECT seq, page, text FROM chunks WHERE paper_id = ? ORDER BY seq")
            .bind(paper_id)
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(seq, page, text)| Chunk { seq, page, text })
        .collect())
}

pub async fn chunk_text(pool: &SqlitePool, paper_id: &str, seq: i64) -> Result<Option<Chunk>> {
    let row: Option<(i64, Option<i64>, String)> =
        sqlx::query_as("SELECT seq, page, text FROM chunks WHERE paper_id = ? AND seq = ?")
            .bind(paper_id)
            .bind(seq)
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|(seq, page, text)| Chunk { seq, page, text }))
}

pub async fn mark_fts_done(pool: &SqlitePool, paper_id: &str) -> Result<()> {
    sqlx::query(
        "UPDATE search_index SET fts_indexed_at = ?, attempts = 0, last_error = NULL \
         WHERE paper_id = ?",
    )
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(paper_id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn mark_vectors_done(pool: &SqlitePool, paper_id: &str, model: &str) -> Result<()> {
    sqlx::query(
        "UPDATE search_index SET vectors_indexed_at = ?, embed_model = ?, \
         attempts = 0, last_error = NULL WHERE paper_id = ?",
    )
    .bind(chrono::Utc::now().to_rfc3339())
    .bind(model)
    .bind(paper_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Record an indexing failure, upserting the row if one doesn't exist yet
/// (e.g. a brand-new paper whose PDF extraction fails before `replace_chunks`
/// ever runs). The placeholder empty hashes never match a real paper's
/// content/meta hash, so the planner still schedules the work once backoff
/// elapses.
pub async fn record_error(pool: &SqlitePool, paper_id: &str, msg: &str) -> Result<()> {
    sqlx::query(
        "INSERT INTO search_index (paper_id, content_hash, meta_hash, last_error, attempts, last_attempt_at) \
         VALUES (?, '', '', ?, 1, ?) \
         ON CONFLICT(paper_id) DO UPDATE SET \
           last_error = excluded.last_error, \
           attempts = search_index.attempts + 1, \
           last_attempt_at = excluded.last_attempt_at",
    )
    .bind(paper_id)
    .bind(msg)
    .bind(chrono::Utc::now().to_rfc3339())
    .execute(pool)
    .await?;
    Ok(())
}

/// Drop a paper's index row and chunks (used after de-indexing a tombstone).
pub async fn remove_index_entry(pool: &SqlitePool, paper_id: &str) -> Result<()> {
    sqlx::query("DELETE FROM chunks WHERE paper_id = ?")
        .bind(paper_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM search_index WHERE paper_id = ?")
        .bind(paper_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Force re-indexing of the given tier(s) for every paper (rebuild).
pub async fn clear_stamps(pool: &SqlitePool, fts: bool, vectors: bool) -> Result<()> {
    if fts {
        sqlx::query("UPDATE search_index SET fts_indexed_at = NULL, attempts = 0, last_error = NULL")
            .execute(pool)
            .await?;
    }
    if vectors {
        sqlx::query(
            "UPDATE search_index SET vectors_indexed_at = NULL, attempts = 0, last_error = NULL",
        )
        .execute(pool)
        .await?;
    }
    Ok(())
}

/// Fetch non-trashed papers by id, preserving the order of `ids` (fusion
/// order), applying the status/project filters the search endpoint supports.
pub async fn papers_by_ids_ordered(
    pool: &SqlitePool,
    ids: &[String],
    status: Option<&str>,
    project: Option<&str>,
) -> Result<Vec<Paper>> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut qb: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("SELECT * FROM papers WHERE deleted_at IS NULL AND id IN (");
    let mut sep = qb.separated(", ");
    for id in ids {
        sep.push_bind(id);
    }
    qb.push(")");
    if let Some(st) = status.filter(|s| matches!(*s, "resolved" | "needs_review")) {
        qb.push(" AND status = ").push_bind(st.to_string());
    }
    if let Some(pid) = project.map(str::trim).filter(|s| !s.is_empty()) {
        qb.push(" AND id IN (SELECT paper_id FROM paper_projects WHERE project_id = ")
            .push_bind(pid.to_string())
            .push(")");
    }
    let papers = qb.build_query_as::<Paper>().fetch_all(pool).await?;
    // Reorder to match `ids` (SQL IN gives no ordering guarantee).
    let mut by_id: std::collections::HashMap<String, Paper> =
        papers.into_iter().map(|p| (p.id.clone(), p)).collect();
    Ok(ids.iter().filter_map(|id| by_id.remove(id)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Authors, PaperMeta, PaperStatus};
    use crate::search::chunker::Chunk;

    async fn pool() -> sqlx::SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let p = crate::db::connect(&url).await.unwrap();
        std::mem::forget(dir); // keep the tempdir alive for the test process
        p
    }

    fn paper(id: &str, hash: &str, title: &str) -> Paper {
        Paper {
            id: id.into(),
            content_hash: hash.into(),
            rel_path: format!("{hash}.pdf"),
            cite_key: None,
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            meta: PaperMeta {
                title: Some(title.into()),
                abstract_text: None,
                authors: Authors::default(),
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

    fn two_chunks() -> Vec<Chunk> {
        vec![
            Chunk { seq: 0, page: None, text: "T\nA".into() },
            Chunk { seq: 1, page: Some(1), text: "body".into() },
        ]
    }

    #[test]
    fn meta_hash_changes_with_metadata_only() {
        let a = paper("p1", "h1", "Title One");
        let mut b = paper("p1", "h1", "Title One");
        assert_eq!(meta_hash(&a), meta_hash(&b));
        b.meta.title = Some("Title Two".into());
        assert_ne!(meta_hash(&a), meta_hash(&b));
    }

    #[tokio::test]
    async fn replace_chunks_roundtrip_and_stamp_lifecycle() {
        let pool = pool().await;
        let p = paper("p1", "h1", "T");
        crate::db::insert_paper(&pool, &p).await.unwrap();

        replace_chunks(&pool, "p1", &two_chunks(), "h1", &meta_hash(&p)).await.unwrap();
        let got = chunks_for_paper(&pool, "p1").await.unwrap();
        assert_eq!(got, two_chunks());
        assert_eq!(chunk_text(&pool, "p1", 1).await.unwrap().unwrap().text, "body");

        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert_eq!(row.chunk_count, 2);
        assert!(row.fts_indexed_at.is_none() && row.vectors_indexed_at.is_none());

        mark_fts_done(&pool, "p1").await.unwrap();
        mark_vectors_done(&pool, "p1", "text-embedding-3-small").await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_some() && row.vectors_indexed_at.is_some());
        assert_eq!(row.embed_model.as_deref(), Some("text-embedding-3-small"));
        assert_eq!(row.attempts, 0);
        assert!(row.last_error.is_none());

        // Replacing chunks again clears the stamps (fresh index required).
        replace_chunks(&pool, "p1", &two_chunks(), "h2", &meta_hash(&p)).await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_none() && row.vectors_indexed_at.is_none());
        assert_eq!(row.content_hash, "h2");
    }

    #[tokio::test]
    async fn record_error_increments_attempts_and_marks_reset_them() {
        let pool = pool().await;
        let p = paper("p1", "h1", "T");
        crate::db::insert_paper(&pool, &p).await.unwrap();
        replace_chunks(&pool, "p1", &two_chunks(), "h1", &meta_hash(&p)).await.unwrap();

        record_error(&pool, "p1", "boom").await.unwrap();
        record_error(&pool, "p1", "boom2").await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert_eq!(row.attempts, 2);
        assert_eq!(row.last_error.as_deref(), Some("boom2"));
        assert!(row.last_attempt_at.is_some());

        mark_fts_done(&pool, "p1").await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert_eq!(row.attempts, 0);
        assert!(row.last_error.is_none());
    }

    #[tokio::test]
    async fn record_error_without_prior_row_creates_one() {
        let pool = pool().await;
        let p = paper("p1", "h1", "T");
        crate::db::insert_paper(&pool, &p).await.unwrap();
        // No replace_chunks call: no search_index row exists yet (mirrors a
        // brand-new paper whose PDF extraction fails before chunking).
        assert!(all_index_rows(&pool).await.unwrap().is_empty());

        record_error(&pool, "p1", "extraction failed").await.unwrap();
        let rows = all_index_rows(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].attempts, 1);
        assert_eq!(rows[0].last_error.as_deref(), Some("extraction failed"));
        assert_eq!(rows[0].content_hash, "");
        assert_eq!(rows[0].meta_hash, "");

        record_error(&pool, "p1", "extraction failed again").await.unwrap();
        let rows = all_index_rows(&pool).await.unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].attempts, 2);
        assert_eq!(rows[0].last_error.as_deref(), Some("extraction failed again"));
    }

    #[tokio::test]
    async fn remove_and_clear_stamps() {
        let pool = pool().await;
        let p = paper("p1", "h1", "T");
        crate::db::insert_paper(&pool, &p).await.unwrap();
        replace_chunks(&pool, "p1", &two_chunks(), "h1", &meta_hash(&p)).await.unwrap();
        mark_fts_done(&pool, "p1").await.unwrap();
        mark_vectors_done(&pool, "p1", "m").await.unwrap();

        clear_stamps(&pool, false, true).await.unwrap();
        let row = &all_index_rows(&pool).await.unwrap()[0];
        assert!(row.fts_indexed_at.is_some() && row.vectors_indexed_at.is_none());

        remove_index_entry(&pool, "p1").await.unwrap();
        assert!(all_index_rows(&pool).await.unwrap().is_empty());
        assert!(chunks_for_paper(&pool, "p1").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn papers_by_ids_ordered_preserves_order_and_filters() {
        let pool = pool().await;
        for (id, hash, title) in [("a", "h1", "A"), ("b", "h2", "B"), ("c", "h3", "C")] {
            crate::db::insert_paper(&pool, &paper(id, hash, title)).await.unwrap();
        }
        crate::db::soft_delete(&pool, "c").await.unwrap();

        let ids = vec!["c".to_string(), "b".to_string(), "a".to_string(), "zz".to_string()];
        let got = papers_by_ids_ordered(&pool, &ids, None, None).await.unwrap();
        let got_ids: Vec<&str> = got.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(got_ids, vec!["b", "a"]); // trashed + unknown dropped, order kept
    }
}
