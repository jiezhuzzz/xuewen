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
npm --prefix agent-runner install      # once, only needed for Agent Ask ([ai.agent.*])

cargo run -- serve                     # web UI at http://127.0.0.1:8080 (loopback; --allow-remote to bind publicly)
nix build                              # ./result/bin/xuewen with the frontend baked in

cargo run -p xuewen-desktop            # macOS desktop app (dev; opens a window)

# Frontend hot-reload dev: run both; Vite (:5173) proxies /api and /papers to the backend (:8080).
cargo run -- serve
npm --prefix frontend run dev
```

Tests & checks:

```sh
cargo test                             # backend unit + integration tests
cargo test -p xuewen-desktop           # desktop crate unit tests
cargo test <name_substring>            # a single backend test
cargo clippy                           # lint
cargo fmt

# Frontend tests MUST run from frontend/ — a bare `npx vitest` at the repo root mis-resolves.
npm --prefix frontend test             # vitest run (whole suite)
cd frontend && npx vitest run src/lib/foo.test.ts   # a single frontend test file
npm --prefix frontend run check        # svelte-check (TypeScript)
npm --prefix agent-runner test         # agent runner protocol tests (node --test)

nix flake check                        # builds packages + checks (a NixOS VM test on Linux)
```

Semantic search and the daily feed need a running **Qdrant** (`http://localhost:6333`) plus `[ai.embedding]` configured; keyword search, chat, and everything else do not.

Desktop crate builds (`cargo build`/`test -p xuewen-desktop`) link against macOS system frameworks (WebKit, AppKit) through Tauri; if that fails inside the Nix shell, fall back to running the same command outside it (plain `cargo`, via a rustup toolchain) rather than fighting the sandboxed linker. To produce the `.dmg`: `bash desktop/scripts/prepare-sidecars.sh` (bundles Node + `pdftotext` as sidecars; needs Homebrew `poppler` and `dylibbundler`), then `cd desktop && npx @tauri-apps/cli build`.

## Architecture

**Workspace: server crate + desktop crate.** The repo is a Cargo workspace (`[workspace] members = [".", "desktop"]`): the root `xuewen` crate is the CLI/server; `desktop/` (`xuewen-desktop`) is a Tauri v2 crate that boots the *same* Axum stack in-process — `xuewen::server::spawn_services` builds every service from a `Config`, then `xuewen::server::serve_on`/`web::serve_on` serve them on a listener the desktop crate binds itself (`127.0.0.1:0`, so it never collides with a `cargo run -- serve` on `:8080`) — and opens a webview at that address instead of mounting routes for a terminal user to hit. Bundled `pdftotext`/`node` sidecars are found by prepending their directory (`Contents/MacOS` inside the `.app`) onto `PATH` before the backend starts, so `src/pdf.rs` and `src/agent`'s plain `Command::new("pdftotext"/"node")` resolve them without any desktop-specific code path. Desktop config bootstraps into `~/Library/Application Support/Xuewen/` on first launch (`desktop/src/bootstrap.rs`) and is otherwise the same `xuewen.toml` the server reads.

**Single binary, embedded SPA.** `src/main.rs` is the CLI; `serve` starts the web server. The Svelte frontend is built to `frontend/dist` and embedded via `rust-embed` (`src/web/assets.rs`). `build.rs` writes a placeholder `index.html` when `frontend/dist` is missing so `cargo build`/tests work without a frontend build. In **debug** builds `rust-embed` reads `frontend/dist` from disk at request time — a frontend rebuild is served live with no Rust recompile; **release** (`nix build`) bakes it in.

**Config (`src/config.rs`).** One `xuewen.toml` (`--config` to override; `xuewen.example.toml` is the documented template). Only three keys are required: `inbox_dir`, `library_root`, `database_url`. All LLM/AI settings live under **`[ai]`** (shared `base_url`/`api_key_env`/`model`); each use — `[ai.embedding]`, `[ai.chat]`, `[ai.summary]`, `[ai.daily]`, `[ai.citations]` — is `#[serde(flatten)]`-ed and overrides only what differs (`AiConfig::resolve`). API keys come from the environment via `api_key_env` (default `OPENAI_API_KEY`), never the file. A feature is **off unless its section is present**.

**Storage & search.** SQLite via SQLx (`src/db.rs`); `migrations/` run automatically on startup (`sqlx::migrate!`). Keyword search is Tantivy (always on); semantic search is Qdrant (optional). `SearchService` (`src/search/`) fuses BM25 + vector hits into one ranked list; the embedder + Qdrant store are built from `[ai.embedding]` + `[search]`.

**Ingest pipeline (`src/pipeline.rs`).** PDF → `pdftotext` extraction (`src/pdf.rs`) → metadata resolution (`src/resolve/`: arXiv, Crossref, DBLP, Unpaywall OA, GROBID header fallback) → dedupe + deterministic cite key (`src/matching.rs`, `src/naming.rs`) → filed under `library_root` → indexed into Tantivy + Qdrant. `ingest` takes a local PDF; `import` fetches one first (arXiv direct → EZproxy proxy → Unpaywall OA, see `src/import.rs`); `watch` auto-ingests the inbox.

**Services (wired in `serve`).** `SearchService`, `DailyService` (ranked daily arXiv recs), `SummaryService` (per-paper LLM summaries), `CitationsService` (per-paper reference parsing), and `TranslateService` (LLM/DeepL selection translation; `POST /api/translate`) are each built `from_config` and disabled when their config is absent. `AgentService` (`src/agent/`) powers the per-paper Ask tab: each turn spawns `node agent-runner/src/runner.mjs` (Claude Code / Codex SDKs) over a read-only workspace `<library_root>/agent/<paper_id>/` (`paper.txt` + optional `repo/` shallow clone; attach via `PUT /api/papers/{id}/code` or `xuewen code set`) — "read-only" bounds writes/exec/network, not reads: neither backend's sandbox is path-scoped, so it can read any file on disk the server process's user can read (see README's "Sandbox boundary, honestly"). Gated by `[ai.agent.claude_code]` / `[ai.agent.codex]`; needs Node ≥ 20 and `npm --prefix agent-runner install`. `web::serve` (`src/web/`: `mod.rs` routes, `api.rs`, `chat.rs`, `dto.rs`, `assets.rs`) mounts the Axum routes plus the SPA. The web UI has **no auth** — mutating endpoints are loopback-only unless `--allow-remote`.

**Organization: projects, tags, star.** Always on (plain SQLite CRUD in `src/db.rs`, no `[ai.*]` gate) — not a `from_config` service. Projects are named groups of papers (many-to-many via a join table); a project carries only a name (and paper count) — it no longer has a `note` field (dropped in migration `0016_drop_project_note.sql`). Tags are free-form labels, also many-to-many, support `/`-nested names (e.g. `nlp/eval`), and are garbage-collected when their last paper reference is removed. Star is a plain boolean flag on the paper row. All three are exposed both from the CLI (`project`, `tag`, `star`/`unstar`) and from the web UI's `FilterRow.svelte` pill bar (filter, create, and — Task 14 — a hover "⋯" menu per pill for rename/delete).

**CLI subcommands** (all in `src/main.rs`): `serve`, `ingest`, `import`, `watch`, `identify` (`--doi`/`--arxiv`/`--title`), `refresh`, `search`, `export` (BibTeX/BibLaTeX), `project`, `tag`, `star`, `unstar`, `index` (`status`/`rebuild`), `summarize`, `code` (`set`/`status`/`rm` — attach/inspect/detach a paper's code repo for Agent Ask), `delete`/`restore`/`purge`, `proxy-cookie`.

**Frontend (`frontend/`).** Svelte 5 (runes) + Vite + Tailwind v4 + Vitest/@testing-library. Reactive app state is in `src/lib/state.svelte.ts` (`$state` stores for tabs/viewer, theme, selection, UI). `src/lib/api.ts` is the fetch client; `src/lib/shortcuts.ts` is the window-level single-key keymap.

## PDF viewer (EmbedPDF) gotchas

The reader is hand-rolled from EmbedPDF's primitives (not the ready-made `@embedpdf/svelte-pdf-viewer`). Engine/plugin config is centralized in `src/lib/pdfEngine.ts` (`ENGINE_OPTIONS`, `viewerPlugins()`); `PdfViewer.svelte` renders one persistent `PdfTab.svelte`/`PdfDeck.svelte`/`PdfPages.svelte` per open tab. Several settings are load-bearing and documented inline — don't strip them:

- Page rendering is layered (`PdfPages.svelte`'s `renderPage` snippet): `RenderLayer` with **`scale={1}`** (a cheap base rendered once, CSS-scaled to fit) plus `TilingLayer` (crisp tiles for the visible area only, drawn at the real zoom). Removing `scale={1}` reverts to full-page re-renders on every zoom tick — the exact perf bug this layering fixed.
- `worker: true` — PDFium runs in EmbedPDF's stock **blob module worker**, not the main thread (an earlier attempt at `worker:true` hung on "Loading document…"; that was diagnosed and fixed on 2026-07-13, not a dead end — see the two points below). Two things had to be fixed to get there:
  - `wasmUrl: new URL('/pdfium.wasm', location.origin).href` — must be a **fully-qualified URL**, not a bare path. The blob worker's own `self.location` is itself a `blob:` URL, which has no hierarchical path to resolve `'/pdfium.wasm'` against — Chromium throws `Failed to parse URL from /pdfium.wasm` *inside the worker*, with no network entry and no error surfaced to the main thread, so the symptom is just an indefinite "Loading document…". A fully-qualified URL needs no base-relative resolution, so it works regardless of the worker's own location. The wasm is still **self-hosted** (copied from `@embedpdf/pdfium` by the `copy-wasm` npm prehook into `frontend/public/`) — the default is a jsDelivr CDN, which breaks offline. `fontFallback: null` (no external font fetches) still applies.
  - Any `PdfDocumentObject`/`PdfPageObject` your own code reads off EmbedPDF's Svelte bindings (e.g. `useDocumentState()`) is a **reactive Svelte `$state` proxy**. Handing one back into an engine call (e.g. `getPageAnnotations(doc, page)`) round-trips it through `postMessage` to the worker, and a live proxy throws `DataCloneError: ... could not be cloned`. Call `$state.snapshot(doc)` once before passing document/page objects into any engine call that might run in worker mode — see `PdfPages.svelte`'s citation-extraction effect.
- Citation extraction (that same effect) is scheduled with `runWhenIdle` (`src/lib/idle.ts`, a `requestIdleCallback` wrapper) rather than firing on first paint — PDFium calls are expensive even off the main thread (the worker round-trip still has to happen before first pages paint). The effect deliberately has **no cleanup on re-run**, only on `onDestroy`: an earlier bug had the initial FitWidth zoom re-firing this effect and cancelling a pending extraction before it ever finished; a one-shot `extractedFor`/`extractionCancelled` guard replaced the cleanup instead.
- Citation popovers show structured references (title/authors/venue/year): the frontend POSTs raw reference text to `/api/papers/{id}/citations` (`src/citations/`), which parses the big-4 CS styles by pattern matching (`src/citations/heuristic.rs`: style vote across entries, venue publisher-family tie-break, strict per-entry validation) and — when `[ai.citations]` is configured — sends only the leftover entries to the LLM; results are cached in SQLite so repeat opens of the same paper don't re-parse. The endpoint is always available; without `[ai.citations]` unparseable entries stay null and their popovers show the raw extracted text. PDFs with no hyperlinked citation annotations fall back further, to text-layer marker detection (`textCitations.ts`, numbered- and author-year-style segmentation in `citations.ts`).
- Inactive tabs are hidden with `visibility:hidden` (not `display:none`) so the viewer isn't remounted — preserving page/scroll and avoiding a thumbnail re-scroll on tab switch.
- Never call the thumbnail plugin's `scrollToThumb` (@embedpdf/plugin-thumbnail 2.14.4): its `scrollTo$` scoped emitter caches its last emission (`cache` defaults `true`; `refreshPages$` opts out of caching, `scrollTo$` doesn't), and `ThumbnailsPane` re-subscribes its `onScrollTo` effect on every window-state change — i.e. on every manual pane scroll — which replays the cached emission and pins the pane to whatever position was last commanded. Diagnosed live 2026-07-14 with stack traces, after two plausible-but-wrong effect-reactivity fixes. Fix: `PdfSidePanel.svelte` positions the pane on open by writing the pane element's `scrollTop` directly from `plugin.getDocumentState(id).thumbs[page].top - 8` (8 = upstream's own `margin` constant), reaching the pane element via a bound wrapper's `firstElementChild` — both couplings version-pinned to ~2.14.4 and guarded to degrade to "pane stays at top" if either breaks upstream. Registration keeps `{ autoScroll: false, scrollBehavior: 'auto' }`: `autoScroll: false` is the other half of the fix (auto-follow snapped the pane on every page change, fighting manual browsing via trackpad-momentum page changes).

## Deployment

NixOS module (`deploy/nixos/`, `nixosModules.default`) and an OCI image built with `nix2container` (`deploy/k8s/`). See `README.md` for the container/registry details.
