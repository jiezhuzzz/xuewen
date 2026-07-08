mod common;

use axum_test::TestServer;
use xuewen::db;
use xuewen::models::Paper;
use xuewen::web::build_router;

async fn temp_pool() -> (tempfile::TempDir, sqlx::SqlitePool) {
    let dir = tempfile::tempdir().unwrap();
    let url = format!("sqlite:{}", dir.path().join("t.db").display());
    let pool = db::connect(&url).await.unwrap();
    (dir, pool)
}

fn paper(id: &str, title: &str, status: &str) -> Paper {
    Paper {
        id: id.into(),
        content_hash: id.into(),
        rel_path: format!("{id}.pdf"),
        title: Some(title.into()),
        abstract_text: Some("An abstract.".into()),
        authors: Some(r#"["Ada Lovelace"]"#.into()),
        venue: Some("KDD".into()),
        year: Some(2020),
        doi: None,
        arxiv_id: None,
        dblp_key: None,
        cite_key: Some(id.into()),
        url: None,
        source: Some("crossref".into()),
        status: status.into(),
        added_at: "2026-07-07T00:00:00Z".into(),
        deleted_at: None,
    }
}

#[tokio::test]
async fn lists_and_details_papers() {
    let (dir, pool) = temp_pool().await;
    db::insert_paper(
        &pool,
        &paper("aaaa1111", "Deep Residual Learning", "resolved"),
    )
    .await
    .unwrap();
    db::insert_paper(
        &pool,
        &paper("bbbb2222", "Attention Is All You Need", "needs_review"),
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
async fn streams_pdf_with_range_and_guards_paths() {
    let (dir, pool) = temp_pool().await;
    let library = dir.path().join("library");
    std::fs::create_dir_all(&library).unwrap();

    // A real paper whose PDF exists inside the library.
    let mut ok = paper("cccc3333", "A Paper", "resolved");
    ok.rel_path = "cccc3333.pdf".into();
    common::write_test_pdf(&library.join("cccc3333.pdf"), &["Hello PDF"]);
    db::insert_paper(&pool, &ok).await.unwrap();

    // A rogue record whose rel_path escapes the library.
    let mut escape = paper("dddd4444", "Escape", "resolved");
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
    db::insert_paper(&pool, &paper("aaaa1111", "First", "resolved"))
        .await
        .unwrap();
    db::insert_paper(&pool, &paper("bbbb2222", "Second", "needs_review"))
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
