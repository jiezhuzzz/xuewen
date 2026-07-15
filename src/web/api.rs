use axum::extract::multipart::MultipartError;
use axum::extract::{Multipart, Path, Query, Request, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use tower::ServiceExt;
use tower_http::services::ServeFile;
use uuid::Uuid;

use super::dto::{
    Candidate, DailyPaperDto, DailyResponse, PaperDetail, PaperSummary, SearchMatch,
    SearchResponse, SearchResult, SearchStatus, SemanticAvailability, Stats, TierCounts,
};
use super::AppState;
use crate::db;
use crate::export;
use crate::import::{self, ImportError};
use crate::models::Identifier;
use crate::pipeline::{IdentifyOutcome, Outcome};
use crate::search::fts::FieldSel;

#[derive(Deserialize)]
pub struct ListParams {
    pub q: Option<String>,
    pub status: Option<String>,
    pub sort: Option<String>,
    pub project: Option<String>,
}

pub async fn list_papers(State(app): State<AppState>, Query(p): Query<ListParams>) -> Response {
    match db::list_papers(
        &app.pool,
        p.q.as_deref(),
        p.status.as_deref(),
        p.sort.as_deref(),
        p.project.as_deref(),
    )
    .await
    {
        Ok(papers) => {
            let out: Vec<PaperSummary> = papers.iter().map(PaperSummary::from).collect();
            Json(out).into_response()
        }
        Err(e) => {
            tracing::error!("list_papers: {e}");
            internal_error()
        }
    }
}

pub async fn get_paper(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    match db::get_by_id(&app.pool, &id).await {
        Ok(Some(p)) => {
            let ids = db::project_ids_for_paper(&app.pool, &p.id)
                .await
                .unwrap_or_default();
            let mut detail = PaperDetail::with_project_ids(&p, ids);
            detail.ai_summary = match crate::summary::store::get(&app.pool, &p.id).await {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!("get_paper summary for {}: {e}", p.id);
                    None
                }
            };
            Json(detail).into_response()
        }
        Ok(None) => not_found(),
        Err(e) => {
            tracing::error!("get_paper: {e}");
            internal_error()
        }
    }
}

/// Soft-delete a paper (web mutation): flag it deleted; the file is untouched.
/// Purge (permanent removal) is CLI-only. Idempotent on an already-trashed paper.
pub async fn delete_paper(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    match db::get_by_id(&app.pool, &id).await {
        Ok(Some(_)) => match db::soft_delete(&app.pool, &id).await {
            Ok(_) => {
                app.wake_search();
                Json(serde_json::json!({ "deleted": true })).into_response()
            }
            Err(e) => {
                tracing::error!("delete_paper: {e}");
                internal_error()
            }
        },
        Ok(None) => not_found(),
        Err(e) => {
            tracing::error!("delete_paper lookup: {e}");
            internal_error()
        }
    }
}

/// Import a single uploaded PDF: validate, stage into `inbox_dir/_uploads`, and
/// run the ingest pipeline. One PDF per request (the frontend uploads files one
/// at a time). Returns `ingested` (with title/status), `duplicate`, or an error.
pub async fn import_paper(State(app): State<AppState>, mut multipart: Multipart) -> Response {
    let ingest = match &app.ingest {
        Some(i) => i.clone(),
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({"error": "import not configured"})),
            )
                .into_response()
        }
    };

    // Take the first file part; skip any non-file fields.
    loop {
        let field = match multipart.next_field().await {
            Ok(Some(f)) => f,
            Ok(None) => return bad_request("no file"),
            Err(e) => return multipart_error(e),
        };
        let Some(filename) = field.file_name().map(|s| s.to_string()) else {
            continue;
        };
        let data = match field.bytes().await {
            Ok(b) => b,
            Err(e) => return multipart_error(e),
        };
        if !data.starts_with(b"%PDF") {
            return bad_request("not a PDF");
        }

        let resp = stage_and_ingest(&ingest, data.as_ref(), &filename, None).await;
        app.wake_search();
        return resp;
    }
}

/// Stage `bytes` under a sanitized, collision-safe name in the staging dir, run
/// the ingest pipeline (optionally with an identifier hint), and map the outcome
/// to the shared `ImportResult` JSON. Shared by file upload and URL import.
async fn stage_and_ingest(
    ingest: &super::Ingest,
    bytes: &[u8],
    filename: &str,
    hint: Option<Identifier>,
) -> Response {
    let stem = std::path::Path::new(filename)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("import.pdf");
    let staged = ingest
        .staging_dir
        .join(format!("{}-{stem}", Uuid::now_v7()));
    if let Err(e) = tokio::fs::create_dir_all(&ingest.staging_dir).await {
        tracing::error!("import staging dir: {e}");
        return internal_error();
    }
    if let Err(e) = tokio::fs::write(&staged, bytes).await {
        tracing::error!("import stage write: {e}");
        return internal_error();
    }
    match ingest.ctx.ingest_file_with_hint(&staged, hint).await {
        Ok(Outcome::Ingested(id)) => {
            let (title, status) = match db::get_by_id(&ingest.ctx.pool, &id).await {
                Ok(Some(p)) => (serde_json::json!(p.meta.title), p.meta.status),
                _ => (
                    serde_json::Value::Null,
                    crate::models::PaperStatus::NeedsReview,
                ),
            };
            Json(serde_json::json!({
                "outcome": "ingested", "id": id, "title": title, "status": status,
            }))
            .into_response()
        }
        Ok(Outcome::Duplicate) => Json(serde_json::json!({"outcome": "duplicate"})).into_response(),
        Ok(Outcome::SameWork(id)) => {
            Json(serde_json::json!({"outcome": "same_work", "id": id})).into_response()
        }
        Ok(Outcome::InTrash(id)) => {
            Json(serde_json::json!({"outcome": "in_trash", "id": id})).into_response()
        }
        Err(e) => {
            tracing::error!("import ingest: {e}");
            let _ = tokio::fs::remove_file(&staged).await;
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "import failed"})),
            )
                .into_response()
        }
    }
}

pub async fn stats(State(app): State<AppState>) -> Response {
    match db::stats(&app.pool).await {
        Ok((total, resolved, needs_review)) => Json(Stats {
            total: total as usize,
            resolved: resolved as usize,
            needs_review: needs_review as usize,
        })
        .into_response(),
        Err(e) => {
            tracing::error!("stats: {e}");
            internal_error()
        }
    }
}

/// Stream a paper's PDF from the library. Range-aware (via `ServeFile`) and
/// path-safe: the resolved file must live under `library_root`.
pub async fn pdf(State(app): State<AppState>, Path(id): Path<String>, req: Request) -> Response {
    let paper = match db::get_by_id(&app.pool, &id).await {
        Ok(Some(p)) => p,
        Ok(None) => return not_found(),
        Err(e) => {
            tracing::error!("pdf lookup: {e}");
            return internal_error();
        }
    };
    let path = app.library_root.join(&paper.rel_path);
    // Defense in depth: the canonical file must live under the library root.
    let under_root = {
        let (p, root) = (path.clone(), app.library_root.clone());
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
    if !under_root {
        return not_found();
    }
    match ServeFile::new(&path).oneshot(req).await {
        Ok(resp) => resp.map(axum::body::Body::new).into_response(),
        Err(e) => {
            tracing::error!("serve pdf: {e}");
            internal_error()
        }
    }
}

#[derive(Deserialize)]
pub struct IdentifyQuery {
    pub q: Option<String>,
}

/// Ungated candidate search for manual identify (the user is the gate).
pub async fn identify_search(
    State(app): State<AppState>,
    Query(p): Query<IdentifyQuery>,
) -> Response {
    let Some(ingest) = &app.ingest else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "identify not configured"})),
        )
            .into_response();
    };
    let Some(q) = p.q.as_deref().map(str::trim).filter(|s| !s.is_empty()) else {
        return bad_request("missing query");
    };
    let cands = ingest.ctx.resolver.search_candidates(q).await;
    let out: Vec<Candidate> = cands.iter().map(Candidate::from).collect();
    Json(out).into_response()
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
) -> Response {
    let Some(ingest) = &app.ingest else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "identify not configured"})),
        )
            .into_response();
    };
    let selectors =
        body.doi.is_some() as u8 + body.arxiv_id.is_some() as u8 + body.candidate.is_some() as u8;
    if selectors != 1 {
        return bad_request("provide exactly one of doi, arxiv_id, candidate");
    }

    // Read through the ctx pool: same handle the apply path writes with
    // (matches the pool-locality convention set in import_paper).
    let mut paper = match db::get_by_id(&ingest.ctx.pool, &id).await {
        Ok(Some(p)) => p,
        Ok(None) => return not_found(),
        Err(e) => {
            tracing::error!("identify lookup: {e}");
            return internal_error();
        }
    };
    let md = if let Some(c) = body.candidate {
        Some(c.into_metadata())
    } else if let Some(doi) = &body.doi {
        ingest
            .ctx
            .resolver
            .resolve(&crate::models::Identifier::Doi(doi.clone()), None)
            .await
    } else if let Some(axv) = &body.arxiv_id {
        ingest
            .ctx
            .resolver
            .resolve(&crate::models::Identifier::Arxiv(axv.clone()), None)
            .await
    } else {
        unreachable!("selector count checked above")
    };
    let Some(md) = md else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "identifier not found — not registered with Crossref/arXiv; try a title search"
            })),
        )
            .into_response();
    };

    let (md_doi, md_arxiv) = (md.doi.clone(), md.arxiv_id.clone());
    match ingest.ctx.apply_match(&mut paper, md).await {
        Ok(IdentifyOutcome::Applied) => {
            app.wake_search();
            let ids = db::project_ids_for_paper(&ingest.ctx.pool, &paper.id)
                .await
                .unwrap_or_default();
            Json(PaperDetail::with_project_ids(&paper, ids)).into_response()
        }
        Ok(IdentifyOutcome::SameWork(other)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": format!("same work as {other}"), "id": other})),
        )
            .into_response(),
        Ok(IdentifyOutcome::Trashed) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "paper is in the trash"})),
        )
            .into_response(),
        Err(e) => {
            // Lost a race: something claimed this identifier between the guard and
            // the update. Report it as the conflict it is, mirroring ingest.
            if db::is_unique_violation(&e) {
                if let Ok(Some(existing)) =
                    db::find_by_identifier(&ingest.ctx.pool, md_doi.as_deref(), md_arxiv.as_deref())
                        .await
                {
                    return (
                        StatusCode::CONFLICT,
                        Json(serde_json::json!({
                            "error": format!("same work as {}", existing.id),
                            "id": existing.id,
                        })),
                    )
                        .into_response();
                }
            }
            tracing::error!("identify apply: {e}");
            internal_error()
        }
    }
}

#[derive(Deserialize)]
pub struct ImportUrlBody {
    pub input: String,
}

/// Import from a URL/DOI/arXiv id: fetch the PDF (arXiv/proxy/OA), then ingest.
pub async fn import_url(State(app): State<AppState>, Json(body): Json<ImportUrlBody>) -> Response {
    let Some(ingest) = app.ingest.clone() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "import not configured"})),
        )
            .into_response();
    };
    let fetcher = match import::Fetcher::new(app.proxy_login_url.clone()) {
        Ok(f) => f,
        Err(e) => {
            tracing::error!("build fetcher: {e}");
            return internal_error();
        }
    };
    let cookie = db::get_setting(&ingest.ctx.pool, "proxy_cookie")
        .await
        .ok()
        .flatten();
    match import::import_source(
        &fetcher,
        &ingest.ctx.resolver,
        &body.input,
        cookie.as_deref(),
    )
    .await
    {
        Ok(fetched) => {
            let resp = stage_and_ingest(&ingest, &fetched.bytes, "import.pdf", fetched.hint).await;
            app.wake_search();
            resp
        }
        Err(ImportError::Unsupported) => bad_request("unsupported input"),
        Err(ImportError::CookieExpired) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({"error": "proxy session expired — refresh your cookie"})),
        )
            .into_response(),
        Err(ImportError::Unfetched { metadata }) => {
            let (title, doi) = match metadata {
                Some(m) => (serde_json::json!(m.title), serde_json::json!(m.doi)),
                None => (serde_json::Value::Null, serde_json::Value::Null),
            };
            Json(serde_json::json!({"outcome": "unfetched", "title": title, "doi": doi}))
                .into_response()
        }
        Err(ImportError::Network(e)) => {
            tracing::error!("import fetch: {e}");
            (
                StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({"error": "fetch failed"})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct ProxyCookieBody {
    pub cookie: String,
}

/// Report whether a proxy cookie is stored (never the value itself).
pub async fn get_settings(State(app): State<AppState>) -> Response {
    let set = db::get_setting(&app.pool, "proxy_cookie")
        .await
        .ok()
        .flatten()
        .is_some();
    let updated = db::setting_updated_at(&app.pool, "proxy_cookie")
        .await
        .ok()
        .flatten();
    Json(serde_json::json!({
        "proxy_cookie_set": set,
        "proxy_cookie_updated_at": updated,
        "fold_abstract": app.ui.fold_abstract,
    }))
    .into_response()
}

/// Store (overwrite) the EZproxy cookie.
pub async fn set_proxy_cookie(
    State(app): State<AppState>,
    Json(body): Json<ProxyCookieBody>,
) -> Response {
    let cookie = body.cookie.trim();
    if cookie.is_empty() {
        return bad_request("empty cookie");
    }
    match db::set_setting(&app.pool, "proxy_cookie", cookie).await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => {
            tracing::error!("set proxy cookie: {e}");
            internal_error()
        }
    }
}

/// Clear the stored EZproxy cookie.
pub async fn clear_proxy_cookie(State(app): State<AppState>) -> Response {
    match db::delete_setting(&app.pool, "proxy_cookie").await {
        Ok(()) => Json(serde_json::json!({"ok": true})).into_response(),
        Err(e) => {
            tracing::error!("clear proxy cookie: {e}");
            internal_error()
        }
    }
}

#[derive(Deserialize)]
pub struct CreateProjectBody {
    pub name: String,
    pub note: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateProjectBody {
    pub name: Option<String>,
    pub note: Option<String>,
}

pub async fn list_projects(State(app): State<AppState>) -> Response {
    match db::list_projects(&app.pool).await {
        Ok(list) => Json(list).into_response(),
        Err(e) => {
            tracing::error!("list_projects: {e}");
            internal_error()
        }
    }
}

pub async fn create_project(
    State(app): State<AppState>,
    Json(body): Json<CreateProjectBody>,
) -> Response {
    let name = body.name.trim();
    if name.is_empty() {
        return bad_request("empty name");
    }
    let note = body
        .note
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());
    match db::create_project(&app.pool, name, note).await {
        Ok(project) => (StatusCode::CREATED, Json(project)).into_response(),
        Err(e) if db::is_unique_violation(&e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "a project with that name already exists"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("create_project: {e}");
            internal_error()
        }
    }
}

pub async fn update_project(
    State(app): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpdateProjectBody>,
) -> Response {
    let existing = match db::get_project(&app.pool, &id).await {
        Ok(Some(p)) => p,
        Ok(None) => return not_found(),
        Err(e) => {
            tracing::error!("update_project lookup: {e}");
            return internal_error();
        }
    };
    // Merge: an omitted/blank name keeps the old one; an omitted note keeps the
    // old one, while an explicit blank note clears it.
    let name = body
        .name
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(&existing.name);
    let note = match &body.note {
        Some(n) => {
            let t = n.trim();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        }
        // `note` no longer exists on `Project` (Task 4 dropped the column); this
        // whole merge-fallback becomes dead once Task 9 removes the `_note`
        // param and its callers. `None` here is inert either way since
        // `db::update_project` ignores its `_note` argument.
        None => None,
    };
    match db::update_project(&app.pool, &id, name, note).await {
        Ok(_) => match db::get_project(&app.pool, &id).await {
            Ok(Some(p)) => Json(p).into_response(),
            _ => internal_error(),
        },
        Err(e) if db::is_unique_violation(&e) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "a project with that name already exists"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("update_project: {e}");
            internal_error()
        }
    }
}

pub async fn delete_project(State(app): State<AppState>, Path(id): Path<String>) -> Response {
    match db::delete_project(&app.pool, &id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(),
        Err(e) => {
            tracing::error!("delete_project: {e}");
            internal_error()
        }
    }
}

pub async fn add_paper_to_project(
    State(app): State<AppState>,
    Path((paper_id, project_id)): Path<(String, String)>,
) -> Response {
    // Pre-check both ids so a bad request is a clean 404 (not an FK error).
    match db::get_by_id(&app.pool, &paper_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found(),
        Err(e) => {
            tracing::error!("membership paper lookup: {e}");
            return internal_error();
        }
    }
    match db::get_project(&app.pool, &project_id).await {
        Ok(Some(_)) => {}
        Ok(None) => return not_found(),
        Err(e) => {
            tracing::error!("membership project lookup: {e}");
            return internal_error();
        }
    }
    match db::add_paper_to_project(&app.pool, &paper_id, &project_id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            tracing::error!("add membership: {e}");
            internal_error()
        }
    }
}

pub async fn remove_paper_from_project(
    State(app): State<AppState>,
    Path((paper_id, project_id)): Path<(String, String)>,
) -> Response {
    match db::remove_paper_from_project(&app.pool, &paper_id, &project_id).await {
        Ok(true) => StatusCode::NO_CONTENT.into_response(),
        Ok(false) => not_found(),
        Err(e) => {
            tracing::error!("remove membership: {e}");
            internal_error()
        }
    }
}

#[derive(Deserialize)]
pub struct FormatParam {
    pub format: Option<String>,
}

#[derive(Deserialize)]
pub struct ExportParams {
    pub format: Option<String>,
    pub q: Option<String>,
    pub status: Option<String>,
    pub sort: Option<String>,
    pub project: Option<String>,
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
) -> Response {
    match db::get_by_id(&app.pool, &id).await {
        Ok(Some(paper)) => {
            let body = export::format_entry(&paper, parse_format(p.format.as_deref()));
            (
                [(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; charset=utf-8",
                )],
                body,
            )
                .into_response()
        }
        Ok(None) => not_found(),
        Err(e) => {
            tracing::error!("export_paper: {e}");
            internal_error()
        }
    }
}

/// The current filtered set as a downloadable `.bib` file. Same filter semantics
/// as `GET /api/papers`.
pub async fn export_papers(State(app): State<AppState>, Query(p): Query<ExportParams>) -> Response {
    match db::list_papers(
        &app.pool,
        p.q.as_deref(),
        p.status.as_deref(),
        p.sort.as_deref(),
        p.project.as_deref(),
    )
    .await
    {
        Ok(papers) => {
            let body = export::format_entries(&papers, parse_format(p.format.as_deref()));
            (
                [
                    (axum::http::header::CONTENT_TYPE, "application/x-bibtex"),
                    (
                        axum::http::header::CONTENT_DISPOSITION,
                        "attachment; filename=\"xuewen.bib\"",
                    ),
                ],
                body,
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("export_papers: {e}");
            internal_error()
        }
    }
}

#[derive(Deserialize)]
pub struct SearchParams {
    pub q: Option<String>,
    pub fields: Option<String>,
    pub engines: Option<String>,
    pub status: Option<String>,
    pub project: Option<String>,
}

/// Hybrid search. `fields`/`engines` are CSV lists; absent or unknown-only
/// values fall back to "all" (mirrors the whitelisting style elsewhere).
pub async fn search_papers(State(app): State<AppState>, Query(p): Query<SearchParams>) -> Response {
    let Some(svc) = &app.search else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "search not configured"})),
        )
            .into_response();
    };
    let (keyword, semantic) = parse_engines(p.engines.as_deref());
    let req = crate::search::SearchRequest {
        q: p.q.unwrap_or_default(),
        fields: FieldSel::parse(p.fields.as_deref()),
        keyword,
        semantic,
        status: p.status,
        project: p.project,
    };
    match svc.search(&req).await {
        Ok(out) => {
            let results: Vec<SearchResult> = out
                .results
                .iter()
                .map(|(paper, m)| SearchResult {
                    paper: PaperSummary::from(paper),
                    match_info: SearchMatch {
                        engine: m.engine.clone(),
                        field: m.field.clone(),
                        snippet: m.snippet.clone(),
                        page: m.page,
                    },
                })
                .collect();
            Json(SearchResponse {
                semantic: SemanticAvailability {
                    available: out.semantic.available,
                    reason: out.semantic.reason,
                },
                results,
            })
            .into_response()
        }
        Err(e) => {
            tracing::error!("search: {e}");
            internal_error()
        }
    }
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

pub async fn search_status(State(app): State<AppState>) -> Response {
    let Some(svc) = &app.search else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "search not configured"})),
        )
            .into_response();
    };
    match svc.status().await {
        Ok(st) => Json(SearchStatus {
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
        .into_response(),
        Err(e) => {
            tracing::error!("search status: {e}");
            internal_error()
        }
    }
}

pub(super) fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({"error": "not found"})),
    )
        .into_response()
}

pub(super) fn internal_error() -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({"error": "internal error"})),
    )
        .into_response()
}

pub(super) fn bad_request(msg: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg })),
    )
        .into_response()
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
pub async fn daily_papers(State(app): State<AppState>) -> Response {
    if app.daily.is_none() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "daily papers not configured"})),
        )
            .into_response();
    }
    match crate::daily::store::latest_batch(&app.pool).await {
        Ok(Some((date, papers))) => Json(DailyResponse {
            date: Some(date),
            papers: papers.iter().map(DailyPaperDto::from).collect(),
        })
        .into_response(),
        Ok(None) => Json(DailyResponse {
            date: None,
            papers: Vec::new(),
        })
        .into_response(),
        Err(e) => {
            tracing::error!("daily papers: {e}");
            internal_error()
        }
    }
}

/// POST /api/daily/run — manual trigger; 202 started, 409 already running.
pub async fn run_daily(State(app): State<AppState>) -> Response {
    let Some(svc) = &app.daily else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "daily papers not configured"})),
        )
            .into_response();
    };
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    if svc.spawn_run(today) {
        (
            StatusCode::ACCEPTED,
            Json(serde_json::json!({"status": "started"})),
        )
            .into_response()
    } else {
        (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "a daily run is already in flight"})),
        )
            .into_response()
    }
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
) -> Response {
    let total: usize = body.references.iter().map(|r| r.len()).sum();
    if body.references.is_empty() || body.references.len() > 500 || total > 200_000 {
        return bad_request("references must be 1..=500 entries and under 200kB");
    }
    // The paper's venue seeds the style vote's tie-breaker.
    let venue = match db::get_by_id(&app.pool, &id).await {
        Ok(Some(p)) => p.meta.venue,
        Ok(None) => return not_found(),
        Err(e) => {
            tracing::error!("parse_citations lookup {id}: {e}");
            return internal_error();
        }
    };
    match app
        .citations
        .parse(&id, &body.references, venue.as_deref())
        .await
    {
        Ok(parsed) => Json(serde_json::json!({ "references": parsed })).into_response(),
        Err(e) => {
            tracing::error!("parse_citations {id}: {e}");
            internal_error()
        }
    }
}
