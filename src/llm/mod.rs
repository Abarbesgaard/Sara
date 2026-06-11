pub mod anthropic;
pub mod azure;
pub mod ollama;
pub mod openai;

use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct EnrichmentRequest {
    pub description: String,
    pub project_name: String,
    pub project_goal: Option<String>,
    pub project_stack: Option<String>,
    pub project_notes: Option<String>,
    /// Existing pending task descriptions for dep suggestion context
    pub existing_tasks: Vec<(String, String)>, // (uuid_short, description)
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct EnrichmentResponse {
    /// Suggested priority: "H", "M", "L", or null
    pub priority: Option<String>,
    /// Suggested due date as ISO 8601 or human-readable string, or null
    pub due: Option<String>,
    /// Suggested tags
    pub tags: Vec<String>,
    /// UUIDs (short prefix) of existing tasks this one should depend on
    pub suggested_dependencies: Vec<String>,
    /// Optional cleaned-up version of the description
    pub description_suggestion: Option<String>,
}

pub trait LlmProvider: Send + Sync {
    fn enrich(&self, req: &EnrichmentRequest) -> Result<EnrichmentResponse>;
}

pub fn build_provider(cfg: &Config) -> Box<dyn LlmProvider> {
    match cfg.llm.provider.as_str() {
        "openai" => Box::new(openai::OpenAiProvider::new(cfg)),
        "anthropic" => Box::new(anthropic::AnthropicProvider::new(cfg)),
        "azure" | "azure_openai" => Box::new(azure::AzureOpenAiProvider::new(cfg)),
        _ => Box::new(ollama::OllamaProvider::new(cfg)),
    }
}

/// Build the system prompt that is shared across all providers.
pub fn system_prompt(req: &EnrichmentRequest) -> String {
    let mut parts = vec![
        "You are a task-management assistant. Analyze the task and return a JSON object with enrichment suggestions.".to_string(),
        format!("Project: {}", req.project_name),
    ];
    if let Some(ref goal) = req.project_goal {
        parts.push(format!("Project goal: {goal}"));
    }
    if let Some(ref stack) = req.project_stack {
        parts.push(format!("Tech stack: {stack}"));
    }
    if let Some(ref notes) = req.project_notes {
        parts.push(format!("Notes/conventions: {notes}"));
    }
    if !req.existing_tasks.is_empty() {
        let list = req
            .existing_tasks
            .iter()
            .map(|(id, desc)| format!("  {id}: {desc}"))
            .collect::<Vec<_>>()
            .join("\n");
        parts.push(format!(
            "Existing pending tasks (id: description):\n{list}"
        ));
    }
    parts.join("\n\n")
}

/// Build the user prompt.
pub fn user_prompt(req: &EnrichmentRequest) -> String {
    format!(
        "Task: \"{}\"\n\n\
         Respond with a JSON object matching the schema. \
         Use null for unknown fields. \
         Only suggest dependencies from the existing tasks list above.",
        req.description
    )
}

/// Post-process schema for OpenAI strict mode:
/// - Flatten $defs/$ref
/// - Make all properties required
/// - Disallow additional properties
pub fn inline_schema_for_openai(mut schema: serde_json::Value) -> serde_json::Value {
    // Remove $schema, $defs
    if let Some(obj) = schema.as_object_mut() {
        obj.remove("$schema");
        obj.remove("$defs");
        // Ensure additionalProperties false
        obj.insert(
            "additionalProperties".to_string(),
            serde_json::Value::Bool(false),
        );
        // Make all properties required
        if let Some(props) = obj.get("properties") {
            if let Some(keys) = props.as_object().map(|m| {
                m.keys()
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect::<Vec<_>>()
            }) {
                obj.insert("required".to_string(), serde_json::Value::Array(keys));
            }
        }
    }
    schema
}
