pub mod arxiv;
pub mod crossref;
pub mod dblp;
pub mod grobid;
pub mod http;
pub mod unpaywall;

use crate::matching;
use crate::models::Identifier;
use anyhow::Result;
use regex::Regex;
use std::sync::LazyLock;
use std::time::Duration;

use self::http::{HttpClient, RetryPolicy};

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

/// Collapse all runs of whitespace to single spaces and trim.
pub(crate) fn collapse_ws(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<[^>]+>").unwrap());

/// Strip XML/HTML tags (e.g. Crossref JATS `<jats:p>`) and collapse whitespace.
pub(crate) fn strip_tags(s: &str) -> String {
    collapse_ws(&TAG_RE.replace_all(s, " "))
}

/// Fetches authoritative metadata for an identifier. A network or parse failure
/// degrades to `None` — resolution never aborts ingestion.
pub struct Resolver {
    http: HttpClient,
    arxiv_base: String,
    crossref_base: String,
    dblp_base: String,
    email: Option<String>,
    unpaywall_base: String,
}

impl Resolver {
    /// Build a resolver pointing at the real arXiv and Crossref endpoints, with a
    /// polite retry/back-off policy.
    pub fn new(contact_email: Option<&str>) -> Result<Self> {
        Self::new_with_policy(contact_email, RetryPolicy::production())
    }

    /// Build a resolver for the real endpoints with an explicit retry policy.
    pub fn new_with_policy(contact_email: Option<&str>, retry: RetryPolicy) -> Result<Self> {
        Self::build(
            contact_email,
            "https://export.arxiv.org".to_string(),
            "https://api.crossref.org".to_string(),
            retry,
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
            email: contact_email.map(str::to_string),
            unpaywall_base: "https://api.unpaywall.org".to_string(),
        })
    }

    /// Override the DBLP base URL (used by tests to point at a mock server).
    pub fn with_dblp_base(mut self, base: String) -> Self {
        self.dblp_base = base;
        self
    }

    /// Override the Unpaywall base URL (used by tests to point at a mock server).
    pub fn with_unpaywall_base(mut self, base: String) -> Self {
        self.unpaywall_base = base;
        self
    }

    /// Route an identifier to its source and return the metadata, or `None` when
    /// nothing resolves confidently. For a PDF with no identifier, `title_hint`
    /// drives a DBLP/Crossref title search.
    pub async fn resolve(
        &self,
        ident: &Identifier,
        title_hint: Option<&str>,
    ) -> Option<ResolvedMetadata> {
        match ident {
            Identifier::Arxiv(id) => self.try_arxiv(id).await,
            Identifier::Doi(doi) => self.try_crossref(doi).await,
            Identifier::None => self.try_title_search(title_hint).await,
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
}

/// Most candidates a manual-identify search returns.
const MAX_CANDIDATES: usize = 8;

impl Resolver {
    /// Title-search candidates from DBLP then Crossref, WITHOUT the
    /// confidence gate: the caller (a human picking a match) is the gate.
    /// Deduped, ranked by similarity to `query`, capped at `MAX_CANDIDATES`.
    /// Source failures degrade to fewer (possibly zero) candidates.
    pub async fn search_candidates(&self, query: &str) -> Vec<ResolvedMetadata> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        let mut cands = Vec::new();
        let (dblp, crossref) = tokio::join!(
            self.fetch_parse_dblp(query),
            self.fetch_parse_crossref_search(query)
        );
        match dblp {
            Ok(c) => cands.extend(c),
            Err(e) => tracing::warn!("dblp candidate search failed for {query:?}: {e}"),
        }
        match crossref {
            Ok(c) => cands.extend(c),
            Err(e) => tracing::warn!("crossref candidate search failed for {query:?}: {e}"),
        }
        rank_candidates(query, cands)
    }
}

/// Dedup (by lowercased DOI, else DBLP key; first occurrence wins), rank by
/// title similarity to `query` (untitled candidates sink), cap the list.
fn rank_candidates(query: &str, cands: Vec<ResolvedMetadata>) -> Vec<ResolvedMetadata> {
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<ResolvedMetadata> = Vec::new();
    for c in cands {
        let key = c
            .doi
            .as_deref()
            .map(|d| format!("doi:{}", d.to_lowercase()))
            .or_else(|| c.dblp_key.as_deref().map(|k| format!("dblp:{k}")));
        if let Some(key) = key {
            if !seen.insert(key) {
                continue;
            }
        }
        out.push(c);
    }
    out.sort_by(|a, b| {
        let score = |c: &ResolvedMetadata| {
            c.title
                .as_deref()
                .map(|t| matching::title_similarity(query, t))
                .unwrap_or(-1.0)
        };
        score(b)
            .partial_cmp(&score(a))
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(MAX_CANDIDATES);
    out
}

/// Pick the highest-similarity candidate whose title confidently matches `query`.
fn best_match(query: &str, candidates: Vec<ResolvedMetadata>) -> Option<ResolvedMetadata> {
    let mut best: Option<(f64, ResolvedMetadata)> = None;
    for c in candidates {
        let score = match c.title.as_deref() {
            Some(t) => matching::title_similarity(query, t),
            None => continue,
        };
        if score >= matching::MATCH_THRESHOLD && best.as_ref().is_none_or(|(bs, _)| score > *bs) {
            best = Some((score, c));
        }
    }
    best.map(|(_, c)| c)
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

    fn cand(title: &str, doi: Option<&str>, dblp_key: Option<&str>) -> ResolvedMetadata {
        ResolvedMetadata {
            title: Some(title.to_string()),
            doi: doi.map(str::to_string),
            dblp_key: dblp_key.map(str::to_string),
            source: "test".to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn rank_candidates_sorts_dedups_and_caps() {
        let query = "AntiFuzz: Impeding Fuzzing Audits of Binary Executables";
        let mut cands = vec![
            cand("Something Unrelated Entirely", None, Some("conf/x/1")),
            cand(query, Some("10.1/af"), Some("conf/uss/GulerAAH19")),
            // Same DOI from the other source: deduped, first occurrence wins.
            cand(query, Some("10.1/AF"), None),
        ];
        // Pad with distinct filler beyond the cap.
        for i in 0..10 {
            cands.push(cand(&format!("Filler Paper Number {i}"), None, None));
        }
        let ranked = rank_candidates(query, cands);
        assert_eq!(ranked.len(), 8); // capped
                                     // Exact-title match ranks first; its DOI-duplicate is gone.
        assert_eq!(ranked[0].dblp_key.as_deref(), Some("conf/uss/GulerAAH19"));
        assert_eq!(
            ranked.iter().filter(|c| c.doi.is_some()).count(),
            1,
            "case-insensitive DOI dedup"
        );
    }

    #[test]
    fn rank_candidates_keeps_untitled_last_and_handles_empty() {
        assert!(rank_candidates("query", Vec::new()).is_empty());
        let ranked = rank_candidates(
            "Deep Residual Learning",
            vec![
                ResolvedMetadata {
                    source: "test".into(),
                    ..Default::default()
                }, // no title
                cand("Deep Residual Learning", None, None),
            ],
        );
        assert_eq!(ranked[0].title.as_deref(), Some("Deep Residual Learning"));
    }

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
}
