//! Integration tests for `sara_tasks::commands::info` (plain/markdown render + edit).
//! Moved out of an inline mod tests block in src/commands/info/plain.rs.

use chrono::Utc;
use sara_tasks::commands::info::edit::{apply_field, current_value, cycle_priority};
use sara_tasks::commands::info::plain::{RenderOpts, render_markdown, render_plain};
use sara_tasks::commands::info::types::{Detail, EditField};
use sara_tasks::infrastructure::config::Config;
use sara_tasks::infrastructure::db;
use sara_tasks::infrastructure::model::{Priority, Task};

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

fn step(
    id: i64,
    text: &str,
    kind: &str,
    done: bool,
) -> sara_tasks::infrastructure::db::ChecklistItem {
    sara_tasks::infrastructure::db::ChecklistItem {
        id,
        text: text.into(),
        done,
        position: id,
        intent: None,
        kind: kind.into(),
        source: "human".into(),
        verify_cmd: None,
        result: None,
        done_commit: None,
        done_at: None,
    }
}

fn history(
    field: &str,
    old: Option<&str>,
    new: Option<&str>,
) -> sara_tasks::infrastructure::db::HistoryEntry {
    sara_tasks::infrastructure::db::HistoryEntry {
        field: field.into(),
        old_value: old.map(Into::into),
        new_value: new.map(Into::into),
        changed_at: chrono::Utc::now(),
    }
}

fn detail(
    checklist: Vec<sara_tasks::infrastructure::db::ChecklistItem>,
    hist: Vec<sara_tasks::infrastructure::db::HistoryEntry>,
) -> Detail {
    Detail {
        task: task(),
        blocked_by: vec![],
        blocking: vec![],
        depends_on_ids: vec![],
        manual_files: vec![],
        suggested_files: vec![],
        links: vec![],
        annotations: vec![],
        history: hist,
        project_root: None,
        branch: None,
        overlaps: vec![],
        similar: vec![],
        checklist,
        urgency_breakdown: None,
        activity: std::collections::HashMap::new(),
        stats: None,
        guide: sara_tasks::infrastructure::db::TaskGuideFields::default(),
        anchors: vec![],
        ai_runs: vec![],
        head_commit: None,
        project_commands: sara_tasks::infrastructure::db::ProjectCommands::default(),
        chain: vec![],
    }
}

#[test]
fn render_plain_collapses_history_by_default() {
    let d = detail(
        vec![],
        vec![history("status", Some("pending"), Some("done"))],
    );
    let collapsed = render_plain(&d, RenderOpts { history: false });
    assert!(collapsed.contains("1 entries (use --history to show)"));
    assert!(!collapsed.contains("status: pending -> done"));

    let full = render_plain(&d, RenderOpts { history: true });
    assert!(full.contains("status: pending -> done"));
}

#[test]
fn render_plain_lists_steps_and_acceptance() {
    let d = detail(
        vec![
            step(1, "do the thing", db::STEP_KIND_STEP, true),
            step(2, "ship it", db::STEP_KIND_ACCEPTANCE, false),
        ],
        vec![],
    );
    let out = render_plain(&d, RenderOpts::default());
    assert!(out.contains("Steps:"));
    assert!(out.contains("[x] 1. do the thing"));
    assert!(out.contains("Acceptance criteria:"));
    assert!(out.contains("[ ] 1. ship it"));
}

#[test]
fn render_markdown_has_description_steps_and_acceptance() {
    let d = detail(
        vec![
            step(1, "first step", db::STEP_KIND_STEP, false),
            step(2, "second step", db::STEP_KIND_STEP, true),
            step(3, "definition of done", db::STEP_KIND_ACCEPTANCE, false),
        ],
        vec![],
    );
    let md = render_markdown(&d, RenderOpts::default());
    // Description present.
    assert!(md.contains("## Description"));
    assert!(md.contains("original"));
    // Steps rendered as GitHub-style checkboxes.
    assert!(md.contains("## Steps"));
    assert!(md.contains("- [ ] first step"));
    assert!(md.contains("- [x] second step"));
    // Acceptance criteria rendered with checkboxes.
    assert!(md.contains("## Acceptance criteria"));
    assert!(md.contains("- [ ] definition of done"));
}

#[test]
fn render_markdown_omits_history_unless_requested() {
    let d = detail(
        vec![],
        vec![history("status", Some("pending"), Some("done"))],
    );
    let lean = render_markdown(&d, RenderOpts { history: false });
    assert!(!lean.contains("## History"));

    let full = render_markdown(&d, RenderOpts { history: true });
    assert!(full.contains("## History"));
    assert!(full.contains("status: pending -> done"));
}
