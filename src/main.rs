use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use xuewen::config::Config;
use xuewen::db;
use xuewen::pipeline::{ingest_file, Libraries, Outcome};
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
    let grobid = cfg
        .grobid_url
        .as_deref()
        .map(Grobid::new)
        .transpose()?;
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
    }
    Ok(())
}
