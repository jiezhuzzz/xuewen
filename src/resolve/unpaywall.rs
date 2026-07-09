use anyhow::Result;

use super::http::HttpClient;

/// Query Unpaywall for a DOI and return the best OA PDF URL, if any.
pub async fn fetch(
    http: &HttpClient,
    base: &str,
    doi: &str,
    email: &str,
) -> Result<Option<String>> {
    let url = format!("{base}/v2/{doi}?email={email}");
    let body = http.get_text(&url).await?;
    parse(&body)
}

/// Extract `best_oa_location.url_for_pdf` from an Unpaywall response.
pub fn parse(body: &str) -> Result<Option<String>> {
    let v: serde_json::Value = serde_json::from_str(body)?;
    Ok(v.get("best_oa_location")
        .and_then(|loc| loc.get("url_for_pdf"))
        .and_then(|u| u.as_str())
        .map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    const OA: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/unpaywall_oa.json"
    ));
    const CLOSED: &str = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/unpaywall_closed.json"
    ));

    #[test]
    fn parses_oa_pdf_url() {
        assert_eq!(
            parse(OA).unwrap().as_deref(),
            Some("https://example.org/paper.pdf")
        );
    }

    #[test]
    fn closed_access_is_none() {
        assert_eq!(parse(CLOSED).unwrap(), None);
    }
}
