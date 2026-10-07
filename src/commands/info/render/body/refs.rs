use chrono::Local;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::infrastructure::db;
use crate::infrastructure::tui::theme::{Ink, ink};

use super::super::lines::{nav_line, section};
use super::Body;
use crate::commands::info::types::Focusable;

impl Body<'_> {
    pub(super) fn blockers(&mut self) {
        let d = self.d;
        if !self.show_panel {
            if !d.blocked_by.is_empty() {
                self.lines.push(Line::from(""));
                self.lines.push(section("Blocked by"));
                for b in &d.blocked_by {
                    self.lines.push(Line::from(format!("  {b}")));
                }
            }
            if !d.blocking.is_empty() {
                self.lines.push(Line::from(""));
                self.lines.push(section("Blocking"));
                for b in &d.blocking {
                    self.lines.push(Line::from(format!("  {b}")));
                }
            }
        }
    }

    pub(super) fn cited(&mut self) {
        let d = self.d;
        if !d.cited.is_empty() {
            self.lines.push(Line::from(""));
            self.lines.push(section("Cited memories"));
            for c in &d.cited {
                self.lines.push(Line::from(format!("  {c}")));
            }
        }
    }

    pub(super) fn links(&mut self) {
        let d = self.d;
        if !d.links.is_empty() {
            self.lines.push(Line::from(""));
            self.lines.push(section("Links"));
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
                    self.sel_range = Some((self.lines.len(), self.lines.len()));
                }
                self.lines.push(Line::from(spans));
            }
        }
    }

    pub(super) fn files(&mut self) {
        let d = self.d;
        if !d.manual_files.is_empty() {
            self.lines.push(Line::from(""));
            self.lines.push(section("Relevant files"));
            for file in &d.manual_files {
                let selected = self.sel == Some(Focusable::File(file.to_string()));
                if selected {
                    self.sel_range = Some((self.lines.len(), self.lines.len()));
                }
                self.lines
                    .push(nav_line(file, ink(Ink::Accent), false, selected));
            }
        }
    }

    pub(super) fn anchors(&mut self) {
        let d = self.d;
        if !d.anchors.is_empty() {
            self.lines.push(Line::from(""));
            self.lines.push(section("Possible relevant files"));
            for (ai, anchor) in d.anchors.iter().enumerate() {
                let is_sel = self.sel == Some(Focusable::Anchor(ai));
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
    }
}
