use anyhow::Context as _;
use axum::extract::multipart::MultipartError;
use axum::extract::{Multipart, Path, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::dto::{
    Candidate, CodeAttachment, DailyPaperDto, DailyResponse, ImportOutcome, PaperDetail,
    PaperNameResponse, PaperSummary, PreviewMeta, ProjectRef, ProxySettings, SearchMatch,
    SearchResponse, SearchResult, SearchStatus, SemanticAvailability, Settings, Stats, TagRef,
    TierCounts, TranslateSettings, TranslationDto,
};
use super::error::ApiError;
use super::AppState;
use crate::annotations::NewAnnotation;
use crate::db;
use crate::export;
use crate::import::{self, ImportError};
use crate::pipeline::{IdentifyOutcome, Outcome};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct ListParams {
    pub q: Option<String>,
    pub status: Option<String>,
    pub sort: Option<String>,
    pub project: Option<String>,
    pub tag: Option<String>,
    pub starred: Option<bool>,
}

impl ListParams {
    /// The shared filter portion (`q`/`sort` travel separately into
    /// `db::list_papers`).
    fn filter(&self) -> db::PaperFilter {
        db::PaperFilter {
            status: self.status.clone(),
            project: self.project.clone(),
            tag: self.tag.clone(),
            starred: self.starred,
        }
    }
}

/// Whether an endpoint accepts a trashed paper. Named at every `fetch_paper`
/// call site so each route's trash policy is a decision, not an accident of
/// which query it happened to use.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Trash {
    /// Trashed papers 404: the UI no longer shows the paper (chat, code).
    Deny,
    /// Trashed papers still resolve: an open reader tab on a just-trashed
    /// paper keeps working, and annotations must survive a restore.
    Allow,
}

/// Look up a paper and apply the caller's trash policy. Doing this before a
/// write makes a bad id a clean 404 rather than an FK-violation 500.
pub(super) async fn fetch_paper(
    pool: &sqlx::SqlitePool,
    id: &str,
    trash: Trash,
    ctx: &'static str,
) -> Result<crate::models::Paper, ApiError> {
    match db::get_by_id(pool, id)
        .await
        .with_context(|| format!("{ctx} paper lookup"))?
    {
        Some(p) if trash == Trash::Allow || p.deleted_at.is_none() => Ok(p),
        _ => Err(ApiError::NotFound),
    }
}

/// Fill each row's `tags`/`projects` from its memberships — two batched
/// queries total, not two per row (`/api/papers` and `/api/search` both run
/// this over every result).
async fn attach_row_extras(
    pool: &sqlx::SqlitePool,
    rows: &mut [PaperSummary],
) -> anyhow::Result<()> {
    let ids: Vec<String> = rows.iter().map(|r| r.id.clone()).collect();
    let mut tags = db::tags_for_papers(pool, &ids).await?;
    let mut projects = db::projects_for_papers(pool, &ids).await?;
    for r in rows.iter_mut() {
        r.tags = tags
            .remove(&r.id)
            .unwrap_or_default()
            .into_iter()
            .map(TagRef::from)
            .collect();
        r.projects = projects
            .remove(&r.id)
            .unwrap_or_default()
            .into_iter()
            .map(ProjectRef::from)
            .collect();
    }
    Ok(())
}

/// A single paper's tag and project memberships, as wire refs.
async fn paper_refs(
    pool: &sqlx::SqlitePool,
    paper_id: &str,
) -> anyhow::Result<(Vec<TagRef>, Vec<ProjectRef>)> {
    let ids = [paper_id.to_string()];
    let mut tags = db::tags_for_papers(pool, &ids).await?;
    let mut projects = db::projects_for_papers(pool, &ids).await?;
    Ok((
        tags.remove(paper_id)
            .unwrap_or_default()
            .into_iter()
            .map(TagRef::from)
            .collect(),
        projects
            .remove(paper_id)
            .unwrap_or_default()
            .into_iter()
            .map(ProjectRef::from)
            .collect(),
    ))
}

pub async fn list_papers(
    State(app): State<AppState>,
    Query(p): Query<ListParams>,
) -> Result<Response, ApiError> {
    let papers = db::list_papers(&app.pool, p.q.as_deref(), p.sort.as_deref(), &p.filter())
        .await
        .context("list_papers")?;
    let mut out: Vec<PaperSummary> = papers.iter().map(PaperSummary::from).collect();
    attach_row_extras(&app.pool, &mut out)
        .await
        .context("list_papers row extras")?;
    Ok(Json(out).into_response())
}

pub async fn get_paper(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let p = fetch_paper(&app.pool, &id, Trash::Allow, "get_paper").await?;
    let (tags, projects) = paper_refs(&app.pool, &p.id)
        .await
        .context("get_paper refs")?;
    let mut detail = PaperDetail::from(&p).attach(tags, projects);
    detail.ai_summary = match crate::summary::store::get(&app.pool, &p.id).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("get_paper summary for {}: {e}", p.id);
            None
        }
    };
    Ok(Json(detail).into_response())
}

/// Soft-delete a paper (web mutation): flag it deleted; the file is untouched.
/// Purge (permanent removal) is CLI-only. Idempotent on an already-trashed paper.
pub async fn delete_paper(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    fetch_paper(&app.pool, &id, Trash::Allow, "delete_paper").await?;
    db::soft_delete(&app.pool, &id)
        .await
        .context("delete_paper")?;
    app.wake_search();
    Ok(Json(serde_json::json!({ "deleted": true })).into_response())
}

/// Un-trash a paper (the Undo behind the web UI's delete toast). 404 when the
/// paper doesn't exist or isn't trashed — `db::restore` only flips trashed rows.
pub async fn restore_paper(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if !db::restore(&app.pool, &id).await.context("restore_paper")? {
        return Err(ApiError::NotFound);
    }
    app.wake_search();
    Ok(Json(serde_json::json!({ "restored": true })).into_response())
}

/// Import a single uploaded PDF: validate, stage into `inbox_dir/_uploads`, and
/// run the ingest pipeline. One PDF per request (the frontend uploads files one
/// at a time). Returns `ingested` (with title/status), `duplicate`, or an error.
pub async fn import_paper(
    State(app): State<AppState>,
    mut multipart: Multipart,
) -> Result<Response, ApiError> {
    let ingest = app
        .ingest
        .clone()
        .ok_or(ApiError::Unavailable("import not configured"))?;

    // Take the first file part; skip any non-file fields.
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => return Err(ApiError::BadRequest("no file".into())),
            Err(e) => return Ok(multipart_error(e)),
        };
        let Some(filename) = field.file_name().map(|s| s.to_string()) else {
            continue;
        };
        let data = match field.bytes().await {
            Ok(b) => b,
            Err(e) => return Ok(multipart_error(e)),
        };
        if !data.starts_with(b"%PDF") {
            return Err(ApiError::BadRequest("not a PDF".into()));
        }

        let resp = ingest_response(
            &ingest.ctx.pool,
            ingest
                .ctx
                .ingest_bytes(data.as_ref(), &filename, None)
                .await,
        )
        .await;
        app.wake_search();
        return Ok(resp);
    }
}

/// Map an ingest result to the shared `ImportOutcome` JSON (`Ingested`
/// enriched with title/status), or a failed ingest to the "import failed" 500
/// — not the generic "internal error": "import failed" is the body the
/// frontend has always surfaced. Shared by file upload and URL import.
async fn ingest_response(pool: &sqlx::SqlitePool, result: anyhow::Result<Outcome>) -> Response {
    match result {
        Ok(Outcome::Ingested(id)) => {
            let (title, status) = match db::get_by_id(pool, &id).await {
                Ok(Some(p)) => (p.meta.title, p.meta.status),
                _ => (None, crate::models::PaperStatus::NeedsReview),
            };
            Json(ImportOutcome::Ingested { id, title, status }).into_response()
        }
        Ok(Outcome::Duplicate(id)) => Json(ImportOutcome::Duplicate { id }).into_response(),
        Ok(Outcome::SameWork(id)) => Json(ImportOutcome::SameWork { id }).into_response(),
        Ok(Outcome::InTrash(id)) => Json(ImportOutcome::InTrash { id }).into_response(),
        Err(e) => {
            tracing::error!("import ingest: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "import failed"})),
            )
                .into_response()
        }
    }
}

pub async fn stats(State(app): State<AppState>) -> Result<Response, ApiError> {
    let (total, resolved, needs_review) = db::stats(&app.pool).await.context("stats")?;
    Ok(Json(Stats {
        total: total as usize,
        resolved: resolved as usize,
        needs_review: needs_review as usize,
    })
    .into_response())
}

/// Stream a paper's PDF from the library. Range-aware (via `ServeFile`) and
/// path-safe: the resolved file must live under `library_root`.
pub async fn pdf(
    State(app): State<AppState>,
    Path(id): Path<String>,
    req: Request,
) -> Result<Response, ApiError> {
    let paper = fetch_paper(&app.pool, &id, Trash::Allow, "pdf").await?;
    let path = library_file(&app.library_root, &paper.rel_path).await?;
    let resp = ServeFile::new(&path)
        .oneshot(req)
        .await
        .context("serve pdf")?;
    Ok(resp.map(axum::body::Body::new).into_response())
}

/// Resolve a paper's file under the library root. Defense in depth, shared by
/// every route that reads library bytes: the canonical file must live under
/// the root, and a missing one answers the same 404 a bad id would — the
/// client learns "no file here" either way.
async fn library_file(
    library_root: &std::path::Path,
    rel_path: &str,
) -> Result<std::path::PathBuf, ApiError> {
    let path = library_root.join(rel_path);
    let under_root = {
        let (p, root) = (path.clone(), library_root.to_path_buf());
        tokio::task::spawn_blocking(move || {
            match (std::fs::canonicalize(&p), std::fs::canonicalize(&root)) {
                (Ok(file), Ok(root)) => file.starts_with(&root),
                _ => false, // missing file or unresolvable path
            }
        })
        .await
        .inspect_err(|e| tracing::error!("canonicalize check panicked: {e}"))
        .unwrap_or(false)
    };
    if under_root {
        Ok(path)
    } else {
        Err(ApiError::NotFound)
    }
}

/// Map a render failure onto the wire. `Unrenderable` is 422 rather than 500
/// because nothing is broken — this PDF simply has no image to show, and it
/// is the one signal the picker needs to switch to its text card.
fn preview_error(e: crate::preview::PreviewError) -> ApiError {
    match e {
        crate::preview::PreviewError::Unrenderable => {
            ApiError::Unprocessable("the PDF could not be rendered".into())
        }
        crate::preview::PreviewError::PageOutOfRange => ApiError::NotFound,
    }
}

/// How many pages the picker should lay out, and page one's shape.
pub async fn preview_meta(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<PreviewMeta>, ApiError> {
    // Trash::Allow, like `pdf`: a paper trashed while the picker is open is
    // still a paper the picker is showing.
    let paper = fetch_paper(&app.pool, &id, Trash::Allow, "preview").await?;
    let path = library_file(&app.library_root, &paper.rel_path).await?;
    let meta = app
        .preview
        .meta(&path, &paper.content_hash)
        .await
        .map_err(preview_error)?;
    Ok(Json(PreviewMeta {
        pages: meta.pages,
        page_width: meta.width,
        page_height: meta.height,
    }))
}

/// One rendered page as a PNG.
pub async fn preview_page(
    State(app): State<AppState>,
    // The page number is parsed here rather than by a typed extractor: a
    // typed one rejects with axum's own body shape, and every error this API
    // returns is `{"error": "..."}`.
    Path((id, page)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    let page: u32 = page
        .parse()
        .map_err(|_| ApiError::BadRequest("page must be a number".into()))?;
    let paper = fetch_paper(&app.pool, &id, Trash::Allow, "preview").await?;
    let path = library_file(&app.library_root, &paper.rel_path).await?;
    let png = app
        .preview
        .page_png(&path, &paper.content_hash, page)
        .await
        .map_err(preview_error)?;
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "image/png"),
            // The bytes only change when the file's hash does, but the URL is
            // keyed by paper id, so this is a bounded cache rather than an
            // immutable one.
            (axum::http::header::CACHE_CONTROL, "private, max-age=3600"),
        ],
        png,
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct IdentifyQuery {
    pub q: Option<String>,
}

/// Ungated candidate search for manual identify (the user is the gate).
pub async fn identify_search(
    State(app): State<AppState>,
    Query(p): Query<IdentifyQuery>,
) -> Result<Response, ApiError> {
    let ingest = app
        .ingest
        .as_ref()
        .ok_or(ApiError::Unavailable("identify not configured"))?;
    let q =
        p.q.as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| ApiError::BadRequest("missing query".into()))?;
    let cands = ingest.ctx.resolver.search_candidates(q).await;
    let out: Vec<Candidate> = cands.iter().map(Candidate::from).collect();
    Ok(Json(out).into_response())
}

#[derive(Deserialize)]
pub struct IdentifyBody {
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub candidate: Option<Candidate>,
}

/// Apply a user-confirmed match: fetch authoritative metadata for a DOI or
/// arXiv id (or take a picked search candidate as-is), overwrite the paper's
/// metadata, and re-file. The user's confirmation replaces the confidence gate.
pub async fn identify_paper(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<IdentifyBody>,
) -> Result<Response, ApiError> {
    let ingest = app
        .ingest
        .as_ref()
        .ok_or(ApiError::Unavailable("identify not configured"))?;
    let selectors =
        body.doi.is_some() as u8 + body.arxiv_id.is_some() as u8 + body.candidate.is_some() as u8;
    if selectors != 1 {
        return Err(ApiError::BadRequest(
            "provide exactly one of doi, arxiv_id, candidate".into(),
        ));
    }

    // Read through the ctx pool: same handle the apply path writes with
    // (matches the pool-locality convention set in import_paper). Trash is
    // Allow here only because `apply_match` itself answers a trashed paper
    // with the 409 below — the pipeline owns that policy.
    let mut paper = fetch_paper(&ingest.ctx.pool, &id, Trash::Allow, "identify").await?;
    let md = if let Some(c) = body.candidate {
        Some(c.into_metadata())
    } else if let Some(doi) = &body.doi {
        ingest
            .ctx
            .resolver
            .resolve(&crate::models::Identifier::doi(doi.clone()), None)
            .await
    } else if let Some(axv) = &body.arxiv_id {
        ingest
            .ctx
            .resolver
            .resolve(&crate::models::Identifier::arxiv(axv.clone()), None)
            .await
    } else {
        unreachable!("selector count checked above")
    };
    let Some(md) = md else {
        return Err(ApiError::NotFoundMsg(
            "identifier not found — not registered with Crossref/arXiv; try a title search".into(),
        ));
    };

    let (md_doi, md_arxiv) = (md.doi.clone(), md.arxiv_id.clone());
    match ingest.ctx.apply_match(&mut paper, md).await {
        Ok(IdentifyOutcome::Applied) => {
            app.wake_search();
            // The identify has already committed and the indexer been woken;
            // failing the response over chip enrichment would misreport a
            // completed identify as a 500 (and invite a client retry), so a
            // refs error degrades to empty tags/projects — unlike get_paper,
            // which propagates.
            let (tags, projects) = match paper_refs(&ingest.ctx.pool, &paper.id).await {
                Ok(refs) => refs,
                Err(e) => {
                    tracing::warn!("identify refs for {}: {e}", paper.id);
                    (Vec::new(), Vec::new())
                }
            };
            Ok(Json(PaperDetail::from(&paper).attach(tags, projects)).into_response())
        }
        Ok(IdentifyOutcome::SameWork(other)) => Err(ApiError::Conflict {
            message: format!("same work as {other}"),
            id: Some(other),
        }),
        Ok(IdentifyOutcome::Trashed) => Err(ApiError::Conflict {
            message: "paper is in the trash".into(),
            id: None,
        }),
        Err(e) => {
            // Lost a race: something claimed this identifier between the guard and
            // the update. Report it as the conflict it is, mirroring ingest.
            if db::is_unique_violation(&e) {
                if let Ok(Some(existing)) =
                    db::find_by_identifier(&ingest.ctx.pool, md_doi.as_deref(), md_arxiv.as_deref())
                        .await
                {
                    return Err(ApiError::Conflict {
                        message: format!("same work as {}", existing.id),
                        id: Some(existing.id),
                    });
                }
            }
            Err(ApiError::Internal(e.context("identify apply")))
        }
    }
}

#[derive(Deserialize)]
pub struct ImportUrlBody {
    pub input: String,
}

/// Import from a URL/DOI/arXiv id: fetch the PDF (arXiv/proxy/OA), then ingest.
pub async fn import_url(
    State(app): State<AppState>,
    Json(body): Json<ImportUrlBody>,
) -> Result<Response, ApiError> {
    let ingest = app
        .ingest
        .clone()
        .ok_or(ApiError::Unavailable("import not configured"))?;
    let cookie = db::get_setting(&ingest.ctx.pool, "proxy_cookie")
        .await
        .context("import proxy cookie")?;
    let outcome = match import::fetch_stage_ingest(
        &ingest.ctx,
        &ingest.fetcher,
        &body.input,
        cookie.as_deref(),
    )
    .await
    {
        Ok(outcome) => Ok(outcome),
        Err(ImportError::Ingest(e)) => Err(e),
        Err(ImportError::Unsupported) => {
            return Err(ApiError::BadRequest("unsupported input".into()))
        }
        Err(ImportError::CookieExpired) => {
            return Err(ApiError::BadGateway(
                "proxy session expired — refresh your cookie",
            ))
        }
        Err(ImportError::Unfetched { metadata }) => {
            let (title, doi) = match metadata {
                Some(m) => (m.title, m.doi),
                None => (None, None),
            };
            return Ok(Json(ImportOutcome::Unfetched { title, doi }).into_response());
        }
        Err(ImportError::Network(e)) => {
            tracing::error!("import fetch: {e}");
            return Err(ApiError::BadGateway("fetch failed"));
        }
    };
    let resp = ingest_response(&ingest.ctx.pool, outcome).await;
    app.wake_search();
    Ok(resp)
}

#[derive(Deserialize)]
pub struct ProxyCookieBody {
    pub cookie: String,
}

/// Report whether a proxy cookie is stored (never the value itself).
pub async fn get_settings(State(app): State<AppState>) -> Result<Response, ApiError> {
    let set = db::get_setting(&app.pool, "proxy_cookie")
        .await
        .context("settings proxy cookie")?
        .is_some();
    let updated = db::setting_updated_at(&app.pool, "proxy_cookie")
        .await
        .context("settings cookie timestamp")?;
    let translate = match app.translate.as_ref() {
        None => TranslateSettings::disabled(),
        Some(svc) => TranslateSettings::from(svc.as_ref()),
    };
    let proxy = app
        .ingest
        .as_ref()
        .and_then(|i| i.fetcher.proxy_host())
        .map(|h| ProxySettings {
            host: h.to_string(),
        });
    Ok(Json(Settings {
        proxy,
        proxy_cookie_set: set,
        proxy_cookie_updated_at: updated,
        fold_abstract: app.ui.fold_abstract,
        translate,
    })
    .into_response())
}

#[derive(Deserialize)]
pub struct TranslateBody {
    pub text: String,
    #[serde(default)]
    pub target_lang: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
}

/// Translate arbitrary text via the configured provider. 503 when
/// translate-on-selection isn't configured; 400 on empty text or an unknown
/// provider name; 502 when the upstream provider call fails.
pub async fn translate(
    State(app): State<AppState>,
    Json(body): Json<TranslateBody>,
) -> Result<Response, ApiError> {
    let svc = app
        .translate
        .as_ref()
        .ok_or(ApiError::Unavailable("translate is not configured"))?;
    let text = body.text.trim();
    if text.is_empty() {
        return Err(ApiError::BadRequest("empty text".into()));
    }
    let provider = match body.provider.as_deref() {
        None => None,
        Some("llm") => Some(crate::config::TranslateProvider::Llm),
        Some("deepl") => Some(crate::config::TranslateProvider::Deepl),
        Some(other) => return Err(ApiError::BadRequest(format!("unknown provider `{other}`"))),
    };
    match svc
        .translate(text, body.target_lang.as_deref(), provider)
        .await
    {
        Ok(t) => Ok(Json(TranslationDto::from(t)).into_response()),
        Err(e) => {
            tracing::error!("translate: {e}");
            Err(ApiError::BadGateway("translation failed"))
        }
    }
}

/// Store (overwrite) the EZproxy cookie.
pub async fn set_proxy_cookie(
    State(app): State<AppState>,
    Json(body): Json<ProxyCookieBody>,
) -> Result<Response, ApiError> {
    let cookie = body.cookie.trim();
    if cookie.is_empty() {
        return Err(ApiError::BadRequest("empty cookie".into()));
    }
    db::set_setting(&app.pool, "proxy_cookie", cookie)
        .await
        .context("set proxy cookie")?;
    Ok(Json(serde_json::json!({"ok": true})).into_response())
}

/// Clear the stored EZproxy cookie.
pub async fn clear_proxy_cookie(State(app): State<AppState>) -> Result<Response, ApiError> {
    db::delete_setting(&app.pool, "proxy_cookie")
        .await
        .context("clear proxy cookie")?;
    Ok(Json(serde_json::json!({"ok": true})).into_response())
}

#[derive(Deserialize)]
pub struct CreateProjectBody {
    pub name: String,
}

#[derive(Deserialize)]
pub struct UpdateProjectBody {
    pub name: Option<String>,
}

pub async fn list_projects(State(app): State<AppState>) -> Result<Response, ApiError> {
    let list = db::list_projects(&app.pool)
        .await
        .context("list_projects")?;
    Ok(Json(list).into_response())
}

pub async fn create_project(
    State(app): State<AppState>,
    Json(body): Json<CreateProjectBody>,
) -> Result<Response, ApiError> {
    let name = body.name.trim();
    if name.is_empty() {
        return Err(ApiError::BadRequest("empty name".into()));
    }
    match db::create_project(&app.pool, name).await {
        Ok(project) => Ok((StatusCode::CREATED, Json(project)).into_response()),
        Err(e) => Err(ApiError::from_db_conflict(
            e,
            "create_project",
            "a project with that name already exists",
        )),
    }
}

pub async fn update_project(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateProjectBody>,
) -> Result<Response, ApiError> {
    // Merge: an omitted/blank name keeps the old one. Blankness is decided
    // here with `str::trim` (all Unicode whitespace), not SQL `TRIM` (spaces
    // only) — a tab-only name must keep the old name, not be stored.
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match db::update_project(&app.pool, &id, name).await {
        Ok(Some(p)) => Ok(Json(p).into_response()),
        Ok(None) => Err(ApiError::NotFound),
        Err(e) => Err(ApiError::from_db_conflict(
            e,
            "update_project",
            "a project with that name already exists",
        )),
    }
}

pub async fn delete_project(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if !db::delete_project(&app.pool, &id)
        .await
        .context("delete_project")?
    {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn add_paper_to_project(
    State(app): State<AppState>,
    Path((paper_id, project_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    // Pre-check both ids so a bad request is a clean 404 (not an FK error).
    fetch_paper(&app.pool, &paper_id, Trash::Allow, "membership").await?;
    db::get_project(&app.pool, &project_id)
        .await
        .context("membership project lookup")?
        .ok_or(ApiError::NotFound)?;
    db::add_paper_to_project(&app.pool, &paper_id, &project_id)
        .await
        .context("add membership")?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn remove_paper_from_project(
    State(app): State<AppState>,
    Path((paper_id, project_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if !db::remove_paper_from_project(&app.pool, &paper_id, &project_id)
        .await
        .context("remove membership")?
    {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn list_tags(State(app): State<AppState>) -> Result<Response, ApiError> {
    let tags = db::list_tags_with_counts(&app.pool)
        .await
        .context("list_tags")?;
    Ok(Json(tags).into_response())
}

#[derive(Deserialize)]
pub struct TagNameBody {
    pub name: String,
}

pub async fn add_paper_tag(
    State(app): State<AppState>,
    Path(paper_id): Path<String>,
    Json(body): Json<TagNameBody>,
) -> Result<Response, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::BadRequest("empty name".into()));
    }
    // Pre-check the paper so a bad id is a clean 404, not an FK-violation 500.
    fetch_paper(&app.pool, &paper_id, Trash::Allow, "add_paper_tag").await?;
    let t = db::add_paper_tag(&app.pool, &paper_id, &body.name)
        .await
        .context("add_paper_tag")?;
    Ok(Json(TagRef::from(t)).into_response())
}

pub async fn remove_paper_tag(
    State(app): State<AppState>,
    Path((paper_id, tag_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if !db::remove_paper_tag(&app.pool, &paper_id, &tag_id)
        .await
        .context("remove_paper_tag")?
    {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn rename_tag(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TagNameBody>,
) -> Result<Response, ApiError> {
    if body.name.trim().is_empty() {
        return Err(ApiError::BadRequest("empty name".into()));
    }
    match db::rename_tag(&app.pool, &id, &body.name).await {
        Ok(Some(t)) => Ok(Json(TagRef::from(t)).into_response()),
        Ok(None) => Err(ApiError::NotFound),
        Err(e) => Err(ApiError::from_db_conflict(
            e,
            "rename_tag",
            "a tag with that name already exists",
        )),
    }
}

pub async fn delete_tag(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    if !db::delete_tag(&app.pool, &id).await.context("delete_tag")? {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

pub async fn star_paper(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    set_star(&app, &id, true).await
}

pub async fn unstar_paper(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    set_star(&app, &id, false).await
}

async fn set_star(app: &AppState, id: &str, on: bool) -> Result<Response, ApiError> {
    if !db::set_paper_starred(&app.pool, id, on)
        .await
        .context("set_star")?
    {
        return Err(ApiError::NotFound);
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[derive(Deserialize)]
pub struct SetPaperNameBody {
    /// Missing key, null, and whitespace-only all mean "clear".
    pub name: Option<String>,
}

/// Longest accepted manual name — a short identifier like "RVSpec", not prose.
const NAME_MAX_CHARS: usize = 200;

pub async fn set_paper_name(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<SetPaperNameBody>,
) -> Result<Response, ApiError> {
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    if let Some(n) = name {
        if n.chars().count() > NAME_MAX_CHARS {
            return Err(ApiError::BadRequest(
                "name is too long (max 200 characters)".into(),
            ));
        }
        // The name renders in single-line chips/cells; an embedded newline or
        // other control character would corrupt every one of them.
        if n.chars().any(char::is_control) {
            return Err(ApiError::BadRequest(
                "name must not contain control characters".into(),
            ));
        }
    }
    if !db::set_paper_name(&app.pool, &id, name)
        .await
        .context("set_paper_name")?
    {
        return Err(ApiError::NotFound);
    }
    Ok(Json(PaperNameResponse {
        name: name.map(str::to_string),
    })
    .into_response())
}

#[derive(Deserialize)]
pub struct FormatParam {
    pub format: Option<String>,
}

fn parse_format(s: Option<&str>) -> export::BibFormat {
    match s {
        Some(v) if v.eq_ignore_ascii_case("biblatex") => export::BibFormat::Biblatex,
        _ => export::BibFormat::Bibtex,
    }
}

/// One paper's `.bib` entry as plain text (inline, so the frontend can copy it
/// or force a download via `<a download>`).
pub async fn export_paper(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Query(p): Query<FormatParam>,
) -> Result<Response, ApiError> {
    let paper = fetch_paper(&app.pool, &id, Trash::Allow, "export_paper").await?;
    let body = export::format_entry(&paper, parse_format(p.format.as_deref()));
    Ok((
        [(
            axum::http::header::CONTENT_TYPE,
            "text/plain; charset=utf-8",
        )],
        body,
    )
        .into_response())
}

/// The current filtered set as a downloadable `.bib` file. Same filter
/// semantics as `GET /api/papers` by construction: the query string is
/// extracted twice (`Query` reads request parts, so this composes) — once as
/// the same `ListParams` the list endpoint uses, once for `format`.
pub async fn export_papers(
    State(app): State<AppState>,
    Query(f): Query<FormatParam>,
    Query(p): Query<ListParams>,
) -> Result<Response, ApiError> {
    let papers = db::list_papers(&app.pool, p.q.as_deref(), p.sort.as_deref(), &p.filter())
        .await
        .context("export_papers")?;
    let body = export::format_entries(&papers, parse_format(f.format.as_deref()));
    Ok((
        [
            (axum::http::header::CONTENT_TYPE, "application/x-bibtex"),
            (
                axum::http::header::CONTENT_DISPOSITION,
                "attachment; filename=\"xuewen.bib\"",
            ),
        ],
        body,
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub fields: Option<String>,
    pub engines: Option<String>,
    pub status: Option<String>,
    pub project: Option<String>,
    pub tag: Option<String>,
    pub starred: Option<bool>,
}

/// Hybrid search. `fields`/`engines` are CSV lists; absent or unknown-only
/// values fall back to "all" (mirrors the whitelisting style elsewhere).
pub async fn search_papers(
    State(app): State<AppState>,
    Query(p): Query<SearchParams>,
) -> Result<Response, ApiError> {
    let svc = app
        .search
        .as_ref()
        .ok_or(ApiError::Unavailable("search not configured"))?;
    let (keyword, semantic) = parse_engines(p.engines.as_deref());
    let req = crate::search::SearchRequest::assemble(
        &app.pool,
        p.q.as_deref().unwrap_or(""),
        crate::search::RequestOverrides {
            keyword,
            semantic,
            fields: p.fields,
            status: p.status,
            project: p.project,
            tag: p.tag,
            starred: p.starred,
        },
    )
    .await
    .context("search request")?;
    let out = svc.search(&req).await.context("search")?;
    // Same enrichment as list_papers: without it, search rows serialize
    // empty tags/projects and the table's chips vanish whenever a query is
    // active.
    let mut summaries: Vec<PaperSummary> = out
        .results
        .iter()
        .map(|(paper, _)| PaperSummary::from(paper))
        .collect();
    attach_row_extras(&app.pool, &mut summaries)
        .await
        .context("search row extras")?;
    let results: Vec<SearchResult> = summaries
        .into_iter()
        .zip(out.results.iter())
        .map(|(paper, (_, m))| SearchResult {
            paper,
            match_info: SearchMatch {
                engine: m.engine.as_str().to_string(),
                field: m.field.as_str().to_string(),
                snippet: m.snippet.clone(),
                page: m.page,
            },
        })
        .collect();
    Ok(Json(SearchResponse {
        semantic: SemanticAvailability {
            available: out.semantic.available,
            reason: out.semantic.reason,
        },
        results,
    })
    .into_response())
}

fn parse_engines(csv: Option<&str>) -> (bool, bool) {
    let (mut keyword, mut semantic) = (false, false);
    for part in csv.unwrap_or("").split(',').map(str::trim) {
        match part {
            "keyword" => keyword = true,
            "semantic" => semantic = true,
            _ => {}
        }
    }
    if keyword || semantic {
        (keyword, semantic)
    } else {
        (true, true) // absent/unknown-only -> both
    }
}

pub async fn search_status(State(app): State<AppState>) -> Result<Response, ApiError> {
    let svc = app
        .search
        .as_ref()
        .ok_or(ApiError::Unavailable("search not configured"))?;
    let st = svc.status().await.context("search status")?;
    Ok(Json(SearchStatus {
        fts: TierCounts {
            indexed: st.fts.indexed,
            pending: st.fts.pending,
            failed: st.fts.failed,
        },
        vectors: TierCounts {
            indexed: st.vectors.indexed,
            pending: st.vectors.pending,
            failed: st.vectors.failed,
        },
        semantic_available: st.semantic_available,
        reason: st.reason,
    })
    .into_response())
}

/// Map a multipart read error to its proper status (e.g. 413 when the body
/// exceeds the limit) with a JSON body.
fn multipart_error(e: MultipartError) -> Response {
    let status = e.status();
    (
        status,
        Json(serde_json::json!({
            "error": status.canonical_reason().unwrap_or("upload error").to_lowercase()
        })),
    )
        .into_response()
}

/// GET /api/daily — the latest non-empty daily batch for the Glance widget.
pub async fn daily_papers(State(app): State<AppState>) -> Result<Response, ApiError> {
    if app.daily.is_none() {
        return Err(ApiError::Unavailable("daily papers not configured"));
    }
    let resp = match crate::daily::store::latest_batch(&app.pool)
        .await
        .context("daily papers")?
    {
        Some((date, papers)) => DailyResponse {
            date: Some(date),
            papers: papers.iter().map(DailyPaperDto::from).collect(),
        },
        None => DailyResponse {
            date: None,
            papers: Vec::new(),
        },
    };
    Ok(Json(resp).into_response())
}

/// POST /api/daily/run — manual trigger; 202 started, 409 already running.
pub async fn run_daily(State(app): State<AppState>) -> Result<Response, ApiError> {
    let svc = app
        .daily
        .as_ref()
        .ok_or(ApiError::Unavailable("daily papers not configured"))?;
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    if !svc.spawn_run(today) {
        return Err(ApiError::Conflict {
            message: "a daily run is already in flight".into(),
            id: None,
        });
    }
    Ok((
        StatusCode::ACCEPTED,
        Json(serde_json::json!({"status": "started"})),
    )
        .into_response())
}

#[derive(Deserialize)]
pub struct ParseCitationsBody {
    pub references: Vec<String>,
}

/// POST /api/papers/{id}/citations — parse extracted reference strings into
/// structured fields: heuristics first (always available), then the
/// [ai.citations] LLM for entries heuristics couldn't parse (cached per
/// paper).
pub async fn parse_citations(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ParseCitationsBody>,
) -> Result<Response, ApiError> {
    let total: usize = body.references.iter().map(|r| r.len()).sum();
    if body.references.is_empty() || body.references.len() > 500 || total > 200_000 {
        return Err(ApiError::BadRequest(
            "references must be 1..=500 entries and under 200kB".into(),
        ));
    }
    // The paper's venue seeds the style vote's tie-breaker.
    let venue = fetch_paper(&app.pool, &id, Trash::Allow, "parse_citations")
        .await?
        .meta
        .venue;
    let parsed = app
        .citations
        .parse(&id, &body.references, venue.as_deref())
        .await
        .with_context(|| format!("parse_citations {id}"))?;
    Ok(Json(serde_json::json!({ "references": parsed })).into_response())
}

#[derive(serde::Deserialize)]
pub struct CodeBody {
    pub repo_url: String,
}

/// Attach (or replace) a paper's code repo: 202 + row while the shallow
/// clone runs in the background; poll GET for the outcome.
pub async fn set_paper_code(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<CodeBody>,
) -> Result<Response, ApiError> {
    let agent = app
        .agent
        .clone()
        .ok_or(ApiError::Unavailable("agent ask is not configured"))?;
    let paper = fetch_paper(&app.pool, &id, Trash::Deny, "code").await?;
    crate::agent::code::validate_repo_url(&body.repo_url, &agent.clone_allowed_hosts)
        .map_err(ApiError::Unprocessable)?;
    let clone_gen =
        crate::agent::store::upsert_paper_code_cloning(&app.pool, &paper.id, body.repo_url.trim())
            .await
            .context("paper_code upsert")?;
    crate::agent::code::spawn_clone(
        app.pool.clone(),
        app.library_root.clone(),
        paper.id.clone(),
        body.repo_url.trim().to_string(),
        agent.max_repo_mb,
        clone_gen,
    );
    let code = crate::agent::store::get_paper_code(&app.pool, &paper.id)
        .await
        .context("paper_code read-back")?;
    Ok((
        StatusCode::ACCEPTED,
        Json(CodeAttachment {
            attached: true,
            code,
        }),
    )
        .into_response())
}

pub async fn get_paper_code(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let p = fetch_paper(&app.pool, &id, Trash::Deny, "code").await?;
    let code = crate::agent::store::get_paper_code(&app.pool, &p.id)
        .await
        .context("paper_code get")?;
    Ok(Json(CodeAttachment {
        attached: code.is_some(),
        code,
    })
    .into_response())
}

pub async fn delete_paper_code(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let p = fetch_paper(&app.pool, &id, Trash::Deny, "code").await?;
    crate::agent::code::remove_checkout(&app.library_root, &p.id).await;
    crate::agent::store::delete_paper_code(&app.pool, &p.id)
        .await
        .context("paper_code delete")?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// GET /api/papers/{id}/annotations — every mark on a paper, in reading order.
/// The reader replays these into the annotation plugin on open.
pub async fn list_annotations(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    fetch_paper(&app.pool, &id, Trash::Allow, "list_annotations").await?;
    let items = app
        .annotations
        .list(&id)
        .await
        .with_context(|| format!("list_annotations {id}"))?;
    Ok(Json(items).into_response())
}

/// PUT /api/papers/{paper_id}/annotations/{annotation_id} — create or replace.
/// Idempotent on purpose: the annotation id is minted by the reader's plugin,
/// so a retried save re-sends the same address instead of duplicating a mark.
pub async fn put_annotation(
    State(app): State<AppState>,
    Path((paper_id, annotation_id)): Path<(String, String)>,
    Json(body): Json<NewAnnotation>,
) -> Result<Response, ApiError> {
    crate::annotations::validate_id(&annotation_id).map_err(ApiError::BadRequest)?;
    crate::annotations::validate_new(&body).map_err(ApiError::BadRequest)?;
    fetch_paper(&app.pool, &paper_id, Trash::Allow, "put_annotation").await?;
    let a = app
        .annotations
        .put(&paper_id, &annotation_id, body)
        .await
        .with_context(|| format!("put_annotation {paper_id}/{annotation_id}"))?;
    // The note feeds the `notes` search field; nudge the sweep so the
    // index catches up without waiting for its next poll.
    app.wake_search();
    Ok(Json(a).into_response())
}

/// DELETE /api/papers/{paper_id}/annotations/{annotation_id}
pub async fn delete_annotation(
    State(app): State<AppState>,
    Path((paper_id, annotation_id)): Path<(String, String)>,
) -> Result<Response, ApiError> {
    if !app
        .annotations
        .delete(&paper_id, &annotation_id)
        .await
        .with_context(|| format!("delete_annotation {paper_id}/{annotation_id}"))?
    {
        return Err(ApiError::NotFound);
    }
    app.wake_search();
    Ok(StatusCode::NO_CONTENT.into_response())
}

/// DELETE /api/papers/{id}/annotations — clear every mark on a paper. Answers
/// with the count rather than 204: "cleared 12 highlights" is worth surfacing.
pub async fn clear_annotations(
    State(app): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    fetch_paper(&app.pool, &id, Trash::Allow, "clear_annotations").await?;
    let n = app
        .annotations
        .delete_all(&id)
        .await
        .with_context(|| format!("clear_annotations {id}"))?;
    app.wake_search();
    Ok(Json(serde_json::json!({ "deleted": n })).into_response())
}
