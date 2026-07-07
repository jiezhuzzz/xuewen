use anyhow::{anyhow, Result};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::models::{Identifier, Paper, PaperStatus};
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

/// Ingest a single PDF: hash, dedup, extract, identify, file, and store.
pub async fn ingest_file(
    pool: &SqlitePool,
    dirs: &Libraries,
    resolver: &Resolver,
    grobid: Option<&Grobid>,
    path: &Path,
) -> Result<Outcome> {
    let path = path.to_path_buf();

    // 1. Hash (blocking IO off the async runtime).
    let content_hash = {
        let p = path.clone();
        tokio::task::spawn_blocking(move || hash::sha256_file(&p)).await??
    };

    // 2. Dedup.
    if db::exists_by_hash(pool, &content_hash).await? {
        move_to(&path, &dirs.processed_dir)?;
        return Ok(Outcome::Duplicate);
    }

    // 3. Extract first-page text and identify.
    let text = {
        let p = path.clone();
        tokio::task::spawn_blocking(move || pdf::extract_text(&p, 1)).await??
    };
    let ident = identify::identify(&text);
    let heuristic_title = identify::guess_title(&text);

    // 3a. For the title-only path, optionally use GROBID to extract a better
    //     title/abstract/authors from the PDF header (degrades to None on failure).
    let extracted: Option<ResolvedMetadata> = match (&ident, grobid) {
        (Identifier::None, Some(g)) => match g.extract_header(&path).await {
            Ok(md) => md,
            Err(e) => {
                tracing::warn!("grobid extraction failed: {e}");
                None
            }
        },
        _ => None,
    };

    // 3b. Search query prefers the GROBID title, else the heuristic first line.
    let title_hint: Option<String> = extracted
        .as_ref()
        .and_then(|m| m.title.clone())
        .or_else(|| heuristic_title.clone());

    // 3c. Resolve authoritative metadata (degrades to Unresolved on failure).
    let resolution = resolver.resolve(&ident, title_hint.as_deref()).await;

    // 4. Decide the stored fields, then the cite-key filename.
    let fields = resolve_fields(heuristic_title, extracted, &ident, resolution);
    let cite_key = match naming::cite_key_base(&fields.authors, fields.year, fields.title.as_deref())
    {
        Some(base) => {
            let taken = db::cite_keys_with_base(pool, &base, None).await?;
            Some(naming::disambiguate(&base, &taken))
        }
        None => None,
    };
    let rel_path = naming::library_rel_path(cite_key.as_deref(), &content_hash);

    // 5. File the PDF into the managed library.
    let dest = dirs.library_root.join(&rel_path);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(&path, &dest)?;

    // 6. Build and store the record.
    let paper = fields.into_paper(content_hash, rel_path, cite_key);
    if let Err(e) = db::insert_paper(pool, &paper).await {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }

    // 6. Move the original out of the inbox.
    move_to(&path, &dirs.processed_dir)?;
    Ok(Outcome::Ingested(paper.id))
}

/// The metadata a paper should store, decided from the resolution outcome and any
/// GROBID extraction. Shared by ingest (and, later, the `refresh` command).
pub struct ResolvedFields {
    pub title: Option<String>,
    pub abstract_text: Option<String>,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
    pub status: String,
}

/// Decide the stored fields. A confident resolution yields `resolved` (with a
/// GROBID abstract backfilled if the source lacked one); otherwise `needs_review`,
/// enriched with GROBID's title/abstract/authors when present.
pub(crate) fn resolve_fields(
    provisional_title: Option<String>,
    extracted: Option<ResolvedMetadata>,
    ident: &Identifier,
    resolution: Resolution,
) -> ResolvedFields {
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
            ResolvedFields {
                title: md.title.or(provisional_title),
                abstract_text,
                authors: md.authors,
                venue: md.venue,
                year: md.year,
                doi: md.doi.or(ext_doi),
                arxiv_id: md.arxiv_id.or(ext_arxiv),
                dblp_key: md.dblp_key,
                url: md.url,
                source: Some(md.source),
                status: PaperStatus::Resolved.as_str().to_string(),
            }
        }
        Resolution::Unresolved => match extracted {
            Some(g) => ResolvedFields {
                title: g.title.or(provisional_title),
                abstract_text: g.abstract_text,
                authors: g.authors,
                venue: None,
                year: None,
                doi: ext_doi,
                arxiv_id: ext_arxiv,
                dblp_key: None,
                url: None,
                source: Some(g.source),
                status: PaperStatus::NeedsReview.as_str().to_string(),
            },
            None => ResolvedFields {
                title: provisional_title,
                abstract_text: None,
                authors: Vec::new(),
                venue: None,
                year: None,
                doi: ext_doi,
                arxiv_id: ext_arxiv,
                dblp_key: None,
                url: None,
                source: None,
                status: PaperStatus::NeedsReview.as_str().to_string(),
            },
        },
    }
}

impl ResolvedFields {
    /// Assemble a full `Paper` with a fresh id/timestamp and the given location.
    pub(crate) fn into_paper(
        self,
        content_hash: String,
        rel_path: String,
        cite_key: Option<String>,
    ) -> Paper {
        let authors = if self.authors.is_empty() {
            None
        } else {
            serde_json::to_string(&self.authors).ok()
        };
        Paper {
            id: Uuid::now_v7().to_string(),
            content_hash,
            rel_path,
            title: self.title,
            abstract_text: self.abstract_text,
            authors,
            venue: self.venue,
            year: self.year,
            doi: self.doi,
            arxiv_id: self.arxiv_id,
            dblp_key: self.dblp_key,
            cite_key,
            url: self.url,
            source: self.source,
            status: self.status,
            added_at: chrono::Utc::now().to_rfc3339(),
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
