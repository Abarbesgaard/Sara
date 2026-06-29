use chrono::Local;

use crate::infrastructure::db;

use super::handler::{guide_is_stale, notes_of_kind, verification_rows};
use super::types::Detail;

/// Options controlling the readable digest renderers (`render_plain` /
/// `render_markdown`). Defaults to the agent-friendly view: History collapsed.
#[derive(Clone, Copy, Default)]
pub(super) struct RenderOpts {
    /// Include the full History log (collapsed to a one-line summary otherwise).
    pub(super) history: bool,
}

/// Render the readable plain-text digest of a task — the single source of truth
/// shared by the non-TTY fallback, `sara info --plain`, and (later) the MCP
/// server. History is collapsed by default to keep agent token usage low.
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

    // ── Guide ───────────────────────────────────────────────────────
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

    // Steps (with intent + result).
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

    // Acceptance criteria.
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

    // Verification commands (project + task-level).
    let verif = verification_rows(d);
    if !verif.is_empty() {
        w!("\nVerification:");
        for (scope, label, cmd) in &verif {
            w!("  {label:<7} {cmd}  ({scope})");
        }
    }

    // Typed AI/human notes grouped by kind.
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

    // Code anchors (relevant files with reasons).
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
    // Comments (human feedback), with anchor + reconsider markers.
    let comments = notes_of_kind(d, "comment");
    if !comments.is_empty() {
        w!("\nComments:");
        for a in comments {
            let date = a.entry.with_timezone(&Local).format("%Y-%m-%d %H:%M");
            let target = match (&a.target_kind, &a.target_id) {
                (Some(k), Some(idv)) => format!(" [{k}:{idv}]"),
                _ => String::new(),
            };
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
            w!(
                "  #{}{}{}{} {} {}",
                a.id,
                target,
                flag,
                resolved,
                date,
                a.text
            );
        }
    }
    // AI activity footer.
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
    // History — collapsed to a one-line summary unless explicitly requested.
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

/// Format a single history entry's timestamp for the readable digest.
pub(super) fn history_changed_at(h: &crate::infrastructure::db::HistoryEntry) -> String {
    h.changed_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

/// Describe a single history entry as a one-line change summary.
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

/// Render a Markdown digest of a task — description, steps and acceptance
/// criteria as checkboxes, plus the key context sections. Suitable for embedding
/// in agent context or a PR body. Shares `RenderOpts` with `render_plain`.
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

    // Typed AI/human notes grouped by kind.
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

    // Human comments — high-signal direction for an agent; flag reconsider/open.
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
mod tests {
    use super::super::edit::{apply_field, current_value, cycle_priority};
    use super::super::types::{Detail, EditField};
    use super::{RenderOpts, render_markdown, render_plain};
    use crate::infrastructure::config::Config;
    use crate::infrastructure::db;
    use crate::infrastructure::model::{Priority, Task};
    use chrono::Utc;

    fn task() -> Task {
        Task::new("original".into(), "tk".into())
    }

    #[test]
    fn editing_description_updates_value() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Description, "new description", &cfg);
        assert_eq!(t.description, "new description");
    }

    #[test]
    fn empty_description_is_ignored() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Description, "   ", &cfg);
        assert_eq!(t.description, "original");
    }

    #[test]
    fn editing_tags_splits_and_trims() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Tags, " rust , cli ,", &cfg);
        assert_eq!(t.tags, vec!["rust".to_string(), "cli".to_string()]);
    }

    #[test]
    fn editing_due_empty_clears_it() {
        let mut t = task();
        let cfg = Config::default();
        t.due = Some(Utc::now());
        apply_field(&mut t, EditField::Due, "", &cfg);
        assert!(t.due.is_none());
    }

    #[test]
    fn editing_due_parses_relative() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Due, "+3d", &cfg);
        assert!(t.due.is_some());
    }

    #[test]
    fn priority_cycles_forward_and_back() {
        let mut t = task();
        assert!(t.priority.is_none());
        cycle_priority(&mut t, true);
        assert_eq!(t.priority, Some(Priority::L));
        cycle_priority(&mut t, true);
        assert_eq!(t.priority, Some(Priority::M));
        cycle_priority(&mut t, true);
        assert_eq!(t.priority, Some(Priority::H));
        cycle_priority(&mut t, true);
        assert!(t.priority.is_none());
        cycle_priority(&mut t, false);
        assert_eq!(t.priority, Some(Priority::H));
    }

    #[test]
    fn current_value_round_trips_with_apply() {
        let mut t = task();
        let cfg = Config::default();
        apply_field(&mut t, EditField::Project, "myproj", &cfg);
        assert_eq!(current_value(&t, EditField::Project), "myproj");
        apply_field(&mut t, EditField::Tags, "a, b", &cfg);
        assert_eq!(current_value(&t, EditField::Tags), "a, b");
    }

    fn step(
        id: i64,
        text: &str,
        kind: &str,
        done: bool,
    ) -> crate::infrastructure::db::ChecklistItem {
        crate::infrastructure::db::ChecklistItem {
            id,
            text: text.into(),
            done,
            position: id,
            intent: None,
            kind: kind.into(),
            source: "human".into(),
            verify_cmd: None,
            result: None,
            done_commit: None,
            done_at: None,
        }
    }

    fn history(
        field: &str,
        old: Option<&str>,
        new: Option<&str>,
    ) -> crate::infrastructure::db::HistoryEntry {
        crate::infrastructure::db::HistoryEntry {
            field: field.into(),
            old_value: old.map(Into::into),
            new_value: new.map(Into::into),
            changed_at: chrono::Utc::now(),
        }
    }

    fn detail(
        checklist: Vec<crate::infrastructure::db::ChecklistItem>,
        hist: Vec<crate::infrastructure::db::HistoryEntry>,
    ) -> Detail {
        Detail {
            task: task(),
            blocked_by: vec![],
            blocking: vec![],
            depends_on_ids: vec![],
            manual_files: vec![],
            suggested_files: vec![],
            links: vec![],
            annotations: vec![],
            history: hist,
            project_root: None,
            branch: None,
            overlaps: vec![],
            similar: vec![],
            checklist,
            urgency_breakdown: None,
            activity: std::collections::HashMap::new(),
            stats: None,
            guide: crate::infrastructure::db::TaskGuideFields::default(),
            anchors: vec![],
            ai_runs: vec![],
            head_commit: None,
            project_commands: crate::infrastructure::db::ProjectCommands::default(),
            chain: vec![],
        }
    }

    #[test]
    fn render_plain_collapses_history_by_default() {
        let d = detail(
            vec![],
            vec![history("status", Some("pending"), Some("done"))],
        );
        let collapsed = render_plain(&d, RenderOpts { history: false });
        assert!(collapsed.contains("1 entries (use --history to show)"));
        assert!(!collapsed.contains("status: pending -> done"));

        let full = render_plain(&d, RenderOpts { history: true });
        assert!(full.contains("status: pending -> done"));
    }

    #[test]
    fn render_plain_lists_steps_and_acceptance() {
        let d = detail(
            vec![
                step(1, "do the thing", db::STEP_KIND_STEP, true),
                step(2, "ship it", db::STEP_KIND_ACCEPTANCE, false),
            ],
            vec![],
        );
        let out = render_plain(&d, RenderOpts::default());
        assert!(out.contains("Steps:"));
        assert!(out.contains("[x] 1. do the thing"));
        assert!(out.contains("Acceptance criteria:"));
        assert!(out.contains("[ ] 1. ship it"));
    }

    #[test]
    fn render_markdown_has_description_steps_and_acceptance() {
        let d = detail(
            vec![
                step(1, "first step", db::STEP_KIND_STEP, false),
                step(2, "second step", db::STEP_KIND_STEP, true),
                step(3, "definition of done", db::STEP_KIND_ACCEPTANCE, false),
            ],
            vec![],
        );
        let md = render_markdown(&d, RenderOpts::default());
        // Description present.
        assert!(md.contains("## Description"));
        assert!(md.contains("original"));
        // Steps rendered as GitHub-style checkboxes.
        assert!(md.contains("## Steps"));
        assert!(md.contains("- [ ] first step"));
        assert!(md.contains("- [x] second step"));
        // Acceptance criteria rendered with checkboxes.
        assert!(md.contains("## Acceptance criteria"));
        assert!(md.contains("- [ ] definition of done"));
    }

    #[test]
    fn render_markdown_omits_history_unless_requested() {
        let d = detail(
            vec![],
            vec![history("status", Some("pending"), Some("done"))],
        );
        let lean = render_markdown(&d, RenderOpts { history: false });
        assert!(!lean.contains("## History"));

        let full = render_markdown(&d, RenderOpts { history: true });
        assert!(full.contains("## History"));
        assert!(full.contains("status: pending -> done"));
    }
}
