use chrono::Local;

use crate::infrastructure::db;

use super::handler::{guide_is_stale, notes_of_kind, verification_rows};
use super::types::Detail;
use crate::commands::shared::annotation_target;

#[derive(Clone, Copy, Default)]
pub(super) struct RenderOpts {
    pub(super) history: bool,
}

fn terminal_width() -> usize {
    crossterm::terminal::size()
        .map(|(cols, _)| cols as usize)
        .unwrap_or(96)
        .clamp(60, 100)
}

fn wrap_body(text: &str, indent: &str, width: usize) -> Vec<String> {
    let avail = width.saturating_sub(indent.len()).max(20);
    let mut lines = Vec::new();
    for para in text.split('\n') {
        if para.trim().is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut current = String::new();
        for word in para.split_whitespace() {
            let extra = if current.is_empty() { 0 } else { 1 };
            if !current.is_empty() && current.len() + extra + word.len() > avail {
                lines.push(format!("{indent}{current}"));
                current.clear();
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(word);
        }
        if !current.is_empty() {
            lines.push(format!("{indent}{current}"));
        }
    }
    lines
}

pub(super) fn render_plain(d: &Detail, opts: RenderOpts) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    macro_rules! w {
        () => {{ let _ = writeln!(out); }};
        ($($arg:tt)*) => {{ let _ = writeln!(out, $($arg)*); }};
    }
    let t = &d.task;
    w!("Task {}", t.id.unwrap_or(0));
    w!();
    w!("{:<14}{}", "Description", t.description);
    w!("{:<14}{}", "Project", t.project);
    w!("{:<14}{}", "Status", t.status);
    w!(
        "{:<14}{}",
        "Priority",
        t.priority.as_ref().map(|p| p.label()).unwrap_or("-")
    );
    w!(
        "{:<14}{}",
        "Due",
        t.due
            .map(|dd| dd
                .with_timezone(&Local)
                .format("%Y-%m-%d %H:%M")
                .to_string())
            .unwrap_or_else(|| "-".to_string())
    );
    w!(
        "{:<14}{}",
        "Tags",
        if t.tags.is_empty() {
            "-".to_string()
        } else {
            t.tags.join(", ")
        }
    );
    w!(
        "{:<14}{}",
        "Time spent",
        crate::infrastructure::model::format_duration(t.total_time_spent())
    );
    w!("{:<14}{:.1}", "Urgency", t.urgency);
    w!("{:<14}{}", "UUID", t.uuid);

    if let Some(a) = &d.guide.assignment {
        w!("{:<14}{}", "Assignment", a);
    }
    if let Some(r) = &d.guide.rationale {
        w!("{:<14}{}", "Rationale", r);
    }
    if guide_is_stale(d) {
        w!(
            "{:<14}guide validated @ {} but HEAD is {} — may be stale (run `sara validate`)",
            "Freshness",
            d.guide.validated_commit.as_deref().unwrap_or("-"),
            d.head_commit.as_deref().unwrap_or("-"),
        );
    } else if let Some(v) = &d.guide.validated_commit {
        w!("{:<14}validated @ {}", "Freshness", v);
    }

    let steps: Vec<&crate::infrastructure::db::ChecklistItem> = d
        .checklist
        .iter()
        .filter(|c| c.kind != db::STEP_KIND_ACCEPTANCE)
        .collect();
    if !steps.is_empty() {
        w!("\nSteps:");
        for (i, s) in steps.iter().enumerate() {
            let mark = if s.done { "x" } else { " " };
            let badge = if s.source == "ai" { " (ai)" } else { "" };
            w!("  [{}] {}. {}{}", mark, i + 1, s.text, badge);
            if let Some(intent) = &s.intent {
                w!("        intent: {intent}");
            }
            if let Some(v) = &s.verify_cmd {
                w!("        verify: {v}");
            }
            if let Some(r) = &s.result {
                w!("        result: {r}");
            }
            if s.done && (s.done_commit.is_some() || s.done_at.is_some()) {
                let commit = s
                    .done_commit
                    .as_deref()
                    .map(|c| {
                        let short: String = c.chars().take(8).collect();
                        format!("@ {short} ")
                    })
                    .unwrap_or_default();
                let when = s.done_at.as_deref().unwrap_or("");
                w!("        done:   {commit}{when}");
            }
        }
    }

    let acceptance: Vec<&crate::infrastructure::db::ChecklistItem> = d
        .checklist
        .iter()
        .filter(|c| c.kind == db::STEP_KIND_ACCEPTANCE)
        .collect();
    if !acceptance.is_empty() {
        w!("\nAcceptance criteria:");
        for (i, a) in acceptance.iter().enumerate() {
            let mark = if a.done { "x" } else { " " };
            w!("  [{}] {}. {}", mark, i + 1, a.text);
        }
    }

    let verif = verification_rows(d);
    if !verif.is_empty() {
        w!("\nVerification:");
        for (scope, label, cmd) in &verif {
            w!("  {label:<7} {cmd}  ({scope})");
        }
    }

    for (label, kind) in [
        ("Findings", "finding"),
        ("Constraints", "constraint"),
        ("Assumptions", "assumption"),
        ("Open questions", "open_question"),
        ("Non-goals", "non_goal"),
        ("Decisions", "decision"),
        ("Risks", "risk"),
        ("Patterns", "pattern"),
    ] {
        let notes = notes_of_kind(d, kind);
        if !notes.is_empty() {
            w!("\n{label}:");
            for n in notes {
                let badge = if n.author == "ai" { " (ai)" } else { "" };
                w!("  - {}{}", n.text, badge);
            }
        }
    }

    let suggested: Vec<&crate::infrastructure::db::Anchor> = d
        .anchors
        .iter()
        .filter(|a| a.source == db::SOURCE_SUGGESTED)
        .collect();
    if !suggested.is_empty() {
        w!("\nRelevant code anchors (suggested by AI):");
        for a in suggested {
            w!("  {}{}", a.path, a.location());
            if let Some(r) = &a.reason {
                w!("      {r}");
            }
        }
    }

    for b in &d.blocked_by {
        w!("{:<14}{}", "Blocked by", b);
    }
    for b in &d.blocking {
        w!("{:<14}{}", "Blocking", b);
    }
    for link in &d.links {
        w!(
            "{:<14}[{}] {}  {}",
            "Link",
            link.id,
            link.display(),
            link.url
        );
    }
    for file in &d.manual_files {
        w!("{:<14}{}", "File", file);
    }
    let comments = notes_of_kind(d, "comment");
    if !comments.is_empty() {
        w!("\nComments:");
        let width = terminal_width();
        for (i, a) in comments.into_iter().enumerate() {
            if i > 0 {
                w!();
            }
            let date = a.entry.with_timezone(&Local).format("%Y-%m-%d %H:%M");
            let target = annotation_target(a);
            let flag = if a.request_revision {
                " (reconsider)"
            } else {
                ""
            };
            let resolved = if a.status == "resolved" {
                " (resolved)"
            } else {
                ""
            };
            w!("  #{}{}{}{}  {}", a.id, target, flag, resolved, date);
            for line in wrap_body(&a.text, "      ", width) {
                w!("{}", line);
            }
        }
    }
    if !d.ai_runs.is_empty() {
        w!("\nAI activity:");
        for r in &d.ai_runs {
            let date = r.created_at.with_timezone(&Local).format("%Y-%m-%d %H:%M");
            w!(
                "  {} via {} [{}] @ {}",
                r.kind,
                r.model.as_deref().unwrap_or("?"),
                r.provider.as_deref().unwrap_or("?"),
                date
            );
        }
    }
    if opts.history {
        for h in &d.history {
            w!(
                "{:<14}{} {}",
                "History",
                history_changed_at(h),
                history_change(h)
            );
        }
    } else if !d.history.is_empty() {
        w!(
            "{:<14}{} entries (use --history to show)",
            "History",
            d.history.len()
        );
    }
    out
}

pub(super) fn history_changed_at(h: &crate::infrastructure::db::HistoryEntry) -> String {
    h.changed_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

pub(super) fn history_change(h: &crate::infrastructure::db::HistoryEntry) -> String {
    if h.field == "created" {
        h.new_value.clone().unwrap_or_default()
    } else if h.field == "annotation" {
        match (&h.new_value, &h.old_value) {
            (Some(text), _) => format!("comment added: {text}"),
            (None, Some(text)) => format!("comment removed: {text}"),
            _ => "comment".to_string(),
        }
    } else {
        format!(
            "{}: {} -> {}",
            h.field,
            h.old_value.as_deref().unwrap_or("-"),
            h.new_value.as_deref().unwrap_or("-"),
        )
    }
}

pub(super) fn render_markdown(d: &Detail, opts: RenderOpts) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    macro_rules! w {
        () => {{ let _ = writeln!(out); }};
        ($($arg:tt)*) => {{ let _ = writeln!(out, $($arg)*); }};
    }
    let t = &d.task;

    w!("# Task {} — {}", t.id.unwrap_or(0), t.status);
    w!();
    w!("- **Project:** {}", t.project);
    w!(
        "- **Priority:** {}",
        t.priority.as_ref().map(|p| p.label()).unwrap_or("-")
    );
    if let Some(due) = t.due {
        w!(
            "- **Due:** {}",
            due.with_timezone(&Local).format("%Y-%m-%d %H:%M")
        );
    }
    if !t.tags.is_empty() {
        w!("- **Tags:** {}", t.tags.join(", "));
    }
    w!("- **Urgency:** {:.1}", t.urgency);
    w!("- **UUID:** `{}`", t.uuid);

    if guide_is_stale(d) {
        w!();
        w!(
            "> ⚠️ Guide validated @ {} but project HEAD is {} — may be stale (run `sara validate`).",
            d.guide.validated_commit.as_deref().unwrap_or("-"),
            d.head_commit.as_deref().unwrap_or("-"),
        );
    }

    w!();
    w!("## Description");
    w!();
    w!("{}", t.description);

    if let Some(a) = &d.guide.assignment {
        w!();
        w!("## Assignment");
        w!();
        w!("{a}");
    }
    if let Some(r) = &d.guide.rationale {
        w!();
        w!("## Rationale");
        w!();
        w!("{r}");
    }

    let steps: Vec<&crate::infrastructure::db::ChecklistItem> = d
        .checklist
        .iter()
        .filter(|c| c.kind != db::STEP_KIND_ACCEPTANCE)
        .collect();
    if !steps.is_empty() {
        w!();
        w!("## Steps");
        w!();
        for s in &steps {
            let mark = if s.done { "x" } else { " " };
            w!("- [{}] {}", mark, s.text);
        }
    }

    let acceptance: Vec<&crate::infrastructure::db::ChecklistItem> = d
        .checklist
        .iter()
        .filter(|c| c.kind == db::STEP_KIND_ACCEPTANCE)
        .collect();
    if !acceptance.is_empty() {
        w!();
        w!("## Acceptance criteria");
        w!();
        for a in &acceptance {
            let mark = if a.done { "x" } else { " " };
            w!("- [{}] {}", mark, a.text);
        }
    }

    for (label, kind) in [
        ("Findings", "finding"),
        ("Constraints", "constraint"),
        ("Assumptions", "assumption"),
        ("Open questions", "open_question"),
        ("Non-goals", "non_goal"),
        ("Decisions", "decision"),
        ("Risks", "risk"),
        ("Patterns", "pattern"),
    ] {
        let notes = notes_of_kind(d, kind);
        if !notes.is_empty() {
            w!();
            w!("## {label}");
            w!();
            for n in notes {
                w!("- {}", n.text);
            }
        }
    }

    let anchors: Vec<&crate::infrastructure::db::Anchor> = d
        .anchors
        .iter()
        .filter(|a| a.source == db::SOURCE_SUGGESTED)
        .collect();
    if !anchors.is_empty() {
        w!();
        w!("## Relevant code anchors");
        w!();
        for a in &anchors {
            match &a.reason {
                Some(r) => w!("- `{}{}` — {}", a.path, a.location(), r),
                None => w!("- `{}{}`", a.path, a.location()),
            }
        }
    }

    if !d.links.is_empty() {
        w!();
        w!("## Links");
        w!();
        for link in &d.links {
            w!("- [{}]({})", link.display(), link.url);
        }
    }

    if !d.blocked_by.is_empty() {
        w!();
        w!("## Blocked by");
        w!();
        for b in &d.blocked_by {
            w!("- {b}");
        }
    }
    if !d.blocking.is_empty() {
        w!();
        w!("## Blocking");
        w!();
        for b in &d.blocking {
            w!("- {b}");
        }
    }

    let comments = notes_of_kind(d, "comment");
    if !comments.is_empty() {
        w!();
        w!("## Comments");
        w!();
        for a in comments {
            let flag = if a.request_revision {
                " **(reconsider)**"
            } else {
                ""
            };
            let resolved = if a.status == "resolved" {
                " _(resolved)_"
            } else {
                ""
            };
            w!("- #{}{}{} {}", a.id, flag, resolved, a.text);
        }
    }

    if opts.history && !d.history.is_empty() {
        w!();
        w!("## History");
        w!();
        for h in &d.history {
            w!("- {} {}", history_changed_at(h), history_change(h));
        }
    }

    out
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/info/plain.rs"]
mod tests;
