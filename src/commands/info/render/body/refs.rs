use chrono::Local;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::infrastructure::db;
use crate::infrastructure::tui::theme::{Ink, ink};

use super::super::lines::{feedback_mark, nav_line};
use super::Body;
use crate::commands::info::types::{Focusable, SectionId};

impl Body<'_> {
    pub(super) fn blockers(&mut self) {
        let d = self.d;
        if !self.show_panel {
            if !d.blocked_by.is_empty() {
                self.label("Blocked by");
                for b in &d.blocked_by {
                    self.lines.push(Line::from(format!("   ← {b}")));
                }
            }
            if !d.blocking.is_empty() {
                self.label("Blocking");
                for b in &d.blocking {
                    self.lines.push(Line::from(format!("   → {b}")));
                }
            }
        }
    }

    pub(super) fn ledger(&mut self) {
        let d = self.d;
        if d.ledger.is_empty() {
            return;
        }
        if !self.fold(SectionId::Memory, "Memory ledger") {
            return;
        }
        for (i, e) in d.ledger.iter().enumerate() {
            let selected = self.sel == Some(Focusable::Memory(i));
            let bg = if selected {
                ink(Ink::Select)
            } else {
                ink(Ink::Plain)
            };
            let prefix = if selected { " ▶ " } else { "   " };
            let mut spans = vec![
                Span::styled(prefix, Style::default().fg(ink(Ink::Text)).bg(bg)),
                Span::styled(
                    format!("{:<6}", e.handle),
                    Style::default()
                        .fg(ink(Ink::Special))
                        .bg(bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    e.snippet.clone(),
                    Style::default()
                        .fg(if selected {
                            ink(Ink::Text)
                        } else {
                            ink(Ink::Plain)
                        })
                        .bg(bg),
                ),
            ];
            for (n, label, color) in [
                (e.cited, "✓\u{a0}cited", ink(Ink::Ok)),
                (e.recalled, "↺\u{a0}recalled", ink(Ink::Info)),
                (e.surfaced, "·\u{a0}surfaced", ink(Ink::Muted)),
            ] {
                if n > 0 {
                    let count = if n > 1 {
                        format!("\u{a0}×{n}")
                    } else {
                        String::new()
                    };
                    spans.push(Span::styled(
                        format!("  {label}{count}"),
                        Style::default().fg(color),
                    ));
                }
            }
            if selected {
                self.sel_range = Some((self.lines.len(), self.lines.len()));
            }
            self.lines.push(Line::from(spans));
        }
    }

    pub(super) fn links(&mut self) {
        let d = self.d;
        if !d.links.is_empty() {
            if !self.fold(SectionId::Links, "Links") {
                return;
            }
            for (i, link) in d.links.iter().enumerate() {
                let selected = self.sel == Some(Focusable::Link(i));
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
                    Span::styled("↗ ", meta_style),
                    Span::styled(link.display(), style),
                ];
                if link.display() != link.url {
                    spans.push(Span::styled(
                        format!("  {}", link.url),
                        Style::default().fg(ink(Ink::Muted)).bg(bg),
                    ));
                }
                spans.push(Span::styled(
                    format!("  [{}]", link.id),
                    Style::default().fg(ink(Ink::Muted)).bg(bg),
                ));
                if selected {
                    self.sel_range = Some((self.lines.len(), self.lines.len()));
                }
                self.lines.push(Line::from(spans));
            }
        }
    }

    pub(super) fn files(&mut self) {
        let d = self.d;
        if !d.manual_files.is_empty() {
            if !self.fold(SectionId::Files, "Relevant files") {
                return;
            }
            for file in &d.manual_files {
                let selected = self.sel == Some(Focusable::File(file.to_string()));
                if selected {
                    self.sel_range = Some((self.lines.len(), self.lines.len()));
                }
                self.lines.push(nav_line(
                    &format!("▪ {file}"),
                    ink(Ink::Accent),
                    false,
                    selected,
                ));
            }
        }
    }

    pub(super) fn anchors(&mut self) {
        let d = self.d;
        if !d.anchors.is_empty() {
            if !self.fold(SectionId::Anchors, "Possible relevant files") {
                return;
            }
            for (ai, anchor) in d.anchors.iter().enumerate() {
                let is_sel = self.sel == Some(Focusable::Anchor(ai));
                let file_text = format!("◌ {}{}", anchor.path, anchor.location());
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
                spans.extend(feedback_mark(open_fb, needs_reconsider, row_bg));
                if is_sel {
                    self.sel_range = Some((self.lines.len(), self.lines.len()));
                }
                self.lines.push(Line::from(spans));

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
                    self.lines.push(Line::from(vec![
                        Span::styled("     ╰ ".to_string(), Style::default().fg(ink(Ink::Muted))),
                        Span::styled(
                            format!("{date}{flag}  "),
                            Style::default().fg(ink(Ink::Muted)),
                        ),
                        Span::styled(a.text.clone(), text_style),
                    ]));
                }
            }
        }
    }
}
