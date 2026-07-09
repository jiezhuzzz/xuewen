# BibTeX / BibLaTeX Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Export stored papers as BibTeX or BibLaTeX — individually or in batch (whole library / a project / current filter) — from both the CLI and the web UI.

**Architecture:** A pure `src/export.rs` module formats a `&Paper` into a `.bib` entry (batch = join). The web adds two thin GET endpoints and the CLI an `export` subcommand, both reusing the existing `db::list_papers` for all batch scopes. The frontend adds a "Cite" block (copy/download + format toggle) in the InfoPanel and an "Export .bib" download button in the Sidebar.

**Tech Stack:** Rust (axum 0.8, sqlx, clap ValueEnum, chrono, uuid), Svelte 5 + Tailwind, Vitest.

**Design:** `docs/superpowers/specs/2026-07-09-bib-export-design.md`

---

## File structure

**Backend**
- `src/export.rs` (create) — pure BibTeX/BibLaTeX formatter (`BibFormat`, `format_entry`, `format_entries`) + unit tests.
- `src/lib.rs` (modify) — add `pub mod export;`.
- `src/web/api.rs` (modify) — `FormatParam`/`ExportParams`, `parse_format`, `export_paper`, `export_papers` handlers.
- `src/web/mod.rs` (modify) — the two export routes.
- `src/main.rs` (modify) — `Export` CLI subcommand + `BibFormatArg`.
- `tests/web_test.rs` (modify) — export endpoints test.

**Frontend**
- `frontend/src/lib/types.ts` (modify) — `BibFormat`.
- `frontend/src/lib/api.ts` (modify) — `exportPaper`, `exportUrl`.
- `frontend/src/lib/state.svelte.ts` (modify) — `bibFormat` state, `copyCitation`.
- `frontend/src/components/InfoPanel.svelte` (modify) — "Cite" block.
- `frontend/src/components/Sidebar.svelte` (modify) — "Export .bib" button.
- `frontend/src/lib/export.test.ts` (create) — `exportUrl` + `copyCitation` test.

**Commit convention for every task:** use `git -c gc.auto=0 -c maintenance.auto=false commit ...`, only `git add` the files the task names (never `-A`), and append this trailer as a final `-m`:
`Claude-Session: https://claude.ai/code/session_01HCmkpYMd5f5U3yh7qXoEjf`
Do NOT commit the unrelated working-tree items `.envrc`, `inbox/`, `library/`, `xuewen.toml`.

---

## Task 1: `src/export.rs` — pure formatter

**Files:**
- Create: `src/export.rs`
- Modify: `src/lib.rs` (add `pub mod export;`)

- [ ] **Step 1: Register the module**

In `src/lib.rs`, add `pub mod export;` in alphabetical position (after `pub mod db;` / before `pub mod hash;` is fine; exact position doesn't matter):

```rust
pub mod export;
```

- [ ] **Step 2: Write the failing tests**

Create `src/export.rs` with only the tests first (the `use super::*;` items won't resolve yet — that's the failing state):

```rust
use crate::models::Paper;
use crate::naming;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BibFormat {
    Bibtex,
    Biblatex,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Authors, PaperMeta, PaperStatus};

    fn paper() -> Paper {
        Paper {
            id: "01890000-0000-7000-8000-000000000001".into(),
            content_hash: "h".into(),
            rel_path: "h.pdf".into(),
            cite_key: Some("wang2019kgat".into()),
            added_at: "2026-07-09T00:00:00Z".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some("KGAT: Knowledge Graph Attention Network".into()),
                abstract_text: None,
                authors: Authors(vec!["Xiang Wang".into(), "Xiangnan He".into()]),
                venue: Some("KDD".into()),
                year: Some(2019),
                doi: Some("10.1145/3292500.3330701".into()),
                arxiv_id: None,
                dblp_key: Some("conf/kdd/WangHCLC19".into()),
                url: None,
                source: Some("dblp".into()),
                status: PaperStatus::Resolved,
            },
        }
    }

    #[test]
    fn inproceedings_from_dblp_conf_prefix() {
        let out = format_entry(&paper(), BibFormat::Bibtex);
        assert!(out.starts_with("@inproceedings{wang2019kgat,\n"), "got: {out}");
        assert!(out.contains("author = {Xiang Wang and Xiangnan He},\n"));
        assert!(out.contains("title = {KGAT: Knowledge Graph Attention Network},\n"));
        assert!(out.contains("booktitle = {KDD},\n"));
        assert!(out.contains("year = {2019},\n"));
        assert!(out.contains("doi = {10.1145/3292500.3330701},\n"));
        assert!(out.ends_with("}"));
    }

    #[test]
    fn article_from_dblp_journals_prefix_and_biblatex_fields() {
        let mut p = paper();
        p.meta.dblp_key = Some("journals/tkde/Smith20".into());
        let out = format_entry(&p, BibFormat::Biblatex);
        assert!(out.starts_with("@article{wang2019kgat,\n"), "got: {out}");
        assert!(out.contains("journaltitle = {KDD},\n"));
        assert!(out.contains("date = {2019},\n"));
    }

    #[test]
    fn article_when_venue_but_no_dblp_key() {
        let mut p = paper();
        p.meta.dblp_key = None;
        let out = format_entry(&p, BibFormat::Bibtex);
        assert!(out.starts_with("@article{"), "got: {out}");
        assert!(out.contains("journal = {KDD},\n"));
    }

    #[test]
    fn arxiv_only_is_misc_bibtex_and_online_biblatex() {
        let mut p = paper();
        p.meta.dblp_key = None;
        p.meta.venue = None;
        p.meta.doi = None;
        p.meta.arxiv_id = Some("1706.03762".into());
        p.meta.url = None;

        let bt = format_entry(&p, BibFormat::Bibtex);
        assert!(bt.starts_with("@misc{"), "got: {bt}");
        assert!(bt.contains("archivePrefix = {arXiv},\n"));
        assert!(bt.contains("eprint = {1706.03762},\n"));
        assert!(bt.contains("url = {https://arxiv.org/abs/1706.03762},\n"));

        let bl = format_entry(&p, BibFormat::Biblatex);
        assert!(bl.starts_with("@online{"), "got: {bl}");
        assert!(bl.contains("eprinttype = {arxiv},\n"));
        assert!(bl.contains("eprint = {1706.03762},\n"));
    }

    #[test]
    fn escapes_latex_specials_and_omits_missing_fields() {
        let mut p = paper();
        p.meta.title = Some("Cost & Effect: 50% Faster #wins".into());
        p.meta.doi = None;
        let out = format_entry(&p, BibFormat::Bibtex);
        assert!(out.contains(r"title = {Cost \& Effect: 50\% Faster \#wins},"), "got: {out}");
        assert!(!out.contains("doi ="));
    }

    #[test]
    fn key_falls_back_to_surname_year_then_id() {
        let mut p = paper();
        p.cite_key = None;
        assert!(format_entry(&p, BibFormat::Bibtex).starts_with("@inproceedings{wang2019,"));

        p.meta.authors = Authors(vec![]);
        p.meta.year = None;
        assert!(format_entry(&p, BibFormat::Bibtex)
            .starts_with("@inproceedings{01890000-0000-7000-8000-000000000001,"));
    }

    #[test]
    fn batch_joins_entries_with_blank_line() {
        let out = format_entries(&[paper(), paper()], BibFormat::Bibtex);
        assert_eq!(out.matches("@inproceedings{").count(), 2);
        assert!(out.contains("}\n\n@inproceedings{"));
        assert!(out.ends_with("}\n"));
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib export::`
Expected: FAIL to compile — `format_entry` / `format_entries` not defined.

- [ ] **Step 4: Implement the formatter**

Add the implementation to `src/export.rs` (above the `#[cfg(test)] mod tests`):

```rust
/// One `.bib` entry for a paper (no trailing newline).
pub fn format_entry(p: &Paper, fmt: BibFormat) -> String {
    let kind = entry_type(p, fmt);
    let key = entry_key(p);
    let mut fields: Vec<(&'static str, String)> = Vec::new();

    if !p.meta.authors.0.is_empty() {
        fields.push(("author", p.meta.authors.0.join(" and ")));
    }
    if let Some(title) = p.meta.title.as_deref() {
        fields.push(("title", title.to_string()));
    }
    if let Some(venue) = p.meta.venue.as_deref() {
        fields.push((venue_field(kind, fmt), venue.to_string()));
    }
    if let Some(year) = p.meta.year {
        let field = if fmt == BibFormat::Biblatex { "date" } else { "year" };
        fields.push((field, year.to_string()));
    }
    if let Some(axv) = p.meta.arxiv_id.as_deref() {
        match fmt {
            BibFormat::Bibtex => fields.push(("archivePrefix", "arXiv".to_string())),
            BibFormat::Biblatex => fields.push(("eprinttype", "arxiv".to_string())),
        }
        fields.push(("eprint", axv.to_string()));
    }
    if let Some(doi) = p.meta.doi.as_deref() {
        fields.push(("doi", doi.to_string()));
    }
    if let Some(url) = entry_url(p) {
        fields.push(("url", url));
    }

    let mut out = format!("@{kind}{{{key},\n");
    for (name, value) in &fields {
        out.push_str(&format!("  {name} = {{{}}},\n", escape(value)));
    }
    out.push('}');
    out
}

/// Many entries, blank-line separated, with a single trailing newline.
pub fn format_entries(papers: &[Paper], fmt: BibFormat) -> String {
    let mut out = papers
        .iter()
        .map(|p| format_entry(p, fmt))
        .collect::<Vec<_>>()
        .join("\n\n");
    out.push('\n');
    out
}

fn entry_type(p: &Paper, fmt: BibFormat) -> &'static str {
    if let Some(key) = p.meta.dblp_key.as_deref() {
        if key.starts_with("conf/") {
            return "inproceedings";
        }
        if key.starts_with("journals/") {
            return "article";
        }
    }
    if p.meta.venue.is_some() {
        return "article";
    }
    if p.meta.arxiv_id.is_some() {
        return if fmt == BibFormat::Biblatex { "online" } else { "misc" };
    }
    "misc"
}

fn venue_field(kind: &str, fmt: BibFormat) -> &'static str {
    if kind == "inproceedings" {
        "booktitle"
    } else if fmt == BibFormat::Biblatex {
        "journaltitle"
    } else {
        "journal"
    }
}

fn entry_key(p: &Paper) -> String {
    if let Some(k) = p.cite_key.as_deref() {
        if !k.is_empty() {
            return k.to_string();
        }
    }
    if let (Some(first), Some(year)) = (p.meta.authors.0.first(), p.meta.year) {
        if let Some(s) = naming::surname(first) {
            let base = naming::fold_ascii_alnum(&format!("{s}{year}"));
            if !base.is_empty() {
                return base;
            }
        }
    }
    p.id.clone()
}

fn entry_url(p: &Paper) -> Option<String> {
    if let Some(u) = p.meta.url.as_deref() {
        if !u.is_empty() {
            return Some(u.to_string());
        }
    }
    p.meta
        .arxiv_id
        .as_deref()
        .map(|a| format!("https://arxiv.org/abs/{a}"))
}

/// Escape LaTeX-special characters in a field value.
fn escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\textbackslash{}"),
            '&' | '%' | '$' | '#' | '_' | '{' | '}' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib export::`
Expected: PASS (7 tests).

- [ ] **Step 6: Commit**

```bash
git add src/export.rs src/lib.rs
git -c gc.auto=0 -c maintenance.auto=false commit \
  -m "feat(export): pure BibTeX/BibLaTeX entry formatter" \
  -m "Claude-Session: https://claude.ai/code/session_01HCmkpYMd5f5U3yh7qXoEjf"
```

---

## Task 2: Web export endpoints

**Files:**
- Modify: `src/web/api.rs`
- Modify: `src/web/mod.rs`
- Modify: `tests/web_test.rs`

- [ ] **Step 1: Write the failing web test**

Add to `tests/web_test.rs` (uses the existing `paper`, `temp_pool`, `build_router`, and `db` helpers):

```rust
#[tokio::test]
async fn exports_bibtex_and_biblatex() {
    let (dir, pool) = temp_pool().await;
    db::insert_paper(&pool, &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved))
        .await
        .unwrap();
    db::insert_paper(&pool, &paper("bbbb2222", "Attention Is All You Need", PaperStatus::Resolved))
        .await
        .unwrap();
    let proj = db::create_project(&pool, "Survey", None).await.unwrap();
    db::add_paper_to_project(&pool, "aaaa1111", &proj.id).await.unwrap();
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();

    // Individual (default bibtex). The `paper` helper sets venue=KDD, no dblp_key -> @article.
    let resp = server.get("/api/papers/aaaa1111/export").await;
    resp.assert_status_ok();
    let text = resp.text();
    assert!(text.contains("@article{aaaa1111,"), "got: {text}");
    assert!(text.contains("journal = {KDD},"));

    // BibLaTeX switches the field names.
    let bl = server.get("/api/papers/aaaa1111/export?format=biblatex").await.text();
    assert!(bl.contains("journaltitle = {KDD},"), "got: {bl}");
    assert!(bl.contains("date = {2020},"));

    // Unknown id -> 404.
    server
        .get("/api/papers/nope/export")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Batch: whole library has both entries.
    let all = server.get("/api/papers/export").await;
    all.assert_status_ok();
    let all_text = all.text();
    assert!(all_text.contains("@article{aaaa1111,"));
    assert!(all_text.contains("@article{bbbb2222,"));

    // Batch filtered by project -> only that project's paper.
    let scoped = server.get(&format!("/api/papers/export?project={}", proj.id)).await.text();
    assert!(scoped.contains("aaaa1111"));
    assert!(!scoped.contains("bbbb2222"));
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test --test web_test exports_bibtex_and_biblatex`
Expected: FAIL — routes/handlers not defined (404s / compile error).

- [ ] **Step 3: Implement the handlers**

In `src/web/api.rs`, add `use crate::export;` near the other `use crate::...` imports, then append:

```rust
#[derive(Deserialize)]
pub struct FormatParam {
    pub format: Option<String>,
}

#[derive(Deserialize)]
pub struct ExportParams {
    pub format: Option<String>,
    pub q: Option<String>,
    pub status: Option<String>,
    pub sort: Option<String>,
    pub project: Option<String>,
}

fn parse_format(s: Option<&str>) -> export::BibFormat {
    match s {
        Some(v) if v.eq_ignore_ascii_case("biblatex") => export::BibFormat::Biblatex,
        _ => export::BibFormat::Bibtex,
    }
}

/// One paper's `.bib` entry as plain text (inline, so the frontend can copy it
/// or force a download via `<a download>`).
pub async fn export_paper(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Query(p): Query<FormatParam>,
) -> Response {
    match db::get_by_id(&app.pool, &id).await {
        Ok(Some(paper)) => {
            let body = export::format_entry(&paper, parse_format(p.format.as_deref()));
            ([(axum::http::header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => {
            tracing::error!("export_paper: {e}");
            internal_error()
        }
    }
}

/// The current filtered set as a downloadable `.bib` file. Same filter semantics
/// as `GET /api/papers`.
pub async fn export_papers(State(app): State<AppState>, Query(p): Query<ExportParams>) -> Response {
    match db::list_papers(
        &app.pool,
        p.q.as_deref(),
        p.status.as_deref(),
        p.sort.as_deref(),
        p.project.as_deref(),
    )
    .await
    {
        Ok(papers) => {
            let body = export::format_entries(&papers, parse_format(p.format.as_deref()));
            (
                [
                    (axum::http::header::CONTENT_TYPE, "application/x-bibtex"),
                    (
                        axum::http::header::CONTENT_DISPOSITION,
                        "attachment; filename=\"xuewen.bib\"",
                    ),
                ],
                body,
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("export_papers: {e}");
            internal_error()
        }
    }
}
```

- [ ] **Step 4: Wire the routes**

In `src/web/mod.rs` `router_with`, add these two routes (place the static `export` route before the `{id}` param routes for clarity; axum/matchit prefers static over param regardless):

```rust
        .route("/api/papers/export", get(api::export_papers))
        .route("/api/papers/{id}/export", get(api::export_paper))
```

- [ ] **Step 5: Run the test + full backend suite**

Run: `cargo test --test web_test exports_bibtex_and_biblatex` then `cargo test`
Expected: PASS (new test + all existing).

- [ ] **Step 6: Commit**

```bash
git add src/web/api.rs src/web/mod.rs tests/web_test.rs
git -c gc.auto=0 -c maintenance.auto=false commit \
  -m "feat(web): BibTeX/BibLaTeX export endpoints (single + filtered batch)" \
  -m "Claude-Session: https://claude.ai/code/session_01HCmkpYMd5f5U3yh7qXoEjf"
```

---

## Task 3: CLI `xuewen export`

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add the `BibFormatArg` enum and `Export` command variant**

In `src/main.rs`, add near the top-level enums (after the `Command` enum definition) a clap `ValueEnum`:

```rust
#[derive(Clone, Copy, clap::ValueEnum)]
enum BibFormatArg {
    Bibtex,
    Biblatex,
}

impl From<BibFormatArg> for xuewen::export::BibFormat {
    fn from(a: BibFormatArg) -> Self {
        match a {
            BibFormatArg::Bibtex => xuewen::export::BibFormat::Bibtex,
            BibFormatArg::Biblatex => xuewen::export::BibFormat::Biblatex,
        }
    }
}
```

And add a variant to `enum Command` (after `Project { .. }`):

```rust
    /// Export papers as BibTeX or BibLaTeX.
    Export {
        /// Paper id (exact or unique prefix) for a single entry.
        #[arg(conflicts_with_all = ["all", "project"])]
        id: Option<String>,
        /// Export the whole (non-trashed) library.
        #[arg(long, conflicts_with = "project")]
        all: bool,
        /// Export all papers in this project (name or id).
        #[arg(long)]
        project: Option<String>,
        /// Filter batch exports by a search term (title/author).
        #[arg(long)]
        query: Option<String>,
        /// Filter batch exports by status (resolved|needs_review).
        #[arg(long)]
        status: Option<String>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = BibFormatArg::Bibtex)]
        format: BibFormatArg,
        /// Write to this file instead of stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
```

- [ ] **Step 2: Dispatch the command**

In `src/main.rs`'s `match cli.command`, add an arm (after the `Command::Project { .. }` arm):

```rust
        Command::Export {
            id,
            all,
            project,
            query,
            status,
            format,
            output,
        } => {
            let fmt = xuewen::export::BibFormat::from(format);
            let text = if let Some(id) = id {
                let paper = db::find_one(&pool, &id).await?;
                xuewen::export::format_entry(&paper, fmt)
            } else {
                if !all && project.is_none() {
                    anyhow::bail!("specify a paper id, --all, or --project <name>");
                }
                let project_id = match &project {
                    Some(sel) => Some(db::find_one_project(&pool, sel).await?.id),
                    None => None,
                };
                let papers = db::list_papers(
                    &pool,
                    query.as_deref(),
                    status.as_deref(),
                    None,
                    project_id.as_deref(),
                )
                .await?;
                xuewen::export::format_entries(&papers, fmt)
            };
            match output {
                Some(path) => {
                    tokio::fs::write(&path, &text).await?;
                    println!("wrote {}", path.display());
                }
                None => print!("{text}"),
            }
        }
```

- [ ] **Step 3: Build + smoke-test**

Run: `cargo build`
Expected: PASS.

Manual smoke (optional, against the project's configured db): `cargo run -- export --all` prints entries (or nothing on an empty library); `cargo run -- export --help` shows the flags.

- [ ] **Step 4: Run the full backend suite**

Run: `cargo test`
Expected: PASS (nothing regressed).

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git -c gc.auto=0 -c maintenance.auto=false commit \
  -m "feat(cli): xuewen export (single/--all/--project, --format, -o)" \
  -m "Claude-Session: https://claude.ai/code/session_01HCmkpYMd5f5U3yh7qXoEjf"
```

---

## Task 4: Frontend types, API client, and state

**Files:**
- Modify: `frontend/src/lib/types.ts`
- Modify: `frontend/src/lib/api.ts`
- Modify: `frontend/src/lib/state.svelte.ts`

- [ ] **Step 1: Add the `BibFormat` type**

In `frontend/src/lib/types.ts`, add:

```ts
export type BibFormat = 'bibtex' | 'biblatex';
```

- [ ] **Step 2: Add the API client functions**

In `frontend/src/lib/api.ts`, add `BibFormat` and `Filters` to the type import if not already present, then add:

```ts
export async function exportPaper(id: string, fmt: BibFormat): Promise<string> {
  const res = await fetch(`/api/papers/${encodeURIComponent(id)}/export?format=${fmt}`);
  if (!res.ok) throw new Error(`export failed: ${res.status}`);
  return res.text();
}

export function exportUrl(f: Filters, fmt: BibFormat): string {
  const params = new URLSearchParams();
  if (f.q.trim()) params.set('q', f.q.trim());
  if (f.status !== 'all') params.set('status', f.status);
  if (f.project && f.project !== 'all') params.set('project', f.project);
  params.set('format', fmt);
  return `/api/papers/export?${params.toString()}`;
}
```

- [ ] **Step 3: Add state (format preference + copy helper)**

In `frontend/src/lib/state.svelte.ts`, add `exportPaper` to the imports from `./api` and `BibFormat` to the imports from `./types`, then add:

```ts
export const bibFormat = $state<{ value: BibFormat }>({ value: 'bibtex' });

/// Fetch a paper's citation in the current format and copy it to the clipboard.
export async function copyCitation(id: string): Promise<void> {
  const text = await exportPaper(id, bibFormat.value);
  await navigator.clipboard.writeText(text);
}
```

- [ ] **Step 4: Verify typecheck**

Run: `cd frontend && npm run check`
Expected: PASS (0 errors).

- [ ] **Step 5: Commit**

```bash
git add frontend/src/lib/types.ts frontend/src/lib/api.ts frontend/src/lib/state.svelte.ts
git -c gc.auto=0 -c maintenance.auto=false commit \
  -m "feat(web): export types, api client, and copy-citation state" \
  -m "Claude-Session: https://claude.ai/code/session_01HCmkpYMd5f5U3yh7qXoEjf"
```

---

## Task 5: Frontend UI — InfoPanel "Cite" block + Sidebar export button

**Files:**
- Modify: `frontend/src/components/InfoPanel.svelte`
- Modify: `frontend/src/components/Sidebar.svelte`

- [ ] **Step 1: Add the "Cite" block to the InfoPanel**

In `frontend/src/components/InfoPanel.svelte`, extend the script imports and add copy state/handler:

```svelte
  import { Check, Copy, Download, ExternalLink, Trash2, Wand2, X } from 'lucide-svelte';
  import {
    addToProject,
    bibFormat,
    copyCitation,
    detailRefresh,
    loadDetail,
    openIdentify,
    projects,
    removeFromProject,
    removePaper,
  } from '../lib/state.svelte';
  import StatusPill from './StatusPill.svelte';
```

Add (alongside the other `let ... = $state(...)` declarations near the top of the script):

```svelte
  let copied = $state(false);
  async function doCopy() {
    try {
      await copyCitation(id);
      copied = true;
      setTimeout(() => (copied = false), 1500);
    } catch {
      /* clipboard blocked (insecure context) — the Download link still works */
    }
  }
```

Then, in the `{:then d}` block, insert a "Cite" section just before the actions block (the `<div class="mt-6 border-t ...">` that holds Identify/Delete). It uses `bibFormat.value` for the format and `d.cite_key ?? id` for the download filename:

```svelte
      <div class="mt-4">
        <h3 class="mb-1 text-xs font-semibold uppercase tracking-wide text-slate-500 dark:text-slate-400">Cite</h3>
        <div class="flex items-center gap-2">
          <select
            bind:value={bibFormat.value}
            aria-label="Citation format"
            class="rounded-lg border border-slate-200 bg-slate-50 px-2 py-1 text-xs dark:border-slate-700 dark:bg-slate-800"
          >
            <option value="bibtex">BibTeX</option>
            <option value="biblatex">BibLaTeX</option>
          </select>
          <button
            type="button"
            onclick={doCopy}
            class="inline-flex items-center gap-1.5 rounded-lg border border-slate-200 px-2 py-1 text-xs font-medium text-indigo-600 hover:bg-indigo-50 dark:border-slate-700 dark:text-indigo-400 dark:hover:bg-indigo-500/10"
          >
            {#if copied}<Check size={12} /> Copied{:else}<Copy size={12} /> Copy{/if}
          </button>
          <a
            href={`/api/papers/${encodeURIComponent(id)}/export?format=${bibFormat.value}`}
            download={`${d.cite_key ?? id}.bib`}
            class="inline-flex items-center gap-1.5 rounded-lg border border-slate-200 px-2 py-1 text-xs font-medium text-indigo-600 hover:bg-indigo-50 dark:border-slate-700 dark:text-indigo-400 dark:hover:bg-indigo-500/10"
          >
            <Download size={12} /> Download
          </a>
        </div>
      </div>
```

- [ ] **Step 2: Add the Sidebar "Export .bib" button**

In `frontend/src/components/Sidebar.svelte`, extend the script imports:

```svelte
  import { Download, FolderOpen, Search, Settings2 } from 'lucide-svelte';
  import {
    bibFormat,
    filters,
    library,
    loadPapers,
    openProjects,
    projects,
    setProjectFilter,
    setSearch,
  } from '../lib/state.svelte';
  import { exportUrl } from '../lib/api';
  import type { Sort, StatusFilter } from '../lib/types';
  import PaperRow from './PaperRow.svelte';
```

Then add an export link at the end of the bordered header block (after the project-filter row, still inside the `<div class="space-y-3 border-b ...">`):

```svelte
    <a
      href={exportUrl(filters, bibFormat.value)}
      download="xuewen.bib"
      class="inline-flex w-full items-center justify-center gap-1.5 rounded-lg border border-slate-200 px-2 py-1.5 text-xs font-medium text-slate-600 hover:bg-slate-100 dark:border-slate-700 dark:text-slate-300 dark:hover:bg-slate-800"
    >
      <Download size={14} /> Export .bib
    </a>
```

- [ ] **Step 3: Verify typecheck + build**

Run: `cd frontend && npm run check && npm run build`
Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/InfoPanel.svelte frontend/src/components/Sidebar.svelte
git -c gc.auto=0 -c maintenance.auto=false commit \
  -m "feat(web): cite block in InfoPanel and Export .bib button in Sidebar" \
  -m "Claude-Session: https://claude.ai/code/session_01HCmkpYMd5f5U3yh7qXoEjf"
```

---

## Task 6: Frontend test

**Files:**
- Create: `frontend/src/lib/export.test.ts`

- [ ] **Step 1: Write the test**

Create `frontend/src/lib/export.test.ts`:

```ts
import { beforeEach, describe, expect, it, vi } from 'vitest';
import { exportUrl } from './api';
import { bibFormat, copyCitation } from './state.svelte';
import type { Filters } from './types';

const baseFilters: Filters = { q: '', status: 'all', sort: 'year_desc', project: 'all' };

describe('exportUrl', () => {
  it('builds a url with only the format when no filters are set', () => {
    const url = exportUrl(baseFilters, 'bibtex');
    expect(url).toBe('/api/papers/export?format=bibtex');
  });

  it('includes active search, status, and project filters', () => {
    const url = exportUrl(
      { q: 'graph', status: 'resolved', sort: 'year_desc', project: 'p1' },
      'biblatex',
    );
    expect(url).toContain('q=graph');
    expect(url).toContain('status=resolved');
    expect(url).toContain('project=p1');
    expect(url).toContain('format=biblatex');
  });
});

describe('copyCitation', () => {
  beforeEach(() => {
    bibFormat.value = 'bibtex';
    vi.unstubAllGlobals();
  });

  it('fetches the entry in the current format and writes it to the clipboard', async () => {
    const writeText = vi.fn(async () => {});
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    let requested = '';
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL) => {
        requested = String(url);
        return new Response('@article{x,\n}', { status: 200 });
      }),
    );

    bibFormat.value = 'biblatex';
    await copyCitation('aaaa1111');

    expect(requested).toBe('/api/papers/aaaa1111/export?format=biblatex');
    expect(writeText).toHaveBeenCalledWith('@article{x,\n}');
  });
});
```

- [ ] **Step 2: Run the test + full frontend suite**

Run: `cd frontend && npx vitest run src/lib/export.test.ts` then `npm test`
Expected: PASS (new tests + all existing).

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/export.test.ts
git -c gc.auto=0 -c maintenance.auto=false commit \
  -m "test(web): exportUrl and copyCitation" \
  -m "Claude-Session: https://claude.ai/code/session_01HCmkpYMd5f5U3yh7qXoEjf"
```

---

## Final verification

- [ ] **Backend:** `cargo test` → all pass; `cargo build` → no warnings.
- [ ] **Frontend:** `cd frontend && npm run check && npm run build && npm test` → all pass.
- [ ] **Manual smoke (optional, needs a running server):** `cargo run -- serve`, open a paper → the InfoPanel "Cite" block: toggle BibTeX/BibLaTeX, click Copy (paste to verify), click Download (`<cite_key>.bib`). In the Sidebar, set a project filter, click **Export .bib** → downloads `xuewen.bib` containing that project's entries.
- [ ] **CLI smoke (optional):** `cargo run -- export <paper-id-prefix>`; `cargo run -- export --all --format biblatex`; `cargo run -- export --project "<name>" -o out.bib`.
