use anyhow::{anyhow, Result};
use std::time::Duration;

use crate::config::DailyLlmConfig;

const ATTEMPTS: u32 = 3;
/// Chars of extracted PDF text included in the full-text prompt.
pub const FULL_TEXT_CAP: usize = 40_000;

const SYSTEM: &str =
    "You summarize scientific papers accurately and concisely for a researcher's daily feed.";

/// Minimal OpenAI-compatible /chat/completions client. Retry behavior
/// mirrors `search::embedder::Embedder` (429/5xx/network, backoff).
pub struct ChatClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
}

impl ChatClient {
    /// `None` when no API key is resolvable — the daily feature is then
    /// disabled, but nothing fails.
    pub fn from_config(cfg: &DailyLlmConfig) -> Option<Self> {
        let key = cfg
            .api_key
            .clone()
            .or_else(|| std::env::var(&cfg.api_key_env).ok())
            .filter(|k| !k.trim().is_empty());
        let Some(key) = key else {
            tracing::warn!(
                "[daily.llm] configured but no API key (set api_key or ${}) — daily papers disabled",
                cfg.api_key_env
            );
            return None;
        };
        Some(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("building chat HTTP client"),
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            model: cfg.model.clone(),
            api_key: Some(key),
        })
    }

    /// Keyless client pointed at a mock server. Test support only.
    pub fn for_tests(base_url: &str, model: &str) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            api_key: None,
        }
    }

    pub async fn complete(&self, system: &str, user: &str) -> Result<String> {
        let url = format!("{}/chat/completions", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
        });
        let mut delay = Duration::from_millis(500);
        let mut last_err = None;
        for attempt in 1..=ATTEMPTS {
            let mut req = self.http.post(&url).json(&body);
            if let Some(k) = &self.api_key {
                req = req.bearer_auth(k);
            }
            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    let v: serde_json::Value = resp.json().await?;
                    let text = v["choices"][0]["message"]["content"]
                        .as_str()
                        .ok_or_else(|| anyhow!("chat API response has no message content"))?;
                    return Ok(text.trim().to_string());
                }
                Ok(resp) => {
                    let status = resp.status();
                    let retriable = status.as_u16() == 429 || status.is_server_error();
                    let text = resp.text().await.unwrap_or_default();
                    let err = anyhow!(
                        "chat API {status}: {}",
                        text.chars().take(200).collect::<String>()
                    );
                    if !retriable || attempt == ATTEMPTS {
                        return Err(err);
                    }
                    last_err = Some(err);
                }
                Err(e) => {
                    if attempt == ATTEMPTS {
                        return Err(e.into());
                    }
                    last_err = Some(e.into());
                }
            }
            tokio::time::sleep(delay).await;
            delay *= 2;
        }
        Err(last_err.expect("loop ran at least once"))
    }
}

fn prompt(language: &str, title: &str, abstract_text: &str, full_text: Option<&str>) -> String {
    let mut p = format!(
        "Given the following information about a paper, write a 2-3 sentence TL;DR in \
         {language}: the problem, the approach, and the key result. Output only the TL;DR.\n\n\
         Title: {title}\n\nAbstract: {abstract_text}\n"
    );
    if let Some(t) = full_text {
        let capped: String = t.chars().take(FULL_TEXT_CAP).collect();
        p.push_str("\nPreview of main content:\n");
        p.push_str(&capped);
        p.push('\n');
    }
    p
}

/// Best-effort TL;DR: full-text prompt, then abstract-only, then `None`.
/// Never propagates an error — a bad paper must not fail the batch.
pub async fn generate_tldr(
    chat: &ChatClient,
    language: &str,
    title: &str,
    abstract_text: &str,
    full_text: Option<&str>,
) -> Option<String> {
    if full_text.is_some() {
        match chat
            .complete(SYSTEM, &prompt(language, title, abstract_text, full_text))
            .await
        {
            Ok(t) => return Some(t),
            Err(e) => tracing::warn!("full-text TL;DR failed for {title}: {e}"),
        }
    }
    match chat
        .complete(SYSTEM, &prompt(language, title, abstract_text, None))
        .await
    {
        Ok(t) => Some(t),
        Err(e) => {
            tracing::warn!("abstract TL;DR failed for {title}: {e}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, body_string_contains, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn chat_response(text: &str) -> serde_json::Value {
        json!({"choices": [{"message": {"role": "assistant", "content": text}}]})
    }

    #[tokio::test]
    async fn complete_sends_model_messages_and_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer sk-test"))
            .and(body_partial_json(json!({"model": "gpt-4o-mini"})))
            .and(body_string_contains("hello user"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_response("  hi  ")))
            .expect(1)
            .mount(&server)
            .await;

        let cfg = crate::config::DailyLlmConfig {
            base_url: format!("{}/v1", server.uri()),
            model: "gpt-4o-mini".into(),
            api_key: Some("sk-test".into()),
            api_key_env: "UNSET_VAR_FOR_TEST".into(),
            language: "English".into(),
        };
        let c = ChatClient::from_config(&cfg).unwrap();
        assert_eq!(c.complete("sys", "hello user").await.unwrap(), "hi");
    }

    #[tokio::test]
    async fn complete_retries_429_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_response("ok")))
            .expect(1)
            .mount(&server)
            .await;
        let c = ChatClient::for_tests(&format!("{}/v1", server.uri()), "m");
        assert_eq!(c.complete("s", "u").await.unwrap(), "ok");
    }

    #[tokio::test]
    async fn complete_does_not_retry_400() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400).set_body_string("bad request"))
            .expect(1)
            .mount(&server)
            .await;
        let c = ChatClient::for_tests(&format!("{}/v1", server.uri()), "m");
        assert!(c.complete("s", "u").await.is_err());
    }

    #[tokio::test]
    async fn tldr_falls_back_from_full_text_to_abstract() {
        let server = MockServer::start().await;
        // Full-text prompts fail non-retriably…
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("Preview of main content"))
            .respond_with(ResponseTemplate::new(400))
            .expect(1)
            .mount(&server)
            .await;
        // …the abstract-only prompt succeeds.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_response("Short TLDR.")))
            .expect(1)
            .mount(&server)
            .await;
        let c = ChatClient::for_tests(&format!("{}/v1", server.uri()), "m");
        let out = generate_tldr(&c, "English", "Title", "An abstract.", Some("full text")).await;
        assert_eq!(out.as_deref(), Some("Short TLDR."));
    }

    #[tokio::test]
    async fn tldr_gives_none_when_all_prompts_fail() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(400))
            .expect(2) // full-text, then abstract-only
            .mount(&server)
            .await;
        let c = ChatClient::for_tests(&format!("{}/v1", server.uri()), "m");
        let out = generate_tldr(&c, "English", "Title", "An abstract.", Some("full text")).await;
        assert!(out.is_none());
    }

    #[test]
    fn from_config_without_key_is_none() {
        let cfg = crate::config::DailyLlmConfig {
            base_url: "https://api.openai.com/v1".into(),
            model: "m".into(),
            api_key: None,
            api_key_env: "XUEWEN_TEST_KEY_THAT_IS_NOT_SET".into(),
            language: "English".into(),
        };
        assert!(ChatClient::from_config(&cfg).is_none());
    }
}
