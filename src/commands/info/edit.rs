use anyhow::Result;
use chrono::{Local, Utc};
use crossterm::event::KeyCode;
use ratatui::{Terminal, backend::Backend};
use ratatui_textarea::TextArea;
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::{Priority, Task};
use crate::infrastructure::tui;
use crate::infrastructure::tui::keymap::{self, Action, KeyDispatcher, Mode};

use super::handler::{
    build_task_tree, checklist_focus_index, comment_target, depends_on_display,
    edit_text_via_external_editor, feedback_for_focus, find_pr_url, focusables, open_in_editor,
    open_url, reconcile_dependencies, reorder_focused_step,
};
use super::render::render;
use super::types::{Detail, EditField, EditState, Focusable};
use crate::commands::shared::{parse_due, parse_duration_mins, split_csv};

pub(super) fn edit_loop<B: Backend<Error: Send + Sync + 'static>>(
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
        adding_step: false,
        editor: TextArea::default(),
        due_error: false,
        dep_error: None,
        scroll: 0,
        last_selected: None,
        tree_expanded: false,
        show_urgency_breakdown: false,
        verbose: false,
        show_notes: false,
    };
    let mut dispatcher = KeyDispatcher::new();
    let mut showing_help = false;

    loop {
        terminal.draw(|f| {
            render(f, &mut st);
            if showing_help {
                tui::render_help_overlay(f, "Task detail", &help_bindings());
            }
        })?;

        let Some(key) = crate::infrastructure::tui::next_key(100)? else {
            continue;
        };

        if showing_help {
            showing_help = false;
            continue;
        }

        let items = focusables(&st.detail, st.show_notes);
        if !items.is_empty() && st.selected >= items.len() {
            st.selected = items.len() - 1;
        }
        let current = items.get(st.selected).cloned();
        let current_field = match &current {
            Some(Focusable::Field(f)) => Some(*f),
            _ => None,
        };

        if st.adding_step {
            match dispatcher.dispatch(key, Mode::Insert) {
                Action::Confirm | Action::Save => {
                    let text = st.editor.lines().join(" ");
                    if !text.trim().is_empty() {
                        let kind = match &current {
                            Some(Focusable::Checklist(i)) => st
                                .detail
                                .checklist
                                .get(*i)
                                .map(|c| c.kind.clone())
                                .unwrap_or_else(|| db::STEP_KIND_STEP.to_string()),
                            _ => db::STEP_KIND_STEP.to_string(),
                        };
                        let new_id = db::add_step(
                            conn,
                            &st.detail.task.uuid,
                            text.trim(),
                            None,
                            &kind,
                            "human",
                            None,
                        )
                        .ok();
                        st.detail.checklist =
                            db::get_checklist(conn, &st.detail.task.uuid).unwrap_or_default();
                        if let Some(id) = new_id
                            && let Some(p) = checklist_focus_index(&st.detail, st.show_notes, id)
                        {
                            st.selected = p;
                        }
                    }
                    st.adding_step = false;
                }
                Action::Cancel => st.adding_step = false,
                Action::Raw(k) => {
                    st.editor.input(k);
                }
                _ => {}
            }
        } else if st.commenting {
            match dispatcher.dispatch(key, Mode::Insert) {
                Action::Confirm | Action::Save => {
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
                Action::Cancel => st.commenting = false,
                Action::Raw(k) => {
                    st.editor.input(k);
                }
                _ => {}
            }
        } else if st.editing {
            let field = current_field.unwrap_or(EditField::Description);
            match dispatcher.dispatch(key, Mode::Insert) {
                Action::Confirm | Action::Save => {
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
                        && !crate::infrastructure::util::dates::is_valid_due(&value)
                    {
                        st.due_error = true;
                        continue;
                    }
                    apply_field(&mut st.detail.task, field, &value, cfg);
                    save(conn, cfg, &mut st.detail)?;
                    st.editing = false;
                    st.due_error = false;
                }
                Action::Cancel => {
                    st.editing = false;
                    st.due_error = false;
                    st.dep_error = None;
                }
                Action::Raw(k) => {
                    st.editor.input(k);
                    if field == EditField::Due {
                        let v = st.editor.lines().join("");
                        st.due_error = !v.trim().is_empty()
                            && !crate::infrastructure::util::dates::is_valid_due(&v);
                    }
                }
                _ => {}
            }
        } else {
            let action = dispatcher.dispatch(key, Mode::Normal);
            let confirmed = matches!(action, Action::Confirm)
                || matches!(&action, Action::Raw(k) if k.code == KeyCode::Char('e'));
            if confirmed {
                match current {
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
                            open_url(&path);
                        } else {
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
                    Some(Focusable::Comment(_)) => {
                        st.editor = TextArea::default();
                        st.commenting = true;
                    }
                    Some(Focusable::Note(_)) => {
                        st.editor = TextArea::default();
                        st.commenting = true;
                    }
                    None => {}
                }
            } else {
                match action {
                    Action::Quit => break,
                    Action::ReorderUp => reorder_focused_step(conn, &mut st, &current, true),
                    Action::ReorderDown => reorder_focused_step(conn, &mut st, &current, false),
                    Action::Down => {
                        if !items.is_empty() {
                            st.selected = (st.selected + 1).min(items.len() - 1);
                        }
                    }
                    Action::Up => {
                        st.selected = st.selected.saturating_sub(1);
                    }
                    Action::Top => st.selected = 0,
                    Action::Bottom => {
                        if !items.is_empty() {
                            st.selected = items.len() - 1;
                        }
                    }
                    Action::PageDown => st.scroll = st.scroll.saturating_add(5),
                    Action::PageUp => st.scroll = st.scroll.saturating_sub(5),
                    Action::ToggleMark => {
                        if let Some(Focusable::Checklist(i)) = &current
                            && let Some(item) = st.detail.checklist.get(*i)
                        {
                            let _ = db::toggle_checklist_item(conn, item.id);
                            st.detail.checklist =
                                db::get_checklist(conn, &st.detail.task.uuid).unwrap_or_default();
                        }
                    }
                    Action::ExternalEdit => {
                        if current_field == Some(EditField::Description) {
                            tui::suspend()?;
                            let result = edit_text_via_external_editor(&st.detail.task.description);
                            tui::resume()?;
                            terminal.clear()?;
                            if let Ok(Some(edited)) = result {
                                apply_field(
                                    &mut st.detail.task,
                                    EditField::Description,
                                    &edited,
                                    cfg,
                                );
                                save(conn, cfg, &mut st.detail)?;
                            }
                        } else {
                            tui::suspend()?;
                            let result = edit_text_via_external_editor("");
                            tui::resume()?;
                            terminal.clear()?;
                            if let Ok(Some(text)) = result {
                                let text = text.trim();
                                if !text.is_empty() {
                                    let (tk, tid) = comment_target(&st.detail, &current);
                                    let _ = db::add_annotation_full(
                                        conn,
                                        &st.detail.task.uuid,
                                        text,
                                        db::NOTE_KIND_COMMENT,
                                        "human",
                                        tk.as_deref(),
                                        tid.as_deref(),
                                        false,
                                    );
                                    st.detail.annotations =
                                        db::get_annotations(conn, &st.detail.task.uuid)
                                            .unwrap_or_default();
                                }
                            }
                        }
                    }
                    Action::Raw(k) => match k.code {
                        KeyCode::Left if current_field == Some(EditField::Priority) => {
                            cycle_priority(&mut st.detail.task, false);
                            save(conn, cfg, &mut st.detail)?;
                        }
                        KeyCode::Right if current_field == Some(EditField::Priority) => {
                            cycle_priority(&mut st.detail.task, true);
                            save(conn, cfg, &mut st.detail)?;
                        }
                        KeyCode::Char('a') => {
                            st.editor = TextArea::default();
                            st.adding_step = true;
                        }
                        KeyCode::Char('d') => {
                            st.tree_expanded = !st.tree_expanded;
                        }
                        KeyCode::Char('c') => {
                            st.editor = TextArea::default();
                            st.commenting = true;
                        }
                        KeyCode::Char('r') => {
                            let fb = feedback_for_focus(&st.detail, &current);
                            if let Some(existing) = fb.first() {
                                let _ = db::set_request_revision(
                                    conn,
                                    existing.id,
                                    !existing.request_revision,
                                );
                            } else {
                                let (tk, tid) = comment_target(&st.detail, &current);
                                let _ = db::add_annotation_full(
                                    conn,
                                    &st.detail.task.uuid,
                                    "⟳ reconsider this",
                                    db::NOTE_KIND_COMMENT,
                                    "human",
                                    tk.as_deref(),
                                    tid.as_deref(),
                                    true,
                                );
                            }
                            st.detail.annotations =
                                db::get_annotations(conn, &st.detail.task.uuid).unwrap_or_default();
                        }
                        KeyCode::Char('x') => {
                            if let Some(fb) = feedback_for_focus(&st.detail, &current).first() {
                                let _ = db::resolve_annotation(conn, fb.id, None);
                                st.detail.annotations =
                                    db::get_annotations(conn, &st.detail.task.uuid)
                                        .unwrap_or_default();
                            }
                        }
                        KeyCode::Char('u') => {
                            st.show_urgency_breakdown = !st.show_urgency_breakdown;
                        }
                        KeyCode::Char('v') => {
                            st.verbose = !st.verbose;
                        }
                        KeyCode::Char('n') => {
                            st.show_notes = !st.show_notes;
                            let items = focusables(&st.detail, st.show_notes);
                            if !items.is_empty() && st.selected >= items.len() {
                                st.selected = items.len() - 1;
                            }
                        }
                        KeyCode::Char('o') => {
                            let url = match &current {
                                Some(Focusable::Link(i)) => {
                                    st.detail.links.get(*i).map(|l| l.url.clone())
                                }
                                _ => find_pr_url(&st.detail),
                            };
                            if let Some(url) = url {
                                open_url(&url);
                            }
                        }
                        KeyCode::Char('?') => {
                            showing_help = true;
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

fn help_bindings() -> Vec<(&'static str, &'static str)> {
    use keymap::help::*;
    vec![
        MOVE,
        TOP_BOTTOM,
        PAGE,
        REORDER,
        TOGGLE_MARK,
        CONFIRM,
        ("e", "edit the focused field"),
        ("a", "add a checklist step"),
        ("c", "comment on the focused element"),
        ("r", "flag the focused element for reconsideration"),
        ("x", "resolve the focused element's feedback"),
        (
            "o",
            "open the highlighted link (else the task's PR) in the browser",
        ),
        ("d", "expand/collapse the task tree"),
        ("u", "show/hide the urgency score breakdown"),
        ("v", "expand/collapse long text and checklist detail"),
        (
            "n",
            "show/hide the AI's execution notes (findings, decisions, …)",
        ),
        SAVE,
        QUIT,
        HELP,
    ]
}

pub(super) fn editor_for(task: &Task, field: EditField) -> TextArea<'static> {
    let value = current_value(task, field);
    let mut ta = TextArea::default();
    ta.insert_str(&value);
    ta
}

pub(super) fn current_value(task: &Task, field: EditField) -> String {
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
        EditField::DependsOn => String::new(),
    }
}

pub(super) fn apply_field(task: &mut Task, field: EditField, value: &str, cfg: &Config) {
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
                task.due = parse_due(value, cfg);
            }
        }
        EditField::Tags => {
            task.tags = split_csv(value);
        }
        EditField::Priority => {}
        EditField::Estimate => {
            task.estimate_mins = parse_duration_mins(value);
        }
        EditField::Recur => {
            let v = value.trim().to_lowercase();
            task.recur = if v.is_empty() { None } else { Some(v) };
        }
        EditField::DependsOn => {}
    }
}

pub(super) fn cycle_priority(task: &mut Task, forward: bool) {
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

pub(super) fn save(conn: &Connection, cfg: &Config, detail: &mut Detail) -> Result<()> {
    let task = &mut detail.task;
    task.modified = Utc::now();
    task.urgency = db::compute_urgency(task, &cfg.urgency, false, 0);
    db::update_task(conn, task)?;
    db::refresh_urgency(conn, &cfg.urgency, &task.uuid)?;
    if let Some(t) = db::get_task_by_uuid_prefix(conn, &task.uuid.to_string()[..8])? {
        task.urgency = t.urgency;
    }
    detail.history = db::get_history(conn, &detail.task.uuid)?;
    detail.project_root = db::get_project(conn, &detail.task.project)?
        .and_then(|p| p.path)
        .map(std::path::PathBuf::from);
    detail.branch = db::get_task_branch(conn, &detail.task.uuid);
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
    detail.tree = build_task_tree(conn, detail.task.uuid);
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/info/edit.rs"]
mod tests;
