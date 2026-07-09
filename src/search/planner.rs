use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};

use crate::search::store::IndexRow;

/// The searchable identity of a paper, as seen by the staleness scan.
#[derive(Debug, Clone)]
pub struct PaperState {
    pub id: String,
    pub content_hash: String,
    pub meta_hash: String,
    pub trashed: bool,
}

/// One paper's pending indexing work (at least one tier is true).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    pub paper_id: String,
    pub fts: bool,
    pub vectors: bool,
}

#[derive(Debug, Default)]
pub struct Plan {
    pub index: Vec<Work>,
    /// Tombstones: index entries whose paper is trashed or gone.
    pub deindex: Vec<String>,
}

/// Compare live papers against `search_index` and decide what to do.
/// Pure: all clock and IO inputs are parameters.
pub fn plan(
    papers: &[PaperState],
    rows: &[IndexRow],
    embed_model: Option<&str>,
    now: DateTime<Utc>,
) -> Plan {
    let by_id: HashMap<&str, &IndexRow> = rows.iter().map(|r| (r.paper_id.as_str(), r)).collect();
    let live: HashSet<&str> = papers.iter().filter(|p| !p.trashed).map(|p| p.id.as_str()).collect();
    let mut out = Plan::default();

    for p in papers.iter().filter(|p| !p.trashed) {
        let row = by_id.get(p.id.as_str()).copied();
        let content_changed = row
            .map(|r| r.content_hash != p.content_hash || r.meta_hash != p.meta_hash)
            .unwrap_or(true);
        let fts = content_changed || row.map(|r| r.fts_indexed_at.is_none()).unwrap_or(true);
        let vectors = embed_model.is_some()
            && (content_changed
                || row
                    .map(|r| {
                        r.vectors_indexed_at.is_none() || r.embed_model.as_deref() != embed_model
                    })
                    .unwrap_or(true));
        if (fts || vectors) && backoff_elapsed(row, now) {
            out.index.push(Work { paper_id: p.id.clone(), fts, vectors });
        }
    }
    for r in rows {
        if !live.contains(r.paper_id.as_str()) {
            out.deindex.push(r.paper_id.clone());
        }
    }
    out
}

/// After a failure, wait 60s · 2^(attempts−1), capped at one hour.
fn backoff_elapsed(row: Option<&IndexRow>, now: DateTime<Utc>) -> bool {
    let Some(r) = row else { return true };
    if r.attempts == 0 {
        return true;
    }
    let Some(last) = r
        .last_attempt_at
        .as_deref()
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok())
    else {
        return true;
    };
    let exp = (r.attempts - 1).clamp(0, 6) as u32;
    let wait = (60i64 << exp).min(3600);
    now.signed_duration_since(last.with_timezone(&Utc)) >= chrono::Duration::seconds(wait)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};

    fn ps(id: &str, ch: &str, mh: &str, trashed: bool) -> PaperState {
        PaperState { id: id.into(), content_hash: ch.into(), meta_hash: mh.into(), trashed }
    }

    fn row(id: &str, ch: &str, mh: &str) -> crate::search::store::IndexRow {
        crate::search::store::IndexRow {
            paper_id: id.into(),
            content_hash: ch.into(),
            meta_hash: mh.into(),
            chunk_count: 2,
            fts_indexed_at: Some("2026-07-09T00:00:00Z".into()),
            vectors_indexed_at: Some("2026-07-09T00:00:00Z".into()),
            embed_model: Some("m1".into()),
            last_error: None,
            attempts: 0,
            last_attempt_at: None,
        }
    }

    #[test]
    fn new_paper_needs_both_tiers() {
        let p = plan(&[ps("a", "h", "m", false)], &[], Some("m1"), Utc::now());
        assert_eq!(p.index.len(), 1);
        assert!(p.index[0].fts && p.index[0].vectors);
        assert!(p.deindex.is_empty());
    }

    #[test]
    fn up_to_date_paper_yields_no_work() {
        let p = plan(&[ps("a", "h", "m", false)], &[row("a", "h", "m")], Some("m1"), Utc::now());
        assert!(p.index.is_empty() && p.deindex.is_empty());
    }

    #[test]
    fn meta_change_and_content_change_force_both_tiers() {
        for (ch, mh) in [("h2", "m"), ("h", "m2")] {
            let p = plan(&[ps("a", ch, mh, false)], &[row("a", "h", "m")], Some("m1"), Utc::now());
            assert!(p.index[0].fts && p.index[0].vectors, "case ({ch},{mh})");
        }
    }

    #[test]
    fn model_change_re_embeds_without_touching_fts() {
        let p = plan(&[ps("a", "h", "m", false)], &[row("a", "h", "m")], Some("m2"), Utc::now());
        assert_eq!(p.index.len(), 1);
        assert!(!p.index[0].fts && p.index[0].vectors);
    }

    #[test]
    fn no_embedder_means_no_vector_work() {
        let p = plan(&[ps("a", "h", "m", false)], &[], None, Utc::now());
        assert!(p.index[0].fts && !p.index[0].vectors);
    }

    #[test]
    fn trashed_and_missing_papers_become_deindex_tombstones() {
        let p = plan(&[ps("a", "h", "m", true)], &[row("a", "h", "m"), row("gone", "h", "m")], Some("m1"), Utc::now());
        assert!(p.index.is_empty());
        let mut d = p.deindex.clone();
        d.sort();
        assert_eq!(d, vec!["a".to_string(), "gone".to_string()]);
    }

    #[test]
    fn failed_rows_back_off_exponentially_capped_at_an_hour() {
        let mut r = row("a", "h", "m");
        r.fts_indexed_at = None;
        r.attempts = 2; // wait = 60 * 2^(2-1) = 120s
        r.last_attempt_at = Some((Utc::now() - Duration::seconds(30)).to_rfc3339());
        let p = plan(&[ps("a", "h", "m", false)], &[r.clone()], None, Utc::now());
        assert!(p.index.is_empty(), "still inside the backoff window");

        r.last_attempt_at = Some((Utc::now() - Duration::seconds(180)).to_rfc3339());
        let p = plan(&[ps("a", "h", "m", false)], &[r.clone()], None, Utc::now());
        assert_eq!(p.index.len(), 1, "window elapsed");

        r.attempts = 50; // cap: never wait more than 3600s
        r.last_attempt_at = Some((Utc::now() - Duration::seconds(3700)).to_rfc3339());
        let p = plan(&[ps("a", "h", "m", false)], &[r], None, Utc::now());
        assert_eq!(p.index.len(), 1);
    }
}
