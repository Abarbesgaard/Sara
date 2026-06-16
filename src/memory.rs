//! Sara's memory layer — the core differentiator.
//!
//! OpenClaw-inspired: markdown on disk, loaded every turn, searched hybrid,
//! auto-extracted from captures and conversations, consolidated via `sara learn`.

use anyhow::{Context, Result};
use chrono::{Duration, Local, NaiveDate};
use rusqlite::Connection;
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::config::Config;

const MEMORY_BUDGET: usize = 8000;
const DAILY_DAYS_LOADED: i64 = 7;
const DAILY_PER_DAY_BUDGET: usize = 1200;

/// Whole-word match — avoids "project" matching inside "program".
pub fn text_contains_word(hay: &str, word: &str) -> bool {
    hay.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|token| token == word.to_lowercase())
}

pub fn is_session_echo(line: &str) -> bool {
    let l = line.trim();
    l.starts_with("**Q:**")
        || l.starts_with("**Q:")
        || l.starts_with("**A:**")
        || l.starts_with("**A:")
}

#[derive(Debug, Clone)]
pub struct MemoryHit {
    pub source: String,
    pub score: f32,
    pub excerpt: String,
}

pub fn init_scaffold(store: &Path) -> Result<()> {
    fs::create_dir_all(store.join("memory"))?;
    let memory_md = store.join(".sara/MEMORY.md");
    if !memory_md.exists() {
        fs::write(
            &memory_md,
            "---\ntype: memory\n---\n\n# Long-term memory\n\n\
             _Durable facts and preferences. Sara loads this on every interaction._\n\n\
             - Tell Sara: `sara remember \"I prefer Rust for CLI tools\"`\n\
             - Run `sara learn` to consolidate daily notes into this file\n",
        )?;
    }
    let today = daily_path_for(store, Local::now().date_naive());
    if !today.exists() {
        fs::write(&today, daily_header(Local::now().date_naive()))?;
    }
    Ok(())
}

fn daily_header(date: NaiveDate) -> String {
    format!("# {date}\n\n_Daily notes — captures, Q&A, observations._\n\n")
}

fn store_path(cfg: &Config) -> Option<PathBuf> {
    crate::config::vault_path(cfg).ok()
}

pub fn long_term_path(cfg: &Config) -> Option<PathBuf> {
    store_path(cfg).map(|s| s.join(".sara/MEMORY.md"))
}

fn daily_path_for(store: &Path, date: NaiveDate) -> PathBuf {
    store.join("memory").join(format!("{date}.md"))
}

/// All memory files for indexing / search.
pub fn all_memory_files(cfg: &Config) -> Vec<(String, PathBuf)> {
    let Some(store) = store_path(cfg) else {
        return Vec::new();
    };
    let mut files = vec![("MEMORY.md".to_string(), store.join(".sara/MEMORY.md"))];
    if let Ok(entries) = fs::read_dir(store.join("memory")) {
        let mut dailies: Vec<_> = entries
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.path()
                    .extension()
                    .is_some_and(|x| x == "md")
            })
            .map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                (name, e.path())
            })
            .collect();
        dailies.sort_by(|a, b| b.0.cmp(&a.0));
        files.extend(dailies);
    }
    files
}

/// Load long-term + recent daily notes for every LLM call.
pub fn read_memory_context(cfg: &Config) -> Option<String> {
    let store = store_path(cfg)?;
    let mut parts = Vec::new();
    let mut budget = MEMORY_BUDGET;

    if let Ok(lt) = fs::read_to_string(store.join(".sara/MEMORY.md")) {
        let body = strip_frontmatter(&lt);
        if !body.trim().is_empty() {
            let chunk = truncate_chars(&body, budget.min(3000));
            budget = budget.saturating_sub(chunk.len());
            parts.push(format!("## Long-term memory\n{chunk}"));
        }
    }

    let today = Local::now().date_naive();
    for offset in 0..DAILY_DAYS_LOADED {
        if budget < 200 {
            break;
        }
        let date = today - Duration::days(offset);
        let path = daily_path_for(&store, date);
        if let Ok(content) = fs::read_to_string(&path) {
            let body = strip_frontmatter(&content);
            let trimmed = body.trim();
            if trimmed.len() > 40 {
                let chunk = truncate_chars(trimmed, budget.min(DAILY_PER_DAY_BUDGET));
                budget = budget.saturating_sub(chunk.len());
                parts.push(format!("## Daily notes ({date})\n{chunk}"));
            }
        }
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

/// Keyword search across MEMORY.md and daily notes.
pub fn search(cfg: &Config, query: &str) -> Vec<MemoryHit> {
    let words = query_terms(query);
    if words.is_empty() {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for (label, path) in all_memory_files(cfg) {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let body = strip_frontmatter(&content);
        let hay = body.to_lowercase();
        let matched = words
            .iter()
            .filter(|w| text_contains_word(&hay, w))
            .count();
        if matched == 0 {
            continue;
        }
        let score = matched as f32 / words.len() as f32;
        let excerpt = extract_excerpt(&body, &words, 400);
        if !is_citable_memory_excerpt(&excerpt) {
            continue;
        }
        hits.push(MemoryHit {
            source: label,
            score,
            excerpt,
        });
    }
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    hits
}

fn query_terms(query: &str) -> Vec<String> {
    const STOP: &[&str] = &[
        "a", "an", "the", "i", "my", "me", "is", "are", "was", "were", "be", "been", "being",
        "have", "has", "had", "do", "does", "did", "will", "would", "could", "should", "can",
        "with", "about", "for", "from", "into", "that", "this", "what", "when", "where", "who",
        "how", "why", "something", "anything", "saved", "save", "find", "show", "tell", "give",
        "know", "remember", "recall", "there", "here", "some", "any", "just", "also",
    ];
    query
        .split_whitespace()
        .filter_map(|w| {
            let cleaned: String = w
                .trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
                .to_lowercase();
            if cleaned.len() >= 2 && !STOP.contains(&cleaned.as_str()) {
                Some(cleaned)
            } else {
                None
            }
        })
        .collect()
}

fn is_noise_line(line: &str) -> bool {
    let l = line.trim();
    if l.is_empty() {
        return true;
    }
    if is_session_echo(l) {
        return true;
    }
    if l.starts_with("# 20") {
        return true;
    }
    if l.starts_with('_') && l.ends_with('_') && l.len() > 2 {
        return true;
    }
    if l.starts_with("### Observations") {
        return true;
    }
    l.contains("**Q:**") || l.contains("**A:**")
}

pub fn is_citable_memory_excerpt(excerpt: &str) -> bool {
    if excerpt.is_empty() {
        return false;
    }
    if is_session_echo(excerpt) {
        return false;
    }
    if excerpt.contains("**Q:**") || excerpt.contains("**A:**") {
        return false;
    }
    if excerpt.contains("### Observations") {
        return false;
    }
    if excerpt.starts_with("# 20") {
        return false;
    }
    excerpt.lines().count() <= 4
}

fn extract_project_snapshot_excerpt(body: &str, max: usize) -> Option<String> {
    let mut in_snap = false;
    let mut lines = Vec::new();
    for line in body.lines() {
        if line.starts_with("### Project snapshot:") {
            in_snap = true;
            lines.push(line.trim().to_string());
            continue;
        }
        if in_snap {
            if line.starts_with("### ") && !line.starts_with("### Project snapshot:") {
                break;
            }
            let t = line.trim();
            if is_session_echo(t) {
                break;
            }
            if t.is_empty() || t.starts_with("_(") {
                continue;
            }
            lines.push(t.to_string());
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(truncate_chars(&lines.join(" "), max))
    }
}

fn extract_excerpt(body: &str, words: &[String], max: usize) -> String {
    if let Some(snap) = extract_project_snapshot_excerpt(body, max) {
        return snap;
    }
    for line in body.lines() {
        if is_noise_line(line) {
            continue;
        }
        let lower = line.to_lowercase();
        if words.iter().any(|w| text_contains_word(&lower, w)) {
            return truncate_chars(line.trim(), max);
        }
    }
    for line in body.lines() {
        if is_noise_line(line) {
            continue;
        }
        if line.trim().starts_with("- ") {
            return truncate_chars(line.trim(), max);
        }
    }
    String::new()
}

fn project_names_match(a: &str, b: &str) -> bool {
    fn norm(s: &str) -> String {
        s.to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect()
    }
    let na = norm(a);
    let nb = norm(b);
    na == nb || na.contains(&nb) || nb.contains(&na)
}

/// Compact status line for LLM context and Sources (MEMORY.md + latest snapshot).
pub fn project_status_summary(cfg: &Config, project: &str) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(line) = project_memory_line(cfg, project) {
        parts.push(line);
    }
    if let Some(snap) = project_daily_snapshot(cfg, project) {
        parts.push(snap);
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

pub fn project_memory_line(cfg: &Config, project: &str) -> Option<String> {
    memory_line_for_project(&read_long_term(cfg), project)
}

fn memory_line_for_project(body: &str, project: &str) -> Option<String> {
    for line in body.lines() {
        let l = line.trim();
        if !l.starts_with("- ") {
            continue;
        }
        let fact = l[2..].trim();
        let name = fact.split(':').next().unwrap_or("").trim();
        if project_names_match(name, project) {
            return Some(fact.to_string());
        }
    }
    None
}

pub fn project_daily_snapshot(cfg: &Config, project: &str) -> Option<String> {
    let store = store_path(cfg)?;
    let today = Local::now().date_naive();
    for offset in 0..14 {
        let date = today - Duration::days(offset);
        let path = daily_path_for(&store, date);
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let body = strip_frontmatter(&content);
        if !body.contains(&format!("### Project snapshot: {project}")) {
            continue;
        }
        let snap = extract_project_snapshot_excerpt(&body, 300)?;
        if is_citable_memory_excerpt(&snap) {
            return Some(snap);
        }
    }
    None
}

pub fn remember(cfg: &Config, text: &str) -> Result<()> {
    let path = long_term_path(cfg).context("Sara store not initialized")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    if !path.exists() {
        init_scaffold(path.parent().unwrap().parent().unwrap())?;
    }
    let mut content = fs::read_to_string(&path).unwrap_or_default();
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(&format!("- {text}\n"));
    fs::write(&path, content)?;
    println!("Remembered in {}", path.display());
    Ok(())
}

pub fn append_daily(cfg: &Config, line: &str) -> Result<()> {
    let store = store_path(cfg).context("Sara store not initialized")?;
    init_scaffold(&store)?;
    let today = Local::now().date_naive();
    let path = daily_path_for(&store, today);
    let mut content = if path.exists() {
        fs::read_to_string(&path)?
    } else {
        daily_header(today)
    };
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(line);
    if !line.ends_with('\n') {
        content.push('\n');
    }
    fs::write(&path, content)?;
    Ok(())
}

/// After a project progress check, record what Sara knows (so next ask has context).
pub fn record_project_snapshot(
    cfg: &Config,
    project: &str,
    tasks: &[crate::model::Task],
) -> Result<()> {
    let today = Local::now().date_naive();
    let pending = format_pending_tasks(tasks);

    if daily_has_project_snapshot(cfg, project, today) {
        if snapshot_pending_matches(cfg, project, today, &pending) {
            upsert_memory_project_line(cfg, project, &pending, today)?;
            return Ok(());
        }
        replace_daily_snapshot(cfg, project, today, &pending)?;
        upsert_memory_project_line(cfg, project, &pending, today)?;
        eprintln!("Updated project state for '{project}' in memory.");
        return Ok(());
    }

    append_daily(
        cfg,
        &format!(
            "\n### Project snapshot: {project} ({today})\n\
             - Progress/milestones documented: no\n\
             - Pending tasks: {pending}\n\
             - _(Auto-recorded by Sara.)_\n"
        ),
    )?;
    upsert_memory_project_line(cfg, project, &pending, today)?;
    eprintln!("Noted project state for '{project}' in memory.");
    Ok(())
}

fn snapshot_pending_matches(
    cfg: &Config,
    project: &str,
    date: NaiveDate,
    pending: &str,
) -> bool {
    read_snapshot_pending(cfg, project, date)
        .is_some_and(|existing| existing == pending)
}

fn read_snapshot_pending(cfg: &Config, project: &str, date: NaiveDate) -> Option<String> {
    let store = store_path(cfg)?;
    let path = daily_path_for(&store, date);
    let content = fs::read_to_string(&path).ok()?;
    let body = strip_frontmatter(&content);
    let marker = format!("### Project snapshot: {project}");
    let after = body.find(&marker)?;
    for line in body[after..].lines() {
        let t = line.trim();
        if t.starts_with("- Pending tasks:") {
            return Some(t["- Pending tasks:".len()..].trim().to_string());
        }
        if t.starts_with("### ") && !t.starts_with(&marker) {
            break;
        }
    }
    None
}

fn replace_daily_snapshot(
    cfg: &Config,
    project: &str,
    date: NaiveDate,
    pending: &str,
) -> Result<()> {
    let store = store_path(cfg).context("Sara store not initialized")?;
    let path = daily_path_for(&store, date);
    let content = fs::read_to_string(&path)?;
    let marker = format!("### Project snapshot: {project}");
    let Some(start) = content.find(&marker) else {
        return Ok(());
    };
    let rest = &content[start..];
    let end = rest[marker.len()..]
        .find("\n### ")
        .map(|i| start + marker.len() + i)
        .unwrap_or(content.len());
    let new_block = format!(
        "### Project snapshot: {project} ({date})\n\
         - Progress/milestones documented: no\n\
         - Pending tasks: {pending}\n\
         - _(Auto-recorded by Sara.)_\n"
    );
    let mut updated = String::new();
    updated.push_str(&content[..start]);
    updated.push_str(&new_block);
    updated.push_str(&content[end..]);
    fs::write(&path, updated)?;
    Ok(())
}

fn format_pending_tasks(tasks: &[crate::model::Task]) -> String {
    if tasks.is_empty() {
        return "none".to_string();
    }
    tasks
        .iter()
        .map(|t| {
            let due = t
                .due
                .map(|d| d.format("%Y-%m-%d").to_string())
                .unwrap_or_else(|| "no due".to_string());
            format!(
                "#{} {} (due {due})",
                t.id.unwrap_or(0),
                t.description
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn daily_has_project_snapshot(cfg: &Config, project: &str, date: NaiveDate) -> bool {
    let Some(store) = store_path(cfg) else {
        return false;
    };
    let path = daily_path_for(&store, date);
    let Ok(content) = fs::read_to_string(&path) else {
        return false;
    };
    content.contains(&format!("### Project snapshot: {project}"))
}

/// True when MEMORY.md or daily notes record real milestones (not just an empty snapshot).
pub fn project_progress_documented(cfg: &Config, project: &str) -> bool {
    if let Some(line) = project_memory_line(cfg, project) {
        if !line.contains("no progress milestones captured yet") {
            return true;
        }
    }
    let Some(store) = store_path(cfg) else {
        return false;
    };
    let today = Local::now().date_naive();
    for offset in 0..30 {
        let date = today - Duration::days(offset);
        let path = daily_path_for(&store, date);
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        if content.contains(&format!("### Project snapshot: {project}"))
            && content.contains("Progress/milestones documented: yes")
        {
            return true;
        }
    }
    false
}

fn upsert_memory_project_line(
    cfg: &Config,
    project: &str,
    pending: &str,
    date: NaiveDate,
) -> Result<()> {
    let path = long_term_path(cfg).context("Sara store not initialized")?;
    let body = strip_frontmatter(&fs::read_to_string(&path).unwrap_or_default());
    let new_line = format!(
        "- {project}: no progress milestones captured yet; pending: {pending}. Last checked {date}."
    );

    let mut lines: Vec<String> = body
        .lines()
        .filter(|l| {
            let t = l.trim();
            if !t.starts_with("- ") {
                return true;
            }
            let name = t[2..].split(':').next().unwrap_or("").trim();
            !project_names_match(name, project)
        })
        .map(|l| l.to_string())
        .collect();
    lines.push(new_line);
    let new_body = lines.join("\n");
    write_long_term(cfg, &new_body)?;
    Ok(())
}

/// Log a capture to today's daily note.
pub fn observe_capture(cfg: &Config, kind: &str, handle: &str, title: &str, summary: Option<&str>) {
    let line = if let Some(s) = summary {
        format!("- **Captured {kind} {handle}:** {title} — _{s}_\n")
    } else {
        format!("- **Captured {kind} {handle}:** {title}\n")
    };
    let _ = append_daily(cfg, &line);
}

/// After ask/capture: LLM extracts durable observations into daily notes.
pub fn extract_observations(cfg: &Config, user_text: &str, assistant_text: &str, source: &str) {
    let system = "You are Sara's memory extractor. From the interaction below, list 0-3 NEW durable \
        facts about the user worth remembering (preferences, projects, decisions, relationships). \
        Output markdown bullet points only. One fact per line starting with '- '. \
        If nothing new worth remembering, output exactly: NONE";
    let user = format!(
        "Source: {source}\n\nUser: {user_text}\n\nAssistant: {}\n\nFacts:",
        truncate_chars(assistant_text, 800)
    );
    let provider = crate::llm::build_provider(cfg);
    if let Ok(facts) = provider.chat(system, &user) {
        let facts = facts.trim();
        if facts.is_empty() || facts.eq_ignore_ascii_case("none") || facts.contains("NONE") && facts.len() < 20 {
            return;
        }
        let block = format!("\n### Observations ({source})\n{facts}\n");
        let _ = append_daily(cfg, &block);
    }
}

/// Index memory file chunks for semantic search (stable UUID per chunk).
pub fn index_embeddings(conn: &Connection, cfg: &Config) -> Result<usize> {
    let ns = Uuid::NAMESPACE_URL;
    let mut count = 0;
    for (label, path) in all_memory_files(cfg) {
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let body = strip_frontmatter(&content);
        for (i, chunk) in chunk_text(&body, 500).iter().enumerate() {
            if chunk.trim().len() < 30 {
                continue;
            }
            let id = Uuid::new_v5(&ns, format!("memory:{label}:{i}").as_bytes());
            let text = format!("[{label}] {chunk}");
            if let Ok(vec) = crate::embed::embed_text(cfg, &text) {
                let _ = crate::db::upsert_embedding(conn, &id, &vec);
                count += 1;
            }
        }
    }
    Ok(count)
}

fn chunk_text(text: &str, size: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    for para in text.split("\n\n") {
        if current.len() + para.len() > size && !current.is_empty() {
            chunks.push(current.trim().to_string());
            current.clear();
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(para);
    }
    if !current.trim().is_empty() {
        chunks.push(current.trim().to_string());
    }
    chunks
}

pub fn recent_daily_notes(cfg: &Config, days: i64) -> Vec<(NaiveDate, String)> {
    let Some(store) = store_path(cfg) else {
        return Vec::new();
    };
    let today = Local::now().date_naive();
    let mut notes = Vec::new();
    for offset in 0..days {
        let date = today - Duration::days(offset);
        let path = daily_path_for(&store, date);
        if let Ok(content) = fs::read_to_string(&path) {
            let body = strip_frontmatter(&content).trim().to_string();
            if body.len() > 40 {
                notes.push((date, body));
            }
        }
    }
    notes
}

pub fn read_long_term(cfg: &Config) -> String {
    long_term_path(cfg)
        .and_then(|p| fs::read_to_string(p).ok())
        .map(|s| strip_frontmatter(&s))
        .unwrap_or_default()
}

pub fn write_long_term(cfg: &Config, body: &str) -> Result<()> {
    let path = long_term_path(cfg).context("Sara store not initialized")?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = format!("---\ntype: memory\n---\n\n{body}");
    fs::write(&path, content)?;
    Ok(())
}

fn strip_frontmatter(content: &str) -> String {
    if content.starts_with("---") {
        if let Some(rest) = content.strip_prefix("---") {
            if let Some((_, body)) = rest.split_once("---") {
                return body.trim().to_string();
            }
        }
    }
    content.to_string()
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(max).collect::<String>())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_yaml_frontmatter() {
        let raw = "---\ntype: memory\n---\n\nHello";
        assert_eq!(strip_frontmatter(raw), "Hello");
    }

    #[test]
    fn query_terms_strips_punctuation() {
        let terms = query_terms("I saved something with 1001?");
        assert!(terms.contains(&"1001".to_string()));
    }

    #[test]
    fn chunks_paragraphs() {
        let chunks = chunk_text("a\n\nb\n\nc", 5);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn whole_word_does_not_match_substrings() {
        assert!(!text_contains_word("program sara", "project"));
        assert!(text_contains_word("building pling backend", "building"));
    }

    #[test]
    fn session_echo_skips_q_and_a_lines() {
        assert!(is_session_echo("**Q:** what is progress"));
        assert!(is_session_echo("**A:** no information yet"));
        assert!(!is_session_echo("- pling-backend: pending task #5"));
    }

    #[test]
    fn citable_excerpt_rejects_daily_dump() {
        assert!(!is_citable_memory_excerpt("# 2026-06-16\n\n**Q:** hello"));
        assert!(is_citable_memory_excerpt(
            "### Project snapshot: pling-backend - Progress/milestones documented: no"
        ));
    }
}
