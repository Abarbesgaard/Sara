use crate::infrastructure::tui::theme::{Ink, heat, ink};
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
use crate::commands::shared::{month_abbr, plural, short_id, truncate};

pub(super) fn render(f: &mut Frame, st: &mut EditState) {
    let area = f.area();
    let d = &st.detail;

    let history_height: u16 = if d.history.is_empty() {
        0
    } else {
        (d.history.len() as u16 + 2).min(6)
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

    let show_panel = chunks[0].width >= 96;

    let mut lines: Vec<Line> = vec![];
    let mut sel_range: Option<(usize, usize)> = None;

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
                Style::default().fg(if active {
                    ink(Ink::Ok)
                } else {
                    ink(Ink::Plain)
                }),
            ),
            Span::styled(estimate_str, Style::default().fg(ink(Ink::Muted))),
        ]));
    }

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
                Style::default().fg(ink(Ink::Muted)),
            ),
        ]));
    }

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
                Style::default().fg(ink(Ink::Muted)),
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

    if let Some(a) = &d.guide.assignment {
        lines.push(Line::from(vec![
            key_span("Assignment"),
            Span::styled(
                collapsed_text(a, st.verbose),
                Style::default().fg(ink(Ink::Muted)),
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
                .fg(ink(Ink::Warn))
                .add_modifier(Modifier::BOLD),
        )]));
    } else if let Some(v) = &d.guide.validated_commit {
        lines.push(Line::from(vec![
            key_span("Freshness"),
            Span::styled(
                format!("validated @ {v}"),
                Style::default().fg(ink(Ink::Ok)),
            ),
        ]));
    }

    let items = focusables(d, st.show_notes);
    let sel: Option<Focusable> = if st.editing {
        None
    } else {
        items.get(st.selected).cloned()
    };
    let file_selected = |path: &str| sel == Some(Focusable::File(path.to_string()));

    let all_typed = typed_notes(d);
    if items.len() > EDIT_FIELDS.len() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  ↑/↓ select · Enter open/toggle · c comment · r reconsider · x resolve",
            Style::default()
                .fg(ink(Ink::Muted))
                .add_modifier(Modifier::ITALIC),
        )));
    }
    let mut note_cursor: usize = 0;
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
            let row_bg = if is_sel {
                ink(Ink::Select)
            } else {
                ink(Ink::Plain)
            };
            let row_fg = if is_sel {
                ink(Ink::Text)
            } else {
                ink(Ink::Plain)
            };

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
                        .fg(if is_sel {
                            ink(Ink::Text)
                        } else {
                            ink(Ink::Soft)
                        })
                        .bg(row_bg),
                ),
                Span::styled(
                    "• ".to_string(),
                    Style::default()
                        .fg(if is_sel {
                            ink(Ink::Text)
                        } else {
                            ink(Ink::Soft)
                        })
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
                        .fg(if is_sel {
                            ink(Ink::Text)
                        } else {
                            ink(Ink::Special)
                        })
                        .bg(row_bg),
                ));
            }
            if !note_fb.is_empty() {
                spans.push(Span::styled(
                    format!("  💬{}", note_fb.len()),
                    Style::default().fg(ink(Ink::Accent)).bg(row_bg),
                ));
            }
            if note_fb.iter().any(|a| a.request_revision) {
                spans.push(Span::styled(
                    " ⟳",
                    Style::default().fg(ink(Ink::Warn)).bg(row_bg),
                ));
            }
            if is_sel {
                sel_range = Some((lines.len(), lines.len()));
            }
            lines.push(Line::from(spans));

            for a in &note_fb {
                let date = a.entry.with_timezone(&Local).format("%H:%M");
                let flag = if a.request_revision { " ⟳" } else { "" };
                lines.push(Line::from(vec![
                    Span::styled("      ╰ ".to_string(), Style::default().fg(ink(Ink::Muted))),
                    Span::styled(
                        format!("{date}{flag}  "),
                        Style::default().fg(ink(Ink::Muted)),
                    ),
                    Span::styled(a.text.clone(), Style::default().fg(ink(Ink::Muted))),
                ]));
            }
        }
    }
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
                plural(total)
            ),
            Style::default()
                .fg(ink(Ink::Muted))
                .add_modifier(Modifier::ITALIC),
        )));
    }

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

    if !d.cited.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Cited memories"));
        for c in &d.cited {
            lines.push(Line::from(format!("  {c}")));
        }
    }

    if !d.links.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Links"));
        for (i, link) in d.links.iter().enumerate() {
            let selected = sel == Some(Focusable::Link(i));
            let (bg, fg) = if selected {
                (ink(Ink::Select), ink(Ink::Text))
            } else {
                (ink(Ink::Plain), ink(Ink::Accent))
            };
            let prefix = if selected { " ▶ " } else { "   " };
            let style = Style::default()
                .fg(fg)
                .bg(bg)
                .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
            let meta_style = Style::default()
                .fg(if selected {
                    ink(Ink::Text)
                } else {
                    ink(Ink::Soft)
                })
                .bg(bg);
            let mut spans = vec![
                Span::styled(prefix.to_string(), meta_style),
                Span::styled(format!("[{}] ", link.id), meta_style),
                Span::styled(link.display(), style),
            ];
            if link.display() != link.url {
                spans.push(Span::styled(
                    format!("  {}", link.url),
                    Style::default().fg(ink(Ink::Muted)).bg(bg),
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
            lines.push(nav_line(file, ink(Ink::Accent), false, selected));
        }
    }
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

            let row_bg = if is_sel {
                ink(Ink::Select)
            } else {
                ink(Ink::Plain)
            };
            let row_fg = if is_sel {
                ink(Ink::Text)
            } else {
                ink(Ink::Accent)
            };
            let meta_fg = if is_sel {
                ink(Ink::Text)
            } else {
                ink(Ink::Soft)
            };

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
                        .fg(if is_sel {
                            ink(Ink::Text)
                        } else {
                            ink(Ink::Special)
                        })
                        .bg(row_bg),
                ),
            ];
            if let Some(r) = &anchor.reason {
                spans.push(Span::styled(
                    format!("  — {r}"),
                    Style::default()
                        .fg(if is_sel {
                            ink(Ink::Text)
                        } else {
                            ink(Ink::Muted)
                        })
                        .bg(row_bg),
                ));
            }
            if open_fb > 0 {
                spans.push(Span::styled(
                    format!("  💬{open_fb}"),
                    Style::default().fg(ink(Ink::Accent)).bg(row_bg),
                ));
            }
            if needs_reconsider {
                spans.push(Span::styled(
                    " ⟳",
                    Style::default().fg(ink(Ink::Warn)).bg(row_bg),
                ));
            }
            if is_sel {
                sel_range = Some((lines.len(), lines.len()));
            }
            lines.push(Line::from(spans));

            for a in &anchor_fb {
                let date = a.entry.with_timezone(&Local).format("%H:%M");
                let resolved = a.status == "resolved";
                let text_style = if resolved {
                    Style::default()
                        .fg(ink(Ink::Muted))
                        .add_modifier(Modifier::CROSSED_OUT)
                } else {
                    Style::default().fg(ink(Ink::Muted))
                };
                let flag = if a.request_revision && !resolved {
                    " ⟳"
                } else {
                    ""
                };
                lines.push(Line::from(vec![
                    Span::styled("      ╰ ".to_string(), Style::default().fg(ink(Ink::Muted))),
                    Span::styled(
                        format!("{date}{flag}  "),
                        Style::default().fg(ink(Ink::Muted)),
                    ),
                    Span::styled(a.text.clone(), text_style),
                ]));
            }
        }
    }

    if !d.checklist.is_empty() {
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
            let row_bg = if is_sel {
                ink(Ink::Select)
            } else {
                ink(Ink::Plain)
            };

            let (box_str, text_style) = if item.done {
                (
                    "[x]",
                    Style::default()
                        .fg(ink(Ink::Muted))
                        .bg(row_bg)
                        .add_modifier(Modifier::CROSSED_OUT),
                )
            } else if is_sel {
                (
                    "[ ]",
                    Style::default()
                        .fg(ink(Ink::Text))
                        .bg(ink(Ink::Select))
                        .add_modifier(Modifier::BOLD),
                )
            } else {
                ("[ ]", Style::default())
            };
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
                .fg(if is_sel {
                    ink(Ink::Text)
                } else {
                    ink(Ink::Soft)
                })
                .bg(row_bg);
            let mut spans = vec![
                Span::styled(prefix.to_string(), box_style),
                Span::styled(format!("{box_str} "), box_style),
                Span::styled(item.text.clone(), text_style),
            ];
            if item.kind == db::STEP_KIND_ACCEPTANCE {
                spans.push(Span::styled(
                    " [accept]",
                    Style::default().fg(ink(Ink::Info)),
                ));
            }
            if item.source == "ai" {
                spans.push(Span::styled(
                    " (ai)",
                    Style::default().fg(ink(Ink::Special)),
                ));
            }
            if !fb.is_empty() {
                spans.push(Span::styled(
                    format!("  💬{}", fb.len()),
                    Style::default().fg(ink(Ink::Accent)),
                ));
            }
            if fb.iter().any(|a| a.request_revision) {
                spans.push(Span::styled(" ⟳", Style::default().fg(ink(Ink::Warn))));
            }
            if is_sel {
                sel_range = Some((lines.len(), lines.len()));
            }
            lines.push(Line::from(spans));
            let show_detail = is_sel || st.verbose;
            if show_detail {
                if let Some(intent) = &item.intent {
                    lines.push(Line::from(Span::styled(
                        format!("         {intent}"),
                        Style::default().fg(ink(Ink::Muted)),
                    )));
                }
                if let Some(v) = &item.verify_cmd {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "         verify ".to_string(),
                            Style::default().fg(ink(Ink::Muted)),
                        ),
                        Span::styled(v.clone(), Style::default().fg(ink(Ink::Info))),
                    ]));
                }
                if let Some(r) = &item.result {
                    lines.push(Line::from(vec![
                        Span::styled("         → ".to_string(), Style::default().fg(ink(Ink::Ok))),
                        Span::styled(r.clone(), Style::default().fg(ink(Ink::Ok))),
                    ]));
                }
                if item.done && (item.done_commit.is_some() || item.done_at.is_some()) {
                    let commit = item
                        .done_commit
                        .as_deref()
                        .map(|c| format!("@ {}", short_id(c)))
                        .unwrap_or_default();
                    let when = item
                        .done_at
                        .as_deref()
                        .map(|w| format!("  {w}"))
                        .unwrap_or_default();
                    lines.push(Line::from(Span::styled(
                        format!("         done {commit}{when}"),
                        Style::default().fg(ink(Ink::Muted)),
                    )));
                }
            }
            for a in &fb {
                let date = a.entry.with_timezone(&Local).format("%H:%M");
                let flag = if a.request_revision { " ⟳" } else { "" };
                lines.push(Line::from(vec![
                    Span::styled(
                        "         ╰ ".to_string(),
                        Style::default().fg(ink(Ink::Muted)),
                    ),
                    Span::styled(
                        format!("{date}{flag}  "),
                        Style::default().fg(ink(Ink::Muted)),
                    ),
                    Span::styled(a.text.clone(), Style::default().fg(ink(Ink::Muted))),
                ]));
            }
            if is_sel && let Some((start, _)) = sel_range {
                sel_range = Some((start, lines.len() - 1));
            }
        }
    }

    let verif = verification_rows(d);
    if !verif.is_empty() {
        lines.push(Line::from(""));
        lines.push(section("Verification  (run: sara verify <id> --run)"));
        for (scope, label, cmd) in &verif {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {label:<7}"),
                    Style::default()
                        .fg(ink(Ink::Soft))
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(cmd.clone(), Style::default().fg(ink(Ink::Info))),
                Span::styled(format!("  ({scope})"), Style::default().fg(ink(Ink::Muted))),
            ]));
        }
    }

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
                Style::default().fg(ink(Ink::Muted)),
            )));
        }
    }
    if !d.similar.is_empty() {
        const RELATED_SHOWN: usize = 3;
        let mut similar: Vec<&(i64, String, f64)> = d.similar.iter().collect();
        similar.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        lines.push(Line::from(""));
        lines.push(section("Related tasks (shared tags)"));
        for (id, desc, urg) in similar.iter().take(RELATED_SHOWN) {
            lines.push(Line::from(vec![
                Span::styled(format!("  #{id:<3} "), Style::default().fg(ink(Ink::Muted))),
                Span::raw(desc.clone()),
                Span::styled(
                    format!("  urg {urg:.1}"),
                    Style::default().fg(ink(Ink::Muted)),
                ),
            ]));
        }
        if similar.len() > RELATED_SHOWN {
            lines.push(Line::from(Span::styled(
                format!("  … {} more", similar.len() - RELATED_SHOWN),
                Style::default()
                    .fg(ink(Ink::Muted))
                    .add_modifier(Modifier::ITALIC),
            )));
        }
    }
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
        let id_map: std::collections::HashMap<i64, &crate::infrastructure::db::Annotation> =
            all_comments.iter().map(|a| (a.id, *a)).collect();
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
                    .fg(ink(Ink::Muted))
                    .add_modifier(Modifier::CROSSED_OUT)
            } else if is_sel {
                Style::default().fg(ink(Ink::Text)).bg(ink(Ink::Select))
            } else {
                Style::default()
            };
            let meta_style = if is_sel {
                Style::default().fg(ink(Ink::Text)).bg(ink(Ink::Select))
            } else {
                Style::default().fg(ink(Ink::Muted))
            };
            let mut spans = vec![
                Span::styled(if is_sel { " ▶ " } else { "   " }.to_string(), meta_style),
                Span::styled(format!("{date}  "), meta_style),
            ];
            if !target_label.is_empty() {
                spans.push(Span::styled(
                    target_label,
                    if is_sel {
                        Style::default().fg(ink(Ink::Text)).bg(ink(Ink::Select))
                    } else {
                        Style::default().fg(ink(Ink::Accent))
                    },
                ));
            }
            if a.request_revision && !resolved {
                spans.push(Span::styled("⟳ ", Style::default().fg(ink(Ink::Warn))));
            }
            spans.push(Span::styled(a.text.clone(), text_style));
            if is_sel {
                sel_range = Some((lines.len(), lines.len()));
            }
            lines.push(Line::from(spans));
        }
    }

    let (main_area, panel_area) = if show_panel {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(42), Constraint::Min(50)])
            .split(chunks[0]);
        (cols[1], Some(cols[0]))
    } else {
        (chunks[0], None)
    };

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
            if top < scroll {
                scroll = top;
            }
            st.scroll = scroll.min(u16::MAX as usize) as u16;
        }
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
                .border_style(Style::default().fg(ink(Ink::Accent))),
        )
        .wrap(Wrap { trim: false })
        .scroll((st.scroll, 0));
    f.render_widget(para, main_area);

    if let Some(panel) = panel_area {
        let tree_lines = task_tree_lines(d, st);
        let top_h: u16 = ((tree_lines.len() + 2) as u16).clamp(7, 24);

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
                .border_style(Style::default().fg(ink(Ink::Special))),
        );
        f.render_widget(tree_para, panel_chunks[0]);

        let git_lines = git_panel_lines(d);
        let git_para = Paragraph::new(git_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Git ")
                    .border_style(Style::default().fg(ink(Ink::Muted))),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(git_para, panel_chunks[1]);
    }

    if history_height > 0 {
        let hist_chunk = chunks[1];
        let hist_lines = history_lines(&d.history);
        let hist_para = Paragraph::new(hist_lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" History ")
                    .border_style(Style::default().fg(ink(Ink::Muted))),
            )
            .wrap(Wrap { trim: false });
        f.render_widget(hist_para, hist_chunk);
    }

    if st.adding_step {
        let edit_chunk_idx = if history_height > 0 { 2 } else { 1 };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Add step  (Enter save · Esc cancel) ".to_string())
            .border_style(Style::default().fg(ink(Ink::Ok)));
        let inner = block.inner(chunks[edit_chunk_idx]);
        f.render_widget(block, chunks[edit_chunk_idx]);
        f.render_widget(&st.editor, inner);
    }

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
            .border_style(Style::default().fg(ink(Ink::Warn)));
        let inner = block.inner(chunks[edit_chunk_idx]);
        f.render_widget(block, chunks[edit_chunk_idx]);
        f.render_widget(&st.editor, inner);
    }

    if st.editing {
        let edit_chunk_idx = if history_height > 0 { 2 } else { 1 };
        let field = EDIT_FIELDS
            .get(st.selected)
            .copied()
            .unwrap_or(EditField::Description);
        let (title, border) = if st.due_error {
            (
                format!(" Editing {} — invalid date ", field.label()),
                ink(Ink::Err),
            )
        } else if let Some(ref err) = st.dep_error {
            (
                format!(" Editing {} — {} ", field.label(), err),
                ink(Ink::Err),
            )
        } else if field == EditField::DependsOn {
            (
                format!(
                    " Editing {}  (task IDs, space/comma separated · Enter confirm · Esc cancel) ",
                    field.label()
                ),
                ink(Ink::Warn),
            )
        } else {
            (
                format!(" Editing {}  (Enter confirm · Esc cancel) ", field.label()),
                ink(Ink::Warn),
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
        Paragraph::new(footer).style(Style::default().fg(ink(Ink::Soft))),
        chunks[footer_idx],
    );
}

const TREE_PANEL_WIDTH: usize = 40;

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
        Style::default().fg(ink(Ink::Muted)),
    )));
    lines.push(current_task_tree_line(&d.task));
    lines.push(Line::from(Span::styled(
        rule,
        Style::default().fg(ink(Ink::Muted)),
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
            Style::default().fg(ink(Ink::Muted)),
        )));
        return;
    }
    push_tree_node_lines(lines, nodes, hidden, "  ", 1, max_depth, max_children);
}

fn tree_section_header(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {text}"),
        Style::default()
            .fg(ink(Ink::Soft))
            .add_modifier(Modifier::BOLD),
    ))
}

fn current_task_tree_line(task: &Task) -> Line<'static> {
    let id_str = task
        .id
        .map(|n| format!("{n:>3}"))
        .unwrap_or_else(|| "  -".to_string());
    let style = Style::default()
        .fg(ink(Ink::Text))
        .bg(ink(Ink::Select))
        .add_modifier(Modifier::BOLD);
    Line::from(vec![
        Span::styled(" ▶ ", style),
        Span::styled(format!("{id_str} "), style),
        Span::styled(truncate(&task.description, 30), style),
    ])
}

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
                        .fg(ink(Ink::Muted))
                        .add_modifier(Modifier::ITALIC),
                )));
            }
        }
    }
    if overflow > 0 {
        lines.push(Line::from(Span::styled(
            format!("{prefix}└─ +{overflow} more  (d to expand)"),
            Style::default()
                .fg(ink(Ink::Muted))
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
            .fg(ink(Ink::Muted))
            .add_modifier(Modifier::CROSSED_OUT)
    } else {
        Style::default().fg(ink(Ink::Accent))
    };
    let desc_budget = TREE_PANEL_WIDTH
        .saturating_sub(prefix.chars().count() + connector.chars().count() + id_str.len() + 4);
    let mut spans = vec![
        Span::styled(
            format!("{prefix}{connector}"),
            Style::default().fg(ink(Ink::Muted)),
        ),
        Span::styled(format!("{glyph} "), style),
        Span::styled(format!("{id_str} "), style),
        Span::styled(truncate(&node.description, desc_budget.max(6)), style),
    ];
    if let Some(label) = link_badge_label(node.badge.as_ref()) {
        spans.push(Span::styled(
            format!(" {label}"),
            Style::default().fg(ink(Ink::Warn)),
        ));
    }
    Line::from(spans)
}

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

#[allow(dead_code)]
fn render_project_stats(f: &mut Frame, area: ratatui::layout::Rect, d: &Detail) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Project ")
        .border_style(Style::default().fg(ink(Ink::Muted)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let Some(ref s) = d.stats else {
        return;
    };

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

    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<10}", "Pending"),
            Style::default().fg(ink(Ink::Soft)),
        ),
        Span::raw(format!("{:>3}", s.pending)),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<10}", "Active"),
            Style::default().fg(ink(Ink::Soft)),
        ),
        Span::styled(
            format!("{:>3}", s.active),
            Style::default().fg(if s.active > 0 {
                ink(Ink::Ok)
            } else {
                ink(Ink::Plain)
            }),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<10}", "Done"),
            Style::default().fg(ink(Ink::Soft)),
        ),
        Span::raw(format!("{:>3}", s.completed_total)),
        Span::styled(
            format!("  {}", completion_rate),
            Style::default().fg(ink(Ink::Muted)),
        ),
    ]));

    lines.push(Line::from(Span::styled(
        "  ─────────────",
        Style::default().fg(ink(Ink::Muted)),
    )));

    let pri_total = s.pending.max(1);
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<5}", "H"),
            Style::default()
                .fg(ink(Ink::Err))
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{:<bar_w$}", bar(s.high, pri_total, bar_w)),
            Style::default().fg(ink(Ink::Err)),
        ),
        Span::styled(format!(" {}", s.high), Style::default().fg(ink(Ink::Muted))),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<5}", "M"), Style::default().fg(ink(Ink::Warn))),
        Span::styled(
            format!("{:<bar_w$}", bar(s.medium, pri_total, bar_w)),
            Style::default().fg(ink(Ink::Warn)),
        ),
        Span::styled(
            format!(" {}", s.medium),
            Style::default().fg(ink(Ink::Muted)),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::styled(format!("  {:<5}", "L"), Style::default().fg(ink(Ink::Ok))),
        Span::styled(
            format!("{:<bar_w$}", bar(s.low, pri_total, bar_w)),
            Style::default().fg(ink(Ink::Ok)),
        ),
        Span::styled(format!(" {}", s.low), Style::default().fg(ink(Ink::Muted))),
    ]));
    lines.push(Line::from(vec![
        Span::styled(
            format!("  {:<5}", "—"),
            Style::default().fg(ink(Ink::Muted)),
        ),
        Span::styled(
            format!("{:<bar_w$}", bar(s.no_pri, pri_total, bar_w)),
            Style::default().fg(ink(Ink::Muted)),
        ),
        Span::styled(
            format!(" {}", s.no_pri),
            Style::default().fg(ink(Ink::Muted)),
        ),
    ]));

    lines.push(Line::from(Span::styled(
        "  ─────────────",
        Style::default().fg(ink(Ink::Muted)),
    )));

    if s.overdue > 0 {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<10}", "Overdue"),
                Style::default().fg(ink(Ink::Err)),
            ),
            Span::styled(
                format!("{:>3}", s.overdue),
                Style::default()
                    .fg(ink(Ink::Err))
                    .add_modifier(Modifier::BOLD),
            ),
        ]));
    }
    if s.due_today > 0 {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<10}", "Today"),
                Style::default().fg(ink(Ink::Warn)),
            ),
            Span::styled(
                format!("{:>3}", s.due_today),
                Style::default().fg(ink(Ink::Warn)),
            ),
        ]));
    }
    let due_later = s.due_week.saturating_sub(s.due_today);
    if due_later > 0 {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:<10}", "This week"),
                Style::default().fg(ink(Ink::Soft)),
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
        .border_style(Style::default().fg(ink(Ink::Muted)));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let max = counts.values().copied().max().unwrap_or(1).max(1);
    let today = Local::now().date_naive();

    let days_since_sunday = today.weekday().num_days_from_sunday();
    let grid_end = today - Duration::days(days_since_sunday as i64);

    let cell_w: u16 = 3;
    let label_w: u16 = 4;
    let num_weeks = ((inner.width.saturating_sub(label_w)) / cell_w).clamp(4, 16) as i64;
    let grid_start = grid_end - Duration::weeks(num_weeks) + Duration::days(1);

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
                    Style::default().fg(ink(Ink::Muted)),
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
            Style::default().fg(ink(Ink::Muted)),
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
                ink(Ink::Void)
            } else {
                heat(count, max)
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
                Style::default().fg(ink(Ink::Muted)),
            ))),
            stats_area,
        );
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
            Span::styled(format!("  {date}  "), Style::default().fg(ink(Ink::Muted))),
            Span::styled(
                format!("{:<11} ", label),
                Style::default().fg(ink(Ink::Accent)),
            ),
        ];
        let additive = matches!(
            h.field.as_str(),
            "annotation" | "link" | "dependency" | "checklist" | "file"
        ) && h.old_value.is_none() != h.new_value.is_none();
        if h.field == "created" {
            spans.push(Span::raw(h.new_value.clone().unwrap_or_default()));
        } else if additive {
            if let Some(text) = &h.new_value {
                spans.push(Span::styled("+ ", Style::default().fg(ink(Ink::Ok))));
                spans.push(Span::raw(text.clone()));
            } else if let Some(text) = &h.old_value {
                spans.push(Span::styled("− ", Style::default().fg(ink(Ink::Err))));
                spans.push(Span::raw(text.clone()));
            }
        } else {
            spans.push(Span::styled(
                h.old_value.clone().unwrap_or_else(|| "—".into()),
                Style::default().fg(ink(Ink::Soft)),
            ));
            spans.push(Span::styled(" → ", Style::default().fg(ink(Ink::Muted))));
            spans.push(Span::raw(h.new_value.clone().unwrap_or_else(|| "—".into())));
        }
        lines.push(Line::from(spans));
    }
    lines
}

fn git_panel_lines(d: &Detail) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = vec![];

    let Some(rec) = &d.branch else {
        lines.push(Line::from(Span::styled(
            "  No branch tied.",
            Style::default().fg(ink(Ink::Muted)),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Run: sara <id> addbranch",
            Style::default().fg(ink(Ink::Soft)),
        )));
        return lines;
    };

    lines.push(Line::from(vec![
        Span::styled("  Branch  ", Style::default().fg(ink(Ink::Muted))),
        Span::styled(
            rec.branch.clone(),
            Style::default()
                .fg(ink(Ink::Accent))
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    lines
}

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

fn nav_line<'a>(text: &str, color: Color, italic: bool, selected: bool) -> Line<'a> {
    let mut style = Style::default().fg(color);
    if italic {
        style = style.add_modifier(Modifier::ITALIC);
    }
    if selected {
        style = style
            .bg(ink(Ink::Select))
            .fg(ink(Ink::Text))
            .add_modifier(Modifier::BOLD);
    }
    let prefix = if selected { " ▶ " } else { "   " };
    Line::from(vec![
        Span::styled(prefix.to_string(), style),
        Span::styled(text.to_string(), style),
    ])
}

#[allow(dead_code)]
fn sel_line<'a>(spans: Vec<Span<'a>>, selected: bool) -> Line<'a> {
    if !selected {
        return Line::from(spans);
    }
    let highlighted: Vec<Span> = spans
        .into_iter()
        .map(|s| Span::styled(s.content, s.style.bg(ink(Ink::Select)).fg(ink(Ink::Text))))
        .collect();
    Line::from(highlighted)
}

fn editable_line<'a>(k: &str, v: &str, selected: bool, field: EditField, task: &Task) -> Line<'a> {
    let (bg, fg) = if selected {
        (ink(Ink::Select), ink(Ink::Text))
    } else {
        (ink(Ink::Plain), ink(Ink::Muted))
    };
    let key_style = if selected {
        Style::default().fg(fg).bg(bg).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(fg)
    };

    let value_span = if field == EditField::Priority {
        match &task.priority {
            Some(Priority::H) => Span::styled("High", Style::default().fg(ink(Ink::Err))),
            Some(Priority::M) => Span::styled("Medium", Style::default().fg(ink(Ink::Warn))),
            Some(Priority::L) => Span::styled("Low", Style::default().fg(ink(Ink::Ok))),
            None => Span::styled("-", Style::default().fg(ink(Ink::Soft))),
        }
    } else if field == EditField::Due {
        due_value_span(task, v)
    } else {
        Span::raw(v.to_string())
    };

    let prefix = if selected { " ▶ " } else { "   " };
    let value_style = if selected {
        Style::default().fg(ink(Ink::Text)).bg(ink(Ink::Select))
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
            ink(Ink::Err)
        } else if days <= 1 {
            ink(Ink::Warn)
        } else {
            ink(Ink::Plain)
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
        Span::styled(fallback.to_string(), Style::default().fg(ink(Ink::Soft)))
    }
}

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

fn collapsed_text(s: &str, verbose: bool) -> String {
    const COLLAPSED_CHARS: usize = 160;
    if verbose || s.chars().count() <= COLLAPSED_CHARS {
        return s.to_string();
    }
    let t: String = s.chars().take(COLLAPSED_CHARS).collect();
    format!("{t}…  (v to expand)")
}

fn key_span(k: &str) -> Span<'static> {
    Span::styled(format!("  {:<12}", k), Style::default().fg(ink(Ink::Muted)))
}

fn field_line<'a>(k: &str, v: &str) -> Line<'a> {
    Line::from(vec![key_span(k), Span::raw(v.to_string())])
}

fn section(k: &str) -> Line<'static> {
    Line::from(Span::styled(
        k.to_string(),
        Style::default()
            .add_modifier(Modifier::BOLD)
            .fg(ink(Ink::Accent)),
    ))
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/info/render.rs"]
mod tests;
