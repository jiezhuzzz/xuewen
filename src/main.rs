use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use xuewen::config::Config;
use xuewen::db;
use xuewen::pipeline::{ingest_file, Libraries, Outcome};
use xuewen::refresh::{self, RefreshTarget};
use xuewen::resolve::grobid::Grobid;
use xuewen::resolve::Resolver;

#[derive(Parser)]
#[command(name = "xuewen", version)]
struct Cli {
    /// Path to the TOML config file.
    #[arg(long, default_value = "xuewen.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Ingest a single PDF file.
    Ingest { path: PathBuf },
    /// Watch the inbox directory and auto-ingest new PDFs (runs until stopped).
    Watch,
    /// Re-resolve failed records and re-file every paper to its cite-key path.
    Refresh {
        /// Paper id (exact or unique prefix) to refresh. Omit to refresh needs_review records.
        #[arg(conflicts_with = "all")]
        id: Option<String>,
        /// Re-resolve every paper, not just needs_review records.
        #[arg(long)]
        all: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let cfg = Config::load(&cli.config)?;
    let pool = db::connect(&cfg.database_url).await?;
    let resolver = Resolver::new(cfg.contact_email.as_deref())?;
    let grobid = cfg.grobid_url.as_deref().map(Grobid::new).transpose()?;
    let dirs = Libraries {
        library_root: cfg.library_root.clone(),
        processed_dir: cfg.inbox_dir.join("_processed"),
    };

    match cli.command {
        Command::Ingest { path } => {
            match ingest_file(&pool, &dirs, &resolver, grobid.as_ref(), &path).await? {
                Outcome::Ingested(id) => println!("ingested {id}"),
                Outcome::Duplicate => println!("duplicate, skipped"),
            }
        }
        Command::Watch => {
            xuewen::watcher::run(&pool, &dirs, &resolver, grobid.as_ref(), &cfg.inbox_dir).await?;
        }
        Command::Refresh { id, all } => {
            let target = match (id, all) {
                (Some(id), _) => RefreshTarget::One(id),
                (None, true) => RefreshTarget::All,
                (None, false) => RefreshTarget::NeedsReview,
            };
            let summary = refresh::run(
                &pool,
                &dirs.library_root,
                &resolver,
                grobid.as_ref(),
                target,
            )
            .await?;
            println!(
                "refresh: {} processed, {} re-resolved, {} re-filed",
                summary.processed, summary.reresolved, summary.refiled
            );
        }
    }
    Ok(())
}
