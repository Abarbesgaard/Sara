use chrono::{Local, Utc};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::infrastructure::model::{Priority, Status, Task, format_duration};
use crate::infrastructure::tui::theme::{Ink, ink};

use super::super::lines::collapsed_text;
use super::Body;
use crate::commands::info::edit::current_value;
use crate::commands::info::handler::depends_on_display;
use crate::commands::info::types::{Detail, EDIT_FIELDS, EditField};

const LABEL_W: usize = 11;
const GRID_MIN_WIDTH: usize = 76;

impl Body<'_> {
    pub(super) fn hero(&mut self) {
        let st = self.st;
        let t = &self.d.task;
        let selected = !st.editing && st.selected == 0;
        let title = if st.editing && st.selected == 0 {
            "…editing below".to_string()
        } else {
            collapsed_text(&t.description, st.verbose || selected)
        };
        let mut style = Style::default()
            .fg(ink(Ink::Text))
            .add_modifier(Modifier::BOLD);
        if selected {
            style = style.bg(ink(Ink::Select));
        }
        self.lines.push(Line::from(""));
        if selected {
            self.sel_range = Some((self.lines.len(), self.lines.len()));
        }
        self.lines.push(Line::from(vec![
            Span::styled(if selected { " ▶ " } else { "   " }, style),
            Span::styled(title, style),
        ]));
        let meta = self.meta_lines();
        self.lines.extend(meta);
    }

    fn meta_lines(&self) -> Vec<Line<'static>> {
        let d = self.d;
        let t = &d.task;
        let muted = Style::default().fg(ink(Ink::Muted));
        let soft = Style::default().fg(ink(Ink::Soft));
        let mut parts: Vec<Vec<Span<'static>>> = vec![];
        if t.status != Status::Pending {
            let color = match t.status {
                Status::Completed => ink(Ink::Ok),
                Status::Deleted => ink(Ink::Err),
                Status::Pending => ink(Ink::Text),
            };
            parts.push(vec![
                Span::styled("status ", muted),
                Span::styled(t.status.to_string(), Style::default().fg(color)),
            ]);
        }
        let active = t.is_active();
        if active || t.time_spent > 0 || t.estimate_mins.is_some() {
            let mut spans = vec![];
            if active {
                spans.push(Span::styled(
                    format!(
                        "● running {} (session {})",
                        format_duration(t.total_time_spent()),
                        format_duration(t.total_time_spent() - t.time_spent)
                    ),
                    Style::default().fg(ink(Ink::Ok)),
                ));
            } else {
                spans.push(Span::styled("spent ", muted));
                spans.push(Span::styled(
                    if t.time_spent > 0 {
                        format_duration(t.time_spent)
                    } else {
                        "—".into()
                    },
                    soft,
                ));
            }
            if let Some(m) = t.estimate_mins {
                let spent = t.total_time_spent() / 60;
                let pct = if m > 0 { (spent * 100 / m).min(999) } else { 0 };
                spans.push(Span::styled(
                    format!(" of {} ({pct}%)", estimate_str(m)),
                    muted,
                ));
            }
            parts.push(spans);
        }
        let mut urgency = vec![
            Span::styled("urgency ", muted),
            Span::styled(format!("{:.1}", t.urgency), soft),
        ];
        if self.st.show_urgency_breakdown {
            urgency.push(Span::styled(urgency_breakdown_str(d), muted));
        }
        parts.push(urgency);
        let age_days = (Utc::now() - t.entry).num_days();
        let age = match age_days {
            0 => "today".to_string(),
            1 => "1 day ago".to_string(),
            n => format!("{n} days ago"),
        };
        parts.push(vec![Span::styled(
            format!(
                "created {} ({age})",
                t.entry.with_timezone(&Local).format("%Y-%m-%d %H:%M")
            ),
            muted,
        )]);
        parts.push(vec![Span::styled(
            format!(
                "modified {}",
                t.modified.with_timezone(&Local).format("%Y-%m-%d %H:%M")
            ),
            muted,
        )]);
        let span_w = |spans: &[Span<'static>]| spans.iter().map(|s| s.width()).sum::<usize>();
        let mut lines = vec![];
        let mut spans = vec![Span::raw("   ")];
        for p in parts {
            let used = span_w(&spans);
            if used > 3 && used + 5 + span_w(&p) > self.width {
                lines.push(Line::from(std::mem::replace(
                    &mut spans,
                    vec![Span::raw("   ")],
                )));
            } else if used > 3 {
                spans.push(Span::styled("  ·  ", muted));
            }
            spans.extend(p);
        }
        lines.push(Line::from(spans));
        lines
    }

    pub(super) fn anchor(&mut self) {
        let d = self.d;
        let st = self.st;
        if d.guide.assignment.is_none() && d.guide.rationale.is_none() {
            return;
        }
        self.lines.push(Line::from(""));
        let bar = Span::styled("   ▎ ", Style::default().fg(ink(Ink::Accent)));
        for (label, value, color) in [
            ("asked", &d.guide.assignment, Ink::Text),
            ("why", &d.guide.rationale, Ink::Soft),
        ] {
            if let Some(v) = value {
                self.lines.push(Line::from(vec![
                    bar.clone(),
                    Span::styled(format!("{label:<7}"), Style::default().fg(ink(Ink::Muted))),
                    Span::styled(
                        collapsed_text(v, st.verbose),
                        Style::default().fg(ink(color)),
                    ),
                ]));
            }
        }
    }

    pub(super) fn details(&mut self) {
        self.label("Details");
        let cells: Vec<(Vec<Span<'static>>, bool)> = EDIT_FIELDS
            .iter()
            .enumerate()
            .skip(1)
            .map(|(i, f)| self.field_cell(i, *f))
            .collect();
        let width = self.width;
        let col = width / 2;
        let grid = width >= GRID_MIN_WIDTH;
        let mut it = cells.into_iter().peekable();
        while let Some((left, left_sel)) = it.next() {
            let lw: usize = left.iter().map(|s| s.width()).sum();
            let pair = grid
                && lw < col
                && it.peek().is_some_and(|(r, _)| {
                    r.iter().map(|s| s.width()).sum::<usize>() <= width - col
                });
            if pair {
                let (right, right_sel) = it.next().unwrap_or_default();
                if left_sel || right_sel {
                    self.sel_range = Some((self.lines.len(), self.lines.len()));
                }
                let mut spans = left;
                spans.push(Span::raw(" ".repeat(col - lw)));
                spans.extend(right);
                self.lines.push(Line::from(spans));
            } else {
                if left_sel {
                    self.sel_range = Some((self.lines.len(), self.lines.len()));
                }
                self.lines.push(Line::from(left));
            }
        }
    }

    fn field_cell(&self, i: usize, field: EditField) -> (Vec<Span<'static>>, bool) {
        let st = self.st;
        let t = &self.d.task;
        let selected = !st.editing && i == st.selected;
        let bg = if selected {
            ink(Ink::Select)
        } else {
            ink(Ink::Plain)
        };
        let key_style = if selected {
            Style::default()
                .fg(ink(Ink::Text))
                .bg(bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(ink(Ink::Muted))
        };
        let dash = || vec![Span::styled("—", Style::default().fg(ink(Ink::Muted)))];
        let value: Vec<Span<'static>> = if st.editing && i == st.selected {
            vec![Span::styled(
                "…editing below",
                Style::default().fg(ink(Ink::Warn)),
            )]
        } else {
            match field {
                EditField::Priority => match &t.priority {
                    Some(Priority::H) => {
                        vec![Span::styled("▲ High", Style::default().fg(ink(Ink::Err)))]
                    }
                    Some(Priority::M) => {
                        vec![Span::styled(
                            "■ Medium",
                            Style::default().fg(ink(Ink::Warn)),
                        )]
                    }
                    Some(Priority::L) => {
                        vec![Span::styled("▼ Low", Style::default().fg(ink(Ink::Ok)))]
                    }
                    None => dash(),
                },
                EditField::Due if t.due.is_some() => vec![due_value_span(t, "")],
                EditField::Tags if !t.tags.is_empty() => {
                    let mut v = vec![];
                    for (n, tag) in t.tags.iter().enumerate() {
                        if n > 0 {
                            v.push(Span::raw(" "));
                        }
                        v.push(Span::styled(
                            format!("#{tag}"),
                            Style::default().fg(ink(Ink::Special)),
                        ));
                    }
                    v
                }
                EditField::DependsOn => {
                    let v = depends_on_display(self.d);
                    if v.is_empty() {
                        dash()
                    } else {
                        vec![Span::raw(v)]
                    }
                }
                _ => {
                    let v = current_value(t, field);
                    if v.is_empty() {
                        dash()
                    } else {
                        vec![Span::raw(v)]
                    }
                }
            }
        };
        let mut spans = vec![
            Span::styled(if selected { " ▶ " } else { "   " }, key_style),
            Span::styled(format!("{:<LABEL_W$}", field.label()), key_style),
        ];
        spans.extend(value.into_iter().map(|s| {
            if selected {
                s.patch_style(Style::default().fg(ink(Ink::Text)).bg(bg))
            } else {
                s
            }
        }));
        (spans, selected)
    }
}

fn estimate_str(m: i64) -> String {
    if m >= 60 {
        let (h, r) = (m / 60, m % 60);
        if r == 0 {
            format!("{h}h")
        } else {
            format!("{h}h{r}m")
        }
    } else {
        format!("{m}m")
    }
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
