# Design: URL / identifier import (arXiv · ACM · IEEE)

**Project:** Xuewen — self-hosted reference manager for CS papers.
**Date:** 2026-07-08
**Status:** Approved (design phase)

## 1. Purpose

Let a user add a paper by **pasting a link, DOI, or arXiv id** — from the web UI
*or* the CLI — instead of only uploading a PDF file. The feature fetches the
paper's PDF and runs it through the **existing `ingest_file` pipeline**, so a URL
import and a file upload produce identical library records.

The headline case is authenticated access to paywalled publishers (ACM Digital
Library, IEEE Xplore) via the user's University of Chicago **EZproxy** session.
Because Xuewen's server is a separate process — and, for this user, a separate
machine — from the browser that holds the UChicago SSO/Duo session, the server
cannot inherit that session automatically. The design therefore has the server
hold a **user-supplied EZproxy session cookie**, refreshed from the browser when
it expires. arXiv and open-access copies need no cookie at all.

This is the web UI's **third mutation** (after delete and file-upload import) and
adds the first CLI-triggered network fetch of external content. The same
trusted-deployment caveat as the earlier mutations applies (no auth; `serve`
refuses a non-loopback bind without `--allow-remote`).

## 2. Core principle: `input → PDF bytes → ingest_file`

The new module's only real job is to turn an input string into PDF bytes. Once it
has bytes, it stages them into the existing `inbox_dir/_uploads` directory and
calls **`ingest_file` unchanged**. Everything downstream — content-hash dedup,
identifier/GROBID resolution, cite-key filing, DB insert, the `Ingested` /
`Duplicate` / `SameWork` / `InTrash` outcomes — is reused verbatim.

```
input (URL | DOI | arXiv id)
   │  parse_source
   ▼
Source::{Arxiv | Doi | Url}
   │  Importer::fetch_pdf        ← the only genuinely new logic
   ▼
PDF bytes ── stage to _uploads ──▶ ingest_file_with_hint()  (reused pipeline)
```

Because both the CLI command and the web endpoint call the same
`Importer` + `ingest_file` path, CLI and web import are symmetric by
construction: neither can do something the other can't.

## 3. Decisions (settled during brainstorming)

| Decision | Choice |
|---|---|
| Paywalled access mechanism | **Server-stored EZproxy session cookie**, supplied by the user from their browser (a cookies.txt-style export of `proxy.uchicago.edu` cookies) |
| Why not auto-read the browser cookies | Server runs on a **different machine** than the browser; and the cookie is `HttpOnly`, so page JavaScript (the web UI) cannot read it either |
| Fetch preference when both exist | **Publisher-via-proxy first**, then open-access (Unpaywall), then clean failure |
| Metadata-only records | **Out of scope** this iteration — a failed fetch reports metadata but creates **no** PDF-less record (avoids a schema/model change) |
| Spec scope | **One combined spec**: no-auth fetch + Unpaywall + EZproxy cookie + settings + CLI + web UI |
| Pipeline reuse | `ingest_file` reused; add an **identifier-hint** variant so a known DOI/arXiv id seeds resolution |
| Cookie storage | New DB **`settings`** table (WebUI-writable, survives a headless/remote server); **not** `xuewen.toml` |
| Static proxy config | `[proxy] login_url` in `xuewen.toml` (institution-agnostic; absent → paywalled fetch disabled) |

## 4. The fetch chain (`Importer::fetch_pdf`)

`parse_source` classifies the input; `fetch_pdf` then tries sources in order and
returns on the first that yields verified PDF bytes.

1. **arXiv** — inputs `arxiv.org/abs/<id>`, `arxiv.org/pdf/<id>`, `arXiv:<id>`, or
   a bare `NNNN.NNNNN` id → fetch `https://arxiv.org/pdf/<id>`. Open; no cookie.
   arXiv is always taken direct (it *is* the arXiv version), never via proxy.
2. **Known paywalled publisher + a stored cookie** — construct the publisher's
   PDF URL and fetch it **through the EZproxy prefix** carrying the cookie. This
   is the primary paywalled path and is **preferred over the OA copy** (the user
   configured the cookie specifically to get the version of record).
   - **ACM** (`10.1145/…` DOI, or a `dl.acm.org/doi/…` URL) →
     `https://dl.acm.org/doi/pdf/<doi>`. Constructible directly from the DOI.
   - **IEEE** (`ieeexplore.ieee.org/document/<arnumber>` URL) →
     `https://ieeexplore.ieee.org/stamp/stamp.jsp?arnumber=<arnumber>`. Requires
     the **arnumber**, so IEEE works from a pasted Xplore document URL. A bare
     `10.1109/…` DOI (no arnumber) is not constructible → falls through to step 3.
3. **Open-access fallback (Unpaywall)** — for any DOI, query
   `https://api.unpaywall.org/v2/<doi>?email=<contact_email>`. If it reports an OA
   location with a PDF, fetch that directly (no cookie). Catches the many
   ACM/IEEE papers that also have an arXiv/repository copy.
4. **Give up cleanly** — resolve metadata with the existing Crossref/arXiv
   resolver and return `ImportError::Unfetched { metadata }`. The caller surfaces
   a clear message ("couldn't fetch the PDF — paywalled, no OA copy, or the cookie
   expired. Title: …. Download it in your browser and drop it in the inbox"). **No
   PDF-less record is created.**

### 4.1 Verification: every fetched body must be a PDF

Each candidate fetch validates that the body begins with the `%PDF` magic marker
(the same guard the upload path uses). This is essential for the proxy path: an
expired EZproxy session returns an **HTML login page**, not a 401. Without the
magic check Xuewen would happily ingest a login page. A non-PDF body from the
proxy maps to `ImportError::CookieExpired` ("proxy returned a non-PDF, likely an
expired session — refresh your cookie").

### 4.2 Publisher registry

A small, code-defined table keyed by DOI prefix / host, each entry providing
`(pdf_url_from(source), requires_proxy: bool)`. Initial entries: arXiv (open),
ACM (proxy), IEEE (proxy). Extensible; an unrecognized host with a discoverable
DOI still gets the Unpaywall + metadata fallback.

## 5. The EZproxy cookie

### 5.1 Storage — a `settings` table

Migration `0004_add_settings.sql`:

```sql
CREATE TABLE settings (
  key        TEXT PRIMARY KEY,
  value      TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
```

`db.rs` gains `get_setting(key) -> Option<String>`, `set_setting(key, value)`,
and `delete_setting(key)`. The cookie lives under key `proxy_cookie`. It is a
rotating secret that the user updates while sitting in the browser (the web UI),
and the server may be headless/remote — both reasons it belongs in the DB rather
than in `xuewen.toml`.

### 5.2 Static proxy config — `xuewen.toml`

```toml
[proxy]
login_url = "https://proxy.uchicago.edu/login?url="
```

Optional. Absent or empty → the proxy step (4.2) is skipped entirely; arXiv,
Unpaywall, and the clean-failure path still work. Institution-agnostic: another
school's EZproxy prefix drops in unchanged. `Config` gains
`#[serde(default)] proxy: Option<ProxyConfig>`.

### 5.3 Fetch wiring

- A `reqwest` cookie jar is seeded with the stored cookie scoped to
  `.proxy.uchicago.edu`, so it rides along the `login?url=` → rewritten-host
  (`dl-acm-org.proxy.uchicago.edu`) `302` redirect chain (reqwest follows
  redirects by default).
- Target URLs are wrapped: `{login_url}{urlencoded publisher PDF URL}`.
- The stored value is the raw **`Cookie:` header string** (`name=value; name2=value2`)
  for `proxy.uchicago.edu` — obtained via a browser cookie extension or DevTools.
  Xuewen treats it as opaque and sends it verbatim on the proxied request. It is
  **never logged**.

### 5.4 Setting the cookie

- **Web:** `PUT /api/settings/proxy-cookie` `{ "cookie": "…" }` stores it;
  `DELETE /api/settings/proxy-cookie` clears it. `GET /api/settings` reports
  `{ "proxy_cookie_set": bool, "proxy_cookie_updated_at": string|null }` — it
  **never** echoes the value.
- **CLI:** `xuewen proxy-cookie --set <value>` / `--clear` / (no flag) prints
  whether one is set and when.

## 6. Backend surface

### 6.1 New module `src/import.rs`

```rust
pub enum Source { Arxiv(String), Doi(String), Url(String) }
pub fn parse_source(input: &str) -> Option<Source>;

pub enum ImportError {
    Unsupported,                         // couldn't classify the input
    CookieExpired,                       // proxy returned non-PDF
    Unfetched { metadata: Option<ResolvedMetadata> }, // no PDF anywhere
    Network(anyhow::Error),
}

pub struct Importer { /* fetch client, proxy cfg, resolver ref */ }
impl Importer {
    pub async fn fetch_pdf(&self, src: &Source, cookie: Option<&str>)
        -> Result<(bytes::Bytes, Option<Identifier>), ImportError>;
}
```

`fetch_pdf` returns the bytes **and** the identifier it derived from the input
(the DOI/arXiv id), which seeds ingest (6.3).

### 6.2 `HttpClient` gains a bytes fetch

`resolve::http::HttpClient` is today text-only (`get_text`/`send_text`, tuned for
the JSON APIs). Add a `get_bytes` / cookie-aware `send_bytes` returning
`bytes::Bytes`, reusing the existing retry/back-off machinery. Redirect-following
must be enabled on the client used for PDF fetches.

### 6.3 Identifier hint into ingest

Add `IngestCtx::ingest_file_with_hint(path, hint: Option<Identifier>)`;
`ingest_file(path)` becomes `ingest_file_with_hint(path, None)` (existing callers
unchanged). Inside `resolve_pdf`, a provided hint takes precedence over the
identifier extracted from the PDF's first page. Rationale: for a URL/DOI import we
already **know** the identifier, and some publisher PDFs don't print it on page 1
— seeding it makes metadata resolution reliable instead of best-effort.

### 6.4 New endpoint `POST /api/import`

Body `{ "input": "<url|doi|arxiv id>" }`. Flow:

1. `parse_source` → `400 {"error":"unsupported input"}` on `None`.
2. Load `proxy_cookie` from settings (may be absent).
3. `Importer::fetch_pdf` → on success, stage bytes to `_uploads` (same sanitized,
   collision-safe naming as `import_paper`) and call `ingest_file_with_hint`.
4. Map outcomes to the **existing `ImportResult` JSON** (`ingested` with
   title/status, `duplicate`, `same_work`, `in_trash`). Two new bodies:
   - `ImportError::Unfetched` → **`200 {"outcome":"unfetched","title":…,"doi":…}`**.
     This is a non-error *outcome*, not a failure — consistent with `duplicate` /
     `same_work` already being `200` — so it flows through the normal
     `ImportResult` path and the UI can render the title + "download & drop"
     hint instead of a generic error row.
   - `ImportError::CookieExpired` → `502 {"error":"proxy session expired — refresh your cookie"}`
   - `ImportError::Unsupported` → `400 {"error":"unsupported input"}`
   - `ImportError::Network` → `502 {"error":"fetch failed"}`

   The `5xx`/`400` cases are genuine failures: `importUrl` throws on them (like
   `importPaper`) and the row shows "failed: <reason>".

Guarded by `AppState.ingest` exactly like `import_paper` (`503` when `None`).
Registered as `POST /api/import`; no raised body limit is needed (the JSON body
is tiny).

### 6.5 CLI `xuewen import <input>`

A new `Command::Import { input: String }` beside `Ingest`. Reads the stored
cookie from settings, runs the same `Importer` + `ingest_file_with_hint`, and
prints the same outcome lines `Ingest` uses, plus the failure guidance for
`Unfetched` / `CookieExpired`. `Command::ProxyCookie { set, clear }` manages the
stored cookie.

## 7. Frontend

- **`ImportModal.svelte`:** add a **"paste a link / DOI / arXiv id"** text input
  above the drop-zone with an add button (and Enter-to-submit). A submitted string
  becomes a queue item that flows through the *same* sequential queue and status
  rows as file uploads, but calls `importUrl(input)` instead of `importPaper`.
  Queue items gain a `kind: 'file' | 'url'` tag and an `unfetched` status
  rendering the returned title + "download & drop in inbox" hint.
- **Collapsible "Institutional access (EZproxy cookie)" panel** in the modal: a
  paste field, a "cookie set ✓ (updated <when>) / not set" indicator, a Clear
  button, and a one-line hint on obtaining the cookie (a `proxy.uchicago.edu`
  cookies.txt export). Placed inline because that's where the user needs it.
- **`lib/api.ts`:** `importUrl(input)`, `setProxyCookie(cookie)`,
  `clearProxyCookie()`, `getSettings()`.
- **`lib/state.svelte.ts`:** `enqueueUrl(input)` mirroring `enqueueFiles`; after a
  batch, the existing `loadPapers()` + `loadStats()` refresh applies unchanged.
- **`lib/types.ts`:** extend `ImportResult` with `{ outcome: 'unfetched'; title: string | null; doi: string | null }`.

## 8. Edge cases

| Case | Behavior |
|---|---|
| Unclassifiable input | `400` "unsupported input"; UI row "failed: unsupported" |
| arXiv id/URL | Direct fetch; no cookie needed even if configured |
| ACM DOI/URL, cookie set | Proxied fetch of `dl.acm.org/doi/pdf/<doi>` |
| ACM/IEEE, no cookie, OA exists | Unpaywall PDF fetched; ingested normally |
| ACM/IEEE, no cookie, no OA | `200 {"outcome":"unfetched"}` + metadata; **no record created** |
| Bare IEEE `10.1109/…` DOI | No arnumber → try Unpaywall → else unfetched |
| Expired cookie (HTML login page) | `%PDF` check fails → `502` "refresh your cookie"; nothing ingested |
| Fetched PDF is a dup / same work / trashed | Existing `ingest_file` outcomes, surfaced as today |
| Proxy configured but `login_url` empty | Proxy step skipped; behaves as no-cookie |
| Cookie contains junk | Proxy fetch returns non-PDF → `CookieExpired` path |

## 9. Testing

### 9.1 Backend

- `parse_source` table tests: arXiv abs/pdf/`arXiv:`/bare id; ACM DOI and
  `dl.acm.org` URL; IEEE `document/<n>` URL; generic DOI; junk → `None`.
- `Importer::fetch_pdf` against `wiremock`:
  - arXiv path returns a fixture `%PDF` body → `Ok(bytes)`.
  - Proxy path: mock proxy returns HTML without the cookie and the fixture PDF
    with it → assert cookie gating and the `%PDF` verification / `CookieExpired`
    mapping.
  - Unpaywall path: mock JSON with an OA `url_for_pdf` → fetch → `Ok`.
  - No source yields a PDF → `Unfetched` carrying resolved metadata.
- `settings` round-trip in `db.rs` (`set`/`get`/`delete`).
- End-to-end `POST /api/import` via `axum-test` `TestServer` +
  `build_router_with_ingest`, using the offline-resolver trick
  (`Resolver::with_bases(None, "http://127.0.0.1:1", …)`) so the ingest side needs
  no network, and pointing the `Importer`'s fetch base at the mock: assert an
  arXiv-style import → `200 {"outcome":"ingested"}`, appears in `GET /api/papers`;
  re-import same bytes → `duplicate`.

### 9.2 Frontend

Mirror `ImportModal.test.ts` (Vitest + `@testing-library/svelte`): mock
`importUrl`, submit a URL, assert the queue row transitions
`queued → importing → ingested/unfetched/failed`; mock the settings API and assert
the cookie panel shows set/unset state and posts on save.

## 10. Security & operational notes

- The stored EZproxy cookie grants access to licensed resources under the user's
  identity. It lives in the SQLite DB file; if Xuewen runs on a VPS, that
  credential is off the user's machine. `serve` already refuses a non-loopback
  bind without `--allow-remote`; the settings endpoint inherits that posture. The
  cookie is never logged and never returned by `GET /api/settings`.
- Automated proxy fetches stay modest — one PDF per import, no crawling — to
  respect ACM/IEEE and EZproxy terms. Imports remain one-at-a-time (the web
  queue is already sequential).
- EZproxy is slated for decommissioning at UChicago (~May 2027) in favor of
  OpenAthens. This design isolates the proxy specifics to `[proxy].login_url` +
  the publisher registry + the cookie, so a future federated-auth path is a
  contained change, not a rewrite.

## 11. Out of scope (YAGNI, this iteration)

- PDF-less "metadata now, attach the PDF later, merge by DOI" records (needs
  nullable `content_hash`/`rel_path` in the model + attach logic).
- Bare-DOI IEEE resolution (DOI → landing-page scrape → arnumber).
- Browser extension / bookmarklet; OpenAthens / post-2027 auth.
- Concurrent multi-URL import; automatic cookie refresh (Duo makes it manual).
- Non-arXiv/ACM/IEEE publisher-specific PDF construction (handled only via the
  Unpaywall OA fallback).
