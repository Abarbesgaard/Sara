use anyhow::Result;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Task;

use super::types::{
    BranchOverlap, Detail, EDIT_FIELDS, EditState, Focusable, GraphNode, NOTE_KINDS, TaskTree,
};

const TREE_FETCH_MAX_DEPTH: usize = 6;
const TREE_FETCH_MAX_NODES: usize = 40;
const TREE_FETCH_MAX_CHILDREN: usize = 8;

pub(super) const TREE_COMPACT_DEPTH: usize = 2;
pub(super) const TREE_COMPACT_CHILDREN: usize = 4;

pub(super) fn load_detail(conn: &Connection, cfg: &Config, task: Task) -> Result<Detail> {
    let resolve_ids = |uuids: Vec<uuid::Uuid>| -> Vec<String> {
        uuids
            .iter()
            .filter_map(|u| {
                db::get_task_by_uuid_prefix(conn, &u.to_string()[..8])
                    .ok()
                    .flatten()
            })
            .map(|t| format!("[{}] {}", t.id.unwrap_or(0), t.description))
            .collect()
    };

    let sourced = db::get_task_files_sourced(conn, &task.uuid)?;
    let mut manual_files = vec![];
    let mut suggested_files = vec![];
    for (path, source) in sourced {
        if source == db::SOURCE_SUGGESTED {
            suggested_files.push(path);
        } else {
            manual_files.push(path);
        }
    }

    let project_root = db::get_project(conn, &task.project)?
        .and_then(|p| p.path)
        .map(std::path::PathBuf::from);

    let branch = db::get_task_branch(conn, &task.uuid);
    let overlaps = compute_overlaps(conn, &task, &branch);

    let similar =
        db::similar_tasks(conn, &task.uuid, &task.project, &task.tags).unwrap_or_default();

    let checklist = db::get_checklist(conn, &task.uuid).unwrap_or_default();

    let blockers = db::get_blockers(conn, &task.uuid).unwrap_or_default();
    let blocking_tasks = db::get_blocking(conn, &task.uuid).unwrap_or_default();
    let urgency_breakdown = Some(db::compute_urgency_breakdown(
        &task,
        &cfg.urgency,
        !blockers.is_empty(),
        blocking_tasks.len(),
    ));

    let activity = db::activity_counts(conn, 16 * 7, Some(&task.project)).unwrap_or_default();

    let stats = db::project_stats(conn, &task.project).ok();

    let guide = db::get_guide_fields(conn, &task.uuid).unwrap_or_default();
    let anchors = db::get_task_anchors(conn, &task.uuid).unwrap_or_default();
    let ai_runs = db::get_ai_runs(conn, &task.uuid).unwrap_or_default();
    let head_commit = project_root
        .as_ref()
        .and_then(|p| crate::infrastructure::git::head_commit(p));
    let project_commands = db::get_project_commands(conn, &task.project).unwrap_or_default();
    let tree = build_task_tree(conn, task.uuid);

    Ok(Detail {
        depends_on_ids: dep_ids(conn, &blockers),
        blocked_by: resolve_ids(blockers.clone()),
        blocking: resolve_ids(blocking_tasks.clone()),
        manual_files,
        suggested_files,
        links: db::get_links(conn, &task.uuid)?,
        annotations: db::get_annotations(conn, &task.uuid)?,
        history: db::get_history(conn, &task.uuid)?,
        project_root,
        branch,
        overlaps,
        similar,
        checklist,
        urgency_breakdown,
        activity,
        stats,
        guide,
        anchors,
        ai_runs,
        head_commit,
        project_commands,
        tree,
        task,
    })
}

#[derive(Clone, Copy)]
enum TreeDir {
    Blockers,
    Dependents,
}

pub(super) fn build_task_tree(conn: &Connection, root: uuid::Uuid) -> TaskTree {
    let flags = db::link_flags_by_task(conn).unwrap_or_default();
    let mut up_visited = std::collections::HashSet::from([root]);
    let mut up_budget = TREE_FETCH_MAX_NODES;
    let (blockers, blockers_hidden) = build_tree_level(
        conn,
        root,
        TreeDir::Blockers,
        1,
        &mut up_visited,
        &mut up_budget,
        &flags,
    );
    let mut down_visited = std::collections::HashSet::from([root]);
    let mut down_budget = TREE_FETCH_MAX_NODES;
    let (dependents, dependents_hidden) = build_tree_level(
        conn,
        root,
        TreeDir::Dependents,
        1,
        &mut down_visited,
        &mut down_budget,
        &flags,
    );
    TaskTree {
        blockers,
        blockers_hidden,
        dependents,
        dependents_hidden,
    }
}

fn build_tree_level(
    conn: &Connection,
    parent: uuid::Uuid,
    dir: TreeDir,
    depth: usize,
    visited: &mut std::collections::HashSet<uuid::Uuid>,
    budget: &mut usize,
    flags: &std::collections::HashMap<String, db::LinkFlags>,
) -> (Vec<GraphNode>, usize) {
    if depth > TREE_FETCH_MAX_DEPTH {
        return (Vec::new(), 0);
    }
    let neighbors = match dir {
        TreeDir::Blockers => db::get_dependency_uuids(conn, &parent).unwrap_or_default(),
        TreeDir::Dependents => db::get_blocking(conn, &parent).unwrap_or_default(),
    };
    let hidden = neighbors.len().saturating_sub(TREE_FETCH_MAX_CHILDREN);
    let mut out = Vec::new();
    for u in neighbors.into_iter().take(TREE_FETCH_MAX_CHILDREN) {
        if *budget == 0 {
            break;
        }
        if !visited.insert(u) {
            continue;
        }
        let Some(t) = db::get_task_by_uuid_prefix(conn, &u.to_string()[..8])
            .ok()
            .flatten()
        else {
            continue;
        };
        *budget -= 1;
        let (children, hidden_children) =
            build_tree_level(conn, u, dir, depth + 1, visited, budget, flags);
        out.push(GraphNode {
            uuid: u,
            id: t.id,
            status: t.status,
            badge: flags.get(&u.to_string()).copied(),
            description: t.description,
            children,
            hidden_children,
        });
    }
    (out, hidden)
}

pub(super) fn verification_rows(d: &Detail) -> Vec<(&'static str, String, String)> {
    let mut rows: Vec<(&'static str, String, String)> = Vec::new();
    let pc = &d.project_commands;
    for (label, cmd) in [
        ("setup", &pc.setup_cmd),
        ("test", &pc.test_cmd),
        ("lint", &pc.lint_cmd),
        ("run", &pc.run_cmd),
    ] {
        if let Some(c) = cmd.as_deref().filter(|s| !s.trim().is_empty()) {
            rows.push(("project", label.to_string(), c.to_string()));
        }
    }
    if let Some(meta) = d
        .guide
        .meta_json
        .as_deref()
        .and_then(|m| serde_json::from_str::<serde_json::Value>(m).ok())
        && let Some(obj) = meta.as_object()
    {
        for (k, v) in obj {
            if let Some(s) = v.as_str().filter(|s| !s.trim().is_empty()) {
                let label = k.strip_suffix("_cmd").unwrap_or(k).to_string();
                rows.push(("task", label, s.to_string()));
            }
        }
    }
    rows
}

pub(super) fn guide_is_stale(d: &Detail) -> bool {
    match (&d.head_commit, &d.guide.validated_commit) {
        (Some(h), Some(v)) => h != v,
        (Some(_), None) => false,
        _ => false,
    }
}

pub(super) fn notes_of_kind<'a>(
    d: &'a Detail,
    kind: &str,
) -> Vec<&'a crate::infrastructure::db::Annotation> {
    d.annotations.iter().filter(|a| a.kind == kind).collect()
}

pub(super) fn typed_notes(d: &Detail) -> Vec<&crate::infrastructure::db::Annotation> {
    let mut out = Vec::new();
    for kind in NOTE_KINDS {
        for n in notes_of_kind(d, kind) {
            out.push(n);
        }
    }
    out
}

pub(super) fn dep_ids(conn: &Connection, uuids: &[uuid::Uuid]) -> Vec<i64> {
    uuids
        .iter()
        .filter_map(|u| {
            db::get_task_by_uuid_prefix(conn, &u.to_string()[..8])
                .ok()
                .flatten()
        })
        .filter_map(|t| t.id)
        .collect()
}

fn dep_labels(conn: &Connection, uuids: &[uuid::Uuid]) -> Vec<String> {
    uuids
        .iter()
        .filter_map(|u| {
            db::get_task_by_uuid_prefix(conn, &u.to_string()[..8])
                .ok()
                .flatten()
        })
        .map(|t| format!("[{}] {}", t.id.unwrap_or(0), t.description))
        .collect()
}

pub(super) fn depends_on_display(d: &Detail) -> String {
    d.depends_on_ids
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

pub(super) fn reconcile_dependencies(
    conn: &Connection,
    cfg: &Config,
    detail: &mut Detail,
    value: &str,
) -> std::result::Result<(), String> {
    let task_uuid = detail.task.uuid;

    let tokens: Vec<&str> = value
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    let mut desired: Vec<uuid::Uuid> = Vec::new();
    for tok in &tokens {
        let t = db::resolve_task(conn, tok).map_err(|_| format!("no task '{tok}'"))?;
        if t.uuid == task_uuid {
            return Err("a task cannot depend on itself".into());
        }
        if !desired.contains(&t.uuid) {
            desired.push(t.uuid);
        }
    }

    let current = db::get_blockers(conn, &task_uuid).unwrap_or_default();

    for u in &current {
        if !desired.contains(u) {
            db::remove_dependency(conn, &task_uuid, u).map_err(|e| e.to_string())?;
        }
    }
    for u in &desired {
        if !current.contains(u) {
            db::add_dependency(conn, &task_uuid, u).map_err(|e| e.to_string())?;
        }
    }

    db::refresh_urgency(conn, &cfg.urgency, &task_uuid).ok();
    for u in current.iter().chain(desired.iter()) {
        db::refresh_urgency(conn, &cfg.urgency, u).ok();
    }

    reload_dep_detail(conn, cfg, detail);
    Ok(())
}

pub(super) fn reload_dep_detail(conn: &Connection, cfg: &Config, detail: &mut Detail) {
    let uuid = detail.task.uuid;
    let blockers = db::get_blockers(conn, &uuid).unwrap_or_default();
    let blocking = db::get_blocking(conn, &uuid).unwrap_or_default();
    detail.depends_on_ids = dep_ids(conn, &blockers);
    detail.blocked_by = dep_labels(conn, &blockers);
    detail.blocking = dep_labels(conn, &blocking);
    if let Some(t) = db::get_task_by_uuid_prefix(conn, &uuid.to_string()[..8])
        .ok()
        .flatten()
    {
        detail.task.urgency = t.urgency;
    }
    detail.urgency_breakdown = Some(db::compute_urgency_breakdown(
        &detail.task,
        &cfg.urgency,
        !blockers.is_empty(),
        blocking.len(),
    ));
    detail.history = db::get_history(conn, &uuid).unwrap_or_default();
    detail.tree = build_task_tree(conn, uuid);
}

pub(super) fn compute_overlaps(
    conn: &Connection,
    task: &Task,
    branch_rec: &Option<db::BranchRecord>,
) -> Vec<BranchOverlap> {
    let my_files: std::collections::HashSet<String> = branch_rec
        .as_ref()
        .and_then(|b| b.files.as_ref())
        .map(|fs| fs.iter().cloned().collect())
        .unwrap_or_default();

    if my_files.is_empty() {
        return vec![];
    }

    let others =
        db::branched_pending_in_project(conn, &task.project, &task.uuid).unwrap_or_default();

    let mut result = vec![];
    for (id, desc, other_rec) in others {
        let other_files: std::collections::HashSet<String> = other_rec
            .files
            .as_ref()
            .map(|fs| fs.iter().cloned().collect())
            .unwrap_or_default();
        let mut shared: Vec<String> = my_files.intersection(&other_files).cloned().collect();
        if !shared.is_empty() {
            shared.sort();
            result.push(BranchOverlap {
                id,
                description: desc,
                branch: other_rec.branch,
                shared_files: shared,
            });
        }
    }
    result
}

pub(super) fn focusables(d: &Detail, show_notes: bool) -> Vec<Focusable> {
    let mut v: Vec<Focusable> = EDIT_FIELDS.iter().map(|f| Focusable::Field(*f)).collect();
    for (i, note) in typed_notes(d).iter().enumerate() {
        if show_notes || note.kind == "risk" {
            v.push(Focusable::Note(i));
        }
    }
    for i in 0..d.links.len() {
        v.push(Focusable::Link(i));
    }
    for f in d.manual_files.iter().chain(d.suggested_files.iter()) {
        v.push(Focusable::File(f.clone()));
    }
    for i in 0..d.anchors.len() {
        v.push(Focusable::Anchor(i));
    }
    for i in 0..d.checklist.len() {
        v.push(Focusable::Checklist(i));
    }
    let comment_count = d
        .annotations
        .iter()
        .filter(|a| a.kind == "comment" && a.target_kind.as_deref() != Some("anchor"))
        .count();
    for i in 0..comment_count {
        v.push(Focusable::Comment(i));
    }
    v
}

pub(super) fn checklist_focus_index(d: &Detail, show_notes: bool, item_id: i64) -> Option<usize> {
    focusables(d, show_notes).iter().position(|f| match f {
        Focusable::Checklist(i) => d.checklist.get(*i).map(|c| c.id) == Some(item_id),
        _ => false,
    })
}

pub(super) fn reorder_focused_step(
    conn: &Connection,
    st: &mut EditState,
    current: &Option<Focusable>,
    up: bool,
) {
    let Some(Focusable::Checklist(i)) = current else {
        return;
    };
    let Some(id) = st.detail.checklist.get(*i).map(|c| c.id) else {
        return;
    };
    if db::move_step(conn, id, up).unwrap_or(false) {
        st.detail.checklist = db::get_checklist(conn, &st.detail.task.uuid).unwrap_or_default();
        if let Some(p) = checklist_focus_index(&st.detail, st.show_notes, id) {
            st.selected = p;
        }
    }
}

pub(super) fn find_pr_url(d: &Detail) -> Option<String> {
    pr_url_from(&d.links, &d.annotations)
}

fn pr_url_from(links: &[db::Link], annotations: &[db::Annotation]) -> Option<String> {
    if let Some(link) = links.iter().find(|l| db::is_pr_link(&l.url)) {
        return Some(link.url.clone());
    }
    annotations.iter().find_map(|a| {
        a.text
            .split_whitespace()
            .map(|tok| tok.trim_matches(|c: char| !c.is_alphanumeric() && c != '/'))
            .find(|tok| db::is_pr_link(tok))
            .map(str::to_string)
    })
}

pub(super) fn open_url(raw: &str) {
    let url = if raw.starts_with("www.") {
        format!("https://{raw}")
    } else {
        raw.to_string()
    };
    let cmd = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        "xdg-open"
    };
    let _ = std::process::Command::new(cmd)
        .arg(&url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

pub(super) fn editor_command() -> String {
    if let Ok(v) = std::env::var("VISUAL")
        && !v.trim().is_empty()
    {
        return v;
    }
    if let Ok(v) = std::env::var("EDITOR")
        && !v.trim().is_empty()
    {
        return v;
    }
    for candidate in ["nvim", "vim", "nano", "vi"] {
        if std::process::Command::new("which")
            .arg(candidate)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
        {
            return candidate.to_string();
        }
    }
    "vi".to_string()
}

pub(super) fn open_in_editor(path: &std::path::Path) -> std::io::Result<()> {
    let editor = editor_command();
    let mut parts = editor.split_whitespace();
    let bin = parts.next().unwrap_or("vi");
    let mut cmd = std::process::Command::new(bin);
    cmd.args(parts).arg(path);
    cmd.status().map(|_| ())
}

pub(super) fn edit_text_via_external_editor(initial: &str) -> std::io::Result<Option<String>> {
    let path = std::env::temp_dir().join(format!("sara-edit-{}.md", uuid::Uuid::new_v4()));
    std::fs::write(&path, initial)?;

    let editor = editor_command();
    let mut parts = editor.split_whitespace();
    let bin = parts.next().unwrap_or("vi");
    let status = std::process::Command::new(bin)
        .args(parts)
        .arg(&path)
        .status();

    let result = match status {
        Ok(s) if s.success() => match std::fs::read_to_string(&path) {
            Ok(edited) if edited.trim() != initial.trim() => Some(edited),
            _ => None,
        },
        _ => None,
    };
    let _ = std::fs::remove_file(&path);
    Ok(result)
}

pub(super) fn comment_target(
    d: &Detail,
    focus: &Option<Focusable>,
) -> (Option<String>, Option<String>) {
    match focus {
        Some(Focusable::Checklist(i)) => {
            if let Some(item) = d.checklist.get(*i) {
                let kind = if item.kind == db::STEP_KIND_ACCEPTANCE {
                    "acceptance"
                } else {
                    "step"
                };
                return (Some(kind.to_string()), Some(item.id.to_string()));
            }
            (None, None)
        }
        Some(Focusable::Anchor(i)) => {
            if let Some(anchor) = d.anchors.get(*i) {
                return (Some("anchor".to_string()), Some(anchor.path.clone()));
            }
            (None, None)
        }
        Some(Focusable::File(path)) => (Some("anchor".to_string()), Some(path.clone())),
        Some(Focusable::Note(i)) => {
            if let Some(note) = typed_notes(d).get(*i) {
                return (Some("note".to_string()), Some(note.id.to_string()));
            }
            (None, None)
        }
        Some(Focusable::Comment(i)) => {
            let comments: Vec<&crate::infrastructure::db::Annotation> = d
                .annotations
                .iter()
                .filter(|a| a.kind == "comment")
                .collect();
            if let Some(a) = comments.get(*i) {
                return (Some("note".to_string()), Some(a.id.to_string()));
            }
            (None, None)
        }
        _ => (None, None),
    }
}

pub(super) fn feedback_for_focus<'a>(
    d: &'a Detail,
    focus: &Option<Focusable>,
) -> Vec<&'a crate::infrastructure::db::Annotation> {
    if let Some(Focusable::Comment(i)) = focus {
        let comments: Vec<&crate::infrastructure::db::Annotation> = d
            .annotations
            .iter()
            .filter(|a| a.kind == "comment")
            .collect();
        return comments
            .get(*i)
            .copied()
            .map(|a| vec![a])
            .unwrap_or_default();
    }
    let (tk, tid) = comment_target(d, focus);
    let mut v: Vec<&crate::infrastructure::db::Annotation> = d
        .annotations
        .iter()
        .filter(|a| {
            a.kind == "comment" && a.status == "open" && a.target_kind == tk && a.target_id == tid
        })
        .collect();
    v.reverse();
    v
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/info/handler.rs"]
mod tests;
