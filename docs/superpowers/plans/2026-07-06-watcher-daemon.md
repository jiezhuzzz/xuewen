# Watcher Daemon Implementation Plan (Slice 1, Plan 3)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A `xuewen watch` daemon that watches the inbox directory and auto-ingests each new PDF — scanning for files already present at startup (catch-up), waiting for in-progress writes to finish (stability debounce), retrying transient failures with backoff, and quarantining files that repeatedly fail to `inbox/_failed/`.

**Architecture:** A `watcher` module. `run()` performs a catch-up scan, then sets up a `notify` filesystem watcher (non-recursive) feeding an unbounded channel; a single async consumer processes paths serially. Each path is stabilized (size polled until steady), then ingested via `pipeline::ingest_file` with bounded exponential-backoff retries; on final failure it is moved to `inbox/_failed/`. Processing serially + moving files out of the inbox on success makes duplicate events harmless.

**Tech Stack:** Adds `notify` (filesystem events); enables tokio `time` + `sync`. Reuses the whole ingest pipeline + resolver + optional GROBID from Plans 1/2.

---

## Plan set context

Slice 1 spec: `docs/superpowers/specs/2026-07-06-pdf-ingest-metadata-pipeline-design.md` (§4.1 Watcher, §7 error handling).
- Plans 1/2a/2b/2c (merged): ingest foundation + full metadata resolution (DOI/arXiv/DBLP/Crossref/optional GROBID).
- **Plan 3 (this file):** the watcher daemon — completes slice 1.

### Current state (on `main`)
- `xuewen::pipeline::{ingest_file(pool: &SqlitePool, dirs: &Libraries, resolver: &Resolver, grobid: Option<&Grobid>, path: &Path) -> Result<Outcome>, Libraries { library_root: PathBuf, processed_dir: PathBuf }, Outcome::{Ingested(String), Duplicate}}`. `ingest_file` moves the original to `processed_dir` on success/duplicate; on error it leaves the original in place (and cleans up any orphan library copy).
- `pipeline` has a private `fn move_to(src: &Path, dir: &Path) -> Result<()>` (create_dir_all + rename with copy+remove fallback).
- `xuewen::resolve::{Resolver, grobid::Grobid}`, `xuewen::db`, `xuewen::config::Config { inbox_dir, library_root, database_url, grobid_url, contact_email }`.
- `main.rs` builds `Config`, `pool`, `Resolver`, `Option<Grobid>`, `Libraries { library_root, processed_dir: inbox_dir.join("_processed") }`, and has a `Command::Ingest { path }`.
- Run cargo via `nix develop -c '<command>'`. tokio features: `["rt-multi-thread","macros","fs"]`.

## File structure

```
Cargo.toml                     # + notify; tokio + "time","sync"
src/
  watcher.rs                   # WatchConfig, catch_up_scan, stabilize, ingest_with_retry, process_one, run
  pipeline.rs                  # make `move_to` pub(crate) for reuse
  lib.rs                       # + pub mod watcher;
  main.rs                      # + Command::Watch -> watcher::run
tests/
  watcher_test.rs              # integration: catch-up + live-drop auto-ingest
```

**Module responsibility:** `watcher` owns filesystem watching, debounce, retry, and quarantine policy. It orchestrates `pipeline::ingest_file` but contains no bibliographic logic.

---

## Task 1: Dependencies

**Files:** Modify `Cargo.toml`.

- [ ] **Step 1: Edit `Cargo.toml`**

Add to `[dependencies]`:
```toml
notify = "6"
```
Change the `tokio` line to add `time` and `sync`:
```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "fs", "time", "sync"] }
```

- [ ] **Step 2: Build**

Run: `nix develop -c cargo build`
Expected: `notify` resolves and compiles; `Finished`. If `notify = "6"` fails to resolve, bump minimally and report.

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add notify; enable tokio time + sync features"
```

---

## Task 2: Watcher core (scan, stabilize, retry, process) + unit tests

**Files:** Create `src/watcher.rs`; modify `src/pipeline.rs` (make `move_to` reusable) and `src/lib.rs`.
**Test:** unit tests inside `src/watcher.rs`.

- [ ] **Step 1: Make `pipeline::move_to` reusable**

In `src/pipeline.rs`, change the helper's visibility from `fn move_to(` to:
```rust
pub(crate) fn move_to(src: &Path, dir: &Path) -> Result<()> {
```
(No other change to that function.)

- [ ] **Step 2: Create `src/watcher.rs`**

```rust
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::time::Duration;

use notify::{RecursiveMode, Watcher};
use sqlx::SqlitePool;

use crate::pipeline::{ingest_file, move_to, Libraries};
use crate::resolve::grobid::Grobid;
use crate::resolve::Resolver;

/// Tunables for the watch loop. `Default` is for production; tests use small values.
#[derive(Debug, Clone)]
pub struct WatchConfig {
    /// How long to wait between file-size polls when checking for stability.
    pub poll_interval: Duration,
    /// Number of consecutive unchanged polls required to consider a file stable.
    pub stable_polls: u32,
    /// Maximum number of polls before giving up waiting for stability.
    pub max_polls: u32,
    /// Maximum ingest attempts before quarantining.
    pub max_attempts: u32,
    /// Base delay for exponential backoff between ingest attempts.
    pub retry_base_delay: Duration,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_millis(400),
            stable_polls: 2,
            max_polls: 25,
            max_attempts: 3,
            retry_base_delay: Duration::from_secs(1),
        }
    }
}

fn is_pdf(p: &Path) -> bool {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("pdf"))
        .unwrap_or(false)
}

/// List the top-level `*.pdf` files currently in the inbox (non-recursive, so the
/// `_processed`/`_failed` subdirectories are skipped). Sorted for determinism.
pub fn catch_up_scan(inbox: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !inbox.exists() {
        return Ok(out);
    }
    for entry in std::fs::read_dir(inbox)? {
        let path = entry?.path();
        if path.is_file() && is_pdf(&path) {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// Wait until the file's size is stable (unchanged for `stable_polls` consecutive
/// polls). Returns `false` if the file vanished; on timeout returns `true` if the
/// file still exists (best effort). Guards against ingesting a half-written download.
async fn stabilize(path: &Path, cfg: &WatchConfig) -> bool {
    let mut last: Option<u64> = None;
    let mut steady = 0u32;
    for _ in 0..cfg.max_polls {
        let size = match tokio::fs::metadata(path).await {
            Ok(m) => m.len(),
            Err(_) => return false, // gone
        };
        if size > 0 && Some(size) == last {
            steady += 1;
            if steady >= cfg.stable_polls {
                return true;
            }
        } else {
            steady = 0;
            last = Some(size);
        }
        tokio::time::sleep(cfg.poll_interval).await;
    }
    tokio::fs::metadata(path).await.is_ok()
}

/// Ingest a file, retrying with exponential backoff. Re-ingestion is safe: the
/// content-hash dedup makes a retry after a partial failure return `Duplicate`.
async fn ingest_with_retry(
    pool: &SqlitePool,
    dirs: &Libraries,
    resolver: &Resolver,
    grobid: Option<&Grobid>,
    cfg: &WatchConfig,
    path: &Path,
) -> Result<crate::pipeline::Outcome> {
    let mut delay = cfg.retry_base_delay;
    let mut last_err = None;
    for attempt in 1..=cfg.max_attempts {
        match ingest_file(pool, dirs, resolver, grobid, path).await {
            Ok(o) => return Ok(o),
            Err(e) => {
                tracing::warn!("ingest attempt {attempt}/{} failed: {e}", cfg.max_attempts);
                last_err = Some(e);
                if attempt < cfg.max_attempts {
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
            }
        }
    }
    Err(last_err.expect("at least one attempt ran"))
}

/// Stabilize then ingest one path; on repeated failure, quarantine to `failed_dir`.
async fn process_one(
    pool: &SqlitePool,
    dirs: &Libraries,
    resolver: &Resolver,
    grobid: Option<&Grobid>,
    failed_dir: &Path,
    cfg: &WatchConfig,
    path: &Path,
) {
    if !stabilize(path, cfg).await {
        return; // file vanished (e.g. already processed by a prior event)
    }
    match ingest_with_retry(pool, dirs, resolver, grobid, cfg, path).await {
        Ok(outcome) => tracing::info!("ingested {}: {outcome:?}", path.display()),
        Err(e) => {
            tracing::error!("giving up on {}: {e}; quarantining to _failed", path.display());
            if let Err(mv) = move_to(path, failed_dir) {
                tracing::error!("could not quarantine {}: {mv}", path.display());
            }
        }
    }
}

/// Run the watch loop: catch up on existing files, then watch for new ones until
/// the process is stopped. Blocks (the `notify` watcher is held for the loop's life).
pub async fn run(
    pool: &SqlitePool,
    dirs: &Libraries,
    resolver: &Resolver,
    grobid: Option<&Grobid>,
    inbox: &Path,
) -> Result<()> {
    let cfg = WatchConfig::default();
    let failed_dir = inbox.join("_failed");

    // Catch-up: ingest anything already sitting in the inbox.
    for path in catch_up_scan(inbox)? {
        process_one(pool, dirs, resolver, grobid, &failed_dir, &cfg, &path).await;
    }

    // Live watch. `notify` calls the handler on its own thread; forward pdf paths
    // over an unbounded channel (send() is non-blocking and runtime-agnostic).
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<PathBuf>();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(event) = res {
            if matches!(event.kind, notify::EventKind::Create(_) | notify::EventKind::Modify(_)) {
                for p in event.paths {
                    if is_pdf(&p) {
                        let _ = tx.send(p);
                    }
                }
            }
        }
    })?;
    watcher.watch(inbox, RecursiveMode::NonRecursive)?;
    tracing::info!("watching {}", inbox.display());

    while let Some(path) = rx.recv().await {
        process_one(pool, dirs, resolver, grobid, &failed_dir, &cfg, &path).await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use printpdf::{BuiltinFont, Mm, PdfDocument};
    use std::fs::File;
    use std::io::{BufWriter, Write};

    fn write_pdf(path: &Path, line: &str) {
        let (doc, page1, layer1) = PdfDocument::new("t", Mm(210.0), Mm(297.0), "L1");
        let font = doc.add_builtin_font(BuiltinFont::Helvetica).unwrap();
        doc.get_page(page1)
            .get_layer(layer1)
            .use_text(line, 12.0, Mm(15.0), Mm(280.0), &font);
        doc.save(&mut BufWriter::new(File::create(path).unwrap())).unwrap();
    }

    fn fast_cfg() -> WatchConfig {
        WatchConfig {
            poll_interval: Duration::from_millis(5),
            stable_polls: 1,
            max_polls: 5,
            max_attempts: 2,
            retry_base_delay: Duration::from_millis(1),
        }
    }

    // A resolver whose upstreams refuse instantly -> every lookup degrades to
    // Unresolved without any real network wait.
    fn offline_resolver() -> Resolver {
        Resolver::with_bases(
            None,
            "http://127.0.0.1:1".to_string(),
            "http://127.0.0.1:1".to_string(),
        )
        .unwrap()
        .with_dblp_base("http://127.0.0.1:1".to_string())
    }

    async fn temp_pool(dir: &Path) -> SqlitePool {
        let url = format!("sqlite:{}", dir.join("library.db").display());
        crate::db::connect(&url).await.unwrap()
    }

    #[test]
    fn catch_up_scan_lists_top_level_pdfs_only() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path();
        File::create(inbox.join("b.pdf")).unwrap();
        File::create(inbox.join("a.pdf")).unwrap();
        File::create(inbox.join("notes.txt")).unwrap();
        std::fs::create_dir_all(inbox.join("_processed")).unwrap();
        File::create(inbox.join("_processed/old.pdf")).unwrap();

        let found = catch_up_scan(inbox).unwrap();
        assert_eq!(
            found,
            vec![inbox.join("a.pdf"), inbox.join("b.pdf")] // sorted, top-level only
        );
    }

    #[tokio::test]
    async fn stabilize_true_for_steady_file_false_for_missing() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("x.pdf");
        let mut fh = File::create(&f).unwrap();
        fh.write_all(b"some bytes").unwrap();
        fh.sync_all().unwrap();
        assert!(stabilize(&f, &fast_cfg()).await);
        assert!(!stabilize(&dir.path().join("missing.pdf"), &fast_cfg()).await);
    }

    #[tokio::test]
    async fn process_one_ingests_and_moves_to_processed() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        let pdf = inbox.join("paper.pdf");
        write_pdf(&pdf, "A Paper With No Identifier Here");

        let pool = temp_pool(dir.path()).await;
        let dirs = Libraries {
            library_root: dir.path().join("library"),
            processed_dir: inbox.join("_processed"),
        };
        let resolver = offline_resolver();

        process_one(&pool, &dirs, &resolver, None, &inbox.join("_failed"), &fast_cfg(), &pdf).await;

        assert!(inbox.join("_processed/paper.pdf").exists());
        assert!(!pdf.exists());
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM papers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 1);
    }

    #[tokio::test]
    async fn process_one_quarantines_unreadable_pdf() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("inbox");
        std::fs::create_dir_all(&inbox).unwrap();
        let bad = inbox.join("bad.pdf");
        std::fs::write(&bad, b"this is not a pdf").unwrap();

        let pool = temp_pool(dir.path()).await;
        let dirs = Libraries {
            library_root: dir.path().join("library"),
            processed_dir: inbox.join("_processed"),
        };
        let resolver = offline_resolver();
        let failed = inbox.join("_failed");

        process_one(&pool, &dirs, &resolver, None, &failed, &fast_cfg(), &bad).await;

        assert!(failed.join("bad.pdf").exists(), "should be quarantined");
        assert!(!bad.exists());
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM papers")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(n, 0);
    }
}
```

- [ ] **Step 3:** In `src/lib.rs` add `pub mod watcher;`.

- [ ] **Step 4: Run the tests**

Run: `nix develop -c cargo test watcher::tests`
Expected: `catch_up_scan_lists_top_level_pdfs_only`, `stabilize_true_for_steady_file_false_for_missing`, `process_one_ingests_and_moves_to_processed`, `process_one_quarantines_unreadable_pdf` PASS. (These require `pdftotext` from the dev shell.) If `notify`'s `recommended_watcher`/`Watcher::watch` API differs, this task doesn't call them in tests — the compile check does; adjust the `run` wiring minimally if the API changed and report.

- [ ] **Step 5: Commit**

```bash
git add src/watcher.rs src/pipeline.rs src/lib.rs
git commit -m "feat: watcher core (catch-up scan, stability debounce, retry, quarantine)"
```

---

## Task 3: `watch` CLI command + live-watch integration test

**Files:** Modify `src/main.rs`; create `tests/watcher_test.rs`.
**Test:** `tests/watcher_test.rs`.

- [ ] **Step 1: Add the `Watch` command to `src/main.rs`**

Add a `Watch` variant to the `Command` enum:
```rust
#[derive(Subcommand)]
enum Command {
    /// Ingest a single PDF file.
    Ingest { path: PathBuf },
    /// Watch the inbox directory and auto-ingest new PDFs (runs until stopped).
    Watch,
}
```

Add the match arm in `main` (the `resolver`, `grobid`, `pool`, `dirs` bindings already exist):
```rust
        Command::Watch => {
            xuewen::watcher::run(&pool, &dirs, &resolver, grobid.as_ref(), &cfg.inbox_dir).await?;
        }
```
(Ensure `cfg` is still in scope where the command is matched. If `main` currently consumes `cfg` fields before the match, keep `cfg` alive — e.g. do not move `cfg.inbox_dir` earlier; `dirs.processed_dir` was built from `cfg.inbox_dir.join("_processed")`, which clones, so `cfg` remains usable.)

- [ ] **Step 2: Build + manual sanity (optional)**

Run: `nix develop -c cargo build`
Expected: compiles. `nix develop -c cargo run -- watch --help` is implicit via the subcommand; `xuewen watch` would block watching (do not run it unattended in the plan).

- [ ] **Step 3: Create `tests/watcher_test.rs`**

```rust
mod common;

use std::time::Duration;

use xuewen::db;
use xuewen::pipeline::Libraries;
use xuewen::resolve::Resolver;

// Upstreams refuse instantly -> lookups degrade to Unresolved (offline, fast).
fn offline_resolver() -> Resolver {
    Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string())
}

async fn wait_for<F: Fn() -> bool>(cond: F, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    cond()
}

#[tokio::test]
async fn watcher_catches_up_and_watches_new_files() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();

    // One file present BEFORE the watcher starts (exercises catch-up)...
    common::write_test_pdf(&inbox.join("existing.pdf"), &["Existing Paper Title Here"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());

    // Move owned values into the watcher task (so it satisfies 'static).
    let inbox_for_task = inbox.clone();
    let db_url = url.clone();
    let handle = tokio::spawn(async move {
        let pool = db::connect(&db_url).await.unwrap();
        let dirs = Libraries {
            library_root: inbox_for_task.parent().unwrap().join("library"),
            processed_dir: inbox_for_task.join("_processed"),
        };
        let resolver = offline_resolver();
        let _ = xuewen::watcher::run(&pool, &dirs, &resolver, None, &inbox_for_task).await;
    });

    // Give the watcher time to finish catch-up and start watching, then drop a
    // second file (exercises the live notify path).
    tokio::time::sleep(Duration::from_millis(600)).await;
    common::write_test_pdf(&inbox.join("dropped.pdf"), &["Freshly Dropped Paper Title"]);

    let processed = inbox.join("_processed");
    let existing_ok = wait_for(|| processed.join("existing.pdf").exists(), Duration::from_secs(10)).await;
    let dropped_ok = wait_for(|| processed.join("dropped.pdf").exists(), Duration::from_secs(10)).await;

    handle.abort();

    assert!(existing_ok, "catch-up file was not ingested");
    assert!(dropped_ok, "live-dropped file was not ingested");

    // Both are recorded.
    let pool = db::connect(&url).await.unwrap();
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM papers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2);
}
```

- [ ] **Step 4: Run the tests + clippy**

Run: `nix develop -c cargo test --test watcher_test`
Expected: PASS. This is timing-dependent (filesystem events + polling); the 10s wait windows and 600ms warm-up are generous. If it is flaky in the dev environment, increase the warm-up/timeouts — but do NOT weaken the assertions.
Run: `nix develop -c cargo test` — expect the whole suite green (unit watcher tests + this integration test + all prior).
Run: `nix develop -c cargo clippy --all-targets 2>&1 | tail -20` — expect no new warnings.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs tests/watcher_test.rs
git commit -m "feat: xuewen watch daemon command + live-watch integration test"
```

---

## Definition of done (Plan 3)

- `xuewen watch` ingests every PDF already in the inbox at startup, then auto-ingests new PDFs as they appear.
- Half-written files are not ingested until their size is stable.
- Transient ingest failures are retried with backoff; files that keep failing are moved to `inbox/_failed/` (not reprocessed forever).
- Duplicate filesystem events are harmless (serial processing + the file is moved out of the inbox on success).
- `xuewen ingest <file>` (Plan 1) still works unchanged.

## Slice 1 complete

With this plan merged, slice 1 from the original request is delivered end-to-end: **a watch dir that, on a new PDF, extracts the title/identifier and resolves precise metadata (DBLP/Crossref/arXiv, optional GROBID) into a self-hosted store.** Natural follow-on slices (separate spec → plan cycles): a read/query API + web UI over the SQLite store; auto-relocation of PDFs into `<venue>/<year>/`; re-resolution of `needs_review` records; BibTeX export.
