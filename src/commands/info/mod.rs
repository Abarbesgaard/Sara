mod edit;
mod handler;
mod plain;
mod render;
mod types;

use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::tui;

use crate::commands::shared::{
    cited_memories, item_label, item_snippet, memory_handle, print_json, project_head,
};
use edit::edit_loop;
use handler::load_detail;
use plain::{RenderOpts, render_markdown, render_plain};

pub fn guide_value(conn: &Connection, id_or_uuid: &str) -> Result<serde_json::Value> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    let mut guide = db::guide_json(conn, &task.uuid)?;

    let head = project_head(conn, &task.project);
    let validated = db::get_guide_fields(conn, &task.uuid)?.validated_commit;
    let stale = match (&head, &validated) {
        (Some(h), Some(v)) => h != v,
        _ => false,
    };

    let feedback = db::get_open_feedback(conn, &task.uuid)?;
    let open_feedback: Vec<_> = feedback
        .iter()
        .map(|f| {
            serde_json::json!({
                "id": f.id,
                "text": f.text,
                "target_kind": f.target_kind,
                "target_id": f.target_id,
                "request_revision": f.request_revision,
            })
        })
        .collect();
    let needs_revision = feedback.iter().any(|f| f.request_revision);

    if let Some(obj) = guide.as_object_mut() {
        obj.insert(
            "freshness".to_string(),
            serde_json::json!({ "head": head, "validated_commit": validated, "stale": stale }),
        );
        obj.insert(
            "open_feedback".to_string(),
            serde_json::Value::Array(open_feedback),
        );
        obj.insert(
            "needs_revision".to_string(),
            serde_json::Value::Bool(needs_revision),
        );
        let cited: Vec<serde_json::Value> = cited_memories(conn, &task.uuid)
            .iter()
            .map(|m| serde_json::json!({ "memory": memory_handle(m), "title": m.title }))
            .collect();
        obj.insert("cited".to_string(), serde_json::Value::Array(cited));

        let description = obj
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let tags: Vec<String> = obj
            .get("tags")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        let similar =
            db::find_similar_strong_memories(conn, &description, &tags).unwrap_or_default();
        if !similar.is_empty() {
            let similar_json: Vec<serde_json::Value> = similar
                .iter()
                .map(|item| {
                    let label = item_label(item);
                    let snippet = item_snippet(item, 160);
                    serde_json::json!({
                        "label": label,
                        "description": item.title,
                        "snippet": snippet,
                        "files": item.files,
                    })
                })
                .collect();
            obj.insert(
                "similar_work".to_string(),
                serde_json::Value::Array(similar_json),
            );
        }
    }

    Ok(guide)
}

pub fn preview_lines(
    conn: &Connection,
    cfg: &Config,
    uuid: &str,
    width: u16,
) -> Result<Vec<ratatui::text::Line<'static>>> {
    let task = db::resolve_task(conn, uuid)?;
    let detail = load_detail(conn, cfg, task)?;
    let st = types::EditState {
        detail,
        selected: usize::MAX,
        editing: false,
        commenting: false,
        adding_step: false,
        editor: ratatui_textarea::TextArea::default(),
        due_error: false,
        dep_error: None,
        scroll: 0,
        last_selected: None,
        tree_expanded: false,
        show_urgency_breakdown: false,
        verbose: false,
        show_notes: false,
        open: handler::load_open_sections(conn),
    };
    Ok(render::preview(&st, width as usize))
}

pub fn run_json(conn: &Connection, _cfg: &Config, id_or_uuid: &str) -> Result<()> {
    print_json(&guide_value(conn, id_or_uuid)?)?;
    Ok(())
}

pub fn run(
    conn: &Connection,
    cfg: &Config,
    id_or_uuid: &str,
    plain: bool,
    md: bool,
    history: bool,
) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    let detail = load_detail(conn, cfg, task)?;
    let opts = RenderOpts { history };

    if md {
        print!("{}", render_markdown(&detail, opts));
        return Ok(());
    }

    use std::io::IsTerminal;
    if plain || !std::io::stdout().is_terminal() {
        print!("{}", render_plain(&detail, opts));
        return Ok(());
    }

    tui::with_terminal(|t| edit_loop(t, conn, cfg, detail)).map(|_| ())
}
