# Design: BibTeX / BibLaTeX export

**Project:** Xuewen — self-hosted reference manager for CS papers.
**Date:** 2026-07-09
**Status:** Approved (design phase)

## 1. Purpose

Users need to cite the papers they collect. There is currently no way to get a
`.bib` entry out of Xuewen — you have to hand-write it. This feature exports
stored papers as BibTeX or BibLaTeX, one at a time or in bulk, from both the CLI
and the web UI.

Because "entire library", "a project", and "the current filter/search" are all
just `db::list_papers(q, status, sort, project)` with different arguments (that
function already exists and is parameterized), batch export needs **no new query
layer** — only a pure formatter, thin endpoints, a CLI subcommand, and UI.

## 2. Decisions (settled during brainstorming)

| Decision | Choice |
|---|---|
| Formats | **BibTeX and BibLaTeX**, chosen by a `--format` flag / web toggle; **default BibTeX** |
| Batch scopes | **Entire library**, **a project**, **current filter/search** (all via `list_papers`). NOT hand-selected |
| Output — CLI | **stdout by default**, optional `-o <file>` |
| Output — web (individual) | **Copy to clipboard** + **Download** (`<cite_key>.bib`) |
| Output — web (batch) | **Download** (`xuewen.bib`) |
| Formatter | One pure Rust module (`src/export.rs`); the single source of truth. The web fetches text, it does not format client-side |

**Out of scope (YAGNI):** RIS / CSL-JSON, the `abstract` field, file-attachment
paths, hand-pick selection UI.

## 3. Core — `src/export.rs` (pure, dependency-free)

```rust
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum BibFormat { Bibtex, Biblatex }

/// One `.bib` entry for a paper (no trailing blank line).
pub fn format_entry(p: &Paper, fmt: BibFormat) -> String;

/// Many entries, blank-line separated, one trailing newline.
pub fn format_entries(papers: &[Paper], fmt: BibFormat) -> String;
```

Depends only on `crate::models::Paper` and `crate::naming` (for the key
fallback). No db, no web, no I/O — trivially unit-testable.

### 3.1 Entry key

Use the paper's `cite_key` verbatim (it is already a clean, disambiguated key).
When it is `None` (e.g. a `needs_review` paper), fall back to `{surname}{year}`
from the first author, cleaned with `naming::fold_ascii_alnum` so it is a valid
key; if even that is unavailable (no author/year), use the paper `id` as-is
(a UUID — hyphens are allowed in BibTeX keys).

### 3.2 Entry type

First match wins:

1. `dblp_key` starts with `conf/` → `@inproceedings`
2. `dblp_key` starts with `journals/` → `@article`
3. `venue` is `Some` → `@article`
4. `arxiv_id` is `Some` → `@misc` (BibTeX) / `@online` (BibLaTeX)
5. otherwise → `@misc`

### 3.3 Field mapping

`author` is the author list joined with ` and `. `venue` maps to `booktitle`
for `@inproceedings`, otherwise to the journal field. A field is omitted
entirely when its source value is `None`/empty.

| Logical value | BibTeX field | BibLaTeX field |
|---|---|---|
| authors | `author` | `author` |
| title | `title` | `title` |
| venue (article) | `journal` | `journaltitle` |
| venue (inproceedings) | `booktitle` | `booktitle` |
| year | `year` | `date` |
| doi | `doi` | `doi` |
| arXiv id | `eprint` + `archivePrefix = {arXiv}` | `eprint` + `eprinttype = {arxiv}` |
| url | `url` | `url` |

**`url` precedence:** stored `url` if present; else `https://arxiv.org/abs/{arxiv_id}`
when an arXiv id exists; else omit. (DOI is emitted in its own `doi` field, not
duplicated into `url`.)

### 3.4 Escaping & layout

- Values are wrapped in braces: `field = {value},`. Brace-wrapping the whole
  `title` also protects its capitalization.
- LaTeX specials inside a value are escaped: `&  %  $  #  _  {  }` → backslashed
  (`\&` etc.), and a literal backslash → `\textbackslash{}`.
- Two-space indented fields, closing `}` on its own line. Example:

```bibtex
@inproceedings{wang2019kgat,
  author = {Xiang Wang and Xiangnan He and Yixin Cao},
  title = {{KGAT}: Knowledge Graph Attention Network for Recommendation},
  booktitle = {KDD},
  year = {2019},
  doi = {10.1145/3292500.3330701},
}
```

```bibtex
@online{vaswani2017attention,
  author = {Ashish Vaswani and Noam Shazeer},
  title = {Attention Is All You Need},
  date = {2017},
  eprinttype = {arxiv},
  eprint = {1706.03762},
  url = {https://arxiv.org/abs/1706.03762},
}
```

### 3.5 Batch

`format_entries` maps `format_entry` over the slice, joined by blank lines with a
single trailing newline. Keys are unique because library `cite_key`s are already
disambiguated at ingest.

## 4. Web API (`src/web/api.rs`, `src/web/mod.rs`)

A shared helper parses the `format` query param (`biblatex` → BibLaTeX; anything
else, including absent, → BibTeX default).

- `GET /api/papers/{id}/export?format=bibtex|biblatex`
  → `200` with `Content-Type: text/plain; charset=utf-8`, body = one entry.
  `404` if the id is unknown. (Plain text so the browser shows it inline and the
  frontend can both copy it and force a download via an `<a download>`.)

- `GET /api/papers/export?format=…&q=…&status=…&project=…`
  → `200` with `Content-Type: application/x-bibtex` and
  `Content-Disposition: attachment; filename="xuewen.bib"`, body = the filtered
  `list_papers` result formatted as a batch. Reuses the same filter semantics as
  `GET /api/papers` (a new `ExportParams` deserialize struct mirroring
  `ListParams` plus `format`). Empty result → an empty body (still `200`).

Routes added in `router_with`:

```rust
.route("/api/papers/export", get(api::export_papers))          // must precede {id}
.route("/api/papers/{id}/export", get(api::export_paper))
```

(`/api/papers/export` is registered before the `{id}` param route so "export" is
not captured as an id.)

## 5. CLI (`src/main.rs`)

```
xuewen export <ID>                              # single paper → stdout
xuewen export --all    [--query Q] [--status S] # whole library (optionally filtered)
xuewen export --project <NAME> [--query Q] [--status S]
  shared flags: --format bibtex|biblatex   (default bibtex)
                -o, --output <FILE>        (default: stdout)
```

- `ID` conflicts with `--all` and `--project`; exactly one target is required.
- Format via `#[derive(clap::ValueEnum)] enum BibFormatArg { Bibtex, Biblatex }`.
- Targets resolve with existing helpers: `db::find_one` (single), `db::list_papers`
  (`--all`/filters), `db::find_one_project` + `list_papers(.. Some(pid))`
  (`--project`).
- Output: print to stdout, or `tokio::fs::write` when `-o` is given.

## 6. Frontend (`frontend/src/`)

- **`lib/types.ts`** — `export type BibFormat = 'bibtex' | 'biblatex'`.
- **`lib/api.ts`** —
  - `exportPaper(id, fmt): Promise<string>` → GET the single-entry text.
  - `exportUrl(filters, fmt): string` → build the batch URL from current filters
    (used as an `<a href download>` target so the browser downloads it).
- **`lib/state.svelte.ts`** — a `bibFormat` preference (`$state`, default
  `'bibtex'`); a `copyCitation(id)` helper that fetches and writes to
  `navigator.clipboard`.
- **`components/InfoPanel.svelte`** — a "Cite" block under the paper: a
  BibTeX/BibLaTeX toggle, a **Copy** button (clipboard), and a **Download**
  link/button (`<cite_key>.bib`, via an `<a download>` pointing at the single
  export endpoint).
- **`components/Sidebar.svelte`** — an **Export .bib** button in the header that
  is an `<a download="xuewen.bib">` whose `href` is `exportUrl(filters, bibFormat)`,
  so it exports exactly the current view (no filter = whole library; project
  filter = that project).

## 7. Testing

- **Rust `export` unit tests:** `@article` (dblp `journals/`), `@inproceedings`
  (dblp `conf/`), arXiv `@misc` (BibTeX) vs `@online` (BibLaTeX), venue→journal
  vs booktitle, `year` vs `date`, escaping of specials in a title, `cite_key`
  fallback when absent, and omission of missing fields. Batch: two entries,
  blank-line separated.
- **Rust web test:** `GET /api/papers/{id}/export` returns an entry with the
  right key; `format=biblatex` switches fields; `GET /api/papers/export?project=…`
  returns only that project's entries; unknown id → 404.
- **Frontend test:** the format toggle changes `exportUrl`; `copyCitation`
  fetches the endpoint and writes to a stubbed `navigator.clipboard`.

## 8. Risks / notes

- **Route ordering:** `/api/papers/export` must be declared before
  `/api/papers/{id}` / `/api/papers/{id}/export` so axum does not treat `export`
  as a paper id. Covered by a web test hitting the batch route.
- **Entry-type imperfection:** without a stored journal-vs-conference flag, a
  venue-only paper with no `dblp_key` defaults to `@article`. Acceptable; the
  `dblp_key` prefix (present for most CS papers) gets conference papers right.
- **Clipboard API:** `navigator.clipboard` requires a secure context
  (localhost counts). The Copy button degrades to the Download path if the write
  rejects.
