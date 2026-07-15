use anyhow::{bail, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::QueryBuilder;
use sqlx::SqlitePool;
use std::collections::HashSet;
use std::str::FromStr;

use crate::models::{Paper, Project, ProjectSummary, Tag, TagSummary};

/// Open (creating if needed) the SQLite database and run migrations.
pub async fn connect(database_url: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(database_url)?
        .create_if_missing(true)
        .foreign_keys(true)
        .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
        .busy_timeout(std::time::Duration::from_secs(10));
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(opts)
        .await?;
    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}

/// The paper (active or trashed) whose stored bytes match `content_hash`.
pub async fn find_by_hash(pool: &SqlitePool, content_hash: &str) -> Result<Option<Paper>> {
    let p = sqlx::query_as::<_, Paper>("SELECT * FROM papers WHERE content_hash = ?")
        .bind(content_hash)
        .fetch_optional(pool)
        .await?;
    Ok(p)
}

/// The paper (active or trashed) already holding `doi` or `arxiv_id`.
/// A DOI match wins over an arXiv match when both exist.
pub async fn find_by_identifier(
    pool: &SqlitePool,
    doi: Option<&str>,
    arxiv_id: Option<&str>,
) -> Result<Option<Paper>> {
    if let Some(doi) = doi {
        let hit = sqlx::query_as::<_, Paper>("SELECT * FROM papers WHERE doi = ?")
            .bind(doi)
            .fetch_optional(pool)
            .await?;
        if hit.is_some() {
            return Ok(hit);
        }
    }
    if let Some(arxiv_id) = arxiv_id {
        let hit = sqlx::query_as::<_, Paper>("SELECT * FROM papers WHERE arxiv_id = ?")
            .bind(arxiv_id)
            .fetch_optional(pool)
            .await?;
        if hit.is_some() {
            return Ok(hit);
        }
    }
    Ok(None)
}

/// Un-trash a paper. Returns true if a row was actually restored.
pub async fn restore(pool: &SqlitePool, id: &str) -> Result<bool> {
    let res =
        sqlx::query("UPDATE papers SET deleted_at = NULL WHERE id = ? AND deleted_at IS NOT NULL")
            .bind(id)
            .execute(pool)
            .await?;
    Ok(res.rows_affected() > 0)
}

/// Whether `e` (from a db call) is a UNIQUE-constraint violation.
pub fn is_unique_violation(e: &anyhow::Error) -> bool {
    e.downcast_ref::<sqlx::Error>()
        .and_then(|e| e.as_database_error())
        .is_some_and(|d| d.kind() == sqlx::error::ErrorKind::UniqueViolation)
}

pub async fn insert_paper(pool: &SqlitePool, p: &Paper) -> Result<()> {
    sqlx::query(
        "INSERT INTO papers \
         (id, content_hash, rel_path, title, abstract, authors, venue, year, \
          doi, arxiv_id, dblp_key, cite_key, url, source, status, added_at, deleted_at) \
         VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
    )
    .bind(&p.id)
    .bind(&p.content_hash)
    .bind(&p.rel_path)
    .bind(&p.meta.title)
    .bind(&p.meta.abstract_text)
    .bind(&p.meta.authors)
    .bind(&p.meta.venue)
    .bind(p.meta.year)
    .bind(&p.meta.doi)
    .bind(&p.meta.arxiv_id)
    .bind(&p.meta.dblp_key)
    .bind(&p.cite_key)
    .bind(&p.meta.url)
    .bind(&p.meta.source)
    .bind(p.meta.status)
    .bind(&p.added_at)
    .bind(&p.deleted_at)
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

/// Cite keys already taken by other papers that share `base` as a prefix.
/// `exclude_id` skips a paper's own key (used when re-filing during refresh).
pub async fn cite_keys_with_base(
    pool: &SqlitePool,
    base: &str,
    exclude_id: Option<&str>,
) -> Result<HashSet<String>> {
    let pattern = format!("{base}%");
    let rows: Vec<(String,)> = match exclude_id {
        Some(id) => {
            sqlx::query_as(
                "SELECT cite_key FROM papers \
                 WHERE cite_key IS NOT NULL AND cite_key LIKE ? AND id <> ?",
            )
            .bind(&pattern)
            .bind(id)
            .fetch_all(pool)
            .await?
        }
        None => {
            sqlx::query_as(
                "SELECT cite_key FROM papers WHERE cite_key IS NOT NULL AND cite_key LIKE ?",
            )
            .bind(&pattern)
            .fetch_all(pool)
            .await?
        }
    };
    Ok(rows.into_iter().map(|(k,)| k).collect())
}

/// Overwrite a paper's mutable columns by id (leaves id/content_hash/added_at).
pub async fn update_paper(pool: &SqlitePool, p: &Paper) -> Result<()> {
    sqlx::query(
        "UPDATE papers SET \
         rel_path = ?, title = ?, abstract = ?, authors = ?, venue = ?, year = ?, \
         doi = ?, arxiv_id = ?, dblp_key = ?, cite_key = ?, url = ?, source = ?, \
         status = ?, deleted_at = ? \
         WHERE id = ?",
    )
    .bind(&p.rel_path)
    .bind(&p.meta.title)
    .bind(&p.meta.abstract_text)
    .bind(&p.meta.authors)
    .bind(&p.meta.venue)
    .bind(p.meta.year)
    .bind(&p.meta.doi)
    .bind(&p.meta.arxiv_id)
    .bind(&p.meta.dblp_key)
    .bind(&p.cite_key)
    .bind(&p.meta.url)
    .bind(&p.meta.source)
    .bind(p.meta.status)
    .bind(&p.deleted_at)
    .bind(&p.id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Every paper, oldest first.
pub async fn all_papers(pool: &SqlitePool) -> Result<Vec<Paper>> {
    let papers = sqlx::query_as::<_, Paper>(
        "SELECT * FROM papers WHERE deleted_at IS NULL ORDER BY added_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(papers)
}

/// Papers whose id starts with `prefix` (for `refresh <ID>` prefix matching).
pub async fn find_by_id_prefix(pool: &SqlitePool, prefix: &str) -> Result<Vec<Paper>> {
    let pattern = format!("{prefix}%");
    let papers = sqlx::query_as::<_, Paper>("SELECT * FROM papers WHERE id LIKE ?")
        .bind(&pattern)
        .fetch_all(pool)
        .await?;
    Ok(papers)
}

/// Mark a paper as trashed (soft-delete). Returns true if a row was newly
/// trashed (false if it didn't exist or was already trashed).
pub async fn soft_delete(pool: &SqlitePool, id: &str) -> Result<bool> {
    let ts = chrono::Utc::now().to_rfc3339();
    let res = sqlx::query("UPDATE papers SET deleted_at = ? WHERE id = ? AND deleted_at IS NULL")
        .bind(ts)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Every trashed paper, oldest-trashed first.
pub async fn trashed_papers(pool: &SqlitePool) -> Result<Vec<Paper>> {
    let papers = sqlx::query_as::<_, Paper>(
        "SELECT * FROM papers WHERE deleted_at IS NOT NULL ORDER BY deleted_at",
    )
    .fetch_all(pool)
    .await?;
    Ok(papers)
}

/// Permanently remove a paper row (the caller removes the PDF file).
pub async fn delete_row(pool: &SqlitePool, id: &str) -> Result<()> {
    sqlx::query("DELETE FROM papers WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Set (or clear) a paper's starred flag. Returns true if a row was updated.
pub async fn set_paper_starred(pool: &SqlitePool, paper_id: &str, starred: bool) -> Result<bool> {
    Ok(sqlx::query("UPDATE papers SET starred = ? WHERE id = ?")
        .bind(starred as i64)
        .bind(paper_id)
        .execute(pool)
        .await?
        .rows_affected()
        > 0)
}

/// Find a paper by exact id, else by unique id prefix (active or trashed).
pub async fn find_one(pool: &SqlitePool, id: &str) -> Result<Paper> {
    if let Some(p) = get_by_id(pool, id).await? {
        return Ok(p);
    }
    let mut matches = find_by_id_prefix(pool, id).await?;
    match matches.len() {
        0 => bail!("no paper with id or prefix {id:?}"),
        1 => Ok(matches.pop().unwrap()),
        n => bail!("ambiguous id prefix {id:?} matches {n} papers"),
    }
}

pub async fn create_project(pool: &SqlitePool, name: &str, _note: Option<&str>) -> Result<Project> {
    let project = Project {
        id: uuid::Uuid::now_v7().to_string(),
        name: name.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    sqlx::query("INSERT INTO projects (id, name, created_at) VALUES (?,?,?)")
        .bind(&project.id)
        .bind(&project.name)
        .bind(&project.created_at)
        .execute(pool)
        .await?;
    Ok(project)
}

pub async fn list_projects(pool: &SqlitePool) -> Result<Vec<ProjectSummary>> {
    // Count only active members: a trashed paper's membership row lingers (until
    // FK cascade on purge), but the `?project=` filter runs under
    // `deleted_at IS NULL`, so the count must match what filtering shows.
    let rows = sqlx::query_as::<_, ProjectSummary>(
        "SELECT p.id, p.name, p.created_at, \
         COUNT(pa.id) AS paper_count \
         FROM projects p \
         LEFT JOIN paper_projects pp ON pp.project_id = p.id \
         LEFT JOIN papers pa ON pa.id = pp.paper_id AND pa.deleted_at IS NULL \
         GROUP BY p.id ORDER BY p.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get_project(pool: &SqlitePool, id: &str) -> Result<Option<Project>> {
    let p = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(p)
}

pub async fn update_project(
    pool: &SqlitePool,
    id: &str,
    name: &str,
    _note: Option<&str>,
) -> Result<bool> {
    let res = sqlx::query("UPDATE projects SET name = ? WHERE id = ?")
        .bind(name)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn delete_project(pool: &SqlitePool, id: &str) -> Result<bool> {
    let res = sqlx::query("DELETE FROM projects WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

pub async fn add_paper_to_project(
    pool: &SqlitePool,
    paper_id: &str,
    project_id: &str,
) -> Result<()> {
    let ts = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO paper_projects (paper_id, project_id, added_at) VALUES (?,?,?) \
         ON CONFLICT (paper_id, project_id) DO NOTHING",
    )
    .bind(paper_id)
    .bind(project_id)
    .bind(&ts)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn remove_paper_from_project(
    pool: &SqlitePool,
    paper_id: &str,
    project_id: &str,
) -> Result<bool> {
    let res = sqlx::query("DELETE FROM paper_projects WHERE paper_id = ? AND project_id = ?")
        .bind(paper_id)
        .bind(project_id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// A paper's project memberships, ordered by when it joined each project.
pub async fn projects_for_paper(pool: &SqlitePool, paper_id: &str) -> Result<Vec<Project>> {
    let projects = sqlx::query_as::<_, Project>(
        "SELECT p.* FROM projects p JOIN paper_projects pp ON pp.project_id = p.id \
         WHERE pp.paper_id = ? ORDER BY pp.added_at",
    )
    .bind(paper_id)
    .fetch_all(pool)
    .await?;
    Ok(projects)
}

pub async fn project_ids_for_paper(pool: &SqlitePool, paper_id: &str) -> Result<Vec<String>> {
    let rows: Vec<(String,)> = sqlx::query_as(
        "SELECT project_id FROM paper_projects WHERE paper_id = ? ORDER BY added_at",
    )
    .bind(paper_id)
    .fetch_all(pool)
    .await?;
    Ok(rows.into_iter().map(|(id,)| id).collect())
}

pub async fn find_project_by_name(pool: &SqlitePool, name: &str) -> Result<Option<Project>> {
    let p = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE name = ? COLLATE NOCASE")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(p)
}

/// Resolve a project selector: exact (case-insensitive) name, then exact id,
/// then unique id prefix. Errors on no match or an ambiguous prefix.
pub async fn find_one_project(pool: &SqlitePool, sel: &str) -> Result<Project> {
    if let Some(p) = find_project_by_name(pool, sel).await? {
        return Ok(p);
    }
    if let Some(p) = get_project(pool, sel).await? {
        return Ok(p);
    }
    let pattern = format!("{sel}%");
    let mut matches = sqlx::query_as::<_, Project>("SELECT * FROM projects WHERE id LIKE ?")
        .bind(&pattern)
        .fetch_all(pool)
        .await?;
    match matches.len() {
        0 => bail!("no project named or id-prefixed {sel:?}"),
        1 => Ok(matches.pop().unwrap()),
        n => bail!("ambiguous project selector {sel:?} matches {n} projects"),
    }
}

/// Split on `/`, trim each segment, drop empties, rejoin. `None` if empty.
pub fn normalize_tag_name(raw: &str) -> Option<String> {
    let joined = raw
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    (!joined.is_empty()).then_some(joined)
}

pub async fn find_tag_by_name(pool: &SqlitePool, name: &str) -> Result<Option<Tag>> {
    let t = sqlx::query_as::<_, Tag>("SELECT * FROM tags WHERE name = ? COLLATE NOCASE")
        .bind(name)
        .fetch_optional(pool)
        .await?;
    Ok(t)
}

pub async fn create_tag(pool: &SqlitePool, name: &str) -> Result<Tag> {
    let tag = Tag {
        id: uuid::Uuid::now_v7().to_string(),
        name: name.to_string(),
        created_at: chrono::Utc::now().to_rfc3339(),
    };
    sqlx::query("INSERT INTO tags (id, name, created_at) VALUES (?,?,?)")
        .bind(&tag.id)
        .bind(&tag.name)
        .bind(&tag.created_at)
        .execute(pool)
        .await?;
    Ok(tag)
}

/// Create the tag if it doesn't exist (case-insensitively), then link it to
/// the paper. Idempotent: re-adding an already-linked tag is a no-op.
pub async fn add_paper_tag(pool: &SqlitePool, paper_id: &str, raw_name: &str) -> Result<Tag> {
    let name = normalize_tag_name(raw_name).ok_or_else(|| anyhow::anyhow!("empty tag name"))?;
    let tag = match find_tag_by_name(pool, &name).await? {
        Some(t) => t,
        None => create_tag(pool, &name).await?,
    };
    let ts = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO paper_tags (paper_id, tag_id, added_at) VALUES (?,?,?) \
         ON CONFLICT (paper_id, tag_id) DO NOTHING",
    )
    .bind(paper_id)
    .bind(&tag.id)
    .bind(&ts)
    .execute(pool)
    .await?;
    Ok(tag)
}

/// Unlink a tag from a paper; if that was the tag's last membership, delete
/// the now-orphaned tag row too.
pub async fn remove_paper_tag(pool: &SqlitePool, paper_id: &str, tag_id: &str) -> Result<bool> {
    let removed = sqlx::query("DELETE FROM paper_tags WHERE paper_id = ? AND tag_id = ?")
        .bind(paper_id)
        .bind(tag_id)
        .execute(pool)
        .await?
        .rows_affected()
        > 0;
    let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM paper_tags WHERE tag_id = ?")
        .bind(tag_id)
        .fetch_one(pool)
        .await?;
    if remaining == 0 {
        sqlx::query("DELETE FROM tags WHERE id = ?")
            .bind(tag_id)
            .execute(pool)
            .await?;
    }
    Ok(removed)
}

pub async fn tags_for_paper(pool: &SqlitePool, paper_id: &str) -> Result<Vec<Tag>> {
    let tags = sqlx::query_as::<_, Tag>(
        "SELECT t.* FROM tags t JOIN paper_tags pt ON pt.tag_id = t.id \
         WHERE pt.paper_id = ? ORDER BY t.name COLLATE NOCASE",
    )
    .bind(paper_id)
    .fetch_all(pool)
    .await?;
    Ok(tags)
}

pub async fn list_tags_with_counts(pool: &SqlitePool) -> Result<Vec<TagSummary>> {
    // `paper_count` is a prefix rollup, not a flat count: it counts distinct
    // papers carrying this tag OR any `name/*` child, matching what clicking the
    // tag's filter returns (parent + every descendant).
    // NOTE: a tag name literally containing `%`/`_` would over-match under LIKE;
    // this mirrors the search filter's known caveat — we don't escape here.
    let rows = sqlx::query_as::<_, TagSummary>(
        "SELECT t.id, t.name, t.created_at, \
                (SELECT COUNT(DISTINCT pt.paper_id) \
                   FROM paper_tags pt JOIN tags c ON c.id = pt.tag_id \
                   WHERE c.name = t.name OR c.name LIKE t.name || '/%') AS paper_count \
         FROM tags t ORDER BY t.name COLLATE NOCASE",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn rename_tag(pool: &SqlitePool, id: &str, raw_name: &str) -> Result<Option<Tag>> {
    let name = normalize_tag_name(raw_name).ok_or_else(|| anyhow::anyhow!("empty tag name"))?;
    let res = sqlx::query("UPDATE tags SET name = ? WHERE id = ?")
        .bind(&name)
        .bind(id)
        .execute(pool)
        .await?;
    if res.rows_affected() == 0 {
        return Ok(None);
    }
    let tag = sqlx::query_as::<_, Tag>("SELECT * FROM tags WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(tag)
}

pub async fn delete_tag(pool: &SqlitePool, id: &str) -> Result<bool> {
    let res = sqlx::query("DELETE FROM tags WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(res.rows_affected() > 0)
}

/// Escape `\`, `%`, `_` in a user search term for `LIKE … ESCAPE '\'`.
fn escape_like(term: &str) -> String {
    term.replace('\\', r"\\")
        .replace('%', r"\%")
        .replace('_', r"\_")
}

/// List papers with optional case-insensitive search (`q` over title+authors),
/// optional status filter, and a whitelisted sort. Unknown status/sort values
/// are ignored (never an error).
pub async fn list_papers(
    pool: &SqlitePool,
    q: Option<&str>,
    status: Option<&str>,
    sort: Option<&str>,
    project: Option<&str>,
) -> Result<Vec<Paper>> {
    let mut qb: QueryBuilder<sqlx::Sqlite> =
        QueryBuilder::new("SELECT * FROM papers WHERE deleted_at IS NULL");
    if let Some(term) = q.map(str::trim).filter(|s| !s.is_empty()) {
        let like = format!("%{}%", escape_like(term));
        qb.push(" AND (title LIKE ")
            .push_bind(like.clone())
            .push(" ESCAPE '\\' OR authors LIKE ")
            .push_bind(like)
            .push(" ESCAPE '\\')");
    }
    if let Some(st) = status.filter(|s| matches!(*s, "resolved" | "needs_review")) {
        qb.push(" AND status = ").push_bind(st.to_string());
    }
    if let Some(pid) = project.map(str::trim).filter(|s| !s.is_empty()) {
        qb.push(" AND id IN (SELECT paper_id FROM paper_projects WHERE project_id = ")
            .push_bind(pid.to_string())
            .push(")");
    }
    // Whitelisted ORDER BY (never interpolate raw user input).
    let order = match sort {
        Some("year_asc") => "year ASC NULLS LAST",
        Some("added_desc") => "added_at DESC",
        Some("title") => "title COLLATE NOCASE ASC",
        Some("year_desc") => "year DESC",
        _ => "year DESC", // unknown values fall back to the default
    };
    qb.push(" ORDER BY ").push(order);
    let papers = qb.build_query_as::<Paper>().fetch_all(pool).await?;
    Ok(papers)
}

/// `(total, resolved, needs_review)` paper counts.
pub async fn stats(pool: &SqlitePool) -> Result<(i64, i64, i64)> {
    let row: (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), \
         COALESCE(SUM(status = 'resolved'), 0), \
         COALESCE(SUM(status = 'needs_review'), 0) \
         FROM papers WHERE deleted_at IS NULL",
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}

/// Read a single setting value by key.
pub async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(v,)| v))
}

/// The RFC3339 timestamp a setting was last written, if it exists.
pub async fn setting_updated_at(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT updated_at FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(v,)| v))
}

/// Insert or overwrite a setting, stamping `updated_at` with the current time.
pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    let ts = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(ts)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove a setting (no-op if absent).
pub async fn delete_setting(pool: &SqlitePool, key: &str) -> Result<()> {
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Authors, PaperMeta, PaperStatus};

    fn sample_paper(id: &str, hash: &str) -> Paper {
        Paper {
            id: id.to_string(),
            content_hash: hash.to_string(),
            rel_path: format!("{hash}.pdf"),
            cite_key: None,
            added_at: "2026-07-06T00:00:00Z".to_string(),
            deleted_at: None,
            starred: false,
            meta: PaperMeta {
                title: Some("A Title".into()),
                abstract_text: None,
                authors: Authors::default(),
                venue: None,
                year: None,
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::NeedsReview,
            },
        }
    }

    async fn temp_pool() -> (tempfile::TempDir, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("test.db");
        let url = format!("sqlite:{}", db_path.display());
        let pool = connect(&url).await.unwrap();
        (dir, pool)
    }

    #[test]
    fn escape_like_escapes_backslash_percent_and_underscore() {
        assert_eq!(escape_like("100%"), r"100\%");
        assert_eq!(escape_like("a_b"), r"a\_b");
        assert_eq!(escape_like(r"back\slash"), r"back\\slash");
        assert_eq!(escape_like("%_\\"), r"\%\_\\");
    }

    #[tokio::test]
    async fn connect_uses_wal_journal_mode() {
        let (_dir, pool) = temp_pool().await;
        let (mode,): (String,) = sqlx::query_as("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(mode.to_lowercase(), "wal");
    }

    #[tokio::test]
    async fn insert_then_fetch_and_dedup() {
        let (_dir, pool) = temp_pool().await;

        assert!(find_by_hash(&pool, "abc").await.unwrap().is_none());

        let p = sample_paper("01890000-0000-7000-8000-000000000000", "abc");
        insert_paper(&pool, &p).await.unwrap();

        assert!(find_by_hash(&pool, "abc").await.unwrap().is_some());

        let got = get_by_id(&pool, &p.id).await.unwrap().unwrap();
        assert_eq!(got.content_hash, "abc");
        assert_eq!(got.meta.title.as_deref(), Some("A Title"));
        assert_eq!(got.meta.status, PaperStatus::NeedsReview);
    }

    #[tokio::test]
    async fn cite_keys_with_base_returns_prefix_matches() {
        let (_dir, pool) = temp_pool().await;

        let mut a = sample_paper("01890000-0000-7000-8000-00000000000a", "ha");
        a.cite_key = Some("he2016deep".into());
        insert_paper(&pool, &a).await.unwrap();

        let mut b = sample_paper("01890000-0000-7000-8000-00000000000b", "hb");
        b.cite_key = Some("he2016deepa".into());
        insert_paper(&pool, &b).await.unwrap();

        let taken = cite_keys_with_base(&pool, "he2016deep", None)
            .await
            .unwrap();
        assert!(taken.contains("he2016deep"));
        assert!(taken.contains("he2016deepa"));

        let taken_excl = cite_keys_with_base(&pool, "he2016deep", Some(&a.id))
            .await
            .unwrap();
        assert!(!taken_excl.contains("he2016deep"));
        assert!(taken_excl.contains("he2016deepa"));
    }

    #[tokio::test]
    async fn update_paper_persists_changes() {
        let (_dir, pool) = temp_pool().await;
        let mut p = sample_paper("01890000-0000-7000-8000-0000000000c1", "h1");
        insert_paper(&pool, &p).await.unwrap();

        // Mutate every updatable column to catch a dropped SET clause.
        p.meta.title = Some("New Title".into());
        p.meta.abstract_text = Some("New abstract".into());
        p.meta.authors = Authors(vec!["Ada Lovelace".into()]);
        p.meta.venue = Some("KDD".into());
        p.meta.year = Some(2019);
        p.meta.doi = Some("10.1/new".into());
        p.meta.arxiv_id = Some("2001.00001".into());
        p.meta.dblp_key = Some("conf/kdd/X".into());
        p.rel_path = "he2016deep.pdf".into();
        p.cite_key = Some("he2016deep".into());
        p.meta.url = Some("https://example.org/x".into());
        p.meta.source = Some("crossref".into());
        p.meta.status = PaperStatus::Resolved;
        update_paper(&pool, &p).await.unwrap();

        let got = get_by_id(&pool, &p.id).await.unwrap().unwrap();
        assert_eq!(got.meta.title.as_deref(), Some("New Title"));
        assert_eq!(got.meta.abstract_text.as_deref(), Some("New abstract"));
        assert_eq!(got.meta.authors, Authors(vec!["Ada Lovelace".into()]));
        assert_eq!(got.meta.venue.as_deref(), Some("KDD"));
        assert_eq!(got.meta.year, Some(2019));
        assert_eq!(got.meta.doi.as_deref(), Some("10.1/new"));
        assert_eq!(got.meta.arxiv_id.as_deref(), Some("2001.00001"));
        assert_eq!(got.meta.dblp_key.as_deref(), Some("conf/kdd/X"));
        assert_eq!(got.rel_path, "he2016deep.pdf");
        assert_eq!(got.cite_key.as_deref(), Some("he2016deep"));
        assert_eq!(got.meta.url.as_deref(), Some("https://example.org/x"));
        assert_eq!(got.meta.source.as_deref(), Some("crossref"));
        assert_eq!(got.meta.status, PaperStatus::Resolved);
        assert_eq!(got.content_hash, "h1"); // immutable columns untouched
    }

    #[tokio::test]
    async fn all_papers_and_find_by_prefix() {
        let (_dir, pool) = temp_pool().await;
        let a = sample_paper("01890000-0000-7000-8000-0000000000a1", "ha");
        let b = sample_paper("01890000-0000-7000-8000-0000000000b2", "hb");
        insert_paper(&pool, &a).await.unwrap();
        insert_paper(&pool, &b).await.unwrap();

        assert_eq!(all_papers(&pool).await.unwrap().len(), 2);

        // Unique prefix → exactly one match.
        let hit = find_by_id_prefix(&pool, "01890000-0000-7000-8000-0000000000a")
            .await
            .unwrap();
        assert_eq!(hit.len(), 1);
        assert_eq!(hit[0].id, a.id);

        // Shared prefix → both.
        let both = find_by_id_prefix(&pool, "01890000").await.unwrap();
        assert_eq!(both.len(), 2);
    }

    #[tokio::test]
    async fn authors_roundtrip_null_json_and_garbage() {
        let (_dir, pool) = temp_pool().await;
        // Empty -> stored NULL -> decodes empty.
        let a = sample_paper("01890000-0000-7000-8000-0000000000e5", "he");
        insert_paper(&pool, &a).await.unwrap();
        let raw: (Option<String>,) = sqlx::query_as("SELECT authors FROM papers WHERE id = ?")
            .bind(&a.id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(raw.0, None, "empty authors must be stored as SQL NULL");
        assert!(get_by_id(&pool, &a.id)
            .await
            .unwrap()
            .unwrap()
            .meta
            .authors
            .0
            .is_empty());
        // Non-empty round-trips.
        let mut b = sample_paper("01890000-0000-7000-8000-0000000000e6", "hf");
        b.meta.authors = Authors(vec!["Kaiming He".into(), "Xiangyu Zhang".into()]);
        insert_paper(&pool, &b).await.unwrap();
        assert_eq!(
            get_by_id(&pool, &b.id)
                .await
                .unwrap()
                .unwrap()
                .meta
                .authors
                .0,
            vec!["Kaiming He", "Xiangyu Zhang"]
        );
        // Garbage in the column decodes to empty (legacy tolerance).
        sqlx::query("UPDATE papers SET authors = 'not json' WHERE id = ?")
            .bind(&b.id)
            .execute(&pool)
            .await
            .unwrap();
        assert!(get_by_id(&pool, &b.id)
            .await
            .unwrap()
            .unwrap()
            .meta
            .authors
            .0
            .is_empty());
    }

    #[tokio::test]
    async fn list_papers_filters_and_sorts() {
        let (_dir, pool) = temp_pool().await;
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000a1", "ha");
        a.meta.title = Some("Deep Residual Learning".into());
        a.meta.authors = Authors(vec!["Kaiming He".into()]);
        a.meta.year = Some(2016);
        a.meta.status = PaperStatus::Resolved;
        let mut b = sample_paper("01890000-0000-7000-8000-0000000000b2", "hb");
        b.meta.title = Some("Attention Is All You Need".into());
        b.meta.authors = Authors(vec!["Ashish Vaswani".into()]);
        b.meta.year = Some(2017);
        b.meta.status = PaperStatus::NeedsReview;
        insert_paper(&pool, &a).await.unwrap();
        insert_paper(&pool, &b).await.unwrap();

        // No filters → both, default sort year DESC (2017 before 2016).
        let all = list_papers(&pool, None, None, None, None).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].meta.year, Some(2017));

        // q matches title (case-insensitive) or authors.
        let hits = list_papers(&pool, Some("residual"), None, None, None)
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, a.id);
        let by_author = list_papers(&pool, Some("vaswani"), None, None, None)
            .await
            .unwrap();
        assert_eq!(by_author.len(), 1);
        assert_eq!(by_author[0].id, b.id);

        // status filter.
        let resolved = list_papers(&pool, None, Some("resolved"), None, None)
            .await
            .unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, a.id);

        // q + status together (covers the AND branch).
        let combined = list_papers(&pool, Some("attention"), Some("needs_review"), None, None)
            .await
            .unwrap();
        assert_eq!(combined.len(), 1);
        assert_eq!(combined[0].id, b.id);
        let none = list_papers(&pool, Some("attention"), Some("resolved"), None, None)
            .await
            .unwrap();
        assert!(none.is_empty());

        // year_asc sort.
        let asc = list_papers(&pool, None, None, Some("year_asc"), None)
            .await
            .unwrap();
        assert_eq!(asc[0].meta.year, Some(2016));

        // An unknown status is ignored (not an error) → both rows.
        let bogus = list_papers(&pool, None, Some("nonsense"), None, None)
            .await
            .unwrap();
        assert_eq!(bogus.len(), 2);
    }

    #[tokio::test]
    async fn search_treats_like_wildcards_literally() {
        let (_dir, pool) = temp_pool().await;
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000f1", "wa");
        a.meta.title = Some("100% Accurate Results".into());
        let mut b = sample_paper("01890000-0000-7000-8000-0000000000f2", "wb");
        b.meta.title = Some("1000 Accurate Results".into());
        insert_paper(&pool, &a).await.unwrap();
        insert_paper(&pool, &b).await.unwrap();

        // "%" must match only the literal percent title, not act as a wildcard.
        let hits = list_papers(&pool, Some("100%"), None, None, None)
            .await
            .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, a.id);
    }

    #[tokio::test]
    async fn stats_counts_by_status() {
        let (_dir, pool) = temp_pool().await;
        assert_eq!(stats(&pool).await.unwrap(), (0, 0, 0));
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000a1", "ha");
        a.meta.status = PaperStatus::Resolved;
        let b = sample_paper("01890000-0000-7000-8000-0000000000b2", "hb"); // needs_review
        insert_paper(&pool, &a).await.unwrap();
        insert_paper(&pool, &b).await.unwrap();
        assert_eq!(stats(&pool).await.unwrap(), (2, 1, 1));
    }

    #[tokio::test]
    async fn deleted_at_round_trips() {
        let (_dir, pool) = temp_pool().await;
        let mut p = sample_paper("01890000-0000-7000-8000-0000000000d0", "hd");
        insert_paper(&pool, &p).await.unwrap();
        // Fresh insert is active.
        assert_eq!(
            get_by_id(&pool, &p.id).await.unwrap().unwrap().deleted_at,
            None
        );
        // update_paper persists a set deleted_at.
        p.deleted_at = Some("2026-07-07T12:00:00Z".into());
        update_paper(&pool, &p).await.unwrap();
        assert_eq!(
            get_by_id(&pool, &p.id)
                .await
                .unwrap()
                .unwrap()
                .deleted_at
                .as_deref(),
            Some("2026-07-07T12:00:00Z")
        );
    }

    #[tokio::test]
    async fn soft_delete_hides_and_purge_removes() {
        let (_dir, pool) = temp_pool().await;
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000a1", "ha");
        a.meta.status = PaperStatus::Resolved;
        let b = sample_paper("01890000-0000-7000-8000-0000000000b2", "hb");
        insert_paper(&pool, &a).await.unwrap();
        insert_paper(&pool, &b).await.unwrap();

        // Soft-delete a: hidden from list/stats/all_papers; b remains.
        assert!(soft_delete(&pool, &a.id).await.unwrap());
        assert!(!soft_delete(&pool, &a.id).await.unwrap()); // idempotent: already trashed
        let listed = list_papers(&pool, None, None, None, None).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, b.id);
        assert_eq!(stats(&pool).await.unwrap().0, 1); // total counts only active
        assert_eq!(all_papers(&pool).await.unwrap().len(), 1);

        // trashed_papers sees a.
        let trashed = trashed_papers(&pool).await.unwrap();
        assert_eq!(trashed.len(), 1);
        assert_eq!(trashed[0].id, a.id);

        // find_one still resolves a trashed paper (by prefix), and get_by_id sees it.
        let found = find_one(&pool, "01890000-0000-7000-8000-0000000000a")
            .await
            .unwrap();
        assert_eq!(found.id, a.id);

        // purge (delete_row) removes it entirely.
        delete_row(&pool, &a.id).await.unwrap();
        assert!(get_by_id(&pool, &a.id).await.unwrap().is_none());
        assert!(trashed_papers(&pool).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn find_by_hash_sees_active_and_trashed() {
        let (_dir, pool) = temp_pool().await;
        assert!(find_by_hash(&pool, "abc").await.unwrap().is_none());
        let p = sample_paper("01890000-0000-7000-8000-000000000001", "abc");
        insert_paper(&pool, &p).await.unwrap();
        assert_eq!(find_by_hash(&pool, "abc").await.unwrap().unwrap().id, p.id);
        soft_delete(&pool, &p.id).await.unwrap();
        let hit = find_by_hash(&pool, "abc").await.unwrap().unwrap();
        assert!(hit.deleted_at.is_some()); // trashed rows still match
    }

    #[tokio::test]
    async fn find_by_identifier_matches_doi_or_arxiv() {
        let (_dir, pool) = temp_pool().await;
        let mut p = sample_paper("01890000-0000-7000-8000-000000000002", "h2");
        p.meta.doi = Some("10.1/x".into());
        p.meta.arxiv_id = Some("2001.00001".into());
        insert_paper(&pool, &p).await.unwrap();

        assert_eq!(
            find_by_identifier(&pool, Some("10.1/x"), None)
                .await
                .unwrap()
                .unwrap()
                .id,
            p.id
        );
        assert_eq!(
            find_by_identifier(&pool, None, Some("2001.00001"))
                .await
                .unwrap()
                .unwrap()
                .id,
            p.id
        );
        assert!(find_by_identifier(&pool, Some("10.9/other"), None)
            .await
            .unwrap()
            .is_none());
        assert!(find_by_identifier(&pool, None, None)
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test]
    async fn find_by_identifier_prefers_doi_over_arxiv() {
        let (_dir, pool) = temp_pool().await;
        // Row A holds only the DOI; row B holds only the arXiv id.
        let mut a = sample_paper("01890000-0000-7000-8000-000000000006", "h6");
        a.meta.doi = Some("10.1/x".into());
        insert_paper(&pool, &a).await.unwrap();
        let mut b = sample_paper("01890000-0000-7000-8000-000000000007", "h7");
        b.meta.arxiv_id = Some("2001.00001".into());
        insert_paper(&pool, &b).await.unwrap();

        // Both identifiers match different rows: the DOI match wins.
        let hit = find_by_identifier(&pool, Some("10.1/x"), Some("2001.00001"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(hit.id, a.id);

        // An unmatched DOI still falls through to the arXiv match.
        let fallback = find_by_identifier(&pool, Some("10.9/other"), Some("2001.00001"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(fallback.id, b.id);
    }

    #[tokio::test]
    async fn restore_untrashes_only_trashed_rows() {
        let (_dir, pool) = temp_pool().await;
        let p = sample_paper("01890000-0000-7000-8000-000000000003", "h3");
        insert_paper(&pool, &p).await.unwrap();
        assert!(!restore(&pool, &p.id).await.unwrap()); // active: nothing to restore
        soft_delete(&pool, &p.id).await.unwrap();
        assert!(restore(&pool, &p.id).await.unwrap());
        assert!(!restore(&pool, &p.id).await.unwrap()); // idempotent: already active
        assert!(get_by_id(&pool, &p.id)
            .await
            .unwrap()
            .unwrap()
            .deleted_at
            .is_none());
        assert_eq!(
            list_papers(&pool, None, None, None, None)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn unique_violation_is_detected() {
        let (_dir, pool) = temp_pool().await;
        let a = sample_paper("01890000-0000-7000-8000-000000000004", "same");
        let b = sample_paper("01890000-0000-7000-8000-000000000005", "same");
        insert_paper(&pool, &a).await.unwrap();
        let err = insert_paper(&pool, &b).await.unwrap_err();
        assert!(is_unique_violation(&err));
        assert!(!is_unique_violation(&anyhow::anyhow!("something else")));
    }

    #[tokio::test]
    async fn project_crud_and_unique_name() {
        let (_dir, pool) = temp_pool().await;

        let p = create_project(&pool, "Survey", Some("draft"))
            .await
            .unwrap();
        assert_eq!(p.name, "Survey");

        // Case-insensitive unique name.
        let dup = create_project(&pool, "survey", None).await;
        assert!(dup.is_err());
        assert!(is_unique_violation(&dup.unwrap_err()));

        // List with counts (zero members yet).
        let list = list_projects(&pool).await.unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].project.id, p.id);
        assert_eq!(list[0].paper_count, 0);

        // Update name + note.
        assert!(update_project(&pool, &p.id, "Survey v2", Some("final"))
            .await
            .unwrap());
        let got = get_project(&pool, &p.id).await.unwrap().unwrap();
        assert_eq!(got.name, "Survey v2");

        // Delete.
        assert!(delete_project(&pool, &p.id).await.unwrap());
        assert!(get_project(&pool, &p.id).await.unwrap().is_none());
        assert!(!delete_project(&pool, &p.id).await.unwrap());
    }

    #[tokio::test]
    async fn membership_add_remove_and_filter_and_cascade() {
        let (_dir, pool) = temp_pool().await;
        insert_paper(
            &pool,
            &sample_paper("01890000-0000-7000-8000-0000000000a1", "ha"),
        )
        .await
        .unwrap();
        insert_paper(
            &pool,
            &sample_paper("01890000-0000-7000-8000-0000000000a2", "hb"),
        )
        .await
        .unwrap();
        let proj = create_project(&pool, "P", None).await.unwrap();

        // Add is idempotent.
        add_paper_to_project(&pool, "01890000-0000-7000-8000-0000000000a1", &proj.id)
            .await
            .unwrap();
        add_paper_to_project(&pool, "01890000-0000-7000-8000-0000000000a1", &proj.id)
            .await
            .unwrap();
        assert_eq!(
            project_ids_for_paper(&pool, "01890000-0000-7000-8000-0000000000a1")
                .await
                .unwrap(),
            vec![proj.id.clone()]
        );

        // Count reflects membership.
        assert_eq!(list_projects(&pool).await.unwrap()[0].paper_count, 1);

        // Soft-deleting a member drops the count (it matches the ?project= filter,
        // which hides trashed papers) while the membership row itself survives.
        soft_delete(&pool, "01890000-0000-7000-8000-0000000000a1")
            .await
            .unwrap();
        assert_eq!(list_projects(&pool).await.unwrap()[0].paper_count, 0);
        assert_eq!(
            project_ids_for_paper(&pool, "01890000-0000-7000-8000-0000000000a1")
                .await
                .unwrap(),
            vec![proj.id.clone()]
        );
        // Restoring it brings the count back.
        restore(&pool, "01890000-0000-7000-8000-0000000000a1")
            .await
            .unwrap();
        assert_eq!(list_projects(&pool).await.unwrap()[0].paper_count, 1);

        // Filter returns only members.
        let filtered = list_papers(&pool, None, None, None, Some(&proj.id))
            .await
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "01890000-0000-7000-8000-0000000000a1");

        // Remove.
        assert!(
            remove_paper_from_project(&pool, "01890000-0000-7000-8000-0000000000a1", &proj.id)
                .await
                .unwrap()
        );
        assert!(!remove_paper_from_project(
            &pool,
            "01890000-0000-7000-8000-0000000000a1",
            &proj.id
        )
        .await
        .unwrap());

        // FK cascade: hard-purging a paper drops its memberships.
        add_paper_to_project(&pool, "01890000-0000-7000-8000-0000000000a2", &proj.id)
            .await
            .unwrap();
        delete_row(&pool, "01890000-0000-7000-8000-0000000000a2")
            .await
            .unwrap();
        assert_eq!(list_projects(&pool).await.unwrap()[0].paper_count, 0);

        // FK cascade: deleting a project drops memberships (no orphan rows).
        add_paper_to_project(&pool, "01890000-0000-7000-8000-0000000000a1", &proj.id)
            .await
            .unwrap();
        delete_project(&pool, &proj.id).await.unwrap();
        assert!(
            project_ids_for_paper(&pool, "01890000-0000-7000-8000-0000000000a1")
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn find_one_project_by_name_then_prefix() {
        let (_dir, pool) = temp_pool().await;
        let p = create_project(&pool, "My Survey", None).await.unwrap();

        // Exact, case-insensitive name.
        assert_eq!(find_one_project(&pool, "my survey").await.unwrap().id, p.id);
        // Id prefix.
        assert_eq!(find_one_project(&pool, &p.id[..8]).await.unwrap().id, p.id);
        // Miss.
        assert!(find_one_project(&pool, "nope").await.is_err());
    }

    #[tokio::test]
    async fn star_toggles() {
        let (_dir, pool) = temp_pool().await;
        let pid = insert_test_paper(&pool).await;
        assert!(set_paper_starred(&pool, &pid, true).await.unwrap());
        let n: i64 = sqlx::query_scalar("SELECT starred FROM papers WHERE id = ?")
            .bind(&pid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
        set_paper_starred(&pool, &pid, false).await.unwrap();
        let n: i64 = sqlx::query_scalar("SELECT starred FROM papers WHERE id = ?")
            .bind(&pid)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn settings_set_get_delete_roundtrip() {
        let (_dir, pool) = temp_pool().await;
        assert_eq!(get_setting(&pool, "proxy_cookie").await.unwrap(), None);

        set_setting(&pool, "proxy_cookie", "ezproxy=abc")
            .await
            .unwrap();
        assert_eq!(
            get_setting(&pool, "proxy_cookie").await.unwrap().as_deref(),
            Some("ezproxy=abc")
        );

        // Upsert overwrites the value.
        set_setting(&pool, "proxy_cookie", "ezproxy=xyz")
            .await
            .unwrap();
        assert_eq!(
            get_setting(&pool, "proxy_cookie").await.unwrap().as_deref(),
            Some("ezproxy=xyz")
        );

        // updated_at is populated.
        assert!(setting_updated_at(&pool, "proxy_cookie")
            .await
            .unwrap()
            .is_some());

        delete_setting(&pool, "proxy_cookie").await.unwrap();
        assert_eq!(get_setting(&pool, "proxy_cookie").await.unwrap(), None);
        assert_eq!(
            setting_updated_at(&pool, "proxy_cookie").await.unwrap(),
            None
        );
    }

    /// Insert a minimal paper (mirroring `sample_paper`) and return its id.
    async fn insert_test_paper(pool: &SqlitePool) -> String {
        let id = uuid::Uuid::now_v7().to_string();
        let p = sample_paper(&id, &id);
        insert_paper(pool, &p).await.unwrap();
        id
    }

    #[test]
    fn normalize_trims_and_drops_empty_segments() {
        assert_eq!(
            normalize_tag_name(" security / fuzzing "),
            Some("security/fuzzing".into())
        );
        assert_eq!(normalize_tag_name("ml//llm/"), Some("ml/llm".into()));
        assert_eq!(normalize_tag_name("  "), None);
        assert_eq!(normalize_tag_name("//"), None);
    }

    #[tokio::test]
    async fn add_tag_creates_then_reuses_case_insensitively() {
        let (_dir, pool) = temp_pool().await;
        let pid = insert_test_paper(&pool).await;
        let a = add_paper_tag(&pool, &pid, "Security/Fuzzing")
            .await
            .unwrap();
        let b = add_paper_tag(&pool, &pid, "security/fuzzing")
            .await
            .unwrap();
        assert_eq!(a.id, b.id, "same tag reused case-insensitively");
        assert_eq!(list_tags_with_counts(&pool).await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn remove_last_membership_gcs_the_tag() {
        let (_dir, pool) = temp_pool().await;
        let pid = insert_test_paper(&pool).await;
        let t = add_paper_tag(&pool, &pid, "ml").await.unwrap();
        assert!(remove_paper_tag(&pool, &pid, &t.id).await.unwrap());
        assert!(
            find_tag_by_name(&pool, "ml").await.unwrap().is_none(),
            "orphan tag GC'd"
        );
    }

    #[tokio::test]
    async fn tag_counts_roll_up_children() {
        let (_dir, pool) = temp_pool().await;
        // One paper carries a child tag `security/fuzzing`; another carries the
        // bare parent `security`.
        let child_paper = insert_test_paper(&pool).await;
        let parent_paper = insert_test_paper(&pool).await;
        add_paper_tag(&pool, &child_paper, "security/fuzzing")
            .await
            .unwrap();
        add_paper_tag(&pool, &parent_paper, "security")
            .await
            .unwrap();

        let counts = list_tags_with_counts(&pool).await.unwrap();
        let by_name = |n: &str| counts.iter().find(|s| s.tag.name == n).unwrap().paper_count;
        // `security` rolls up itself + every `security/*` child paper.
        assert_eq!(by_name("security"), 2);
        // The leaf child is unaffected: its rollup equals its direct count.
        assert_eq!(by_name("security/fuzzing"), 1);
    }
}
