# Web UI Plan A — Backend (`serve` + JSON API + PDF streaming)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A new `xuewen serve` command running an axum server that exposes a read-only JSON API over the SQLite store, streams library PDFs (range-aware, path-safe), and serves an embedded SPA (placeholder page until Plan B builds the Svelte app).

**Architecture:** A new `src/web/` module. `build_router(pool, library_root) -> Router` assembles routes over an `AppState { pool, library_root }`; `serve(host, port, pool, library_root)` binds and runs it. JSON handlers read via `db`; the PDF handler resolves id→rel_path and delegates to `tower_http::ServeFile` (range support); a fallback handler serves embedded assets (`rust-embed`) with SPA fallback to `index.html`. `main.rs` gains a `Serve` subcommand.

**Tech Stack:** Rust, axum 0.8, tower (util), tower-http (fs), rust-embed, mime_guess, sqlx, tokio. Dev: axum-test.

**Environment:** `$IN_NIX_SHELL` is not set — run every cargo command through the flake dev shell with SEPARATE args: `nix develop -c cargo test` (NOT a single quoted string). Commit with `git -c commit.gpgsign=false commit -m "..."` (SSH signing unavailable). Conventional Commits, scope required, types feat/fix/docs/chore/ci. Run `cargo fmt` before each commit (the tree is rustfmt-clean; keep it so). Spec: `docs/superpowers/specs/2026-07-07-web-ui-design.md`.

---

## File Structure

- **Modify** `Cargo.toml` — add axum/tower/tower-http/rust-embed/mime_guess deps + axum-test dev-dep.
- **Create** `build.rs` — ensure `frontend/dist/index.html` exists (placeholder) so `rust-embed` compiles without a frontend build.
- **Modify** `.gitignore` — ignore `/frontend/dist/`.
- **Modify** `src/db.rs` — add `list_papers` (filtered/sorted) and `stats` queries.
- **Create** `src/web/mod.rs` — `AppState`, `build_router`, `serve`.
- **Create** `src/web/dto.rs` — `PaperSummary`, `PaperDetail`, `Stats` serialize types.
- **Create** `src/web/api.rs` — `list_papers`, `get_paper`, `stats`, `pdf` handlers.
- **Create** `src/web/assets.rs` — `rust-embed` `Assets` + `static_handler` (SPA fallback).
- **Modify** `src/lib.rs` — `pub mod web;`.
- **Modify** `src/main.rs` — `Serve { host, port }` subcommand + dispatch.
- **Create** `tests/web_test.rs` — axum-test API tests.

`frontend/` itself (the Svelte app) is Plan B; this plan only creates the placeholder `frontend/dist/index.html` via `build.rs`.

---

## Task 1: Dependencies, `build.rs` placeholder, and db queries

**Files:** `Cargo.toml`, `build.rs`, `.gitignore`, `src/db.rs`.

- [ ] **Step 1: Add dependencies**

In `Cargo.toml` `[dependencies]`, append:
```toml
axum = "0.8"
tower = { version = "0.5", features = ["util"] }
tower-http = { version = "0.6", features = ["fs"] }
rust-embed = "8"
mime_guess = "2"
```
In `[dev-dependencies]`, append:
```toml
axum-test = "17"
```

- [ ] **Step 2: Add `build.rs` (placeholder for the embed folder)**

Create `build.rs` at the repo root:
```rust
use std::fs;
use std::path::Path;

// rust-embed embeds `frontend/dist` at compile time and errors if the folder is
// absent. Ensure it exists with a placeholder page so `cargo build`/tests work
// without a frontend build; a real `npm run build` (Plan B) overwrites it.
fn main() {
    let dist = Path::new("frontend/dist");
    let index = dist.join("index.html");
    if !index.exists() {
        fs::create_dir_all(dist).expect("create frontend/dist");
        fs::write(&index, PLACEHOLDER).expect("write placeholder index.html");
    }
    println!("cargo:rerun-if-changed=frontend/dist");
}

const PLACEHOLDER: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\
<title>Xuewen</title></head><body style=\"font-family:system-ui;max-width:40rem;margin:4rem auto;padding:0 1rem\">\
<h1>Xuewen</h1><p>The API is running. The web UI has not been built yet — run \
<code>npm --prefix frontend run build</code> and rebuild.</p>\
<p>Try <a href=\"/api/stats\">/api/stats</a>.</p></body></html>";
```

- [ ] **Step 3: Ignore the build artifact**

In `.gitignore`, add a line:
```
/frontend/dist/
```

- [ ] **Step 4: Write failing tests for the new db queries**

In `src/db.rs`, inside the existing `#[cfg(test)] mod tests` block, append:
```rust
    #[tokio::test]
    async fn list_papers_filters_and_sorts() {
        let (_dir, pool) = temp_pool().await;
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000a1", "ha");
        a.title = Some("Deep Residual Learning".into());
        a.authors = Some(r#"["Kaiming He"]"#.into());
        a.year = Some(2016);
        a.status = PaperStatus::Resolved.as_str().to_string();
        let mut b = sample_paper("01890000-0000-7000-8000-0000000000b2", "hb");
        b.title = Some("Attention Is All You Need".into());
        b.authors = Some(r#"["Ashish Vaswani"]"#.into());
        b.year = Some(2017);
        b.status = PaperStatus::NeedsReview.as_str().to_string();
        insert_paper(&pool, &a).await.unwrap();
        insert_paper(&pool, &b).await.unwrap();

        // No filters → both, default sort year DESC (2017 before 2016).
        let all = list_papers(&pool, None, None, None).await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].year, Some(2017));

        // q matches title (case-insensitive) or authors.
        let hits = list_papers(&pool, Some("residual"), None, None).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].id, a.id);
        let by_author = list_papers(&pool, Some("vaswani"), None, None).await.unwrap();
        assert_eq!(by_author.len(), 1);
        assert_eq!(by_author[0].id, b.id);

        // status filter.
        let resolved = list_papers(&pool, None, Some("resolved"), None).await.unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].id, a.id);

        // year_asc sort.
        let asc = list_papers(&pool, None, None, Some("year_asc")).await.unwrap();
        assert_eq!(asc[0].year, Some(2016));

        // An unknown status is ignored (not an error) → both rows.
        let bogus = list_papers(&pool, None, Some("nonsense"), None).await.unwrap();
        assert_eq!(bogus.len(), 2);
    }

    #[tokio::test]
    async fn stats_counts_by_status() {
        let (_dir, pool) = temp_pool().await;
        assert_eq!(stats(&pool).await.unwrap(), (0, 0, 0));
        let mut a = sample_paper("01890000-0000-7000-8000-0000000000a1", "ha");
        a.status = PaperStatus::Resolved.as_str().to_string();
        let b = sample_paper("01890000-0000-7000-8000-0000000000b2", "hb"); // needs_review
        insert_paper(&pool, &a).await.unwrap();
        insert_paper(&pool, &b).await.unwrap();
        assert_eq!(stats(&pool).await.unwrap(), (2, 1, 1));
    }
```

- [ ] **Step 5: Run tests to verify they fail**

Run: `nix develop -c cargo test --lib db::tests`
Expected: FAIL to compile — `cannot find function list_papers` / `stats`.

- [ ] **Step 6: Implement `list_papers` and `stats`**

In `src/db.rs`, add `use sqlx::QueryBuilder;` to the imports at the top (next to the other `use sqlx::...` lines). After `find_by_id_prefix` (before the `#[cfg(test)]` block), add:
```rust
/// List papers with optional case-insensitive search (`q` over title+authors),
/// optional status filter, and a whitelisted sort. Unknown status/sort values
/// are ignored (never an error).
pub async fn list_papers(
    pool: &SqlitePool,
    q: Option<&str>,
    status: Option<&str>,
    sort: Option<&str>,
) -> Result<Vec<Paper>> {
    let mut qb: QueryBuilder<sqlx::Sqlite> = QueryBuilder::new("SELECT * FROM papers");
    let mut has_where = false;
    if let Some(term) = q.map(str::trim).filter(|s| !s.is_empty()) {
        let like = format!("%{term}%");
        qb.push(" WHERE (title LIKE ")
            .push_bind(like.clone())
            .push(" OR authors LIKE ")
            .push_bind(like)
            .push(")");
        has_where = true;
    }
    if let Some(st) = status.filter(|s| matches!(*s, "resolved" | "needs_review")) {
        qb.push(if has_where { " AND " } else { " WHERE " })
            .push("status = ")
            .push_bind(st.to_string());
    }
    // Whitelisted ORDER BY (never interpolate raw user input).
    let order = match sort {
        Some("year_asc") => "year ASC",
        Some("added_desc") => "added_at DESC",
        Some("title") => "title COLLATE NOCASE ASC",
        _ => "year DESC",
    };
    qb.push(" ORDER BY ").push(order);
    let papers = qb.build_query_as::<Paper>().fetch_all(pool).await?;
    Ok(papers)
}

/// `(total, resolved, needs_review)` paper counts.
pub async fn stats(pool: &SqlitePool) -> Result<(i64, i64, i64)> {
    let row: (i64, i64, i64) = sqlx::query_as(
        "SELECT COUNT(*), \
         COALESCE(SUM(status = 'resolved'), 0), \
         COALESCE(SUM(status = 'needs_review'), 0) \
         FROM papers",
    )
    .fetch_one(pool)
    .await?;
    Ok(row)
}
```

- [ ] **Step 7: Run tests + build + clippy**

Run: `nix develop -c cargo test --lib db::tests` then `nix develop -c cargo build` then `nix develop -c cargo clippy --all-targets -- -D warnings`
Expected: db tests pass; build succeeds (a `frontend/dist/index.html` placeholder is created by `build.rs`); clippy clean. (The new deps are unused so far — that's fine; unused *dependencies* don't warn.)

- [ ] **Step 8: Format and commit**

```bash
nix develop -c cargo fmt
git add Cargo.toml Cargo.lock build.rs .gitignore src/db.rs
git -c commit.gpgsign=false commit -m "feat(web): add axum deps, embed placeholder, and list/stats queries"
```

---

## Task 2: Web module — JSON API + embedded-asset fallback

**Files:** create `src/web/{mod.rs,dto.rs,api.rs,assets.rs}`; modify `src/lib.rs`; create `tests/web_test.rs`.

- [ ] **Step 1: Write the first failing API test**

Create `tests/web_test.rs`:
```rust
mod common;

use axum_test::TestServer;
use xuewen::db;
use xuewen::models::Paper;
use xuewen::web::build_router;

async fn temp_pool() -> (tempfile::TempDir, sqlx::SqlitePool) {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    (dir, pool)
}

fn paper(id: &str, title: &str, status: &str) -> Paper {
    Paper {
        id: id.into(),
        content_hash: id.into(),
        rel_path: format!("{id}.pdf"),
        title: Some(title.into()),
        abstract_text: Some("An abstract.".into()),
        authors: Some(r#"["Ada Lovelace"]"#.into()),
        venue: Some("KDD".into()),
        year: Some(2020),
        doi: None,
        arxiv_id: None,
        dblp_key: None,
        cite_key: Some(id.into()),
        url: None,
        source: Some("crossref".into()),
        status: status.into(),
        added_at: "2026-07-07T00:00:00Z".into(),
    }
}

#[tokio::test]
async fn lists_and_details_papers() {
    let (dir, pool) = temp_pool().await;
    db::insert_paper(&pool, &paper("aaaa1111", "Deep Residual Learning", "resolved"))
        .await
        .unwrap();
    db::insert_paper(&pool, &paper("bbbb2222", "Attention Is All You Need", "needs_review"))
        .await
        .unwrap();
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();

    // List: JSON array of summaries, authors as an array, no abstract field.
    let resp = server.get("/api/papers").await;
    resp.assert_status_ok();
    let list: Vec<serde_json::Value> = resp.json();
    assert_eq!(list.len(), 2);
    assert!(list[0]["authors"].is_array());
    assert!(list[0].get("abstract").is_none());

    // Search filter.
    let resp = server.get("/api/papers?q=attention").await;
    let hits: Vec<serde_json::Value> = resp.json();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["id"], "bbbb2222");

    // Detail includes abstract.
    let resp = server.get("/api/papers/aaaa1111").await;
    resp.assert_status_ok();
    let detail: serde_json::Value = resp.json();
    assert_eq!(detail["abstract"], "An abstract.");
    assert_eq!(detail["cite_key"], "aaaa1111");

    // Unknown id → 404.
    server
        .get("/api/papers/nope")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Stats.
    let resp = server.get("/api/stats").await;
    let s: serde_json::Value = resp.json();
    assert_eq!(s["total"], 2);
    assert_eq!(s["resolved"], 1);
    assert_eq!(s["needs_review"], 1);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `nix develop -c cargo test --test web_test`
Expected: FAIL to compile — `unresolved import xuewen::web` / `build_router`.

- [ ] **Step 3: Create the DTOs (`src/web/dto.rs`)**

```rust
use serde::Serialize;

use crate::models::Paper;

/// A paper for the list view (no abstract, to keep the payload light).
#[derive(Serialize)]
pub struct PaperSummary {
    pub id: String,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub cite_key: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
    pub status: String,
    pub added_at: String,
}

impl From<&Paper> for PaperSummary {
    fn from(p: &Paper) -> Self {
        Self {
            id: p.id.clone(),
            title: p.title.clone(),
            authors: p.authors_vec(),
            venue: p.venue.clone(),
            year: p.year,
            doi: p.doi.clone(),
            arxiv_id: p.arxiv_id.clone(),
            dblp_key: p.dblp_key.clone(),
            cite_key: p.cite_key.clone(),
            url: p.url.clone(),
            source: p.source.clone(),
            status: p.status.clone(),
            added_at: p.added_at.clone(),
        }
    }
}

/// A paper for the detail view: the summary fields plus the abstract.
#[derive(Serialize)]
pub struct PaperDetail {
    #[serde(flatten)]
    pub summary: PaperSummary,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
}

impl From<&Paper> for PaperDetail {
    fn from(p: &Paper) -> Self {
        Self {
            summary: PaperSummary::from(p),
            abstract_text: p.abstract_text.clone(),
        }
    }
}

/// Library counts for the header.
#[derive(Serialize)]
pub struct Stats {
    pub total: usize,
    pub resolved: usize,
    pub needs_review: usize,
}
```

- [ ] **Step 4: Create the asset handler (`src/web/assets.rs`)**

```rust
use axum::http::{header, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "frontend/dist"]
struct Assets;

/// Serve an embedded SPA asset by path, falling back to `index.html` for
/// client-side routes (so deep links work).
pub async fn static_handler(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match Assets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                [(header::CONTENT_TYPE, mime.as_ref())],
                content.data.into_owned(),
            )
                .into_response()
        }
        None => match Assets::get("index.html") {
            Some(index) => (
                [(header::CONTENT_TYPE, "text/html")],
                index.data.into_owned(),
            )
                .into_response(),
            None => (StatusCode::NOT_FOUND, "not found").into_response(),
        },
    }
}
```

- [ ] **Step 5: Create the JSON handlers (`src/web/api.rs`)**

```rust
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;

use super::dto::{PaperDetail, PaperSummary, Stats};
use super::AppState;
use crate::db;

#[derive(Deserialize)]
pub struct ListParams {
    pub q: Option<String>,
    pub status: Option<String>,
    pub sort: Option<String>,
}

pub async fn list_papers(State(app): State<AppState>, Query(p): Query<ListParams>) -> Response {
    match db::list_papers(&app.pool, p.q.as_deref(), p.status.as_deref(), p.sort.as_deref()).await {
        Ok(papers) => {
            let out: Vec<PaperSummary> = papers.iter().map(PaperSummary::from).collect();
            Json(out).into_response()
        }
        Err(e) => {
            tracing::error!("list_papers: {e}");
            internal_error()
        }
    }
}

pub async fn get_paper(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    match db::get_by_id(&app.pool, &id).await {
        Ok(Some(p)) => Json(PaperDetail::from(&p)).into_response(),
        Ok(None) => not_found(),
        Err(e) => {
            tracing::error!("get_paper: {e}");
            internal_error()
        }
    }
}

pub async fn stats(State(app): State<AppState>) -> Response {
    match db::stats(&app.pool).await {
        Ok((total, resolved, needs_review)) => Json(Stats {
            total: total as usize,
            resolved: resolved as usize,
            needs_review: needs_review as usize,
        })
        .into_response(),
        Err(e) => {
            tracing::error!("stats: {e}");
            internal_error()
        }
    }
}

pub(super) fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "not found"})),
    )
        .into_response()
}

pub(super) fn internal_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "internal error"})),
    )
        .into_response()
}
```

- [ ] **Step 6: Create the module root (`src/web/mod.rs`)**

```rust
mod api;
mod assets;
mod dto;

use std::path::PathBuf;

use anyhow::Result;
use axum::routing::get;
use axum::Router;
use sqlx::SqlitePool;

/// Shared state for the web handlers.
#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub library_root: PathBuf,
}

/// Assemble the read-only web router (pure — used directly by tests).
pub fn build_router(pool: SqlitePool, library_root: PathBuf) -> Router {
    let state = AppState { pool, library_root };
    Router::new()
        .route("/api/papers", get(api::list_papers))
        .route("/api/papers/{id}", get(api::get_paper))
        .route("/api/stats", get(api::stats))
        .fallback(assets::static_handler)
        .with_state(state)
}

/// Bind `host:port` and serve the router until the process is stopped.
pub async fn serve(host: &str, port: u16, pool: SqlitePool, library_root: PathBuf) -> Result<()> {
    let app = build_router(pool, library_root);
    let addr = format!("{host}:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("xuewen serving on http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}
```

- [ ] **Step 7: Declare the module**

In `src/lib.rs`, add `pub mod web;` alphabetically (after `pub mod watcher;` — `web` sorts after `watcher`).

- [ ] **Step 8: Run the API test + clippy**

Run: `nix develop -c cargo test --test web_test` then `nix develop -c cargo clippy --all-targets -- -D warnings`
Expected: the `lists_and_details_papers` test PASSES; clippy clean. (`serve` is unused so far but `pub`, so no warning.)

- [ ] **Step 9: Format and commit**

```bash
nix develop -c cargo fmt
git add src/web/ src/lib.rs tests/web_test.rs
git -c commit.gpgsign=false commit -m "feat(web): JSON API (papers/detail/stats) + embedded-asset fallback"
```

---

## Task 3: Range-aware, path-safe PDF streaming

**Files:** `src/web/api.rs`, `src/web/mod.rs`, `tests/web_test.rs`.

- [ ] **Step 1: Write failing PDF tests**

Append to `tests/web_test.rs` (the `common` module has `write_test_pdf`):
```rust
#[tokio::test]
async fn streams_pdf_with_range_and_guards_paths() {
    let (dir, pool) = temp_pool().await;
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();

    // A real paper whose PDF exists inside the library.
    let mut ok = paper("cccc3333", "A Paper", "resolved");
    ok.rel_path = "cccc3333.pdf".into();
    common::write_test_pdf(&library.join("cccc3333.pdf"), &["Hello PDF"]);
    db::insert_paper(&pool, &ok).await.unwrap();

    // A rogue record whose rel_path escapes the library.
    let mut escape = paper("dddd4444", "Escape", "resolved");
    escape.rel_path = "../outside.pdf".into();
    std::fs::write(dir.path().join("outside.pdf"), b"secret").unwrap();
    db::insert_paper(&pool, &escape).await.unwrap();

    let server = TestServer::new(build_router(pool, library.clone())).unwrap();

    // Full GET → 200, application/pdf.
    let resp = server.get("/papers/cccc3333/pdf").await;
    resp.assert_status_ok();
    assert_eq!(
        resp.header("content-type").to_str().unwrap(),
        "application/pdf"
    );
    let full_len = resp.as_bytes().len();
    assert!(full_len > 0);

    // Range request → 206 Partial Content, 100 bytes.
    let resp = server
        .get("/papers/cccc3333/pdf")
        .add_header(axum::http::header::RANGE, "bytes=0-99")
        .await;
    resp.assert_status(axum::http::StatusCode::PARTIAL_CONTENT);
    assert_eq!(resp.as_bytes().len(), 100);

    // Missing id → 404.
    server
        .get("/papers/zzzz9999/pdf")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Path-escaping record → 404 (guard rejects it, does NOT serve outside file).
    server
        .get("/papers/dddd4444/pdf")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}
```

- [ ] **Step 2: Run it to verify it fails**

Run: `nix develop -c cargo test --test web_test streams_pdf_with_range_and_guards_paths`
Expected: FAIL — route `/papers/{id}/pdf` doesn't exist yet (the fallback returns the placeholder HTML / 404, so assertions fail).

- [ ] **Step 3: Add the `pdf` handler (`src/web/api.rs`)**

Add these imports at the top of `src/web/api.rs`:
```rust
use axum::extract::Request;
use tower::ServiceExt;
use tower_http::services::ServeFile;
```
Add the handler (after `stats`):
```rust
/// Stream a paper's PDF from the library. Range-aware (via `ServeFile`) and
/// path-safe: the resolved file must live under `library_root`.
pub async fn pdf(State(app): State<AppState>, Path(id): Path<String>, req: Request) -> Response {
    let paper = match db::get_by_id(&app.pool, &id).await {
        Ok(Some(p)) => p,
        Ok(None) => return not_found(),
        Err(e) => {
            tracing::error!("pdf lookup: {e}");
            return internal_error();
        }
    };
    let path = app.library_root.join(&paper.rel_path);
    // Defense in depth: the canonical file must live under the library root.
    let under_root = match (std::fs::canonicalize(&path), std::fs::canonicalize(&app.library_root))
    {
        (Ok(file), Ok(root)) => file.starts_with(&root),
        _ => false, // missing file or unresolvable path
    };
    if !under_root {
        return not_found();
    }
    match ServeFile::new(&path).oneshot(req).await {
        Ok(resp) => resp.map(axum::body::Body::new).into_response(),
        Err(e) => {
            tracing::error!("serve pdf: {e}");
            internal_error()
        }
    }
}
```

- [ ] **Step 4: Register the route (`src/web/mod.rs`)**

In `build_router`, add the PDF route (before `.fallback(...)`):
```rust
        .route("/papers/{id}/pdf", get(api::pdf))
```

- [ ] **Step 5: Run the PDF tests + full suite + clippy**

Run: `nix develop -c cargo test --test web_test` then `nix develop -c cargo test` then `nix develop -c cargo clippy --all-targets -- -D warnings`
Expected: both web tests PASS; whole suite green; clippy clean.

- [ ] **Step 6: Format and commit**

```bash
nix develop -c cargo fmt
git add src/web/api.rs src/web/mod.rs tests/web_test.rs
git -c commit.gpgsign=false commit -m "feat(web): range-aware, path-safe PDF streaming"
```

---

## Task 4: `xuewen serve` CLI subcommand

**Files:** `src/main.rs`.

- [ ] **Step 1: Add the `Serve` subcommand**

In `src/main.rs`, add the import next to the other `use xuewen::...` lines:
```rust
use xuewen::web;
```
Add a variant to the `Command` enum (after `Refresh { ... }`):
```rust
    /// Serve the read-only web UI over HTTP (localhost).
    Serve {
        /// Address to bind.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind.
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
```
Add a match arm in `main` (after the `Command::Refresh` arm):
```rust
        Command::Serve { host, port } => {
            web::serve(&host, port, pool, cfg.library_root.clone()).await?;
        }
```
(`pool` is moved into `serve` here; this arm is terminal so that's fine. The `resolver`/`grobid`/`dirs` bindings built earlier are unused by `serve` but are used by other arms, so they don't warn.)

- [ ] **Step 2: Build + verify the CLI wiring**

Run: `nix develop -c cargo build`
Expected: compiles.

Run: `nix develop -c cargo run -- serve --help`
Expected: help text listing `--host` (default 127.0.0.1) and `--port` (default 8080).

- [ ] **Step 3: Manual smoke against a temp library**

`timeout` runs the server for 3s (auto-stopping it) while `curl` hits it:
```bash
SM=$(mktemp -d); mkdir -p "$SM/library"
printf 'inbox_dir="%s/inbox"\nlibrary_root="%s/library"\ndatabase_url="sqlite:%s/library.db"\n' "$SM" "$SM" "$SM" > "$SM/xuewen.toml"
nix develop -c bash -c "timeout 3 ./target/debug/xuewen --config '$SM/xuewen.toml' serve --port 8137 & sleep 1; echo '--- /api/stats ---'; curl -s http://127.0.0.1:8137/api/stats; echo; echo '--- / placeholder ---'; curl -s http://127.0.0.1:8137/ | head -c 100; echo"
```
Expected: `/api/stats` returns `{"total":0,"resolved":0,"needs_review":0}`; `/` returns the placeholder HTML. (Manual sanity check; there is no automated test for `main`'s `Serve` arm.)

- [ ] **Step 4: Full verification + commit**

Run: `nix develop -c cargo fmt -- --check` then `nix develop -c cargo clippy --all-targets -- -D warnings` then `nix develop -c cargo test`
Expected: fmt clean, clippy clean, whole suite green.
```bash
git add src/main.rs
git -c commit.gpgsign=false commit -m "feat(web): add serve subcommand"
```

---

## Verification (Definition of Done)

- `nix develop -c cargo test` — whole suite green, including the new `db` unit tests and `tests/web_test.rs` (list/filter/detail/stats + PDF full/range/404/path-guard).
- `nix develop -c cargo clippy --all-targets -- -D warnings` and `cargo fmt -- --check` — clean.
- `xuewen serve` starts, `/api/stats` returns counts, `/api/papers` returns a JSON array with `authors` as arrays, `/papers/:id/pdf` streams a PDF (with range), and `/` returns the placeholder page.
- `build.rs` created `frontend/dist/index.html`; `frontend/dist/` is git-ignored; no frontend build was required for any of the above.

## Notes for the executor

- axum 0.8 path params use `{id}` (not `:id`). If a dep version in the plan doesn't resolve, pick the nearest compatible release and note it — but keep axum at 0.8 (tower-http 0.6 / tower 0.5 / axum-test 17 are the matched set).
- The path-safety guard uses `std::fs::canonicalize`, which requires the file to exist; a missing file therefore also yields 404 (correct).
- Do NOT add auth, CORS, or any write/mutation endpoint (read-only, same-origin, localhost — per spec §1/§10).
- Do NOT build or scaffold the Svelte app — that's Plan B. This plan only ships the API behind the `build.rs` placeholder page.
- Every commit uses `git -c commit.gpgsign=false`.
