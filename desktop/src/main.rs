mod bootstrap;
mod bundle;

use anyhow::{Context, Result};
use tauri::Manager;

fn main() {
    if let Err(e) = run() {
        rfd::MessageDialog::new()
            .set_level(rfd::MessageLevel::Error)
            .set_title("Xuewen failed to start")
            .set_description(format!("{e:#}"))
            .show();
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let dirs = bootstrap::AppDirs::resolve()?;
    let cfg_path = bootstrap::ensure_bootstrap(&dirs)?;

    // Environment first, before any thread exists: the `env` file (API keys),
    // then PATH so the bundled pdftotext/node sidecars shadow system copies —
    // src/pdf.rs and src/agent resolve both via `Command::new("...")`.
    if let Ok(text) = std::fs::read_to_string(dirs.env_file()) {
        for (k, v) in bootstrap::parse_env_file(&text) {
            std::env::set_var(k, v);
        }
    }
    let bundle = bundle::bundle_dirs();
    if let Some((macos_dir, _)) = &bundle {
        let path = std::env::var("PATH").ok();
        std::env::set_var("PATH", bootstrap::prepend_path(macos_dir, path.as_deref()));
    }

    // A GUI app has no terminal: log to ~/Library/Logs/Xuewen/.
    let file = tracing_appender::rolling::never(&dirs.logs, "xuewen-desktop.log");
    let (writer, _log_guard) = tracing_appender::non_blocking(file);
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(writer)
        .with_ansi(false)
        .init();

    let mut cfg = xuewen::config::Config::load(&cfg_path)
        .with_context(|| format!("config file: {}", cfg_path.display()))?;
    if let Some((_, resources)) = &bundle {
        if cfg.ai.agent.runner.is_none() {
            // In-memory only — never written back to the user's config.
            cfg.ai.agent.runner = Some(resources.join("agent-runner/src/runner.mjs"));
        }
    }

    // The backend runs on our own tokio runtime; hand its handle to Tauri so
    // there is exactly one runtime in the process. `rt` stays in scope for
    // the app's whole life.
    let rt = tokio::runtime::Runtime::new()?;
    tauri::async_runtime::set(rt.handle().clone());

    // Backend bring-up happens inside `.setup()`, which Tauri runs *after*
    // plugin initialization: a second app launch is terminated by the
    // single-instance plugin before it ever touches the SQLite db, spawns
    // services, or binds a port.
    let setup_cfg_path = cfg_path.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // Second launch: focus the existing window instead of racing the
            // first instance for the SQLite db and schedulers.
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(move |app| {
            let addr = tauri::async_runtime::block_on(async {
                let pool = xuewen::db::connect(&cfg.database_url).await?;
                let services = xuewen::server::spawn_services(&cfg, pool.clone()).await?;
                let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
                let addr = listener.local_addr()?;
                let server = xuewen::server::serve_on(listener, pool, &cfg, services);
                tokio::spawn(async move {
                    if let Err(e) = server.await {
                        tracing::error!("server exited: {e:#}");
                    }
                });
                anyhow::Ok(addr)
            })
            .with_context(|| format!("config file: {}", setup_cfg_path.display()))?;
            tracing::info!("desktop backend on http://{addr}");

            let url: tauri::Url = format!("http://{addr}")
                .parse()
                .context("building backend url")?;
            tauri::WebviewWindowBuilder::new(app, "main", tauri::WebviewUrl::External(url))
                .title("Xuewen")
                .inner_size(1280.0, 800.0)
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .with_context(|| {
            format!(
                "running tauri application (config file: {})",
                cfg_path.display()
            )
        })?;
    Ok(())
}
