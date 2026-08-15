use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use xuewen::config::Config;
use xuewen::db;
use xuewen::http::RetryPolicy;
use xuewen::import::ImportError;
use xuewen::models::Identifier;
use xuewen::pipeline::{IdentifyOutcome, IngestCtx, Outcome};
use xuewen::refresh::{self, RefreshTarget};
use xuewen::search::{indexer, SearchService};
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

/// One-line preview of quoted text, cut on a character boundary. A PDF
/// selection routinely spans line breaks, which would otherwise split one
/// annotation across several rows of the listing.
fn ellipsize(s: &str, max: usize) -> String {
    let flat = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        return flat;
    }
    let head: String = flat.chars().take(max.saturating_sub(1)).collect();
    format!("{}…", head.trim_end())
}

/// First three authors, "et al."-truncated — matches the web UI's rule.
fn author_line(authors: &[String]) -> String {
    if authors.len() > 3 {
        format!("{}, et al.", authors[..3].join(", "))
    } else {
        authors.join(", ")
    }
}

/// Terminal output: drop <mark> tags and undo the snippet's HTML escaping.
fn strip_snippet_html(s: &str) -> String {
    s.replace("<mark>", "")
        .replace("</mark>", "")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&")
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
    /// Import a paper from a URL, DOI, or arXiv id.
    Import { input: String },
    /// Manage the stored EZproxy session cookie used for paywalled imports.
    ProxyCookie {
        /// Store this cookie value (a `name=value; name2=value2` header string).
        #[arg(long, conflicts_with = "clear")]
        set: Option<String>,
        /// Remove the stored cookie.
        #[arg(long)]
        clear: bool,
    },
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
    /// Manually match a paper to a DOI, arXiv id, or searched title.
    Identify {
        /// Paper id (exact or unique prefix).
        id: String,
        /// Apply the Crossref record for this DOI.
        #[arg(long, conflicts_with_all = ["arxiv", "title"])]
        doi: Option<String>,
        /// Apply the arXiv record for this id.
        #[arg(long, conflicts_with_all = ["doi", "title"])]
        arxiv: Option<String>,
        /// Search DBLP/Crossref for this title and list candidates.
        #[arg(long, conflicts_with_all = ["doi", "arxiv"])]
        title: Option<String>,
        /// Apply candidate N from the --title list (1-based).
        #[arg(long, requires = "title")]
        pick: Option<usize>,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Serve the web UI over HTTP (loopback by default).
    Serve {
        /// Address to bind.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind.
        #[arg(long, default_value_t = 8080)]
        port: u16,
        /// Allow binding a non-loopback address (mutating endpoints have no auth).
        #[arg(long)]
        allow_remote: bool,
    },
    /// Soft-delete a paper: hide it from the library (recoverable).
    Delete {
        /// Paper id (exact or unique prefix).
        id: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Restore a trashed paper back into the library.
    Restore {
        /// Paper id (exact or unique prefix).
        id: String,
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
    /// Manage projects (named groups of related papers).
    Project {
        #[command(subcommand)]
        cmd: ProjectCmd,
    },
    /// Manage topical tags.
    Tag {
        #[command(subcommand)]
        cmd: TagCmd,
    },
    /// Attach, inspect, or detach a paper's code repository
    Code {
        #[command(subcommand)]
        cmd: CodeCmd,
    },
    /// Inspect or clear a paper's reader annotations.
    Annotation {
        #[command(subcommand)]
        cmd: AnnotationCmd,
    },
    /// Star a paper.
    Star { paper: String },
    /// Un-star a paper.
    Unstar { paper: String },
    /// Export papers as BibTeX or BibLaTeX.
    Export {
        /// Paper id (exact or unique prefix) for a single entry.
        #[arg(conflicts_with_all = ["all", "project", "tag", "starred"])]
        id: Option<String>,
        /// Export the whole (non-trashed) library.
        #[arg(long, conflicts_with = "project")]
        all: bool,
        /// Export all papers in this project (name or id).
        #[arg(long)]
        project: Option<String>,
        /// Filter batch exports to papers carrying this tag (or its prefix).
        #[arg(long)]
        tag: Option<String>,
        /// Filter batch exports to starred papers only.
        #[arg(long)]
        starred: bool,
        /// Filter batch exports by a search term (title/author).
        #[arg(long)]
        query: Option<String>,
        /// Filter batch exports by status (resolved|needs_review).
        #[arg(long)]
        status: Option<String>,
        /// Output format.
        #[arg(long, value_enum, default_value_t = BibFormatArg::Bibtex)]
        format: BibFormatArg,
        /// Write to this file instead of stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Search the library from the terminal.
    Search {
        /// Query; supports tag:/project:/is:starred/status:/in:/author: qualifiers
        /// and "quoted phrases" (e.g. 'tag:nlp author:smith attention').
        query: String,
        /// Comma-separated fields: title,authors,abstract,body,notes (default all).
        #[arg(long)]
        fields: Option<String>,
        /// Keyword (BM25) engine only.
        #[arg(long, conflicts_with = "semantic_only")]
        keyword_only: bool,
        /// Semantic (embedding) engine only.
        #[arg(long)]
        semantic_only: bool,
    },
    /// Generate LLM summaries for library papers (needs [ai.summary]).
    Summarize {
        /// Paper id (exact) to (re)summarize. Omit to fill gaps for the whole library.
        #[arg(conflicts_with = "all")]
        id: Option<String>,
        /// Clear and regenerate every paper's summary.
        #[arg(long)]
        all: bool,
    },
    /// Inspect or rebuild the search indexes.
    Index {
        #[command(subcommand)]
        cmd: IndexCmd,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum BibFormatArg {
    Bibtex,
    Biblatex,
}

impl From<BibFormatArg> for xuewen::export::BibFormat {
    fn from(a: BibFormatArg) -> Self {
        match a {
            BibFormatArg::Bibtex => xuewen::export::BibFormat::Bibtex,
            BibFormatArg::Biblatex => xuewen::export::BibFormat::Biblatex,
        }
    }
}

#[derive(Subcommand)]
enum ProjectCmd {
    /// List projects with paper counts.
    List,
    /// Create a new project.
    New { name: String },
    /// Delete a project (papers are kept).
    Rm { project: String },
    /// Add one or more papers to a project.
    Add {
        project: String,
        #[arg(required = true)]
        papers: Vec<String>,
    },
    /// Remove a paper from a project.
    Remove { project: String, paper: String },
    /// List the papers in a project.
    Show { project: String },
}

#[derive(Subcommand)]
enum TagCmd {
    /// List tags with paper counts.
    List,
    /// Add a tag to a paper (created if new).
    Add { paper: String, name: String },
    /// Remove a tag from a paper.
    Remove { paper: String, name: String },
    /// Rename a tag.
    Rename { old: String, new: String },
    /// Delete a tag from all papers.
    Rm { name: String },
    /// List papers carrying a tag (or its prefix).
    Show { name: String },
}

#[derive(Subcommand)]
enum CodeCmd {
    /// Attach a repo (https URL) — clones now, pinned to HEAD
    Set { paper: String, url: String },
    /// Show the attached repo and its clone status
    Status { paper: String },
    /// Detach the repo and delete the local checkout
    Rm { paper: String },
}

#[derive(Subcommand)]
enum AnnotationCmd {
    /// List a paper's annotations in reading order.
    List { paper: String },
    /// Delete one annotation by id.
    Rm { paper: String, id: String },
    /// Delete every annotation on a paper.
    Clear {
        paper: String,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand)]
enum IndexCmd {
    /// Show per-tier indexing counts.
    Status,
    /// Drop and re-derive the search indexes from SQLite + PDFs (stop `xuewen serve` first).
    Rebuild {
        /// Rebuild only the Tantivy full-text index.
        #[arg(long, conflicts_with = "vectors_only")]
        fts_only: bool,
        /// Rebuild only the Qdrant vectors (recreates the collection).
        #[arg(long)]
        vectors_only: bool,
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
    // Only the arms that ingest build an IngestCtx (two HTTP clients), always
    // with the production retry policy; `serve` gets its own interactive one
    // inside `spawn_services`.
    let ingest_ctx = || IngestCtx::from_config(&cfg, pool.clone(), RetryPolicy::production());

    match cli.command {
        Command::Ingest { path } => match ingest_ctx()?.ingest_file(&path).await? {
            Outcome::Ingested(id) => println!("ingested {id}"),
            Outcome::Duplicate(id) => {
                match db::get_by_id(&pool, &id).await?.and_then(|p| p.cite_key) {
                    Some(key) => println!("duplicate of {key} ({id}), skipped"),
                    None => println!("duplicate ({id}), skipped"),
                }
            }
            Outcome::SameWork(id) => {
                match db::get_by_id(&pool, &id).await?.and_then(|p| p.cite_key) {
                    Some(key) => println!("already in library as {key} ({id})"),
                    None => println!("already in library ({id})"),
                }
            }
            Outcome::InTrash(id) => println!("in trash — run: xuewen restore {id}"),
        },
        Command::Import { input } => {
            let ctx = ingest_ctx()?;
            let fetcher =
                xuewen::import::Fetcher::new(cfg.proxy.as_ref().map(|p| p.login_url.clone()))?;
            let cookie = db::get_setting(&pool, "proxy_cookie").await?;
            match xuewen::import::fetch_stage_ingest(&ctx, &fetcher, &input, cookie.as_deref())
                .await
            {
                Ok(Outcome::Ingested(id)) => println!("ingested {id}"),
                Ok(Outcome::Duplicate(id)) => {
                    match db::get_by_id(&pool, &id).await?.and_then(|p| p.cite_key) {
                        Some(key) => println!("duplicate of {key} ({id}), skipped"),
                        None => println!("duplicate ({id}), skipped"),
                    }
                }
                Ok(Outcome::SameWork(id)) => println!("already in library ({id})"),
                Ok(Outcome::InTrash(id)) => {
                    println!("in trash — run: xuewen restore {id}")
                }
                Err(ImportError::Unsupported) => {
                    anyhow::bail!("could not recognize {input:?} as a URL, DOI, or arXiv id")
                }
                Err(ImportError::CookieExpired) => anyhow::bail!(
                    "proxy session expired — refresh it: xuewen proxy-cookie --set '<cookie>'"
                ),
                Err(ImportError::Unfetched { metadata }) => {
                    let title = metadata
                        .as_ref()
                        .and_then(|m| m.title.as_deref())
                        .unwrap_or("(unknown title)");
                    anyhow::bail!(
                        "could not fetch a PDF for {title:?} — paywalled with no open-access \
                         copy, or the cookie is missing/expired. Download it in your browser \
                         and drop it in the inbox."
                    )
                }
                Err(ImportError::Network(e)) => return Err(e.context("fetch failed")),
                Err(ImportError::Ingest(e)) => return Err(e),
            }
        }
        Command::ProxyCookie { set, clear } => {
            if clear {
                db::delete_setting(&pool, "proxy_cookie").await?;
                println!("proxy cookie cleared");
            } else if let Some(cookie) = set {
                db::set_setting(&pool, "proxy_cookie", cookie.trim()).await?;
                println!("proxy cookie stored");
            } else {
                match db::setting_updated_at(&pool, "proxy_cookie").await? {
                    Some(ts) => println!("proxy cookie set (updated {ts})"),
                    None => println!("no proxy cookie set"),
                }
            }
        }
        Command::Watch => {
            match SearchService::open(pool.clone(), &cfg.search, &cfg.ai).await {
                Ok(s) => {
                    tokio::spawn(indexer::run(
                        s,
                        cfg.library_root.clone(),
                        std::time::Duration::from_secs(30),
                    ));
                }
                Err(e) => tracing::warn!("search indexing disabled: {e}"),
            }
            xuewen::watcher::run(&ingest_ctx()?, &cfg.inbox_dir).await?;
        }
        Command::Refresh { id, all } => {
            let target = match (id, all) {
                (Some(id), _) => RefreshTarget::One(id),
                (None, true) => RefreshTarget::All,
                (None, false) => RefreshTarget::NeedsReview,
            };
            let summary = refresh::run(&ingest_ctx()?, target).await?;
            println!(
                "refresh: {} processed, {} re-resolved, {} re-filed",
                summary.processed, summary.reresolved, summary.refiled
            );
        }
        Command::Identify {
            id,
            doi,
            arxiv,
            title,
            pick,
            yes,
        } => {
            let ctx = ingest_ctx()?;
            let mut paper = db::find_one(&pool, &id).await?;
            // Early UX check; the enforced guard lives in apply_match. This lets us
            // bail before the interactive search/confirm flow rather than after.
            if paper.deleted_at.is_some() {
                anyhow::bail!(
                    "{} is in the trash — run: xuewen restore {}",
                    paper.id,
                    paper.id
                );
            }
            let md = if let Some(doi) = doi {
                ctx.resolver
                    .resolve(&Identifier::doi(doi.clone()), None)
                    .await
                    .ok_or_else(|| {
                        anyhow::anyhow!("no Crossref record for doi {doi} — try --title")
                    })?
            } else if let Some(axv) = arxiv {
                ctx.resolver
                    .resolve(&Identifier::arxiv(axv.clone()), None)
                    .await
                    .ok_or_else(|| anyhow::anyhow!("no arXiv record for {axv} — try --title"))?
            } else if let Some(query) = title {
                let cands = ctx.resolver.search_candidates(&query).await;
                if cands.is_empty() {
                    anyhow::bail!("no candidates found for {query:?}");
                }
                match pick {
                    Some(n) if n >= 1 && n <= cands.len() => cands.into_iter().nth(n - 1).unwrap(),
                    Some(n) => anyhow::bail!("--pick {n} is out of range (1..={})", cands.len()),
                    None => {
                        for (i, c) in cands.iter().enumerate() {
                            println!(
                                "{:2}. {} — {} ({}, {}) [{}]",
                                i + 1,
                                c.title.as_deref().unwrap_or("(untitled)"),
                                author_line(&c.authors),
                                c.venue.as_deref().unwrap_or("?"),
                                c.year.map_or("?".to_string(), |y| y.to_string()),
                                c.source,
                            );
                        }
                        println!("re-run with --pick <N> to apply one");
                        return Ok(());
                    }
                }
            } else {
                anyhow::bail!("provide one of --doi, --arxiv, or --title");
            };

            println!(
                "match: {} — {} ({}, {})",
                md.title.as_deref().unwrap_or("(untitled)"),
                author_line(&md.authors),
                md.venue.as_deref().unwrap_or("?"),
                md.year.map_or("?".to_string(), |y| y.to_string()),
            );
            if yes || confirm("Apply this match?")? {
                match ctx.apply_match(&mut paper, md).await? {
                    IdentifyOutcome::Applied => println!(
                        "identified {} as {}",
                        paper.id,
                        paper.cite_key.as_deref().unwrap_or("(no cite key)")
                    ),
                    IdentifyOutcome::SameWork(other) => {
                        anyhow::bail!("that identifier already belongs to {other}")
                    }
                    IdentifyOutcome::Trashed => {
                        anyhow::bail!(
                            "{} is in the trash — run: xuewen restore {}",
                            paper.id,
                            paper.id
                        )
                    }
                }
            } else {
                println!("cancelled");
            }
        }
        Command::Serve {
            host,
            port,
            allow_remote,
        } => {
            if !web::is_loopback_host(&host) {
                if allow_remote {
                    eprintln!(
                        "warning: binding {host}: the web UI has mutating endpoints and no auth — \
                         anyone who can reach this address can import and delete papers"
                    );
                } else {
                    anyhow::bail!(
                        "refusing to bind non-loopback address {host}: the web UI has no auth; \
                         pass --allow-remote to override"
                    );
                }
            }
            let services = xuewen::server::spawn_services(&cfg, pool.clone()).await?;
            let listener = tokio::net::TcpListener::bind(format!("{host}:{port}")).await?;
            tracing::info!("xuewen serving on http://{host}:{port}");
            xuewen::server::serve_on(listener, pool.clone(), &cfg, services).await?;
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
        Command::Restore { id } => {
            let paper = db::find_one(&pool, &id).await?;
            // Unlike delete's soft "already deleted" no-op, a restore of an active
            // paper is a hard error: it usually means a mistyped id prefix, and
            // silently "succeeding" would hide that.
            if paper.deleted_at.is_none() {
                anyhow::bail!("{} is not in the trash", paper.id);
            }
            db::restore(&pool, &paper.id).await?;
            println!("restored {}", paper.id);
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
                // One paper's failure must not strand the rest half-purged:
                // keep going and report the stragglers at the end.
                let mut failures = 0usize;
                for p in &targets {
                    if let Err(e) = xuewen::pipeline::purge_paper(&pool, &cfg.library_root, p).await
                    {
                        eprintln!("could not purge {}: {e:#}", p.id);
                        failures += 1;
                    }
                }
                println!("purged {} paper(s)", targets.len() - failures);
                if failures > 0 {
                    anyhow::bail!("{failures} paper(s) could not be purged — re-run to retry");
                }
            } else {
                println!("cancelled");
            }
        }
        Command::Project { cmd } => match cmd {
            ProjectCmd::List => {
                let projects = db::list_projects(&pool).await?;
                if projects.is_empty() {
                    println!("no projects");
                }
                for s in projects {
                    println!("{}  ({} papers)", s.project.name, s.paper_count);
                }
            }
            ProjectCmd::New { name } => {
                let name = name.trim();
                if name.is_empty() {
                    anyhow::bail!("project name cannot be empty");
                }
                let p = db::create_project(&pool, name).await?;
                println!("created project {} ({})", p.name, p.id);
            }
            ProjectCmd::Rm { project } => {
                let p = db::find_one_project(&pool, &project).await?;
                db::delete_project(&pool, &p.id).await?;
                println!("deleted project {}", p.name);
            }
            ProjectCmd::Add { project, papers } => {
                let proj = db::find_one_project(&pool, &project).await?;
                for sel in &papers {
                    let paper = db::find_one(&pool, sel).await?;
                    db::add_paper_to_project(&pool, &paper.id, &proj.id).await?;
                    let label = paper.cite_key.as_deref().unwrap_or(&paper.id);
                    println!("added {label} to {}", proj.name);
                }
            }
            ProjectCmd::Remove { project, paper } => {
                let proj = db::find_one_project(&pool, &project).await?;
                let paper = db::find_one(&pool, &paper).await?;
                let label = paper.cite_key.as_deref().unwrap_or(&paper.id);
                if db::remove_paper_from_project(&pool, &paper.id, &proj.id).await? {
                    println!("removed {label} from {}", proj.name);
                } else {
                    println!("{label} was not in {}", proj.name);
                }
            }
            ProjectCmd::Show { project } => {
                let proj = db::find_one_project(&pool, &project).await?;
                let filter = db::PaperFilter {
                    project: Some(proj.id.clone()),
                    ..Default::default()
                };
                let papers = db::list_papers(&pool, None, None, &filter).await?;
                println!("{} — {} paper(s)", proj.name, papers.len());
                for p in papers {
                    println!(
                        "  {}  {}",
                        p.id,
                        p.meta.title.as_deref().unwrap_or("(untitled)")
                    );
                }
            }
        },
        Command::Tag { cmd } => match cmd {
            TagCmd::List => {
                let tags = db::list_tags_with_counts(&pool).await?;
                if tags.is_empty() {
                    println!("no tags");
                }
                for s in tags {
                    println!("{}  ({} papers)", s.tag.name, s.paper_count);
                }
            }
            TagCmd::Add { paper, name } => {
                let paper = db::find_one(&pool, &paper).await?;
                let label = paper.cite_key.as_deref().unwrap_or(&paper.id);
                let tag = db::add_paper_tag(&pool, &paper.id, &name).await?;
                println!("added tag {} to {label}", tag.name);
            }
            TagCmd::Remove { paper, name } => {
                let paper = db::find_one(&pool, &paper).await?;
                let label = paper.cite_key.as_deref().unwrap_or(&paper.id);
                let tag = db::find_tag_by_name(&pool, &name)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("no tag named {name:?}"))?;
                if db::remove_paper_tag(&pool, &paper.id, &tag.id).await? {
                    println!("removed tag {} from {label}", tag.name);
                } else {
                    println!("{label} did not carry tag {}", tag.name);
                }
            }
            TagCmd::Rename { old, new } => {
                let tag = db::find_tag_by_name(&pool, &old)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("no tag named {old:?}"))?;
                let renamed = db::rename_tag(&pool, &tag.id, &new)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("no tag named {old:?}"))?;
                println!("renamed tag {} to {}", old, renamed.name);
            }
            TagCmd::Rm { name } => {
                let tag = db::find_tag_by_name(&pool, &name)
                    .await?
                    .ok_or_else(|| anyhow::anyhow!("no tag named {name:?}"))?;
                db::delete_tag(&pool, &tag.id).await?;
                println!("deleted tag {}", tag.name);
            }
            TagCmd::Show { name } => {
                let filter = db::PaperFilter {
                    tag: Some(name.clone()),
                    ..Default::default()
                };
                let papers = db::list_papers(&pool, None, None, &filter).await?;
                println!("{name} — {} paper(s)", papers.len());
                for p in papers {
                    println!(
                        "  {}  {}",
                        p.id,
                        p.meta.title.as_deref().unwrap_or("(untitled)")
                    );
                }
            }
        },
        Command::Code { cmd } => {
            match cmd {
                CodeCmd::Set { paper, url } => {
                    let paper = db::find_one(&pool, &paper).await?;
                    let paper_id = paper.id.clone();
                    xuewen::agent::code::validate_repo_url(&url, &cfg.ai.agent.clone_allowed_hosts)
                        .map_err(|e| anyhow::anyhow!(e))?;
                    let clone_gen = xuewen::agent::store::upsert_paper_code_cloning(
                        &pool,
                        &paper_id,
                        url.trim(),
                    )
                    .await?;
                    // CLI clones inline so the outcome prints immediately.
                    xuewen::agent::code::run_clone(
                        pool.clone(),
                        cfg.library_root.clone(),
                        paper_id.clone(),
                        url.trim().to_string(),
                        cfg.ai.agent.max_repo_mb,
                        clone_gen,
                    )
                    .await;
                    match xuewen::agent::store::get_paper_code(&pool, &paper_id).await? {
                        Some(c) if c.status == xuewen::agent::store::CodeStatus::Ready => {
                            println!(
                                "attached {} at {}",
                                c.repo_url,
                                c.commit_sha.as_deref().unwrap_or("?")
                            )
                        }
                        Some(c) => println!(
                            "attach failed: {}",
                            c.error.as_deref().unwrap_or("unknown error")
                        ),
                        None => println!("attach failed: no record"),
                    }
                }
                CodeCmd::Status { paper } => {
                    let paper = db::find_one(&pool, &paper).await?;
                    let paper_id = paper.id;
                    match xuewen::agent::store::get_paper_code(&pool, &paper_id).await? {
                        None => println!("no repo attached"),
                        Some(c) => println!(
                            "{} — {}{}",
                            c.repo_url,
                            c.status,
                            c.commit_sha.map(|s| format!(" @ {s}")).unwrap_or_default()
                        ),
                    }
                }
                CodeCmd::Rm { paper } => {
                    let paper = db::find_one(&pool, &paper).await?;
                    let paper_id = paper.id;
                    xuewen::agent::code::remove_checkout(&cfg.library_root, &paper_id).await;
                    xuewen::agent::store::delete_paper_code(&pool, &paper_id).await?;
                    println!("detached");
                }
            }
        }
        Command::Annotation { cmd } => {
            let svc = xuewen::annotations::AnnotationsService::new(pool.clone());
            match cmd {
                AnnotationCmd::List { paper } => {
                    let paper = db::find_one(&pool, &paper).await?;
                    let items = svc.list(&paper.id).await?;
                    if items.is_empty() {
                        println!("no annotations");
                    }
                    for a in &items {
                        // Pages are 0-based in storage and 1-based to a reader.
                        println!(
                            "{}  p{}  {}/{}  {}",
                            a.id,
                            a.page_index + 1,
                            a.kind,
                            a.color,
                            ellipsize(a.quoted_text.as_deref().unwrap_or(""), 60)
                        );
                        if let Some(note) = &a.note {
                            println!("      note: {note}");
                        }
                    }
                }
                AnnotationCmd::Rm { paper, id } => {
                    let paper = db::find_one(&pool, &paper).await?;
                    if svc.delete(&paper.id, &id).await? {
                        println!("removed {id}");
                    } else {
                        anyhow::bail!("no annotation {id} on {}", paper.id);
                    }
                }
                AnnotationCmd::Clear { paper, yes } => {
                    let paper = db::find_one(&pool, &paper).await?;
                    let n = svc.list(&paper.id).await?.len();
                    if n == 0 {
                        println!("no annotations");
                    } else if yes || confirm(&format!("Delete {n} annotation(s)?"))? {
                        println!("removed {}", svc.delete_all(&paper.id).await?);
                    }
                }
            }
            // The search index carries annotation notes; a CLI edit while
            // `serve` holds Tantivy's single-writer lock cannot reindex here
            // (see `index rebuild`). The running server's sweep picks the
            // change up on its next pass; a CLI-only session catches it the
            // next time one runs.
        }
        Command::Star { paper } => {
            let paper = db::find_one(&pool, &paper).await?;
            let label = paper.cite_key.as_deref().unwrap_or(&paper.id);
            if db::set_paper_starred(&pool, &paper.id, true).await? {
                println!("starred {label}");
            } else {
                println!("{label} not found");
            }
        }
        Command::Unstar { paper } => {
            let paper = db::find_one(&pool, &paper).await?;
            let label = paper.cite_key.as_deref().unwrap_or(&paper.id);
            if db::set_paper_starred(&pool, &paper.id, false).await? {
                println!("unstarred {label}");
            } else {
                println!("{label} not found");
            }
        }
        Command::Export {
            id,
            all,
            project,
            tag,
            starred,
            query,
            status,
            format,
            output,
        } => {
            let fmt = xuewen::export::BibFormat::from(format);
            let text = if let Some(id) = id {
                let paper = db::find_one(&pool, &id).await?;
                xuewen::export::format_entry(&paper, fmt)
            } else {
                if !all && project.is_none() && tag.is_none() && !starred {
                    anyhow::bail!(
                        "specify a paper id, --all, --project <name>, --tag <name>, or --starred"
                    );
                }
                let project_id = match &project {
                    Some(sel) => Some(db::find_one_project(&pool, sel).await?.id),
                    None => None,
                };
                let filter = db::PaperFilter {
                    status,
                    project: project_id,
                    tag,
                    starred: starred.then_some(true),
                };
                let papers = db::list_papers(&pool, query.as_deref(), None, &filter).await?;
                xuewen::export::format_entries(&papers, fmt)
            };
            // Normalize to exactly one trailing newline so single-entry output
            // (which has none) doesn't abut the shell prompt; batch output is
            // already newline-terminated and is left unchanged.
            let text = if text.ends_with('\n') {
                text
            } else {
                format!("{text}\n")
            };
            match output {
                Some(path) => {
                    tokio::fs::write(&path, &text).await?;
                    println!("wrote {}", path.display());
                }
                None => print!("{text}"),
            }
        }
        Command::Search {
            query,
            fields,
            keyword_only,
            semantic_only,
        } => {
            let svc = SearchService::open(pool.clone(), &cfg.search, &cfg.ai).await?;
            let req = xuewen::search::SearchRequest::assemble(
                &pool,
                &query,
                xuewen::search::RequestOverrides {
                    keyword: !semantic_only,
                    semantic: !keyword_only,
                    fields,
                    status: None,
                    project: None,
                    tag: None,
                    starred: None,
                },
            )
            .await?;
            let out = svc.search(&req).await?;
            if let Some(reason) = &out.semantic.reason {
                if !keyword_only {
                    eprintln!("note: semantic search unavailable — {reason}");
                }
            }
            if out.results.is_empty() {
                println!("no matches");
            }
            for (i, (p, m)) in out.results.iter().enumerate() {
                let label = p.cite_key.as_deref().unwrap_or(&p.id);
                println!(
                    "{:2}. {}  {}",
                    i + 1,
                    label,
                    p.meta.title.as_deref().unwrap_or("(untitled)")
                );
                let loc = match m.page {
                    Some(pg) => format!("{} p.{pg}", m.field),
                    None => m.field.to_string(),
                };
                println!("      [{loc}] {}", strip_snippet_html(&m.snippet));
            }
        }
        Command::Summarize { id, all } => {
            let Some(svc) = xuewen::summary::SummaryService::from_config(pool.clone(), &cfg) else {
                // Say precisely why the service is off: an absent section, a
                // missing model, or a key whose env var came up empty.
                let Some(use_) = cfg.ai.summary.as_ref() else {
                    anyhow::bail!("[ai.summary] is not configured — nothing to do");
                };
                let r = cfg.ai.resolve(use_);
                if r.model.is_none() {
                    anyhow::bail!(
                        "[ai.summary] has no model — set `model` under [ai] or [ai.summary]"
                    );
                }
                anyhow::bail!(
                    "[ai.summary] has no API key — set ${} or `api_key_env`",
                    r.api_key_env
                );
            };
            if all {
                xuewen::summary::store::clear(&pool, None).await?;
                xuewen::summary::store::clear_failure(&pool, None).await?;
                println!("generated {} summaries", svc.sweep_all().await?);
            } else if let Some(id) = &id {
                xuewen::summary::store::clear(&pool, Some(id)).await?;
                xuewen::summary::store::clear_failure(&pool, Some(id)).await?;
                let ok = svc.summarize_one(id).await?;
                println!(
                    "{}",
                    if ok {
                        "summary generated"
                    } else {
                        "no summary generated (see logs)"
                    }
                );
            } else {
                println!("generated {} summaries", svc.sweep_all().await?);
            }
        }
        Command::Index { cmd } => match cmd {
            IndexCmd::Status => {
                let svc = SearchService::open(pool.clone(), &cfg.search, &cfg.ai).await?;
                let st = svc.status().await?;
                println!(
                    "full-text: {} indexed, {} pending, {} failed",
                    st.fts.indexed, st.fts.pending, st.fts.failed
                );
                println!(
                    "vectors:   {} indexed, {} pending, {} failed",
                    st.vectors.indexed, st.vectors.pending, st.vectors.failed
                );
                match st.reason {
                    None => println!("semantic search: available"),
                    Some(r) => println!("semantic search: unavailable — {r}"),
                }
            }
            IndexCmd::Rebuild {
                fts_only,
                vectors_only,
            } => {
                let scope = match (fts_only, vectors_only) {
                    (true, false) => indexer::RebuildScope::Fts,
                    (false, true) => indexer::RebuildScope::Vectors,
                    // clap's conflicts_with forbids both flags at once.
                    _ => indexer::RebuildScope::Both,
                };
                let s =
                    indexer::rebuild(pool.clone(), &cfg.search, &cfg.ai, &cfg.library_root, scope)
                        .await?;
                println!(
                    "rebuild: {} indexed, {} removed, {} failed",
                    s.indexed, s.deindexed, s.failed
                );
                if s.failed > 0 {
                    anyhow::bail!("some papers failed to index — see the log; re-run to retry");
                }
            }
        },
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ellipsize;

    #[test]
    fn short_text_passes_through() {
        assert_eq!(ellipsize("a short quote", 60), "a short quote");
    }

    #[test]
    fn line_breaks_and_runs_of_space_collapse() {
        // PDF selections carry the newlines of the wrapped source text; the
        // listing is one row per annotation, so they cannot survive.
        assert_eq!(ellipsize("two\nlines   here", 60), "two lines here");
    }

    #[test]
    fn overlong_text_is_cut_with_an_ellipsis() {
        assert_eq!(ellipsize("abcdefghij", 5), "abcd…");
    }

    #[test]
    fn cutting_respects_character_boundaries() {
        // Byte slicing here would panic mid-codepoint: each of these is three
        // bytes wide, so `max` counts characters, not bytes.
        assert_eq!(ellipsize("學問學問學問", 3), "學問…");
        assert_eq!(ellipsize("學問學問學問", 4), "學問學…");
        assert_eq!(ellipsize("學問學問學問", 6), "學問學問學問");
    }
}
