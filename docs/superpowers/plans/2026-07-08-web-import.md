# Web Import (multi-file PDF upload) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user upload one or more PDFs from the web UI; each runs through the existing `ingest_file` pipeline with live per-file progress.

**Architecture:** A new `POST /api/papers` multipart endpoint (one PDF per request) validates the `%PDF` magic, stages bytes into a watcher-ignored `inbox_dir/_uploads`, calls `ingest_file`, and returns `ingested`/`duplicate`/error. The frontend fires one request per file **sequentially** from a drop-zone modal, showing each file's status, then refreshes the list + stats.

**Tech Stack:** Rust (axum 0.8 with the `multipart` feature, sqlx, tokio), Svelte 5 (runes) + Vite + Tailwind, Vitest.

**Spec:** `docs/superpowers/specs/2026-07-08-web-import-design.md`

---

## File Structure

- `Cargo.toml` — enable axum's `multipart` feature.
- `src/web/mod.rs` — add `Ingest` struct, `AppState.ingest` field, `build_router_with_ingest`, extend `serve`, register the POST route with a raised body limit.
- `src/web/api.rs` — new `import_paper` handler + `bad_request`/`multipart_error` helpers.
- `src/main.rs` — build the `Ingest` bundle in the `Serve` arm and pass it to `serve`.
- `tests/web_test.rs` — offline integration test for import.
- `frontend/src/lib/types.ts` — `ImportResult` type.
- `frontend/src/lib/api.ts` — `importPaper(file)`.
- `frontend/src/lib/state.svelte.ts` — `ui.importOpen`, `openImport`/`closeImport`, `importState`, `enqueueFiles`.
- `frontend/src/components/ImportModal.test.ts` — state-logic test (mirrors `InfoPanel.test.ts`).
- `frontend/src/components/ImportModal.svelte` — the modal.
- `frontend/src/components/TopBar.svelte` — the "Import" button.
- `frontend/src/App.svelte` — render the modal.

---

## Task 1: Backend plumbing (Ingest bundle, router builders, serve wiring)

Add all the wiring so the codebase compiles with a stub handler and every existing test still passes. No import behavior yet.

**Files:**
- Modify: `Cargo.toml`
- Modify: `src/web/mod.rs`
- Modify: `src/web/api.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Enable the axum `multipart` feature**

In `Cargo.toml`, change the `axum` line:

```toml
axum = { version = "0.8", features = ["multipart"] }
```

- [ ] **Step 2: Add the `Ingest` bundle, `AppState.ingest`, and router builders**

Replace the top of `src/web/mod.rs` (the imports, `AppState`, and `build_router`) with:

```rust
mod api;
mod assets;
mod dto;

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use axum::extract::DefaultBodyLimit;
use axum::routing::get;
use axum::Router;
use sqlx::SqlitePool;

use crate::pipeline::Libraries;
use crate::resolve::grobid::Grobid;
use crate::resolve::Resolver;

/// Everything the web import handler needs to run the ingest pipeline. Held
/// behind an `Arc` in `AppState` because `Resolver`/`Grobid` are not `Clone`.
pub struct Ingest {
    pub resolver: Resolver,
    pub grobid: Option<Grobid>,
    pub dirs: Libraries,
    /// Where uploaded bytes are written before ingest (`inbox_dir/_uploads`).
    pub staging_dir: PathBuf,
}

/// Shared state for the web handlers.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub library_root: PathBuf,
    /// Present only when the server was started to allow uploads (`serve`).
    pub ingest: Option<Arc<Ingest>>,
}

/// Assemble the read-only web router (no import). Used directly by tests.
pub fn build_router(pool: SqlitePool, library_root: PathBuf) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: None,
    })
}

/// Assemble the full web router, including the import endpoint.
pub fn build_router_with_ingest(
    pool: SqlitePool,
    library_root: PathBuf,
    ingest: Arc<Ingest>,
) -> Router {
    router_with(AppState {
        pool,
        library_root,
        ingest: Some(ingest),
    })
}

fn router_with(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/papers",
            get(api::list_papers)
                .post(api::import_paper)
                .layer(DefaultBodyLimit::max(50 * 1024 * 1024)),
        )
        .route(
            "/api/papers/{id}",
            get(api::get_paper).delete(api::delete_paper),
        )
        .route("/api/stats", get(api::stats))
        .route("/papers/{id}/pdf", get(api::pdf))
        .fallback(assets::static_handler)
        .with_state(state)
}
```

- [ ] **Step 3: Extend `serve` to take the ingest bundle**

Replace the `serve` function at the bottom of `src/web/mod.rs`:

```rust
/// Bind `host:port` and serve the router until the process is stopped.
pub async fn serve(
    host: &str,
    port: u16,
    pool: SqlitePool,
    library_root: PathBuf,
    ingest: Arc<Ingest>,
) -> Result<()> {
    let app = build_router_with_ingest(pool, library_root, ingest);
    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("xuewen serving on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 4: Add a stub `import_paper` handler**

In `src/web/api.rs`, add this handler after `delete_paper` (the real body comes in Task 2). It compiles the new POST route and keeps existing tests green:

```rust
/// Import a PDF (stub — real implementation in the next task).
pub async fn import_paper(State(app): State<AppState>) -> Response {
    if app.ingest.is_none() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "import not configured"})),
        )
            .into_response();
    }
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({"error": "not implemented"})),
    )
        .into_response()
}
```

- [ ] **Step 5: Wire the `Serve` arm in `src/main.rs`**

Replace the `Command::Serve { host, port } => { ... }` arm (currently `src/main.rs:127-129`) with:

```rust
        Command::Serve { host, port } => {
            let ingest = std::sync::Arc::new(web::Ingest {
                resolver,
                grobid,
                dirs,
                staging_dir: cfg.inbox_dir.join("_uploads"),
            });
            web::serve(&host, port, pool, cfg.library_root.clone(), ingest).await?;
        }
```

- [ ] **Step 6: Build and run the whole test suite**

Run: `cargo test`
Expected: PASS — everything compiles and all existing tests still pass (no test exercises the POST route yet).

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml Cargo.lock src/web/mod.rs src/web/api.rs src/main.rs
git commit -m "feat(web-import): ingest bundle + router/serve plumbing (stub handler)"
```

---

## Task 2: Import endpoint (TDD)

Write the failing integration test, then implement the real handler.

**Files:**
- Modify: `tests/web_test.rs`
- Modify: `src/web/api.rs`

- [ ] **Step 1: Write the failing integration test**

Add to the top of `tests/web_test.rs` (alongside the existing `use` lines):

```rust
use xuewen::pipeline::Libraries;
use xuewen::resolve::Resolver;
use xuewen::web::{build_router_with_ingest, Ingest};
use axum_test::multipart::{MultipartForm, Part};
```

Then add this test at the end of `tests/web_test.rs`:

```rust
#[tokio::test]
async fn imports_a_pdf_dedups_and_rejects_non_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Offline resolver: upstreams refuse instantly, so resolution degrades to
    // needs_review with no network wait (same trick as the watcher tests).
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());

    let ingest = std::sync::Arc::new(Ingest {
        resolver,
        grobid: None,
        dirs: Libraries {
            library_root: library.clone(),
            processed_dir: inbox.join("_processed"),
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server =
        TestServer::new(build_router_with_ingest(pool, library.clone(), ingest)).unwrap();

    // A real one-page PDF whose header has no DOI/arXiv id.
    let pdf_path = dir.path().join("paper.pdf");
    common::write_test_pdf(&pdf_path, &["A Paper With No Identifier"]);
    let pdf_bytes = std::fs::read(&pdf_path).unwrap();

    // Import -> 200 ingested, needs_review.
    let form = MultipartForm::new()
        .add_part("file", Part::bytes(pdf_bytes.clone()).file_name("paper.pdf"));
    let resp = server.post("/api/papers").multipart(form).await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["outcome"], "ingested");
    assert_eq!(body["status"], "needs_review");

    // It now shows up in the list and the stats.
    assert_eq!(
        server
            .get("/api/papers")
            .await
            .json::<Vec<serde_json::Value>>()
            .len(),
        1
    );
    assert_eq!(
        server.get("/api/stats").await.json::<serde_json::Value>()["total"],
        1
    );

    // Re-import identical bytes -> 200 duplicate.
    let form2 = MultipartForm::new()
        .add_part("file", Part::bytes(pdf_bytes).file_name("paper.pdf"));
    let dup: serde_json::Value = server.post("/api/papers").multipart(form2).await.json();
    assert_eq!(dup["outcome"], "duplicate");

    // Non-PDF bytes -> 400.
    let form3 = MultipartForm::new()
        .add_part("file", Part::bytes(b"not a pdf".to_vec()).file_name("x.pdf"));
    server
        .post("/api/papers")
        .multipart(form3)
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test --test web_test imports_a_pdf_dedups_and_rejects_non_pdf`
Expected: FAIL — the stub returns `501 NOT_IMPLEMENTED`, so `resp.assert_status_ok()` fails.

- [ ] **Step 3: Implement the real handler**

In `src/web/api.rs`, update the imports at the top of the file to add:

```rust
use axum::extract::multipart::MultipartError;
use axum::extract::Multipart;
use uuid::Uuid;

use crate::pipeline::{ingest_file, Outcome};
```

Then replace the stub `import_paper` from Task 1 with the full implementation:

```rust
/// Import a single uploaded PDF: validate, stage into `inbox_dir/_uploads`, and
/// run the ingest pipeline. One PDF per request (the frontend uploads files one
/// at a time). Returns `ingested` (with title/status), `duplicate`, or an error.
pub async fn import_paper(State(app): State<AppState>, mut multipart: Multipart) -> Response {
    let ingest = match &app.ingest {
        Some(i) => i.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "import not configured"})),
            )
                .into_response()
        }
    };

    // Take the first file part; skip any non-file fields.
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => return bad_request("no file"),
            Err(e) => return multipart_error(e),
        };
        let Some(filename) = field.file_name().map(|s| s.to_string()) else {
            continue;
        };
        let data = match field.bytes().await {
            Ok(b) => b,
            Err(e) => return multipart_error(e),
        };
        if !data.starts_with(b"%PDF") {
            return bad_request("not a PDF");
        }

        // Stage the bytes under a sanitized, collision-safe name.
        let stem = std::path::Path::new(&filename)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("upload.pdf");
        let unique = &Uuid::now_v7().to_string()[..8];
        let staged = ingest.staging_dir.join(format!("{unique}-{stem}"));
        if let Err(e) = std::fs::create_dir_all(&ingest.staging_dir) {
            tracing::error!("import staging dir: {e}");
            return internal_error();
        }
        if let Err(e) = std::fs::write(&staged, data.as_ref()) {
            tracing::error!("import stage write: {e}");
            return internal_error();
        }

        return match ingest_file(
            &app.pool,
            &ingest.dirs,
            &ingest.resolver,
            ingest.grobid.as_ref(),
            &staged,
        )
        .await
        {
            Ok(Outcome::Ingested(id)) => {
                // Look up the fresh row so the UI can show title + resolved/needs_review.
                let (title, status) = match db::get_by_id(&app.pool, &id).await {
                    Ok(Some(p)) => (serde_json::json!(p.title), p.status),
                    _ => (serde_json::Value::Null, "resolved".to_string()),
                };
                Json(serde_json::json!({
                    "outcome": "ingested",
                    "id": id,
                    "title": title,
                    "status": status,
                }))
                .into_response()
            }
            Ok(Outcome::Duplicate) => {
                Json(serde_json::json!({"outcome": "duplicate"})).into_response()
            }
            Err(e) => {
                tracing::error!("import ingest: {e}");
                // On error the pipeline did not move the original — clean it up.
                let _ = std::fs::remove_file(&staged);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "import failed"})),
                )
                    .into_response()
            }
        };
    }
}
```

Then add these two helpers next to the existing `not_found`/`internal_error` helpers at the bottom of `src/web/api.rs`:

```rust
pub(super) fn bad_request(msg: &str) -> Response {
    (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": msg }))).into_response()
}

/// Map a multipart read error to its proper status (e.g. 413 when the body
/// exceeds the limit) with a JSON body.
fn multipart_error(e: MultipartError) -> Response {
    let status = e.into_response().status();
    (
        status,
        Json(serde_json::json!({
            "error": status.canonical_reason().unwrap_or("upload error").to_lowercase()
        })),
    )
        .into_response()
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cargo test --test web_test imports_a_pdf_dedups_and_rejects_non_pdf`
Expected: PASS.

- [ ] **Step 5: Run the full suite (no regressions)**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/web/api.rs tests/web_test.rs
git commit -m "feat(web-import): POST /api/papers multipart ingest endpoint"
```

---

## Task 3: Frontend API client + type

**Files:**
- Modify: `frontend/src/lib/types.ts`
- Modify: `frontend/src/lib/api.ts`

- [ ] **Step 1: Add the `ImportResult` type**

Append to `frontend/src/lib/types.ts`:

```ts
export type ImportResult =
  | { outcome: 'ingested'; id: string; title: string | null; status: string }
  | { outcome: 'duplicate' };
```

- [ ] **Step 2: Add `importPaper` to the API client**

In `frontend/src/lib/api.ts`, update the import line and append the function:

```ts
import type { Filters, ImportResult, PaperDetail, PaperSummary, Stats } from './types';
```

```ts
export async function importPaper(file: File): Promise<ImportResult> {
  const body = new FormData();
  body.append('file', file, file.name);
  const res = await fetch('/api/papers', { method: 'POST', body });
  if (!res.ok) {
    let msg = `import failed: ${res.status}`;
    try {
      const j = await res.json();
      if (j && typeof j.error === 'string') msg = j.error;
    } catch {
      /* non-JSON error body */
    }
    throw new Error(msg);
  }
  return res.json();
}
```

- [ ] **Step 3: Type-check the frontend**

Run: `cd frontend && npm run check`
Expected: PASS (0 errors).

- [ ] **Step 4: Commit**

```bash
git add frontend/src/lib/types.ts frontend/src/lib/api.ts
git commit -m "feat(web-import): frontend importPaper API + ImportResult type"
```

---

## Task 4: Frontend import state (TDD)

Add the reactive import queue and the sequential drain loop, tested like `InfoPanel.test.ts`.

**Files:**
- Modify: `frontend/src/lib/state.svelte.ts`
- Test: `frontend/src/components/ImportModal.test.ts`

- [ ] **Step 1: Write the failing test**

Create `frontend/src/components/ImportModal.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { enqueueFiles, importState, openImport } from '../lib/state.svelte';

function pdf(name: string): File {
  return new File([new Uint8Array([0x25, 0x50, 0x44, 0x46])], name, {
    type: 'application/pdf',
  });
}

// A fetch stub: POST /api/papers -> per-file JSON body; GET list/stats -> empty.
// The HTTP status is 400 when the body carries an `error`, else 200 (so a
// paper's own `status: 'resolved'` string never gets mistaken for an HTTP code).
function stubFetch(outcome: (name: string) => object) {
  vi.stubGlobal(
    'fetch',
    vi.fn(async (url: string | URL, init?: RequestInit) => {
      const u = String(url);
      const json = (o: unknown, status = 200) =>
        new Response(JSON.stringify(o), {
          status,
          headers: { 'content-type': 'application/json' },
        });
      if (u === '/api/papers' && init?.method === 'POST') {
        const f = (init.body as FormData).get('file') as File;
        const payload = outcome(f.name) as Record<string, unknown>;
        return json(payload, typeof payload.error === 'string' ? 400 : 200);
      }
      if (u.startsWith('/api/papers')) return json([]);
      return json({ total: 0, resolved: 0, needs_review: 0 });
    }),
  );
}

describe('enqueueFiles', () => {
  beforeEach(() => {
    openImport(); // resets importState
    vi.restoreAllMocks();
  });

  it('imports files sequentially and records each outcome', async () => {
    const seen: string[] = [];
    stubFetch((name) => {
      seen.push(name);
      return name === 'a.pdf'
        ? { outcome: 'ingested', id: '1', title: 'A', status: 'resolved' }
        : { outcome: 'duplicate' };
    });

    await enqueueFiles([pdf('a.pdf'), pdf('b.pdf')]);

    expect(seen).toEqual(['a.pdf', 'b.pdf']); // one at a time, in order
    expect(importState.items.map((i) => i.status)).toEqual(['ingested', 'duplicate']);
    expect(importState.items[0].message).toBe('A');
  });

  it('marks a rejected upload as failed with the server message', async () => {
    stubFetch(() => ({ error: 'not a PDF' }));

    await enqueueFiles([pdf('bad.pdf')]);

    expect(importState.items[0].status).toBe('failed');
    expect(importState.items[0].message).toBe('not a PDF');
  });
});
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cd frontend && npx vitest run src/components/ImportModal.test.ts`
Expected: FAIL — `enqueueFiles` / `importState` / `openImport` are not exported yet.

- [ ] **Step 3: Implement the import state**

In `frontend/src/lib/state.svelte.ts`, update the first import line to add `importPaper`:

```ts
import { deletePaper, getPaper, getStats, importPaper, listPapers } from './api';
```

Extend the `ui` state and its helpers (replace the existing `ui`/`toggleSidebar` block):

```ts
export const ui = $state<{ sidebarOpen: boolean; importOpen: boolean }>({
  sidebarOpen: true,
  importOpen: false,
});
export function toggleSidebar(): void {
  ui.sidebarOpen = !ui.sidebarOpen;
}
export function openImport(): void {
  importState.items = [];
  importState.cancelled = false;
  ui.importOpen = true;
}
export function closeImport(): void {
  importState.cancelled = true;
  ui.importOpen = false;
}
```

Append the import queue and drain loop at the end of the file:

```ts
export interface ImportItem {
  name: string;
  status: 'queued' | 'importing' | 'ingested' | 'duplicate' | 'failed';
  message?: string;
}

export const importState = $state<{ items: ImportItem[]; cancelled: boolean }>({
  items: [],
  cancelled: false,
});

// Files waiting to upload, paired with their row index in importState.items.
const pending: { file: File; index: number }[] = [];
let draining: Promise<void> | null = null;

/// Queue files for import and (re)start the sequential drain. Resolves when the
/// current batch finishes.
export function enqueueFiles(files: File[]): Promise<void> {
  for (const file of files) {
    const index = importState.items.push({ name: file.name, status: 'queued' }) - 1;
    pending.push({ file, index });
  }
  if (!draining) {
    draining = drainQueue().finally(() => {
      draining = null;
    });
  }
  return draining;
}

async function drainQueue(): Promise<void> {
  while (pending.length > 0) {
    if (importState.cancelled) {
      pending.length = 0;
      break;
    }
    const { file, index } = pending.shift()!;
    importState.items[index].status = 'importing';
    try {
      const res = await importPaper(file);
      if (res.outcome === 'duplicate') {
        importState.items[index].status = 'duplicate';
      } else {
        importState.items[index].status = 'ingested';
        importState.items[index].message = res.title ?? '(untitled)';
      }
    } catch (e) {
      importState.items[index].status = 'failed';
      importState.items[index].message = (e as Error).message;
    }
  }
  // Reflect the newly ingested papers in the sidebar list and counts.
  await loadPapers();
  await loadStats();
}
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `cd frontend && npx vitest run src/components/ImportModal.test.ts`
Expected: PASS (both cases).

- [ ] **Step 5: Type-check and run the full frontend test suite**

Run: `cd frontend && npm run check && npm run test`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add frontend/src/lib/state.svelte.ts frontend/src/components/ImportModal.test.ts
git commit -m "feat(web-import): import queue state + sequential drain (tested)"
```

---

## Task 5: Import modal component + top-bar button

**Files:**
- Create: `frontend/src/components/ImportModal.svelte`
- Modify: `frontend/src/components/TopBar.svelte`
- Modify: `frontend/src/App.svelte`

- [ ] **Step 1: Create the modal component**

Create `frontend/src/components/ImportModal.svelte`:

```svelte
<script lang="ts">
  import { Check, CircleAlert, Copy, Loader, Upload, X } from 'lucide-svelte';
  import { closeImport, enqueueFiles, importState } from '../lib/state.svelte';

  let dragging = $state(false);
  let input: HTMLInputElement;

  function pick(list: FileList | null) {
    if (!list) return;
    const files = Array.from(list).filter(
      (f) => /\.pdf$/i.test(f.name) || f.type === 'application/pdf',
    );
    if (files.length) void enqueueFiles(files);
  }

  function onDrop(e: DragEvent) {
    e.preventDefault();
    dragging = false;
    pick(e.dataTransfer?.files ?? null);
  }

  const summary = $derived.by(() => {
    const c = { ingested: 0, duplicate: 0, failed: 0 };
    for (const i of importState.items) {
      if (i.status in c) c[i.status as keyof typeof c]++;
    }
    return c;
  });
</script>

<div
  class="fixed inset-0 z-50 flex items-center justify-center bg-slate-900/50 p-4"
  role="dialog"
  aria-modal="true"
  aria-label="Import papers"
>
  <div class="flex max-h-[80vh] w-full max-w-lg flex-col rounded-xl bg-white shadow-xl dark:bg-slate-900">
    <div class="flex items-center justify-between border-b border-slate-200 p-4 dark:border-slate-800">
      <h2 class="text-base font-semibold">Import papers</h2>
      <button
        type="button"
        onclick={closeImport}
        aria-label="Close import"
        class="rounded-lg p-1.5 text-slate-500 hover:bg-slate-100 dark:text-slate-400 dark:hover:bg-slate-800"
      >
        <X size={18} />
      </button>
    </div>

    <div class="min-h-0 flex-1 overflow-y-auto p-4">
      <button
        type="button"
        onclick={() => input.click()}
        ondragover={(e) => {
          e.preventDefault();
          dragging = true;
        }}
        ondragleave={() => (dragging = false)}
        ondrop={onDrop}
        class="flex w-full flex-col items-center gap-2 rounded-xl border-2 border-dashed p-8 text-sm transition-colors {dragging
          ? 'border-indigo-400 bg-indigo-50 dark:bg-indigo-500/10'
          : 'border-slate-300 dark:border-slate-700'}"
      >
        <Upload size={24} class="text-slate-400" />
        <span class="text-slate-600 dark:text-slate-300">Drag PDFs here, or click to browse</span>
      </button>
      <input
        bind:this={input}
        type="file"
        accept=".pdf,application/pdf"
        multiple
        class="hidden"
        onchange={(e) => pick((e.currentTarget as HTMLInputElement).files)}
      />

      {#if importState.items.length}
        <ul class="mt-4 space-y-1">
          {#each importState.items as item, i (i)}
            <li class="flex items-center gap-2 rounded-lg px-2 py-1.5 text-sm">
              {#if item.status === 'importing'}
                <Loader size={14} class="shrink-0 animate-spin text-indigo-500" />
              {:else if item.status === 'ingested'}
                <Check size={14} class="shrink-0 text-emerald-500" />
              {:else if item.status === 'duplicate'}
                <Copy size={14} class="shrink-0 text-slate-400" />
              {:else if item.status === 'failed'}
                <CircleAlert size={14} class="shrink-0 text-red-500" />
              {:else}
                <span class="h-3.5 w-3.5 shrink-0 rounded-full border border-slate-300 dark:border-slate-600"></span>
              {/if}
              <span class="min-w-0 flex-1 truncate text-slate-700 dark:text-slate-200">{item.name}</span>
              <span class="shrink-0 text-xs text-slate-500 dark:text-slate-400">
                {#if item.status === 'ingested'}{item.message}
                {:else if item.status === 'duplicate'}duplicate
                {:else if item.status === 'failed'}{item.message}
                {:else if item.status === 'importing'}importing…
                {:else}queued{/if}
              </span>
            </li>
          {/each}
        </ul>
      {/if}
    </div>

    {#if importState.items.length}
      <div class="border-t border-slate-200 p-3 text-xs text-slate-500 dark:border-slate-800 dark:text-slate-400">
        {summary.ingested} ingested, {summary.duplicate} duplicate, {summary.failed} failed
      </div>
    {/if}
  </div>
</div>
```

- [ ] **Step 2: Add the "Import" button to the top bar**

In `frontend/src/components/TopBar.svelte`, update the imports and add the button. Change the script imports to:

```svelte
<script lang="ts">
  import { Library, Moon, PanelLeft, Sun, Upload } from 'lucide-svelte';
  import { openImport, stats, theme, toggleSidebar, toggleTheme } from '../lib/state.svelte';
</script>
```

Then, inside the right-hand `<div class="flex items-center gap-4">`, add the Import button immediately before the theme-toggle `<button>`:

```svelte
    <button
      type="button"
      onclick={openImport}
      class="inline-flex items-center gap-1.5 rounded-lg bg-indigo-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-indigo-700"
    >
      <Upload size={16} /> Import
    </button>
```

- [ ] **Step 3: Render the modal in `App.svelte`**

In `frontend/src/App.svelte`, add the import (alphabetical, after `EmptyState`):

```svelte
  import ImportModal from './components/ImportModal.svelte';
```

Then add the modal render immediately after the closing `</div>` of the root layout div (as the last element in the markup):

```svelte
{#if ui.importOpen}<ImportModal />{/if}
```

- [ ] **Step 4: Type-check and build the frontend**

Run: `cd frontend && npm run check && npm run build`
Expected: PASS — 0 type errors, and `dist/` is produced (this is what `rust-embed` serves).

- [ ] **Step 5: Commit**

```bash
git add frontend/src/components/ImportModal.svelte frontend/src/components/TopBar.svelte frontend/src/App.svelte
git commit -m "feat(web-import): drop-zone import modal + top-bar button"
```

---

## Task 6: Full build + manual smoke test

**Files:** none (verification only)

- [ ] **Step 1: Build the release binary with the embedded frontend**

Run: `cargo build`
Expected: PASS — `frontend/dist` (from Task 5) is embedded via `rust-embed`.

- [ ] **Step 2: Run the entire test suite one more time**

Run: `cargo test && (cd frontend && npm run test)`
Expected: PASS on both.

- [ ] **Step 3: Manual smoke test (optional but recommended)**

Start the server against your real config: `cargo run -- serve`
In the browser at `http://127.0.0.1:8080`:
- Click **Import** → the modal opens.
- Drag two PDFs (or click to browse) → each row goes `importing… → Ingested/duplicate`.
- Confirm the summary line and that the new papers appear in the sidebar and the header counts update.
- Drop a non-PDF (e.g. rename a `.txt`) → its row shows `not a PDF`.

- [ ] **Step 4: Final commit (if the manual test surfaced any tweaks)**

```bash
git add -A
git commit -m "chore(web-import): final polish after smoke test"
```

---

## Self-Review Notes

- **Spec coverage:** POST endpoint + one-file-per-request (Task 2), `%PDF` validation + 400 (Task 2 test + handler), staging into `inbox_dir/_uploads` (Task 2 handler; `staging_dir` set in Task 1 `main.rs`), `ingested`/`duplicate`/error mapping + title/status lookup (Task 2), 50 MB body limit + `multipart` feature (Task 1), `Option<Arc<Ingest>>` + `build_router`/`build_router_with_ingest` + 503 (Task 1), `serve`/`main.rs` wiring (Task 1), drop-zone modal + sequential per-file progress (Tasks 4–5), list/stats refresh (Task 4 `drainQueue`), top-bar button (Task 5), backend offline test + frontend state test (Tasks 2, 4).
- **Type consistency:** `Ingest { resolver, grobid, dirs, staging_dir }` is defined in Task 1 and used identically in Task 2's test and `main.rs`. `ImportResult` (Task 3) matches the handler's JSON (Task 2) and is consumed in `drainQueue` (Task 4). `importState`/`enqueueFiles`/`openImport`/`closeImport` names are identical across Tasks 4 and 5. `import_paper`, `bad_request`, `multipart_error` names are consistent between Tasks 1 and 2.
- **Edge cases carried from the spec:** non-PDF (400), duplicate (distinct, not error), ingest error (500 + staged-file cleanup), traversal (basename-only staged name), ingest-not-configured (503).
