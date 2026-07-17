use std::path::Path;

/// Minimal loadable config with all paths under `dir`.
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
async fn serve_on_serves_api_on_an_ephemeral_port() {
    let tmp = tempfile::tempdir().unwrap();
    let cfg = test_cfg(tmp.path());
    let pool = xuewen::db::connect(&cfg.database_url).await.unwrap();

    let state = xuewen::web::AppState {
        pool: pool.clone(),
        library_root: cfg.library_root.clone(),
        ingest: None,
        proxy_login_url: None,
        search: None,
        daily: None,
        agent: None,
        citations: xuewen::citations::CitationsService::from_config(pool.clone(), &cfg),
        translate: None,
        ui: cfg.ui.clone(),
    };

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    assert_ne!(addr.port(), 0, "caller can learn the real bound port");

    tokio::spawn(xuewen::web::serve_on(listener, state));

    let resp = reqwest::get(format!("http://{addr}/api/stats"))
        .await
        .unwrap();
    assert!(resp.status().is_success(), "got {}", resp.status());
}
