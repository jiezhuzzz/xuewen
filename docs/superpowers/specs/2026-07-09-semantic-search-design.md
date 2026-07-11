# Design: Hybrid search — keyword full-text (Tantivy) + semantic (Qdrant)

**Project:** Xuewen — self-hosted reference manager for CS papers.
**Date:** 2026-07-09
**Status:** Approved (design phase)

## 1. Purpose

Search today is `LIKE '%term%'` over title+authors (`db::list_papers`). It
cannot find a paper by something said in its body ("the paper that mentioned
AFL dictionary mutation"), cannot rank, and cannot match by meaning ("papers
about making binaries resistant to fuzzing" → *AntiFuzz*, *Fuzzification*).

This feature adds a uniform search box backed by two engines the user can
toggle per query:

- **Keyword** — BM25 full-text search over title / authors / abstract / body
  via an embedded **Tantivy** index.
- **Semantic** — embedding search over chunked full text via a **Qdrant**
  server, embeddings from an **OpenAI-compatible API**.

Results remain a ranked list of papers, each showing a highlighted snippet
and which field matched.

## 2. Decisions (settled during brainstorming)

| Decision | Choice |
|---|---|
| Search scope | Metadata **and full PDF text** (extracted with the existing `pdftotext` path) |
| Keyword engine | **Tantivy** (embedded, index directory next to `library.db`) |
| Vector store | **Qdrant as a service** (user runs it; REST API on `:6333`) |
| Embeddings | **OpenAI-compatible endpoint**, configurable `base_url`/`model`/`dims`/key; default `text-embedding-3-small`, 1536 dims |
| Indexing | **Background indexer** (approach B): imports never block on search indexing; a watcher-style tokio task indexes filed papers, backfills at startup, retries failures |
| UX | **One search box + toggle chips**: fields (Title/Authors/Abstract/Body) and engines (Keyword/Semantic) |
| Results | **Papers + snippets** (field tag, `<mark>` highlight, page number for body hits) |
| Fusion | **Reciprocal Rank Fusion** (k=60) when both engines run |
| Language | English only (Tantivy default tokenizer + English stemming) |

**Out of scope (YAGNI):** passage-level result UI, CJK tokenization, local
embedding models, reranking models, per-field semantic weighting, OCR for
scanned PDFs (papers whose `pdftotext` output is empty are searchable by
metadata/abstract only — no body text in either engine).

## 3. Architecture

**SQLite is the single source of truth** — metadata (existing `papers`
table), extracted chunks, and indexing state. Tantivy and Qdrant are
**derived indexes**: either can be deleted and rebuilt from SQLite alone.
Backups only need `library.db` + the PDFs.

New module `src/search/`:

| File | Responsibility |
|---|---|
| `chunker.rs` | Pure: page-aware chunking of `pdftotext` output |
| `fts.rs` | Tantivy index wrapper (schema, upsert, delete, query, snippets) |
| `vector.rs` | Qdrant REST client (`reqwest`); tests point its base URL at `wiremock` |
| `planner.rs` | Pure: staleness/tombstone computation (papers vs `search_index`) |
| `embedder.rs` | OpenAI-compatible `/v1/embeddings` client, batched |
| `indexer.rs` | Background worker: watches for stale papers, runs the pipeline |
| `fusion.rs` | Pure: Reciprocal Rank Fusion of ranked ID lists |
| `mod.rs` | `SearchService`: wires the above, owns the search entry point |

**Qdrant over REST, not the `qdrant-client` crate.** The crate pulls in the
tonic/prost gRPC stack; we need four calls (ensure collection, upsert points,
search, delete by filter), which is ~150 lines against the stable REST API
using the `reqwest` already in the tree — and `wiremock`-testable like every
other HTTP client in this project.

**New dependency:** `tantivy = "0.24"` only.

## 4. Data model

### 4.1 SQLite (one migration)

```sql
CREATE TABLE chunks (
  paper_id  TEXT NOT NULL REFERENCES papers(id) ON DELETE CASCADE,
  seq       INTEGER NOT NULL,        -- 0 = synthetic title+abstract chunk
  page      INTEGER,                 -- NULL for seq 0
  text      TEXT NOT NULL,
  PRIMARY KEY (paper_id, seq)
);

-- NOTE: deliberately NO foreign key — a row may outlive its paper and act
-- as a tombstone telling the indexer to remove Tantivy/Qdrant entries.
CREATE TABLE search_index (
  paper_id           TEXT PRIMARY KEY,
  content_hash       TEXT NOT NULL,  -- papers.content_hash at index time
  meta_hash          TEXT NOT NULL,  -- hash of title/abstract/authors/venue/year
  chunk_count        INTEGER NOT NULL DEFAULT 0,
  fts_indexed_at     TEXT,           -- Tantivy tier done
  vectors_indexed_at TEXT,           -- Qdrant tier done
  embed_model        TEXT,           -- model used for the stored vectors
  last_error         TEXT,
  attempts           INTEGER NOT NULL DEFAULT 0,  -- consecutive failures
  last_attempt_at    TEXT            -- for retry backoff
);
```

The two `*_indexed_at` columns are deliberately independent: Tantivy can be
fresh while vectors are pending (API/Qdrant down). Staleness is **computed by
scan, not by event plumbing** (`planner.rs`, pure): a paper is *stale* for a
tier when its row is missing, the tier's timestamp is NULL, `content_hash` no
longer matches the paper (re-imported file), or `meta_hash` no longer matches
(identify/refresh edited metadata — no mutation path needs to remember to
signal). A `search_index` row whose paper is trashed or gone is a *tombstone*:
the indexer removes the Tantivy doc, Qdrant points, and the row itself.

### 4.2 Chunking (`chunker.rs`)

`pdftotext` emits form feeds (`\f`) between pages. Chunking is page-aware so
body snippets can cite a page number:

- Split into pages, then paragraphs (blank-line runs).
- Pack paragraphs into chunks of **~1,200 chars** (≈300 tokens) with
  **~200-char overlap** between adjacent chunks; a paragraph longer than the
  budget is split at sentence boundaries, hard-split as a last resort.
- Chunks never span pages (keeps `page` exact; the overlap loss at page
  boundaries is acceptable).
- **`seq 0` is synthetic**: `title + "\n" + abstract` (skipped when both are
  missing). It gives semantic search a strong paper-level target and is what
  "Abstract"-scoped semantic search matches against.

### 4.3 Tantivy (`fts.rs`)

One document per paper. Fields: `paper_id` (STRING, stored), `title`,
`authors`, `venue`, `abstract`, `body` (all TEXT, stored — stored fields are
what the snippet generator reads). `body` is the concatenated chunk texts
(seq ≥ 1). Default tokenizer + English stemming. Upsert =
`delete_term(paper_id)` + `add_document` + commit after each paper (commit
cost is negligible at personal-library scale). Index
directory: `config.search.index_dir`, default `./search-index` (sibling of
`library.db`; added to `.gitignore`).

### 4.4 Qdrant (`vector.rs`)

- Collection `config.search.qdrant_collection` (default `xuewen`), vector
  size = `config.search.embedding.dims`, cosine distance. Created on startup
  if absent.
- One point per chunk. Point ID = **UUIDv5 of `"{paper_id}:{seq}"`**
  (deterministic → upserts are idempotent). Payload:
  `{paper_id, seq, page}` — chunk **text stays in SQLite**; snippets are
  looked up by `(paper_id, seq)`.
- Delete = filter on `payload.paper_id`.
- `QdrantStore` struct (`ensure_collection`, `upsert`, `search`,
  `delete_paper`) over REST; tests exercise it against `wiremock` — no trait
  indirection (dyn-async friction for no gain at this scale).

## 5. Background indexer (`indexer.rs`)

A tokio task started with the web server (and by CLI commands that need it),
mirroring the inbox watcher pattern:

1. **Wake-up**: a `tokio::sync::Notify` signalled after import-filed,
   identify/refresh, restore-from-trash, and trash/delete events — plus a
   startup scan (backfill) and a periodic retry tick (for rows with
   `last_error`, exponential backoff capped at 1 h).
2. **Per stale paper**: `pdftotext` (full document) → `chunker` → in one
   SQLite transaction replace `chunks` rows and update `search_index`
   (`content_hash`, `meta_hash`, `chunk_count`) → Tantivy upsert → set
   `fts_indexed_at`.
3. **Vector tier** (only if embedding is configured): embed chunks in batches
   of 64 → Qdrant upsert → set `vectors_indexed_at` + `embed_model`. Failure
   here records `last_error` but leaves the FTS tier intact.
4. **Trash/delete**: remove Tantivy doc, Qdrant points, `chunks` +
   `search_index` rows. Restore re-enqueues.

Papers whose PDF extracts to empty text (scanned) still index metadata fields
in Tantivy and the `seq 0` chunk in Qdrant; `chunk_count` reflects reality.

## 6. Search flow

### 6.1 Endpoint

`GET /api/search?q=…&fields=title,authors,abstract,body&engines=keyword,semantic&project=…&trashed=false`

Both lists default to "all available". A companion
`GET /api/search/status` returns indexing progress and tier availability
(`{fts: {indexed, pending, failed}, vectors: {…}, semantic_available,
reason?}`) for the UI and `xuewen index status`. Search response:

```jsonc
{
  "semantic": { "available": true },        // or {available:false, reason:"…"}
  "results": [ {
      "paper": { /* existing PaperDto */ },
      "match": { "engine": "keyword|semantic|both",
                 "field": "title|authors|abstract|body",
                 "snippet": "… with <mark>…</mark> …",
                 "page": 7 }                // body hits only
  } ]
}
```

### 6.2 Keyword path

Tantivy query restricted to the selected fields with boosts
`title^3, authors^2, abstract^1.5, body^1`; top 100 paper IDs by BM25.
Snippet from the best-scoring matched field via `SnippetGenerator`.

### 6.3 Semantic path

Embed the query (one API call) → Qdrant top 50 chunks → aggregate to papers
by best chunk score. Field toggles map coarsely: deselecting **Body** filters
to `seq = 0` points; deselecting both Title and Abstract filters to
`seq ≥ 1`. **Authors-only selection disables semantic** (the UI greys the
chip; the API just omits the engine). Semantic snippet = the best chunk's
text (trimmed ~200 chars) from `chunks`, with its page.

### 6.4 Fusion & hydration

When both engines ran: RRF with k=60 over the two ranked lists
(`score(p) = Σ 1/(60+rank)`); single-engine queries skip fusion. The fused ID
list is hydrated by one SQL query that re-applies real filters (project
membership, trash status) and preserves fusion order — Qdrant/Tantivy never
need to know about projects. Over-fetching (100/50) before filtering keeps
post-filter recall reasonable.

### 6.5 Sort

When `q` is non-empty the effective sort is **relevance** (fusion order);
existing sort options continue to apply to browsing without a query.

## 7. Frontend (Sidebar + results list)

- Search box unchanged in position. Below it, one row of small toggle chips:
  `Title · Authors · Abstract · Body` and `Keyword · Semantic`. Defaults:
  all fields on, both engines on. Chip state persists in the existing
  client-side settings store.
- **Latency split:** while typing (debounced) → keyword-only request; on
  Enter or ~400 ms pause → full request with the selected engines. The
  embedding API is hit once per settled query, never per keystroke.
- Result rows gain one snippet line (`<mark>` rendered as highlight) and a
  muted tag like `body · p.7` or `title`.
- Semantic unavailable → chip greyed with tooltip (reason from the API).
  Indexing in progress → subtle "indexing N papers…" note (count from
  `/api/search/status`).

## 8. Config (`xuewen.toml`)

```toml
[search]                                    # section optional
index_dir         = "./search-index"       # Tantivy (derived, rebuildable)
qdrant_url        = "http://localhost:6333"
qdrant_collection = "xuewen"

[search.embedding]                          # subsection optional
base_url    = "https://api.openai.com/v1"
model       = "text-embedding-3-small"
dims        = 1536
api_key_env = "OPENAI_API_KEY"              # or api_key = "…" inline
```

Tiered availability, never fatal:

| Situation | Behaviour |
|---|---|
| No `[search]` at all | Defaults apply; keyword works; semantic off unless embedding configured |
| No `[search.embedding]` / no key | Keyword only; semantic reported unavailable with reason |
| Qdrant unreachable | Keyword only; indexer retries vectors with backoff |
| `model` changed (same `dims`) | Vectors marked stale by the scan and re-embedded gradually; idempotent point IDs make the collection converge (brief mixed-space window) |
| `dims` changed (collection size mismatch) | Semantic disabled with reason until `xuewen index rebuild --vectors-only` recreates the collection |
| Tantivy dir missing/corrupt | Rebuilt automatically from SQLite `chunks` (derived data) |
| Embedding API 429/5xx | Batch retry with exponential backoff; `last_error` recorded; FTS unaffected |

## 9. CLI

- `xuewen index status` — per-tier counts (indexed / pending / failed), model.
- `xuewen index rebuild [--fts-only | --vectors-only]` — drop the tier(s) and
  re-derive from SQLite (re-extracting PDFs only when chunks are missing).
- `xuewen search "query" [--fields …] [--keyword-only | --semantic-only]` —
  same `SearchService` code path as the web endpoint; prints ranked results
  with snippets.

## 10. Testing

Following existing project style (`wiremock`, `tempfile`, `axum-test`):

- **`chunker`** — pure unit tests: page boundaries, overlap, long-paragraph
  splitting, `seq 0` synthesis, empty-text PDFs.
- **`fusion`** — pure unit tests: RRF ordering, single-list passthrough,
  both-engines boost.
- **`embedder`** — `wiremock` OpenAI-compatible server: batching, backoff on
  429, dims validation.
- **`vector.rs`** — `wiremock` Qdrant REST: upsert/search/delete payload
  shapes; higher-level tests point `QdrantStore` at a wiremock server too.
- **`fts.rs`** — real Tantivy in a `tempfile` dir: upsert/delete/query,
  field boosts, snippet extraction.
- **`planner`** — pure unit tests: staleness per tier, tombstones, backoff.
- **`indexer`** — wiremock Qdrant + wiremock embedder: staleness detection,
  FTS-succeeds-while-vectors-fail split, trash cleanup, backfill.
- **Web** — `axum-test` on `/api/search`: toggles, filter hydration,
  semantic-unavailable reporting.
- **End-to-end** — import a generated PDF (`printpdf`) → wait for indexer →
  find it by a body phrase via keyword search.
