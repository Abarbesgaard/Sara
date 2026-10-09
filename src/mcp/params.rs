use schemars::JsonSchema;
use serde::Deserialize;

fn de_id<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct V;
    impl serde::de::Visitor<'_> for V {
        type Value = String;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a task id as a string or integer")
        }
        fn visit_str<E>(self, v: &str) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_string<E>(self, v: String) -> Result<String, E> {
            Ok(v)
        }
        fn visit_i64<E>(self, v: i64) -> Result<String, E> {
            Ok(v.to_string())
        }
        fn visit_u64<E>(self, v: u64) -> Result<String, E> {
            Ok(v.to_string())
        }
    }
    d.deserialize_any(V)
}

fn de_opt_index<'de, D>(d: D) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum NumOrStr {
        Num(i64),
        Str(String),
    }
    use serde::de::Error;
    match Option::<NumOrStr>::deserialize(d)? {
        None => Ok(None),
        Some(NumOrStr::Num(n)) => usize::try_from(n)
            .map(Some)
            .map_err(|_| D::Error::custom("position must be non-negative")),
        Some(NumOrStr::Str(s)) => s
            .trim()
            .parse::<usize>()
            .map(Some)
            .map_err(|_| D::Error::custom("position must be a 1-based integer")),
    }
}

fn de_opt_string_list<'de, D>(d: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct V;
    impl<'de> serde::de::Visitor<'de> for V {
        type Value = Option<Vec<String>>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("an array of strings or a comma-separated string")
        }
        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }
        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }
        fn visit_some<D2>(self, d: D2) -> Result<Self::Value, D2::Error>
        where
            D2: serde::Deserializer<'de>,
        {
            d.deserialize_any(V)
        }
        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(crate::commands::shared::split_csv(v)))
        }
        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            self.visit_str(&v)
        }
        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut out = Vec::new();
            while let Some(s) = seq.next_element::<String>()? {
                out.push(s);
            }
            Ok(Some(out))
        }
    }
    d.deserialize_option(V)
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ListParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "List pending tasks across every project.")]
    pub(crate) all: Option<bool>,
    #[schemars(description = "Project name to list instead of the one at project_path.")]
    pub(crate) project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct FindParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(
        description = "uuid fragment to search for. Case-insensitive SUBSTRING match, not a prefix; hyphens are optional, so '4c449b0e' matches '…-4c44-9b0e-…'. '%', '_' and '\\' are matched literally. Must be at least 4 characters once hyphens are removed."
    )]
    pub(crate) fragment: String,
    #[schemars(
        description = "Status scope: 'pending', 'completed' or 'all' (default 'all'). Deleted tasks are never returned."
    )]
    pub(crate) status: Option<String>,
    #[schemars(description = "Search every project instead of only the one at project_path.")]
    pub(crate) all: Option<bool>,
    #[schemars(description = "Maximum matches to return (default 20).")]
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct IdParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AddParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    pub(crate) description: String,
    #[schemars(description = "Project name; defaults to the project at project_path.")]
    pub(crate) project: Option<String>,
    #[schemars(description = "H, M or L.")]
    pub(crate) priority: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tags: Option<Vec<String>>,
    #[schemars(description = "Recurrence interval, e.g. daily, weekly or 2w.")]
    pub(crate) recur: Option<String>,
    #[schemars(description = "Notes to attach as comments.")]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) annotations: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) links: Option<Vec<String>>,
    #[schemars(description = "Checklist steps to add.")]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) checks: Option<Vec<String>>,
    #[schemars(description = "Task UUID prefixes this task is blocked by.")]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) depends_on: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct BeginParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    pub(crate) description: String,
    #[schemars(description = "Project name; defaults to the project at project_path.")]
    pub(crate) project: Option<String>,
    #[schemars(description = "H, M or L.")]
    pub(crate) priority: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tags: Option<Vec<String>>,
    #[schemars(description = "Files the task will touch.")]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) files: Option<Vec<String>>,
    #[schemars(description = "What was asked, in the requester's words.")]
    pub(crate) assignment: Option<String>,
    #[schemars(description = "Why the task exists.")]
    pub(crate) rationale: Option<String>,
    #[schemars(description = "One acceptance criterion (definition of done).")]
    pub(crate) check: Option<String>,
    #[schemars(description = "Shell command that proves the acceptance criterion.")]
    pub(crate) verify: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StepsParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
    #[schemars(description = "Return only steps 1..until.")]
    pub(crate) until: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StepDoneParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    #[serde(alias = "task", alias = "task_id", deserialize_with = "de_id")]
    pub(crate) id: String,
    #[schemars(
        description = "1-based position of the step (or acceptance criterion with kind=acceptance). Omit with step_id to complete the current step."
    )]
    #[serde(default, alias = "index", deserialize_with = "de_opt_index")]
    pub(crate) n: Option<usize>,
    #[schemars(description = "Step rowid returned by check; takes precedence over n.")]
    pub(crate) step_id: Option<i64>,
    #[schemars(description = "Evidence of what was done.")]
    pub(crate) result: Option<String>,
    #[schemars(description = "step (default) or acceptance.")]
    pub(crate) kind: Option<String>,
    #[schemars(
        description = "Memory labels (e.g. [\"m12\"]) that actually helped. Records them as cited by this task, the strongest signal that a memory is useful. Every label must name an active memory, or the call fails and nothing is changed."
    )]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) used: Option<Vec<String>>,
    #[schemars(
        description = "Optional one-line \"now doing\" status to report in the same call (what you move on to next); shown live in `sara follow`."
    )]
    pub(crate) doing: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct VerifyParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
    #[schemars(description = "Only this 1-based step.")]
    pub(crate) step: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RecallParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Free-text keyword query; empty returns the most recent memories.")]
    #[serde(default)]
    pub(crate) query: String,
    #[schemars(description = "Only memories with these tags.")]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tag: Option<Vec<String>>,
    #[schemars(description = "Only memories in these projects.")]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) project: Option<Vec<String>>,
    #[schemars(
        description = "Only memories bound to these files; a trailing / matches a directory."
    )]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) files: Option<Vec<String>>,
    #[schemars(description = "Maximum hits (default 10).")]
    pub(crate) limit: Option<i64>,
    #[schemars(description = "Also return graph-related memories that share no keyword.")]
    pub(crate) spread: Option<bool>,
    #[schemars(
        description = "Task UUID prefix or display id this recall is for. Records the returned memories as recalled (direct hits) or surfaced (associative) by that task, so they earn credit when it succeeds. Pass it whenever you recall while working on a task; an unknown id is an error."
    )]
    pub(crate) task: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AnnotateParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
    pub(crate) text: String,
    #[schemars(
        description = "comment (default), finding, decision, constraint, risk, assumption, open_question, ..."
    )]
    pub(crate) kind: Option<String>,
    #[schemars(description = "Defaults to human.")]
    pub(crate) author: Option<String>,
    #[schemars(description = "Anchor: step:N, acceptance:N, anchor:ID or note:ID.")]
    pub(crate) on: Option<String>,
    #[schemars(description = "Flag the note for reconsideration.")]
    pub(crate) reconsider: Option<bool>,
    #[schemars(
        description = "Optional one-line \"now doing\" status to report in the same call; shown live in `sara follow`."
    )]
    pub(crate) doing: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PlanImportParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Plan as a JSON string; see the tool description for its shape.")]
    pub(crate) plan_json: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DoneParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
    #[schemars(description = "Complete even if blocked or targeted from a different branch.")]
    pub(crate) force: Option<bool>,
    #[schemars(
        description = "Memory labels (e.g. [\"m12\"]) that actually helped. Records them as cited by this task, the strongest signal that a memory is useful. Every label must name an active memory, or the call fails and nothing is changed."
    )]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) used: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LinkParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
    pub(crate) url: String,
    #[schemars(description = "Optional display text.")]
    pub(crate) label: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DepParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
    #[schemars(description = "on, off or list.")]
    pub(crate) action: String,
    #[schemars(description = "The blocking task's id; required for on/off.")]
    pub(crate) other: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CheckParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    #[serde(alias = "task", alias = "task_id", deserialize_with = "de_id")]
    pub(crate) id: String,
    #[schemars(description = "The step or criterion text.")]
    pub(crate) text: String,
    #[schemars(description = "step (default) or acceptance.")]
    pub(crate) kind: Option<String>,
    #[schemars(description = "Why this step matters.")]
    pub(crate) intent: Option<String>,
    #[schemars(
        description = "Shell command that proves it; required for validate to pass acceptance criteria."
    )]
    pub(crate) verify: Option<String>,
    #[schemars(description = "human (default) or ai.")]
    pub(crate) source: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ModifyParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
    pub(crate) description: Option<String>,
    #[schemars(description = "H, M or L.")]
    pub(crate) priority: Option<String>,
    #[schemars(description = "Due date, e.g. 2026-10-31, today or tomorrow.")]
    pub(crate) due: Option<String>,
    pub(crate) clear_due: Option<bool>,
    #[schemars(description = "Replaces the whole tag set.")]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) clear_tags: Option<bool>,
    #[schemars(description = "Time estimate, e.g. 30m or 2h.")]
    pub(crate) estimate: Option<String>,
    pub(crate) clear_estimate: Option<bool>,
    #[schemars(description = "Recurrence interval, e.g. daily, weekly or 2w.")]
    pub(crate) every: Option<String>,
    pub(crate) clear_recur: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ResolveParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Annotation id from feedback/info output, not a task id.")]
    pub(crate) feedback_id: i64,
    #[schemars(description = "run_id from record_run that addressed it.")]
    pub(crate) run_id: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RecordRunParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
    #[schemars(description = "Free-form label for the interaction, e.g. review.")]
    pub(crate) kind: String,
    pub(crate) model: Option<String>,
    pub(crate) provider: Option<String>,
    pub(crate) prompt: Option<String>,
    pub(crate) response: Option<String>,
    pub(crate) prompt_tokens: Option<i64>,
    pub(crate) completion_tokens: Option<i64>,
    pub(crate) total_tokens: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StepEditParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    #[serde(alias = "task", alias = "task_id", deserialize_with = "de_id")]
    pub(crate) id: String,
    #[schemars(
        description = "1-based position of the step (or acceptance criterion with kind=acceptance)."
    )]
    #[serde(default, alias = "index", deserialize_with = "de_opt_index")]
    pub(crate) n: Option<usize>,
    #[schemars(description = "Step rowid returned by check; takes precedence over n.")]
    pub(crate) step_id: Option<i64>,
    #[schemars(description = "step (default) or acceptance.")]
    pub(crate) kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GuideTextParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
    pub(crate) text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AttachParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
    #[schemars(description = "File path (relative to project_path or absolute) or URL.")]
    pub(crate) path: String,
    pub(crate) reason: Option<String>,
    pub(crate) symbol: Option<String>,
    #[schemars(description = "Line range as start:end, e.g. 10:57.")]
    pub(crate) lines: Option<String>,
    #[schemars(description = "human (default) or ai.")]
    pub(crate) source: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LearnParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "The insight, one idea, at most 2000 characters.")]
    pub(crate) text: String,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tags: Option<Vec<String>>,
    #[schemars(
        description = "Project names to scope the memory to; defaults to the current project."
    )]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) projects: Option<Vec<String>>,
    #[schemars(description = "Task UUID prefixes to link.")]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tasks: Option<Vec<String>>,
    #[schemars(description = "Files to bind; relative paths resolve against project_path.")]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) files: Option<Vec<String>>,
    #[schemars(description = "Skip the size, secret and overlap checks.")]
    pub(crate) force: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ForgetParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Memory label, e.g. m3.")]
    pub(crate) handle: String,
    #[schemars(description = "Also archive derived_from children of a canonical.")]
    pub(crate) cascade: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PromoteParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Memory label, e.g. m14.")]
    pub(crate) handle: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RelearnParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Memory label, e.g. m3.")]
    pub(crate) handle: String,
    #[schemars(description = "New body; omit to keep the current one.")]
    pub(crate) text: Option<String>,
    #[schemars(description = "Replaces the tag set; omit to keep it.")]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tags: Option<Vec<String>>,
    #[schemars(description = "Replaces the file set; omit to keep it.")]
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) files: Option<Vec<String>>,
    #[schemars(description = "Skip the size and secret checks.")]
    pub(crate) force: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct TagsParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MemoriesParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ProjectsParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UnlinkParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Link id shown in info.")]
    pub(crate) link_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DenotateParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Annotation id shown in info.")]
    pub(crate) annotation_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MoveTaskParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
    #[schemars(description = "Target project name.")]
    pub(crate) project: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LinkMemoryParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Source memory label, e.g. m3.")]
    pub(crate) from: String,
    #[schemars(description = "supersedes, similar_to, derived_from or used_in.")]
    pub(crate) relation: String,
    #[schemars(description = "Target memory label.")]
    pub(crate) to: String,
    #[schemars(description = "Link weight (default 1.0).")]
    pub(crate) weight: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UnlinkMemoryParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Source memory label.")]
    pub(crate) from: String,
    #[schemars(description = "Relation to remove.")]
    pub(crate) relation: String,
    #[schemars(description = "Target memory label.")]
    pub(crate) to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PruneMemoriesParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Preview only (default true).")]
    pub(crate) dry_run: Option<bool>,
    #[schemars(description = "Age before an unlinked weak memory is pruned (default 90).")]
    pub(crate) weak_days: Option<i64>,
    #[schemars(
        description = "Age before an unreviewed provisional memory is pruned (default 30)."
    )]
    pub(crate) provisional_days: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ConsolidateParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Days of recall history to sweep (default 30).")]
    pub(crate) window_days: Option<i64>,
    #[schemars(description = "Recalls this many seconds apart count as co-firing (default 5).")]
    pub(crate) bucket_secs: Option<i64>,
    #[schemars(description = "Weight added per co-firing (default 0.1).")]
    pub(crate) delta: Option<f64>,
    #[schemars(description = "Skip buckets with more memories than this (default 5).")]
    pub(crate) max_bucket: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ReflectParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Minimum co_activated weight to cluster on (default 0.5).")]
    pub(crate) min_weight: Option<f64>,
    #[schemars(description = "Maximum memories per cluster (default 8).")]
    pub(crate) max_cluster: Option<usize>,
    #[schemars(description = "Create the proposed derived_from links.")]
    pub(crate) apply: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DiagnoseMemoriesParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Minimum cosine similarity to report (default 0.75).")]
    pub(crate) threshold: Option<f32>,
    #[schemars(description = "Only this project.")]
    pub(crate) project: Option<String>,
    #[schemars(description = "Maximum pairs to return.")]
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DoctorParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ReindexEmbeddingsParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DoingParams {
    #[schemars(
        description = "Absolute path to the target project's repo; omit to use the server's launch directory."
    )]
    pub(crate) project_path: Option<String>,
    #[schemars(description = "Task UUID prefix (stable, preferred) or numeric display id.")]
    pub(crate) id: String,
    #[schemars(
        description = "One short line saying what you are doing right now (max 200 chars), e.g. \"running the parser tests\"."
    )]
    pub(crate) text: String,
}
