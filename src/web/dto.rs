use serde::Serialize;

use crate::models::{Paper, PaperStatus};
use crate::resolve::ResolvedMetadata;

/// A paper for the list view (no abstract, to keep the payload light).
#[derive(Serialize)]
pub struct PaperSummary {
    pub id: String,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub cite_key: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
    pub status: PaperStatus,
    pub added_at: String,
}

impl From<&Paper> for PaperSummary {
    fn from(p: &Paper) -> Self {
        Self {
            id: p.id.clone(),
            title: p.meta.title.clone(),
            authors: p.meta.authors.0.clone(),
            venue: p.meta.venue.clone(),
            year: p.meta.year,
            doi: p.meta.doi.clone(),
            arxiv_id: p.meta.arxiv_id.clone(),
            dblp_key: p.meta.dblp_key.clone(),
            cite_key: p.cite_key.clone(),
            url: p.meta.url.clone(),
            source: p.meta.source.clone(),
            status: p.meta.status,
            added_at: p.added_at.clone(),
        }
    }
}

/// A paper for the detail view: the summary fields plus the abstract.
#[derive(Serialize)]
pub struct PaperDetail {
    #[serde(flatten)]
    pub summary: PaperSummary,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
}

impl From<&Paper> for PaperDetail {
    fn from(p: &Paper) -> Self {
        Self {
            summary: PaperSummary::from(p),
            abstract_text: p.meta.abstract_text.clone(),
        }
    }
}

/// Library counts for the header.
#[derive(Serialize)]
pub struct Stats {
    pub total: usize,
    pub resolved: usize,
    pub needs_review: usize,
}

/// A manual-identify candidate: a lossless wire mirror of `ResolvedMetadata`
/// (round-trips through POST /api/papers/{id}/identify without loss).
#[derive(Serialize, serde::Deserialize)]
pub struct Candidate {
    pub title: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub url: Option<String>,
    pub source: String,
}

impl From<&ResolvedMetadata> for Candidate {
    fn from(m: &ResolvedMetadata) -> Self {
        Self {
            title: m.title.clone(),
            abstract_text: m.abstract_text.clone(),
            authors: m.authors.clone(),
            venue: m.venue.clone(),
            year: m.year,
            doi: m.doi.clone(),
            arxiv_id: m.arxiv_id.clone(),
            dblp_key: m.dblp_key.clone(),
            url: m.url.clone(),
            source: m.source.clone(),
        }
    }
}

impl Candidate {
    /// Back to resolver metadata for the apply path.
    // Wired up by POST /api/papers/{id}/identify (Task 4); unused until then.
    #[allow(dead_code)]
    pub fn into_metadata(self) -> ResolvedMetadata {
        ResolvedMetadata {
            title: self.title,
            abstract_text: self.abstract_text,
            authors: self.authors,
            venue: self.venue,
            year: self.year,
            doi: self.doi,
            arxiv_id: self.arxiv_id,
            dblp_key: self.dblp_key,
            url: self.url,
            source: self.source,
        }
    }
}
