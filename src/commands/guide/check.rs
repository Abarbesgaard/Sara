use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use super::support::kind_arg;
use crate::infrastructure::db;

pub fn check_value(
    conn: &Connection,
    id: &str,
    text: &str,
    intent: Option<&str>,
    kind: Option<&str>,
    source: Option<&str>,
    verify: Option<&str>,
) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id)?;
    let kind = kind_arg(kind);
    let source = source.unwrap_or("human");
    let step_id = db::add_step(conn, &task.uuid, text, intent, kind, source, verify)?;
    let index = db::get_steps(conn, &task.uuid, kind)?.len();
    let mut out = json!({
        "task": task.id,
        "uuid": task.uuid.to_string(),
        "kind": kind,
        "text": text,
        "step_id": step_id,
        "index": index,
    });
    if kind == db::STEP_KIND_ACCEPTANCE && verify.map(str::trim).filter(|v| !v.is_empty()).is_none()
    {
        out["warning"] = json!(
            "acceptance criterion has no verify command — `validate` cannot prove \
             it and will refuse; add one with `--verify \"<cmd>\"`"
        );
    }
    Ok(out)
}
