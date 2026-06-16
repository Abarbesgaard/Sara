use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use rusqlite::Connection;
use std::collections::HashMap;
use std::time::Duration;

use crate::config::Config;
use crate::db;
use crate::embed;
use crate::learn;
use crate::llm::{self, search_system_prompt};
use crate::model::{Item, Project, Task};
use crate::project::detect_current_project;

struct ProjectScope {
    name: String,
    profile: Option<Project>,
    tasks: Vec<Task>,
}

struct SearchHit {
    score: f32,
    item: Item,
}

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "i", "my", "me", "is", "are", "was", "were", "be", "been", "being",
    "have", "has", "had", "do", "does", "did", "will", "would", "could", "should", "can",
    "with", "about", "for", "from", "into", "that", "this", "what", "when", "where", "who",
    "how", "why", "something", "anything", "saved", "save", "find", "show", "tell", "give",
    "know", "remember", "recall", "there", "here", "some", "any", "did", "just", "also",
    "atm", "currently", "now", "working",
];

pub fn run(conn: &Connection, cfg: &Config, query: &str) -> Result<()> {
    db::record_event(conn, "search", None, None, &[query.to_string()], None)?;

    let profile = learn::read_profile_context(cfg);
    let profile_lower = profile.as_deref().unwrap_or("").to_lowercase();

    let project_scope = project_scope(conn, cfg, query);
    if let Some(ref scope) = project_scope {
        if wants_project_context(query)
            && !crate::memory::project_progress_documented(cfg, &scope.name)
        {
            let _ = crate::memory::record_project_snapshot(cfg, &scope.name, &scope.tasks);
        }
    }

    let memory_hits = crate::memory::search(cfg, query);
    let has_memory = !memory_hits.is_empty() || memory_has_content(cfg);
    let hits = gather_item_hits(conn, cfg, query, &profile_lower, has_memory);
    let mut tasks = relevant_tasks(conn, query);
    if let Some(ref scope) = project_scope {
        for t in &scope.tasks {
            if !tasks.iter().any(|x| x.uuid == t.uuid) {
                tasks.push(t.clone());
            }
        }
    }

    let context = build_context(cfg, &memory_hits, &hits, &tasks, project_scope.as_ref());
    let system = search_system_prompt(profile.as_deref());
    let user = format!(
        "Context from my store:\n\n{context}\n\n---\n\nMy question: {query}"
    );

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("{spinner:.cyan} {msg}")
            .unwrap(),
    );
    spinner.set_message("Sara is thinking…");
    spinner.enable_steady_tick(Duration::from_millis(80));

    let provider = llm::build_provider(cfg);
    let answer = provider.chat(&system, &user);
    spinner.finish_and_clear();

    match answer {
        Ok(text) => {
            println!("{text}\n");
            let summary = truncate_answer(&text, 300);
            let _ = crate::memory::append_daily(
                cfg,
                &format!("**Q:** {query}\n**A:** {summary}\n"),
            );
            crate::memory::extract_observations(cfg, query, &text, "ask");
        }
        Err(e) => {
            eprintln!("Sara couldn't answer ({e:#}).");
            if hits.is_empty() && tasks.is_empty() {
                println!("No matching notes, links, or tasks in your store.");
            } else {
                print_sources(cfg, query, &memory_hits, &hits, &tasks, project_scope.as_ref());
            }
            return Ok(());
        }
    }

    if !hits.is_empty() || !tasks.is_empty() || !memory_hits.is_empty() || memory_has_content(cfg) {
        print_sources(cfg, query, &memory_hits, &hits, &tasks, project_scope.as_ref());
    }

    Ok(())
}

fn gather_item_hits(
    conn: &Connection,
    cfg: &Config,
    query: &str,
    profile_lower: &str,
    has_memory: bool,
) -> Vec<SearchHit> {
    let mut by_uuid: HashMap<String, SearchHit> = HashMap::new();

    // Semantic search — skip when memory already covers the question
    if !has_memory {
        if let Ok(qvec) = embed::embed_text(cfg, query) {
            if let Ok(all) = db::all_embeddings(conn) {
                for (uuid, vec) in all {
                    let mut score = embed::cosine_similarity(&qvec, &vec);
                    if let Ok(item) = find_item_by_uuid(conn, &uuid) {
                        score += profile_boost(profile_lower, &item);
                        if score >= 0.35 {
                            merge_hit(&mut by_uuid, uuid, score, item);
                        }
                    }
                }
            }
        }
    }

    // Keyword search (whole-word only)
    if !has_memory {
        for (score, item) in keyword_hits(conn, query, profile_lower) {
            if score >= 0.34 {
                merge_hit(&mut by_uuid, item.uuid.to_string(), score, item);
            }
        }
    }

    // Only pad with recent captures when nothing else matched at all
    if by_uuid.is_empty() && !has_memory {
        if let Ok(all) = db::list_items(conn, None) {
            let mut recent = all;
            recent.sort_by(|a, b| b.modified.cmp(&a.modified));
            for item in recent.into_iter().take(5) {
                merge_hit(&mut by_uuid, item.uuid.to_string(), 0.01, item);
            }
        }
    }

    let mut hits: Vec<_> = by_uuid.into_values().collect();
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits.truncate(8);
    hits
}

fn merge_hit(map: &mut HashMap<String, SearchHit>, uuid: String, score: f32, item: Item) {
    map.entry(uuid)
        .and_modify(|h| {
            if score > h.score {
                h.score = score;
            }
        })
        .or_insert(SearchHit { score, item });
}

fn query_terms(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .filter_map(|w| {
            let cleaned: String = w
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
                .to_lowercase();
            if cleaned.len() >= 2 && !STOP_WORDS.contains(&cleaned.as_str()) {
                Some(cleaned)
            } else {
                None
            }
        })
        .collect()
}

fn keyword_hits(conn: &Connection, query: &str, profile_lower: &str) -> Vec<(f32, Item)> {
    let words = query_terms(query);

    let items = db::list_items(conn, None).unwrap_or_default();
    let mut hits = Vec::new();
    for item in items {
        // Direct handle match: "l1", "n1"
        let handle = item.handle().to_lowercase();
        if query.to_lowercase().contains(&handle) {
            hits.push((2.0 + profile_boost(profile_lower, &item), item));
            continue;
        }

        if words.is_empty() {
            continue;
        }

        let hay = format!(
            "{} {} {} {}",
            item.title,
            item.body,
            item.summary.as_deref().unwrap_or(""),
            item.url.as_deref().unwrap_or("")
        )
        .to_lowercase();
        let matched = words
            .iter()
            .filter(|w| crate::memory::text_contains_word(&hay, w))
            .count();
        if matched > 0 {
            let score = (matched as f32 / words.len() as f32) + profile_boost(profile_lower, &item);
            hits.push((score, item));
        }
    }
    hits.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    hits
}

fn memory_has_content(cfg: &Config) -> bool {
    let lt = crate::memory::read_long_term(cfg);
    let body = lt.trim();
    body.len() > 80 && body.contains("- ")
}

fn relevant_memory_line(cfg: &Config, query: &str) -> Option<String> {
    let lt = crate::memory::read_long_term(cfg);
    let words = query_terms(query);
    if words.is_empty() {
        return None;
    }
    let mut best: Option<(usize, String)> = None;
    for line in lt.lines() {
        let l = line.trim();
        if !l.starts_with("- ") {
            continue;
        }
        if l.contains("Tell Sara") || l.contains("Run `sara learn`") {
            continue;
        }
        let lower = l.to_lowercase();
        let matched = words
            .iter()
            .filter(|w| crate::memory::text_contains_word(&lower, w))
            .count();
        if matched == 0 {
            continue;
        }
        let fact = l[2..].to_string();
        if best.as_ref().is_none_or(|(n, _)| matched > *n) {
            best = Some((matched, fact));
        }
    }
    best.map(|(_, fact)| fact)
}

fn wants_project_context(query: &str) -> bool {
    let q = query.to_lowercase();
    if q.contains("this project")
        || q.contains("the project")
        || q.contains("current project")
        || q.contains("in this repo")
        || q.contains("this repo")
    {
        return true;
    }
    query_terms(query).iter().any(|w| {
        matches!(
            w.as_str(),
            "progress" | "status" | "milestone" | "blocked" | "backlog" | "deadline" | "due"
        )
    })
}

fn project_scope(conn: &Connection, cfg: &Config, query: &str) -> Option<ProjectScope> {
    if !wants_project_context(query) {
        return None;
    }
    let (name, _path) = detect_current_project(conn, cfg).ok()?;
    let tasks = db::list_tasks(conn, Some(&name)).unwrap_or_default();
    if tasks.is_empty() && !wants_project_context(query) {
        return None;
    }
    let profile = db::get_project(conn, &name).ok().flatten();
    Some(ProjectScope {
        name,
        profile,
        tasks,
    })
}

fn relevant_tasks(conn: &Connection, query: &str) -> Vec<Task> {
    let words = query_terms(query);
    if words.is_empty() {
        return Vec::new();
    }
    let mut tasks = db::list_tasks(conn, None).unwrap_or_default();
    tasks.retain(|t| {
        let hay = format!("{} {}", t.description, t.project).to_lowercase();
        words
            .iter()
            .any(|w| crate::memory::text_contains_word(&hay, w))
    });
    tasks.sort_by(|a, b| b.urgency.partial_cmp(&a.urgency).unwrap_or(std::cmp::Ordering::Equal));
    tasks.truncate(5);
    tasks
}

fn build_context(
    cfg: &Config,
    memory_hits: &[crate::memory::MemoryHit],
    hits: &[SearchHit],
    tasks: &[Task],
    project: Option<&ProjectScope>,
) -> String {
    let mut parts = Vec::new();

    if let Some(scope) = project {
        parts.push(format!(
            "Current project (from git repo you are in): **{}**",
            scope.name
        ));
        if let Some(ref p) = scope.profile {
            if let Some(ref goal) = p.goal {
                parts.push(format!("  Goal: {goal}"));
            }
            if let Some(ref stack) = p.stack {
                parts.push(format!("  Stack: {stack}"));
            }
            if let Some(ref notes) = p.notes {
                parts.push(format!("  Notes: {notes}"));
            }
        }
        if scope.tasks.is_empty() {
            parts.push("  No pending tasks in this project.".to_string());
        } else {
            parts.push("  Pending tasks in this project:".to_string());
            for t in &scope.tasks {
                let due = t
                    .due
                    .map(|d| d.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| "no due date".to_string());
                let pri = t
                    .priority
                    .as_ref()
                    .map(|p| p.label().to_string())
                    .unwrap_or_else(|| "-".to_string());
                parts.push(format!(
                    "    [{}] {} — due {due}, priority {pri}, urgency {:.1}, status {}",
                    t.id.unwrap_or(0),
                    t.description,
                    t.urgency,
                    t.status
                ));
            }
        }
        if let Some(status) = crate::memory::project_status_summary(cfg, &scope.name) {
            parts.push(format!("  Recorded project status: {status}"));
        } else {
            parts.push(
                "  No milestones or progress notes captured yet for this project.".to_string(),
            );
        }
        parts.push(String::new());
    }

    if !memory_hits.is_empty() {
        parts.push("Personal memory (highest priority — MEMORY.md & daily notes):".to_string());
        for hit in memory_hits.iter().take(6) {
            if !crate::memory::is_citable_memory_excerpt(&hit.excerpt) {
                continue;
            }
            parts.push(format!("  [{}] {}", hit.source, hit.excerpt));
        }
        parts.push(String::new());
    }

    if hits.is_empty()
        && tasks.is_empty()
        && memory_hits.is_empty()
        && project.is_none()
    {
        return "(Your store has no captured notes, links, or pending tasks yet.)".to_string();
    }

    let strong_hits: Vec<_> = hits.iter().filter(|h| h.score > 0.05).collect();
    let recent_hits: Vec<_> = hits.iter().filter(|h| h.score <= 0.05).collect();

    if !strong_hits.is_empty() {
        parts.push("Matching notes & links:".to_string());
        for hit in &strong_hits {
            parts.push(format_item_block(&hit.item));
        }
    }

    if !recent_hits.is_empty() && memory_hits.is_empty() && strong_hits.is_empty() {
        parts.push(
            "Recent captures (weak match — question did not hit keywords or memory):"
                .to_string(),
        );
        for hit in recent_hits.iter().take(5) {
            parts.push(format_item_block(&hit.item));
        }
    }

    if !tasks.is_empty() {
        parts.push("\nPending tasks:".to_string());
        for t in tasks {
            parts.push(format!(
                "  [{}] {} (project: {}, urgency: {:.1})",
                t.id.unwrap_or(0),
                t.description,
                t.project,
                t.urgency
            ));
        }
    }

    parts.join("\n")
}

fn format_item_block(item: &Item) -> String {
    let mut block = format!("[{}] {} ({})", item.handle(), item.title, item.kind);
    if let Some(ref url) = item.url {
        block.push_str(&format!("\n  URL: {url}"));
    }
    if let Some(ref summary) = item.summary {
        block.push_str(&format!("\n  Summary: {summary}"));
    }
    if !item.tags.is_empty() {
        block.push_str(&format!("\n  Tags: {}", item.tags.join(", ")));
    }
    let body = truncate(&item.body, 400);
    if !body.is_empty() {
        block.push_str(&format!("\n  Content: {body}"));
    }
    block
}

fn print_sources(
    cfg: &Config,
    query: &str,
    memory_hits: &[crate::memory::MemoryHit],
    hits: &[SearchHit],
    tasks: &[Task],
    project: Option<&ProjectScope>,
) {
    let mut lines: Vec<String> = Vec::new();

    if let Some(scope) = project {
        for t in &scope.tasks {
            lines.push(format!(
                "  task {} — {} ({})",
                t.id.unwrap_or(0),
                t.description,
                scope.name
            ));
        }
        if let Some(status) = crate::memory::project_status_summary(cfg, &scope.name) {
            lines.push(format!("  memory — {status}"));
        }
    } else {
        for hit in memory_hits.iter() {
            if !crate::memory::is_citable_memory_excerpt(&hit.excerpt) {
                continue;
            }
            lines.push(format!("  memory {} — {}", hit.source, hit.excerpt));
            if lines.len() >= 5 {
                break;
            }
        }
        if lines.is_empty() || memory_hits.is_empty() {
            if let Some(fact) = relevant_memory_line(cfg, query) {
                lines.push(format!("  memory MEMORY.md — {fact}"));
            }
        }
    }

    let memory_only = !lines.is_empty()
        && lines.iter().all(|l| l.starts_with("  memory "))
        && project.is_none();
    if !memory_only {
        let strong: Vec<_> = hits.iter().filter(|h| h.score > 0.34).collect();
        for hit in strong.iter().take(5) {
            let item = &hit.item;
            lines.push(format!("  {} {} — {}", item.kind, item.handle(), item.title));
        }
        for t in tasks.iter().take(3) {
            if project.is_some_and(|s| s.tasks.iter().any(|pt| pt.uuid == t.uuid)) {
                continue;
            }
            lines.push(format!("  task {} — {}", t.id.unwrap_or(0), t.description));
        }
    }

    if lines.is_empty() {
        return;
    }
    println!("Sources:");
    for line in lines {
        println!("{line}");
    }
}

fn truncate_answer(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

fn profile_boost(profile_lower: &str, item: &Item) -> f32 {
    if profile_lower.is_empty() {
        return 0.0;
    }
    let mut boost = 0.0f32;
    for tag in &item.tags {
        if profile_lower.contains(&tag.to_lowercase()) {
            boost += 0.05;
        }
    }
    boost
}

fn find_item_by_uuid(conn: &Connection, uuid: &str) -> Result<Item> {
    conn.query_row(
        "SELECT uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status
         FROM items WHERE uuid = ?1",
        [uuid],
        |row| {
            let tags_json: String = row.get(6)?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            Ok(Item {
                uuid: uuid::Uuid::parse_str(&row.get::<_, String>(0)?)
                    .unwrap_or_else(|_| uuid::Uuid::new_v4()),
                display_id: row.get(2)?,
                kind: row.get(1)?,
                title: row.get(3)?,
                url: row.get(4)?,
                project: row.get(5)?,
                tags,
                path: Some(row.get(7)?),
                summary: row.get(8)?,
                body: row.get(9)?,
                created: chrono::Utc::now(),
                modified: chrono::Utc::now(),
                status: row.get(12)?,
            })
        },
    )
    .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_long_text() {
        assert_eq!(truncate("hello", 10), "hello");
        assert!(truncate("hello world this is long", 5).ends_with('…'));
    }

    #[test]
    fn query_terms_strips_punctuation_and_stop_words() {
        let terms = query_terms("I saved something with 1001?");
        assert!(terms.contains(&"1001".to_string()));
        assert!(!terms.contains(&"saved".to_string()));
        assert!(!terms.contains(&"something".to_string()));
    }

    #[test]
    fn query_terms_keeps_meaningful_words() {
        let terms = query_terms("rust async patterns");
        assert!(terms.contains(&"rust".to_string()));
        assert!(terms.contains(&"async".to_string()));
    }
}
