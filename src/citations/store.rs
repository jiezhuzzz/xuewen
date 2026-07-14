//! Persistence for cached LLM parses of extracted reference strings.

use anyhow::Result;
use sqlx::SqlitePool;

/// Cached (parsed, provenance) for `paper_id`, only if the stored input
/// matches exactly. Provenance is the `model` column: `heuristic-v1` or
/// `heuristic-v1+<model>`.
pub async fn get(
    pool: &SqlitePool,
    paper_id: &str,
    refs_json: &str,
) -> Result<Option<(String, String)>> {
    let row: Option<(String, String, String)> =
        sqlx::query_as("SELECT refs, parsed, model FROM citation_parses WHERE paper_id = ?")
            .bind(paper_id)
            .fetch_optional(pool)
            .await?;
    Ok(row.and_then(|(refs, parsed, model)| (refs == refs_json).then_some((parsed, model))))
}

/// Insert or replace a paper's cached parse.
pub async fn upsert(
    pool: &SqlitePool,
    paper_id: &str,
    refs_json: &str,
    parsed_json: &str,
    model: &str,
) -> Result<()> {
    let now = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO citation_parses (paper_id, refs, parsed, model, created_at) \
         VALUES (?, ?, ?, ?, ?) \
         ON CONFLICT(paper_id) DO UPDATE SET refs = excluded.refs, \
             parsed = excluded.parsed, model = excluded.model, created_at = excluded.created_at",
    )
    .bind(paper_id)
    .bind(refs_json)
    .bind(parsed_json)
    .bind(model)
    .bind(&now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Migrated pool with one seeded paper — shared by store + service tests.
#[cfg(test)]
pub(crate) async fn tests_pool_with_paper(id: &str) -> sqlx::SqlitePool {
    use crate::models::{Authors, Paper, PaperMeta, PaperStatus};

    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = crate::db::connect(&url).await.unwrap();

    let paper = Paper {
        id: id.into(),
        content_hash: id.into(),
        rel_path: "test.pdf".into(),
        cite_key: None,
        added_at: "2026-07-13T00:00:00Z".into(),
        deleted_at: None,
        meta: PaperMeta {
            title: None,
            abstract_text: None,
            authors: Authors(vec![]),
            venue: None,
            year: None,
            doi: None,
            arxiv_id: None,
            dblp_key: None,
            url: None,
            source: None,
            status: PaperStatus::Resolved,
        },
    };
    crate::db::insert_paper(&pool, &paper).await.unwrap();

    std::mem::forget(dir); // keep the tempdir alive for the pool's lifetime
    pool
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn upsert_then_get_roundtrips_only_on_matching_input() {
        let pool = tests_pool_with_paper("p1").await;
        upsert(&pool, "p1", r#"["a","b"]"#, r#"[null,null]"#, "m")
            .await
            .unwrap();
        assert_eq!(
            get(&pool, "p1", r#"["a","b"]"#).await.unwrap(),
            Some((r#"[null,null]"#.to_string(), "m".to_string()))
        );
        // Different input (changed PDF) ⇒ miss.
        assert!(get(&pool, "p1", r#"["a","c"]"#).await.unwrap().is_none());
        // Re-upsert replaces.
        upsert(&pool, "p1", r#"["a","c"]"#, r#"[null]"#, "m")
            .await
            .unwrap();
        assert_eq!(
            get(&pool, "p1", r#"["a","c"]"#).await.unwrap(),
            Some((r#"[null]"#.to_string(), "m".to_string()))
        );
    }

    #[tokio::test]
    async fn get_on_unknown_paper_is_none() {
        let pool = tests_pool_with_paper("p1").await;
        assert!(get(&pool, "nope", "[]").await.unwrap().is_none());
    }
}
