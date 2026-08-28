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
    /// Search query (keyword / FTS). Optional when `tag`/`project`/`files` narrow the
    /// lookup instead; combines with them using AND when both are given.
    #[serde(default)]
    pub(crate) query: String,
    /// Exact tag match against indexed memory tags (a memory must carry every
    /// tag given to match).
    pub(crate) tag: Option<Vec<String>>,
    /// Exact project match against indexed memory project references (a
    /// memory matches if it references any of the given projects).
    pub(crate) project: Option<Vec<String>>,
    /// Filter by associated file path (exact match; trailing '/' = prefix/directory match).
    pub(crate) files: Option<Vec<String>>,
    /// Max results (default 20).
    pub(crate) limit: Option<i64>,
    /// Also surface associatively-related memories by spreading activation
    /// across the memory graph (links + shared tag/file/task anchors), returned
    /// in an `associative` array alongside `keyword`. Off by default.
    pub(crate) spread: Option<bool>,
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
    /// Time estimate, e.g. "90m", "2h", "2h30m".
    pub(crate) estimate: Option<String>,
    /// Clear the time estimate.
    pub(crate) clear_estimate: Option<bool>,
    /// Recurrence interval: daily, weekly, monthly, 2w, 3d, 1m, etc.
    pub(crate) every: Option<String>,
    /// Clear the recurrence interval.
    pub(crate) clear_recur: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ResolveParams {
    pub(crate) project_path: Option<String>,
    /// The feedback (annotation) id to resolve — NOT a task id/uuid.
    pub(crate) feedback_id: i64,
    /// Link this resolution to an AI run id (from `record_run`) that addressed it.
    pub(crate) run_id: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RecordRunParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    /// What the AI did, e.g. enrich, refine, implement, review.
    pub(crate) kind: String,
    /// Model used, e.g. claude-sonnet-5-thinking-high.
    pub(crate) model: Option<String>,
    /// Provider/platform, e.g. cursor.
    pub(crate) provider: Option<String>,
    /// The prompt/instruction given to the model (stored, not displayed in `sara info`).
    pub(crate) prompt: Option<String>,
    /// A summary of the model's response (stored, not displayed in `sara info`).
    pub(crate) response: Option<String>,
    /// Number of prompt/input tokens used by the model.
    pub(crate) prompt_tokens: Option<i64>,
    /// Number of completion/output tokens used by the model.
    pub(crate) completion_tokens: Option<i64>,
    /// Total tokens used (prompt + completion). If omitted, computed from the other two when both are present.
    pub(crate) total_tokens: Option<i64>,
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

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LearnParams {
    pub(crate) project_path: Option<String>,
    /// The memory text (a concise distilled paragraph — one key insight).
    pub(crate) text: String,
    /// Tags for lookup (repeatable). Prefer existing tags — check `sara tags` first.
    pub(crate) tags: Option<Vec<String>>,
    /// Projects this memory references. Defaults to the current project.
    pub(crate) projects: Option<Vec<String>>,
    /// UUID prefixes of tasks this memory was learned from/about (repeatable; source='explicit').
    pub(crate) tasks: Option<Vec<String>>,
    /// Absolute file paths to associate with this memory (repeatable).
    pub(crate) files: Option<Vec<String>>,
    /// Skip size and secret-pattern guardrails (use only when content is safe).
    pub(crate) force: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ForgetParams {
    pub(crate) project_path: Option<String>,
    /// Memory label to archive, e.g. "m3". Get labels from `recall` output.
    pub(crate) handle: String,
    /// Also archive any memories `derived_from` this one (one level). If
    /// omitted/false, derived children are only listed in the response for
    /// review, never auto-archived.
    pub(crate) cascade: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PromoteParams {
    pub(crate) project_path: Option<String>,
    /// Provisional memory label to promote to active, e.g. "m14".
    pub(crate) handle: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RelearnParams {
    pub(crate) project_path: Option<String>,
    /// Memory label to edit, e.g. "m3".
    pub(crate) handle: String,
    /// New body text (omit to only change tags/files).
    pub(crate) text: Option<String>,
    /// Replacement tag set (omit to keep existing tags).
    pub(crate) tags: Option<Vec<String>>,
    /// Replacement file associations, absolute paths (omit to keep existing).
    pub(crate) files: Option<Vec<String>>,
    /// Skip size and secret-pattern guardrails (use only when content is safe).
    pub(crate) force: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct TagsParams {
    pub(crate) project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MemoriesParams {
    pub(crate) project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ProjectsParams {
    pub(crate) project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UnlinkParams {
    pub(crate) project_path: Option<String>,
    /// Sequential link id shown in `sara info` (the number before the URL).
    pub(crate) link_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DenotateParams {
    pub(crate) project_path: Option<String>,
    /// Sequential annotation id shown in `sara info` (the number before the text).
    pub(crate) annotation_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MoveTaskParams {
    pub(crate) project_path: Option<String>,
    /// Task id or 8-char uuid prefix.
    pub(crate) id: String,
    /// Name of the target project.
    pub(crate) project: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LinkMemoryParams {
    pub(crate) project_path: Option<String>,
    /// Source memory label (e.g. "m12") or 8-char uuid prefix.
    pub(crate) from: String,
    /// Relation type: supersedes | similar_to | derived_from | used_in
    pub(crate) relation: String,
    /// Target memory label (e.g. "m7") or 8-char uuid prefix.
    pub(crate) to: String,
    /// Edge weight (default 1.0).
    pub(crate) weight: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UnlinkMemoryParams {
    pub(crate) project_path: Option<String>,
    /// Source memory label (e.g. "m12") or 8-char uuid prefix.
    pub(crate) from: String,
    /// Relation type: supersedes | similar_to | derived_from | used_in
    pub(crate) relation: String,
    /// Target memory label (e.g. "m7") or 8-char uuid prefix.
    pub(crate) to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PruneMemoriesParams {
    pub(crate) project_path: Option<String>,
    /// If true (default), only show candidates — do not archive anything.
    pub(crate) dry_run: Option<bool>,
    /// Days before a Weak memory (no task link) is eligible for pruning. Default 90.
    pub(crate) weak_days: Option<i64>,
    /// Days before a Provisional auto-memory is eligible if unreviewed. Default 30.
    pub(crate) provisional_days: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ConsolidateParams {
    pub(crate) project_path: Option<String>,
    /// How many days of recall history to sweep. Default 30.
    pub(crate) window_days: Option<i64>,
    /// Co-firing window in seconds: recalls closer than this fired together. Default 5.
    pub(crate) bucket_secs: Option<i64>,
    /// Weight added to a synapse per co-firing. Default 0.1.
    pub(crate) delta: Option<f64>,
    /// Bursts with more distinct memories than this are bulk listings, not genuine
    /// co-activation, and are skipped. 0 disables the guard. Default 5.
    pub(crate) max_bucket: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ReflectParams {
    pub(crate) project_path: Option<String>,
    /// Minimum co-activation weight for two memories to count as clustered.
    pub(crate) min_weight: Option<f64>,
    /// If true, create the proposed `derived_from` edges. Default false
    /// (read-only: return the proposal so you can review it first).
    pub(crate) apply: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DiagnoseMemoriesParams {
    pub(crate) project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ReindexEmbeddingsParams {
    pub(crate) project_path: Option<String>,
}
