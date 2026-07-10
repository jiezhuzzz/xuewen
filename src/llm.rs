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
        }
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
        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
        });
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
    use wiremock::matchers::{method, path};
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
}
