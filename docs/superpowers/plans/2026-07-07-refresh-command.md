# `xuewen refresh` Command Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `xuewen refresh [ID] [--all]` — re-resolve metadata for failed (`needs_review`) records and re-file **every** paper to its correct cite-key path, upgrading older `<hash>.pdf` files and previously-unresolved `_unsorted/` records in place.

**Architecture:** Factor the extract→identify→GROBID→resolve chain out of `pipeline::ingest_file` into a reusable `resolve_pdf`, and add an in-place `ResolvedFields::apply_to(&mut Paper)`. A new `src/refresh.rs` walks the library: it re-resolves the target subset from each stored PDF, recomputes each paper's cite-key path from its current metadata (collision set excluding self), moves the file when the path changes, and persists via a new `db::update_paper`. `content_hash` stays the dedup identity; only `rel_path`/`cite_key`/metadata change. Every paper is processed independently — one failure logs and never aborts the pass.

**Tech Stack:** Rust, tokio, sqlx (SQLite), clap (derive), anyhow, wiremock + printpdf (dev, integration tests).

**Environment:** `$IN_NIX_SHELL` is not set — run every cargo command through the flake dev shell with SEPARATE args: `nix develop -c cargo test` (NOT `nix develop -c 'cargo test ...'`, which fails). Commit with `git -c commit.gpgsign=false commit -m "..."` (SSH signing unavailable). Conventional Commits, scope required, types limited to feat/fix/docs/chore/ci. The tree is currently rustfmt-clean, so `cargo fmt` is a no-op.

**Spec:** `docs/superpowers/specs/2026-07-07-cite-key-naming-and-refresh-design.md` §7–§10 (this is "Plan B").

---

## File Structure

- **Modify** `src/db.rs` — add `update_paper`, `all_papers`, `find_by_id_prefix` (thin sqlx queries). `cite_keys_with_base` already exists with the `exclude_id` param refresh needs.
- **Modify** `src/models.rs` — add `Paper::authors_vec()` (parse the stored `authors` JSON back to `Vec<String>` for cite-key computation).
- **Modify** `src/pipeline.rs` — factor `resolve_pdf` + `ResolveInputs` out of `ingest_file`; add `ResolvedFields::apply_to(&mut Paper)` and `move_file(from, to)`. `ingest_file` behaviour is unchanged.
- **Create** `src/refresh.rs` — `RefreshTarget`, `RefreshSummary`, `run`, and internal `refresh_one`/`find_one`. One responsibility: a refresh pass over the library.
- **Modify** `src/lib.rs` — add `pub mod refresh;`.
- **Modify** `src/main.rs` — add the `Refresh { id, all }` subcommand and dispatch.
- **Create** `tests/refresh_test.rs` — the four spec §9 integration tests.

**Deviation from spec §7's "supporting functions" list:** the spec names `papers_by_status`, but the chosen design fetches `all_papers` and filters the re-resolve subset in memory (because the default target re-files **all** papers regardless of status), so `papers_by_status` is not needed and is omitted (YAGNI).

---

## Task 1: DB queries + `Paper::authors_vec`

**Files:**
- Modify: `src/db.rs`
- Modify: `src/models.rs`
- Test: unit tests in `src/db.rs`

- [ ] **Step 1: Write failing tests for the new queries**

In `src/db.rs`, inside the existing `#[cfg(test)] mod tests` block (which already has `sample_paper`, `temp_pool`, and imports `super::*` + `PaperStatus`), append:

```rust
    #[tokio::test]
    async fn update_paper_persists_changes() {
        let (_dir, pool) = temp_pool().await;
        let mut p = sample_paper("01890000-0000-7000-8000-0000000000c1", "h1");
        insert_paper(&pool, &p).await.unwrap();

        p.title = Some("New Title".into());
        p.rel_path = "he2016deep.pdf".into();
        p.cite_key = Some("he2016deep".into());
        p.status = PaperStatus::Resolved.as_str().to_string();
        update_paper(&pool, &p).await.unwrap();

        let got = get_by_id(&pool, &p.id).await.unwrap().unwrap();
        assert_eq!(got.title.as_deref(), Some("New Title"));
        assert_eq!(got.rel_path, "he2016deep.pdf");
        assert_eq!(got.cite_key.as_deref(), Some("he2016deep"));
        assert_eq!(got.status, "resolved");
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
        let both = find_by_id_prefix(&pool, "01890000")
            .await
            .unwrap();
        assert_eq!(both.len(), 2);
    }

    #[test]
    fn authors_vec_parses_and_defaults() {
        let mut p = sample_paper("01890000-0000-7000-8000-0000000000e5", "he");
        assert!(p.authors_vec().is_empty()); // None → empty
        p.authors = Some(r#"["Kaiming He","Xiangyu Zhang"]"#.into());
        assert_eq!(p.authors_vec(), vec!["Kaiming He", "Xiangyu Zhang"]);
        p.authors = Some("not json".into());
        assert!(p.authors_vec().is_empty()); // invalid → empty
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `nix develop -c cargo test --lib db::tests`
Expected: FAIL to compile — `cannot find function update_paper` / `all_papers` / `find_by_id_prefix`, and no method `authors_vec`.

- [ ] **Step 3: Add `Paper::authors_vec` in `src/models.rs`**

After the `Paper` struct definition (before the `#[cfg(test)]` block), add:

```rust
impl Paper {
    /// The stored `authors` JSON parsed back into a list (empty if absent or
    /// unparseable). Used to recompute the cite key during refresh.
    pub fn authors_vec(&self) -> Vec<String> {
        self.authors
            .as_deref()
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default()
    }
}
```

(`serde_json` is already a crate dependency; no import needed for the fully-qualified path.)

- [ ] **Step 4: Add the three queries in `src/db.rs`**

After `cite_keys_with_base` (before the `#[cfg(test)]` block), add:

```rust
/// Overwrite a paper's mutable columns by id (leaves id/content_hash/added_at).
pub async fn update_paper(pool: &SqlitePool, p: &Paper) -> Result<()> {
    sqlx::query(
        "UPDATE papers SET \
         rel_path = ?, title = ?, abstract = ?, authors = ?, venue = ?, year = ?, \
         doi = ?, arxiv_id = ?, dblp_key = ?, cite_key = ?, url = ?, source = ?, status = ? \
         WHERE id = ?",
    )
    .bind(&p.rel_path)
    .bind(&p.title)
    .bind(&p.abstract_text)
    .bind(&p.authors)
    .bind(&p.venue)
    .bind(p.year)
    .bind(&p.doi)
    .bind(&p.arxiv_id)
    .bind(&p.dblp_key)
    .bind(&p.cite_key)
    .bind(&p.url)
    .bind(&p.source)
    .bind(&p.status)
    .bind(&p.id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Every paper, oldest first.
pub async fn all_papers(pool: &SqlitePool) -> Result<Vec<Paper>> {
    let papers = sqlx::query_as::<_, Paper>("SELECT * FROM papers ORDER BY added_at")
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
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `nix develop -c cargo test --lib db::tests`
Expected: PASS (existing db tests + the 3 new ones).

- [ ] **Step 6: Lint + commit**

Run: `nix develop -c cargo clippy --all-targets -- -D warnings`
Expected: clean.

```bash
git add src/db.rs src/models.rs
git -c commit.gpgsign=false commit -m "feat(db): add update/all/find-by-prefix queries and authors_vec"
```

---

## Task 2: Factor `resolve_pdf` + add `apply_to`/`move_file` in `pipeline.rs`

**Files:**
- Modify: `src/pipeline.rs`

This is a refactor guarded by the existing `pipeline_test.rs`/`resolve_test.rs`/`watcher_test.rs` suites — `ingest_file` behaviour must not change.

- [ ] **Step 1: Add `ResolveInputs` + `resolve_pdf`**

In `src/pipeline.rs`, add this struct and function (place `ResolveInputs` just above `ingest_file`, and `resolve_pdf` just below `ingest_file`):

```rust
/// The raw inputs a resolution produces from a stored PDF, shared by ingest and
/// refresh. Consumed by `resolve_fields`.
pub(crate) struct ResolveInputs {
    pub(crate) ident: Identifier,
    pub(crate) provisional_title: Option<String>,
    pub(crate) extracted: Option<ResolvedMetadata>,
    pub(crate) resolution: Resolution,
}

/// Extract first-page text, identify a DOI/arXiv id, optionally enrich via GROBID
/// (title-only path), and resolve authoritative metadata. Degrades to
/// `Resolution::Unresolved` on any resolver/network failure — never aborts.
pub(crate) async fn resolve_pdf(
    path: &Path,
    resolver: &Resolver,
    grobid: Option<&Grobid>,
) -> Result<ResolveInputs> {
    // Extract first-page text (blocking IO off the async runtime) and identify.
    let text = {
        let p = path.to_path_buf();
        tokio::task::spawn_blocking(move || pdf::extract_text(&p, 1)).await??
    };
    let ident = identify::identify(&text);
    let provisional_title = identify::guess_title(&text);

    // For the title-only path, optionally use GROBID for a better header
    // (degrades to None on failure).
    let extracted: Option<ResolvedMetadata> = match (&ident, grobid) {
        (Identifier::None, Some(g)) => match g.extract_header(path).await {
            Ok(md) => md,
            Err(e) => {
                tracing::warn!("grobid extraction failed: {e}");
                None
            }
        },
        _ => None,
    };

    // Search query prefers the GROBID title, else the heuristic first line.
    let title_hint: Option<String> = extracted
        .as_ref()
        .and_then(|m| m.title.clone())
        .or_else(|| provisional_title.clone());

    let resolution = resolver.resolve(&ident, title_hint.as_deref()).await;
    Ok(ResolveInputs {
        ident,
        provisional_title,
        extracted,
        resolution,
    })
}
```

- [ ] **Step 2: Rewrite `ingest_file` steps 3–4 to use `resolve_pdf`**

In `ingest_file`, replace the current block from the `// 3. Extract first-page text and identify.` comment down through the `let fields = resolve_fields(...);` line (i.e. the old steps 3, 3a, 3b, 3c, and the first line of step 4) with:

```rust
    // 3. Extract, identify, optionally GROBID, and resolve (factored for reuse).
    let ResolveInputs {
        ident,
        provisional_title,
        extracted,
        resolution,
    } = resolve_pdf(&path, resolver, grobid).await?;

    // 4. Decide the stored fields, then the cite-key filename.
    let fields = resolve_fields(provisional_title, extracted, &ident, resolution);
```

Leave the rest of `ingest_file` (the `cite_key`/`rel_path` computation, filing, insert, move) exactly as-is. The `resolve_fields` call now uses `provisional_title` instead of the old `heuristic_title` binding — same value, just the destructured field name.

- [ ] **Step 3: Add `ResolvedFields::apply_to`**

Inside `impl ResolvedFields` (which already has `into_paper`), add:

```rust
    /// Overwrite an existing paper's metadata columns from a fresh resolution,
    /// leaving id/content_hash/rel_path/cite_key/added_at for the caller to manage.
    pub(crate) fn apply_to(self, paper: &mut Paper) {
        paper.authors = if self.authors.is_empty() {
            None
        } else {
            serde_json::to_string(&self.authors).ok()
        };
        paper.title = self.title;
        paper.abstract_text = self.abstract_text;
        paper.venue = self.venue;
        paper.year = self.year;
        paper.doi = self.doi;
        paper.arxiv_id = self.arxiv_id;
        paper.dblp_key = self.dblp_key;
        paper.url = self.url;
        paper.source = self.source;
        paper.status = self.status;
    }
```

- [ ] **Step 4: Add `move_file`**

Next to the existing `move_to` function, add:

```rust
/// Move `from` to the exact path `to` (renaming across directories), creating
/// parent directories, with a copy+remove fallback across filesystems.
pub(crate) fn move_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(from, to).is_err() {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)?;
    }
    Ok(())
}
```

- [ ] **Step 5: Build + run the full suite (regression net)**

Run: `nix develop -c cargo build` then `nix develop -c cargo test`
Expected: compiles; ALL existing tests still pass (ingest behaviour unchanged). `resolve_pdf`/`ResolveInputs` are used immediately by `ingest_file`, but `apply_to` and `move_file` are `pub(crate)` and stay unused until Task 3, so `cargo build` will emit **dead-code warnings** for them (and `cargo clippy -- -D warnings` would ERROR). This is expected at this checkpoint. **Do NOT run clippy here, do NOT add `#[allow(dead_code)]`, and do NOT delete `apply_to`/`move_file` — Task 3 consumes them and Task 3's clippy is the gate.** Just confirm the build succeeds and the full test suite is green.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline.rs
git -c commit.gpgsign=false commit -m "chore(pipeline): factor resolve_pdf + add apply_to/move_file for refresh"
```

---

## Task 3: `refresh` module + integration tests (a) and (b)

**Files:**
- Create: `src/refresh.rs`
- Modify: `src/lib.rs`
- Create: `tests/refresh_test.rs`

- [ ] **Step 1: Write the first failing integration test**

Create `tests/refresh_test.rs`:

```rust
mod common;

use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::db;
use xuewen::models::Paper;
use xuewen::refresh::{self, RefreshTarget};
use xuewen::resolve::Resolver;

const CROSSREF_FIXTURE: &str = include_str!("fixtures/crossref_kgat.json");

/// A minimal stored paper for seeding; callers set the fields they care about.
fn seed_paper(id: &str, hash: &str, rel_path: &str, status: &str) -> Paper {
    Paper {
        id: id.into(),
        content_hash: hash.into(),
        rel_path: rel_path.into(),
        title: None,
        abstract_text: None,
        authors: None,
        venue: None,
        year: None,
        doi: None,
        arxiv_id: None,
        dblp_key: None,
        cite_key: None,
        url: None,
        source: None,
        status: status.into(),
        added_at: "2026-07-07T00:00:00Z".into(),
    }
}

#[tokio::test]
async fn needs_review_reresolves_and_refiles() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let hash = "deadbeefhash";
    let unsorted = library.join(format!("_unsorted/{hash}.pdf"));
    std::fs::create_dir_all(unsorted.parent().unwrap()).unwrap();

    // The stored PDF carries the DOI so re-resolution can identify it.
    let doi = "10.1145/3292500.3330701";
    common::write_test_pdf(&unsorted, &["Some Header", &format!("https://doi.org/{doi}")]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let p = seed_paper(
        "01890000-0000-7000-8000-0000000000a1",
        hash,
        &format!("_unsorted/{hash}.pdf"),
        "needs_review",
    );
    db::insert_paper(&pool, &p).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let summary = refresh::run(&pool, &library, &resolver, None, RefreshTarget::NeedsReview)
        .await
        .unwrap();
    assert_eq!(summary.reresolved, 1);
    assert_eq!(summary.refiled, 1);

    let got = db::get_by_id(&pool, &p.id).await.unwrap().unwrap();
    assert_eq!(got.status, "resolved");
    assert_eq!(got.cite_key.as_deref(), Some("wang2019kgat"));
    assert_eq!(got.rel_path, "wang2019kgat.pdf");
    assert!(library.join("wang2019kgat.pdf").exists());
    assert!(!unsorted.exists());
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `nix develop -c cargo test --test refresh_test needs_review_reresolves_and_refiles`
Expected: FAIL to compile — `unresolved import xuewen::refresh` (module doesn't exist yet).

- [ ] **Step 3: Create the `refresh` module**

Add `pub mod refresh;` to `src/lib.rs` (keep the list alphabetical — after `pub mod pipeline;`).

Create `src/refresh.rs`:

```rust
use anyhow::{bail, Result};
use sqlx::SqlitePool;
use std::path::Path;

use crate::db;
use crate::models::{Paper, PaperStatus};
use crate::naming;
use crate::pipeline::{move_file, resolve_fields, resolve_pdf, ResolveInputs};
use crate::resolve::grobid::Grobid;
use crate::resolve::Resolver;

/// Which papers a refresh pass re-resolves. Every processed paper is re-filed
/// regardless; this only controls whose metadata is re-fetched.
pub enum RefreshTarget {
    /// Default: re-resolve `needs_review` records; re-file all papers.
    NeedsReview,
    /// Re-resolve (and re-file) every paper.
    All,
    /// Re-resolve (and re-file) the single paper with this id (exact or unique prefix).
    One(String),
}

/// Tally of a refresh pass, for CLI feedback.
#[derive(Debug, Default)]
pub struct RefreshSummary {
    pub processed: usize,
    pub reresolved: usize,
    pub refiled: usize,
}

/// Run one refresh pass over the library. Each paper is handled independently:
/// a per-paper failure is logged and never aborts the run.
pub async fn run(
    pool: &SqlitePool,
    library_root: &Path,
    resolver: &Resolver,
    grobid: Option<&Grobid>,
    target: RefreshTarget,
) -> Result<RefreshSummary> {
    let (papers, reresolve_all) = match target {
        RefreshTarget::NeedsReview => (db::all_papers(pool).await?, false),
        RefreshTarget::All => (db::all_papers(pool).await?, true),
        RefreshTarget::One(id) => (vec![find_one(pool, &id).await?], true),
    };

    let mut summary = RefreshSummary::default();
    for mut paper in papers {
        summary.processed += 1;
        let reresolve = reresolve_all || paper.status == PaperStatus::NeedsReview.as_str();
        match refresh_one(pool, library_root, resolver, grobid, &mut paper, reresolve).await {
            Ok(outcome) => {
                summary.reresolved += outcome.reresolved as usize;
                summary.refiled += outcome.refiled as usize;
            }
            Err(e) => tracing::warn!("refresh failed for {}: {e}", paper.id),
        }
    }
    Ok(summary)
}

#[derive(Default)]
struct OneOutcome {
    reresolved: bool,
    refiled: bool,
}

async fn refresh_one(
    pool: &SqlitePool,
    library_root: &Path,
    resolver: &Resolver,
    grobid: Option<&Grobid>,
    paper: &mut Paper,
    reresolve: bool,
) -> Result<OneOutcome> {
    let mut outcome = OneOutcome::default();
    let pdf = library_root.join(&paper.rel_path);
    if !pdf.exists() {
        tracing::warn!(
            "library file missing for {} ({}); skipping",
            paper.id,
            paper.rel_path
        );
        return Ok(outcome);
    }

    // Re-resolve metadata from the stored PDF (best-effort; keep old data on failure).
    if reresolve {
        match resolve_pdf(&pdf, resolver, grobid).await {
            Ok(inputs) => {
                let ResolveInputs {
                    ident,
                    provisional_title,
                    extracted,
                    resolution,
                } = inputs;
                let fields = resolve_fields(provisional_title, extracted, &ident, resolution);
                fields.apply_to(paper);
                outcome.reresolved = true;
            }
            Err(e) => tracing::warn!(
                "re-resolve failed for {}: {e}; keeping existing metadata",
                paper.id
            ),
        }
    }

    // Re-file: recompute the cite-key path from the paper's current metadata,
    // excluding this paper's own key from the collision set.
    let cite_key = match naming::cite_key_base(&paper.authors_vec(), paper.year, paper.title.as_deref())
    {
        Some(base) => {
            let taken = db::cite_keys_with_base(pool, &base, Some(&paper.id)).await?;
            Some(naming::disambiguate(&base, &taken))
        }
        None => None,
    };
    let new_rel = naming::library_rel_path(cite_key.as_deref(), &paper.content_hash);
    if new_rel != paper.rel_path {
        let to = library_root.join(&new_rel);
        match move_file(&pdf, &to) {
            Ok(()) => {
                paper.rel_path = new_rel;
                paper.cite_key = cite_key;
                outcome.refiled = true;
            }
            Err(e) => tracing::warn!(
                "re-file move failed for {}: {e}; leaving in place",
                paper.id
            ),
        }
    }

    db::update_paper(pool, paper).await?;
    Ok(outcome)
}

/// Resolve a paper by exact id, else by unique id prefix.
async fn find_one(pool: &SqlitePool, id: &str) -> Result<Paper> {
    if let Some(p) = db::get_by_id(pool, id).await? {
        return Ok(p);
    }
    let mut matches = db::find_by_id_prefix(pool, id).await?;
    match matches.len() {
        0 => bail!("no paper with id or prefix {id:?}"),
        1 => Ok(matches.pop().unwrap()),
        n => bail!("ambiguous id prefix {id:?} matches {n} papers"),
    }
}
```

- [ ] **Step 4: Run test (a) to verify it passes**

Run: `nix develop -c cargo test --test refresh_test needs_review_reresolves_and_refiles`
Expected: PASS.

- [ ] **Step 5: Add integration test (b) — resolved paper re-files without re-resolving**

Append to `tests/refresh_test.rs`:

```rust
#[tokio::test]
async fn resolved_paper_refiles_without_reresolving() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let hash = "abc123hash";
    let old = library.join(format!("{hash}.pdf"));
    // Content is irrelevant — a resolved paper is not re-resolved under the default target.
    common::write_test_pdf(&old, &["Whatever"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mut p = seed_paper(
        "01890000-0000-7000-8000-0000000000b2",
        hash,
        &format!("{hash}.pdf"),
        "resolved",
    );
    p.title = Some("Deep Residual Learning for Image Recognition".into());
    p.authors = Some(r#"["Kaiming He"]"#.into());
    p.year = Some(2016);
    p.source = Some("crossref".into());
    db::insert_paper(&pool, &p).await.unwrap();

    // Unreachable resolver: a resolved paper must NOT be re-resolved, so no HTTP happens.
    let resolver =
        Resolver::with_bases(None, "http://127.0.0.1:1".into(), "http://127.0.0.1:1".into()).unwrap();

    let summary = refresh::run(&pool, &library, &resolver, None, RefreshTarget::NeedsReview)
        .await
        .unwrap();
    assert_eq!(summary.reresolved, 0);
    assert_eq!(summary.refiled, 1);

    let got = db::get_by_id(&pool, &p.id).await.unwrap().unwrap();
    // Metadata unchanged; only the location moved to the cite-key path.
    assert_eq!(got.status, "resolved");
    assert_eq!(
        got.title.as_deref(),
        Some("Deep Residual Learning for Image Recognition")
    );
    assert_eq!(got.year, Some(2016));
    assert_eq!(got.cite_key.as_deref(), Some("he2016deep"));
    assert_eq!(got.rel_path, "he2016deep.pdf");
    assert!(library.join("he2016deep.pdf").exists());
    assert!(!old.exists());
}
```

- [ ] **Step 6: Run both tests + clippy**

Run: `nix develop -c cargo test --test refresh_test` then `nix develop -c cargo clippy --all-targets -- -D warnings`
Expected: both tests PASS; clippy clean (Task 2's `apply_to`/`move_file`/`resolve_pdf` are now used).

- [ ] **Step 7: Commit**

```bash
git add src/refresh.rs src/lib.rs tests/refresh_test.rs
git -c commit.gpgsign=false commit -m "feat(refresh): re-resolve and re-file library papers"
```

---

## Task 4: `refresh` CLI subcommand + integration tests (c) and (d)

**Files:**
- Modify: `src/main.rs`
- Test: `tests/refresh_test.rs`

- [ ] **Step 1: Add the `Refresh` subcommand**

In `src/main.rs`, add the import (next to the existing `use xuewen::pipeline::...`):

```rust
use xuewen::refresh::{self, RefreshTarget};
```

Add a variant to the `Command` enum (after `Watch`):

```rust
    /// Re-resolve failed records and re-file every paper to its cite-key path.
    Refresh {
        /// Paper id (exact or unique prefix) to refresh. Omit to refresh needs_review records.
        #[arg(conflicts_with = "all")]
        id: Option<String>,
        /// Re-resolve every paper, not just needs_review records.
        #[arg(long)]
        all: bool,
    },
```

Add a match arm in `main` (after the `Command::Watch` arm):

```rust
        Command::Refresh { id, all } => {
            let target = match (id, all) {
                (Some(id), _) => RefreshTarget::One(id),
                (None, true) => RefreshTarget::All,
                (None, false) => RefreshTarget::NeedsReview,
            };
            let summary =
                refresh::run(&pool, &dirs.library_root, &resolver, grobid.as_ref(), target).await?;
            println!(
                "refresh: {} processed, {} re-resolved, {} re-filed",
                summary.processed, summary.reresolved, summary.refiled
            );
        }
```

- [ ] **Step 2: Verify the CLI builds and wires up**

Run: `nix develop -c cargo run -- refresh --help`
Expected: help text for `refresh` listing the `[ID]` positional and `--all` flag. (It exits 0 without touching a DB because `--help` short-circuits before `Config::load`.)

Also verify the conflict guard: `nix develop -c cargo run -- refresh someid --all`
Expected: clap error "the argument '[ID]' cannot be used with '--all'" (non-zero exit). This proves `conflicts_with`.

- [ ] **Step 3: Add integration test (c) — `refresh <ID>` targets exactly one**

Append to `tests/refresh_test.rs`:

```rust
#[tokio::test]
async fn refresh_by_id_prefix_targets_one() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();

    // P1: targeted; its PDF carries a DOI so re-resolution succeeds.
    let h1 = "hash0001";
    let doi = "10.1145/3292500.3330701";
    let f1 = library.join(format!("{h1}.pdf"));
    common::write_test_pdf(&f1, &["Header", &format!("https://doi.org/{doi}")]);
    // P2: not targeted; must be untouched.
    let h2 = "hash0002";
    let f2 = library.join(format!("{h2}.pdf"));
    common::write_test_pdf(&f2, &["Other"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let p1 = seed_paper(
        "01890000-0000-7000-8000-0000000000a1",
        h1,
        &format!("{h1}.pdf"),
        "needs_review",
    );
    db::insert_paper(&pool, &p1).await.unwrap();
    let mut p2 = seed_paper(
        "01890000-0000-7000-8000-0000000000b2",
        h2,
        &format!("{h2}.pdf"),
        "resolved",
    );
    p2.title = Some("Some Resolved Paper".into());
    db::insert_paper(&pool, &p2).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    // A prefix unique to P1 (P2's id starts ...0000b2).
    let summary = refresh::run(
        &pool,
        &library,
        &resolver,
        None,
        RefreshTarget::One("01890000-0000-7000-8000-0000000000a".into()),
    )
    .await
    .unwrap();
    assert_eq!(summary.processed, 1);

    let got1 = db::get_by_id(&pool, &p1.id).await.unwrap().unwrap();
    assert_eq!(got1.rel_path, "wang2019kgat.pdf");
    assert_eq!(got1.status, "resolved");

    // P2 completely untouched.
    let got2 = db::get_by_id(&pool, &p2.id).await.unwrap().unwrap();
    assert_eq!(got2.rel_path, format!("{h2}.pdf"));
    assert_eq!(got2.cite_key, None);
    assert!(f2.exists());
}
```

- [ ] **Step 4: Add integration test (d) — `--all` re-resolves a resolved paper**

Append to `tests/refresh_test.rs`:

```rust
#[tokio::test]
async fn all_reresolves_resolved_paper() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();
    let hash = "stalehash1";
    let doi = "10.1145/3292500.3330701";
    // The paper currently lives at a stale cite-key path; put the real PDF there.
    let f = library.join("old2000stale.pdf");
    common::write_test_pdf(&f, &["Header", &format!("https://doi.org/{doi}")]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mut p = seed_paper(
        "01890000-0000-7000-8000-0000000000d4",
        hash,
        "old2000stale.pdf",
        "resolved",
    );
    p.title = Some("Old Stale Title".into());
    p.authors = Some(r#"["Old Author"]"#.into());
    p.year = Some(2000);
    p.cite_key = Some("old2000stale".into());
    db::insert_paper(&pool, &p).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let summary = refresh::run(&pool, &library, &resolver, None, RefreshTarget::All)
        .await
        .unwrap();
    assert_eq!(summary.reresolved, 1);

    let got = db::get_by_id(&pool, &p.id).await.unwrap().unwrap();
    // Stale metadata replaced by the freshly-resolved record, and re-filed.
    assert_eq!(
        got.title.as_deref(),
        Some("KGAT: Knowledge Graph Attention Network for Recommendation")
    );
    assert_eq!(got.year, Some(2019));
    assert_eq!(got.rel_path, "wang2019kgat.pdf");
    assert!(library.join("wang2019kgat.pdf").exists());
    assert!(!f.exists());
}
```

- [ ] **Step 5: Full verification**

Run: `nix develop -c cargo test` then `nix develop -c cargo clippy --all-targets -- -D warnings` then `nix develop -c cargo fmt -- --check`
Expected: entire suite PASS (4 refresh tests + all prior); clippy clean; fmt clean.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/refresh_test.rs
git -c commit.gpgsign=false commit -m "feat(refresh): add refresh CLI subcommand"
```

---

## Verification (Definition of Done)

- `nix develop -c cargo test` — whole suite green, including the four `refresh_test` integration tests and the new `db` unit tests.
- `nix develop -c cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check` — clean.
- `xuewen refresh --help` shows `[ID]` + `--all`; `refresh <id> --all` is a clap conflict error.
- `ingest_file` behaviour unchanged (existing pipeline/watcher tests still pass) after the `resolve_pdf` factoring.
- A `needs_review` record whose PDF now resolves moves from `_unsorted/<hash>.pdf` to `<citekey>.pdf` and flips to `resolved`; an old resolved `<hash>.pdf` re-files without re-resolving; `refresh <id>` touches exactly one; `--all` re-resolves even a resolved paper.

## Notes for the executor

- Cite keys the tests assert: Crossref KGAT fixture (authors `Xiang Wang`, `Xiangnan He`; year 2019; title "KGAT: …") → `wang2019kgat`; He/2016/"Deep Residual Learning …" → `he2016deep`. If either differs, STOP — a mismatch means the naming or fixture changed, not a test bug to paper over.
- `refresh` never re-hashes or dedups — it operates on the already-stored library copy in place. Do not add hashing.
- Graceful degradation is load-bearing: a `refresh_one` that can't read/resolve a PDF logs and continues; only genuine DB errors bubble up (and are caught per-paper in `run`). Do not turn any of these into hard aborts.
- Do not touch `grobid.rs`. Every commit uses `git -c commit.gpgsign=false`.
