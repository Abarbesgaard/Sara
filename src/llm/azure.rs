use anyhow::{Context, Result};
use serde_json::json;
use std::time::Duration;

use super::{
    EnrichmentRequest, EnrichmentResponse, ItemEnrichmentRequest, ItemEnrichmentResponse,
    LlmProvider, inline_schema_for_openai, item_system_prompt, item_user_prompt, system_prompt,
    user_prompt,
};
use crate::config::LlmConfig;

/// Azure OpenAI provider.
/// Endpoint: https://{resource}.openai.azure.com/openai/deployments/{deployment}/chat/completions?api-version=...
pub struct AzureOpenAiProvider {
    api_key: String,
    /// e.g. "https://my-resource.openai.azure.com"
    base_url: String,
    /// deployment name (usually the same as your model alias, e.g. "gpt-4o")
    deployment: String,
    api_version: String,
    timeout: Duration,
}

impl AzureOpenAiProvider {
    pub fn new(cfg: &LlmConfig) -> Self {
        let resource = cfg
            .base_url
            .clone()
            .unwrap_or_else(|| "https://my-resource.openai.azure.com".to_string());

        // base_url can be either just the resource name ("vippsi-oai") or the full URL
        let base_url = if resource.starts_with("http") {
            resource
        } else {
            format!("https://{}.openai.azure.com", resource)
        };

        AzureOpenAiProvider {
            api_key: cfg.api_key.clone().unwrap_or_default(),
            base_url,
            deployment: cfg.model.clone(),
            api_version: "2024-08-01-preview".to_string(),
            timeout: Duration::from_secs(cfg.timeout_secs),
        }
    }
}

impl LlmProvider for AzureOpenAiProvider {
    fn enrich(&self, req: &EnrichmentRequest) -> Result<EnrichmentResponse> {
        let schema = schemars::schema_for!(EnrichmentResponse);
        let schema_val = inline_schema_for_openai(serde_json::to_value(&schema)?);

        let body = json!({
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

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            self.base_url, self.deployment, self.api_version
        );

        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()?;

        let resp = client
            .post(&url)
            .header("api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .context("Azure OpenAI request failed")?;

        let resp_json: serde_json::Value = resp
            .error_for_status()
            .context("Azure OpenAI returned an error")?
            .json()
            .context("Azure OpenAI response was not valid JSON")?;

        let content = resp_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| {
                anyhow::anyhow!("Unexpected Azure OpenAI response shape: {}", resp_json)
            })?;

        let enrichment: EnrichmentResponse = serde_json::from_str(content)
            .context("Could not parse Azure OpenAI enrichment JSON")?;
        Ok(enrichment)
    }

    fn enrich_item(&self, req: &ItemEnrichmentRequest) -> Result<ItemEnrichmentResponse> {
        let schema = schemars::schema_for!(ItemEnrichmentResponse);
        let schema_val = inline_schema_for_openai(serde_json::to_value(&schema)?);

        let body = json!({
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

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            self.base_url, self.deployment, self.api_version
        );

        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()?;

        let resp = client
            .post(&url)
            .header("api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .context("Azure OpenAI request failed")?;

        let resp_json: serde_json::Value = resp
            .error_for_status()
            .context("Azure OpenAI returned an error")?
            .json()
            .context("Azure OpenAI response was not valid JSON")?;

        let content = resp_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| {
                anyhow::anyhow!("Unexpected Azure OpenAI response shape: {}", resp_json)
            })?;

        serde_json::from_str(content).context("Could not parse Azure OpenAI item enrichment JSON")
    }

    fn chat(&self, system: &str, user: &str) -> Result<String> {
        let body = json!({
            "messages": [
                {"role": "system", "content": system},
                {"role": "user",   "content": user}
            ],
            "temperature": 0.3
        });

        let url = format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            self.base_url, self.deployment, self.api_version
        );

        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()?;

        let resp = client
            .post(&url)
            .header("api-key", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .context("Azure OpenAI request failed")?;

        let resp_json: serde_json::Value = resp
            .error_for_status()
            .context("Azure OpenAI returned an error")?
            .json()
            .context("Azure OpenAI response was not valid JSON")?;

        resp_json["choices"][0]["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Unexpected Azure OpenAI response shape: {}", resp_json))
    }
}
