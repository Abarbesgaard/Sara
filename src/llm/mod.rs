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
    /// Gitignore-aware repo file-tree summary, so suggestions reference real paths.
    pub repo_tree: Option<String>,
    /// Project build/test/lint/run commands, so verification is grounded.
    pub project_commands: Option<String>,
}

/// A code anchor the LLM thinks is relevant to the task.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
pub struct RelevantFile {
    /// Repo-relative path.
    pub path: String,
    /// Why this file/symbol matters for the task.
    pub reason: Option<String>,
    /// Specific symbol (function/type) to change, if known.
    pub symbol: Option<String>,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default)]
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
    /// Why this task exists / how it fits the project.
    pub rationale: Option<String>,
    /// Ordered implementation steps (checkmarks); each item is a step intent.
    pub steps: Vec<String>,
    /// Acceptance criteria / definition of done.
    pub acceptance_criteria: Vec<String>,
    /// Key findings about the codebase relevant to this task.
    pub findings: Vec<String>,
    /// Hard constraints the implementation must respect.
    pub constraints: Vec<String>,
    /// Explicit non-goals / out-of-scope items.
    pub non_goals: Vec<String>,
    /// Assumptions the plan makes.
    pub assumptions: Vec<String>,
    /// Open questions for the human to answer.
    pub open_questions: Vec<String>,
    /// Relevant files / code anchors.
    pub relevant_files: Vec<RelevantFile>,
    /// Command that verifies the task (tests).
    pub test_cmd: Option<String>,
    /// Lint command for the task.
    pub lint_cmd: Option<String>,
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
    /// Git-detected project when capturing from a repo
    pub current_project: Option<String>,
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
        "You are an AI software-engineering planner. Turn the task into a thorough, \
         self-contained implementation guide that another engineer (human or LLM) can execute \
         step by step. Return a JSON object matching the schema. Ground every file/anchor in the \
         repo tree below (use real paths), keep steps ordered and concrete, make acceptance \
         criteria verifiable, and surface assumptions/open questions instead of guessing."
            .to_string(),
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
    if let Some(ref cmds) = req.project_commands {
        parts.push(format!("Project commands:\n{cmds}"));
    }
    if let Some(ref tree) = req.repo_tree {
        parts.push(format!(
            "Repository file tree (suggest only real paths):\n{tree}"
        ));
    }
    if !req.existing_tasks.is_empty() {
        let list = req
            .existing_tasks
            .iter()
            .map(|(id, desc)| format!("  {id}: {desc}"))
            .collect::<Vec<_>>()
            .join("\n");
        parts.push(format!("Existing pending tasks (id: description):\n{list}"));
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
        parts.push(format!(
            "User profile (adapt to their patterns):\n{profile}"
        ));
    }
    if let Some(ref project) = req.current_project {
        parts.push(format!(
            "Current git project: {project}\n\
             Default PARA destination for active project material: 1 Projects/{project}\n\
             Use 3 Resources only for general reference not tied to this active project.\n\
             Use 2 Areas for ongoing responsibilities (health, finance, etc.).\n\
             Use Inbox when unsure."
        ));
    }
    parts.push(
        "Suggest para_folder as one of: \"1 Projects\", \"2 Areas\", \"3 Resources\", \"Inbox\". \
         Prefer 1 Projects when content supports active work in the current git project."
            .to_string(),
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
         Don't invent facts not in the context."
            .to_string(),
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
         Reference sources (MEMORY.md, l1, n2, task ids, PARA folder) when citing. \
         Notes and links are organized in PARA folders (Projects, Areas, Resources, Inbox) — \
         prefer project-scoped resources when the user asks about the current project. \
         If context is insufficient, say so and suggest what to capture or remember. \
         When asked about project progress: if a 'Recorded project status' or project snapshot \
         says no milestones yet, state that clearly and summarize pending tasks — do not say \
         memory is empty when tasks or snapshots are present. Only suggest capturing milestones \
         if the user asks how to track progress. \
         When the context includes a live pending tasks list, treat it as authoritative — \
         it reflects the current database and overrides older project snapshots in memory. \
         When recent notes & links are listed for a project, summarize those captures first \
         for \"what's recent\" questions — they are live and override stale memory snapshots."
            .to_string(),
    ];
    if let Some(profile) = profile_context {
        parts.push(format!("User profile:\n{profile}"));
    }
    parts.join("\n\n")
}

/// Post-process schema for OpenAI/Azure strict mode:
/// - Inline every `$ref` against `$defs`/`definitions` (recursively)
/// - Make all object properties `required`
/// - Disallow `additionalProperties` on every object
///
/// This handles nested objects (e.g. `relevant_files: Vec<RelevantFile>`),
/// which schemars emits as `$ref` into `$defs`.
pub fn inline_schema_for_openai(schema: serde_json::Value) -> serde_json::Value {
    let defs = schema
        .get("$defs")
        .or_else(|| schema.get("definitions"))
        .cloned();
    let mut root = schema;
    if let Some(obj) = root.as_object_mut() {
        obj.remove("$schema");
        obj.remove("$defs");
        obj.remove("definitions");
    }
    resolve_strict(root, defs.as_ref())
}

fn resolve_strict(node: serde_json::Value, defs: Option<&serde_json::Value>) -> serde_json::Value {
    use serde_json::Value;

    // Resolve a `$ref` by inlining the referenced definition.
    if let Some(reference) = node.get("$ref").and_then(|r| r.as_str()) {
        let name = reference.rsplit('/').next().unwrap_or("");
        if let Some(def) = defs.and_then(|d| d.get(name)) {
            return resolve_strict(def.clone(), defs);
        }
    }

    match node {
        Value::Object(mut map) => {
            map.remove("$ref");
            if let Some(Value::Object(props)) = map.get_mut("properties") {
                let resolved: serde_json::Map<String, Value> = props
                    .iter()
                    .map(|(k, v)| (k.clone(), resolve_strict(v.clone(), defs)))
                    .collect();
                *props = resolved;
            }
            if let Some(items) = map.get_mut("items") {
                *items = resolve_strict(items.clone(), defs);
            }
            for key in ["anyOf", "oneOf", "allOf"] {
                if let Some(Value::Array(arr)) = map.get_mut(key) {
                    *arr = arr
                        .iter()
                        .map(|v| resolve_strict(v.clone(), defs))
                        .collect();
                }
            }
            let is_object = map.get("type").map(|t| t == "object").unwrap_or(false)
                || map.contains_key("properties");
            if is_object {
                map.insert("additionalProperties".to_string(), Value::Bool(false));
                if let Some(Value::Object(props)) = map.get("properties") {
                    let keys: Vec<Value> = props.keys().cloned().map(Value::String).collect();
                    map.insert("required".to_string(), Value::Array(keys));
                }
            }
            Value::Object(map)
        }
        other => other,
    }
}
