pub mod anthropic;
pub mod azure;
pub mod mlx;
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct ItemEnrichmentResponse {
    /// One-line summary of the captured content
    pub summary: Option<String>,
    /// Suggested tags
    pub tags: Vec<String>,
    /// PARA destination: "1 Projects", "2 Areas", "3 Resources", or "Inbox"
    pub para_folder: Option<String>,
    /// Improved title if the original is weak
    pub title: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ItemEnrichmentRequest {
    pub kind: String,
    pub title: String,
    pub body: String,
    pub url: Option<String>,
    pub profile_context: Option<String>,
}

pub trait LlmProvider: Send + Sync {
    fn enrich(&self, req: &EnrichmentRequest) -> Result<EnrichmentResponse>;
    fn enrich_item(&self, req: &ItemEnrichmentRequest) -> Result<ItemEnrichmentResponse> {
        let _ = req;
        Ok(ItemEnrichmentResponse::default())
    }
    /// Free-form assistant reply (search, ask).
    fn chat(&self, system: &str, user: &str) -> Result<String> {
        let _ = (system, user);
        anyhow::bail!("Chat not supported for this LLM provider")
    }
}

pub fn build_provider(cfg: &Config) -> Box<dyn LlmProvider> {
    let llm = cfg.effective_llm();
    match llm.provider.as_str() {
        "openai" => Box::new(openai::OpenAiProvider::new(llm)),
        "mlx" => Box::new(mlx::MlxProvider::new(llm)),
        "anthropic" => Box::new(anthropic::AnthropicProvider::new(llm)),
        "azure" | "azure_openai" => Box::new(azure::AzureOpenAiProvider::new(llm)),
        _ => Box::new(ollama::OllamaProvider::new(llm)),
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

/// Build prompts for note/link capture enrichment.
pub fn item_system_prompt(req: &ItemEnrichmentRequest) -> String {
    let mut parts = vec![
        "You are Sara, a personal assistant. Analyze captured content and return JSON with enrichment.".to_string(),
        format!("Content type: {}", req.kind),
    ];
    if let Some(ref profile) = req.profile_context {
        parts.push(format!("User profile (adapt to their patterns):\n{profile}"));
    }
    parts.push(
        "Suggest para_folder as one of: \"1 Projects\", \"2 Areas\", \"3 Resources\", \"Inbox\".".to_string(),
    );
    parts.join("\n\n")
}

pub fn item_user_prompt(req: &ItemEnrichmentRequest) -> String {
    let mut s = format!("Title: {}\nContent:\n{}", req.title, req.body);
    if let Some(ref url) = req.url {
        s.push_str(&format!("\nURL: {url}"));
    }
    s.push_str("\n\nRespond with JSON matching the schema.");
    s
}

/// Build the user prompt for task enrichment.
pub fn user_prompt(req: &EnrichmentRequest) -> String {
    format!(
        "Task: \"{}\"\n\n\
         Respond with a JSON object matching the schema. \
         Use null for unknown fields. \
         Only suggest dependencies from the existing tasks list above.",
        req.description
    )
}

/// System prompt for `sara brief` — personal check-in, not a data dump.
pub fn brief_system_prompt(profile_context: Option<&str>) -> String {
    let mut parts = vec![
        "You are Sara, the user's personal CLI assistant. Write a brief check-in — warm, \
         direct, second person, like a thoughtful colleague who knows their context. \
         Use ONLY the facts provided. 2–4 short paragraphs max, then optionally one line \
         starting with → for a suggested next action. \
         Lead with what matters most (due today, current project). \
         Don't list raw urgency scores or numbered task dumps. \
         Don't say \"I'd be happy to help\" or other chatbot filler. \
         Don't invent facts not in the context.".to_string(),
    ];
    if let Some(profile) = profile_context {
        parts.push(format!("Background on the user:\n{profile}"));
    }
    parts.join("\n\n")
}

/// System prompt for search / ask — Sara answers from the user's store.
pub fn search_system_prompt(profile_context: Option<&str>) -> String {
    let mut parts = vec![
        "You are Sara, a personal CLI assistant built around memory. The user has a private \
         second brain: long-term MEMORY.md, daily notes, captured notes/links, and tasks. \
         Personal memory files are the highest-authority source — trust them over inference. \
         Answer using ONLY the context provided. Be concise and helpful. \
         Reference sources (MEMORY.md, l1, n2, task ids) when citing. \
         If context is insufficient, say so and suggest what to capture or remember. \
         When asked about project progress: if a 'Recorded project status' or project snapshot \
         says no milestones yet, state that clearly and summarize pending tasks — do not say \
         memory is empty when tasks or snapshots are present. Only suggest capturing milestones \
         if the user asks how to track progress.".to_string(),
    ];
    if let Some(profile) = profile_context {
        parts.push(format!("User profile:\n{profile}"));
    }
    parts.join("\n\n")
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
