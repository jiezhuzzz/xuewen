use crate::models::Identifier;
use anyhow::{anyhow, Result};
use regex::Regex;
use std::sync::LazyLock;
use std::time::Duration;

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

/// arXiv id preceded by an explicit "arXiv:" marker or an arxiv.org abs/pdf URL.
/// The prefix is REQUIRED so a bare number sitting in prose (even prose that
/// mentions "arxiv") is not misread as an id.
static ARXIV_CONTEXT_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)(?:arxiv:\s*|arxiv\.org/(?:abs|pdf)/)(\d{4}\.\d{4,5}(?:v\d+)?)").unwrap()
});
/// A bare arXiv id occupying the whole (trimmed) input.
static ARXIV_BARE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\d{4}\.\d{4,5}(?:v\d+)?$").unwrap());
static IEEE_DOC_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)ieeexplore\.ieee\.org/document/(\d+)").unwrap());

/// Classify a pasted input into a `Source`. Order matters: IEEE document URL,
/// then any DOI, then arXiv (explicit context, else a bare whole-input id).
pub fn parse_source(input: &str) -> Option<Source> {
    let t = input.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(c) = IEEE_DOC_RE.captures(t) {
        return Some(Source::IeeeDocument(c[1].to_string()));
    }
    // A DOI anywhere (including doi.org / dl.acm.org URLs). Reuse identify's
    // extractor so the DOI pattern stays defined in one place.
    if let Some(doi) = crate::identify::extract_doi(t) {
        return Some(Source::Doi(doi));
    }
    // arXiv: an explicit "arXiv:"/arxiv.org context, else a bare id that is the
    // entire input (tolerating trailing punctuation from a paste).
    if let Some(c) = ARXIV_CONTEXT_RE.captures(t) {
        return Some(Source::Arxiv(c[1].to_string()));
    }
    let bare = t.trim_end_matches(['.', ',', ';']);
    if ARXIV_BARE_RE.is_match(bare) {
        return Some(Source::Arxiv(bare.to_string()));
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
        assert_eq!(
            parse_source("1706.03762v5"),
            Some(Source::Arxiv("1706.03762v5".into()))
        );
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
        assert_eq!(
            t.url,
            "https://ieeexplore.ieee.org/stamp/stamp.jsp?arnumber=8835311"
        );
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
        assert_eq!(
            source_identifier(&Source::IeeeDocument("8835311".into())),
            None
        );
    }

    #[test]
    fn ignores_bare_id_embedded_in_prose() {
        // "arxiv" in surrounding prose must not turn a stray number into an id.
        assert_eq!(
            parse_source("arxiv preprint, see paper 1234.5678 in the appendix"),
            None
        );
    }

    #[test]
    fn bare_arxiv_id_tolerates_trailing_punctuation() {
        assert_eq!(
            parse_source("1706.03762."),
            Some(Source::Arxiv("1706.03762".into()))
        );
    }

    #[test]
    fn doi_takes_priority_over_arxiv_context() {
        // Both an arXiv marker and a DOI present → DOI wins (checked first).
        assert_eq!(
            parse_source("arXiv:1706.03762 also 10.1145/3292500.3330701"),
            Some(Source::Doi("10.1145/3292500.3330701".into()))
        );
    }
}

/// Downloads PDF bytes. `client` follows redirects (open URLs). `no_redirect`
/// does NOT auto-follow: the proxied path drives redirects manually so it can
/// re-attach the `Cookie` header through the EZproxy chain (reqwest strips
/// `Cookie` on cross-host redirects otherwise).
pub struct Fetcher {
    client: reqwest::Client,
    no_redirect: reqwest::Client,
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
        let no_redirect = reqwest::Client::builder()
            .user_agent("xuewen/0.1")
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let proxy_host = proxy_login_url
            .as_deref()
            .and_then(|u| reqwest::Url::parse(u).ok())
            .and_then(|u| u.host_str().map(str::to_string));
        Ok(Self {
            client,
            no_redirect,
            proxy_login_url,
            proxy_host,
        })
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
    /// manually (so the cookie rides along) but only to the proxy host or its
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
                .no_redirect
                .get(&url)
                .header(reqwest::header::COOKIE, cookie)
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
            f.fetch_plain(&format!("{}/ok.pdf", server.uri()))
                .await
                .unwrap()
                .as_deref(),
            Some(PDF)
        );
        assert_eq!(
            f.fetch_plain(&format!("{}/nope", server.uri()))
                .await
                .unwrap(),
            None
        );
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

        let login = format!("{}/login?url=", server.uri());
        let f = Fetcher::new(Some(login)).unwrap();

        let target = "https://dl.acm.org/doi/pdf/10.1145/x";
        assert_eq!(
            f.fetch_proxied(target, "ezproxy=good")
                .await
                .unwrap()
                .as_deref(),
            Some(PDF)
        );
        // Wrong cookie → non-PDF body → None (caller maps to CookieExpired).
        assert_eq!(
            f.fetch_proxied(target, "ezproxy=stale").await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn fetch_proxied_follows_redirect_reattaching_cookie() {
        let server = MockServer::start().await;
        // Hop 1: the login URL redirects (302) to a rewritten path.
        Mock::given(method("GET"))
            .and(path("/login"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", "/rewritten/paper.pdf"),
            )
            .mount(&server)
            .await;
        // Hop 2: the rewritten path serves the PDF ONLY when the cookie rode along
        // (proves the manual redirect loop re-attaches the Cookie header).
        Mock::given(method("GET"))
            .and(path("/rewritten/paper.pdf"))
            .and(header("cookie", "ezproxy=good"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(PDF))
            .mount(&server)
            .await;
        let f = Fetcher::new(Some(format!("{}/login?url=", server.uri()))).unwrap();
        assert_eq!(
            f.fetch_proxied("https://dl.acm.org/doi/pdf/10.1145/x", "ezproxy=good")
                .await
                .unwrap()
                .as_deref(),
            Some(PDF)
        );
    }
}
