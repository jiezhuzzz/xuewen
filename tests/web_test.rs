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
