//! Paper chat: per-paper LLM conversations grounded in the paper's text.

pub mod context;
pub mod store;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::config::{ChatConfig, ChatModelConfig};
use crate::models::Paper;

/// The configured chat feature: the model list plus a process-lifetime cache
/// of extracted paper text (extraction spawns pdftotext; repeat turns and
/// concurrent requests must not re-run it).
pub struct ChatService {
    pub models: Vec<ChatModelConfig>,
    pub max_context_chars: usize,
    text_cache: Mutex<HashMap<String, Arc<String>>>,
}

impl ChatService {
    /// `None` when no models are configured — chat is then disabled and the
    /// API answers 503 / `available: false`.
    pub fn from_config(cfg: &ChatConfig) -> Option<Arc<Self>> {
        if cfg.models.is_empty() {
            return None;
        }
        Some(Arc::new(Self {
            models: cfg.models.clone(),
            max_context_chars: cfg.max_context_chars,
            text_cache: Mutex::new(HashMap::new()),
        }))
    }

    /// The paper's extracted full text, cached. `None` when extraction fails
    /// — the chat then runs on metadata alone (see `context::system_prompt`).
    pub async fn paper_text(&self, library_root: &Path, paper: &Paper) -> Option<Arc<String>> {
        if let Some(t) = self.text_cache.lock().await.get(&paper.id) {
            return Some(t.clone());
        }
        let path = library_root.join(&paper.rel_path);
        let text = tokio::task::spawn_blocking(move || crate::pdf::extract_text_all(&path))
            .await
            .ok()?
            .ok()?;
        let text = Arc::new(text);
        self.text_cache
            .lock()
            .await
            .insert(paper.id.clone(), text.clone());
        Some(text)
    }
}
