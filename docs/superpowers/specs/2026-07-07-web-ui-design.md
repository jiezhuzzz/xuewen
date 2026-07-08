# Design: Xuewen Web UI — Svelte Reader (read-only)

**Project:** Xuewen — self-hosted reference manager for CS papers.
**Date:** 2026-07-07
**Status:** Approved (design phase)

## 1. Purpose

A local, read-only web UI to **browse, search, and read** the library. A new
`xuewen serve` command runs an axum server that exposes a small JSON API over the
existing SQLite store and streams the library PDFs, and serves an embedded Svelte
single-page app that renders it as a modern two-pane **reader**: a searchable
sidebar of papers on the left, and a tabbed **inline PDF viewer** (multiple PDFs
open at once) on the right.

Read-only: the UI never mutates the database or files. Ingest/refresh stay on the
CLI. Single-user, localhost — no authentication.

## 2. Decisions (settled during brainstorming)

| Decision | Choice |
|---|---|
| Scope | **Read-only** display (browse/search/read); no edits, no actions |
| Backend | **axum** server, reuses the CLI's `Config` + sqlx pool |
| Data flow | JSON API; the SPA fetches and renders it |
| Frontend | **Svelte + Vite + TypeScript + Tailwind CSS + lucide** (plain Svelte SPA, no SvelteKit server) |
| Layout | Two-pane: searchable **sidebar** + **tabbed inline PDF viewer** |
| PDF viewer | **iframe per tab** (browser-native viewer); multiple tabs open at once; PDF.js deferred |
| Serve/access | `xuewen serve [--host 127.0.0.1] [--port 8080]`, localhost, **no auth** |
| Asset delivery | Svelte build embedded in the binary via **rust-embed** (single binary) |
| Offline | Fonts + icons bundled into the build; **no CDN** |

## 3. Architecture

```
xuewen serve
  └─ axum Router (state: sqlx pool + library_root)
       ├─ GET /api/papers?q=&status=&sort=   → JSON list (summaries)
       ├─ GET /api/papers/:id                → JSON detail (incl. abstract)
       ├─ GET /api/stats                     → JSON counts
       ├─ GET /papers/:id/pdf                → streams library/<rel_path> (range-aware)
       └─ GET /* (everything else)           → embedded Svelte SPA (rust-embed), fallback index.html

frontend/  (Svelte + Vite + TS + Tailwind)
   npm run build → frontend/dist/  ──embedded──▶  rust-embed Assets
```

- The server is same-origin: the SPA, API, and PDFs are all served by axum, so
  **no CORS** is needed. In dev, Vite's dev server proxies `/api` and `/papers`
  to the running `xuewen serve`.
- New Rust deps: `axum` (0.8), `tower` (util — for `ServeFile.oneshot`),
  `tower-http` (fs — range-aware file serving), `rust-embed`, `mime_guess`.
  Dev-dep: `axum-test` (ergonomic handler tests).

## 4. Backend — JSON API

Handlers live in a new `src/web/` module (`mod.rs` router+serve, `api.rs`
handlers, `dto.rs` response types, `assets.rs` embedded-asset serving). The
`Resolver`/`Grobid`/pipeline are **not** used here — this is a read surface over
`db`.

### 4.1 Router construction (testable)

- `pub fn build_router(pool: SqlitePool, library_root: PathBuf) -> Router` — pure
  assembly of routes + shared `AppState { pool, library_root }`. Unit/integration
  tests build this directly (no socket needed).
- `pub async fn serve(host, port, pool, library_root) -> Result<()>` — binds a
  `TcpListener` and runs `axum::serve(listener, build_router(...))`. `main.rs`'s
  `serve` subcommand calls this.

### 4.2 DTOs (`serde::Serialize`)

- `PaperSummary` — `id, title, authors: Vec<String>, venue, year, doi, arxiv_id,
  dblp_key, cite_key, url, source, status, added_at`. `authors` is the parsed
  array (from `Paper::authors_vec()`), not the raw JSON string. **No abstract**
  (keeps the list payload light).
- `PaperDetail` — all of `PaperSummary` **plus** `abstract` (from
  `abstract_text`). Serialized with `#[serde(rename = "abstract")]`.
- `Stats` — `total, resolved, needs_review: usize`.

Both DTOs have a `from(&Paper)` conversion; `authors` via `Paper::authors_vec()`.

### 4.3 Endpoints

- `GET /api/papers` — query params (all optional): `q` (case-insensitive
  substring over title + authors), `status` (`resolved` | `needs_review`),
  `sort` (`year_desc` default | `year_asc` | `added_desc` | `title`). Returns
  `Vec<PaperSummary>`. Implemented as a `db::list_papers(pool, filter) -> Vec<Paper>`
  (parameterized SQL: `WHERE (:q IS NULL OR title LIKE … OR authors LIKE …) AND
  (:status IS NULL OR status = …) ORDER BY …`), mapped to summaries. Empty result
  → `[]`, 200.
- `GET /api/papers/:id` — `db::get_by_id`; `Some` → `PaperDetail` 200, `None` →
  404 (JSON `{"error":"not found"}`).
- `GET /api/stats` — `Stats` via count queries (or one `GROUP BY status`).
- `GET /papers/:id/pdf` — see §4.4.

### 4.4 PDF streaming (range-aware, path-safe)

Handler `async fn pdf(Path(id), State(app), req: Request) -> Response`:
1. `db::get_by_id(id)` → 404 if absent.
2. `let path = app.library_root.join(&paper.rel_path);`
3. **Path-safety guard** (defense in depth — `rel_path` comes from our own DB, and
   the client only supplies a UUID `id`, never a path): canonicalize `path` and
   `library_root`; if the canonical path does not start with the canonical
   `library_root`, return 404. If the file is missing, return 404.
4. Delegate to `tower_http::services::ServeFile::new(&path).oneshot(req)` — this
   sets `Content-Type: application/pdf` (from the `.pdf` extension), handles
   **HTTP Range** requests (seek/stream in the inline viewer), and conditional
   requests. No `Content-Disposition: attachment`, so browsers render inline.

### 4.5 SPA / asset serving

- `#[derive(RustEmbed)] #[folder = "frontend/dist"] struct Assets;`
- A fallback handler serves `Assets::get(path)` with a `mime_guess` content-type;
  if the path isn't an embedded asset (e.g. a client-side route), serve
  `index.html` (SPA fallback), so deep links work. API/`/papers` routes take
  precedence over the fallback.
- **`build.rs`** ensures `frontend/dist/` exists with a placeholder `index.html`
  when absent, so `cargo build`/tests work **without** a frontend build (rust-embed
  needs the folder to exist). A real `npm run build` overwrites the placeholder for
  production. `frontend/dist/` is git-ignored (build artifact).

## 5. Frontend — Svelte reader

`frontend/` — plain Svelte SPA (Vite, TypeScript, Tailwind). Build → `frontend/dist/`.

### 5.1 Layout & components

- **App shell**: full-height flex; left **Sidebar**, right **Viewer**. A thin top
  bar carries the app name, live **Stats** (total · resolved · needs_review), and a
  **theme toggle**.
- **Sidebar** (**collapsible** via a top-bar toggle; drag-to-resize deferred): a **search box** (debounced, drives
  `?q=`), a **status filter** (All / Resolved / Needs review), a **sort** control,
  and the **paper list** — compact rows: title (prominent), authors (muted, `et
  al.` past 3), and a meta line (`year · venue`, a **status pill**, mono cite-key
  badge). Clicking a row opens/focuses that paper's PDF tab.
- **Viewer** (main): a **TabBar** (one tab per open paper — title, close ✕,
  overflow scroll) + the active **PdfTab**. Each `PdfTab` is an
  `<iframe src="/papers/{id}/pdf">`; tabs stay mounted (hidden when inactive) so
  switching is instant. An **info** button toggles a slide-over **InfoPanel** with
  the paper's full metadata, abstract, and clickable **DOI / arXiv / DBLP / URL**
  links (fetched from `/api/papers/:id`).
- **EmptyState**: when no tabs are open, the Viewer shows a welcome + the library
  as a responsive **card grid** (same data, larger cards) as an alternate way in.
- **States**: skeleton/loading, empty-search, and error (failed fetch) states.

### 5.2 State (Svelte stores)

- `papers` (list + `loading`/`error`), driven by `filters` (`q`, `status`,
  `sort`) → refetch `/api/papers` on change (debounced for `q`).
- `tabs`: `{ id, title }[]` + `activeId`. Open = focus if present else push;
  close removes (and picks a neighbor). Detail metadata per tab is lazy-fetched
  and cached.
- `theme`: `light | dark`, initialized from `prefers-color-scheme`, toggle
  persists to `localStorage`, applied via Tailwind `dark` class on `<html>`.

### 5.3 Look & feel ("modern and beautiful")

- Generous whitespace, clear type hierarchy (bundled variable font, e.g.
  Fontsource Inter — no CDN), an accent color, subtle borders/shadows, rounded
  corners. **Status pill** colors: resolved = green, needs_review = amber.
  Full **dark + light** parity. The sidebar is **collapsible** via a top-bar
  toggle (giving the PDF full width); automatic collapse on narrow viewports is a
  future enhancement. Lucide icons (bundled).

### 5.4 API client & types

- `src/lib/api.ts`: typed `fetch` wrappers (`listPapers(filters)`,
  `getPaper(id)`, `getStats()`), `pdfUrl(id)` = `/papers/${id}/pdf`.
- `src/lib/types.ts`: `PaperSummary`, `PaperDetail`, `Stats` mirroring the DTOs.

## 6. Build integration

- `flake.nix` devShell gains `nodejs` (with npm).
- Two-stage build: `npm --prefix frontend ci && npm --prefix frontend run build`
  (→ `frontend/dist/`), then `cargo build` (embeds `dist/`).
- **Dev loop:** run `xuewen serve` (API on :8080) and `npm --prefix frontend run
  dev` (Vite on :5173, proxying `/api` + `/papers` → :8080) for hot-reload
  frontend iteration.
- `build.rs` placeholder (see §4.5) keeps `cargo build`/tests working standalone.

## 7. Error handling

- API: not-found → 404 JSON `{"error":"..."}`; bad query params → ignored/defaulted
  (never 500 on a stray filter value). DB errors → 500 JSON, logged via `tracing`.
- PDF: missing record or missing/rogue file → 404. Range/conditional handled by
  `ServeFile`.
- SPA: unknown non-API path → `index.html` (client routing). Failed fetches render
  an in-app error state, never a blank page.
- `serve` bind failure (port in use) → a clear `anyhow` error from `main`.

## 8. Testing

- **Backend (Rust, `axum-test` + seeded temp DB):** `/api/papers` shape + `q`,
  `status`, `sort` filters (incl. empty → `[]`); `/api/papers/:id` detail (with
  `abstract`) and 404; `/api/stats` counts; `/papers/:id/pdf` returns the bytes
  with `Content-Type: application/pdf`, honors a `Range` request (206 + partial),
  404s a missing id, and the **path-safety** guard rejects a record whose
  `rel_path` escapes `library_root`. `authors` serialized as a JSON array.
- **Frontend (vitest + @testing-library/svelte), kept light:** a couple of
  component smoke tests — a paper row renders title/status; opening two papers
  yields two tabs and switching changes the active iframe. Plus `npm run build`
  succeeds (type-check + bundle).
- All backend tests offline/deterministic (temp SQLite + tiny fixture PDF).

## 9. Decomposition (implementation plans)

Two plans (B builds on A):

- **Plan A — Backend (`serve` + JSON API + PDF streaming):** deps; `src/web/`
  module (`build_router`/`serve`, DTOs, handlers, embedded-asset fallback with a
  `build.rs` placeholder); `db::list_papers` + stats query; the `serve` CLI
  subcommand; Rust API tests. Produces a working JSON API + PDF streaming behind a
  placeholder page — fully testable without any frontend.
- **Plan B — Svelte reader:** `flake.nix` node toolchain; `frontend/` scaffold
  (Vite + Svelte + TS + Tailwind + lucide + bundled font); the sidebar, tabbed
  inline viewer, info panel, empty-state grid, theming; API client + types;
  vitest smoke tests; build wiring so `cargo build` embeds the real `dist/`.

## 10. Out of scope

- Any mutation (editing metadata, triggering ingest/refresh) — CLI only.
- Authentication / multi-user / non-localhost exposure.
- PDF.js custom-rendered viewer (iframe native viewer for v1).
- Full-text search inside PDFs; pagination/virtualized lists (fine at personal-
  library scale; revisit if the list grows to many thousands).
- A read API for external consumers beyond what the SPA needs.
