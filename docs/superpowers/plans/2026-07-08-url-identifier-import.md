# URL / Identifier Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Let a user add a paper by pasting a URL, DOI, or arXiv id — from the web UI or the CLI — fetching the PDF (arXiv direct, ACM/IEEE via a stored UChicago EZproxy cookie, or an open-access copy via Unpaywall) and running it through the existing ingest pipeline.

**Architecture:** A new `src/import.rs` turns an input string into PDF bytes: `parse_source` classifies it, a publisher registry maps it to a PDF URL, and a `Fetcher` downloads it (plain for arXiv/OA; manual-redirect + `Cookie` header for the EZproxy path). The bytes are staged and handed to the *unchanged* `ingest_file` pipeline. The EZproxy cookie lives in a new `settings` DB table, set via the web UI or CLI. arXiv and open-access need no cookie.

**Tech Stack:** Rust (axum 0.8, sqlx 0.8/SQLite, reqwest 0.12, anyhow, clap 4), Svelte 5 + TypeScript + Vitest. Tests: `wiremock`, `axum-test`, `@testing-library/svelte`.

**Fetch order (per the spec):** arXiv direct → publisher-via-proxy (cookie) → Unpaywall OA → clean failure carrying metadata (no PDF-less record).

**Shared types (defined in Task 3 / Task 5 / Task 6, referenced throughout):**

```rust
// src/import.rs
pub enum Source {
    Arxiv(String),          // normalized id, e.g. "1706.03762" or "1706.03762v5"
    Doi(String),            // normalized DOI, e.g. "10.1145/3292500.3330701"
    IeeeDocument(String),   // arnumber from an ieeexplore.ieee.org/document/<n> URL
}

pub struct PdfTarget { pub url: String, pub requires_proxy: bool }

pub struct Fetched { pub bytes: Vec<u8>, pub hint: Option<crate::models::Identifier> }

pub enum ImportError {
    Unsupported,                                              // could not classify input
    CookieExpired,                                            // proxy returned a non-PDF
    Unfetched { metadata: Option<crate::resolve::ResolvedMetadata> }, // no PDF anywhere
    Network(anyhow::Error),
}
```

---

## Task 1: `settings` table + DB accessors

**Files:**
- Create: `migrations/0004_add_settings.sql`
- Modify: `src/db.rs` (add three functions after `stats`, tests in the `tests` module)

- [ ] **Step 1: Write the migration**

Create `migrations/0004_add_settings.sql`:

```sql
CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

- [ ] **Step 2: Write the failing test**

Add to the `tests` module in `src/db.rs`:

```rust
    #[tokio::test]
    async fn settings_set_get_delete_roundtrip() {
        let (_dir, pool) = temp_pool().await;
        assert_eq!(get_setting(&pool, "proxy_cookie").await.unwrap(), None);

        set_setting(&pool, "proxy_cookie", "ezproxy=abc").await.unwrap();
        assert_eq!(
            get_setting(&pool, "proxy_cookie").await.unwrap().as_deref(),
            Some("ezproxy=abc")
        );

        // Upsert overwrites the value.
        set_setting(&pool, "proxy_cookie", "ezproxy=xyz").await.unwrap();
        assert_eq!(
            get_setting(&pool, "proxy_cookie").await.unwrap().as_deref(),
            Some("ezproxy=xyz")
        );

        // updated_at is populated.
        assert!(setting_updated_at(&pool, "proxy_cookie").await.unwrap().is_some());

        delete_setting(&pool, "proxy_cookie").await.unwrap();
        assert_eq!(get_setting(&pool, "proxy_cookie").await.unwrap(), None);
        assert_eq!(setting_updated_at(&pool, "proxy_cookie").await.unwrap(), None);
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib db::tests::settings_set_get_delete_roundtrip`
Expected: FAIL — `get_setting`/`set_setting`/`delete_setting`/`setting_updated_at` not found.

- [ ] **Step 4: Implement the accessors**

Add to `src/db.rs` (after the `stats` function, before `#[cfg(test)]`):

```rust
/// Read a single setting value by key.
pub async fn get_setting(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(v,)| v))
}

/// The RFC3339 timestamp a setting was last written, if it exists.
pub async fn setting_updated_at(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let row: Option<(String,)> = sqlx::query_as("SELECT updated_at FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(row.map(|(v,)| v))
}

/// Insert or overwrite a setting, stamping `updated_at` with the current time.
pub async fn set_setting(pool: &SqlitePool, key: &str, value: &str) -> Result<()> {
    let ts = chrono::Utc::now().to_rfc3339();
    sqlx::query(
        "INSERT INTO settings (key, value, updated_at) VALUES (?, ?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
    )
    .bind(key)
    .bind(value)
    .bind(ts)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove a setting (no-op if absent).
pub async fn delete_setting(pool: &SqlitePool, key: &str) -> Result<()> {
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib db::tests::settings_set_get_delete_roundtrip`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add migrations/0004_add_settings.sql src/db.rs
git commit -m "feat(db): settings key/value table with get/set/delete"
```

---

## Task 2: `[proxy]` config section

**Files:**
- Modify: `src/config.rs` (add `ProxyConfig`, a `proxy` field, and a test)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `src/config.rs`:

```rust
    #[test]
    fn loads_proxy_section() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"

[proxy]
login_url = "https://proxy.uchicago.edu/login?url="
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(
            cfg.proxy.unwrap().login_url,
            "https://proxy.uchicago.edu/login?url="
        );
    }

    #[test]
    fn proxy_defaults_to_none() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"
"#
        )
        .unwrap();
        assert!(Config::load(f.path()).unwrap().proxy.is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib config::tests::loads_proxy_section`
Expected: FAIL — `Config` has no field `proxy`.

- [ ] **Step 3: Add the config types**

In `src/config.rs`, add the struct and field:

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct ProxyConfig {
    /// EZproxy login prefix; a target URL is percent-encoded and appended.
    /// e.g. "https://proxy.uchicago.edu/login?url="
    pub login_url: String,
}
```

Add to `struct Config` (after `contact_email`):

```rust
    #[serde(default)]
    pub proxy: Option<ProxyConfig>,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --lib config::tests`
Expected: PASS (both new tests + existing config tests)

- [ ] **Step 5: Commit**

```bash
git add src/config.rs
git commit -m "feat(config): optional [proxy] login_url section"
```

---

## Task 3: `parse_source` + publisher registry (pure logic)

**Files:**
- Create: `src/import.rs`
- Modify: `src/lib.rs` (add `pub mod import;`)

- [ ] **Step 1: Register the module**

In `src/lib.rs`, add in alphabetical position (after `pub mod identify;`):

```rust
pub mod import;
```

- [ ] **Step 2: Write the failing tests**

Create `src/import.rs`:

```rust
use crate::models::Identifier;

/// A classified import input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Arxiv(String),
    Doi(String),
    IeeeDocument(String),
}

/// A resolved PDF location and whether it must be fetched through the proxy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PdfTarget {
    pub url: String,
    pub requires_proxy: bool,
}

#[cfg(test)]
mod parse_tests {
    use super::*;

    #[test]
    fn parses_arxiv_forms() {
        for s in [
            "1706.03762",
            "arXiv:1706.03762",
            "arxiv:1706.03762v5",
            "https://arxiv.org/abs/1706.03762",
            "https://arxiv.org/pdf/1706.03762v5",
        ] {
            assert!(matches!(parse_source(s), Some(Source::Arxiv(_))), "{s}");
        }
        assert_eq!(parse_source("1706.03762v5"), Some(Source::Arxiv("1706.03762v5".into())));
    }

    #[test]
    fn parses_doi_forms() {
        assert_eq!(
            parse_source("10.1145/3292500.3330701"),
            Some(Source::Doi("10.1145/3292500.3330701".into()))
        );
        assert_eq!(
            parse_source("https://doi.org/10.1145/3292500.3330701"),
            Some(Source::Doi("10.1145/3292500.3330701".into()))
        );
        // ACM landing URL carries the DOI in its path.
        assert_eq!(
            parse_source("https://dl.acm.org/doi/10.1145/3292500.3330701"),
            Some(Source::Doi("10.1145/3292500.3330701".into()))
        );
    }

    #[test]
    fn parses_ieee_document_url() {
        assert_eq!(
            parse_source("https://ieeexplore.ieee.org/document/8835311"),
            Some(Source::IeeeDocument("8835311".into()))
        );
    }

    #[test]
    fn rejects_junk() {
        assert_eq!(parse_source(""), None);
        assert_eq!(parse_source("just a title of a paper"), None);
        assert_eq!(parse_source("https://example.com/thing"), None);
    }

    #[test]
    fn pdf_target_arxiv_is_open() {
        let t = pdf_target(&Source::Arxiv("1706.03762".into())).unwrap();
        assert_eq!(t.url, "https://arxiv.org/pdf/1706.03762");
        assert!(!t.requires_proxy);
    }

    #[test]
    fn pdf_target_acm_needs_proxy() {
        let t = pdf_target(&Source::Doi("10.1145/3292500.3330701".into())).unwrap();
        assert_eq!(t.url, "https://dl.acm.org/doi/pdf/10.1145/3292500.3330701");
        assert!(t.requires_proxy);
    }

    #[test]
    fn pdf_target_ieee_document_needs_proxy() {
        let t = pdf_target(&Source::IeeeDocument("8835311".into())).unwrap();
        assert_eq!(t.url, "https://ieeexplore.ieee.org/stamp/stamp.jsp?arnumber=8835311");
        assert!(t.requires_proxy);
    }

    #[test]
    fn pdf_target_unknown_doi_is_none() {
        // A non-ACM/IEEE DOI has no constructible publisher PDF URL.
        assert!(pdf_target(&Source::Doi("10.1109/5.771073".into())).is_none()); // IEEE DOI: no arnumber
        assert!(pdf_target(&Source::Doi("10.1000/xyz".into())).is_none());
    }

    #[test]
    fn source_identifier_maps_doi_and_arxiv() {
        assert_eq!(
            source_identifier(&Source::Doi("10.1/x".into())),
            Some(Identifier::Doi("10.1/x".into()))
        );
        assert_eq!(
            source_identifier(&Source::Arxiv("1706.03762".into())),
            Some(Identifier::Arxiv("1706.03762".into()))
        );
        assert_eq!(source_identifier(&Source::IeeeDocument("8835311".into())), None);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib import::parse_tests`
Expected: FAIL — `parse_source`, `pdf_target`, `source_identifier` not defined.

- [ ] **Step 4: Implement the parser + registry**

Add to `src/import.rs` (above the test module):

```rust
use regex::Regex;
use std::sync::LazyLock;

static ARXIV_RE: LazyLock<Regex> = LazyLock::new(|| {
    // arXiv id inside "arXiv:", an arxiv.org URL, or bare.
    Regex::new(r"(?i)(?:arxiv:\s*|arxiv\.org/(?:abs|pdf)/)?(\d{4}\.\d{4,5}(?:v\d+)?)").unwrap()
});
static DOI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"10\.\d{4,9}/[-._;()/:A-Za-z0-9]+").unwrap());
static IEEE_DOC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)ieeexplore\.ieee\.org/document/(\d+)").unwrap());

/// Classify a pasted input into a `Source`. Order matters: an IEEE document URL
/// and a DOI-bearing URL are checked before the (broad) bare-arXiv pattern.
pub fn parse_source(input: &str) -> Option<Source> {
    let t = input.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(c) = IEEE_DOC_RE.captures(t) {
        return Some(Source::IeeeDocument(c[1].to_string()));
    }
    // A DOI anywhere (including doi.org / dl.acm.org URLs), trimmed of trailing prose.
    if let Some(m) = DOI_RE.find(t) {
        let doi = m.as_str().trim_end_matches(['.', ',', ')', ';']).to_string();
        return Some(Source::Doi(doi));
    }
    // arXiv id: require the arxiv.org host / "arXiv:" prefix, OR a bare id that is
    // the entire (trimmed) input, so a random "1234.5678" inside prose is ignored.
    if t.to_lowercase().contains("arxiv") {
        if let Some(c) = ARXIV_RE.captures(t) {
            return Some(Source::Arxiv(c[1].to_string()));
        }
    }
    if let Some(c) = ARXIV_RE.captures(t) {
        if c.get(0).is_some_and(|m| m.as_str() == t) {
            return Some(Source::Arxiv(c[1].to_string()));
        }
    }
    None
}

/// Map a source to its PDF URL, or `None` when no publisher PDF URL is
/// constructible (unknown publisher, or an IEEE DOI without an arnumber).
pub fn pdf_target(src: &Source) -> Option<PdfTarget> {
    match src {
        Source::Arxiv(id) => Some(PdfTarget {
            url: format!("https://arxiv.org/pdf/{id}"),
            requires_proxy: false,
        }),
        Source::Doi(doi) if doi.starts_with("10.1145/") => Some(PdfTarget {
            url: format!("https://dl.acm.org/doi/pdf/{doi}"),
            requires_proxy: true,
        }),
        Source::IeeeDocument(arnumber) => Some(PdfTarget {
            url: format!("https://ieeexplore.ieee.org/stamp/stamp.jsp?arnumber={arnumber}"),
            requires_proxy: true,
        }),
        Source::Doi(_) => None,
    }
}

/// The identifier a source implies, used to seed metadata resolution during
/// ingest. An IEEE arnumber is not a DOI, so it yields no hint.
pub fn source_identifier(src: &Source) -> Option<Identifier> {
    match src {
        Source::Doi(d) => Some(Identifier::Doi(d.clone())),
        Source::Arxiv(a) => Some(Identifier::Arxiv(a.clone())),
        Source::IeeeDocument(_) => None,
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib import::parse_tests`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/lib.rs src/import.rs
git commit -m "feat(import): parse_source + publisher PDF registry"
```

---

## Task 4: Unpaywall open-access lookup

**Files:**
- Create: `src/resolve/unpaywall.rs`
- Create: `tests/fixtures/unpaywall_oa.json`, `tests/fixtures/unpaywall_closed.json`
- Modify: `src/resolve/mod.rs` (declare module, add `email`/`unpaywall_base`, `oa_pdf_url`, `with_unpaywall_base`)

- [ ] **Step 1: Add fixtures**

Create `tests/fixtures/unpaywall_oa.json`:

```json
{
  "doi": "10.1145/3292500.3330701",
  "is_oa": true,
  "best_oa_location": {
    "url_for_pdf": "https://example.org/paper.pdf",
    "url": "https://example.org/paper"
  }
}
```

Create `tests/fixtures/unpaywall_closed.json`:

```json
{ "doi": "10.1145/closed", "is_oa": false, "best_oa_location": null }
```

- [ ] **Step 2: Write the failing parse test**

Create `src/resolve/unpaywall.rs`:

```rust
use anyhow::Result;

use super::http::HttpClient;

/// Query Unpaywall for a DOI and return the best OA PDF URL, if any.
pub async fn fetch(http: &HttpClient, base: &str, doi: &str, email: &str) -> Result<Option<String>> {
    let url = format!("{base}/v2/{doi}?email={email}");
    let body = http.get_text(&url).await?;
    parse(&body)
}

/// Extract `best_oa_location.url_for_pdf` from an Unpaywall response.
pub fn parse(body: &str) -> Result<Option<String>> {
    let v: serde_json::Value = serde_json::from_str(body)?;
    Ok(v.get("best_oa_location")
        .and_then(|loc| loc.get("url_for_pdf"))
        .and_then(|u| u.as_str())
        .map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OA: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/unpaywall_oa.json"
    ));
    const CLOSED: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/unpaywall_closed.json"
    ));

    #[test]
    fn parses_oa_pdf_url() {
        assert_eq!(parse(OA).unwrap().as_deref(), Some("https://example.org/paper.pdf"));
    }

    #[test]
    fn closed_access_is_none() {
        assert_eq!(parse(CLOSED).unwrap(), None);
    }
}
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib resolve::unpaywall`
Expected: FAIL — module `unpaywall` not declared in `resolve`.

- [ ] **Step 4: Wire the module + Resolver method**

In `src/resolve/mod.rs`, add the module declaration at the top with the others:

```rust
pub mod unpaywall;
```

Add two fields to `struct Resolver` (after `dblp_base`):

```rust
    email: Option<String>,
    unpaywall_base: String,
```

In `Resolver::build`, set them when constructing `Self` (the `contact_email` is already the `email` param there):

```rust
        Ok(Self {
            http: HttpClient::new(client, retry),
            arxiv_base,
            crossref_base,
            dblp_base: "https://dblp.org".to_string(),
            email: contact_email.map(str::to_string),
            unpaywall_base: "https://api.unpaywall.org".to_string(),
        })
```

Add a test override next to `with_dblp_base`:

```rust
    /// Override the Unpaywall base URL (used by tests to point at a mock server).
    pub fn with_unpaywall_base(mut self, base: String) -> Self {
        self.unpaywall_base = base;
        self
    }
```

Add the OA lookup method inside the first `impl Resolver` block (after `try_crossref_search` helpers, before the closing brace):

```rust
    /// The best open-access PDF URL for a DOI via Unpaywall, or `None` when
    /// there is no OA copy, no configured contact email, or the lookup fails.
    pub async fn oa_pdf_url(&self, doi: &str) -> Option<String> {
        let email = self.email.as_deref()?;
        match unpaywall::fetch(&self.http, &self.unpaywall_base, doi, email).await {
            Ok(u) => u,
            Err(e) => {
                tracing::warn!("unpaywall lookup failed for {doi}: {e}");
                None
            }
        }
    }
```

- [ ] **Step 5: Write the wiremock integration test**

Add to the `tests` module in `src/resolve/mod.rs`:

```rust
    #[tokio::test]
    async fn oa_pdf_url_hits_unpaywall() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        let body = include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/unpaywall_oa.json"
        ));
        Mock::given(method("GET"))
            .and(path("/v2/10.1145/3292500.3330701"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        let r = Resolver::with_bases(Some("me@uchicago.edu"), server.uri(), server.uri())
            .unwrap()
            .with_unpaywall_base(server.uri());
        assert_eq!(
            r.oa_pdf_url("10.1145/3292500.3330701").await.as_deref(),
            Some("https://example.org/paper.pdf")
        );
        // No email configured → skipped entirely.
        let r2 = Resolver::with_bases(None, server.uri(), server.uri())
            .unwrap()
            .with_unpaywall_base(server.uri());
        assert_eq!(r2.oa_pdf_url("10.1145/3292500.3330701").await, None);
    }
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test --lib resolve::unpaywall && cargo test --lib resolve::tests::oa_pdf_url_hits_unpaywall`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add src/resolve/unpaywall.rs src/resolve/mod.rs tests/fixtures/unpaywall_oa.json tests/fixtures/unpaywall_closed.json
git commit -m "feat(resolve): Unpaywall OA PDF lookup"
```

---

## Task 5: `Fetcher` — PDF byte downloads (plain + proxied)

**Files:**
- Modify: `src/import.rs` (add `Fetcher`, `encode_target`, `%PDF` verification; tests)

- [ ] **Step 1: Write the failing tests**

Add a new test module to `src/import.rs`:

```rust
#[cfg(test)]
mod fetch_tests {
    use super::*;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const PDF: &[u8] = b"%PDF-1.4\nfake body\n";

    #[test]
    fn encode_target_percent_encodes_reserved() {
        assert_eq!(
            encode_target("https://dl.acm.org/doi/pdf/10.1145/3292500.3330701"),
            "https%3A%2F%2Fdl.acm.org%2Fdoi%2Fpdf%2F10.1145%2F3292500.3330701"
        );
    }

    #[tokio::test]
    async fn fetch_plain_returns_pdf_and_rejects_html() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/ok.pdf"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(PDF))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/nope"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>login</html>"))
            .mount(&server)
            .await;
        let f = Fetcher::new(None).unwrap();
        assert_eq!(
            f.fetch_plain(&format!("{}/ok.pdf", server.uri())).await.unwrap().as_deref(),
            Some(PDF)
        );
        assert_eq!(f.fetch_plain(&format!("{}/nope", server.uri())).await.unwrap(), None);
    }

    #[tokio::test]
    async fn fetch_proxied_requires_cookie_and_verifies_pdf() {
        let server = MockServer::start().await;
        // With the right cookie → the mock returns the PDF.
        Mock::given(method("GET"))
            .and(header("cookie", "ezproxy=good"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(PDF))
            .mount(&server)
            .await;
        // Any other request (no/incorrect cookie) → an HTML login page.
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>Shibboleth</html>"))
            .mount(&server)
            .await;

        // login_url points at the mock; the target is appended, percent-encoded.
        let login = format!("{}/login?url=", server.uri());
        let f = Fetcher::new(Some(login)).unwrap();

        let target = "https://dl.acm.org/doi/pdf/10.1145/x";
        assert_eq!(
            f.fetch_proxied(target, "ezproxy=good").await.unwrap().as_deref(),
            Some(PDF)
        );
        // Wrong cookie → non-PDF body → None (caller maps to CookieExpired).
        assert_eq!(f.fetch_proxied(target, "ezproxy=stale").await.unwrap(), None);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib import::fetch_tests`
Expected: FAIL — `Fetcher`, `encode_target` not defined.

- [ ] **Step 3: Implement `Fetcher` + helpers**

Add to `src/import.rs` (above the `fetch_tests` module):

```rust
use anyhow::{anyhow, Result};
use std::time::Duration;

/// Downloads PDF bytes: a redirect-following client for open URLs, and a
/// manual-redirect path that re-attaches the `Cookie` header through the
/// EZproxy chain (reqwest strips `Cookie` on cross-host redirects otherwise).
pub struct Fetcher {
    client: reqwest::Client,
    proxy_login_url: Option<String>,
    proxy_host: Option<String>,
}

/// Percent-encode a URL for use as the `?url=` value of the EZproxy login.
/// Encodes everything except the RFC3986 unreserved set.
pub fn encode_target(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

impl Fetcher {
    pub fn new(proxy_login_url: Option<String>) -> Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent("xuewen/0.1")
            .timeout(Duration::from_secs(30))
            .build()?;
        let proxy_host = proxy_login_url
            .as_deref()
            .and_then(|u| reqwest::Url::parse(u).ok())
            .and_then(|u| u.host_str().map(str::to_string));
        Ok(Self { client, proxy_login_url, proxy_host })
    }

    /// Whether `proxy_login_url` is configured (paywalled fetch is possible).
    pub fn proxy_enabled(&self) -> bool {
        self.proxy_login_url.is_some()
    }

    /// GET `url` following redirects. `Ok(Some(bytes))` if the body is a PDF,
    /// `Ok(None)` if the fetch succeeded but the body is not a PDF, `Err` on a
    /// network/HTTP error.
    pub async fn fetch_plain(&self, url: &str) -> Result<Option<Vec<u8>>> {
        let resp = self.client.get(url).send().await?;
        if !resp.status().is_success() {
            return Err(anyhow!("HTTP {} fetching {url}", resp.status()));
        }
        let bytes = resp.bytes().await?.to_vec();
        Ok(is_pdf(&bytes).then_some(bytes))
    }

    /// GET the proxied `target_url` carrying `cookie`, following redirects
    /// manually so the cookie rides along, but only to the proxy host or its
    /// subdomains. `Ok(Some(bytes))` iff a PDF is returned; `Ok(None)` when the
    /// body is non-PDF (typically an expired-session login page).
    pub async fn fetch_proxied(&self, target_url: &str, cookie: &str) -> Result<Option<Vec<u8>>> {
        let login = self
            .proxy_login_url
            .as_deref()
            .ok_or_else(|| anyhow!("proxy not configured"))?;
        let mut url = format!("{login}{}", encode_target(target_url));
        for _ in 0..10 {
            let resp = self
                .client
                .get(&url)
                .header(reqwest::header::COOKIE, cookie)
                // Do not auto-follow: we re-attach the cookie and guard the host.
                .send()
                .await?;
            let status = resp.status();
            if status.is_redirection() {
                let loc = resp
                    .headers()
                    .get(reqwest::header::LOCATION)
                    .and_then(|v| v.to_str().ok())
                    .ok_or_else(|| anyhow!("redirect without Location"))?;
                let next = reqwest::Url::parse(&url)?.join(loc)?;
                if !self.host_allowed(next.host_str()) {
                    // Redirected off the proxy domain (e.g. to an IdP): treat as
                    // an expired session rather than following and leaking the cookie.
                    return Ok(None);
                }
                url = next.to_string();
                continue;
            }
            if !status.is_success() {
                return Err(anyhow!("HTTP {status} via proxy for {target_url}"));
            }
            let bytes = resp.bytes().await?.to_vec();
            return Ok(is_pdf(&bytes).then_some(bytes));
        }
        Err(anyhow!("too many redirects via proxy for {target_url}"))
    }

    /// A redirect target host is allowed iff it equals the proxy host or is a
    /// subdomain of it (`dl-acm-org.proxy.uchicago.edu` for `proxy.uchicago.edu`).
    fn host_allowed(&self, host: Option<&str>) -> bool {
        match (host, self.proxy_host.as_deref()) {
            (Some(h), Some(p)) => h == p || h.ends_with(&format!(".{p}")),
            _ => false,
        }
    }
}

/// Whether bytes begin with the PDF magic marker.
fn is_pdf(bytes: &[u8]) -> bool {
    bytes.starts_with(b"%PDF")
}
```

> Note: the manual-redirect client uses reqwest's default redirect policy but we
> never call a helper that auto-follows for the proxied path — each hop is an
> explicit `send()`. reqwest returns the 3xx response (with `Location`) because we
> read the status before any body, and the default policy only follows on the
> *next* implicit send, which we don't issue. If a future reqwest change
> auto-follows here, add `.redirect(reqwest::redirect::Policy::none())` to a
> dedicated client; the test `fetch_proxied_requires_cookie_and_verifies_pdf`
> guards the behavior.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib import::fetch_tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/import.rs
git commit -m "feat(import): Fetcher for plain + EZproxy PDF downloads"
```

---

## Task 6: Orchestration — `import_source`

**Files:**
- Modify: `src/import.rs` (add `Fetched`, `ImportError`, `import_source`; tests)

- [ ] **Step 1: Write the failing tests**

Add a new test module to `src/import.rs`:

```rust
#[cfg(test)]
mod orchestration_tests {
    use super::*;
    use crate::resolve::Resolver;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const PDF: &[u8] = b"%PDF-1.4\nx\n";

    fn offline_resolver() -> Resolver {
        Resolver::with_bases(None, "http://127.0.0.1:1".into(), "http://127.0.0.1:1".into())
            .unwrap()
            .with_dblp_base("http://127.0.0.1:1".into())
            .with_unpaywall_base("http://127.0.0.1:1".into())
    }

    #[tokio::test]
    async fn doi_oa_copy_is_fetched() {
        // The arXiv host is hard-coded (arxiv.org) and can't be repointed at a
        // mock, so we exercise the plain-download path via the OA fallback: a DOI
        // with no cookie whose Unpaywall record points at a PDF the mock serves.
        // (fetch_plain itself is unit-tested against a mock in fetch_tests.)
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v2/10.1145/oa"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                format!(r#"{{"best_oa_location":{{"url_for_pdf":"{}/paper.pdf"}}}}"#, server.uri()),
            ))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/paper.pdf"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(PDF))
            .mount(&server)
            .await;
        let resolver = Resolver::with_bases(Some("me@uchicago.edu"), server.uri(), server.uri())
            .unwrap()
            .with_dblp_base(server.uri())
            .with_unpaywall_base(server.uri());
        let fetcher = Fetcher::new(None).unwrap();

        let out = import_source(&fetcher, &resolver, "10.1145/oa", None).await.unwrap();
        assert_eq!(out.bytes, PDF);
        assert_eq!(out.hint, Some(crate::models::Identifier::Doi("10.1145/oa".into())));
    }

    #[tokio::test]
    async fn unsupported_input_errors() {
        let fetcher = Fetcher::new(None).unwrap();
        let err = import_source(&fetcher, &offline_resolver(), "not an id", None).await.unwrap_err();
        assert!(matches!(err, ImportError::Unsupported));
    }

    #[tokio::test]
    async fn paywalled_no_cookie_no_oa_is_unfetched() {
        let fetcher = Fetcher::new(None).unwrap(); // proxy disabled
        // ACM DOI, no cookie, offline resolver → no OA, no metadata → Unfetched.
        let err = import_source(&fetcher, &offline_resolver(), "10.1145/paywalled", None)
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::Unfetched { .. }));
    }

    #[tokio::test]
    async fn expired_cookie_reported() {
        // Proxy returns HTML for any cookie → CookieExpired (no OA fallback available).
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>login</html>"))
            .mount(&server)
            .await;
        let fetcher = Fetcher::new(Some(format!("{}/login?url=", server.uri()))).unwrap();
        let err = import_source(&fetcher, &offline_resolver(), "10.1145/x", Some("ezproxy=stale"))
            .await
            .unwrap_err();
        assert!(matches!(err, ImportError::CookieExpired));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib import::orchestration_tests`
Expected: FAIL — `Fetched`, `ImportError`, `import_source` not defined.

- [ ] **Step 3: Implement the orchestration**

Add to `src/import.rs` (above `orchestration_tests`):

```rust
use crate::resolve::{ResolvedMetadata, Resolver};

/// PDF bytes plus the identifier to seed ingest metadata resolution.
pub struct Fetched {
    pub bytes: Vec<u8>,
    pub hint: Option<Identifier>,
}

/// Why a URL/identifier import could not produce a PDF.
#[derive(Debug)]
pub enum ImportError {
    Unsupported,
    CookieExpired,
    Unfetched { metadata: Option<ResolvedMetadata> },
    Network(anyhow::Error),
}

/// Turn an input string into PDF bytes, following the spec's fetch order:
/// arXiv direct → publisher-via-proxy (cookie) → Unpaywall OA → clean failure.
pub async fn import_source(
    fetcher: &Fetcher,
    resolver: &Resolver,
    input: &str,
    cookie: Option<&str>,
) -> Result<Fetched, ImportError> {
    let src = parse_source(input).ok_or(ImportError::Unsupported)?;
    let hint = source_identifier(&src);

    // 1. arXiv is always taken direct (open, no proxy).
    if let Source::Arxiv(_) = &src {
        let target = pdf_target(&src).expect("arxiv always has a target");
        return match fetcher.fetch_plain(&target.url).await {
            Ok(Some(bytes)) => Ok(Fetched { bytes, hint }),
            Ok(None) => Err(ImportError::Unfetched { metadata: metadata_for(resolver, &src).await }),
            Err(e) => Err(ImportError::Network(e)),
        };
    }

    // 2. Known paywalled publisher + a cookie → proxied fetch (preferred).
    let mut cookie_expired = false;
    if let (Some(target), Some(cookie)) = (pdf_target(&src), cookie) {
        if target.requires_proxy && fetcher.proxy_enabled() {
            match fetcher.fetch_proxied(&target.url, cookie).await {
                Ok(Some(bytes)) => return Ok(Fetched { bytes, hint }),
                Ok(None) => cookie_expired = true, // non-PDF: likely expired session
                Err(e) => tracing::warn!("proxied fetch failed: {e}"),
            }
        }
    }

    // 3. Open-access fallback via Unpaywall (needs a DOI).
    if let Source::Doi(doi) = &src {
        if let Some(oa) = resolver.oa_pdf_url(doi).await {
            if let Ok(Some(bytes)) = fetcher.fetch_plain(&oa).await {
                return Ok(Fetched { bytes, hint });
            }
        }
    }

    // 4. Give up. Prefer the actionable "cookie expired" over generic "unfetched".
    if cookie_expired {
        Err(ImportError::CookieExpired)
    } else {
        Err(ImportError::Unfetched { metadata: metadata_for(resolver, &src).await })
    }
}

/// Best-effort metadata for the clean-failure message.
async fn metadata_for(resolver: &Resolver, src: &Source) -> Option<ResolvedMetadata> {
    match source_identifier(src) {
        Some(ident) => resolver.resolve(&ident, None).await,
        None => None,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib import::orchestration_tests`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/import.rs
git commit -m "feat(import): import_source orchestration (arxiv/proxy/OA/fallback)"
```

---

## Task 7: Ingest identifier hint

**Files:**
- Modify: `src/pipeline.rs` (add `ingest_file_with_hint`, thread hint into `resolve_pdf`)
- Modify: `src/refresh.rs:78` (pass `None` to `resolve_pdf`)

- [ ] **Step 1: Write the failing test**

Add to `tests/pipeline_test.rs` (an integration test, so it can use the file's
existing `common::write_test_pdf` helper). First extend its models import to
include `Identifier`:

```rust
use xuewen::models::{Authors, Identifier, PaperMeta, PaperStatus};
```

Then add this test (mirrors the refusing-resolver + `IngestCtx` pattern already
used throughout the file):

```rust
#[tokio::test]
async fn ingest_with_hint_seeds_identifier() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Offline resolver: upstreams refuse instantly (degrades to needs_review).
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());
    let ctx = IngestCtx {
        pool: pool.clone(),
        dirs: Libraries {
            library_root: library.clone(),
            processed_dir: inbox.join("_processed"),
        },
        resolver,
        grobid: None,
    };

    // A one-page PDF whose text carries NO identifier.
    let pdf_path = inbox.join("in.pdf");
    common::write_test_pdf(&pdf_path, &["A Paper Without Any Identifier In Text"]);

    // Import with a DOI hint: the hint seeds the stored identifier even though
    // the PDF text has none.
    let id = match ctx
        .ingest_file_with_hint(&pdf_path, Some(Identifier::Doi("10.1234/hinted".into())))
        .await
        .unwrap()
    {
        Outcome::Ingested(id) => id,
        other => panic!("expected Ingested, got {other:?}"),
    };
    let got = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(got.meta.doi.as_deref(), Some("10.1234/hinted"));
    assert_eq!(got.meta.status, PaperStatus::NeedsReview);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test pipeline_test ingest_with_hint_seeds_identifier`
Expected: FAIL — `ingest_file_with_hint` not defined.

- [ ] **Step 3: Add the hint variant + thread it through**

In `src/pipeline.rs`, rename the body of `ingest_file` into a hint-taking variant and keep `ingest_file` as a wrapper. Replace the current signature:

```rust
    pub async fn ingest_file(&self, path: &Path) -> Result<Outcome> {
```

with:

```rust
    /// Ingest a single PDF with no identifier hint (text extraction decides).
    pub async fn ingest_file(&self, path: &Path) -> Result<Outcome> {
        self.ingest_file_with_hint(path, None).await
    }

    /// Ingest a single PDF, optionally seeding metadata resolution with a known
    /// identifier (used by URL/DOI import, where we already know the id and the
    /// PDF's first page may not print it).
    pub async fn ingest_file_with_hint(
        &self,
        path: &Path,
        hint: Option<Identifier>,
    ) -> Result<Outcome> {
```

Then, inside that method body, change the `resolve_pdf` call at what was line 70:

```rust
        } = self.resolve_pdf(&path, hint).await?;
```

Change `resolve_pdf`'s signature and its identifier line:

```rust
    pub(crate) async fn resolve_pdf(
        &self,
        path: &Path,
        hint: Option<Identifier>,
    ) -> Result<ResolveInputs> {
```

and replace `let ident = identify::identify(&text);` with:

```rust
        let ident = hint.unwrap_or_else(|| identify::identify(&text));
```

In `src/refresh.rs:78`, update the call site:

```rust
        match ctx.resolve_pdf(&pdf, None).await {
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test ingest_with_hint_seeds_identifier && cargo test --test pipeline_test && cargo test refresh`
Expected: PASS (existing ingest/refresh tests still green; `ingest_file` wrapper preserves behavior).

- [ ] **Step 5: Commit**

```bash
git add src/pipeline.rs src/refresh.rs tests/pipeline_test.rs
git commit -m "feat(pipeline): ingest_file_with_hint seeds resolution identifier"
```

---

## Task 8: Web endpoints — `POST /api/import` + settings

**Files:**
- Modify: `src/web/mod.rs` (AppState `proxy_login_url`, new `build_router_with_ingest_proxy`, `serve` param)
- Modify: `src/web/api.rs` (shared `stage_and_ingest`, `import_url`, `get_settings`, `set_proxy_cookie`, `clear_proxy_cookie`)
- Modify: `tests/web_test.rs` (integration tests)

- [ ] **Step 1: Add AppState field + proxy-aware router (no failing test yet — enables the rest)**

In `src/web/mod.rs`, add to `struct AppState` (after `ingest`):

```rust
    /// EZproxy login prefix (from `[proxy].login_url`); `None` disables proxy fetch.
    pub proxy_login_url: Option<String>,
```

Set `proxy_login_url: None` in both existing `build_router` and `build_router_with_ingest` bodies (their signatures do not change, so every existing caller keeps compiling). Add a new constructor and update `serve`:

```rust
/// Full router with import + a configured proxy prefix. Used by `serve`.
pub fn build_router_with_ingest_proxy(
    pool: SqlitePool,
    library_root: PathBuf,
    ingest: Arc<Ingest>,
    proxy_login_url: Option<String>,
) -> Router {
    router_with(AppState { pool, library_root, ingest: Some(ingest), proxy_login_url })
}
```

Change `serve` to accept and pass the proxy prefix:

```rust
pub async fn serve(
    host: &str,
    port: u16,
    pool: SqlitePool,
    library_root: PathBuf,
    ingest: Arc<Ingest>,
    proxy_login_url: Option<String>,
) -> Result<()> {
    let app = build_router_with_ingest_proxy(pool, library_root, ingest, proxy_login_url);
    // ... unchanged bind/serve body ...
```

Register routes in `router_with` (add to the existing builder chain):

```rust
        .route("/api/import", axum::routing::post(api::import_url))
        .route(
            "/api/settings",
            get(api::get_settings),
        )
        .route(
            "/api/settings/proxy-cookie",
            axum::routing::put(api::set_proxy_cookie).delete(api::clear_proxy_cookie),
        )
```

Update the `serve` call in `src/main.rs` (Task 9 covers the CLI, but this call must compile now):

```rust
            web::serve(
                &host,
                port,
                pool,
                cfg.library_root.clone(),
                ingest,
                cfg.proxy.as_ref().map(|p| p.login_url.clone()),
            )
            .await?;
```

- [ ] **Step 2: Write the failing integration tests**

Add to `tests/web_test.rs`. First, a helper mirroring the existing ingest setup but with a proxy prefix and a fetchable arXiv/OA source is heavy; instead assert the two most valuable behaviors: settings round-trip, and an unsupported input. Add:

```rust
use xuewen::web::build_router_with_ingest_proxy;

#[tokio::test]
async fn settings_report_and_set_proxy_cookie() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(None, "http://127.0.0.1:1".into(), "http://127.0.0.1:1".into())
        .unwrap()
        .with_dblp_base("http://127.0.0.1:1".into());
    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries { library_root: library.clone(), processed_dir: inbox.join("_processed") },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server = TestServer::new(build_router_with_ingest_proxy(
        pool.clone(),
        library.clone(),
        ingest,
        Some("https://proxy.uchicago.edu/login?url=".into()),
    ))
    .unwrap();

    // Initially unset.
    let s: serde_json::Value = server.get("/api/settings").await.json();
    assert_eq!(s["proxy_cookie_set"], false);

    // Set it.
    server
        .put("/api/settings/proxy-cookie")
        .json(&serde_json::json!({ "cookie": "ezproxy=abc" }))
        .await
        .assert_status_ok();
    let s: serde_json::Value = server.get("/api/settings").await.json();
    assert_eq!(s["proxy_cookie_set"], true);
    assert!(s["proxy_cookie_updated_at"].is_string());
    // The value is never echoed.
    assert!(s.get("cookie").is_none());

    // Clear it.
    server.delete("/api/settings/proxy-cookie").await.assert_status_ok();
    let s: serde_json::Value = server.get("/api/settings").await.json();
    assert_eq!(s["proxy_cookie_set"], false);
}

#[tokio::test]
async fn import_url_rejects_unsupported_input() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(None, "http://127.0.0.1:1".into(), "http://127.0.0.1:1".into())
        .unwrap()
        .with_dblp_base("http://127.0.0.1:1".into());
    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries { library_root: library.clone(), processed_dir: inbox.join("_processed") },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server = TestServer::new(build_router_with_ingest_proxy(pool, library, ingest, None)).unwrap();

    server
        .post("/api/import")
        .json(&serde_json::json!({ "input": "just a title, not an id" }))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn import_url_needs_ingest_context() {
    let (dir, pool) = temp_pool().await;
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();
    server
        .post("/api/import")
        .json(&serde_json::json!({ "input": "10.1145/x" }))
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test web_test settings_report_and_set_proxy_cookie import_url_rejects_unsupported_input import_url_needs_ingest_context`
Expected: FAIL — handlers not defined / routes missing.

- [ ] **Step 4: Implement the handlers**

In `src/web/api.rs`, add imports at the top as needed:

```rust
use crate::import::{self, ImportError};
use crate::models::Identifier;
```

Refactor the staging+ingest+outcome mapping out of `import_paper` into a shared helper (place near `import_paper`). Extract the block that starts at "Stage the bytes…" through the `match ingest.ctx.ingest_file(...)` into:

```rust
/// Stage `bytes` under a sanitized, collision-safe name in the staging dir, run
/// the ingest pipeline (optionally with an identifier hint), and map the outcome
/// to the shared `ImportResult` JSON. Shared by file upload and URL import.
async fn stage_and_ingest(
    ingest: &super::Ingest,
    bytes: &[u8],
    filename: &str,
    hint: Option<Identifier>,
) -> Response {
    let stem = std::path::Path::new(filename)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("import.pdf");
    let staged = ingest.staging_dir.join(format!("{}-{stem}", Uuid::now_v7()));
    if let Err(e) = tokio::fs::create_dir_all(&ingest.staging_dir).await {
        tracing::error!("import staging dir: {e}");
        return internal_error();
    }
    if let Err(e) = tokio::fs::write(&staged, bytes).await {
        tracing::error!("import stage write: {e}");
        return internal_error();
    }
    match ingest.ctx.ingest_file_with_hint(&staged, hint).await {
        Ok(Outcome::Ingested(id)) => {
            let (title, status) = match db::get_by_id(&ingest.ctx.pool, &id).await {
                Ok(Some(p)) => (serde_json::json!(p.meta.title), p.meta.status),
                _ => (serde_json::Value::Null, crate::models::PaperStatus::NeedsReview),
            };
            Json(serde_json::json!({
                "outcome": "ingested", "id": id, "title": title, "status": status,
            }))
            .into_response()
        }
        Ok(Outcome::Duplicate) => Json(serde_json::json!({"outcome": "duplicate"})).into_response(),
        Ok(Outcome::SameWork(id)) => {
            Json(serde_json::json!({"outcome": "same_work", "id": id})).into_response()
        }
        Ok(Outcome::InTrash(id)) => {
            Json(serde_json::json!({"outcome": "in_trash", "id": id})).into_response()
        }
        Err(e) => {
            tracing::error!("import ingest: {e}");
            let _ = tokio::fs::remove_file(&staged).await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "import failed"})),
            )
                .into_response()
        }
    }
}
```

Then replace the tail of `import_paper` (from "Stage the bytes…" onward) with:

```rust
        return stage_and_ingest(&ingest, data.as_ref(), &filename, None).await;
```

Add the URL-import handler:

```rust
#[derive(Deserialize)]
pub struct ImportUrlBody {
    pub input: String,
}

/// Import from a URL/DOI/arXiv id: fetch the PDF (arXiv/proxy/OA), then ingest.
pub async fn import_url(State(app): State<AppState>, Json(body): Json<ImportUrlBody>) -> Response {
    let Some(ingest) = app.ingest.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "import not configured"})),
        )
            .into_response();
    };
    let fetcher = match import::Fetcher::new(app.proxy_login_url.clone()) {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("build fetcher: {e}");
            return internal_error();
        }
    };
    let cookie = db::get_setting(&ingest.ctx.pool, "proxy_cookie").await.ok().flatten();
    match import::import_source(&fetcher, &ingest.ctx.resolver, &body.input, cookie.as_deref()).await
    {
        Ok(fetched) => {
            stage_and_ingest(&ingest, &fetched.bytes, "import.pdf", fetched.hint).await
        }
        Err(ImportError::Unsupported) => bad_request("unsupported input"),
        Err(ImportError::CookieExpired) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": "proxy session expired — refresh your cookie"})),
        )
            .into_response(),
        Err(ImportError::Unfetched { metadata }) => {
            let (title, doi) = match metadata {
                Some(m) => (serde_json::json!(m.title), serde_json::json!(m.doi)),
                None => (serde_json::Value::Null, serde_json::Value::Null),
            };
            Json(serde_json::json!({"outcome": "unfetched", "title": title, "doi": doi}))
                .into_response()
        }
        Err(ImportError::Network(e)) => {
            tracing::error!("import fetch: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "fetch failed"})),
            )
                .into_response()
        }
    }
}
```

Add the settings handlers:

```rust
#[derive(Deserialize)]
pub struct ProxyCookieBody {
    pub cookie: String,
}

/// Report whether a proxy cookie is stored (never the value itself).
pub async fn get_settings(State(app): State<AppState>) -> Response {
    let set = db::get_setting(&app.pool, "proxy_cookie").await.ok().flatten().is_some();
    let updated = db::setting_updated_at(&app.pool, "proxy_cookie").await.ok().flatten();
    Json(serde_json::json!({
        "proxy_cookie_set": set,
        "proxy_cookie_updated_at": updated,
    }))
    .into_response()
}

/// Store (overwrite) the EZproxy cookie.
pub async fn set_proxy_cookie(State(app): State<AppState>, Json(body): Json<ProxyCookieBody>) -> Response {
    let cookie = body.cookie.trim();
    if cookie.is_empty() {
        return bad_request("empty cookie");
    }
    match db::set_setting(&app.pool, "proxy_cookie", cookie).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => {
            tracing::error!("set proxy cookie: {e}");
            internal_error()
        }
    }
}

/// Clear the stored EZproxy cookie.
pub async fn clear_proxy_cookie(State(app): State<AppState>) -> Response {
    match db::delete_setting(&app.pool, "proxy_cookie").await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => {
            tracing::error!("clear proxy cookie: {e}");
            internal_error()
        }
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test web_test`
Expected: PASS (new settings/import-url tests + all existing web tests, since `import_paper` behavior is preserved by `stage_and_ingest`).

- [ ] **Step 6: Commit**

```bash
git add src/web/mod.rs src/web/api.rs src/main.rs tests/web_test.rs
git commit -m "feat(web): POST /api/import + proxy-cookie settings endpoints"
```

---

## Task 9: CLI — `xuewen import` and `xuewen proxy-cookie`

**Files:**
- Modify: `src/main.rs` (two subcommands + their match arms)

- [ ] **Step 1: Add the subcommands**

In `src/main.rs`, add to `enum Command`:

```rust
    /// Import a paper from a URL, DOI, or arXiv id.
    Import { input: String },
    /// Manage the stored EZproxy session cookie used for paywalled imports.
    ProxyCookie {
        /// Store this cookie value (a `name=value; name2=value2` header string).
        #[arg(long, conflicts_with = "clear")]
        set: Option<String>,
        /// Remove the stored cookie.
        #[arg(long)]
        clear: bool,
    },
```

- [ ] **Step 2: Add the match arms**

Add these arms to the `match cli.command` block (e.g. after `Command::Ingest`). They reuse `ctx`, `pool`, and `cfg` already in scope:

```rust
        Command::Import { input } => {
            let fetcher =
                xuewen::import::Fetcher::new(cfg.proxy.as_ref().map(|p| p.login_url.clone()))?;
            let cookie = db::get_setting(&pool, "proxy_cookie").await?;
            match xuewen::import::import_source(&fetcher, &ctx.resolver, &input, cookie.as_deref())
                .await
            {
                Ok(fetched) => {
                    let staged = cfg.inbox_dir.join("_uploads").join(format!(
                        "{}-import.pdf",
                        uuid::Uuid::now_v7()
                    ));
                    tokio::fs::create_dir_all(staged.parent().unwrap()).await?;
                    tokio::fs::write(&staged, &fetched.bytes).await?;
                    match ctx.ingest_file_with_hint(&staged, fetched.hint).await {
                        Ok(Outcome::Ingested(id)) => println!("ingested {id}"),
                        Ok(Outcome::Duplicate) => println!("duplicate, skipped"),
                        Ok(Outcome::SameWork(id)) => println!("already in library ({id})"),
                        Ok(Outcome::InTrash(id)) => {
                            println!("in trash — run: xuewen restore {id}")
                        }
                        Err(e) => {
                            let _ = tokio::fs::remove_file(&staged).await;
                            return Err(e);
                        }
                    }
                }
                Err(xuewen::import::ImportError::Unsupported) => {
                    anyhow::bail!("could not recognize {input:?} as a URL, DOI, or arXiv id")
                }
                Err(xuewen::import::ImportError::CookieExpired) => anyhow::bail!(
                    "proxy session expired — refresh it: xuewen proxy-cookie --set '<cookie>'"
                ),
                Err(xuewen::import::ImportError::Unfetched { metadata }) => {
                    let title = metadata
                        .as_ref()
                        .and_then(|m| m.title.as_deref())
                        .unwrap_or("(unknown title)");
                    anyhow::bail!(
                        "could not fetch a PDF for {title:?} — paywalled with no open-access \
                         copy, or the cookie is missing/expired. Download it in your browser \
                         and drop it in the inbox."
                    )
                }
                Err(xuewen::import::ImportError::Network(e)) => {
                    return Err(e.context("fetch failed"))
                }
            }
        }
        Command::ProxyCookie { set, clear } => {
            if clear {
                db::delete_setting(&pool, "proxy_cookie").await?;
                println!("proxy cookie cleared");
            } else if let Some(cookie) = set {
                db::set_setting(&pool, "proxy_cookie", cookie.trim()).await?;
                println!("proxy cookie stored");
            } else {
                match db::setting_updated_at(&pool, "proxy_cookie").await? {
                    Some(ts) => println!("proxy cookie set (updated {ts})"),
                    None => println!("no proxy cookie set"),
                }
            }
        }
```

Ensure `use xuewen::import;` is not required (the arms use fully-qualified `xuewen::import::…`). `uuid` is already a dependency; add `use uuid;` is unnecessary with the fully-qualified path `uuid::Uuid`.

- [ ] **Step 3: Verify it builds and the CLI parses**

Run: `cargo build`
Expected: builds clean.

Run: `cargo run -- --config xuewen.example.toml proxy-cookie`
Expected: prints `no proxy cookie set` (using the example config's DB path, or an error naming a missing config — either confirms the subcommand wired up; do not commit any created DB).

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "feat(cli): xuewen import <url> and proxy-cookie management"
```

---

## Task 10: Frontend — API, types, and import state

**Files:**
- Modify: `frontend/src/lib/types.ts` (ImportResult `unfetched`; `Settings`)
- Modify: `frontend/src/lib/api.ts` (`importUrl`, `getSettings`, `setProxyCookie`, `clearProxyCookie`)
- Modify: `frontend/src/lib/state.svelte.ts` (`enqueueUrl`, url-aware drain, `unfetched` status)
- Modify: `frontend/src/components/ImportModal.test.ts` (state test for the URL path)

- [ ] **Step 1: Extend types**

In `frontend/src/lib/types.ts`, extend `ImportResult` and add `Settings`:

```ts
export type ImportResult =
  | { outcome: 'ingested'; id: string; title: string | null; status: string }
  | { outcome: 'duplicate' }
  | { outcome: 'same_work'; id: string }
  | { outcome: 'in_trash'; id: string }
  | { outcome: 'unfetched'; title: string | null; doi: string | null };

export interface Settings {
  proxy_cookie_set: boolean;
  proxy_cookie_updated_at: string | null;
}
```

- [ ] **Step 2: Add API functions**

In `frontend/src/lib/api.ts`, add (and extend the type import to include `Settings`):

```ts
export async function importUrl(input: string): Promise<ImportResult> {
  const res = await fetch('/api/import', {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ input }),
  });
  if (!res.ok) {
    let msg = `import failed: ${res.status}`;
    try {
      const j = await res.json();
      if (j && typeof j.error === 'string') msg = j.error;
    } catch {
      /* non-JSON error body */
    }
    throw new Error(msg);
  }
  return res.json();
}

export async function getSettings(): Promise<Settings> {
  const res = await fetch('/api/settings');
  if (!res.ok) throw new Error(`settings failed: ${res.status}`);
  return res.json();
}

export async function setProxyCookie(cookie: string): Promise<void> {
  const res = await fetch('/api/settings/proxy-cookie', {
    method: 'PUT',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ cookie }),
  });
  if (!res.ok) throw new Error(`save cookie failed: ${res.status}`);
}

export async function clearProxyCookie(): Promise<void> {
  const res = await fetch('/api/settings/proxy-cookie', { method: 'DELETE' });
  if (!res.ok) throw new Error(`clear cookie failed: ${res.status}`);
}
```

- [ ] **Step 3: Write the failing state test**

Add to `frontend/src/components/ImportModal.test.ts` a describe block for the URL path:

```ts
import { enqueueUrl } from '../lib/state.svelte';

describe('enqueueUrl', () => {
  beforeEach(() => {
    openImport();
    vi.restoreAllMocks();
  });

  it('imports a URL and records ingested', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL, init?: RequestInit) => {
        const u = String(url);
        const json = (o: unknown, status = 200) =>
          new Response(JSON.stringify(o), { status, headers: { 'content-type': 'application/json' } });
        if (u === '/api/import' && init?.method === 'POST') {
          return json({ outcome: 'ingested', id: '1', title: 'Fetched Paper', status: 'resolved' });
        }
        if (u.startsWith('/api/papers')) return json([]);
        return json({ total: 0, resolved: 0, needs_review: 0 });
      }),
    );

    await enqueueUrl('https://arxiv.org/abs/1706.03762');

    expect(importState.items).toHaveLength(1);
    expect(importState.items[0].status).toBe('ingested');
    expect(importState.items[0].message).toBe('Fetched Paper');
  });

  it('marks an unfetched result distinctly', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async (url: string | URL, init?: RequestInit) => {
        const u = String(url);
        const json = (o: unknown, status = 200) =>
          new Response(JSON.stringify(o), { status, headers: { 'content-type': 'application/json' } });
        if (u === '/api/import' && init?.method === 'POST') {
          return json({ outcome: 'unfetched', title: 'Paywalled Paper', doi: '10.1145/x' });
        }
        if (u.startsWith('/api/papers')) return json([]);
        return json({ total: 0, resolved: 0, needs_review: 0 });
      }),
    );

    await enqueueUrl('10.1145/x');

    expect(importState.items[0].status).toBe('unfetched');
    expect(importState.items[0].message).toBe('Paywalled Paper');
  });
});
```

- [ ] **Step 4: Run the test to verify it fails**

Run: `npm --prefix frontend run test -- ImportModal`
Expected: FAIL — `enqueueUrl` not exported; `'unfetched'` not a valid status.

- [ ] **Step 5: Implement url-aware queue in state**

In `frontend/src/lib/state.svelte.ts`:

1. Import `importUrl` alongside `importPaper` at the top.
2. Add `'unfetched'` to the `ImportItem` status union:

```ts
export interface ImportItem {
  name: string;
  status:
    | 'queued'
    | 'importing'
    | 'ingested'
    | 'duplicate'
    | 'same-work'
    | 'in-trash'
    | 'unfetched'
    | 'failed';
  message?: string;
  needsReview?: boolean;
}
```

3. Replace the `pending` queue typing and add `enqueueUrl`. Change:

```ts
const pending: { file: File; index: number; session: number }[] = [];
```

to a discriminated union:

```ts
type Job =
  | { kind: 'file'; file: File }
  | { kind: 'url'; input: string };
const pending: { job: Job; index: number; session: number }[] = [];
```

4. Rewrite `enqueueFiles` to push the new shape, and add `enqueueUrl`:

```ts
export function enqueueFiles(files: File[]): Promise<void> {
  const session = importSession;
  for (const file of files) {
    const index = importState.items.push({ name: file.name, status: 'queued' }) - 1;
    pending.push({ job: { kind: 'file', file }, index, session });
  }
  return startDrain();
}

export function enqueueUrl(input: string): Promise<void> {
  const session = importSession;
  const index = importState.items.push({ name: input, status: 'queued' }) - 1;
  pending.push({ job: { kind: 'url', input }, index, session });
  return startDrain();
}

function startDrain(): Promise<void> {
  if (!draining) {
    draining = drainQueue().finally(() => {
      draining = null;
    });
  }
  return draining;
}
```

5. Update `drainQueue`'s per-job body: fetch via the right function and handle `unfetched`:

```ts
async function drainQueue(): Promise<void> {
  while (pending.length > 0) {
    const item = pending.shift()!;
    if (importState.cancelled || item.session !== importSession) continue;
    importState.items[item.index].status = 'importing';
    try {
      const res =
        item.job.kind === 'file'
          ? await importPaper(item.job.file)
          : await importUrl(item.job.input);
      if (item.session !== importSession) continue;
      if (res.outcome === 'duplicate') {
        importState.items[item.index].status = 'duplicate';
      } else if (res.outcome === 'same_work') {
        importState.items[item.index].status = 'same-work';
      } else if (res.outcome === 'in_trash') {
        importState.items[item.index].status = 'in-trash';
        importState.items[item.index].message = res.id;
      } else if (res.outcome === 'unfetched') {
        importState.items[item.index].status = 'unfetched';
        importState.items[item.index].message = res.title ?? '(untitled)';
      } else {
        importState.items[item.index].status = 'ingested';
        importState.items[item.index].message = res.title ?? '(untitled)';
        importState.items[item.index].needsReview = res.status === 'needs_review';
      }
    } catch (e) {
      if (item.session !== importSession) continue;
      importState.items[item.index].status = 'failed';
      importState.items[item.index].message = (e as Error).message;
    }
  }
  await loadPapers();
  await loadStats();
}
```

- [ ] **Step 6: Run the tests to verify they pass**

Run: `npm --prefix frontend run test -- ImportModal`
Expected: PASS (existing `enqueueFiles` tests + new `enqueueUrl` tests).

- [ ] **Step 7: Commit**

```bash
git add frontend/src/lib/types.ts frontend/src/lib/api.ts frontend/src/lib/state.svelte.ts frontend/src/components/ImportModal.test.ts
git commit -m "feat(web): URL import + settings client and queue state"
```

---

## Task 11: Frontend — ImportModal URL input + cookie panel

**Files:**
- Modify: `frontend/src/components/ImportModal.svelte`

- [ ] **Step 1: Add the URL input and cookie panel to the modal**

In `frontend/src/components/ImportModal.svelte`:

1. Extend the script imports:

```ts
  import { Check, CircleAlert, Copy, FileWarning, Link, Loader, Upload, X } from 'lucide-svelte';
  import {
    clearProxyCookie,
    getSettings,
    setProxyCookie,
  } from '../lib/api';
  import { closeImport, enqueueFiles, enqueueUrl, importState } from '../lib/state.svelte';
  import type { Settings } from '../lib/types';
```

2. Add local state and handlers in the script:

```ts
  let urlInput = $state('');
  function submitUrl() {
    const v = urlInput.trim();
    if (!v) return;
    urlInput = '';
    void enqueueUrl(v);
  }

  let settings = $state<Settings | null>(null);
  let cookieInput = $state('');
  let savingCookie = $state(false);
  async function loadSettings() {
    try {
      settings = await getSettings();
    } catch {
      settings = null;
    }
  }
  async function saveCookie() {
    const v = cookieInput.trim();
    if (!v) return;
    savingCookie = true;
    try {
      await setProxyCookie(v);
      cookieInput = '';
      await loadSettings();
    } finally {
      savingCookie = false;
    }
  }
  async function removeCookie() {
    await clearProxyCookie();
    await loadSettings();
  }
  // Load once when the modal mounts.
  $effect(() => {
    void loadSettings();
  });
```

3. In the template, add a URL row directly above the drop-zone `<button>` (inside the scrollable body `<div class="min-h-0 flex-1 overflow-y-auto p-4">`, before the drop-zone button):

```svelte
      <form
        class="mb-3 flex gap-2"
        onsubmit={(e) => {
          e.preventDefault();
          submitUrl();
        }}
      >
        <div class="flex flex-1 items-center gap-2 rounded-lg border border-slate-300 px-2 dark:border-slate-700">
          <Link size={16} class="shrink-0 text-slate-400" />
          <input
            bind:value={urlInput}
            type="text"
            placeholder="Paste a link, DOI, or arXiv id"
            class="w-full bg-transparent py-2 text-sm outline-none"
          />
        </div>
        <button
          type="submit"
          class="rounded-lg bg-indigo-600 px-3 py-2 text-sm font-medium text-white hover:bg-indigo-500 disabled:opacity-50"
          disabled={!urlInput.trim()}
        >
          Add
        </button>
      </form>
```

4. Add the collapsible cookie panel just below the queue `<ul>` block (still inside the scrollable body), so it renders under the file/url list:

```svelte
      <details class="mt-4 rounded-lg border border-slate-200 text-sm dark:border-slate-800">
        <summary class="cursor-pointer px-3 py-2 text-slate-600 dark:text-slate-300">
          Institutional access (EZproxy cookie)
          {#if settings?.proxy_cookie_set}
            <span class="ml-1 rounded bg-emerald-100 px-1.5 py-0.5 text-xs text-emerald-700 dark:bg-emerald-500/15 dark:text-emerald-400">set</span>
          {:else}
            <span class="ml-1 rounded bg-slate-100 px-1.5 py-0.5 text-xs text-slate-500 dark:bg-slate-800 dark:text-slate-400">not set</span>
          {/if}
        </summary>
        <div class="space-y-2 border-t border-slate-200 p-3 dark:border-slate-800">
          <p class="text-xs text-slate-500 dark:text-slate-400">
            Paste the <code>Cookie:</code> header for <code>proxy.uchicago.edu</code> (from a browser
            cookie extension or DevTools) to fetch paywalled ACM/IEEE PDFs. It expires — refresh it here.
          </p>
          <div class="flex gap-2">
            <input
              bind:value={cookieInput}
              type="password"
              placeholder="ezproxy=…; …"
              class="w-full rounded-lg border border-slate-300 bg-transparent px-2 py-1.5 text-sm outline-none dark:border-slate-700"
            />
            <button
              type="button"
              onclick={saveCookie}
              disabled={!cookieInput.trim() || savingCookie}
              class="rounded-lg bg-slate-700 px-3 py-1.5 text-sm text-white hover:bg-slate-600 disabled:opacity-50"
            >Save</button>
          </div>
          {#if settings?.proxy_cookie_set}
            <div class="flex items-center justify-between text-xs text-slate-500 dark:text-slate-400">
              <span>Updated {settings.proxy_cookie_updated_at ?? '—'}</span>
              <button type="button" onclick={removeCookie} class="text-red-500 hover:underline">Clear</button>
            </div>
          {/if}
        </div>
      </details>
```

5. Add an `unfetched` status row rendering to the queue `{#each}` block. In the icon block, add a branch (after the `failed` branch):

```svelte
              {:else if item.status === 'unfetched'}
                <FileWarning size={14} class="shrink-0 text-amber-500" />
```

And in the trailing status-text block (the `{:else}` span), add before the closing `{/if}`:

```svelte
                  {:else if item.status === 'unfetched'}no PDF — download & drop in inbox
```

6. Update the `summary` derived counter so `unfetched` counts as a non-failure "skipped": in the `$derived.by` loop, extend the skipped condition:

```ts
      else if (
        i.status === 'duplicate' ||
        i.status === 'same-work' ||
        i.status === 'in-trash' ||
        i.status === 'unfetched'
      )
        c.skipped++;
```

- [ ] **Step 2: Verify the frontend builds and tests pass**

Run: `npm --prefix frontend run build`
Expected: builds clean (no type errors).

Run: `npm --prefix frontend run test`
Expected: PASS (all frontend tests).

- [ ] **Step 3: Commit**

```bash
git add frontend/src/components/ImportModal.svelte
git commit -m "feat(web): URL import field + EZproxy cookie panel in ImportModal"
```

---

## Task 12: Example config + docs

**Files:**
- Modify: `xuewen.example.toml` (document `[proxy]`)

- [ ] **Step 1: Document the proxy section**

Append to `xuewen.example.toml`:

```toml
# Optional: institutional proxy for fetching paywalled PDFs by URL/DOI.
# The login prefix has the target URL percent-encoded and appended. The rotating
# session cookie is NOT stored here — set it via the web UI's "Institutional
# access" panel or `xuewen proxy-cookie --set '<cookie>'`.
# [proxy]
# login_url = "https://proxy.uchicago.edu/login?url="
```

- [ ] **Step 2: Full test sweep**

Run: `cargo test`
Expected: PASS (all Rust unit + integration tests).

Run: `npm --prefix frontend run test`
Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add xuewen.example.toml
git commit -m "docs(config): document [proxy] login_url for URL import"
```

---

## Final verification

- [ ] **Run the complete backend suite**

Run: `cargo test`
Expected: all pass.

- [ ] **Run the complete frontend suite**

Run: `npm --prefix frontend run test`
Expected: all pass.

- [ ] **Manual smoke (optional, needs network + a real cookie)**

Build the frontend, serve, and try: an arXiv URL (no cookie needed) should ingest; an ACM DOI with a freshly-pasted cookie should ingest via the proxy; a paywalled DOI with no cookie and no OA copy should show an `unfetched` row with the resolved title.

```bash
npm --prefix frontend run build && cargo run -- serve
```
