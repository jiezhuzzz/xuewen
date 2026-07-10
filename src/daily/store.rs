use anyhow::Result;
use sqlx::SqlitePool;
use std::collections::HashSet;

/// One recommended paper in a daily batch. Columns match `daily_papers`.
#[derive(Debug, Clone, PartialEq)]
pub struct DailyPaper {
    pub batch_date: String,
    /// 1-based, by descending score.
    pub rank: i64,
    /// Versionless arXiv id, e.g. "2507.01234".
    pub arxiv_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub abstract_text: String,
    pub categories: Vec<String>,
    pub score: f64,
    /// `None` when TL;DR generation failed (widget falls back to abstract).
    pub tldr: Option<String>,
    pub abs_url: String,
    pub pdf_url: String,
}

/// Outcome row for one day's run. Columns match `daily_runs`.
#[derive(Debug, Clone, PartialEq)]
pub struct DailyRun {
    pub batch_date: String,
    /// "ok" | "empty" | "failed"
    pub status: String,
    /// Candidates after dedup, before top-N.
    pub papers_found: i64,
    pub error: Option<String>,
    pub ran_at: String,
}

pub async fn record_run(pool: &SqlitePool, run: &DailyRun) -> Result<()> {
    sqlx::query(
        "INSERT INTO daily_runs (batch_date, status, papers_found, error, ran_at)
         VALUES (?, ?, ?, ?, ?)
         ON CONFLICT(batch_date) DO UPDATE SET
           status = excluded.status, papers_found = excluded.papers_found,
           error = excluded.error, ran_at = excluded.ran_at",
    )
    .bind(&run.batch_date)
    .bind(&run.status)
    .bind(run.papers_found)
    .bind(&run.error)
    .bind(&run.ran_at)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn get_run(pool: &SqlitePool, batch_date: &str) -> Result<Option<DailyRun>> {
    let row: Option<(String, String, i64, Option<String>, String)> = sqlx::query_as(
        "SELECT batch_date, status, papers_found, error, ran_at
         FROM daily_runs WHERE batch_date = ?",
    )
    .bind(batch_date)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|(batch_date, status, papers_found, error, ran_at)| DailyRun {
        batch_date,
        status,
        papers_found,
        error,
        ran_at,
    }))
}

/// Replace `batch_date`'s papers in one transaction (re-runs overwrite).
pub async fn replace_batch(
    pool: &SqlitePool,
    batch_date: &str,
    papers: &[DailyPaper],
) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query("DELETE FROM daily_papers WHERE batch_date = ?")
        .bind(batch_date)
        .execute(&mut *tx)
        .await?;
    for p in papers {
        sqlx::query(
            "INSERT INTO daily_papers
               (batch_date, rank, arxiv_id, title, authors, abstract,
                categories, score, tldr, abs_url, pdf_url)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(batch_date)
        .bind(p.rank)
        .bind(&p.arxiv_id)
        .bind(&p.title)
        .bind(serde_json::to_string(&p.authors)?)
        .bind(&p.abstract_text)
        .bind(serde_json::to_string(&p.categories)?)
        .bind(p.score)
        .bind(&p.tldr)
        .bind(&p.abs_url)
        .bind(&p.pdf_url)
        .execute(&mut *tx)
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

/// The newest batch that has papers, in rank order.
pub async fn latest_batch(pool: &SqlitePool) -> Result<Option<(String, Vec<DailyPaper>)>> {
    let date: Option<(String,)> =
        sqlx::query_as("SELECT batch_date FROM daily_papers ORDER BY batch_date DESC LIMIT 1")
            .fetch_optional(pool)
            .await?;
    let Some((date,)) = date else { return Ok(None) };
    type Row = (
        String,
        i64,
        String,
        String,
        String,
        String,
        String,
        f64,
        Option<String>,
        String,
        String,
    );
    let rows: Vec<Row> = sqlx::query_as(
        "SELECT batch_date, rank, arxiv_id, title, authors, abstract,
                categories, score, tldr, abs_url, pdf_url
         FROM daily_papers WHERE batch_date = ? ORDER BY rank",
    )
    .bind(&date)
    .fetch_all(pool)
    .await?;
    let papers = rows
        .into_iter()
        .map(|r| -> Result<DailyPaper> {
            Ok(DailyPaper {
                batch_date: r.0,
                rank: r.1,
                arxiv_id: r.2,
                title: r.3,
                authors: serde_json::from_str(&r.4)?,
                abstract_text: r.5,
                categories: serde_json::from_str(&r.6)?,
                score: r.7,
                tldr: r.8,
                abs_url: r.9,
                pdf_url: r.10,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(Some((date, papers)))
}

/// Delete rows with `batch_date < cutoff` (YYYY-MM-DD compares correctly
/// as text) from both tables.
pub async fn prune(pool: &SqlitePool, cutoff: &str) -> Result<()> {
    sqlx::query("DELETE FROM daily_papers WHERE batch_date < ?")
        .bind(cutoff)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM daily_runs WHERE batch_date < ?")
        .bind(cutoff)
        .execute(pool)
        .await?;
    Ok(())
}

/// Every arXiv id in the library, INCLUDING trashed papers: a deleted
/// paper was a deliberate removal, so we never recommend it again.
pub async fn library_arxiv_ids(pool: &SqlitePool) -> Result<HashSet<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT arxiv_id FROM papers WHERE arxiv_id IS NOT NULL")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn pool() -> sqlx::SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let p = crate::db::connect(&url).await.unwrap();
        std::mem::forget(dir);
        p
    }

    fn paper(date: &str, rank: i64, id: &str) -> DailyPaper {
        DailyPaper {
            batch_date: date.into(),
            rank,
            arxiv_id: id.into(),
            title: format!("Paper {id}"),
            authors: vec!["Ada Lovelace".into(), "Alan Turing".into()],
            abstract_text: "We do things.".into(),
            categories: vec!["cs.AI".into()],
            score: 0.5,
            tldr: Some("Short.".into()),
            abs_url: format!("https://arxiv.org/abs/{id}"),
            pdf_url: format!("https://arxiv.org/pdf/{id}"),
        }
    }

    #[tokio::test]
    async fn record_run_upserts_by_date() {
        let pool = pool().await;
        let mut run = DailyRun {
            batch_date: "2026-07-10".into(),
            status: "failed".into(),
            papers_found: 0,
            error: Some("boom".into()),
            ran_at: "2026-07-10T09:00:00Z".into(),
        };
        record_run(&pool, &run).await.unwrap();
        run.status = "ok".into();
        run.papers_found = 5;
        run.error = None;
        record_run(&pool, &run).await.unwrap();
        let got = get_run(&pool, "2026-07-10").await.unwrap().unwrap();
        assert_eq!(got.status, "ok");
        assert_eq!(got.papers_found, 5);
        assert_eq!(got.error, None);
        assert!(get_run(&pool, "2026-07-09").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn replace_batch_and_latest_batch_roundtrip() {
        let pool = pool().await;
        replace_batch(
            &pool,
            "2026-07-09",
            &[paper("2026-07-09", 1, "2507.00001")],
        )
        .await
        .unwrap();
        replace_batch(
            &pool,
            "2026-07-10",
            &[
                paper("2026-07-10", 1, "2507.00002"),
                paper("2026-07-10", 2, "2507.00003"),
            ],
        )
        .await
        .unwrap();

        let (date, papers) = latest_batch(&pool).await.unwrap().unwrap();
        assert_eq!(date, "2026-07-10");
        assert_eq!(papers.len(), 2);
        assert_eq!(papers[0].rank, 1);
        assert_eq!(papers[0].arxiv_id, "2507.00002");
        assert_eq!(papers[0].authors, vec!["Ada Lovelace", "Alan Turing"]);
        assert_eq!(papers[0].categories, vec!["cs.AI"]);

        // Re-run replaces the date's rows.
        replace_batch(
            &pool,
            "2026-07-10",
            &[paper("2026-07-10", 1, "2507.00009")],
        )
        .await
        .unwrap();
        let (_, papers) = latest_batch(&pool).await.unwrap().unwrap();
        assert_eq!(papers.len(), 1);
        assert_eq!(papers[0].arxiv_id, "2507.00009");
    }

    #[tokio::test]
    async fn latest_batch_none_when_empty() {
        let pool = pool().await;
        assert!(latest_batch(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn prune_deletes_older_batches_and_runs() {
        let pool = pool().await;
        for date in ["2026-06-01", "2026-07-10"] {
            replace_batch(&pool, date, &[paper(date, 1, "x")]).await.unwrap();
            record_run(
                &pool,
                &DailyRun {
                    batch_date: date.into(),
                    status: "ok".into(),
                    papers_found: 1,
                    error: None,
                    ran_at: format!("{date}T09:00:00Z"),
                },
            )
            .await
            .unwrap();
        }
        prune(&pool, "2026-06-26").await.unwrap();
        assert!(get_run(&pool, "2026-06-01").await.unwrap().is_none());
        assert!(get_run(&pool, "2026-07-10").await.unwrap().is_some());
        let (date, _) = latest_batch(&pool).await.unwrap().unwrap();
        assert_eq!(date, "2026-07-10");
    }

    #[tokio::test]
    async fn library_arxiv_ids_includes_trashed() {
        let pool = pool().await;
        let mut p = crate::models::Paper {
            id: "p1".into(),
            content_hash: "h1".into(),
            rel_path: "p1.pdf".into(),
            cite_key: None,
            added_at: "2026-07-01T00:00:00Z".into(),
            deleted_at: None,
            meta: crate::models::PaperMeta {
                title: Some("T".into()),
                abstract_text: None,
                authors: crate::models::Authors(vec![]),
                venue: None,
                year: None,
                doi: None,
                arxiv_id: Some("2401.00001".into()),
                dblp_key: None,
                url: None,
                source: None,
                status: crate::models::PaperStatus::Resolved,
            },
        };
        crate::db::insert_paper(&pool, &p).await.unwrap();
        p.id = "p2".into();
        p.content_hash = "h2".into();
        p.rel_path = "p2.pdf".into();
        p.meta.arxiv_id = Some("2401.00002".into());
        crate::db::insert_paper(&pool, &p).await.unwrap();
        crate::db::soft_delete(&pool, "p2").await.unwrap();

        let ids = library_arxiv_ids(&pool).await.unwrap();
        assert!(ids.contains("2401.00001"));
        assert!(ids.contains("2401.00002"), "trashed papers still dedupe");
        assert_eq!(ids.len(), 2);
    }
}
