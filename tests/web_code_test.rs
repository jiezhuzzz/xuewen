mod common;

use axum_test::TestServer;
use serde_json::json;
use std::path::PathBuf;
use xuewen::agent::AgentService;
use xuewen::config::{AgentBackendConfig, AgentConfig};

fn stub_agent() -> std::sync::Arc<AgentService> {
    AgentService::from_config(&AgentConfig {
        claude_code: Some(AgentBackendConfig::default()),
        runner: Some(PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/stub_runner.mjs"
        ))),
        ..AgentConfig::default()
    })
    .unwrap()
}

#[tokio::test]
async fn code_endpoints_gate_validate_and_report() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await;

    // 503 without the agent service.
    let plain = TestServer::new(xuewen::web::build_router(pool.clone(), root.clone())).unwrap();
    plain
        .put("/api/papers/p1/code")
        .json(&json!({"repo_url": "https://github.com/x/y"}))
        .await
        .assert_status(axum::http::StatusCode::SERVICE_UNAVAILABLE);

    let server = TestServer::new(xuewen::web::build_router_with_agent(
        pool,
        root,
        stub_agent(),
    ))
    .unwrap();

    // Nothing attached yet.
    let v: serde_json::Value = server.get("/api/papers/p1/code").await.json();
    assert_eq!(v["attached"], false);

    // Bad URLs are rejected up front.
    server
        .put("/api/papers/p1/code")
        .json(&json!({"repo_url": "git@github.com:x/y.git"}))
        .await
        .assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);

    // A valid URL is accepted and the row enters 'cloning' (the background
    // clone then fails offline, flipping it to 'error' — poll for terminal).
    let resp = server
        .put("/api/papers/p1/code")
        .json(&json!({"repo_url": "https://invalid.example/none.git"}))
        .await;
    resp.assert_status(axum::http::StatusCode::ACCEPTED);
    let mut status = String::new();
    for _ in 0..100 {
        let v: serde_json::Value = server.get("/api/papers/p1/code").await.json();
        status = v["code"]["status"].as_str().unwrap_or_default().to_string();
        if status != "cloning" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    assert_eq!(status, "error");

    // Detach removes row and checkout.
    server
        .delete("/api/papers/p1/code")
        .await
        .assert_status(axum::http::StatusCode::NO_CONTENT);
    let v: serde_json::Value = server.get("/api/papers/p1/code").await.json();
    assert_eq!(v["attached"], false);

    // Unknown paper -> 404.
    server
        .get("/api/papers/missing/code")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}
