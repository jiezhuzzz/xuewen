//! Attach a paper's code repository: validate the URL, shallow-clone into
//! the agent workspace as `repo/`, pin the commit, and record the outcome
//! in `paper_code`. Clones are local-only and never redistributed.

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use tokio::process::Command;

/// Endpoint-level guard. Deliberately simple: https only, no embedded
/// credentials — this is a self-hosted personal tool, not a multi-tenant
/// service. (Tests hand `run_clone` file:// URLs directly, below this gate.)
pub fn validate_repo_url(url: &str) -> Result<(), String> {
    let u = url.trim();
    if u.len() > 2000 {
        return Err("the repo URL is implausibly long".into());
    }
    let Some(rest) = u.strip_prefix("https://") else {
        return Err("the repo URL must start with https://".into());
    };
    let authority = rest.split('/').next().unwrap_or("");
    if authority.is_empty() {
        return Err("the repo URL has no host".into());
    }
    if authority.contains('@') {
        return Err("the repo URL must not carry credentials".into());
    }
    Ok(())
}

/// Fire-and-forget background clone; the row is already 'cloning'.
pub fn spawn_clone(
    pool: SqlitePool,
    library_root: PathBuf,
    paper_id: String,
    repo_url: String,
    max_repo_mb: u64,
) {
    tokio::spawn(run_clone(
        pool,
        library_root,
        paper_id,
        repo_url,
        max_repo_mb,
    ));
}

/// The clone job body (awaitable directly in tests). Never panics; every
/// outcome lands in `paper_code.status`.
pub async fn run_clone(
    pool: SqlitePool,
    library_root: PathBuf,
    paper_id: String,
    repo_url: String,
    max_repo_mb: u64,
) {
    let fail = |e: String| {
        let pool = pool.clone();
        let paper_id = paper_id.clone();
        async move {
            if let Err(db) = crate::db::set_paper_code_error(&pool, &paper_id, &e).await {
                tracing::error!("paper_code error write failed: {db}");
            }
        }
    };

    let ws = super::workspace_dir(&library_root, &paper_id);
    let dst = ws.join("repo");
    let _ = tokio::fs::remove_dir_all(&dst).await; // re-attach replaces the checkout
    if let Err(e) = tokio::fs::create_dir_all(&ws).await {
        return fail(format!("could not create the workspace: {e}")).await;
    }

    let out = Command::new("git")
        .args(["clone", "--depth", "1", "--single-branch", &repo_url])
        .arg(&dst)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .await;
    let out = match out {
        Ok(o) => o,
        Err(e) => return fail(format!("could not run git: {e}")).await,
    };
    if !out.status.success() {
        let tail: String = String::from_utf8_lossy(&out.stderr)
            .chars()
            .take(300)
            .collect();
        return fail(format!("git clone failed: {tail}")).await;
    }

    let sha = Command::new("git")
        .args(["-C"])
        .arg(&dst)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .await;
    let sha = match sha {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => return fail("could not resolve the cloned commit".into()).await,
    };

    let join_result = {
        let dst = dst.clone();
        tokio::task::spawn_blocking(move || dir_size(&dst)).await
    };
    let size = match join_result {
        Ok(size) => size,
        Err(e) => {
            return fail(format!(
                "could not measure the cloned repository's size: {e}"
            ))
            .await;
        }
    };
    if size > max_repo_mb.saturating_mul(1024 * 1024) {
        let _ = tokio::fs::remove_dir_all(&dst).await;
        return fail(format!(
            "the repository is {} MB, over the {max_repo_mb} MB limit ([ai.agent].max_repo_mb)",
            size / (1024 * 1024)
        ))
        .await;
    }

    if let Err(e) = crate::db::set_paper_code_ready(&pool, &paper_id, &sha, size as i64).await {
        tracing::error!("paper_code ready write failed: {e}");
    }
}

/// Remove a paper's checkout directory (detach keeps paper.txt).
pub async fn remove_checkout(library_root: &Path, paper_id: &str) {
    let _ =
        tokio::fs::remove_dir_all(super::workspace_dir(library_root, paper_id).join("repo")).await;
}

fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if let Ok(md) = entry.metadata() {
                if md.is_dir() {
                    total += dir_size(&entry.path());
                } else {
                    total += md.len();
                }
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_rejects_non_https_and_credentials() {
        assert!(validate_repo_url("https://github.com/x/y").is_ok());
        assert!(validate_repo_url("http://github.com/x/y").is_err());
        assert!(validate_repo_url("git@github.com:x/y.git").is_err());
        assert!(validate_repo_url("https://user:pw@github.com/x/y").is_err());
        assert!(validate_repo_url("file:///etc").is_err());
    }

    /// Happy path against a local repo — git accepts file:// URLs for
    /// --depth clones, so this runs offline. Requires `git` (dev shell).
    #[tokio::test]
    async fn run_clone_pins_commit_and_reports_ready() {
        let src = tempfile::tempdir().unwrap();
        let ok = |st: std::process::ExitStatus| assert!(st.success());
        ok(std::process::Command::new("git")
            .args(["init", "-q"])
            .current_dir(src.path())
            .status()
            .unwrap());
        ok(std::process::Command::new("git")
            .args([
                "-c",
                "user.email=t@t",
                "-c",
                "user.name=t",
                "commit",
                "-q",
                "--allow-empty",
                "-m",
                "x",
            ])
            .current_dir(src.path())
            .status()
            .unwrap());

        let lib = tempfile::tempdir().unwrap();
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf',datetime('now'),'resolved')")
            .execute(&pool).await.unwrap();
        let url = format!("file://{}", src.path().display());
        crate::db::upsert_paper_code_cloning(&pool, "p1", &url)
            .await
            .unwrap();

        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            500,
        )
        .await;

        let c = crate::db::get_paper_code(&pool, "p1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(c.status, "ready", "error: {:?}", c.error);
        assert!(c.commit_sha.is_some());
        assert!(crate::agent::workspace_dir(lib.path(), "p1")
            .join("repo/.git")
            .exists());
    }

    #[tokio::test]
    async fn run_clone_reports_failure_as_error_status() {
        let lib = tempfile::tempdir().unwrap();
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf',datetime('now'),'resolved')")
            .execute(&pool).await.unwrap();
        let url = format!("file://{}/nonexistent-repo", std::env::temp_dir().display());
        crate::db::upsert_paper_code_cloning(&pool, "p1", &url)
            .await
            .unwrap();

        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            500,
        )
        .await;

        let c = crate::db::get_paper_code(&pool, "p1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(c.status, "error");
        assert!(c.error.is_some());
    }
}
