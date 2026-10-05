use serde_json::{Value, json};

use crate::commands::add::types::Created;

pub(in crate::commands::add) fn created_json(created: Created) -> Value {
    let Created {
        task,
        branch,
        similar,
        duplicate,
    } = created;
    let duplicate = duplicate.map(|t| {
        json!({
            "id": t.id,
            "uuid": t.uuid.to_string(),
            "description": t.description,
        })
    });
    json!({
        "id": task.id,
        "uuid": task.uuid.to_string(),
        "project": task.project,
        "description": task.description,
        "branch": branch,
        "similar": similar.unwrap_or_default(),
        "duplicate": duplicate,
    })
}
