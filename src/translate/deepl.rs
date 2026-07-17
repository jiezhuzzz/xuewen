use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::Deserialize;

use crate::config::DeeplConfig;

pub struct DeeplTranslator {
    base_url: String,
    api_key_env: Option<String>,
    http: reqwest::Client,
}

impl DeeplTranslator {
    pub fn new(cfg: &DeeplConfig) -> Self {
        Self {
            base_url: cfg.plan.base_url().to_string(),
            api_key_env: cfg.api_key_env.clone(),
            http: reqwest::Client::new(),
        }
    }

    #[cfg(test)]
    pub fn set_base_url_for_test(&mut self, url: String) {
        self.base_url = url;
    }

    fn key(&self) -> Result<String> {
        let env = self.api_key_env.as_deref().unwrap_or("DEEPL_API_KEY");
        std::env::var(env)
            .ok()
            .filter(|k| !k.trim().is_empty())
            .ok_or_else(|| anyhow!("DeepL API key env `{env}` is not set"))
    }
}

/// DeepL uses uppercase language codes (e.g. ZH, EN, DE).
fn deepl_target(target: &str) -> String {
    target.to_ascii_uppercase()
}

#[derive(Deserialize)]
struct DeeplResp {
    translations: Vec<DeeplItem>,
}
#[derive(Deserialize)]
struct DeeplItem {
    text: String,
    #[serde(default)]
    detected_source_language: Option<String>,
}

#[async_trait]
impl super::Translator for DeeplTranslator {
    async fn translate(&self, text: &str, target: &str) -> Result<(String, Option<String>)> {
        let key = self.key()?;
        let url = format!("{}/v2/translate", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .header("Authorization", format!("DeepL-Auth-Key {key}"))
            .json(&serde_json::json!({
                "text": [text],
                "target_lang": deepl_target(target),
            }))
            .send()
            .await
            .context("DeepL request failed")?;
        if !resp.status().is_success() {
            let code = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(anyhow!("DeepL returned {code}: {body}"));
        }
        let parsed: DeeplResp = resp.json().await.context("DeepL response parse failed")?;
        let item = parsed
            .translations
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("DeepL returned no translations"))?;
        Ok((item.text, item.detected_source_language))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{DeeplConfig, DeeplPlan};
    use crate::translate::Translator;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn posts_to_deepl_and_parses_translation() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/translate"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "translations": [{ "detected_source_language": "EN", "text": "你好世界" }]
            })))
            .mount(&server)
            .await;

        std::env::set_var("DEEPL_KEY_TEST", "secret");
        let cfg = DeeplConfig {
            api_key_env: Some("DEEPL_KEY_TEST".into()),
            plan: DeeplPlan::Free,
        };
        let mut t = DeeplTranslator::new(&cfg);
        t.set_base_url_for_test(server.uri());

        let (text, src) = t.translate("hello world", "zh").await.unwrap();
        assert_eq!(text, "你好世界");
        assert_eq!(src.as_deref(), Some("EN"));
    }
}
