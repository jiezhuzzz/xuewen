pub mod store;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use std::sync::Arc;

use crate::config::Config;

/// One bibliography entry parsed to fields. Field names are the JSON wire
/// format shared with the frontend (`StructuredReference` in types.ts).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StructuredReference {
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub venue: Option<String>,
    #[serde(default)]
    pub year: Option<i64>,
    #[serde(default)]
    pub doi: Option<String>,
    #[serde(default)]
    pub arxiv_id: Option<String>,
    #[serde(default)]
    pub url: Option<String>,
}

pub struct CitationsService {
    pool: SqlitePool,
    llm: crate::llm::LlmClient,
}

const SYSTEM: &str = "You convert bibliography entries from research papers into structured JSON. \
Output ONLY a JSON array — no prose, no markdown fences.";

impl CitationsService {
    /// `None` when `[ai.citations]` is absent (silently) or present but
    /// missing a model/API key (warns) — mirrors SummaryService.
    pub fn from_config(pool: SqlitePool, cfg: &Config) -> Option<Arc<Self>> {
        let use_ = cfg.ai.citations.as_ref()?;
        let r = cfg.ai.resolve(use_);
        let (Some(model), Some(key)) = (r.model.clone(), r.api_key.clone()) else {
            tracing::warn!("[ai.citations] has no model or API key — citation parsing disabled");
            return None;
        };
        let llm = crate::llm::LlmClient::new(&r.base_url, &model, Some(key))
            .with_reasoning_effort(r.reasoning_effort.clone());
        Some(Arc::new(Self { pool, llm }))
    }

    /// Keyless client pointed at a mock server. Test support only.
    pub fn for_tests(pool: SqlitePool, base_url: &str, model: &str) -> Arc<Self> {
        Arc::new(Self {
            pool,
            llm: crate::llm::LlmClient::new(base_url, model, None),
        })
    }

    /// Parse `refs` for `paper_id`: exact-input cache hit, or one batched LLM
    /// call whose result is cached. The result is index-aligned with `refs`.
    pub async fn parse(
        &self,
        paper_id: &str,
        refs: &[String],
    ) -> Result<Vec<Option<StructuredReference>>> {
        let refs_json = serde_json::to_string(refs)?;
        if let Some(cached) = store::get(&self.pool, paper_id, &refs_json).await? {
            return Ok(serde_json::from_str(&cached)?);
        }
        let numbered: String = refs
            .iter()
            .enumerate()
            .map(|(i, r)| format!("{}. {}\n", i + 1, r))
            .collect();
        let user = format!(
            "Parse each bibliography entry below into an object \
             {{\"authors\":[\"Given Family\",...],\"title\":string|null,\"venue\":string|null,\
             \"year\":int|null,\"doi\":string|null,\"arxiv_id\":string|null,\"url\":string|null}}.\n\
             venue is the conference/journal name only. doi is the bare DOI (no https://). \
             arxiv_id is like \"1412.6980\".\n\
             Return a JSON array with EXACTLY {} elements, one per entry in order. \
             Use null for an entry that is not a parseable bibliography reference.\n\n{}",
            refs.len(),
            numbered
        );
        let text = self.llm.complete(SYSTEM, &user).await?;
        let cleaned = text
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let parsed: Vec<Option<StructuredReference>> = serde_json::from_str(cleaned)
            .map_err(|e| anyhow!("citation parse: model returned invalid JSON: {e}"))?;
        if parsed.len() != refs.len() {
            return Err(anyhow!(
                "citation parse: expected {} entries, got {}",
                refs.len(),
                parsed.len()
            ));
        }
        store::upsert(
            &self.pool,
            paper_id,
            &refs_json,
            &serde_json::to_string(&parsed)?,
            self.llm.model(),
        )
        .await?;
        Ok(parsed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_string_contains, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn chat_reply(text: &str) -> serde_json::Value {
        json!({"choices": [{"message": {"role": "assistant", "content": text}}]})
    }

    const PARSED: &str = r#"[{"authors":["D. Kingma","J. Ba"],"title":"Adam: A Method for Stochastic Optimization","venue":"ICLR","year":2015,"doi":null,"arxiv_id":"1412.6980","url":null}, null]"#;

    #[tokio::test]
    async fn parses_via_llm_then_serves_from_cache() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("Kingma"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(PARSED)))
            .expect(1) // second parse() must hit the cache
            .mount(&server)
            .await;
        let pool = crate::citations::store::tests_pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool, &format!("{}/v1", server.uri()), "m");
        let refs = vec![
            "[1] D. Kingma, J. Ba. Adam...".to_string(),
            "garbage".to_string(),
        ];

        let out = svc.parse("p1", &refs).await.unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].as_ref().unwrap().year, Some(2015));
        assert!(out[1].is_none());

        let again = svc.parse("p1", &refs).await.unwrap(); // no second LLM call (mock .expect(1))
        assert_eq!(again, out);
    }

    #[tokio::test]
    async fn malformed_llm_output_errors_and_caches_nothing() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply("not json")))
            .mount(&server)
            .await;
        let pool = crate::citations::store::tests_pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool.clone(), &format!("{}/v1", server.uri()), "m");
        assert!(svc.parse("p1", &["x".to_string()]).await.is_err());
        assert!(store::get(&pool, "p1", r#"["x"]"#).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn wrong_length_array_is_an_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply("[null]")))
            .mount(&server)
            .await;
        let pool = crate::citations::store::tests_pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool, &format!("{}/v1", server.uri()), "m");
        assert!(svc
            .parse("p1", &["a".to_string(), "b".to_string()])
            .await
            .is_err());
    }
}
