//! Paper chat: per-paper LLM conversations grounded in the paper's text.

pub mod context;
pub mod store;

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::models::Paper;

/// A chat model resolved against the [ai] defaults, ready to serve.
#[derive(Debug, Clone)]
pub struct ChatModel {
    pub label: String,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub reasoning_effort: Option<String>,
}

/// The configured chat feature: the model list plus a process-lifetime cache
/// of extracted paper text (extraction spawns pdftotext; repeat turns and
/// concurrent requests must not re-run it).
pub struct ChatService {
    pub models: Vec<ChatModel>,
    pub max_context_chars: usize,
    text_cache: Mutex<HashMap<String, Arc<String>>>,
}

impl ChatService {
    /// `None` when no chat models are configured, or none resolve a model id.
    pub fn from_config(ai: &crate::config::AiConfig) -> Option<Arc<Self>> {
        let mut models = Vec::new();
        for m in &ai.chat.models {
            let r = ai.resolve(&m.endpoint);
            let Some(model) = r.model else {
                tracing::warn!("[[ai.chat.models]] '{}' has no model (set model here or in [ai]) — skipped", m.label);
                continue;
            };
            models.push(ChatModel {
                label: m.label.clone(), base_url: r.base_url, model,
                api_key: r.api_key, reasoning_effort: r.reasoning_effort,
            });
        }
        if models.is_empty() { return None; }
        Some(Arc::new(Self { models, max_context_chars: ai.chat.max_context_chars, text_cache: Mutex::new(HashMap::new()) }))
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
