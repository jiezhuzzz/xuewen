//! Shared structured-summary generation: an OpenAI-compatible chat call that
//! returns a five-part `Summary`. Used by the daily feed and by per-paper
//! library summaries (`SummaryService`).

use anyhow::Result;

use crate::config::{DailyLlmConfig, SummaryConfig};

/// Chars of extracted PDF text included in the full-text prompt.
pub const FULL_TEXT_CAP: usize = 40_000;

const SYSTEM: &str =
    "You summarize scientific papers accurately and concisely for a researcher's daily feed.";

/// Structured five-part paper summary produced by the LLM.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Summary {
    pub tldr: String,
    pub problem: String,
    pub approach: String,
    pub results: String,
    pub limitations: String,
}

/// Resolve an API key: inline value wins, else the named env var; empty ⇒ None.
fn resolve_key(explicit: Option<String>, api_key_env: &str) -> Option<String> {
    explicit
        .or_else(|| std::env::var(api_key_env).ok())
        .filter(|k| !k.trim().is_empty())
}

/// Config-agnostic summary chat client (was `daily::tldr::ChatClient`).
pub struct Summarizer {
    inner: crate::llm::LlmClient,
}

impl Summarizer {
    fn new(base_url: &str, model: &str, api_key: Option<String>) -> Self {
        Self { inner: crate::llm::LlmClient::new(base_url, model, api_key) }
    }

    /// Build from `[daily.llm]`. `None` when no API key resolves (warns).
    pub fn from_daily_llm(cfg: &DailyLlmConfig) -> Option<Self> {
        let Some(key) = resolve_key(cfg.api_key.clone(), &cfg.api_key_env) else {
            tracing::warn!(
                "[daily.llm] configured but no API key (set api_key or ${}) — daily papers disabled",
                cfg.api_key_env
            );
            return None;
        };
        Some(Self::new(&cfg.base_url, &cfg.model, Some(key)))
    }

    /// Build from `[summary]`. `None` when no API key resolves (warns).
    pub fn from_summary(cfg: &SummaryConfig) -> Option<Self> {
        let Some(key) = resolve_key(cfg.api_key.clone(), &cfg.api_key_env) else {
            tracing::warn!(
                "[summary] configured but no API key (set api_key or ${}) — library summaries disabled",
                cfg.api_key_env
            );
            return None;
        };
        Some(Self::new(&cfg.base_url, &cfg.model, Some(key)))
    }

    /// Keyless client pointed at a mock server. Test support only.
    pub fn for_tests(base_url: &str, model: &str) -> Self {
        Self::new(base_url, model, None)
    }

    pub async fn complete(&self, system: &str, user: &str) -> Result<String> {
        self.inner.complete(system, user).await
    }
}

fn prompt(language: &str, title: &str, abstract_text: &str, full_text: Option<&str>) -> String {
    let mut p = format!(
        "Summarize the following paper as a JSON object with exactly these string \
         keys: \"tldr\", \"problem\", \"approach\", \"results\", \"limitations\". \
         Write in {language}. Keep \"tldr\" to one sentence and every other field \
         to 1-2 sentences, about 120 words in total. Prefer concrete numbers in \
         \"results\" (benchmark, metric, delta over baseline). Base \"limitations\" \
         on the paper's own discussion when present. Output ONLY the JSON object.\n\n\
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

/// Parse the model's reply as a `Summary`, tolerating a Markdown code fence.
fn parse_summary(reply: &str) -> Result<Summary> {
    let mut s = reply.trim();
    if let Some(rest) = s.strip_prefix("```") {
        let rest = rest.strip_prefix("json").unwrap_or(rest);
        s = rest.strip_suffix("```").unwrap_or(rest).trim();
    }
    Ok(serde_json::from_str(s)?)
}

async fn summary_attempt(
    chat: &Summarizer,
    language: &str,
    title: &str,
    abstract_text: &str,
    full_text: Option<&str>,
) -> Result<Summary> {
    let reply = chat
        .complete(SYSTEM, &prompt(language, title, abstract_text, full_text))
        .await?;
    parse_summary(&reply)
}

/// Best-effort structured summary: full-text prompt, then abstract-only, then
/// `None`. A parse failure counts as a call failure. Never propagates an error.
pub async fn generate_summary(
    chat: &Summarizer,
    language: &str,
    title: &str,
    abstract_text: &str,
    full_text: Option<&str>,
) -> Option<Summary> {
    if full_text.is_some() {
        match summary_attempt(chat, language, title, abstract_text, full_text).await {
            Ok(s) => return Some(s),
            Err(e) => tracing::warn!("full-text summary failed for {title}: {e}"),
        }
    }
    match summary_attempt(chat, language, title, abstract_text, None).await {
        Ok(s) => Some(s),
        Err(e) => {
            tracing::warn!("abstract summary failed for {title}: {e}");
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
        let c = Summarizer::from_daily_llm(&cfg).unwrap();
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
        let c = Summarizer::for_tests(&format!("{}/v1", server.uri()), "m");
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
        let c = Summarizer::for_tests(&format!("{}/v1", server.uri()), "m");
        assert!(c.complete("s", "u").await.is_err());
    }

    #[test]
    fn from_daily_llm_without_key_is_none() {
        let cfg = crate::config::DailyLlmConfig {
            base_url: "https://api.openai.com/v1".into(),
            model: "m".into(),
            api_key: None,
            api_key_env: "XUEWEN_TEST_KEY_THAT_IS_NOT_SET".into(),
            language: "English".into(),
        };
        assert!(Summarizer::from_daily_llm(&cfg).is_none());
    }

    fn summary_json() -> serde_json::Value {
        json!({
            "tldr": "One line.",
            "problem": "Gap.",
            "approach": "Idea.",
            "results": "+4.2 on X.",
            "limitations": "Small data."
        })
    }

    #[test]
    fn parses_plain_and_fenced_summary_json() {
        let plain = summary_json().to_string();
        assert_eq!(parse_summary(&plain).unwrap().tldr, "One line.");
        let fenced = format!("```json\n{plain}\n```");
        assert_eq!(parse_summary(&fenced).unwrap().problem, "Gap.");
        let bare_fence = format!("```\n{plain}\n```");
        assert_eq!(parse_summary(&bare_fence).unwrap().approach, "Idea.");
        assert!(parse_summary("not json at all").is_err());
    }

    #[test]
    fn prompt_names_all_keys_and_language() {
        let p = prompt("German", "T", "A", None);
        for key in ["tldr", "problem", "approach", "results", "limitations"] {
            assert!(p.contains(&format!("\"{key}\"")), "missing key {key}");
        }
        assert!(p.contains("German"));
    }

    #[tokio::test]
    async fn summary_falls_back_from_full_text_to_abstract() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("Preview of main content"))
            .respond_with(ResponseTemplate::new(400))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(chat_response(&summary_json().to_string())),
            )
            .expect(1)
            .mount(&server)
            .await;
        let c = Summarizer::for_tests(&format!("{}/v1", server.uri()), "m");
        let out = generate_summary(&c, "English", "Title", "An abstract.", Some("full text")).await;
        assert_eq!(out.unwrap().tldr, "One line.");
    }

    #[tokio::test]
    async fn summary_unparsable_reply_falls_back_then_none() {
        // 200s with non-JSON content: parse failure on the full-text attempt,
        // parse failure again on the abstract-only attempt -> None.
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(chat_response("free text, no JSON")),
            )
            .expect(2)
            .mount(&server)
            .await;
        let c = Summarizer::for_tests(&format!("{}/v1", server.uri()), "m");
        let out = generate_summary(&c, "English", "T", "A", Some("full text")).await;
        assert!(out.is_none());
    }
}
