<div align="center">

<img src="assets/logo.svg" width="112" height="112" alt="Xuewen — the 學 seal" />

# 學問 · Xuewen

</div>

A self-hosted reference manager for research papers (currently for computer science only).

## The name

**學問** (xuéwèn, simplified 学问) — "learning / scholarship." The two
characters double as the interface marks: **學** (xué, *learning*) is the
cinnabar seal wordmark, and **問** (wèn, *to ask*) is the amber chat launcher —
the assistant that answers questions about a paper.

## Features

- **Automatic metadata** — resolved from arXiv, Crossref and DBLP, with optional
  [GROBID](https://github.com/kermitt2/grobid) header extraction as a fallback.
  Papers are filed under a deterministic cite key (e.g. `vaswani2017attention`).
- **Manual identify** — when auto-resolution is unsure, match a paper to a DOI,
  arXiv id, or a title search from the UI or CLI.
- **Search** — BM25 keyword search (always on) plus optional semantic search
  over title/abstract/body chunks, fused into one ranked list.
- **Paper chat** — optional per-paper conversations with an LLM, grounded in the
  extracted full text (any OpenAI-compatible endpoint; per-model
  `reasoning_effort`).
- **Daily arXiv recommendations** — a ranked, LLM-summarized feed of new papers
  scored against your library's interests (optional).
- **Citation export** — BibTeX / BibLaTeX for a single paper, a project, or the
  whole library.
- **Paywall helper** — optional institutional (EZproxy) support for fetching
  PDFs you have access to.

## Architecture

| Layer | Tech |
| --- | --- |
| Backend | Rust — [Axum](https://github.com/tokio-rs/axum), [SQLx](https://github.com/launchbadge/sqlx)/SQLite, [Tantivy](https://github.com/quickwit-oss/tantivy) (keyword), [Qdrant](https://qdrant.tech/) (vectors), reqwest |
| Frontend | [Svelte 5](https://svelte.dev/) + Vite + Tailwind, embedded via `rust-embed` |
| Text extraction | `pdftotext` (poppler-utils) |

## Quick start

With [Nix](https://nixos.org/) (recommended — pulls in cargo, node, poppler,
sqlite):

```sh
nix develop            # or: direnv allow

# Build the frontend (rust-embed bundles frontend/dist into the binary).
npm --prefix frontend install
npm --prefix frontend run build

# Configure, then run the web UI.
cp xuewen.example.toml xuewen.toml   # edit paths / optional sections
cargo run -- serve                   # http://127.0.0.1:8080
```

Without Nix, install Rust, Node.js and poppler-utils yourself, then run the same
commands.

### Frontend hot-reload (development)

Run the backend and the Vite dev server side by side — Vite proxies `/api` and
`/papers` to the backend:

```sh
cargo run -- serve                   # backend on :8080
npm --prefix frontend run dev        # UI on :5173, hot-reloads
```

### Prebuilt binary

```sh
nix build            # ./result/bin/xuewen  (frontend already embedded)
```

## Configuration

Copy `xuewen.example.toml` to `xuewen.toml`. Only three keys are required:

```toml
inbox_dir    = "./inbox"
library_root = "./library"
database_url = "sqlite:./library.db"
```

Optional sections enable the richer features:

- `[search.embedding]` + `qdrant_url` — semantic search.
- `[[chat.models]]` — paper chat (one entry per selectable model).
- `[daily]` — daily arXiv recommendations.
- `[proxy]` — institutional paywall access.

API keys are read from environment variables via `api_key_env` (e.g.
`OPENAI_API_KEY`), so they never need to live in the config file. See
`xuewen.example.toml` for the fully documented set of options.

## CLI

The same binary drives everything from the terminal:

| Command | Purpose |
| --- | --- |
| `serve` | Run the web UI (loopback by default; `--allow-remote` to bind publicly) |
| `ingest <pdf>` | Ingest a single local PDF |
| `import <input>` | Import by arXiv id, DOI, or URL |
| `watch` | Auto-ingest new PDFs dropped in the inbox |
| `identify <id>` | Manually match a paper (`--doi` / `--arxiv` / `--title`) |
| `refresh` | Re-resolve failed records and re-file to cite-key paths |
| `search <query>` | Search from the terminal (`--keyword-only` / `--semantic-only`) |
| `export` | Emit BibTeX / BibLaTeX (single, project, or whole library) |
| `project` | Manage projects (named groups of papers) |
| `index` | Inspect or rebuild the search indexes |
| `delete` / `restore` / `purge` | Trash lifecycle |
| `proxy-cookie` | Manage the stored EZproxy session cookie |

Run `xuewen --help` (or `xuewen <command> --help`) for the full flags.

## Deployment

- **NixOS module** — `nixosModules.default` provides a hardened systemd service.
  See [`deploy/nixos/README.md`](deploy/nixos/README.md).
- **Container image** — a minimal OCI image is built with `nix2container`; a
  Kubernetes example lives in [`deploy/k8s/`](deploy/k8s/).

## Development

```sh
cargo test                       # backend unit + integration tests
npm --prefix frontend test       # frontend unit tests (Vitest)
npm --prefix frontend run check  # svelte-check / TypeScript
```

`nix flake check` builds the packages and runs the checks (including a NixOS VM
test on Linux).

## License

[MIT](LICENSE) © Jie Zhu
