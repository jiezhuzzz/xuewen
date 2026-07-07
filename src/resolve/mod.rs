pub mod arxiv;
pub mod crossref;
pub mod dblp;
pub mod grobid;
pub mod http;

use crate::matching;
use crate::models::Identifier;
use anyhow::Result;
use regex::Regex;
use std::sync::LazyLock;
use std::time::Duration;

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
// Resolution is short-lived (built in resolve(), consumed immediately in build_paper);
// boxing the large variant would add churn with no measurable benefit.
#[allow(clippy::large_enum_variant)]
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

/// Fetches authoritative metadata for an identifier. A network or parse failure
/// degrades to `Resolution::Unresolved` — resolution never aborts ingestion.
pub struct Resolver {
    http: reqwest::Client,
    arxiv_base: String,
    crossref_base: String,
    dblp_base: String,
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
        Ok(Self {
            http,
            arxiv_base,
            crossref_base,
            dblp_base: "https://dblp.org".to_string(),
        })
    }

    /// Override the DBLP base URL (used by tests to point at a mock server).
    pub fn with_dblp_base(mut self, base: String) -> Self {
        self.dblp_base = base;
        self
    }

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
