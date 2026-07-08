# Design: Review-Fix Package (structure refactor + correctness fixes)

**Project:** Xuewen — self-hosted reference manager for CS papers.
**Date:** 2026-07-08
**Status:** Approved (design phase)

## 1. Purpose

A full-codebase review (2026-07-08) found the project healthy but flagged a set
of issues. This package fixes them in three phases:

1. **Structure** — behavior-preserving refactors that remove the two biggest
   smells (parameter trains, hand-copied metadata fields) so the behavior fixes
   land on clean ground.
2. **Behavior** — user-facing correctness: identifier-level duplicate handling,
   a trash-restore path, crash-safe refresh re-filing, a guard on non-loopback
   binds, and bounded web-import latency.
3. **Cleanups** — small polish items, one commit.

## 2. Decisions (settled during brainstorming)

| Decision | Choice |
|---|---|
| Scope | Everything from the review **except** async (202+poll) web import and bearer-token auth |
| DOI/arXiv collision | **Report as same-work, keep the first copy** — never overwrite, never error |
| Trash semantics | New `restore <id>` CLI command; ingest matching a trashed paper reports a distinct **in-trash** outcome (no auto-restore, no silent "duplicate") |
| Non-loopback serve | **Refuse** unless `--allow-remote` is passed (flag also logs a warning) |
| Web import latency | Keep synchronous; use a shorter **interactive retry policy** for the resolver in `serve` mode |
| Sequencing | **Refactor first**, verified by the existing suite, then behavior fixes with tests-first, then cleanups |

## 3. Phase 1 — Structure (behavior-preserving)

The existing test suite is the safety net: it must pass unchanged (modulo
renamed symbols) after each refactor.

### 3.1 `PaperMeta` extraction

The 11 metadata fields currently repeated across `Paper`, `ResolvedFields`,
and the DTOs move into one struct:

```rust
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
```

(`cite_key` is deliberately **not** part of `PaperMeta` — it is
location/naming state managed by the pipeline, not resolution output. It stays
on `Paper`.)

`Paper` becomes identity/location + flattened metadata:

```rust
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

- `ResolvedFields` is **deleted**. `resolve_fields()` returns `PaperMeta`;
  `apply_to` becomes `paper.meta = meta;` (plus the status-downgrade guard in
  refresh); `into_paper` shrinks to struct construction.
- `db::insert_paper` / `update_paper` keep explicit column lists but bind from
  `p.meta.*`. Reads keep `query_as::<Paper>` via the flatten.
- No schema change.

### 3.2 Typed `status` and `authors`

- `PaperStatus` derives `sqlx::Type` + `Serialize`/`Deserialize`
  (`rename_all = "snake_case"` → `resolved` / `needs_review`, matching the
  existing TEXT values). String comparisons like
  `paper.status == PaperStatus::Resolved.as_str()` become enum equality.
- `Authors(pub Vec<String>)` newtype: stored as a JSON-array TEXT column,
  `NULL ⇄ empty vec` (both encode and decode), serde-serializes as a plain
  array. Implements `sqlx::Type`/`Encode`/`Decode` for SQLite. The four
  scattered `serde_json` encode/parse sites (`into_paper`, `apply_to`,
  `Paper::authors_vec`, `PaperSummary::from`) collapse into these impls;
  `authors_vec()` is deleted.

### 3.3 `IngestCtx`

```rust
pub struct IngestCtx {
    pub pool: SqlitePool,
    pub dirs: Libraries,
    pub resolver: Resolver,
    pub grobid: Option<Grobid>,
}
```

Lives in `pipeline.rs`; built once in `main`. `ingest_file` and `resolve_pdf`
become methods (`ctx.ingest_file(path)`); `watcher::run(&ctx, inbox)` and
`refresh::run(&ctx, target)` take it (refresh reads `ctx.dirs.library_root`).
`web::Ingest` becomes `{ ctx: IngestCtx, staging_dir: PathBuf }` (still behind
`Arc` in `AppState`; `AppState.pool` remains a cheap clone of the same pool).

### 3.4 Dead weight

- `Resolution` enum → `Option<ResolvedMetadata>` (`resolve()` already builds it
  from an Option and both callers immediately match it apart). The
  `clippy::large_enum_variant` allow and the stale `build_paper` comment go
  with it.
- Delete unused `ResolvedMetadata::authors_json` and
  `matching::is_confident_match` (production code inlines both; only their own
  unit tests use them).

## 4. Phase 2 — Behavior fixes (tests first, one commit each)

### 4.1 Same-work and in-trash ingest outcomes

`Outcome` grows:

```rust
pub enum Outcome {
    Ingested(String),  // new paper id
    Duplicate,         // same bytes as an active paper
    SameWork(String),  // same DOI/arXiv id as an active paper → its id
    InTrash(String),   // same bytes or same identifier as a trashed paper → its id
}
```

Pipeline changes inside `ingest_file`:

1. **Hash dedup** — `db::find_by_hash(hash) -> Option<Paper>` (replaces
   `exists_by_hash`): active hit → `Duplicate`; trashed hit → `InTrash(id)`.
   File moves to `_processed` either way (unchanged for `Duplicate`).
2. **Identifier dedup** — after resolution, before filing/insert: when the
   decided fields carry a `doi` or `arxiv_id`, a new
   `db::find_by_identifier(doi, arxiv_id) -> Option<Paper>` lookup runs.
   Active hit → `SameWork(id)`; trashed hit → `InTrash(id)`. File moves to
   `_processed`; nothing is inserted; the library copy is never made.
3. **Race fallback** — two concurrent imports can both pass the checks. A
   UNIQUE-constraint violation from `insert_paper` (content_hash / doi /
   arxiv_id) is detected (SQLite constraint error), the freshly copied library
   file is removed (existing cleanup path), and the row that won the race is
   re-queried to produce the same `Duplicate`/`SameWork`/`InTrash` outcomes
   instead of an error. Any other DB error still propagates as today.

Surface behavior:

- **CLI `ingest`** prints:
  `already in library as he2016deep (a1b2c3…)` for `SameWork`,
  `in trash — run: xuewen restore a1b2c3` for `InTrash`.
- **Watcher** logs the outcome (no quarantine for these; the file went to
  `_processed`).
- **Web `POST /api/papers`** returns `{"outcome":"same_work","id":…}` /
  `{"outcome":"in_trash","id":…}` alongside the existing `ingested` /
  `duplicate`. The frontend `ImportResult` type and `ImportModal` gain
  matching statuses/labels ("already in library", "in trash — restore via
  CLI"), styled like the existing `duplicate` row.

### 4.2 `xuewen restore <id>`

- CLI: `restore <id>` (exact or unique prefix, same `find_one` resolution as
  delete/purge). Errors if the paper is not in the trash; otherwise
  `db::restore(pool, id)` sets `deleted_at = NULL` and prints `restored <id>`.
- No confirmation prompt (restoring is non-destructive).
- Web stays as-is (no trash view yet).

### 4.3 Crash-safe refresh re-filing

`refresh_one` currently moves the PDF, then updates the DB; a DB failure
orphans the file (refresh forever skips "missing" papers). New order:

1. Re-resolve → mutate `paper` in memory (unchanged).
2. If the recomputed rel-path differs: **copy** the PDF to the new path
   (create parents). Copy failure → warn, keep the old path (as today).
3. `db::update_paper` (with the new rel-path only if step 2 succeeded).
4. On DB success: remove the old file (failure → warn; a stale extra copy is
   harmless). On DB failure after a copy: remove the new copy and return the
   error — DB and filesystem stay consistent.

Refresh was `move_file`'s only caller, so it is deleted (the pipeline's
`move_to` for inbox → `_processed` is unaffected).

### 4.4 Non-loopback serve guard

`serve` gains `--allow-remote`. At startup, the resolved bind host is
classified: loopback = `127.0.0.0/8`, `::1`, or the literal `localhost`.
Non-loopback without the flag → startup error naming the flag; with the flag →
serve, plus a `tracing::warn!` that mutating endpoints are exposed without
auth. Classification lives in a small pure function with unit tests.

### 4.5 Interactive retry policy for `serve`

`RetryPolicy::interactive()` — 2 attempts, 500 ms base delay, 2 s max. A new
`Resolver::new_with_policy(contact_email, policy)` constructor; `main` uses it
for the `Serve` command and the existing polite production policy everywhere
else. Worst-case synchronous import latency drops substantially (2 attempts
per source instead of 4 with long back-off), though GROBID's own 60s timeout
and the DBLP→Crossref fallback still apply.

## 5. Phase 3 — Minor cleanups (one commit)

- **LIKE escaping**: the `q` search term escapes `%`, `_`, `\` and the query
  uses `ESCAPE '\'`; an explicit `year_desc` arm joins the sort whitelist.
- **Streaming hash**: `sha256_file` uses `std::io::copy` into the hasher
  instead of reading the whole file.
- **Async hygiene**: blocking fs work in async contexts moves off the runtime —
  pipeline library-copy and `_processed` moves, refresh copy/remove, the staged
  upload write (`tokio::fs::write`), and the `canonicalize` pair in the pdf
  handler (`spawn_blocking`).
- **Config polish**: `Config::load` wraps errors with the path
  (`anyhow::Context`); a leading `~/` in `inbox_dir`/`library_root` expands via
  `$HOME`.
- **GROBID**: stays on plain reqwest without retry — deliberate (local
  service); documented with a comment.

## 6. Testing

- **Phase 1**: no new tests required; the existing suite (unit + wiremock +
  axum-test + vitest) must pass unchanged apart from mechanical renames.
- **Phase 2** (each fix lands test-first):
  - Same-work: two different PDF fixtures resolving (via wiremock) to the same
    DOI → second ingest returns `SameWork` with the first paper's id, one row
    in the DB, file in `_processed`.
  - In-trash: ingest, soft-delete, re-ingest same bytes → `InTrash`; and a
    different-bytes/same-DOI variant → `InTrash`.
  - Race fallback: unit-test the violation→outcome mapping directly — seed a
    row, force a UNIQUE violation via `insert_paper` (same hash; then same DOI
    with a different hash), and assert the mapped outcome and that no library
    file is left behind.
  - Restore: trashed paper restored → visible in `list_papers`/`stats`;
    restoring an active paper errors.
  - Refresh ordering: refile happy path (new file exists, old gone, DB row
    matches); simulated copy failure keeps old path and DB consistent.
  - Serve guard: unit tests for the loopback classifier; refusal exercised at
    the CLI layer.
  - Web: `same_work` / `in_trash` responses in `web_test.rs`; ImportModal
    label rendering in vitest.

## 7. Out of scope

- Async (202 + poll) web import; bearer-token auth.
- Search semantics over the authors JSON encoding.
- Web trash view / web restore.
- Identifier-level merge or PDF replacement (same-work keeps the first copy,
  always).
