use anyhow::Result;
use chrono::{Local, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    Frame, Terminal,
    backend::Backend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use rusqlite::Connection;
use tui_textarea::TextArea;

use crate::config::Config;
use crate::db;
use crate::model::{Priority, Task, format_duration};
use crate::tui;

struct Detail {
    task: Task,
    blocked_by: Vec<String>,
    blocking: Vec<String>,
    /// Display IDs of the tasks this task currently depends on (pending
    /// blockers). Used to pre-fill and reconcile the editable "Depends on" field.
    depends_on_ids: Vec<i64>,
    /// Files the user attached themselves.
    manual_files: Vec<String>,
    /// Files attached as suggestions.
    suggested_files: Vec<String>,
    links: Vec<crate::db::Link>,
    annotations: Vec<crate::db::Annotation>,
    history: Vec<crate::db::HistoryEntry>,
    /// Absolute project root, used to open relative file paths.
    project_root: Option<std::path::PathBuf>,
    /// Persisted branch snapshot (set via `sara addbranch`, populated on `sara stop`).
    branch: Option<crate::db::BranchRecord>,
    /// Tasks in the same project whose snapshot files overlap with this task's.
    overlaps: Vec<BranchOverlap>,
    /// Other pending tasks in the same project sharing at least one tag.
    similar: Vec<(i64, String, f64)>,
    /// Checklist items for this task.
    checklist: Vec<crate::db::ChecklistItem>,
    /// Urgency score components.
    urgency_breakdown: Option<crate::db::UrgencyBreakdown>,
    /// Daily activity counts for the task's project (last ~16 weeks).
    activity: std::collections::HashMap<chrono::NaiveDate, u32>,
    /// Aggregated stats for the project.
    stats: Option<crate::db::ProjectStats>,
    /// Guide fields: assignment, rationale, freshness, meta.
    guide: crate::db::TaskGuideFields,
    /// Code anchors (relevant files with reasons / symbols / lines).
    anchors: Vec<crate::db::Anchor>,
    /// AI run audit trail.
    ai_runs: Vec<crate::db::AiRun>,
    /// Current project HEAD commit, for the freshness banner.
    head_commit: Option<String>,
    /// Project-level setup/test/lint/run commands (verification context).
    project_commands: crate::db::ProjectCommands,
    /// The dependency chain (feature) this task belongs to, in blockers-first
    /// order. Empty when the task has no linked tasks. Used by the right-hand
    /// "Feature chain" panel to show progress and highlight the current task.
    chain: Vec<Task>,
}

struct BranchOverlap {
    id: i64,
    description: String,
    branch: String,
    shared_files: Vec<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum EditField {
    Description,
    Project,
    Priority,
    Due,
    Tags,
    Estimate,
    Recur,
    DependsOn,
}

const EDIT_FIELDS: [EditField; 8] = [
    EditField::Description,
    EditField::Project,
    EditField::Priority,
    EditField::Due,
    EditField::Tags,
    EditField::Estimate,
    EditField::Recur,
    EditField::DependsOn,
];

impl EditField {
    fn label(&self) -> &'static str {
        match self {
            EditField::Description => "Description",
            EditField::Project => "Project",
            EditField::Priority => "Priority",
            EditField::Due => "Due",
            EditField::Tags => "Tags",
            EditField::Estimate => "Estimate",
            EditField::Recur => "Recur",
            EditField::DependsOn => "Depends on",
        }
    }
}

/// Something the cursor can land on in the detail view.
#[derive(Clone, PartialEq)]
enum Focusable {
    Field(EditField),
    File(String),
    Link(usize),
    Checklist(usize),
    /// Index into `d.anchors` (code anchors).
    Anchor(usize),
    /// Index into the task-level comment list (annotations where kind="comment").
    Comment(usize),
    /// Index into the flat list of typed notes (finding, constraint, …).
    Note(usize),
}

/// Ordered list of focusable items — matches on-screen render order so ↑/↓ feels natural.
/// Screen order: metadata fields → typed notes (findings/constraints/…) → links →
///               manual files → anchors → checklist → task-level comments.
fn focusables(d: &Detail) -> Vec<Focusable> {
    let mut v: Vec<Focusable> = EDIT_FIELDS.iter().map(|f| Focusable::Field(*f)).collect();
    // Typed notes appear right after the metadata block in the TUI.
    for i in 0..typed_notes(d).len() {
        v.push(Focusable::Note(i));
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
    // Task-level comments (anchor-threaded ones are shown inline under their element).
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

/// Open a URL in the OS default browser (non-blocking). Adds a scheme for
/// bare `www.` style links.
fn open_url(raw: &str) {
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

/// Pick the user's terminal editor: $VISUAL, then $EDITOR, then the first of
/// nvim/vim/nano that exists on PATH.
fn editor_command() -> String {
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

/// Launch the editor on `path`, inheriting stdio so it takes over the terminal.
/// The caller is responsible for suspending/resuming the TUI around this.
fn open_in_editor(path: &std::path::Path) -> std::io::Result<()> {
    // $EDITOR may contain args (e.g. "code -w"); split on whitespace.
    let editor = editor_command();
    let mut parts = editor.split_whitespace();
    let bin = parts.next().unwrap_or("vi");
    let mut cmd = std::process::Command::new(bin);
    cmd.args(parts).arg(path);
    cmd.status().map(|_| ())
}

fn load_detail(conn: &Connection, cfg: &Config, task: Task) -> Result<Detail> {
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

    // Branch snapshot and overlap detection (pure stored-data, no live git).
    let branch = db::get_task_branch(conn, &task.uuid);
    let overlaps = compute_overlaps(conn, &task, &branch);

    // Similar tasks (shared tags, same project)
    let similar =
        db::similar_tasks(conn, &task.uuid, &task.project, &task.tags).unwrap_or_default();

    // Checklist
    let checklist = db::get_checklist(conn, &task.uuid).unwrap_or_default();

    // Urgency breakdown
    let blockers = db::get_blockers(conn, &task.uuid).unwrap_or_default();
    let blocking_tasks = db::get_blocking(conn, &task.uuid).unwrap_or_default();
    let urgency_breakdown = Some(db::compute_urgency_breakdown(
        &task,
        &cfg.urgency,
        !blockers.is_empty(),
        blocking_tasks.len(),
    ));

    // Activity heatmap for the project (last 16 weeks)
    let activity = db::activity_counts(conn, 16 * 7, Some(&task.project)).unwrap_or_default();

    // Project stats
    let stats = db::project_stats(conn, &task.project).ok();

    let guide = db::get_guide_fields(conn, &task.uuid).unwrap_or_default();
    let anchors = db::get_task_anchors(conn, &task.uuid).unwrap_or_default();
    let ai_runs = db::get_ai_runs(conn, &task.uuid).unwrap_or_default();
    let head_commit = project_root
        .as_ref()
        .and_then(|p| crate::git::head_commit(p));
    let project_commands = db::get_project_commands(conn, &task.project).unwrap_or_default();
    let chain = db::feature_chain(conn, &task.uuid).unwrap_or_default();

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
        chain,
        task,
    })
}

/// Verification/execution commands an executor should run, gathered from the
/// project record (setup/test/lint/run) and the task's `meta_json` grab-bag
/// (e.g. task-specific test_cmd / lint_cmd). Returns (scope, label, command).
fn verification_rows(d: &Detail) -> Vec<(&'static str, String, String)> {
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
    // Task-level commands from the meta_json grab-bag (render any "*_cmd" key).
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

/// Whether the guide is stale (validated against a different commit than HEAD).
fn guide_is_stale(d: &Detail) -> bool {
    match (&d.head_commit, &d.guide.validated_commit) {
        (Some(h), Some(v)) => h != v,
        (Some(_), None) => false,
        _ => false,
    }
}

/// Annotations of a given kind (findings, constraints, …) authored on the task.
fn notes_of_kind<'a>(d: &'a Detail, kind: &str) -> Vec<&'a crate::db::Annotation> {
    d.annotations.iter().filter(|a| a.kind == kind).collect()
}

/// All typed notes in render order (finding, constraint, assumption, …).
const NOTE_KINDS: [&str; 8] = [
    "finding",
    "constraint",
    "assumption",
    "open_question",
    "non_goal",
    "decision",
    "risk",
    "pattern",
];
fn typed_notes(d: &Detail) -> Vec<&crate::db::Annotation> {
    let mut out = Vec::new();
    for kind in NOTE_KINDS {
        for n in notes_of_kind(d, kind) {
            out.push(n);
        }
    }
    out
}

/// Resolve dependency uuids to their display IDs (skips tasks without an id).
fn dep_ids(conn: &Connection, uuids: &[uuid::Uuid]) -> Vec<i64> {
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

/// Resolve dependency uuids to "[id] description" labels.
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

/// Current value shown (and pre-filled when editing) for the "Depends on" field:
/// the task's pending blocker IDs, space-separated.
fn depends_on_display(d: &Detail) -> String {
    d.depends_on_ids
        .iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Apply an edited "Depends on" value: reconcile the task's pending dependency
/// edges with the IDs the user typed (space/comma separated). Adds and removes
/// edges as needed, then refreshes urgency and reloads dependency detail.
/// Returns a human-readable error (kept on screen) if a token can't be resolved
/// or the change would be invalid (self/cycle).
fn reconcile_dependencies(
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

/// Refresh the dependency-derived parts of the detail view after an edit.
fn reload_dep_detail(conn: &Connection, cfg: &Config, detail: &mut Detail) {
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
    // Dependency edits reshape the feature chain.
    detail.chain = db::feature_chain(conn, &uuid).unwrap_or_default();
}

fn compute_overlaps(
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

/// `sara info --json` — emit the full guide (assembled by the `task_guide` view)
/// plus freshness + open-feedback, in one machine-readable document.
pub fn run_json(conn: &Connection, _cfg: &Config, id_or_uuid: &str) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    let mut guide = db::guide_json(conn, &task.uuid)?;

    // Freshness: compare the validated commit against the project's current HEAD.
    let head = db::get_project(conn, &task.project)
        .ok()
        .flatten()
        .and_then(|p| p.path)
        .and_then(|path| crate::git::head_commit(std::path::Path::new(&path)));
    let validated = db::get_guide_fields(conn, &task.uuid)?.validated_commit;
    let stale = match (&head, &validated) {
        (Some(h), Some(v)) => h != v,
        _ => false,
    };

    // Open feedback the agent should act on (flagged-for-reconsider first).
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
    }

    println!("{}", serde_json::to_string_pretty(&guide)?);
    Ok(())
}

pub fn run(conn: &Connection, cfg: &Config, id_or_uuid: &str) -> Result<()> {
    let task = db::resolve_task(conn, id_or_uuid)?;
    let detail = load_detail(conn, cfg, task)?;

    // If not a TTY, fall back to plain text output (read-only).
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() {
        print_plain(&detail);
        return Ok(());
    }

    let mut terminal = tui::init_terminal()?;
    let result = edit_loop(&mut terminal, conn, cfg, detail);
    tui::restore_terminal()?;
    result.map(|_| ())
}

struct EditState {
    detail: Detail,
    selected: usize,
    editing: bool,
    /// True while typing a comment anchored to the focused element.
    commenting: bool,
    editor: TextArea<'static>,
    due_error: bool,
    /// Error from the last "Depends on" commit, shown until the next edit.
    dep_error: Option<String>,
    scroll: u16,
}

/// The (target_kind, target_id) a comment would anchor to given the focused item.
fn comment_target(d: &Detail, focus: &Option<Focusable>) -> (Option<String>, Option<String>) {
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
            // Replying to a comment: anchor to the note itself.
            let comments: Vec<&crate::db::Annotation> = d
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

/// Open feedback annotations anchored to the focused element (most recent first).
fn feedback_for_focus<'a>(
    d: &'a Detail,
    focus: &Option<Focusable>,
) -> Vec<&'a crate::db::Annotation> {
    // When the cursor is ON a comment, r/x act on that comment directly.
    if let Some(Focusable::Comment(i)) = focus {
        let comments: Vec<&crate::db::Annotation> = d
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
    let mut v: Vec<&crate::db::Annotation> = d
        .annotations
        .iter()
        .filter(|a| {
            a.kind == "comment" && a.status == "open" && a.target_kind == tk && a.target_id == tid
        })
        .collect();
    v.reverse();
    v
}

fn edit_loop<B: Backend>(
    terminal: &mut Terminal<B>,
    conn: &Connection,
    cfg: &Config,
    detail: Detail,
) -> Result<()> {
    let mut st = EditState {
        detail,
        selected: 0,
        editing: false,
        commenting: false,
        editor: TextArea::default(),
        due_error: false,
        dep_error: None,
        scroll: 0,
    };

    loop {
        terminal.draw(|f| render(f, &st))?;

        if !event::poll(std::time::Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }

        let items = focusables(&st.detail);
        // Keep the cursor in range (links/files can disappear after a reload).
        if !items.is_empty() && st.selected >= items.len() {
            st.selected = items.len() - 1;
        }
        let current = items.get(st.selected).cloned();
        let current_field = match &current {
            Some(Focusable::Field(f)) => Some(*f),
            _ => None,
        };

        if st.commenting {
            match key.code {
                KeyCode::Enter => {
                    let text = st.editor.lines().join(" ");
                    if !text.trim().is_empty() {
                        let (tk, tid) = comment_target(&st.detail, &current);
                        let _ = db::add_annotation_full(
                            conn,
                            &st.detail.task.uuid,
                            text.trim(),
                            db::NOTE_KIND_COMMENT,
                            "human",
                            tk.as_deref(),
                            tid.as_deref(),
                            false,
                        );
                        st.detail.annotations =
                            db::get_annotations(conn, &st.detail.task.uuid).unwrap_or_default();
                    }
                    st.commenting = false;
                }
                KeyCode::Esc => st.commenting = false,
                _ => {
                    st.editor.input(key);
                }
            }
        } else if st.editing {
            let field = current_field.unwrap_or(EditField::Description);
            match key.code {
                KeyCode::Enter => {
                    let value = st.editor.lines().join("");
                    if field == EditField::DependsOn {
                        match reconcile_dependencies(conn, cfg, &mut st.detail, &value) {
                            Ok(()) => {
                                st.editing = false;
                                st.dep_error = None;
                            }
                            Err(e) => st.dep_error = Some(e),
                        }
                        continue;
                    }
                    if field == EditField::Due
                        && !value.trim().is_empty()
                        && !crate::dates::is_valid_due(&value)
                    {
                        st.due_error = true;
                        continue;
                    }
                    apply_field(&mut st.detail.task, field, &value, cfg);
                    save(conn, cfg, &mut st.detail)?;
                    st.editing = false;
                    st.due_error = false;
                }
                KeyCode::Esc => {
                    st.editing = false;
                    st.due_error = false;
                    st.dep_error = None;
                }
                _ => {
                    st.editor.input(key);
                    if field == EditField::Due {
                        let v = st.editor.lines().join("");
                        st.due_error = !v.trim().is_empty() && !crate::dates::is_valid_due(&v);
                    }
                }
            }
        } else {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                KeyCode::Down | KeyCode::Char('j') => {
                    if !items.is_empty() {
                        st.selected = (st.selected + 1).min(items.len() - 1);
                    }
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    st.selected = st.selected.saturating_sub(1);
                }
                KeyCode::PageDown => st.scroll = st.scroll.saturating_add(5),
                KeyCode::PageUp => st.scroll = st.scroll.saturating_sub(5),
                KeyCode::Left if current_field == Some(EditField::Priority) => {
                    cycle_priority(&mut st.detail.task, false);
                    save(conn, cfg, &mut st.detail)?;
                }
                KeyCode::Right if current_field == Some(EditField::Priority) => {
                    cycle_priority(&mut st.detail.task, true);
                    save(conn, cfg, &mut st.detail)?;
                }
                KeyCode::Enter | KeyCode::Char('e') => match current {
                    Some(Focusable::Field(EditField::Priority)) => {
                        cycle_priority(&mut st.detail.task, true);
                        save(conn, cfg, &mut st.detail)?;
                    }
                    Some(Focusable::Field(field)) => {
                        st.editor = if field == EditField::DependsOn {
                            let mut ta = TextArea::default();
                            ta.insert_str(depends_on_display(&st.detail));
                            ta
                        } else {
                            editor_for(&st.detail.task, field)
                        };
                        st.editing = true;
                        st.due_error = false;
                        st.dep_error = None;
                    }
                    Some(Focusable::Link(i)) => {
                        if let Some(link) = st.detail.links.get(i) {
                            open_url(&link.url);
                        }
                    }
                    Some(Focusable::File(path)) => {
                        if db::is_url(&path) {
                            // URL stored as a file (legacy attach) -> browser.
                            open_url(&path);
                        } else {
                            // Real file -> open in the user's editor. Hand the
                            // terminal back while the editor runs.
                            let target = st
                                .detail
                                .project_root
                                .as_ref()
                                .map(|r| r.join(&path))
                                .unwrap_or_else(|| std::path::PathBuf::from(&path));
                            tui::suspend()?;
                            let _ = open_in_editor(&target);
                            tui::resume()?;
                            terminal.clear()?;
                        }
                    }
                    Some(Focusable::Checklist(i)) => {
                        if let Some(item) = st.detail.checklist.get(i) {
                            let _ = db::toggle_checklist_item(conn, item.id);
                            st.detail.checklist =
                                db::get_checklist(conn, &st.detail.task.uuid).unwrap_or_default();
                        }
                    }
                    // Enter on an anchor opens the file in the editor.
                    Some(Focusable::Anchor(i)) => {
                        if let Some(anchor) = st.detail.anchors.get(i) {
                            let target = st
                                .detail
                                .project_root
                                .as_ref()
                                .map(|r| r.join(&anchor.path))
                                .unwrap_or_else(|| std::path::PathBuf::from(&anchor.path));
                            tui::suspend()?;
                            let _ = open_in_editor(&target);
                            tui::resume()?;
                            terminal.clear()?;
                        }
                    }
                    // Enter on a comment opens the comment input to reply.
                    Some(Focusable::Comment(_)) => {
                        st.editor = TextArea::default();
                        st.commenting = true;
                    }
                    // Enter on a typed note opens the comment input to comment on it.
                    Some(Focusable::Note(_)) => {
                        st.editor = TextArea::default();
                        st.commenting = true;
                    }
                    None => {}
                },
                KeyCode::Char(' ') => {
                    if let Some(Focusable::Checklist(i)) = &current
                        && let Some(item) = st.detail.checklist.get(*i)
                    {
                        let _ = db::toggle_checklist_item(conn, item.id);
                        st.detail.checklist =
                            db::get_checklist(conn, &st.detail.task.uuid).unwrap_or_default();
                    }
                }
                // ── Review & comment loop ────────────────────────────────────
                KeyCode::Char('c') => {
                    st.editor = TextArea::default();
                    st.commenting = true;
                }
                KeyCode::Char('r') => {
                    // Mark the focused element for reconsideration.
                    // If it already has an open comment, toggle the flag on it.
                    // If not, create a new comment with request_revision=true so
                    // the element is flagged even without a written note.
                    let fb = feedback_for_focus(&st.detail, &current);
                    if let Some(existing) = fb.first() {
                        let _ =
                            db::set_request_revision(conn, existing.id, !existing.request_revision);
                    } else {
                        // No existing comment — auto-create a reconsider marker.
                        let (tk, tid) = comment_target(&st.detail, &current);
                        let _ = db::add_annotation_full(
                            conn,
                            &st.detail.task.uuid,
                            "⟳ reconsider this",
                            db::NOTE_KIND_COMMENT,
                            "human",
                            tk.as_deref(),
                            tid.as_deref(),
                            true, // request_revision = true
                        );
                    }
                    st.detail.annotations =
                        db::get_annotations(conn, &st.detail.task.uuid).unwrap_or_default();
                }
                KeyCode::Char('x') => {
                    // Resolve the focused element's latest open feedback.
                    if let Some(fb) = feedback_for_focus(&st.detail, &current).first() {
                        let _ = db::resolve_annotation(conn, fb.id, None);
                        st.detail.annotations =
                            db::get_annotations(conn, &st.detail.task.uuid).unwrap_or_default();
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}

fn editor_for(task: &Task, field: EditField) -> TextArea<'static> {
    let value = current_value(task, field);
    let mut ta = TextArea::default();
    ta.insert_str(&value);
    ta
}

fn current_value(task: &Task, field: EditField) -> String {
    match field {
        EditField::Description => task.description.clone(),
        EditField::Project => task.project.clone(),
        EditField::Priority => task
            .priority
            .as_ref()
            .map(|p| p.label().to_string())
            .unwrap_or_default(),
        EditField::Due => task
            .due
            .map(|d| d.with_timezone(&Local).format("%Y-%m-%d").to_string())
            .unwrap_or_default(),
        EditField::Tags => task.tags.join(", "),
        EditField::Estimate => task
            .estimate_mins
            .map(|m| {
                if m >= 60 {
                    let h = m / 60;
                    let rem = m % 60;
                    if rem == 0 {
                        format!("{h}h")
                    } else {
                        format!("{h}h{rem}m")
                    }
                } else {
                    format!("{m}m")
                }
            })
            .unwrap_or_default(),
        EditField::Recur => task.recur.clone().unwrap_or_default(),
        // Dependencies live in a separate table; handled via depends_on_display.
        EditField::DependsOn => String::new(),
    }
}

fn apply_field(task: &mut Task, field: EditField, value: &str, cfg: &Config) {
    match field {
        EditField::Description => {
            if !value.trim().is_empty() {
                task.description = value.trim().to_string();
            }
        }
        EditField::Project => {
            if !value.trim().is_empty() {
                task.project = value.trim().to_string();
            }
        }
        EditField::Due => {
            if value.trim().is_empty() {
                task.due = None;
            } else {
                task.due = crate::commands::add::parse_due(value, cfg);
            }
        }
        EditField::Tags => {
            task.tags = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        EditField::Priority => {}
        EditField::Estimate => {
            task.estimate_mins = parse_duration_to_mins(value);
        }
        EditField::Recur => {
            let v = value.trim().to_lowercase();
            task.recur = if v.is_empty() { None } else { Some(v) };
        }
        // Reconciled against the dependencies table in reconcile_dependencies.
        EditField::DependsOn => {}
    }
}

fn cycle_priority(task: &mut Task, forward: bool) {
    task.priority = match (&task.priority, forward) {
        (None, true) => Some(Priority::L),
        (Some(Priority::L), true) => Some(Priority::M),
        (Some(Priority::M), true) => Some(Priority::H),
        (Some(Priority::H), true) => None,
        (None, false) => Some(Priority::H),
        (Some(Priority::H), false) => Some(Priority::M),
        (Some(Priority::M), false) => Some(Priority::L),
        (Some(Priority::L), false) => None,
    };
}

fn save(conn: &Connection, cfg: &Config, detail: &mut Detail) -> Result<()> {
    let task = &mut detail.task;
    task.modified = Utc::now();
    task.urgency = db::compute_urgency(task, &cfg.urgency, false, 0);
    db::update_task(conn, task)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;
    // Pull back the authoritative urgency (refresh accounts for blocking).
    if let Some(t) = db::get_task_by_uuid_prefix(conn, &task.uuid.to_string()[..8])? {
        task.urgency = t.urgency;
    }
    detail.history = db::get_history(conn, &detail.task.uuid)?;
    // Reload branch / overlaps in case project changed.
    detail.branch = db::get_task_branch(conn, &detail.task.uuid);
    detail.overlaps = compute_overlaps(conn, &detail.task, &detail.branch);
    // Reload similar tasks and checklist after any save.
    detail.similar = db::similar_tasks(
        conn,
        &detail.task.uuid,
        &detail.task.project,
        &detail.task.tags,
    )
    .unwrap_or_default();
    detail.checklist = db::get_checklist(conn, &detail.task.uuid).unwrap_or_default();
    let blockers = db::get_blockers(conn, &detail.task.uuid).unwrap_or_default();
    let blocking_tasks = db::get_blocking(conn, &detail.task.uuid).unwrap_or_default();
    detail.urgency_breakdown = Some(db::compute_urgency_breakdown(
        &detail.task,
        &cfg.urgency,
        !blockers.is_empty(),
        blocking_tasks.len(),
    ));
    // A project change can move the task into a different feature chain.
    detail.chain = db::feature_chain(conn, &detail.task.uuid).unwrap_or_default();
    Ok(())
}

fn render(f: &mut Frame, st: &EditState) {
    let area = f.area();
    let d = &st.detail;

    let history_height: u16 = if d.history.is_empty() {
        0
    } else {
        (d.history.len() as u16 + 2).min(6) // border (2) + up to 4 most-recent entries
    };

    let constraints = if st.editing || st.commenting {
        if history_height > 0 {
            vec![
                Constraint::Min(1),
                Constraint::Length(history_height),
                Constraint::Length(3),
                Constraint::Length(1),
            ]
        } else {
            vec![
                Constraint::Min(1),
                Constraint::Length(3),
                Constraint::Length(1),
            ]
        }
    } else if history_height > 0 {
        vec![
            Constraint::Min(1),
            Constraint::Length(history_height),
            Constraint::Length(1),
        ]
    } else {
        vec![Constraint::Min(1), Constraint::Length(1)]
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints)
        .split(area);

    let t = &d.task;
    let active = t.is_active();
    let title = format!(
        " Task {}{} ",
        t.id.map(|i| i.to_string()).unwrap_or_else(|| "-".into()),
        if active { "  ● ACTIVE" } else { "" }
    );

    let mut lines: Vec<Line> = vec![];

    // ── Editable fields
    for (i, field) in EDIT_FIELDS.iter().enumerate() {
        let selected = !st.editing && i == st.selected;
        let editing_this = st.editing && i == st.selected;
        let value = if editing_this {
            "…(editing below)".to_string()
        } else if *field == EditField::DependsOn {
            let v = depends_on_display(d);
            if v.is_empty() { "-".to_string() } else { v }
        } else {
            let v = current_value(t, *field);
            if v.is_empty() { "-".to_string() } else { v }
        };
        lines.push(editable_line(field.label(), &value, selected, *field, t));
    }

    // ── Read-only fields
    lines.push(field_line("Status", &t.status.to_string()));

    // Age / deadline counter line
    {
        let age_days = (Utc::now() - t.entry).num_days();
        let age_str = if age_days == 0 {
            "today".to_string()
        } else if age_days == 1 {
            "1 day ago".to_string()
        } else {
            format!("{age_days} days ago")
        };
        let deadline_str = if let Some(due) = t.due {
            let diff = (due - Utc::now()).num_days();
            if diff < 0 {
                format!(
                    "  ·  {} day{} overdue",
                    -diff,
                    if diff == -1 { "" } else { "s" }
                )
            } else if diff == 0 {
                "  ·  due today".to_string()
            } else if diff == 1 {
                "  ·  due tomorrow".to_string()
            } else {
                format!("  ·  due in {diff} days")
            }
        } else {
            String::new()
        };
        let overdue = t.due.map(|d| d < Utc::now()).unwrap_or(false);
        lines.push(Line::from(vec![
            key_span("Age"),
            Span::styled(
                format!("{age_str}{deadline_str}"),
                Style::default().fg(if overdue { Color::Red } else { Color::DarkGray }),
            ),
        ]));
    }

    let time_str = if active {
        format!(
            "{}  (running, this session {})",
            format_duration(t.total_time_spent()),
            format_duration(t.total_time_spent() - t.time_spent)
        )
    } else if t.time_spent > 0 {
        format_duration(t.time_spent)
    } else {
        "-".to_string()
    };
    // Time spent / estimate on the same conceptual row
    {
        let estimate_str = t
            .estimate_mins
            .map(|m| {
                let spent_mins = t.total_time_spent() / 60;
                let pct = if m > 0 {
                    (spent_mins * 100 / m).min(999)
                } else {
                    0
                };
                format!(
                    " / est {} ({pct}%)",
                    if m >= 60 {
                        let h = m / 60;
                        let r = m % 60;
                        if r == 0 {
                            format!("{h}h")
                        } else {
                            format!("{h}h{r}m")
                        }
                    } else {
                        format!("{m}m")
                    }
                )
            })
            .unwrap_or_default();
        lines.push(Line::from(vec![
            key_span("Time spent"),
            Span::styled(
                time_str,
                Style::default().fg(if active { Color::Green } else { Color::Reset }),
            ),
            Span::styled(estimate_str, Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Urgency with breakdown
    {
        let breakdown_str = if let Some(ref bd) = d.urgency_breakdown {
            let mut parts = vec![];
            if bd.priority != 0.0 {
                parts.push(format!("pri {:.1}", bd.priority));
            }
            if bd.due != 0.0 {
                parts.push(format!("due {:.1}", bd.due));
            }
            if bd.blocking != 0.0 {
                parts.push(format!("blocking {:.1}", bd.blocking));
            }
            if bd.blocked != 0.0 {
                parts.push(format!("blocked {:.1}", bd.blocked));
            }
            if bd.active != 0.0 {
                parts.push(format!("active {:.1}", bd.active));
            }
            if bd.age != 0.0 {
                parts.push(format!("age {:.1}", bd.age));
            }
            if bd.tags != 0.0 {
                parts.push(format!("tags {:.1}", bd.tags));
            }
            if bd.project != 0.0 {
                parts.push(format!("proj {:.1}", bd.project));
            }
            if parts.is_empty() {
                String::new()
            } else {
                format!("  ({})", parts.join(" + "))
            }
        } else {
            String::new()
        };
        lines.push(Line::from(vec![
            key_span("Urgency"),
            Span::raw(format!("{:.1}", t.urgency)),
            Span::styled(breakdown_str, Style::default().fg(Color::DarkGray)),
        ]));
    }

    lines.push(field_line(
        "Entered",
        &t.entry
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
    ));
    lines.push(field_line(
        "Modified",
        &t.modified
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
    ));
    lines.push(field_line("UUID", &t.uuid.to_string()));

    // ── Guide: assignment / rationale / freshness banner ────────────
    if let Some(a) = &d.guide.assignment {
        lines.push(Line::from(vec![
            key_span("Assignment"),
            Span::styled(a.clone(), Style::default().fg(Color::DarkGray)),
        ]));
    }
    if let Some(r) = &d.guide.rationale {
        lines.push(Line::from(vec![
            key_span("Rationale"),
            Span::raw(r.clone()),
        ]));
    }
    if guide_is_stale(d) {
        lines.push(Line::from(vec![Span::styled(
            format!(
                "  ⚠ guide may be stale — validated @ {} but HEAD is {} (run `sara validate`)",
                d.guide.validated_commit.as_deref().unwrap_or("-"),
                d.head_commit.as_deref().unwrap_or("-"),
            ),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]));
    } else if let Some(v) = &d.guide.validated_commit {
        lines.push(Line::from(vec![
            key_span("Freshness"),
            Span::styled(
                format!("validated @ {v}"),
                Style::default().fg(Color::Green),
            ),
        ]));
    }

    // Compute selection once here so typed notes, anchors, comments and
    // checklist can all reference it below.
    let items = focusables(d);
    let sel: Option<Focusable> = if st.editing {
        None
    } else {
        items.get(st.selected).cloned()
    };
    let file_selected = |path: &str| sel == Some(Focusable::File(path.to_string()));

    // ── Typed notes (findings, constraints, …) ───────────────────────────────
    // Build a flat note list once so indices match Focusable::Note(i).
    let all_typed = typed_notes(d);
    let mut note_cursor: usize = 0; // tracks position in all_typed across kinds
    for (label, kind) in [
        ("Findings", "finding"),
        ("Constraints", "constraint"),
        ("Assumptions", "assumption"),
        ("Open questions", "open_question"),
        ("Non-goals", "non_goal"),
        ("Decisions", "decision"),
        ("Risks", "risk"),
        ("Patterns", "pattern"),
    ] {
        let notes = notes_of_kind(d, kind);
        if notes.is_empty() {
            continue;
        }
        lines.push(Line::from(""));
        lines.push(section(&format!(
            "{label}  (↑/↓ select · c comment · r reconsider)"
        )));
        for n in &notes {
            let note_idx = note_cursor;
            note_cursor += 1;
            let is_sel = sel == Some(Focusable::Note(note_idx));
            let row_bg = if is_sel { Color::Blue } else { Color::Reset };
            let row_fg = if is_sel { Color::White } else { Color::Reset };

            // Open comments targeting this note.
            let note_id_str = n.id.to_string();
            let note_fb: Vec<&crate::db::Annotation> = d
                .annotations
                .iter()
                .filter(|a| {
                    a.kind == "comment"
                        && a.status == "open"
                        && a.target_kind.as_deref() == Some("note")
                        && a.target_id.as_deref() == Some(note_id_str.as_str())
                })
                .collect();

            let prefix = if is_sel { " ▶ " } else { "   " };
            let mut spans = vec![
                Span::styled(
                    prefix.to_string(),
                    Style::default()
                        .fg(if is_sel { Color::White } else { Color::Gray })
                        .bg(row_bg),
                ),
                Span::styled(
                    "• ".to_string(),
                    Style::default()
                        .fg(if is_sel { Color::White } else { Color::Gray })
                        .bg(row_bg),
                ),
                Span::styled(
                    n.text.clone(),
                    Style::default()
                        .fg(row_fg)
                        .bg(row_bg)
                        .add_modifier(if is_sel {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
            ];
            if n.author == "ai" {
                spans.push(Span::styled(
                    " (ai)",
                    Style::default()
                        .fg(if is_sel { Color::White } else { Color::Magenta })
                        .bg(row_bg),
                ));
            }
            if !note_fb.is_empty() {
                spans.push(Span::styled(
                    format!("  💬{}", note_fb.len()),
                    Style::default().fg(Color::Cyan).bg(row_bg),
                ));
            }
            if note_fb.iter().any(|a| a.request_revision) {
                spans.push(Span::styled(
                    " ⟳",
                    Style::default().fg(Color::Yellow).bg(row_bg),
                ));
            }
            lines.push(Line::from(spans));

            // Thread: show open comments indented beneath this note.
            for a in &note_fb {
                let date = a.entry.with_timezone(&Local).format("%H:%M");
                let flag = if a.request_revision { " ⟳" } else { "" };
                lines.push(Line::from(vec![
                    Span::styled("      ╰ ".to_string(), Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{date}{flag}  "),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(a.text.clone(), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
        // note_cursor already advanced per-note above.
    }
    // Sanity: note_cursor should equal all_typed.len() — unused but kept for
    // clarity; the compiler will optimise it away.
    let _ = all_typed.len();

    if !d.blocked_by.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Blocked by"));
        for b in &d.blocked_by {
            lines.push(Line::from(format!("  {b}")));
        }
    }
    if !d.blocking.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Blocking"));
        for b in &d.blocking {
            lines.push(Line::from(format!("  {b}")));
        }
    }
    // (sel / items / file_selected already computed above — before typed notes)

    if !d.links.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Links  (Enter to open)"));
        for (i, link) in d.links.iter().enumerate() {
            let selected = sel == Some(Focusable::Link(i));
            let (bg, fg) = if selected {
                (Color::Blue, Color::White)
            } else {
                (Color::Reset, Color::Cyan)
            };
            let prefix = if selected { " ▶ " } else { "   " };
            let style = Style::default()
                .fg(fg)
                .bg(bg)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            let meta_style = Style::default()
                .fg(if selected { Color::White } else { Color::Gray })
                .bg(bg);
            let mut spans = vec![
                Span::styled(prefix.to_string(), meta_style),
                Span::styled(format!("[{}] ", link.id), meta_style),
                Span::styled(link.display(), style),
            ];
            if link.display() != link.url {
                spans.push(Span::styled(
                    format!("  {}", link.url),
                    Style::default().fg(Color::DarkGray).bg(bg),
                ));
            }
            lines.push(Line::from(spans));
        }
    }
    if !d.manual_files.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Relevant files"));
        for file in &d.manual_files {
            lines.push(nav_line(file, Color::Cyan, false, file_selected(file)));
        }
    }
    // ── Code anchors: each is focusable, shows 💬/⟳ markers + threaded comments ──
    if !d.anchors.is_empty() {
        lines.push(Line::from(""));
        lines.push(section(
            "Possible relevant files  · ↑/↓ select · c comment · r reconsider",
        ));
        for (ai, anchor) in d.anchors.iter().enumerate() {
            let is_sel = sel == Some(Focusable::Anchor(ai));
            let file_text = format!("{}{}", anchor.path, anchor.location());
            let badge = if anchor.source == db::SOURCE_SUGGESTED {
                " (ai)"
            } else {
                ""
            };

            // Threaded comments anchored to this file.
            let anchor_fb: Vec<&crate::db::Annotation> = d
                .annotations
                .iter()
                .filter(|a| {
                    a.kind == "comment"
                        && a.target_kind.as_deref() == Some("anchor")
                        && a.target_id.as_deref() == Some(anchor.path.as_str())
                })
                .collect();
            let open_fb = anchor_fb.iter().filter(|a| a.status == "open").count();
            let needs_reconsider = anchor_fb
                .iter()
                .any(|a| a.request_revision && a.status == "open");

            let row_bg = if is_sel { Color::Blue } else { Color::Reset };
            let row_fg = if is_sel { Color::White } else { Color::Cyan };
            let meta_fg = if is_sel { Color::White } else { Color::Gray };

            let mut spans = vec![
                Span::styled(
                    if is_sel { " ▶ " } else { "   " }.to_string(),
                    Style::default().fg(meta_fg).bg(row_bg),
                ),
                Span::styled(
                    file_text,
                    Style::default()
                        .fg(row_fg)
                        .bg(row_bg)
                        .add_modifier(if is_sel {
                            Modifier::BOLD
                        } else {
                            Modifier::empty()
                        }),
                ),
                Span::styled(
                    badge.to_string(),
                    Style::default()
                        .fg(if is_sel { Color::White } else { Color::Magenta })
                        .bg(row_bg),
                ),
            ];
            if let Some(r) = &anchor.reason {
                spans.push(Span::styled(
                    format!("  — {r}"),
                    Style::default()
                        .fg(if is_sel {
                            Color::White
                        } else {
                            Color::DarkGray
                        })
                        .bg(row_bg),
                ));
            }
            if open_fb > 0 {
                spans.push(Span::styled(
                    format!("  💬{open_fb}"),
                    Style::default().fg(Color::Cyan).bg(row_bg),
                ));
            }
            if needs_reconsider {
                spans.push(Span::styled(
                    " ⟳",
                    Style::default().fg(Color::Yellow).bg(row_bg),
                ));
            }
            lines.push(Line::from(spans));

            // Thread: show comments anchored to this file, indented beneath it.
            for a in &anchor_fb {
                let date = a.entry.with_timezone(&Local).format("%H:%M");
                let resolved = a.status == "resolved";
                let text_style = if resolved {
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let flag = if a.request_revision && !resolved {
                    " ⟳"
                } else {
                    ""
                };
                lines.push(Line::from(vec![
                    Span::styled("      ╰ ".to_string(), Style::default().fg(Color::DarkGray)),
                    Span::styled(
                        format!("{date}{flag}  "),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(a.text.clone(), text_style),
                ]));
            }
        }
    }

    // ── Checklist (steps + acceptance criteria with intent + provenance)
    if !d.checklist.is_empty() {
        // At-a-glance progress: steps done / total, acceptance done / total.
        let (mut steps_done, mut steps_total, mut acc_done, mut acc_total) = (0, 0, 0, 0);
        for it in &d.checklist {
            if it.kind == db::STEP_KIND_ACCEPTANCE {
                acc_total += 1;
                acc_done += it.done as i32;
            } else {
                steps_total += 1;
                steps_done += it.done as i32;
            }
        }
        let mut progress = String::new();
        if steps_total > 0 {
            progress.push_str(&format!("{steps_done}/{steps_total} steps"));
        }
        if acc_total > 0 {
            if !progress.is_empty() {
                progress.push_str(" · ");
            }
            progress.push_str(&format!("{acc_done}/{acc_total} acceptance"));
        }
        lines.push(Line::from(""));
        lines.push(section(&format!(
            "Checklist  {progress}  (Space toggle · c comment · r reconsider · x resolve)"
        )));
        for (i, item) in d.checklist.iter().enumerate() {
            let is_sel = sel == Some(Focusable::Checklist(i));
            let row_bg = if is_sel { Color::Blue } else { Color::Reset };

            let (box_str, text_style) = if item.done {
                (
                    "[x]",
                    Style::default()
                        .fg(Color::DarkGray)
                        .bg(row_bg)
                        .add_modifier(Modifier::CROSSED_OUT),
                )
            } else if is_sel {
                (
                    "[ ]",
                    Style::default()
                        .fg(Color::White)
                        .bg(Color::Blue)
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("[ ]", Style::default())
            };
            // Feedback markers for this step: comment count + reconsider flag.
            let target_k = if item.kind == db::STEP_KIND_ACCEPTANCE {
                "acceptance"
            } else {
                "step"
            };
            let item_id_str = item.id.to_string();
            let fb: Vec<&crate::db::Annotation> = d
                .annotations
                .iter()
                .filter(|a| {
                    a.kind == "comment"
                        && a.status == "open"
                        && a.target_kind.as_deref() == Some(target_k)
                        && a.target_id.as_deref() == Some(item_id_str.as_str())
                })
                .collect();
            let prefix = if is_sel { " ▶ " } else { "   " };
            let box_style = Style::default()
                .fg(if is_sel { Color::White } else { Color::Gray })
                .bg(row_bg);
            let mut spans = vec![
                Span::styled(prefix.to_string(), box_style),
                Span::styled(format!("{box_str} "), box_style),
                Span::styled(item.text.clone(), text_style),
            ];
            if item.kind == db::STEP_KIND_ACCEPTANCE {
                spans.push(Span::styled(" [accept]", Style::default().fg(Color::Blue)));
            }
            if item.source == "ai" {
                spans.push(Span::styled(" (ai)", Style::default().fg(Color::Magenta)));
            }
            if !fb.is_empty() {
                spans.push(Span::styled(
                    format!("  💬{}", fb.len()),
                    Style::default().fg(Color::Cyan),
                ));
            }
            if fb.iter().any(|a| a.request_revision) {
                spans.push(Span::styled(" ⟳", Style::default().fg(Color::Yellow)));
            }
            lines.push(Line::from(spans));
            if let Some(intent) = &item.intent {
                lines.push(Line::from(Span::styled(
                    format!("         {intent}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            // Verify command — how this step/criterion is checked.
            if let Some(v) = &item.verify_cmd {
                lines.push(Line::from(vec![
                    Span::styled(
                        "         verify ".to_string(),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(v.clone(), Style::default().fg(Color::Blue)),
                ]));
            }
            // Execution outcome recorded when the step was marked done.
            if let Some(r) = &item.result {
                lines.push(Line::from(vec![
                    Span::styled("         → ".to_string(), Style::default().fg(Color::Green)),
                    Span::styled(r.clone(), Style::default().fg(Color::Green)),
                ]));
            }
            // Completion provenance: which commit / when the step was finished.
            if item.done && (item.done_commit.is_some() || item.done_at.is_some()) {
                let commit = item
                    .done_commit
                    .as_deref()
                    .map(|c| {
                        let short: String = c.chars().take(8).collect();
                        format!("@ {short}")
                    })
                    .unwrap_or_default();
                let when = item
                    .done_at
                    .as_deref()
                    .map(|w| format!("  {w}"))
                    .unwrap_or_default();
                lines.push(Line::from(Span::styled(
                    format!("         done {commit}{when}"),
                    Style::default().fg(Color::DarkGray),
                )));
            }
            // Thread: show comments anchored to this step/acceptance, indented.
            for a in &fb {
                let date = a.entry.with_timezone(&Local).format("%H:%M");
                let flag = if a.request_revision { " ⟳" } else { "" };
                lines.push(Line::from(vec![
                    Span::styled(
                        "         ╰ ".to_string(),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{date}{flag}  "),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(a.text.clone(), Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
    }

    // ── Verification: how to test/lint/run this task (project + task commands)
    let verif = verification_rows(d);
    if !verif.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Verification  (run: sara guide <id> --run)"));
        for (scope, label, cmd) in &verif {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {label:<7}"),
                    Style::default()
                        .fg(Color::Gray)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(cmd.clone(), Style::default().fg(Color::Blue)),
                Span::styled(format!("  ({scope})"), Style::default().fg(Color::DarkGray)),
            ]));
        }
    }

    // ── AI activity (provenance footer)
    if !d.ai_runs.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("AI activity"));
        for r in &d.ai_runs {
            let date = r.created_at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
            lines.push(Line::from(Span::styled(
                format!(
                    "  {} via {} [{}] @ {date}",
                    r.kind,
                    r.model.as_deref().unwrap_or("?"),
                    r.provider.as_deref().unwrap_or("?"),
                ),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    // ── Similar tasks (shared tags, same project)
    if !d.similar.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Related tasks (shared tags)"));
        for (id, desc, urg) in &d.similar {
            lines.push(Line::from(vec![
                Span::styled(format!("  #{id:<3} "), Style::default().fg(Color::Gray)),
                Span::raw(desc.clone()),
                Span::styled(
                    format!("  urg {urg:.1}"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
    }
    // ── Comments section: task-level + replies only (anchored ones shown inline above) ─
    let all_comments: Vec<&crate::db::Annotation> = d
        .annotations
        .iter()
        .filter(|a| a.kind == "comment")
        .collect();
    let unthreaded: Vec<&crate::db::Annotation> = all_comments
        .iter()
        .copied()
        .filter(|a| a.target_kind.as_deref() != Some("anchor"))
        .collect();
    if !unthreaded.is_empty() {
        lines.push(Line::from(""));
        lines.push(section(
            "Comments  (↑/↓ select · c add · r reconsider · x resolve)",
        ));
        // Build an index: comment-id -> annotation, for resolving note: replies.
        let id_map: std::collections::HashMap<i64, &crate::db::Annotation> =
            all_comments.iter().map(|a| (a.id, *a)).collect();
        // Build an index: checklist-item-id -> text, for resolving step/acceptance replies.
        let checklist_map: std::collections::HashMap<i64, &str> = d
            .checklist
            .iter()
            .map(|it| (it.id, it.text.as_str()))
            .collect();

        for (ci, a) in all_comments.iter().enumerate() {
            if a.target_kind.as_deref() == Some("anchor") {
                continue;
            }
            let is_sel = sel == Some(Focusable::Comment(ci));
            let date = a.entry.with_timezone(&Local).format("%Y-%m-%d %H:%M");

            let target_label = match (a.target_kind.as_deref(), a.target_id.as_deref()) {
                (Some("note"), Some(idv)) => {
                    if let Ok(parent_id) = idv.parse::<i64>()
                        && let Some(parent) = id_map.get(&parent_id)
                    {
                        let snippet: String = parent.text.chars().take(40).collect();
                        format!("↩ \"{snippet}\"  ")
                    } else {
                        String::new()
                    }
                }
                (Some("step"), Some(idv)) => {
                    if let Ok(item_id) = idv.parse::<i64>()
                        && let Some(text) = checklist_map.get(&item_id)
                    {
                        let snippet: String = text.chars().take(40).collect();
                        format!("step: \"{snippet}\"  ")
                    } else {
                        String::new()
                    }
                }
                (Some("acceptance"), Some(idv)) => {
                    if let Ok(item_id) = idv.parse::<i64>()
                        && let Some(text) = checklist_map.get(&item_id)
                    {
                        let snippet: String = text.chars().take(40).collect();
                        format!("accept: \"{snippet}\"  ")
                    } else {
                        String::new()
                    }
                }
                _ => String::new(),
            };

            let resolved = a.status == "resolved";
            let text_style = if resolved {
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::CROSSED_OUT)
            } else if is_sel {
                Style::default().fg(Color::White).bg(Color::Blue)
            } else {
                Style::default()
            };
            let meta_style = if is_sel {
                Style::default().fg(Color::White).bg(Color::Blue)
            } else {
                Style::default().fg(Color::Gray)
            };
            let mut spans = vec![
                Span::styled(if is_sel { " ▶ " } else { "   " }.to_string(), meta_style),
                Span::styled(format!("{date}  "), meta_style),
            ];
            if !target_label.is_empty() {
                spans.push(Span::styled(
                    target_label,
                    if is_sel {
                        Style::default().fg(Color::White).bg(Color::Blue)
                    } else {
                        Style::default().fg(Color::Cyan)
                    },
                ));
            }
            if a.request_revision && !resolved {
                spans.push(Span::styled("⟳ ", Style::default().fg(Color::Yellow)));
            }
            spans.push(Span::styled(a.text.clone(), text_style));
            lines.push(Line::from(spans));
        }
    }

    // History is rendered in its own box at the bottom — not in the main lines.

    // Split the main content area horizontally when wide enough for the panel.
    let show_panel = chunks[0].width >= 96;
    let (left_area, panel_area) = if show_panel {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Min(50), Constraint::Length(42)])
            .split(chunks[0]);
        (cols[0], Some(cols[1]))
    } else {
        (chunks[0], None)
    };

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false })
        .scroll((st.scroll, 0));
    f.render_widget(para, left_area);

    // ── Feature chain (top) + Git + stats + mini heatmap
    if let Some(panel) = panel_area {
        // The chain panel only appears when the task is linked to others. Its
        // height flexes with the chain length (capped) so short chains stay compact.
        let has_chain = d.chain.len() > 1;
        let chain_h: u16 = if has_chain {
            // +3 for border (2) and the progress bar row (1).
            ((d.chain.len() as u16) + 3).clamp(5, 14)
        } else {
            0
        };

        let constraints: Vec<Constraint> = if has_chain {
            vec![
                Constraint::Length(chain_h),
                Constraint::Min(4),
                Constraint::Length(14),
                Constraint::Length(11),
            ]
        } else {
            vec![
                Constraint::Min(4),
                Constraint::Length(14),
                Constraint::Length(11),
            ]
        };
        let panel_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(panel);

        let base = if has_chain {
            render_chain_panel(f, panel_chunks[0], d);
            1
        } else {
            0
        };

        let git_lines = git_panel_lines(d);
        let git_para = Paragraph::new(git_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Git ")
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(git_para, panel_chunks[base]);

        render_project_stats(f, panel_chunks[base + 1], d);
        render_mini_heatmap(f, panel_chunks[base + 2], &d.activity, &d.task.project);
    }

    // ── History box (pinned to bottom, above edit bar and footer)
    if history_height > 0 {
        let hist_chunk = chunks[1]; // always chunk[1] when history is shown
        let hist_lines = history_lines(&d.history);
        let hist_para = Paragraph::new(hist_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" History ")
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(hist_para, hist_chunk);
    }

    // ── Comment bar (anchored to the focused element)
    if st.commenting {
        let edit_chunk_idx = if history_height > 0 { 2 } else { 1 };
        let items = focusables(d);
        let focus = items.get(st.selected).cloned();
        let (tk, tid) = comment_target(d, &focus);
        let target = match (tk, tid) {
            (Some(k), Some(i)) => format!("{k}:{i}"),
            _ => "task".to_string(),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(format!(" Comment on {target}  (Enter save · Esc cancel) "))
            .border_style(Style::default().fg(Color::Yellow));
        let inner = block.inner(chunks[edit_chunk_idx]);
        f.render_widget(block, chunks[edit_chunk_idx]);
        f.render_widget(&st.editor, inner);
    }

    // ── Edit bar (chunk index depends on whether history box is present)
    if st.editing {
        let edit_chunk_idx = if history_height > 0 { 2 } else { 1 };
        let field = EDIT_FIELDS
            .get(st.selected)
            .copied()
            .unwrap_or(EditField::Description);
        let (title, border) = if st.due_error {
            (
                format!(" Editing {} — invalid date ", field.label()),
                Color::Red,
            )
        } else if let Some(ref err) = st.dep_error {
            (format!(" Editing {} — {} ", field.label(), err), Color::Red)
        } else if field == EditField::DependsOn {
            (
                format!(
                    " Editing {}  (task IDs, space/comma separated · Enter confirm · Esc cancel) ",
                    field.label()
                ),
                Color::Yellow,
            )
        } else {
            (
                format!(" Editing {}  (Enter confirm · Esc cancel) ", field.label()),
                Color::Yellow,
            )
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(Style::default().fg(border));
        let inner = block.inner(chunks[edit_chunk_idx]);
        f.render_widget(block, chunks[edit_chunk_idx]);
        f.render_widget(&st.editor, inner);
    }

    let footer = if st.commenting {
        " type a comment  •  Enter save  •  Esc cancel ".to_string()
    } else if st.editing {
        " type to edit  •  Enter confirm  •  Esc cancel ".to_string()
    } else {
        " ↑/↓ move • Enter edit/open • c comment • r reconsider • x resolve • q close ".to_string()
    };
    let footer_idx = chunks.len() - 1;
    f.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::Gray)),
        chunks[footer_idx],
    );
}

/// Right-hand panel showing the dependency chain (feature) the task belongs to,
/// in blockers-first order: a progress bar plus one row per linked task. Completed
/// tasks are struck through; the task currently being viewed is highlighted.
fn render_chain_panel(f: &mut Frame, area: ratatui::layout::Rect, d: &Detail) {
    let total = d.chain.len();
    let done = d
        .chain
        .iter()
        .filter(|t| t.status == crate::model::Status::Completed)
        .count();
    let all_done = total > 0 && done == total;

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Feature chain  {done}/{total} "))
        .border_style(Style::default().fg(if all_done { Color::Green } else { Color::Cyan }));
    let inner = block.inner(area);
    f.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    // Progress bar across the panel width.
    let bar_w = inner.width.saturating_sub(2) as usize;
    if bar_w > 0 {
        let filled = (done * bar_w + total / 2).checked_div(total).unwrap_or(0);
        let mut spans = vec![Span::raw(" ")];
        spans.push(Span::styled(
            "█".repeat(filled),
            Style::default().fg(if all_done { Color::Green } else { Color::Cyan }),
        ));
        spans.push(Span::styled(
            "░".repeat(bar_w - filled),
            Style::default().fg(Color::DarkGray),
        ));
        lines.push(Line::from(spans));
    }

    let current_idx = d.chain.iter().position(|t| t.uuid == d.task.uuid);
    let desc_w = inner.width.saturating_sub(8) as usize;
    for (i, t) in d.chain.iter().enumerate() {
        let completed = t.status == crate::model::Status::Completed;
        let is_current = Some(i) == current_idx;
        let id_str =
            t.id.map(|n| format!("{n:>3}"))
                .unwrap_or_else(|| "  -".to_string());
        let marker = if is_current { "▶ " } else { "  " };
        let glyph = if completed {
            "✓"
        } else if is_current {
            "◉"
        } else {
            "○"
        };
        let desc = truncate_str(&t.description, desc_w.max(8));

        let (glyph_style, text_style) = if is_current {
            (
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
                Style::default()
                    .fg(Color::White)
                    .bg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            )
        } else if completed {
            (
                Style::default().fg(Color::Green),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::CROSSED_OUT),
            )
        } else {
            (
                Style::default().fg(Color::Cyan),
                Style::default().fg(Color::Gray),
            )
        };

        lines.push(Line::from(vec![
            Span::styled(marker.to_string(), glyph_style),
            Span::styled(format!("{glyph} "), glyph_style),
            Span::styled(format!("{id_str} "), text_style),
            Span::styled(desc, text_style),
        ]));
    }

    // Scroll so the current task stays visible in long chains (1 = progress bar row).
    let visible = inner.height as usize;
    let cur_line = current_idx.map(|i| i + 1).unwrap_or(0);
    let scroll = if cur_line >= visible {
        (cur_line + 1 - visible) as u16
    } else {
        0
    };

    f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), inner);
}

/// Build lines for the History box at the bottom of the detail view.
fn render_project_stats(f: &mut Frame, area: ratatui::layout::Rect, d: &Detail) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Project ")
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(ref s) = d.stats else {
        return;
    };

    // Mini bar: fill `width` chars proportionally
    let bar = |count: u32, total: u32, width: usize| -> String {
        if total == 0 {
            return " ".repeat(width);
        }
        let filled = ((count as f64 / total as f64) * width as f64).round() as usize;
        "█".repeat(filled.min(width))
    };

    let total_ever = s.pending + s.completed_total;
    let completion_rate = if total_ever > 0 {
        format!(
            "{:.0}%",
            s.completed_total as f64 / total_ever as f64 * 100.0
        )
    } else {
        "—".to_string()
    };

    let w = inner.width.saturating_sub(2) as usize;
    let bar_w = w.saturating_sub(16).clamp(3, 10);

    let mut lines: Vec<Line> = vec![];

    // Status counts
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<10}", "Pending"),
            Style::default().fg(Color::Gray),
        ),
        Span::raw(format!("{:>3}", s.pending)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<10}", "Active"),
            Style::default().fg(Color::Gray),
        ),
        Span::styled(
            format!("{:>3}", s.active),
            Style::default().fg(if s.active > 0 {
                Color::Green
            } else {
                Color::Reset
            }),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<10}", "Done"),
            Style::default().fg(Color::Gray),
        ),
        Span::raw(format!("{:>3}", s.completed_total)),
        Span::styled(
            format!("  {}", completion_rate),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    lines.push(Line::from(Span::styled(
        "  ─────────────",
        Style::default().fg(Color::DarkGray),
    )));

    // Priority mini bars
    let pri_total = s.pending.max(1);
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<5}", "H"),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<bar_w$}", bar(s.high, pri_total, bar_w)),
            Style::default().fg(Color::Red),
        ),
        Span::styled(format!(" {}", s.high), Style::default().fg(Color::DarkGray)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<5}", "M"), Style::default().fg(Color::Yellow)),
        Span::styled(
            format!("{:<bar_w$}", bar(s.medium, pri_total, bar_w)),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(
            format!(" {}", s.medium),
            Style::default().fg(Color::DarkGray),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<5}", "L"), Style::default().fg(Color::Green)),
        Span::styled(
            format!("{:<bar_w$}", bar(s.low, pri_total, bar_w)),
            Style::default().fg(Color::Green),
        ),
        Span::styled(format!(" {}", s.low), Style::default().fg(Color::DarkGray)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<5}", "—"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!("{:<bar_w$}", bar(s.no_pri, pri_total, bar_w)),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            format!(" {}", s.no_pri),
            Style::default().fg(Color::DarkGray),
        ),
    ]));

    lines.push(Line::from(Span::styled(
        "  ─────────────",
        Style::default().fg(Color::DarkGray),
    )));

    // Due status
    if s.overdue > 0 {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<10}", "Overdue"),
                Style::default().fg(Color::Red),
            ),
            Span::styled(
                format!("{:>3}", s.overdue),
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if s.due_today > 0 {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<10}", "Today"),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(
                format!("{:>3}", s.due_today),
                Style::default().fg(Color::Yellow),
            ),
        ]));
    }
    let due_later = s.due_week.saturating_sub(s.due_today);
    if due_later > 0 {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<10}", "This week"),
                Style::default().fg(Color::Gray),
            ),
            Span::raw(format!("{:>3}", due_later)),
        ]));
    }

    f.render_widget(Paragraph::new(lines), inner);
}

fn render_mini_heatmap(
    f: &mut Frame,
    area: ratatui::layout::Rect,
    counts: &std::collections::HashMap<chrono::NaiveDate, u32>,
    project: &str,
) {
    use chrono::{Datelike, Duration, Local};

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" {} ", project))
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let max = counts.values().copied().max().unwrap_or(1).max(1);
    let today = Local::now().date_naive();

    // Align to most recent Sunday
    let days_since_sunday = today.weekday().num_days_from_sunday();
    let grid_end = today - Duration::days(days_since_sunday as i64);

    // Fit weeks into available inner width: label(4) + weeks * 3
    let cell_w: u16 = 3; // "██ "
    let label_w: u16 = 4;
    let num_weeks = ((inner.width.saturating_sub(label_w)) / cell_w).clamp(4, 16) as i64;
    let grid_start = grid_end - Duration::weeks(num_weeks) + Duration::days(1);

    // Month label row (row 0 of inner)
    {
        let mut spans: Vec<Span> = vec![Span::raw(format!(
            "{:<width$}",
            "",
            width = label_w as usize
        ))];
        let mut last_month = 0u32;
        let mut ws = grid_start;
        for _ in 0..num_weeks {
            let m = ws.month();
            if m != last_month {
                let name = &crate::commands::activity::month_abbr(m)[..3];
                spans.push(Span::styled(
                    format!("{:<width$}", name, width = cell_w as usize),
                    Style::default().fg(Color::DarkGray),
                ));
                last_month = m;
            } else {
                spans.push(Span::raw(format!(
                    "{:<width$}",
                    "",
                    width = cell_w as usize
                )));
            }
            ws += Duration::weeks(1);
        }
        let month_area = ratatui::layout::Rect {
            x: inner.x,
            y: inner.y,
            width: inner.width,
            height: 1,
        };
        f.render_widget(Paragraph::new(Line::from(spans)), month_area);
    }

    // 7 day rows (1..=7 of inner)
    const DAY_LABELS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    const SHOW_LABEL: [bool; 7] = [false, true, false, true, false, true, false];

    for row in 0..7u32 {
        if inner.y + 1 + row as u16 >= inner.y + inner.height {
            break;
        }
        let mut spans: Vec<Span> = vec![];
        let label = if SHOW_LABEL[row as usize] {
            DAY_LABELS[row as usize]
        } else {
            "   "
        };
        spans.push(Span::styled(
            format!("{label} "),
            Style::default().fg(Color::DarkGray),
        ));

        let mut ws = grid_start;
        for _ in 0..num_weeks {
            let day = ws + Duration::days(row as i64);
            let in_future = day > today;
            let count = if in_future {
                0
            } else {
                counts.get(&day).copied().unwrap_or(0)
            };
            let color = if in_future {
                Color::Rgb(12, 14, 18)
            } else {
                heat_color_mini(count, max)
            };
            spans.push(Span::styled("██ ", Style::default().bg(color).fg(color)));
            ws += Duration::weeks(1);
        }

        let row_area = ratatui::layout::Rect {
            x: inner.x,
            y: inner.y + 1 + row as u16,
            width: inner.width,
            height: 1,
        };
        f.render_widget(Paragraph::new(Line::from(spans)), row_area);
    }

    // Stats line at the bottom
    let total: u32 = counts.values().sum();
    let stats_area = ratatui::layout::Rect {
        x: inner.x,
        y: inner.y + 8,
        width: inner.width,
        height: 1,
    };
    if stats_area.y < inner.y + inner.height {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("  {total} events (16w)"),
                Style::default().fg(Color::DarkGray),
            ))),
            stats_area,
        );
    }
}

fn heat_color_mini(count: u32, max: u32) -> Color {
    if count == 0 {
        return Color::Rgb(22, 27, 34);
    }
    let ratio = count as f64 / max.max(1) as f64;
    if ratio < 0.25 {
        Color::Rgb(14, 68, 41)
    } else if ratio < 0.5 {
        Color::Rgb(0, 109, 50)
    } else if ratio < 0.75 {
        Color::Rgb(38, 166, 65)
    } else {
        Color::Rgb(57, 211, 83)
    }
}

fn history_lines(history: &[crate::db::HistoryEntry]) -> Vec<Line<'static>> {
    let mut lines = vec![];
    for h in history.iter().rev() {
        let date = h
            .changed_at
            .with_timezone(&Local)
            .format("%m-%d %H:%M")
            .to_string();
        let label = if h.field == "annotation" {
            "comment"
        } else {
            &h.field
        };
        let mut spans = vec![
            Span::styled(format!("  {date}  "), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{:<11} ", label), Style::default().fg(Color::Cyan)),
        ];
        // Additive fields render as +/− when exactly one side is set; a
        // checklist toggle (both sides set) falls through to the arrow form.
        let additive = matches!(
            h.field.as_str(),
            "annotation" | "link" | "dependency" | "checklist" | "file"
        ) && h.old_value.is_none() != h.new_value.is_none();
        if h.field == "created" {
            spans.push(Span::raw(h.new_value.clone().unwrap_or_default()));
        } else if additive {
            if let Some(text) = &h.new_value {
                spans.push(Span::styled("+ ", Style::default().fg(Color::Green)));
                spans.push(Span::raw(text.clone()));
            } else if let Some(text) = &h.old_value {
                spans.push(Span::styled("− ", Style::default().fg(Color::Red)));
                spans.push(Span::raw(text.clone()));
            }
        } else {
            spans.push(Span::styled(
                h.old_value.clone().unwrap_or_else(|| "—".into()),
                Style::default().fg(Color::Gray),
            ));
            spans.push(Span::styled(" → ", Style::default().fg(Color::DarkGray)));
            spans.push(Span::raw(h.new_value.clone().unwrap_or_else(|| "—".into())));
        }
        lines.push(Line::from(spans));
    }
    lines
}

/// Build the content lines for the Git branch panel.
fn git_panel_lines(d: &Detail) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = vec![];

    let Some(rec) = &d.branch else {
        lines.push(Line::from(Span::styled(
            "  No branch tied.",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Run: sara <id> addbranch",
            Style::default().fg(Color::Gray),
        )));
        lines.push(Line::from(Span::styled(
            "  Then: sara stop <id> to snapshot.",
            Style::default().fg(Color::Gray),
        )));
        return lines;
    };

    // Branch name line
    lines.push(Line::from(vec![
        Span::styled("  Branch  ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            rec.branch.clone(),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
    ]));
    if let Some(base) = &rec.base {
        lines.push(Line::from(vec![
            Span::styled("  Base    ", Style::default().fg(Color::DarkGray)),
            Span::styled(base.clone(), Style::default().fg(Color::Gray)),
        ]));
    }
    if let Some(logged_at) = rec.logged_at {
        let ts = logged_at
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
        lines.push(Line::from(vec![
            Span::styled("  Logged  ", Style::default().fg(Color::DarkGray)),
            Span::styled(ts, Style::default().fg(Color::Gray)),
        ]));
    }
    lines.push(Line::from(""));

    match &rec.files {
        None => {
            lines.push(Line::from(Span::styled(
                "  No snapshot yet.",
                Style::default().fg(Color::DarkGray),
            )));
            lines.push(Line::from(Span::styled(
                "  Run: sara stop <id>",
                Style::default().fg(Color::Gray),
            )));
        }
        Some(files) if files.is_empty() => {
            lines.push(Line::from(Span::styled(
                "  No changes vs base.",
                Style::default().fg(Color::Green),
            )));
        }
        Some(files) => {
            const MAX_FILES: usize = 20;
            lines.push(Line::from(Span::styled(
                format!(
                    "  {} file{} changed",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" }
                ),
                Style::default().fg(Color::Yellow),
            )));
            for f in files.iter().take(MAX_FILES) {
                // Show only filename for brevity; full path on hover isn't feasible in TUI
                let name = std::path::Path::new(f)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(f.as_str());
                lines.push(Line::from(vec![
                    Span::styled("    ", Style::default()),
                    Span::styled(name.to_string(), Style::default().fg(Color::Cyan)),
                    if name != f.as_str() {
                        Span::styled(format!("  {}", f), Style::default().fg(Color::DarkGray))
                    } else {
                        Span::raw("")
                    },
                ]));
            }
            if files.len() > MAX_FILES {
                lines.push(Line::from(Span::styled(
                    format!("    +{} more", files.len() - MAX_FILES),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
    }

    // Overlap section
    if !d.overlaps.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  ⚠  Potential overlaps",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
        for ov in &d.overlaps {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  [{:>2}] ", ov.id),
                    Style::default().fg(Color::Gray),
                ),
                Span::styled(
                    truncate_str(&ov.description, 20),
                    Style::default().fg(Color::Yellow),
                ),
                Span::styled(
                    format!(" ({})", ov.branch),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
            for sf in &ov.shared_files {
                let name = std::path::Path::new(sf)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(sf.as_str());
                lines.push(Line::from(Span::styled(
                    format!("    ↳ {name}"),
                    Style::default().fg(Color::Red),
                )));
            }
        }
    }

    lines
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max - 1).collect();
        format!("{t}…")
    }
}

/// A selectable file/link row with a `›` marker when focused.
fn nav_line<'a>(text: &str, color: Color, italic: bool, selected: bool) -> Line<'a> {
    let mut style = Style::default().fg(color);
    if italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if selected {
        style = style
            .bg(Color::Blue)
            .fg(Color::White)
            .add_modifier(Modifier::BOLD);
    }
    let prefix = if selected { " ▶ " } else { "   " };
    Line::from(vec![
        Span::styled(prefix.to_string(), style),
        Span::styled(text.to_string(), style),
    ])
}

/// A single rendered row with optional selection highlight (blue bg).
fn sel_line<'a>(spans: Vec<Span<'a>>, selected: bool) -> Line<'a> {
    if !selected {
        return Line::from(spans);
    }
    // Paint the entire row blue so it's unmissable.
    let highlighted: Vec<Span> = spans
        .into_iter()
        .map(|s| Span::styled(s.content, s.style.bg(Color::Blue).fg(Color::White)))
        .collect();
    Line::from(highlighted)
}

fn editable_line<'a>(k: &str, v: &str, selected: bool, field: EditField, task: &Task) -> Line<'a> {
    let (bg, fg) = if selected {
        (Color::Blue, Color::White)
    } else {
        (Color::Reset, Color::Gray)
    };
    let key_style = if selected {
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(fg)
    };

    // Priority gets a colored value.
    let value_span = if field == EditField::Priority {
        match &task.priority {
            Some(Priority::H) => Span::styled("High", Style::default().fg(Color::Red)),
            Some(Priority::M) => Span::styled("Medium", Style::default().fg(Color::Yellow)),
            Some(Priority::L) => Span::styled("Low", Style::default().fg(Color::Green)),
            None => Span::styled("-", Style::default().fg(Color::Gray)),
        }
    } else if field == EditField::Due {
        due_value_span(task, v)
    } else {
        Span::raw(v.to_string())
    };

    let prefix = if selected { " ▶ " } else { "   " };
    let value_style = if selected {
        Style::default().fg(Color::White).bg(Color::Blue)
    } else {
        Style::default()
    };
    let value_span = Span::styled(value_span.content, value_span.style.patch(value_style));
    Line::from(vec![
        Span::styled(prefix.to_string(), key_style),
        Span::styled(format!("{:<12}", k), key_style),
        value_span,
    ])
}

fn due_value_span<'a>(task: &Task, fallback: &str) -> Span<'a> {
    if let Some(dd) = task.due {
        let days = (dd - Utc::now()).num_days();
        let color = if days < 0 {
            Color::Red
        } else if days <= 1 {
            Color::Yellow
        } else {
            Color::Reset
        };
        Span::styled(
            dd.with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
            Style::default().fg(color),
        )
    } else {
        Span::styled(fallback.to_string(), Style::default().fg(Color::Gray))
    }
}

fn key_span(k: &str) -> Span<'static> {
    Span::styled(format!("  {:<12}", k), Style::default().fg(Color::Gray))
}

fn field_line<'a>(k: &str, v: &str) -> Line<'a> {
    Line::from(vec![key_span(k), Span::raw(v.to_string())])
}

fn section(k: &str) -> Line<'static> {
    Line::from(Span::styled(
        k.to_string(),
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(Color::Cyan),
    ))
}

/// Parse a human duration string like "2h30m", "90m", "1h", "45" (minutes) into minutes.
fn parse_duration_to_mins(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let s_lower = s.to_lowercase();
    let rest = s_lower.as_str();
    // Parse hours (handles "2h", "2h30m", "2h 30m")
    if let Some(h_pos) = rest.find('h')
        && let Ok(h) = rest[..h_pos].trim().parse::<i64>()
    {
        let mut total = h * 60;
        let after_h = rest[h_pos + 1..].trim().trim_end_matches('m').trim();
        if !after_h.is_empty()
            && let Ok(m) = after_h.parse::<i64>()
        {
            total += m;
        }
        return Some(total);
    }
    // "Ym" or bare number (minutes)
    let m_part = rest.trim_end_matches('m').trim();
    m_part.parse::<i64>().ok()
}

fn print_plain(d: &Detail) {
    let t = &d.task;
    println!("Task {}", t.id.unwrap_or(0));
    println!();
    println!("{:<14}{}", "Description", t.description);
    println!("{:<14}{}", "Project", t.project);
    println!("{:<14}{}", "Status", t.status);
    println!(
        "{:<14}{}",
        "Priority",
        t.priority.as_ref().map(|p| p.label()).unwrap_or("-")
    );
    println!(
        "{:<14}{}",
        "Due",
        t.due
            .map(|dd| dd
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    println!(
        "{:<14}{}",
        "Tags",
        if t.tags.is_empty() {
            "-".to_string()
        } else {
            t.tags.join(", ")
        }
    );
    println!(
        "{:<14}{}",
        "Time spent",
        format_duration(t.total_time_spent())
    );
    println!("{:<14}{:.1}", "Urgency", t.urgency);
    println!("{:<14}{}", "UUID", t.uuid);

    // ── Guide ───────────────────────────────────────────────────────
    if let Some(a) = &d.guide.assignment {
        println!("{:<14}{}", "Assignment", a);
    }
    if let Some(r) = &d.guide.rationale {
        println!("{:<14}{}", "Rationale", r);
    }
    if guide_is_stale(d) {
        println!(
            "{:<14}guide validated @ {} but HEAD is {} — may be stale (run `sara validate`)",
            "Freshness",
            d.guide.validated_commit.as_deref().unwrap_or("-"),
            d.head_commit.as_deref().unwrap_or("-"),
        );
    } else if let Some(v) = &d.guide.validated_commit {
        println!("{:<14}validated @ {}", "Freshness", v);
    }

    // Steps (with intent + result).
    let steps: Vec<&crate::db::ChecklistItem> = d
        .checklist
        .iter()
        .filter(|c| c.kind != db::STEP_KIND_ACCEPTANCE)
        .collect();
    if !steps.is_empty() {
        println!("\nSteps:");
        for (i, s) in steps.iter().enumerate() {
            let mark = if s.done { "x" } else { " " };
            let badge = if s.source == "ai" { " (ai)" } else { "" };
            println!("  [{}] {}. {}{}", mark, i + 1, s.text, badge);
            if let Some(intent) = &s.intent {
                println!("        intent: {intent}");
            }
            if let Some(v) = &s.verify_cmd {
                println!("        verify: {v}");
            }
            if let Some(r) = &s.result {
                println!("        result: {r}");
            }
            if s.done && (s.done_commit.is_some() || s.done_at.is_some()) {
                let commit = s
                    .done_commit
                    .as_deref()
                    .map(|c| {
                        let short: String = c.chars().take(8).collect();
                        format!("@ {short} ")
                    })
                    .unwrap_or_default();
                let when = s.done_at.as_deref().unwrap_or("");
                println!("        done:   {commit}{when}");
            }
        }
    }

    // Acceptance criteria.
    let acceptance: Vec<&crate::db::ChecklistItem> = d
        .checklist
        .iter()
        .filter(|c| c.kind == db::STEP_KIND_ACCEPTANCE)
        .collect();
    if !acceptance.is_empty() {
        println!("\nAcceptance criteria:");
        for (i, a) in acceptance.iter().enumerate() {
            let mark = if a.done { "x" } else { " " };
            println!("  [{}] {}. {}", mark, i + 1, a.text);
        }
    }

    // Verification commands (project + task-level).
    let verif = verification_rows(d);
    if !verif.is_empty() {
        println!("\nVerification:");
        for (scope, label, cmd) in &verif {
            println!("  {label:<7} {cmd}  ({scope})");
        }
    }

    // Typed AI/human notes grouped by kind.
    for (label, kind) in [
        ("Findings", "finding"),
        ("Constraints", "constraint"),
        ("Assumptions", "assumption"),
        ("Open questions", "open_question"),
        ("Non-goals", "non_goal"),
        ("Decisions", "decision"),
        ("Risks", "risk"),
        ("Patterns", "pattern"),
    ] {
        let notes = notes_of_kind(d, kind);
        if !notes.is_empty() {
            println!("\n{label}:");
            for n in notes {
                let badge = if n.author == "ai" { " (ai)" } else { "" };
                println!("  - {}{}", n.text, badge);
            }
        }
    }

    // Code anchors (relevant files with reasons).
    let suggested: Vec<&crate::db::Anchor> = d
        .anchors
        .iter()
        .filter(|a| a.source == db::SOURCE_SUGGESTED)
        .collect();
    if !suggested.is_empty() {
        println!("\nRelevant code anchors (suggested by AI):");
        for a in suggested {
            println!("  {}{}", a.path, a.location());
            if let Some(r) = &a.reason {
                println!("      {r}");
            }
        }
    }

    for b in &d.blocked_by {
        println!("{:<14}{}", "Blocked by", b);
    }
    for b in &d.blocking {
        println!("{:<14}{}", "Blocking", b);
    }
    for link in &d.links {
        println!(
            "{:<14}[{}] {}  {}",
            "Link",
            link.id,
            link.display(),
            link.url
        );
    }
    for file in &d.manual_files {
        println!("{:<14}{}", "File", file);
    }
    // Comments (human feedback), with anchor + reconsider markers.
    let comments = notes_of_kind(d, "comment");
    if !comments.is_empty() {
        println!("\nComments:");
        for a in comments {
            let date = a.entry.with_timezone(&Local).format("%Y-%m-%d %H:%M");
            let target = match (&a.target_kind, &a.target_id) {
                (Some(k), Some(idv)) => format!(" [{k}:{idv}]"),
                _ => String::new(),
            };
            let flag = if a.request_revision {
                " (reconsider)"
            } else {
                ""
            };
            let resolved = if a.status == "resolved" {
                " (resolved)"
            } else {
                ""
            };
            println!(
                "  #{}{}{}{} {} {}",
                a.id, target, flag, resolved, date, a.text
            );
        }
    }
    // AI activity footer.
    if !d.ai_runs.is_empty() {
        println!("\nAI activity:");
        for r in &d.ai_runs {
            let date = r.created_at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
            println!(
                "  {} via {} [{}] @ {}",
                r.kind,
                r.model.as_deref().unwrap_or("?"),
                r.provider.as_deref().unwrap_or("?"),
                date
            );
        }
    }
    for h in &d.history {
        let date = h.changed_at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
        let change = if h.field == "created" {
            h.new_value.clone().unwrap_or_default()
        } else if h.field == "annotation" {
            match (&h.new_value, &h.old_value) {
                (Some(text), _) => format!("comment added: {text}"),
                (None, Some(text)) => format!("comment removed: {text}"),
                _ => "comment".to_string(),
            }
        } else {
            format!(
                "{}: {} -> {}",
                h.field,
                h.old_value.as_deref().unwrap_or("-"),
                h.new_value.as_deref().unwrap_or("-"),
            )
        };
        println!("{:<14}{} {}", "History", date, change);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task() -> Task {
        Task::new("original".into(), "tk".into())
    }

    #[test]
    fn editing_description_updates_value() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Description, "new description", &cfg);
        assert_eq!(t.description, "new description");
    }

    #[test]
    fn empty_description_is_ignored() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Description, "   ", &cfg);
        assert_eq!(t.description, "original");
    }

    #[test]
    fn editing_tags_splits_and_trims() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Tags, " rust , cli ,", &cfg);
        assert_eq!(t.tags, vec!["rust".to_string(), "cli".to_string()]);
    }

    #[test]
    fn editing_due_empty_clears_it() {
        let mut t = task();
        let cfg = Config::default();
        t.due = Some(Utc::now());
        apply_field(&mut t, EditField::Due, "", &cfg);
        assert!(t.due.is_none());
    }

    #[test]
    fn editing_due_parses_relative() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Due, "+3d", &cfg);
        assert!(t.due.is_some());
    }

    #[test]
    fn priority_cycles_forward_and_back() {
        let mut t = task();
        assert!(t.priority.is_none());
        cycle_priority(&mut t, true);
        assert_eq!(t.priority, Some(Priority::L));
        cycle_priority(&mut t, true);
        assert_eq!(t.priority, Some(Priority::M));
        cycle_priority(&mut t, true);
        assert_eq!(t.priority, Some(Priority::H));
        cycle_priority(&mut t, true);
        assert!(t.priority.is_none());
        cycle_priority(&mut t, false);
        assert_eq!(t.priority, Some(Priority::H));
    }

    #[test]
    fn current_value_round_trips_with_apply() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Project, "myproj", &cfg);
        assert_eq!(current_value(&t, EditField::Project), "myproj");
        apply_field(&mut t, EditField::Tags, "a, b", &cfg);
        assert_eq!(current_value(&t, EditField::Tags), "a, b");
    }
}
