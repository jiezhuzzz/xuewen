use crate::models::Identifier;
use regex::Regex;
use std::sync::LazyLock;

static DOI_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"10\.\d{4,9}/[-._;()/:A-Za-z0-9]+").unwrap());
static ARXIV_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?i)arxiv:\s*(\d{4}\.\d{4,5}(?:v\d+)?)").unwrap());

pub fn extract_doi(text: &str) -> Option<String> {
    DOI_RE.find(text).map(|m| {
        m.as_str()
            .trim_end_matches(['.', ',', ')', ';'])
            .to_string()
    })
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

/// Words that indicate a line ended mid-phrase (a wrapped title).
const JOIN_WORDS: &[&str] = &[
    "a", "an", "and", "by", "for", "from", "in", "of", "on", "or", "the", "to", "via", "with",
];

/// A line usable as (part of) a title under the existing filter rules.
fn substantive(line: &str) -> Option<&str> {
    let t = line.trim();
    (t.len() >= 8
        && !t.to_lowercase().starts_with("arxiv")
        && !t.contains('@')
        && !DOI_RE.is_match(t)
        && t.chars().any(|c| c.is_alphabetic()))
    .then_some(t)
}

/// Whether a title line ends "mid-phrase" (wrapped onto the next line).
fn ends_mid_phrase(line: &str) -> bool {
    if line.ends_with(':') || line.ends_with('-') {
        return true;
    }
    line.rsplit(|c: char| !c.is_alphanumeric())
        .find(|w| !w.is_empty())
        .is_some_and(|w| JOIN_WORDS.contains(&w.to_lowercase().as_str()))
}

/// Best-effort provisional title: the first substantive line of the header
/// text, joined with the next line when the first ends mid-phrase (wrapped
/// two-line titles are common on conference cover sheets).
pub fn guess_title(text: &str) -> Option<String> {
    // `loop`+`next()` rather than `while let` (clippy::while_let_on_iterator):
    // the join branch needs to pull a second line from the same iterator.
    let mut lines = text.lines();
    loop {
        let first = match lines.next() {
            Some(line) => match substantive(line) {
                Some(t) => t,
                None => continue,
            },
            None => return None,
        };
        if ends_mid_phrase(first) {
            if let Some(next) = lines.next().and_then(substantive) {
                if let Some(stem) = first.strip_suffix('-') {
                    // A trailing '-' is a mid-word split ("Hyperdimen-" / "sional") only
                    // when both sides are lowercase; otherwise it's a hyphenated compound
                    // wrapped at the hyphen ("State-of-the-" / "Art") — keep the hyphen.
                    let mid_word = stem.chars().last().is_some_and(|c| c.is_lowercase())
                        && next.chars().next().is_some_and(|c| c.is_lowercase());
                    return Some(if mid_word {
                        format!("{stem}{next}")
                    } else {
                        format!("{first}{next}")
                    });
                }
                return Some(format!("{first} {next}"));
            }
        }
        return Some(first.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_doi() {
        let text = "See https://doi.org/10.1145/3292500.3330701 for details.";
        assert_eq!(
            extract_doi(text).as_deref(),
            Some("10.1145/3292500.3330701")
        );
    }

    #[test]
    fn finds_arxiv() {
        assert_eq!(
            extract_arxiv("arXiv:1706.03762v5").as_deref(),
            Some("1706.03762v5")
        );
        assert_eq!(
            extract_arxiv("arXiv: 2001.00001").as_deref(),
            Some("2001.00001")
        );
    }

    #[test]
    fn doi_wins_over_arxiv() {
        let text = "arXiv:1706.03762  doi:10.1145/3292500.3330701";
        assert_eq!(
            identify(text),
            Identifier::Doi("10.1145/3292500.3330701".into())
        );
    }

    #[test]
    fn no_identifier() {
        assert_eq!(identify("Just some prose with no ids."), Identifier::None);
    }

    #[test]
    fn guesses_title_skipping_arxiv_banner() {
        let text =
            "arXiv:1706.03762v5 [cs.CL] 6 Dec 2017\nAttention Is All You Need\nAshish Vaswani";
        assert_eq!(
            guess_title(text).as_deref(),
            Some("Attention Is All You Need")
        );
    }

    #[test]
    fn joins_wrapped_title_ending_mid_phrase() {
        // Exact shape of the motivating USENIX cover sheet.
        let text = "AntiFuzz: Impeding Fuzzing Audits of\nBinary Executables\nEmre Güler, Cornelius Aschermann, Ali Abbasi, and Thorsten Holz,\nRuhr-Universität Bochum";
        assert_eq!(
            guess_title(text).as_deref(),
            Some("AntiFuzz: Impeding Fuzzing Audits of Binary Executables")
        );
    }

    #[test]
    fn joins_after_trailing_colon_and_dehyphenates() {
        assert_eq!(
            guess_title("Some System:\nA Grand Unified Theory\nAuthor Name").as_deref(),
            Some("Some System: A Grand Unified Theory")
        );
        assert_eq!(
            guess_title("Hyperdimen-\nsional Computing Systems\nAuthor Name").as_deref(),
            Some("Hyperdimensional Computing Systems")
        );
        assert_eq!(
            guess_title("Towards State-of-the-\nArt Adversarial Robustness\nAuthor Name")
                .as_deref(),
            Some("Towards State-of-the-Art Adversarial Robustness")
        );
    }

    #[test]
    fn does_not_join_complete_titles() {
        // First line doesn't end mid-phrase: next line (authors) is not joined.
        let text = "Attention Is All You Need\nAshish Vaswani, Noam Shazeer";
        assert_eq!(
            guess_title(text).as_deref(),
            Some("Attention Is All You Need")
        );
    }

    #[test]
    fn does_not_join_when_next_line_is_not_substantive() {
        // Next line carries an email -> not substantive -> no join.
        let text = "A Study of\nauthor@example.com things\nReal Second Line";
        assert_eq!(guess_title(text).as_deref(), Some("A Study of"));
    }
}
