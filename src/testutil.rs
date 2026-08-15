//! Shared test fixtures. Compiled only for `cargo test` (`#[cfg(test)]` at
//! the `lib.rs` declaration), so nothing here can leak into a release build.

use crate::models::{Authors, Paper, PaperMeta, PaperStatus};

/// Migrated pool with one seeded paper — the setup every subsystem whose
/// tables FK onto `papers(id)` (citations, annotations) needs. Returns the
/// `TempDir` so the caller's `let (pool, _dir)` keeps the database directory
/// alive for the test's duration and removes it afterwards. (Drop order —
/// dir before pool — is fine: SQLite on unix keeps working against an
/// unlinked file.)
pub(crate) async fn pool_with_paper(id: &str) -> (sqlx::SqlitePool, tempfile::TempDir) {
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
        starred: false,
        name: None,
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

    (pool, dir)
}
