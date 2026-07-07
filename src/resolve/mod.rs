pub mod arxiv;
pub mod crossref;

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
