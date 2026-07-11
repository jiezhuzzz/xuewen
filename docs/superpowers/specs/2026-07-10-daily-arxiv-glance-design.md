# Daily arXiv Recommendations as a Glance Source

**Date:** 2026-07-10
**Status:** Approved design, pending implementation plan

## Overview

Xuewen gains a daily arXiv recommendation feature modeled on
[zotero-arxiv-daily](https://github.com/TideDra/zotero-arxiv-daily), with the
Xuewen library playing the role of the Zotero corpus and a
[Glance](https://github.com/glanceapp/glance) `custom-api` widget playing the
role of the email. Once a day the server fetches yesterday's arXiv
announcements in the configured categories, ranks them by embedding
similarity to the user's library, generates an LLM TL;DR for each of the top
papers from its full text, stores the batch in SQLite, and serves it as JSON
at `GET /api/daily` for the Glance dashboard to render.

Everything runs natively in the existing Rust binary. The feature reuses the
search stack's `Embedder` (OpenAI-compatible embeddings API), the Qdrant
seq-0 title+abstract vectors as the library corpus, `pdf::extract_text` for
TL;DR input, and the axum server for delivery.

## Goals

- Daily ranked list of new arXiv papers relevant to the library, visible on
  the Glance dashboard.
- LLM TL;DR per recommended paper, generated from the paper's full text.
- Zero new infrastructure: no new services, no second language stack.
- Graceful degradation: the feature disables cleanly when unconfigured, and
  per-paper failures never kill a daily batch.

## Non-goals

- bioRxiv/medRxiv sources (the feed module leaves room, but only arXiv is
  built).
- Email delivery.
- A page in Xuewen's own frontend (Glance is the UI).
- Affiliation extraction (zotero-arxiv-daily has it; skipped as YAGNI).
- Backfilling missed days (the arXiv RSS feed is a live window; only the
  current announcement day is retrievable).

## Architecture

New module `src/daily/`, a background scheduler task spawned by `serve`, one
migration, and two API routes.

### Daily job pipeline

1. **Fetch** `https://rss.arxiv.org/atom/{cat1+cat2+...}` (one request for
   all configured categories), parsed with `roxmltree` (existing
   dependency). Keep entries whose `arxiv:announce_type` is `new` (plus
   `cross` when `include_cross_list = true`); `replace*` entries are always
   dropped. A well-formed feed titled "Feed error for query" signals a bad
   category list and fails the run with a config error.
2. **Dedup**: strip the version suffix (`v\d+$`) from each entry's arXiv id
   and drop candidates whose id already exists in `papers.arxiv_id`
   (including soft-deleted rows — a deleted paper was a deliberate removal).
3. **Build the interest profile**: scroll all seq-0 vectors from Qdrant
   (new `QdrantStore::scroll_summaries()`), join with `papers.added_at`
   (non-deleted papers only), sort newest-first, weight rank *i* (0-based)
   by `1/(1 + log10(i + 1))`, normalize weights to sum 1, defensively
   L2-normalize each vector, and sum the weighted vectors into a single
   profile vector. Scoring a candidate is then one dot product, which
   equals zotero-arxiv-daily's recency-weighted mean cosine similarity.
4. **Score**: embed candidate `title + "\n" + abstract` strings with the
   existing `Embedder` (same model as the library vectors), L2-normalize,
   score = dot(candidate, profile). Sort descending, keep the top
   `max_papers`.
5. **TL;DR** each kept paper, best-effort with a fallback chain:
   - Download the PDF from `https://arxiv.org/pdf/{id}` (60 s timeout,
     30 MB size cap), extract text via `pdf::extract_text(path, 12)` (first
     12 pages), truncate to 40,000 characters, prompt the LLM for a 2–3
     sentence TL;DR in the configured language (prompt adapted from
     zotero-arxiv-daily).
   - On any failure: retry the prompt with title+abstract only.
   - On failure again: store `tldr = NULL` (the widget falls back to an
     abstract snippet).
6. **Store** the batch in `daily_papers` and record the run in
   `daily_runs`; delete rows in both tables older than `retention_days`.

Raw scores are stored as floats; display formatting (rounding, ×10, etc.) is
the widget template's concern.

### Scheduler

A tokio task spawned by `serve` when the feature is active:

- Computes the next occurrence of `run_at` (a `"HH:MM"` UTC wall time) as a
  pure, unit-testable function; sleeps until then; runs the job.
- On failure, retries hourly until the (UTC) day ends.
- On server boot, runs immediately if today's batch is missing or failed
  (same-day catch-up only).
- A run-in-flight guard (atomic flag) prevents overlapping runs from the
  scheduler and the manual trigger.

### Activation

The feature is active only when the `[daily]` config section is present
**and** the search service is available with a working embedder (Qdrant +
`[search.embedding]` + API key). If `[daily]` is present but the search
stack is not, `serve` logs a warning and the feature stays off — the same
degradation pattern semantic search already uses. When off, both routes
return 503.

## Configuration

New optional section in `xuewen.toml`:

```toml
[daily]
categories         = ["cs.AI", "cs.LG"]  # required; arXiv category codes
include_cross_list = false               # include cross-listed papers
max_papers         = 20                  # ranked papers kept per day
run_at             = "09:00"             # daily run, UTC wall time
retention_days     = 14                  # prune batches older than this

[daily.llm]                              # chat-completions API for TL;DRs
base_url    = "https://api.openai.com/v1"
model       = "gpt-4o-mini"
api_key_env = "OPENAI_API_KEY"           # or: api_key = "sk-..."
language    = "English"
```

`DailyConfig` joins `Config` as `daily: Option<DailyConfig>`, with the same
`api_key` / `api_key_env` resolution as `EmbeddingConfig`. Defaults: every
field except `categories` and the `[daily.llm]` connection details has the
default shown above. Missing LLM key follows the embedding pattern: warn and
disable the feature.

## Storage

Migration `0008_add_daily.sql`:

```sql
CREATE TABLE daily_runs (
  batch_date   TEXT PRIMARY KEY,  -- YYYY-MM-DD (UTC) of the run
  status       TEXT NOT NULL,     -- 'ok' | 'empty' | 'failed'
  papers_found INTEGER NOT NULL,  -- candidates after dedup, before top-N
  error        TEXT,              -- populated when status = 'failed'
  ran_at       TEXT NOT NULL
);

CREATE TABLE daily_papers (
  batch_date TEXT NOT NULL,
  rank       INTEGER NOT NULL,    -- 1-based, by descending score
  arxiv_id   TEXT NOT NULL,       -- versionless
  title      TEXT NOT NULL,
  authors    TEXT NOT NULL,       -- JSON array (same convention as papers)
  abstract   TEXT NOT NULL,
  categories TEXT NOT NULL,       -- JSON array
  score      REAL NOT NULL,
  tldr       TEXT,                -- NULL when generation failed
  abs_url    TEXT NOT NULL,
  pdf_url    TEXT NOT NULL,
  PRIMARY KEY (batch_date, rank)
);
```

A re-run of an existing `batch_date` (manual trigger, boot catch-up after
`failed`) replaces that date's rows transactionally.

## API

Two routes on the existing axum router:

- `GET /api/daily` — the most recent batch that has papers:

  ```json
  {
    "date": "2026-07-10",
    "papers": [
      {
        "rank": 1,
        "arxiv_id": "2507.01234",
        "title": "...",
        "authors": ["..."],
        "abstract": "...",
        "categories": ["cs.LG"],
        "score": 0.83,
        "tldr": "...",
        "abs_url": "https://arxiv.org/abs/2507.01234",
        "pdf_url": "https://arxiv.org/pdf/2507.01234"
      }
    ]
  }
  ```

  On Monday this still shows Friday's batch, with its date. If no batch
  exists yet: `{"date": null, "papers": []}` with 200. Feature off: 503.
  No auth, consistent with the rest of the API (private network).

- `POST /api/daily/run` — manual trigger. 202 and runs in the background;
  409 if a run is already in flight; 503 when the feature is off. Serves as
  the test hook and a future escape hatch for external (CronJob) scheduling.

## Glance widget

A ready-to-paste `custom-api` widget snippet ships in the deploy docs
(`deploy/k8s/README.md`). Sketch — exact template syntax to be verified
against Glance docs during implementation:

```yaml
- type: custom-api
  title: Daily arXiv
  cache: 1h
  url: http://xuewen.<namespace>.svc.cluster.local/api/daily
  template: |
    {{ if .JSON.Array "papers" }}
    <ul class="list list-gap-14">
      {{ range .JSON.Array "papers" }}
      <li>
        <a class="size-h4 color-primary" href="{{ .String "abs_url" }}">{{ .String "title" }}</a>
        <div class="size-h6 color-subdue">
          {{ printf "%.2f" (.Float "score") }} · {{ .String "arxiv_id" }} ·
          <a href="{{ .String "pdf_url" }}">PDF</a>
        </div>
        <p>{{ if .Exists "tldr" }}{{ .String "tldr" }}{{ else }}{{ .String "abstract" }}{{ end }}</p>
      </li>
      {{ end }}
    </ul>
    {{ else }}
    <p>No papers yet.</p>
    {{ end }}
```

## Error handling

- Every run writes a `daily_runs` row — failures are recorded, not silent.
- Feed fetch uses HTTP retries with backoff (same spirit as the resolver
  retry design) before the run is marked `failed`.
- Empty feed (weekends, holidays) → status `empty`, zero papers; the
  endpoint keeps serving the last non-empty batch.
- No library vectors in Qdrant (index not built) → `failed` with a clear
  "no indexed library papers" error.
- Candidate embedding failure after the Embedder's built-in retries →
  `failed` (scores without embeddings are meaningless).
- TL;DR/PDF failures degrade per paper (fallback chain above) and never
  fail the batch.
- Scheduler failure retry: hourly until the UTC day ends.

## Module layout

```
src/daily/
  mod.rs        — DailyService: config, deps, run-in-flight guard
  feed.rs       — arXiv Atom fetch + parse → Candidate structs
  score.rs      — profile vector construction + candidate scoring
  tldr.rs       — chat-completions client, prompt, fallback chain
  job.rs        — orchestration: fetch → dedup → score → tldr → store → prune
  scheduler.rs  — next-run computation (pure) + tokio loop
  store.rs      — SQLite reads/writes for daily_runs / daily_papers
```

Touched elsewhere: `config.rs` (`DailyConfig`), `search/vector.rs`
(`QdrantStore::scroll_summaries()`), `web/api.rs` + `web/dto.rs` (routes and
response types), `web/mod.rs` (`AppState` gains an `Option<Arc<DailyService>>`),
`main.rs` (`serve` wiring + scheduler spawn), `migrations/0008_add_daily.sql`,
`deploy/k8s/README.md` (widget snippet), `xuewen.example.toml`.

## Testing

Follows existing patterns: wiremock for HTTP dependencies, fixture-driven
unit tests, real SQLite (in-memory) for storage.

- **feed.rs**: fixture Atom XML — entry parsing, `announce_type` filtering
  (new/cross/replace), version stripping, feed-error-title detection.
- **score.rs**: recency weights (a candidate matching a newly added library
  paper outranks one matching an old paper); profile-vector score equals
  the explicit weighted mean of per-paper cosine similarities.
- **tldr.rs**: wiremock chat API — happy path, full-text failure falling
  back to abstract-only, both failing → `None`.
- **job.rs end-to-end**: mocked feed + embeddings + chat + Qdrant scroll,
  real SQLite → asserts stored batch contents, dedup against library
  `arxiv_id`, `daily_runs` statuses (ok/empty/failed), pruning, and
  same-date re-run replacement.
- **API**: test router with seeded DB — populated batch, empty state,
  503 when off, 409 on concurrent manual trigger.
- **scheduler.rs**: pure next-run computation across day boundaries.
