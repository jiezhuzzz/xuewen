use anyhow::{anyhow, Result};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::models::{Identifier, Paper, PaperStatus};
use crate::resolve::{Resolution, Resolver};
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
    let title = identify::guess_title(&text);

    // 3b. Resolve authoritative metadata (degrades to Unresolved on failure).
    let resolution = resolver.resolve(&ident).await;

    // 4. File the PDF into the managed library as <hash>.pdf.
    std::fs::create_dir_all(&dirs.library_root)?;
    let rel_path = format!("{content_hash}.pdf");
    let dest = dirs.library_root.join(&rel_path);
    std::fs::copy(&path, &dest)?;

    // 5. Build and store the record.
    let paper = build_paper(content_hash, rel_path, title, &ident, resolution);
    if let Err(e) = db::insert_paper(pool, &paper).await {
        let _ = std::fs::remove_file(&dest);
        return Err(e);
    }

    // 6. Move the original out of the inbox.
    move_to(&path, &dirs.processed_dir)?;
    Ok(Outcome::Ingested(paper.id))
}

/// Assemble a `Paper` from the content hash, relative path, provisional title,
/// extracted identifier, and the resolution outcome. A confident resolution
/// yields `status = resolved` with authoritative fields; otherwise the record
/// stays `needs_review` with whatever the identifier/heuristics provided.
fn build_paper(
    content_hash: String,
    rel_path: String,
    provisional_title: Option<String>,
    ident: &Identifier,
    resolution: Resolution,
) -> Paper {
    let (ext_doi, ext_arxiv) = match ident {
        Identifier::Doi(d) => (Some(d.clone()), None),
        Identifier::Arxiv(a) => (None, Some(a.clone())),
        Identifier::None => (None, None),
    };
    let now = chrono::Utc::now().to_rfc3339();
    let id = Uuid::now_v7().to_string();

    match resolution {
        Resolution::Resolved(md) => {
            let authors = md.authors_json();
            Paper {
                id,
                content_hash,
                rel_path,
                title: md.title.or(provisional_title),
                abstract_text: md.abstract_text,
                authors,
                venue: md.venue,
                year: md.year,
                doi: md.doi.or(ext_doi),
                arxiv_id: md.arxiv_id.or(ext_arxiv),
                dblp_key: md.dblp_key,
                url: md.url,
                source: Some(md.source),
                status: PaperStatus::Resolved.as_str().to_string(),
                added_at: now,
            }
        }
        Resolution::Unresolved => Paper {
            id,
            content_hash,
            rel_path,
            title: provisional_title,
            abstract_text: None,
            authors: None,
            venue: None,
            year: None,
            doi: ext_doi,
            arxiv_id: ext_arxiv,
            dblp_key: None,
            url: None,
            source: None,
            status: PaperStatus::NeedsReview.as_str().to_string(),
            added_at: now,
        },
    }
}

/// Move `src` into `dir`, falling back to copy+remove across filesystems.
fn move_to(src: &Path, dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    let name = src.file_name().ok_or_else(|| anyhow!("path has no file name"))?;
    let dest = dir.join(name);
    if std::fs::rename(src, &dest).is_err() {
        std::fs::copy(src, &dest)?;
        std::fs::remove_file(src)?;
    }
    Ok(())
}
