# Resolver Retry + arXiv HTTPS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the three public bibliographic resolvers (arXiv, Crossref, DBLP) resilient to transient HTTP `429`/`5xx`/network failures by routing every request through a shared retrying `HttpClient`, and point arXiv at `https://` to skip the 301 redirect.

**Architecture:** A new `src/resolve/http.rs` provides `HttpClient` (wraps a `reqwest::Client` + a `RetryPolicy`) with `get_text`/`send_text` that retry transient failures with bounded exponential back-off honoring `Retry-After`. The `Resolver` holds an `HttpClient` instead of a bare `reqwest::Client`; the three `fetch`/`search` functions take `&HttpClient` and drop their duplicated send/status/text blocks. Production uses a polite 1s-base policy; `Resolver::with_bases` (used by tests) bakes in a near-zero policy so retry paths test fast. Graceful degradation is unchanged: exhausting retries still yields `Unresolved` → `needs_review`.

**Tech Stack:** Rust, tokio (async + `time::sleep`), reqwest 0.12 (rustls), anyhow, wiremock (dev, HTTP mock).

**Environment:** `$IN_NIX_SHELL` is not set — run every cargo command through the flake dev shell: `nix develop -c '<command>'`.

---

## File Structure

- **Create** `src/resolve/http.rs` — `RetryPolicy` (attempts/delays + `production()`/`fast_for_tests()`), `HttpClient` (`new`, `get`, `send_text`, `get_text`), and the retry unit tests. One responsibility: retrying HTTP GETs.
- **Modify** `src/resolve/mod.rs` — declare `pub mod http;`; change the `Resolver.http` field type to `HttpClient`; add a private `build` constructor taking a `RetryPolicy`; `new` → https + `production()`, `with_bases` → `fast_for_tests()`.
- **Modify** `src/resolve/arxiv.rs`, `src/resolve/crossref.rs`, `src/resolve/dblp.rs` — `fetch`/`search` take `&HttpClient` and call `get_text`/`send_text`; drop the inline `anyhow!("… HTTP …")` blocks.
- **Modify** `tests/resolve_test.rs` — add one integration test proving a transient `429`-then-`200` still resolves through the `Resolver`.

No other files change. GROBID (`src/resolve/grobid.rs`, local multipart POST) is out of scope. No migration.

---

## Task 1: Retrying `HttpClient` helper

**Files:**
- Create: `src/resolve/http.rs`
- Modify: `src/resolve/mod.rs:1-4` (add `pub mod http;` beside the other `pub mod` lines)

- [ ] **Step 1: Declare the module**

In `src/resolve/mod.rs`, the top currently reads:

```rust
pub mod arxiv;
pub mod crossref;
pub mod dblp;
pub mod grobid;
```

Add the new module (keep alphabetical-ish grouping):

```rust
pub mod arxiv;
pub mod crossref;
pub mod dblp;
pub mod grobid;
pub mod http;
```

- [ ] **Step 2: Write the first failing test**

Create `src/resolve/http.rs` with ONLY the test below (the types it references don't exist yet — this is the RED state):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client() -> HttpClient {
        HttpClient::new(reqwest::Client::new(), RetryPolicy::fast_for_tests())
    }

    #[tokio::test]
    async fn retries_429_then_succeeds() {
        let server = MockServer::start().await;
        // First hit → 429 (stops matching after 1), later hits → 200.
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        let body = client()
            .get_text(&format!("{}/x", server.uri()))
            .await
            .unwrap();
        assert_eq!(body, "ok");
    }
}
```

- [ ] **Step 3: Run the test to verify it fails**

Run: `nix develop -c 'cargo test --lib resolve::http'`
Expected: FAIL to compile — `cannot find type HttpClient` / `RetryPolicy` in this scope.

- [ ] **Step 4: Implement `RetryPolicy` + `HttpClient`**

Insert this ABOVE the `#[cfg(test)] mod tests` block in `src/resolve/http.rs`:

```rust
use anyhow::{anyhow, Result};
use std::time::Duration;

/// A `Retry-After` value is honored but never allowed to stall ingest longer
/// than this.
const RETRY_AFTER_CAP: Duration = Duration::from_secs(30);

/// How `HttpClient` retries transient HTTP failures.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Total attempts including the first (default 4 = initial + 3 retries).
    pub max_attempts: u32,
    /// Exponential back-off base: the delay before retry `n` is `base_delay * 2^n`.
    pub base_delay: Duration,
    /// Upper bound on any single back-off sleep.
    pub max_delay: Duration,
}

impl RetryPolicy {
    /// Polite defaults for the real bibliographic APIs.
    pub fn production() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(16),
        }
    }

    /// Near-zero delays so tests exercise the retry paths without real sleeps.
    pub fn fast_for_tests() -> Self {
        Self {
            max_attempts: 4,
            base_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(5),
        }
    }
}

/// A `reqwest::Client` wrapper that retries transient `429`/`5xx`/network
/// failures with bounded exponential back-off, then returns the body text.
/// A non-retryable status (e.g. 404) or an exhausted budget returns `Err`, so
/// callers still degrade to `Unresolved` exactly as before.
pub struct HttpClient {
    client: reqwest::Client,
    retry: RetryPolicy,
}

impl HttpClient {
    pub fn new(client: reqwest::Client, retry: RetryPolicy) -> Self {
        Self { client, retry }
    }

    /// Start a GET request the caller can add `.query(...)` to before handing it
    /// to `send_text`.
    pub fn get(&self, url: &str) -> reqwest::RequestBuilder {
        self.client.get(url)
    }

    /// GET `url` with retries, returning the body text.
    pub async fn get_text(&self, url: &str) -> Result<String> {
        self.send_text(self.get(url)).await
    }

    /// Send `req` with retries, returning the body text. `req` must be cloneable
    /// (true for the query-only GETs used here).
    pub async fn send_text(&self, req: reqwest::RequestBuilder) -> Result<String> {
        let mut attempt: u32 = 0;
        loop {
            let last = attempt + 1 >= self.retry.max_attempts;
            let this = req
                .try_clone()
                .ok_or_else(|| anyhow!("request body is not retryable"))?;
            match this.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp.text().await?);
                    }
                    if last || !is_retryable_status(status.as_u16()) {
                        return Err(anyhow!("HTTP {status}"));
                    }
                    let delay = self.delay_for(attempt, retry_after(&resp));
                    tokio::time::sleep(delay).await;
                }
                Err(e) => {
                    if last || !(e.is_timeout() || e.is_connect()) {
                        return Err(e.into());
                    }
                    let delay = self.delay_for(attempt, None);
                    tokio::time::sleep(delay).await;
                }
            }
            attempt += 1;
        }
    }

    /// The back-off before the next retry: `Retry-After` when supplied, otherwise
    /// exponential; both capped.
    fn delay_for(&self, attempt: u32, retry_after: Option<Duration>) -> Duration {
        match retry_after {
            Some(d) => d.min(RETRY_AFTER_CAP),
            None => {
                let shift = attempt.min(16); // guard the shift against overflow
                self.retry
                    .base_delay
                    .saturating_mul(1u32 << shift)
                    .min(self.retry.max_delay)
            }
        }
    }
}

/// Transient statuses worth retrying (rate limit + transient server errors).
fn is_retryable_status(code: u16) -> bool {
    matches!(code, 429 | 500 | 502 | 503 | 504)
}

/// Parse the delta-seconds form of `Retry-After`. The HTTP-date form is ignored
/// (falls through to exponential back-off).
fn retry_after(resp: &reqwest::Response) -> Option<Duration> {
    resp.headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}
```

- [ ] **Step 5: Run the test to verify it passes**

Run: `nix develop -c 'cargo test --lib resolve::http'`
Expected: PASS (`retries_429_then_succeeds ... ok`).

- [ ] **Step 6: Add the remaining retry tests**

Append these inside the `mod tests` block (after `retries_429_then_succeeds`):

```rust
    #[tokio::test]
    async fn gives_up_after_max_attempts() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(429))
            .expect(4) // RetryPolicy::max_attempts
            .mount(&server)
            .await;

        let out = client().get_text(&format!("{}/x", server.uri())).await;
        assert!(out.is_err());
        // MockServer verifies on drop that exactly 4 requests arrived.
    }

    #[tokio::test]
    async fn does_not_retry_404() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1) // no retry on a non-retryable status
            .mount(&server)
            .await;

        assert!(client()
            .get_text(&format!("{}/x", server.uri()))
            .await
            .is_err());
    }

    #[tokio::test]
    async fn honors_retry_after_zero() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        assert_eq!(
            client().get_text(&format!("{}/x", server.uri())).await.unwrap(),
            "ok"
        );
    }

    #[tokio::test]
    async fn retries_503() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/x"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;

        assert_eq!(
            client().get_text(&format!("{}/x", server.uri())).await.unwrap(),
            "ok"
        );
    }
```

- [ ] **Step 7: Run the full module tests + clippy**

Run: `nix develop -c 'cargo test --lib resolve::http && cargo clippy --all-targets -- -D warnings'`
Expected: all 5 `resolve::http` tests PASS; clippy reports no warnings.

- [ ] **Step 8: Format and commit**

```bash
nix develop -c 'cargo fmt'
git add src/resolve/http.rs src/resolve/mod.rs
git -c commit.gpgsign=false commit -m "feat(resolve): add retrying HttpClient helper"
```

---

## Task 2: Route resolvers through `HttpClient` + arXiv HTTPS

**Files:**
- Modify: `src/resolve/mod.rs:65-102` (struct field + constructors)
- Modify: `src/resolve/arxiv.rs:1-13`
- Modify: `src/resolve/crossref.rs:1-27`
- Modify: `src/resolve/dblp.rs:1-17`
- Test: `tests/resolve_test.rs` (add one integration test)

- [ ] **Step 1: Point the `Resolver` at `HttpClient` and split the constructors**

In `src/resolve/mod.rs`, add the import near the other `use` lines (top of file, after `use crate::models::Identifier;`):

```rust
use self::http::{HttpClient, RetryPolicy};
```

Change the struct field type. Current:

```rust
pub struct Resolver {
    http: reqwest::Client,
    arxiv_base: String,
    crossref_base: String,
    dblp_base: String,
}
```

New:

```rust
pub struct Resolver {
    http: HttpClient,
    arxiv_base: String,
    crossref_base: String,
    dblp_base: String,
}
```

Replace the whole `new` + `with_bases` block (currently `mod.rs:73-102`) with a private `build` core plus the two public constructors. Note the arXiv base is now `https`:

```rust
    /// Build a resolver pointing at the real arXiv and Crossref endpoints, with a
    /// polite retry/back-off policy.
    pub fn new(contact_email: Option<&str>) -> Result<Self> {
        Self::build(
            contact_email,
            "https://export.arxiv.org".to_string(),
            "https://api.crossref.org".to_string(),
            RetryPolicy::production(),
        )
    }

    /// Build a resolver with explicit base URLs (used by tests to point at a mock
    /// server). Uses a near-zero back-off so retry paths test fast.
    pub fn with_bases(
        contact_email: Option<&str>,
        arxiv_base: String,
        crossref_base: String,
    ) -> Result<Self> {
        Self::build(
            contact_email,
            arxiv_base,
            crossref_base,
            RetryPolicy::fast_for_tests(),
        )
    }

    fn build(
        contact_email: Option<&str>,
        arxiv_base: String,
        crossref_base: String,
        retry: RetryPolicy,
    ) -> Result<Self> {
        let ua = match contact_email {
            Some(email) => format!("xuewen/0.1 (mailto:{email})"),
            None => "xuewen/0.1".to_string(),
        };
        let client = reqwest::Client::builder()
            .user_agent(ua)
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self {
            http: HttpClient::new(client, retry),
            arxiv_base,
            crossref_base,
            dblp_base: "https://dblp.org".to_string(),
        })
    }
```

Leave `with_dblp_base` (currently `mod.rs:104-108`) unchanged. The internal call sites `arxiv::fetch(&self.http, …)`, `crossref::fetch(&self.http, …)`, `crossref::search(&self.http, …)`, `dblp::fetch(&self.http, …)` are unchanged in mod.rs — only the callees' parameter types change (next steps).

- [ ] **Step 2: Update `arxiv::fetch`**

In `src/resolve/arxiv.rs`, change the imports (line 1 + line 3). Current:

```rust
use anyhow::{anyhow, Result};

use super::{collapse_ws, ResolvedMetadata};
```

New:

```rust
use anyhow::Result;

use super::http::HttpClient;
use super::{collapse_ws, ResolvedMetadata};
```

Replace the `fetch` function (currently `arxiv.rs:5-13`) with:

```rust
/// Fetch the Atom response for a single arXiv id from `{base}/api/query`.
pub async fn fetch(http: &HttpClient, base: &str, id: &str) -> Result<String> {
    let url = format!("{base}/api/query?id_list={id}");
    http.get_text(&url).await
}
```

- [ ] **Step 3: Update `crossref::fetch` and `crossref::search`**

In `src/resolve/crossref.rs`, change the imports (lines 1 + 4). Current:

```rust
use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{collapse_ws, strip_tags, ResolvedMetadata};
```

New:

```rust
use anyhow::Result;
use serde_json::Value;

use super::http::HttpClient;
use super::{collapse_ws, strip_tags, ResolvedMetadata};
```

Replace both `fetch` and `search` (currently `crossref.rs:6-27`) with:

```rust
/// Fetch the Crossref work record for a DOI from `{base}/works/{doi}`.
pub async fn fetch(http: &HttpClient, base: &str, doi: &str) -> Result<String> {
    let url = format!("{base}/works/{doi}");
    http.get_text(&url).await
}

/// Search Crossref by bibliographic string (title). Returns raw JSON.
pub async fn search(http: &HttpClient, base: &str, title: &str) -> Result<String> {
    let req = http
        .get(&format!("{base}/works"))
        .query(&[("query.bibliographic", title), ("rows", "5")]);
    http.send_text(req).await
}
```

- [ ] **Step 4: Update `dblp::fetch`**

In `src/resolve/dblp.rs`, change the imports (lines 1 + 4). Current:

```rust
use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{collapse_ws, ResolvedMetadata};
```

New:

```rust
use anyhow::Result;
use serde_json::Value;

use super::http::HttpClient;
use super::{collapse_ws, ResolvedMetadata};
```

Replace the `fetch` function (currently `dblp.rs:6-17`) with:

```rust
/// Search DBLP publications by title. Returns raw JSON.
pub async fn fetch(http: &HttpClient, base: &str, title: &str) -> Result<String> {
    let req = http
        .get(&format!("{base}/search/publ/api"))
        .query(&[("q", title), ("format", "json"), ("h", "5")]);
    http.send_text(req).await
}
```

- [ ] **Step 5: Build the crate**

Run: `nix develop -c 'cargo build'`
Expected: compiles cleanly (no unused-import or type-mismatch errors).

- [ ] **Step 6: Run the full test suite (regression safety net)**

The existing `resolve_test.rs`, `pipeline_test.rs`, and `watcher_test.rs` drive these fetch paths through wiremock, so they prove the refactor preserves behaviour.

Run: `nix develop -c 'cargo test'`
Expected: all existing tests PASS (no regressions).

- [ ] **Step 7: Add a resolver-level transient-429 integration test**

In `tests/resolve_test.rs`, append this test (the fixtures `CROSSREF_FIXTURE`, and imports `method`/`path`/`Mock`/`MockServer`/`ResponseTemplate`, `Identifier`, `Resolution`, `Resolver` already exist at the top of the file):

```rust
#[tokio::test]
async fn resolves_after_transient_429() {
    let server = MockServer::start().await;
    let doi = "10.1145/3292500.3330701";
    // First request is rate-limited, the retry succeeds.
    Mock::given(method("GET"))
        .and(path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(429))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;

    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();
    let res = resolver
        .resolve(&Identifier::Doi(doi.to_string()), None)
        .await;

    assert!(matches!(res, Resolution::Resolved(_)));
}
```

- [ ] **Step 8: Run the new test**

Run: `nix develop -c 'cargo test --test resolve_test resolves_after_transient_429'`
Expected: PASS (the Crossref DOI resolves despite the first 429).

- [ ] **Step 9: Lint, format, and full verification**

Run: `nix develop -c 'cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test'`
Expected: fmt clean; clippy no warnings; entire suite PASS.

- [ ] **Step 10: Commit**

```bash
git add src/resolve/mod.rs src/resolve/arxiv.rs src/resolve/crossref.rs src/resolve/dblp.rs tests/resolve_test.rs
git -c commit.gpgsign=false commit -m "feat(resolve): route resolvers through HttpClient; arXiv over https"
```

---

## Verification (Definition of Done)

- `nix develop -c 'cargo test'` — whole suite green, including the 5 new `resolve::http` unit tests and `resolves_after_transient_429`.
- `nix develop -c 'cargo clippy --all-targets -- -D warnings'` — clean.
- `Resolver::new` uses `https://export.arxiv.org` and `RetryPolicy::production()`; `with_bases` uses `fast_for_tests()`.
- The three resolvers no longer contain inline `!resp.status().is_success()` blocks — all HTTP goes through `HttpClient`.
- Behaviour on permanent failure is unchanged: a resolver whose every attempt fails still returns `Unresolved`, so ingest still degrades to `needs_review`.

## Notes for the executor

- Retryable = HTTP `429/500/502/503/504` or a `reqwest` timeout/connect error; everything else (e.g. `404`) fails immediately with no retry.
- `MockServer` verifies `.expect(n)` counts when it is dropped at end of test, which is how the "exactly N requests" assertions work — don't add manual counters.
- Do not touch `src/resolve/grobid.rs` (local multipart POST, deliberately out of scope) or add a proactive throttle (reactive-only was chosen in the spec).
- Every commit uses `git -c commit.gpgsign=false` (SSH signing is unavailable this session).
