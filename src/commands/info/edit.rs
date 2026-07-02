use anyhow::Result;
use chrono::{Local, Utc};
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{Terminal, backend::Backend};
use rusqlite::Connection;
use tui_textarea::TextArea;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::{Priority, Task};
use crate::infrastructure::tui;

use super::handler::{
    checklist_focus_index, comment_target, compute_overlaps, depends_on_display,
    feedback_for_focus, focusables, open_in_editor, open_url, reconcile_dependencies,
    reorder_focused_step,
};
use super::render::render;
use super::types::{Detail, EditField, EditState, Focusable};

pub(super) fn edit_loop<B: Backend>(
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

        if st.adding_step {
            match key.code {
                KeyCode::Enter => {
                    let text = st.editor.lines().join(" ");
                    if !text.trim().is_empty() {
                        // Add to the kind of the focused checklist row (so adding
                        // while on an acceptance criterion adds another criterion);
                        // default to a step otherwise.
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
                            && let Some(p) = checklist_focus_index(&st.detail, id)
                        {
                            st.selected = p;
                        }
                    }
                    st.adding_step = false;
                }
                KeyCode::Esc => st.adding_step = false,
                _ => {
                    st.editor.input(key);
                }
            }
        } else if st.commenting {
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
                        && !crate::infrastructure::dates::is_valid_due(&value)
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
                        st.due_error =
                            !v.trim().is_empty() && !crate::infrastructure::dates::is_valid_due(&v);
                    }
                }
            }
        } else {
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break,
                // Reorder the focused checklist step within its kind.
                // Uppercase J/K are inherently shifted; Shift+Arrow is the alias.
                KeyCode::Char('K') => reorder_focused_step(conn, &mut st, &current, true),
                KeyCode::Char('J') => reorder_focused_step(conn, &mut st, &current, false),
                KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    reorder_focused_step(conn, &mut st, &current, true);
                }
                KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    reorder_focused_step(conn, &mut st, &current, false);
                }
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
                // ── Add a new checklist step ────────────────────────────────
                KeyCode::Char('a') => {
                    st.editor = TextArea::default();
                    st.adding_step = true;
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

pub(super) fn editor_for(task: &Task, field: EditField) -> TextArea<'static> {
    let value = current_value(task, field);
    let mut ta = TextArea::default();
    ta.insert_str(&value);
    ta
}

pub fn current_value(task: &Task, field: EditField) -> String {
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

pub fn apply_field(task: &mut Task, field: EditField, value: &str, cfg: &Config) {
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

pub fn cycle_priority(task: &mut Task, forward: bool) {
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

/// Parse a human duration string like "2h30m", "90m", "1h", "45" (minutes) into minutes.
pub(super) fn parse_duration_to_mins(s: &str) -> Option<i64> {
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
