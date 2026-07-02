//! Portable task transfer: a self-contained, serde-friendly snapshot of one or
//! more tasks plus their related rows, encoded as a single copy-pasteable blob.
//!
//! The blob is `sara-task-v1:<base64(json)>` — a recognizable prefix followed by
//! the standard-base64 encoding of the JSON [`Bundle`]. Decoding tolerates
//! surrounding whitespace/newlines (so a blob that got line-wrapped in an email
//! or chat still imports) and an optional missing prefix.
//!
//! This module is intentionally pure data + codec: building a [`Bundle`] from the
//! database and writing one back live in `commands::export` / `commands::import`.

use anyhow::{Context, Result, bail};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::infrastructure::model::{Priority, Status};

/// Marker + version prefix for the copy-paste blob.
pub const BLOB_PREFIX: &str = "sara-task-v1:";
/// `format` discriminator stored inside the JSON envelope.
pub const BUNDLE_FORMAT: &str = "sara-task";
/// Current bundle schema version.
pub const BUNDLE_VERSION: u32 = 1;

/// A portable collection of tasks (a dependency closure) and their edges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    /// Always [`BUNDLE_FORMAT`]; guards against decoding unrelated base64.
    pub format: String,
    /// Schema version; see [`BUNDLE_VERSION`].
    pub version: u32,
    pub exported_at: DateTime<Utc>,
    /// The task the user asked to export (the closure root), by original uuid.
    pub root: Uuid,
    pub tasks: Vec<TaskEnvelope>,
}

/// One task plus everything that travels with it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEnvelope {
    /// Original uuid — used only to remap dependency edges within the bundle;
    /// a fresh uuid is generated on import.
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
    /// Original uuids of the tasks this one depends on (blockers). Edges whose
    /// endpoint is outside the bundle are dropped on import.
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
    /// Encode the bundle to the `sara-task-v1:<base64(json)>` copy-paste blob.
    pub fn encode(&self) -> Result<String> {
        let json = serde_json::to_vec(self).context("serializing task bundle")?;
        Ok(format!("{BLOB_PREFIX}{}", B64.encode(json)))
    }

    /// Decode a copy-paste blob back into a [`Bundle`].
    ///
    /// Tolerant of: surrounding whitespace, internal whitespace/newlines
    /// introduced by line-wrapping, and a missing `sara-task-v1:` prefix.
    pub fn decode(blob: &str) -> Result<Bundle> {
        // Strip the prefix if present, then remove *all* ASCII whitespace so a
        // wrapped/re-indented blob still decodes.
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
