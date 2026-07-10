use anyhow::{anyhow, bail, Result};
use serde::Deserialize;
use std::time::Duration;

use crate::config::EmbeddingConfig;

const BATCH: usize = 64;
const ATTEMPTS: u32 = 3;

/// Client for an OpenAI-compatible `/embeddings` endpoint.
pub struct Embedder {
    http: reqwest::Client,
    base_url: String,
    model: String,
    dims: usize,
    api_key: Option<String>,
}

impl Embedder {
    /// `None` when no API key is resolvable — semantic search is then
    /// unavailable, but nothing fails.
    pub fn from_config(cfg: &EmbeddingConfig) -> Option<Self> {
        let key = cfg
            .api_key
            .clone()
            .or_else(|| std::env::var(&cfg.api_key_env).ok())
            .filter(|k| !k.trim().is_empty());
        let Some(key) = key else {
            tracing::warn!(
                "[search.embedding] configured but no API key (set api_key or ${})  — semantic search disabled",
                cfg.api_key_env
            );
            return None;
        };
        Some(Self {
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(60))
                .build()
                .expect("building embedding HTTP client"),
            base_url: cfg.base_url.trim_end_matches('/').to_string(),
            model: cfg.model.clone(),
            dims: cfg.dims,
            api_key: Some(key),
        })
    }

    /// Keyless client pointed at a mock server. Test support only.
    pub fn for_tests(base_url: &str, model: &str, dims: usize) -> Self {
        Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            model: model.to_string(),
            dims,
            api_key: None,
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub fn dims(&self) -> usize {
        self.dims
    }

    pub async fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for batch in texts.chunks(BATCH) {
            out.extend(self.embed_batch(batch).await?);
        }
        Ok(out)
    }

    async fn embed_batch(&self, batch: &[String]) -> Result<Vec<Vec<f32>>> {
        #[derive(Deserialize)]
        struct Item {
            index: usize,
            embedding: Vec<f32>,
        }
        #[derive(Deserialize)]
        struct Body {
            data: Vec<Item>,
        }

        let url = format!("{}/embeddings", self.base_url);
        let mut delay = Duration::from_millis(500);
        let mut last_err = None;
        for attempt in 1..=ATTEMPTS {
            let mut req = self
                .http
                .post(&url)
                .json(&serde_json::json!({ "model": self.model, "input": batch }));
            if let Some(k) = &self.api_key {
                req = req.bearer_auth(k);
            }
            match req.send().await {
                Ok(resp) if resp.status().is_success() => {
                    let mut body: Body = resp.json().await?;
                    if body.data.len() != batch.len() {
                        bail!(
                            "embedding API returned {} vectors for {} inputs",
                            body.data.len(),
                            batch.len()
                        );
                    }
                    body.data.sort_by_key(|d| d.index);
                    for d in &body.data {
                        if d.embedding.len() != self.dims {
                            bail!(
                                "embedding dims mismatch: API returned {}, config says {} — fix [search.embedding].dims",
                                d.embedding.len(),
                                self.dims
                            );
                        }
                    }
                    return Ok(body.data.into_iter().map(|d| d.embedding).collect());
                }
                Ok(resp) => {
                    let status = resp.status();
                    let retriable = status.as_u16() == 429 || status.is_server_error();
                    let text = resp.text().await.unwrap_or_default();
                    let err = anyhow!("embedding API {status}: {}", text.chars().take(200).collect::<String>());
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, Request, ResponseTemplate};

    fn embedding_response(n: usize, dims: usize) -> serde_json::Value {
        let data: Vec<_> = (0..n)
            .map(|i| json!({"index": i, "embedding": vec![0.1f32; dims]}))
            .collect();
        json!({"data": data})
    }

    #[tokio::test]
    async fn embeds_with_bearer_auth_and_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(header("authorization", "Bearer sk-test"))
            .and(body_partial_json(json!({"model": "text-embedding-3-small"})))
            .respond_with(ResponseTemplate::new(200).set_body_json(embedding_response(2, 4)))
            .expect(1)
            .mount(&server)
            .await;

        let cfg = crate::config::EmbeddingConfig {
            base_url: format!("{}/v1", server.uri()),
            model: "text-embedding-3-small".into(),
            dims: 4,
            api_key: Some("sk-test".into()),
            api_key_env: "UNSET_VAR_FOR_TEST".into(),
        };
        let e = Embedder::from_config(&cfg).unwrap();
        let out = e.embed(&["a".into(), "b".into()]).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].len(), 4);
    }

    #[tokio::test]
    async fn batches_requests_of_64() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(move |req: &Request| {
                let body: serde_json::Value = serde_json::from_slice(&req.body).unwrap();
                let n = body["input"].as_array().unwrap().len();
                assert!(n <= 64, "batch too large: {n}");
                ResponseTemplate::new(200).set_body_json(embedding_response(n, 4))
            })
            .expect(2) // 100 texts -> 64 + 36
            .mount(&server)
            .await;

        let e = Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let texts: Vec<String> = (0..100).map(|i| format!("t{i}")).collect();
        let out = e.embed(&texts).await.unwrap();
        assert_eq!(out.len(), 100);
    }

    #[tokio::test]
    async fn retries_on_429_then_succeeds() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(429))
            .up_to_n_times(1)
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(embedding_response(1, 4)))
            .expect(1)
            .mount(&server)
            .await;

        let e = Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let out = e.embed(&["a".into()]).await.unwrap();
        assert_eq!(out.len(), 1);
    }

    #[tokio::test]
    async fn wrong_dims_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(embedding_response(1, 3)))
            .mount(&server)
            .await;
        let e = Embedder::for_tests(&format!("{}/v1", server.uri()), "m", 4);
        let err = e.embed(&["a".into()]).await.unwrap_err().to_string();
        assert!(err.contains("dims"), "got: {err}");
    }

    #[test]
    fn from_config_without_key_is_none() {
        let cfg = crate::config::EmbeddingConfig {
            base_url: "https://api.openai.com/v1".into(),
            model: "m".into(),
            dims: 4,
            api_key: None,
            api_key_env: "XUEWEN_TEST_KEY_THAT_IS_NOT_SET".into(),
        };
        assert!(Embedder::from_config(&cfg).is_none());
    }
}
