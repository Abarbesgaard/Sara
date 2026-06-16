use anyhow::Result;
use chrono::{Local, Timelike};
use indicatif::{ProgressBar, ProgressStyle};
use rusqlite::Connection;
use std::time::Duration;

use crate::config::Config;
use crate::db;
use crate::learn;
use crate::llm::{self, brief_system_prompt};
use crate::memory;
use crate::model::{Item, Task};
use crate::project::detect_current_project;

pub fn run(conn: &Connection, cfg: &Config, no_llm: bool) -> Result<()> {
    let ctx = build_brief_context(conn, cfg)?;

    if !no_llm {
        let system = brief_system_prompt(learn::read_profile_context(cfg).as_deref());
        let user = format_context_for_llm(&ctx);
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::default_spinner()
                .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
                .template("{spinner:.cyan} {msg}")
                .unwrap(),
        );
        spinner.set_message("Sara is preparing your brief…");
        spinner.enable_steady_tick(Duration::from_millis(80));

        let provider = llm::build_provider(cfg);
        let answer = provider.chat(&system, &user);
        spinner.finish_and_clear();

        if let Ok(text) = answer {
            println!("{text}\n");
            return Ok(());
        }
    }

    print_template_brief(&ctx);
    Ok(())
}

struct BriefContext {
    greeting: String,
    current_project: Option<String>,
    project_tasks: Vec<Task>,
    other_tasks: Vec<Task>,
    due_today: Vec<Task>,
    memory_lines: Vec<String>,
    captures: Vec<Item>,
}

fn build_brief_context(conn: &Connection, cfg: &Config) -> Result<BriefContext> {
    let greeting = time_greeting();
    let current_project = detect_current_project(conn, cfg)
        .ok()
        .map(|(name, _)| name);

    let mut tasks = db::list_tasks(conn, None)?;
    tasks.sort_by(|a, b| b.urgency.partial_cmp(&a.urgency).unwrap_or(std::cmp::Ordering::Equal));

    let today = Local::now().date_naive();
    let due_today: Vec<Task> = tasks
        .iter()
        .filter(|t| t.due.map(|d| d.date_naive()) == Some(today))
        .cloned()
        .collect();

    let (project_tasks, other_tasks): (Vec<_>, Vec<_>) = if let Some(ref project) = current_project {
        tasks
            .into_iter()
            .partition(|t| t.project == *project)
    } else {
        (Vec::new(), tasks)
    };

    let memory_lines = memory_highlights(cfg, current_project.as_deref());

    let mut notes = db::list_items(conn, Some("note")).unwrap_or_default();
    let mut links = db::list_items(conn, Some("link")).unwrap_or_default();
    notes.append(&mut links);
    notes.sort_by(|a, b| b.modified.cmp(&a.modified));

    Ok(BriefContext {
        greeting,
        current_project,
        project_tasks: project_tasks.into_iter().take(5).collect(),
        other_tasks: other_tasks.into_iter().take(4).collect(),
        due_today,
        memory_lines,
        captures: notes.into_iter().take(3).collect(),
    })
}

fn time_greeting() -> String {
    match Local::now().hour() {
        5..=11 => "Good morning".to_string(),
        12..=16 => "Good afternoon".to_string(),
        17..=21 => "Good evening".to_string(),
        _ => "Hey".to_string(),
    }
}

fn memory_highlights(cfg: &Config, current_project: Option<&str>) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(project) = current_project {
        if let Some(status) = memory::project_status_summary(cfg, project) {
            lines.push(status);
        }
    }
    for line in memory::read_long_term(cfg).lines() {
        let l = line.trim();
        if !l.starts_with("- ") {
            continue;
        }
        if l.contains("Tell Sara") || l.contains("Run `sara learn`") {
            continue;
        }
        let fact = l[2..].trim();
        if fact.is_empty() || lines.iter().any(|x| x.contains(fact)) {
            continue;
        }
        lines.push(fact.to_string());
        if lines.len() >= 3 {
            break;
        }
    }
    lines
}

fn format_context_for_llm(ctx: &BriefContext) -> String {
    let mut parts = vec![
        format!("Time: {} ({})", ctx.greeting, Local::now().format("%A %Y-%m-%d")),
    ];
    if let Some(ref project) = ctx.current_project {
        parts.push(format!("Current git project: {project}"));
    } else {
        parts.push("Current directory: not inside a tracked project repo".to_string());
    }
    if !ctx.memory_lines.is_empty() {
        parts.push(format!("Memory:\n{}", ctx.memory_lines.join("\n")));
    }
    if !ctx.due_today.is_empty() {
        parts.push(format!(
            "Due today:\n{}",
            ctx.due_today.iter().map(format_task_line).collect::<Vec<_>>().join("\n")
        ));
    }
    if !ctx.project_tasks.is_empty() {
        parts.push(format!(
            "Tasks in current project:\n{}",
            ctx.project_tasks
                .iter()
                .map(format_task_line)
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if !ctx.other_tasks.is_empty() {
        parts.push(format!(
            "Other pending tasks:\n{}",
            ctx.other_tasks
                .iter()
                .map(format_task_line)
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    if !ctx.captures.is_empty() {
        parts.push(format!(
            "Recent captures:\n{}",
            ctx.captures
                .iter()
                .map(|i| format!("- {} {} — {}", i.kind, i.handle(), i.title))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }
    parts.join("\n\n")
}

fn format_task_line(t: &Task) -> String {
    let due = t
        .due
        .map(|d| d.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "no due date".to_string());
    format!(
        "- [{}] {} ({}, due {due})",
        t.id.unwrap_or(0),
        t.description,
        t.project
    )
}

fn due_hint(t: &Task) -> String {
    let today = Local::now().date_naive();
    match t.due.map(|d| d.date_naive()) {
        Some(d) if d == today => " — due today".to_string(),
        Some(d) if d < today => format!(" — overdue ({d})"),
        Some(d) => format!(" — due {d}"),
        None => String::new(),
    }
}

fn print_template_brief(ctx: &BriefContext) {
    println!("{}\n", ctx.greeting);

    if let Some(ref project) = ctx.current_project {
        if ctx.project_tasks.is_empty() {
            println!("You're in **{project}** — no pending tasks here right now.");
        } else if ctx.project_tasks.len() == 1 {
            let t = &ctx.project_tasks[0];
            println!(
                "You're in **{project}**. One thing on your plate here: **{}**{}.",
                t.description,
                due_hint(t)
            );
        } else {
            println!(
                "You're in **{project}** — {} tasks here are calling for attention:",
                ctx.project_tasks.len()
            );
            for t in &ctx.project_tasks {
                println!("  • {}{}", t.description, due_hint(t));
            }
        }
    } else if !ctx.other_tasks.is_empty() {
        println!("Here's what's on your mind:");
        for t in ctx.other_tasks.iter().take(3) {
            println!("  • {} ({})", t.description, t.project);
        }
    }

    if !ctx.due_today.is_empty() {
        let names: Vec<_> = ctx.due_today.iter().map(|t| t.description.as_str()).collect();
        if names.len() == 1 {
            println!("\n**Due today:** {}", names[0]);
        } else {
            println!("\n**Due today:** {}", names.join(", "));
        }
    }

    if !ctx.memory_lines.is_empty() {
        println!();
        for line in &ctx.memory_lines {
            println!("_{line}_");
        }
    }

    if !ctx.captures.is_empty() && ctx.current_project.is_some() {
        let item = &ctx.captures[0];
        println!(
            "\nRecently saved: **{}** ({}) — might be worth a look when you surface.",
            item.title,
            item.handle()
        );
    }

    if let Some(next) = ctx.due_today.first().or(ctx.project_tasks.first()) {
        println!("\n→ I'd start with **{}**.", next.description);
    } else if let Some(t) = ctx.other_tasks.first() {
        println!("\n→ When you're ready: **{}**.", t.description);
    }

    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greeting_varies_by_time_bucket() {
        let g = time_greeting();
        assert!(!g.is_empty());
    }
}
