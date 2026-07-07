use anyhow::Result;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

use crate::models::Paper;

/// Open (creating if needed) the SQLite database and run migrations.
pub async fn connect(database_url: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(database_url)?.create_if_missing(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

pub async fn exists_by_hash(pool: &SqlitePool, content_hash: &str) -> Result<bool> {
    let row: Option<(String,)> = sqlx::query_as("SELECT id FROM papers WHERE content_hash = ?")
        .bind(content_hash)
        .fetch_optional(pool)
        .await?;
    Ok(row.is_some())
}

pub async fn insert_paper(pool: &SqlitePool, p: &Paper) -> Result<()> {
    sqlx::query(
        "INSERT INTO papers \
         (id, content_hash, rel_path, title, abstract, authors, venue, year, \
          doi, arxiv_id, dblp_key, url, source, status, added_at) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&p.id)
    .bind(&p.content_hash)
    .bind(&p.rel_path)
    .bind(&p.title)
    .bind(&p.abstract_text)
    .bind(&p.authors)
    .bind(&p.venue)
    .bind(p.year)
    .bind(&p.doi)
    .bind(&p.arxiv_id)
    .bind(&p.dblp_key)
    .bind(&p.url)
    .bind(&p.source)
    .bind(&p.status)
    .bind(&p.added_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_by_id(pool: &SqlitePool, id: &str) -> Result<Option<Paper>> {
    let p = sqlx::query_as::<_, Paper>("SELECT * FROM papers WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::PaperStatus;

    fn sample_paper(id: &str, hash: &str) -> Paper {
        Paper {
            id: id.to_string(),
            content_hash: hash.to_string(),
            rel_path: format!("{hash}.pdf"),
            title: Some("A Title".into()),
            abstract_text: None,
            authors: None,
            venue: None,
            year: None,
            doi: None,
            arxiv_id: None,
            dblp_key: None,
            url: None,
            source: None,
            status: PaperStatus::NeedsReview.as_str().to_string(),
            added_at: "2026-07-06T00:00:00Z".to_string(),
        }
    }

    async fn temp_pool() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let url = format!("sqlite:{}", db_path.display());
        let pool = connect(&url).await.unwrap();
        (dir, pool)
    }

    #[tokio::test]
    async fn insert_then_fetch_and_dedup() {
        let (_dir, pool) = temp_pool().await;

        assert!(!exists_by_hash(&pool, "abc").await.unwrap());

        let p = sample_paper("01890000-0000-7000-8000-000000000000", "abc");
        insert_paper(&pool, &p).await.unwrap();

        assert!(exists_by_hash(&pool, "abc").await.unwrap());

        let got = get_by_id(&pool, &p.id).await.unwrap().unwrap();
        assert_eq!(got.content_hash, "abc");
        assert_eq!(got.title.as_deref(), Some("A Title"));
        assert_eq!(got.status, "needs_review");
    }
}
