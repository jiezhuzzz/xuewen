use anyhow::{bail, Context, Result};
use chrono::Utc;
use regex::Regex;
use std::sync::LazyLock;

use super::{feed, score, store, tldr, DailyService, ARXIV_ABS_BASE, ARXIV_PDF_BASE};

/// Pages of the PDF fed to the TL;DR prompt.
const TLDR_PDF_PAGES: u32 = 12;
const PDF_MAX_BYTES: usize = 30 * 1024 * 1024;
const PDF_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

static GITHUB_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"https?://github\.com/[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+").unwrap()
});

/// First GitHub repository URL in the text; trailing sentence punctuation
/// the PDF extraction glues on is trimmed.
fn find_code_url(text: &str) -> Option<String> {
    let m = GITHUB_RE.find(text)?;
    Some(m.as_str().trim_end_matches('.').to_string())
}

/// One full daily run. Never fails: the outcome (ok/empty/failed) is
/// recorded in `daily_runs` and returned. Old batches are pruned after.
pub async fn run_once(svc: &DailyService, batch_date: &str) -> store::DailyRun {
    let (status, found, error) = match pipeline(svc, batch_date).await {
        Ok(0) => ("empty", 0, None),
        Ok(found) => ("ok", found, None),
        Err(e) => {
            tracing::error!("daily run {batch_date} failed: {e:#}");
            ("failed", 0, Some(format!("{e:#}")))
        }
    };
    let run = store::DailyRun {
        batch_date: batch_date.to_string(),
        status: status.to_string(),
        papers_found: found,
        error,
        ran_at: Utc::now().to_rfc3339(),
    };
    if let Err(e) = store::record_run(&svc.pool, &run).await {
        tracing::error!("recording daily run {batch_date}: {e:#}");
    }
    if let Err(e) = prune_old(svc, batch_date).await {
        tracing::warn!("pruning old daily batches: {e:#}");
    }
    run
}

/// Fetch → dedup → score → TL;DR → store. Returns the candidate count
/// after dedup (0 ⇒ the caller records an "empty" run).
async fn pipeline(svc: &DailyService, batch_date: &str) -> Result<i64> {
    let xml = feed::fetch_feed(&svc.http, &svc.feed_base, &svc.cfg.categories)
        .await
        .context("fetching arXiv feed")?;
    let mut candidates = feed::parse_feed(&xml, svc.cfg.include_cross_list)?;

    let known = store::library_arxiv_ids(&svc.pool).await?;
    candidates.retain(|c| !known.contains(&c.arxiv_id));
    let found = candidates.len() as i64;
    if candidates.is_empty() {
        return Ok(0);
    }

    let Some(profile) = score::build_profile(&svc.pool, &svc.vectors).await? else {
        bail!(
            "no indexed library papers — let `xuewen serve` finish indexing \
             or run `xuewen index rebuild` first"
        );
    };

    let texts: Vec<String> = candidates
        .iter()
        .map(|c| format!("{}\n{}", c.title, c.abstract_text))
        .collect();
    let embeddings = svc
        .embedder
        .embed(&texts)
        .await
        .context("embedding candidates")?;

    let mut scored: Vec<(f32, feed::Candidate)> = candidates
        .into_iter()
        .zip(embeddings)
        .map(|(c, mut v)| {
            score::l2_normalize(&mut v);
            (score::dot(&v, &profile), c)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(svc.cfg.max_papers);

    let mut rows = Vec::with_capacity(scored.len());
    for (i, (s, c)) in scored.into_iter().enumerate() {
        let full_text = match fetch_pdf_text(svc, &c.arxiv_id).await {
            Ok(t) => Some(t),
            Err(e) => {
                tracing::warn!("PDF text for {}: {e:#}", c.arxiv_id);
                None
            }
        };
        let code_url = full_text.as_deref().and_then(find_code_url);
        let summary = tldr::generate_summary(
            &svc.chat,
            &svc.cfg.llm.language,
            &c.title,
            &c.abstract_text,
            full_text.as_deref(),
        )
        .await;
        rows.push(store::DailyPaper {
            batch_date: batch_date.to_string(),
            rank: i as i64 + 1,
            arxiv_id: c.arxiv_id.clone(),
            title: c.title,
            authors: c.authors,
            abstract_text: c.abstract_text,
            categories: c.categories,
            score: s as f64,
            tldr: summary.as_ref().map(|s| s.tldr.clone()),
            summary,
            code_url,
            abs_url: format!("{ARXIV_ABS_BASE}/{}", c.arxiv_id),
            pdf_url: format!("{ARXIV_PDF_BASE}/{}", c.arxiv_id),
        });
    }
    store::replace_batch(&svc.pool, batch_date, &rows).await?;
    Ok(found)
}

/// Download the paper's PDF and return the text of its first pages,
/// capped for the prompt. Any failure here is per-paper and non-fatal.
async fn fetch_pdf_text(svc: &DailyService, arxiv_id: &str) -> Result<String> {
    let url = format!("{}/{}", svc.pdf_base, arxiv_id);
    let resp = svc.plain_http.get(&url).timeout(PDF_TIMEOUT).send().await?;
    if !resp.status().is_success() {
        bail!("PDF download {url}: {}", resp.status());
    }
    let bytes = resp.bytes().await?;
    if bytes.len() > PDF_MAX_BYTES {
        bail!("PDF too large: {} bytes", bytes.len());
    }
    let path = std::env::temp_dir().join(format!("xuewen-daily-{}.pdf", uuid::Uuid::now_v7()));
    // The image may lack /tmp entirely; pdftotext is a blocking subprocess —
    // both belong on the blocking pool, not an async worker.
    tokio::task::spawn_blocking(move || -> Result<String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let result = (|| -> Result<String> {
            std::fs::write(&path, &bytes)?;
            let text = crate::pdf::extract_text(&path, TLDR_PDF_PAGES)?;
            Ok(text.chars().take(tldr::FULL_TEXT_CAP).collect())
        })();
        let _ = std::fs::remove_file(&path);
        result
    })
    .await?
}

async fn prune_old(svc: &DailyService, batch_date: &str) -> Result<()> {
    let date = chrono::NaiveDate::parse_from_str(batch_date, "%Y-%m-%d")?;
    let cutoff = date
        .checked_sub_days(chrono::Days::new(svc.cfg.retention_days as u64))
        .unwrap_or(date);
    store::prune(&svc.pool, &cutoff.format("%Y-%m-%d").to_string()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DailyConfig, DailyLlmConfig};
    use crate::daily::{store, tldr::ChatClient, DailyService};
    use crate::search::{embedder::Embedder, vector::QdrantStore};
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    // Library paper 2401.00001 is deduped; candidates A (2507.0000**2**,
    // orthogonal to the profile) and B (2507.0000**3**, parallel) get ranked.
    const FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom"
      xmlns:dc="http://purl.org/dc/elements/1.1/"
      xmlns:arxiv="http://arxiv.org/schemas/atom">
  <title>cs.AI updates on arXiv.org</title>
  <entry>
    <id>oai:arXiv.org:2401.00001v1</id>
    <title>Already In The Library</title>
    <summary>arXiv:2401.00001v1 Announce Type: new
Abstract: Old news.</summary>
    <dc:creator>Lib Author</dc:creator>
    <category term="cs.AI"/>
    <arxiv:announce_type>new</arxiv:announce_type>
  </entry>
  <entry>
    <id>oai:arXiv.org:2507.00002v1</id>
    <title>Candidate A</title>
    <summary>arXiv:2507.00002v1 Announce Type: new
Abstract: Unrelated to the library.</summary>
    <dc:creator>Alice</dc:creator>
    <category term="cs.AI"/>
    <arxiv:announce_type>new</arxiv:announce_type>
  </entry>
  <entry>
    <id>oai:arXiv.org:2507.00003v1</id>
    <title>Candidate B</title>
    <summary>arXiv:2507.00003v1 Announce Type: new
Abstract: Very similar to the library.</summary>
    <dc:creator>Bob</dc:creator>
    <category term="cs.AI"/>
    <arxiv:announce_type>new</arxiv:announce_type>
  </entry>
</feed>"#;

    const EMPTY_FEED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom">
  <title>cs.AI updates on arXiv.org</title>
</feed>"#;

    async fn pool_with_library_paper() -> sqlx::SqlitePool {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let pool = crate::db::connect(&url).await.unwrap();
        std::mem::forget(dir);
        let p = crate::models::Paper {
            id: "lib1".into(),
            content_hash: "h".into(),
            rel_path: "lib1.pdf".into(),
            cite_key: None,
            added_at: "2026-07-01T00:00:00Z".into(),
            deleted_at: None,
            meta: crate::models::PaperMeta {
                title: Some("Library Paper".into()),
                abstract_text: Some("lib abstract".into()),
                authors: crate::models::Authors(vec![]),
                venue: None,
                year: None,
                doi: None,
                arxiv_id: Some("2401.00001".into()),
                dblp_key: None,
                url: None,
                source: None,
                status: crate::models::PaperStatus::Resolved,
            },
        };
        crate::db::insert_paper(&pool, &p).await.unwrap();
        pool
    }

    fn cfg() -> DailyConfig {
        DailyConfig {
            categories: vec!["cs.AI".into(), "cs.LG".into()],
            include_cross_list: false,
            max_papers: 20,
            run_at: "09:00".into(),
            retention_days: 14,
            llm: DailyLlmConfig {
                base_url: "unused".into(),
                model: "m".into(),
                api_key: None,
                api_key_env: "UNSET".into(),
                language: "English".into(),
            },
        }
    }

    fn service(server: &MockServer, pool: sqlx::SqlitePool) -> std::sync::Arc<DailyService> {
        DailyService::for_tests(
            cfg(),
            pool,
            Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4),
            QdrantStore::new(&server.uri(), "xuewen", 4).unwrap(),
            ChatClient::for_tests(&format!("{}/v1", server.uri()), "m"),
            &format!("{}/atom", server.uri()),
            &format!("{}/pdf", server.uri()),
        )
    }

    async fn mount_scroll(server: &MockServer, points: serde_json::Value) {
        Mock::given(method("POST"))
            .and(path("/collections/xuewen/points/scroll"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "result": {"points": points, "next_page_offset": null}
            })))
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn full_run_dedupes_ranks_and_stores() {
        let pool = pool_with_library_paper().await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FEED))
            .mount(&server)
            .await;
        mount_scroll(
            &server,
            json!([{"id": "x", "payload": {"paper_id": "lib1", "seq": 0},
                    "vector": [1.0, 0.0, 0.0, 0.0]}]),
        )
        .await;
        // Candidate order in the feed: A then B. A is orthogonal, B parallel.
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "data": [
                    {"index": 0, "embedding": [0.0, 1.0, 0.0, 0.0]},
                    {"index": 1, "embedding": [1.0, 0.0, 0.0, 0.0]}
                ]
            })))
            .mount(&server)
            .await;
        // PDFs 404 -> TL;DR falls back to abstract-only, which succeeds.
        Mock::given(method("GET"))
            .and(wiremock::matchers::path_regex("^/pdf/.*"))
            .respond_with(ResponseTemplate::new(404))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "choices": [{"message": {"role": "assistant",
                    "content": "{\"tldr\":\"A TLDR.\",\"problem\":\"Gap.\",\"approach\":\"Idea.\",\"results\":\"+1.\",\"limitations\":\"Few.\"}"}}]
            })))
            .mount(&server)
            .await;

        let svc = service(&server, pool.clone());
        let run = run_once(&svc, "2026-07-10").await;

        assert_eq!(run.status, "ok");
        assert_eq!(run.papers_found, 2, "library paper must be deduped");
        let (date, papers) = store::latest_batch(&pool).await.unwrap().unwrap();
        assert_eq!(date, "2026-07-10");
        assert_eq!(papers.len(), 2);
        assert_eq!(papers[0].arxiv_id, "2507.00003", "parallel candidate ranks first");
        assert_eq!(papers[0].rank, 1);
        assert!(papers[0].score > papers[1].score);
        assert_eq!(papers[0].tldr.as_deref(), Some("A TLDR."));
        let s = papers[0].summary.as_ref().expect("summary stored");
        assert_eq!(s.tldr, "A TLDR.");
        assert_eq!(s.problem, "Gap.");
        assert!(papers[0].code_url.is_none(), "PDFs 404 -> no text -> no code link");
        assert_eq!(papers[0].abs_url, "https://arxiv.org/abs/2507.00003");
        assert_eq!(papers[0].pdf_url, "https://arxiv.org/pdf/2507.00003");
        let recorded = store::get_run(&pool, "2026-07-10").await.unwrap().unwrap();
        assert_eq!(recorded.status, "ok");
    }

    #[tokio::test]
    async fn empty_feed_records_empty_run() {
        let pool = pool_with_library_paper().await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(ResponseTemplate::new(200).set_body_string(EMPTY_FEED))
            .mount(&server)
            .await;
        let svc = service(&server, pool.clone());
        let run = run_once(&svc, "2026-07-10").await;
        assert_eq!(run.status, "empty");
        assert_eq!(run.papers_found, 0);
        assert!(store::latest_batch(&pool).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn feed_failure_records_failed_run() {
        let pool = pool_with_library_paper().await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let svc = service(&server, pool.clone());
        let run = run_once(&svc, "2026-07-10").await;
        assert_eq!(run.status, "failed");
        assert!(run.error.is_some());
    }

    #[tokio::test]
    async fn missing_library_vectors_fail_with_clear_error() {
        let pool = pool_with_library_paper().await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(ResponseTemplate::new(200).set_body_string(FEED))
            .mount(&server)
            .await;
        mount_scroll(&server, json!([])).await;
        let svc = service(&server, pool.clone());
        let run = run_once(&svc, "2026-07-10").await;
        assert_eq!(run.status, "failed");
        assert!(
            run.error.unwrap().contains("no indexed library papers"),
            "error should tell the user to build the index"
        );
    }

    #[tokio::test]
    async fn run_guarded_refuses_concurrent_runs() {
        let pool = pool_with_library_paper().await;
        let server = MockServer::start().await;
        // Slow feed keeps the first run in flight.
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(EMPTY_FEED)
                    .set_delay(std::time::Duration::from_millis(500)),
            )
            .mount(&server)
            .await;
        let svc = service(&server, pool);
        assert!(svc.spawn_run("2026-07-10".into()));
        assert!(svc.is_running());
        assert!(svc.run_guarded("2026-07-10").await.is_none());
    }

    #[tokio::test]
    async fn run_flag_clears_when_run_future_is_dropped() {
        let pool = pool_with_library_paper().await;
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/atom/cs.AI+cs.LG"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(EMPTY_FEED)
                    .set_delay(std::time::Duration::from_millis(500)),
            )
            .mount(&server)
            .await;
        let svc = service(&server, pool);
        {
            let fut = svc.run_guarded("2026-07-10");
            tokio::pin!(fut);
            tokio::select! {
                _ = &mut fut => panic!("slow run should not finish before the timeout"),
                _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
            }
        } // fut dropped here, mid-await
        assert!(!svc.is_running(), "dropped run must clear the flag");
        // A new run can start immediately afterwards.
        assert!(svc.run_guarded("2026-07-10").await.is_some());
    }

    #[test]
    fn finds_github_url_and_trims_punctuation() {
        assert_eq!(
            find_code_url("Code at https://github.com/acme/widget. More text"),
            Some("https://github.com/acme/widget".to_string())
        );
        assert_eq!(
            find_code_url("(https://github.com/a-b/c_d)"),
            Some("https://github.com/a-b/c_d".to_string())
        );
        assert_eq!(find_code_url("no links here"), None);
        assert_eq!(find_code_url("see https://gitlab.com/x/y"), None);
    }
}
