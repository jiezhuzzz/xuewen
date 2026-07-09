# Design: Identify (manual match, Plex-style)

**Project:** Xuewen — self-hosted reference manager for CS papers.
**Date:** 2026-07-08
**Status:** Approved (design phase)

## 1. Purpose

When automatic resolution fails, a paper lands in `needs_review` with no way to
tell the system the right answer. Motivating case (diagnosed 2026-07-08): a
USENIX paper whose cover sheet has no DOI, whose wrapped two-line title the
heuristic truncated, so the correct sole DBLP hit scored 0.648 against the
0.85 confidence gate and was rejected.

This feature adds a **manual identify** flow — like "Fix match" in Plex or
Jellyfin: the user supplies a DOI, an arXiv id, or a corrected title, the
system fetches/offers authoritative candidates, and the user's confirmation
replaces the gate. Metadata always comes from the bibliographic sources, never
hand-typed. A small pipeline fix for the root cause (wrapped-title truncation)
rides along.

## 2. Decisions (settled during brainstorming)

| Decision | Choice |
|---|---|
| Surfaces | **Web UI + CLI** (backend endpoint does the work; CLI is a thin wrapper) |
| Inputs | **DOI or arXiv id (direct fetch) + title search with a candidate picker** |
| Confidence gate | Bypassed by design — the user's pick **is** the gate |
| Metadata authority | Always from Crossref/arXiv/DBLP; no free-form field editing |
| Scope extras | **Include the `guess_title` wrapped-line fix**; GROBID docs nudge out of scope |
| Conflict semantics | Identifier already on a different paper → **409, no changes** (mirrors ingest's SameWork) |

## 3. Backend

### 3.1 Resolver: ungated candidate search

New method on `Resolver` (src/resolve/mod.rs):

```rust
/// Title-search candidates from DBLP then Crossref, WITHOUT the confidence
/// gate: deduped (by doi, else dblp_key), ranked by title similarity to
/// `query` (descending), capped at 8. Network failures degrade to fewer
/// (possibly zero) candidates, never an error.
pub async fn search_candidates(&self, query: &str) -> Vec<ResolvedMetadata>
```

Reuses the existing `dblp::fetch/parse` and `crossref::search/parse_search`
modules; ranking reuses `matching::title_similarity` (as a sort key only — no
threshold).

### 3.2 `GET /api/identify/search?q=<query>`

- Requires the ingest context (serve mode); otherwise `503 {"error":"identify
  not configured"}` — same pattern as import.
- Empty/whitespace `q` → `400`.
- Returns `200` with a JSON array of candidates via a new `Candidate` DTO
  (src/web/dto.rs): `title`, `abstract`, `authors` (array), `venue`, `year`,
  `doi`, `arxiv_id`, `dblp_key`, `url`, `source` — a lossless mirror of
  `ResolvedMetadata` (round-trips through `{"candidate": …}` without dropping
  the url or a Crossref abstract), built `From<&ResolvedMetadata>`.

### 3.3 `POST /api/papers/{id}/identify`

Body is exactly one of:

```json
{ "doi": "10.1145/…" }
{ "arxiv_id": "1706.03762" }
{ "candidate": { …full Candidate object from /api/identify/search… } }
```

Flow:

1. Ingest context present? else `503`. Paper exists? else `404`. Paper
   trashed? → `409 {"error":"paper is in the trash"}`.
2. Obtain metadata:
   - `doi` → `resolver.resolve(&Identifier::Doi(doi), None)` (Crossref fetch).
   - `arxiv_id` → `resolver.resolve(&Identifier::Arxiv(id), None)`.
   - Upstream returns nothing/errors → `404 {"error":"identifier not found"}`
     (the resolve layer already degrades network failures to `None`; a
     distinct 502 is not worth plumbing — the message covers both).
   - `candidate` → converted back to `ResolvedMetadata` as-is. Trust
     rationale: it originated from our own search seconds earlier, and this
     localhost API is unauthenticated everywhere (delete/import already
     exist); no new trust boundary is crossed.
3. **Conflict guard:** if the incoming metadata carries a doi/arxiv_id that
   `db::find_by_identifier` maps to a *different* paper (any trash state) →
   `409 {"error":"same work as <id>", "id": "<id>"}` , nothing modified.
4. Apply: `paper.meta = md` except `abstract_text`, which keeps the old value
   when the source has none (`md.abstract_text.or(old)` — DBLP has no
   abstracts; don't destroy a GROBID abstract). `status = Resolved`.
5. Re-file: recompute cite key (excluding own id) and move the PDF using the
   crash-safe copy → `update_paper` → remove-old sequence. This sequence is
   **extracted from `refresh_one` into a shared helper** (`refile_paper` in
   src/refresh.rs or pipeline.rs) so refresh and identify share one
   implementation; refresh behavior is unchanged.
6. Return `200` with the updated `PaperDetail` JSON.

## 4. CLI

```
xuewen identify <ID> --doi <DOI>
xuewen identify <ID> --arxiv <ARXIV_ID>
xuewen identify <ID> --title "<QUERY>" [--pick N]
```

- `<ID>` resolves exact-or-unique-prefix via `db::find_one` (like
  delete/restore/purge). The three inputs are mutually exclusive (clap
  `conflicts_with`); exactly one is required.
- `--title` prints a numbered candidate list (title, authors, venue, year,
  source). `--pick N` selects non-interactively; without it the command
  prints the list, a hint to re-run with `--pick`, and exits 0 (listing is a
  successful operation; no interactive pager — keeps `main.rs` wiring simple
  and scriptable).
- Before applying, print the fetched metadata (title, authors, venue, year,
  identifiers) and confirm `[y/N]` unless `--yes` (mirrors delete/purge).
- Trashed paper or identifier conflict → error message naming the other
  paper, exit 1.
- Implementation shares the same apply/re-file helper as the web endpoint
  (both go through the `IngestCtx`); the CLI uses the production retry
  policy it already has.

## 5. Web UI

- **Info panel** gains an "Identify…" button next to Delete (visible for all
  papers; most useful for `needs_review`).
- **Identify modal** (ImportModal styling): a single text input. Input is
  auto-classified: matches the DOI pattern (`10.\d{4,9}/…`) → direct DOI
  fetch; matches the arXiv pattern (`\d{4}\.\d{4,5}`, optional `vN`) → arXiv
  fetch; anything else → title search on Enter.
- Search results render as candidate rows: title, authors (truncated), venue
  + year, source badge (dblp/crossref). A direct DOI/arXiv input is staged
  as-is with the parsed value shown ("Direct identifier detected (…)"); Apply
  performs the authoritative fetch server-side. (The CLI, by contrast,
  previews the fetched record before confirming — the web flow trades that
  preview for one fewer round-trip, and a mis-typed identifier is recoverable
  by re-identifying.)
- Clicking a row selects it; an **Apply** button POSTs
  `/api/papers/{id}/identify` (with `{"candidate": …}` for picked rows,
  `{"doi"/"arxiv_id": …}` for direct fetches).
- On success: close the modal, invalidate that paper's detail cache, reload
  the paper list and stats, and update the open tab's title.
- Errors render inline in the modal (`same work as …` shows the conflicting
  paper's id; mapping it to a title when the loaded list has one is a
  possible polish follow-up; upstream-not-found shows "identifier not
  found").
- New api.ts functions: `identifySearch(q)`, `identifyPaper(id, body)`;
  state additions live in `state.svelte.ts` following the import-modal
  pattern (plain-TS testable).

## 6. Pipeline fix: wrapped-title heuristic

`identify::guess_title` (src/identify.rs) currently returns only the first
substantive line. New rule: after selecting that line, **join the immediately
following line** when BOTH hold:

- the first line ends "mid-phrase": its last word (lowercased, punctuation
  stripped) is in the joining set {`a an and by for from in of on or the to
  via with` } **or** the line ends with `:` or `-`;
- the next line is also substantive under the existing filter (≥8 chars, no
  `@`, no DOI, has alphabetic chars, not an arXiv banner).

Join with a single space (for a trailing `-`, join without a space and drop
the hyphen — standard de-hyphenation). At most one join (two lines total).

Effect on the motivating case: "AntiFuzz: Impeding Fuzzing Audits of" +
"Binary Executables" → full title → DBLP similarity 1.0 → auto-resolves.
Non-effect: "Attention Is All You Need" ends on "need" (not in the set) → no
join, unchanged.

Unit tests cover: the exact USENIX cover-sheet text, a trailing-`:` join, a
hyphen de-hyphenation join, a no-join single-line title, and a next-line-not-
substantive case (e.g. author line rejected because the first line didn't end
mid-phrase — note the rule joins on line-1 shape, accepting that a mid-phrase
ending followed by an author line joins wrongly; the similarity gate then
simply rejects as today, no worse than the status quo). An integration test
ingests an AntiFuzz-style PDF against a DBLP mock and asserts `resolved`.

## 7. Testing

- **Resolver:** `search_candidates` unit tests on the existing DBLP/Crossref
  fixtures (ordering, dedup, cap, no-gate).
- **Web (wiremock + axum-test):** search happy path + 400 + 503; identify by
  DOI (mocked Crossref) updating metadata/status/cite-key/file; identify by
  candidate; 409 conflict (other paper owns the DOI); 409 trashed; 404
  unknown id/identifier-not-found.
- **Refile helper extraction:** existing refresh tests (9) must pass
  unchanged — they are the safety net for the extraction.
- **Heuristic:** unit tests per §6 + the pipeline integration test.
- **Frontend:** vitest on the identify state functions (classification,
  search, apply, error mapping), mirroring the import tests.
- **CLI:** untested wiring by repo convention (db/apply layers are tested).

## 8. Out of scope

- GROBID setup documentation / nudges.
- Free-form metadata editing.
- Web trash view; identify for trashed papers (explicitly 409s).
- Relaxing or changing the automatic confidence threshold (0.85 stays).
- Batch identify.
