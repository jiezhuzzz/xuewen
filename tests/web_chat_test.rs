mod common;

use axum_test::TestServer;
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};
use xuewen::config::{ChatConfig, ChatModelConfig};

fn chat_cfg(base_url: &str) -> ChatConfig {
    ChatConfig {
        models: vec![ChatModelConfig {
            label: "Mock Model".into(),
            base_url: base_url.into(),
            model: "mock-1".into(),
            api_key: None,
            api_key_env: "XUEWEN_TEST_UNSET".into(),
        }],
        max_context_chars: 60_000,
    }
}

#[tokio::test]
async fn models_report_unavailable_without_config() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    let server = TestServer::new(xuewen::web::build_router(pool, root)).unwrap();
    let resp = server.get("/api/chat/models").await;
    resp.assert_status_ok();
    let v: serde_json::Value = resp.json();
    assert_eq!(v["available"], false);
}

#[tokio::test]
async fn models_list_labels_but_never_keys() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    let chat = xuewen::chat::ChatService::from_config(&chat_cfg("http://example.invalid")).unwrap();
    let server = TestServer::new(xuewen::web::build_router_with_chat(pool, root, chat)).unwrap();
    let resp = server.get("/api/chat/models").await;
    let v: serde_json::Value = resp.json();
    assert_eq!(v["available"], true);
    assert_eq!(v["models"][0]["id"], "0");
    assert_eq!(v["models"][0]["label"], "Mock Model");
    let raw = resp.text();
    assert!(
        !raw.contains("base_url") && !raw.contains("api_key"),
        "no provider details leak"
    );
}

#[tokio::test]
async fn send_streams_deltas_and_persists_the_exchange() {
    let upstream = MockServer::start().await;
    let sse = "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n\
               data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
               data: [DONE]\n\n";
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(sse, "text/event-stream"))
        .mount(&upstream)
        .await;

    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    let chat = xuewen::chat::ChatService::from_config(&chat_cfg(&upstream.uri())).unwrap();
    let server = TestServer::new(xuewen::web::build_router_with_chat(
        pool.clone(),
        root,
        chat,
    ))
    .unwrap();

    let resp = server
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "0", "message": "what is this?"}))
        .await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(body.contains("event: delta"), "body: {body}");
    assert!(body.contains("Hel"));
    assert!(body.contains("event: done"));

    let rows = xuewen::chat::store::list(&pool, "p1").await.unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].role, "user");
    assert_eq!(rows[1].content, "Hello");
    assert_eq!(rows[1].model.as_deref(), Some("Mock Model"));
}

#[tokio::test]
async fn send_validates_model_message_paper_and_config() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await;

    // 503 when chat is unconfigured.
    let plain = TestServer::new(xuewen::web::build_router(pool.clone(), root.clone())).unwrap();
    plain
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "0", "message": "hi"}))
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);

    let chat = xuewen::chat::ChatService::from_config(&chat_cfg("http://example.invalid")).unwrap();
    let server = TestServer::new(xuewen::web::build_router_with_chat(pool, root, chat)).unwrap();
    // 400: unknown model id; 400: empty message; 404: unknown paper.
    server
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "9", "message": "hi"}))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    server
        .post("/api/papers/p1/chat")
        .json(&json!({"model_id": "0", "message": "   "}))
        .await
        .assert_status(axum::http::StatusCode::BAD_REQUEST);
    server
        .post("/api/papers/nope/chat")
        .json(&json!({"model_id": "0", "message": "hi"}))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn history_roundtrip_and_clear() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    xuewen::chat::store::insert_exchange(&pool, "p1", "q", "a", "M")
        .await
        .unwrap();
    let chat = xuewen::chat::ChatService::from_config(&chat_cfg("http://example.invalid")).unwrap();
    let server = TestServer::new(xuewen::web::build_router_with_chat(pool, root, chat)).unwrap();

    let rows: serde_json::Value = server.get("/api/papers/p1/chat").await.json();
    assert_eq!(rows.as_array().unwrap().len(), 2);

    server
        .delete("/api/papers/p1/chat")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let rows: serde_json::Value = server.get("/api/papers/p1/chat").await.json();
    assert_eq!(rows.as_array().unwrap().len(), 0);

    server
        .get("/api/papers/nope/chat")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn send_with_empty_reply_errors_and_persists_nothing() {
    let upstream = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200).set_body_raw("data: [DONE]\n\n", "text/event-stream"),
        )
        .mount(&upstream)
        .await;

    let (pool, root) = common::pool_and_root_with_paper("p1").await;
    let chat = xuewen::chat::ChatService::from_config(&chat_cfg(&upstream.uri())).unwrap();
    let server = TestServer::new(xuewen::web::build_router_with_chat(
        pool.clone(),
        root,
        chat,
    ))
    .unwrap();

    let resp = server
        .post("/api/papers/p1/chat")
        .json(&serde_json::json!({"model_id": "0", "message": "hi"}))
        .await;
    resp.assert_status_ok();
    let body = resp.text();
    assert!(body.contains("event: error"), "body: {body}");
    assert!(body.contains("empty reply"));
    assert!(!body.contains("event: done"));

    let rows = xuewen::chat::store::list(&pool, "p1").await.unwrap();
    assert!(rows.is_empty(), "nothing may persist for an empty reply");
}
