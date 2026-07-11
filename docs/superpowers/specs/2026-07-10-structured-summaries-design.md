# Structured Summaries for Daily arXiv Recommendations

**Date:** 2026-07-10
**Status:** Approved design, pending implementation plan
**Builds on:** `2026-07-10-daily-arxiv-glance-design.md` (merged)

## Overview

The daily recommendation job currently asks the LLM for a free-text 2–3
sentence TL;DR per paper. This increment upgrades that single call to
produce a structured five-part summary — TL;DR, problem, approach, key
results, limitations — plus a non-LLM "code available" link, and renders
the richer content in a collapsed `<details>` block on the Glance widget.
Same number of LLM calls, only longer output.

## Goals

- Structured per-paper summary from the one existing LLM call:
  `{tldr, problem, approach, results, limitations}`.
- A `code_url` (GitHub repository) extracted from the paper text by regex,
  no LLM involved.
- Widget stays scannable: TL;DR always visible, the rest collapsed.
- Full backward compatibility: old rows, the `tldr` column, and every
  existing consumer keep working; failures degrade exactly as today.

## Non-goals

- Relevance-to-your-library blurbs (the score already carries that signal).
- Keyword/topic lists (categories cover this).
- Additional LLM calls, new config keys, or new crate dependencies.
- Backfilling summaries for existing batches (old rows keep `summary = NULL`).

## Design

### Generation (`src/daily/tldr.rs`)

New type, serialized to/from JSON:

```rust
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Summary {
    pub tldr: String,
    pub problem: String,
    pub approach: String,
    pub results: String,
    pub limitations: String,
}
```

`generate_tldr` is replaced by:

```rust
pub async fn generate_summary(
    chat: &ChatClient,
    language: &str,
    title: &str,
    abstract_text: &str,
    full_text: Option<&str>,
) -> Option<Summary>
```

- One chat call. The prompt instructs the model to output ONLY a JSON
  object with exactly the five string keys, in the configured `language`,
  with length budgets: `tldr` one sentence; each other field 1–2
  sentences; ~120 words total. `results` should prefer concrete numbers
  (benchmark, metric, delta); `limitations` should draw on the paper's own
  limitations discussion when present.
- Parsing: trim the response, strip a Markdown code fence if the model
  wrapped one around the JSON (```json ... ``` or ``` ... ```), then
  `serde_json::from_str::<Summary>`. A parse failure is treated exactly
  like an API failure.
- Fallback chain unchanged in shape: full-text prompt → abstract-only
  prompt → `None` (each failure logged with `tracing::warn!`).
- No `response_format` parameter — plain instruction-based JSON keeps any
  OpenAI-compatible endpoint working. `ChatClient` itself is unchanged.
- `FULL_TEXT_CAP` and the retry/timeout behavior are unchanged.

### Code link (`src/daily/job.rs`)

After PDF text extraction (and independent of the LLM):

```rust
/// First GitHub repository URL in the text, if any.
fn find_code_url(text: &str) -> Option<String>
```

- Regex: `https?://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+` (regex
  crate is already a dependency; compiled once via `std::sync::LazyLock`).
- Trailing punctuation the PDF text may glue on (`.`, `,`, `)`, `;`) is
  trimmed from the match.
- Applied to the extracted (already 40k-char-capped) full text; `None`
  when the PDF was unavailable or contains no match.

### Storage (migration `0009_add_daily_summary.sql`)

```sql
ALTER TABLE daily_papers ADD COLUMN summary  TEXT;  -- JSON Summary object
ALTER TABLE daily_papers ADD COLUMN code_url TEXT;
```

- `store::DailyPaper` gains `summary: Option<tldr::Summary>` and
  `code_url: Option<String>`. `summary` is written as a JSON string and
  parsed on read (same convention as `authors`/`categories`); a NULL or
  unparsable stored value reads back as `None` (unparsable additionally
  warns — it indicates a bug, not bad input).
- The `tldr` column stays and is populated from `summary.tldr` when a
  summary was generated, `NULL` otherwise — the widget's compact line and
  all existing fallbacks keep working, including for pre-migration rows.
- `replace_batch` / `latest_batch` extend their column lists; everything
  else in the store is untouched.

### Job wiring (`src/daily/job.rs`)

Per kept paper: fetch PDF text (unchanged) → `find_code_url` on the text →
`generate_summary(...)` → store
`tldr: summary.as_ref().map(|s| s.tldr.clone())`, `summary`, `code_url`.
Batch-level semantics are untouched: a summary failure can never fail the
run; statuses, dedup, pruning, and the run guard are unchanged.

### API (`src/web/dto.rs`)

`DailyPaperDto` gains:

```rust
pub summary: Option<crate::daily::tldr::Summary>,  // reused directly — it already derives Serialize
pub code_url: Option<String>,
```

Both serialize as `null` when absent. `tldr` remains a top-level field.
No route, auth, or status-code changes.

### Widget (`deploy/k8s/README.md`)

The `custom-api` template keeps the title/score line and the always-visible
TL;DR (falling back to the abstract as today), and adds beneath it:

```html
{{ if .Exists "summary.problem" }}
<details>
  <summary class="size-h6 color-subdue">details</summary>
  <p><strong>Problem:</strong> {{ .String "summary.problem" }}</p>
  <p><strong>Approach:</strong> {{ .String "summary.approach" }}</p>
  <p><strong>Results:</strong> {{ .String "summary.results" }}</p>
  <p><strong>Limitations:</strong> {{ .String "summary.limitations" }}</p>
</details>
{{ end }}
{{ if .String "code_url" }} · <a href="{{ .String "code_url" }}">Code</a>{{ end }}
```

(Exact template syntax remains subject to the README's existing "verify
against your installed Glance version" caveat; the JSON contract above is
the stable part.)

## Error handling

- JSON parse failure → same path as an API error: warn, fall back
  (full-text → abstract-only → `None`). No new failure modes at batch level.
- `summary = NULL` rows (old batches, failed generation) serialize as
  `null`; the widget shows the abstract snippet exactly as today.
- Stored-JSON parse failure on read: warn + `None` (defensive only).

## Testing

- `tldr.rs`: happy-path JSON parse; fenced-JSON response; garbage/non-JSON
  response falls back to the abstract-only prompt; both prompts failing →
  `None`; prompt contains the five keys and the language.
- `job.rs`: `find_code_url` units (plain URL, trailing punctuation, no
  match); e2e test updated so the chat mock returns a JSON summary and the
  stored row has `summary` populated, `tldr` equal to `summary.tldr`, and
  `code_url` NULL (PDF mocks 404 as today).
- `store.rs`: roundtrip with `summary` + `code_url` set and unset; NULL
  summary reads back `None`.
- `web`: `GET /api/daily` JSON includes nested `summary` object and
  `code_url`, and `null`s for rows without them.

## Compatibility

- Migration 0009 is additive; existing databases upgrade in place.
- `generate_tldr` has no callers outside `job.rs`, so renaming it to
  `generate_summary` is a contained change.
- The API only adds fields; the previously documented widget template
  keeps rendering (it reads `tldr`/`abstract` which are unchanged).
