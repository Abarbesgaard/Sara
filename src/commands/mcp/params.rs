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
            let items: Vec<String> = v
                .split(',')
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(|s| s.to_string())
                .collect();
            Ok(Some(items))
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
    pub(crate) project_path: Option<String>,
    pub(crate) all: Option<bool>,
    pub(crate) project: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct IdParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AddParams {
    pub(crate) project_path: Option<String>,
    pub(crate) description: String,
    pub(crate) project: Option<String>,
    pub(crate) priority: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) recur: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) annotations: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) links: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) checks: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) depends_on: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct BeginParams {
    pub(crate) project_path: Option<String>,
    pub(crate) description: String,
    pub(crate) project: Option<String>,
    pub(crate) priority: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tags: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) files: Option<Vec<String>>,
    pub(crate) assignment: Option<String>,
    pub(crate) rationale: Option<String>,
    pub(crate) check: Option<String>,
    pub(crate) verify: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StepsParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    pub(crate) until: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct StepDoneParams {
    pub(crate) project_path: Option<String>,
    #[serde(alias = "task", alias = "task_id", deserialize_with = "de_id")]
    pub(crate) id: String,
    #[serde(default, alias = "index", deserialize_with = "de_opt_index")]
    pub(crate) n: Option<usize>,
    pub(crate) step_id: Option<i64>,
    pub(crate) result: Option<String>,
    pub(crate) kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct VerifyParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    pub(crate) step: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RecallParams {
    pub(crate) project_path: Option<String>,
    #[serde(default)]
    pub(crate) query: String,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tag: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) project: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) files: Option<Vec<String>>,
    pub(crate) limit: Option<i64>,
    pub(crate) spread: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AnnotateParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) kind: Option<String>,
    pub(crate) author: Option<String>,
    pub(crate) on: Option<String>,
    pub(crate) reconsider: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PlanImportParams {
    pub(crate) project_path: Option<String>,
    pub(crate) plan_json: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DoneParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    pub(crate) force: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LinkParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    pub(crate) url: String,
    pub(crate) label: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DepParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    pub(crate) action: String,
    pub(crate) other: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct CheckParams {
    pub(crate) project_path: Option<String>,
    #[serde(alias = "task", alias = "task_id", deserialize_with = "de_id")]
    pub(crate) id: String,
    pub(crate) text: String,
    pub(crate) kind: Option<String>,
    pub(crate) intent: Option<String>,
    pub(crate) verify: Option<String>,
    pub(crate) source: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ModifyParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    pub(crate) description: Option<String>,
    pub(crate) priority: Option<String>,
    pub(crate) due: Option<String>,
    pub(crate) clear_due: Option<bool>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tags: Option<Vec<String>>,
    pub(crate) clear_tags: Option<bool>,
    pub(crate) estimate: Option<String>,
    pub(crate) clear_estimate: Option<bool>,
    pub(crate) every: Option<String>,
    pub(crate) clear_recur: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ResolveParams {
    pub(crate) project_path: Option<String>,
    pub(crate) feedback_id: i64,
    pub(crate) run_id: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RecordRunParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
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
    pub(crate) project_path: Option<String>,
    #[serde(alias = "task", alias = "task_id", deserialize_with = "de_id")]
    pub(crate) id: String,
    #[serde(default, alias = "index", deserialize_with = "de_opt_index")]
    pub(crate) n: Option<usize>,
    pub(crate) step_id: Option<i64>,
    pub(crate) kind: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct GuideTextParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    pub(crate) text: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct AttachParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    pub(crate) path: String,
    pub(crate) reason: Option<String>,
    pub(crate) symbol: Option<String>,
    pub(crate) lines: Option<String>,
    pub(crate) source: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LearnParams {
    pub(crate) project_path: Option<String>,
    pub(crate) text: String,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tags: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) projects: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tasks: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) files: Option<Vec<String>>,
    pub(crate) force: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ForgetParams {
    pub(crate) project_path: Option<String>,
    pub(crate) handle: String,
    pub(crate) cascade: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PromoteParams {
    pub(crate) project_path: Option<String>,
    pub(crate) handle: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct RelearnParams {
    pub(crate) project_path: Option<String>,
    pub(crate) handle: String,
    pub(crate) text: Option<String>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) tags: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_opt_string_list")]
    pub(crate) files: Option<Vec<String>>,
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
    pub(crate) link_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DenotateParams {
    pub(crate) project_path: Option<String>,
    pub(crate) annotation_id: i64,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct MoveTaskParams {
    pub(crate) project_path: Option<String>,
    pub(crate) id: String,
    pub(crate) project: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct LinkMemoryParams {
    pub(crate) project_path: Option<String>,
    pub(crate) from: String,
    pub(crate) relation: String,
    pub(crate) to: String,
    pub(crate) weight: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct UnlinkMemoryParams {
    pub(crate) project_path: Option<String>,
    pub(crate) from: String,
    pub(crate) relation: String,
    pub(crate) to: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct PruneMemoriesParams {
    pub(crate) project_path: Option<String>,
    pub(crate) dry_run: Option<bool>,
    pub(crate) weak_days: Option<i64>,
    pub(crate) provisional_days: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ConsolidateParams {
    pub(crate) project_path: Option<String>,
    pub(crate) window_days: Option<i64>,
    pub(crate) bucket_secs: Option<i64>,
    pub(crate) delta: Option<f64>,
    pub(crate) max_bucket: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ReflectParams {
    pub(crate) project_path: Option<String>,
    pub(crate) min_weight: Option<f64>,
    pub(crate) max_cluster: Option<usize>,
    pub(crate) apply: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct DiagnoseMemoriesParams {
    pub(crate) project_path: Option<String>,
    pub(crate) threshold: Option<f32>,
    pub(crate) project: Option<String>,
    pub(crate) limit: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub(crate) struct ReindexEmbeddingsParams {
    pub(crate) project_path: Option<String>,
}
