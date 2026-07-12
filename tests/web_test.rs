mod common;

use axum_test::multipart::{MultipartForm, Part};
use axum_test::TestServer;
use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::db;
use xuewen::models::{Authors, Paper, PaperMeta, PaperStatus};
use xuewen::pipeline::{IngestCtx, Libraries};
use xuewen::resolve::Resolver;
use xuewen::web::build_router_with_ingest_proxy;
use xuewen::web::{build_router, build_router_with_ingest, Ingest};

const DBLP_FIXTURE: &str = include_str!("fixtures/dblp_kgat.json");
const CROSSREF_FIXTURE: &str = include_str!("fixtures/crossref_kgat.json");
const ARXIV_FIXTURE: &str = include_str!("fixtures/arxiv_attention.xml");

async fn temp_pool() -> (tempfile::TempDir, sqlx::SqlitePool) {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    (dir, pool)
}

fn paper(id: &str, title: &str, status: PaperStatus) -> Paper {
    Paper {
        id: id.into(),
        content_hash: id.into(),
        rel_path: format!("{id}.pdf"),
        cite_key: Some(id.into()),
        added_at: "2026-07-07T00:00:00Z".into(),
        deleted_at: None,
        meta: PaperMeta {
            title: Some(title.into()),
            abstract_text: Some("An abstract.".into()),
            authors: Authors(vec!["Ada Lovelace".into()]),
            venue: Some("KDD".into()),
            year: Some(2020),
            doi: None,
            arxiv_id: None,
            dblp_key: None,
            url: None,
            source: Some("crossref".into()),
            status,
        },
    }
}

#[tokio::test]
async fn lists_and_details_papers() {
    let (dir, pool) = temp_pool().await;
    db::insert_paper(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    db::insert_paper(
        &pool,
        &paper(
            "bbbb2222",
            "Attention Is All You Need",
            PaperStatus::NeedsReview,
        ),
    )
    .await
    .unwrap();
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();

    // List: JSON array of summaries, authors as an array, no abstract field.
    let resp = server.get("/api/papers").await;
    resp.assert_status_ok();
    let list: Vec<serde_json::Value> = resp.json();
    assert_eq!(list.len(), 2);
    assert!(list[0]["authors"].is_array());
    assert!(list[0].get("abstract").is_none());

    // Search filter.
    let resp = server.get("/api/papers?q=attention").await;
    let hits: Vec<serde_json::Value> = resp.json();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0]["id"], "bbbb2222");

    // Detail includes abstract.
    let resp = server.get("/api/papers/aaaa1111").await;
    resp.assert_status_ok();
    let detail: serde_json::Value = resp.json();
    assert_eq!(detail["abstract"], "An abstract.");
    assert_eq!(detail["cite_key"], "aaaa1111");

    // Unknown id → 404.
    server
        .get("/api/papers/nope")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Stats.
    let resp = server.get("/api/stats").await;
    let s: serde_json::Value = resp.json();
    assert_eq!(s["total"], 2);
    assert_eq!(s["resolved"], 1);
    assert_eq!(s["needs_review"], 1);
}

#[tokio::test]
async fn paper_detail_includes_summary_when_present() {
    let (dir, pool) = temp_pool().await;
    db::insert_paper(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved),
    )
    .await
    .unwrap();
    db::insert_paper(
        &pool,
        &paper("bbbb2222", "Attention Is All You Need", PaperStatus::Resolved),
    )
    .await
    .unwrap();

    let summary = xuewen::summary::Summary {
        tldr: "A one-line summary.".into(),
        problem: "The identified gap.".into(),
        approach: "The proposed method.".into(),
        results: "+4.2 over baseline.".into(),
        limitations: "Small dataset.".into(),
    };
    xuewen::summary::store::upsert(&pool, "aaaa1111", &summary, "test-model")
        .await
        .unwrap();

    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();

    // Paper with a stored summary: the `summary` key is present with the
    // expected fields.
    let resp = server.get("/api/papers/aaaa1111").await;
    resp.assert_status_ok();
    let detail: serde_json::Value = resp.json();
    assert_eq!(detail["summary"]["tldr"], "A one-line summary.");
    assert_eq!(detail["summary"]["results"], "+4.2 over baseline.");

    // Paper with no summary: the `summary` key is absent entirely (serde
    // `skip_serializing_if` omits it, rather than serializing `null`).
    let resp = server.get("/api/papers/bbbb2222").await;
    resp.assert_status_ok();
    let detail: serde_json::Value = resp.json();
    assert!(detail.get("summary").is_none());
}

#[tokio::test]
async fn streams_pdf_with_range_and_guards_paths() {
    let (dir, pool) = temp_pool().await;
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();

    // A real paper whose PDF exists inside the library.
    let mut ok = paper("cccc3333", "A Paper", PaperStatus::Resolved);
    ok.rel_path = "cccc3333.pdf".into();
    common::write_test_pdf(&library.join("cccc3333.pdf"), &["Hello PDF"]);
    db::insert_paper(&pool, &ok).await.unwrap();

    // A rogue record whose rel_path escapes the library.
    let mut escape = paper("dddd4444", "Escape", PaperStatus::Resolved);
    escape.rel_path = "../outside.pdf".into();
    std::fs::write(dir.path().join("outside.pdf"), b"secret").unwrap();
    db::insert_paper(&pool, &escape).await.unwrap();

    let server = TestServer::new(build_router(pool, library.clone())).unwrap();

    // Full GET → 200, application/pdf.
    let resp = server.get("/papers/cccc3333/pdf").await;
    resp.assert_status_ok();
    assert_eq!(
        resp.header("content-type").to_str().unwrap(),
        "application/pdf"
    );
    let full_len = resp.as_bytes().len();
    assert!(full_len > 0);

    // Range request → 206 Partial Content, 100 bytes.
    let resp = server
        .get("/papers/cccc3333/pdf")
        .add_header(axum::http::header::RANGE, "bytes=0-99")
        .await;
    resp.assert_status(axum::http::StatusCode::PARTIAL_CONTENT);
    assert_eq!(resp.as_bytes().len(), 100);

    // Missing id → 404.
    server
        .get("/papers/zzzz9999/pdf")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Path-escaping record → 404 (guard rejects it, does NOT serve outside file).
    server
        .get("/papers/dddd4444/pdf")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn deletes_a_paper_softly() {
    let (dir, pool) = temp_pool().await;
    db::insert_paper(&pool, &paper("aaaa1111", "First", PaperStatus::Resolved))
        .await
        .unwrap();
    db::insert_paper(
        &pool,
        &paper("bbbb2222", "Second", PaperStatus::NeedsReview),
    )
    .await
    .unwrap();
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();

    // Before: both listed.
    assert_eq!(
        server
            .get("/api/papers")
            .await
            .json::<Vec<serde_json::Value>>()
            .len(),
        2
    );

    // DELETE one → 200, and it drops out of the active list + stats.
    server
        .delete("/api/papers/aaaa1111")
        .await
        .assert_status_ok();
    let list = server
        .get("/api/papers")
        .await
        .json::<Vec<serde_json::Value>>();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["id"], "bbbb2222");
    assert_eq!(
        server.get("/api/stats").await.json::<serde_json::Value>()["total"],
        1
    );

    // DELETE an unknown id → 404.
    server
        .delete("/api/papers/nope")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn imports_a_pdf_dedups_and_rejects_non_pdf() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Offline resolver: upstreams refuse instantly, so resolution degrades to
    // needs_review with no network wait (same trick as the watcher tests).
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());

    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: inbox.join("_processed"),
            },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server = TestServer::new(build_router_with_ingest(pool, library.clone(), ingest)).unwrap();

    // A real one-page PDF whose header has no DOI/arXiv id.
    let pdf_path = dir.path().join("paper.pdf");
    common::write_test_pdf(&pdf_path, &["A Paper With No Identifier"]);
    let pdf_bytes = std::fs::read(&pdf_path).unwrap();

    // Import -> 200 ingested, needs_review.
    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(pdf_bytes.clone()).file_name("paper.pdf"),
    );
    let resp = server.post("/api/papers").multipart(form).await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["outcome"], "ingested");
    assert_eq!(body["status"], "needs_review");

    // It now shows up in the list and the stats.
    assert_eq!(
        server
            .get("/api/papers")
            .await
            .json::<Vec<serde_json::Value>>()
            .len(),
        1
    );
    assert_eq!(
        server.get("/api/stats").await.json::<serde_json::Value>()["total"],
        1
    );

    // Re-import identical bytes -> 200 duplicate.
    let form2 =
        MultipartForm::new().add_part("file", Part::bytes(pdf_bytes).file_name("paper.pdf"));
    let dup: serde_json::Value = server.post("/api/papers").multipart(form2).await.json();
    assert_eq!(dup["outcome"], "duplicate");

    // Non-PDF bytes -> 400.
    let form3 = MultipartForm::new().add_part(
        "file",
        Part::bytes(b"not a pdf".to_vec()).file_name("x.pdf"),
    );
    server
        .post("/api/papers")
        .multipart(form3)
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn import_sanitizes_traversal_filenames() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());
    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: inbox.join("_processed"),
            },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server = TestServer::new(build_router_with_ingest(pool, library.clone(), ingest)).unwrap();

    let pdf_path = dir.path().join("p.pdf");
    common::write_test_pdf(&pdf_path, &["Traversal Test Paper"]);
    let bytes = std::fs::read(&pdf_path).unwrap();

    // A filename containing path separators must be reduced to its basename:
    // the upload still succeeds and nothing is written outside the staging dir.
    let form =
        MultipartForm::new().add_part("file", Part::bytes(bytes).file_name("nested/evil.pdf"));
    let resp = server.post("/api/papers").multipart(form).await;
    resp.assert_status_ok();
    assert_eq!(resp.json::<serde_json::Value>()["outcome"], "ingested");
    // No traversal artifacts landed outside the _uploads staging dir.
    assert!(!inbox.join("evil.pdf").exists());
    assert!(!inbox.join("nested").exists());
}

#[tokio::test]
async fn import_returns_503_when_not_configured() {
    let (dir, pool) = temp_pool().await;
    // The read-only router (no ingest bundle) must refuse uploads.
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();
    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(b"%PDF-1.4\n".to_vec()).file_name("x.pdf"),
    );
    server
        .post("/api/papers")
        .multipart(form)
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn import_reports_in_trash_for_deleted_paper() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());
    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: inbox.join("_processed"),
            },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server = TestServer::new(build_router_with_ingest(
        pool.clone(),
        library.clone(),
        ingest,
    ))
    .unwrap();

    let pdf_path = dir.path().join("paper.pdf");
    common::write_test_pdf(&pdf_path, &["A Paper With No Identifier"]);
    let pdf_bytes = std::fs::read(&pdf_path).unwrap();

    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(pdf_bytes.clone()).file_name("paper.pdf"),
    );
    let body: serde_json::Value = server.post("/api/papers").multipart(form).await.json();
    assert_eq!(body["outcome"], "ingested");
    let id = body["id"].as_str().unwrap().to_string();

    db::soft_delete(&pool, &id).await.unwrap();

    // Re-upload the same bytes → in_trash with the trashed paper's id.
    let form2 =
        MultipartForm::new().add_part("file", Part::bytes(pdf_bytes).file_name("paper.pdf"));
    let body2: serde_json::Value = server.post("/api/papers").multipart(form2).await.json();
    assert_eq!(body2["outcome"], "in_trash");
    assert_eq!(body2["id"], serde_json::json!(id));
}

#[tokio::test]
async fn import_reports_same_work_for_known_doi() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Seed an active paper that already owns this DOI.
    let mut existing = paper(
        "01890000-0000-7000-8000-0000000000aa",
        "Seed",
        PaperStatus::Resolved,
    );
    existing.meta.doi = Some("10.1000/xyz123".into());
    db::insert_paper(&pool, &existing).await.unwrap();

    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());
    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: inbox.join("_processed"),
            },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server = TestServer::new(build_router_with_ingest(pool, library.clone(), ingest)).unwrap();

    // Upload a different file whose first page carries the same DOI. The
    // resolver is offline, but the extracted identifier still lands in the
    // decided fields, so the identifier dedup fires.
    let pdf_path = dir.path().join("other.pdf");
    common::write_test_pdf(
        &pdf_path,
        &["A Different Upload", "https://doi.org/10.1000/xyz123"],
    );
    let form = MultipartForm::new().add_part(
        "file",
        Part::bytes(std::fs::read(&pdf_path).unwrap()).file_name("other.pdf"),
    );
    let body: serde_json::Value = server.post("/api/papers").multipart(form).await.json();
    assert_eq!(body["outcome"], "same_work");
    assert_eq!(body["id"], serde_json::json!(existing.id));
}

#[tokio::test]
async fn identify_search_returns_candidates() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();

    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/search/publ/api"))
        .respond_with(ResponseTemplate::new(200).set_body_string(DBLP_FIXTURE))
        .mount(&server)
        .await;
    // Crossref search 404s -> degrade to DBLP-only candidates.
    let resolver = Resolver::with_bases(None, server.uri(), server.uri())
        .unwrap()
        .with_dblp_base(server.uri());
    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: inbox.join("_processed"),
            },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server_http =
        TestServer::new(build_router_with_ingest(pool, library.clone(), ingest)).unwrap();

    let body: serde_json::Value = server_http
        .get("/api/identify/search")
        .add_query_param("q", "KGAT Knowledge Graph")
        .await
        .json();
    let arr = body.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(
        arr[0]["title"],
        "KGAT: Knowledge Graph Attention Network for Recommendation"
    );
    assert_eq!(arr[0]["source"], "dblp");
    assert!(arr[0]["authors"].is_array());

    // Empty q -> 400.
    server_http
        .get("/api/identify/search")
        .add_query_param("q", "  ")
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn identify_search_needs_ingest_context() {
    let (dir, pool) = temp_pool().await;
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();
    server
        .get("/api/identify/search")
        .add_query_param("q", "anything")
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn identify_applies_doi_candidate_and_guards() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();

    // Crossref mock for the direct-DOI path.
    let doi = "10.1145/3292500.3330701";
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path(format!("/works/{doi}")))
        .respond_with(ResponseTemplate::new(200).set_body_string(CROSSREF_FIXTURE))
        .mount(&mock)
        .await;
    let resolver = Resolver::with_bases(None, mock.uri(), mock.uri())
        .unwrap()
        .with_dblp_base(mock.uri());

    // Seed a needs_review paper whose PDF exists in the library.
    let mut p = paper(
        "01890000-0000-7000-8000-0000000000a1",
        "Provisional",
        PaperStatus::NeedsReview,
    );
    std::fs::create_dir_all(library.join("_unsorted")).unwrap();
    p.rel_path = format!("_unsorted/{}.pdf", p.content_hash);
    std::fs::write(library.join(&p.rel_path), b"%PDF-1.4 fake").unwrap();
    db::insert_paper(&pool, &p).await.unwrap();

    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: inbox.join("_processed"),
            },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server = TestServer::new(build_router_with_ingest(
        pool.clone(),
        library.clone(),
        ingest,
    ))
    .unwrap();

    // Direct DOI identify: 200, resolved, re-filed to the cite-key path.
    let resp = server
        .post(&format!("/api/papers/{}/identify", p.id))
        .json(&serde_json::json!({ "doi": doi }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "resolved");
    assert_eq!(
        body["title"],
        "KGAT: Knowledge Graph Attention Network for Recommendation"
    );
    assert_eq!(body["cite_key"], "wang2019kgat");
    assert!(library.join("wang2019kgat.pdf").exists());

    // Unknown paper id -> 404.
    server
        .post("/api/papers/01890000-0000-7000-8000-00000000dead/identify")
        .json(&serde_json::json!({ "doi": doi }))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // A second paper trying to claim the SAME DOI -> 409 naming the winner.
    let mut q = paper(
        "01890000-0000-7000-8000-0000000000b2",
        "Other",
        PaperStatus::NeedsReview,
    );
    q.content_hash = "otherhash".into();
    q.rel_path = "_unsorted/otherhash.pdf".into();
    std::fs::write(library.join(&q.rel_path), b"%PDF-1.4 other").unwrap();
    db::insert_paper(&pool, &q).await.unwrap();
    let resp = server
        .post(&format!("/api/papers/{}/identify", q.id))
        .json(&serde_json::json!({ "doi": doi }))
        .await;
    resp.assert_status(axum::http::StatusCode::CONFLICT);
    let body: serde_json::Value = resp.json();
    assert_eq!(body["id"], serde_json::json!(p.id));

    // Trashed paper -> 409.
    db::soft_delete(&pool, &q.id).await.unwrap();
    server
        .post(&format!("/api/papers/{}/identify", q.id))
        .json(&serde_json::json!({ "candidate": {
            "title": "T", "abstract": null, "authors": [], "venue": null,
            "year": null, "doi": null, "arxiv_id": null, "dblp_key": null,
            "url": null, "source": "dblp"
        }}))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);

    // Bad body (two selectors) -> 400.
    db::restore(&pool, &q.id).await.unwrap();
    server
        .post(&format!("/api/papers/{}/identify", q.id))
        .json(&serde_json::json!({ "doi": "10.1/x", "arxiv_id": "2001.00001" }))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);

    // Unresolvable DOI (mock has no route for it -> 404 upstream) -> 404,
    // with an actionable hint pointing at the title-search path.
    let resp = server
        .post(&format!("/api/papers/{}/identify", q.id))
        .json(&serde_json::json!({ "doi": "10.9999/nope" }))
        .await;
    resp.assert_status(axum::http::StatusCode::NOT_FOUND);
    let body: serde_json::Value = resp.json();
    assert!(
        body["error"]
            .as_str()
            .unwrap()
            .contains("try a title search"),
        "not-found error should suggest the title path, got: {body}"
    );
}

#[tokio::test]
async fn identify_applies_candidate_without_network() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string());

    let mut p = paper(
        "01890000-0000-7000-8000-00000000c001",
        "Prov",
        PaperStatus::NeedsReview,
    );
    std::fs::create_dir_all(library.join("_unsorted")).unwrap();
    p.rel_path = format!("_unsorted/{}.pdf", p.content_hash);
    std::fs::write(library.join(&p.rel_path), b"%PDF-1.4 fake").unwrap();
    db::insert_paper(&pool, &p).await.unwrap();

    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: inbox.join("_processed"),
            },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server = TestServer::new(build_router_with_ingest(pool, library.clone(), ingest)).unwrap();

    let resp = server
        .post(&format!("/api/papers/{}/identify", p.id))
        .json(&serde_json::json!({ "candidate": {
            "title": "AntiFuzz: Impeding Fuzzing Audits of Binary Executables",
            "abstract": null,
            "authors": ["Emre Güler", "Cornelius Aschermann", "Ali Abbasi", "Thorsten Holz"],
            "venue": "USENIX Security Symposium",
            "year": 2019,
            "doi": null, "arxiv_id": null,
            "dblp_key": "conf/uss/GulerAAH19",
            "url": "https://www.usenix.org/conference/usenixsecurity19/presentation/guler",
            "source": "dblp"
        }}))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "resolved");
    assert_eq!(body["cite_key"], "guler2019antifuzz");
    assert_eq!(body["source"], "dblp");
    assert!(library.join("guler2019antifuzz.pdf").exists());
}

#[tokio::test]
async fn identify_needs_ingest_context() {
    let (dir, pool) = temp_pool().await;
    // The read-only router (no ingest bundle) must refuse identify writes.
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();
    server
        .post("/api/papers/whatever/identify")
        .json(&serde_json::json!({ "doi": "10.1/x" }))
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn identify_applies_arxiv_id_via_api() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();

    // arXiv mock for the direct-id path.
    let mock = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/api/query"))
        .respond_with(ResponseTemplate::new(200).set_body_string(ARXIV_FIXTURE))
        .mount(&mock)
        .await;
    let resolver = Resolver::with_bases(None, mock.uri(), mock.uri())
        .unwrap()
        .with_dblp_base(mock.uri());

    let mut p = paper(
        "01890000-0000-7000-8000-00000000d001",
        "Prov",
        PaperStatus::NeedsReview,
    );
    std::fs::create_dir_all(library.join("_unsorted")).unwrap();
    p.rel_path = format!("_unsorted/{}.pdf", p.content_hash);
    std::fs::write(library.join(&p.rel_path), b"%PDF-1.4 fake").unwrap();
    db::insert_paper(&pool, &p).await.unwrap();

    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: inbox.join("_processed"),
            },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server = TestServer::new(build_router_with_ingest(pool, library.clone(), ingest)).unwrap();

    let resp = server
        .post(&format!("/api/papers/{}/identify", p.id))
        .json(&serde_json::json!({ "arxiv_id": "1706.03762" }))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["status"], "resolved");
    assert_eq!(body["title"], "Attention Is All You Need");
    assert_eq!(body["cite_key"], "vaswani2017attention");
    assert!(library.join("vaswani2017attention.pdf").exists());
}

#[tokio::test]
async fn settings_report_and_set_proxy_cookie() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".into(),
        "http://127.0.0.1:1".into(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".into());
    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: inbox.join("_processed"),
            },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server = TestServer::new(build_router_with_ingest_proxy(
        pool.clone(),
        library.clone(),
        ingest,
        Some("https://proxy.uchicago.edu/login?url=".into()),
    ))
    .unwrap();

    // Initially unset.
    let s: serde_json::Value = server.get("/api/settings").await.json();
    assert_eq!(s["proxy_cookie_set"], false);

    // Set it.
    server
        .put("/api/settings/proxy-cookie")
        .json(&serde_json::json!({ "cookie": "ezproxy=abc" }))
        .await
        .assert_status_ok();
    let s: serde_json::Value = server.get("/api/settings").await.json();
    assert_eq!(s["proxy_cookie_set"], true);
    assert!(s["proxy_cookie_updated_at"].is_string());
    // The value is never echoed.
    assert!(s.get("cookie").is_none());

    // Clear it.
    server
        .delete("/api/settings/proxy-cookie")
        .await
        .assert_status_ok();
    let s: serde_json::Value = server.get("/api/settings").await.json();
    assert_eq!(s["proxy_cookie_set"], false);
}

#[tokio::test]
async fn settings_reports_fold_abstract() {
    // The read-only test router builds its AppState with `UiConfig::default()`
    // (fold_abstract: true), so this exercises the default straight through.
    let (dir, pool) = temp_pool().await;
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();

    let res = server.get("/api/settings").await;
    res.assert_status_ok();
    assert_eq!(
        res.json::<serde_json::Value>()["fold_abstract"],
        serde_json::json!(true)
    );
}

#[tokio::test]
async fn import_url_rejects_unsupported_input() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    let library = dir.path().join("library");
    std::fs::create_dir_all(&inbox).unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    let resolver = Resolver::with_bases(
        None,
        "http://127.0.0.1:1".into(),
        "http://127.0.0.1:1".into(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".into());
    let ingest = std::sync::Arc::new(Ingest {
        ctx: IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: inbox.join("_processed"),
            },
            resolver,
            grobid: None,
        },
        staging_dir: inbox.join("_uploads"),
    });
    let server =
        TestServer::new(build_router_with_ingest_proxy(pool, library, ingest, None)).unwrap();

    server
        .post("/api/import")
        .json(&serde_json::json!({ "input": "just a title, not an id" }))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn projects_crud_membership_and_filter() {
    let (dir, pool) = temp_pool().await;
    db::insert_paper(&pool, &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved))
        .await
        .unwrap();
    db::insert_paper(&pool, &paper("bbbb2222", "Attention Is All You Need", PaperStatus::Resolved))
        .await
        .unwrap();
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();

    // Create.
    let resp = server
        .post("/api/projects")
        .json(&serde_json::json!({"name": "Survey", "note": "draft"}))
        .await;
    resp.assert_status(axum::http::StatusCode::CREATED);
    let proj: serde_json::Value = resp.json();
    let pid = proj["id"].as_str().unwrap().to_string();
    assert_eq!(proj["name"], "Survey");

    // Duplicate name -> 409.
    server
        .post("/api/projects")
        .json(&serde_json::json!({"name": "survey"}))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);

    // Add membership (idempotent) -> 204.
    server
        .put(&format!("/api/papers/aaaa1111/projects/{pid}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .put(&format!("/api/papers/aaaa1111/projects/{pid}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);

    // Membership to a missing paper -> 404.
    server
        .put(&format!("/api/papers/nope/projects/{pid}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // List shows count 1.
    let list: Vec<serde_json::Value> = server.get("/api/projects").await.json();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["paper_count"], 1);

    // Detail carries project_ids.
    let detail: serde_json::Value = server.get("/api/papers/aaaa1111").await.json();
    assert_eq!(detail["project_ids"], serde_json::json!([pid]));

    // Filter list by project.
    let filtered: Vec<serde_json::Value> =
        server.get(&format!("/api/papers?project={pid}")).await.json();
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0]["id"], "aaaa1111");

    // Remove membership -> 204, then 404 when absent.
    server
        .delete(&format!("/api/papers/aaaa1111/projects/{pid}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    server
        .delete(&format!("/api/papers/aaaa1111/projects/{pid}"))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Delete project -> 204.
    server
        .delete(&format!("/api/projects/{pid}"))
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn patch_project_merge_rules() {
    let (dir, pool) = temp_pool().await;
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();

    // Seed two projects: one to edit, one to collide names with.
    let a: serde_json::Value = server
        .post("/api/projects")
        .json(&serde_json::json!({"name": "Survey", "note": "draft"}))
        .await
        .json();
    let a_id = a["id"].as_str().unwrap().to_string();
    server
        .post("/api/projects")
        .json(&serde_json::json!({"name": "Other"}))
        .await
        .assert_status(axum::http::StatusCode::CREATED);

    // (a) Rename → 200, name changed, note preserved (note omitted).
    let resp = server
        .patch(&format!("/api/projects/{a_id}"))
        .json(&serde_json::json!({"name": "Survey v2"}))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["name"], "Survey v2");
    // (c) Omitting note preserves the existing note.
    assert_eq!(body["note"], "draft");

    // (b) {"note":""} clears the note to null.
    let resp = server
        .patch(&format!("/api/projects/{a_id}"))
        .json(&serde_json::json!({"note": ""}))
        .await;
    resp.assert_status_ok();
    let body: serde_json::Value = resp.json();
    assert_eq!(body["note"], serde_json::Value::Null);
    // Name unchanged when name omitted.
    assert_eq!(body["name"], "Survey v2");

    // (d) Rename onto another project's name → 409.
    server
        .patch(&format!("/api/projects/{a_id}"))
        .json(&serde_json::json!({"name": "other"}))
        .await
        .assert_status(axum::http::StatusCode::CONFLICT);

    // (e) PATCH an unknown id → 404.
    server
        .patch("/api/projects/does-not-exist")
        .json(&serde_json::json!({"name": "whatever"}))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn import_url_needs_ingest_context() {
    let (dir, pool) = temp_pool().await;
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();
    server
        .post("/api/import")
        .json(&serde_json::json!({ "input": "10.1145/x" }))
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn exports_bibtex_and_biblatex() {
    let (dir, pool) = temp_pool().await;
    db::insert_paper(&pool, &paper("aaaa1111", "Deep Residual Learning", PaperStatus::Resolved))
        .await
        .unwrap();
    db::insert_paper(&pool, &paper("bbbb2222", "Attention Is All You Need", PaperStatus::Resolved))
        .await
        .unwrap();
    let proj = db::create_project(&pool, "Survey", None).await.unwrap();
    db::add_paper_to_project(&pool, "aaaa1111", &proj.id).await.unwrap();
    let server = TestServer::new(build_router(pool, dir.path().join("library"))).unwrap();

    // Individual (default bibtex). The `paper` helper sets venue=KDD, no dblp_key -> @article.
    let resp = server.get("/api/papers/aaaa1111/export").await;
    resp.assert_status_ok();
    assert_eq!(
        resp.header("content-type"),
        "text/plain; charset=utf-8",
        "single-entry export should be inline plain text"
    );
    let text = resp.text();
    assert!(text.contains("@article{aaaa1111,"), "got: {text}");
    assert!(text.contains("journal = {KDD},"));

    // BibLaTeX switches the field names.
    let bl = server.get("/api/papers/aaaa1111/export?format=biblatex").await.text();
    assert!(bl.contains("journaltitle = {KDD},"), "got: {bl}");
    assert!(bl.contains("date = {2020},"));

    // Unknown id -> 404.
    server
        .get("/api/papers/nope/export")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Batch: whole library has both entries, served as a downloadable .bib file.
    let all = server.get("/api/papers/export").await;
    all.assert_status_ok();
    assert_eq!(all.header("content-type"), "application/x-bibtex");
    let disposition = all.header("content-disposition");
    let disposition = disposition.to_str().unwrap();
    assert!(disposition.contains("attachment"), "got: {disposition}");
    assert!(disposition.contains("xuewen.bib"), "got: {disposition}");
    let all_text = all.text();
    assert!(all_text.contains("@article{aaaa1111,"));
    assert!(all_text.contains("@article{bbbb2222,"));

    // Batch filtered by project -> only that project's paper.
    let scoped = server.get(&format!("/api/papers/export?project={}", proj.id)).await.text();
    assert!(scoped.contains("aaaa1111"));
    assert!(!scoped.contains("bbbb2222"));
}

mod search_api {
    use super::*; // reuse the file's pool/server helpers
    use xuewen::search::{fts, vector, SearchService};

    // This file's pool/paper helpers are named `temp_pool`/`paper` (not
    // `test_pool`/`insert_sample_paper` as in the brief); these two small
    // wrappers adapt to that existing local style.
    async fn test_pool() -> sqlx::SqlitePool {
        let (dir, pool) = temp_pool().await;
        std::mem::forget(dir); // keep the sqlite file alive for the test's duration
        pool
    }

    async fn insert_sample_paper(pool: &sqlx::SqlitePool, id: &str, title: &str) {
        db::insert_paper(pool, &paper(id, title, PaperStatus::Resolved))
            .await
            .unwrap();
    }

    async fn server_with_search(pool: sqlx::SqlitePool) -> axum_test::TestServer {
        let idx = tempfile::tempdir().unwrap();
        let (fts_idx, _) = fts::FtsIndex::open(idx.path()).unwrap();
        std::mem::forget(idx);
        let vectors = vector::QdrantStore::new("http://127.0.0.1:1", "xuewen", 4).unwrap();
        let svc = SearchService::open_with(pool.clone(), fts_idx, vectors, None);
        svc.fts
            .upsert(&fts::PaperDoc {
                id: "p1".into(),
                title: "Fuzzing Firmware".into(),
                authors: "Ada Lovelace".into(),
                venue: String::new(),
                abstract_text: String::new(),
                body: "router dictionaries".into(),
            })
            .unwrap();
        let router = xuewen::web::build_router_with_search(
            pool,
            std::path::PathBuf::from("/nonexistent"),
            svc,
        );
        axum_test::TestServer::new(router).unwrap()
    }

    #[tokio::test]
    async fn search_returns_papers_with_match_info() {
        let pool = test_pool().await; // the file's existing helper
        insert_sample_paper(&pool, "p1", "Fuzzing Firmware").await; // existing helper or add one
        let server = server_with_search(pool).await;

        let resp = server.get("/api/search").add_query_param("q", "fuzzing").await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["semantic"]["available"], false);
        assert_eq!(body["results"][0]["paper"]["id"], "p1");
        assert_eq!(body["results"][0]["match"]["engine"], "keyword");
        assert!(body["results"][0]["match"]["snippet"].as_str().unwrap().contains("<mark>"));
    }

    #[tokio::test]
    async fn fields_param_restricts_search() {
        let pool = test_pool().await;
        insert_sample_paper(&pool, "p1", "Fuzzing Firmware").await;
        let server = server_with_search(pool).await;
        let resp = server
            .get("/api/search")
            .add_query_param("q", "dictionaries")
            .add_query_param("fields", "title")
            .await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert_eq!(body["results"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn status_reports_tiers() {
        let pool = test_pool().await;
        insert_sample_paper(&pool, "p1", "Fuzzing Firmware").await;
        let server = server_with_search(pool).await;
        let resp = server.get("/api/search/status").await;
        resp.assert_status_ok();
        let body: serde_json::Value = resp.json();
        assert!(body["fts"]["pending"].as_i64().unwrap() >= 1);
        assert_eq!(body["semantic_available"], false);
    }

    #[tokio::test]
    async fn search_without_service_is_503() {
        let pool = test_pool().await;
        let router = xuewen::web::build_router(pool, std::path::PathBuf::from("/nonexistent"));
        let server = axum_test::TestServer::new(router).unwrap();
        let resp = server.get("/api/search").add_query_param("q", "x").await;
        resp.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
    }
}
