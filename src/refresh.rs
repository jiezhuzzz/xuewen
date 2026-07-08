use anyhow::Result;
use sqlx::SqlitePool;
use std::path::Path;

use crate::db;
use crate::models::{Paper, PaperStatus};
use crate::naming;
use crate::pipeline::{move_file, resolve_fields, resolve_pdf, ResolveInputs};
use crate::resolve::grobid::Grobid;
use crate::resolve::Resolver;

/// Which papers a refresh pass re-resolves. Every processed paper is re-filed
/// regardless; this only controls whose metadata is re-fetched.
pub enum RefreshTarget {
    /// Default: re-resolve `needs_review` records; re-file all papers.
    NeedsReview,
    /// Re-resolve (and re-file) every paper.
    All,
    /// Re-resolve (and re-file) the single paper with this id (exact or unique prefix).
    One(String),
}

/// Tally of a refresh pass, for CLI feedback.
#[derive(Debug, Default)]
pub struct RefreshSummary {
    pub processed: usize,
    pub reresolved: usize,
    pub refiled: usize,
}

/// Run one refresh pass over the library. Each paper is handled independently:
/// a per-paper failure is logged and never aborts the run.
pub async fn run(
    pool: &SqlitePool,
    library_root: &Path,
    resolver: &Resolver,
    grobid: Option<&Grobid>,
    target: RefreshTarget,
) -> Result<RefreshSummary> {
    let (papers, reresolve_all) = match target {
        RefreshTarget::NeedsReview => (db::all_papers(pool).await?, false),
        RefreshTarget::All => (db::all_papers(pool).await?, true),
        RefreshTarget::One(id) => {
            let p = db::find_one(pool, &id).await?;
            if p.deleted_at.is_some() {
                tracing::warn!("{} is in the trash; skipping refresh", p.id);
                return Ok(RefreshSummary::default());
            }
            (vec![p], true)
        }
    };

    let mut summary = RefreshSummary::default();
    for mut paper in papers {
        summary.processed += 1;
        let reresolve = reresolve_all || paper.status == PaperStatus::NeedsReview;
        match refresh_one(pool, library_root, resolver, grobid, &mut paper, reresolve).await {
            Ok(outcome) => {
                summary.reresolved += outcome.reresolved as usize;
                summary.refiled += outcome.refiled as usize;
            }
            Err(e) => tracing::warn!("refresh failed for {}: {e}", paper.id),
        }
    }
    Ok(summary)
}

#[derive(Default)]
struct OneOutcome {
    reresolved: bool,
    refiled: bool,
}

async fn refresh_one(
    pool: &SqlitePool,
    library_root: &Path,
    resolver: &Resolver,
    grobid: Option<&Grobid>,
    paper: &mut Paper,
    reresolve: bool,
) -> Result<OneOutcome> {
    let mut outcome = OneOutcome::default();
    let pdf = library_root.join(&paper.rel_path);
    if !pdf.exists() {
        tracing::warn!(
            "library file missing for {} ({}); skipping",
            paper.id,
            paper.rel_path
        );
        return Ok(outcome);
    }

    // Re-resolve metadata from the stored PDF (best-effort; keep old data on failure).
    if reresolve {
        match resolve_pdf(&pdf, resolver, grobid).await {
            Ok(inputs) => {
                let ResolveInputs {
                    ident,
                    provisional_title,
                    extracted,
                    resolution,
                } = inputs;
                let fields = resolve_fields(provisional_title, extracted, &ident, resolution);
                // Never downgrade an already-resolved record: if this re-resolution
                // came back unconfident (needs_review) but the paper is already
                // resolved, keep the existing metadata rather than wiping it.
                let would_downgrade = fields.status == PaperStatus::NeedsReview
                    && paper.status == PaperStatus::Resolved;
                if would_downgrade {
                    tracing::warn!(
                        "re-resolve of {} came back unresolved; keeping existing resolved metadata",
                        paper.id
                    );
                } else {
                    fields.apply_to(paper);
                    outcome.reresolved = true;
                }
            }
            Err(e) => tracing::warn!(
                "re-resolve failed for {}: {e}; keeping existing metadata",
                paper.id
            ),
        }
    }

    // Re-file: recompute the cite-key path from the paper's current metadata,
    // excluding this paper's own key from the collision set.
    let cite_key = match naming::cite_key_base(&paper.authors.0, paper.year, paper.title.as_deref())
    {
        Some(base) => {
            let taken = db::cite_keys_with_base(pool, &base, Some(&paper.id)).await?;
            Some(naming::disambiguate(&base, &taken))
        }
        None => None,
    };
    let new_rel = naming::library_rel_path(cite_key.as_deref(), &paper.content_hash);
    if new_rel != paper.rel_path {
        let to = library_root.join(&new_rel);
        match move_file(&pdf, &to) {
            Ok(()) => {
                paper.rel_path = new_rel;
                paper.cite_key = cite_key;
                outcome.refiled = true;
            }
            Err(e) => tracing::warn!(
                "re-file move failed for {}: {e}; leaving in place",
                paper.id
            ),
        }
    }

    db::update_paper(pool, paper).await?;
    Ok(outcome)
}
