use chrono::{Local, Utc};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};

use crate::infrastructure::db;
use crate::infrastructure::model::{Priority, Task, format_duration};

use super::edit::current_value;
use super::handler::{
    TREE_COMPACT_CHILDREN, TREE_COMPACT_DEPTH, comment_target, depends_on_display, focusables,
    guide_is_stale, notes_of_kind, typed_notes, verification_rows,
};
use super::types::{Detail, EDIT_FIELDS, EditField, EditState, Focusable, GraphNode};

pub(super) fn render(f: &mut Frame, st: &mut EditState) {
    let area = f.area();
    let d = &st.detail;

    let history_height: u16 = if d.history.is_empty() {
        0
    } else {
        (d.history.len() as u16 + 2).min(6) // border (2) + up to 4 most-recent entries
    };

    let constraints = if st.editing || st.commenting || st.adding_step {
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

    // Wide enough to show the task-tree side panel — when it's shown, the
    // plain "Blocked by"/"Blocking" text lists below are redundant (the tree
    // already covers direct neighbors) and are skipped.
    let show_panel = chunks[0].width >= 96;

    let mut lines: Vec<Line> = vec![];
    // Display-line range (start..=end, pre-wrap indices into `lines`) of the
    // focused row, captured while building so the viewport can follow it.
    let mut sel_range: Option<(usize, usize)> = None;

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
        if selected {
            sel_range = Some((lines.len() - 1, lines.len() - 1));
        }
    }

    // ── Read-only fields
    // Status is only worth a row when it's not the boring default — a task
    // open in this view is pending the overwhelming majority of the time.
    if t.status != crate::infrastructure::model::Status::Pending {
        lines.push(field_line("Status", &t.status.to_string()));
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

    // Urgency: bare number by default, additive breakdown behind 'u' — the
    // formula is only interesting when the score looks surprising.
    {
        let breakdown_str = if st.show_urgency_breakdown {
            urgency_breakdown_str(d)
        } else {
            String::new()
        };
        let hint = if !st.show_urgency_breakdown && d.urgency_breakdown.is_some() {
            "  (u for breakdown)"
        } else {
            ""
        };
        lines.push(Line::from(vec![
            key_span("Urgency"),
            Span::raw(format!("{:.1}", t.urgency)),
            Span::styled(
                format!("{breakdown_str}{hint}"),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    // Entered, with the task's age folded in rather than its own row.
    {
        let age_days = (Utc::now() - t.entry).num_days();
        let age_str = if age_days == 0 {
            "today".to_string()
        } else if age_days == 1 {
            "1 day ago".to_string()
        } else {
            format!("{age_days} days ago")
        };
        lines.push(Line::from(vec![
            key_span("Entered"),
            Span::raw(
                t.entry
                    .with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string(),
            ),
            Span::styled(
                format!("  ({age_str})"),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    lines.push(field_line(
        "Modified",
        &t.modified
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string(),
    ));

    // ── Guide: assignment / rationale / freshness banner ────────────
    // Collapsed to ~2 lines by default (the full text is what most guides
    // need at a glance); 'v' expands to the full text.
    if let Some(a) = &d.guide.assignment {
        lines.push(Line::from(vec![
            key_span("Assignment"),
            Span::styled(
                collapsed_text(a, st.verbose),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }
    if let Some(r) = &d.guide.rationale {
        lines.push(Line::from(vec![
            key_span("Rationale"),
            Span::raw(collapsed_text(r, st.verbose)),
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
    let items = focusables(d, st.show_notes);
    let sel: Option<Focusable> = if st.editing {
        None
    } else {
        items.get(st.selected).cloned()
    };
    let file_selected = |path: &str| sel == Some(Focusable::File(path.to_string()));

    // ── Typed notes (findings, constraints, …) ───────────────────────────────
    // Build a flat note list once so indices match Focusable::Note(i).
    let all_typed = typed_notes(d);
    // One combined legend for every navigable section below, instead of
    // repeating "↑/↓ select · c comment · r reconsider · x resolve" in each
    // section's own header. Shown whenever there's a focusable item beyond
    // the always-present editable metadata fields.
    if items.len() > EDIT_FIELDS.len() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  ↑/↓ select · Enter open/toggle · c comment · r reconsider · x resolve",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    }
    // "Risks" always renders in full — it's the one typed-note kind that
    // answers what a human reviewer actually wants (impact/what could go
    // wrong), not the AI's own execution workpaper. Every other kind
    // (findings, constraints, assumptions, decisions, …) collapses to a
    // single counted summary line unless `show_notes` is toggled on.
    let mut note_cursor: usize = 0; // tracks position in all_typed across kinds
    let mut hidden_note_counts: Vec<(&str, usize)> = Vec::new();
    for (label, kind) in [
        ("Risks", "risk"),
        ("Findings", "finding"),
        ("Constraints", "constraint"),
        ("Assumptions", "assumption"),
        ("Open questions", "open_question"),
        ("Non-goals", "non_goal"),
        ("Decisions", "decision"),
        ("Patterns", "pattern"),
    ] {
        let notes = notes_of_kind(d, kind);
        if notes.is_empty() {
            continue;
        }
        if kind != "risk" && !st.show_notes {
            hidden_note_counts.push((label, notes.len()));
            note_cursor += notes.len();
            continue;
        }
        lines.push(Line::from(""));
        lines.push(section(label));
        for n in &notes {
            let note_idx = note_cursor;
            note_cursor += 1;
            let is_sel = sel == Some(Focusable::Note(note_idx));
            let row_bg = if is_sel { Color::Blue } else { Color::Reset };
            let row_fg = if is_sel { Color::White } else { Color::Reset };

            // Open comments targeting this note.
            let note_id_str = n.id.to_string();
            let note_fb: Vec<&crate::infrastructure::db::Annotation> = d
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
                    collapsed_text(&n.text, st.verbose || is_sel),
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
            if is_sel {
                sel_range = Some((lines.len(), lines.len()));
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
    if !hidden_note_counts.is_empty() {
        let total: usize = hidden_note_counts.iter().map(|(_, c)| c).sum();
        let breakdown = hidden_note_counts
            .iter()
            .map(|(label, c)| format!("{c} {}", label.to_lowercase()))
            .collect::<Vec<_>>()
            .join(" · ");
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!(
                "  {total} AI work note{} ({breakdown})  — the AI's execution workpaper, not usually needed for review  (n to view)",
                if total == 1 { "" } else { "s" }
            ),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    }

    // On narrow terminals there's no room for the task-tree panel, so these
    // stay as the only view of blockers/dependents; on wide terminals the
    // tree already shows them (with more structure), so skip the duplicate.
    if !show_panel {
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
    }
    // (sel / items / file_selected already computed above — before typed notes)

    if !d.links.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Links"));
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
            if selected {
                sel_range = Some((lines.len(), lines.len()));
            }
            lines.push(Line::from(spans));
        }
    }
    if !d.manual_files.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Relevant files"));
        for file in &d.manual_files {
            let selected = file_selected(file);
            if selected {
                sel_range = Some((lines.len(), lines.len()));
            }
            lines.push(nav_line(file, Color::Cyan, false, selected));
        }
    }
    // ── Code anchors: each is focusable, shows 💬/⟳ markers + threaded comments ──
    if !d.anchors.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Possible relevant files"));
        for (ai, anchor) in d.anchors.iter().enumerate() {
            let is_sel = sel == Some(Focusable::Anchor(ai));
            let file_text = format!("{}{}", anchor.path, anchor.location());
            let badge = if anchor.source == db::SOURCE_SUGGESTED {
                " (ai)"
            } else {
                ""
            };

            // Threaded comments anchored to this file.
            let anchor_fb: Vec<&crate::infrastructure::db::Annotation> = d
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
            if is_sel {
                sel_range = Some((lines.len(), lines.len()));
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
        lines.push(section(&format!("Checklist  {progress}")));
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
            let fb: Vec<&crate::infrastructure::db::Annotation> = d
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
            if is_sel {
                sel_range = Some((lines.len(), lines.len()));
            }
            lines.push(Line::from(spans));
            // Intent/verify/result/provenance detail is only shown for the
            // selected row (or in verbose mode) — with many AI-authored steps
            // this metadata otherwise buries the checklist itself.
            let show_detail = is_sel || st.verbose;
            if show_detail {
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
            // The selected step reveals its intent/verify/result detail and
            // comment thread below the row — keep that block in view too.
            if is_sel && let Some((start, _)) = sel_range {
                sel_range = Some((start, lines.len() - 1));
            }
        }
    }

    // ── Verification: how to test/lint/run this task (project + task commands)
    let verif = verification_rows(d);
    if !verif.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Verification  (run: sara verify <id> --run)"));
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
    // ── Similar tasks (shared tags, same project) — low-signal, so only the
    // top few by urgency are shown; the rest collapse into a count.
    if !d.similar.is_empty() {
        const RELATED_SHOWN: usize = 3;
        let mut similar: Vec<&(i64, String, f64)> = d.similar.iter().collect();
        similar.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        lines.push(Line::from(""));
        lines.push(section("Related tasks (shared tags)"));
        for (id, desc, urg) in similar.iter().take(RELATED_SHOWN) {
            lines.push(Line::from(vec![
                Span::styled(format!("  #{id:<3} "), Style::default().fg(Color::Gray)),
                Span::raw(desc.clone()),
                Span::styled(
                    format!("  urg {urg:.1}"),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
        if similar.len() > RELATED_SHOWN {
            lines.push(Line::from(Span::styled(
                format!("  … {} more", similar.len() - RELATED_SHOWN),
                Style::default()
                    .fg(Color::DarkGray)
                    .add_modifier(Modifier::ITALIC),
            )));
        }
    }
    // ── Comments section: task-level + replies only (anchored ones shown inline above) ─
    let all_comments: Vec<&crate::infrastructure::db::Annotation> = d
        .annotations
        .iter()
        .filter(|a| a.kind == "comment")
        .collect();
    let unthreaded: Vec<&crate::infrastructure::db::Annotation> = all_comments
        .iter()
        .copied()
        .filter(|a| a.target_kind.as_deref() != Some("anchor"))
        .collect();
    if !unthreaded.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Comments"));
        // Build an index: comment-id -> annotation, for resolving note: replies.
        let id_map: std::collections::HashMap<i64, &crate::infrastructure::db::Annotation> =
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
            if is_sel {
                sel_range = Some((lines.len(), lines.len()));
            }
            lines.push(Line::from(spans));
        }
    }

    // History is rendered in its own box at the bottom — not in the main lines.

    // Split the main content area horizontally when wide enough for the
    // panel — the task tree goes first (leftmost): it's the "how does this
    // fit together" orientation a reviewer wants before the task's own
    // details, not an afterthought tucked off to the side.
    let (main_area, panel_area) = if show_panel {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(42), Constraint::Min(50)])
            .split(chunks[0]);
        (cols[1], Some(cols[0]))
    } else {
        (chunks[0], None)
    };

    // ── Scroll-follow: keep the highlighted row in view ─────────────────
    // Only when the selection actually moved (arrow keys / j/k / g/G) — a
    // manual PageUp/PageDown scroll is left alone so the user can still
    // peek around freely. `Paragraph::scroll` counts wrapped (visual) rows,
    // so logical lines are measured at the pane's inner width.
    let inner_w = main_area.width.saturating_sub(2).max(1);
    let viewport = main_area.height.saturating_sub(2) as usize;
    let selection_moved = st.last_selected != Some(st.selected);
    st.last_selected = Some(st.selected);
    if viewport > 0 {
        if selection_moved && let Some((first, last)) = sel_range {
            let top: usize = lines[..first]
                .iter()
                .map(|l| wrapped_rows(l, inner_w))
                .sum();
            let bottom: usize = top
                + lines[first..=last]
                    .iter()
                    .map(|l| wrapped_rows(l, inner_w))
                    .sum::<usize>();
            let mut scroll = st.scroll as usize;
            if bottom > scroll + viewport {
                scroll = bottom - viewport;
            }
            // Applied second so the top of a taller-than-viewport block wins.
            if top < scroll {
                scroll = top;
            }
            st.scroll = scroll.min(u16::MAX as usize) as u16;
        }
        // Never leave the viewport scrolled past the end of the content.
        let total: usize = lines.iter().map(|l| wrapped_rows(l, inner_w)).sum();
        st.scroll = st
            .scroll
            .min(total.saturating_sub(viewport).min(u16::MAX as usize) as u16);
    }

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false })
        .scroll((st.scroll, 0));
    f.render_widget(para, main_area);

    // ── Task tree (top) + Git
    if let Some(panel) = panel_area {
        // The task tree is always shown — it's the primary answer to "how is
        // this task tied to others" — compact by default, 'd' expands it.
        let tree_lines = task_tree_lines(d, st);
        // +2 for the panel's own border.
        let top_h: u16 = ((tree_lines.len() + 2) as u16).clamp(7, 24);

        // The GitHub-style activity heatmap panel and the per-task project
        // stats panel are both deprecated for now — kept out of the layout,
        // not deleted (`render_mini_heatmap`/`render_project_stats` and
        // `d.activity`/`d.stats` are still populated in case a project-wide
        // command resurfaces them later). Project-wide stats aren't
        // task-specific, so they didn't earn a permanent slot on a screen
        // about *this* task.
        let panel_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(top_h), Constraint::Min(4)])
            .split(panel);

        let tree_title = if st.tree_expanded {
            " Task tree — expanded  (d to collapse) "
        } else {
            " Task tree  (d to expand) "
        };
        let tree_para = Paragraph::new(tree_lines).block(
            Block::default()
                .borders(Borders::ALL)
                .title(tree_title)
                .border_style(Style::default().fg(Color::Magenta)),
        );
        f.render_widget(tree_para, panel_chunks[0]);

        let git_lines = git_panel_lines(d);
        let git_para = Paragraph::new(git_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Git ")
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(git_para, panel_chunks[1]);
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

    // ── Add-step bar ────────────────────────────────────────────────────────
    if st.adding_step {
        let edit_chunk_idx = if history_height > 0 { 2 } else { 1 };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Add step  (Enter save · Esc cancel) ".to_string())
            .border_style(Style::default().fg(Color::Green));
        let inner = block.inner(chunks[edit_chunk_idx]);
        f.render_widget(block, chunks[edit_chunk_idx]);
        f.render_widget(&st.editor, inner);
    }

    // ── Comment bar (anchored to the focused element)
    if st.commenting {
        let edit_chunk_idx = if history_height > 0 { 2 } else { 1 };
        let items = focusables(d, st.show_notes);
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

    let footer = if st.adding_step {
        " type a step  •  Enter/Ctrl+S save  •  Esc cancel ".to_string()
    } else if st.commenting {
        " type a comment  •  Enter/Ctrl+S save  •  Esc cancel ".to_string()
    } else if st.editing {
        " type to edit  •  Enter/Ctrl+S confirm  •  Esc cancel ".to_string()
    } else {
        " ↑/↓ move • Enter edit/open • c comment • a step • d tree • n notes • u urgency • v expand • ? help • q close "
            .to_string()
    };
    let footer_idx = chunks.len() - 1;
    f.render_widget(
        Paragraph::new(footer).style(Style::default().fg(Color::Gray)),
        chunks[footer_idx],
    );
}

/// Inner width the task tree panel is laid out for (panel is a fixed
/// `Constraint::Length(42)` column, minus 2 for the left/right border).
const TREE_PANEL_WIDTH: usize = 40;

/// Right-hand panel showing how this task is tied to others: every blocker
/// recursively above (nearest hop first), the current task highlighted in
/// the middle, every dependent recursively below — the primary answer to
/// "how did earlier work lead here, and what does finishing this unblock".
/// Replaces the old flat "feature chain" list, which flattened a branching
/// DAG into a single line and lost that structure. Compact by default (2
/// levels, a few siblings per node); 'd' expands to the tree's full fetched
/// depth/fan-out.
fn task_tree_lines(d: &Detail, st: &EditState) -> Vec<Line<'static>> {
    let (max_depth, max_children) = if st.tree_expanded {
        (usize::MAX, usize::MAX)
    } else {
        (TREE_COMPACT_DEPTH, TREE_COMPACT_CHILDREN)
    };

    let mut lines: Vec<Line<'static>> = Vec::new();
    lines.push(tree_section_header(&format!(
        "← blocked by ({})",
        d.tree.blockers.len()
    )));
    push_tree_side_lines(
        &mut lines,
        &d.tree.blockers,
        d.tree.blockers_hidden,
        max_depth,
        max_children,
    );

    let rule = "─".repeat(TREE_PANEL_WIDTH);
    lines.push(Line::from(Span::styled(
        rule.clone(),
        Style::default().fg(Color::DarkGray),
    )));
    lines.push(current_task_tree_line(&d.task));
    lines.push(Line::from(Span::styled(
        rule,
        Style::default().fg(Color::DarkGray),
    )));

    lines.push(tree_section_header(&format!(
        "blocks → ({})",
        d.tree.dependents.len()
    )));
    push_tree_side_lines(
        &mut lines,
        &d.tree.dependents,
        d.tree.dependents_hidden,
        max_depth,
        max_children,
    );
    lines
}

fn push_tree_side_lines(
    lines: &mut Vec<Line<'static>>,
    nodes: &[GraphNode],
    hidden: usize,
    max_depth: usize,
    max_children: usize,
) {
    if nodes.is_empty() && hidden == 0 {
        lines.push(Line::from(Span::styled(
            "   — none —",
            Style::default().fg(Color::DarkGray),
        )));
        return;
    }
    push_tree_node_lines(lines, nodes, hidden, "  ", 1, max_depth, max_children);
}

fn tree_section_header(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {text}"),
        Style::default()
            .fg(Color::Gray)
            .add_modifier(Modifier::BOLD),
    ))
}

fn current_task_tree_line(task: &Task) -> Line<'static> {
    let id_str = task
        .id
        .map(|n| format!("{n:>3}"))
        .unwrap_or_else(|| "  -".to_string());
    let style = Style::default()
        .fg(Color::White)
        .bg(Color::Blue)
        .add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::styled(" ▶ ", style),
        Span::styled(format!("{id_str} "), style),
        Span::styled(truncate_str(&task.description, 30), style),
    ])
}

/// Recursively append one line per node (`tree`-command style: `├─`/`└─`
/// connectors, `│ `/`  ` continuation prefixes), capped per level at
/// `max_children` siblings and `max_depth` levels — whatever's cut off by
/// either cap collapses into a trailing "+N more" / "… (d to expand)" line
/// rather than being silently dropped.
fn push_tree_node_lines(
    lines: &mut Vec<Line<'static>>,
    nodes: &[GraphNode],
    hidden_here: usize,
    prefix: &str,
    depth: usize,
    max_depth: usize,
    max_children: usize,
) {
    let total = nodes.len();
    let visible = total.min(max_children);
    let overflow = hidden_here + total.saturating_sub(visible);
    for (i, node) in nodes.iter().take(visible).enumerate() {
        let is_last = overflow == 0 && i + 1 == visible;
        let connector = if is_last { "└─" } else { "├─" };
        lines.push(tree_node_line(node, prefix, connector));
        let child_prefix = format!("{prefix}{}", if is_last { "   " } else { "│  " });
        let has_children = !node.children.is_empty() || node.hidden_children > 0;
        if has_children {
            if depth < max_depth {
                push_tree_node_lines(
                    lines,
                    &node.children,
                    node.hidden_children,
                    &child_prefix,
                    depth + 1,
                    max_depth,
                    max_children,
                );
            } else {
                lines.push(Line::from(Span::styled(
                    format!("{child_prefix}└─ … (d to expand)"),
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::ITALIC),
                )));
            }
        }
    }
    if overflow > 0 {
        lines.push(Line::from(Span::styled(
            format!("{prefix}└─ +{overflow} more  (d to expand)"),
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )));
    }
}

fn tree_node_line(node: &GraphNode, prefix: &str, connector: &str) -> Line<'static> {
    let completed = node.status == crate::infrastructure::model::Status::Completed;
    let glyph = if completed { "✓" } else { "○" };
    let id_str = node.id.map(|n| n.to_string()).unwrap_or_else(|| "-".into());
    let style = if completed {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::CROSSED_OUT)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let desc_budget = TREE_PANEL_WIDTH
        .saturating_sub(prefix.chars().count() + connector.chars().count() + id_str.len() + 4);
    let mut spans = vec![
        Span::styled(
            format!("{prefix}{connector}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(format!("{glyph} "), style),
        Span::styled(format!("{id_str} "), style),
        Span::styled(truncate_str(&node.description, desc_budget.max(6)), style),
    ];
    if let Some(label) = link_badge_label(node.badge.as_ref()) {
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(Color::Yellow),
        ));
    }
    Line::from(spans)
}

/// Badge label for a task's PR/issue links, mirroring `sara list`'s badge
/// precedence (PR > issue > generic link). Kept local rather than reusing
/// `commands::list`'s private `LinkBadge` type — command slices don't import
/// each other; only the underlying data (`link_flags_by_task`) is shared.
fn link_badge_label(flags: Option<&db::LinkFlags>) -> Option<&'static str> {
    let f = flags?;
    if f.pr {
        Some("PR")
    } else if f.issue {
        Some("ISS")
    } else if f.any {
        Some("●")
    } else {
        None
    }
}

/// Project-wide stats panel — no longer wired into the layout (it isn't
/// task-specific), kept for a possible future project-level command.
#[allow(dead_code)]
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

#[allow(dead_code)]
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
                let name = &month_abbr(m)[..3];
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

pub(super) fn history_lines(
    history: &[crate::infrastructure::db::HistoryEntry],
) -> Vec<Line<'static>> {
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

/// Number of terminal rows a logical line occupies in the main paragraph
/// once word-wrapped to `width` columns. A greedy estimate that mirrors
/// ratatui's `WordWrapper` closely enough for scroll-follow — an off-by-one
/// in a pathological wrap case only shifts the follow point by a row.
fn wrapped_rows(line: &Line, width: u16) -> usize {
    let width = width.max(1) as usize;
    if line.width() <= width {
        return 1;
    }
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    let mut rows = 1usize;
    let mut used = 0usize;
    for word in text.split(' ') {
        let w = Span::raw(word).width();
        let sep = usize::from(used > 0);
        if used + sep + w <= width {
            used += sep + w;
        } else if w > width {
            // A word wider than the pane hard-wraps mid-word.
            if used > 0 {
                rows += 1;
            }
            let mut rem = w;
            while rem > width {
                rows += 1;
                rem -= width;
            }
            used = rem;
        } else {
            rows += 1;
            used = w;
        }
    }
    rows
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
#[allow(dead_code)]
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
            format!(
                "{}  {}",
                dd.with_timezone(&Local).format("%Y-%m-%d %H:%M"),
                due_countdown_str(days),
            ),
            Style::default().fg(color),
        )
    } else {
        Span::styled(fallback.to_string(), Style::default().fg(Color::Gray))
    }
}

/// Human countdown text for a due date ("overdue by N days", "due today", …).
fn due_countdown_str(days: i64) -> String {
    if days < 0 {
        format!(
            "({} day{} overdue)",
            -days,
            if days == -1 { "" } else { "s" }
        )
    } else if days == 0 {
        "(due today)".to_string()
    } else if days == 1 {
        "(due tomorrow)".to_string()
    } else {
        format!("(due in {days} days)")
    }
}

/// Additive urgency breakdown as "(pri 1.0 + due 2.0 + …)", empty when every
/// component is zero.
fn urgency_breakdown_str(d: &Detail) -> String {
    let Some(ref bd) = d.urgency_breakdown else {
        return String::new();
    };
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
}

/// Collapse long free text to ~2 lines worth of characters with an ellipsis
/// and an expand hint, unless `verbose` is set (then the full text passes
/// through unchanged). Char-based rather than word-wrap-aware since the
/// caller's `Paragraph` already wraps — this just bounds how much of a very
/// long field shows before the reader has to opt in to more.
fn collapsed_text(s: &str, verbose: bool) -> String {
    const COLLAPSED_CHARS: usize = 160;
    if verbose || s.chars().count() <= COLLAPSED_CHARS {
        return s.to_string();
    }
    let t: String = s.chars().take(COLLAPSED_CHARS).collect();
    format!("{t}…  (v to expand)")
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

fn month_abbr(m: u32) -> &'static str {
    match m {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "???",
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::TaskTree;
    use super::*;
    use crate::infrastructure::model::{Status, Task};
    use chrono::Utc;
    use ratatui::{Terminal, backend::TestBackend};

    fn task() -> Task {
        Task::new("root task".into(), "tk".into())
    }

    fn node(id: i64, status: Status) -> GraphNode {
        GraphNode {
            uuid: uuid::Uuid::new_v4(),
            id: Some(id),
            status,
            badge: None,
            description: format!("task {id}"),
            children: vec![],
            hidden_children: 0,
        }
    }

    fn node_with_children(id: i64, children: Vec<GraphNode>) -> GraphNode {
        GraphNode {
            children,
            ..node(id, Status::Pending)
        }
    }

    fn typed_note(id: i64, kind: &str, text: &str) -> crate::infrastructure::db::Annotation {
        crate::infrastructure::db::Annotation {
            id,
            text: text.into(),
            entry: Utc::now(),
            kind: kind.into(),
            author: "ai".into(),
            target_kind: None,
            target_id: None,
            status: "open".into(),
            request_revision: false,
            resolved_by_run: None,
        }
    }

    fn base_detail(task: Task) -> Detail {
        Detail {
            task,
            blocked_by: vec![],
            blocking: vec![],
            depends_on_ids: vec![],
            manual_files: vec![],
            suggested_files: vec![],
            links: vec![],
            annotations: vec![],
            history: vec![],
            project_root: None,
            branch: None,
            overlaps: vec![],
            similar: vec![],
            checklist: vec![],
            urgency_breakdown: None,
            activity: std::collections::HashMap::new(),
            stats: None,
            guide: crate::infrastructure::db::TaskGuideFields::default(),
            anchors: vec![],
            ai_runs: vec![],
            head_commit: None,
            project_commands: crate::infrastructure::db::ProjectCommands::default(),
            tree: TaskTree::default(),
        }
    }

    fn base_state(detail: Detail) -> EditState {
        EditState {
            detail,
            selected: 0,
            editing: false,
            commenting: false,
            adding_step: false,
            editor: tui_textarea::TextArea::default(),
            due_error: false,
            dep_error: None,
            scroll: 0,
            last_selected: None,
            tree_expanded: false,
            show_urgency_breakdown: false,
            verbose: false,
            show_notes: false,
        }
    }

    fn draw(st: &mut EditState) -> String {
        // Wide enough that render()'s `chunks[0].width >= 96` gate shows the
        // side panel at all, and tall enough that the panel's own stacked
        // constraints (task tree + Git(Min 4)) don't get starved and
        // silently truncated by Layout::split.
        draw_at(st, 140, 60)
    }

    fn draw_at(st: &mut EditState, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| render(f, st)).unwrap();
        let buf = terminal.backend().buffer();
        let area = *buf.area();
        let mut out = String::new();
        for y in 0..area.height {
            let mut line = String::new();
            for x in 0..area.width {
                line.push_str(buf[(x, y)].symbol());
            }
            out.push_str(line.trim_end());
            out.push('\n');
        }
        out
    }

    #[test]
    fn task_tree_does_not_panic_when_empty() {
        let d = base_detail(task());
        let mut st = base_state(d);
        let out = draw(&mut st);
        assert!(out.contains("Task tree"));
        assert!(out.contains("none"));
    }

    #[test]
    fn task_tree_shows_branching_blockers_and_dependents() {
        let mut d = base_detail(task());
        d.tree = TaskTree {
            blockers: vec![node(1, Status::Pending), node(2, Status::Completed)],
            blockers_hidden: 0,
            dependents: vec![node(3, Status::Pending)],
            dependents_hidden: 0,
        };
        let mut st = base_state(d);
        let out = draw(&mut st);
        // Both blockers (one completed, one pending) and the dependent render
        // as distinct rows — this is exactly what the old linear feature
        // chain couldn't do for a task with neighbors in different features.
        assert!(out.contains("blocked by (2)"));
        assert!(out.contains("blocks"));
        assert!(out.contains("task 1"));
        assert!(out.contains("task 2"));
        assert!(out.contains("task 3"));
    }

    #[test]
    fn task_tree_collapses_overflow_to_a_summary_row_by_default() {
        let mut d = base_detail(task());
        d.tree = TaskTree {
            blockers: (1..=8).map(|i| node(i, Status::Pending)).collect(),
            blockers_hidden: 0,
            dependents: vec![],
            dependents_hidden: 0,
        };
        let mut st = base_state(d);
        let out = draw(&mut st);
        assert!(out.contains("more"));
    }

    #[test]
    fn task_tree_expanded_shows_everything_and_drops_the_summary_row() {
        let mut d = base_detail(task());
        d.tree = TaskTree {
            blockers: (1..=8).map(|i| node(i, Status::Pending)).collect(),
            blockers_hidden: 0,
            dependents: vec![],
            dependents_hidden: 0,
        };
        let mut st = base_state(d);
        st.tree_expanded = true;
        let out = draw(&mut st);
        assert!(!out.contains("more"));
        for i in 1..=8 {
            assert!(
                out.contains(&format!("task {i}")),
                "expected task {i} to be visible"
            );
        }
    }

    #[test]
    fn task_tree_nests_grandchildren_with_tree_connectors() {
        let mut d = base_detail(task());
        let grandchild = node(100, Status::Pending);
        let child = node_with_children(2, vec![grandchild]);
        d.tree = TaskTree {
            blockers: vec![node(1, Status::Pending), child],
            blockers_hidden: 0,
            dependents: vec![],
            dependents_hidden: 0,
        };
        let mut st = base_state(d);
        let out = draw(&mut st);
        assert!(out.contains("task 100"));
        assert!(out.contains("└─") || out.contains("├─"));
    }

    #[test]
    fn task_tree_hides_beyond_compact_depth_until_expanded() {
        // blocker(10) -> blocker(100) -> blocker(1000): three hops up.
        // Compact depth is 2, so task 1000 (the third hop) stays hidden
        // behind the expand hint until 'd' is toggled.
        let great_grandchild = node(1000, Status::Pending);
        let grandchild = node_with_children(100, vec![great_grandchild]);
        let child = node_with_children(10, vec![grandchild]);
        let mut d = base_detail(task());
        d.tree = TaskTree {
            blockers: vec![child],
            blockers_hidden: 0,
            dependents: vec![],
            dependents_hidden: 0,
        };
        let mut st = base_state(d);
        let out = draw(&mut st);
        assert!(out.contains("task 10"));
        assert!(out.contains("task 100"));
        assert!(!out.contains("task 1000"));

        st.tree_expanded = true;
        let out2 = draw(&mut st);
        assert!(out2.contains("task 1000"));
    }

    #[test]
    fn status_row_hidden_when_pending_shown_otherwise() {
        let d = base_detail(task());
        let mut st = base_state(d);
        let out = draw(&mut st);
        assert!(!out.lines().any(|l| l.trim_start().starts_with("Status")));

        let mut completed_task = task();
        completed_task.status = Status::Completed;
        let d2 = base_detail(completed_task);
        let mut st2 = base_state(d2);
        let out2 = draw(&mut st2);
        assert!(out2.contains("Status"));
        assert!(out2.contains("completed"));
    }

    #[test]
    fn urgency_breakdown_hidden_by_default_and_shown_when_toggled() {
        let mut d = base_detail(task());
        d.task.urgency = 5.0;
        d.urgency_breakdown = Some(crate::infrastructure::db::UrgencyBreakdown {
            priority: 3.0,
            due: 0.0,
            blocking: 0.0,
            blocked: 0.0,
            active: 0.0,
            tags: 0.0,
            project: 0.0,
            age: 0.0,
        });
        let mut st = base_state(d);
        let out = draw(&mut st);
        assert!(out.contains("u for breakdown"));
        assert!(!out.contains("pri 3.0"));

        st.show_urgency_breakdown = true;
        let out2 = draw(&mut st);
        assert!(out2.contains("pri 3.0"));
    }

    #[test]
    fn risk_notes_always_show_but_other_notes_collapse_until_toggled() {
        let mut d = base_detail(task());
        d.annotations = vec![
            typed_note(1, "risk", "touches the shared urgency formula"),
            typed_note(2, "finding", "existing tests cover this path"),
            typed_note(3, "decision", "kept the old signature"),
        ];
        let mut st = base_state(d);
        let out = draw(&mut st);
        // Risk is the human-relevant one — always visible.
        assert!(out.contains("Risks"));
        assert!(out.contains("touches the shared urgency formula"));
        // The AI's own process notes collapse to a counted summary instead
        // of dumping their text — that's the whole point of the toggle.
        assert!(!out.contains("existing tests cover this path"));
        assert!(!out.contains("kept the old signature"));
        assert!(out.contains("n to view"));

        let mut st2 = st;
        st2.show_notes = true;
        let out2 = draw(&mut st2);
        assert!(out2.contains("existing tests cover this path"));
        assert!(out2.contains("kept the old signature"));
    }

    #[test]
    fn long_assignment_is_collapsed_until_verbose() {
        let mut d = base_detail(task());
        let long = "x".repeat(200);
        d.guide.assignment = Some(long.clone());
        let mut st = base_state(d);
        let out = draw(&mut st);
        assert!(out.contains("v to expand"));
        assert!(!out.contains(&long));

        st.verbose = true;
        let out2 = draw(&mut st);
        assert!(!out2.contains("v to expand"));
    }

    #[test]
    fn checklist_detail_only_shows_for_selected_row_unless_verbose() {
        let mut d = base_detail(task());
        d.checklist = vec![
            crate::infrastructure::db::ChecklistItem {
                id: 1,
                text: "first step".into(),
                done: false,
                position: 0,
                intent: Some("do the first thing".into()),
                kind: "step".into(),
                source: "human".into(),
                verify_cmd: None,
                result: None,
                done_commit: None,
                done_at: None,
            },
            crate::infrastructure::db::ChecklistItem {
                id: 2,
                text: "second step".into(),
                done: false,
                position: 1,
                intent: Some("do the second thing".into()),
                kind: "step".into(),
                source: "human".into(),
                verify_cmd: None,
                result: None,
                done_commit: None,
                done_at: None,
            },
        ];
        let mut st = base_state(d);
        st.selected = 0; // metadata field, not a checklist row: nothing selected below
        let out = draw(&mut st);
        assert!(!out.contains("do the first thing"));
        assert!(!out.contains("do the second thing"));

        // Select the first checklist row explicitly.
        let idx = focusables(&st.detail, st.show_notes)
            .iter()
            .position(|f| matches!(f, Focusable::Checklist(0)))
            .unwrap();
        st.selected = idx;
        let out2 = draw(&mut st);
        assert!(out2.contains("do the first thing"));
        assert!(!out2.contains("do the second thing"));

        st.verbose = true;
        let out3 = draw(&mut st);
        assert!(out3.contains("do the first thing"));
        assert!(out3.contains("do the second thing"));
    }

    fn many_steps(n: i64) -> Vec<crate::infrastructure::db::ChecklistItem> {
        (0..n)
            .map(|i| crate::infrastructure::db::ChecklistItem {
                id: i + 1,
                text: format!("step number {i}"),
                done: false,
                position: i,
                intent: None,
                kind: "step".into(),
                source: "human".into(),
                verify_cmd: None,
                result: None,
                done_commit: None,
                done_at: None,
            })
            .collect()
    }

    #[test]
    fn selection_follow_scrolls_highlighted_row_into_view() {
        let mut d = base_detail(task());
        d.checklist = many_steps(40);
        let mut st = base_state(d);
        st.selected = focusables(&st.detail, st.show_notes)
            .iter()
            .position(|f| matches!(f, Focusable::Checklist(39)))
            .unwrap();

        // On a short terminal the last checklist row sits far below the fold;
        // navigating to it must pull the viewport down so the highlight
        // stays visible.
        let out = draw_at(&mut st, 100, 20);
        assert!(st.scroll > 0, "viewport should have scrolled down");
        assert!(out.contains("step number 39"));
    }

    #[test]
    fn manual_scroll_is_clamped_but_not_snapped_back() {
        let mut d = base_detail(task());
        d.checklist = many_steps(40);
        let mut st = base_state(d);
        // Selection unchanged since the last frame (no navigation) …
        st.last_selected = Some(st.selected);
        // … then a manual scroll far past the end of the content.
        st.scroll = 500;
        draw_at(&mut st, 100, 20);
        // Clamped to the end of the content, but NOT snapped back up to the
        // still-selected first row — free scrolling stays free.
        assert!(st.scroll < 500, "scroll should be clamped to content");
        assert!(st.scroll > 0, "scroll must not snap back to the selection");
    }
}
