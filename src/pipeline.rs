use anyhow::{anyhow, Result};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::models::{Authors, Identifier, Paper, PaperMeta, PaperStatus};
use crate::naming;
use crate::resolve::grobid::Grobid;
use crate::resolve::{Resolution, ResolvedMetadata, Resolver};
use crate::{db, hash, identify, pdf};

/// Directories the pipeline manages.
pub struct Libraries {
    pub library_root: PathBuf,
    pub processed_dir: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ingested(String), // new paper id
    Duplicate,
}

/// The raw inputs a resolution produces from a stored PDF, shared by ingest and
/// refresh. Consumed by `resolve_fields`.
pub(crate) struct ResolveInputs {
    pub(crate) ident: Identifier,
    pub(crate) provisional_title: Option<String>,
    pub(crate) extracted: Option<ResolvedMetadata>,
    pub(crate) resolution: Resolution,
}

/// Everything the ingest/refresh pipeline needs; built once in `main`.
pub struct IngestCtx {
    pub pool: SqlitePool,
    pub dirs: Libraries,
    pub resolver: Resolver,
    pub grobid: Option<Grobid>,
}

impl IngestCtx {
    /// Ingest a single PDF: hash, dedup, extract, identify, file, and store.
    pub async fn ingest_file(&self, path: &Path) -> Result<Outcome> {
        let path = path.to_path_buf();

        // 1. Hash (blocking IO off the async runtime).
        let content_hash = {
            let p = path.clone();
            tokio::task::spawn_blocking(move || hash::sha256_file(&p)).await??
        };

        // 2. Dedup.
        if db::exists_by_hash(&self.pool, &content_hash).await? {
            move_to(&path, &self.dirs.processed_dir)?;
            return Ok(Outcome::Duplicate);
        }

        // 3. Extract, identify, optionally GROBID, and resolve (factored for reuse).
        let ResolveInputs {
            ident,
            provisional_title,
            extracted,
            resolution,
        } = self.resolve_pdf(&path).await?;

        // 4. Decide the stored fields, then the cite-key filename.
        let fields = resolve_fields(provisional_title, extracted, &ident, resolution);
        let cite_key =
            match naming::cite_key_base(&fields.authors.0, fields.year, fields.title.as_deref()) {
                Some(base) => {
                    let taken = db::cite_keys_with_base(&self.pool, &base, None).await?;
                    Some(naming::disambiguate(&base, &taken))
                }
                None => None,
            };
        let rel_path = naming::library_rel_path(cite_key.as_deref(), &content_hash);

        // 5. File the PDF into the managed library.
        let dest = self.dirs.library_root.join(&rel_path);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&path, &dest)?;

        // 6. Build and store the record.
        let paper = fields.into_paper(content_hash, rel_path, cite_key);
        if let Err(e) = db::insert_paper(&self.pool, &paper).await {
            let _ = std::fs::remove_file(&dest);
            return Err(e);
        }

        // 7. Move the original out of the inbox.
        move_to(&path, &self.dirs.processed_dir)?;
        Ok(Outcome::Ingested(paper.id))
    }

    /// Extract first-page text, identify a DOI/arXiv id, optionally enrich via GROBID
    /// (title-only path), and resolve authoritative metadata. Degrades to
    /// `Resolution::Unresolved` on any resolver/network failure — never aborts.
    pub(crate) async fn resolve_pdf(&self, path: &Path) -> Result<ResolveInputs> {
        // Extract first-page text (blocking IO off the async runtime) and identify.
        let text = {
            let p = path.to_path_buf();
            tokio::task::spawn_blocking(move || pdf::extract_text(&p, 1)).await??
        };
        let ident = identify::identify(&text);
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

        let resolution = self.resolver.resolve(&ident, title_hint.as_deref()).await;
        Ok(ResolveInputs {
            ident,
            provisional_title,
            extracted,
            resolution,
        })
    }
}

/// Decide the stored fields. A confident resolution yields `resolved` (with a
/// GROBID abstract backfilled if the source lacked one); otherwise `needs_review`,
/// enriched with GROBID's title/abstract/authors when present.
pub(crate) fn resolve_fields(
    provisional_title: Option<String>,
    extracted: Option<ResolvedMetadata>,
    ident: &Identifier,
    resolution: Resolution,
) -> PaperMeta {
    let (ext_doi, ext_arxiv) = match ident {
        Identifier::Doi(d) => (Some(d.clone()), None),
        Identifier::Arxiv(a) => (None, Some(a.clone())),
        Identifier::None => (None, None),
    };
    match resolution {
        Resolution::Resolved(md) => {
            let abstract_text = md
                .abstract_text
                .or_else(|| extracted.and_then(|g| g.abstract_text));
            PaperMeta {
                title: md.title.or(provisional_title),
                abstract_text,
                authors: Authors(md.authors),
                venue: md.venue,
                year: md.year,
                doi: md.doi.or(ext_doi),
                arxiv_id: md.arxiv_id.or(ext_arxiv),
                dblp_key: md.dblp_key,
                url: md.url,
                source: Some(md.source),
                status: PaperStatus::Resolved,
            }
        }
        Resolution::Unresolved => match extracted {
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
            added_at: chrono::Utc::now().to_rfc3339(),
            deleted_at: None,
            meta: self,
        }
    }
}

/// Move `src` into `dir`, falling back to copy+remove across filesystems.
pub(crate) fn move_to(src: &Path, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let name = src
        .file_name()
        .ok_or_else(|| anyhow!("path has no file name"))?;
    let dest = dir.join(name);
    if std::fs::rename(src, &dest).is_err() {
        std::fs::copy(src, &dest)?;
        std::fs::remove_file(src)?;
    }
    Ok(())
}

/// Move `from` to the exact path `to` (renaming across directories), creating
/// parent directories, with a copy+remove fallback across filesystems.
pub(crate) fn move_file(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if std::fs::rename(from, to).is_err() {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)?;
    }
    Ok(())
}
