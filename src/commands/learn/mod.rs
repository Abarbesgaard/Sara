use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Item;
use crate::infrastructure::project::detect_current_project;

/// Size threshold in characters above which we warn that the text is probably
/// not a distilled paragraph. A full conversation paste defeats the
/// token-minimization premise of the whole feature.
const SIZE_WARN_CHARS: usize = 2000;

/// `sara learn "<text>" [--tag] [-p] [--task] [--file] [--auto-files]`
pub fn run(
    conn: &Connection,
    cfg: &Config,
    text: &str,
    tags: &[String],
    projects: &[String],
    tasks: &[String],
    files: &[String],
    auto_files: bool,
    force: bool,
) -> Result<()> {
    let v = learn_value(conn, cfg, text, tags, projects, tasks, files, auto_files, force)?;
    let label = v["label"].as_str().unwrap_or("m?");
    let uuid  = v["uuid"].as_str().unwrap_or("");
    let body  = v["text"].as_str().unwrap_or("");

    let file_suffix = {
        let fs: Vec<&str> = v["files"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();
        if fs.is_empty() {
            String::new()
        } else {
            format!(" (files: {})", fs.join(", "))
        }
    };
    let task_suffix = {
        let ts = v["linked_tasks"].as_array();
        let parts: Vec<String> = ts
            .map(|a| {
                a.iter()
                    .filter_map(|t| {
                        let id   = t["id"].as_i64()?;
                        let desc = t["description"].as_str()?;
                        let src  = t["source"].as_str().unwrap_or("auto");
                        Some(format!("#{id} {desc} [{src}]"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if parts.is_empty() {
            String::new()
        } else {
            format!(" (tasks: {})", parts.join(", "))
        }
    };

    println!("Learned {label} ({uuid}): {body}{file_suffix}{task_suffix}");
    Ok(())
}

/// Print-free core shared by the CLI `learn` command and the MCP `learn` tool.
pub fn learn_value(
    conn: &Connection,
    cfg: &Config,
    text: &str,
    tags: &[String],
    projects: &[String],
    tasks: &[String],
    files: &[String],
    auto_files: bool,
    force: bool,
) -> Result<Value> {
    let text = text.trim();
    if !force {
        check_size(text)?;
        check_secrets(text)?;
        check_overlap(conn, tags)?;
    }

    let resolved_files = collect_files(files, auto_files)?;
    let item = save(conn, cfg, text, tags, projects, tasks, &resolved_files)?;

    let files_json: Vec<Value> = resolved_files.iter().map(|f| json!(f)).collect();
    let tasks_json: Vec<Value> = item.linked_tasks.iter().map(|(id, desc, src)| json!({
        "id": id.parse::<i64>().unwrap_or(0),
        "description": desc,
        "source": src,
    })).collect();

    Ok(json!({
        "label": format!("m{}", item.display_id.unwrap_or(0)),
        "uuid": &item.uuid.to_string()[..8],
        "text": summarize(text),
        "tags": item.tags,
        "files": files_json,
        "linked_tasks": tasks_json,
    }))
}

/// Detect existing memories whose tags overlap significantly with the new one.
/// Two bands:
///   - Near-duplicate: existing memory shares ALL given tags → warn prominently.
///   - Partial overlap: shares ≥50% (but not all) tags → softer note.
/// Prints warnings to stderr; never blocks the write (use --force to silence).
fn check_overlap(conn: &Connection, tags: &[String]) -> Result<()> {
    if tags.is_empty() {
        return Ok(());
    }
    let normalized: Vec<String> = tags
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    if normalized.is_empty() {
        return Ok(());
    }

    // Build UUID sets per tag, then intersect (near-dupe) and union (any overlap).
    let mut any_union: std::collections::HashSet<uuid::Uuid> = std::collections::HashSet::new();
    let mut all_intersection: Option<std::collections::HashSet<uuid::Uuid>> = None;

    for tag in &normalized {
        let uuids: std::collections::HashSet<uuid::Uuid> = db::find_items_by_tag(conn, tag)?
            .into_iter()
            .filter(|i| i.kind == "memory")
            .map(|i| i.uuid)
            .collect();
        any_union.extend(&uuids);
        all_intersection = Some(match all_intersection {
            Some(existing) => existing.intersection(&uuids).copied().collect(),
            None => uuids,
        });
    }

    let near_dupes: std::collections::HashSet<uuid::Uuid> =
        all_intersection.unwrap_or_default();
    let partial: std::collections::HashSet<uuid::Uuid> = any_union
        .difference(&near_dupes)
        .copied()
        .collect();

    // Only surface partial overlaps if they share ≥50% of the given tags.
    let threshold = ((normalized.len() as f64) * 0.5).ceil() as usize;
    let significant_partial: Vec<uuid::Uuid> = if normalized.len() > 1 {
        partial
            .into_iter()
            .filter(|u| {
                let count = normalized.iter().filter(|t| {
                    db::find_items_by_tag(conn, t)
                        .unwrap_or_default()
                        .iter()
                        .any(|i| &i.uuid == u)
                }).count();
                count >= threshold
            })
            .collect()
    } else {
        vec![]
    };

    if !near_dupes.is_empty() {
        eprintln!(
            "Warning: {} existing {} with identical tags [{}]:",
            near_dupes.len(),
            if near_dupes.len() == 1 { "memory" } else { "memories" },
            normalized.join(", ")
        );
        for u in &near_dupes {
            if let Ok(item) = db::get_item_by_uuid(conn, &u.to_string()) {
                let label = format!("m{}", item.display_id.unwrap_or(0));
                let snippet: String = item.body.chars().take(80).collect();
                eprintln!("  {} — {}", label, snippet.trim());
            }
        }
        eprintln!("Consider updating an existing memory with `sara forget <label>` + re-learn, or pass --force to create anyway.");
    }

    if !significant_partial.is_empty() {
        eprintln!(
            "Note: {} potentially related {} (partial tag overlap — possible contradiction):",
            significant_partial.len(),
            if significant_partial.len() == 1 { "memory" } else { "memories" }
        );
        for u in &significant_partial {
            if let Ok(item) = db::get_item_by_uuid(conn, &u.to_string()) {
                let label = format!("m{}", item.display_id.unwrap_or(0));
                let snippet: String = item.body.chars().take(80).collect();
                eprintln!("  {} — {}", label, snippet.trim());
            }
        }
    }

    Ok(())
}

/// Collect file paths from explicit --file flags and optionally --auto-files.
/// All paths are resolved to absolute. Returns an error if --auto-files is
/// requested but no git root can be found and no --file was given.
fn collect_files(explicit: &[String], auto_files: bool) -> Result<Vec<String>> {
    let mut paths: Vec<String> = explicit
        .iter()
        .map(|p| resolve_to_absolute(Path::new(p)))
        .collect();

    if auto_files {
        match find_git_root(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))) {
            Some(root) => {
                let diff = git_diff_files(&root)?;
                paths.extend(diff);
            }
            None => {
                if explicit.is_empty() {
                    anyhow::bail!(
                        "No git root found — cannot use --auto-files outside a git repository.\n\
                         Pass --file <path> explicitly instead."
                    );
                }
                // git root not found but --file was given — skip auto-files silently
                // (the explicit files are still stored).
            }
        }
    }

    // Deduplicate while preserving order.
    let mut seen = std::collections::HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));
    Ok(paths)
}

/// Walk parent directories looking for a `.git` entry.
fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut dir = start.to_path_buf();
    loop {
        if dir.join(".git").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Run `git diff --name-only HEAD` in `git_root` and return absolute paths.
fn git_diff_files(git_root: &Path) -> Result<Vec<String>> {
    let output = std::process::Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(git_root)
        .output()
        .context("running git diff --name-only HEAD")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(|l| resolve_to_absolute(&git_root.join(l)))
        .collect())
}

/// Resolve a path to absolute without requiring it to exist on disk.
fn resolve_to_absolute(path: &Path) -> String {
    if path.is_absolute() {
        path.to_string_lossy().into_owned()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .into_owned()
    }
}

fn save(
    conn: &Connection,
    cfg: &Config,
    text: &str,
    tags: &[String],
    projects: &[String],
    task_prefixes: &[String],
    files: &[String],
) -> Result<Item> {
    if text.is_empty() {
        anyhow::bail!("Memory text cannot be empty");
    }

    // Use the first explicit --task as source_task_uuid (backward compat with
    // item_strength). All tasks (explicit + auto-detected) go into item_task_links.
    let first_task_uuid: Option<Uuid> = match task_prefixes.first() {
        Some(prefix) => Some(
            db::get_task_by_uuid_prefix(conn, prefix)
                .context("looking up --task")?
                .ok_or_else(|| anyhow::anyhow!("No task found for prefix '{prefix}'"))?
                .uuid,
        ),
        None => None,
    };

    let projects: Vec<String> = if projects.is_empty() {
        let (name, _) = detect_current_project(conn, cfg)?;
        vec![name]
    } else {
        projects.to_vec()
    };

    let mut item = Item::new_memory(summarize(text), text.to_string(), first_task_uuid);
    item.tags = tags.to_vec();
    item.path = Some(String::new());

    db::insert_item(conn, &mut item)?;
    db::set_item_projects(conn, &item.uuid, &projects)?;

    // Store file associations.
    if !files.is_empty() {
        db::set_item_files(conn, &item.uuid, files)?;
    }

    // Build task links: auto-detect from file reverse-lookup, then merge explicit.
    let mut task_links: Vec<(Uuid, &'static str)> = vec![];

    for file in files {
        let found = db::find_tasks_by_file(conn, file, false)?;
        for t in found {
            if !task_links.iter().any(|(u, _)| *u == t.uuid) {
                task_links.push((t.uuid, "auto"));
            }
        }
    }

    // Resolve all explicit --task prefixes and merge (explicit wins).
    for prefix in task_prefixes {
        if let Some(t) = db::get_task_by_uuid_prefix(conn, prefix)
            .context("looking up --task")?
        {
            if let Some(pos) = task_links.iter().position(|(u, _)| *u == t.uuid) {
                task_links[pos].1 = "explicit";
            } else {
                task_links.push((t.uuid, "explicit"));
            }
        }
    }

    if !task_links.is_empty() {
        // Convert &'static str slice to owned for set_item_task_links.
        let owned: Vec<(Uuid, &str)> = task_links.iter().map(|(u, s)| (*u, *s)).collect();
        db::set_item_task_links(conn, &item.uuid, &owned)?;

        // Populate item.linked_tasks for the caller to render.
        let links = db::get_item_task_links(conn, &item.uuid)?;
        item.linked_tasks = links
            .into_iter()
            .map(|(t, src)| {
                (
                    t.id.map(|i| i.to_string()).unwrap_or_default(),
                    t.description,
                    src,
                )
            })
            .collect();
    }

    Ok(item)
}

/// A short title for display, taken from the start of the memory text.
fn summarize(text: &str) -> String {
    const MAX: usize = 80;
    let trimmed = text.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        let truncated: String = trimmed.chars().take(MAX).collect();
        format!("{}…", truncated.trim_end())
    }
}

/// Warn (and abort) when the body is too long to be a distilled paragraph.
fn check_size(text: &str) -> Result<()> {
    let len = text.chars().count();
    if len > SIZE_WARN_CHARS {
        anyhow::bail!(
            "Memory body is {len} characters — that looks like a raw conversation paste, \
not a distilled paragraph ({SIZE_WARN_CHARS} char limit).\n\
Distill the key insight into one short paragraph first, then run sara learn again.\n\
To save anyway (not recommended): add --force."
        );
    }
    Ok(())
}

fn check_secrets(text: &str) -> Result<()> {
    if let Some(reason) = detect_secret(text) {
        anyhow::bail!(
            "Memory body may contain a secret ({reason}).\n\
Remove the sensitive value before saving, or add --force to skip this check."
        );
    }
    Ok(())
}

pub(crate) fn detect_secret(text: &str) -> Option<&'static str> {
    let kv_patterns = [
        "api_key", "apikey", "api-key", "secret", "password", "passwd", "token",
        "private_key", "privatekey", "client_secret", "access_key", "accesskey",
        "auth_token", "bearer", "authorization",
    ];
    let lower = text.to_lowercase();
    for kw in &kv_patterns {
        for sep in ["=", ": ", ":\"", "=\""] {
            if lower.contains(&format!("{kw}{sep}")) {
                return Some("key=value assignment pattern");
            }
        }
    }
    if contains_aws_key(text) {
        return Some("AWS-style access key (AKIA…)");
    }
    if contains_high_entropy_token(text) {
        return Some("high-entropy token");
    }
    None
}

fn contains_aws_key(text: &str) -> bool {
    let bytes = text.as_bytes();
    for i in 0..bytes.len().saturating_sub(19) {
        if bytes[i..i + 4] == *b"AKIA" {
            let rest = &bytes[i + 4..i + 20];
            if rest.len() == 16 && rest.iter().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

fn contains_high_entropy_token(text: &str) -> bool {
    for word in text.split_whitespace() {
        let w = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
        if w.len() < 32 || !w.is_ascii() || is_uuid(w) {
            continue;
        }
        let hex_count = w.bytes().filter(|b| b.is_ascii_hexdigit()).count();
        if hex_count * 100 / w.len() > 55 && w.len() >= 32 {
            return true;
        }
    }
    false
}

fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 { return false; }
    let dashes = [8, 13, 18, 23];
    for (i, &byte) in b.iter().enumerate() {
        if dashes.contains(&i) {
            if byte != b'-' { return false; }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_check_passes_under_threshold() {
        assert!(check_size("short text").is_ok());
        assert!(check_size(&"x".repeat(SIZE_WARN_CHARS)).is_ok());
    }

    #[test]
    fn size_check_fails_over_threshold() {
        let long = "x".repeat(SIZE_WARN_CHARS + 1);
        assert!(check_size(&long).is_err());
    }

    #[test]
    fn secret_check_flags_api_key_assignment() {
        assert!(detect_secret("api_key=abc123secret").is_some());
        assert!(detect_secret("apikey: supersecret").is_some());
        assert!(detect_secret("password=\"hunter2\"").is_some());
        assert!(detect_secret("client_secret: my-secret-value").is_some());
    }

    #[test]
    fn secret_check_does_not_flag_normal_prose() {
        assert!(detect_secret("sara recall uses item_tags for exact tag lookup").is_none());
        assert!(detect_secret("the token field is optional").is_none());
        assert!(detect_secret("AuthAppUri (required string)").is_none());
    }

    #[test]
    fn secret_check_flags_aws_style_key() {
        assert!(detect_secret("key: AKIAIOSFODNN7EXAMPLE").is_some());
    }

    #[test]
    fn secret_check_flags_high_entropy_hex_token() {
        // 64-char hex string — looks like a SHA256 hash or access token
        assert!(
            detect_secret(
                "token: a3f1b2c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2"
            )
            .is_some()
        );
    }

    #[test]
    fn secret_check_does_not_flag_uuid_in_prose() {
        // UUIDs in normal prose (as field *values described*, not assigned) should
        // not trigger — they're short enough (36 chars with dashes) and mixed
        // alphanumeric so the hex-ratio heuristic won't catch them.
        assert!(
            detect_secret(
                "AuthAppUri is a UUID like a2923ccd-a496-4cbd-9673-f40156552a92 in the config"
            )
            .is_none()
        );
    }
}
