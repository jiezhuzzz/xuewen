# Exact-Identifier Resolvers Implementation Plan (Slice 1, Plan 2a)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a PDF's extracted identifier is an arXiv ID or a DOI, fetch authoritative metadata (title, abstract, authors, venue, year, url) from arXiv/Crossref and store a `resolved` record; otherwise (no identifier, or lookup fails) keep the existing `needs_review` behavior from Plan 1.

**Architecture:** A new `resolve` module with two source clients (`arxiv`, `crossref`), each split into a pure `parse()` function (fixture-tested offline) and a thin async `fetch()` HTTP wrapper. A `Resolver` struct owns a `reqwest` client and routes by `Identifier`. Network/parse failures degrade gracefully to "unresolved" (→ `needs_review`) — a lookup never aborts ingest. The pipeline calls the resolver between identify and store.

**Tech Stack:** Adds `reqwest` (rustls), `serde_json`, `roxmltree` (arXiv Atom XML). Dev: `wiremock` (offline HTTP mock). Reuses tokio, sqlx, uuid, regex, chrono, anyhow from Plan 1.

---

## Plan set context

Slice 1 spec: `docs/superpowers/specs/2026-07-06-pdf-ingest-metadata-pipeline-design.md`.
- Plan 1 (done, merged): offline ingest foundation — a PDF becomes a stored `needs_review` record.
- **Plan 2a (this file):** exact-identifier resolution (arXiv + Crossref). Resolves papers that carry a DOI or arXiv ID.
- Plan 2b (next): title-search path — GROBID extraction + DBLP search + Crossref fallback + fuzzy confidence gate, for the `Identifier::None` case.
- Plan 3: `notify` watcher daemon + debounce + catch-up + retry/backoff.

### Current state (from Plan 1, all on `main`)
- `xuewen::pipeline::ingest_file(pool: &SqlitePool, dirs: &Libraries, path: &Path) -> Result<Outcome>` — hashes, dedups, extracts text, identifies (`Identifier::{Doi,Arxiv,None}`), guesses a provisional title, copies the PDF to `library_root/<hash>.pdf`, inserts a `Paper` (`status="needs_review"`), moves the original to `_processed/`. On insert failure it removes the copied file (no orphan).
- `xuewen::models`: `Identifier`, `PaperStatus::{Resolved,NeedsReview}` (`.as_str()`), `Paper { id, content_hash, rel_path, title, abstract_text (col "abstract"), authors (JSON string), venue, year: Option<i64>, doi, arxiv_id, dblp_key, url, source, status, added_at }`.
- `xuewen::identify::{identify, guess_title}`, `xuewen::db::{connect, exists_by_hash, insert_paper, get_by_id}`, `xuewen::config::Config { inbox_dir, library_root, database_url, grobid_url, contact_email }`.
- Runtime needs `pdftotext`; the Nix dev shell provides it. Run cargo via `nix develop -c '<command>'`.

## File structure

```
Cargo.toml                     # + reqwest, serde_json, roxmltree; dev: wiremock
tests/fixtures/
  arxiv_attention.xml          # recorded arXiv Atom response
  crossref_kgat.json           # recorded Crossref /works response
src/
  resolve/
    mod.rs                     # ResolvedMetadata, Resolution, Resolver, helpers
    arxiv.rs                   # arXiv Atom fetch + parse
    crossref.rs                # Crossref JSON fetch + parse
  lib.rs                       # + pub mod resolve;
  pipeline.rs                  # ingest_file gains &Resolver; sets resolved fields
  main.rs                      # builds a Resolver, passes it to ingest_file
tests/
  resolve_test.rs              # wiremock: Resolver routes Doi/Arxiv end-to-end
  pipeline_test.rs             # updated calls + new resolved-path test
```

**Module responsibilities:**
- `resolve::arxiv` / `resolve::crossref`: each turns a source response into `Option<ResolvedMetadata>` (`parse`, pure) and fetches it over HTTP (`fetch`, thin). No routing, no DB.
- `resolve::mod`: shared types + text helpers + the `Resolver` (client, base URLs, routing, graceful degradation).
- `pipeline`: unchanged responsibilities; gains one resolve step and a `build_paper` helper.

---

## Task 1: Dependencies + fixtures directory

**Files:** Modify `Cargo.toml`; create `tests/fixtures/` (with the two fixture files below).

- [ ] **Step 1: Add dependencies to `Cargo.toml`**

Under `[dependencies]` add:

```toml
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls"] }
serde_json = "1"
roxmltree = "0.20"
```

Under `[dev-dependencies]` add:

```toml
wiremock = "0.6"
```

(rustls avoids a system OpenSSL dependency. We fetch response bodies as text and parse them ourselves, so the reqwest `json` feature is intentionally omitted.)

- [ ] **Step 2: Create `tests/fixtures/arxiv_attention.xml`**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:arxiv="http://arxiv.org/schemas/atom">
  <entry>
    <id>http://arxiv.org/abs/1706.03762v5</id>
    <published>2017-06-12T17:57:34Z</published>
    <title>Attention Is All You Need</title>
    <summary>  The dominant sequence transduction models are based on complex
recurrent or convolutional neural networks that include an encoder and a
decoder.  </summary>
    <author><name>Ashish Vaswani</name></author>
    <author><name>Noam Shazeer</name></author>
    <arxiv:doi>10.5555/3295222.3295349</arxiv:doi>
  </entry>
</feed>
```

- [ ] **Step 3: Create `tests/fixtures/crossref_kgat.json`**

```json
{
  "status": "ok",
  "message": {
    "DOI": "10.1145/3292500.3330701",
    "title": ["KGAT: Knowledge Graph Attention Network for Recommendation"],
    "author": [
      {"given": "Xiang", "family": "Wang"},
      {"given": "Xiangnan", "family": "He"}
    ],
    "container-title": ["Proceedings of the 25th ACM SIGKDD International Conference on Knowledge Discovery & Data Mining"],
    "issued": {"date-parts": [[2019, 7, 25]]},
    "abstract": "<jats:p>Knowledge graphs are used to improve recommendation.</jats:p>",
    "URL": "http://dx.doi.org/10.1145/3292500.3330701"
  }
}
```

- [ ] **Step 4: Build to resolve the new crates**

Run: `nix develop -c cargo build`
Expected: reqwest/serde_json/roxmltree/wiremock resolve and compile; `Finished`. If a listed minor version fails to resolve, bump only that constraint minimally (keep the same crate + features) and note it.

**Known reqwest footgun:** with `default-features = false`, some builds need the `charset` feature for `Response::text()` to compile, and `http2` is occasionally required at runtime. If a later task fails to compile on `.text()` (or a runtime "unsupported protocol" error appears), add features so the line reads `features = ["rustls-tls", "charset", "http2"]`. Do NOT re-enable `default-tls`/`native-tls` (that would pull in a system OpenSSL dependency, which rustls exists to avoid). Report any feature you had to add.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock tests/fixtures/
git commit -m "chore: add http + parsing deps and API fixtures for resolvers"
```

---

## Task 2: Resolver shared types + text helpers

**Files:** Create `src/resolve/mod.rs`; modify `src/lib.rs`.
**Test:** unit tests inside `src/resolve/mod.rs`.

- [ ] **Step 1: Create `src/resolve/mod.rs`**

```rust
use regex::Regex;
use std::sync::LazyLock;

/// Normalized bibliographic metadata produced by a source resolver.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ResolvedMetadata {
    pub title: Option<String>,
    pub abstract_text: Option<String>,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub url: Option<String>,
    /// Which source produced this record: "arxiv" | "crossref".
    pub source: String,
}

impl ResolvedMetadata {
    /// The authors as a JSON array string for the `papers.authors` column,
    /// or `None` when there are no authors.
    pub fn authors_json(&self) -> Option<String> {
        if self.authors.is_empty() {
            None
        } else {
            serde_json::to_string(&self.authors).ok()
        }
    }
}

/// Outcome of a resolution attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    Resolved(ResolvedMetadata),
    Unresolved,
}

/// Collapse all runs of whitespace to single spaces and trim.
pub(crate) fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());

/// Strip XML/HTML tags (e.g. Crossref JATS `<jats:p>`) and collapse whitespace.
pub(crate) fn strip_tags(s: &str) -> String {
    collapse_ws(&TAG_RE.replace_all(s, " "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collapse_ws_normalizes() {
        assert_eq!(collapse_ws("  a\n  b\t c "), "a b c");
    }

    #[test]
    fn strip_tags_removes_jats() {
        assert_eq!(
            strip_tags("<jats:p>Hello  <b>world</b></jats:p>"),
            "Hello world"
        );
    }

    #[test]
    fn authors_json_roundtrip() {
        let md = ResolvedMetadata {
            authors: vec!["Ada Lovelace".into(), "Alan Turing".into()],
            ..Default::default()
        };
        assert_eq!(
            md.authors_json().as_deref(),
            Some(r#"["Ada Lovelace","Alan Turing"]"#)
        );

        let empty = ResolvedMetadata::default();
        assert_eq!(empty.authors_json(), None);
    }
}
```

- [ ] **Step 2: Declare the module in `src/lib.rs`**

Add (keep the list alphabetical-ish; placement doesn't matter functionally):

```rust
pub mod resolve;
```

- [ ] **Step 3: Run the tests**

Run: `nix develop -c cargo test resolve::tests`
Expected: `collapse_ws_normalizes`, `strip_tags_removes_jats`, `authors_json_roundtrip` PASS.

- [ ] **Step 4: Commit**

```bash
git add src/resolve/mod.rs src/lib.rs
git commit -m "feat: resolver shared types (ResolvedMetadata, Resolution) + text helpers"
```

---

## Task 3: arXiv client (parse + fetch)

**Files:** Create `src/resolve/arxiv.rs`; modify `src/resolve/mod.rs` (add `pub mod arxiv;`).
**Test:** unit test inside `src/resolve/arxiv.rs` using the fixture.

**IMPORTANT — roxmltree + namespaces:** The Atom feed uses a default namespace (`xmlns="http://www.w3.org/2005/Atom"`), so every element is namespaced. `Node::has_tag_name("entry")` with a bare `&str` matches only *no-namespace* elements and will therefore NOT match. Match by **local name** instead: `node.tag_name().name() == "entry"`. This also matches `arxiv:doi` via local name `"doi"`.

- [ ] **Step 1: Create `src/resolve/arxiv.rs`**

```rust
use anyhow::{anyhow, Result};

use super::{collapse_ws, ResolvedMetadata};

/// Fetch the Atom response for a single arXiv id from `{base}/api/query`.
pub async fn fetch(client: &reqwest::Client, base: &str, id: &str) -> Result<String> {
    let url = format!("{base}/api/query?id_list={id}");
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow!("arxiv HTTP {}", resp.status()));
    }
    Ok(resp.text().await?)
}

/// Parse an arXiv Atom feed into metadata. Returns `Ok(None)` if there is no entry.
pub fn parse(atom: &str) -> Result<Option<ResolvedMetadata>> {
    let doc = roxmltree::Document::parse(atom)?;

    let entry = match doc.descendants().find(|n| n.tag_name().name() == "entry") {
        Some(e) => e,
        None => return Ok(None),
    };

    let child_text = |tag: &str| {
        entry
            .children()
            .find(|c| c.tag_name().name() == tag)
            .and_then(|n| n.text())
            .map(collapse_ws)
    };

    let title = child_text("title");
    let abstract_text = child_text("summary");
    let url = child_text("id");
    let year = child_text("published")
        .and_then(|s| s.get(0..4).and_then(|y| y.parse::<i64>().ok()));

    let authors: Vec<String> = entry
        .children()
        .filter(|c| c.tag_name().name() == "author")
        .filter_map(|a| {
            a.children()
                .find(|n| n.tag_name().name() == "name")
                .and_then(|n| n.text())
                .map(collapse_ws)
        })
        .collect();

    let doi = entry
        .children()
        .find(|c| c.tag_name().name() == "doi")
        .and_then(|n| n.text())
        .map(|s| s.trim().to_string());

    Ok(Some(ResolvedMetadata {
        title,
        abstract_text,
        authors,
        venue: None,
        year,
        doi,
        arxiv_id: None, // stamped by the Resolver, which knows the queried id
        dblp_key: None,
        url,
        source: "arxiv".to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/arxiv_attention.xml"));

    #[test]
    fn parses_arxiv_entry() {
        let md = parse(FIXTURE).unwrap().unwrap();
        assert_eq!(md.title.as_deref(), Some("Attention Is All You Need"));
        assert_eq!(md.year, Some(2017));
        assert_eq!(md.authors, vec!["Ashish Vaswani", "Noam Shazeer"]);
        assert_eq!(md.doi.as_deref(), Some("10.5555/3295222.3295349"));
        assert_eq!(md.url.as_deref(), Some("http://arxiv.org/abs/1706.03762v5"));
        assert!(md.abstract_text.unwrap().starts_with("The dominant sequence"));
        assert_eq!(md.source, "arxiv");
    }

    #[test]
    fn empty_feed_is_none() {
        let feed = r#"<feed xmlns="http://www.w3.org/2005/Atom"></feed>"#;
        assert!(parse(feed).unwrap().is_none());
    }
}
```

- [ ] **Step 2: Declare the submodule** — in `src/resolve/mod.rs` add near the top:

```rust
pub mod arxiv;
```

- [ ] **Step 3: Run the tests**

Run: `nix develop -c cargo test resolve::arxiv::tests`
Expected: `parses_arxiv_entry` and `empty_feed_is_none` PASS. If `parses_arxiv_entry` fails to find elements, confirm you matched by `tag_name().name()` (local name), not bare `has_tag_name`.

- [ ] **Step 4: Commit**

```bash
git add src/resolve/arxiv.rs src/resolve/mod.rs
git commit -m "feat: arXiv Atom client (fetch + parse)"
```

---

## Task 4: Crossref client (parse + fetch)

**Files:** Create `src/resolve/crossref.rs`; modify `src/resolve/mod.rs` (add `pub mod crossref;`).
**Test:** unit test inside `src/resolve/crossref.rs` using the fixture.

- [ ] **Step 1: Create `src/resolve/crossref.rs`**

```rust
use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{collapse_ws, strip_tags, ResolvedMetadata};

/// Fetch the Crossref work record for a DOI from `{base}/works/{doi}`.
pub async fn fetch(client: &reqwest::Client, base: &str, doi: &str) -> Result<String> {
    let url = format!("{base}/works/{doi}");
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow!("crossref HTTP {}", resp.status()));
    }
    Ok(resp.text().await?)
}

/// Parse a Crossref `/works/{doi}` JSON body. Returns `Ok(None)` if there is no message.
pub fn parse(json: &str) -> Result<Option<ResolvedMetadata>> {
    let v: Value = serde_json::from_str(json)?;
    let m = &v["message"];
    if m.is_null() {
        return Ok(None);
    }

    let title = m["title"].get(0).and_then(Value::as_str).map(collapse_ws);
    let venue = m["container-title"].get(0).and_then(Value::as_str).map(collapse_ws);
    let doi = m["DOI"].as_str().map(str::to_string);
    let url = m["URL"].as_str().map(str::to_string);
    let abstract_text = m["abstract"].as_str().map(strip_tags);
    let year = m["issued"]["date-parts"]
        .get(0)
        .and_then(|dp| dp.get(0))
        .and_then(Value::as_i64);

    let authors: Vec<String> = m["author"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    let given = a["given"].as_str().unwrap_or("");
                    let family = a["family"].as_str().unwrap_or("");
                    let name = format!("{given} {family}").trim().to_string();
                    (!name.is_empty()).then_some(name)
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Some(ResolvedMetadata {
        title,
        abstract_text,
        authors,
        venue,
        year,
        doi,
        arxiv_id: None,
        dblp_key: None,
        url,
        source: "crossref".to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/crossref_kgat.json"));

    #[test]
    fn parses_crossref_work() {
        let md = parse(FIXTURE).unwrap().unwrap();
        assert_eq!(
            md.title.as_deref(),
            Some("KGAT: Knowledge Graph Attention Network for Recommendation")
        );
        assert_eq!(md.year, Some(2019));
        assert_eq!(md.authors, vec!["Xiang Wang", "Xiangnan He"]);
        assert_eq!(md.doi.as_deref(), Some("10.1145/3292500.3330701"));
        assert_eq!(
            md.abstract_text.as_deref(),
            Some("Knowledge graphs are used to improve recommendation.")
        );
        assert!(md.venue.unwrap().starts_with("Proceedings of the 25th ACM SIGKDD"));
        assert_eq!(md.source, "crossref");
    }

    #[test]
    fn missing_message_is_none() {
        assert!(parse(r#"{"status":"ok"}"#).unwrap().is_none());
    }
}
```

- [ ] **Step 2: Declare the submodule** — in `src/resolve/mod.rs` add:

```rust
pub mod crossref;
```

- [ ] **Step 3: Run the tests**

Run: `nix develop -c cargo test resolve::crossref::tests`
Expected: `parses_crossref_work` and `missing_message_is_none` PASS.

- [ ] **Step 4: Commit**

```bash
git add src/resolve/crossref.rs src/resolve/mod.rs
git commit -m "feat: Crossref JSON client (fetch + parse)"
```

---

## Task 5: Resolver (routing + graceful degradation)

**Files:** Modify `src/resolve/mod.rs` (add the `Resolver`); create `tests/resolve_test.rs`.
**Test:** `tests/resolve_test.rs` using `wiremock`.

- [ ] **Step 1: Add the `Resolver` to `src/resolve/mod.rs`**

Add these imports at the top of `src/resolve/mod.rs` (alongside the existing ones):

```rust
use crate::models::Identifier;
use anyhow::Result;
use std::time::Duration;
```

Then append this block (after the helpers):

```rust
/// Fetches authoritative metadata for an identifier. A network or parse failure
/// degrades to `Resolution::Unresolved` — resolution never aborts ingestion.
pub struct Resolver {
    http: reqwest::Client,
    arxiv_base: String,
    crossref_base: String,
}

impl Resolver {
    /// Build a resolver pointing at the real arXiv and Crossref endpoints.
    pub fn new(contact_email: Option<&str>) -> Result<Self> {
        Self::with_bases(
            contact_email,
            "http://export.arxiv.org".to_string(),
            "https://api.crossref.org".to_string(),
        )
    }

    /// Build a resolver with explicit base URLs (used by tests to point at a mock server).
    pub fn with_bases(
        contact_email: Option<&str>,
        arxiv_base: String,
        crossref_base: String,
    ) -> Result<Self> {
        let ua = match contact_email {
            Some(email) => format!("xuewen/0.1 (mailto:{email})"),
            None => "xuewen/0.1".to_string(),
        };
        let http = reqwest::Client::builder()
            .user_agent(ua)
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self { http, arxiv_base, crossref_base })
    }

    /// Route an identifier to its source and return the outcome.
    pub async fn resolve(&self, ident: &Identifier) -> Resolution {
        let md = match ident {
            Identifier::Arxiv(id) => self.try_arxiv(id).await,
            Identifier::Doi(doi) => self.try_crossref(doi).await,
            Identifier::None => None,
        };
        match md {
            Some(m) => Resolution::Resolved(m),
            None => Resolution::Unresolved,
        }
    }

    async fn try_arxiv(&self, id: &str) -> Option<ResolvedMetadata> {
        match self.fetch_parse_arxiv(id).await {
            Ok(Some(mut m)) => {
                m.arxiv_id = Some(id.to_string());
                Some(m)
            }
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("arxiv resolve failed for {id}: {e}");
                None
            }
        }
    }

    async fn fetch_parse_arxiv(&self, id: &str) -> Result<Option<ResolvedMetadata>> {
        let body = arxiv::fetch(&self.http, &self.arxiv_base, id).await?;
        arxiv::parse(&body)
    }

    async fn try_crossref(&self, doi: &str) -> Option<ResolvedMetadata> {
        match self.fetch_parse_crossref(doi).await {
            Ok(Some(mut m)) => {
                if m.doi.is_none() {
                    m.doi = Some(doi.to_string());
                }
                Some(m)
            }
            Ok(None) => None,
            Err(e) => {
                tracing::warn!("crossref resolve failed for {doi}: {e}");
                None
            }
        }
    }

    async fn fetch_parse_crossref(&self, doi: &str) -> Result<Option<ResolvedMetadata>> {
        let body = crossref::fetch(&self.http, &self.crossref_base, doi).await?;
        crossref::parse(&body)
    }
}
```

- [ ] **Step 2: Create `tests/resolve_test.rs`**

```rust
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

use xuewen::models::Identifier;
use xuewen::resolve::{Resolution, Resolver};

const ARXIV_FIXTURE: &str = include_str!("fixtures/arxiv_attention.xml");
const CROSSREF_FIXTURE: &str = include_str!("fixtures/crossref_kgat.json");

#[tokio::test]
async fn resolves_doi_via_crossref() {
    let server = MockServer::start().await;
    let doi = "10.1145/3292500.3330701";
    Mock::given(method("GET"))
        .and(path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver.resolve(&Identifier::Doi(doi.to_string())).await;

    match res {
        Resolution::Resolved(md) => {
            assert_eq!(md.source, "crossref");
            assert_eq!(
                md.title.as_deref(),
                Some("KGAT: Knowledge Graph Attention Network for Recommendation")
            );
            assert_eq!(md.doi.as_deref(), Some(doi));
            assert_eq!(md.year, Some(2019));
        }
        Resolution::Unresolved => panic!("expected Resolved"),
    }
}

#[tokio::test]
async fn resolves_arxiv_via_api() {
    let server = MockServer::start().await;
    let id = "1706.03762";
    Mock::given(method("GET"))
        .and(path("/api/query"))
        .and(query_param("id_list", id))
        .respond_with(ResponseTemplate::new(200).set_body_string(ARXIV_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver.resolve(&Identifier::Arxiv(id.to_string())).await;

    match res {
        Resolution::Resolved(md) => {
            assert_eq!(md.source, "arxiv");
            assert_eq!(md.title.as_deref(), Some("Attention Is All You Need"));
            assert_eq!(md.arxiv_id.as_deref(), Some(id)); // stamped by the resolver
        }
        Resolution::Unresolved => panic!("expected Resolved"),
    }
}

#[tokio::test]
async fn http_error_degrades_to_unresolved() {
    // A server with no stubs returns 404 for everything.
    let server = MockServer::start().await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Doi("10.9999/nope".to_string()))
        .await;
    assert_eq!(res, Resolution::Unresolved);
}

#[tokio::test]
async fn none_identifier_is_unresolved() {
    let resolver = Resolver::new(None).unwrap();
    assert_eq!(resolver.resolve(&Identifier::None).await, Resolution::Unresolved);
}
```

- [ ] **Step 3: Run the tests**

Run: `nix develop -c cargo test --test resolve_test`
Expected: all four PASS. `http_error_degrades_to_unresolved` proves a failed lookup degrades rather than errors. If `wiremock`'s API differs (e.g. `set_body_string` renamed), adjust only the mock setup to return the fixture body with a 200, keeping the assertions unchanged; check the installed API under `~/.cargo/registry/src/*/wiremock-*/src/` if needed and report changes.

- [ ] **Step 4: Commit**

```bash
git add src/resolve/mod.rs tests/resolve_test.rs
git commit -m "feat: Resolver routing with graceful degradation to unresolved"
```

---

## Task 6: Pipeline + CLI integration

**Files:** Modify `src/pipeline.rs`, `src/main.rs`, `tests/pipeline_test.rs`.
**Test:** updated + new tests in `tests/pipeline_test.rs`.

- [ ] **Step 1: Update `src/pipeline.rs` to resolve and populate metadata**

Change the imports at the top of `src/pipeline.rs` to add the resolver types:

```rust
use crate::models::{Identifier, Paper, PaperStatus};
use crate::resolve::{Resolution, Resolver};
use crate::{db, hash, identify, pdf};
```

Change the `ingest_file` signature to take a `&Resolver`:

```rust
pub async fn ingest_file(
    pool: &SqlitePool,
    dirs: &Libraries,
    resolver: &Resolver,
    path: &Path,
) -> Result<Outcome> {
```

Inside `ingest_file`, after computing `let title = identify::guess_title(&text);` and BEFORE step 4 (the library copy), add the resolve call:

```rust
    // 3b. Resolve authoritative metadata (degrades to Unresolved on failure).
    let resolution = resolver.resolve(&ident).await;
```

Then REPLACE the record-building block (the `let (doi, arxiv_id) = match ... ;` through the `let paper = Paper { ... };`) with a single call to a new helper, keeping the surrounding copy/insert/cleanup/move logic intact:

```rust
    let paper = build_paper(content_hash, rel_path, title, &ident, resolution);
```

Note: `content_hash` and `rel_path` are moved into `build_paper`; make sure they are still available at this point (they are — `rel_path` was created for the copy in step 4, and `content_hash` from step 1). Keep the existing `let dest = dirs.library_root.join(&rel_path);` and `std::fs::copy(&path, &dest)?;` BEFORE this line so `rel_path` is used for the copy first, then moved into `build_paper`. Concretely the order in step 4/5 becomes:

```rust
    // 4. File the PDF into the managed library as <hash>.pdf.
    std::fs::create_dir_all(&dirs.library_root)?;
    let rel_path = format!("{content_hash}.pdf");
    let dest = dirs.library_root.join(&rel_path);
    std::fs::copy(&path, &dest)?;

    // 5. Build and store the record.
    let paper = build_paper(content_hash, rel_path, title, &ident, resolution);
    if let Err(e) = db::insert_paper(pool, &paper).await {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }
```

Add this helper function to `src/pipeline.rs` (after `ingest_file`, before `move_to`):

```rust
/// Assemble a `Paper` from the content hash, relative path, provisional title,
/// extracted identifier, and the resolution outcome. A confident resolution
/// yields `status = resolved` with authoritative fields; otherwise the record
/// stays `needs_review` with whatever the identifier/heuristics provided.
fn build_paper(
    content_hash: String,
    rel_path: String,
    provisional_title: Option<String>,
    ident: &Identifier,
    resolution: Resolution,
) -> Paper {
    let (ext_doi, ext_arxiv) = match ident {
        Identifier::Doi(d) => (Some(d.clone()), None),
        Identifier::Arxiv(a) => (None, Some(a.clone())),
        Identifier::None => (None, None),
    };
    let now = chrono::Utc::now().to_rfc3339();
    let id = Uuid::now_v7().to_string();

    match resolution {
        Resolution::Resolved(md) => {
            let authors = md.authors_json();
            Paper {
                id,
                content_hash,
                rel_path,
                title: md.title.or(provisional_title),
                abstract_text: md.abstract_text,
                authors,
                venue: md.venue,
                year: md.year,
                doi: md.doi.or(ext_doi),
                arxiv_id: md.arxiv_id.or(ext_arxiv),
                dblp_key: md.dblp_key,
                url: md.url,
                source: Some(md.source),
                status: PaperStatus::Resolved.as_str().to_string(),
                added_at: now,
            }
        }
        Resolution::Unresolved => Paper {
            id,
            content_hash,
            rel_path,
            title: provisional_title,
            abstract_text: None,
            authors: None,
            venue: None,
            year: None,
            doi: ext_doi,
            arxiv_id: ext_arxiv,
            dblp_key: None,
            url: None,
            source: None,
            status: PaperStatus::NeedsReview.as_str().to_string(),
            added_at: now,
        },
    }
}
```

(The previous inline `Uuid::now_v7()` / `chrono::...` calls now live in `build_paper`; ensure the `use uuid::Uuid;` import remains.)

- [ ] **Step 2: Update `src/main.rs` to build and pass a `Resolver`**

Add the import:

```rust
use xuewen::resolve::Resolver;
```

After `let pool = db::connect(&cfg.database_url).await?;` add:

```rust
    let resolver = Resolver::new(cfg.contact_email.as_deref())?;
```

Change the ingest call to pass the resolver:

```rust
        Command::Ingest { path } => match ingest_file(&pool, &dirs, &resolver, &path).await? {
```

- [ ] **Step 3: Update existing pipeline tests to pass a resolver, and add a resolved-path test**

In `tests/pipeline_test.rs`, add imports at the top:

```rust
use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::resolve::Resolver;
```

Both existing tests (`ingests_pdf_and_dedups`, `same_doi_different_bytes_errors_without_orphan`) must construct a resolver pointed at a **stub-less** mock server (so every lookup 404s → Unresolved → `needs_review`, preserving their existing assertions) and pass it to `ingest_file`. In each test, after `let pool = db::connect(...)...;` add:

```rust
    let mock = MockServer::start().await;
    let resolver = Resolver::with_bases(None, mock.uri(), mock.uri()).unwrap();
```

and change every `ingest_file(&pool, &dirs, &X).await` call to `ingest_file(&pool, &dirs, &resolver, &X).await`.

Then add this new test verifying the resolved path end-to-end:

```rust
const CROSSREF_FIXTURE: &str = include_str!("fixtures/crossref_kgat.json");

#[tokio::test]
async fn ingest_with_doi_resolves_via_crossref() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    let doi = "10.1145/3292500.3330701";
    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(
        &pdf_path,
        &["Some Provisional Header", &format!("https://doi.org/{doi}")],
    );

    // Mock Crossref returning the KGAT record for this DOI.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };

    let out = ingest_file(&pool, &dirs, &resolver, &pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };

    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.status, "resolved");
    assert_eq!(paper.source.as_deref(), Some("crossref"));
    assert_eq!(
        paper.title.as_deref(),
        Some("KGAT: Knowledge Graph Attention Network for Recommendation")
    );
    assert_eq!(paper.doi.as_deref(), Some(doi));
    assert_eq!(paper.year, Some(2019));
    assert!(paper.authors.as_deref().unwrap().contains("Xiang Wang"));
}
```

- [ ] **Step 4: Run the whole suite**

Run: `nix develop -c cargo test`
Expected: all pass — resolver unit tests, arxiv/crossref parse tests, `resolve_test` (4), and `pipeline_test` (now 3: the two updated tests still assert `needs_review`, plus the new resolved-path test). Also run `nix develop -c cargo clippy --all-targets 2>&1 | tail -20` and expect no new warnings.

- [ ] **Step 5: Commit**

```bash
git add src/pipeline.rs src/main.rs tests/pipeline_test.rs
git commit -m "feat: resolve metadata during ingest (arXiv/Crossref) and store resolved records"
```

---

## Definition of done (Plan 2a)

- Ingesting a PDF that carries a **DOI** stores a `resolved` record with Crossref title/authors/venue/year/url/abstract; a PDF with an **arXiv ID** stores a `resolved` record from the arXiv API.
- A PDF with **no identifier**, or when the lookup **fails/times out/returns non-2xx**, still stores a `needs_review` record (Plan 1 behavior preserved) — ingestion never aborts on a resolver failure.
- The extracted identifier is preserved even when resolution fails (`doi`/`arxiv_id` populated from `identify`).
- All tests pass offline (mock HTTP + recorded fixtures); clippy clean.

## What Plan 2b will add (not in scope here)

- `resolve/grobid.rs`: POST the PDF to a GROBID `processHeaderDocument` endpoint, parse the TEI header (title/abstract/authors).
- `resolve/dblp.rs`: DBLP title search (JSON API), returning candidate records with `dblp_key`/venue.
- `src/matching.rs`: title normalization + fuzzy similarity (add `strsim`); the confidence gate (accept a title-search candidate only when similarity ≥ threshold, else `needs_review`).
- Extend `Resolver::resolve` so `Identifier::None` runs GROBID → DBLP (Crossref fallback) through the confidence gate; wire `grobid_url` from `Config`.
```
