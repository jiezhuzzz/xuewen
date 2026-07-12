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

/// Ids of live (non-trashed) papers with no summary row yet, capped at `limit`.
pub async fn missing_ids(pool: &SqlitePool, limit: i64) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT p.id FROM papers p \
         LEFT JOIN paper_summaries s ON s.paper_id = p.id \
         WHERE p.deleted_at IS NULL AND s.paper_id IS NULL \
         ORDER BY p.added_at DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
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

    #[tokio::test]
    async fn upsert_get_missing_and_clear() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("p1")).await.unwrap();
        crate::db::insert_paper(&pool, &paper("p2")).await.unwrap();

        // Both papers start missing.
        let mut missing = missing_ids(&pool, 10).await.unwrap();
        missing.sort();
        assert_eq!(missing, vec!["p1".to_string(), "p2".to_string()]);

        upsert(&pool, "p1", &sample(), "gpt-x").await.unwrap();
        assert_eq!(get(&pool, "p1").await.unwrap(), Some(sample()));
        assert_eq!(get(&pool, "p2").await.unwrap(), None);
        assert_eq!(missing_ids(&pool, 10).await.unwrap(), vec!["p2".to_string()]);

        // upsert replaces.
        let mut two = sample();
        two.tldr = "Changed.".into();
        upsert(&pool, "p1", &two, "gpt-x").await.unwrap();
        assert_eq!(get(&pool, "p1").await.unwrap().unwrap().tldr, "Changed.");

        clear(&pool, Some("p1")).await.unwrap();
        assert_eq!(get(&pool, "p1").await.unwrap(), None);
    }
}
