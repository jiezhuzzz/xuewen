use anyhow::Result;

use crate::db;
use crate::models::{Paper, PaperStatus};
use crate::naming;
use crate::pipeline::{copy_to, resolve_fields, IngestCtx, ResolveInputs};

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
pub async fn run(ctx: &IngestCtx, target: RefreshTarget) -> Result<RefreshSummary> {
    let (papers, reresolve_all) = match target {
        RefreshTarget::NeedsReview => (db::all_papers(&ctx.pool).await?, false),
        RefreshTarget::All => (db::all_papers(&ctx.pool).await?, true),
        RefreshTarget::One(id) => {
            let p = db::find_one(&ctx.pool, &id).await?;
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
        let reresolve = reresolve_all || paper.meta.status == PaperStatus::NeedsReview;
        match refresh_one(ctx, &mut paper, reresolve).await {
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

async fn refresh_one(ctx: &IngestCtx, paper: &mut Paper, reresolve: bool) -> Result<OneOutcome> {
    let mut outcome = OneOutcome::default();
    let library_root = &ctx.dirs.library_root;
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
        match ctx.resolve_pdf(&pdf).await {
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
                    && paper.meta.status == PaperStatus::Resolved;
                if would_downgrade {
                    tracing::warn!(
                        "re-resolve of {} came back unresolved; keeping existing resolved metadata",
                        paper.id
                    );
                } else {
                    paper.meta = fields;
                    outcome.reresolved = true;
                }
            }
            Err(e) => tracing::warn!(
                "re-resolve failed for {}: {e}; keeping existing metadata",
                paper.id
            ),
        }
    }

    // Re-file: copy first, persist the row second, remove the old file last —
    // a failure at any step never leaves the DB pointing at a missing file.
    let cite_key = match naming::cite_key_base(
        &paper.meta.authors.0,
        paper.meta.year,
        paper.meta.title.as_deref(),
    ) {
        Some(base) => {
            let taken = db::cite_keys_with_base(&ctx.pool, &base, Some(&paper.id)).await?;
            Some(naming::disambiguate(&base, &taken))
        }
        None => None,
    };
    let new_rel = naming::library_rel_path(cite_key.as_deref(), &paper.content_hash);
    let mut refiled_paths: Option<(std::path::PathBuf, std::path::PathBuf)> = None; // (old, new)
    if new_rel != paper.rel_path {
        let to = library_root.join(&new_rel);
        match copy_to(&pdf, &to) {
            Ok(()) => {
                refiled_paths = Some((pdf.clone(), to));
                paper.rel_path = new_rel;
                paper.cite_key = cite_key;
                outcome.refiled = true;
            }
            Err(e) => {
                tracing::warn!(
                    "re-file copy failed for {}: {e}; leaving in place",
                    paper.id
                )
            }
        }
    }

    if let Err(e) = db::update_paper(&ctx.pool, paper).await {
        // Roll the copy back so filesystem and DB stay consistent.
        if let Some((_, new_path)) = &refiled_paths {
            let _ = std::fs::remove_file(new_path);
        }
        return Err(e);
    }
    if let Some((old_path, _)) = &refiled_paths {
        if let Err(e) = std::fs::remove_file(old_path) {
            tracing::warn!("could not remove old file {}: {e}", old_path.display());
        }
    }
    Ok(outcome)
}
