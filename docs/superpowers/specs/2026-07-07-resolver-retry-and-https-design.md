# Design: arXiv HTTPS + Reactive Retry/Back-off for Public Resolvers

**Project:** Xuewen — self-hosted reference manager for CS papers.
**Date:** 2026-07-07
**Status:** Approved (design phase)

## 1. Purpose

Two robustness fixes to the metadata-resolution layer, prompted by a live smoke
where a repeated arXiv lookup returned **HTTP 429** and the paper degraded to
`needs_review`:

1. **arXiv over HTTPS** — the arXiv base is `http://export.arxiv.org`, which now
   301-redirects to `https`. Point directly at `https://export.arxiv.org` to skip
   the needless redirect round-trip.
2. **Reactive retry + back-off** — a shared HTTP helper for the three public
   resolvers (arXiv, Crossref, DBLP) that retries transient `429`/`5xx`/network
   failures with bounded exponential back-off, honoring `Retry-After`.

Graceful degradation is preserved end-to-end: exhausting retries still yields
`Unresolved` → `needs_review`, exactly as today. Retry only rescues *transient*
rate limits and blips; it never turns a failure into an abort.

Proactive per-host throttling is deliberately **out of scope** (reactive-only was
chosen): the CLI/watcher resolves serially at a naturally low request rate, and a
`refresh --all` burst is still covered by reactive back-off on the occasional 429.

## 2. Current shape

All three public resolvers duplicate the same block (`src/resolve/arxiv.rs`,
`crossref.rs`, `dblp.rs`) across four call sites (`arxiv::fetch`,
`crossref::fetch`, `crossref::search`, `dblp::fetch`):

```rust
let resp = client.get(&url).send().await?;
if !resp.status().is_success() {
    return Err(anyhow!("… HTTP {}", resp.status()));
}
Ok(resp.text().await?)
```

The `Resolver` (`src/resolve/mod.rs`) owns the single `reqwest::Client` (built with
a polite `User-Agent` and a 20s timeout) and passes `&self.http` into each fetch
fn. GROBID (`src/resolve/grobid.rs`) is a **local, self-hosted multipart POST**
service with its own client — outside this change.

## 3. Shared HTTP helper

New module `src/resolve/http.rs`:

```rust
pub struct RetryPolicy {
    pub max_attempts: u32,   // total tries incl. the first (default 4)
    pub base_delay: Duration, // exponential base (default 1s)
    pub max_delay: Duration,  // per-sleep cap (default 16s)
}

pub struct HttpClient {
    client: reqwest::Client,
    retry: RetryPolicy,
}

impl HttpClient {
    pub fn new(client: reqwest::Client, retry: RetryPolicy) -> Self;
    /// GET `url`, retrying transient failures, returning the response body.
    pub async fn get_text(&self, url: &str) -> Result<String>;
}
```

- The `Resolver` holds an `HttpClient` in place of the bare `reqwest::Client`. The
  three fetch fns take `&HttpClient` and call `http.get_text(&url).await?`,
  replacing their inline `.send()` / status-check / `.text()` block. Their `base`
  parameters and `parse` logic are unchanged.

### 3.1 `get_text` behaviour

For attempt `n` in `0..max_attempts`:

1. `client.get(url).send().await`.
2. **Success** (`2xx`) → return `resp.text().await`.
3. **Retryable** — status in `{429, 500, 502, 503, 504}`, **or** a `reqwest::Error`
   with `is_timeout()` or `is_connect()` — and this is not the last attempt →
   compute the delay, `tokio::time::sleep(delay)`, then continue to the next attempt.
4. **Non-retryable** non-success (e.g. `404`, `400`) → return `Err` immediately.
5. Last attempt still retryable/failing → return `Err` (the resolver degrades to
   `Unresolved`).

**Delay** = `Retry-After` header when present and parseable as delta-seconds,
otherwise exponential back-off `base_delay · 2^n`; the chosen delay is capped at
`max_delay` (and a `Retry-After` is additionally capped so a hostile/large value
can't stall ingest). The HTTP-date form of `Retry-After` is not parsed — fall
through to exponential back-off if the header isn't a plain integer.

## 4. arXiv HTTPS

`Resolver::new` builds with `https://export.arxiv.org` instead of `http://…`.
`with_bases` / `with_dblp_base` (tests) already take explicit bases, so only the
real-endpoint constructor changes.

## 5. Defaults & testability

- Production `RetryPolicy`: `max_attempts 4`, `base_delay 1s`, `max_delay 16s`,
  `Retry-After` capped at 30s. `Resolver::new` uses these.
- Tests need instant retries. `with_bases` (and `with_dblp_base`) construct the
  `Resolver` with a near-zero `base_delay` (e.g. 1ms) so wiremock retry tests don't
  sleep for real seconds. (Either a fixed test policy inside `with_bases`, or a
  `with_retry_policy` builder — implementer's choice; the constraint is that tests
  run fast and deterministically.)

## 6. Testing (wiremock, offline, deterministic)

- **`http.rs` unit/integration:**
  - `429` once then `200` → `get_text` returns the body (retry works); exactly 2
    requests.
  - Always `429` → `Err` after exactly `max_attempts` requests (bounded).
  - `404` → `Err` after exactly **1** request (no retry on non-retryable).
  - `Retry-After: 0` on a `429`-then-`200` → honored (retry succeeds), test stays fast.
  - `503` then `200` → retried and succeeds (5xx is retryable).
- **Resolver-level (existing wiremock tests):** unchanged behaviour on the happy
  path; add one test that a transient `429`-then-`200` from a mock arXiv/Crossref
  still yields `Resolution::Resolved`.
- **`parse` unit tests:** unchanged.

## 7. Out of scope

- Proactive per-host rate limiting / min-interval throttle.
- Retrying GROBID (local service, different call shape).
- Parsing the HTTP-date form of `Retry-After`.
- The `refresh` command (separate plan, `2026-07-07-cite-key-naming-and-refresh-design.md`).
