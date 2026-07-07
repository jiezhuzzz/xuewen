use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{collapse_ws, ResolvedMetadata};

/// Search DBLP publications by title. Returns raw JSON.
pub async fn fetch(client: &reqwest::Client, base: &str, title: &str) -> Result<String> {
    let resp = client
        .get(format!("{base}/search/publ/api"))
        .query(&[("q", title), ("format", "json"), ("h", "5")])
        .send()
        .await?;
    if !resp.status().is_success() {
        return Err(anyhow!("dblp HTTP {}", resp.status()));
    }
    Ok(resp.text().await?)
}

/// Parse a DBLP publ-search response into candidate records (possibly empty).
pub fn parse(json: &str) -> Result<Vec<ResolvedMetadata>> {
    let v: Value = serde_json::from_str(json)?;
    let hits = values(&v["result"]["hits"]["hit"]);

    let mut out = Vec::new();
    for h in hits {
        let info = &h["info"];
        if info.is_null() {
            continue;
        }
        let title = info["title"].as_str().map(clean_title);
        let year = info["year"].as_str().and_then(|s| s.parse::<i64>().ok());
        let venue = venue_of(&info["venue"]);
        let doi = info["doi"].as_str().map(str::to_string);
        let url = info["ee"]
            .as_str()
            .or_else(|| info["url"].as_str())
            .map(str::to_string);
        let dblp_key = info["key"].as_str().map(str::to_string);
        let authors = values(&info["authors"]["author"])
            .iter()
            .filter_map(|a| a["text"].as_str().map(collapse_ws))
            .collect();

        out.push(ResolvedMetadata {
            title,
            abstract_text: None,
            authors,
            venue,
            year,
            doi,
            arxiv_id: None,
            dblp_key,
            url,
            source: "dblp".to_string(),
        });
    }
    Ok(out)
}

/// A DBLP JSON field that may be a single object, an array, or absent.
fn values(v: &Value) -> Vec<&Value> {
    if let Some(arr) = v.as_array() {
        arr.iter().collect()
    } else if v.is_null() {
        Vec::new()
    } else {
        vec![v]
    }
}

/// DBLP `venue` may be a string or an array of strings.
fn venue_of(v: &Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        Some(collapse_ws(s))
    } else if let Some(arr) = v.as_array() {
        arr.iter().find_map(|x| x.as_str()).map(collapse_ws)
    } else {
        None
    }
}

/// DBLP titles end with a trailing period; strip it and collapse whitespace.
fn clean_title(s: &str) -> String {
    collapse_ws(s.trim_end_matches('.'))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/dblp_kgat.json"
    ));

    #[test]
    fn parses_dblp_hit() {
        let cands = parse(FIXTURE).unwrap();
        assert_eq!(cands.len(), 1);
        let c = &cands[0];
        assert_eq!(
            c.title.as_deref(),
            Some("KGAT: Knowledge Graph Attention Network for Recommendation")
        );
        assert_eq!(c.year, Some(2019));
        assert_eq!(c.venue.as_deref(), Some("KDD"));
        assert_eq!(c.doi.as_deref(), Some("10.1145/3292500.3330701"));
        assert_eq!(c.dblp_key.as_deref(), Some("conf/kdd/WangHCLC19"));
        assert_eq!(c.authors, vec!["Xiang Wang", "Xiangnan He", "Yixin Cao"]);
        assert_eq!(c.source, "dblp");
    }

    #[test]
    fn zero_hits_is_empty() {
        let json = r#"{"result":{"hits":{"@total":"0"}}}"#;
        assert!(parse(json).unwrap().is_empty());
    }

    #[test]
    fn single_author_object_is_handled() {
        // DBLP returns a bare object (not array) when there is exactly one author.
        let json = r#"{"result":{"hits":{"hit":[{"info":{
            "title":"A One Author Paper.","year":"2020",
            "authors":{"author":{"@pid":"1","text":"Solo Writer"}}
        }}]}}}"#;
        let cands = parse(json).unwrap();
        assert_eq!(cands[0].authors, vec!["Solo Writer"]);
    }
}
