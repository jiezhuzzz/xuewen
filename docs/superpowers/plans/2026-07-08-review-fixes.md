# Review-Fix Package Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the review-fix package: `PaperMeta`/`IngestCtx` refactor, same-work/in-trash ingest outcomes, `restore` command, crash-safe refresh re-filing, non-loopback serve guard, interactive retry policy, and minor cleanups.

**Architecture:** Three phases per the spec (`docs/superpowers/specs/2026-07-08-review-fixes-design.md`): behavior-preserving refactors first (existing tests are the safety net), then behavior fixes test-first, then cleanups. Rust backend (axum + sqlx/SQLite + tokio), Svelte 5 frontend.

**Tech Stack:** Rust (sqlx 0.8, axum 0.8, anyhow, tokio), wiremock/axum-test for tests, Svelte 5 + vitest.

**Conventions:** All commands run from the repo root. Rust tests: `cargo test` (nix devshell is already active via direnv). Frontend tests: `npm --prefix frontend test -- --run`. Commit after every task with the exact message given. Branch: `fix/review-fixes` (already created).

**A note on Phase 1 tasks:** they are refactors — no new behavior, no new tests. The "test" step is the full existing suite. When a task says "mechanical rule", apply it to *every* occurrence in the named files; the worked examples show the exact target form.

---

## Phase 1 — Structure

### Task 1: Type `PaperStatus` as a sqlx/serde enum

**Files:**
- Modify: `src/models.rs`
- Modify: `src/pipeline.rs`, `src/refresh.rs`, `src/web/dto.rs`, `src/web/api.rs`
- Modify (tests): `src/db.rs` (test mod), `tests/pipeline_test.rs`, `tests/refresh_test.rs`, `tests/web_test.rs`

- [ ] **Step 1: Change the enum derives in `src/models.rs`**

Replace the current `PaperStatus` definition with:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, sqlx::Type)]
#[sqlx(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum PaperStatus {
    Resolved,
    NeedsReview,
}
```

Keep the existing `impl PaperStatus { pub fn as_str(...) }` and its test unchanged. `sqlx::Type` on an enum encodes/decodes the variant name as TEXT using the rename rule, so the stored values stay `resolved` / `needs_review`; serde produces the same strings in JSON.

- [ ] **Step 2: Change `Paper.status` to the enum**

In `src/models.rs`, `Paper`: `pub status: String,` → `pub status: PaperStatus,`.
Delete nothing else; `authors_vec` stays for now.

- [ ] **Step 3: Update producers and comparisons**

- `src/pipeline.rs` — `ResolvedFields.status: String` → `pub status: PaperStatus`. In `resolve_fields`, the three arms become `status: PaperStatus::Resolved,` / `status: PaperStatus::NeedsReview,` (drop the `.as_str().to_string()` calls).
- `src/refresh.rs:56` — `paper.status == PaperStatus::NeedsReview.as_str()` → `paper.status == PaperStatus::NeedsReview`.
- `src/refresh.rs:107-108` — the downgrade check becomes:
  ```rust
  let would_downgrade = fields.status == PaperStatus::NeedsReview
      && paper.status == PaperStatus::Resolved;
  ```
- `src/web/dto.rs` — `PaperSummary.status: String` → `pub status: PaperStatus` (add `use crate::models::PaperStatus;`); `From` impl: `status: p.status,` (it's `Copy`). JSON output is unchanged.
- `src/web/api.rs` import handler fallback — `(serde_json::Value::Null, "needs_review".to_string())` → `(serde_json::Value::Null, crate::models::PaperStatus::NeedsReview)`. (`json!({... "status": status})` still serializes to the same string.)

- [ ] **Step 4: Update tests mechanically**

Mechanical rule: any `status: "x".to_string()`/`status: status.into()` construction becomes the enum; any `assert_eq!(<paper>.status, "x")` becomes enum equality. Specifically:

- `src/db.rs` tests: `sample_paper` already uses `PaperStatus::NeedsReview.as_str().to_string()` → now just `PaperStatus::NeedsReview`; same for the `Resolved` assignments; assertions like `assert_eq!(got.status, "needs_review")` → `assert_eq!(got.status, PaperStatus::NeedsReview)`.
- `tests/refresh_test.rs`: `seed_paper(..., status: &str)` → change the parameter to `status: PaperStatus` and the field to `status,`; call sites pass `PaperStatus::NeedsReview` / `PaperStatus::Resolved` (add `use xuewen::models::PaperStatus;`). Assertions `assert_eq!(got.status, "resolved")` → `assert_eq!(got.status, PaperStatus::Resolved)`.
- `tests/web_test.rs`: same change to its `paper(id, title, status)` helper. JSON assertions on response bodies (`body["status"] == "needs_review"`) stay as strings — do not change them.
- `tests/pipeline_test.rs`: assertions `assert_eq!(paper.status, "resolved")` → `assert_eq!(paper.status, PaperStatus::Resolved)` (add the import); the seed literal at ~line 419 uses `status: "resolved".to_string()` → `status: PaperStatus::Resolved`.

- [ ] **Step 5: Run the suite**

Run: `cargo test`
Expected: all tests pass.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "chore(models): type PaperStatus as a sqlx/serde enum"
```

---

### Task 2: `Authors` newtype for the JSON authors column

**Files:**
- Modify: `src/models.rs` (add `Authors`, change `Paper.authors`, delete `authors_vec`)
- Modify: `src/pipeline.rs`, `src/refresh.rs`, `src/web/dto.rs`
- Modify (tests): `src/db.rs`, `tests/pipeline_test.rs`, `tests/refresh_test.rs`, `tests/web_test.rs`

- [ ] **Step 1: Add the newtype with sqlx impls in `src/models.rs`**

```rust
/// Author list stored as a JSON array in a nullable TEXT column.
/// NULL ⇄ empty; unparseable stored JSON decodes to empty (matching the old
/// `authors_vec` behavior).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(transparent)]
pub struct Authors(pub Vec<String>);

impl sqlx::Type<sqlx::Sqlite> for Authors {
    fn type_info() -> sqlx::sqlite::SqliteTypeInfo {
        <String as sqlx::Type<sqlx::Sqlite>>::type_info()
    }
    fn compatible(ty: &sqlx::sqlite::SqliteTypeInfo) -> bool {
        <String as sqlx::Type<sqlx::Sqlite>>::compatible(ty)
    }
}

impl<'r> sqlx::Decode<'r, sqlx::Sqlite> for Authors {
    fn decode(
        value: sqlx::sqlite::SqliteValueRef<'r>,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let raw = <Option<&str> as sqlx::Decode<sqlx::Sqlite>>::decode(value)?;
        Ok(Authors(
            raw.and_then(|s| serde_json::from_str(s).ok())
                .unwrap_or_default(),
        ))
    }
}

impl<'q> sqlx::Encode<'q, sqlx::Sqlite> for Authors {
    fn encode_by_ref(
        &self,
        buf: &mut Vec<sqlx::sqlite::SqliteArgumentValue<'q>>,
    ) -> Result<sqlx::encode::IsNull, sqlx::error::BoxDynError> {
        if self.0.is_empty() {
            return Ok(sqlx::encode::IsNull::Yes);
        }
        let json = serde_json::to_string(&self.0)?;
        <String as sqlx::Encode<sqlx::Sqlite>>::encode(json, buf)
    }
}
```

Change `Paper.authors: Option<String>` → `pub authors: Authors,` and **delete** `Paper::authors_vec` (and its `use serde_json` if now unused — check).

- [ ] **Step 2: Update producers/consumers**

- `src/pipeline.rs` — `ResolvedFields.authors` stays `Vec<String>`. In `into_paper` and `apply_to`, delete the `serde_json::to_string` blocks; the field assignment becomes `authors: Authors(self.authors),` / `paper.authors = Authors(self.authors);` (add `Authors` to the models import).
- `src/refresh.rs:129` — `&paper.authors_vec()` → `&paper.authors.0`.
- `src/web/dto.rs` — `authors: p.authors_vec(),` → `authors: p.authors.0.clone(),`.

- [ ] **Step 3: Update tests mechanically**

- `src/db.rs` tests: `authors: None` in `sample_paper` → `authors: Authors::default()`; `p.authors = Some(r#"["Ada Lovelace"]"#.into())` → `p.authors = Authors(vec!["Ada Lovelace".into()])`; the round-trip assertion `got.authors.as_deref() == Some(r#"["Ada Lovelace"]"#)` → `got.authors == Authors(vec!["Ada Lovelace".into()])`. **Replace** the `authors_vec_parses_and_defaults` test with a DB round-trip test:

  ```rust
  #[tokio::test]
  async fn authors_roundtrip_null_json_and_garbage() {
      let (_dir, pool) = temp_pool().await;
      // Empty -> stored NULL -> decodes empty.
      let a = sample_paper("01890000-0000-7000-8000-0000000000e5", "he");
      insert_paper(&pool, &a).await.unwrap();
      assert!(get_by_id(&pool, &a.id).await.unwrap().unwrap().authors.0.is_empty());
      // Non-empty round-trips.
      let mut b = sample_paper("01890000-0000-7000-8000-0000000000e6", "hf");
      b.authors = Authors(vec!["Kaiming He".into(), "Xiangyu Zhang".into()]);
      insert_paper(&pool, &b).await.unwrap();
      assert_eq!(
          get_by_id(&pool, &b.id).await.unwrap().unwrap().authors.0,
          vec!["Kaiming He", "Xiangyu Zhang"]
      );
      // Garbage in the column decodes to empty (legacy tolerance).
      sqlx::query("UPDATE papers SET authors = 'not json' WHERE id = ?")
          .bind(&b.id).execute(&pool).await.unwrap();
      assert!(get_by_id(&pool, &b.id).await.unwrap().unwrap().authors.0.is_empty());
  }
  ```

- `tests/refresh_test.rs` / `tests/web_test.rs` / `tests/pipeline_test.rs`: `authors: None` in seed helpers → `authors: Authors::default()` (import it). Assertions like `paper.authors.as_deref().unwrap().contains("Xiang Wang")` → `paper.authors.0.iter().any(|a| a == "Xiang Wang")`.

- [ ] **Step 4: Run the suite**

Run: `cargo test`
Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore(models): add Authors newtype for the JSON authors column"
```

---

### Task 3: Extract `PaperMeta` and flatten it into `Paper`

**Files:**
- Modify: `src/models.rs`, `src/pipeline.rs`, `src/db.rs`, `src/refresh.rs`, `src/web/dto.rs`, `src/web/api.rs`
- Modify (tests): `src/db.rs` tests, `tests/pipeline_test.rs`, `tests/refresh_test.rs`, `tests/web_test.rs`

- [ ] **Step 1: Restructure `src/models.rs`**

```rust
/// The metadata block shared by resolution output and the stored record.
/// Column names match the `papers` table; flattened into `Paper` for sqlx/serde.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, sqlx::FromRow)]
pub struct PaperMeta {
    pub title: Option<String>,
    #[sqlx(rename = "abstract")]
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub authors: Authors,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
    pub status: PaperStatus,
}

/// A stored bibliographic record. Column names match `migrations/0001_init.sql`.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Paper {
    pub id: String,
    pub content_hash: String,
    pub rel_path: String,
    pub cite_key: Option<String>,
    pub added_at: String,
    pub deleted_at: Option<String>,
    #[sqlx(flatten)]
    #[serde(flatten)]
    pub meta: PaperMeta,
}
```

(`cite_key` deliberately stays on `Paper` — pipeline-managed naming state, not resolution output.)

- [ ] **Step 2: Replace `ResolvedFields` with `PaperMeta` in `src/pipeline.rs`**

- Delete the `ResolvedFields` struct. `resolve_fields(...) -> PaperMeta`; the three construction arms build `PaperMeta { ..., authors: Authors(md.authors), ... }` with identical field logic.
- Delete `ResolvedFields::apply_to`. Replace `impl ResolvedFields { into_paper }` with:

```rust
impl PaperMeta {
    /// Assemble a full `Paper` with a fresh id/timestamp and the given location.
    pub(crate) fn into_paper(
        self,
        content_hash: String,
        rel_path: String,
        cite_key: Option<String>,
    ) -> Paper {
        Paper {
            id: Uuid::now_v7().to_string(),
            content_hash,
            rel_path,
            cite_key,
            added_at: chrono::Utc::now().to_rfc3339(),
            deleted_at: None,
            meta: self,
        }
    }
}
```

- In `ingest_file`, `fields.authors`/`fields.year`/`fields.title` references for the cite key become `&fields.authors.0, fields.year, fields.title.as_deref()`.

- [ ] **Step 3: Update `src/refresh.rs`**

The apply site becomes an assignment:

```rust
let fields = resolve_fields(provisional_title, extracted, &ident, resolution);
let would_downgrade = fields.status == PaperStatus::NeedsReview
    && paper.meta.status == PaperStatus::Resolved;
if would_downgrade {
    tracing::warn!(/* unchanged message */);
} else {
    paper.meta = fields;
    outcome.reresolved = true;
}
```

Cite-key recompute: `naming::cite_key_base(&paper.meta.authors.0, paper.meta.year, paper.meta.title.as_deref())`.

- [ ] **Step 4: Update `src/db.rs` binds and `src/web` accessors**

Mechanical rule: any `p.<metafield>` → `p.meta.<metafield>` for the 11 meta fields, everywhere in `src/` — `insert_paper` and `update_paper` binds (`.bind(&p.meta.title)` … `.bind(p.meta.status)`; note `status` is `Copy`, bind by value or ref both fine), `dto.rs` `From` impls, `api.rs` (`p.title`, `p.status` in the import handler → `p.meta.title`, `p.meta.status`), `main.rs` (`paper.title` in delete → `paper.meta.title`). The SQL strings themselves are unchanged.

- [ ] **Step 5: Update test constructors**

Mechanical rule: `Paper { ... }` literals become nested. Worked example — new `sample_paper` in `src/db.rs` tests:

```rust
fn sample_paper(id: &str, hash: &str) -> Paper {
    Paper {
        id: id.to_string(),
        content_hash: hash.to_string(),
        rel_path: format!("{hash}.pdf"),
        cite_key: None,
        added_at: "2026-07-06T00:00:00Z".to_string(),
        deleted_at: None,
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
```

Apply the same shape to `seed_paper` in `tests/refresh_test.rs`, `paper()` in `tests/web_test.rs`, and the seed literal in `tests/pipeline_test.rs`. Field accesses in test assertions follow the same `p.meta.` rule (e.g. `got.meta.title`, `got.meta.status`, `paper.meta.doi`); `id`/`content_hash`/`rel_path`/`cite_key`/`added_at`/`deleted_at` accesses are unchanged.

- [ ] **Step 6: Run the suite**

Run: `cargo test`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "chore(models): extract PaperMeta and flatten it into Paper"
```

---

### Task 4: Introduce `IngestCtx`

**Files:**
- Modify: `src/pipeline.rs`, `src/watcher.rs`, `src/refresh.rs`, `src/web/mod.rs`, `src/web/api.rs`, `src/main.rs`
- Modify (tests): `src/watcher.rs` tests, `tests/pipeline_test.rs`, `tests/refresh_test.rs`, `tests/watcher_test.rs`, `tests/web_test.rs`, `tests/grobid_test.rs` (if it calls `ingest_file`)

- [ ] **Step 1: Add the context struct and method-ize the pipeline**

In `src/pipeline.rs`:

```rust
/// Everything the ingest/refresh pipeline needs; built once in `main`.
pub struct IngestCtx {
    pub pool: SqlitePool,
    pub dirs: Libraries,
    pub resolver: Resolver,
    pub grobid: Option<Grobid>,
}

impl IngestCtx {
    /// Ingest a single PDF: hash, dedup, extract, identify, file, and store.
    pub async fn ingest_file(&self, path: &Path) -> Result<Outcome> {
        // body of the old free fn, with pool/dirs/resolver/grobid
        // read from self (`&self.pool`, `&self.dirs`, `&self.resolver`,
        // `self.grobid.as_ref()`)
    }

    /// (old free fn `resolve_pdf`, same substitutions)
    pub(crate) async fn resolve_pdf(&self, path: &Path) -> Result<ResolveInputs> { ... }
}
```

Delete the free `ingest_file` / `resolve_pdf` functions.

- [ ] **Step 2: Thread it through watcher and refresh**

- `src/watcher.rs`: `run(ctx: &IngestCtx, inbox: &Path)`; `process_one(ctx, failed_dir, cfg, path)`; `ingest_with_retry(ctx, cfg, path)` — each replaces the `(pool, dirs, resolver, grobid)` params with `ctx` and calls `ctx.ingest_file(path)`.
- `src/refresh.rs`: `run(ctx: &IngestCtx, target: RefreshTarget)`; `refresh_one(ctx, paper, reresolve)`; use `&ctx.pool`, `ctx.dirs.library_root`, `ctx.resolve_pdf(&pdf)`.

- [ ] **Step 3: Rebase `web::Ingest` on it**

`src/web/mod.rs`:

```rust
/// Everything the web import handler needs to run the ingest pipeline.
pub struct Ingest {
    pub ctx: IngestCtx,
    /// Where uploaded bytes are written before ingest (`inbox_dir/_uploads`).
    pub staging_dir: PathBuf,
}
```

`src/web/api.rs` import handler: the `ingest_file(...)` call becomes `ingest.ctx.ingest_file(&staged).await` (drop the now-unused `use crate::pipeline::ingest_file` import; keep `Outcome`).

- [ ] **Step 4: Rebuild `main.rs` wiring**

```rust
let ctx = xuewen::pipeline::IngestCtx {
    pool: pool.clone(),
    dirs,
    resolver,
    grobid,
};

match cli.command {
    Command::Ingest { path } => match ctx.ingest_file(&path).await? { ... },
    Command::Watch => xuewen::watcher::run(&ctx, &cfg.inbox_dir).await?,
    Command::Refresh { id, all } => {
        // target match unchanged
        let summary = refresh::run(&ctx, target).await?;
        ...
    }
    Command::Serve { host, port } => {
        let ingest = std::sync::Arc::new(web::Ingest {
            ctx,
            staging_dir: cfg.inbox_dir.join("_uploads"),
        });
        web::serve(&host, port, pool, cfg.library_root.clone(), ingest).await?;
    }
    // Delete/Purge unchanged (use `pool` directly)
}
```

- [ ] **Step 5: Update tests**

Mechanical rule: wherever a test built `dirs`+`resolver` and called `ingest_file(&pool, &dirs, &resolver, g, &p)` or `refresh::run(&pool, &library, &resolver, g, target)`, build a ctx instead. Worked example (`tests/pipeline_test.rs`):

```rust
use xuewen::pipeline::{IngestCtx, Libraries, Outcome};
...
let ctx = IngestCtx {
    pool: pool.clone(),
    dirs: Libraries { library_root: library.clone(), processed_dir: processed.clone() },
    resolver,
    grobid: None, // or Some(grobid) in the two GROBID tests
};
let out = ctx.ingest_file(&pdf_path).await.unwrap();
```

`tests/refresh_test.rs`: `refresh::run(&ctx, RefreshTarget::…)` with a ctx whose `dirs.processed_dir` can be any temp path (refresh never uses it — use `dir.path().join("_processed")`). `tests/web_test.rs`: the `Ingest { resolver, grobid, dirs, staging_dir }` literals become `Ingest { ctx: IngestCtx { pool: pool.clone(), dirs: …, resolver, grobid: None }, staging_dir: … }`. `src/watcher.rs` unit tests: build a ctx in place of the loose args.

- [ ] **Step 6: Run the suite**

Run: `cargo test`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add -A
git commit -m "chore(pipeline): introduce IngestCtx and method-ize the pipeline"
```

---

### Task 5: Drop `Resolution` and dead helpers

**Files:**
- Modify: `src/resolve/mod.rs`, `src/pipeline.rs`, `src/matching.rs`
- Modify (tests): `tests/resolve_test.rs`

- [ ] **Step 1: Return `Option<ResolvedMetadata>` from `resolve`**

In `src/resolve/mod.rs`: delete the `Resolution` enum, its `#[allow(clippy::large_enum_variant)]` and the stale `build_paper` comment. `resolve` becomes:

```rust
/// Route an identifier to its source and return the metadata, or `None` when
/// nothing resolves confidently. For a PDF with no identifier, `title_hint`
/// drives a DBLP/Crossref title search.
pub async fn resolve(
    &self,
    ident: &Identifier,
    title_hint: Option<&str>,
) -> Option<ResolvedMetadata> {
    match ident {
        Identifier::Arxiv(id) => self.try_arxiv(id).await,
        Identifier::Doi(doi) => self.try_crossref(doi).await,
        Identifier::None => self.try_title_search(title_hint).await,
    }
}
```

Also delete `ResolvedMetadata::authors_json` and its `authors_json_roundtrip` test.

- [ ] **Step 2: Update the pipeline**

`src/pipeline.rs`: `ResolveInputs.resolution: Option<ResolvedMetadata>`; `resolve_fields(..., resolution: Option<ResolvedMetadata>)` with arms `Some(md) => { ... }` / `None => match extracted { ... }` (same bodies as the old `Resolved`/`Unresolved` arms). Remove `Resolution` from imports.

- [ ] **Step 3: Delete `is_confident_match`**

In `src/matching.rs`, delete the function; rewrite its two tests against the primitive:

```rust
#[test]
fn identical_titles_clear_the_threshold() {
    let q = "KGAT: Knowledge Graph Attention Network for Recommendation";
    let c = "KGAT: Knowledge Graph Attention Network for Recommendation.";
    assert!(title_similarity(q, c) >= MATCH_THRESHOLD);
}

#[test]
fn unrelated_titles_fall_below_the_threshold() {
    assert!(
        title_similarity(
            "Deep Residual Learning for Image Recognition",
            "Attention Is All You Need"
        ) < MATCH_THRESHOLD
    );
}
```

- [ ] **Step 4: Update `tests/resolve_test.rs`**

Mechanical rule: `Resolution::Resolved(md) => …` match arms become `Some(md) => …`; `Resolution::Unresolved => panic!(…)` becomes `None => panic!(…)`; `assert_eq!(res, Resolution::Unresolved)` becomes `assert_eq!(res, None)`. Drop the `Resolution` import.

- [ ] **Step 5: Run the suite, then commit**

Run: `cargo test` — all pass. Then:

```bash
git add -A
git commit -m "chore(resolve): return Option from resolve and drop dead helpers"
```

---

## Phase 2 — Behavior fixes

### Task 6: DB queries — `find_by_hash`, `find_by_identifier`, `restore`, `is_unique_violation`

**Files:**
- Modify: `src/db.rs` (+ its test mod), `src/pipeline.rs` (one call site)

- [ ] **Step 1: Write the failing tests** (append to `src/db.rs` `mod tests`)

```rust
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
        find_by_identifier(&pool, Some("10.1/x"), None).await.unwrap().unwrap().id,
        p.id
    );
    assert_eq!(
        find_by_identifier(&pool, None, Some("2001.00001")).await.unwrap().unwrap().id,
        p.id
    );
    assert!(find_by_identifier(&pool, Some("10.9/other"), None).await.unwrap().is_none());
    assert!(find_by_identifier(&pool, None, None).await.unwrap().is_none());
}

#[tokio::test]
async fn restore_untrashes_only_trashed_rows() {
    let (_dir, pool) = temp_pool().await;
    let p = sample_paper("01890000-0000-7000-8000-000000000003", "h3");
    insert_paper(&pool, &p).await.unwrap();
    assert!(!restore(&pool, &p.id).await.unwrap()); // active: nothing to restore
    soft_delete(&pool, &p.id).await.unwrap();
    assert!(restore(&pool, &p.id).await.unwrap());
    assert!(get_by_id(&pool, &p.id).await.unwrap().unwrap().deleted_at.is_none());
    assert_eq!(list_papers(&pool, None, None, None).await.unwrap().len(), 1);
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
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p xuewen --lib db::tests`
Expected: compile errors — `find_by_hash`, `find_by_identifier`, `restore`, `is_unique_violation` not found.

- [ ] **Step 3: Implement in `src/db.rs`**

Replace `exists_by_hash` with:

```rust
/// The paper (active or trashed) whose stored bytes match `content_hash`.
pub async fn find_by_hash(pool: &SqlitePool, content_hash: &str) -> Result<Option<Paper>> {
    let p = sqlx::query_as::<_, Paper>("SELECT * FROM papers WHERE content_hash = ?")
        .bind(content_hash)
        .fetch_optional(pool)
        .await?;
    Ok(p)
}

/// The paper (active or trashed) already holding `doi` or `arxiv_id`.
pub async fn find_by_identifier(
    pool: &SqlitePool,
    doi: Option<&str>,
    arxiv_id: Option<&str>,
) -> Result<Option<Paper>> {
    if doi.is_none() && arxiv_id.is_none() {
        return Ok(None);
    }
    let p = sqlx::query_as::<_, Paper>(
        "SELECT * FROM papers \
         WHERE (?1 IS NOT NULL AND doi = ?1) OR (?2 IS NOT NULL AND arxiv_id = ?2) \
         LIMIT 1",
    )
    .bind(doi)
    .bind(arxiv_id)
    .fetch_optional(pool)
    .await?;
    Ok(p)
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
```

Update the one caller: in `src/pipeline.rs` the dedup check becomes
`if db::find_by_hash(&self.pool, &content_hash).await?.is_some() { ... }` (full mapping lands in Task 7). Update the `exists_by_hash` assertions in the `insert_then_fetch_and_dedup` test to `find_by_hash(...).await.unwrap().is_some()` / `.is_none()`.

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: all pass, including the four new ones.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat(db): collision lookups, restore, and unique-violation detection"
```

---

### Task 7: `SameWork`/`InTrash` outcomes in the pipeline

**Files:**
- Modify: `src/pipeline.rs`
- Test: `tests/pipeline_test.rs`

- [ ] **Step 1: Write/adjust the tests first**

In `tests/pipeline_test.rs`:

(a) **Rewrite** `same_doi_different_bytes_errors_without_orphan` as:

```rust
#[tokio::test]
async fn same_doi_different_bytes_reports_same_work() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    let doi_line = "https://doi.org/10.1000/xyz123";
    let a = inbox.join("a.pdf");
    let b = inbox.join("b.pdf");
    common::write_test_pdf(&a, &["Paper A Title", doi_line]);
    common::write_test_pdf(&b, &["Paper B Different Title", doi_line]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mock = MockServer::start().await;
    let ctx = IngestCtx {
        pool: pool.clone(),
        dirs: Libraries { library_root: library.clone(), processed_dir: processed.clone() },
        resolver: Resolver::with_bases(None, mock.uri(), mock.uri()).unwrap(),
        grobid: None,
    };

    let id_a = match ctx.ingest_file(&a).await.unwrap() {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };

    // Same DOI, different bytes → reported as the same work; file archived.
    let out = ctx.ingest_file(&b).await.unwrap();
    assert_eq!(out, Outcome::SameWork(id_a));
    assert_eq!(std::fs::read_dir(library.join("_unsorted")).unwrap().count(), 1);
    assert!(!b.exists());
    assert!(processed.join("b.pdf").exists());
}
```

(b) **Rewrite** `reingesting_a_trashed_paper_is_still_duplicate` as `reingesting_a_trashed_paper_reports_in_trash` — identical setup, final assertions become:

```rust
    let out2 = ctx.ingest_file(&again).await.unwrap();
    assert_eq!(out2, Outcome::InTrash(id));
```

(c) **Add** the trashed same-DOI variant (full test):

```rust
#[tokio::test]
async fn same_doi_of_trashed_paper_reports_in_trash() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    let doi_line = "https://doi.org/10.1000/xyz123";
    let a = inbox.join("a.pdf");
    let b = inbox.join("b.pdf");
    common::write_test_pdf(&a, &["Paper A Title", doi_line]);
    common::write_test_pdf(&b, &["Paper B Different Title", doi_line]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mock = MockServer::start().await;
    let ctx = IngestCtx {
        pool: pool.clone(),
        dirs: Libraries { library_root: library.clone(), processed_dir: processed.clone() },
        resolver: Resolver::with_bases(None, mock.uri(), mock.uri()).unwrap(),
        grobid: None,
    };

    let id_a = match ctx.ingest_file(&a).await.unwrap() {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };
    db::soft_delete(&pool, &id_a).await.unwrap();

    let out = ctx.ingest_file(&b).await.unwrap();
    assert_eq!(out, Outcome::InTrash(id_a));
}
```

(d) Mechanical rule: every `match out { Outcome::Ingested(id) => id, Outcome::Duplicate => panic!(…) }` in this file gains a catch-all arm instead: `other => panic!("expected Ingested, got {other:?}")` (the enum is about to grow).

- [ ] **Step 2: Run to verify failures**

Run: `cargo test --test pipeline_test`
Expected: compile error — `Outcome` has no `SameWork`/`InTrash` variants.

- [ ] **Step 3: Implement in `src/pipeline.rs`**

Extend the enum:

```rust
#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ingested(String),  // new paper id
    Duplicate,         // same bytes as an active paper
    SameWork(String),  // same DOI/arXiv id as an active paper → its id
    InTrash(String),   // same bytes or identifier as a trashed paper → its id
}
```

Hash-dedup step in `ingest_file`:

```rust
// 2. Dedup by content (active → Duplicate, trashed → InTrash).
if let Some(existing) = db::find_by_hash(&self.pool, &content_hash).await? {
    move_to(&path, &self.dirs.processed_dir)?;
    return Ok(if existing.deleted_at.is_some() {
        Outcome::InTrash(existing.id)
    } else {
        Outcome::Duplicate
    });
}
```

Identifier-dedup step, inserted between "decide the stored fields" and the cite-key computation:

```rust
// 4b. A different file of a work we already have (same DOI/arXiv id)?
if let Some(existing) =
    db::find_by_identifier(&self.pool, fields.doi.as_deref(), fields.arxiv_id.as_deref())
        .await?
{
    move_to(&path, &self.dirs.processed_dir)?;
    return Ok(if existing.deleted_at.is_some() {
        Outcome::InTrash(existing.id)
    } else {
        Outcome::SameWork(existing.id)
    });
}
```

- [ ] **Step 4: Run the full suite**

Run: `cargo test`
Expected: all pass (web/watcher/refresh untouched — the enum grew but their matches use the existing variants; the watcher logs `{outcome:?}`).

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "fix(pipeline): report same-work and in-trash instead of failing ingest"
```

---

### Task 8: Map insert-race UNIQUE violations to collision outcomes

**Files:**
- Modify: `src/pipeline.rs` (+ new `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing unit test** (new `mod tests` at the bottom of `src/pipeline.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::{Authors, Paper, PaperMeta, PaperStatus};

    fn paper(id: &str, hash: &str, doi: Option<&str>) -> Paper {
        Paper {
            id: id.into(),
            content_hash: hash.into(),
            rel_path: format!("{hash}.pdf"),
            cite_key: None,
            added_at: "2026-07-08T00:00:00Z".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some("T".into()),
                abstract_text: None,
                authors: Authors::default(),
                venue: None,
                year: None,
                doi: doi.map(str::to_string),
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::NeedsReview,
            },
        }
    }

    #[tokio::test]
    async fn recover_unique_collision_maps_all_cases() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let pool = db::connect(&url).await.unwrap();
        let a = paper("01890000-0000-7000-8000-0000000000aa", "h1", Some("10.1/x"));
        db::insert_paper(&pool, &a).await.unwrap();

        // Hash collision with an active row → Duplicate.
        assert_eq!(
            recover_unique_collision(&pool, "h1", None, None).await.unwrap(),
            Some(Outcome::Duplicate)
        );
        // Identifier collision with an active row → SameWork.
        assert_eq!(
            recover_unique_collision(&pool, "h2", Some("10.1/x"), None).await.unwrap(),
            Some(Outcome::SameWork(a.id.clone()))
        );
        // Trashed row → InTrash for both shapes.
        db::soft_delete(&pool, &a.id).await.unwrap();
        assert_eq!(
            recover_unique_collision(&pool, "h1", None, None).await.unwrap(),
            Some(Outcome::InTrash(a.id.clone()))
        );
        assert_eq!(
            recover_unique_collision(&pool, "h2", Some("10.1/x"), None).await.unwrap(),
            Some(Outcome::InTrash(a.id.clone()))
        );
        // No matching row → None (the violation was something else).
        assert_eq!(
            recover_unique_collision(&pool, "h3", Some("10.9/none"), None).await.unwrap(),
            None
        );
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --lib pipeline::tests`
Expected: compile error — `recover_unique_collision` not found.

- [ ] **Step 3: Implement**

In `src/pipeline.rs`:

```rust
/// After a UNIQUE violation on insert, find the row that won the race and map
/// it to the outcome the pre-insert checks would have produced.
pub(crate) async fn recover_unique_collision(
    pool: &SqlitePool,
    content_hash: &str,
    doi: Option<&str>,
    arxiv_id: Option<&str>,
) -> Result<Option<Outcome>> {
    if let Some(existing) = db::find_by_hash(pool, content_hash).await? {
        return Ok(Some(if existing.deleted_at.is_some() {
            Outcome::InTrash(existing.id)
        } else {
            Outcome::Duplicate
        }));
    }
    if let Some(existing) = db::find_by_identifier(pool, doi, arxiv_id).await? {
        return Ok(Some(if existing.deleted_at.is_some() {
            Outcome::InTrash(existing.id)
        } else {
            Outcome::SameWork(existing.id)
        }));
    }
    Ok(None)
}
```

And wire it into `ingest_file`'s insert-error branch:

```rust
if let Err(e) = db::insert_paper(&self.pool, &paper).await {
    let _ = std::fs::remove_file(&dest);
    // Lost a race with a concurrent ingest of the same work? Report the
    // winner's outcome instead of surfacing a constraint error.
    if db::is_unique_violation(&e) {
        if let Some(outcome) = recover_unique_collision(
            &self.pool,
            &paper.content_hash,
            paper.meta.doi.as_deref(),
            paper.meta.arxiv_id.as_deref(),
        )
        .await?
        {
            move_to(&path, &self.dirs.processed_dir)?;
            return Ok(outcome);
        }
    }
    return Err(e);
}
```

- [ ] **Step 4: Run the suite, then commit**

Run: `cargo test` — all pass.

```bash
git add -A
git commit -m "fix(pipeline): map unique-violation races to collision outcomes"
```

---

### Task 9: CLI — `restore` command and collision messages

**Files:**
- Modify: `src/main.rs`

(`main.rs` is untested wiring by convention in this codebase; `db::restore` was tested in Task 6.)

- [ ] **Step 1: Print the new outcomes**

In the `Ingest` arm:

```rust
Command::Ingest { path } => match ctx.ingest_file(&path).await? {
    Outcome::Ingested(id) => println!("ingested {id}"),
    Outcome::Duplicate => println!("duplicate, skipped"),
    Outcome::SameWork(id) => {
        let label = db::get_by_id(&pool, &id)
            .await?
            .and_then(|p| p.cite_key)
            .unwrap_or_else(|| id.clone());
        println!("already in library as {label} ({id})");
    }
    Outcome::InTrash(id) => println!("in trash — run: xuewen restore {id}"),
},
```

- [ ] **Step 2: Add the subcommand**

```rust
/// Restore a trashed paper back into the library.
Restore {
    /// Paper id (exact or unique prefix).
    id: String,
},
```

and its arm:

```rust
Command::Restore { id } => {
    let paper = db::find_one(&pool, &id).await?;
    if paper.deleted_at.is_none() {
        anyhow::bail!("{} is not in the trash", paper.id);
    }
    db::restore(&pool, &paper.id).await?;
    println!("restored {}", paper.id);
}
```

- [ ] **Step 3: Build + full suite**

Run: `cargo test`
Expected: compiles, all pass. Sanity: `cargo run -- --help` lists `restore`.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "feat(cli): restore command and collision outcome messages"
```

---

### Task 10: Web import — `same_work` / `in_trash` responses

**Files:**
- Modify: `src/web/api.rs`
- Test: `tests/web_test.rs`

- [ ] **Step 1: Write the failing tests** (append to `tests/web_test.rs`; reuse the file's existing imports/helpers — `IngestCtx` import added in Task 4)

```rust
#[tokio::test]
async fn import_reports_in_trash_for_deleted_paper() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());
    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: inbox.join("_processed"),
            },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server =
        TestServer::new(build_router_with_ingest(pool.clone(), library.clone(), ingest)).unwrap();

    let pdf_path = dir.path().join("paper.pdf");
    common::write_test_pdf(&pdf_path, &["A Paper With No Identifier"]);
    let pdf_bytes = std::fs::read(&pdf_path).unwrap();

    let form = MultipartForm::new()
        .add_part("file", Part::bytes(pdf_bytes.clone()).file_name("paper.pdf"));
    let body: serde_json::Value = server.post("/api/papers").multipart(form).await.json();
    assert_eq!(body["outcome"], "ingested");
    let id = body["id"].as_str().unwrap().to_string();

    db::soft_delete(&pool, &id).await.unwrap();

    // Re-upload the same bytes → in_trash with the trashed paper's id.
    let form2 =
        MultipartForm::new().add_part("file", Part::bytes(pdf_bytes).file_name("paper.pdf"));
    let body2: serde_json::Value = server.post("/api/papers").multipart(form2).await.json();
    assert_eq!(body2["outcome"], "in_trash");
    assert_eq!(body2["id"], serde_json::json!(id));
}

#[tokio::test]
async fn import_reports_same_work_for_known_doi() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Seed an active paper that already owns this DOI.
    let mut existing = paper("01890000-0000-7000-8000-0000000000aa", "Seed", PaperStatus::Resolved);
    existing.meta.doi = Some("10.1000/xyz123".into());
    db::insert_paper(&pool, &existing).await.unwrap();

    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());
    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: inbox.join("_processed"),
            },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server = TestServer::new(build_router_with_ingest(pool, library.clone(), ingest)).unwrap();

    // Upload a different file whose first page carries the same DOI. The
    // resolver is offline, but the extracted identifier still lands in
    // fields.doi, so the identifier dedup fires.
    let pdf_path = dir.path().join("other.pdf");
    common::write_test_pdf(
        &pdf_path,
        &["A Different Upload", "https://doi.org/10.1000/xyz123"],
    );
    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(std::fs::read(&pdf_path).unwrap()).file_name("other.pdf"),
    );
    let body: serde_json::Value = server.post("/api/papers").multipart(form).await.json();
    assert_eq!(body["outcome"], "same_work");
    assert_eq!(body["id"], serde_json::json!(existing.id));
}
```

(If `tests/web_test.rs`'s `paper()` helper signature differs after Task 3, adapt the seed call to it — the helper takes `(id, title, status)` and the test then sets `existing.meta.doi`.)

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test web_test`
Expected: the two new tests fail — the handler currently has no `SameWork`/`InTrash` arms, so those outcomes fall into… (nothing: non-exhaustive match is a compile error). Expected concretely: compile error on the `match` in `import_paper`.

- [ ] **Step 3: Implement in `src/web/api.rs`**

Add arms to the `ingest_file` result match:

```rust
Ok(Outcome::SameWork(id)) => {
    Json(serde_json::json!({"outcome": "same_work", "id": id})).into_response()
}
Ok(Outcome::InTrash(id)) => {
    Json(serde_json::json!({"outcome": "in_trash", "id": id})).into_response()
}
```

- [ ] **Step 4: Run tests, commit**

Run: `cargo test` — all pass.

```bash
git add -A
git commit -m "fix(web-import): surface same_work and in_trash outcomes"
```

---

### Task 11: Frontend — show same-work / in-trash results

**Files:**
- Modify: `frontend/src/lib/types.ts`, `frontend/src/lib/state.svelte.ts`, `frontend/src/components/ImportModal.svelte`
- Test: `frontend/src/components/ImportModal.test.ts`

- [ ] **Step 1: Write the failing test** (append inside the `describe('enqueueFiles', …)` block)

```ts
it('records same_work and in_trash outcomes', async () => {
  stubFetch((name) =>
    name === 'dup.pdf' ? { outcome: 'same_work', id: 'x1' } : { outcome: 'in_trash', id: 'x2' },
  );

  await enqueueFiles([pdf('dup.pdf'), pdf('trashed.pdf')]);

  expect(importState.items.map((i) => i.status)).toEqual(['same-work', 'in-trash']);
});
```

- [ ] **Step 2: Run to verify failure**

Run: `npm --prefix frontend test -- --run`
Expected: the new test fails (statuses come back as `ingested` — the else-branch swallows unknown outcomes).

- [ ] **Step 3: Implement**

`frontend/src/lib/types.ts`:

```ts
export type ImportResult =
  | { outcome: 'ingested'; id: string; title: string | null; status: string }
  | { outcome: 'duplicate' }
  | { outcome: 'same_work'; id: string }
  | { outcome: 'in_trash'; id: string };
```

`frontend/src/lib/state.svelte.ts` — `ImportItem.status` union gains `'same-work' | 'in-trash'`:

```ts
export interface ImportItem {
  name: string;
  status: 'queued' | 'importing' | 'ingested' | 'duplicate' | 'same-work' | 'in-trash' | 'failed';
  message?: string;
  needsReview?: boolean;
}
```

and the drain-loop mapping becomes:

```ts
if (res.outcome === 'duplicate') {
  importState.items[job.index].status = 'duplicate';
} else if (res.outcome === 'same_work') {
  importState.items[job.index].status = 'same-work';
} else if (res.outcome === 'in_trash') {
  importState.items[job.index].status = 'in-trash';
} else {
  importState.items[job.index].status = 'ingested';
  importState.items[job.index].message = res.title ?? '(untitled)';
  importState.items[job.index].needsReview = res.status === 'needs_review';
}
```

`frontend/src/components/ImportModal.svelte` — treat the two new statuses like `duplicate` visually (Copy icon, muted label) and count them as skipped:

- Icon block: extend the `duplicate` branch to `{:else if item.status === 'duplicate' || item.status === 'same-work' || item.status === 'in-trash'}`.
- Right-hand label block: inside the non-ingested `<span>`, extend the chain:

```svelte
{#if item.status === 'duplicate'}duplicate
{:else if item.status === 'same-work'}already in library
{:else if item.status === 'in-trash'}in trash — restore via CLI
{:else if item.status === 'failed'}{item.message}
{:else if item.status === 'importing'}importing…
{:else}queued{/if}
```

- Summary derived counter becomes:

```ts
const summary = $derived.by(() => {
  const c = { ingested: 0, skipped: 0, failed: 0 };
  for (const i of importState.items) {
    if (i.status === 'ingested') c.ingested++;
    else if (i.status === 'duplicate' || i.status === 'same-work' || i.status === 'in-trash')
      c.skipped++;
    else if (i.status === 'failed') c.failed++;
  }
  return c;
});
```

and the footer line: `{summary.ingested} ingested, {summary.skipped} skipped, {summary.failed} failed`.

- [ ] **Step 4: Run frontend tests + build**

Run: `npm --prefix frontend test -- --run` — all pass.
Run: `npm --prefix frontend run build` — succeeds (keeps `frontend/dist` fresh for rust-embed).

- [ ] **Step 5: Commit**

```bash
git add frontend/src
git commit -m "fix(web-import): show same-work and in-trash results in the modal"
```

---

### Task 12: Crash-safe refresh re-filing

**Files:**
- Modify: `src/refresh.rs`, `src/pipeline.rs` (swap `move_file` for `copy_to`)
- Test: `tests/refresh_test.rs`

- [ ] **Step 1: Write the failing test** (append to `tests/refresh_test.rs`)

```rust
#[tokio::test]
async fn refile_copy_failure_keeps_db_and_file_consistent() {
    let dir = tempfile::tempdir().unwrap();
    let library = dir.path().join("library");
    let hash = "copyfailhash";
    let unsorted = library.join(format!("_unsorted/{hash}.pdf"));
    std::fs::create_dir_all(unsorted.parent().unwrap()).unwrap();
    let doi = "10.1145/3292500.3330701";
    common::write_test_pdf(&unsorted, &["Header", &format!("https://doi.org/{doi}")]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let p = seed_paper(
        "01890000-0000-7000-8000-0000000000c9",
        hash,
        &format!("_unsorted/{hash}.pdf"),
        PaperStatus::NeedsReview,
    );
    db::insert_paper(&pool, &p).await.unwrap();

    // Make the re-file destination impossible: a DIRECTORY occupies the
    // target path "wang2019kgat.pdf", so the copy must fail.
    std::fs::create_dir_all(library.join("wang2019kgat.pdf")).unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let ctx = IngestCtx {
        pool: pool.clone(),
        dirs: Libraries {
            library_root: library.clone(),
            processed_dir: dir.path().join("_processed"),
        },
        resolver: Resolver::with_bases(None, server.uri(), server.uri()).unwrap(),
        grobid: None,
    };

    let summary = refresh::run(&ctx, RefreshTarget::NeedsReview).await.unwrap();
    assert_eq!(summary.reresolved, 1);
    assert_eq!(summary.refiled, 0); // copy failed → not refiled

    // DB still points at the ORIGINAL path, and that file still exists:
    // metadata updated, location untouched, nothing orphaned.
    let got = db::get_by_id(&pool, &p.id).await.unwrap().unwrap();
    assert_eq!(got.meta.status, PaperStatus::Resolved);
    assert_eq!(got.rel_path, format!("_unsorted/{hash}.pdf"));
    assert!(unsorted.exists());
}
```

- [ ] **Step 2: Run to verify current behavior**

Run: `cargo test --test refresh_test refile_copy_failure`
Expected: PASS against the current code — this is a **pinning test**, not a red test. The actual bug (DB failure after a successful move orphaning the file) needs fault injection to reproduce, which this codebase doesn't do; the fix is verified by the copy→update→remove structure in Step 3 while this pin proves the copy-failure path stays consistent through the restructure.

- [ ] **Step 3: Restructure `refresh_one`**

In `src/pipeline.rs`, replace `move_file` with:

```rust
/// Copy `from` to the exact path `to`, creating parent directories.
pub(crate) fn copy_to(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to)?;
    Ok(())
}
```

(delete `move_file`; refresh was its only caller).

In `src/refresh.rs`, the re-file + persist block at the end of `refresh_one` becomes:

```rust
    // Re-file: copy first, persist the row second, remove the old file last —
    // a failure at any step never leaves the DB pointing at a missing file.
    let cite_key =
        match naming::cite_key_base(&paper.meta.authors.0, paper.meta.year, paper.meta.title.as_deref()) {
            Some(base) => {
                let taken = db::cite_keys_with_base(&ctx.pool, &base, Some(&paper.id)).await?;
                Some(naming::disambiguate(&base, &taken))
            }
            None => None,
        };
    let new_rel = naming::library_rel_path(cite_key.as_deref(), &paper.content_hash);
    let mut refiled_paths: Option<(std::path::PathBuf, std::path::PathBuf)> = None; // (old, new)
    if new_rel != paper.rel_path {
        let to = ctx.dirs.library_root.join(&new_rel);
        match copy_to(&pdf, &to) {
            Ok(()) => {
                refiled_paths = Some((pdf.clone(), to));
                paper.rel_path = new_rel;
                paper.cite_key = cite_key;
                outcome.refiled = true;
            }
            Err(e) => {
                tracing::warn!("re-file copy failed for {}: {e}; leaving in place", paper.id)
            }
        }
    }

    if let Err(e) = db::update_paper(&ctx.pool, paper).await {
        // Roll the copy back so filesystem and DB stay consistent.
        if let Some((_, new_path)) = &refiled_paths {
            let _ = std::fs::remove_file(new_path);
        }
        return Err(e);
    }
    if let Some((old_path, _)) = &refiled_paths {
        if let Err(e) = std::fs::remove_file(old_path) {
            tracing::warn!("could not remove old file {}: {e}", old_path.display());
        }
    }
    Ok(outcome)
```

(`use crate::pipeline::copy_to;` replaces the `move_file` import.)

- [ ] **Step 4: Run the full suite**

Run: `cargo test`
Expected: all pass — the existing refile tests (`needs_review_reresolves_and_refiles`, `refiles_two_same_base_papers_with_distinct_keys`, etc.) verify old-file-gone/new-file-present, which copy+remove preserves.

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "fix(refresh): copy-update-remove ordering so the DB never points at a missing file"
```

---

### Task 13: Serve guard — refuse non-loopback without `--allow-remote`

**Files:**
- Modify: `src/web/mod.rs`, `src/main.rs`

- [ ] **Step 1: Write the failing unit tests** (new `mod tests` in `src/web/mod.rs`)

```rust
#[cfg(test)]
mod tests {
    use super::is_loopback_host;

    #[test]
    fn classifies_loopback_hosts() {
        assert!(is_loopback_host("127.0.0.1"));
        assert!(is_loopback_host("127.1.2.3"));
        assert!(is_loopback_host("::1"));
        assert!(is_loopback_host("localhost"));
        assert!(is_loopback_host("LOCALHOST"));
        assert!(!is_loopback_host("0.0.0.0"));
        assert!(!is_loopback_host("192.168.1.10"));
        assert!(!is_loopback_host("example.com"));
    }
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib web::tests`
Expected: compile error — `is_loopback_host` not found.

- [ ] **Step 3: Implement**

In `src/web/mod.rs`:

```rust
/// Whether `host` is a loopback bind (safe to serve without auth). Non-IP
/// hostnames other than "localhost" are conservatively treated as remote.
pub fn is_loopback_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}
```

In `src/main.rs`, extend the subcommand:

```rust
Serve {
    /// Address to bind.
    #[arg(long, default_value = "127.0.0.1")]
    host: String,
    /// Port to bind.
    #[arg(long, default_value_t = 8080)]
    port: u16,
    /// Allow binding a non-loopback address (mutating endpoints have no auth).
    #[arg(long)]
    allow_remote: bool,
},
```

and guard at the top of the `Serve` arm:

```rust
Command::Serve { host, port, allow_remote } => {
    if !web::is_loopback_host(&host) {
        if allow_remote {
            tracing::warn!(
                "binding {host}: the web UI has mutating endpoints and no auth — \
                 anyone who can reach this address can import and delete papers"
            );
        } else {
            anyhow::bail!(
                "refusing to bind non-loopback address {host}: the web UI has no auth; \
                 pass --allow-remote to override"
            );
        }
    }
    // ... existing ingest/serve wiring
}
```

- [ ] **Step 4: Run tests, commit**

Run: `cargo test` — all pass. Sanity: `cargo run -- serve --host 0.0.0.0` should print the refusal error (config permitting; a config error first is fine — the flag parse is what matters, verified by compile + unit tests).

```bash
git add -A
git commit -m "feat(serve): refuse non-loopback binds without --allow-remote"
```

---

### Task 14: Interactive retry policy for `serve`

**Files:**
- Modify: `src/resolve/http.rs`, `src/resolve/mod.rs`, `src/main.rs`

- [ ] **Step 1: Add the policy** (`src/resolve/http.rs`, in `impl RetryPolicy`)

```rust
/// Short budget for interactive use (web import): a single quick retry so a
/// synchronous upload response never stalls for minutes.
pub fn interactive() -> Self {
    Self {
        max_attempts: 2,
        base_delay: Duration::from_millis(500),
        max_delay: Duration::from_secs(2),
    }
}
```

- [ ] **Step 2: Add the constructor** (`src/resolve/mod.rs`)

```rust
/// Build a resolver for the real endpoints with an explicit retry policy.
pub fn new_with_policy(contact_email: Option<&str>, retry: RetryPolicy) -> Result<Self> {
    Self::build(
        contact_email,
        "https://export.arxiv.org".to_string(),
        "https://api.crossref.org".to_string(),
        retry,
    )
}
```

and change `new()` to delegate: `Self::new_with_policy(contact_email, RetryPolicy::production())`.

- [ ] **Step 3: Pick the policy per command in `src/main.rs`**

Replace `let resolver = Resolver::new(cfg.contact_email.as_deref())?;` with:

```rust
// Interactive serving answers uploads synchronously; keep retries short there.
let retry = match &cli.command {
    Command::Serve { .. } => xuewen::resolve::http::RetryPolicy::interactive(),
    _ => xuewen::resolve::http::RetryPolicy::production(),
};
let resolver = Resolver::new_with_policy(cfg.contact_email.as_deref(), retry)?;
```

- [ ] **Step 4: Run tests, commit**

Run: `cargo test` — all pass.

```bash
git add -A
git commit -m "fix(serve): bound web-import latency with an interactive retry policy"
```

---

## Phase 3 — Cleanups

### Task 15: DB cleanups — LIKE escaping + explicit sort arm

**Files:**
- Modify: `src/db.rs` (+ tests)

- [ ] **Step 1: Write the failing test**

```rust
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
    let hits = list_papers(&pool, Some("100%"), None, None).await.unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].id, a.id);
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib db::tests::search_treats_like_wildcards_literally`
Expected: FAIL — both rows match because `%` is a wildcard.

- [ ] **Step 3: Implement**

```rust
/// Escape `\`, `%`, `_` in a user search term for `LIKE … ESCAPE '\'`.
fn escape_like(term: &str) -> String {
    term.replace('\\', r"\\").replace('%', r"\%").replace('_', r"\_")
}
```

In `list_papers`:

```rust
if let Some(term) = q.map(str::trim).filter(|s| !s.is_empty()) {
    let like = format!("%{}%", escape_like(term));
    qb.push(" AND (title LIKE ")
        .push_bind(like.clone())
        .push(" ESCAPE '\\' OR authors LIKE ")
        .push_bind(like)
        .push(" ESCAPE '\\')");
}
```

and make the default sort arm explicit:

```rust
let order = match sort {
    Some("year_asc") => "year ASC NULLS LAST",
    Some("added_desc") => "added_at DESC",
    Some("title") => "title COLLATE NOCASE ASC",
    Some("year_desc") => "year DESC",
    _ => "year DESC", // unknown values fall back to the default
};
```

- [ ] **Step 4: Run tests, commit**

Run: `cargo test` — all pass.

```bash
git add -A
git commit -m "chore(db): escape LIKE wildcards in search; explicit year_desc arm"
```

---

### Task 16: Config cleanups — error context + `~` expansion

**Files:**
- Modify: `src/config.rs` (+ tests)

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn expands_leading_tilde_with_home() {
    let home = Some(PathBuf::from("/home/u"));
    assert_eq!(
        expand_tilde(PathBuf::from("~/papers/inbox"), home.clone()),
        PathBuf::from("/home/u/papers/inbox")
    );
    // No tilde, or no HOME: unchanged.
    assert_eq!(
        expand_tilde(PathBuf::from("/data/inbox"), home),
        PathBuf::from("/data/inbox")
    );
    assert_eq!(
        expand_tilde(PathBuf::from("~/x"), None),
        PathBuf::from("~/x")
    );
}

#[test]
fn load_error_names_the_file() {
    let err = Config::load(Path::new("/nope/xuewen.toml")).unwrap_err();
    assert!(err.to_string().contains("/nope/xuewen.toml"));
}
```

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib config`
Expected: compile error (`expand_tilde` missing); the context test would also fail.

- [ ] **Step 3: Implement**

```rust
use anyhow::{Context, Result};

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg: Config = toml::from_str(&text)
            .with_context(|| format!("parsing config {}", path.display()))?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        cfg.inbox_dir = expand_tilde(cfg.inbox_dir, home.clone());
        cfg.library_root = expand_tilde(cfg.library_root, home);
        Ok(cfg)
    }
}

/// Expand a leading `~/` (or bare `~`) using `home`; otherwise return as-is.
fn expand_tilde(p: PathBuf, home: Option<PathBuf>) -> PathBuf {
    match (p.strip_prefix("~"), home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => p,
    }
}
```

- [ ] **Step 4: Run tests, commit**

Run: `cargo test` — all pass.

```bash
git add -A
git commit -m "chore(config): add path context to errors and expand leading tilde"
```

---

### Task 17: IO hygiene — streaming hash, blocking fs off the runtime

**Files:**
- Modify: `src/hash.rs`, `src/pipeline.rs`, `src/refresh.rs`

- [ ] **Step 1: Stream the hash** (`src/hash.rs` — existing test pins the digest)

```rust
/// SHA-256 of a file's bytes (streamed), lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hex::encode(hasher.finalize()))
}
```

- [ ] **Step 2: Async wrappers in `src/pipeline.rs`**

```rust
/// `move_to` off the async runtime.
pub(crate) async fn move_to_async(src: &Path, dir: &Path) -> Result<()> {
    let (src, dir) = (src.to_path_buf(), dir.to_path_buf());
    tokio::task::spawn_blocking(move || move_to(&src, &dir)).await?
}

/// `copy_to` off the async runtime.
pub(crate) async fn copy_to_async(from: &Path, to: &Path) -> Result<()> {
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    tokio::task::spawn_blocking(move || copy_to(&from, &to)).await?
}
```

Inside `ingest_file`, replace every `move_to(...)?` with `move_to_async(...).await?` and the library-copy block (`create_dir_all` + `std::fs::copy`) with `copy_to_async(&path, &dest).await?`. In `refresh_one`, `copy_to(&pdf, &to)` → `copy_to_async(&pdf, &to).await`, and the two `std::fs::remove_file` calls → `tokio::fs::remove_file(...).await`. In `src/watcher.rs`, `process_one`'s quarantine `move_to` → `move_to_async(...).await`.

- [ ] **Step 3: Run the full suite, commit**

Run: `cargo test` — all pass (behavior identical; only the executor thread changes).

```bash
git add -A
git commit -m "chore(pipeline): stream hashing and move blocking fs off the runtime"
```

---

### Task 18: Web IO hygiene + GROBID note

**Files:**
- Modify: `src/web/api.rs`, `src/resolve/grobid.rs`

- [ ] **Step 1: Async staging write in `import_paper`**

Replace the `std::fs::create_dir_all` / `std::fs::write` pair with:

```rust
if let Err(e) = tokio::fs::create_dir_all(&ingest.staging_dir).await {
    tracing::error!("import staging dir: {e}");
    return internal_error();
}
if let Err(e) = tokio::fs::write(&staged, data.as_ref()).await {
    tracing::error!("import stage write: {e}");
    return internal_error();
}
```

and the error-path cleanup `std::fs::remove_file(&staged)` → `tokio::fs::remove_file(&staged).await`.

- [ ] **Step 2: Async canonicalize in the `pdf` handler**

```rust
let under_root = {
    let (p, root) = (path.clone(), app.library_root.clone());
    tokio::task::spawn_blocking(move || {
        match (std::fs::canonicalize(&p), std::fs::canonicalize(&root)) {
            (Ok(file), Ok(root)) => file.starts_with(&root),
            _ => false, // missing file or unresolvable path
        }
    })
    .await
    .unwrap_or(false)
};
```

- [ ] **Step 3: GROBID comment** (`src/resolve/grobid.rs`, above the `Grobid` struct)

```rust
/// A GROBID service client.
///
/// Deliberately plain `reqwest` with no retry policy: GROBID is a local,
/// user-run service — if it's down, degrading to the no-GROBID path
/// immediately beats stalling ingest with retries.
```

- [ ] **Step 4: Run everything, commit**

Run: `cargo test` — all pass.

```bash
git add src/web/api.rs
git commit -m "chore(web): move staging write and canonicalize off the runtime"
git add src/resolve/grobid.rs
git commit -m "docs(resolve): note grobid retry is deliberately absent"
```

---

### Task 19: Final verification

- [ ] **Step 1: Full backend check**

Run: `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
Expected: clean formatting, zero clippy warnings, all tests pass. Fix anything that surfaces (`cargo fmt` to apply formatting).

- [ ] **Step 2: Full frontend check**

Run: `npm --prefix frontend test -- --run && npm --prefix frontend run build`
Expected: all vitest suites pass; production build succeeds.

- [ ] **Step 3: End-to-end smoke** (manual, no commit)

Run: `cargo run -- --help` — shows `restore`; `cargo run -- serve --host 0.0.0.0` (with a scratch config) — refuses with the `--allow-remote` hint.

- [ ] **Step 4: Commit any straggler fixes**

```bash
git status   # should be clean; commit stragglers under the matching scope
```
