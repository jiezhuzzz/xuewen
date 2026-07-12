//! Minimal OpenAI-compatible chat-completions client. One client, two
//! callers: the daily TL;DR uses blocking `complete` (with retries); the
//! paper chat uses SSE `stream` (no retry once streaming has begun).

use anyhow::{anyhow, Result};
use futures_util::{Stream, StreamExt};
use std::time::Duration;

const ATTEMPTS: u32 = 3;

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChatMessage {
    pub role: &'static str, // "system" | "user" | "assistant"
    pub content: String,
}

pub struct LlmClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
    api_key: Option<String>,
    reasoning_effort: Option<String>,
}

impl LlmClient {
    pub fn new(base_url: &str, model: &str, api_key: Option<String>) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(120))
                .build()
                .expect("building chat HTTP client"),
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
            .post(format!("{}/chat/completions", self.base_url))
            .json(body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        req
    }

    /// Blocking completion with retries — behavior moved verbatim from
    /// `daily::tldr::ChatClient::complete`.
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
        let mut delay = Duration::from_millis(500);
        let mut last_err = None;
        for attempt in 1..=ATTEMPTS {
            let req = self.request(&body);
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

    /// Stream assistant deltas from a `stream: true` completion. The HTTP
    /// error (non-2xx) is returned from this call; mid-stream failures come
    /// through as an `Err` item. Per-request timeout is longer than the
    /// client default because generation time counts against it.
    pub async fn stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<impl Stream<Item = Result<String>> + Send> {
        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
        });
        if let Some(effort) = &self.reasoning_effort {
            body["reasoning_effort"] = serde_json::json!(effort);
        }
        let resp = self
            .request(&body)
            .timeout(Duration::from_secs(600))
            .send()
            .await
            .map_err(|e| anyhow!("chat request failed: {e}"))?;
        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("chat completions failed: {status}: {text}"));
        }
        let mut bytes = resp.bytes_stream();
        Ok(async_stream::try_stream! {
            let mut buf: Vec<u8> = Vec::new();
            'read: while let Some(chunk) = bytes.next().await {
                let chunk = chunk.map_err(|e| anyhow!("stream read failed: {e}"))?;
                buf.extend_from_slice(&chunk);
                // SSE events end with a blank line.
                while let Some(pos) = find_double_newline(&buf) {
                    let event: Vec<u8> = buf.drain(..pos + 2).collect();
                    let event = String::from_utf8_lossy(&event).into_owned();
                    for line in event.lines() {
                        let Some(data) = line.strip_prefix("data:") else { continue };
                        let data = data.trim_start();
                        if data == "[DONE]" {
                            break 'read;
                        }
                        let v: serde_json::Value = serde_json::from_str(data)
                            .map_err(|e| anyhow!("bad stream payload: {e}"))?;
                        if let Some(err) = v.get("error") {
                            let msg = err
                                .get("message")
                                .and_then(|m| m.as_str())
                                .unwrap_or("unknown provider error");
                            Err(anyhow!("provider error: {msg}"))?;
                        }
                        if let Some(s) = v["choices"][0]["delta"]["content"].as_str() {
                            if !s.is_empty() {
                                yield s.to_string();
                            }
                        }
                    }
                }
            }
        })
    }
}

fn find_double_newline(buf: &[u8]) -> Option<usize> {
    buf.windows(2).position(|w| w == b"\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::StreamExt;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn stream_yields_deltas_until_done() {
        let server = MockServer::start().await;
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n\
                    data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                    data: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;

        let client = LlmClient::new(&server.uri(), "test-model", None);
        let stream = client
            .stream(&[ChatMessage {
                role: "user",
                content: "hi".into(),
            }])
            .await
            .unwrap();
        futures_util::pin_mut!(stream);
        let mut out = String::new();
        while let Some(item) = stream.next().await {
            out.push_str(&item.unwrap());
        }
        assert_eq!(out, "Hello");
    }

    #[tokio::test]
    async fn stream_sends_reasoning_effort_when_set() {
        let server = MockServer::start().await;
        // The mock only matches when the request body carries the effort, so a
        // missing/renamed field makes the request 404 and the stream error out.
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .and(body_string_contains("\"reasoning_effort\":\"high\""))
            .respond_with(ResponseTemplate::new(200).set_body_raw("data: [DONE]\n\n", "text/event-stream"))
            .mount(&server)
            .await;

        let client =
            LlmClient::new(&server.uri(), "test-model", None).with_reasoning_effort(Some("high".into()));
        let _stream = client
            .stream(&[ChatMessage {
                role: "user",
                content: "hi".into(),
            }])
            .await
            .expect("body with reasoning_effort must match the mock");
    }

    #[tokio::test]
    async fn stream_surfaces_http_errors() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(401).set_body_string("bad key"))
            .mount(&server)
            .await;

        let client = LlmClient::new(&server.uri(), "test-model", None);
        let err = client
            .stream(&[ChatMessage {
                role: "user",
                content: "hi".into(),
            }])
            .await
            .err()
            .expect("401 must fail");
        assert!(err.to_string().contains("401"), "got: {err}");
    }

    #[tokio::test]
    async fn stream_surfaces_mid_stream_error_frames() {
        let server = MockServer::start().await;
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"He\"}}]}\n\n\
                    data: {\"error\":{\"message\":\"rate limited\"}}\n\n";
        Mock::given(method("POST"))
            .and(path("/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;

        let client = LlmClient::new(&server.uri(), "test-model", None);
        let stream = client
            .stream(&[ChatMessage {
                role: "user",
                content: "hi".into(),
            }])
            .await
            .unwrap();
        futures_util::pin_mut!(stream);
        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first, "He");
        let second = stream.next().await.unwrap();
        let err = second.err().expect("error frame must surface as Err");
        assert!(err.to_string().contains("rate limited"), "got: {err}");
    }
}
