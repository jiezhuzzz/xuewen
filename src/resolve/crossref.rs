use anyhow::{anyhow, Result};
use serde_json::Value;

use super::{collapse_ws, strip_tags, ResolvedMetadata};

/// Fetch the Crossref work record for a DOI from `{base}/works/{doi}`.
pub async fn fetch(client: &reqwest::Client, base: &str, doi: &str) -> Result<String> {
    let url = format!("{base}/works/{doi}");
    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        return Err(anyhow!("crossref HTTP {}", resp.status()));
    }
    Ok(resp.text().await?)
}

/// Parse a Crossref `/works/{doi}` JSON body. Returns `Ok(None)` if there is no message.
pub fn parse(json: &str) -> Result<Option<ResolvedMetadata>> {
    let v: Value = serde_json::from_str(json)?;
    let m = &v["message"];
    if m.is_null() {
        return Ok(None);
    }

    let title = m["title"].get(0).and_then(Value::as_str).map(collapse_ws);
    let venue = m["container-title"].get(0).and_then(Value::as_str).map(collapse_ws);
    let doi = m["DOI"].as_str().map(str::to_string);
    let url = m["URL"].as_str().map(str::to_string);
    let abstract_text = m["abstract"].as_str().map(strip_tags);
    let year = m["issued"]["date-parts"]
        .get(0)
        .and_then(|dp| dp.get(0))
        .and_then(Value::as_i64);

    let authors: Vec<String> = m["author"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| {
                    let given = a["given"].as_str().unwrap_or("");
                    let family = a["family"].as_str().unwrap_or("");
                    let name = format!("{given} {family}").trim().to_string();
                    (!name.is_empty()).then_some(name)
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(Some(ResolvedMetadata {
        title,
        abstract_text,
        authors,
        venue,
        year,
        doi,
        arxiv_id: None,
        dblp_key: None,
        url,
        source: "crossref".to_string(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str =
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/crossref_kgat.json"));

    #[test]
    fn parses_crossref_work() {
        let md = parse(FIXTURE).unwrap().unwrap();
        assert_eq!(
            md.title.as_deref(),
            Some("KGAT: Knowledge Graph Attention Network for Recommendation")
        );
        assert_eq!(md.year, Some(2019));
        assert_eq!(md.authors, vec!["Xiang Wang", "Xiangnan He"]);
        assert_eq!(md.doi.as_deref(), Some("10.1145/3292500.3330701"));
        assert_eq!(
            md.abstract_text.as_deref(),
            Some("Knowledge graphs are used to improve recommendation.")
        );
        assert!(md.venue.unwrap().starts_with("Proceedings of the 25th ACM SIGKDD"));
        assert_eq!(md.source, "crossref");
    }

    #[test]
    fn missing_message_is_none() {
        assert!(parse(r#"{"status":"ok"}"#).unwrap().is_none());
    }
}
