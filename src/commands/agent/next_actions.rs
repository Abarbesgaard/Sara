use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::commands::shared::project_head;
use crate::infrastructure::db;
use crate::infrastructure::model::{Status, Task};

/// Whether a URL points at a pull or merge request.
pub fn is_pr_url(url: &str) -> bool {
    url.contains("/pull/") || url.contains("/merge_requests/") || url.contains("/pullrequest/")
}

/// The most recently linked pull request on a task, if any.
pub fn pr_link(conn: &Connection, task: &Task) -> Result<Option<String>> {
    Ok(db::get_links(conn, &task.uuid)?
        .into_iter()
        .rev()
        .find(|l| is_pr_url(&l.url))
        .map(|l| l.url))
}

fn action(tool: &str, args: Value, why: String) -> Value {
    json!({ "tool": tool, "args": args, "why": why })
}

/// The tool calls that move a task forward from where it stands, in order.
/// Derived from the task's state so an agent never has to remember the
/// workflow: work the open steps, tick the acceptance criteria, validate at
/// HEAD, link the PR, then `finish` once it has merged.
pub fn next_actions(conn: &Connection, task: &Task) -> Result<Vec<Value>> {
    if task.status != Status::Pending {
        return Ok(vec![]);
    }
    let id = task.uuid.to_string()[..8].to_string();
    let steps = db::get_steps(conn, &task.uuid, db::STEP_KIND_STEP)?;
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;

    if let Some((i, s)) = steps.iter().enumerate().find(|(_, s)| !s.done) {
        return Ok(vec![action(
            "step_done",
            json!({ "id": id }),
            format!(
                "do step {}/{} — {} — then tick it with a result",
                i + 1,
                steps.len(),
                s.text
            ),
        )]);
    }
    if steps.is_empty() && acceptance.is_empty() {
        return Ok(vec![action(
            "check",
            json!({ "id": id }),
            "no plan yet: add steps, and an acceptance criterion (kind=\"acceptance\") with a `verify` command".into(),
        )]);
    }
    if let Some((i, a)) = acceptance.iter().enumerate().find(|(_, a)| !a.done) {
        return Ok(vec![action(
            "step_done",
            json!({ "id": id, "kind": "acceptance", "n": i + 1 }),
            format!("acceptance criterion {} is not ticked: {}", i + 1, a.text),
        )]);
    }

    let mut out = Vec::new();
    if acceptance.is_empty() {
        out.push(action(
            "check",
            json!({ "id": id, "kind": "acceptance" }),
            "no acceptance criterion: `done` would close without a proven definition of done"
                .into(),
        ));
    } else {
        let validated = db::get_guide_fields(conn, &task.uuid)?.validated_commit;
        let head = project_head(conn, &task.project);
        let stale = match (&validated, &head) {
            (None, _) => Some("never validated"),
            (Some(v), Some(h)) if v != h => Some("validated at an older commit"),
            _ => None,
        };
        if let Some(reason) = stale {
            out.push(action(
                "validate",
                json!({ "id": id }),
                format!("{reason}: run the acceptance gate at the current HEAD"),
            ));
        }
    }
    match pr_link(conn, task)? {
        None => out.push(action(
            "link",
            json!({ "id": id, "url": "<pull request url>" }),
            "open a pull request and link it to the task".into(),
        )),
        Some(url) => out.push(action(
            "finish",
            json!({ "id": id, "pr_url": url }),
            "once the PR has merged (base branch checked out and pulled), finish validates and closes the task".into(),
        )),
    }
    Ok(out)
}

/// Attach `next_actions` for task `id` to a tool result object. Best-effort:
/// a lookup failure leaves the result untouched rather than failing the call.
pub fn with_next_actions(conn: &Connection, id: &str, mut v: Value) -> Value {
    let Ok(task) = db::resolve_task(conn, id) else {
        return v;
    };
    if let (Some(obj), Ok(actions)) = (v.as_object_mut(), next_actions(conn, &task)) {
        obj.insert("next_actions".into(), Value::Array(actions));
    }
    v
}
