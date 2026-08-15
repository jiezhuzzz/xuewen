//! Minimal OpenAI-compatible chat-completions client, shared by every
//! LLM-backed service — per-paper and daily-feed summaries, the citations
//! LLM fallback, and LLM translate — all through blocking `complete` (with
//! retries). The paper chat does not use it: that streams from the agent
//! sidecar (`src/agent`).

use crate::http::{HttpClient, RetryPolicy};
use anyhow::{anyhow, Result};
use std::time::Duration;

pub struct LlmClient {
    /// Shared retrying transport (`crate::http`) — one retry/backoff
    /// implementation for the whole crate, Retry-After included.
    http: HttpClient,
    base_url: String,
    model: String,
    api_key: Option<String>,
    reasoning_effort: Option<String>,
}

impl LlmClient {
    pub fn new(base_url: &str, model: &str, api_key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("building chat HTTP client");
        Self {
            http: HttpClient::new(client, RetryPolicy::llm()),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: api_key.filter(|k| !k.trim().is_empty()),
            reasoning_effort: None,
        }
    }

    /// Set the OpenAI `reasoning_effort` ("minimal" | "low" | "medium" |
    /// "high") sent with every request. `None`/empty omits the field, leaving
    /// the model's own default; endpoints that don't support it ignore it.
    pub fn with_reasoning_effort(mut self, effort: Option<String>) -> Self {
        self.reasoning_effort = effort.filter(|e| !e.trim().is_empty());
        self
    }

    /// The chat model id this client targets.
    pub fn model(&self) -> &str {
        &self.model
    }

    fn request(&self, body: &serde_json::Value) -> reqwest::RequestBuilder {
        let mut req = self
            .http
            .post(&format!("{}/chat/completions", self.base_url))
            .json(body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        req
    }

    /// Blocking completion; transient failures retry via the shared
    /// `crate::http` policy (`RetryPolicy::llm`).
    pub async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        });
        if let Some(effort) = &self.reasoning_effort {
            body["reasoning_effort"] = serde_json::json!(effort);
        }
        let text = self
            .http
            .send_text(self.request(&body))
            .await
            .map_err(|e| anyhow!("chat API: {e}"))?;
        let v: serde_json::Value = serde_json::from_str(&text)?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow!("chat API response has no message content"))?;
        Ok(content.trim().to_string())
    }
}

/// Strip one Markdown code fence (``` or ```json) that models sometimes wrap
/// around JSON replies. Shared by every module that parses LLM JSON output.
pub(crate) fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    let s = s
        .strip_prefix("```")
        .map(|rest| rest.strip_prefix("json").unwrap_or(rest))
        .unwrap_or(s);
    s.strip_suffix("```").unwrap_or(s).trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn complete_retries_on_429_then_succeeds() {
        // Pins the wiring to the shared retrying transport: one 429, then ok.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": " hi "}}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = LlmClient::new(&server.uri(), "test-model", None);
        assert_eq!(client.complete("s", "u").await.unwrap(), "hi");
    }

    #[tokio::test]
    async fn complete_sends_reasoning_effort_when_set() {
        let server = MockServer::start().await;
        // The mock only matches when the request body carries the effort, so a
        // missing/renamed field makes the request 404 and the call error out.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("\"reasoning_effort\":\"high\""))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": "ok"}}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let client = LlmClient::new(&server.uri(), "test-model", None)
            .with_reasoning_effort(Some("high".into()));
        assert_eq!(client.complete("s", "u").await.unwrap(), "ok");
    }
}
