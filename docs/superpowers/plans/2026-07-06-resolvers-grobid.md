# Optional GROBID Header Extraction Implementation Plan (Slice 1, Plan 2c)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When a `grobid_url` is configured and reachable, extract a high-quality title/abstract/authors from a no-identifier PDF via a GROBID service; use the GROBID title (instead of the crude heuristic first-line) to drive the DBLP/Crossref title search, and enrich the stored record with GROBID's abstract/authors. GROBID is entirely optional — when unconfigured or unreachable, behavior is exactly Plan 2b.

**Architecture:** A new `resolve::grobid` module with a `Grobid` client (`extract_header`: multipart-POST the PDF to `/api/processHeaderDocument`, parse the returned TEI) and a pure `parse_tei`. The pipeline, for the `Identifier::None` path only, calls GROBID (if provided) to get a title hint and provisional metadata, then resolves as in Plan 2b. `build_paper` gains a provisional-metadata parameter: it backfills a missing abstract on `resolved` records and enriches `needs_review` records with GROBID's title/abstract/authors.

**Tech Stack:** Enables reqwest's `multipart` feature and tokio's `fs` feature. Reuses roxmltree (TEI XML), everything from Plans 1/2a/2b. No new crates.

---

## Plan set context

Slice 1 spec: `docs/superpowers/specs/2026-07-06-pdf-ingest-metadata-pipeline-design.md`.
- Plans 1, 2a, 2b (merged): ingest foundation; DOI→Crossref / arXiv→arXiv; DBLP title search + Crossref fallback + fuzzy gate.
- **Plan 2c (this file):** optional GROBID header extraction to improve title quality + enrich records.
- Plan 3 (later): `notify` watcher daemon.

### Current state (from Plan 2b, on `main`)
- `xuewen::resolve::{Resolver, ResolvedMetadata, Resolution}`; `ResolvedMetadata { title, abstract_text, authors: Vec<String>, venue, year, doi, arxiv_id, dblp_key, url, source }` with `authors_json()`; `pub(crate) fn collapse_ws(&str) -> String`.
- `Resolver::resolve(&self, ident: &Identifier, title_hint: Option<&str>) -> Resolution`.
- `resolve` submodules: `pub mod arxiv; pub mod crossref; pub mod dblp;`.
- `xuewen::config::Config { inbox_dir, library_root, database_url, grobid_url: Option<String>, contact_email: Option<String> }` — `grobid_url` already exists, currently unused.
- `xuewen::pipeline::ingest_file(pool, dirs, resolver: &Resolver, path)`: extracts text, `let ident = identify::identify(&text); let title = identify::guess_title(&text);`, `let resolution = resolver.resolve(&ident, title.as_deref()).await;`, copies file, `let paper = build_paper(content_hash, rel_path, title, &ident, resolution);`, insert-with-cleanup, move. `build_paper(content_hash: String, rel_path: String, provisional_title: Option<String>, ident: &Identifier, resolution: Resolution) -> Paper`.
- `main.rs`: builds `Config`, `pool`, `Resolver::new(cfg.contact_email.as_deref())`, `Libraries`, calls `ingest_file(&pool, &dirs, &resolver, &path)`.
- Run cargo via `nix develop -c '<command>'`. `reqwest` features are `["rustls-tls"]`; tokio features `["rt-multi-thread","macros"]`.

## File structure

```
Cargo.toml                     # reqwest + "multipart"; tokio + "fs"
tests/fixtures/
  grobid_bert.tei.xml          # recorded GROBID processHeaderDocument TEI
src/
  resolve/
    grobid.rs                  # Grobid client (extract_header) + parse_tei
    mod.rs                     # + pub mod grobid;
  pipeline.rs                  # ingest_file gains Option<&Grobid>; build_paper gains provisional md
  main.rs                      # builds Option<Grobid> from cfg.grobid_url
tests/
  grobid_test.rs               # wiremock: extract_header POST -> TEI -> ResolvedMetadata
  pipeline_test.rs             # updated calls + grobid-title-search + grobid-enriched-needs-review
```

**Module responsibility:** `resolve::grobid` turns a PDF into header metadata (title/abstract/authors) via the GROBID service — extraction, parallel to `pdf`/`identify`, producing a `ResolvedMetadata` with `source="grobid"`. It does not route or search; the pipeline orchestrates it.

---

## Task 1: Features + TEI fixture

**Files:** Modify `Cargo.toml`; create `tests/fixtures/grobid_bert.tei.xml`.

- [ ] **Step 1: Enable the required features in `Cargo.toml`**

Change the `reqwest` and `tokio` dependency lines to:
```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "fs"] }
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls", "multipart"] }
```
(Keep every other dependency unchanged. `multipart` is needed for the file upload; tokio `fs` for async file reads.)

- [ ] **Step 2: Create `tests/fixtures/grobid_bert.tei.xml`**

```xml
<?xml version="1.0" encoding="UTF-8"?>
<TEI xmlns="http://www.tei-c.org/ns/1.0">
  <teiHeader>
    <fileDesc>
      <titleStmt>
        <title level="a" type="main">BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding</title>
      </titleStmt>
      <sourceDesc>
        <biblStruct>
          <analytic>
            <author>
              <persName><forename type="first">Jacob</forename><surname>Devlin</surname></persName>
            </author>
            <author>
              <persName><forename type="first">Ming-Wei</forename><surname>Chang</surname></persName>
            </author>
          </analytic>
        </biblStruct>
      </sourceDesc>
    </fileDesc>
    <profileDesc>
      <abstract>
        <div><p>We introduce a new language representation model called BERT.</p></div>
      </abstract>
    </profileDesc>
  </teiHeader>
</TEI>
```

- [ ] **Step 3: Create `tests/fixtures/dblp_bert.json`** (a DBLP hit that matches the GROBID BERT title, used by a Task 4 test)

```json
{
  "result": {
    "hits": {
      "@total": "1",
      "hit": [
        {
          "info": {
            "authors": {
              "author": [
                {"text": "Jacob Devlin"},
                {"text": "Ming-Wei Chang"},
                {"text": "Kenton Lee"},
                {"text": "Kristina Toutanova"}
              ]
            },
            "title": "BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding.",
            "venue": "NAACL-HLT",
            "year": "2019",
            "key": "conf/naacl/DevlinCLT19",
            "doi": "10.18653/v1/n19-1423",
            "ee": "https://doi.org/10.18653/v1/n19-1423",
            "url": "https://dblp.org/rec/conf/naacl/DevlinCLT19"
          }
        }
      ]
    }
  }
}
```

- [ ] **Step 4: Build**

Run: `nix develop -c cargo build`
Expected: recompiles with the new features; `Finished`. If `reqwest::multipart` is reported missing later, the `multipart` feature here is what provides it.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock tests/fixtures/grobid_bert.tei.xml tests/fixtures/dblp_bert.json
git commit -m "chore: enable reqwest multipart + tokio fs; add GROBID TEI + DBLP BERT fixtures"
```

---

## Task 2: GROBID TEI parser

**Files:** Create `src/resolve/grobid.rs` (parser + a placeholder for the client added in Task 3); modify `src/resolve/mod.rs` (add `pub mod grobid;`).
**Test:** unit tests inside `src/resolve/grobid.rs`.

**roxmltree namespace note:** TEI uses `xmlns="http://www.tei-c.org/ns/1.0"`, so match by LOCAL name (`node.tag_name().name() == "title"`), never bare `has_tag_name`.

- [ ] **Step 1: Create `src/resolve/grobid.rs` with this content**

```rust
use anyhow::Result;

use super::{collapse_ws, ResolvedMetadata};

/// Parse a GROBID `processHeaderDocument` TEI response into metadata.
/// Returns `Ok(None)` if no title, abstract, or authors could be found.
pub fn parse_tei(xml: &str) -> Result<Option<ResolvedMetadata>> {
    let doc = roxmltree::Document::parse(xml)?;
    let is = |n: &roxmltree::Node, name: &str| n.tag_name().name() == name;

    // Title: prefer <title type="main">, else the first <title>.
    let title = doc
        .descendants()
        .find(|n| is(n, "title") && n.attribute("type") == Some("main"))
        .or_else(|| doc.descendants().find(|n| is(n, "title")))
        .and_then(|n| n.text())
        .map(collapse_ws)
        .filter(|s| !s.is_empty());

    // Abstract: concatenate every <p> under <abstract>.
    let abstract_text = doc
        .descendants()
        .find(|n| is(n, "abstract"))
        .map(|ab| {
            ab.descendants()
                .filter(|n| is(n, "p"))
                .filter_map(|p| p.text().map(collapse_ws))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|s| !s.is_empty());

    // Authors: each <author>'s <persName> -> forename(s) + surname.
    let authors: Vec<String> = doc
        .descendants()
        .filter(|n| is(n, "author"))
        .filter_map(|a| {
            let pn = a.descendants().find(|n| is(n, "persName"))?;
            let parts: Vec<String> = pn
                .descendants()
                .filter(|n| is(n, "forename") || is(n, "surname"))
                .filter_map(|n| n.text().map(|t| t.trim().to_string()))
                .filter(|s| !s.is_empty())
                .collect();
            (!parts.is_empty()).then(|| parts.join(" "))
        })
        .collect();

    if title.is_none() && abstract_text.is_none() && authors.is_empty() {
        return Ok(None);
    }
    Ok(Some(ResolvedMetadata {
        title,
        abstract_text,
        authors,
        venue: None,
        year: None,
        doi: None,
        arxiv_id: None,
        dblp_key: None,
        url: None,
        source: "grobid".to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/grobid_bert.tei.xml"));

    #[test]
    fn parses_tei_header() {
        let md = parse_tei(FIXTURE).unwrap().unwrap();
        assert_eq!(
            md.title.as_deref(),
            Some("BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding")
        );
        assert_eq!(md.authors, vec!["Jacob Devlin", "Ming-Wei Chang"]);
        assert_eq!(
            md.abstract_text.as_deref(),
            Some("We introduce a new language representation model called BERT.")
        );
        assert_eq!(md.source, "grobid");
    }

    #[test]
    fn empty_tei_is_none() {
        let xml = r#"<TEI xmlns="http://www.tei-c.org/ns/1.0"><teiHeader/></TEI>"#;
        assert!(parse_tei(xml).unwrap().is_none());
    }
}
```

- [ ] **Step 2:** In `src/resolve/mod.rs` add `pub mod grobid;`.

- [ ] **Step 3: Run the tests**

Run: `nix develop -c cargo test resolve::grobid::tests`
Expected: `parses_tei_header`, `empty_tei_is_none` PASS. If element lookups fail, confirm local-name matching (`tag_name().name()`), not bare `has_tag_name`.

- [ ] **Step 4: Commit**

```bash
git add src/resolve/grobid.rs src/resolve/mod.rs
git commit -m "feat: GROBID TEI header parser"
```

---

## Task 3: GROBID client (multipart upload)

**Files:** Modify `src/resolve/grobid.rs` (add the `Grobid` struct); create `tests/grobid_test.rs`.
**Test:** `tests/grobid_test.rs` using `wiremock`.

- [ ] **Step 1: Add the `Grobid` client to `src/resolve/grobid.rs`**

Add these imports at the top of the file (alongside the existing ones):
```rust
use anyhow::anyhow;
use std::path::Path;
use std::time::Duration;
```

Append this to `src/resolve/grobid.rs` (after `parse_tei`, before the `#[cfg(test)]` module):
```rust
/// A GROBID service client.
pub struct Grobid {
    http: reqwest::Client,
    base: String,
}

impl Grobid {
    /// Build a client for the GROBID service at `base` (e.g. `http://localhost:8070`).
    pub fn new(base: &str) -> Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(60))
            .build()?;
        Ok(Self {
            http,
            base: base.to_string(),
        })
    }

    /// Upload the PDF to `processHeaderDocument` and parse the TEI header.
    /// Returns `Ok(None)` if GROBID found nothing usable.
    pub async fn extract_header(&self, pdf_path: &Path) -> Result<Option<ResolvedMetadata>> {
        let bytes = tokio::fs::read(pdf_path).await?;
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name("input.pdf")
            .mime_str("application/pdf")?;
        let form = reqwest::multipart::Form::new().part("input", part);

        let resp = self
            .http
            .post(format!("{}/api/processHeaderDocument", self.base))
            .multipart(form)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(anyhow!("grobid HTTP {}", resp.status()));
        }
        let tei = resp.text().await?;
        parse_tei(&tei)
    }
}
```

- [ ] **Step 2: Create `tests/grobid_test.rs`**

```rust
use std::io::Write;

use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

use xuewen::resolve::grobid::Grobid;

const TEI_FIXTURE: &str = include_str!("fixtures/grobid_bert.tei.xml");

#[tokio::test]
async fn extract_header_posts_pdf_and_parses_tei() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/processHeaderDocument"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TEI_FIXTURE))
        .mount(&server)
        .await;

    // Any file works; the mock ignores the uploaded bytes.
    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(b"%PDF-1.4 dummy").unwrap();

    let grobid = Grobid::new(&server.uri()).unwrap();
    let md = grobid.extract_header(f.path()).await.unwrap().unwrap();

    assert_eq!(
        md.title.as_deref(),
        Some("BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding")
    );
    assert_eq!(md.authors, vec!["Jacob Devlin", "Ming-Wei Chang"]);
    assert_eq!(md.source, "grobid");
}

#[tokio::test]
async fn extract_header_errors_on_non_2xx() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/processHeaderDocument"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let mut f = tempfile::NamedTempFile::new().unwrap();
    f.write_all(b"%PDF-1.4 dummy").unwrap();

    let grobid = Grobid::new(&server.uri()).unwrap();
    assert!(grobid.extract_header(f.path()).await.is_err());
}
```

- [ ] **Step 3: Run the tests**

Run: `nix develop -c cargo test --test grobid_test`
Expected: both PASS. If `reqwest::multipart` is missing, confirm Task 1 added the `multipart` feature. If the `Part::bytes`/`mime_str` API differs, adjust the multipart construction minimally to upload the file bytes under form field `"input"` with an `application/pdf` mime; keep the assertions unchanged and report the change.

- [ ] **Step 4: Commit**

```bash
git add src/resolve/grobid.rs tests/grobid_test.rs
git commit -m "feat: GROBID client (multipart processHeaderDocument upload)"
```

---

## Task 4: Pipeline + CLI integration

**Files:** Modify `src/pipeline.rs`, `src/main.rs`, `tests/pipeline_test.rs`.
**Test:** `tests/pipeline_test.rs`.

- [ ] **Step 1: Update `src/pipeline.rs`**

**(a)** Add imports: at the top, add
```rust
use crate::resolve::grobid::Grobid;
use crate::resolve::ResolvedMetadata;
```
(The existing `use crate::resolve::{Resolution, Resolver};` stays.)

**(b)** Change `ingest_file` to accept an optional GROBID client:
```rust
pub async fn ingest_file(
    pool: &SqlitePool,
    dirs: &Libraries,
    resolver: &Resolver,
    grobid: Option<&Grobid>,
    path: &Path,
) -> Result<Outcome> {
```

**(c)** Replace the identify + resolve section (from `let ident = ...` through the resolve call) with GROBID-aware logic. After the text is extracted, it should read:
```rust
    let ident = identify::identify(&text);
    let heuristic_title = identify::guess_title(&text);

    // 3a. For the title-only path, optionally use GROBID to extract a better
    //     title/abstract/authors from the PDF header (degrades to None on failure).
    let extracted: Option<ResolvedMetadata> = match (&ident, grobid) {
        (Identifier::None, Some(g)) => match g.extract_header(&path).await {
            Ok(md) => md,
            Err(e) => {
                tracing::warn!("grobid extraction failed: {e}");
                None
            }
        },
        _ => None,
    };

    // 3b. Search query prefers the GROBID title, else the heuristic first line.
    let title_hint: Option<String> = extracted
        .as_ref()
        .and_then(|m| m.title.clone())
        .or_else(|| heuristic_title.clone());

    // 3c. Resolve authoritative metadata (degrades to Unresolved on failure).
    let resolution = resolver.resolve(&ident, title_hint.as_deref()).await;
```

**(d)** Change the `build_paper` call to pass the provisional metadata (the existing copy-before-insert-with-cleanup and move logic stays):
```rust
    let paper = build_paper(content_hash, rel_path, heuristic_title, extracted, &ident, resolution);
```

**(e)** Replace the `build_paper` function with this version (adds the `extracted: Option<ResolvedMetadata>` parameter; backfills abstract on resolved records and enriches needs_review records):
```rust
/// Assemble a `Paper` from the content hash, path, provisional title, optional
/// GROBID-extracted metadata, the identifier, and the resolution outcome.
/// A confident resolution yields `status = resolved` (with a GROBID abstract
/// backfilled if the bibliographic source lacked one). Otherwise the record is
/// `needs_review`, enriched with GROBID's title/abstract/authors when available.
fn build_paper(
    content_hash: String,
    rel_path: String,
    provisional_title: Option<String>,
    extracted: Option<ResolvedMetadata>,
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
            let abstract_text = md
                .abstract_text
                .or_else(|| extracted.and_then(|g| g.abstract_text));
            Paper {
                id,
                content_hash,
                rel_path,
                title: md.title.or(provisional_title),
                abstract_text,
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
        Resolution::Unresolved => {
            let (title, abstract_text, authors, source) = match extracted {
                Some(g) => {
                    let authors = g.authors_json();
                    (g.title.or(provisional_title), g.abstract_text, authors, Some(g.source))
                }
                None => (provisional_title, None, None, None),
            };
            Paper {
                id,
                content_hash,
                rel_path,
                title,
                abstract_text,
                authors,
                venue: None,
                year: None,
                doi: ext_doi,
                arxiv_id: ext_arxiv,
                dblp_key: None,
                url: None,
                source,
                status: PaperStatus::NeedsReview.as_str().to_string(),
                added_at: now,
            }
        }
    }
}
```

- [ ] **Step 2: Update `src/main.rs`**

Add import: `use xuewen::resolve::grobid::Grobid;`

After the `let resolver = ...;` line, build an optional GROBID client from config:
```rust
    let grobid = cfg
        .grobid_url
        .as_deref()
        .map(Grobid::new)
        .transpose()?;
```
Change the ingest call to pass it:
```rust
        Command::Ingest { path } => {
            match ingest_file(&pool, &dirs, &resolver, grobid.as_ref(), &path).await? {
```

- [ ] **Step 3: Update `tests/pipeline_test.rs`**

Every existing `ingest_file(&pool, &dirs, &resolver, &X)` call must gain a `None` GROBID argument: `ingest_file(&pool, &dirs, &resolver, None, &X)`. There are calls in `ingests_pdf_and_dedups` (two), `same_doi_different_bytes_errors_without_orphan` (two), `ingest_with_doi_resolves_via_crossref` (one), `ingest_with_arxiv_resolves_via_api` (one), and `ingest_without_identifier_resolves_via_dblp` (one) — update them all to insert `None` before `&path`/`&pdf_path`/`&again`.

Then add these two tests. Add the GROBID import and the two new fixture consts at the top (`DBLP_FIXTURE` for the KGAT hit already exists from Plan 2b — leave it):
```rust
use xuewen::resolve::grobid::Grobid;

const TEI_FIXTURE: &str = include_str!("fixtures/grobid_bert.tei.xml");
const DBLP_BERT_FIXTURE: &str = include_str!("fixtures/dblp_bert.json");
```
Append:
```rust
#[tokio::test]
async fn grobid_title_drives_dblp_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    // The PDF's own text is a poor/truncated title; GROBID supplies the clean one.
    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(&pdf_path, &["BERT Pre-training of Deep Bidir"]);

    // GROBID returns the full BERT header; DBLP is stubbed to match a BERT query.
    let grobid_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wm_path("/api/processHeaderDocument"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TEI_FIXTURE))
        .mount(&grobid_server)
        .await;
    let grobid = Grobid::new(&grobid_server.uri()).unwrap();

    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_BERT_FIXTURE))
        .mount(&api_server)
        .await;
    let resolver = Resolver::with_bases(None, api_server.uri(), api_server.uri())
        .unwrap()
        .with_dblp_base(api_server.uri());

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };

    let out = ingest_file(&pool, &dirs, &resolver, Some(&grobid), &pdf_path)
        .await
        .unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };
    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    // DBLP matched (its title fuzzily matches the GROBID title) -> resolved via dblp.
    assert_eq!(paper.status, "resolved");
    assert_eq!(paper.source.as_deref(), Some("dblp"));
    // DBLP has no abstract; the GROBID abstract is backfilled.
    assert!(paper.abstract_text.as_deref().unwrap().contains("language representation model"));
}

#[tokio::test]
async fn grobid_enriches_needs_review_when_unmatched() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(&pdf_path, &["garbled first line xyz"]);

    let grobid_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wm_path("/api/processHeaderDocument"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TEI_FIXTURE))
        .mount(&grobid_server)
        .await;
    let grobid = Grobid::new(&grobid_server.uri()).unwrap();

    // Resolver points at a stub-less server: DBLP + Crossref both 404 -> Unresolved.
    let api_server = MockServer::start().await;
    let resolver = Resolver::with_bases(None, api_server.uri(), api_server.uri())
        .unwrap()
        .with_dblp_base(api_server.uri());

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };

    let out = ingest_file(&pool, &dirs, &resolver, Some(&grobid), &pdf_path)
        .await
        .unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };
    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.status, "needs_review");
    assert_eq!(paper.source.as_deref(), Some("grobid"));
    // GROBID title/abstract/authors replace the garbled heuristic.
    assert_eq!(
        paper.title.as_deref(),
        Some("BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding")
    );
    assert!(paper.authors.as_deref().unwrap().contains("Jacob Devlin"));
}
```

- [ ] **Step 4: Run the whole suite + clippy**

Run: `nix develop -c cargo test`
Expected: ALL pass — grobid unit (2), grobid_test (2), the two new pipeline tests, and every prior test (existing pipeline tests now pass `None` for grobid). Roughly 46 tests total.
Run: `nix develop -c cargo clippy --all-targets 2>&1 | tail -20`
Expected: no new warnings.

- [ ] **Step 5: Commit**

```bash
git add src/pipeline.rs src/main.rs tests/pipeline_test.rs
git commit -m "feat: optional GROBID header extraction for title-search + record enrichment"
```

---

## Definition of done (Plan 2c)

- With `grobid_url` set and reachable, a no-identifier PDF's DBLP/Crossref search uses the **GROBID title** (not the crude heuristic), and a resolved record backfills its abstract from GROBID when the bibliographic source has none.
- When nothing matches, the `needs_review` record is **enriched** with GROBID's title/abstract/authors (`source="grobid"`).
- With `grobid_url` unset (or GROBID unreachable/erroring), behavior is exactly Plan 2b — GROBID is never required and never aborts ingestion.
- DOI/arXiv paths are unchanged (GROBID is only invoked for `Identifier::None`).
- All tests pass offline (wiremock + fixtures); clippy clean.

## Slice 1 status after this plan

The metadata-resolution half of slice 1 is complete: exact identifiers (DOI/arXiv), title search (DBLP/Crossref), and optional GROBID extraction. The remaining slice-1 piece is **Plan 3 — the `notify` watcher daemon** (auto-ingest on drop, debounce, catch-up scan, retry/backoff), which turns the CLI `ingest` into the "watch dir" service from the original spec.
