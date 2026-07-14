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

/// Word endings that do NOT close a sentence even before ". ".
// "al" is intentionally absent: the period after "et al." genuinely ends an author segment.
const ABBREVS: &[&str] = &[
    "proc", "conf", "symp", "int", "intl", "trans", "vol", "no", "pp", "ed", "eds", "univ", "dept",
    "rev", "jr", "sr", "st",
];

/// "J." (initials, incl. "D.P"), "(J" and known abbreviations don't end a sentence.
fn is_initial_or_abbrev(word: &str) -> bool {
    let w = word.trim_start_matches(['(', '[', '"', '\u{201C}', '\u{201D}', '\'']);
    let alpha = w.chars().filter(|c| c.is_alphabetic()).count();
    if alpha == 0 {
        return false;
    }
    let all_upper = w
        .chars()
        .filter(|c| c.is_alphabetic())
        .all(|c| c.is_uppercase());
    if all_upper && alpha <= 3 && w.chars().count() <= 5 {
        return true;
    }
    ABBREVS.contains(&w.trim_end_matches('.').to_ascii_lowercase().as_str())
}

/// Split at ". " boundaries that actually end a sentence.
pub(super) fn split_sentences(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, c) in s.char_indices() {
        if c != '.' {
            continue;
        }
        if !s[i + 1..].chars().next().is_none_or(|c| c.is_whitespace()) {
            continue; // mid-token period: DOI, URL, "1.5"
        }
        let prev = s[start..i].split_whitespace().last().unwrap_or("");
        if is_initial_or_abbrev(prev) {
            continue;
        }
        let seg = s[start..i].trim();
        if !seg.is_empty() {
            out.push(seg.to_string());
        }
        start = i + 1;
    }
    let tail = s[start..].trim().trim_end_matches('.').trim();
    if !tail.is_empty() {
        out.push(tail.to_string());
    }
    out
}

/// "A and B", "A, B, and C", "A & B" → individual names; "et al." dropped.
pub(super) fn split_authors(s: &str) -> Vec<String> {
    s.replace(" and ", ", ")
        .replace(" & ", ", ")
        .split(',')
        .map(str::trim)
        .filter(|t| !t.is_empty() && !t.trim_end_matches('.').eq_ignore_ascii_case("et al"))
        .map(String::from)
        .collect()
}

/// Strict name shape: uppercase present, sane length, no digits/parens.
pub(super) fn looks_like_name(a: &str) -> bool {
    let n = a.chars().count();
    (2..=60).contains(&n)
        && a.chars().any(|c| c.is_uppercase())
        && !a
            .chars()
            .any(|c| c.is_ascii_digit() || c == '(' || c == ')')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Style {
    Ieee = 0,
    Acm = 1,
    Lncs = 2,
    Plain = 3,
}

/// IEEE titles are quoted (straight or curly).
pub(super) static QUOTED_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#"[""]([^"""]{4,}?)[""]"#).unwrap());
/// ACM: leading author segment, then ". YYYY. ", then the rest.
pub(super) static ACM_HEAD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)^(.{3,300}?)\.\s+((19|20)\d{2})\.\s+(.+)$").unwrap());
/// LNCS entries start "Lastname, F." — full signature also needs the ".:"
/// that closes the author block.
static LNCS_START_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\p{Lu}[\p{L}''-]+,\s*\p{Lu}\.").unwrap());

fn style_of_entry(e: &str) -> Style {
    if QUOTED_RE.is_match(e) {
        Style::Ieee
    } else if LNCS_START_RE.is_match(e) && e.contains(".:") {
        Style::Lncs
    } else if ACM_HEAD_RE.is_match(e) {
        Style::Acm
    } else {
        Style::Plain
    }
}

/// Publisher family inferred from the paper's venue string — a prior, used
/// only when the content vote is ambiguous. Deliberately not a maintained
/// venue map: four substring rules cover the publisher families.
fn venue_family_style(venue: &str) -> Option<Style> {
    let v = venue.to_ascii_lowercase();
    if v.contains("acm") {
        Some(Style::Acm)
    } else if v.contains("ieee") {
        Some(Style::Ieee)
    } else if v.contains("usenix") {
        Some(Style::Plain)
    } else if v.contains("springer") || v.contains("lncs") || v.contains("lecture notes") {
        Some(Style::Lncs)
    } else {
        None
    }
}

/// One bibliography = one style: vote across entries, ≥60% wins; otherwise
/// the venue family decides; otherwise None (every entry goes to the LLM).
pub(super) fn detect_style(entries: &[&str], venue: Option<&str>) -> Option<Style> {
    if entries.is_empty() {
        return None;
    }
    let mut counts = [0usize; 4];
    for e in entries {
        counts[style_of_entry(e) as usize] += 1;
    }
    let styles = [Style::Ieee, Style::Acm, Style::Lncs, Style::Plain];
    let (best, &max) = counts.iter().enumerate().max_by_key(|(_, c)| **c).unwrap();
    if max * 10 >= entries.len() * 6 {
        return Some(styles[best]);
    }
    venue.and_then(venue_family_style)
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

    #[test]
    fn split_sentences_respects_initials_and_abbrevs() {
        // "D." and "J." are initials, "Ba." ends the author sentence.
        assert_eq!(
            split_sentences("D. P. Kingma and J. Ba. Adam: A method. ICLR, 2015"),
            vec!["D. P. Kingma and J. Ba", "Adam: A method", "ICLR, 2015"]
        );
        // "Proc." and "vol." don't split; "et al." does — it genuinely ends the author list.
        assert_eq!(
            split_sentences("J. Smith et al. Title here. In Proc. of CCS, vol. 3, 2020"),
            vec![
                "J. Smith et al",
                "Title here",
                "In Proc. of CCS, vol. 3, 2020"
            ]
        );
        // Periods not followed by whitespace (DOIs, URLs) never split.
        assert_eq!(
            split_sentences("See 10.1145/1234.5678 now"),
            vec!["See 10.1145/1234.5678 now"]
        );
    }

    #[test]
    fn curly_quote_before_abbrev_does_not_split() {
        // A leading curly quote before an abbreviation must not cause a split.
        assert_eq!(
            split_sentences("See \u{201C}Proc. of CCS\u{201D} for details. Second sentence"),
            vec![
                "See \u{201C}Proc. of CCS\u{201D} for details",
                "Second sentence"
            ]
        );
        // Same for a straight double quote.
        assert_eq!(
            split_sentences("See \"Proc. of CCS\" for details. Second sentence"),
            vec!["See \"Proc. of CCS\" for details", "Second sentence"]
        );
    }

    #[test]
    fn split_authors_handles_and_ampersand_etal() {
        assert_eq!(
            split_authors("D. Kingma and J. Ba"),
            vec!["D. Kingma", "J. Ba"]
        );
        assert_eq!(
            split_authors("Martín Abadi, Andy Chu, and Li Zhang"),
            vec!["Martín Abadi", "Andy Chu", "Li Zhang"]
        );
        assert_eq!(split_authors("A. One & B. Two"), vec!["A. One", "B. Two"]);
        assert_eq!(split_authors("J. Smith, et al."), vec!["J. Smith"]);
    }

    #[test]
    fn name_shape_rejects_junk() {
        assert!(looks_like_name("D. Kingma"));
        assert!(looks_like_name("Martín Abadi"));
        assert!(looks_like_name("KINGMA, D")); // small-caps extraction
        assert!(!looks_like_name("J. (2015)")); // natbib year glued to initial
        assert!(!looks_like_name("x"));
        assert!(!looks_like_name("3rd Workshop"));
        assert!(!looks_like_name("proceedings of the acm")); // no uppercase
    }

    #[test]
    fn detects_style_by_majority_vote() {
        let ieee = vec![
            r#"K. Kim and T. Kim, "PGFUZZ: Policy-guided fuzzing," in Proc. of NDSS, 2021."#,
            r#"D. Kingma and J. Ba, "Adam: A method for stochastic optimization," in Proc. of ICLR, 2015."#,
            "garbled entry without a quote 2020",
        ];
        assert_eq!(detect_style(&ieee, None), Some(Style::Ieee)); // 2/3 ≥ 60%

        let acm = vec![
            "Martín Abadi and Andy Chu. 2016. Deep Learning with Differential Privacy. In CCS.",
            "Jane Doe. 2019. Another Paper Title. In SOSP.",
        ];
        assert_eq!(detect_style(&acm, None), Some(Style::Acm));

        let lncs = vec![
            "Ateniese, G., Magri, B.: Redactable blockchain. In: EuroS&P, pp. 111–126. IEEE (2017)",
            "Kingma, D., Ba, J.: Adam. In: ICLR (2015)",
        ];
        assert_eq!(detect_style(&lncs, None), Some(Style::Lncs));

        let plain = vec![
            "D. Kingma and J. Ba. Adam: A method. ICLR, 2015.",
            "J. Smith. A systems paper. In Proc. of OSDI, 2020.",
        ];
        assert_eq!(detect_style(&plain, None), Some(Style::Plain));
    }

    #[test]
    fn ambiguous_vote_falls_back_to_venue_family() {
        // 1 IEEE + 1 ACM + 1 LNCS: no style reaches 60%.
        let mixed = vec![
            r#"A. B, "Quoted title here," in Proc. X, 2020."#,
            "Jane Doe. 2019. Some Title. In SOSP.",
            "Roe, R.: Colon Title. In: S&P (2018)",
        ];
        assert_eq!(detect_style(&mixed, None), None);
        assert_eq!(
            detect_style(
                &mixed,
                Some("2021 IEEE Symposium on Security and Privacy (SP)")
            ),
            Some(Style::Ieee)
        );
        assert_eq!(
            detect_style(&mixed, Some("Proceedings of the ACM SIGSAC CCS")),
            Some(Style::Acm)
        );
        assert_eq!(
            detect_style(&mixed, Some("USENIX Security Symposium")),
            Some(Style::Plain)
        );
        assert_eq!(
            detect_style(&mixed, Some("ESORICS, Lecture Notes in CS")),
            Some(Style::Lncs)
        );
        assert_eq!(detect_style(&mixed, Some("Journal of Cryptology")), None);
    }

    #[test]
    fn empty_entries_detect_nothing() {
        assert_eq!(detect_style(&[], None), None);
        assert_eq!(detect_style(&[], Some("ACM CCS")), None);
    }
}
