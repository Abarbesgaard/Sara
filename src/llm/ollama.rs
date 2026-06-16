use anyhow::{Context, Result};
use serde_json::json;
use std::time::Duration;

use super::{EnrichmentRequest, EnrichmentResponse, ItemEnrichmentRequest, LlmProvider, system_prompt, user_prompt};
use crate::config::LlmConfig;

pub struct OllamaProvider {
    base_url: String,
    model: String,
    timeout: Duration,
}

impl OllamaProvider {
    pub fn new(cfg: &LlmConfig) -> Self {
        OllamaProvider {
            base_url: cfg
                .base_url
                .clone()
                .unwrap_or_else(|| "http://localhost:11434".to_string()),
            model: cfg.model.clone(),
            timeout: Duration::from_secs(cfg.timeout_secs),
        }
    }
}

impl LlmProvider for OllamaProvider {
    fn enrich(&self, req: &EnrichmentRequest) -> Result<EnrichmentResponse> {
        let schema = schemars::schema_for!(EnrichmentResponse);
        let schema_val = serde_json::to_value(&schema)?;

        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system_prompt(req)},
                {"role": "user",   "content": user_prompt(req)}
            ],
            "format": schema_val,
            "options": { "temperature": 0 },
            "stream": false
        });

        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()?;

        let url = format!("{}/api/chat", self.base_url);
        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .context("Ollama request failed — is Ollama running?")?;

        if resp.status() == 404 {
            anyhow::bail!(
                "Ollama model '{}' not found. Run: ollama pull {}",
                self.model, self.model
            );
        }

        let resp_json: serde_json::Value = resp
            .error_for_status()
            .context("Ollama returned an error")?
            .json()
            .context("Ollama response was not valid JSON")?;

        let content = resp_json["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Unexpected Ollama response shape"))?;

        let enrichment: EnrichmentResponse =
            serde_json::from_str(content).context("Could not parse Ollama enrichment JSON")?;
        Ok(enrichment)
    }

    fn enrich_item(&self, req: &ItemEnrichmentRequest) -> Result<super::ItemEnrichmentResponse> {
        use super::{item_system_prompt, item_user_prompt, ItemEnrichmentResponse};
        let schema = schemars::schema_for!(ItemEnrichmentResponse);
        let schema_val = serde_json::to_value(&schema)?;

        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": item_system_prompt(req)},
                {"role": "user",   "content": item_user_prompt(req)}
            ],
            "format": schema_val,
            "options": { "temperature": 0 },
            "stream": false
        });

        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()?;

        let url = format!("{}/api/chat", self.base_url);
        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .context("Ollama request failed — is Ollama running?")?;

        let resp_json: serde_json::Value = resp.error_for_status()?.json()?;
        let content = resp_json["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Unexpected Ollama response shape"))?;

        serde_json::from_str(content).context("Could not parse Ollama item enrichment JSON")
    }

    fn chat(&self, system: &str, user: &str) -> Result<String> {
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user",   "content": user}
            ],
            "options": { "temperature": 0.3 },
            "stream": false
        });

        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()?;

        let url = format!("{}/api/chat", self.base_url);
        let resp = client
            .post(&url)
            .json(&body)
            .send()
            .context("Ollama request failed — is Ollama running?")?;

        let resp_json: serde_json::Value = resp.error_for_status()?.json()?;
        resp_json["message"]["content"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow::anyhow!("Unexpected Ollama response shape"))
    }
}
