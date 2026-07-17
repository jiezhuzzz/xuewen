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

    // Bad URLs are rejected up front with JSON error body.
    let bad_url_resp = server
        .put("/api/papers/p1/code")
        .json(&json!({"repo_url": "git@github.com:x/y.git"}))
        .await;
    bad_url_resp.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    let bad_url_json: serde_json::Value = bad_url_resp.json();
    assert!(bad_url_json.get("error").is_some());
    assert!(
        bad_url_json["error"]
            .as_str()
            .map(|s| s.contains("https"))
            .unwrap_or(false)
            || bad_url_json["error"]
                .as_str()
                .map(|s| s.contains("credentials"))
                .unwrap_or(false),
        "error message should mention https or credentials, got: {:?}",
        bad_url_json["error"]
    );

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

    // Unknown paper PUT -> 404.
    server
        .put("/api/papers/missing/code")
        .json(&json!({"repo_url": "https://github.com/x/y"}))
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);

    // Unknown paper DELETE -> 404.
    server
        .delete("/api/papers/missing/code")
        .await
        .assert_status(axum::http::StatusCode::NOT_FOUND);
}

/// Regression for the whole-branch-review purge bug: `paper_code` had no
/// `ON DELETE CASCADE` on its `paper_id` FK, so purging a paper with an
/// attached repo hit `FOREIGN KEY constraint failed` after the PDF was
/// already removed. This exercises the same sequence `xuewen purge` runs at
/// the db/fs level (chat clear, explicit `delete_paper_code`, agent
/// workspace removal, then `delete_row`) and asserts everything is gone
/// afterward — both belt-and-braces (explicit delete) and the migration's
/// cascade are covered, since the explicit `delete_paper_code` call would
/// mask a cascade regression on its own.
#[tokio::test]
async fn purge_clears_paper_code_and_agent_workspace() {
    let (pool, root) = common::pool_and_root_with_paper("p1").await;

    xuewen::db::upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/y")
        .await
        .unwrap();
    assert!(xuewen::db::get_paper_code(&pool, "p1")
        .await
        .unwrap()
        .is_some());

    let ws = xuewen::agent::workspace_dir(&root, "p1");
    let repo_dir = ws.join("repo");
    std::fs::create_dir_all(&repo_dir).unwrap();
    std::fs::write(repo_dir.join("dummy.txt"), b"hello").unwrap();
    assert!(ws.exists());

    // Mirror `Command::Purge`'s per-paper cleanup sequence in src/main.rs.
    xuewen::chat::store::clear(&pool, "p1").await.unwrap();
    xuewen::db::delete_paper_code(&pool, "p1").await.unwrap();
    tokio::fs::remove_dir_all(&ws).await.unwrap();
    xuewen::db::delete_row(&pool, "p1").await.unwrap();

    assert!(xuewen::db::get_paper_code(&pool, "p1")
        .await
        .unwrap()
        .is_none());
    assert!(!ws.exists());
    let row: Option<(String,)> = sqlx::query_as("SELECT id FROM papers WHERE id = ?")
        .bind("p1")
        .fetch_optional(&pool)
        .await
        .unwrap();
    assert!(row.is_none());
}

/// Isolates the migration's `ON DELETE CASCADE` itself: deletes the papers
/// row directly (no explicit `delete_paper_code` first) and asserts the FK
/// cascade alone removes the attached `paper_code` row, rather than failing
/// with `FOREIGN KEY constraint failed`.
#[tokio::test]
async fn deleting_paper_cascades_to_paper_code() {
    let (pool, _root) = common::pool_and_root_with_paper("p1").await;

    xuewen::db::upsert_paper_code_cloning(&pool, "p1", "https://github.com/x/y")
        .await
        .unwrap();
    assert!(xuewen::db::get_paper_code(&pool, "p1")
        .await
        .unwrap()
        .is_some());

    sqlx::query("DELETE FROM papers WHERE id = ?")
        .bind("p1")
        .execute(&pool)
        .await
        .expect("delete should succeed under the ON DELETE CASCADE fix");

    assert!(xuewen::db::get_paper_code(&pool, "p1")
        .await
        .unwrap()
        .is_none());
}
