use clap_complete::engine::CompletionCandidate;
use rusqlite::Connection;

pub fn task_ids() -> Vec<CompletionCandidate> {
    crate::infrastructure::db::open()
        .ok()
        .map(|conn| task_ids_from(&conn))
        .unwrap_or_default()
}

fn task_ids_from(conn: &Connection) -> Vec<CompletionCandidate> {
    crate::infrastructure::db::list_tasks(conn, None)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|t| {
            t.id.map(|id| CompletionCandidate::new(id.to_string()).help(Some(t.description.into())))
        })
        .collect()
}

pub fn projects() -> Vec<CompletionCandidate> {
    crate::infrastructure::db::open()
        .ok()
        .map(|conn| projects_from(&conn))
        .unwrap_or_default()
}

fn projects_from(conn: &Connection) -> Vec<CompletionCandidate> {
    crate::infrastructure::db::project_names(conn)
        .unwrap_or_default()
        .into_iter()
        .map(CompletionCandidate::new)
        .collect()
}

pub fn memory_labels() -> Vec<CompletionCandidate> {
    crate::infrastructure::db::open()
        .ok()
        .map(|conn| memory_labels_from(&conn))
        .unwrap_or_default()
}

fn memory_labels_from(conn: &Connection) -> Vec<CompletionCandidate> {
    crate::infrastructure::db::list_memories(conn)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|m| {
            m.display_id.map(|id| {
                let label = format!("m{id}");
                let snippet: String = m.body.chars().take(60).collect();
                CompletionCandidate::new(label).help(Some(snippet.into()))
            })
        })
        .collect()
}
