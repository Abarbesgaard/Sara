use chrono::{Local, Utc};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::infrastructure::model::{Priority, Task, format_duration};
use crate::infrastructure::tui::theme::{Ink, ink};

use super::super::lines::{collapsed_text, field_line, key_span, section};
use super::Body;
use crate::commands::info::edit::current_value;
use crate::commands::info::handler::depends_on_display;
use crate::commands::info::types::{Detail, EDIT_FIELDS, EditField};

impl Body<'_> {
    pub(super) fn edit_rows(&mut self) {
        let d = self.d;
        let st = self.st;
        let t = &d.task;
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
            self.lines
                .push(editable_line(field.label(), &value, selected, *field, t));
            if selected {
                self.sel_range = Some((self.lines.len() - 1, self.lines.len() - 1));
            }
        }
    }

    pub(super) fn status_row(&mut self) {
        let d = self.d;
        let t = &d.task;
        if t.status != crate::infrastructure::model::Status::Pending {
            self.lines.push(field_line("Status", &t.status.to_string()));
        }
    }

    pub(super) fn time_row(&mut self) {
        let d = self.d;
        let t = &d.task;
        let active = t.is_active();
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
            self.lines.push(Line::from(vec![
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
    }

    pub(super) fn urgency_row(&mut self) {
        let d = self.d;
        let st = self.st;
        let t = &d.task;
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
            self.lines.push(Line::from(vec![
                key_span("Urgency"),
                Span::raw(format!("{:.1}", t.urgency)),
                Span::styled(
                    format!("{breakdown_str}{hint}"),
                    Style::default().fg(ink(Ink::Muted)),
                ),
            ]));
        }
    }

    pub(super) fn date_rows(&mut self) {
        let d = self.d;
        let t = &d.task;
        {
            let age_days = (Utc::now() - t.entry).num_days();
            let age_str = if age_days == 0 {
                "today".to_string()
            } else if age_days == 1 {
                "1 day ago".to_string()
            } else {
                format!("{age_days} days ago")
            };
            self.lines.push(Line::from(vec![
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
        self.lines.push(field_line(
            "Modified",
            &t.modified
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string(),
        ));
    }

    pub(super) fn anchor(&mut self) {
        let d = self.d;
        let st = self.st;
        if d.guide.assignment.is_none() && d.guide.rationale.is_none() {
            return;
        }
        self.lines.push(section("Anchor"));
        if let Some(a) = &d.guide.assignment {
            self.lines.push(Line::from(vec![
                key_span("Assignment"),
                Span::styled(
                    collapsed_text(a, st.verbose),
                    Style::default().fg(ink(Ink::Text)),
                ),
            ]));
        }
        if let Some(r) = &d.guide.rationale {
            self.lines.push(Line::from(vec![
                key_span("Why"),
                Span::styled(
                    collapsed_text(r, st.verbose),
                    Style::default().fg(ink(Ink::Soft)),
                ),
            ]));
        }
        self.lines.push(Line::from(""));
    }
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
