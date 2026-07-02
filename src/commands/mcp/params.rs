//! Typed parameter schemas for the MCP tools. Each derives `JsonSchema` (for the
//! tool `inputSchema`) and `Deserialize` (rmcp deserializes the call arguments
//! into these). Shared across the tool slices (`read`/`guide`/`lifecycle`), so
//! both the structs and their fields are `pub(crate)`.

use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListParams {
    /// Absolute path of the target git repo/project. Omit to use the launch dir.
    pub(crate) project_path: Option<String>,
    /// List tasks across all projects instead of just the current one.
    pub(crate) all: Option<bool>,
    /// Explicit project name filter (overrides project_path detection).
    pub(crate) project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct IdParams {
    pub(crate) project_path: Option<String>,
    /// Task id or (preferred) 8-char uuid prefix.
    pub(crate) id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AddParams {
    pub(crate) project_path: Option<String>,
    /// The task description / title.
    pub(crate) description: String,
    pub(crate) project: Option<String>,
    /// Priority: H, M, or L.
    pub(crate) priority: Option<String>,
    pub(crate) tags: Option<Vec<String>>,
    /// Recurrence interval: daily, weekly, 2w, 3d, 1m, …
    pub(crate) recur: Option<String>,
    /// Notes to attach at creation.
    pub(crate) annotations: Option<Vec<String>>,
    /// URLs to link at creation.
    pub(crate) links: Option<Vec<String>>,
    /// Checklist steps to add at creation.
    pub(crate) checks: Option<Vec<String>>,
    /// UUID prefixes of tasks this new task depends on.
    pub(crate) depends_on: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StepsParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    /// Only return steps 1..=until.
    pub(crate) until: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StepDoneParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    /// 1-based step number.
    pub(crate) n: usize,
    /// Execution result / evidence recorded with the step.
    pub(crate) result: Option<String>,
    /// Item kind: "step" (default) or "acceptance".
    pub(crate) kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct VerifyParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    /// Only return the verify command for step N.
    pub(crate) step: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RecallParams {
    pub(crate) project_path: Option<String>,
    /// Search query (keyword / FTS).
    pub(crate) query: String,
    /// Max results (default 20).
    pub(crate) limit: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AnnotateParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    /// The note / comment text.
    pub(crate) text: String,
    /// Note kind: comment (default), finding, decision, constraint, risk, …
    pub(crate) kind: Option<String>,
    /// Author: human (default) or ai.
    pub(crate) author: Option<String>,
    /// Anchor to a guide element: step:N, acceptance:N, anchor:ID, note:ID.
    pub(crate) on: Option<String>,
    /// Flag the anchored element for reconsideration.
    pub(crate) reconsider: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PlanImportParams {
    pub(crate) project_path: Option<String>,
    /// The plan graph as a JSON string ({"project"?, "tasks":[…]}). Passed inline
    /// (never via stdin, which is the MCP transport channel).
    pub(crate) plan_json: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DoneParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    /// Complete the task even if it is blocked by unfinished dependencies.
    pub(crate) force: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LinkParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    /// The URL to attach (e.g. a PR or issue link).
    pub(crate) url: String,
    /// Optional human-readable label for the link.
    pub(crate) label: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DepParams {
    pub(crate) project_path: Option<String>,
    /// The dependent task (the one that gets blocked).
    pub(crate) id: String,
    /// "on" (add: `id` depends on `other`), "off" (remove), or "list".
    pub(crate) action: String,
    /// The blocker task; required for "on"/"off", ignored for "list".
    pub(crate) other: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CheckParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    /// The step / acceptance-criterion text.
    pub(crate) text: String,
    /// Item kind: "step" (default) or "acceptance".
    pub(crate) kind: Option<String>,
    /// Optional intent / why-note for the step.
    pub(crate) intent: Option<String>,
    /// Optional shell command that verifies this step.
    pub(crate) verify: Option<String>,
    /// Author/source of the item: "human" (default) or "ai".
    pub(crate) source: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ModifyParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    /// New description / title.
    pub(crate) description: Option<String>,
    /// Priority: H, M, or L.
    pub(crate) priority: Option<String>,
    /// Due date (same formats as `add`, e.g. "friday", "2026-07-15").
    pub(crate) due: Option<String>,
    /// Clear the due date.
    pub(crate) clear_due: Option<bool>,
    /// Tags — REPLACES the whole tag set (not additive).
    pub(crate) tags: Option<Vec<String>>,
    /// Clear all tags.
    pub(crate) clear_tags: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ResolveParams {
    pub(crate) project_path: Option<String>,
    /// The feedback (annotation) id to resolve — NOT a task id/uuid.
    pub(crate) feedback_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StepEditParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    /// 1-based step number.
    pub(crate) n: usize,
    /// Item kind: "step" (default) or "acceptance".
    pub(crate) kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GuideTextParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    /// The text to set (assignment / rationale).
    pub(crate) text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AttachParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    /// File path or URL to attach. URLs are stored as links.
    pub(crate) path: String,
    /// Why this file/anchor matters.
    pub(crate) reason: Option<String>,
    /// Symbol (function/type) the anchor points at.
    pub(crate) symbol: Option<String>,
    /// Line range, e.g. "10:57" or "10-57".
    pub(crate) lines: Option<String>,
    /// Provenance: "ai" marks it suggested; anything else is manual.
    pub(crate) source: Option<String>,
}
