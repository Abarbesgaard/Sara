use chrono::Local;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::infrastructure::tui::theme::{Ink, ink};

use super::Body;
use crate::commands::info::types::{Focusable, SectionId};

impl Body<'_> {
    pub(super) fn ai_activity(&mut self) {
        let d = self.d;
        if d.ai_runs.is_empty() {
            return;
        }
        if !self.fold(SectionId::Ai, "AI activity") {
            return;
        }
        for r in &d.ai_runs {
            let date = r.created_at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
            let mut spans = vec![Span::styled(
                format!("   ◇ {:<8}", r.kind),
                Style::default().fg(ink(Ink::Text)),
            )];
            spans.push(Span::styled(
                format!(
                    "{} · {}  {date}",
                    r.model.as_deref().unwrap_or("?"),
                    r.provider.as_deref().unwrap_or("?"),
                ),
                Style::default().fg(ink(Ink::Muted)),
            ));
            if let Some(total) = run_tokens(r) {
                let split = match (r.prompt_tokens, r.completion_tokens) {
                    (Some(p), Some(c)) => {
                        format!(" ({} in / {} out)", thousands(p), thousands(c))
                    }
                    _ => String::new(),
                };
                spans.push(Span::styled(
                    format!("  · {} tok{split}", thousands(total)),
                    Style::default().fg(ink(Ink::Info)),
                ));
            }
            self.lines.push(Line::from(spans));
        }
        let total: i64 = d.ai_runs.iter().filter_map(run_tokens).sum();
        let runs = d.ai_runs.len();
        let mut summary = format!("   Σ {runs} run{}", if runs == 1 { "" } else { "s" });
        if total > 0 {
            summary.push_str(&format!(" · {} tokens", thousands(total)));
        }
        self.lines.push(Line::from(Span::styled(
            summary,
            Style::default()
                .fg(ink(Ink::Soft))
                .add_modifier(Modifier::BOLD),
        )));
    }

    pub(super) fn related_tasks(&mut self) {
        let d = self.d;
        if !d.similar.is_empty() {
            const RELATED_SHOWN: usize = 3;
            let mut similar: Vec<&(i64, String, f64)> = d.similar.iter().collect();
            similar.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
            if !self.fold(SectionId::Related, "Related tasks") {
                return;
            }
            for (id, desc, urg) in similar.iter().take(RELATED_SHOWN) {
                self.lines.push(Line::from(vec![
                    Span::styled(
                        format!("   #{id:<4} "),
                        Style::default().fg(ink(Ink::Muted)),
                    ),
                    Span::raw(desc.clone()),
                    Span::styled(
                        format!("  urg {urg:.1}"),
                        Style::default().fg(ink(Ink::Muted)),
                    ),
                ]));
            }
            if similar.len() > RELATED_SHOWN {
                self.lines.push(Line::from(Span::styled(
                    format!("   … {} more", similar.len() - RELATED_SHOWN),
                    Style::default()
                        .fg(ink(Ink::Muted))
                        .add_modifier(Modifier::ITALIC),
                )));
            }
        }
    }

    pub(super) fn comments(&mut self) {
        let d = self.d;
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
            if !self.fold(SectionId::Feedback, "Feedback") {
                return;
            }
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
                let is_sel = self.sel == Some(Focusable::Comment(ci));
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
                    self.sel_range = Some((self.lines.len(), self.lines.len()));
                }
                self.lines.push(Line::from(spans));
            }
        }
    }
}

fn run_tokens(r: &crate::infrastructure::db::AiRun) -> Option<i64> {
    r.total_tokens
        .or(match (r.prompt_tokens, r.completion_tokens) {
            (None, None) => None,
            (p, c) => Some(p.unwrap_or(0) + c.unwrap_or(0)),
        })
}

pub(super) fn thousands(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if n < 0 { format!("-{out}") } else { out }
}
