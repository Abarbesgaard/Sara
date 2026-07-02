use anyhow::Result;
use chrono::{Local, Utc};
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::{Priority, Task};
use crate::infrastructure::project::detect_current_project;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const GRAY: &str = "\x1b[90m";
const MAGENTA: &str = "\x1b[35m";

/// Resolve the project filter for a list request: `None` when `all`, an explicit
/// override when given, else the current git project.
fn resolve_filter(
    conn: &Connection,
    cfg: &Config,
    all: bool,
    project_filter: Option<&str>,
) -> Result<Option<String>> {
    if all {
        Ok(None)
    } else if let Some(p) = project_filter {
        Ok(Some(p.to_string()))
    } else {
        let (name, _) = detect_current_project(conn, cfg)?;
        Ok(Some(name))
    }
}

/// Serialize a slice of tasks into the `{ "tasks": [...] }` shape. Single source
/// of truth for the `--json` CLI path and the MCP `list` tool.
fn tasks_to_value(tasks: &[Task]) -> serde_json::Value {
    let arr: Vec<_> = tasks
        .iter()
        .map(|t| {
            serde_json::json!({
                "id": t.id,
                "uuid": t.uuid.to_string(),
                "description": t.description,
                "project": t.project,
                "priority": t.priority.as_ref().map(|p| p.label()),
                "due": t.due.map(|d| d.to_rfc3339()),
                "urgency": t.urgency,
                "status": t.status.to_string(),
                "tags": t.tags,
            })
        })
        .collect();
    serde_json::json!({ "tasks": arr })
}

/// Structured task list for the MCP `list` tool.
pub fn list_value(
    conn: &Connection,
    cfg: &Config,
    all: bool,
    project_filter: Option<&str>,
) -> Result<serde_json::Value> {
    let filter = resolve_filter(conn, cfg, all, project_filter)?;
    let tasks = db::list_tasks(conn, filter.as_deref())?;
    Ok(tasks_to_value(&tasks))
}

pub fn run(
    conn: &Connection,
    cfg: &Config,
    all: bool,
    project_filter: Option<&str>,
    as_json: bool,
    by_issue: bool,
) -> Result<()> {
    let no_color = std::env::var("NO_COLOR").is_ok();

    let filter = resolve_filter(conn, cfg, all, project_filter)?;

    let tasks = db::list_tasks(conn, filter.as_deref())?;
    let link_flags = db::link_flags_by_task(conn).unwrap_or_default();
    let dep_info = db::dep_info_by_task(conn).unwrap_or_default();

    if as_json {
        println!("{}", serde_json::to_string_pretty(&tasks_to_value(&tasks))?);
        return Ok(());
    }

    if by_issue {
        return print_by_issue(conn, &tasks, no_color);
    }

    if tasks.is_empty() {
        let scope = filter
            .as_deref()
            .map(|p| format!("project '{p}'"))
            .unwrap_or_else(|| "any project".to_string());
        println!("No pending tasks for {scope}.");
        if !all && filter.is_some() {
            let all_tasks = db::list_tasks(conn, None)?;
            if !all_tasks.is_empty() {
                println!(
                    "Tip: run `sara list -a` to see all {} pending tasks.",
                    all_tasks.len()
                );
            }
        }
        return Ok(());
    }

    // Header
    let header = format!(
        "    {id:>3}  {pri:<4}  {proj:<16}  {due:<12}  {urg:>6}  {dep:<16}  {desc}",
        id = "ID",
        pri = "PRI",
        proj = "PROJECT",
        due = "DUE",
        urg = "URG",
        dep = "DEPS",
        desc = "DESCRIPTION",
    );
    if no_color {
        println!("{header}");
        println!("{}", "─".repeat(header.len()));
    } else {
        println!("{BOLD}{header}{RESET}");
        println!("{DIM}{}{RESET}", "─".repeat(80));
    }

    for task in &tasks {
        let id_str = task
            .id
            .map(|i| i.to_string())
            .unwrap_or_else(|| "-".to_string());
        let pri_str = task
            .priority
            .as_ref()
            .map(|p| p.label().to_string())
            .unwrap_or_else(|| "-".to_string());
        let due_str = task
            .due
            .map(|d| d.with_timezone(&Local).format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "-".to_string());
        let urg_str = format!("{:.1}", task.urgency);

        // Truncate project and description for display
        let proj_display = truncate(&task.project, 16);
        let desc_display = truncate(&task.description, 60);

        let active_marker = if task.is_active() { "●" } else { " " };

        // PR / link / recur indicators for an at-a-glance scan.
        let flags = link_flags
            .get(&task.uuid.to_string())
            .copied()
            .unwrap_or_default();
        let recur_mark = if task.recur.is_some() { "♺" } else { " " };

        // Dependency state: ⊘ = blocked by an unfinished task, ⛓ = blocks others.
        // The glyph is a quick-scan gutter marker; the DEPS column spells it out.
        let dep = dep_info.get(&task.uuid.to_string());
        let dep_mark = match dep {
            Some(d) if d.is_blocked() => "⊘",
            Some(d) if d.blocking > 0 => "⛓",
            _ => " ",
        };
        let dep_text = truncate(&dep_column_text(dep), DEP_COL_W);
        let link_badge = LinkBadge::from_flags(flags);
        let pr_badge_plain = match link_badge {
            LinkBadge::Pr => "[PR] ",
            LinkBadge::Issue => "[ISSUE] ",
            LinkBadge::Link => "[link] ",
            LinkBadge::None => "",
        };

        // Colorize
        if no_color {
            println!(
                "{active}{recur}{dep} {id:>3}  {pri:<4}  {proj:<16}  {due:<12}  {urg:>6}  {deptext:<width$}  {pr}{desc}",
                active = active_marker,
                recur = recur_mark,
                dep = dep_mark,
                id = id_str,
                pri = pri_str,
                proj = proj_display,
                due = due_str,
                urg = urg_str,
                deptext = dep_text,
                width = DEP_COL_W,
                pr = pr_badge_plain,
                desc = desc_display,
            );
        } else {
            let pri_colored = match &task.priority {
                Some(Priority::H) => format!("{RED}{BOLD}{pri_str:<4}{RESET}"),
                Some(Priority::M) => format!("{YELLOW}{pri_str:<4}{RESET}"),
                Some(Priority::L) => format!("{GREEN}{pri_str:<4}{RESET}"),
                None => format!("{GRAY}{pri_str:<4}{RESET}"),
            };
            let due_colored = color_due(task, &due_str, no_color);
            let active_col = if task.is_active() {
                format!("{GREEN}●{RESET}")
            } else {
                " ".to_string()
            };
            let recur_col = if task.recur.is_some() {
                format!("{CYAN}♺{RESET}")
            } else {
                " ".to_string()
            };
            let dep_col = match dep {
                Some(d) if d.is_blocked() => format!("{RED}⊘{RESET}"),
                Some(d) if d.blocking > 0 => format!("{CYAN}⛓{RESET}"),
                _ => " ".to_string(),
            };
            let dep_padded = format!("{:<width$}", dep_text, width = DEP_COL_W);
            let dep_text_col = match dep {
                Some(d) if d.is_blocked() => format!("{RED}{dep_padded}{RESET}"),
                Some(d) if d.blocking > 0 => format!("{GRAY}{dep_padded}{RESET}"),
                _ => dep_padded,
            };
            let pr_badge = match link_badge {
                LinkBadge::Pr => format!("{MAGENTA}{BOLD}PR{RESET} "),
                LinkBadge::Issue => format!("{GREEN}{BOLD}ISS{RESET} "),
                LinkBadge::Link => format!("{CYAN}↗{RESET} "),
                LinkBadge::None => String::new(),
            };
            println!(
                "{active}{recur}{dep} {CYAN}{id:>3}{RESET}  {pri}  {GRAY}{proj:<16}{RESET}  {due:<12}  {GRAY}{urg:>6}{RESET}  {deptext}  {pr}{desc}",
                active = active_col,
                recur = recur_col,
                dep = dep_col,
                id = id_str,
                pri = pri_colored,
                proj = proj_display,
                due = due_colored,
                urg = urg_str,
                deptext = dep_text_col,
                pr = pr_badge,
                desc = desc_display,
            );
        }
    }

    println!();
    let summary = format!(
        "Showing {} task{}{}",
        tasks.len(),
        if tasks.len() == 1 { "" } else { "s" },
        filter
            .as_deref()
            .map(|p| format!(" for project '{p}'"))
            .unwrap_or_default()
    );
    if no_color {
        println!("{summary}");
    } else {
        println!("{DIM}{summary}{RESET}");
    }

    Ok(())
}

/// `sara list --by-issue`: trace tasks back to the GitHub issue they link to,
/// so a broken-down issue and its concrete tasks read together at a glance.
fn print_by_issue(conn: &Connection, tasks: &[Task], no_color: bool) -> Result<()> {
    let (groups, ungrouped) = db::group_tasks_by_issue(conn, tasks)?;

    if groups.is_empty() && ungrouped.is_empty() {
        println!("No pending tasks.");
        return Ok(());
    }

    let print_group_rows = |tasks: &[Task]| {
        for task in tasks {
            let id_str = task
                .id
                .map(|i| i.to_string())
                .unwrap_or_else(|| "-".to_string());
            println!("   {:>3}  {}", id_str, truncate(&task.description, 60));
        }
    };

    for group in &groups {
        let header = format!("Issue #{} · {}", group.number, group.owner_repo);
        if no_color {
            println!("{header}");
        } else {
            println!("{BOLD}{CYAN}{header}{RESET}");
        }
        print_group_rows(&group.tasks);
        println!();
    }

    if !ungrouped.is_empty() {
        if no_color {
            println!("No linked issue");
        } else {
            println!("{DIM}No linked issue{RESET}");
        }
        print_group_rows(&ungrouped);
        println!();
    }

    Ok(())
}

/// Which link badge a task should show at a glance, in priority order.
/// PR outranks Issue, which outranks a plain/generic link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkBadge {
    Pr,
    Issue,
    Link,
    None,
}

impl LinkBadge {
    pub fn from_flags(flags: db::LinkFlags) -> Self {
        if flags.pr {
            LinkBadge::Pr
        } else if flags.issue {
            LinkBadge::Issue
        } else if flags.any {
            LinkBadge::Link
        } else {
            LinkBadge::None
        }
    }
}

/// Width of the DEPS column in the task list.
const DEP_COL_W: usize = 16;

fn fmt_id_list(ids: &[i64]) -> String {
    ids.iter()
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(",")
}

/// Plain text for the DEPS column: what the task is waiting on or blocking.
fn dep_column_text(dep: Option<&db::DepInfo>) -> String {
    match dep {
        Some(d) if d.is_blocked() => format!("blocked by {}", fmt_id_list(&d.blocked_by)),
        Some(d) if d.blocking > 0 => format!(
            "blocks {} task{}",
            d.blocking,
            if d.blocking == 1 { "" } else { "s" }
        ),
        _ => String::new(),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max - 1).collect();
        out.push('…');
        out
    }
}

fn color_due(task: &Task, due_str: &str, no_color: bool) -> String {
    if no_color || task.due.is_none() {
        return format!("{due_str:<12}");
    }
    let now = Utc::now();
    let days = task.due.map(|d| (d - now).num_days()).unwrap_or(999);
    let color = if days < 0 {
        RED
    } else if days <= 1 {
        YELLOW
    } else {
        RESET
    };
    format!("{color}{due_str:<12}{RESET}")
}
