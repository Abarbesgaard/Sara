use anyhow::{Context, Result};
use serde_json::json;
use std::time::Duration;

use super::{EnrichmentRequest, EnrichmentResponse, LlmProvider, system_prompt, user_prompt};
use crate::config::LlmConfig;

pub struct MlxProvider {
    model: String,
    base_url: String,
    timeout: Duration,
}

impl MlxProvider {
    pub fn new(cfg: &LlmConfig) -> Self {
        MlxProvider {
            model: cfg.model.clone(),
            base_url: cfg
                .base_url
                .clone()
                .unwrap_or_else(|| "http://localhost:8080".to_string()),
            timeout: Duration::from_secs(cfg.timeout_secs),
        }
    }
}

/// Build a system prompt that includes the JSON schema inline, so models that
/// don't support OpenAI structured-output mode still return the right shape.
fn mlx_system_prompt(req: &EnrichmentRequest) -> String {
    let schema = r#"{
  "priority": "H" | "M" | "L" | null,
  "due": "<ISO date or relative string>" | null,
  "tags": ["<tag>"],
  "suggested_dependencies": ["<task-id-prefix>"],
  "description_suggestion": "<cleaned description>" | null,
  "rationale": "<why this task exists>" | null,
  "steps": ["<ordered implementation step>"],
  "acceptance_criteria": ["<verifiable done condition>"],
  "findings": ["<relevant codebase fact>"],
  "constraints": ["<hard constraint>"],
  "non_goals": ["<out of scope>"],
  "assumptions": ["<assumption made>"],
  "open_questions": ["<question for the human>"],
  "relevant_files": [{"path": "<repo path>", "reason": "<why>", "symbol": null, "line_start": null, "line_end": null}],
  "test_cmd": "<test command>" | null,
  "lint_cmd": "<lint command>" | null
}"#;
    format!(
        "{}\n\nRespond ONLY with a JSON object that exactly matches this schema — no extra text:\n{}",
        system_prompt(req),
        schema
    )
}

impl LlmProvider for MlxProvider {
    fn enrich(&self, req: &EnrichmentRequest) -> Result<EnrichmentResponse> {
        let body = json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": mlx_system_prompt(req)},
                {"role": "user",   "content": user_prompt(req)}
            ],
            "response_format": { "type": "json_object" },
            "temperature": 0
        });

        let client = reqwest::blocking::Client::builder()
            .timeout(self.timeout)
            .build()?;

        let url = format!("{}/v1/chat/completions", self.base_url);
        let resp = client.post(&url).json(&body).send().context(
            "mlx_lm.server request failed — is it running? (mlx_lm.server --model <model>)",
        )?;

        let status = resp.status();
        let resp_json: serde_json::Value = resp
            .json()
            .context("mlx_lm.server response was not valid JSON")?;

        if !status.is_success() {
            anyhow::bail!("mlx_lm.server returned {}: {}", status, resp_json);
        }

        let content = resp_json["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Unexpected mlx response shape: {}", resp_json))?;

        // Strip markdown code fences if the model wrapped its output
        let content = content.trim();
        let content = content
            .strip_prefix("```json")
            .or_else(|| content.strip_prefix("```"))
            .map(|s| s.trim_end_matches("```").trim())
            .unwrap_or(content);

        let enrichment: EnrichmentResponse =
            serde_json::from_str(content).context("Could not parse mlx enrichment JSON")?;
        Ok(enrichment)
    }
}
