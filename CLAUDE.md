# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Xuewen (學問) is a self-hosted reference manager for CS research papers: a single Rust binary that is both a CLI and an Axum web server, with a Svelte 5 SPA embedded into the binary.

## Dev environment

This is a Nix flake. Enter the dev shell with `nix develop` (or `direnv allow` — there's a `use flake` `.envrc`). Treat the shell as already active when `$IN_NIX_SHELL`/`$DIRENV_DIR` is set; otherwise wrap commands in `nix develop -c '<cmd>'`. The shell provides `cargo/rustc/clippy/rustfmt`, `poppler-utils` (`pdftotext`), `sqlite`, and `nodejs`.

## Common commands

```sh
# Frontend must be built before the embedded web UI works (see Architecture).
npm --prefix frontend install          # once
npm --prefix frontend run build        # build the SPA into frontend/dist

cargo run -- serve                     # web UI at http://127.0.0.1:8080 (loopback; --allow-remote to bind publicly)
nix build                              # ./result/bin/xuewen with the frontend baked in

# Frontend hot-reload dev: run both; Vite (:5173) proxies /api and /papers to the backend (:8080).
cargo run -- serve
npm --prefix frontend run dev
```

Tests & checks:

```sh
cargo test                             # backend unit + integration tests
cargo test <name_substring>            # a single backend test
cargo clippy                           # lint
cargo fmt

# Frontend tests MUST run from frontend/ — a bare `npx vitest` at the repo root mis-resolves.
npm --prefix frontend test             # vitest run (whole suite)
cd frontend && npx vitest run src/lib/foo.test.ts   # a single frontend test file
npm --prefix frontend run check        # svelte-check (TypeScript)

nix flake check                        # builds packages + checks (a NixOS VM test on Linux)
```

Semantic search and the daily feed need a running **Qdrant** (`http://localhost:6333`) plus `[ai.embedding]` configured; keyword search, chat, and everything else do not.

## Architecture

**Single binary, embedded SPA.** `src/main.rs` is the CLI; `serve` starts the web server. The Svelte frontend is built to `frontend/dist` and embedded via `rust-embed` (`src/web/assets.rs`). `build.rs` writes a placeholder `index.html` when `frontend/dist` is missing so `cargo build`/tests work without a frontend build. In **debug** builds `rust-embed` reads `frontend/dist` from disk at request time — a frontend rebuild is served live with no Rust recompile; **release** (`nix build`) bakes it in.

**Config (`src/config.rs`).** One `xuewen.toml` (`--config` to override; `xuewen.example.toml` is the documented template). Only three keys are required: `inbox_dir`, `library_root`, `database_url`. All LLM/AI settings live under **`[ai]`** (shared `base_url`/`api_key_env`/`model`); each use — `[ai.embedding]`, `[ai.chat]`, `[ai.summary]`, `[ai.daily]` — is `#[serde(flatten)]`-ed and overrides only what differs (`AiConfig::resolve`). API keys come from the environment via `api_key_env` (default `OPENAI_API_KEY`), never the file. A feature is **off unless its section is present**.

**Storage & search.** SQLite via SQLx (`src/db.rs`); `migrations/` run automatically on startup (`sqlx::migrate!`). Keyword search is Tantivy (always on); semantic search is Qdrant (optional). `SearchService` (`src/search/`) fuses BM25 + vector hits into one ranked list; the embedder + Qdrant store are built from `[ai.embedding]` + `[search]`.

**Ingest pipeline (`src/pipeline.rs`).** PDF → `pdftotext` extraction (`src/pdf.rs`) → metadata resolution (`src/resolve/`: arXiv, Crossref, DBLP, Unpaywall OA, GROBID header fallback) → dedupe + deterministic cite key (`src/matching.rs`, `src/naming.rs`) → filed under `library_root` → indexed into Tantivy + Qdrant. `ingest` takes a local PDF; `import` fetches one first (arXiv direct → EZproxy proxy → Unpaywall OA, see `src/import.rs`); `watch` auto-ingests the inbox.

**Services (wired in `serve`).** `SearchService`, `DailyService` (ranked daily arXiv recs), `SummaryService` (per-paper LLM summaries), and `ChatService` (per-paper chat) are each built `from_config` and disabled when their config is absent. `web::serve` (`src/web/`: `mod.rs` routes, `api.rs`, `chat.rs`, `dto.rs`, `assets.rs`) mounts the Axum routes plus the SPA. The web UI has **no auth** — mutating endpoints are loopback-only unless `--allow-remote`.

**CLI subcommands** (all in `src/main.rs`): `serve`, `ingest`, `import`, `watch`, `identify` (`--doi`/`--arxiv`/`--title`), `refresh`, `search`, `export` (BibTeX/BibLaTeX), `project`, `index` (`status`/`rebuild`), `summarize`, `delete`/`restore`/`purge`, `proxy-cookie`.

**Frontend (`frontend/`).** Svelte 5 (runes) + Vite + Tailwind v4 + Vitest/@testing-library. Reactive app state is in `src/lib/state.svelte.ts` (`$state` stores for tabs/viewer, theme, selection, UI). `src/lib/api.ts` is the fetch client; `src/lib/shortcuts.ts` is the window-level single-key keymap.

## PDF viewer (EmbedPDF) gotchas

The reader is hand-rolled from EmbedPDF's primitives (not the ready-made `@embedpdf/svelte-pdf-viewer`). Engine/plugin config is centralized in `src/lib/pdfEngine.ts` (`ENGINE_OPTIONS`, `viewerPlugins()`); `PdfViewer.svelte` renders one persistent `PdfTab.svelte`/`PdfDeck.svelte`/`PdfPages.svelte` per open tab. Several settings are load-bearing and documented inline — don't strip them:

- `worker: true` — PDFium runs in EmbedPDF's stock **blob module worker**, not the main thread (an earlier attempt at `worker:true` hung on "Loading document…"; that was diagnosed and fixed on 2026-07-13, not a dead end — see the two points below). Two things had to be fixed to get there:
  - `wasmUrl: new URL('/pdfium.wasm', location.origin).href` — must be a **fully-qualified URL**, not a bare path. The blob worker's own `self.location` is itself a `blob:` URL, which has no hierarchical path to resolve `'/pdfium.wasm'` against — Chromium throws `Failed to parse URL from /pdfium.wasm` *inside the worker*, with no network entry and no error surfaced to the main thread, so the symptom is just an indefinite "Loading document…". A fully-qualified URL needs no base-relative resolution, so it works regardless of the worker's own location. The wasm is still **self-hosted** (copied from `@embedpdf/pdfium` by the `copy-wasm` npm prehook into `frontend/public/`) — the default is a jsDelivr CDN, which breaks offline. `fontFallback: null` (no external font fetches) still applies.
  - Any `PdfDocumentObject`/`PdfPageObject` your own code reads off EmbedPDF's Svelte bindings (e.g. `useDocumentState()`) is a **reactive Svelte `$state` proxy**. Handing one back into an engine call (e.g. `getPageAnnotations(doc, page)`) round-trips it through `postMessage` to the worker, and a live proxy throws `DataCloneError: ... could not be cloned`. Call `$state.snapshot(doc)` once before passing document/page objects into any engine call that might run in worker mode — see `PdfPages.svelte`'s citation-extraction effect.
- Inactive tabs are hidden with `visibility:hidden` (not `display:none`) so the viewer isn't remounted — preserving page/scroll and avoiding a thumbnail re-scroll on tab switch.

## Deployment

NixOS module (`deploy/nixos/`, `nixosModules.default`) and an OCI image built with `nix2container` (`deploy/k8s/`). See `README.md` for the container/registry details.
