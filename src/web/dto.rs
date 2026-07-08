use serde::Serialize;

use crate::models::{Paper, PaperStatus};

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
            title: p.title.clone(),
            authors: p.authors.0.clone(),
            venue: p.venue.clone(),
            year: p.year,
            doi: p.doi.clone(),
            arxiv_id: p.arxiv_id.clone(),
            dblp_key: p.dblp_key.clone(),
            cite_key: p.cite_key.clone(),
            url: p.url.clone(),
            source: p.source.clone(),
            status: p.status,
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
            abstract_text: p.abstract_text.clone(),
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
