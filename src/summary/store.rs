//! Persistence for per-paper summaries (`paper_summaries`).

use anyhow::Result;
use sqlx::SqlitePool;

use crate::summary::Summary;

/// The stored summary for a paper, or `None` if not yet generated.
pub async fn get(pool: &SqlitePool, paper_id: &str) -> Result<Option<Summary>> {
    let row: Option<(String,)> =
        sqlx::query_as("SELECT summary FROM paper_summaries WHERE paper_id = ?")
            .bind(paper_id)
            .fetch_optional(pool)
            .await?;
    match row {
        Some((json,)) => Ok(Some(serde_json::from_str(&json)?)),
        None => Ok(None),
    }
}

/// Insert or replace a paper's summary.
pub async fn upsert(pool: &SqlitePool, paper_id: &str, s: &Summary, model: &str) -> Result<()> {
    let json = serde_json::to_string(s)?;
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO paper_summaries (paper_id, summary, model, generated_at) \
         VALUES (?, ?, ?, ?) \
         ON CONFLICT(paper_id) DO UPDATE SET summary = excluded.summary, \
             model = excluded.model, generated_at = excluded.generated_at",
    )
    .bind(paper_id)
    .bind(&json)
    .bind(model)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Ids of live papers due for a summary: no summary row, and either never
/// failed or past the backoff window (`last_attempt_at < retry_before`) and
/// under `max_attempts`. Never-failed papers sort first so failures never
/// starve the queue.
pub async fn due_ids(
    pool: &SqlitePool,
    limit: i64,
    max_attempts: i64,
    retry_before: &str,
) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT p.id FROM papers p \
         LEFT JOIN paper_summaries s ON s.paper_id = p.id \
         LEFT JOIN summary_failures f ON f.paper_id = p.id \
         WHERE p.deleted_at IS NULL AND s.paper_id IS NULL \
           AND (f.paper_id IS NULL OR (f.attempts < ? AND f.last_attempt_at < ?)) \
         ORDER BY (f.paper_id IS NOT NULL), p.added_at DESC \
         LIMIT ?",
    )
    .bind(max_attempts)
    .bind(retry_before)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

/// Record (or increment) a failed generation attempt.
pub async fn record_failure(pool: &SqlitePool, paper_id: &str) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO summary_failures (paper_id, attempts, last_attempt_at) \
         VALUES (?, 1, ?) \
         ON CONFLICT(paper_id) DO UPDATE SET attempts = attempts + 1, \
             last_attempt_at = excluded.last_attempt_at",
    )
    .bind(paper_id)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Delete one paper's summary (`Some`) or every summary (`None`).
pub async fn clear(pool: &SqlitePool, paper_id: Option<&str>) -> Result<()> {
    match paper_id {
        Some(id) => {
            sqlx::query("DELETE FROM paper_summaries WHERE paper_id = ?")
                .bind(id)
                .execute(pool)
                .await?;
        }
        None => {
            sqlx::query("DELETE FROM paper_summaries").execute(pool).await?;
        }
    }
    Ok(())
}

/// Clear one paper's failure record (`Some`) or all of them (`None`).
pub async fn clear_failure(pool: &SqlitePool, paper_id: Option<&str>) -> Result<()> {
    match paper_id {
        Some(id) => {
            sqlx::query("DELETE FROM summary_failures WHERE paper_id = ?")
                .bind(id)
                .execute(pool)
                .await?;
        }
        None => {
            sqlx::query("DELETE FROM summary_failures").execute(pool).await?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Authors, Paper, PaperMeta, PaperStatus};

    async fn pool() -> sqlx::SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let p = crate::db::connect(&url).await.unwrap();
        std::mem::forget(dir); // keep the temp file alive for the test process
        p
    }

    fn paper(id: &str) -> Paper {
        Paper {
            id: id.into(),
            content_hash: format!("h-{id}"),
            rel_path: format!("{id}.pdf"),
            cite_key: None,
            added_at: "2026-07-12T00:00:00Z".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some("T".into()),
                abstract_text: Some("A".into()),
                authors: Authors(vec!["Ada".into()]),
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

    fn sample() -> Summary {
        Summary {
            tldr: "One line.".into(),
            problem: "Gap.".into(),
            approach: "Idea.".into(),
            results: "+4.2.".into(),
            limitations: "Small.".into(),
        }
    }

    // High cap, far-future cutoff: no failure rows exist in most tests, so this
    // returns every un-summarized live paper (mirrors old `missing_ids` behavior).
    const FUTURE_CUTOFF: &str = "9999-01-01T00:00:00Z";

    #[tokio::test]
    async fn upsert_get_missing_and_clear() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("p1")).await.unwrap();
        crate::db::insert_paper(&pool, &paper("p2")).await.unwrap();

        // Both papers start missing.
        let mut missing = due_ids(&pool, 10, 100, FUTURE_CUTOFF).await.unwrap();
        missing.sort();
        assert_eq!(missing, vec!["p1".to_string(), "p2".to_string()]);

        upsert(&pool, "p1", &sample(), "gpt-x").await.unwrap();
        assert_eq!(get(&pool, "p1").await.unwrap(), Some(sample()));
        assert_eq!(get(&pool, "p2").await.unwrap(), None);
        assert_eq!(
            due_ids(&pool, 10, 100, FUTURE_CUTOFF).await.unwrap(),
            vec!["p2".to_string()]
        );

        // upsert replaces.
        let mut two = sample();
        two.tldr = "Changed.".into();
        upsert(&pool, "p1", &two, "gpt-x").await.unwrap();
        assert_eq!(get(&pool, "p1").await.unwrap().unwrap().tldr, "Changed.");

        clear(&pool, Some("p1")).await.unwrap();
        assert_eq!(get(&pool, "p1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn due_ids_excludes_trashed() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("p1")).await.unwrap();
        crate::db::insert_paper(&pool, &paper("p2")).await.unwrap();

        crate::db::soft_delete(&pool, "p2").await.unwrap();

        let missing = due_ids(&pool, 10, 100, FUTURE_CUTOFF).await.unwrap();
        assert_eq!(missing, vec!["p1".to_string()]);
    }

    #[tokio::test]
    async fn due_ids_skips_recently_failed() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("p1")).await.unwrap();
        record_failure(&pool, "p1").await.unwrap();

        // last_attempt_at is "now", which is NOT before a cutoff far in the past.
        let past_cutoff = "2000-01-01T00:00:00Z";
        assert_eq!(due_ids(&pool, 10, 5, past_cutoff).await.unwrap(), Vec::<String>::new());

        // Eligible again once the cutoff is far in the future.
        assert_eq!(due_ids(&pool, 10, 5, FUTURE_CUTOFF).await.unwrap(), vec!["p1".to_string()]);
    }

    #[tokio::test]
    async fn due_ids_skips_at_max_attempts() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("p1")).await.unwrap();
        record_failure(&pool, "p1").await.unwrap();
        record_failure(&pool, "p1").await.unwrap();
        record_failure(&pool, "p1").await.unwrap();

        // attempts == 3, not < 3 -> excluded.
        assert_eq!(due_ids(&pool, 10, 3, FUTURE_CUTOFF).await.unwrap(), Vec::<String>::new());
        // attempts == 3 < 4 -> included.
        assert_eq!(due_ids(&pool, 10, 4, FUTURE_CUTOFF).await.unwrap(), vec!["p1".to_string()]);
    }

    #[tokio::test]
    async fn due_ids_untried_before_failed() {
        let pool = pool().await;
        let mut p_old = paper("p_old");
        p_old.added_at = "2026-01-01T00:00:00Z".into();
        let mut p_new = paper("p_new");
        p_new.added_at = "2026-06-01T00:00:00Z".into();
        crate::db::insert_paper(&pool, &p_old).await.unwrap();
        crate::db::insert_paper(&pool, &p_new).await.unwrap();

        record_failure(&pool, "p_new").await.unwrap();

        let ids = due_ids(&pool, 10, 5, FUTURE_CUTOFF).await.unwrap();
        assert_eq!(ids, vec!["p_old".to_string(), "p_new".to_string()]);
    }

    #[tokio::test]
    async fn clear_failure_removes_row() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("p1")).await.unwrap();
        record_failure(&pool, "p1").await.unwrap();
        assert_eq!(due_ids(&pool, 10, 5, "2000-01-01T00:00:00Z").await.unwrap(), Vec::<String>::new());

        clear_failure(&pool, Some("p1")).await.unwrap();
        assert_eq!(
            due_ids(&pool, 10, 5, "2000-01-01T00:00:00Z").await.unwrap(),
            vec!["p1".to_string()]
        );
    }

    #[tokio::test]
    async fn cascade_delete_removes_summary() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("p1")).await.unwrap();
        upsert(&pool, "p1", &sample(), "gpt-x").await.unwrap();
        assert_eq!(get(&pool, "p1").await.unwrap(), Some(sample()));

        crate::db::delete_row(&pool, "p1").await.unwrap();
        assert_eq!(get(&pool, "p1").await.unwrap(), None);
    }

    #[tokio::test]
    async fn clear_all_removes_every_row() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("p1")).await.unwrap();
        crate::db::insert_paper(&pool, &paper("p2")).await.unwrap();
        upsert(&pool, "p1", &sample(), "gpt-x").await.unwrap();
        upsert(&pool, "p2", &sample(), "gpt-x").await.unwrap();

        clear(&pool, None).await.unwrap();

        assert_eq!(get(&pool, "p1").await.unwrap(), None);
        assert_eq!(get(&pool, "p2").await.unwrap(), None);
    }
}
