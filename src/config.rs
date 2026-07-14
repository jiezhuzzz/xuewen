use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub inbox_dir: PathBuf,
    pub library_root: PathBuf,
    pub database_url: String,
    #[serde(default)]
    pub grobid_url: Option<String>,
    #[serde(default)]
    pub contact_email: Option<String>,
    #[serde(default)]
    pub proxy: Option<ProxyConfig>,
    #[serde(default)]
    pub search: SearchConfig,
    /// Daily arXiv recommendations. Absent ⇒ the feature is off.
    #[serde(default)]
    pub daily: Option<DailyConfig>,
    #[serde(default)]
    pub ai: AiConfig,
    #[serde(default)]
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProxyConfig {
    /// EZproxy login prefix; a target URL is percent-encoded and appended.
    /// e.g. "https://proxy.uchicago.edu/login?url="
    pub login_url: String,
}

/// Search settings. Always present: defaults apply when `[search]` is absent.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SearchConfig {
    /// Tantivy index directory (derived data; safe to delete).
    pub index_dir: PathBuf,
    pub qdrant_url: String,
    pub qdrant_collection: String,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            index_dir: PathBuf::from("./search-index"),
            qdrant_url: "http://localhost:6333".to_string(),
            qdrant_collection: "xuewen".to_string(),
        }
    }
}

/// Daily arXiv recommendations (`[daily]`).
#[derive(Debug, Clone, Deserialize)]
pub struct DailyConfig {
    /// arXiv category codes, e.g. ["cs.AI", "cs.LG"].
    pub categories: Vec<String>,
    /// Also keep cross-listed announcements.
    #[serde(default)]
    pub include_cross_list: bool,
    /// Ranked papers kept per day.
    #[serde(default = "default_daily_max_papers")]
    pub max_papers: usize,
    /// Daily run time, UTC wall clock "HH:MM".
    #[serde(default = "default_daily_run_at")]
    pub run_at: String,
    /// Batches older than this many days are pruned.
    #[serde(default = "default_daily_retention_days")]
    pub retention_days: u32,
}

fn default_daily_max_papers() -> usize {
    20
}
fn default_daily_run_at() -> String {
    "09:00".to_string()
}
fn default_daily_retention_days() -> u32 {
    14
}

/// Endpoint + model overrides shared by every AI use. `#[serde(flatten)]`-ed
/// into `[ai]` and each use-section so its fields sit at that section's level.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct AiDefaults {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub reasoning_effort: Option<String>,
}

/// A use's endpoint resolved against the `[ai]` defaults + built-ins.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
}

/// All AI/LLM config (`[ai]`): shared defaults plus each use.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    #[serde(flatten)]
    pub defaults: AiDefaults,
    /// Semantic-search embeddings. Absent ⇒ semantic search off.
    pub embedding: Option<EmbeddingConfig>,
    /// Paper chat. No models ⇒ chat off.
    pub chat: ChatConfig,
    /// Per-paper library summaries. Absent ⇒ off.
    pub summary: Option<AiDefaults>,
    /// Daily-feed summaries. Absent ⇒ off.
    pub daily: Option<AiDefaults>,
    /// Structured reference parsing for PDF citation popovers. Absent ⇒ off.
    pub citations: Option<AiDefaults>,
}

impl AiConfig {
    /// Merge a use's overrides over the `[ai]` defaults and built-ins.
    pub fn resolve(&self, use_: &AiDefaults) -> Resolved {
        let pick = |a: &Option<String>, b: &Option<String>| a.clone().or_else(|| b.clone());
        let base_url =
            pick(&use_.base_url, &self.defaults.base_url).unwrap_or_else(default_ai_base_url);
        let api_key_env =
            pick(&use_.api_key_env, &self.defaults.api_key_env).unwrap_or_else(default_api_key_env);
        let api_key = resolve_key(pick(&use_.api_key, &self.defaults.api_key), &api_key_env);
        Resolved {
            base_url,
            api_key,
            model: pick(&use_.model, &self.defaults.model),
            reasoning_effort: pick(&use_.reasoning_effort, &self.defaults.reasoning_effort),
        }
    }
}

/// Inline key wins; else the named env var; empty ⇒ None.
pub fn resolve_key(inline: Option<String>, api_key_env: &str) -> Option<String> {
    inline
        .or_else(|| std::env::var(api_key_env).ok())
        .filter(|k| !k.trim().is_empty())
}

fn default_ai_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}
fn default_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
}

/// Semantic-search embeddings (`[ai.embedding]`).
#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(flatten)]
    pub endpoint: AiDefaults,
    #[serde(default = "default_embed_dims")]
    pub dims: usize,
}
fn default_embed_dims() -> usize {
    1536
}

impl EmbeddingConfig {
    /// The embedding model: its own override, or the built-in default.
    /// Embeddings never inherit `[ai].model` (a chat model is never a valid
    /// embedding model), which is why this ignores the `[ai]` defaults.
    pub fn model(&self) -> String {
        self.endpoint
            .model
            .clone()
            .unwrap_or_else(|| "text-embedding-3-small".to_string())
    }
}

/// Paper chat (`[ai.chat]`). No models ⇒ feature disabled.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ChatConfig {
    pub models: Vec<ChatModelConfig>,
    pub max_context_chars: usize,
}
impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            max_context_chars: 60_000,
        }
    }
}

/// One selectable chat model (`[[ai.chat.models]]`).
#[derive(Debug, Clone, Deserialize)]
pub struct ChatModelConfig {
    /// Shown in the UI dropdown; display-only.
    pub label: String,
    #[serde(flatten)]
    pub endpoint: AiDefaults,
}

/// UI preferences (`[ui]`), surfaced to the frontend via `/api/settings`.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    /// Whether the abstract disclosure starts collapsed in the Details panel.
    pub fold_abstract: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            fold_abstract: true,
        }
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading config {}", path.display()))?;
        let mut cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing config {}", path.display()))?;
        let home = std::env::var_os("HOME").map(PathBuf::from);
        cfg.inbox_dir = expand_tilde(cfg.inbox_dir, home.clone());
        cfg.library_root = expand_tilde(cfg.library_root, home.clone());
        cfg.search.index_dir = expand_tilde(cfg.search.index_dir, home);
        Ok(cfg)
    }
}

/// Expand a leading `~/` (or bare `~`) using `home`; otherwise return as-is.
fn expand_tilde(p: PathBuf, home: Option<PathBuf>) -> PathBuf {
    match (p.strip_prefix("~"), home) {
        (Ok(rest), Some(home)) => home.join(rest),
        _ => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn loads_minimal_config() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"
"#
        )
        .unwrap();

        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(cfg.inbox_dir, PathBuf::from("/data/inbox"));
        assert_eq!(cfg.library_root, PathBuf::from("/data/library"));
        assert_eq!(cfg.database_url, "sqlite:/data/library.db");
        assert_eq!(cfg.grobid_url, None);
    }

    #[test]
    fn expands_leading_tilde_with_home() {
        let home = Some(PathBuf::from("/home/u"));
        assert_eq!(
            expand_tilde(PathBuf::from("~/papers/inbox"), home.clone()),
            PathBuf::from("/home/u/papers/inbox")
        );
        // No tilde, or no HOME: unchanged.
        assert_eq!(
            expand_tilde(PathBuf::from("/data/inbox"), home),
            PathBuf::from("/data/inbox")
        );
        assert_eq!(
            expand_tilde(PathBuf::from("~/x"), None),
            PathBuf::from("~/x")
        );
    }

    #[test]
    fn load_error_names_the_file() {
        let err = Config::load(Path::new("/nope/xuewen.toml")).unwrap_err();
        assert!(err.to_string().contains("/nope/xuewen.toml"));
    }

    #[test]
    fn loads_proxy_section() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"

[proxy]
login_url = "https://proxy.uchicago.edu/login?url="
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(
            cfg.proxy.unwrap().login_url,
            "https://proxy.uchicago.edu/login?url="
        );
    }

    #[test]
    fn proxy_defaults_to_none() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"
"#
        )
        .unwrap();
        assert!(Config::load(f.path()).unwrap().proxy.is_none());
    }

    #[test]
    fn search_defaults_when_section_absent() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        assert_eq!(cfg.search.index_dir, PathBuf::from("./search-index"));
        assert_eq!(cfg.search.qdrant_url, "http://localhost:6333");
        assert_eq!(cfg.search.qdrant_collection, "xuewen");
        assert!(cfg.ai.embedding.is_none());
    }

    #[test]
    fn loads_ai_embedding_section_with_defaults() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"

[search]
index_dir = "~/idx"

[ai.embedding]
api_key = "sk-test"
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        // tilde expanded like inbox_dir/library_root
        assert!(!cfg.search.index_dir.starts_with("~"));
        let e = cfg.ai.embedding.as_ref().unwrap();
        assert_eq!(e.dims, 1536);
        assert_eq!(e.endpoint.api_key.as_deref(), Some("sk-test"));
        // embedding has no model of its own here; base_url/api_key resolve
        // through [ai] defaults + built-ins.
        let r = cfg.ai.resolve(&e.endpoint);
        assert_eq!(r.base_url, "https://api.openai.com/v1");
        assert_eq!(r.api_key.as_deref(), Some("sk-test"));
    }

    #[test]
    fn daily_defaults_to_none() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"
"#
        )
        .unwrap();
        assert!(Config::load(f.path()).unwrap().daily.is_none());
    }

    #[test]
    fn loads_daily_section_with_defaults() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"

[daily]
categories = ["cs.AI", "cs.LG"]
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        let d = cfg.daily.unwrap();
        assert_eq!(d.categories, vec!["cs.AI", "cs.LG"]);
        assert!(!d.include_cross_list);
        assert_eq!(d.max_papers, 20);
        assert_eq!(d.run_at, "09:00");
        assert_eq!(d.retention_days, 14);
        // The daily summarizer LLM now lives under [ai.daily], separately.
        assert!(cfg.ai.daily.is_none());
    }

    #[test]
    fn ai_chat_config_parses_models_with_defaults() {
        let cfg: Config = toml::from_str(
            r#"
inbox_dir     = "./inbox"
library_root  = "./library"
database_url  = "sqlite:./x.db"

[[ai.chat.models]]
label = "GPT-5 Mini"
model = "gpt-5-mini"

[[ai.chat.models]]
label    = "Local Qwen"
base_url = "http://localhost:11434/v1"
model    = "qwen3:32b"
"#,
        )
        .unwrap();
        assert_eq!(cfg.ai.chat.models.len(), 2);
        assert_eq!(
            cfg.ai.chat.models[1].endpoint.model.as_deref(),
            Some("qwen3:32b")
        );
        assert_eq!(cfg.ai.chat.max_context_chars, 60_000);
        // Endpoint fields resolve through [ai] defaults + built-ins.
        let r0 = cfg.ai.resolve(&cfg.ai.chat.models[0].endpoint);
        assert_eq!(r0.base_url, "https://api.openai.com/v1");
        let r1 = cfg.ai.resolve(&cfg.ai.chat.models[1].endpoint);
        assert_eq!(r1.base_url, "http://localhost:11434/v1");
    }

    #[test]
    fn ai_chat_config_absent_means_disabled() {
        let cfg: Config = toml::from_str(
            r#"
inbox_dir     = "./inbox"
library_root  = "./library"
database_url  = "sqlite:./x.db"
"#,
        )
        .unwrap();
        assert!(cfg.ai.chat.models.is_empty());
        assert_eq!(cfg.ai.chat.max_context_chars, 60_000);
    }

    #[test]
    fn resolve_key_prefers_inline_over_env_and_empty_is_none() {
        assert_eq!(
            resolve_key(Some("sk-inline".into()), "XUEWEN_TEST_UNSET_ENV"),
            Some("sk-inline".into())
        );
        // Env var unset -> keyless (requests carry no Authorization).
        assert_eq!(resolve_key(None, "XUEWEN_TEST_UNSET_ENV"), None);
    }

    #[test]
    fn loads_ai_summary_section_with_defaults() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/d/i"
library_root = "/d/l"
database_url = "sqlite:/d/x.db"

[ai.summary]
model = "gpt-4o-mini"
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        let s = cfg.ai.summary.as_ref().expect("summary section present");
        assert_eq!(s.model.as_deref(), Some("gpt-4o-mini"));
        let r = cfg.ai.resolve(&s);
        assert_eq!(r.base_url, "https://api.openai.com/v1");
        assert_eq!(r.model.as_deref(), Some("gpt-4o-mini"));
    }

    #[test]
    fn ui_fold_abstract_defaults_true_and_summary_absent() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/d/i"
library_root = "/d/l"
database_url = "sqlite:/d/x.db"
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        assert!(cfg.ai.summary.is_none());
        assert!(cfg.ui.fold_abstract, "fold_abstract defaults to true");
    }

    #[test]
    fn ai_resolve_precedence_use_over_ai_over_builtin() {
        use super::{AiConfig, AiDefaults};
        let ai = AiConfig {
            defaults: AiDefaults {
                base_url: Some("https://ai.example/v1".into()),
                model: Some("gpt-4o-mini".into()),
                reasoning_effort: Some("high".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        // Empty use inherits [ai] defaults.
        let r = ai.resolve(&AiDefaults::default());
        assert_eq!(r.base_url, "https://ai.example/v1");
        assert_eq!(r.model.as_deref(), Some("gpt-4o-mini"));
        assert_eq!(r.reasoning_effort.as_deref(), Some("high"));
        // Use override wins.
        let r2 = ai.resolve(&AiDefaults {
            model: Some("gpt-5.6-terra".into()),
            ..Default::default()
        });
        assert_eq!(r2.model.as_deref(), Some("gpt-5.6-terra"));
        // Built-in base_url when [ai] omits it.
        let bare = AiConfig::default();
        assert_eq!(
            bare.resolve(&AiDefaults::default()).base_url,
            "https://api.openai.com/v1"
        );
    }

    #[test]
    fn ai_sections_parse_with_flatten_and_inheritance() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/d/i"
library_root = "/d/l"
database_url = "sqlite:/d/x.db"

[ai]
model = "gpt-4o-mini"
reasoning_effort = "high"

[ai.embedding]
model = "text-embedding-3-small"
dims = 1536

[ai.chat]
[[ai.chat.models]]
label = "Terra"
model = "gpt-5.6-terra"

[ai.summary]

[ai.daily]
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        let ai = &cfg.ai;
        assert_eq!(ai.defaults.model.as_deref(), Some("gpt-4o-mini"));
        // embedding overrides model, keeps its own model
        assert_eq!(
            ai.embedding.as_ref().unwrap().endpoint.model.as_deref(),
            Some("text-embedding-3-small")
        );
        assert_eq!(ai.embedding.as_ref().unwrap().dims, 1536);
        // chat model present with its own model
        assert_eq!(ai.chat.models[0].label, "Terra");
        assert_eq!(
            ai.chat.models[0].endpoint.model.as_deref(),
            Some("gpt-5.6-terra")
        );
        assert_eq!(ai.chat.max_context_chars, 60_000);
        // present-but-empty summary/daily ⇒ Some(defaults), inherit via resolve
        assert!(ai.summary.is_some());
        assert_eq!(
            ai.resolve(ai.summary.as_ref().unwrap()).model.as_deref(),
            Some("gpt-4o-mini")
        );
        assert_eq!(
            ai.resolve(ai.summary.as_ref().unwrap())
                .reasoning_effort
                .as_deref(),
            Some("high")
        );
        assert!(ai.daily.is_some());
    }

    #[test]
    fn ai_absent_means_features_off() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            "inbox_dir=\"/d/i\"\nlibrary_root=\"/d/l\"\ndatabase_url=\"sqlite:/d/x.db\"\n"
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        assert!(cfg.ai.embedding.is_none());
        assert!(cfg.ai.chat.models.is_empty());
        assert!(cfg.ai.summary.is_none());
        assert!(cfg.ai.daily.is_none());
        assert!(cfg.ai.citations.is_none());
    }

    #[test]
    fn embedding_model_uses_own_or_builtin_never_ai_default() {
        use super::{AiDefaults, EmbeddingConfig};
        let e = EmbeddingConfig {
            endpoint: AiDefaults::default(),
            dims: 1536,
        };
        assert_eq!(e.model(), "text-embedding-3-small"); // no inherit, built-in
        let e2 = EmbeddingConfig {
            endpoint: AiDefaults {
                model: Some("my-embed".into()),
                ..Default::default()
            },
            dims: 1536,
        };
        assert_eq!(e2.model(), "my-embed"); // own override
    }

    #[test]
    fn example_config_parses() {
        // The committed example must always load (all sections commented is fine).
        let cfg = Config::load(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/xuewen.example.toml"
        )));
        assert!(
            cfg.is_ok(),
            "xuewen.example.toml must parse: {:?}",
            cfg.err()
        );
    }

    #[test]
    fn ai_resolve_inherits_shared_inline_api_key() {
        use super::{AiConfig, AiDefaults};
        let ai = AiConfig {
            defaults: AiDefaults {
                api_key: Some("sk-shared".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        // A use with no key of its own inherits the [ai] inline key.
        assert_eq!(
            ai.resolve(&AiDefaults::default()).api_key.as_deref(),
            Some("sk-shared")
        );
        // A use's own inline key wins.
        let own = AiDefaults {
            api_key: Some("sk-own".into()),
            ..Default::default()
        };
        assert_eq!(ai.resolve(&own).api_key.as_deref(), Some("sk-own"));
    }

    #[test]
    fn ai_citations_section_parses_and_is_off_by_default() {
        let with: crate::config::AiConfig =
            toml::from_str("model = \"gpt-4o-mini\"\n[citations]\nmodel = \"gpt-5-mini\"").unwrap();
        let use_ = with.citations.as_ref().expect("[ai.citations] present");
        assert_eq!(with.resolve(use_).model.as_deref(), Some("gpt-5-mini"));

        let without: crate::config::AiConfig = toml::from_str("model = \"gpt-4o-mini\"").unwrap();
        assert!(without.citations.is_none());
    }
}
