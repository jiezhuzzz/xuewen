use anyhow::{anyhow, Context as _, Result};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::config::Config;
use crate::http::RetryPolicy;
use crate::models::{Authors, Identifier, Paper, PaperMeta, PaperStatus};
use crate::naming;
use crate::resolve::grobid::Grobid;
use crate::resolve::{ResolvedMetadata, Resolver, TitleQuery};
use crate::{db, hash, identify, pdf};

/// Directories the pipeline manages. `under` is the one place the managed
/// inbox subdirectory names (`_processed`, `_uploads`, `_failed`) are spelled.
pub struct Libraries {
    pub library_root: PathBuf,
    /// Where ingested originals are archived out of the inbox.
    pub processed_dir: PathBuf,
    /// Where fetched/uploaded bytes are staged before ingest (`ingest_bytes`).
    pub staging_dir: PathBuf,
    /// Where the watcher quarantines PDFs that repeatedly fail to ingest.
    pub failed_dir: PathBuf,
}

impl Libraries {
    /// The managed directories under an inbox and a library root.
    pub fn under(inbox_dir: &Path, library_root: &Path) -> Self {
        Self {
            library_root: library_root.to_path_buf(),
            processed_dir: inbox_dir.join("_processed"),
            staging_dir: inbox_dir.join("_uploads"),
            failed_dir: inbox_dir.join("_failed"),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ingested(String),  // new paper id
    Duplicate(String), // same bytes as an active paper → its id
    SameWork(String),  // same DOI/arXiv id as an active paper → its id
    InTrash(String),   // same bytes or identifier as a trashed paper → its id
}

/// What happens to the source file once ingest reaches an outcome. An ingest
/// *error* always leaves the source in place for the caller to handle (the
/// watcher retries/quarantines, `ingest_bytes` deletes its staged copy).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposal {
    /// Archive into `_processed`: real user files (inbox watcher, CLI ingest).
    Archive,
    /// Delete: app-created staged copies (`ingest_bytes`), whose bytes ingest
    /// has already filed into the library — archiving those too would store
    /// every upload/URL import twice forever.
    Discard,
}

/// The raw inputs a resolution produces from a stored PDF, shared by ingest and
/// refresh. Consumed by `resolve_fields`.
pub(crate) struct ResolveInputs {
    pub(crate) ident: Identifier,
    pub(crate) provisional_title: Option<String>,
    pub(crate) extracted: Option<ResolvedMetadata>,
    pub(crate) resolution: Option<ResolvedMetadata>,
}

/// Everything the ingest/refresh pipeline needs.
pub struct IngestCtx {
    pub pool: SqlitePool,
    pub dirs: Libraries,
    pub resolver: Resolver,
    pub grobid: Option<Grobid>,
}

impl IngestCtx {
    /// Build the full ingest stack from config. The retry policy is the
    /// caller's one decision: `production()` for CLI batch work,
    /// `interactive()` for serving (uploads answer synchronously).
    pub fn from_config(cfg: &Config, pool: SqlitePool, retry: RetryPolicy) -> Result<Self> {
        let resolver = Resolver::new_with_policy(cfg.contact_email.as_deref(), retry)?;
        let grobid = cfg.grobid_url.as_deref().map(Grobid::new).transpose()?;
        Ok(Self {
            pool,
            dirs: Libraries::under(&cfg.inbox_dir, &cfg.library_root),
            resolver,
            grobid,
        })
    }

    /// Ingest a single PDF with no identifier hint (text extraction decides),
    /// archiving the original out of the inbox.
    pub async fn ingest_file(&self, path: &Path) -> Result<Outcome> {
        self.ingest_file_with_hint(path, None, Disposal::Archive)
            .await
    }

    /// Stage `bytes` under a collision-safe name in the staging dir, ingest
    /// the staged file, and delete it on ingest failure. Shared by CLI import,
    /// web upload, and web URL import.
    pub async fn ingest_bytes(
        &self,
        bytes: &[u8],
        name: &str,
        hint: Option<Identifier>,
    ) -> Result<Outcome> {
        // Reduce `name` to its basename so a hostile filename cannot escape
        // the staging dir.
        let stem = Path::new(name)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("import.pdf");
        let staged = self
            .dirs
            .staging_dir
            .join(format!("{}-{stem}", Uuid::now_v7()));
        tokio::fs::create_dir_all(&self.dirs.staging_dir)
            .await
            .context("import staging dir")?;
        tokio::fs::write(&staged, bytes)
            .await
            .context("import stage write")?;
        match self
            .ingest_file_with_hint(&staged, hint, Disposal::Discard)
            .await
        {
            Ok(outcome) => Ok(outcome),
            Err(e) => {
                let _ = tokio::fs::remove_file(&staged).await;
                Err(e)
            }
        }
    }

    /// Ingest a single PDF, optionally seeding metadata resolution with a known
    /// identifier (used by URL/DOI import, where we already know the id and the
    /// PDF's first page may not print it). `disposal` decides what happens to
    /// the source once an outcome is reached; on `Err` it is left in place.
    pub async fn ingest_file_with_hint(
        &self,
        path: &Path,
        hint: Option<Identifier>,
        disposal: Disposal,
    ) -> Result<Outcome> {
        let outcome = self.ingest_inner(path, hint).await?;
        // The one disposal point, shared by every outcome.
        match disposal {
            Disposal::Archive => move_to_async(path, &self.dirs.processed_dir).await?,
            Disposal::Discard => tokio::fs::remove_file(path)
                .await
                .context("discard ingest source")?,
        }
        Ok(outcome)
    }

    async fn ingest_inner(&self, path: &Path, hint: Option<Identifier>) -> Result<Outcome> {
        let path = path.to_path_buf();

        // 1. Hash (blocking IO off the async runtime).
        let content_hash = {
            let p = path.clone();
            tokio::task::spawn_blocking(move || hash::sha256_file(&p)).await??
        };

        // 2. Dedup by content (active → Duplicate, trashed → InTrash).
        if let Some(existing) = db::find_by_hash(&self.pool, &content_hash).await? {
            return Ok(if existing.deleted_at.is_some() {
                Outcome::InTrash(existing.id)
            } else {
                Outcome::Duplicate(existing.id)
            });
        }

        // 3. Extract, identify, optionally GROBID, and resolve (factored for reuse).
        let ResolveInputs {
            ident,
            provisional_title,
            extracted,
            resolution,
        } = self.resolve_pdf(&path, hint).await?;

        // 4. Decide the stored fields, then the cite-key filename.
        let fields = resolve_fields(provisional_title, extracted, &ident, resolution);

        // 4b. A different file of a work we already have (same DOI/arXiv id)?
        if let Some(existing) = db::find_by_identifier(
            &self.pool,
            fields.doi.as_deref(),
            fields.arxiv_id.as_deref(),
        )
        .await?
        {
            return Ok(if existing.deleted_at.is_some() {
                Outcome::InTrash(existing.id)
            } else {
                Outcome::SameWork(existing.id)
            });
        }

        // 5. Pick a free cite key and file the PDF under it exclusively.
        // Disambiguation is read-compute-write with no lock, so two concurrent
        // ingests sharing a base key can compute the same "free" key; the
        // exclusive create turns that into a collision the loser observes.
        // The loser must not merely re-query the DB — the winner copies before
        // inserting, so its key may not be a row yet; the collided key joins a
        // local taken set instead.
        let base = naming::cite_key_base(&fields.authors.0, fields.year, fields.title.as_deref());
        let mut taken = match base.as_deref() {
            Some(b) => db::cite_keys_with_base(&self.pool, b, None).await?,
            None => std::collections::HashSet::new(),
        };
        let (cite_key, rel_path, dest, created_excl) = loop {
            let cite_key = base.as_deref().map(|b| naming::disambiguate(b, &taken));
            let rel_path = naming::library_rel_path(cite_key.as_deref(), &content_hash);
            let dest = self.dirs.library_root.join(&rel_path);
            match cite_key {
                Some(key) => match copy_to_excl_async(&path, &dest).await {
                    Ok(()) => break (Some(key), rel_path, dest, true),
                    Err(e) if is_already_exists(&e) => {
                        taken.insert(key);
                    }
                    Err(e) => return Err(e),
                },
                // No cite key: the path is content-addressed
                // (`_unsorted/<hash>.pdf`), so an existing dest already holds
                // these bytes (or a truncated copy from a crashed run) — a
                // plain overwrite is a repair, never a clobber.
                None => {
                    copy_to_async(&path, &dest).await?;
                    break (None, rel_path, dest, false);
                }
            }
        };

        // 6. Build and store the record.
        let paper = fields.into_paper(content_hash, rel_path, cite_key);
        if let Err(e) = db::insert_paper(&self.pool, &paper).await {
            // Clean up only a file this ingest exclusively created: the
            // content-addressed path may be shared with — and still referenced
            // by — the concurrent winner of a same-bytes race.
            if created_excl {
                let _ = tokio::fs::remove_file(&dest).await;
            }
            // Lost a race with a concurrent ingest of the same work? Report the
            // winner's outcome instead of surfacing a constraint error.
            if db::is_unique_violation(&e) {
                tracing::warn!(
                    "insert hit a UNIQUE constraint (concurrent ingest of the same work?); re-checking"
                );
                if let Some(outcome) = recover_unique_collision(
                    &self.pool,
                    &paper.content_hash,
                    paper.meta.doi.as_deref(),
                    paper.meta.arxiv_id.as_deref(),
                )
                .await?
                {
                    return Ok(outcome);
                }
            }
            return Err(e);
        }

        Ok(Outcome::Ingested(paper.id))
    }

    /// Extract first-page text, identify a DOI/arXiv id, optionally enrich via GROBID
    /// (title-only path), and resolve authoritative metadata. Degrades to
    /// `None` on any resolver/network failure — never aborts.
    pub(crate) async fn resolve_pdf(
        &self,
        path: &Path,
        hint: Option<Identifier>,
    ) -> Result<ResolveInputs> {
        // Extract first-page text (blocking IO off the async runtime) and identify.
        let text = {
            let p = path.to_path_buf();
            tokio::task::spawn_blocking(move || pdf::extract_text(&p, 1)).await??
        };
        let ident = hint.unwrap_or_else(|| identify::identify(&text));
        let provisional_title = identify::guess_title(&text);

        // For the title-only path, optionally use GROBID for a better header
        // (degrades to None on failure).
        let extracted: Option<ResolvedMetadata> = match (&ident, self.grobid.as_ref()) {
            (Identifier::None, Some(g)) => match g.extract_header(path).await {
                Ok(md) => md,
                Err(e) => {
                    tracing::warn!("grobid extraction failed: {e}");
                    None
                }
            },
            _ => None,
        };

        // Search query prefers the GROBID title, else the heuristic first line.
        let title_hint: Option<String> = extracted
            .as_ref()
            .and_then(|m| m.title.clone())
            .or_else(|| provisional_title.clone());

        // The first-page text goes along as corroboration: a title alone cannot
        // tell this paper from a same-titled work by other authors.
        let query = title_hint
            .as_deref()
            .map(|title| TitleQuery { title, text: &text });
        let resolution = self.resolver.resolve(&ident, query).await;
        Ok(ResolveInputs {
            ident,
            provisional_title,
            extracted,
            resolution,
        })
    }

    /// Persist `paper` and re-file its PDF to the cite-key path implied by its
    /// current metadata. Copy-first ordering: copy the file, update the row,
    /// remove the old file — a failure at any step never leaves the DB
    /// pointing at a missing file. If the current file is missing or
    /// unreadable, the copy step fails and only the metadata is persisted —
    /// the row keeps its previous `rel_path`/`cite_key` and the caller gets
    /// `Ok(false)`. Returns whether the file moved.
    pub async fn save_and_refile(&self, paper: &mut Paper) -> Result<bool> {
        let pdf = self.dirs.library_root.join(&paper.rel_path);
        let cite_key = match naming::cite_key_base(
            &paper.meta.authors.0,
            paper.meta.year,
            paper.meta.title.as_deref(),
        ) {
            Some(base) => {
                let taken = db::cite_keys_with_base(&self.pool, &base, Some(&paper.id)).await?;
                Some(naming::disambiguate(&base, &taken))
            }
            None => None,
        };
        let new_rel = naming::library_rel_path(cite_key.as_deref(), &paper.content_hash);
        let mut refiled_paths: Option<(std::path::PathBuf, std::path::PathBuf)> = None; // (old, new)
        if new_rel != paper.rel_path {
            let to = self.dirs.library_root.join(&new_rel);
            // Exclusive create: an occupied destination usually means a
            // concurrent filing race and lands in the same leave-in-place
            // degradation as any other copy failure. The exception is a file
            // holding this paper's own bytes — a crash between a previous
            // refile's copy and its row update leaves exactly that, and every
            // retry would hit it forever; adopt it and finish the refile.
            match copy_to_excl_async(&pdf, &to).await {
                Ok(()) => {
                    refiled_paths = Some((pdf.clone(), to));
                    paper.rel_path = new_rel;
                    paper.cite_key = cite_key;
                }
                Err(e) => {
                    if is_already_exists(&e) && file_hash_matches(&to, &paper.content_hash).await {
                        refiled_paths = Some((pdf.clone(), to));
                        paper.rel_path = new_rel;
                        paper.cite_key = cite_key;
                    } else {
                        tracing::warn!(
                            "re-file copy failed for {}: {e}; leaving in place",
                            paper.id
                        )
                    }
                }
            }
        }

        if let Err(e) = db::update_paper(&self.pool, paper).await {
            // Roll the copy back so filesystem and DB stay consistent.
            if let Some((_, new_path)) = &refiled_paths {
                let _ = tokio::fs::remove_file(new_path).await;
            }
            return Err(e);
        }
        if let Some((old_path, _)) = &refiled_paths {
            if let Err(e) = tokio::fs::remove_file(old_path).await {
                tracing::warn!("could not remove old file {}: {e}", old_path.display());
            }
        }
        Ok(refiled_paths.is_some())
    }

    /// Apply user-confirmed metadata to `paper`: guard identifier conflicts,
    /// overwrite the metadata block (keeping the old abstract when the source
    /// has none), mark it resolved, then persist + re-file.
    pub async fn apply_match(
        &self,
        paper: &mut Paper,
        md: ResolvedMetadata,
    ) -> Result<IdentifyOutcome> {
        if paper.deleted_at.is_some() {
            return Ok(IdentifyOutcome::Trashed);
        }
        if let Some(existing) =
            db::find_by_identifier(&self.pool, md.doi.as_deref(), md.arxiv_id.as_deref()).await?
        {
            if existing.id != paper.id {
                return Ok(IdentifyOutcome::SameWork(existing.id));
            }
        }
        let mut meta = PaperMeta::from_resolved(md);
        meta.abstract_text = meta
            .abstract_text
            .or_else(|| paper.meta.abstract_text.clone());
        paper.meta = meta;
        // No file-existence pre-check: fixing metadata must succeed even if the
        // PDF is missing; save_and_refile degrades to metadata-only in that case.
        self.save_and_refile(paper).await?;
        Ok(IdentifyOutcome::Applied)
    }
}

/// Permanently delete `paper`: the library PDF, its chat/code sidecars, the
/// agent workspace, then the row. The sidecar deletes stay explicit —
/// belt-and-braces beside the schema's ON DELETE cascades, whose regressions
/// they would otherwise mask (see tests/web_code_test.rs). PDF and workspace
/// removal degrade to a warning/no-op: a missing file must not block a purge.
pub async fn purge_paper(pool: &SqlitePool, library_root: &Path, paper: &Paper) -> Result<()> {
    let path = library_root.join(&paper.rel_path);
    match std::fs::remove_file(&path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => tracing::warn!("could not remove {}: {e}", path.display()),
    }
    crate::chat::store::clear(pool, &paper.id).await?;
    crate::agent::store::delete_paper_code(pool, &paper.id).await?;
    let _ = tokio::fs::remove_dir_all(crate::agent::workspace_dir(library_root, &paper.id)).await;
    db::delete_row(pool, &paper.id).await?;
    Ok(())
}

/// Result of applying a user-confirmed identify match.
#[derive(Debug, PartialEq, Eq)]
pub enum IdentifyOutcome {
    Applied,
    /// The chosen identifier already belongs to this other paper; no changes.
    SameWork(String),
    /// The paper is in the trash; restore it first. No changes.
    Trashed,
}

/// Decide the stored fields. A confident resolution yields `resolved` (with a
/// GROBID abstract backfilled if the source lacked one); otherwise `needs_review`,
/// enriched with GROBID's title/abstract/authors when present.
pub(crate) fn resolve_fields(
    provisional_title: Option<String>,
    extracted: Option<ResolvedMetadata>,
    ident: &Identifier,
    resolution: Option<ResolvedMetadata>,
) -> PaperMeta {
    let (ext_doi, ext_arxiv) = match ident {
        Identifier::Doi(d) => (Some(d.clone()), None),
        Identifier::Arxiv(a) => (None, Some(a.clone())),
        Identifier::None => (None, None),
    };
    match resolution {
        Some(md) => {
            let mut meta = PaperMeta::from_resolved(md);
            meta.title = meta.title.or(provisional_title);
            meta.abstract_text = meta
                .abstract_text
                .or_else(|| extracted.and_then(|g| g.abstract_text));
            meta.doi = meta.doi.or(ext_doi);
            meta.arxiv_id = meta.arxiv_id.or(ext_arxiv);
            meta
        }
        None => match extracted {
            Some(g) => PaperMeta {
                title: g.title.or(provisional_title),
                abstract_text: g.abstract_text,
                authors: Authors(g.authors),
                venue: None,
                year: None,
                doi: ext_doi,
                arxiv_id: ext_arxiv,
                dblp_key: None,
                url: None,
                source: Some(g.source),
                status: PaperStatus::NeedsReview,
            },
            None => PaperMeta {
                title: provisional_title,
                abstract_text: None,
                authors: Authors::default(),
                venue: None,
                year: None,
                doi: ext_doi,
                arxiv_id: ext_arxiv,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::NeedsReview,
            },
        },
    }
}

impl PaperMeta {
    /// The direct mapping of a confident resolution: every field carried over,
    /// status `Resolved`. The one place resolver output turns into stored
    /// metadata — `resolve_fields` and `apply_match` layer their differing
    /// fallbacks (provisional title, kept abstract, extracted-id merges) on
    /// top rather than re-spelling the field list.
    pub(crate) fn from_resolved(md: ResolvedMetadata) -> Self {
        PaperMeta {
            title: md.title,
            abstract_text: md.abstract_text,
            authors: Authors(md.authors),
            venue: md.venue,
            year: md.year,
            doi: md.doi,
            arxiv_id: md.arxiv_id,
            dblp_key: md.dblp_key,
            url: md.url,
            source: Some(md.source),
            status: PaperStatus::Resolved,
        }
    }

    /// Assemble a full `Paper` with a fresh id/timestamp and the given location.
    pub(crate) fn into_paper(
        self,
        content_hash: String,
        rel_path: String,
        cite_key: Option<String>,
    ) -> Paper {
        Paper {
            id: Uuid::now_v7().to_string(),
            content_hash,
            rel_path,
            cite_key,
            added_at: db::now_rfc3339(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: self,
        }
    }
}

/// After a UNIQUE violation on insert, find the row that won the race and map
/// it to the outcome the pre-insert checks would have produced.
pub(crate) async fn recover_unique_collision(
    pool: &SqlitePool,
    content_hash: &str,
    doi: Option<&str>,
    arxiv_id: Option<&str>,
) -> Result<Option<Outcome>> {
    if let Some(existing) = db::find_by_hash(pool, content_hash).await? {
        return Ok(Some(if existing.deleted_at.is_some() {
            Outcome::InTrash(existing.id)
        } else {
            Outcome::Duplicate(existing.id)
        }));
    }
    if let Some(existing) = db::find_by_identifier(pool, doi, arxiv_id).await? {
        return Ok(Some(if existing.deleted_at.is_some() {
            Outcome::InTrash(existing.id)
        } else {
            Outcome::SameWork(existing.id)
        }));
    }
    Ok(None)
}

/// Move `src` into `dir` under its basename, uniquifying on collision
/// (`name-1.pdf`, `name-2.pdf`, …): `_processed`/`_failed` are archives, and
/// `rename` replaces silently on Unix — a later same-named inbox drop must
/// not destroy the bytes an earlier one archived (for `SameWork` those bytes
/// exist nowhere else). Falls back to copy+remove across filesystems.
pub(crate) fn move_to(src: &Path, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let name = src
        .file_name()
        .ok_or_else(|| anyhow!("path has no file name"))?;
    if is_same_file(&dir.join(name), src) {
        // Already in the archive (a dedup re-ingest from `_processed`
        // itself), under whatever spelling the caller used: HEAD's plain
        // rename was a POSIX no-op on two names for the same file, and the
        // uniquify loop below would instead shuffle the archived file to a
        // `-1` suffix on every re-ingest.
        return Ok(());
    }
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("file")
        .to_string();
    let ext = Path::new(name).extension().and_then(|s| s.to_str());
    // Reserve a free name with an exclusive create so the probe itself cannot
    // be clobbered by a concurrent mover; renaming over our own reservation
    // (or truncating it in the copy fallback) is then safe.
    let mut dest = dir.join(name);
    for n in 1.. {
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
        {
            Ok(_) => break,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                dest = dir.join(match ext {
                    Some(ext) => format!("{stem}-{n}.{ext}"),
                    None => format!("{stem}-{n}"),
                });
            }
            Err(e) => return Err(e.into()),
        }
    }
    if std::fs::rename(src, &dest).is_err() {
        if let Err(e) = std::fs::copy(src, &dest) {
            // The reservation is ours alone, and empty: left behind it would
            // wear the PDF's name in the archive (the watcher's quarantine
            // signal is bare existence) and push later same-named archives to
            // suffixes. A successful copy whose trailing source removal fails
            // is different — dest then holds the full bytes and stays.
            let _ = std::fs::remove_file(&dest);
            return Err(e.into());
        }
        std::fs::remove_file(src)?;
    }
    Ok(())
}

/// Whether two paths name the same file, by (dev, ino) — path spellings
/// (relative vs absolute, `./` prefixes, symlinked components) don't
/// normalize under `PathBuf` equality. Any metadata error reads as "not the
/// same file": a vanished `src` must fall through to the move's own error
/// path rather than fail here.
fn is_same_file(a: &Path, b: &Path) -> bool {
    if a == b {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(ma), Ok(mb)) = (std::fs::metadata(a), std::fs::metadata(b)) {
            return ma.dev() == mb.dev() && ma.ino() == mb.ino();
        }
    }
    false
}

/// Whether the file at `path` hashes to `content_hash`; any read failure is
/// "no".
async fn file_hash_matches(path: &Path, content_hash: &str) -> bool {
    let p = path.to_path_buf();
    matches!(
        tokio::task::spawn_blocking(move || hash::sha256_file(&p)).await,
        Ok(Ok(h)) if h == content_hash
    )
}

/// Copy `from` to the exact path `to`, creating parent directories.
pub(crate) fn copy_to(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to)?;
    Ok(())
}

/// The exclusive create found the destination already present. A dedicated
/// type rather than matching `io::ErrorKind::AlreadyExists` anywhere in the
/// chain: `create_dir_all` also fails with `AlreadyExists` when `library_root`
/// exists as a regular file, and reading that as a cite-key collision would
/// send the filing loop through an endless suffix walk instead of surfacing
/// the misconfiguration.
#[derive(Debug)]
pub(crate) struct DestExists;

impl std::fmt::Display for DestExists {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("destination file already exists")
    }
}

impl std::error::Error for DestExists {}

/// `copy_to`, but refusing to replace an existing file (`create_new`):
/// cite-key destinations are chosen by a racy read-compute-write, and
/// `std::fs::copy` would silently truncate the concurrent winner's file.
/// A destination this call created but could not fill is removed before the
/// error propagates — a stranded partial file would otherwise hold the path
/// against every retry (and steal the cite key for good).
pub(crate) fn copy_to_excl(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut dest = match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(to)
    {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(anyhow::Error::new(DestExists));
        }
        Err(e) => return Err(e.into()),
    };
    let copied = std::fs::File::open(from)
        .and_then(|mut src| std::io::copy(&mut src, &mut dest))
        .map(|_| ());
    if copied.is_err() {
        // Only ever our own fresh file: a loser of the create_new race
        // returns above without having created anything.
        let _ = std::fs::remove_file(to);
    }
    Ok(copied?)
}

/// Whether `e` is `copy_to_excl` reporting an occupied destination (a lost
/// `create_new` race). Any other failure — including `AlreadyExists` from
/// `create_dir_all` — is a real error, not a collision.
pub(crate) fn is_already_exists(e: &anyhow::Error) -> bool {
    e.chain().any(|c| c.downcast_ref::<DestExists>().is_some())
}

/// `move_to` off the async runtime.
pub(crate) async fn move_to_async(src: &Path, dir: &Path) -> Result<()> {
    let (src, dir) = (src.to_path_buf(), dir.to_path_buf());
    tokio::task::spawn_blocking(move || move_to(&src, &dir)).await?
}

/// `copy_to` off the async runtime.
pub(crate) async fn copy_to_async(from: &Path, to: &Path) -> Result<()> {
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    tokio::task::spawn_blocking(move || copy_to(&from, &to)).await?
}

/// `copy_to_excl` off the async runtime.
pub(crate) async fn copy_to_excl_async(from: &Path, to: &Path) -> Result<()> {
    let (from, to) = (from.to_path_buf(), to.to_path_buf());
    tokio::task::spawn_blocking(move || copy_to_excl(&from, &to)).await?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;
    use crate::models::{Authors, Paper, PaperMeta, PaperStatus};

    fn paper(id: &str, hash: &str, doi: Option<&str>) -> Paper {
        Paper {
            id: id.into(),
            content_hash: hash.into(),
            rel_path: format!("{hash}.pdf"),
            cite_key: None,
            added_at: "2026-07-08T00:00:00Z".into(),
            deleted_at: None,
            starred: false,
            name: None,
            meta: PaperMeta {
                title: Some("T".into()),
                abstract_text: None,
                authors: Authors::default(),
                venue: None,
                year: None,
                doi: doi.map(str::to_string),
                arxiv_id: None,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::NeedsReview,
            },
        }
    }

    #[tokio::test]
    async fn recover_unique_collision_maps_all_cases() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let pool = db::connect(&url).await.unwrap();
        let a = paper("01890000-0000-7000-8000-0000000000aa", "h1", Some("10.1/x"));
        db::insert_paper(&pool, &a).await.unwrap();

        // Hash collision with an active row → Duplicate.
        assert_eq!(
            recover_unique_collision(&pool, "h1", None, None)
                .await
                .unwrap(),
            Some(Outcome::Duplicate(a.id.clone()))
        );
        // Identifier collision with an active row → SameWork.
        assert_eq!(
            recover_unique_collision(&pool, "h2", Some("10.1/x"), None)
                .await
                .unwrap(),
            Some(Outcome::SameWork(a.id.clone()))
        );
        // Trashed row → InTrash for both shapes.
        db::soft_delete(&pool, &a.id).await.unwrap();
        assert_eq!(
            recover_unique_collision(&pool, "h1", None, None)
                .await
                .unwrap(),
            Some(Outcome::InTrash(a.id.clone()))
        );
        assert_eq!(
            recover_unique_collision(&pool, "h2", Some("10.1/x"), None)
                .await
                .unwrap(),
            Some(Outcome::InTrash(a.id.clone()))
        );
        // No matching row → None (the violation was something else).
        assert_eq!(
            recover_unique_collision(&pool, "h3", Some("10.9/none"), None)
                .await
                .unwrap(),
            None
        );
    }

    #[test]
    fn move_to_uniquifies_archive_collisions() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("_processed");
        let a = dir.path().join("paper.pdf");
        let b = dir.path().join("sub");
        std::fs::create_dir_all(&b).unwrap();
        let b = b.join("paper.pdf");
        std::fs::write(&a, b"first bytes").unwrap();
        std::fs::write(&b, b"second bytes").unwrap();

        move_to(&a, &archive).unwrap();
        move_to(&b, &archive).unwrap();

        // The earlier archive entry survives; the later drop gets a suffix.
        assert_eq!(
            std::fs::read(archive.join("paper.pdf")).unwrap(),
            b"first bytes"
        );
        assert_eq!(
            std::fs::read(archive.join("paper-1.pdf")).unwrap(),
            b"second bytes"
        );

        // Re-archiving a file already at its archive path is a no-op.
        move_to(&archive.join("paper.pdf"), &archive).unwrap();
        assert_eq!(
            std::fs::read(archive.join("paper.pdf")).unwrap(),
            b"first bytes"
        );
        assert!(!archive.join("paper-2.pdf").exists());
    }

    #[test]
    fn move_to_cleans_its_reservation_when_the_move_fails() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("_failed");
        // Source gone (concurrent mover, or deleted mid-quarantine): both the
        // rename and the copy fallback fail.
        assert!(move_to(&dir.path().join("bad.pdf"), &archive).is_err());
        assert!(
            !archive.join("bad.pdf").exists(),
            "no empty reservation may impersonate a quarantined PDF"
        );

        // The name is still free for a real quarantine.
        let src = dir.path().join("bad.pdf");
        std::fs::write(&src, b"real bytes").unwrap();
        move_to(&src, &archive).unwrap();
        assert_eq!(
            std::fs::read(archive.join("bad.pdf")).unwrap(),
            b"real bytes"
        );
    }

    #[test]
    fn move_to_no_ops_on_a_differently_spelled_same_file() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("_processed");
        let src = dir.path().join("paper.pdf");
        std::fs::write(&src, b"bytes").unwrap();
        move_to(&src, &archive).unwrap();

        // Re-ingest via a spelling PathBuf equality can't see through.
        let respelled = archive.join("..").join("_processed").join("paper.pdf");
        assert_ne!(respelled, archive.join("paper.pdf"));
        move_to(&respelled, &archive).unwrap();
        assert_eq!(std::fs::read(archive.join("paper.pdf")).unwrap(), b"bytes");
        assert!(
            !archive.join("paper-1.pdf").exists(),
            "the archived file must not be shuffled to a suffix"
        );
    }

    #[test]
    fn copy_to_excl_removes_its_partial_destination_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("lib/key.pdf");
        let err = copy_to_excl(&dir.path().join("missing.pdf"), &dest).unwrap_err();
        assert!(!is_already_exists(&err));
        assert!(
            !dest.exists(),
            "a failed copy must not strand a partial destination"
        );

        // The key is still free: a retry files under the canonical name
        // instead of ceding it to garbage and taking a suffix.
        let src = dir.path().join("src.pdf");
        std::fs::write(&src, b"bytes").unwrap();
        copy_to_excl(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"bytes");
    }

    #[test]
    fn a_file_squatting_the_destination_parent_is_not_a_key_collision() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.pdf");
        std::fs::write(&src, b"bytes").unwrap();
        std::fs::write(dir.path().join("library"), b"not a directory").unwrap();
        let err = copy_to_excl(&src, &dir.path().join("library").join("key.pdf")).unwrap_err();
        assert!(
            !is_already_exists(&err),
            "create_dir_all's AlreadyExists must not read as a cite-key \
             collision — the filing loop would walk suffixes forever"
        );
    }

    #[test]
    fn copy_to_excl_refuses_to_replace() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.pdf");
        let dest = dir.path().join("lib/key.pdf");
        std::fs::write(&src, b"new").unwrap();
        copy_to_excl(&src, &dest).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"new");

        std::fs::write(&src, b"other work, same key").unwrap();
        let err = copy_to_excl(&src, &dest).unwrap_err();
        assert!(is_already_exists(&err));
        assert_eq!(std::fs::read(&dest).unwrap(), b"new", "winner untouched");
    }

    #[tokio::test]
    async fn ingest_bytes_sanitizes_the_name_and_cleans_up_on_failure() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let pool = db::connect(&url).await.unwrap();
        let inbox = dir.path().join("inbox");
        let ctx = IngestCtx {
            pool,
            dirs: Libraries::under(&inbox, &dir.path().join("library")),
            resolver: Resolver::with_bases(
                None,
                "http://127.0.0.1:1".to_string(),
                "http://127.0.0.1:1".to_string(),
            )
            .unwrap(),
            grobid: None,
        };

        // Junk bytes fail extraction. The hostile name must stage as a bare
        // basename inside _uploads, and the staged copy must not survive the
        // failed ingest.
        let out = ctx.ingest_bytes(b"not a pdf", "../evil.pdf", None).await;
        assert!(out.is_err());
        assert!(!dir.path().join("evil.pdf").exists());
        assert!(!inbox.join("evil.pdf").exists());
        assert_eq!(
            std::fs::read_dir(&ctx.dirs.staging_dir).unwrap().count(),
            0,
            "staged file must be removed on ingest failure"
        );
    }

    /// An `IngestCtx` wired to dead resolver endpoints (no network use).
    async fn offline_ctx(root: &Path, library: &Path) -> IngestCtx {
        let url = format!("sqlite:{}", root.join("t.db").display());
        let pool = db::connect(&url).await.unwrap();
        IngestCtx {
            pool,
            dirs: Libraries::under(root, library),
            resolver: Resolver::with_bases(
                None,
                "http://127.0.0.1:1".to_string(),
                "http://127.0.0.1:1".to_string(),
            )
            .unwrap(),
            grobid: None,
        }
    }

    /// Seed a paper whose metadata refiles to `he2016deep.pdf`, with `bytes`
    /// at `old.pdf`.
    async fn refile_fixture(ctx: &IngestCtx, library: &Path, bytes: &[u8]) -> Paper {
        std::fs::create_dir_all(library).unwrap();
        let src = library.join("hash-me.tmp");
        std::fs::write(&src, bytes).unwrap();
        let content_hash = crate::hash::sha256_file(&src).unwrap();
        std::fs::remove_file(&src).unwrap();
        let mut p = paper("01890000-0000-7000-8000-0000000000c3", &content_hash, None);
        p.rel_path = "old.pdf".into();
        p.meta.title = Some("Deep Residual Learning".into());
        p.meta.authors = Authors(vec!["Kaiming He".into()]);
        p.meta.year = Some(2016);
        db::insert_paper(&ctx.pool, &p).await.unwrap();
        std::fs::write(library.join("old.pdf"), bytes).unwrap();
        p
    }

    #[tokio::test]
    async fn save_and_refile_adopts_its_own_completed_copy() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("library");
        let ctx = offline_ctx(dir.path(), &library).await;
        let bytes = b"%PDF-1.4 same bytes";
        let mut p = refile_fixture(&ctx, &library, bytes).await;
        // A previous refile's completed copy, orphaned by a crash before the
        // row update: the destination already holds this paper's own bytes.
        std::fs::write(library.join("he2016deep.pdf"), bytes).unwrap();

        assert!(ctx.save_and_refile(&mut p).await.unwrap());
        assert_eq!(p.rel_path, "he2016deep.pdf");
        assert_eq!(p.cite_key.as_deref(), Some("he2016deep"));
        assert!(!library.join("old.pdf").exists(), "old file removed");
        assert_eq!(
            std::fs::read(library.join("he2016deep.pdf")).unwrap(),
            bytes
        );
        let row = db::get_by_id(&ctx.pool, &p.id).await.unwrap().unwrap();
        assert_eq!(row.rel_path, "he2016deep.pdf");
    }

    #[tokio::test]
    async fn save_and_refile_still_defers_to_a_foreign_occupant() {
        let dir = tempfile::tempdir().unwrap();
        let library = dir.path().join("library");
        let ctx = offline_ctx(dir.path(), &library).await;
        let mut p = refile_fixture(&ctx, &library, b"%PDF-1.4 same bytes").await;
        // Same key, different work: a concurrent winner's file must not be
        // adopted.
        std::fs::write(library.join("he2016deep.pdf"), b"other work").unwrap();

        assert!(!ctx.save_and_refile(&mut p).await.unwrap());
        assert_eq!(p.rel_path, "old.pdf", "left in place");
        assert!(library.join("old.pdf").exists());
        assert_eq!(
            std::fs::read(library.join("he2016deep.pdf")).unwrap(),
            b"other work",
            "occupant untouched"
        );
    }

    #[tokio::test]
    async fn apply_match_updates_conflicts_and_keeps_abstract() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let pool = db::connect(&url).await.unwrap();
        let library = dir.path().join("library");
        std::fs::create_dir_all(&library).unwrap();
        let ctx = IngestCtx {
            pool: pool.clone(),
            dirs: Libraries::under(dir.path(), &library),
            resolver: Resolver::with_bases(
                None,
                "http://127.0.0.1:1".to_string(),
                "http://127.0.0.1:1".to_string(),
            )
            .unwrap(),
            grobid: None,
        };

        // Seed a needs_review paper with a GROBID abstract and a real file.
        let mut a = paper("01890000-0000-7000-8000-0000000000a1", "ha", None);
        a.meta.abstract_text = Some("kept abstract".into());
        std::fs::write(library.join("ha.pdf"), b"%PDF-1.4 fake").unwrap();
        db::insert_paper(&pool, &a).await.unwrap();
        // A manual name must survive identify: apply_match replaces the whole
        // metadata block, and `name` lives outside it precisely for this.
        db::set_paper_name(&pool, &a.id, Some("AntiFuzz"))
            .await
            .unwrap();

        // Another paper already owns a DOI (for the conflict case).
        let b = paper(
            "01890000-0000-7000-8000-0000000000b2",
            "hb",
            Some("10.9/owned"),
        );
        db::insert_paper(&pool, &b).await.unwrap();

        // Conflict: applying b's DOI to a -> SameWork(b.id), nothing changed.
        let md_conflict = ResolvedMetadata {
            title: Some("X".into()),
            doi: Some("10.9/owned".into()),
            source: "crossref".into(),
            ..Default::default()
        };
        let out = ctx.apply_match(&mut a.clone(), md_conflict).await.unwrap();
        assert_eq!(out, IdentifyOutcome::SameWork(b.id.clone()));
        let unchanged = db::get_by_id(&pool, &a.id).await.unwrap().unwrap();
        assert_eq!(unchanged.meta.status, PaperStatus::NeedsReview);

        // Apply: DBLP-style metadata without an abstract keeps the old abstract,
        // sets Resolved, recomputes cite key and re-files.
        let md = ResolvedMetadata {
            title: Some("AntiFuzz: Impeding Fuzzing Audits of Binary Executables".into()),
            authors: vec!["Emre Güler".into(), "Thorsten Holz".into()],
            venue: Some("USENIX Security Symposium".into()),
            year: Some(2019),
            dblp_key: Some("conf/uss/GulerAAH19".into()),
            source: "dblp".into(),
            ..Default::default()
        };
        let out = ctx.apply_match(&mut a, md.clone()).await.unwrap();
        assert_eq!(out, IdentifyOutcome::Applied);
        let got = db::get_by_id(&pool, &a.id).await.unwrap().unwrap();
        assert_eq!(got.meta.status, PaperStatus::Resolved);
        assert_eq!(got.meta.abstract_text.as_deref(), Some("kept abstract"));
        assert_eq!(got.cite_key.as_deref(), Some("guler2019antifuzz"));
        assert_eq!(got.rel_path, "guler2019antifuzz.pdf");
        assert_eq!(got.name.as_deref(), Some("AntiFuzz"));
        assert!(library.join("guler2019antifuzz.pdf").exists());

        // Idempotent: re-applying the same match succeeds and changes nothing.
        let out = ctx.apply_match(&mut a, md.clone()).await.unwrap();
        assert_eq!(out, IdentifyOutcome::Applied);
        let again = db::get_by_id(&pool, &a.id).await.unwrap().unwrap();
        assert_eq!(again.cite_key.as_deref(), Some("guler2019antifuzz"));
        assert_eq!(again.rel_path, "guler2019antifuzz.pdf");
        assert!(library.join("guler2019antifuzz.pdf").exists());

        // Trashed: a soft-deleted paper is guarded inside apply_match itself,
        // not just at the CLI/web call sites.
        db::soft_delete(&pool, &a.id).await.unwrap();
        a.deleted_at = Some("x".into());
        let out = ctx.apply_match(&mut a, md).await.unwrap();
        assert_eq!(out, IdentifyOutcome::Trashed);
        let still_trashed = db::get_by_id(&pool, &a.id).await.unwrap().unwrap();
        assert_eq!(still_trashed.cite_key.as_deref(), Some("guler2019antifuzz"));
        assert_eq!(still_trashed.rel_path, "guler2019antifuzz.pdf");
    }
}
