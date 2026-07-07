# Design: PDF Ingest → Metadata Pipeline (Slice 1)

**Project:** Xuewen — a self-hosted reference manager, tailored (for now) to computer-science papers.
**Date:** 2026-07-06
**Status:** Approved (design phase)

## 1. Purpose

Xuewen will eventually be a full reference manager with a backend and a web UI.
This spec covers only the **first slice**: an ingest pipeline that watches a
directory, and for each new PDF extracts a title/identifier, resolves precise
bibliographic metadata from authoritative sources, and records it in a database.

Everything else (web UI, search, auto-relocation of files, BibTeX export,
tagging, notes) is out of scope for this slice but the data model is designed so
those features can be added without a schema-identity migration.

## 2. Goals & non-goals

### Goals
- Detect a newly added PDF in a watched **inbox** directory.
- Identify the paper: prefer an exact identifier (DOI / arXiv ID); fall back to
  structural extraction of title/abstract.
- Resolve authoritative metadata, routing by identifier type.
- Persist a normalized record to SQLite with a stable, transferable identity.
- Never silently drop a file: unresolved PDFs are kept and flagged for review.

### Non-goals (deferred)
- Web UI / HTTP API.
- Auto-relocation of PDFs into `<venue>/<year>/…` (schema supports it; logic
  comes later).
- Multi-user, auth, sync/merge across servers (schema is merge-*ready* via UUID,
  but no merge tooling is built now).
- Full-text indexing / search.

## 3. Key decisions (settled during brainstorming)

| Decision | Choice | Rationale |
|---|---|---|
| Backend language | **Rust** | User preference; single static daemon binary. |
| Identification strategy | **DOI/arXiv first, then GROBID** | Exact IDs avoid guessing; GROBID handles the rest. |
| Metadata routing | **Source by identifier type** | arXiv→arXiv API, DOI→Crossref, title-only→DBLP (Crossref fallback). Each source used where strongest. |
| Metadata store | **SQLite (embedded)** | Single-file, self-host friendly, natural backing store for the future web UI. |
| Primary key | **UUIDv7 (TEXT)** | Server-independent, merge-safe, time-ordered for index locality. |
| File paths | **Relative to a library root** | Whole library is relocatable across mounts/servers. |
| File handling | **Inbox → copy into managed library**, then move original to `inbox/_processed/` | Keeps inbox clean; reversible; later relocation reorganizes within the managed root. |

## 4. Architecture

A single Rust daemon (async, tokio) with these internal units, each with one
clear purpose and a well-defined interface so it can be tested in isolation:

```
                 ┌─────────────┐
   inbox/*.pdf → │   Watcher   │  (notify + debounce)
                 └──────┬──────┘
                        │  IngestJob { path }
                        ▼
                 ┌─────────────┐
                 │   Ingest    │  orchestrates the stages below
                 │  pipeline   │
                 └──────┬──────┘
        ┌───────────────┼───────────────┬───────────────┐
        ▼               ▼               ▼               ▼
   Hash & dedup    Identify        Resolve         Store
   (sha2)          (text+regex)    (routed client) (sqlx/SQLite)
```

### 4.1 Watcher
- Uses the `notify` crate to watch the inbox for created/moved-in `.pdf` files.
- **Debounce**: wait until file size is stable across a short interval before
  enqueuing, so partially-written downloads are not processed.
- Emits `IngestJob { path }` onto an async queue (bounded channel).
- On startup, also scans the inbox once for any pre-existing PDFs (catch-up).

### 4.2 Ingest pipeline (per PDF)
Linear stages, each a pure-ish function over an in-memory context:

1. **Hash & dedup** — compute SHA-256 of the file bytes. If `content_hash`
   already exists in `papers`, skip (idempotent across restarts) and move the
   original to `inbox/_processed/`.
2. **Identify** — extract first-page(s) text and embedded XMP metadata; run
   regexes for:
   - DOI: `10\.\d{4,9}/[-._;()/:A-Za-z0-9]+`
   - arXiv ID: `arXiv:\d{4}\.\d{4,5}(v\d+)?` and legacy `arch-ive/YYMMNNN` forms.
   Produce an `Identifier` enum: `Doi`, `ArxivId`, or `None`.
3. **Resolve** — route by identifier (see §5).
4. **Confidence gate** (title-search path only) — normalize and fuzzy-compare
   the returned title against the extracted title (and optionally first author).
   Below threshold → mark `needs_review` instead of accepting a wrong match.
5. **Store** — copy the PDF into the library root as
   `library/<content_hash>.pdf` (flat for this slice), then upsert a `papers`
   row and move the original to `inbox/_processed/`.

### 4.3 PDF text extraction
- **Primary:** shell out to `pdftotext` (poppler-utils) — robust, well-tested,
  easy to declare as a Nix dependency.
- **Alternative (recorded, not chosen now):** `pdfium-render` binding if we want
  in-process extraction and font-size access later.

### 4.4 Configuration
A TOML config file (serde) with:
- `inbox_dir`, `library_root`
- `database_url` (SQLite path)
- `grobid_url` (e.g. `http://localhost:8070`)
- `contact_email` — sent in User-Agent for polite Crossref/DBLP/arXiv access
- rate-limit / retry knobs

## 5. Metadata resolution (source-by-identifier)

| Input | Source | Notes |
|---|---|---|
| arXiv ID | **arXiv API** (Atom XML) | Title, abstract, authors directly. Optionally cross-reference DBLP for a published venue. |
| DOI | **Crossref** (`/works/{doi}`, JSON) | Exact record; often includes abstract (JATS), venue, authors. |
| Title only | **GROBID** → **DBLP** search (JSON), **Crossref** fallback | GROBID extracts title/abstract/authors from the PDF; DBLP is queried by title (CS-focused, clean venues); Crossref used if DBLP has no confident match. |

- **GROBID** runs as a separate service (Docker, `http://localhost:8070`) and is
  invoked **only** on the title-only path (`processHeaderDocument`), returning
  TEI XML that we parse for title/abstract/authors.
- **Politeness:** every outbound request sends a descriptive User-Agent
  including `contact_email`; requests are rate-limited and retried with
  exponential backoff on transient failures.

### Normalized record fields
`title, abstract, authors[], venue, year, doi, arxiv_id, dblp_key, url`.

## 6. Data model

```sql
CREATE TABLE papers (
  id            TEXT PRIMARY KEY,   -- UUIDv7, generated at insert
  content_hash  TEXT UNIQUE,        -- SHA-256 of the PDF bytes (dedup + file identity)
  rel_path      TEXT,               -- relative to library_root, e.g. "<hash>.pdf"
  title         TEXT,
  abstract      TEXT,
  authors       TEXT,               -- JSON array of names (normalized later if needed)
  venue         TEXT,               -- <publication> — drives future relocation path
  year          INTEGER,            -- <year>        — drives future relocation path
  doi           TEXT UNIQUE,
  arxiv_id      TEXT UNIQUE,
  dblp_key      TEXT,
  url           TEXT,
  source        TEXT,               -- which resolver produced this: arxiv|crossref|dblp|grobid
  status        TEXT NOT NULL,      -- 'resolved' | 'needs_review'
  added_at      TEXT NOT NULL       -- RFC3339 timestamp
);

CREATE INDEX idx_papers_status ON papers(status);
CREATE INDEX idx_papers_year   ON papers(year);
```

**Identity model (three separated concepts):**
- **Paper identity** = `id` (UUIDv7) — the anchor; never changes.
- **File identity / dedup** = `content_hash` — changes only if bytes change.
- **File location** = `rel_path` — freely mutable; updated on any future move.

This is what makes both **auto-relocation** (just `UPDATE rel_path`) and
**server transfer** (copy `library.db` + the relative-path library dir; UUIDs
never collide on merge) painless.

## 7. Error handling & robustness

| Situation | Behavior |
|---|---|
| File still being written | Debounce until size stable; then process. |
| Duplicate (hash seen) | Skip insert; move original to `_processed/`; log. |
| No identifier + GROBID/search fails | Keep file; insert row with `status='needs_review'` and whatever fields were extracted. Never drop. |
| Low-confidence title match | `status='needs_review'`; store best candidate for later confirmation. |
| Network / GROBID down | Retry with exponential backoff; on exhaustion leave job failed/queued, file stays in inbox (not `_processed/`) for a later run. |
| Restart mid-run | `content_hash UNIQUE` + inbox re-scan makes reprocessing safe/idempotent. |

Logging via `tracing`; structured, leveled logs for each stage.

## 8. Testing strategy

- **Unit tests**
  - DOI / arXiv regex extraction over representative text snippets (incl. false-positive traps).
  - Title normalization + fuzzy match thresholds.
  - `content_hash` computation stability.
- **Integration tests (offline, deterministic)**
  - arXiv / Crossref / DBLP / GROBID clients run against **recorded fixture
    responses** (checked-in sample Atom/JSON/TEI), never live APIs.
- **Pipeline test**
  - Drop a checked-in open-access PDF into a temp inbox; assert a `papers` row
    appears with expected `title`, `doi`/`arxiv_id`, and `status='resolved'`,
    and that the file was copied to the library and the original moved to
    `_processed/`.

## 9. Suggested crates (finalized in the implementation plan)

`tokio`, `notify`, `reqwest` (json), `sqlx` (sqlite, migrate), `uuid` (v7),
`sha2`, `serde`/`serde_json`, `quick-xml` or `roxmltree` (Atom/TEI/DBLP XML),
`regex`, `strsim` (fuzzy title match), `toml`, `anyhow`/`thiserror`,
`tracing`/`tracing-subscriber`. System dependency: `pdftotext` (poppler-utils),
declared via the Nix flake.

## 10. Open items for the implementation plan

- Exact debounce interval and watcher edge cases (atomic-move vs streamed write).
- Retry/backoff policy specifics and where the failed-job queue lives (in-memory
  vs a `jobs` table for durability across restarts).
- Whether the first slice ships a small CLI (`ingest <file>`, `watch`) for manual
  testing alongside the daemon — recommended.
- Fuzzy-match threshold value and whether first-author check is required.
