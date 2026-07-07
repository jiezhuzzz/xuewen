use strsim::normalized_levenshtein;

/// Similarity (0.0–1.0) at or above which a candidate title is accepted as a match.
pub const MATCH_THRESHOLD: f64 = 0.85;

/// Lowercase, replace every non-alphanumeric char with a space, and collapse whitespace.
pub fn normalize_title(s: &str) -> String {
    let spaced: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .to_lowercase();
    spaced.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Normalized-Levenshtein similarity of two titles after normalization.
pub fn title_similarity(a: &str, b: &str) -> f64 {
    normalized_levenshtein(&normalize_title(a), &normalize_title(b))
}

/// Whether `candidate` is a confident title match for `query`.
pub fn is_confident_match(query: &str, candidate: &str) -> bool {
    title_similarity(query, candidate) >= MATCH_THRESHOLD
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_strips_punctuation_and_case() {
        assert_eq!(
            normalize_title("KGAT: Knowledge-Graph  Attention Network!"),
            "kgat knowledge graph attention network"
        );
    }

    #[test]
    fn identical_titles_are_confident() {
        let q = "KGAT: Knowledge Graph Attention Network for Recommendation";
        let c = "KGAT: Knowledge Graph Attention Network for Recommendation.";
        assert!(title_similarity(q, c) > 0.95);
        assert!(is_confident_match(q, c));
    }

    #[test]
    fn unrelated_titles_are_not_confident() {
        assert!(!is_confident_match(
            "Deep Residual Learning for Image Recognition",
            "Attention Is All You Need"
        ));
    }
}
