use anyhow::Result;
use async_trait::async_trait;

use crate::config::DeeplConfig;

pub struct DeeplTranslator {
    base_url: String,
    api_key_env: Option<String>,
}

impl DeeplTranslator {
    pub fn new(cfg: &DeeplConfig) -> Self {
        Self {
            base_url: cfg.plan.base_url().to_string(),
            api_key_env: cfg.api_key_env.clone(),
        }
    }
}

#[async_trait]
impl super::Translator for DeeplTranslator {
    async fn translate(&self, _text: &str, _target: &str) -> Result<(String, Option<String>)> {
        let _ = (&self.base_url, &self.api_key_env);
        anyhow::bail!("DeepL translator not yet implemented")
    }
}
