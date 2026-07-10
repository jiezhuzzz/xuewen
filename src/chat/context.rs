//! Builds the system prompt: instructions + metadata + capped paper text.

use crate::models::Paper;

pub fn system_prompt(paper: &Paper, full_text: Option<&str>, cap: usize) -> String {
    let mut s = String::from(
        "You are a research assistant discussing one specific paper with a researcher.\n\
         Answer from the paper's content; when the paper does not contain the answer, say so plainly.\n\
         Answer in plain prose without markdown formatting.\n\n--- PAPER METADATA ---\n",
    );
    let m = &paper.meta;
    s.push_str(&format!(
        "Title: {}\n",
        m.title.as_deref().unwrap_or("(untitled)")
    ));
    if !m.authors.0.is_empty() {
        s.push_str(&format!("Authors: {}\n", m.authors.0.join(", ")));
    }
    if let Some(v) = &m.venue {
        s.push_str(&format!("Venue: {v}\n"));
    }
    if let Some(y) = m.year {
        s.push_str(&format!("Year: {y}\n"));
    }
    if let Some(a) = &m.abstract_text {
        s.push_str(&format!("Abstract: {a}\n"));
    }
    match full_text {
        Some(t) => {
            let clipped: String = t.chars().take(cap).collect();
            let marker = if t.chars().count() > cap {
                " (truncated)"
            } else {
                ""
            };
            s.push_str(&format!("\n--- PAPER TEXT{marker} ---\n{clipped}\n"));
        }
        None => s.push_str(
            "\n(The paper's full text was unavailable; only the metadata above is known.)\n",
        ),
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Authors, Paper, PaperMeta, PaperStatus};

    fn paper() -> Paper {
        Paper {
            id: "p1".into(),
            content_hash: "h".into(),
            rel_path: "p.pdf".into(),
            cite_key: Some("smith2024".into()),
            added_at: "2026-01-01".into(),
            deleted_at: None,
            meta: PaperMeta {
                title: Some("A Great Paper".into()),
                abstract_text: Some("We do things.".into()),
                authors: Authors(vec!["A. Smith".into(), "B. Jones".into()]),
                venue: Some("NeurIPS".into()),
                year: Some(2024),
                doi: None,
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::Resolved,
            },
        }
    }

    #[test]
    fn prompt_includes_metadata_and_capped_text() {
        let text = "x".repeat(100);
        let p = system_prompt(&paper(), Some(&text), 10);
        assert!(p.contains("Title: A Great Paper"));
        assert!(p.contains("Authors: A. Smith, B. Jones"));
        assert!(p.contains("Venue: NeurIPS"));
        assert!(p.contains("Abstract: We do things."));
        assert!(p.contains("PAPER TEXT (truncated)"));
        assert!(p.contains(&"x".repeat(10)));
        assert!(!p.contains(&"x".repeat(11)), "cap must apply");
        assert!(p.contains("plain prose"), "markdown-free instruction");
    }

    #[test]
    fn prompt_notes_missing_text() {
        let p = system_prompt(&paper(), None, 10);
        assert!(p.contains("full text was unavailable"));
        assert!(!p.contains("PAPER TEXT"));
    }
}
