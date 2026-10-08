use chrono::Local;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::commands::shared::plural;
use crate::infrastructure::tui::theme::{Ink, ink};

use super::super::lines::{collapsed_text, feedback_mark};
use super::Body;
use crate::commands::info::handler::notes_of_kind;
use crate::commands::info::types::Focusable;

impl Body<'_> {
    pub(super) fn typed_notes(&mut self) {
        let d = self.d;
        let st = self.st;
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
            self.label(label);
            for n in &notes {
                let note_idx = note_cursor;
                note_cursor += 1;
                let is_sel = self.sel == Some(Focusable::Note(note_idx));
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
                spans.extend(feedback_mark(
                    note_fb.len(),
                    note_fb.iter().any(|a| a.request_revision),
                    row_bg,
                ));
                if is_sel {
                    self.sel_range = Some((self.lines.len(), self.lines.len()));
                }
                self.lines.push(Line::from(spans));

                for a in &note_fb {
                    let date = a.entry.with_timezone(&Local).format("%H:%M");
                    let flag = if a.request_revision { " ⟳" } else { "" };
                    self.lines.push(Line::from(vec![
                        Span::styled("     ╰ ".to_string(), Style::default().fg(ink(Ink::Muted))),
                        Span::styled(
                            format!("{date}{flag}  "),
                            Style::default().fg(ink(Ink::Muted)),
                        ),
                        Span::styled(a.text.clone(), Style::default().fg(ink(Ink::Muted))),
                    ]));
                }
            }
        }
        if !hidden_note_counts.is_empty() {
            let total: usize = hidden_note_counts.iter().map(|(_, c)| c).sum();
            let breakdown = hidden_note_counts
                .iter()
                .map(|(label, c)| format!("{c} {}", label.to_lowercase()))
                .collect::<Vec<_>>()
                .join(" · ");
            self.lines.push(Line::from(""));
            self.lines.push(Line::from(Span::styled(
                format!(
                    "   ⋯ {total} AI work note{} hidden ({breakdown})  · n to view",
                    plural(total)
                ),
                Style::default()
                    .fg(ink(Ink::Muted))
                    .add_modifier(Modifier::ITALIC),
            )));
        }
    }
}
