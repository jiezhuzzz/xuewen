mod common;

use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::db;
use xuewen::pipeline::{ingest_file, Libraries, Outcome};
use xuewen::resolve::grobid::Grobid;
use xuewen::resolve::Resolver;

const TEI_FIXTURE: &str = include_str!("fixtures/grobid_bert.tei.xml");
const DBLP_BERT_FIXTURE: &str = include_str!("fixtures/dblp_bert.json");

#[tokio::test]
async fn ingests_pdf_and_dedups() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    // A PDF whose header carries a title and a DOI.
    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(
        &pdf_path,
        &[
            "Attention Is All You Need",
            "https://doi.org/10.1145/3292500.3330701",
        ],
    );

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mock = MockServer::start().await;
    let resolver = Resolver::with_bases(None, mock.uri(), mock.uri()).unwrap();
    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };

    // First ingest: stored, filed, moved.
    let out = ingest_file(&pool, &dirs, &resolver, None, &pdf_path)
        .await
        .unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };

    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.title.as_deref(), Some("Attention Is All You Need"));
    assert_eq!(paper.doi.as_deref(), Some("10.1145/3292500.3330701"));
    assert_eq!(paper.status, "needs_review");

    // File was copied into the library and the original moved to _processed.
    assert!(library.join(format!("_unsorted/{}.pdf", paper.content_hash)).exists());
    assert_eq!(paper.cite_key, None);
    assert!(!pdf_path.exists());
    assert!(processed.join("paper.pdf").exists());

    // Re-ingest identical content (from processed copy) -> Duplicate.
    let again = processed.join("paper.pdf");
    let out2 = ingest_file(&pool, &dirs, &resolver, None, &again)
        .await
        .unwrap();
    assert_eq!(out2, Outcome::Duplicate);
}

#[tokio::test]
async fn same_doi_different_bytes_errors_without_orphan() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    let doi_line = "https://doi.org/10.1000/xyz123";
    let a = inbox.join("a.pdf");
    let b = inbox.join("b.pdf");
    common::write_test_pdf(&a, &["Paper A Title", doi_line]);
    common::write_test_pdf(&b, &["Paper B Different Title", doi_line]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let mock = MockServer::start().await;
    let resolver = Resolver::with_bases(None, mock.uri(), mock.uri()).unwrap();
    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };

    // First ingests fine.
    let out = ingest_file(&pool, &dirs, &resolver, None, &a)
        .await
        .unwrap();
    assert!(matches!(out, Outcome::Ingested(_)));

    // Second: same DOI, different bytes. Content-hash dedup passes, then the
    // doi UNIQUE constraint rejects it -> Err, and NO orphan file remains.
    let res = ingest_file(&pool, &dirs, &resolver, None, &b).await;
    assert!(
        res.is_err(),
        "expected a UNIQUE-constraint error on duplicate DOI"
    );

    // Library holds exactly one PDF (paper A); paper B's copy was cleaned up.
    let count = std::fs::read_dir(library.join("_unsorted")).unwrap().count();
    assert_eq!(count, 1, "library should contain only paper A, no orphan");

    // b.pdf was not moved (ingest errored before the move step).
    assert!(b.exists());
}

const CROSSREF_FIXTURE: &str = include_str!("fixtures/crossref_kgat.json");
const ARXIV_FIXTURE: &str = include_str!("fixtures/arxiv_attention.xml");

#[tokio::test]
async fn ingest_with_doi_resolves_via_crossref() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    let doi = "10.1145/3292500.3330701";
    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(
        &pdf_path,
        &["Some Provisional Header", &format!("https://doi.org/{doi}")],
    );

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };

    let out = ingest_file(&pool, &dirs, &resolver, None, &pdf_path)
        .await
        .unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };

    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.status, "resolved");
    assert_eq!(paper.source.as_deref(), Some("crossref"));
    assert_eq!(
        paper.title.as_deref(),
        Some("KGAT: Knowledge Graph Attention Network for Recommendation")
    );
    assert_eq!(paper.doi.as_deref(), Some(doi));
    assert_eq!(paper.year, Some(2019));
    assert!(paper.authors.as_deref().unwrap().contains("Xiang Wang"));
    assert_eq!(paper.cite_key.as_deref(), Some("wang2019kgat"));
    assert!(library.join("wang2019kgat.pdf").exists());
}

#[tokio::test]
async fn ingest_with_arxiv_resolves_via_api() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    let arxiv_id = "1706.03762";
    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(
        &pdf_path,
        &["Provisional Header", &format!("arXiv:{arxiv_id}")],
    );

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/api/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ARXIV_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };

    let out = ingest_file(&pool, &dirs, &resolver, None, &pdf_path)
        .await
        .unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };

    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.status, "resolved");
    assert_eq!(paper.source.as_deref(), Some("arxiv"));
    assert_eq!(paper.title.as_deref(), Some("Attention Is All You Need"));
    assert_eq!(paper.arxiv_id.as_deref(), Some(arxiv_id));
    assert_eq!(paper.year, Some(2017));
    assert_eq!(paper.cite_key.as_deref(), Some("vaswani2017attention"));
    assert!(library.join("vaswani2017attention.pdf").exists());
}

const DBLP_FIXTURE: &str = include_str!("fixtures/dblp_kgat.json");

#[tokio::test]
async fn ingest_without_identifier_resolves_via_dblp() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    // No DOI/arXiv anywhere; the first substantive line is the title.
    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(
        &pdf_path,
        &["KGAT: Knowledge Graph Attention Network for Recommendation"],
    );

    // DBLP mock returns the matching hit.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };

    let out = ingest_file(&pool, &dirs, &resolver, None, &pdf_path)
        .await
        .unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };

    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.status, "resolved");
    assert_eq!(paper.source.as_deref(), Some("dblp"));
    assert_eq!(paper.dblp_key.as_deref(), Some("conf/kdd/WangHCLC19"));
    assert_eq!(paper.venue.as_deref(), Some("KDD"));
    assert_eq!(paper.year, Some(2019));
    assert!(paper.doi.as_deref().is_some());
    assert_eq!(paper.cite_key.as_deref(), Some("wang2019kgat"));
    assert!(library.join("wang2019kgat.pdf").exists());
}

#[tokio::test]
async fn grobid_title_drives_dblp_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    // The PDF's own text is a poor/truncated title; GROBID supplies the clean one.
    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(&pdf_path, &["BERT Pre-training of Deep Bidir"]);

    // GROBID returns the full BERT header; DBLP is stubbed with the BERT record.
    let grobid_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wm_path("/api/processHeaderDocument"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TEI_FIXTURE))
        .mount(&grobid_server)
        .await;
    let grobid = Grobid::new(&grobid_server.uri()).unwrap();

    let api_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_BERT_FIXTURE))
        .mount(&api_server)
        .await;
    let resolver = Resolver::with_bases(None, api_server.uri(), api_server.uri())
        .unwrap()
        .with_dblp_base(api_server.uri());

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };

    let out = ingest_file(&pool, &dirs, &resolver, Some(&grobid), &pdf_path)
        .await
        .unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };
    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    // DBLP matched (its title fuzzily matches the GROBID title) -> resolved via dblp.
    assert_eq!(paper.status, "resolved");
    assert_eq!(paper.source.as_deref(), Some("dblp"));
    // DBLP has no abstract; the GROBID abstract is backfilled.
    assert!(paper
        .abstract_text
        .as_deref()
        .unwrap()
        .contains("language representation model"));
}

#[tokio::test]
async fn grobid_enriches_needs_review_when_unmatched() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(&pdf_path, &["garbled first line xyz"]);

    let grobid_server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(wm_path("/api/processHeaderDocument"))
        .respond_with(ResponseTemplate::new(200).set_body_string(TEI_FIXTURE))
        .mount(&grobid_server)
        .await;
    let grobid = Grobid::new(&grobid_server.uri()).unwrap();

    // Resolver points at a stub-less server: DBLP + Crossref both 404 -> Unresolved.
    let api_server = MockServer::start().await;
    let resolver = Resolver::with_bases(None, api_server.uri(), api_server.uri())
        .unwrap()
        .with_dblp_base(api_server.uri());

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();
    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };

    let out = ingest_file(&pool, &dirs, &resolver, Some(&grobid), &pdf_path)
        .await
        .unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };
    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.status, "needs_review");
    assert_eq!(paper.source.as_deref(), Some("grobid"));
    assert_eq!(
        paper.title.as_deref(),
        Some("BERT: Pre-training of Deep Bidirectional Transformers for Language Understanding")
    );
    assert!(paper.authors.as_deref().unwrap().contains("Jacob Devlin"));
}

#[tokio::test]
async fn colliding_cite_key_gets_letter_suffix() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    let processed = inbox.join("_processed");
    std::fs::create_dir_all(&inbox).unwrap();

    let doi = "10.1145/3292500.3330701";
    let pdf_path = inbox.join("paper.pdf");
    common::write_test_pdf(&pdf_path, &["Header", &format!("https://doi.org/{doi}")]);

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&server)
        .await;
    let resolver = Resolver::with_bases(None, server.uri(), server.uri()).unwrap();

    let url = format!("sqlite:{}", dir.path().join("library.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Pre-seed a different paper that already owns the base key "wang2019kgat".
    let seed = xuewen::models::Paper {
        id: "01890000-0000-7000-8000-0000000000ff".to_string(),
        content_hash: "seedhash".to_string(),
        rel_path: "wang2019kgat.pdf".to_string(),
        title: Some("Seed".to_string()),
        abstract_text: None,
        authors: None,
        venue: None,
        year: Some(2019),
        doi: None,
        arxiv_id: None,
        dblp_key: None,
        cite_key: Some("wang2019kgat".to_string()),
        url: None,
        source: Some("crossref".to_string()),
        status: "resolved".to_string(),
        added_at: "2026-07-07T00:00:00Z".to_string(),
    };
    db::insert_paper(&pool, &seed).await.unwrap();

    let dirs = Libraries {
        library_root: library.clone(),
        processed_dir: processed.clone(),
    };
    let out = ingest_file(&pool, &dirs, &resolver, None, &pdf_path).await.unwrap();
    let id = match out {
        Outcome::Ingested(id) => id,
        Outcome::Duplicate => panic!("expected Ingested"),
    };
    let paper = db::get_by_id(&pool, &id).await.unwrap().unwrap();
    assert_eq!(paper.cite_key.as_deref(), Some("wang2019kgata"));
    assert!(library.join("wang2019kgata.pdf").exists());
}
