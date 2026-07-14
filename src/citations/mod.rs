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
    /// `null_as_empty`: the model sometimes emits `"authors": null` for an
    /// entry it half-parsed; that must not fail the whole batch.
    #[serde(default, deserialize_with = "null_as_empty")]
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

fn null_as_empty<'de, D>(d: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v: Option<Vec<String>> = Option::deserialize(d)?;
    Ok(v.unwrap_or_default())
}

pub struct CitationsService {
    pool: SqlitePool,
    llm: crate::llm::LlmClient,
}

/// Max bibliography entries per LLM call. Large batches make the model drop
/// or merge entries (observed live: 111 in → 69 out; 68 → 52), which trips
/// the exact-length validation on every retry. Small chunks count reliably;
/// results are concatenated in order.
const CHUNK_SIZE: usize = 25;

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

    /// Parse `refs` for `paper_id`: exact-input cache hit, or LLM calls in
    /// `CHUNK_SIZE` batches whose concatenation is cached. The result is
    /// index-aligned with `refs`.
    pub async fn parse(
        &self,
        paper_id: &str,
        refs: &[String],
    ) -> Result<Vec<Option<StructuredReference>>> {
        let refs_json = serde_json::to_string(refs)?;
        if let Some(cached) = store::get(&self.pool, paper_id, &refs_json).await? {
            return Ok(serde_json::from_str(&cached)?);
        }
        let mut parsed = Vec::with_capacity(refs.len());
        for chunk in refs.chunks(CHUNK_SIZE) {
            parsed.extend(self.parse_chunk(chunk).await?);
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

    /// One LLM call for one chunk. Results are INDEX-KEYED, not positional:
    /// models reliably fail exact-count array contracts (observed live even
    /// at 25 entries: 18 in → 19 out, messy extracted strings read as two
    /// entries). Each returned object names its entry via `"i"`; entries the
    /// model skips stay null, duplicates keep the first, out-of-range are
    /// dropped — a miscount can no longer poison the whole chunk.
    async fn parse_chunk(&self, refs: &[String]) -> Result<Vec<Option<StructuredReference>>> {
        let numbered: String = refs
            .iter()
            .enumerate()
            .map(|(i, r)| format!("{}. {}\n", i + 1, r))
            .collect();
        let user = format!(
            "Parse the numbered bibliography entries below. Return a JSON array of \
             objects, one per entry you can parse. Each object MUST be \
             {{\"i\":<the entry's number exactly as shown below>,\
             \"authors\":[\"Given Family\",...],\"title\":string|null,\"venue\":string|null,\
             \"year\":int|null,\"doi\":string|null,\"arxiv_id\":string|null,\"url\":string|null}}.\n\
             venue is the conference/journal name only. doi is the bare DOI (no https://). \
             arxiv_id is like \"1412.6980\".\n\
             SKIP entries that are not parseable bibliography references — do not \
             invent placeholders for them.\n\n{numbered}"
        );
        let text = self.llm.complete(SYSTEM, &user).await?;
        let cleaned = text
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        let parsed: Vec<IndexedReference> = serde_json::from_str(cleaned)
            .map_err(|e| anyhow!("citation parse: model returned invalid JSON: {e}"))?;
        let mut out: Vec<Option<StructuredReference>> = vec![None; refs.len()];
        for item in parsed {
            if item.i >= 1 && item.i <= refs.len() && out[item.i - 1].is_none() {
                out[item.i - 1] = Some(item.reference);
            }
        }
        Ok(out)
    }
}

/// One element of the model's reply: a parsed entry tagged with the 1-based
/// chunk-local entry number it belongs to.
#[derive(Deserialize)]
struct IndexedReference {
    i: usize,
    #[serde(flatten)]
    reference: StructuredReference,
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

    const PARSED: &str = r#"[{"i":1,"authors":["D. Kingma","J. Ba"],"title":"Adam: A Method for Stochastic Optimization","venue":"ICLR","year":2015,"doi":null,"arxiv_id":"1412.6980","url":null}]"#;

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
    async fn chunks_large_batches_and_concatenates() {
        let server = MockServer::start().await;
        // 55 refs -> chunks of 25/25/5, one LLM call each. Chunks 2 and 3 are
        // recognized by their first entry; chunk 1 falls through to the
        // catch-all mounted last. Replies use chunk-local indexes; an entry
        // the model skips (or an empty reply) is simply null in the output.
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("REF-26 "))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(
                r#"[{"i":1,"title":"chunk2-first","authors":[],"venue":null,"year":null,"doi":null,"arxiv_id":null,"url":null}]"#,
            )))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(body_string_contains("REF-51 "))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply("[]")))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply("[]")))
            .expect(1)
            .mount(&server)
            .await;
        let refs: Vec<String> = (1..=55).map(|i| format!("REF-{i} some paper title")).collect();
        let pool = crate::citations::store::tests_pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool.clone(), &format!("{}/v1", server.uri()), "m");

        let out = svc.parse("p1", &refs).await.unwrap();
        assert_eq!(out.len(), 55);
        // chunk-local index 1 of chunk 2 = overall entry 26
        assert_eq!(out[25].as_ref().unwrap().title.as_deref(), Some("chunk2-first"));
        assert!(out[0].is_none());
        let refs_json = serde_json::to_string(&refs).unwrap();
        assert!(store::get(&pool, "p1", &refs_json).await.unwrap().is_some());
    }

    #[tokio::test]
    async fn tolerates_explicit_null_authors() {
        let server = MockServer::start().await;
        let reply = r#"[{"i":1,"authors":null,"title":"T","venue":null,"year":2020,"doi":null,"arxiv_id":null,"url":null}]"#;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(reply)))
            .mount(&server)
            .await;
        let pool = crate::citations::store::tests_pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool, &format!("{}/v1", server.uri()), "m");
        let out = svc.parse("p1", &["x".to_string()]).await.unwrap();
        assert_eq!(out[0].as_ref().unwrap().title.as_deref(), Some("T"));
        assert!(out[0].as_ref().unwrap().authors.is_empty());
    }

    #[tokio::test]
    async fn unindexed_entries_become_null_and_bad_indexes_are_dropped() {
        let server = MockServer::start().await;
        // Model omits entry 2, duplicates entry 1, and invents entry 99:
        // entry 1 = first occurrence wins; entry 2 = null; 99 dropped.
        let reply = r#"[
            {"i":1,"title":"First","authors":[],"venue":null,"year":null,"doi":null,"arxiv_id":null,"url":null},
            {"i":1,"title":"Dup","authors":[],"venue":null,"year":null,"doi":null,"arxiv_id":null,"url":null},
            {"i":99,"title":"Ghost","authors":[],"venue":null,"year":null,"doi":null,"arxiv_id":null,"url":null}
        ]"#;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(chat_reply(reply)))
            .mount(&server)
            .await;
        let pool = crate::citations::store::tests_pool_with_paper("p1").await;
        let svc = CitationsService::for_tests(pool, &format!("{}/v1", server.uri()), "m");
        let out = svc
            .parse("p1", &["a".to_string(), "b".to_string()])
            .await
            .unwrap();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].as_ref().unwrap().title.as_deref(), Some("First"));
        assert!(out[1].is_none());
    }
}
