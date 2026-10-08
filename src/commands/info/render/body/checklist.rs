use chrono::Local;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::commands::shared::short_id;
use crate::infrastructure::db;
use crate::infrastructure::tui::theme::{Ink, ink};

use super::Body;
use crate::commands::info::handler::verification_rows;
use crate::commands::info::types::{Focusable, SectionId};

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
            let progress =
                Span::styled(format!("  {progress}"), Style::default().fg(ink(Ink::Soft)));
            if !self.fold(SectionId::Steps, "Checklist", vec![progress]) {
                return;
            }
            if acc_total > 0 {
                self.lines
                    .push(progress_bar("acceptance", acc_done, acc_total));
            }
            let current = d
                .checklist
                .iter()
                .position(|it| it.kind != db::STEP_KIND_ACCEPTANCE && !it.done);
            for (i, item) in d.checklist.iter().enumerate() {
                let is_sel = self.sel == Some(Focusable::Checklist(i));
                let is_current = current == Some(i);
                let is_acc = item.kind == db::STEP_KIND_ACCEPTANCE;
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
                } else if is_current {
                    (
                        "[ ]",
                        Style::default()
                            .fg(ink(Ink::Accent))
                            .add_modifier(Modifier::BOLD),
                    )
                } else {
                    ("[ ]", Style::default())
                };
                let target_k = if is_acc { "acceptance" } else { "step" };
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
                let prefix = if is_sel {
                    " ▶ "
                } else if is_current {
                    " ◆ "
                } else {
                    "   "
                };
                let box_style = Style::default()
                    .fg(if is_sel {
                        ink(Ink::Text)
                    } else if is_current {
                        ink(Ink::Accent)
                    } else {
                        ink(Ink::Soft)
                    })
                    .bg(row_bg);
                let mut spans = vec![
                    Span::styled(prefix.to_string(), box_style),
                    Span::styled(format!("{box_str} "), box_style),
                    Span::styled(item.text.clone(), text_style),
                ];
                if is_acc {
                    spans.push(Span::styled(
                        " [accept]",
                        Style::default().fg(ink(Ink::Info)),
                    ));
                }
                if is_current {
                    spans.push(Span::styled(
                        "  ← next",
                        Style::default().fg(ink(Ink::Accent)),
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
                if (show_detail || is_current)
                    && let Some(intent) = &item.intent
                {
                    self.lines.push(Line::from(Span::styled(
                        format!("         {intent}"),
                        Style::default().fg(ink(Ink::Muted)),
                    )));
                }
                if (show_detail || is_acc)
                    && let Some(row) = verify_row(item)
                {
                    self.lines.push(row);
                }
                if show_detail
                    && item.done
                    && (item.done_commit.is_some() || item.done_at.is_some())
                {
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
            let hint = Span::styled(
                "  run: sara verify <id> --run",
                Style::default().fg(ink(Ink::Muted)),
            );
            if !self.fold(SectionId::Verification, "Verification", vec![hint]) {
                return;
            }
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

fn verify_row(item: &db::ChecklistItem) -> Option<Line<'static>> {
    let muted = Style::default().fg(ink(Ink::Muted));
    let mut spans = vec![Span::styled("         ".to_string(), muted)];
    if let Some(v) = &item.verify_cmd {
        spans.push(Span::styled("verify ".to_string(), muted));
        spans.push(Span::styled(v.clone(), Style::default().fg(ink(Ink::Info))));
        spans.push(Span::raw("  "));
    }
    match &item.result {
        Some(r) => spans.push(Span::styled(
            format!("→ {r}"),
            Style::default().fg(ink(Ink::Ok)),
        )),
        None if item.verify_cmd.is_some() => {
            spans.push(Span::styled("→ not run".to_string(), muted))
        }
        None => return None,
    }
    Some(Line::from(spans))
}

pub(super) fn progress_bar(label: &str, done: i32, total: i32) -> Line<'static> {
    const WIDTH: i32 = 20;
    let filled = if total > 0 { done * WIDTH / total } else { 0 };
    Line::from(vec![
        Span::styled(
            format!("  {label:<12}"),
            Style::default().fg(ink(Ink::Muted)),
        ),
        Span::styled(
            "█".repeat(filled as usize),
            Style::default().fg(ink(Ink::Ok)),
        ),
        Span::styled(
            "░".repeat((WIDTH - filled) as usize),
            Style::default().fg(ink(Ink::Muted)),
        ),
        Span::styled(
            format!(" {done}/{total}"),
            Style::default().fg(ink(Ink::Soft)),
        ),
    ])
}
