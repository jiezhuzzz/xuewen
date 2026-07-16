mod common;

use axum_test::TestServer;

#[tokio::test]
async fn translate_returns_503_when_disabled() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(xuewen::web::build_router(pool, root)).unwrap();
    server
        .post("/api/translate")
        .json(&serde_json::json!({"text": "hello"}))
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn settings_report_translate_disabled_by_default() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(xuewen::web::build_router(pool, root)).unwrap();
    let resp = server.get("/api/settings").await;
    resp.assert_status_ok();
    let v: serde_json::Value = resp.json();
    assert_eq!(v["translate"]["enabled"], false);
}
