//! Pattern-matching bibliography parser: the free path in front of the LLM.
//!
//! One bibliography = one style (the camera-ready template fixes the
//! BibTeX style), so the style is detected once per paper by voting across
//! entries, then every entry is parsed with that style's grammar and
//! strictly validated — a false None costs one LLM entry; a false Some
//! shows a wrong popover.

use regex::Regex;
use std::sync::LazyLock;

/// `[12]` / `12.` entry markers left on entries by the frontend extraction.
/// The `12.` form requires trailing whitespace so "1.5 Gbps…" survives.
static MARKER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(?:\[\d{1,3}\]\s*|\d{1,3}\.\s+)").unwrap());

pub(super) fn strip_marker(s: &str) -> &str {
    let t = s.trim();
    match MARKER_RE.find(t) {
        Some(m) => t[m.end()..].trim_start(),
        None => t,
    }
}

/// Style-independent fields extractable from any entry.
pub(super) struct UniversalFields {
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub url: Option<String>,
}

static URL_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"https?://[^\s<>]+").unwrap());
static DOI_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b10\.\d{4,9}/[^\s<>]+").unwrap());
static ARXIV_NEW_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(\d{4}\.\d{4,5})(?:v\d+)?\b").unwrap());
static ARXIV_OLD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b([a-z-]+(?:\.[A-Z]{2})?/\d{7})\b").unwrap());
static YEAR_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b(?:19|20)\d{2}\b").unwrap());
static RANGE_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d+\s*[-–—]\s*\d+").unwrap());

pub(super) fn universal_fields(entry: &str) -> UniversalFields {
    let trim_trail = |s: &str| s.trim_end_matches(['.', ',', ';', ')']).to_string();
    let url = URL_RE.find(entry).map(|m| trim_trail(m.as_str()));
    let doi = DOI_RE.find(entry).map(|m| trim_trail(m.as_str()));
    // arXiv entries always say "arXiv"; a bare NNNN.NNNNN elsewhere is not
    // evidence enough (DOI suffixes and decimals produce lookalikes).
    let arxiv_id = if entry.to_lowercase().contains("arxiv") {
        ARXIV_NEW_RE
            .captures(entry)
            .map(|c| c[1].to_string())
            .or_else(|| ARXIV_OLD_RE.captures(entry).map(|c| c[1].to_string()))
    } else {
        None
    };
    // Year: search a copy with URLs/DOIs/arXiv-ids/number-ranges blanked
    // (page ranges like 1993–2008 read as years) and keep the LAST in-range
    // hit — years sit at the entry tail in every style whose grammar doesn't
    // extract its own (IEEE, plain).
    let mut cleaned = entry.to_string();
    for re in [&*URL_RE, &*DOI_RE, &*ARXIV_NEW_RE, &*RANGE_RE] {
        cleaned = re.replace_all(&cleaned, " ").into_owned();
    }
    let year = YEAR_RE
        .find_iter(&cleaned)
        .filter_map(|m| m.as_str().parse::<i64>().ok())
        .filter(|y| (1900..=2035).contains(y))
        .last();
    UniversalFields {
        year,
        doi,
        arxiv_id,
        url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_bracket_and_dot_markers() {
        assert_eq!(strip_marker("[12] D. Kingma"), "D. Kingma");
        assert_eq!(strip_marker("  3.  Smith, J."), "Smith, J.");
        assert_eq!(strip_marker("D. Kingma"), "D. Kingma"); // "D." is not a marker
        assert_eq!(strip_marker("1.5 Gbps links"), "1.5 Gbps links");
        assert_eq!(strip_marker("[0, 1] interval"), "[0, 1] interval");
    }

    #[test]
    fn extracts_universal_fields() {
        let u = universal_fields(
            "D. Kingma and J. Ba, \"Adam,\" arXiv preprint arXiv:1412.6980v9, 2015. \
             https://doi.org/10.48550/arXiv.1412.6980",
        );
        assert_eq!(u.year, Some(2015));
        assert_eq!(u.arxiv_id.as_deref(), Some("1412.6980"));
        assert_eq!(u.doi.as_deref(), Some("10.48550/arXiv.1412.6980"));
        assert_eq!(
            u.url.as_deref(),
            Some("https://doi.org/10.48550/arXiv.1412.6980")
        );
    }

    #[test]
    fn year_ignores_page_ranges_and_ids() {
        // Page range 1993–2008 must not read as a year; real year is 2020.
        let u = universal_fields("A. Author. Title. In CCS, 2020, pp. 1993–2008.");
        assert_eq!(u.year, Some(2020));
        // No "arxiv" marker → NNNN.NNNNN is not an arXiv id, nor a year.
        let u2 = universal_fields("A. Author. Title with 2004.12345 constant. VLDB, 2019.");
        assert_eq!(u2.arxiv_id, None);
        assert_eq!(u2.year, Some(2019));
        // Old-style id.
        let u3 = universal_fields("S. Aaronson. Limits. arXiv:quant-ph/0502072, 2005.");
        assert_eq!(u3.arxiv_id.as_deref(), Some("quant-ph/0502072"));
    }

    #[test]
    fn universal_fields_absent_when_absent() {
        let u = universal_fields("garbage entry with no fields");
        assert!(u.year.is_none() && u.doi.is_none() && u.arxiv_id.is_none() && u.url.is_none());
    }
}
