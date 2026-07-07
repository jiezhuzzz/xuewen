use regex::Regex;
use std::sync::LazyLock;

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
