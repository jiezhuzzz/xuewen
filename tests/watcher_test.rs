mod common;

use std::time::Duration;

use xuewen::db;
use xuewen::pipeline::Libraries;
use xuewen::resolve::Resolver;

// Upstreams refuse instantly -> lookups degrade to Unresolved (offline, fast).
fn offline_resolver() -> Resolver {
    Resolver::with_bases(
        None,
        "http://127.0.0.1:1".to_string(),
        "http://127.0.0.1:1".to_string(),
    )
    .unwrap()
    .with_dblp_base("http://127.0.0.1:1".to_string())
}

async fn wait_for<F: Fn() -> bool>(cond: F, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    cond()
}

#[tokio::test]
async fn watcher_catches_up_and_watches_new_files() {
    let dir = tempfile::tempdir().unwrap();
    let inbox = dir.path().join("inbox");
    std::fs::create_dir_all(&inbox).unwrap();

    // One file present BEFORE the watcher starts (exercises catch-up)...
    common::write_test_pdf(&inbox.join("existing.pdf"), &["Existing Paper Title Here"]);

    let url = format!("sqlite:{}", dir.path().join("library.db").display());

    // Move owned values into the watcher task (so it satisfies 'static).
    let inbox_for_task = inbox.clone();
    let db_url = url.clone();
    let handle = tokio::spawn(async move {
        let pool = db::connect(&db_url).await.unwrap();
        let dirs = Libraries {
            library_root: inbox_for_task.parent().unwrap().join("library"),
            processed_dir: inbox_for_task.join("_processed"),
        };
        let resolver = offline_resolver();
        let _ = xuewen::watcher::run(&pool, &dirs, &resolver, None, &inbox_for_task).await;
    });

    // Give the watcher time to finish catch-up and start watching, then drop a
    // second file (exercises the live notify path).
    tokio::time::sleep(Duration::from_millis(2500)).await;
    common::write_test_pdf(&inbox.join("dropped.pdf"), &["Freshly Dropped Paper Title"]);

    let processed = inbox.join("_processed");
    let existing_ok = wait_for(
        || processed.join("existing.pdf").exists(),
        Duration::from_secs(10),
    )
    .await;
    let dropped_ok = wait_for(
        || processed.join("dropped.pdf").exists(),
        Duration::from_secs(10),
    )
    .await;

    handle.abort();

    assert!(existing_ok, "catch-up file was not ingested");
    assert!(dropped_ok, "live-dropped file was not ingested");

    // Both are recorded.
    let pool = db::connect(&url).await.unwrap();
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM papers")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(n, 2);
}
