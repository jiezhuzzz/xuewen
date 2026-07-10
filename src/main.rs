use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

use xuewen::config::Config;
use xuewen::daily::{self, DailyService};
use xuewen::db;
use xuewen::models::Identifier;
use xuewen::pipeline::{IdentifyOutcome, IngestCtx, Libraries, Outcome};
use xuewen::refresh::{self, RefreshTarget};
use xuewen::resolve::grobid::Grobid;
use xuewen::resolve::http::RetryPolicy;
use xuewen::resolve::Resolver;
use xuewen::search::fts::FieldSel;
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
    /// Export papers as BibTeX or BibLaTeX.
    Export {
        /// Paper id (exact or unique prefix) for a single entry.
        #[arg(conflicts_with_all = ["all", "project"])]
        id: Option<String>,
        /// Export the whole (non-trashed) library.
        #[arg(long, conflicts_with = "project")]
        all: bool,
        /// Export all papers in this project (name or id).
        #[arg(long)]
        project: Option<String>,
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
        query: String,
        /// Comma-separated fields: title,authors,abstract,body (default all).
        #[arg(long)]
        fields: Option<String>,
        /// Keyword (BM25) engine only.
        #[arg(long, conflicts_with = "semantic_only")]
        keyword_only: bool,
        /// Semantic (embedding) engine only.
        #[arg(long)]
        semantic_only: bool,
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
    New {
        name: String,
        /// Optional free-text note (e.g. the manuscript's working title).
        #[arg(long)]
        note: Option<String>,
    },
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
    // Interactive serving answers uploads synchronously; keep retries short there.
    let retry = match &cli.command {
        Command::Serve { .. } => RetryPolicy::interactive(),
        _ => RetryPolicy::production(),
    };
    let resolver = Resolver::new_with_policy(cfg.contact_email.as_deref(), retry)?;
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
            Outcome::SameWork(id) => {
                match db::get_by_id(&pool, &id).await?.and_then(|p| p.cite_key) {
                    Some(key) => println!("already in library as {key} ({id})"),
                    None => println!("already in library ({id})"),
                }
            }
            Outcome::InTrash(id) => println!("in trash — run: xuewen restore {id}"),
        },
        Command::Import { input } => {
            let fetcher =
                xuewen::import::Fetcher::new(cfg.proxy.as_ref().map(|p| p.login_url.clone()))?;
            let cookie = db::get_setting(&pool, "proxy_cookie").await?;
            match xuewen::import::import_source(&fetcher, &ctx.resolver, &input, cookie.as_deref())
                .await
            {
                Ok(fetched) => {
                    let staged = cfg
                        .inbox_dir
                        .join("_uploads")
                        .join(format!("{}-import.pdf", uuid::Uuid::now_v7()));
                    tokio::fs::create_dir_all(staged.parent().unwrap()).await?;
                    tokio::fs::write(&staged, &fetched.bytes).await?;
                    match ctx.ingest_file_with_hint(&staged, fetched.hint).await {
                        Ok(Outcome::Ingested(id)) => println!("ingested {id}"),
                        Ok(Outcome::Duplicate) => println!("duplicate, skipped"),
                        Ok(Outcome::SameWork(id)) => println!("already in library ({id})"),
                        Ok(Outcome::InTrash(id)) => {
                            println!("in trash — run: xuewen restore {id}")
                        }
                        Err(e) => {
                            let _ = tokio::fs::remove_file(&staged).await;
                            return Err(e);
                        }
                    }
                }
                Err(xuewen::import::ImportError::Unsupported) => {
                    anyhow::bail!("could not recognize {input:?} as a URL, DOI, or arXiv id")
                }
                Err(xuewen::import::ImportError::CookieExpired) => anyhow::bail!(
                    "proxy session expired — refresh it: xuewen proxy-cookie --set '<cookie>'"
                ),
                Err(xuewen::import::ImportError::Unfetched { metadata }) => {
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
                Err(xuewen::import::ImportError::Network(e)) => {
                    return Err(e.context("fetch failed"))
                }
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
            match SearchService::open(pool.clone(), &cfg.search).await {
                Ok(s) => {
                    tokio::spawn(indexer::run(
                        s,
                        cfg.library_root.clone(),
                        std::time::Duration::from_secs(30),
                    ));
                }
                Err(e) => tracing::warn!("search indexing disabled: {e}"),
            }
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
        Command::Identify {
            id,
            doi,
            arxiv,
            title,
            pick,
            yes,
        } => {
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
                    .resolve(&Identifier::Doi(doi.clone()), None)
                    .await
                    .ok_or_else(|| {
                        anyhow::anyhow!("no Crossref record for doi {doi} — try --title")
                    })?
            } else if let Some(axv) = arxiv {
                ctx.resolver
                    .resolve(&Identifier::Arxiv(axv.clone()), None)
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
            let ingest = std::sync::Arc::new(web::Ingest {
                ctx,
                staging_dir: cfg.inbox_dir.join("_uploads"),
            });
            let search = match SearchService::open(pool.clone(), &cfg.search).await {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!("search disabled: {e}");
                    None
                }
            };
            if let Some(s) = &search {
                tokio::spawn(indexer::run(
                    s.clone(),
                    cfg.library_root.clone(),
                    std::time::Duration::from_secs(30),
                ));
            }
            let daily = DailyService::from_config(&cfg, pool.clone())?;
            if let Some(d) = &daily {
                tokio::spawn(daily::scheduler::run(d.clone()));
            }
            let chat = xuewen::chat::ChatService::from_config(&cfg.chat);
            if chat.is_none() {
                tracing::info!("paper chat disabled (no [[chat.models]] configured)");
            }
            web::serve(
                &host,
                port,
                pool,
                cfg.library_root.clone(),
                ingest,
                cfg.proxy.as_ref().map(|p| p.login_url.clone()),
                search,
                daily,
                chat,
            )
            .await?;
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
                for p in &targets {
                    let path = cfg.library_root.join(&p.rel_path);
                    match std::fs::remove_file(&path) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => tracing::warn!("could not remove {}: {e}", path.display()),
                    }
                    xuewen::chat::store::clear(&pool, &p.id).await?;
                    db::delete_row(&pool, &p.id).await?;
                }
                println!("purged {} paper(s)", targets.len());
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
                    match s.project.note {
                        Some(note) => {
                            println!("{}  ({} papers)  — {}", s.project.name, s.paper_count, note)
                        }
                        None => println!("{}  ({} papers)", s.project.name, s.paper_count),
                    }
                }
            }
            ProjectCmd::New { name, note } => {
                let name = name.trim();
                if name.is_empty() {
                    anyhow::bail!("project name cannot be empty");
                }
                let p = db::create_project(&pool, name, note.as_deref()).await?;
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
                let papers = db::list_papers(&pool, None, None, None, Some(&proj.id)).await?;
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
        Command::Export {
            id,
            all,
            project,
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
                if !all && project.is_none() {
                    anyhow::bail!("specify a paper id, --all, or --project <name>");
                }
                let project_id = match &project {
                    Some(sel) => Some(db::find_one_project(&pool, sel).await?.id),
                    None => None,
                };
                let papers = db::list_papers(
                    &pool,
                    query.as_deref(),
                    status.as_deref(),
                    None,
                    project_id.as_deref(),
                )
                .await?;
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
            let svc = SearchService::open(pool.clone(), &cfg.search).await?;
            let req = xuewen::search::SearchRequest {
                q: query,
                fields: FieldSel::parse(fields.as_deref()),
                keyword: !semantic_only,
                semantic: !keyword_only,
                status: None,
                project: None,
            };
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
                    None => m.field.clone(),
                };
                println!("      [{loc}] {}", strip_snippet_html(&m.snippet));
            }
        }
        Command::Index { cmd } => match cmd {
            IndexCmd::Status => {
                let svc = SearchService::open(pool.clone(), &cfg.search).await?;
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
                let do_fts = !vectors_only;
                let do_vectors = !fts_only;
                if do_fts {
                    // Refuse to wipe an index another process is writing
                    // (tantivy's writer lock, e.g. a running `xuewen serve`).
                    if cfg.search.index_dir.join("meta.json").exists() {
                        let (probe, _) =
                            xuewen::search::fts::FtsIndex::open(&cfg.search.index_dir)?;
                        probe.delete("__rebuild_lock_probe__").map_err(|e| {
                            anyhow::anyhow!(
                                "search index at {} is in use (is `xuewen serve` running?) — stop it and retry ({e})",
                                cfg.search.index_dir.display()
                            )
                        })?;
                        drop(probe);
                    }
                    // Wipe before opening: SearchService::open detects the
                    // fresh directory and clears the FTS stamps itself.
                    match std::fs::remove_dir_all(&cfg.search.index_dir) {
                        Ok(()) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                        Err(e) => anyhow::bail!(
                            "could not clear search index dir {}: {e}",
                            cfg.search.index_dir.display()
                        ),
                    }
                }
                let svc = SearchService::open(pool.clone(), &cfg.search).await?;
                xuewen::search::store::clear_stamps(&pool, do_fts, do_vectors).await?;
                if do_vectors && svc.embedder.is_some() {
                    svc.vectors.recreate_collection().await?;
                }
                let s = indexer::sweep(&svc, &cfg.library_root).await?;
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
