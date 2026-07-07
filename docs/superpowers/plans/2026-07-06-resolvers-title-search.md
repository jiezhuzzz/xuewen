# DBLP Title-Search Resolver Implementation Plan (Slice 1, Plan 2b)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** For a PDF with no DOI/arXiv identifier, use its heuristic title to search DBLP (CS-focused), fall back to a Crossref bibliographic search, and accept the best candidate only when its title fuzzily matches the query above a threshold — storing a `resolved` record; otherwise keep the Plan-1 `needs_review` behavior.

**Architecture:** A new `matching` module (title normalization + `strsim` similarity + a confidence gate) and a new `resolve::dblp` client (search → parse candidate list). `resolve::crossref` gains a bibliographic-search path (refactored to share a per-item parser). `Resolver::resolve` gains a `title_hint` and a title-search route for `Identifier::None`: DBLP first, Crossref second, each filtered through the confidence gate. Failures degrade to `Unresolved`, exactly as in Plan 2a.

**Tech Stack:** Adds `strsim` (fuzzy string similarity). Reuses reqwest/serde_json/roxmltree/wiremock and everything from Plans 1 & 2a. No GROBID, no new services.

---

## Plan set context

Slice 1 spec: `docs/superpowers/specs/2026-07-06-pdf-ingest-metadata-pipeline-design.md`.
- Plan 1 (merged): offline ingest foundation.
- Plan 2a (merged): exact-identifier resolvers (DOI→Crossref, arXiv→arXiv API), with `Resolver`, `Resolution::{Resolved,Unresolved}`, `ResolvedMetadata`, and graceful degradation.
- **Plan 2b (this file):** title-only path — DBLP title search + Crossref bibliographic fallback + fuzzy confidence gate. Brings in DBLP.
- Plan 2c (later, optional): GROBID header extraction to produce a *better* title/abstract than the heuristic (and enrich `needs_review` records). Deferred by explicit user choice — Plan 2b must NOT depend on any GROBID service.
- Plan 3: `notify` watcher daemon.

### Current state (from Plan 2a, on `main`)
- `xuewen::resolve::Resolver`:
  - `Resolver::new(contact_email: Option<&str>) -> Result<Resolver>` (real endpoints)
  - `Resolver::with_bases(contact_email: Option<&str>, arxiv_base: String, crossref_base: String) -> Result<Resolver>` (test injection)
  - `async fn resolve(&self, ident: &Identifier) -> Resolution` — routes Doi→crossref, Arxiv→arxiv, None→Unresolved; degrades errors to `Unresolved`.
  - private fields: `http: reqwest::Client`, `arxiv_base: String`, `crossref_base: String`.
- `xuewen::resolve::ResolvedMetadata { title, abstract_text, authors: Vec<String>, venue, year: Option<i64>, doi, arxiv_id, dblp_key, url, source: String }` with `authors_json(&self) -> Option<String>`; `pub(crate) fn collapse_ws(&str) -> String`; `pub(crate) fn strip_tags(&str) -> String`.
- `xuewen::resolve::crossref`: `pub async fn fetch(client, base, doi) -> Result<String>`, `pub fn parse(&str) -> Result<Option<ResolvedMetadata>>`.
- `xuewen::pipeline::ingest_file(pool, dirs, resolver: &Resolver, path)` calls `resolver.resolve(&ident).await`, then `build_paper(content_hash, rel_path, title, &ident, resolution)`.
- Tests: `tests/resolve_test.rs` (5), `tests/pipeline_test.rs` (4). Run cargo via `nix develop -c '<command>'`.

## File structure

```
Cargo.toml                          # + strsim
tests/fixtures/
  dblp_kgat.json                    # recorded DBLP publ search response
  crossref_search_kgat.json         # recorded Crossref /works?query.bibliographic response
src/
  matching.rs                       # normalize_title, title_similarity, is_confident_match, MATCH_THRESHOLD
  lib.rs                            # + pub mod matching;
  resolve/
    mod.rs                          # Resolver: + dblp_base, with_dblp_base, title_hint param, title-search route, best_match
    dblp.rs                         # DBLP search fetch + parse (candidate list)
    crossref.rs                     # refactor: parse_item; + search fetch + parse_search
  pipeline.rs                       # pass title_hint into resolve
tests/
  resolve_test.rs                   # updated resolve() calls (title_hint arg) + DBLP/fallback/low-sim tests
  pipeline_test.rs                  # + title-search resolved pipeline test
```

---

## Task 1: Dependency + fixtures

**Files:** Modify `Cargo.toml`; create `tests/fixtures/dblp_kgat.json`, `tests/fixtures/crossref_search_kgat.json`.

- [ ] **Step 1: Add to `Cargo.toml` `[dependencies]`:**

```toml
strsim = "0.11"
```

- [ ] **Step 2: Create `tests/fixtures/dblp_kgat.json`**

```json
{
  "result": {
    "hits": {
      "@total": "1",
      "@sent": "1",
      "hit": [
        {
          "@score": "5",
          "info": {
            "authors": {
              "author": [
                {"@pid": "180/5794", "text": "Xiang Wang"},
                {"@pid": "59/1007", "text": "Xiangnan He"},
                {"@pid": "24/8048", "text": "Yixin Cao"}
              ]
            },
            "title": "KGAT: Knowledge Graph Attention Network for Recommendation.",
            "venue": "KDD",
            "year": "2019",
            "type": "Conference and Workshop Papers",
            "key": "conf/kdd/WangHCLC19",
            "doi": "10.1145/3292500.3330701",
            "ee": "https://doi.org/10.1145/3292500.3330701",
            "url": "https://dblp.org/rec/conf/kdd/WangHCLC19"
          }
        }
      ]
    }
  }
}
```

- [ ] **Step 3: Create `tests/fixtures/crossref_search_kgat.json`**

```json
{
  "status": "ok",
  "message": {
    "items": [
      {
        "DOI": "10.1145/3292500.3330701",
        "title": ["KGAT: Knowledge Graph Attention Network for Recommendation"],
        "author": [
          {"given": "Xiang", "family": "Wang"},
          {"given": "Xiangnan", "family": "He"}
        ],
        "container-title": ["Proceedings of the 25th ACM SIGKDD International Conference on Knowledge Discovery & Data Mining"],
        "issued": {"date-parts": [[2019, 7, 25]]},
        "URL": "http://dx.doi.org/10.1145/3292500.3330701"
      }
    ]
  }
}
```

- [ ] **Step 4: Build**

Run: `nix develop -c cargo build`
Expected: `strsim` resolves and compiles; `Finished`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock tests/fixtures/dblp_kgat.json tests/fixtures/crossref_search_kgat.json
git commit -m "chore: add strsim + DBLP/Crossref-search fixtures"
```

---

## Task 2: Matching module (title normalization + confidence gate)

**Files:** Create `src/matching.rs`; modify `src/lib.rs`.
**Test:** unit tests inside `src/matching.rs`.

- [ ] **Step 1: Create `src/matching.rs`**

```rust
use strsim::normalized_levenshtein;

/// Similarity (0.0–1.0) at or above which a candidate title is accepted as a match.
pub const MATCH_THRESHOLD: f64 = 0.85;

/// Lowercase, replace every non-alphanumeric char with a space, and collapse whitespace.
pub fn normalize_title(s: &str) -> String {
    let spaced: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .to_lowercase();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Normalized-Levenshtein similarity of two titles after normalization.
pub fn title_similarity(a: &str, b: &str) -> f64 {
    normalized_levenshtein(&normalize_title(a), &normalize_title(b))
}

/// Whether `candidate` is a confident title match for `query`.
pub fn is_confident_match(query: &str, candidate: &str) -> bool {
    title_similarity(query, candidate) >= MATCH_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_punctuation_and_case() {
        assert_eq!(
            normalize_title("KGAT: Knowledge-Graph  Attention Network!"),
            "kgat knowledge graph attention network"
        );
    }

    #[test]
    fn identical_titles_are_confident() {
        let q = "KGAT: Knowledge Graph Attention Network for Recommendation";
        let c = "KGAT: Knowledge Graph Attention Network for Recommendation.";
        assert!(title_similarity(q, c) > 0.95);
        assert!(is_confident_match(q, c));
    }

    #[test]
    fn unrelated_titles_are_not_confident() {
        assert!(!is_confident_match(
            "Deep Residual Learning for Image Recognition",
            "Attention Is All You Need"
        ));
    }
}
```

- [ ] **Step 2: Declare the module** — in `src/lib.rs` add `pub mod matching;`.

- [ ] **Step 3: Run the tests**

Run: `nix develop -c cargo test matching::tests`
Expected: all three PASS. If `normalize_strips_punctuation_and_case` fails, check that non-alphanumeric chars (including `-` and `:`) map to spaces and whitespace collapses.

- [ ] **Step 4: Commit**

```bash
git add src/matching.rs src/lib.rs
git commit -m "feat: title normalization + fuzzy confidence gate (strsim)"
```

---

## Task 3: DBLP client (search + parse candidates)

**Files:** Create `src/resolve/dblp.rs`; modify `src/resolve/mod.rs` (add `pub mod dblp;`).
**Test:** unit tests inside `src/resolve/dblp.rs`.

**DBLP JSON quirks handled below:** `result.hits.hit` is absent when there are 0 results, and can be a single object or an array; `info.authors.author` and `info.venue` can each be a single value or an array; titles end with a trailing `.`. The helpers normalize all of these.

- [ ] **Step 1: Create `src/resolve/dblp.rs`**

```rust
use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{collapse_ws, ResolvedMetadata};

/// Search DBLP publications by title. Returns raw JSON.
pub async fn fetch(client: &reqwest::Client, base: &str, title: &str) -> Result<String> {
    let resp = client
        .get(format!("{base}/search/publ/api"))
        .query(&[("q", title), ("format", "json"), ("h", "5")])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow!("dblp HTTP {}", resp.status()));
    }
    Ok(resp.text().await?)
}

/// Parse a DBLP publ-search response into candidate records (possibly empty).
pub fn parse(json: &str) -> Result<Vec<ResolvedMetadata>> {
    let v: Value = serde_json::from_str(json)?;
    let hits = values(&v["result"]["hits"]["hit"]);

    let mut out = Vec::new();
    for h in hits {
        let info = &h["info"];
        if info.is_null() {
            continue;
        }
        let title = info["title"].as_str().map(clean_title);
        let year = info["year"].as_str().and_then(|s| s.parse::<i64>().ok());
        let venue = venue_of(&info["venue"]);
        let doi = info["doi"].as_str().map(str::to_string);
        let url = info["ee"]
            .as_str()
            .or_else(|| info["url"].as_str())
            .map(str::to_string);
        let dblp_key = info["key"].as_str().map(str::to_string);
        let authors = values(&info["authors"]["author"])
            .iter()
            .filter_map(|a| a["text"].as_str().map(collapse_ws))
            .collect();

        out.push(ResolvedMetadata {
            title,
            abstract_text: None,
            authors,
            venue,
            year,
            doi,
            arxiv_id: None,
            dblp_key,
            url,
            source: "dblp".to_string(),
        });
    }
    Ok(out)
}

/// A DBLP JSON field that may be a single object, an array, or absent.
fn values(v: &Value) -> Vec<&Value> {
    if let Some(arr) = v.as_array() {
        arr.iter().collect()
    } else if v.is_null() {
        Vec::new()
    } else {
        vec![v]
    }
}

/// DBLP `venue` may be a string or an array of strings.
fn venue_of(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        Some(collapse_ws(s))
    } else if let Some(arr) = v.as_array() {
        arr.iter().find_map(|x| x.as_str()).map(collapse_ws)
    } else {
        None
    }
}

/// DBLP titles end with a trailing period; strip it and collapse whitespace.
fn clean_title(s: &str) -> String {
    collapse_ws(s.trim_end_matches('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/dblp_kgat.json"));

    #[test]
    fn parses_dblp_hit() {
        let cands = parse(FIXTURE).unwrap();
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(
            c.title.as_deref(),
            Some("KGAT: Knowledge Graph Attention Network for Recommendation")
        );
        assert_eq!(c.year, Some(2019));
        assert_eq!(c.venue.as_deref(), Some("KDD"));
        assert_eq!(c.doi.as_deref(), Some("10.1145/3292500.3330701"));
        assert_eq!(c.dblp_key.as_deref(), Some("conf/kdd/WangHCLC19"));
        assert_eq!(c.authors, vec!["Xiang Wang", "Xiangnan He", "Yixin Cao"]);
        assert_eq!(c.source, "dblp");
    }

    #[test]
    fn zero_hits_is_empty() {
        let json = r#"{"result":{"hits":{"@total":"0"}}}"#;
        assert!(parse(json).unwrap().is_empty());
    }

    #[test]
    fn single_author_object_is_handled() {
        // DBLP returns a bare object (not array) when there is exactly one author.
        let json = r#"{"result":{"hits":{"hit":[{"info":{
            "title":"A One Author Paper.","year":"2020",
            "authors":{"author":{"@pid":"1","text":"Solo Writer"}}
        }}]}}}"#;
        let cands = parse(json).unwrap();
        assert_eq!(cands[0].authors, vec!["Solo Writer"]);
    }
}
```

- [ ] **Step 2: Declare the submodule** — in `src/resolve/mod.rs` add `pub mod dblp;`.

- [ ] **Step 3: Run the tests**

Run: `nix develop -c cargo test resolve::dblp::tests`
Expected: `parses_dblp_hit`, `zero_hits_is_empty`, `single_author_object_is_handled` PASS.

- [ ] **Step 4: Commit**

```bash
git add src/resolve/dblp.rs src/resolve/mod.rs
git commit -m "feat: DBLP publ-search client (fetch + parse candidates)"
```

---

## Task 4: Crossref bibliographic search (refactor + search)

**Files:** Modify `src/resolve/crossref.rs`.
**Test:** unit tests inside `src/resolve/crossref.rs` (keep the existing `parses_crossref_work` passing; add a search test).

- [ ] **Step 1: Refactor `parse` to share a per-item parser, and add search functions**

In `src/resolve/crossref.rs`, extract the field-extraction logic from `parse` into a private `parse_item(&Value) -> ResolvedMetadata`, and have `parse` call it. Then add `search` (fetch) and `parse_search`. The full file becomes:

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

/// Search Crossref by bibliographic string (title). Returns raw JSON.
pub async fn search(client: &reqwest::Client, base: &str, title: &str) -> Result<String> {
    let resp = client
        .get(format!("{base}/works"))
        .query(&[("query.bibliographic", title), ("rows", "5")])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow!("crossref search HTTP {}", resp.status()));
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
    Ok(Some(parse_item(m)))
}

/// Parse a Crossref `/works?query...` search body into candidate records.
pub fn parse_search(json: &str) -> Result<Vec<ResolvedMetadata>> {
    let v: Value = serde_json::from_str(json)?;
    let items = v["message"]["items"].as_array();
    Ok(items
        .map(|arr| arr.iter().map(parse_item).collect())
        .unwrap_or_default())
}

/// Extract normalized metadata from a single Crossref work object
/// (either `message` for a DOI lookup or one element of `message.items`).
fn parse_item(m: &Value) -> ResolvedMetadata {
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

    ResolvedMetadata {
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/crossref_kgat.json"));
    const SEARCH_FIXTURE: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/crossref_search_kgat.json"));

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

    #[test]
    fn parses_crossref_search_items() {
        let cands = parse_search(SEARCH_FIXTURE).unwrap();
        assert_eq!(cands.len(), 1);
        assert_eq!(
            cands[0].title.as_deref(),
            Some("KGAT: Knowledge Graph Attention Network for Recommendation")
        );
        assert_eq!(cands[0].doi.as_deref(), Some("10.1145/3292500.3330701"));
        assert_eq!(cands[0].year, Some(2019));
    }

    #[test]
    fn empty_search_is_empty() {
        assert!(parse_search(r#"{"message":{"items":[]}}"#).unwrap().is_empty());
    }
}
```

- [ ] **Step 2: Run the tests**

Run: `nix develop -c cargo test resolve::crossref::tests`
Expected: all four PASS (the two original still pass after the refactor; two new search tests pass).

- [ ] **Step 3: Commit**

```bash
git add src/resolve/crossref.rs
git commit -m "feat: Crossref bibliographic search (parse_item refactor + parse_search)"
```

---

## Task 5: Resolver title-search route (DBLP → Crossref, confidence gate)

**Files:** Modify `src/resolve/mod.rs`, `src/pipeline.rs`, `tests/resolve_test.rs`.
**Test:** `tests/resolve_test.rs`.

- [ ] **Step 1: Extend the `Resolver` in `src/resolve/mod.rs`**

Add `use crate::matching;` to the imports at the top of `src/resolve/mod.rs` (alongside the existing `use crate::models::Identifier;`).

Add a `dblp_base` field to the struct:
```rust
pub struct Resolver {
    http: reqwest::Client,
    arxiv_base: String,
    crossref_base: String,
    dblp_base: String,
}
```

In `with_bases`, initialize `dblp_base` to the real endpoint (keep the existing 3-argument signature so Plan 2a call sites are unchanged):
```rust
        Ok(Self {
            http,
            arxiv_base,
            crossref_base,
            dblp_base: "https://dblp.org".to_string(),
        })
```

Add a builder to override the DBLP base (used by tests):
```rust
    /// Override the DBLP base URL (used by tests to point at a mock server).
    pub fn with_dblp_base(mut self, base: String) -> Self {
        self.dblp_base = base;
        self
    }
```

Change `resolve` to take a `title_hint` and route `Identifier::None` to a title search:
```rust
    /// Route an identifier to its source and return the outcome. For a PDF with
    /// no identifier, `title_hint` (the heuristic title) drives a DBLP/Crossref
    /// title search.
    pub async fn resolve(&self, ident: &Identifier, title_hint: Option<&str>) -> Resolution {
        let md = match ident {
            Identifier::Arxiv(id) => self.try_arxiv(id).await,
            Identifier::Doi(doi) => self.try_crossref(doi).await,
            Identifier::None => self.try_title_search(title_hint).await,
        };
        match md {
            Some(m) => Resolution::Resolved(m),
            None => Resolution::Unresolved,
        }
    }
```

Add the title-search methods (place them alongside the other `try_*` methods):
```rust
    /// DBLP first, then Crossref bibliographic search; each filtered by the gate.
    async fn try_title_search(&self, title: Option<&str>) -> Option<ResolvedMetadata> {
        let title = title?;
        if title.trim().is_empty() {
            return None;
        }
        if let Some(md) = self.try_dblp(title).await {
            return Some(md);
        }
        self.try_crossref_search(title).await
    }

    async fn try_dblp(&self, title: &str) -> Option<ResolvedMetadata> {
        match self.fetch_parse_dblp(title).await {
            Ok(cands) => best_match(title, cands),
            Err(e) => {
                tracing::warn!("dblp search failed for {title:?}: {e}");
                None
            }
        }
    }

    async fn fetch_parse_dblp(&self, title: &str) -> Result<Vec<ResolvedMetadata>> {
        let body = dblp::fetch(&self.http, &self.dblp_base, title).await?;
        dblp::parse(&body)
    }

    async fn try_crossref_search(&self, title: &str) -> Option<ResolvedMetadata> {
        match self.fetch_parse_crossref_search(title).await {
            Ok(cands) => best_match(title, cands),
            Err(e) => {
                tracing::warn!("crossref search failed for {title:?}: {e}");
                None
            }
        }
    }

    async fn fetch_parse_crossref_search(&self, title: &str) -> Result<Vec<ResolvedMetadata>> {
        let body = crossref::search(&self.http, &self.crossref_base, title).await?;
        crossref::parse_search(&body)
    }
```

Add a free function `best_match` (after the `impl Resolver` block, alongside the other module items):
```rust
/// Pick the highest-similarity candidate whose title confidently matches `query`.
fn best_match(query: &str, candidates: Vec<ResolvedMetadata>) -> Option<ResolvedMetadata> {
    let mut best: Option<(f64, ResolvedMetadata)> = None;
    for c in candidates {
        let score = match c.title.as_deref() {
            Some(t) => matching::title_similarity(query, t),
            None => continue,
        };
        if score >= matching::MATCH_THRESHOLD && best.as_ref().map_or(true, |(bs, _)| score > *bs) {
            best = Some((score, c));
        }
    }
    best.map(|(_, c)| c)
}
```

- [ ] **Step 2: Keep the pipeline caller compiling (minimal, temporary)**

In `src/pipeline.rs`, the call `resolver.resolve(&ident).await` no longer compiles (new arg). Change it to pass `None` for now (Task 6 wires the real title):
```rust
    let resolution = resolver.resolve(&ident, None).await;
```

- [ ] **Step 3: Update existing `tests/resolve_test.rs` calls and add title-search tests**

Every existing `resolver.resolve(&Identifier::X(...))` / `resolver.resolve(&Identifier::None)` call must gain a second argument `None`:
- `resolves_doi_via_crossref`: `resolver.resolve(&Identifier::Doi(doi.to_string()), None).await`
- `resolves_arxiv_via_api`: `resolver.resolve(&Identifier::Arxiv(id.to_string()), None).await`
- `http_error_degrades_to_unresolved`: `resolver.resolve(&Identifier::Doi("10.9999/nope".to_string()), None).await`
- `none_identifier_is_unresolved`: `resolver.resolve(&Identifier::None, None).await`
- `parse_error_degrades_to_unresolved`: `resolver.resolve(&Identifier::Doi(doi.to_string()), None).await`

Add these imports if not present (path matcher already imported): the tests below use `query_param` and `include_str!` fixtures. Then append:

```rust
const DBLP_FIXTURE: &str = include_str!("fixtures/dblp_kgat.json");
const CROSSREF_SEARCH_FIXTURE: &str = include_str!("fixtures/crossref_search_kgat.json");

const KGAT_TITLE: &str = "KGAT: Knowledge Graph Attention Network for Recommendation";

#[tokio::test]
async fn resolves_title_via_dblp() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let res = resolver.resolve(&Identifier::None, Some(KGAT_TITLE)).await;

    match res {
        Resolution::Resolved(md) => {
            assert_eq!(md.source, "dblp");
            assert_eq!(md.dblp_key.as_deref(), Some("conf/kdd/WangHCLC19"));
            assert_eq!(md.venue.as_deref(), Some("KDD"));
            assert_eq!(md.year, Some(2019));
        }
        Resolution::Unresolved => panic!("expected Resolved via DBLP"),
    }
}

#[tokio::test]
async fn falls_back_to_crossref_search_when_dblp_empty() {
    let server = MockServer::start().await;
    // DBLP returns zero hits...
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"result":{"hits":{"@total":"0"}}}"#))
        .mount(&server)
        .await;
    // ...Crossref bibliographic search returns the match.
    Mock::given(method("GET"))
        .and(path("/works"))
        .and(query_param("query.bibliographic", KGAT_TITLE))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_SEARCH_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let res = resolver.resolve(&Identifier::None, Some(KGAT_TITLE)).await;

    match res {
        Resolution::Resolved(md) => {
            assert_eq!(md.source, "crossref");
            assert_eq!(md.doi.as_deref(), Some("10.1145/3292500.3330701"));
        }
        Resolution::Unresolved => panic!("expected Resolved via Crossref fallback"),
    }
}

#[tokio::test]
async fn low_similarity_title_is_unresolved() {
    let server = MockServer::start().await;
    // DBLP returns the KGAT hit, but our query is unrelated -> below threshold.
    Mock::given(method("GET"))
        .and(path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;
    // Crossref search has no stub -> 404 -> None. So overall Unresolved.

    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let res = resolver
        .resolve(&Identifier::None, Some("An Entirely Unrelated Paper Title About Frogs"))
        .await;
    assert_eq!(res, Resolution::Unresolved);
}
```

- [ ] **Step 4: Run the resolver tests + build**

Run: `nix develop -c cargo test --test resolve_test`
Expected: all PASS (5 updated + 3 new = 8). Also run `nix develop -c cargo build` to confirm `src/pipeline.rs` still compiles with the `None` arg.

- [ ] **Step 5: Commit**

```bash
git add src/resolve/mod.rs src/pipeline.rs tests/resolve_test.rs
git commit -m "feat: title-search route (DBLP -> Crossref) with fuzzy confidence gate"
```

---

## Task 6: Pipeline wiring + end-to-end title-search test

**Files:** Modify `src/pipeline.rs`, `tests/pipeline_test.rs`.
**Test:** `tests/pipeline_test.rs`.

- [ ] **Step 1: Pass the heuristic title into `resolve`**

In `src/pipeline.rs`, change the resolve call to pass the provisional title:
```rust
    // 3b. Resolve authoritative metadata (degrades to Unresolved on failure).
    let resolution = resolver.resolve(&ident, title.as_deref()).await;
```
(`title` is the `Option<String>` from `identify::guess_title(&text)`; `.as_deref()` yields `Option<&str>`. It is used here BEFORE being moved into `build_paper` later — reading via `as_deref()` only borrows, so the later move is fine.)

- [ ] **Step 2: Add an end-to-end title-search pipeline test**

Append to `tests/pipeline_test.rs`:
```rust
const DBLP_FIXTURE: &str = include_str!("fixtures/dblp_kgat.json");

#[tokio::test]
async fn ingest_without_identifier_resolves_via_dblp() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    // No DOI/arXiv anywhere; the first substantive line is the title.
    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(
        &pdf_path,
        &["KGAT: Knowledge Graph Attention Network for Recommendation"],
    );

    // DBLP mock returns the matching hit.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());

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
    assert_eq!(paper.source.as_deref(), Some("dblp"));
    assert_eq!(paper.dblp_key.as_deref(), Some("conf/kdd/WangHCLC19"));
    assert_eq!(paper.venue.as_deref(), Some("KDD"));
    assert_eq!(paper.year, Some(2019));
    assert!(paper.doi.as_deref().is_some());
}
```

- [ ] **Step 3: Run the whole suite + clippy**

Run: `nix develop -c cargo test`
Expected: ALL pass — matching (3), resolve unit (arxiv/crossref 4+dblp 3), resolve_test (8), pipeline_test (5), plus prior lib unit tests. Roughly 33 tests total across suites.
Run: `nix develop -c cargo clippy --all-targets 2>&1 | tail -20`
Expected: no new warnings (the pre-existing `#[allow(clippy::large_enum_variant)]` on `Resolution` remains).

- [ ] **Step 4: Commit**

```bash
git add src/pipeline.rs tests/pipeline_test.rs
git commit -m "feat: use heuristic title for DBLP/Crossref title search during ingest"
```

---

## Definition of done (Plan 2b)

- A PDF with **no DOI/arXiv ID** whose heuristic title confidently matches a DBLP record is stored `resolved` with `source="dblp"`, `dblp_key`, venue, year, authors, doi/url.
- When DBLP has no confident match, a **Crossref bibliographic search** is tried; a confident match yields `source="crossref"`.
- When neither yields a title above `MATCH_THRESHOLD` (0.85), or a search fails, the record stays `needs_review` (Plan 1/2a behavior preserved) — ingestion never aborts.
- DOI and arXiv paths (Plan 2a) are unchanged.
- All tests pass offline (wiremock + fixtures); clippy clean.

## What Plan 2c will add (not in scope here)

- `resolve/grobid.rs`: POST the PDF to a GROBID `processHeaderDocument` endpoint, parse the TEI header for a high-quality title/abstract/authors. Optional — only used when `grobid_url` is configured and reachable.
- Use the GROBID title (instead of the heuristic) as the DBLP/Crossref query, improving hit rate.
- Enrich `needs_review` records with GROBID's title/abstract/authors even when no DB match is found (likely via a new `Resolution::Provisional(ResolvedMetadata)` variant handled in `build_paper`).
