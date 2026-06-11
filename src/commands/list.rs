use anyhow::Result;
use chrono::{Local, Utc};
use rusqlite::Connection;

use crate::config::Config;
use crate::db;
use crate::model::{Priority, Task};
use crate::project::detect_current_project;

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RED: &str = "\x1b[31m";
const YELLOW: &str = "\x1b[33m";
const GREEN: &str = "\x1b[32m";
const CYAN: &str = "\x1b[36m";
const GRAY: &str = "\x1b[90m";

pub fn run(
    conn: &Connection,
    cfg: &Config,
    all: bool,
    project_filter: Option<&str>,
) -> Result<()> {
    let no_color = std::env::var("NO_COLOR").is_ok();

    let filter = if all {
        None
    } else if let Some(p) = project_filter {
        Some(p.to_string())
    } else {
        let (name, _) = detect_current_project(conn, cfg)?;
        Some(name)
    };

    let tasks = db::list_tasks(conn, filter.as_deref())?;

    if tasks.is_empty() {
        let scope = filter
            .as_deref()
            .map(|p| format!("project '{p}'"))
            .unwrap_or_else(|| "any project".to_string());
        println!("No pending tasks for {scope}.");
        return Ok(());
    }

    // Header
    let header = format!(
        " {id:>3}  {pri:<4}  {proj:<16}  {due:<12}  {urg:>6}  {desc}",
        id = "ID",
        pri = "PRI",
        proj = "PROJECT",
        due = "DUE",
        urg = "URG",
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
        let id_str = task.id.map(|i| i.to_string()).unwrap_or_else(|| "-".to_string());
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

        // Colorize
        if no_color {
            println!(
                "{active}{id:>3}  {pri:<4}  {proj:<16}  {due:<12}  {urg:>6}  {desc}",
                active = active_marker,
                id = id_str,
                pri = pri_str,
                proj = proj_display,
                due = due_str,
                urg = urg_str,
                desc = desc_display,
            );
        } else {
            let pri_colored = match &task.priority {
                Some(Priority::H) => format!("{RED}{BOLD}{pri_str:<4}{RESET}"),
                Some(Priority::M) => format!("{YELLOW}{pri_str:<4}{RESET}"),
                Some(Priority::L) => format!("{GREEN}{pri_str:<4}{RESET}"),
                None => format!("{GRAY}{pri_str:<4}{RESET}"),
            };
            let due_colored = color_due(&task, &due_str, no_color);
            let active_col = if task.is_active() {
                format!("{GREEN}●{RESET}")
            } else {
                " ".to_string()
            };
            println!(
                "{active}{CYAN}{id:>3}{RESET}  {pri}  {GRAY}{proj:<16}{RESET}  {due:<12}  {GRAY}{urg:>6}{RESET}  {desc}",
                active = active_col,
                id = id_str,
                pri = pri_colored,
                proj = proj_display,
                due = due_colored,
                urg = urg_str,
                desc = desc_display,
            );
        }
    }

    println!();
    println!(
        "{DIM}Showing {} task{}{}",
        tasks.len(),
        if tasks.len() == 1 { "" } else { "s" },
        filter
            .as_deref()
            .map(|p| format!(" for project '{p}'"))
            .unwrap_or_default()
    );
    print!("{RESET}");

    Ok(())
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
    let days = task
        .due
        .map(|d| (d - now).num_days())
        .unwrap_or(999);
    let color = if days < 0 {
        RED
    } else if days <= 1 {
        YELLOW
    } else {
        RESET
    };
    format!("{color}{due_str:<12}{RESET}")
}
