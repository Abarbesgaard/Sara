use chrono::Local;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::commands::shared::short_id;
use crate::infrastructure::db;
use crate::infrastructure::tui::theme::{Ink, ink};

use super::super::lines::section;
use super::Body;
use crate::commands::info::handler::verification_rows;
use crate::commands::info::types::Focusable;

impl Body<'_> {
    pub(super) fn checklist(&mut self) {
        let d = self.d;
        let st = self.st;
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
            self.lines.push(Line::from(""));
            self.lines.push(section(&format!("Checklist  {progress}")));
            for (i, item) in d.checklist.iter().enumerate() {
                let is_sel = self.sel == Some(Focusable::Checklist(i));
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
                    self.sel_range = Some((self.lines.len(), self.lines.len()));
                }
                self.lines.push(Line::from(spans));
                let show_detail = is_sel || st.verbose;
                if show_detail {
                    if let Some(intent) = &item.intent {
                        self.lines.push(Line::from(Span::styled(
                            format!("         {intent}"),
                            Style::default().fg(ink(Ink::Muted)),
                        )));
                    }
                    if let Some(v) = &item.verify_cmd {
                        self.lines.push(Line::from(vec![
                            Span::styled(
                                "         verify ".to_string(),
                                Style::default().fg(ink(Ink::Muted)),
                            ),
                            Span::styled(v.clone(), Style::default().fg(ink(Ink::Info))),
                        ]));
                    }
                    if let Some(r) = &item.result {
                        self.lines.push(Line::from(vec![
                            Span::styled(
                                "         → ".to_string(),
                                Style::default().fg(ink(Ink::Ok)),
                            ),
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
                        self.lines.push(Line::from(Span::styled(
                            format!("         done {commit}{when}"),
                            Style::default().fg(ink(Ink::Muted)),
                        )));
                    }
                }
                for a in &fb {
                    let date = a.entry.with_timezone(&Local).format("%H:%M");
                    let flag = if a.request_revision { " ⟳" } else { "" };
                    self.lines.push(Line::from(vec![
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
                if is_sel && let Some((start, _)) = self.sel_range {
                    self.sel_range = Some((start, self.lines.len() - 1));
                }
            }
        }
    }

    pub(super) fn verification(&mut self) {
        let d = self.d;
        let verif = verification_rows(d);
        if !verif.is_empty() {
            self.lines.push(Line::from(""));
            self.lines
                .push(section("Verification  (run: sara verify <id> --run)"));
            for (scope, label, cmd) in &verif {
                self.lines.push(Line::from(vec![
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
    }
}
