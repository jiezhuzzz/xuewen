# Daily arXiv Recommendations (Glance Source) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Once a day, fetch new arXiv papers, rank them by embedding similarity to the Xuewen library, TL;DR the top N with an LLM, store the batch in SQLite, and serve it at `GET /api/daily` for a Glance custom-api widget.

**Architecture:** New `src/daily/` module (feed fetch → dedup → interest-profile scoring → TL;DR → store), an in-process tokio scheduler spawned by `serve`, one migration (`daily_runs` + `daily_papers`), and two axum routes. The interest profile is a recency-weighted sum of the library's seq-0 Qdrant vectors; scoring a candidate is one dot product. Full spec: `docs/superpowers/specs/2026-07-10-daily-arxiv-glance-design.md`.

**Tech Stack:** Rust (axum 0.8, sqlx/SQLite, reqwest, roxmltree, chrono, tokio), wiremock + axum-test for tests. Reuses existing `Embedder`, `QdrantStore`, `pdf::extract_text`, `resolve::http::HttpClient`.

## Global Constraints

- **No new crate dependencies.** Everything needed (chrono, uuid, roxmltree, reqwest, sqlx, serde_json, wiremock, axum-test, tempfile) is already in `Cargo.toml`.
- Run tests with `cargo test` from the repo root. If a tool is missing (not in the dev shell), wrap: `nix develop -c 'cargo test'`. `pdftotext` (poppler-utils) is in the dev shell.
- Defaults (from the spec, use these exact values): `max_papers = 20`, `run_at = "09:00"` (UTC), `retention_days = 14`, LLM `base_url = "https://api.openai.com/v1"`, `api_key_env = "OPENAI_API_KEY"`, `language = "English"`.
- TL;DR input caps: PDF first **12** pages, **30 MB** download cap, **60 s** download timeout, **40,000 chars** of extracted text.
- URLs: feed `https://rss.arxiv.org/atom/{cats joined with +}`, abs `https://arxiv.org/abs/{id}`, pdf `https://arxiv.org/pdf/{id}`. Stored arXiv ids are **versionless** (`v\d+` suffix stripped).
- `daily_runs.status` values are exactly `"ok"`, `"empty"`, `"failed"`.
- API behavior: `GET /api/daily` → latest batch with papers, else `{"date": null, "papers": []}`; `503` when the feature is off. `POST /api/daily/run` → `202` started / `409` in flight / `503` off. No auth (matches the rest of the API).
- Commit style: conventional commits with scope (`feat(daily): …`, `docs(deploy): …`). Commit implementation code frequently; do NOT commit `docs/superpowers/` plan/spec files.
- Never edit `migrations/0008_add_daily.sql` after it has been committed (sqlx checksums migrations).

---

### Task 1: Config — `[daily]` section

**Files:**
- Modify: `src/config.rs` (add `DailyConfig`, `DailyLlmConfig`, field on `Config`, defaults, tests)
- Modify: `xuewen.example.toml` (commented example block)

**Interfaces:**
- Consumes: nothing new.
- Produces: `Config.daily: Option<DailyConfig>`;
  `DailyConfig { categories: Vec<String>, include_cross_list: bool, max_papers: usize, run_at: String, retention_days: u32, llm: DailyLlmConfig }`;
  `DailyLlmConfig { base_url: String, model: String, api_key: Option<String>, api_key_env: String, language: String }`.
  All fields `pub`. Later tasks (5, 6, 7, 8) construct these literally in tests.

- [ ] **Step 1: Write the failing tests** — append to the `tests` module in `src/config.rs`:

```rust
    #[test]
    fn daily_defaults_to_none() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"
"#
        )
        .unwrap();
        assert!(Config::load(f.path()).unwrap().daily.is_none());
    }

    #[test]
    fn loads_daily_section_with_defaults() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"

[daily]
categories = ["cs.AI", "cs.LG"]

[daily.llm]
model = "gpt-4o-mini"
"#
        )
        .unwrap();
        let d = Config::load(f.path()).unwrap().daily.unwrap();
        assert_eq!(d.categories, vec!["cs.AI", "cs.LG"]);
        assert!(!d.include_cross_list);
        assert_eq!(d.max_papers, 20);
        assert_eq!(d.run_at, "09:00");
        assert_eq!(d.retention_days, 14);
        assert_eq!(d.llm.base_url, "https://api.openai.com/v1");
        assert_eq!(d.llm.model, "gpt-4o-mini");
        assert_eq!(d.llm.api_key, None);
        assert_eq!(d.llm.api_key_env, "OPENAI_API_KEY");
        assert_eq!(d.llm.language, "English");
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib config::tests::daily -- --nocapture`
Expected: compile error — `no field 'daily' on type 'Config'`.

- [ ] **Step 3: Implement.** In `src/config.rs`, add to `Config` (after the `search` field):

```rust
    /// Daily arXiv recommendations. Absent ⇒ the feature is off.
    #[serde(default)]
    pub daily: Option<DailyConfig>,
```

After the `EmbeddingConfig` block, add:

```rust
/// Daily arXiv recommendations (`[daily]`).
#[derive(Debug, Clone, Deserialize)]
pub struct DailyConfig {
    /// arXiv category codes, e.g. ["cs.AI", "cs.LG"].
    pub categories: Vec<String>,
    /// Also keep cross-listed announcements.
    #[serde(default)]
    pub include_cross_list: bool,
    /// Ranked papers kept per day.
    #[serde(default = "default_daily_max_papers")]
    pub max_papers: usize,
    /// Daily run time, UTC wall clock "HH:MM".
    #[serde(default = "default_daily_run_at")]
    pub run_at: String,
    /// Batches older than this many days are pruned.
    #[serde(default = "default_daily_retention_days")]
    pub retention_days: u32,
    pub llm: DailyLlmConfig,
}

/// Chat-completions API used for TL;DRs (`[daily.llm]`).
#[derive(Debug, Clone, Deserialize)]
pub struct DailyLlmConfig {
    #[serde(default = "default_embed_base_url")]
    pub base_url: String,
    pub model: String,
    /// Inline key; when absent the key is read from `api_key_env`.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    /// Language the TL;DRs are written in.
    #[serde(default = "default_daily_language")]
    pub language: String,
}

fn default_daily_max_papers() -> usize {
    20
}
fn default_daily_run_at() -> String {
    "09:00".to_string()
}
fn default_daily_retention_days() -> u32 {
    14
}
fn default_daily_language() -> String {
    "English".to_string()
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib config`
Expected: all config tests PASS.

- [ ] **Step 5: Update `xuewen.example.toml`** — append at the end:

```toml

# Daily arXiv recommendations, shown on a Glance dashboard via /api/daily
# (optional). Requires [search.embedding]: the library's title+abstract
# vectors are the interest profile the new papers are scored against.
#[daily]
#categories         = ["cs.AI", "cs.LG"]  # arXiv category codes (required)
#include_cross_list = false
#max_papers         = 20        # ranked papers kept per day
#run_at             = "09:00"   # daily run, UTC wall time
#retention_days     = 14

#[daily.llm]                    # chat-completions API for TL;DRs
#base_url    = "https://api.openai.com/v1"
#model       = "gpt-4o-mini"
#api_key_env = "OPENAI_API_KEY" # or: api_key = "sk-..."
#language    = "English"
```

- [ ] **Step 6: Commit**

```bash
git add src/config.rs xuewen.example.toml
git commit -m "feat(config): [daily] section for arXiv recommendations"
```

---

### Task 2: Migration + daily store

**Files:**
- Create: `migrations/0008_add_daily.sql`
- Create: `src/daily/mod.rs`, `src/daily/store.rs`
- Modify: `src/lib.rs` (register module)

**Interfaces:**
- Consumes: `crate::db::connect` (runs migrations), `papers` table (`arxiv_id`).
- Produces (all in `crate::daily::store`):
  - `struct DailyPaper { batch_date: String, rank: i64, arxiv_id: String, title: String, authors: Vec<String>, abstract_text: String, categories: Vec<String>, score: f64, tldr: Option<String>, abs_url: String, pdf_url: String }`
  - `struct DailyRun { batch_date: String, status: String, papers_found: i64, error: Option<String>, ran_at: String }`
  - `async fn record_run(pool: &SqlitePool, run: &DailyRun) -> Result<()>` (upsert by date)
  - `async fn get_run(pool: &SqlitePool, batch_date: &str) -> Result<Option<DailyRun>>`
  - `async fn replace_batch(pool: &SqlitePool, batch_date: &str, papers: &[DailyPaper]) -> Result<()>`
  - `async fn latest_batch(pool: &SqlitePool) -> Result<Option<(String, Vec<DailyPaper>)>>`
  - `async fn prune(pool: &SqlitePool, cutoff: &str) -> Result<()>` (deletes `batch_date < cutoff` in both tables)
  - `async fn library_arxiv_ids(pool: &SqlitePool) -> Result<HashSet<String>>` (includes trashed papers)

- [ ] **Step 1: Write the migration** — `migrations/0008_add_daily.sql`:

```sql
CREATE TABLE daily_runs (
  batch_date   TEXT PRIMARY KEY,  -- YYYY-MM-DD (UTC) of the run
  status       TEXT NOT NULL,     -- 'ok' | 'empty' | 'failed'
  papers_found INTEGER NOT NULL,  -- candidates after dedup, before top-N
  error        TEXT,              -- populated when status = 'failed'
  ran_at       TEXT NOT NULL
);

CREATE TABLE daily_papers (
  batch_date TEXT NOT NULL,
  rank       INTEGER NOT NULL,    -- 1-based, by descending score
  arxiv_id   TEXT NOT NULL,       -- versionless
  title      TEXT NOT NULL,
  authors    TEXT NOT NULL,       -- JSON array
  abstract   TEXT NOT NULL,
  categories TEXT NOT NULL,       -- JSON array
  score      REAL NOT NULL,
  tldr       TEXT,                -- NULL when generation failed
  abs_url    TEXT NOT NULL,
  pdf_url    TEXT NOT NULL,
  PRIMARY KEY (batch_date, rank)
);
```

- [ ] **Step 2: Register the module.** In `src/lib.rs`, add after `pub mod db;`:

```rust
pub mod daily;
```

Create `src/daily/mod.rs`:

```rust
pub mod store;
```

- [ ] **Step 3: Write failing tests** — create `src/daily/store.rs` with the types (so tests compile) and a `tests` module; leave function bodies `todo!()` OR write tests first against the full signatures below. Tests:

```rust
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
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test --lib daily::store`
Expected: compile error (functions/types missing) or `todo!()` panics.

- [ ] **Step 5: Implement `src/daily/store.rs`** (above the tests module):

```rust
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
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --lib daily::store`
Expected: 5 tests PASS.

- [ ] **Step 7: Commit**

```bash
git add migrations/0008_add_daily.sql src/lib.rs src/daily/
git commit -m "feat(daily): storage for daily runs and ranked batches"
```

---

### Task 3: arXiv announcement feed — `src/daily/feed.rs`

**Files:**
- Create: `src/daily/feed.rs`
- Modify: `src/daily/mod.rs` (add `pub mod feed;`)

**Interfaces:**
- Consumes: `crate::resolve::http::HttpClient` (`get_text` with retries).
- Produces (in `crate::daily::feed`):
  - `struct Candidate { arxiv_id: String, title: String, authors: Vec<String>, abstract_text: String, categories: Vec<String> }` (all `pub`, derives `Debug, Clone, PartialEq`)
  - `async fn fetch_feed(http: &HttpClient, feed_base: &str, categories: &[String]) -> Result<String>` — GETs `{feed_base}/{cat1+cat2}`
  - `fn parse_feed(xml: &str, include_cross_list: bool) -> Result<Vec<Candidate>>`

- [ ] **Step 1: Write failing tests.** Create `src/daily/feed.rs` containing only this test module (implementation comes in Step 3). Add `pub mod feed;` to `src/daily/mod.rs` (keep `pub mod` lines alphabetical).

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"
      xmlns:dc="http://purl.org/dc/elements/1.1/"
      xmlns:arxiv="http://arxiv.org/schemas/atom">
  <title>cs.AI updates on arXiv.org</title>
  <entry>
    <id>oai:arXiv.org:2507.00001v2</id>
    <title>Attention Is Still
      All You Need</title>
    <summary>arXiv:2507.00001v2 Announce Type: new
Abstract: We revisit attention
and find it sufficient.</summary>
    <dc:creator>Ada Lovelace, Alan Turing</dc:creator>
    <category term="cs.AI"/>
    <category term="cs.LG"/>
    <arxiv:announce_type>new</arxiv:announce_type>
  </entry>
  <entry>
    <id>oai:arXiv.org:2507.00002v1</id>
    <title>A Cross-Listed Paper</title>
    <summary>arXiv:2507.00002v1 Announce Type: cross
Abstract: Crossing over.</summary>
    <dc:creator>Grace Hopper</dc:creator>
    <category term="cs.AI"/>
    <arxiv:announce_type>cross</arxiv:announce_type>
  </entry>
  <entry>
    <id>oai:arXiv.org:2507.00003v3</id>
    <title>A Replaced Paper</title>
    <summary>arXiv:2507.00003v3 Announce Type: replace
Abstract: New version.</summary>
    <dc:creator>Nobody</dc:creator>
    <category term="cs.AI"/>
    <arxiv:announce_type>replace</arxiv:announce_type>
  </entry>
</feed>"#;

    #[test]
    fn keeps_new_only_by_default() {
        let out = parse_feed(FEED, false).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].arxiv_id, "2507.00001");
    }

    #[test]
    fn include_cross_list_keeps_cross_never_replace() {
        let out = parse_feed(FEED, true).unwrap();
        let ids: Vec<&str> = out.iter().map(|c| c.arxiv_id.as_str()).collect();
        assert_eq!(ids, vec!["2507.00001", "2507.00002"]);
    }

    #[test]
    fn extracts_fields_and_strips_noise() {
        let c = &parse_feed(FEED, false).unwrap()[0];
        assert_eq!(c.title, "Attention Is Still All You Need");
        assert_eq!(c.abstract_text, "We revisit attention and find it sufficient.");
        assert_eq!(c.authors, vec!["Ada Lovelace", "Alan Turing"]);
        assert_eq!(c.categories, vec!["cs.AI", "cs.LG"]);
    }

    #[test]
    fn feed_error_title_is_an_error() {
        let xml = r#"<?xml version="1.0"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>Feed error for query: nosuch.CAT</title>
</feed>"#;
        let err = parse_feed(xml, false).unwrap_err().to_string();
        assert!(err.contains("categories"), "got: {err}");
    }

    #[test]
    fn strip_version_handles_old_style_ids() {
        assert_eq!(strip_version("2507.00001v12"), "2507.00001");
        assert_eq!(strip_version("cs/0501001v2"), "cs/0501001");
        assert_eq!(strip_version("2507.00001"), "2507.00001");
    }

    #[tokio::test]
    async fn fetch_feed_joins_categories_with_plus() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FEED))
            .expect(1)
            .mount(&server)
            .await;
        let http = crate::resolve::http::HttpClient::new(
            reqwest::Client::new(),
            crate::resolve::http::RetryPolicy::fast_for_tests(),
        );
        let base = format!("{}/atom", server.uri());
        let xml = fetch_feed(&http, &base, &["cs.AI".into(), "cs.LG".into()])
            .await
            .unwrap();
        assert!(xml.contains("2507.00001"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib daily::feed`
Expected: compile error — `parse_feed` / `fetch_feed` / `Candidate` not found.

- [ ] **Step 3: Implement** — prepend to `src/daily/feed.rs`:

```rust
use anyhow::{bail, Result};

use crate::resolve::http::HttpClient;

/// A new arXiv paper parsed from the announcement feed.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// Versionless id, e.g. "2507.01234".
    pub arxiv_id: String,
    pub title: String,
    pub authors: Vec<String>,
    pub abstract_text: String,
    pub categories: Vec<String>,
}

/// GET the announcement feed for `categories`, joined with '+'
/// (rss.arxiv.org serves one combined feed for multiple categories).
pub async fn fetch_feed(
    http: &HttpClient,
    feed_base: &str,
    categories: &[String],
) -> Result<String> {
    let url = format!(
        "{}/{}",
        feed_base.trim_end_matches('/'),
        categories.join("+")
    );
    http.get_text(&url).await
}

fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// "2507.01234v2" -> "2507.01234"; ids without a version pass through.
fn strip_version(id: &str) -> String {
    match id.rfind('v') {
        Some(i) if i + 1 < id.len() && id[i + 1..].chars().all(|c| c.is_ascii_digit()) => {
            id[..i].to_string()
        }
        _ => id.to_string(),
    }
}

/// Parse the rss.arxiv.org Atom feed, keeping `new` announcements (plus
/// `cross` when `include_cross_list`); `replace*` is always dropped.
pub fn parse_feed(xml: &str, include_cross_list: bool) -> Result<Vec<Candidate>> {
    let doc = roxmltree::Document::parse(xml)?;
    let root = doc.root_element();
    let feed_title = root
        .children()
        .find(|n| n.tag_name().name() == "title")
        .and_then(|n| n.text())
        .unwrap_or("");
    if feed_title.contains("Feed error for query") {
        bail!("arXiv feed error — check [daily].categories: {feed_title}");
    }

    let mut out = Vec::new();
    for entry in root.children().filter(|n| n.tag_name().name() == "entry") {
        let child_text = |tag: &str| {
            entry
                .children()
                .find(|c| c.tag_name().name() == tag)
                .and_then(|n| n.text())
                .map(collapse_ws)
        };

        let announce = child_text("announce_type").unwrap_or_else(|| "new".into());
        let keep = announce == "new" || (include_cross_list && announce == "cross");
        if !keep {
            continue;
        }

        let Some(raw_id) = child_text("id") else { continue };
        let arxiv_id = strip_version(raw_id.trim_start_matches("oai:arXiv.org:"));
        let Some(title) = child_text("title") else { continue };

        // Summary looks like "arXiv:...v1 Announce Type: new Abstract: <text>".
        let raw_summary = child_text("summary").unwrap_or_default();
        let abstract_text = match raw_summary.find("Abstract:") {
            Some(i) => collapse_ws(&raw_summary[i + "Abstract:".len()..]),
            None => raw_summary,
        };

        // Authors: dc:creator ("A, B"), falling back to <author><name>.
        let mut authors: Vec<String> = entry
            .children()
            .filter(|c| c.tag_name().name() == "creator")
            .filter_map(|n| n.text())
            .flat_map(|t| t.split(", ").map(|s| s.trim().to_string()))
            .filter(|s| !s.is_empty())
            .collect();
        if authors.is_empty() {
            authors = entry
                .children()
                .filter(|c| c.tag_name().name() == "author")
                .filter_map(|a| {
                    a.children()
                        .find(|n| n.tag_name().name() == "name")
                        .and_then(|n| n.text())
                        .map(|s| s.trim().to_string())
                })
                .collect();
        }

        let categories: Vec<String> = entry
            .children()
            .filter(|c| c.tag_name().name() == "category")
            .filter_map(|c| c.attribute("term"))
            .map(String::from)
            .collect();

        out.push(Candidate {
            arxiv_id,
            title,
            authors,
            abstract_text,
            categories,
        });
    }
    Ok(out)
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib daily::feed`
Expected: 6 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/daily/feed.rs src/daily/mod.rs
git commit -m "feat(daily): arXiv announcement feed fetch and parse"
```

---

### Task 4: Qdrant scroll + interest-profile scoring — `src/daily/score.rs`

**Files:**
- Modify: `src/search/vector.rs` (add `scroll_summaries` + test)
- Create: `src/daily/score.rs`
- Modify: `src/daily/mod.rs` (add `pub mod score;`)

**Interfaces:**
- Consumes: `QdrantStore` internals (private `http`/`url` — the new method lives inside `vector.rs`), `papers` table (`added_at`, `deleted_at`).
- Produces:
  - `QdrantStore::scroll_summaries(&self) -> Result<Vec<(String, Vec<f32>)>>` — all seq-0 points as (paper_id, vector)
  - In `crate::daily::score`:
    - `fn recency_weights(n: usize) -> Vec<f32>` — normalized `1/(1+log10(i+1))`, newest-first
    - `fn l2_normalize(v: &mut [f32])`
    - `fn dot(a: &[f32], b: &[f32]) -> f32`
    - `async fn build_profile(pool: &SqlitePool, vectors: &QdrantStore) -> Result<Option<Vec<f32>>>` — `None` when no live paper has a summary vector

- [ ] **Step 1: Write the failing scroll test** — append to the `tests` module in `src/search/vector.rs`:

```rust
    #[tokio::test]
    async fn scroll_summaries_pages_until_offset_is_null() {
        let server = MockServer::start().await;
        // Page 2 (has "offset" in the body) — mount FIRST so it wins when it matches.
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/scroll"))
            .and(body_partial_json(json!({"offset": "cursor-1"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {
                    "points": [
                        {"id": "b", "payload": {"paper_id": "p2", "seq": 0}, "vector": [0.0, 1.0, 0.0, 0.0]}
                    ],
                    "next_page_offset": null
                }
            })))
            .expect(1)
            .mount(&server)
            .await;
        // Page 1: filters seq=0, requests vectors.
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/scroll"))
            .and(body_partial_json(json!({
                "filter": {"must": [{"key": "seq", "match": {"value": 0}}]},
                "with_vector": true
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {
                    "points": [
                        {"id": "a", "payload": {"paper_id": "p1", "seq": 0}, "vector": [1.0, 0.0, 0.0, 0.0]}
                    ],
                    "next_page_offset": "cursor-1"
                }
            })))
            .expect(1)
            .mount(&server)
            .await;

        let s = store(&server);
        let out = s.scroll_summaries().await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].0, "p1");
        assert_eq!(out[0].1, vec![1.0, 0.0, 0.0, 0.0]);
        assert_eq!(out[1].0, "p2");
    }
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib search::vector::tests::scroll`
Expected: compile error — no method `scroll_summaries`.

- [ ] **Step 3: Implement `scroll_summaries`** — add to `impl QdrantStore` in `src/search/vector.rs` (after `search`):

```rust
    /// All seq-0 (title+abstract) points as (paper_id, vector), paging
    /// through the scroll API. Feeds the daily-recommendation profile.
    pub async fn scroll_summaries(&self) -> Result<Vec<(String, Vec<f32>)>> {
        let mut out = Vec::new();
        let mut offset: Option<serde_json::Value> = None;
        loop {
            let mut body = json!({
                "filter": {"must": [{"key": "seq", "match": {"value": 0}}]},
                "with_payload": true,
                "with_vector": true,
                "limit": 256,
            });
            if let Some(o) = &offset {
                body["offset"] = o.clone();
            }
            let resp = self
                .http
                .post(self.url("/points/scroll"))
                .json(&body)
                .send()
                .await?;
            if !resp.status().is_success() {
                bail!("qdrant scroll: {}", resp.status());
            }
            let body: serde_json::Value = resp.json().await?;
            if let Some(points) = body["result"]["points"].as_array() {
                for p in points {
                    let Some(paper_id) = p["payload"]["paper_id"].as_str() else {
                        continue;
                    };
                    let Some(vec) = p["vector"].as_array() else { continue };
                    let v: Vec<f32> =
                        vec.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
                    out.push((paper_id.to_string(), v));
                }
            }
            offset = match &body["result"]["next_page_offset"] {
                serde_json::Value::Null => None,
                o => Some(o.clone()),
            };
            if offset.is_none() {
                break;
            }
        }
        Ok(out)
    }
```

Run: `cargo test --lib search::vector` — all PASS.

- [ ] **Step 4: Write failing score tests.** Create `src/daily/score.rs` with this test module; add `pub mod score;` to `src/daily/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn recency_weights_normalized_and_decreasing() {
        let w = recency_weights(3);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        assert!(w[0] > w[1] && w[1] > w[2]);
    }

    #[test]
    fn profile_score_equals_weighted_mean_of_cosines() {
        // Unit corpus vectors, newest first.
        let v1 = vec![1.0f32, 0.0];
        let v2 = vec![0.6f32, 0.8];
        let mut cand = vec![0.8f32, 0.6];
        l2_normalize(&mut cand);
        let w = recency_weights(2);
        let explicit = w[0] * dot(&cand, &v1) + w[1] * dot(&cand, &v2);

        let mut profile = vec![0.0f32; 2];
        for (v, wi) in [v1, v2].iter().zip(&w) {
            for (p, x) in profile.iter_mut().zip(v) {
                *p += wi * x;
            }
        }
        assert!((dot(&cand, &profile) - explicit).abs() < 1e-5);
    }

    async fn pool() -> sqlx::SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let p = crate::db::connect(&url).await.unwrap();
        std::mem::forget(dir);
        p
    }

    fn paper(id: &str, added_at: &str) -> crate::models::Paper {
        crate::models::Paper {
            id: id.into(),
            content_hash: format!("h-{id}"),
            rel_path: format!("{id}.pdf"),
            cite_key: None,
            added_at: added_at.into(),
            deleted_at: None,
            meta: crate::models::PaperMeta {
                title: Some("T".into()),
                abstract_text: None,
                authors: crate::models::Authors(vec![]),
                venue: None,
                year: None,
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: crate::models::PaperStatus::Resolved,
            },
        }
    }

    fn scroll_mock(points: serde_json::Value) -> Mock {
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/scroll"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"points": points, "next_page_offset": null}
            })))
    }

    #[tokio::test]
    async fn newer_library_paper_dominates_profile() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("new1", "2026-07-09T00:00:00Z"))
            .await
            .unwrap();
        crate::db::insert_paper(&pool, &paper("old1", "2026-01-01T00:00:00Z"))
            .await
            .unwrap();

        let server = MockServer::start().await;
        scroll_mock(json!([
            {"id": "a", "payload": {"paper_id": "new1", "seq": 0}, "vector": [1.0, 0.0, 0.0, 0.0]},
            {"id": "b", "payload": {"paper_id": "old1", "seq": 0}, "vector": [0.0, 1.0, 0.0, 0.0]}
        ]))
        .mount(&server)
        .await;
        let vectors = crate::search::vector::QdrantStore::new(&server.uri(), "xuewen", 4).unwrap();

        let profile = build_profile(&pool, &vectors).await.unwrap().unwrap();
        // Candidate matching the NEW paper must outrank one matching the OLD.
        let like_new = dot(&[1.0, 0.0, 0.0, 0.0], &profile);
        let like_old = dot(&[0.0, 1.0, 0.0, 0.0], &profile);
        assert!(like_new > like_old, "{like_new} vs {like_old}");
    }

    #[tokio::test]
    async fn empty_library_gives_no_profile() {
        let pool = pool().await;
        let server = MockServer::start().await;
        scroll_mock(json!([])).mount(&server).await;
        let vectors = crate::search::vector::QdrantStore::new(&server.uri(), "xuewen", 4).unwrap();
        assert!(build_profile(&pool, &vectors).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn trashed_papers_are_excluded_from_profile() {
        let pool = pool().await;
        crate::db::insert_paper(&pool, &paper("p1", "2026-07-09T00:00:00Z"))
            .await
            .unwrap();
        crate::db::soft_delete(&pool, "p1").await.unwrap();
        let server = MockServer::start().await;
        scroll_mock(json!([
            {"id": "a", "payload": {"paper_id": "p1", "seq": 0}, "vector": [1.0, 0.0, 0.0, 0.0]}
        ]))
        .mount(&server)
        .await;
        let vectors = crate::search::vector::QdrantStore::new(&server.uri(), "xuewen", 4).unwrap();
        assert!(build_profile(&pool, &vectors).await.unwrap().is_none());
    }
}
```

- [ ] **Step 5: Run to verify they fail**

Run: `cargo test --lib daily::score`
Expected: compile error — functions not found.

- [ ] **Step 6: Implement** — prepend to `src/daily/score.rs`:

```rust
use anyhow::Result;
use sqlx::SqlitePool;
use std::collections::HashMap;

use crate::search::vector::QdrantStore;

/// zotero-arxiv-daily's recency weights for `n` corpus papers ranked
/// newest-first: w_i = 1/(1+log10(i+1)), normalized to sum 1.
pub fn recency_weights(n: usize) -> Vec<f32> {
    let raw: Vec<f32> = (0..n)
        .map(|i| 1.0 / (1.0 + ((i + 1) as f32).log10()))
        .collect();
    let sum: f32 = raw.iter().sum();
    raw.into_iter().map(|w| w / sum).collect()
}

pub fn l2_normalize(v: &mut [f32]) {
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        for x in v.iter_mut() {
            *x /= norm;
        }
    }
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// Interest-profile vector: the recency-weighted sum of the library's
/// normalized seq-0 vectors. Scoring a candidate against it with `dot`
/// equals the weighted mean cosine similarity over the whole library.
/// `None` when no live paper has an indexed summary vector.
pub async fn build_profile(pool: &SqlitePool, vectors: &QdrantStore) -> Result<Option<Vec<f32>>> {
    let points = vectors.scroll_summaries().await?;
    let mut by_id: HashMap<String, Vec<f32>> = points.into_iter().collect();

    let ids: Vec<(String,)> = sqlx::query_as(
        "SELECT id FROM papers WHERE deleted_at IS NULL ORDER BY added_at DESC, id",
    )
    .fetch_all(pool)
    .await?;

    // Newest-first vectors for live papers that are actually indexed.
    let mut ranked: Vec<Vec<f32>> = Vec::new();
    for (id,) in ids {
        if let Some(mut v) = by_id.remove(&id) {
            l2_normalize(&mut v);
            ranked.push(v);
        }
    }
    if ranked.is_empty() {
        return Ok(None);
    }

    let weights = recency_weights(ranked.len());
    let mut profile = vec![0.0f32; ranked[0].len()];
    for (v, w) in ranked.iter().zip(&weights) {
        for (p, x) in profile.iter_mut().zip(v) {
            *p += w * x;
        }
    }
    Ok(Some(profile))
}
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test --lib daily::score`
Expected: 5 tests PASS.

- [ ] **Step 8: Commit**

```bash
git add src/search/vector.rs src/daily/score.rs src/daily/mod.rs
git commit -m "feat(daily): interest-profile scoring from library vectors"
```

---

### Task 5: Chat client + TL;DR fallback chain — `src/daily/tldr.rs`

**Files:**
- Create: `src/daily/tldr.rs`
- Modify: `src/daily/mod.rs` (add `pub mod tldr;`)

**Interfaces:**
- Consumes: `crate::config::DailyLlmConfig` (Task 1).
- Produces (in `crate::daily::tldr`):
  - `const FULL_TEXT_CAP: usize = 40_000` (chars of extracted PDF text in the prompt; Task 6 uses it)
  - `struct ChatClient` with `from_config(cfg: &DailyLlmConfig) -> Option<Self>` (None + warn when no key, mirrors `Embedder::from_config`), `for_tests(base_url: &str, model: &str) -> Self`, `async fn complete(&self, system: &str, user: &str) -> Result<String>`
  - `async fn generate_tldr(chat: &ChatClient, language: &str, title: &str, abstract_text: &str, full_text: Option<&str>) -> Option<String>` — full-text prompt → abstract-only → `None`

- [ ] **Step 1: Write failing tests.** Create `src/daily/tldr.rs` with this test module; add `pub mod tldr;` to `src/daily/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn chat_response(text: &str) -> serde_json::Value {
        json!({"choices": [{"message": {"role": "assistant", "content": text}}]})
    }

    #[tokio::test]
    async fn complete_sends_model_messages_and_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer sk-test"))
            .and(body_partial_json(json!({"model": "gpt-4o-mini"})))
            .and(body_string_contains("hello user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_response("  hi  ")))
            .expect(1)
            .mount(&server)
            .await;

        let cfg = crate::config::DailyLlmConfig {
            base_url: format!("{}/v1", server.uri()),
            model: "gpt-4o-mini".into(),
            api_key: Some("sk-test".into()),
            api_key_env: "UNSET_VAR_FOR_TEST".into(),
            language: "English".into(),
        };
        let c = ChatClient::from_config(&cfg).unwrap();
        assert_eq!(c.complete("sys", "hello user").await.unwrap(), "hi");
    }

    #[tokio::test]
    async fn complete_retries_429_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_response("ok")))
            .expect(1)
            .mount(&server)
            .await;
        let c = ChatClient::for_tests(&format!("{}/v1", server.uri()), "m");
        assert_eq!(c.complete("s", "u").await.unwrap(), "ok");
    }

    #[tokio::test]
    async fn complete_does_not_retry_400() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .expect(1)
            .mount(&server)
            .await;
        let c = ChatClient::for_tests(&format!("{}/v1", server.uri()), "m");
        assert!(c.complete("s", "u").await.is_err());
    }

    #[tokio::test]
    async fn tldr_falls_back_from_full_text_to_abstract() {
        let server = MockServer::start().await;
        // Full-text prompts fail non-retriably…
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("Preview of main content"))
            .respond_with(ResponseTemplate::new(400))
            .expect(1)
            .mount(&server)
            .await;
        // …the abstract-only prompt succeeds.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_response("Short TLDR.")))
            .expect(1)
            .mount(&server)
            .await;
        let c = ChatClient::for_tests(&format!("{}/v1", server.uri()), "m");
        let out = generate_tldr(&c, "English", "Title", "An abstract.", Some("full text")).await;
        assert_eq!(out.as_deref(), Some("Short TLDR."));
    }

    #[tokio::test]
    async fn tldr_gives_none_when_all_prompts_fail() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400))
            .expect(2) // full-text, then abstract-only
            .mount(&server)
            .await;
        let c = ChatClient::for_tests(&format!("{}/v1", server.uri()), "m");
        let out = generate_tldr(&c, "English", "Title", "An abstract.", Some("full text")).await;
        assert!(out.is_none());
    }

    #[test]
    fn from_config_without_key_is_none() {
        let cfg = crate::config::DailyLlmConfig {
            base_url: "https://api.openai.com/v1".into(),
            model: "m".into(),
            api_key: None,
            api_key_env: "XUEWEN_TEST_KEY_THAT_IS_NOT_SET".into(),
            language: "English".into(),
        };
        assert!(ChatClient::from_config(&cfg).is_none());
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib daily::tldr`
Expected: compile error — `ChatClient` not found.

- [ ] **Step 3: Implement** — prepend to `src/daily/tldr.rs`:

```rust
use anyhow::{anyhow, Result};
use std::time::Duration;

use crate::config::DailyLlmConfig;

const ATTEMPTS: u32 = 3;
/// Chars of extracted PDF text included in the full-text prompt.
pub const FULL_TEXT_CAP: usize = 40_000;

const SYSTEM: &str =
    "You summarize scientific papers accurately and concisely for a researcher's daily feed.";

/// Minimal OpenAI-compatible /chat/completions client. Retry behavior
/// mirrors `search::embedder::Embedder` (429/5xx/network, backoff).
pub struct ChatClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl ChatClient {
    /// `None` when no API key is resolvable — the daily feature is then
    /// disabled, but nothing fails.
    pub fn from_config(cfg: &DailyLlmConfig) -> Option<Self> {
        let key = cfg
            .api_key
            .clone()
            .or_else(|| std::env::var(&cfg.api_key_env).ok())
            .filter(|k| !k.trim().is_empty());
        let Some(key) = key else {
            tracing::warn!(
                "[daily.llm] configured but no API key (set api_key or ${}) — daily papers disabled",
                cfg.api_key_env
            );
            return None;
        };
        Some(Self {
            http: reqwest::Client::new(),
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            model: cfg.model.clone(),
            api_key: Some(key),
        })
    }

    /// Keyless client pointed at a mock server. Test support only.
    pub fn for_tests(base_url: &str, model: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: None,
        }
    }

    pub async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        });
        let mut delay = Duration::from_millis(500);
        let mut last_err = None;
        for attempt in 1..=ATTEMPTS {
            let mut req = self.http.post(&url).json(&body);
            if let Some(k) = &self.api_key {
                req = req.bearer_auth(k);
            }
            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    let v: serde_json::Value = resp.json().await?;
                    let text = v["choices"][0]["message"]["content"]
                        .as_str()
                        .ok_or_else(|| anyhow!("chat API response has no message content"))?;
                    return Ok(text.trim().to_string());
                }
                Ok(resp) => {
                    let status = resp.status();
                    let retriable = status.as_u16() == 429 || status.is_server_error();
                    let text = resp.text().await.unwrap_or_default();
                    let err = anyhow!(
                        "chat API {status}: {}",
                        text.chars().take(200).collect::<String>()
                    );
                    if !retriable || attempt == ATTEMPTS {
                        return Err(err);
                    }
                    last_err = Some(err);
                }
                Err(e) => {
                    if attempt == ATTEMPTS {
                        return Err(e.into());
                    }
                    last_err = Some(e.into());
                }
            }
            tokio::time::sleep(delay).await;
            delay *= 2;
        }
        Err(last_err.expect("loop ran at least once"))
    }
}

fn prompt(language: &str, title: &str, abstract_text: &str, full_text: Option<&str>) -> String {
    let mut p = format!(
        "Given the following information about a paper, write a 2-3 sentence TL;DR in \
         {language}: the problem, the approach, and the key result. Output only the TL;DR.\n\n\
         Title: {title}\n\nAbstract: {abstract_text}\n"
    );
    if let Some(t) = full_text {
        let capped: String = t.chars().take(FULL_TEXT_CAP).collect();
        p.push_str("\nPreview of main content:\n");
        p.push_str(&capped);
        p.push('\n');
    }
    p
}

/// Best-effort TL;DR: full-text prompt, then abstract-only, then `None`.
/// Never propagates an error — a bad paper must not fail the batch.
pub async fn generate_tldr(
    chat: &ChatClient,
    language: &str,
    title: &str,
    abstract_text: &str,
    full_text: Option<&str>,
) -> Option<String> {
    if full_text.is_some() {
        match chat
            .complete(SYSTEM, &prompt(language, title, abstract_text, full_text))
            .await
        {
            Ok(t) => return Some(t),
            Err(e) => tracing::warn!("full-text TL;DR failed for {title}: {e}"),
        }
    }
    match chat
        .complete(SYSTEM, &prompt(language, title, abstract_text, None))
        .await
    {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!("abstract TL;DR failed for {title}: {e}");
            None
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib daily::tldr`
Expected: 6 tests PASS (the 429-retry test sleeps ~500 ms once; that matches the embedder's existing test).

- [ ] **Step 5: Commit**

```bash
git add src/daily/tldr.rs src/daily/mod.rs
git commit -m "feat(daily): chat client and TL;DR fallback chain"
```

---

### Task 6: `DailyService` + job orchestration — `src/daily/job.rs`

**Files:**
- Modify: `src/daily/mod.rs` (DailyService, constants, run guard; add `pub mod job;`)
- Create: `src/daily/job.rs`

**Interfaces:**
- Consumes: everything from Tasks 1–5, plus `crate::pdf::extract_text`, `crate::resolve::http::{HttpClient, RetryPolicy}`, `crate::search::embedder::Embedder`, `crate::search::vector::QdrantStore`.
- Produces (in `crate::daily`):
  - `const ARXIV_FEED_BASE: &str = "https://rss.arxiv.org/atom"`, `ARXIV_PDF_BASE: &str = "https://arxiv.org/pdf"`, `ARXIV_ABS_BASE: &str = "https://arxiv.org/abs"`
  - `struct DailyService` with:
    - `from_config(cfg: &Config, pool: SqlitePool) -> anyhow::Result<Option<Arc<Self>>>` — `Ok(None)` + warn when off (no `[daily]`, no `[search.embedding]`, or a missing API key); `Err` on empty `categories` (Task 8 adds `run_at` validation here)
    - `for_tests(cfg: DailyConfig, pool, embedder: Embedder, vectors: QdrantStore, chat: tldr::ChatClient, feed_base: &str, pdf_base: &str) -> Arc<Self>`
    - `is_running(&self) -> bool`
    - `async fn run_guarded(&self, batch_date: &str) -> Option<store::DailyRun>` — `None` if in flight
    - `fn spawn_run(self: &Arc<Self>, batch_date: String) -> bool` — guard taken synchronously, run on a background task; `false` if in flight
    - public fields used by tasks 7/8: `cfg: DailyConfig`, `pool: SqlitePool`
  - `job::run_once(svc: &DailyService, batch_date: &str) -> store::DailyRun` — never errors; records the run row and prunes

- [ ] **Step 1: Extend `src/daily/mod.rs`** (replace its current whole content):

```rust
pub mod feed;
pub mod job;
pub mod score;
pub mod store;
pub mod tldr;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use sqlx::SqlitePool;

use crate::config::{Config, DailyConfig};
use crate::resolve::http::{HttpClient, RetryPolicy};
use crate::search::embedder::Embedder;
use crate::search::vector::QdrantStore;

pub const ARXIV_FEED_BASE: &str = "https://rss.arxiv.org/atom";
pub const ARXIV_PDF_BASE: &str = "https://arxiv.org/pdf";
pub const ARXIV_ABS_BASE: &str = "https://arxiv.org/abs";

/// Daily arXiv recommendations. Owns its own HTTP clients (all stateless)
/// so it stays independent of `SearchService`.
pub struct DailyService {
    pub cfg: DailyConfig,
    pub pool: SqlitePool,
    /// Feed fetches: retried like the resolvers.
    pub(crate) http: HttpClient,
    /// PDF downloads (bytes; single attempt — the TL;DR chain absorbs failures).
    pub(crate) plain_http: reqwest::Client,
    pub(crate) embedder: Embedder,
    pub(crate) vectors: QdrantStore,
    pub(crate) chat: tldr::ChatClient,
    pub(crate) feed_base: String,
    pub(crate) pdf_base: String,
    running: AtomicBool,
}

impl DailyService {
    /// `Ok(None)` when the feature is off: no `[daily]` section, no
    /// `[search.embedding]`, or a missing API key (each case warns).
    /// `Err` only on invalid `[daily]` values.
    pub fn from_config(cfg: &Config, pool: SqlitePool) -> anyhow::Result<Option<Arc<Self>>> {
        let Some(daily) = &cfg.daily else { return Ok(None) };
        if daily.categories.is_empty() {
            anyhow::bail!("[daily].categories must not be empty");
        }
        let Some(embed_cfg) = &cfg.search.embedding else {
            tracing::warn!("[daily] set but [search.embedding] missing — daily papers disabled");
            return Ok(None);
        };
        let Some(embedder) = Embedder::from_config(embed_cfg) else {
            return Ok(None); // warned inside
        };
        let Some(chat) = tldr::ChatClient::from_config(&daily.llm) else {
            return Ok(None); // warned inside
        };
        let vectors = QdrantStore::new(
            &cfg.search.qdrant_url,
            &cfg.search.qdrant_collection,
            embed_cfg.dims,
        )?;
        Ok(Some(Arc::new(Self {
            cfg: daily.clone(),
            pool,
            http: HttpClient::new(reqwest::Client::new(), RetryPolicy::production()),
            plain_http: reqwest::Client::new(),
            embedder,
            vectors,
            chat,
            feed_base: ARXIV_FEED_BASE.to_string(),
            pdf_base: ARXIV_PDF_BASE.to_string(),
            running: AtomicBool::new(false),
        })))
    }

    /// DI constructor: every remote endpoint is overridable. Test support only.
    pub fn for_tests(
        cfg: DailyConfig,
        pool: SqlitePool,
        embedder: Embedder,
        vectors: QdrantStore,
        chat: tldr::ChatClient,
        feed_base: &str,
        pdf_base: &str,
    ) -> Arc<Self> {
        Arc::new(Self {
            cfg,
            pool,
            http: HttpClient::new(reqwest::Client::new(), RetryPolicy::fast_for_tests()),
            plain_http: reqwest::Client::new(),
            embedder,
            vectors,
            chat,
            feed_base: feed_base.trim_end_matches('/').to_string(),
            pdf_base: pdf_base.trim_end_matches('/').to_string(),
            running: AtomicBool::new(false),
        })
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    fn try_begin(&self) -> bool {
        self.running
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
    }

    /// Run for `batch_date` unless a run is already in flight (then `None`).
    pub async fn run_guarded(&self, batch_date: &str) -> Option<store::DailyRun> {
        if !self.try_begin() {
            return None;
        }
        let run = job::run_once(self, batch_date).await;
        self.running.store(false, Ordering::SeqCst);
        Some(run)
    }

    /// Guarded run on a background task; `false` if one was in flight.
    /// The guard is taken synchronously, so a caller seeing `true` knows
    /// the very next `spawn_run`/`run_guarded` will refuse.
    pub fn spawn_run(self: &Arc<Self>, batch_date: String) -> bool {
        if !self.try_begin() {
            return false;
        }
        let svc = self.clone();
        tokio::spawn(async move {
            let run = job::run_once(&svc, &batch_date).await;
            tracing::info!(
                "daily run {}: {} ({} candidates)",
                run.batch_date,
                run.status,
                run.papers_found
            );
            svc.running.store(false, Ordering::SeqCst);
        });
        true
    }
}
```

- [ ] **Step 2: Write failing job tests.** Create `src/daily/job.rs` with this test module:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DailyConfig, DailyLlmConfig};
    use crate::daily::{store, tldr::ChatClient, DailyService};
    use crate::search::{embedder::Embedder, vector::QdrantStore};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Library paper 2401.00001 is deduped; candidates A (2507.0000**2**,
    // orthogonal to the profile) and B (2507.0000**3**, parallel) get ranked.
    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"
      xmlns:dc="http://purl.org/dc/elements/1.1/"
      xmlns:arxiv="http://arxiv.org/schemas/atom">
  <title>cs.AI updates on arXiv.org</title>
  <entry>
    <id>oai:arXiv.org:2401.00001v1</id>
    <title>Already In The Library</title>
    <summary>arXiv:2401.00001v1 Announce Type: new
Abstract: Old news.</summary>
    <dc:creator>Lib Author</dc:creator>
    <category term="cs.AI"/>
    <arxiv:announce_type>new</arxiv:announce_type>
  </entry>
  <entry>
    <id>oai:arXiv.org:2507.00002v1</id>
    <title>Candidate A</title>
    <summary>arXiv:2507.00002v1 Announce Type: new
Abstract: Unrelated to the library.</summary>
    <dc:creator>Alice</dc:creator>
    <category term="cs.AI"/>
    <arxiv:announce_type>new</arxiv:announce_type>
  </entry>
  <entry>
    <id>oai:arXiv.org:2507.00003v1</id>
    <title>Candidate B</title>
    <summary>arXiv:2507.00003v1 Announce Type: new
Abstract: Very similar to the library.</summary>
    <dc:creator>Bob</dc:creator>
    <category term="cs.AI"/>
    <arxiv:announce_type>new</arxiv:announce_type>
  </entry>
</feed>"#;

    const EMPTY_FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>cs.AI updates on arXiv.org</title>
</feed>"#;

    async fn pool_with_library_paper() -> sqlx::SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let pool = crate::db::connect(&url).await.unwrap();
        std::mem::forget(dir);
        let p = crate::models::Paper {
            id: "lib1".into(),
            content_hash: "h".into(),
            rel_path: "lib1.pdf".into(),
            cite_key: None,
            added_at: "2026-07-01T00:00:00Z".into(),
            deleted_at: None,
            meta: crate::models::PaperMeta {
                title: Some("Library Paper".into()),
                abstract_text: Some("lib abstract".into()),
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
        pool
    }

    fn cfg() -> DailyConfig {
        DailyConfig {
            categories: vec!["cs.AI".into(), "cs.LG".into()],
            include_cross_list: false,
            max_papers: 20,
            run_at: "09:00".into(),
            retention_days: 14,
            llm: DailyLlmConfig {
                base_url: "unused".into(),
                model: "m".into(),
                api_key: None,
                api_key_env: "UNSET".into(),
                language: "English".into(),
            },
        }
    }

    fn service(server: &MockServer, pool: sqlx::SqlitePool) -> std::sync::Arc<DailyService> {
        DailyService::for_tests(
            cfg(),
            pool,
            Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4),
            QdrantStore::new(&server.uri(), "xuewen", 4).unwrap(),
            ChatClient::for_tests(&format!("{}/v1", server.uri()), "m"),
            &format!("{}/atom", server.uri()),
            &format!("{}/pdf", server.uri()),
        )
    }

    async fn mount_scroll(server: &MockServer, points: serde_json::Value) {
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/scroll"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"points": points, "next_page_offset": null}
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn full_run_dedupes_ranks_and_stores() {
        let pool = pool_with_library_paper().await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FEED))
            .mount(&server)
            .await;
        mount_scroll(
            &server,
            json!([{"id": "x", "payload": {"paper_id": "lib1", "seq": 0},
                    "vector": [1.0, 0.0, 0.0, 0.0]}]),
        )
        .await;
        // Candidate order in the feed: A then B. A is orthogonal, B parallel.
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {"index": 0, "embedding": [0.0, 1.0, 0.0, 0.0]},
                    {"index": 1, "embedding": [1.0, 0.0, 0.0, 0.0]}
                ]
            })))
            .mount(&server)
            .await;
        // PDFs 404 -> TL;DR falls back to abstract-only, which succeeds.
        Mock::given(method("GET"))
            .and(wiremock::matchers::path_regex("^/pdf/.*"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"role": "assistant", "content": "A TLDR."}}]
            })))
            .mount(&server)
            .await;

        let svc = service(&server, pool.clone());
        let run = run_once(&svc, "2026-07-10").await;

        assert_eq!(run.status, "ok");
        assert_eq!(run.papers_found, 2, "library paper must be deduped");
        let (date, papers) = store::latest_batch(&pool).await.unwrap().unwrap();
        assert_eq!(date, "2026-07-10");
        assert_eq!(papers.len(), 2);
        assert_eq!(papers[0].arxiv_id, "2507.00003", "parallel candidate ranks first");
        assert_eq!(papers[0].rank, 1);
        assert!(papers[0].score > papers[1].score);
        assert_eq!(papers[0].tldr.as_deref(), Some("A TLDR."));
        assert_eq!(papers[0].abs_url, "https://arxiv.org/abs/2507.00003");
        assert_eq!(papers[0].pdf_url, "https://arxiv.org/pdf/2507.00003");
        let recorded = store::get_run(&pool, "2026-07-10").await.unwrap().unwrap();
        assert_eq!(recorded.status, "ok");
    }

    #[tokio::test]
    async fn empty_feed_records_empty_run() {
        let pool = pool_with_library_paper().await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(ResponseTemplate::new(200).set_body_string(EMPTY_FEED))
            .mount(&server)
            .await;
        let svc = service(&server, pool.clone());
        let run = run_once(&svc, "2026-07-10").await;
        assert_eq!(run.status, "empty");
        assert_eq!(run.papers_found, 0);
        assert!(store::latest_batch(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn feed_failure_records_failed_run() {
        let pool = pool_with_library_paper().await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let svc = service(&server, pool.clone());
        let run = run_once(&svc, "2026-07-10").await;
        assert_eq!(run.status, "failed");
        assert!(run.error.is_some());
    }

    #[tokio::test]
    async fn missing_library_vectors_fail_with_clear_error() {
        let pool = pool_with_library_paper().await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FEED))
            .mount(&server)
            .await;
        mount_scroll(&server, json!([])).await;
        let svc = service(&server, pool.clone());
        let run = run_once(&svc, "2026-07-10").await;
        assert_eq!(run.status, "failed");
        assert!(
            run.error.unwrap().contains("no indexed library papers"),
            "error should tell the user to build the index"
        );
    }

    #[tokio::test]
    async fn run_guarded_refuses_concurrent_runs() {
        let pool = pool_with_library_paper().await;
        let server = MockServer::start().await;
        // Slow feed keeps the first run in flight.
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(EMPTY_FEED)
                    .set_delay(std::time::Duration::from_millis(500)),
            )
            .mount(&server)
            .await;
        let svc = service(&server, pool);
        assert!(svc.spawn_run("2026-07-10".into()));
        assert!(svc.is_running());
        assert!(svc.run_guarded("2026-07-10").await.is_none());
    }
}
```

- [ ] **Step 3: Run to verify failure**

Run: `cargo test --lib daily::job`
Expected: compile error — `run_once` not found.

- [ ] **Step 4: Implement** — prepend to `src/daily/job.rs`:

```rust
use anyhow::{bail, Context, Result};
use chrono::Utc;

use super::{feed, score, store, tldr, DailyService, ARXIV_ABS_BASE, ARXIV_PDF_BASE};

/// Pages of the PDF fed to the TL;DR prompt.
const TLDR_PDF_PAGES: u32 = 12;
const PDF_MAX_BYTES: usize = 30 * 1024 * 1024;
const PDF_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// One full daily run. Never fails: the outcome (ok/empty/failed) is
/// recorded in `daily_runs` and returned. Old batches are pruned after.
pub async fn run_once(svc: &DailyService, batch_date: &str) -> store::DailyRun {
    let (status, found, error) = match pipeline(svc, batch_date).await {
        Ok(0) => ("empty", 0, None),
        Ok(found) => ("ok", found, None),
        Err(e) => {
            tracing::error!("daily run {batch_date} failed: {e:#}");
            ("failed", 0, Some(format!("{e:#}")))
        }
    };
    let run = store::DailyRun {
        batch_date: batch_date.to_string(),
        status: status.to_string(),
        papers_found: found,
        error,
        ran_at: Utc::now().to_rfc3339(),
    };
    if let Err(e) = store::record_run(&svc.pool, &run).await {
        tracing::error!("recording daily run {batch_date}: {e:#}");
    }
    if let Err(e) = prune_old(svc, batch_date).await {
        tracing::warn!("pruning old daily batches: {e:#}");
    }
    run
}

/// Fetch → dedup → score → TL;DR → store. Returns the candidate count
/// after dedup (0 ⇒ the caller records an "empty" run).
async fn pipeline(svc: &DailyService, batch_date: &str) -> Result<i64> {
    let xml = feed::fetch_feed(&svc.http, &svc.feed_base, &svc.cfg.categories)
        .await
        .context("fetching arXiv feed")?;
    let mut candidates = feed::parse_feed(&xml, svc.cfg.include_cross_list)?;

    let known = store::library_arxiv_ids(&svc.pool).await?;
    candidates.retain(|c| !known.contains(&c.arxiv_id));
    let found = candidates.len() as i64;
    if candidates.is_empty() {
        return Ok(0);
    }

    let Some(profile) = score::build_profile(&svc.pool, &svc.vectors).await? else {
        bail!(
            "no indexed library papers — let `xuewen serve` finish indexing \
             or run `xuewen index rebuild` first"
        );
    };

    let texts: Vec<String> = candidates
        .iter()
        .map(|c| format!("{}\n{}", c.title, c.abstract_text))
        .collect();
    let embeddings = svc
        .embedder
        .embed(&texts)
        .await
        .context("embedding candidates")?;

    let mut scored: Vec<(f32, feed::Candidate)> = candidates
        .into_iter()
        .zip(embeddings)
        .map(|(c, mut v)| {
            score::l2_normalize(&mut v);
            (score::dot(&v, &profile), c)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(svc.cfg.max_papers);

    let mut rows = Vec::with_capacity(scored.len());
    for (i, (s, c)) in scored.into_iter().enumerate() {
        let full_text = match fetch_pdf_text(svc, &c.arxiv_id).await {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::warn!("PDF text for {}: {e:#}", c.arxiv_id);
                None
            }
        };
        let tldr = tldr::generate_tldr(
            &svc.chat,
            &svc.cfg.llm.language,
            &c.title,
            &c.abstract_text,
            full_text.as_deref(),
        )
        .await;
        rows.push(store::DailyPaper {
            batch_date: batch_date.to_string(),
            rank: i as i64 + 1,
            arxiv_id: c.arxiv_id.clone(),
            title: c.title,
            authors: c.authors,
            abstract_text: c.abstract_text,
            categories: c.categories,
            score: s as f64,
            tldr,
            abs_url: format!("{ARXIV_ABS_BASE}/{}", c.arxiv_id),
            pdf_url: format!("{ARXIV_PDF_BASE}/{}", c.arxiv_id),
        });
    }
    store::replace_batch(&svc.pool, batch_date, &rows).await?;
    Ok(found)
}

/// Download the paper's PDF and return the text of its first pages,
/// capped for the prompt. Any failure here is per-paper and non-fatal.
async fn fetch_pdf_text(svc: &DailyService, arxiv_id: &str) -> Result<String> {
    let url = format!("{}/{}", svc.pdf_base, arxiv_id);
    let resp = svc.plain_http.get(&url).timeout(PDF_TIMEOUT).send().await?;
    if !resp.status().is_success() {
        bail!("PDF download {url}: {}", resp.status());
    }
    let bytes = resp.bytes().await?;
    if bytes.len() > PDF_MAX_BYTES {
        bail!("PDF too large: {} bytes", bytes.len());
    }
    let path = std::env::temp_dir().join(format!("xuewen-daily-{}.pdf", uuid::Uuid::now_v7()));
    let result = (|| -> Result<String> {
        std::fs::write(&path, &bytes)?;
        let text = crate::pdf::extract_text(&path, TLDR_PDF_PAGES)?;
        Ok(text.chars().take(tldr::FULL_TEXT_CAP).collect())
    })();
    let _ = std::fs::remove_file(&path);
    result
}

async fn prune_old(svc: &DailyService, batch_date: &str) -> Result<()> {
    let date = chrono::NaiveDate::parse_from_str(batch_date, "%Y-%m-%d")?;
    let cutoff = date
        .checked_sub_days(chrono::Days::new(svc.cfg.retention_days as u64))
        .unwrap_or(date);
    store::prune(&svc.pool, &cutoff.format("%Y-%m-%d").to_string()).await
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib daily`
Expected: all `daily::` tests PASS (job tests: 5).

- [ ] **Step 6: Commit**

```bash
git add src/daily/mod.rs src/daily/job.rs
git commit -m "feat(daily): daily job orchestration and DailyService"
```

---

### Task 7: Web API — `GET /api/daily`, `POST /api/daily/run`

**Files:**
- Modify: `src/web/mod.rs` (AppState field, all router builders, `serve` param, new routes, `build_router_with_daily`)
- Modify: `src/web/api.rs` (two handlers)
- Modify: `src/web/dto.rs` (`DailyPaperDto`, `DailyResponse`)
- Modify: `src/main.rs` (pass `None` for the new `serve` param — real wiring is Task 8)
- Create: `tests/web_daily_test.rs`

**Interfaces:**
- Consumes: `DailyService` (`spawn_run`, `is_running`), `daily::store::latest_batch` (Tasks 2, 6).
- Produces:
  - `AppState.daily: Option<Arc<crate::daily::DailyService>>`
  - `pub fn build_router_with_daily(pool: SqlitePool, library_root: PathBuf, daily: Arc<crate::daily::DailyService>) -> Router`
  - `web::serve(...)` gains a trailing `daily: Option<Arc<crate::daily::DailyService>>` parameter
  - JSON: `DailyResponse { date: Option<String>, papers: Vec<DailyPaperDto> }`; `DailyPaperDto { rank, arxiv_id, title, authors, abstract (renamed), categories, score, tldr, abs_url, pdf_url }`

- [ ] **Step 1: Write failing integration tests** — create `tests/web_daily_test.rs`:

```rust
use axum_test::TestServer;
use serde_json::Value;
use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::config::{DailyConfig, DailyLlmConfig};
use xuewen::daily::{store, tldr::ChatClient, DailyService};
use xuewen::db;
use xuewen::search::{embedder::Embedder, vector::QdrantStore};
use xuewen::web::{build_router, build_router_with_daily};

async fn temp_pool() -> (tempfile::TempDir, sqlx::SqlitePool) {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    (dir, pool)
}

fn daily_cfg() -> DailyConfig {
    DailyConfig {
        categories: vec!["cs.AI".into()],
        include_cross_list: false,
        max_papers: 20,
        run_at: "09:00".into(),
        retention_days: 14,
        llm: DailyLlmConfig {
            base_url: "http://127.0.0.1:1/v1".into(),
            model: "m".into(),
            api_key: None,
            api_key_env: "UNSET".into(),
            language: "English".into(),
        },
    }
}

/// A DailyService whose remote endpoints are all dead — fine for GET tests,
/// which never call out.
fn dead_service(pool: sqlx::SqlitePool) -> std::sync::Arc<DailyService> {
    DailyService::for_tests(
        daily_cfg(),
        pool,
        Embedder::for_tests("http://127.0.0.1:1/v1", "m", 4),
        QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap(),
        ChatClient::for_tests("http://127.0.0.1:1/v1", "m"),
        "http://127.0.0.1:1/atom",
        "http://127.0.0.1:1/pdf",
    )
}

fn batch_paper(date: &str, rank: i64, id: &str, tldr: Option<&str>) -> store::DailyPaper {
    store::DailyPaper {
        batch_date: date.into(),
        rank,
        arxiv_id: id.into(),
        title: format!("Paper {id}"),
        authors: vec!["Ada".into()],
        abstract_text: "An abstract.".into(),
        categories: vec!["cs.AI".into()],
        score: 0.9,
        tldr: tldr.map(String::from),
        abs_url: format!("https://arxiv.org/abs/{id}"),
        pdf_url: format!("https://arxiv.org/pdf/{id}"),
    }
}

#[tokio::test]
async fn get_daily_returns_latest_batch() {
    let (dir, pool) = temp_pool().await;
    store::replace_batch(&pool, "2026-07-09", &[batch_paper("2026-07-09", 1, "2507.1", None)])
        .await
        .unwrap();
    store::replace_batch(
        &pool,
        "2026-07-10",
        &[
            batch_paper("2026-07-10", 1, "2507.2", Some("Short.")),
            batch_paper("2026-07-10", 2, "2507.3", None),
        ],
    )
    .await
    .unwrap();
    let daily = dead_service(pool.clone());
    let server =
        TestServer::new(build_router_with_daily(pool, dir.path().to_path_buf(), daily)).unwrap();

    let resp = server.get("/api/daily").await;
    assert_eq!(resp.status_code(), 200);
    let v: Value = resp.json();
    assert_eq!(v["date"], "2026-07-10");
    assert_eq!(v["papers"].as_array().unwrap().len(), 2);
    assert_eq!(v["papers"][0]["rank"], 1);
    assert_eq!(v["papers"][0]["arxiv_id"], "2507.2");
    assert_eq!(v["papers"][0]["tldr"], "Short.");
    assert_eq!(v["papers"][0]["abstract"], "An abstract.");
    assert_eq!(v["papers"][1]["tldr"], Value::Null);
}

#[tokio::test]
async fn get_daily_empty_state_is_200_with_null_date() {
    let (dir, pool) = temp_pool().await;
    let daily = dead_service(pool.clone());
    let server =
        TestServer::new(build_router_with_daily(pool, dir.path().to_path_buf(), daily)).unwrap();
    let resp = server.get("/api/daily").await;
    assert_eq!(resp.status_code(), 200);
    let v: Value = resp.json();
    assert_eq!(v["date"], Value::Null);
    assert_eq!(v["papers"].as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn daily_routes_503_when_unconfigured() {
    let (dir, pool) = temp_pool().await;
    let server = TestServer::new(build_router(pool, dir.path().to_path_buf())).unwrap();
    assert_eq!(server.get("/api/daily").await.status_code(), 503);
    assert_eq!(server.post("/api/daily/run").await.status_code(), 503);
}

#[tokio::test]
async fn post_run_starts_then_conflicts_while_running() {
    let (dir, pool) = temp_pool().await;
    // Feed answers slowly so the first run stays in flight for the 409 check.
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/atom/cs.AI"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string(
                    r#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom"><title>ok</title></feed>"#,
                )
                .set_delay(std::time::Duration::from_secs(2)),
        )
        .mount(&mock)
        .await;
    let daily = DailyService::for_tests(
        daily_cfg(),
        pool.clone(),
        Embedder::for_tests("http://127.0.0.1:1/v1", "m", 4),
        QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap(),
        ChatClient::for_tests("http://127.0.0.1:1/v1", "m"),
        &format!("{}/atom", mock.uri()),
        "http://127.0.0.1:1/pdf",
    );
    let server =
        TestServer::new(build_router_with_daily(pool, dir.path().to_path_buf(), daily)).unwrap();

    assert_eq!(server.post("/api/daily/run").await.status_code(), 202);
    assert_eq!(server.post("/api/daily/run").await.status_code(), 409);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test web_daily_test`
Expected: compile error — `build_router_with_daily` not found.

- [ ] **Step 3: Implement.**

`src/web/dto.rs` — append:

```rust
/// One paper in the daily-recommendations response (Glance widget input).
#[derive(Serialize)]
pub struct DailyPaperDto {
    pub rank: i64,
    pub arxiv_id: String,
    pub title: String,
    pub authors: Vec<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: String,
    pub categories: Vec<String>,
    pub score: f64,
    pub tldr: Option<String>,
    pub abs_url: String,
    pub pdf_url: String,
}

impl From<&crate::daily::store::DailyPaper> for DailyPaperDto {
    fn from(p: &crate::daily::store::DailyPaper) -> Self {
        Self {
            rank: p.rank,
            arxiv_id: p.arxiv_id.clone(),
            title: p.title.clone(),
            authors: p.authors.clone(),
            abstract_text: p.abstract_text.clone(),
            categories: p.categories.clone(),
            score: p.score,
            tldr: p.tldr.clone(),
            abs_url: p.abs_url.clone(),
            pdf_url: p.pdf_url.clone(),
        }
    }
}

/// `date` is `None` until the first non-empty batch exists.
#[derive(Serialize)]
pub struct DailyResponse {
    pub date: Option<String>,
    pub papers: Vec<DailyPaperDto>,
}
```

`src/web/mod.rs`:
- Add to `AppState`:

```rust
    /// Present when daily arXiv recommendations are configured (`serve`).
    /// `None` -> /api/daily answers 503.
    pub daily: Option<Arc<crate::daily::DailyService>>,
```

- Add `daily: None,` to the `AppState` literals in `build_router`, `build_router_with_ingest`, `build_router_with_ingest_proxy`, and `build_router_with_search`.
- Add after `build_router_with_search`:

```rust
/// Read-only router plus a daily-recommendations service. Used by tests.
pub fn build_router_with_daily(
    pool: SqlitePool,
    library_root: PathBuf,
    daily: Arc<crate::daily::DailyService>,
) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: None,
        proxy_login_url: None,
        search: None,
        daily: Some(daily),
    })
}
```

- In `router_with`, add after the `/api/search/status` route:

```rust
        .route("/api/daily", get(api::daily_papers))
        .route("/api/daily/run", axum::routing::post(api::run_daily))
```

- Change `serve` to accept and forward the service (new last parameter):

```rust
pub async fn serve(
    host: &str,
    port: u16,
    pool: SqlitePool,
    library_root: PathBuf,
    ingest: Arc<Ingest>,
    proxy_login_url: Option<String>,
    search: Option<Arc<crate::search::SearchService>>,
    daily: Option<Arc<crate::daily::DailyService>>,
) -> Result<()> {
    let app = router_with(AppState {
        pool,
        library_root,
        ingest: Some(ingest),
        proxy_login_url,
        search,
        daily,
    });
```

`src/web/api.rs` — append (uses the existing `internal_error()` helper; add `DailyPaperDto, DailyResponse` to the `dto` imports at the top of the file):

```rust
/// GET /api/daily — the latest non-empty daily batch for the Glance widget.
pub async fn daily_papers(State(app): State<AppState>) -> Response {
    if app.daily.is_none() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "daily papers not configured"})),
        )
            .into_response();
    }
    match crate::daily::store::latest_batch(&app.pool).await {
        Ok(Some((date, papers))) => Json(DailyResponse {
            date: Some(date),
            papers: papers.iter().map(DailyPaperDto::from).collect(),
        })
        .into_response(),
        Ok(None) => Json(DailyResponse { date: None, papers: Vec::new() }).into_response(),
        Err(e) => {
            tracing::error!("daily papers: {e}");
            internal_error()
        }
    }
}

/// POST /api/daily/run — manual trigger; 202 started, 409 already running.
pub async fn run_daily(State(app): State<AppState>) -> Response {
    let Some(svc) = &app.daily else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "daily papers not configured"})),
        )
            .into_response();
    };
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    if svc.spawn_run(today) {
        (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"status": "started"})),
        )
            .into_response()
    } else {
        (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "a daily run is already in flight"})),
        )
            .into_response()
    }
}
```

`src/main.rs` — the `web::serve(...)` call in the `Serve` arm gains a trailing `None,` argument (real wiring lands in Task 8):

```rust
            web::serve(
                &host,
                port,
                pool,
                cfg.library_root.clone(),
                ingest,
                cfg.proxy.as_ref().map(|p| p.login_url.clone()),
                search,
                None,
            )
            .await?;
```

- [ ] **Step 4: Run the full test suite**

Run: `cargo test`
Expected: everything PASSES, including the 4 new `web_daily_test` tests and all pre-existing web tests (their routers now set `daily: None`).

- [ ] **Step 5: Commit**

```bash
git add src/web/ src/main.rs tests/web_daily_test.rs
git commit -m "feat(web): /api/daily endpoints for Glance"
```

---

### Task 8: Scheduler + serve wiring

**Files:**
- Create: `src/daily/scheduler.rs`
- Modify: `src/daily/mod.rs` (add `pub mod scheduler;`, validate `run_at` in `from_config`)
- Modify: `src/main.rs` (construct `DailyService`, spawn scheduler, pass to `serve`)

**Interfaces:**
- Consumes: `DailyService::{run_guarded, from_config}`, `store::get_run` (Tasks 2, 6).
- Produces (in `crate::daily::scheduler`):
  - `fn parse_run_at(s: &str) -> anyhow::Result<(u32, u32)>`
  - `fn run_due(now: DateTime<Utc>, run_at: (u32, u32), today_status: Option<&str>) -> bool`
  - `fn sleep_secs(now: DateTime<Utc>, run_at: (u32, u32)) -> u64` — capped at 3600 (hourly failure retry)
  - `async fn run(svc: Arc<DailyService>)` — the forever loop `serve` spawns

- [ ] **Step 1: Write failing tests.** Create `src/daily/scheduler.rs` with this test module; add `pub mod scheduler;` to `src/daily/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(h: u32, m: u32) -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 7, 10, h, m, 0).unwrap()
    }

    #[test]
    fn parses_and_validates_run_at() {
        assert_eq!(parse_run_at("09:00").unwrap(), (9, 0));
        assert_eq!(parse_run_at("23:59").unwrap(), (23, 59));
        assert!(parse_run_at("24:00").is_err());
        assert!(parse_run_at("09:60").is_err());
        assert!(parse_run_at("0900").is_err());
        assert!(parse_run_at("morning").is_err());
    }

    #[test]
    fn run_due_only_after_run_at_and_not_after_success() {
        // Before run_at: never due.
        assert!(!run_due(at(8, 59), (9, 0), None));
        // After run_at with no run yet (boot catch-up): due.
        assert!(run_due(at(15, 0), (9, 0), None));
        // Failed earlier today: due again (hourly retry).
        assert!(run_due(at(10, 0), (9, 0), Some("failed")));
        // Succeeded (ok or empty): not due.
        assert!(!run_due(at(10, 0), (9, 0), Some("ok")));
        assert!(!run_due(at(10, 0), (9, 0), Some("empty")));
    }

    #[test]
    fn sleep_secs_targets_run_at_and_caps_at_one_hour() {
        // 08:30, run at 09:00 -> 30 minutes.
        assert_eq!(sleep_secs(at(8, 30), (9, 0)), 30 * 60);
        // 10:00, run at 09:00 -> next occurrence is tomorrow, capped hourly.
        assert_eq!(sleep_secs(at(10, 0), (9, 0)), 3600);
        // Exactly at run_at -> next occurrence tomorrow, capped.
        assert_eq!(sleep_secs(at(9, 0), (9, 0)), 3600);
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib daily::scheduler`
Expected: compile error — functions not found.

- [ ] **Step 3: Implement** — prepend to `src/daily/scheduler.rs`:

```rust
use std::sync::Arc;

use chrono::{DateTime, Timelike, Utc};

use super::{store, DailyService};

/// Parse `[daily].run_at` as 24h "HH:MM".
pub fn parse_run_at(s: &str) -> anyhow::Result<(u32, u32)> {
    let (h, m) = s
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("[daily].run_at must be \"HH:MM\", got {s:?}"))?;
    let (h, m): (u32, u32) = (h.parse()?, m.parse()?);
    if h > 23 || m > 59 {
        anyhow::bail!("[daily].run_at out of range: {s:?}");
    }
    Ok((h, m))
}

/// A run is due when we are past today's `run_at` and today's run is
/// missing (boot catch-up) or failed (hourly retry). "ok"/"empty" are done.
pub fn run_due(now: DateTime<Utc>, run_at: (u32, u32), today_status: Option<&str>) -> bool {
    let past = (now.hour(), now.minute()) >= run_at;
    past && !matches!(today_status, Some("ok") | Some("empty"))
}

/// Seconds until the next occurrence of `run_at` (UTC), capped at 3600 so
/// the loop re-checks hourly (which is what retries failed runs).
pub fn sleep_secs(now: DateTime<Utc>, run_at: (u32, u32)) -> u64 {
    let today_target = now
        .date_naive()
        .and_hms_opt(run_at.0, run_at.1, 0)
        .expect("validated by parse_run_at");
    let target = if today_target > now.naive_utc() {
        today_target
    } else {
        today_target + chrono::Days::new(1)
    };
    let secs = (target - now.naive_utc()).num_seconds().max(1) as u64;
    secs.min(3600)
}

/// Forever loop spawned by `serve`: boot catch-up, the daily scheduled
/// run, and hourly retry after a failure — all same-day only (the arXiv
/// feed is a live window; there is no backfill).
pub async fn run(svc: Arc<DailyService>) {
    let run_at = parse_run_at(&svc.cfg.run_at).expect("validated in from_config");
    loop {
        let now = Utc::now();
        let today = now.format("%Y-%m-%d").to_string();
        let status = match store::get_run(&svc.pool, &today).await {
            Ok(r) => r.map(|r| r.status),
            Err(e) => {
                tracing::error!("daily scheduler: reading run state: {e:#}");
                None
            }
        };
        if run_due(now, run_at, status.as_deref()) {
            if let Some(run) = svc.run_guarded(&today).await {
                tracing::info!(
                    "daily run {}: {} ({} candidates)",
                    run.batch_date,
                    run.status,
                    run.papers_found
                );
            }
        }
        let secs = sleep_secs(Utc::now(), run_at);
        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
    }
}
```

- [ ] **Step 4: Validate `run_at` at startup.** In `src/daily/mod.rs`, `DailyService::from_config`, right after the empty-categories check, add:

```rust
        scheduler::parse_run_at(&daily.run_at)?; // fail fast on typos
```

- [ ] **Step 5: Wire into `serve`.** In `src/main.rs`:

Add to the imports:

```rust
use xuewen::daily::{self, DailyService};
```

In the `Command::Serve` arm, after the `if let Some(s) = &search { tokio::spawn(indexer::run(...)); }` block, add:

```rust
            let daily = DailyService::from_config(&cfg, pool.clone())?;
            if let Some(d) = &daily {
                tokio::spawn(daily::scheduler::run(d.clone()));
            }
```

and replace the `None,` placeholder in the `web::serve(...)` call (from Task 7) with `daily,`.

- [ ] **Step 6: Run the full test suite**

Run: `cargo test`
Expected: all tests PASS (scheduler unit tests: 3).

- [ ] **Step 7: Smoke-test manually (optional but recommended).** With a `[daily]` section in a scratch config and real keys, `cargo run -- serve` then `curl -X POST localhost:PORT/api/daily/run` and watch the log; `curl localhost:PORT/api/daily`. Skip if no key at hand — the wiremock coverage is the gate.

- [ ] **Step 8: Commit**

```bash
git add src/daily/scheduler.rs src/daily/mod.rs src/main.rs
git commit -m "feat(daily): in-process scheduler and serve wiring"
```

---

### Task 9: Deploy docs — Glance widget snippet

**Files:**
- Modify: `deploy/k8s/README.md`

**Interfaces:**
- Consumes: the `GET /api/daily` JSON shape (Task 7).
- Produces: documentation only.

- [ ] **Step 1: Add a section to `deploy/k8s/README.md`** (after the "Expose it" section). Verify the template functions against the installed Glance version's `custom-api` docs (https://github.com/glanceapp/glance/blob/main/docs/custom-api.md) when deploying; adjust if its API differs:

````markdown
## Daily arXiv papers on Glance

`xuewen serve` exposes daily arXiv recommendations at `GET /api/daily`
when the ConfigMap's `xuewen.toml` has a `[daily]` section (see
`xuewen.example.toml`; requires `[search.embedding]` and the
`OPENAI_API_KEY` secret, which the TL;DR generation shares by default).

Add a `custom-api` widget to your Glance dashboard's `glance.yml`:

```yaml
- type: custom-api
  title: Daily arXiv
  cache: 1h
  url: http://xuewen.<namespace>.svc.cluster.local/api/daily
  template: |
    {{ if .JSON.Array "papers" }}
    <p class="size-h6 color-subdue">{{ .JSON.String "date" }}</p>
    <ul class="list list-gap-14">
      {{ range .JSON.Array "papers" }}
      <li>
        <a class="size-h4 color-primary" href="{{ .String "abs_url" }}">{{ .String "title" }}</a>
        <div class="size-h6 color-subdue">
          {{ printf "%.2f" (.Float "score") }} · {{ .String "arxiv_id" }} ·
          <a href="{{ .String "pdf_url" }}">PDF</a>
        </div>
        <p>{{ if .String "tldr" }}{{ .String "tldr" }}{{ else }}{{ .String "abstract" }}{{ end }}</p>
      </li>
      {{ end }}
    </ul>
    {{ else }}
    <p>No papers yet — the first batch appears after the daily run.</p>
    {{ end }}
```

Trigger a run without waiting for the schedule:

    kubectl exec deploy/xuewen -- curl -s -X POST localhost:8000/api/daily/run
````

- [ ] **Step 2: Also add the `[daily]` block to the ConfigMap example.** In `deploy/k8s/xuewen-config.yaml`, append the same commented `[daily]` / `[daily.llm]` block added to `xuewen.example.toml` in Task 1 (keep it commented; enabling is a per-cluster decision).

- [ ] **Step 3: Commit**

```bash
git add deploy/k8s/README.md deploy/k8s/xuewen-config.yaml
git commit -m "docs(deploy): Glance widget for daily arXiv papers"
```

---

## Plan Self-Review (completed)

- **Spec coverage:** config (T1), storage/migration (T2), feed fetch+parse+announce-type filter+version strip+feed-error (T3), Qdrant scroll + profile + recency weights + trashed exclusion (T4), chat client + fallback chain + caps (T5), orchestration + dedup incl. trashed + PDF caps + prune + run recording + in-flight guard (T6), both endpoints + DTOs + 503/409/empty states (T7), scheduler (parse/due/sleep, boot catch-up, hourly retry) + serve wiring + fail-fast validation (T8), Glance widget + deploy docs (T9). No gaps found.
- **Types:** `DailyPaper`/`DailyRun` field names match the migration and DTOs; `for_tests` signatures match their uses in T6/T7 tests; `serve` signature change is applied in T7 and consumed in T8.
- **Known judgment call:** the Glance template syntax is documented as "verify against the installed version" (T9) since it can't be tested here.
