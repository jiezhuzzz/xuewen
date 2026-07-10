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
    pub chat: ChatConfig,
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
    /// Absent ⇒ semantic search is unavailable (keyword still works).
    pub embedding: Option<EmbeddingConfig>,
}

impl Default for SearchConfig {
    fn default() -> Self {
        Self {
            index_dir: PathBuf::from("./search-index"),
            qdrant_url: "http://localhost:6333".to_string(),
            qdrant_collection: "xuewen".to_string(),
            embedding: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct EmbeddingConfig {
    #[serde(default = "default_embed_base_url")]
    pub base_url: String,
    #[serde(default = "default_embed_model")]
    pub model: String,
    #[serde(default = "default_embed_dims")]
    pub dims: usize,
    /// Inline key; when absent the key is read from `api_key_env`.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
}

fn default_embed_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}
fn default_embed_model() -> String {
    "text-embedding-3-small".to_string()
}
fn default_embed_dims() -> usize {
    1536
}
fn default_api_key_env() -> String {
    "OPENAI_API_KEY".to_string()
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
    pub llm: DailyLlmConfig,
}

/// Chat-completions API used for the structured summaries (`[daily.llm]`).
#[derive(Debug, Clone, Deserialize)]
pub struct DailyLlmConfig {
    #[serde(default = "default_embed_base_url")]
    pub base_url: String,
    pub model: String,
    /// Inline key; when absent the key is read from `api_key_env`.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    /// Language the summaries are written in.
    #[serde(default = "default_daily_language")]
    pub language: String,
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
fn default_daily_language() -> String {
    "English".to_string()
}

/// One selectable chat model (`[[chat.models]]`): an OpenAI-compatible
/// chat-completions endpoint. Its API id is its position in the config file.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatModelConfig {
    /// Shown in the UI dropdown; display-only, need not be unique.
    pub label: String,
    #[serde(default = "default_embed_base_url")]
    pub base_url: String,
    pub model: String,
    /// Inline key; when absent the key is read from `api_key_env`.
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
}

impl ChatModelConfig {
    /// Inline key wins; else the env var. Unset/empty -> None: the entry is
    /// served keyless (no Authorization header) — right for local servers,
    /// and a forgotten hosted key surfaces as a 401 in the chat's inline
    /// error rather than silently hiding the model.
    pub fn resolve_key(&self) -> Option<String> {
        self.api_key
            .clone()
            .or_else(|| std::env::var(&self.api_key_env).ok())
            .filter(|k| !k.trim().is_empty())
    }
}

/// Paper-chat settings (`[chat]`). No models = feature disabled.
#[derive(Debug, Clone, Deserialize)]
pub struct ChatConfig {
    #[serde(default)]
    pub models: Vec<ChatModelConfig>,
    /// Chars of extracted paper text included in the system prompt.
    #[serde(default = "default_chat_max_context_chars")]
    pub max_context_chars: usize,
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self {
            models: Vec::new(),
            max_context_chars: default_chat_max_context_chars(),
        }
    }
}

fn default_chat_max_context_chars() -> usize {
    60_000
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
        assert!(cfg.search.embedding.is_none());
    }

    #[test]
    fn loads_search_section_with_embedding_defaults() {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        write!(
            f,
            r#"
inbox_dir = "/data/inbox"
library_root = "/data/library"
database_url = "sqlite:/data/library.db"

[search]
index_dir = "~/idx"

[search.embedding]
api_key = "sk-test"
"#
        )
        .unwrap();
        let cfg = Config::load(f.path()).unwrap();
        // tilde expanded like inbox_dir/library_root
        assert!(!cfg.search.index_dir.starts_with("~"));
        let e = cfg.search.embedding.unwrap();
        assert_eq!(e.base_url, "https://api.openai.com/v1");
        assert_eq!(e.model, "text-embedding-3-small");
        assert_eq!(e.dims, 1536);
        assert_eq!(e.api_key.as_deref(), Some("sk-test"));
        assert_eq!(e.api_key_env, "OPENAI_API_KEY");
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

[daily.llm]
model = "gpt-4o-mini"
"#
        )
        .unwrap();
        let d = Config::load(f.path()).unwrap().daily.unwrap();
        assert_eq!(d.categories, vec!["cs.AI", "cs.LG"]);
        assert!(!d.include_cross_list);
        assert_eq!(d.max_papers, 20);
        assert_eq!(d.run_at, "09:00");
        assert_eq!(d.retention_days, 14);
        assert_eq!(d.llm.base_url, "https://api.openai.com/v1");
        assert_eq!(d.llm.model, "gpt-4o-mini");
        assert_eq!(d.llm.api_key, None);
        assert_eq!(d.llm.api_key_env, "OPENAI_API_KEY");
        assert_eq!(d.llm.language, "English");
    }

    #[test]
    fn chat_config_parses_models_with_defaults() {
        let cfg: Config = toml::from_str(
            r#"
inbox_dir     = "./inbox"
library_root  = "./library"
database_url  = "sqlite:./x.db"

[[chat.models]]
label = "GPT-5 Mini"
model = "gpt-5-mini"

[[chat.models]]
label    = "Local Qwen"
base_url = "http://localhost:11434/v1"
model    = "qwen3:32b"
"#,
        )
        .unwrap();
        assert_eq!(cfg.chat.models.len(), 2);
        assert_eq!(cfg.chat.models[0].base_url, "https://api.openai.com/v1");
        assert_eq!(cfg.chat.models[0].api_key_env, "OPENAI_API_KEY");
        assert_eq!(cfg.chat.models[1].model, "qwen3:32b");
        assert_eq!(cfg.chat.max_context_chars, 60_000);
    }

    #[test]
    fn chat_config_absent_means_disabled() {
        let cfg: Config = toml::from_str(
            r#"
inbox_dir     = "./inbox"
library_root  = "./library"
database_url  = "sqlite:./x.db"
"#,
        )
        .unwrap();
        assert!(cfg.chat.models.is_empty());
        assert_eq!(cfg.chat.max_context_chars, 60_000);
    }

    #[test]
    fn chat_model_key_resolution() {
        let m = ChatModelConfig {
            label: "x".into(),
            base_url: "http://localhost".into(),
            model: "m".into(),
            api_key: Some("sk-inline".into()),
            api_key_env: "XUEWEN_TEST_UNSET_ENV".into(),
        };
        assert_eq!(m.resolve_key().as_deref(), Some("sk-inline"));

        let keyless = ChatModelConfig { api_key: None, ..m };
        // Env var unset -> keyless entry (requests carry no Authorization).
        assert_eq!(keyless.resolve_key(), None);
    }
}
