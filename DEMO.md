# Running the demo

A pre-populated instance lives in `demo/` (gitignored): **8 papers** with filed
PDFs, a built keyword index, and **860 embedded vectors** for semantic search —
so keyword search, semantic search, paper chat, and summaries all work out of
the box. The OpenAI key and all paths are in `demo/xuewen.toml`, which also
has `[ai.citations]` configured — hovering a citation marker in the PDF
reader shows a parsed title/authors/venue/year card (the first hover per
paper pays one LLM call; the result is cached in SQLite, so later
hovers/reopens are instant).

## Prerequisites

- Enter the Nix dev shell: `nix develop` (or `direnv allow` — there's a
  `use flake` `.envrc`). This provides `cargo`, `pdftotext`, `nodejs`, etc.
- Build the frontend once (the web UI is embedded from `frontend/dist`):

  ```sh
  npm --prefix frontend install
  npm --prefix frontend run build
  ```

## Launch

From the repo root:

```sh
# 1) Qdrant — ONLY needed for semantic search. Skip it and keyword search +
#    chat still work. It reuses the demo's vector storage:
QDRANT__STORAGE__STORAGE_PATH="$PWD/demo/qdrant/storage" \
QDRANT__STORAGE__SNAPSHOTS_PATH="$PWD/demo/qdrant/snapshots" \
  nix run nixpkgs#qdrant &

# 2) The app — key + all data paths come from demo/xuewen.toml:
cargo run -- --config demo/xuewen.toml serve
```

Open <http://127.0.0.1:8080>.

## Stop

```sh
pkill -f 'xuewen .*serve'
pkill -f qdrant
```

## What's in `demo/`

```
demo/
├── xuewen.toml        # config (absolute paths into demo/; inline OpenAI key)
├── library.db         # SQLite — paper records (+ -wal / -shm)
├── library/           # filed PDFs
├── inbox/             # drop new PDFs here (auto-ingested by `watch`)
├── search-index/      # Tantivy keyword index (derived; rebuildable)
└── qdrant/storage/    # Qdrant vectors for semantic search
```

## Notes

- **API key:** stored inline as `api_key = "…"` in `demo/xuewen.toml`. `demo/`
  is gitignored, so it isn't committed. If you revoke/rotate the key, update
  that line (or switch to `api_key_env = "OPENAI_API_KEY"` and export it).
- **Rebuild derived data** if needed (e.g. after clearing Qdrant): with the
  server stopped, `cargo run -- --config demo/xuewen.toml index rebuild`
  (`--vectors` for Qdrant only). The Tantivy index and vectors are derived
  from `library.db` + the PDFs.
- **Fully offline** once running — the only outbound calls are to OpenAI for
  embeddings/chat/citation parsing (and metadata/PDF fetches when importing
  new papers). These calls are made server-side, so the browser itself never
  talks to anything but `127.0.0.1`.
