//! Attach a paper's code repository: validate the URL, shallow-clone into
//! the agent workspace as `repo/`, pin the commit, and record the outcome
//! in `paper_code`. Clones are local-only and never redistributed.

use std::path::{Path, PathBuf};

use sqlx::SqlitePool;
use tokio::process::Command;

/// Public git forges permitted as clone targets out of the box. Self-hosted
/// forges are added via `[ai.agent].clone_allowed_hosts`.
const DEFAULT_ALLOWED_HOSTS: &[&str] =
    &["github.com", "gitlab.com", "bitbucket.org", "codeberg.org"];

/// Endpoint-level guard. https only, no embedded credentials, and the host must
/// be on an **allowlist** (built-in forges plus `extra_hosts` from config).
///
/// An allowlist — rather than a block-list of internal ranges — is what closes
/// clone SSRF here: the endpoint may be reachable remotely (`--allow-remote`),
/// and `git` resolves DNS itself, so a block-list plus name resolution is
/// bypassable via DNS rebinding. Only letting known hosts through sidesteps that
/// entirely. (Tests hand `run_clone` file:// URLs directly, below this gate.)
pub fn validate_repo_url(url: &str, extra_hosts: &[String]) -> Result<(), String> {
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
    let host = host_of(authority)
        .trim_end_matches('.')
        .to_ascii_lowercase();
    if !host_is_allowed(&host, extra_hosts) {
        return Err(format!(
            "the repo host '{host}' is not allowed; add it to [ai.agent].clone_allowed_hosts"
        ));
    }
    Ok(())
}

/// Extract the bare host from a URL authority, dropping any `:port` and the
/// brackets around an IPv6 literal (`[::1]:443` → `::1`).
fn host_of(authority: &str) -> &str {
    if let Some(rest) = authority.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        authority.split(':').next().unwrap_or("")
    }
}

/// Whether `host` equals, or is a subdomain of, any allowed host (built-in
/// forges plus the configured extras). `host` is assumed already lowercased.
fn host_is_allowed(host: &str, extra_hosts: &[String]) -> bool {
    DEFAULT_ALLOWED_HOSTS
        .iter()
        .map(|s| s.to_string())
        .chain(extra_hosts.iter().map(|s| s.trim().to_ascii_lowercase()))
        .filter(|allowed| !allowed.is_empty())
        .any(|allowed| host == allowed || host.ends_with(&format!(".{allowed}")))
}

/// Fire-and-forget background clone; the row is already 'cloning'. `clone_gen`
/// is the generation returned by `upsert_paper_code_cloning`, used to detect
/// supersession by a later attach.
pub fn spawn_clone(
    pool: SqlitePool,
    library_root: PathBuf,
    paper_id: String,
    repo_url: String,
    max_repo_mb: u64,
    clone_gen: i64,
) {
    tokio::spawn(run_clone(
        pool,
        library_root,
        paper_id,
        repo_url,
        max_repo_mb,
        clone_gen,
    ));
}

/// The clone job body (awaitable directly in tests). Never panics; every
/// outcome lands in `paper_code.status` — unless a later attach (a higher
/// `clone_gen`) has superseded this job, in which case its writes are dropped
/// and it cleans up only its own staging directory, never the live checkout.
pub async fn run_clone(
    pool: SqlitePool,
    library_root: PathBuf,
    paper_id: String,
    repo_url: String,
    max_repo_mb: u64,
    clone_gen: i64,
) {
    let fail = |e: String| {
        let pool = pool.clone();
        let paper_id = paper_id.clone();
        async move {
            // Ok(false) = a newer attach superseded this job; drop the outcome.
            if let Err(db) = crate::db::set_paper_code_error(&pool, &paper_id, &e, clone_gen).await {
                tracing::error!("paper_code error write failed: {db}");
            }
        }
    };

    let ws = super::workspace_dir(&library_root, &paper_id);
    let dst = ws.join("repo");
    // Clone into a generation-scoped staging dir so concurrent jobs (an old one
    // still running when a re-attach starts) never write the same path, then
    // swap it into place atomically once this job is confirmed current.
    let staging = ws.join(format!(".repo.cloning.{clone_gen}"));
    if let Err(e) = tokio::fs::create_dir_all(&ws).await {
        return fail(format!("could not create the workspace: {e}")).await;
    }
    let _ = tokio::fs::remove_dir_all(&staging).await; // any remnant of this gen

    let out = Command::new("git")
        .args(["clone", "--depth", "1", "--single-branch", &repo_url])
        .arg(&staging)
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
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return fail(format!("git clone failed: {tail}")).await;
    }

    let sha = Command::new("git")
        .args(["-C"])
        .arg(&staging)
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .await;
    let sha = match sha {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            return fail("could not resolve the cloned commit".into()).await;
        }
    };

    let join_result = {
        let staging = staging.clone();
        tokio::task::spawn_blocking(move || dir_size(&staging)).await
    };
    let size = match join_result {
        Ok(size) => size,
        Err(e) => {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            return fail(format!(
                "could not measure the cloned repository's size: {e}"
            ))
            .await;
        }
    };
    if size > max_repo_mb.saturating_mul(1024 * 1024) {
        let _ = tokio::fs::remove_dir_all(&staging).await;
        return fail(format!(
            "the repository is {} MB, over the {max_repo_mb} MB limit ([ai.agent].max_repo_mb)",
            size / (1024 * 1024)
        ))
        .await;
    }

    // Publish only if still the current generation. Checking before the swap
    // keeps a superseded job from clobbering the live checkout a newer job owns.
    match crate::db::current_clone_gen(&pool, &paper_id).await {
        Ok(Some(g)) if g == clone_gen => {}
        Ok(_) => {
            let _ = tokio::fs::remove_dir_all(&staging).await; // superseded
            return;
        }
        Err(e) => {
            let _ = tokio::fs::remove_dir_all(&staging).await;
            return fail(format!("could not check clone generation: {e}")).await;
        }
    }
    let _ = tokio::fs::remove_dir_all(&dst).await;
    if let Err(e) = tokio::fs::rename(&staging, &dst).await {
        return fail(format!("could not place the checkout: {e}")).await;
    }

    match crate::db::set_paper_code_ready(&pool, &paper_id, &sha, size as i64, clone_gen).await {
        Ok(true) => {}
        Ok(false) => {
            // Raced a newer attach between the generation check and here; the
            // newer job owns the row, so leave our just-placed checkout for it
            // to replace and do not report ready.
        }
        Err(e) => tracing::error!("paper_code ready write failed: {e}"),
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
        let none: &[String] = &[];
        assert!(validate_repo_url("https://github.com/x/y", none).is_ok());
        assert!(validate_repo_url("http://github.com/x/y", none).is_err());
        assert!(validate_repo_url("git@github.com:x/y.git", none).is_err());
        assert!(validate_repo_url("https://user:pw@github.com/x/y", none).is_err());
        assert!(validate_repo_url("file:///etc", none).is_err());
    }

    #[test]
    fn validate_allowlist_admits_forges_and_rejects_the_rest_ssrf() {
        let none: &[String] = &[];
        // Built-in public forges (and their subdomains) pass, port tolerated.
        for u in [
            "https://github.com/x/y",
            "https://gist.github.com/x/y",
            "https://gitlab.com/x/y.git",
            "https://bitbucket.org/x/y",
            "https://codeberg.org/x/y",
            "https://github.com:443/x/y",
        ] {
            assert!(validate_repo_url(u, none).is_ok(), "should allow {u}");
        }
        // Everything off the allowlist is refused — including internal targets
        // an SSRF probe would reach, and arbitrary public hosts.
        for u in [
            "https://localhost/x/y",
            "https://127.0.0.1/x/y",
            "https://169.254.169.254/latest/meta-data", // cloud metadata
            "https://[::1]/x/y",
            "https://gitea/x/y",
            "https://nas.local/x/y",
            "https://evil.example.com/x/y",
            "https://notgithub.com/x/y",
            "https://github.com.evil.com/x/y", // suffix-spoof attempt
        ] {
            assert!(validate_repo_url(u, none).is_err(), "should reject {u}");
        }
        // A configured self-hosted forge (and its subdomains) is admitted.
        let extra = vec!["git.example.com".to_string()];
        assert!(validate_repo_url("https://git.example.com/x/y", &extra).is_ok());
        assert!(validate_repo_url("https://ci.git.example.com/x/y", &extra).is_ok());
        assert!(validate_repo_url("https://git.example.com.evil.com/x/y", &extra).is_err());
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
        let gen = crate::db::upsert_paper_code_cloning(&pool, "p1", &url)
            .await
            .unwrap();

        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            500,
            gen,
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
        let gen = crate::db::upsert_paper_code_cloning(&pool, "p1", &url)
            .await
            .unwrap();

        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            500,
            gen,
        )
        .await;

        let c = crate::db::get_paper_code(&pool, "p1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(c.status, "error");
        assert!(c.error.is_some());
    }

    /// A stale clone (lower generation) must not overwrite the row a newer
    /// attach owns, and must not disturb the live checkout.
    #[tokio::test]
    async fn superseded_clone_does_not_overwrite_newer_status() {
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
        let url = format!("file://{}", src.path().display());

        let lib = tempfile::tempdir().unwrap();
        let pool = sqlx::SqlitePool::connect("sqlite::memory:").await.unwrap();
        sqlx::migrate!().run(&pool).await.unwrap();
        sqlx::query("INSERT INTO papers (id, content_hash, rel_path, added_at, status) VALUES ('p1','h','p.pdf',datetime('now'),'resolved')")
            .execute(&pool).await.unwrap();

        // gen 0 is captured, then a re-attach bumps the row to gen 1 before the
        // stale gen-0 job runs.
        let stale_gen = crate::db::upsert_paper_code_cloning(&pool, "p1", &url)
            .await
            .unwrap();
        let current_gen = crate::db::upsert_paper_code_cloning(&pool, "p1", &url)
            .await
            .unwrap();
        assert!(current_gen > stale_gen);

        run_clone(
            pool.clone(),
            lib.path().to_path_buf(),
            "p1".into(),
            url,
            500,
            stale_gen,
        )
        .await;

        // The stale job dropped its outcome: the row is still 'cloning' (gen 1),
        // not 'ready', and no checkout was published.
        let c = crate::db::get_paper_code(&pool, "p1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(c.status, "cloning");
        assert!(!crate::agent::workspace_dir(lib.path(), "p1")
            .join("repo")
            .exists());
    }
}
