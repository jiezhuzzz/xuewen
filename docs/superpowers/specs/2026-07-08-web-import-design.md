# Design: Web Import (multi-file PDF upload)

**Project:** Xuewen — self-hosted reference manager for CS papers.
**Date:** 2026-07-08
**Status:** Approved (design phase)

## 1. Purpose

Let a user add papers from the **web UI** by uploading one or more PDFs from the
browser, instead of only via the CLI `ingest` command or the inbox `watch`
daemon. Each uploaded PDF runs through the **existing** `ingest_file` pipeline
(hash → dedup → extract → resolve metadata → file into the library → insert the
record), so web import and CLI ingest produce identical results.

Import is the web UI's **second mutation** (after delete). The read-only design
is relaxed for this one operation. Because the server has no authentication and
may be bound to a LAN, the same caveat as delete applies: any client that can
reach the server can upload. This is acceptable for the intended
single-user / trusted-LAN deployment.

## 2. Decisions (settled during brainstorming)

| Decision | Choice |
|---|---|
| Processing & progress | **Per-file, live progress** — the frontend sends one request per file, **sequentially**, and shows each file's status live |
| Trigger UI | **Drop-zone modal** — an "Import" button opens a modal with drag-and-drop + click-to-browse |
| Backend endpoint | `POST /api/papers`, `multipart/form-data`, **one PDF per request** |
| Pipeline | **Reuse** `ingest_file` unchanged |
| Upload staging | Write bytes into a watcher-ignored **`inbox_dir/_uploads`** subdir, then ingest |
| Body size limit | Raise to **50 MB** on the import route (axum default is 2 MB) |
| Router wiring | `AppState.ingest: Option<Arc<Ingest>>`; keep `build_router` read-only, add `build_router_with_ingest` |

Sequential per-file upload (not concurrent) is deliberate: each ingest performs
network metadata resolution against Crossref / arXiv / DBLP, and one request at a
time keeps that traffic polite.

## 3. Backend

### 3.1 Endpoint

`POST /api/papers` — `multipart/form-data` carrying a single file part (one PDF).
Multiple files are handled by the frontend issuing multiple sequential requests,
not by one multipart body with many parts.

**Handler flow:**

1. Parse the multipart body; take the first file field → its filename + bytes.
   (Missing file part → `400 {"error":"no file"}`.)
2. **Validate**: the bytes must begin with the `%PDF` magic marker. Otherwise
   → `400 {"error":"not a PDF"}`. (Cheap guard; the pipeline would fail on a
   non-PDF anyway, but a clear 400 gives better per-file feedback.)
3. **Stage**: write the bytes into a dedicated **`inbox_dir/_uploads`**
   directory (created on demand) under a sanitized, collision-safe name —
   basename only (strip any path separators to prevent traversal), prefixed with
   a short unique token (e.g. first 8 chars of a UUID) so repeated uploads never
   clobber an existing staged file. `_uploads` is a subdirectory, so a
   concurrently running `watch` daemon never races for it: the watcher scans the
   inbox **non-recursively** and skips underscore subdirs (`catch_up_scan` +
   `RecursiveMode::NonRecursive`). The final library filename is
   cite-key/hash based, so this staged name is transient.
4. **Ingest**: call `ingest_file(pool, dirs, resolver, grobid.as_ref(), &path)`
   and map the result:
   - `Ok(Outcome::Ingested(id))` → look up the freshly inserted row by `id` and
     return `200 {"outcome":"ingested","id":<id>,"title":<title|null>,"status":<"resolved"|"needs_review">}`
     so the UI can distinguish a clean resolve from a needs-review ingest.
   - `Ok(Outcome::Duplicate)` → `200 {"outcome":"duplicate"}`. On a duplicate,
     `ingest_file` has already moved the staged inbox copy into `_processed`, so
     no cleanup is needed here.
   - `Err(e)` → log, **best-effort remove the leftover staged inbox file**
     (on error `ingest_file` does not move the original), then
     `500 {"error":"import failed"}`.

### 3.2 `AppState` and router wiring

The ingest pipeline needs a `Resolver`, an optional `Grobid`, and the `Libraries`
dirs. The import handler additionally needs a staging directory. Bundle them:

```rust
pub struct Ingest {
    pub resolver: Resolver,
    pub grobid: Option<Grobid>,
    pub dirs: Libraries,         // library_root + processed_dir (unchanged struct)
    pub staging_dir: PathBuf,    // = inbox_dir/_uploads, set in main.rs
}
```

**`Libraries` is left unchanged** (`{ library_root, processed_dir }`). It is
constructed in `main.rs`, `pipeline_test`, and `watcher`'s tests; adding a field
would break those call sites, and `ingest_file` itself never reads `inbox_dir`.
The staging directory is a web-handler concern, so it lives on `Ingest` as
`staging_dir`, computed in `main.rs` as `cfg.inbox_dir.join("_uploads")`.

`AppState` gains the bundle behind an `Arc` (because `Resolver`/`Grobid` are not
`Clone`, but `AppState` must be `Clone` for axum):

```rust
pub struct AppState {
    pub pool: SqlitePool,
    pub library_root: PathBuf,
    pub ingest: Option<Arc<Ingest>>,
}
```

Router builders:

- **Keep** `build_router(pool, library_root)` — constructs `ingest: None`. Every
  existing read-only test (`tests/web_test.rs`) keeps compiling and passing
  unchanged.
- **Add** `build_router_with_ingest(pool, library_root, ingest: Arc<Ingest>)` —
  used by `serve` and by the import integration test.
- The `POST /api/papers` handler returns `503 {"error":"import not configured"}`
  when `state.ingest` is `None`. This branch is unreachable in production (serve
  always supplies it) but keeps the read-only router valid.

Route registration adds the POST verb with a raised body limit (the default 2 MB
is too small for real PDFs):

```rust
.route("/api/papers",
    get(api::list_papers)
    .post(api::import_paper)
    .layer(DefaultBodyLimit::max(50 * 1024 * 1024)))
```

An over-limit upload yields axum's `413 Payload Too Large`, surfaced per-file in
the UI.

The handler uses axum's `Multipart` extractor, which is **not** a default
feature. `Cargo.toml` must enable it:
`axum = { version = "0.8", features = ["multipart"] }`.

### 3.3 `serve` and `main.rs`

`web::serve` extends to accept the ingest bundle:
`serve(host, port, pool, library_root, ingest: Arc<Ingest>)`. The `Command::Serve`
arm in `main.rs` already constructs `resolver`, `grobid`, and `dirs`; it wraps
them into an `Ingest`, `Arc`s it, and passes it through. Ownership is moved into
`serve` (nothing else runs after it).

## 4. Frontend

### 4.1 Components & state

- **`components/ImportModal.svelte`** (new): a fixed-inset backdrop with a
  centered panel. Inside: a drop-zone that is also click-to-browse via a hidden
  `<input type="file" accept=".pdf,application/pdf" multiple>`. Files dropped or
  picked are appended to a reactive queue.
- **Queue item state**: `{ name, status: 'queued'|'importing'|'ingested'|'duplicate'|'failed', message? }`.
  The queue is processed **sequentially** (`for … of`, awaiting each). Each row
  shows an icon + label: ingested (with title / *resolved* vs *needs review*),
  duplicate, or failed (with reason). A summary line reads e.g.
  "3 ingested, 1 duplicate, 1 failed".
- **`lib/api.ts`**: `importPaper(file: File): Promise<ImportResult>` — builds a
  `FormData`, `POST`s to `/api/papers`, throws on non-2xx with the server's
  error text so the row can show a reason.
- **`lib/state.svelte.ts`**: add `ui.importOpen` plus `openImport()` /
  `closeImport()` (mirroring `sidebarOpen`). After the batch finishes (or after
  each successful ingest), call `loadPapers()` + `loadStats()` so new papers
  appear in the sidebar and the counts update. Closing the modal sets a cancel
  flag that stops launching further requests (an in-flight request is allowed to
  finish).
- **`components/TopBar.svelte`**: an "Import" button (Lucide `FilePlus` /
  `Upload` icon) in the top bar that calls `openImport()`.
- **`App.svelte`**: render `{#if ui.importOpen}<ImportModal />{/if}`.

### 4.2 Types

`lib/types.ts` gains:

```ts
export type ImportResult =
  | { outcome: 'ingested'; id: string; title: string | null; status: string }
  | { outcome: 'duplicate' };
```

Client-side, a light pre-check may mark obviously-non-PDF files as failed before
upload, but the **server is the source of truth** for validation.

## 5. Edge cases

| Case | Behavior |
|---|---|
| Non-PDF bytes | `400 {"error":"not a PDF"}` → row shows "failed: not a PDF" |
| Missing file part | `400 {"error":"no file"}` |
| Over 50 MB | axum `413` → row shows "failed: too large" |
| Duplicate content | `200 {"outcome":"duplicate"}` → shown distinctly, **not** an error |
| Ingest error (corrupt PDF, extraction/DB failure) | `500`; staged inbox file removed best-effort; row shows "failed" |
| Path traversal in filename | Neutralized — basename only, unique prefix; nothing written outside `inbox_dir` |
| Ingest not configured (`None`) | `503` — unreachable in production |

## 6. Testing

### 6.1 Backend (`tests/web_test.rs`)

Ingest fully offline using the same trick as `watcher`'s tests: a resolver whose
upstreams refuse instantly, so every lookup degrades to `needs_review` with no
network wait and no mock server —
`Resolver::with_bases(None, "http://127.0.0.1:1", "http://127.0.0.1:1").with_dblp_base("http://127.0.0.1:1")`.
Use a fixture PDF built with `common::write_test_pdf`. Build the router via
`build_router_with_ingest` with an `Ingest` whose `dirs`/`staging_dir` point at
tempdir paths. Assert:

- Import a fixture PDF (multipart) → `200 {"outcome":"ingested",…}` with
  `status:"needs_review"`, and the paper then appears in `GET /api/papers` and in
  `GET /api/stats`.
- Re-import the **same bytes** → `200 {"outcome":"duplicate"}`.
- Import non-PDF bytes → `400`.

(`axum-test`'s `TestServer` supports multipart request bodies, matching the
existing web tests' use of `TestServer`.)

### 6.2 Frontend (`components/ImportModal.test.ts`)

Mirror `InfoPanel.test.ts` (Vitest + `@testing-library/svelte`): mock
`lib/api`'s `importPaper`, simulate selecting files on the hidden input, and
assert the queue rows transition `queued → importing → ingested/duplicate/failed`
and that `loadPapers` / `loadStats` are invoked after the batch.

## 7. Out of scope (YAGNI)

- Concurrent multi-file uploads (sequential is polite and simpler).
- Background job + polling (overkill for a single-user tool).
- Re-ordering / retrying individual failed files (user can re-drop them).
- URL / DOI / arXiv-id import (file upload only, this iteration).
- Authentication (unchanged; same trusted-deployment assumption as delete).
