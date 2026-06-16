use anyhow::{Context, Result};
use serde_json::json;
use std::time::Duration;

use super::{
    EnrichmentRequest, EnrichmentResponse, ItemEnrichmentRequest, ItemEnrichmentResponse,
    LlmProvider, inline_schema_for_openai, item_system_prompt, item_user_prompt, system_prompt,
    user_prompt,
};
use crate::config::LlmConfig;

pub struct OpenAiProvider {
    api_key: String,
    model: String,
    base_url: String,
    timeout: Duration,
}

impl OpenAiProvider {
    pub fn new(cfg: &LlmConfig) -> Self {
        OpenAiProvider {
            api_key: cfg.api_key.clone().unwrap_or_default(),
            model: cfg.model.clone(),
            base_url: cfg
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com".to_string()),
            timeout: Duration::from_secs(cfg.timeout_secs),
        }
    }
}

impl LlmProvider for OpenAiProvider {
    fn enrich(&self, req: &EnrichmentRequest) -> Result<EnrichmentResponse> {
        let schema = schemars::schema_for!(EnrichmentResponse);
        let schema_val = inline_schema_for_openai(serde_json::to_value(&schema)?);

        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt(req)},
                {"role": "user",   "content": user_prompt(req)}
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "EnrichmentResponse",
                    "strict": true,
                    "schema": schema_val
                }
            }
        });

        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()?;

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .context("OpenAI request failed")?;

        let resp_json: serde_json::Value = resp
            .error_for_status()
            .context("OpenAI returned an error")?
            .json()
            .context("OpenAI response was not valid JSON")?;

        let content = resp_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Unexpected OpenAI response shape"))?;

        let enrichment: EnrichmentResponse =
            serde_json::from_str(content).context("Could not parse OpenAI enrichment JSON")?;
        Ok(enrichment)
    }

    fn enrich_item(&self, req: &ItemEnrichmentRequest) -> Result<ItemEnrichmentResponse> {
        let schema = schemars::schema_for!(ItemEnrichmentResponse);
        let schema_val = inline_schema_for_openai(serde_json::to_value(&schema)?);

        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": item_system_prompt(req)},
                {"role": "user",   "content": item_user_prompt(req)}
            ],
            "response_format": {
                "type": "json_schema",
                "json_schema": {
                    "name": "ItemEnrichmentResponse",
                    "strict": true,
                    "schema": schema_val
                }
            }
        });

        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()?;

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .context("OpenAI request failed")?;

        let resp_json: serde_json::Value = resp
            .error_for_status()
            .context("OpenAI returned an error")?
            .json()
            .context("OpenAI response was not valid JSON")?;

        let content = resp_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Unexpected OpenAI response shape"))?;

        serde_json::from_str(content).context("Could not parse OpenAI item enrichment JSON")
    }

    fn chat(&self, system: &str, user: &str) -> Result<String> {
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user",   "content": user}
            ],
            "temperature": 0.3
        });

        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()?;

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = client
            .post(&url)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .context("OpenAI request failed")?;

        let resp_json: serde_json::Value = resp
            .error_for_status()
            .context("OpenAI returned an error")?
            .json()
            .context("OpenAI response was not valid JSON")?;

        resp_json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Unexpected OpenAI response shape"))
    }
}
