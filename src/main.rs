use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use xuewen::config::Config;
use xuewen::db;
use xuewen::pipeline::{IngestCtx, Libraries, Outcome};
use xuewen::refresh::{self, RefreshTarget};
use xuewen::resolve::grobid::Grobid;
use xuewen::resolve::Resolver;
use xuewen::web;

/// Ask a yes/no question on the terminal; returns true only on an explicit yes.
fn confirm(prompt: &str) -> anyhow::Result<bool> {
    use std::io::Write;
    print!("{prompt} [y/N] ");
    std::io::stdout().flush()?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(matches!(
        line.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

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
    /// Serve the read-only web UI over HTTP (localhost).
    Serve {
        /// Address to bind.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind.
        #[arg(long, default_value_t = 8080)]
        port: u16,
    },
    /// Soft-delete a paper: hide it from the library (recoverable).
    Delete {
        /// Paper id (exact or unique prefix).
        id: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Permanently remove trashed papers and their PDF files.
    Purge {
        /// A trashed paper id (exact or unique prefix) to purge.
        #[arg(conflicts_with = "all")]
        id: Option<String>,
        /// Purge every trashed paper.
        #[arg(long)]
        all: bool,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
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
    let ctx = IngestCtx {
        pool: pool.clone(),
        dirs,
        resolver,
        grobid,
    };

    match cli.command {
        Command::Ingest { path } => match ctx.ingest_file(&path).await? {
            Outcome::Ingested(id) => println!("ingested {id}"),
            Outcome::Duplicate => println!("duplicate, skipped"),
            Outcome::SameWork(id) => println!("already in library ({id})"),
            Outcome::InTrash(id) => println!("in trash ({id})"),
        },
        Command::Watch => {
            xuewen::watcher::run(&ctx, &cfg.inbox_dir).await?;
        }
        Command::Refresh { id, all } => {
            let target = match (id, all) {
                (Some(id), _) => RefreshTarget::One(id),
                (None, true) => RefreshTarget::All,
                (None, false) => RefreshTarget::NeedsReview,
            };
            let summary = refresh::run(&ctx, target).await?;
            println!(
                "refresh: {} processed, {} re-resolved, {} re-filed",
                summary.processed, summary.reresolved, summary.refiled
            );
        }
        Command::Serve { host, port } => {
            let ingest = std::sync::Arc::new(web::Ingest {
                ctx,
                staging_dir: cfg.inbox_dir.join("_uploads"),
            });
            web::serve(&host, port, pool, cfg.library_root.clone(), ingest).await?;
        }
        Command::Delete { id, yes } => {
            let paper = db::find_one(&pool, &id).await?;
            if paper.deleted_at.is_some() {
                println!("already deleted: {}", paper.id);
            } else {
                let title = paper.meta.title.as_deref().unwrap_or("(untitled)");
                if yes || confirm(&format!("Delete {title:?}?"))? {
                    db::soft_delete(&pool, &paper.id).await?;
                    println!("deleted {}", paper.id);
                } else {
                    println!("cancelled");
                }
            }
        }
        Command::Purge { id, all, yes } => {
            let targets = match (id, all) {
                (Some(id), _) => {
                    let p = db::find_one(&pool, &id).await?;
                    if p.deleted_at.is_none() {
                        anyhow::bail!("{} is not in the trash (delete it first)", p.id);
                    }
                    vec![p]
                }
                (None, true) => db::trashed_papers(&pool).await?,
                (None, false) => anyhow::bail!("specify an <ID> or --all"),
            };
            if targets.is_empty() {
                println!("trash is empty");
            } else if yes
                || confirm(&format!(
                    "Permanently delete {} paper(s) and their files?",
                    targets.len()
                ))?
            {
                for p in &targets {
                    let path = cfg.library_root.join(&p.rel_path);
                    match std::fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => tracing::warn!("could not remove {}: {e}", path.display()),
                    }
                    db::delete_row(&pool, &p.id).await?;
                }
                println!("purged {} paper(s)", targets.len());
            } else {
                println!("cancelled");
            }
        }
    }
    Ok(())
}
