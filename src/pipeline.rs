use anyhow::{anyhow, Result};
use sqlx::SqlitePool;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::models::{Authors, Identifier, Paper, PaperMeta, PaperStatus};
use crate::naming;
use crate::resolve::grobid::Grobid;
use crate::resolve::{ResolvedMetadata, Resolver};
use crate::{db, hash, identify, pdf};

/// Directories the pipeline manages.
pub struct Libraries {
    pub library_root: PathBuf,
    pub processed_dir: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Ingested(String), // new paper id
    Duplicate,        // same bytes as an active paper
    SameWork(String), // same DOI/arXiv id as an active paper → its id
    InTrash(String),  // same bytes or identifier as a trashed paper → its id
}

/// The raw inputs a resolution produces from a stored PDF, shared by ingest and
/// refresh. Consumed by `resolve_fields`.
pub(crate) struct ResolveInputs {
    pub(crate) ident: Identifier,
    pub(crate) provisional_title: Option<String>,
    pub(crate) extracted: Option<ResolvedMetadata>,
    pub(crate) resolution: Option<ResolvedMetadata>,
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

        // 2. Dedup by content (active → Duplicate, trashed → InTrash).
        if let Some(existing) = db::find_by_hash(&self.pool, &content_hash).await? {
            move_to_async(&path, &self.dirs.processed_dir).await?;
            return Ok(if existing.deleted_at.is_some() {
                Outcome::InTrash(existing.id)
            } else {
                Outcome::Duplicate
            });
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

        // 4b. A different file of a work we already have (same DOI/arXiv id)?
        if let Some(existing) = db::find_by_identifier(
            &self.pool,
            fields.doi.as_deref(),
            fields.arxiv_id.as_deref(),
        )
        .await?
        {
            move_to_async(&path, &self.dirs.processed_dir).await?;
            return Ok(if existing.deleted_at.is_some() {
                Outcome::InTrash(existing.id)
            } else {
                Outcome::SameWork(existing.id)
            });
        }

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
        copy_to_async(&path, &dest).await?;

        // 6. Build and store the record.
        let paper = fields.into_paper(content_hash, rel_path, cite_key);
        if let Err(e) = db::insert_paper(&self.pool, &paper).await {
            let _ = tokio::fs::remove_file(&dest).await;
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
                    move_to_async(&path, &self.dirs.processed_dir).await?;
                    return Ok(outcome);
                }
            }
            return Err(e);
        }

        // 7. Move the original out of the inbox.
        move_to_async(&path, &self.dirs.processed_dir).await?;
        Ok(Outcome::Ingested(paper.id))
    }

    /// Extract first-page text, identify a DOI/arXiv id, optionally enrich via GROBID
    /// (title-only path), and resolve authoritative metadata. Degrades to
    /// `None` on any resolver/network failure — never aborts.
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
            match copy_to_async(&pdf, &to).await {
                Ok(()) => {
                    refiled_paths = Some((pdf.clone(), to));
                    paper.rel_path = new_rel;
                    paper.cite_key = cite_key;
                }
                Err(e) => {
                    tracing::warn!(
                        "re-file copy failed for {}: {e}; leaving in place",
                        paper.id
                    )
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
        let abstract_text = md
            .abstract_text
            .or_else(|| paper.meta.abstract_text.clone());
        paper.meta = PaperMeta {
            title: md.title,
            abstract_text,
            authors: Authors(md.authors),
            venue: md.venue,
            year: md.year,
            doi: md.doi,
            arxiv_id: md.arxiv_id,
            dblp_key: md.dblp_key,
            url: md.url,
            source: Some(md.source),
            status: PaperStatus::Resolved,
        };
        // No file-existence pre-check: fixing metadata must succeed even if the
        // PDF is missing; save_and_refile degrades to metadata-only in that case.
        self.save_and_refile(paper).await?;
        Ok(IdentifyOutcome::Applied)
    }
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
            Outcome::Duplicate
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

/// Copy `from` to the exact path `to`, creating parent directories.
pub(crate) fn copy_to(from: &Path, to: &Path) -> Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::copy(from, to)?;
    Ok(())
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
            Some(Outcome::Duplicate)
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

    #[tokio::test]
    async fn apply_match_updates_conflicts_and_keeps_abstract() {
        let dir = tempfile::tempdir().unwrap();
        let url = format!("sqlite:{}", dir.path().join("t.db").display());
        let pool = db::connect(&url).await.unwrap();
        let library = dir.path().join("library");
        std::fs::create_dir_all(&library).unwrap();
        let ctx = IngestCtx {
            pool: pool.clone(),
            dirs: Libraries {
                library_root: library.clone(),
                processed_dir: dir.path().join("_processed"),
            },
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
