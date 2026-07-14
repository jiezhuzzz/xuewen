mod common;

use axum_test::TestServer;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn chat_reply(text: &str) -> serde_json::Value {
    json!({"choices": [{"message": {"role": "assistant", "content": text}}]})
}

#[tokio::test]
async fn parses_references_and_caches() {
    let upstream = MockServer::start().await;
    let parsed = r#"[{"i":1,"authors":["A. Author"],"title":"Adam","venue":"ICLR","year":2015,"doi":null,"arxiv_id":null,"url":null}]"#;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(parsed)))
        .expect(1)
        .mount(&upstream)
        .await;

    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    let svc = xuewen::citations::CitationsService::for_tests(pool.clone(), &upstream.uri(), "m");
    let server =
        TestServer::new(xuewen::web::build_router_with_citations(pool, root, svc)).unwrap();

    let body = json!({"references": ["[1] A. Author. Adam. ICLR 2015."]});
    let resp = server.post("/api/papers/p1/citations").json(&body).await;
    resp.assert_status_ok();
    let v: serde_json::Value = resp.json();
    assert_eq!(v["references"][0]["title"], "Adam");

    // Second call: cache hit — the upstream mock's .expect(1) enforces no 2nd LLM call.
    let resp2 = server.post("/api/papers/p1/citations").json(&body).await;
    resp2.assert_status_ok();
}

#[tokio::test]
async fn unconfigured_returns_503_and_unknown_paper_404() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    // Plain router: no citations service.
    let server = TestServer::new(xuewen::web::build_router(pool.clone(), root.clone())).unwrap();
    let resp = server
        .post("/api/papers/p1/citations")
        .json(&json!({"references": ["x"]}))
        .await;
    resp.assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);

    let upstream = MockServer::start().await;
    let svc = xuewen::citations::CitationsService::for_tests(pool.clone(), &upstream.uri(), "m");
    let server =
        TestServer::new(xuewen::web::build_router_with_citations(pool, root, svc)).unwrap();
    let resp = server
        .post("/api/papers/nope/citations")
        .json(&json!({"references": ["x"]}))
        .await;
    resp.assert_status_not_found();
}

#[tokio::test]
async fn empty_references_is_bad_request() {
    let upstream = MockServer::start().await;
    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    let svc = xuewen::citations::CitationsService::for_tests(pool.clone(), &upstream.uri(), "m");
    let server =
        TestServer::new(xuewen::web::build_router_with_citations(pool, root, svc)).unwrap();

    let resp = server
        .post("/api/papers/p1/citations")
        .json(&json!({"references": []}))
        .await;
    resp.assert_status(axum::http::StatusCode::BAD_REQUEST);
}
