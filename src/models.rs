use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identifier {
    Doi(String),
    Arxiv(String),
    None,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperStatus {
    Resolved,
    NeedsReview,
}

impl PaperStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            PaperStatus::Resolved => "resolved",
            PaperStatus::NeedsReview => "needs_review",
        }
    }
}

/// A stored bibliographic record. Column names match `migrations/0001_init.sql`.
#[derive(Debug, Clone, Serialize, sqlx::FromRow)]
pub struct Paper {
    pub id: String,
    pub content_hash: String,
    pub rel_path: String,
    pub title: Option<String>,
    #[sqlx(rename = "abstract")]
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub authors: Option<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
    pub status: String,
    pub added_at: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_strings_match_schema() {
        assert_eq!(PaperStatus::Resolved.as_str(), "resolved");
        assert_eq!(PaperStatus::NeedsReview.as_str(), "needs_review");
    }

    #[test]
    fn identifier_equality() {
        assert_eq!(Identifier::Doi("10.1/x".into()), Identifier::Doi("10.1/x".into()));
        assert_ne!(Identifier::Doi("10.1/x".into()), Identifier::None);
    }
}
