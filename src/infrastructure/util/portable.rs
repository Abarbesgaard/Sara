use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infrastructure::model::{Priority, Status};

pub const BLOB_PREFIX: &str = "sara-task-v1:";
pub const BUNDLE_FORMAT: &str = "sara-task";
pub const BUNDLE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    pub format: String,
    pub version: u32,
    pub exported_at: DateTime<Utc>,
    pub root: Uuid,
    pub tasks: Vec<TaskEnvelope>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEnvelope {
    pub uuid: Uuid,
    pub description: String,
    pub project: String,
    pub status: Status,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub due: Option<DateTime<Utc>>,
    pub entry: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub estimate_mins: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recur: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_by: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<AnnotationDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub checklist: Vec<ChecklistDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<LinkDto>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<FileDto>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnnotationDto {
    pub text: String,
    pub kind: String,
    pub author: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChecklistDto {
    pub text: String,
    pub done: bool,
    pub kind: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_cmd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done_commit: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkDto {
    pub url: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileDto {
    pub path: String,
    pub source: String,
}

impl Bundle {
    pub fn encode(&self) -> Result<String> {
        let json = serde_json::to_vec(self).context("serializing task bundle")?;
        Ok(format!("{BLOB_PREFIX}{}", B64.encode(json)))
    }

    pub fn decode(blob: &str) -> Result<Bundle> {
        let trimmed = blob.trim();
        let body = trimmed.strip_prefix(BLOB_PREFIX).unwrap_or(trimmed);
        let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.is_empty() {
            bail!("empty task blob");
        }
        let json = B64.decode(compact.as_bytes()).context(
            "this does not look like a sara task blob (base64 decode failed) — \
             paste the whole `sara-task-v1:…` token",
        )?;
        let bundle: Bundle = serde_json::from_slice(&json).context("parsing task bundle JSON")?;
        if bundle.format != BUNDLE_FORMAT {
            bail!(
                "unexpected bundle format '{}' (expected '{BUNDLE_FORMAT}')",
                bundle.format
            );
        }
        if bundle.version > BUNDLE_VERSION {
            bail!(
                "task blob is version {} but this sara only understands up to {BUNDLE_VERSION} — upgrade sara",
                bundle.version
            );
        }
        if bundle.tasks.is_empty() {
            bail!("task blob contains no tasks");
        }
        Ok(bundle)
    }
}

#[cfg(test)]
#[path = "../../tests/unit/infrastructure/portable.rs"]
mod tests;
