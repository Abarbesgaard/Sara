use anyhow::Result;
use rusqlite::Connection;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::config::Config;
use crate::db;
use crate::memory;
use crate::model::{Item, Task};

/// OpenClaw-style "dreaming": distill daily notes + store into MEMORY.md.
pub fn rebuild_profile(conn: &Connection, cfg: &Config) -> Result<()> {
    let store = crate::vault::store_root(cfg)?;
    memory::init_scaffold(&store)?;

    let notes = db::list_items(conn, Some("note")).unwrap_or_default();
    let links = db::list_items(conn, Some("link")).unwrap_or_default();
    let tasks = db::list_tasks(conn, None).unwrap_or_default();
    let daily = memory::recent_daily_notes(cfg, 7);
    let current_memory = memory::read_long_term(cfg);

    // Dream + index memory for semantic recall
    if let Some(new_memory) = dream_into_memory(cfg, &current_memory, &daily, &notes, &links, &tasks) {
        memory::write_long_term(cfg, &new_memory)?;
        println!("Long-term memory updated at {}", store.join(".sara/MEMORY.md").display());
    }
    if let Ok(n) = memory::index_embeddings(conn, cfg) {
        eprintln!("Indexed {n} memory chunks for semantic search.");
    }

    // Dashboard profile (human-readable index)
    let profile_path = store.join(".sara/profile.md");
    let body = build_dashboard(conn, cfg, &notes, &links, &tasks)?;
    write_profile(&profile_path, &body)?;
    println!("Profile dashboard at {}", profile_path.display());
    Ok(())
}

fn dream_into_memory(
    cfg: &Config,
    current: &str,
    daily: &[(chrono::NaiveDate, String)],
    notes: &[Item],
    links: &[Item],
    tasks: &[Task],
) -> Option<String> {
    let mut input = String::new();
    if !current.trim().is_empty() {
        input.push_str("Current MEMORY.md:\n");
        input.push_str(current);
        input.push_str("\n\n");
    }
    if !daily.is_empty() {
        input.push_str("Recent daily notes:\n");
        for (date, body) in daily {
            input.push_str(&format!("--- {date} ---\n{body}\n\n"));
        }
    }
    for item in notes.iter().chain(links).take(15) {
        input.push_str(&format!(
            "Capture {}: {} — {}\n",
            item.handle(),
            item.title,
            item.summary.as_deref().unwrap_or("")
        ));
    }
    if !tasks.is_empty() {
        input.push_str("\nPending tasks:\n");
        for t in tasks.iter().take(10) {
            input.push_str(&format!("- {} ({})\n", t.description, t.project));
        }
    }
    if input.trim().is_empty() {
        return None;
    }

    let system = "You are Sara's memory consolidation pass (like OpenClaw dreaming). \
        Rewrite MEMORY.md as curated long-term memory: durable facts, preferences, active projects, \
        and standing decisions about the user. Use markdown with short bullet points. \
        Drop stale or duplicate items. Do NOT include raw activity counts or daily logs — \
        those belong in daily notes. Keep under 80 lines. Output ONLY the markdown body \
        (no frontmatter, no code fences). Start with `# Long-term memory`.";
    let user = format!("Consolidate into an updated MEMORY.md:\n\n{input}");

    let provider = crate::llm::build_provider(cfg);
    provider.chat(system, &user).ok()
}

fn build_dashboard(
    conn: &Connection,
    cfg: &Config,
    notes: &[Item],
    links: &[Item],
    tasks: &[Task],
) -> Result<String> {
    let events = db::recent_events(conn, 500)?;
    let mut body = String::from("# Sara's dashboard\n\n");
    body.push_str("_Index view. Durable memory lives in `.sara/MEMORY.md`; daily notes in `memory/`._\n\n");

    if let Some(mem) = memory::read_memory_context(cfg) {
        let preview: String = mem.chars().take(500).collect();
        body.push_str("## Memory preview\n\n");
        body.push_str(&preview);
        if mem.len() > 500 {
            body.push_str("…\n");
        }
        body.push('\n');
    }

    let mut recent: Vec<Item> = notes.to_vec();
    recent.extend(links.iter().cloned());
    recent.sort_by(|a, b| b.modified.cmp(&a.modified));
    if !recent.is_empty() {
        body.push_str("## Recent captures\n\n");
        for item in recent.iter().take(10) {
            body.push_str(&format!("- **{}** — {}\n", item.handle(), item.title));
        }
        body.push('\n');
    }

    let mut top_tasks: Vec<Task> = tasks.to_vec();
    top_tasks.sort_by(|a, b| b.urgency.partial_cmp(&a.urgency).unwrap_or(std::cmp::Ordering::Equal));
    if !top_tasks.is_empty() {
        body.push_str("## Active tasks\n\n");
        for t in top_tasks.iter().take(8) {
            body.push_str(&format!("- [{}] {} _({})_\n", t.id.unwrap_or(0), t.description, t.project));
        }
        body.push('\n');
    }

    if let Ok(searches) = db::recent_search_queries(conn, 8) {
        if !searches.is_empty() {
            body.push_str("## Recent questions\n\n");
            for q in &searches {
                body.push_str(&format!("- \"{q}\"\n"));
            }
            body.push('\n');
        }
    }

    let mut action_counts: HashMap<String, u32> = HashMap::new();
    for (action, _, _) in &events {
        *action_counts.entry(action.clone()).or_insert(0) += 1;
    }
    body.push_str("## Activity\n\n");
    for (action, count) in action_counts {
        body.push_str(&format!("- {action}: {count}\n"));
    }

    Ok(body)
}

/// Memory + profile for LLM calls (OpenClaw loads MEMORY.md every turn).
pub fn read_profile_context(cfg: &Config) -> Option<String> {
    memory::read_memory_context(cfg).or_else(|| {
        let store = crate::config::vault_path(cfg).ok()?;
        let profile_path = store.join(".sara/profile.md");
        let content = fs::read_to_string(&profile_path).ok()?;
        Some(content.chars().take(4000).collect())
    })
}

fn write_profile(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = format!("---\ntype: profile\n---\n\n{body}");
    fs::write(path, content)?;
    Ok(())
}
