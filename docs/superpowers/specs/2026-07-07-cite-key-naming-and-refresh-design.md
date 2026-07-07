# Design: Cite-Key Filenames + `refresh` Command

**Project:** Xuewen — self-hosted reference manager for CS papers.
**Date:** 2026-07-07
**Status:** Approved (design phase)

## 1. Purpose

Two cohesive additions to the existing ingest pipeline:

1. **Cite-key filenames** — file each new PDF under a Google-Scholar-style cite key
   (`{surname}{year}{titleword}.pdf`) in a flat `library/` directory, instead of
   the current content-hash name (`<hash>.pdf`).
2. **`xuewen refresh` command** — re-resolve metadata for records that failed the
   first time and re-file every paper to its correct cite-key path (upgrading
   older `<hash>.pdf` files and `needs_review` records).

`content_hash` remains the dedup identity; only a paper's *location* (`rel_path`)
changes. This is exactly what the schema was built for (mutable `rel_path`).

## 2. Decisions (settled during brainstorming)

| Decision | Choice |
|---|---|
| Directory layout | **Flat** `library/<citekey>.pdf` (no venue/year folders) |
| Un-keyable papers | `library/_unsorted/<content_hash>.pdf` |
| Filename content | **Pure cite key** (venue/year stay DB columns, not in the name) |
| Cite-key form | `{surname}{year}{titleword}`, lowercased + diacritics folded + non-alphanumerics stripped |
| Title word | first word **after skipping leading stop words** |
| When (naming) | at **ingest** (new files) **and** via `refresh` (existing files) |
| Collisions | letter suffix (`…a`, `…b`), detected via the **`cite_key` DB column** (exclude self) |
| Persist key | new **`cite_key TEXT`** column (migration `0002`) |
| `refresh` default | re-resolve `needs_review`, re-file all |
| `refresh` flags | `--all` (re-resolve everything); `<ID>` (one paper, any status); mutually exclusive |

## 3. Cite-key algorithm

`cite_key_base(authors: &[String], year: Option<i64>, title: Option<&str>) -> Option<String>`

- **surname** = last whitespace-separated token of `authors[0]`
  (`"Kaiming He"` → `He`; `"Laurens van der Maaten"` → `Maaten`).
- **year** = the resolved `year`.
- **titleword** = the first title token **after skipping leading stop words**.
  - Stop words (lowercased): `a, an, the, on, of, in, for, to, and, or, with, at,
    by, from, as, is, are, be, this, that`.
  - `"A Neural Probabilistic Language Model"` → `neural`;
    `"Attention Is All You Need"` → `attention` (not a stop word);
    `"On Large-Batch Training"` → `large` (first alnum run of `large-batch`).
  - If every token is a stop word (degenerate), fall back to the first token.
- **Folding** (`fold_ascii_alnum`): NFKD-normalize (via `unicode-normalization`),
  lowercase, keep only ASCII `[a-z0-9]` (drops diacritics, spaces, punctuation).
  `"Müller"` → `muller`.
- Returns `Some("{surname}{year}{titleword}")` only if **surname**, **year**, and
  **titleword** are all present and non-empty after folding; otherwise `None`
  (→ the paper is un-keyable → `_unsorted/`).

Examples: He/2016/"Deep Residual Learning…" → `he2016deep`;
Vaswani/2017/"Attention Is All You Need" → `vaswani2017attention`;
Devlin/2019/"BERT: Pre-training…" → `devlin2019bert`.

## 4. Path & collision resolution

- `library_rel_path(cite_key: Option<&str>, content_hash: &str) -> String`
  - `Some(key)` → `"{key}.pdf"`; `None` → `"_unsorted/{content_hash}.pdf"`.
- **Collision** (`disambiguate(base, taken: &HashSet<String>) -> String`, pure):
  return `base` if not in `taken`, else `base+"a"`, `base+"b"`, … first free.
  `taken` is the set of cite keys already used by **other** papers sharing the
  base prefix, fetched from the DB (`cite_key LIKE base||'%'`, excluding the
  paper's own id on `refresh`). Single-writer (CLI/serial watcher) → no race.
- The `naming` module (surname/title/fold/base/rel_path/disambiguate) is **pure
  and fully unit-testable**; only the `taken`-set fetch touches the DB.

## 5. Schema change

`migrations/0002_add_cite_key.sql`:
```sql
ALTER TABLE papers ADD COLUMN cite_key TEXT;
```
- Existing rows get `NULL` (they keep their `<hash>.pdf` paths until a `refresh`).
- Add `cite_key: Option<String>` to the `Paper` struct; add it to `insert_paper`
  and a new `update_paper`. `SELECT *` / `FromRow` picks it up by name.
- Not `UNIQUE` — the suffix logic already yields unique keys, and a non-unique
  column avoids surprising insert failures on any edge case.

## 6. Ingest integration

The extract→identify→GROBID→resolve→field-selection logic currently lives inline
in `pipeline::ingest_file` + `build_paper`. Refactor for reuse:

- **`resolve_fields(provisional_title, extracted, ident, resolution) -> ResolvedFields`**
  — the metadata a paper should store: `title, abstract, authors: Vec<String>,
  venue, year, doi, arxiv_id, dblp_key, url, source, status`. (This is the
  field-selection half of today's `build_paper`, minus id/hash/rel_path/added_at.)

`ingest_file` new flow (steps 1–3 unchanged: hash, dedup, extract/identify/GROBID/resolve):
1. `fields = resolve_fields(...)`.
2. `base = naming::cite_key_base(&fields.authors, fields.year, fields.title.as_deref())`.
3. If `Some(base)`: `taken = db::cite_keys_with_base(pool, &base, None)`;
   `key = naming::disambiguate(&base, &taken)`; `rel_path = "{key}.pdf"`,
   `cite_key = Some(key)`. Else `rel_path = "_unsorted/{hash}.pdf"`, `cite_key = None`.
4. `create_dir_all(dest.parent())`; copy the PDF to `library_root/rel_path`.
5. Assemble the `Paper` (`id` = new UUIDv7, `content_hash`, `rel_path`, `cite_key`,
   `…fields`, `added_at` = now); `insert_paper`; on error remove the copied file.
6. Move the original out of the inbox (unchanged).

Authors: `resolve_fields` keeps `authors: Vec<String>` so the key uses `authors[0]`
directly; the `Paper` still stores the JSON string via `authors_json()`.

## 7. `refresh` command

`xuewen refresh [ID] [--all]` — one pass over the library.

**Target set:**
- no args → papers with `status = 'needs_review'` are re-resolved; **all** papers
  are re-filed.
- `--all` → **all** papers are re-resolved and re-filed.
- `<ID>` → the single paper whose `id` equals `ID` (exact or unique prefix) is
  re-resolved (regardless of status) and re-filed; ambiguous/absent prefix → error.
- `--all` + `<ID>` → clap `conflicts_with` error.

**Per paper:**
1. `pdf = library_root.join(&paper.rel_path)`; if it doesn't exist → warn + skip.
2. **Re-resolve** (if this paper is in the re-resolve set): run the reusable
   chain on the stored PDF — `resolve_pdf(&pdf, resolver, grobid)` returns
   `(ident, provisional_title, extracted, resolution)` (the same steps
   `ingest_file` runs, factored out); then `fields = resolve_fields(...)`; update
   the paper's metadata columns + `status` from `fields`. (Uses GROBID if now
   configured; retries Crossref/DBLP/arXiv now that the network/service is up.)
   Papers **not** in the re-resolve set keep their existing metadata.
3. **Re-file:** recompute `base`/`cite_key`/`rel_path` from the paper's *current*
   metadata (collision `taken` set **excludes this paper's id**). If the new
   `rel_path` differs from the stored one, `move_to` the file to the new path
   (create parent dirs) and update `rel_path` + `cite_key`.
4. `db::update_paper(pool, &paper)`.

**Supporting DB functions:** `update_paper`, `all_papers`, `papers_by_status`,
`find_by_id_prefix`, `cite_keys_with_base` (all thin `sqlx` queries).

Re-resolution reuses `resolve_pdf` (factored out of `ingest_file`); `refresh` does
**not** re-hash, dedup, or move-from-inbox — it operates on the already-stored
library copy in place.

## 8. Error handling

- Missing library PDF for a record → warn, skip (don't fail the whole run).
- **Refresh never downgrades a record.** A re-resolution that comes back
  `Unresolved` (network/rate-limit/no confident match) does **not** overwrite an
  already-`resolved` record: its existing metadata is kept and it stays
  `resolved`. A `needs_review` record that fails to re-resolve simply stays
  `needs_review`. Only a confident `Resolved` result (or re-resolving a
  not-yet-`resolved` record) updates the stored metadata. A local failure to even
  read the PDF is likewise non-destructive (warn, keep existing metadata).
  Rationale: unlike ingest — where a fresh PDF degrading to `needs_review` loses
  nothing — a failed re-resolve of an *already-resolved* paper would otherwise
  wipe good metadata (a real risk under `--all`/`refresh <id>` during a resolver
  outage or rate-limit). Resolution never aborts the run.
- File move failure during re-file → log, leave the record's `rel_path` unchanged
  (the DB stays consistent with disk).
- `refresh` processes papers independently; one failure never aborts the pass.

## 9. Testing

- **`naming` unit tests (pure):** cite keys for representative inputs; stop-word
  skipping; diacritic folding; missing author/year/title → `None`; `disambiguate`
  suffixing against a `taken` set; `library_rel_path` for keyed vs un-keyed.
- **Pipeline integration (updated + new):** resolved-via-Crossref →
  `library/<citekey>.pdf` with `cite_key` set; `needs_review` →
  `library/_unsorted/<hash>.pdf` with `cite_key = NULL`; a genuine collision →
  the second file suffixed. The two existing tests asserting `library/<hash>.pdf`
  are updated to the new paths.
- **`refresh` integration:** (a) a `needs_review` record whose re-resolution now
  succeeds (mock DBLP/Crossref that failed the first time) moves from
  `_unsorted/<hash>.pdf` to `<citekey>.pdf` and flips to `resolved`; (b) an old
  `<hash>.pdf` resolved record is re-filed to `<citekey>.pdf` without re-resolving;
  (c) `refresh <id>` targets exactly one; (d) `--all` re-resolves a resolved paper.
- All external calls mocked (wiremock) + fixtures; offline & deterministic.

## 10. Decomposition (implementation plans)

Two plans (the second builds on the first):

- **Plan A — Cite-key naming at ingest:** `naming` module, migration `0002` +
  `Paper.cite_key`, `resolve_fields` refactor, `db::cite_keys_with_base`, pipeline
  filing at the cite-key/`_unsorted` path, updated pipeline tests.
- **Plan B — `refresh` command:** `resolve_pdf` refactor, `db::{update_paper,
  all_papers, papers_by_status, find_by_id_prefix}`, the `refresh` flow + flags,
  `refresh` integration tests.

## 11. Out of scope

- Relocating into `<venue>/<year>/` folders (flat was chosen deliberately).
- Venue-abbreviation mapping.
- Normalized author entities / author-level queries.
- A `list`/`show`/`export` read surface (separate slice).
