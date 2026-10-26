use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::project::detect_current_project;

pub const DEFAULT_MEMORIES_LIMIT: usize = 50;

/// Which memories a listing covers: the current project's (plus unscoped,
/// global ones), or every project's.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    #[default]
    Project,
    All,
}

impl Scope {
    pub fn parse(s: Option<&str>) -> Result<Self> {
        match s.map(str::trim) {
            None | Some("") | Some("project") => Ok(Scope::Project),
            Some("all") => Ok(Scope::All),
            Some(other) => {
                anyhow::bail!("unknown scope `{other}` (expected \"project\" or \"all\")")
            }
        }
    }

    fn project(self, conn: &Connection, cfg: &Config) -> Option<String> {
        match self {
            Scope::All => None,
            Scope::Project => detect_current_project(conn, cfg).ok().map(|(p, _)| p),
        }
    }
}

fn scope_label(project: &Option<String>) -> Value {
    match project {
        Some(p) => json!(p),
        None => json!("all"),
    }
}

/// Tag counts over the scoped memories, optionally narrowed to a prefix and
/// capped at `limit` (most-used first).
pub fn tags_value(
    conn: &Connection,
    cfg: &Config,
    scope: Scope,
    prefix: Option<&str>,
    limit: Option<usize>,
) -> Result<Value> {
    let project = scope.project(conn, cfg);
    let counts = match &project {
        Some(p) => db::list_tags_with_counts_for_project(conn, p)?,
        None => db::list_tags_with_counts(conn)?,
    };
    let prefix = prefix.map(|p| p.trim().to_lowercase());
    let matching: Vec<(String, i64)> = counts
        .into_iter()
        .filter(|(t, _)| {
            prefix
                .as_deref()
                .is_none_or(|p| t.to_lowercase().starts_with(p))
        })
        .collect();
    let total = matching.len();
    let tags: Vec<Value> = matching
        .into_iter()
        .take(limit.unwrap_or(usize::MAX))
        .map(|(tag, count)| json!({ "tag": tag, "count": count }))
        .collect();
    Ok(json!({
        "scope": scope_label(&project),
        "total": total,
        "truncated": tags.len() < total,
        "tags": tags,
    }))
}

fn strength_label(strength: f64) -> &'static str {
    if strength >= 2.0 {
        "Strong"
    } else if strength >= 1.5 {
        "Linked"
    } else {
        "Weak"
    }
}

/// The scoped memories, newest first, capped at `limit`
/// (default [`DEFAULT_MEMORIES_LIMIT`]).
pub fn memories_value(
    conn: &Connection,
    cfg: &Config,
    scope: Scope,
    limit: Option<usize>,
) -> Result<Value> {
    let project = scope.project(conn, cfg);
    let items = match &project {
        Some(p) => db::list_memories_for_project(conn, p)?,
        None => db::list_memories(conn)?,
    };
    let total = items.len();
    let items: Vec<_> = items
        .into_iter()
        .take(limit.unwrap_or(DEFAULT_MEMORIES_LIMIT))
        .collect();
    let strengths = db::item_strengths(conn, &items);
    let rows: Vec<Value> = items
        .iter()
        .map(|m| {
            let strength = strengths.get(&m.uuid).copied().unwrap_or(1.0);
            let label = format!(
                "{}{}",
                m.kind.chars().next().unwrap_or('m'),
                m.display_id.unwrap_or(0)
            );
            json!({
                "label": label,
                "title": m.title,
                "body": m.body,
                "strength": strength,
                "strength_label": strength_label(strength),
                "tags": m.tags,
                "files": db::get_item_files(conn, &m.uuid).unwrap_or_default(),
                "created": m.created.to_rfc3339(),
                "modified": m.modified.to_rfc3339(),
            })
        })
        .collect();
    Ok(json!({
        "scope": scope_label(&project),
        "total": total,
        "truncated": rows.len() < total,
        "memories": rows,
    }))
}
