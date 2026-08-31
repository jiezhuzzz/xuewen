use serde::Serialize;

use crate::config::TranslateProvider;
use crate::models::{Paper, PaperStatus};
use crate::resolve::ResolvedMetadata;

/// A tag reference as embedded in a paper row/detail.
#[derive(Serialize, Clone)]
pub struct TagRef {
    pub id: String,
    pub name: String,
}

impl From<crate::models::Tag> for TagRef {
    fn from(t: crate::models::Tag) -> Self {
        Self {
            id: t.id,
            name: t.name,
        }
    }
}

/// A project reference as embedded in a paper row/detail.
#[derive(Serialize, Clone)]
pub struct ProjectRef {
    pub id: String,
    pub name: String,
}

impl From<crate::models::Project> for ProjectRef {
    fn from(p: crate::models::Project) -> Self {
        Self {
            id: p.id,
            name: p.name,
        }
    }
}

/// A paper for the list view (no abstract, to keep the payload light).
#[derive(Serialize)]
pub struct PaperSummary {
    pub id: String,
    /// Manual "known as" name (e.g. "RVSpec"); see `models::Paper::name`.
    pub name: Option<String>,
    pub title: Option<String>,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub cite_key: Option<String>,
    pub url: Option<String>,
    pub source: Option<String>,
    pub status: PaperStatus,
    pub added_at: String,
    pub starred: bool,
    pub tags: Vec<TagRef>,
    pub projects: Vec<ProjectRef>,
}

impl From<&Paper> for PaperSummary {
    fn from(p: &Paper) -> Self {
        Self {
            id: p.id.clone(),
            name: p.name.clone(),
            title: p.meta.title.clone(),
            authors: p.meta.authors.0.clone(),
            venue: p.meta.venue.clone(),
            year: p.meta.year,
            doi: p.meta.doi.clone(),
            arxiv_id: p.meta.arxiv_id.clone(),
            dblp_key: p.meta.dblp_key.clone(),
            cite_key: p.cite_key.clone(),
            url: p.meta.url.clone(),
            source: p.meta.source.clone(),
            status: p.meta.status,
            added_at: p.added_at.clone(),
            starred: p.starred,
            tags: Vec::new(),
            projects: Vec::new(),
        }
    }
}

/// A paper for the detail view: the summary fields plus the abstract. The
/// summary's `tags`/`projects` carry the paper's tag and project memberships
/// (see `attach`).
#[derive(Serialize)]
pub struct PaperDetail {
    #[serde(flatten)]
    pub summary: PaperSummary,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    /// LLM-generated structured summary (JSON key "summary"); absent until generated.
    #[serde(rename = "summary", skip_serializing_if = "Option::is_none")]
    pub ai_summary: Option<crate::summary::Summary>,
}

impl From<&Paper> for PaperDetail {
    fn from(p: &Paper) -> Self {
        Self {
            summary: PaperSummary::from(p),
            abstract_text: p.meta.abstract_text.clone(),
            ai_summary: None,
        }
    }
}

impl PaperDetail {
    /// Attach the paper's tag and project memberships to the embedded summary.
    pub fn attach(mut self, tags: Vec<TagRef>, projects: Vec<ProjectRef>) -> Self {
        self.summary.tags = tags;
        self.summary.projects = projects;
        self
    }
}

/// PATCH /api/papers/{id}/name response: the normalized value as stored —
/// the client treats this echo as authoritative, not its own input.
#[derive(Serialize)]
pub struct PaperNameResponse {
    pub name: Option<String>,
}

/// Library counts for the header.
#[derive(Serialize)]
pub struct Stats {
    pub total: usize,
    pub resolved: usize,
    pub needs_review: usize,
}

/// A manual-identify candidate: a lossless wire mirror of `ResolvedMetadata`
/// (round-trips through POST /api/papers/{id}/identify without loss).
#[derive(Serialize, serde::Deserialize)]
pub struct Candidate {
    pub title: Option<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: Option<String>,
    pub authors: Vec<String>,
    pub venue: Option<String>,
    pub year: Option<i64>,
    pub doi: Option<String>,
    pub arxiv_id: Option<String>,
    pub dblp_key: Option<String>,
    pub url: Option<String>,
    pub source: String,
}

impl From<&ResolvedMetadata> for Candidate {
    fn from(m: &ResolvedMetadata) -> Self {
        Self {
            title: m.title.clone(),
            abstract_text: m.abstract_text.clone(),
            authors: m.authors.clone(),
            venue: m.venue.clone(),
            year: m.year,
            doi: m.doi.clone(),
            arxiv_id: m.arxiv_id.clone(),
            dblp_key: m.dblp_key.clone(),
            url: m.url.clone(),
            source: m.source.clone(),
        }
    }
}

impl Candidate {
    /// Back to resolver metadata for the apply path.
    pub fn into_metadata(self) -> ResolvedMetadata {
        ResolvedMetadata {
            title: self.title,
            abstract_text: self.abstract_text,
            authors: self.authors,
            venue: self.venue,
            year: self.year,
            doi: self.doi,
            arxiv_id: self.arxiv_id,
            dblp_key: self.dblp_key,
            url: self.url,
            source: self.source,
        }
    }
}

/// Why a paper matched a search query.
#[derive(Serialize)]
pub struct SearchMatch {
    pub engine: String,
    pub field: String,
    /// HTML-safe: escaped text with <mark> highlights only.
    pub snippet: String,
    pub page: Option<i64>,
}

#[derive(Serialize)]
pub struct SearchResult {
    pub paper: PaperSummary,
    #[serde(rename = "match")]
    pub match_info: SearchMatch,
}

#[derive(Serialize)]
pub struct SemanticAvailability {
    pub available: bool,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct SearchResponse {
    pub semantic: SemanticAvailability,
    pub results: Vec<SearchResult>,
}

#[derive(Serialize)]
pub struct TierCounts {
    pub indexed: i64,
    pub pending: i64,
    pub failed: i64,
}

#[derive(Serialize)]
pub struct SearchStatus {
    pub fts: TierCounts,
    pub vectors: TierCounts,
    pub semantic_available: bool,
    pub reason: Option<String>,
}

/// One paper in the daily-recommendations response (Glance widget input).
#[derive(Serialize)]
pub struct DailyPaperDto {
    pub rank: i64,
    pub arxiv_id: String,
    pub title: String,
    pub authors: Vec<String>,
    #[serde(rename = "abstract")]
    pub abstract_text: String,
    pub categories: Vec<String>,
    pub score: f64,
    pub tldr: Option<String>,
    pub abs_url: String,
    pub pdf_url: String,
    pub summary: Option<crate::summary::Summary>,
    pub code_url: Option<String>,
}

impl From<&crate::daily::store::DailyPaper> for DailyPaperDto {
    fn from(p: &crate::daily::store::DailyPaper) -> Self {
        Self {
            rank: p.rank,
            arxiv_id: p.arxiv_id.clone(),
            title: p.title.clone(),
            authors: p.authors.clone(),
            abstract_text: p.abstract_text.clone(),
            categories: p.categories.clone(),
            score: p.score,
            tldr: p.tldr.clone(),
            abs_url: p.abs_url.clone(),
            pdf_url: p.pdf_url.clone(),
            summary: p.summary.clone(),
            code_url: p.code_url.clone(),
        }
    }
}

/// `date` is `None` until the first non-empty batch exists.
#[derive(Serialize)]
pub struct DailyResponse {
    pub date: Option<String>,
    pub papers: Vec<DailyPaperDto>,
}

/// Outcome of POST /api/papers (upload) and POST /api/import (URL), tagged on
/// `outcome` — the frontend's `ImportResult` union in types.ts. `Unfetched`
/// is URL-import only: metadata resolved but no PDF could be fetched.
#[derive(Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ImportOutcome {
    Ingested {
        id: String,
        title: Option<String>,
        status: PaperStatus,
    },
    Duplicate {
        id: String,
    },
    SameWork {
        id: String,
    },
    InTrash {
        id: String,
    },
    Unfetched {
        title: Option<String>,
        doi: Option<String>,
    },
}

/// GET /api/settings (the frontend's `Settings` in types.ts). Reports only
/// whether a proxy cookie is stored — never the value.
#[derive(Serialize)]
pub struct Settings {
    /// `None` when no `[proxy]` is configured (the UI hides institutional
    /// access entirely — a cookie saved there would never be used).
    pub proxy: Option<ProxySettings>,
    pub proxy_cookie_set: bool,
    pub proxy_cookie_updated_at: Option<String>,
    pub fold_abstract: bool,
    pub translate: TranslateSettings,
}

/// The `proxy` block of GET /api/settings: the host of `[proxy].login_url`,
/// so the UI can name the deployment's own proxy in its cookie help copy.
#[derive(Serialize)]
pub struct ProxySettings {
    pub host: String,
}

/// The `translate` block of GET /api/settings: `{"enabled": false}` alone
/// when the service is off (the provider fields are absent, not null), the
/// full provider set when on.
#[derive(Serialize)]
pub struct TranslateSettings {
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub providers: Option<Vec<TranslateProvider>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default_provider: Option<TranslateProvider>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_lang: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<&'static str>,
}

impl TranslateSettings {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            providers: None,
            default_provider: None,
            target_lang: None,
            trigger: None,
        }
    }
}

impl From<&crate::translate::TranslateService> for TranslateSettings {
    fn from(svc: &crate::translate::TranslateService) -> Self {
        Self {
            enabled: true,
            providers: Some(svc.providers()),
            default_provider: Some(svc.default_provider()),
            target_lang: Some(svc.target_lang().to_string()),
            trigger: Some(match svc.trigger() {
                crate::config::TranslateTrigger::Auto => "auto",
                crate::config::TranslateTrigger::Manual => "manual",
            }),
        }
    }
}

/// POST /api/translate response.
#[derive(Serialize)]
pub struct TranslationDto {
    pub translation: String,
    pub provider: TranslateProvider,
    pub source_lang: Option<String>,
    pub target_lang: String,
}

impl From<crate::translate::Translated> for TranslationDto {
    fn from(t: crate::translate::Translated) -> Self {
        Self {
            translation: t.text,
            provider: t.provider,
            source_lang: t.source_lang,
            target_lang: t.target_lang,
        }
    }
}

/// GET/PUT /api/papers/{id}/code response: `code` is the `paper_code` row
/// (the frontend's `PaperCodeStatus`), null when nothing is attached.
#[derive(Serialize)]
pub struct CodeAttachment {
    pub attached: bool,
    pub code: Option<crate::agent::store::PaperCode>,
}

/// GET /api/chat/models (`ChatModelInfo` rows in the frontend's types.ts).
#[derive(Serialize)]
pub struct ChatModels {
    pub available: bool,
    pub models: Vec<ChatModel>,
}

#[derive(Serialize)]
pub struct ChatModel {
    pub id: String,
    pub label: String,
}

/// One stored chat turn as GET /api/papers/{id}/chat returns it (the
/// frontend's `ChatTurnRow`). The store's `tools_json` TEXT is parsed here,
/// once, so the wire carries the tool log structured — the same shape the
/// live SSE `tool` events use — instead of a JSON string the client would
/// have to re-parse.
#[derive(Serialize)]
pub struct ChatTurn {
    pub id: i64,
    pub role: String,
    pub content: String,
    pub model: Option<String>,
    pub created_at: String,
    pub tools: Option<serde_json::Value>,
}

impl From<crate::chat::store::ChatMessageRow> for ChatTurn {
    fn from(r: crate::chat::store::ChatMessageRow) -> Self {
        let tools = r.tools_json.as_deref().and_then(|s| {
            serde_json::from_str(s)
                .map_err(|e| tracing::warn!("chat turn {}: unparseable tools_json: {e}", r.id))
                .ok()
        });
        Self {
            id: r.id,
            role: r.role,
            content: r.content,
            model: r.model,
            created_at: r.created_at,
            tools,
        }
    }
}

/// A paper's preview geometry: how many pages the picker should lay out, and
/// the shape of page one so the placeholders are correct before any image
/// loads.
#[derive(Serialize)]
pub struct PreviewMeta {
    pub pages: u32,
    pub page_width: f32,
    pub page_height: f32,
}
