use crate::models::Identifier;
use regex::Regex;
use std::sync::LazyLock;

static DOI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"10\.\d{4,9}/[-._;()/:A-Za-z0-9]+").unwrap());
static ARXIV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)arxiv:\s*(\d{4}\.\d{4,5}(?:v\d+)?)").unwrap());

pub fn extract_doi(text: &str) -> Option<String> {
    DOI_RE
        .find(text)
        .map(|m| m.as_str().trim_end_matches(['.', ',', ')', ';']).to_string())
}

pub fn extract_arxiv(text: &str) -> Option<String> {
    ARXIV_RE.captures(text).map(|c| c[1].to_string())
}

/// Prefer a DOI (published record) over an arXiv id (preprint) when both appear.
pub fn identify(text: &str) -> Identifier {
    if let Some(doi) = extract_doi(text) {
        return Identifier::Doi(doi);
    }
    if let Some(id) = extract_arxiv(text) {
        return Identifier::Arxiv(id);
    }
    Identifier::None
}

/// Best-effort provisional title: the first substantive line of the header text.
/// Overwritten by authoritative metadata in Plan 2.
pub fn guess_title(text: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim();
        if t.len() >= 8
            && !t.to_lowercase().starts_with("arxiv")
            && !t.contains('@')
            && !DOI_RE.is_match(t)
            && t.chars().any(|c| c.is_alphabetic())
        {
            return Some(t.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_doi() {
        let text = "See https://doi.org/10.1145/3292500.3330701 for details.";
        assert_eq!(extract_doi(text).as_deref(), Some("10.1145/3292500.3330701"));
    }

    #[test]
    fn finds_arxiv() {
        assert_eq!(extract_arxiv("arXiv:1706.03762v5").as_deref(), Some("1706.03762v5"));
        assert_eq!(extract_arxiv("arXiv: 2001.00001").as_deref(), Some("2001.00001"));
    }

    #[test]
    fn doi_wins_over_arxiv() {
        let text = "arXiv:1706.03762  doi:10.1145/3292500.3330701";
        assert_eq!(identify(text), Identifier::Doi("10.1145/3292500.3330701".into()));
    }

    #[test]
    fn no_identifier() {
        assert_eq!(identify("Just some prose with no ids."), Identifier::None);
    }

    #[test]
    fn guesses_title_skipping_arxiv_banner() {
        let text = "arXiv:1706.03762v5 [cs.CL] 6 Dec 2017\nAttention Is All You Need\nAshish Vaswani";
        assert_eq!(guess_title(text).as_deref(), Some("Attention Is All You Need"));
    }
}
