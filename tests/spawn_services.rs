use std::path::Path;

fn test_cfg(dir: &Path) -> xuewen::config::Config {
    let f = dir.join("xuewen.toml");
    std::fs::write(
        &f,
        format!(
            "inbox_dir = {:?}\nlibrary_root = {:?}\ndatabase_url = \"sqlite::memory:\"\n\n[search]\nindex_dir = {:?}\n",
            dir.join("inbox"),
            dir.join("library"),
            dir.join("search-index"),
        ),
    )
    .unwrap();
    xuewen::config::Config::load(&f).unwrap()
}

#[tokio::test]
async fn spawn_services_with_minimal_config() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_cfg(tmp.path());
    let pool = xuewen::db::connect(&cfg.database_url).await.unwrap();

    let svc = xuewen::server::spawn_services(&cfg, pool).await.unwrap();

    // No [ai.*]/[daily]/[translate] sections => those services are off;
    // ingest and citations are always built.
    assert!(svc.agent.is_none());
    assert!(svc.daily.is_none());
    assert!(svc.translate.is_none());
    assert_eq!(svc.ingest.staging_dir, cfg.inbox_dir.join("_uploads"));
}

#[tokio::test]
async fn server_serve_on_serves_the_full_stack() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_cfg(tmp.path());
    let pool = xuewen::db::connect(&cfg.database_url).await.unwrap();
    let svc = xuewen::server::spawn_services(&cfg, pool.clone())
        .await
        .unwrap();

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(xuewen::server::serve_on(listener, pool, &cfg, svc));

    let resp = reqwest::get(format!("http://{addr}/api/stats"))
        .await
        .unwrap();
    assert!(resp.status().is_success(), "got {}", resp.status());
}
