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
/// `sara learn "<text>" [--tag] [-p] [--task] [--file] [--auto-files] [--supersedes <label>]`
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
    supersedes: &[String],
    derived_from: &[String],
    similar_to: &[String],
) -> Result<()> {
    let v = learn_value(conn, cfg, text, tags, projects, tasks, files, auto_files, force, supersedes, derived_from, similar_to)?;
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
    if let Some(links) = v["superseded"].as_array() {
        for link in links {
            if let Some(old_label) = link.as_str() {
                println!("  ↳ supersedes {old_label}");
            }
        }
    }
    if let Some(links) = v["derived_from"].as_array() {
        for link in links {
            if let Some(canon_label) = link.as_str() {
                println!("  ↳ derived from {canon_label}");
            }
        }
    }
    if let Some(links) = v["similar_to"].as_array() {
        for link in links {
            if let Some(other_label) = link.as_str() {
                println!("  ↳ similar to {other_label}");
            }
        }
    }
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
    supersedes: &[String],
    derived_from: &[String],
    similar_to: &[String],
) -> Result<Value> {
    let text = text.trim();
    let resolved_files = collect_files(files, auto_files)?;
    if !force {
        crate::infrastructure::safety::check_size(text)?;
        crate::infrastructure::safety::check_secrets(text)?;
        check_overlap(conn, tags, &resolved_files)?;
    }

    let item = save(conn, cfg, text, tags, projects, tasks, &resolved_files)?;

    let new_uuid = item.uuid.to_string();

    // Resolve and insert typed links atomically with the learn. `supersedes` and
    // `derived_from` point FROM the new memory TO the existing one; `similar_to`
    // is symmetric intent but stored new→existing. An unresolvable label warns
    // and is skipped — it never aborts the learn.
    let resolve_and_link =
        |handles: &[String], relation: &str, flag: &str| -> Result<Vec<String>> {
            let mut labels: Vec<String> = Vec::new();
            for handle in handles {
                match db::get_item_by_handle(conn, handle) {
                    Ok(target) => {
                        db::insert_memory_link(
                            conn,
                            &new_uuid,
                            &target.uuid.to_string(),
                            relation,
                            1.0,
                        )?;
                        let target_label = target
                            .display_id
                            .map(|id| format!("m{id}"))
                            .unwrap_or_else(|| handle.clone());
                        labels.push(target_label);
                    }
                    Err(e) => {
                        eprintln!(
                            "warning: could not resolve memory '{}' for {flag}: {e}",
                            handle
                        );
                    }
                }
            }
            Ok(labels)
        };

    let superseded_labels = resolve_and_link(supersedes, "supersedes", "--supersedes")?;
    let derived_from_labels = resolve_and_link(derived_from, "derived_from", "--derived-from")?;
    let similar_to_labels = resolve_and_link(similar_to, "similar_to", "--similar-to")?;

    let files_json: Vec<Value> = resolved_files.iter().map(|f| json!(f)).collect();
    let tasks_json: Vec<Value> = item.linked_tasks.iter().map(|(id, desc, src)| json!({
        "id": id.parse::<i64>().unwrap_or(0),
        "description": desc,
        "source": src,
    })).collect();

    Ok(json!({
        "label": format!("m{}", item.display_id.unwrap_or(0)),
        "uuid": &new_uuid[..8],
        "text": summarize(text),
        "tags": item.tags,
        "files": files_json,
        "linked_tasks": tasks_json,
        "superseded": superseded_labels,
        "derived_from": derived_from_labels,
        "similar_to": similar_to_labels,
    }))
}

/// Build the copy-pasteable typed-link suggestion for a near-duplicate overlap
/// (existing memory shares ALL of the new memory's tags). A near-dupe is either
/// a specialisation (→ `derived_from`) or a replacement (→ `supersedes`).
pub(crate) fn near_dupe_suggestion(label: &str) -> String {
    format!(
        "→ Link it typed: re-run with `--derived-from {label}` if this specialises it, \
         or `--supersedes {label}` if it replaces it (or --force to keep both)."
    )
}

/// Build the copy-pasteable typed-link suggestion for a partial overlap
/// (shares ≥50% but not all tags). A partial overlap is a lateral relation
/// (→ `similar_to`).
pub(crate) fn partial_overlap_suggestion(label: &str) -> String {
    format!(
        "→ Link it typed: re-run with `--similar-to {label}` to connect them in the memory graph."
    )
}

/// Detect existing memories whose tags overlap significantly with the new one,
/// and memories that share any of the same file links (even with different tags).
/// Two tag-overlap bands:
///   - Near-duplicate: existing memory shares ALL given tags → warn prominently.
///   - Partial overlap: shares ≥50% (but not all) tags → softer note.
/// File-overlap: any memory already linked to one of the new memory's files → warn.
/// Prints warnings to stderr; never blocks the write (use --force to silence).
pub(crate) fn check_overlap(conn: &Connection, tags: &[String], files: &[String]) -> Result<()> {
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
    let mut per_tag_sets: Vec<std::collections::HashSet<uuid::Uuid>> = Vec::new();

    for tag in &normalized {
        let uuids: std::collections::HashSet<uuid::Uuid> = db::find_items_by_tag(conn, tag)?
            .into_iter()
            .filter(|i| i.kind == "memory")
            .map(|i| i.uuid)
            .collect();
        any_union.extend(&uuids);
        all_intersection = Some(match all_intersection {
            Some(existing) => existing.intersection(&uuids).copied().collect(),
            None => uuids.clone(),
        });
        per_tag_sets.push(uuids);
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
                let count = per_tag_sets.iter().filter(|set| set.contains(u)).count();
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
        let mut labels: Vec<String> = Vec::new();
        for u in &near_dupes {
            if let Ok(item) = db::get_item_by_uuid(conn, &u.to_string()) {
                let label = format!("m{}", item.display_id.unwrap_or(0));
                let snippet: String = item.body.chars().take(80).collect();
                eprintln!("  {} — {}", label, snippet.trim());
                labels.push(label);
            }
        }
        // Offer a concrete, copy-pasteable typed link rather than a vague note.
        if let Some(first) = labels.first() {
            eprintln!("{}", near_dupe_suggestion(first));
        }
    }

    if !significant_partial.is_empty() {
        eprintln!(
            "Note: {} potentially related {} (partial tag overlap):",
            significant_partial.len(),
            if significant_partial.len() == 1 { "memory" } else { "memories" }
        );
        let mut labels: Vec<String> = Vec::new();
        for u in &significant_partial {
            if let Ok(item) = db::get_item_by_uuid(conn, &u.to_string()) {
                let label = format!("m{}", item.display_id.unwrap_or(0));
                let snippet: String = item.body.chars().take(80).collect();
                eprintln!("  {} — {}", label, snippet.trim());
                labels.push(label);
            }
        }
        if let Some(first) = labels.first() {
            eprintln!("{}", partial_overlap_suggestion(first));
        }
    }

    // File-overlap check: warn for any existing memory sharing a file path.
    if !files.is_empty() {
        let mut file_overlap: Vec<(String, uuid::Uuid)> = Vec::new();
        for path in files {
            let matches = db::find_items_by_file(conn, path, false)?;
            for item in matches {
                if item.kind == "memory"
                    && !file_overlap.iter().any(|(_, u)| *u == item.uuid)
                    && !near_dupes.contains(&item.uuid)
                {
                    file_overlap.push((path.clone(), item.uuid));
                }
            }
        }
        if !file_overlap.is_empty() {
            eprintln!(
                "Note: {} existing {} linked to the same file(s) — consider --supersedes or sara relearn:",
                file_overlap.len(),
                if file_overlap.len() == 1 { "memory" } else { "memories" }
            );
            for (path, u) in &file_overlap {
                if let Ok(item) = db::get_item_by_uuid(conn, &u.to_string()) {
                    let label = format!("m{}", item.display_id.unwrap_or(0));
                    let snippet: String = item.body.chars().take(80).collect();
                    let short_path = std::path::Path::new(path)
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(path.as_str());
                    eprintln!("  {} [{short_path}] — {}", label, snippet.trim());
                }
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

#[cfg(test)]
mod tests {
    use crate::infrastructure::safety;

    #[test]
    fn size_check_passes_under_threshold() {
        assert!(safety::check_size("short text").is_ok());
        assert!(safety::check_size(&"x".repeat(safety::SIZE_LIMIT_CHARS)).is_ok());
    }

    #[test]
    fn size_check_fails_over_threshold() {
        let long = "x".repeat(safety::SIZE_LIMIT_CHARS + 1);
        assert!(safety::check_size(&long).is_err());
    }

    #[test]
    fn secret_check_flags_api_key_assignment() {
        assert!(safety::detect_secret("api_key=abc123secret").is_some());
        assert!(safety::detect_secret("apikey: supersecret").is_some());
        assert!(safety::detect_secret("client_secret: my-secret-value").is_some());
    }

    #[test]
    fn secret_check_does_not_flag_normal_prose() {
        assert!(safety::detect_secret("sara recall uses item_tags for exact tag lookup").is_none());
        assert!(safety::detect_secret("the token field is optional").is_none());
        assert!(safety::detect_secret("AuthAppUri (required string)").is_none());
    }

    #[test]
    fn secret_check_flags_aws_style_key() {
        assert!(safety::detect_secret("key: AKIAIOSFODNN7EXAMPLE").is_some());
    }

    #[test]
    fn secret_check_flags_high_entropy_hex_token() {
        assert!(
            safety::detect_secret(
                "token: a3f1b2c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2"
            )
            .is_some()
        );
    }

    #[test]
    fn secret_check_does_not_flag_uuid_in_prose() {
        assert!(
            safety::detect_secret(
                "AuthAppUri is a UUID like a2923ccd-a496-4cbd-9673-f40156552a92 in the config"
            )
            .is_none()
        );
    }

    #[test]
    fn learn_value_supersedes_inserts_link() {
        use crate::infrastructure::{config::Config, db};
        let conn = db::open_in_memory_for_test();
        let cfg = Config::default();

        // Learn a first memory to supersede.
        let old = super::learn_value(
            &conn, &cfg, "old finding", &["tag-a".to_string()], &[], &[], &[], false, true, &[], &[], &[],
        )
        .unwrap();
        let old_label = old["label"].as_str().unwrap().to_string();

        // Learn a new memory that supersedes the old one.
        let new_v = super::learn_value(
            &conn,
            &cfg,
            "new finding supersedes old",
            &["tag-a".to_string()],
            &[],
            &[],
            &[],
            false,
            true,
            &[old_label.clone()],
            &[],
            &[],
        )
        .unwrap();

        // The superseded array should contain the old label.
        let superseded = new_v["superseded"].as_array().unwrap();
        assert_eq!(superseded.len(), 1);
        assert_eq!(superseded[0].as_str().unwrap(), old_label);
    }

    #[test]
    fn check_overlap_warns_on_file_overlap() {
        use crate::infrastructure::{config::Config, db};
        let conn = db::open_in_memory_for_test();
        let cfg = Config::default();

        let file_path = "/tmp/test_sara_overlap_check.rs".to_string();

        // Learn first memory tied to the file.
        super::learn_value(
            &conn,
            &cfg,
            "first memory about this file",
            &["tag-x".to_string()],
            &[],
            &[],
            &[file_path.clone()],
            false,
            true, // force — skip safety guardrails
            &[],
            &[],
            &[],
        )
        .unwrap();

        // Learning a second memory on the same file with DIFFERENT tags should
        // still succeed (file-overlap is advisory, not blocking). The test just
        // verifies check_overlap() itself doesn't error out.
        let result = super::check_overlap(&conn, &["tag-y".to_string()], &[file_path.clone()]);
        assert!(result.is_ok(), "check_overlap should not error on file overlap");
    }

    #[test]
    fn learn_creates_typed_links() {
        use crate::infrastructure::{config::Config, db};
        let conn = db::open_in_memory_for_test();
        let cfg = Config::default();

        // A canonical memory and a lateral one to link against.
        let canon = super::learn_value(
            &conn, &cfg, "canonical pattern", &["pat".to_string()], &[], &[], &[], false, true, &[], &[], &[],
        )
        .unwrap();
        let canon_label = canon["label"].as_str().unwrap().to_string();

        let sibling = super::learn_value(
            &conn, &cfg, "sibling note", &["side".to_string()], &[], &[], &[], false, true, &[], &[], &[],
        )
        .unwrap();
        let sibling_label = sibling["label"].as_str().unwrap().to_string();

        // Learn a new memory that is derived_from the canonical AND similar_to the sibling.
        let new_v = super::learn_value(
            &conn,
            &cfg,
            "applied specialisation of the pattern",
            &["apply".to_string()],
            &[],
            &[],
            &[],
            false,
            true,
            &[],
            &[canon_label.clone()],
            &[sibling_label.clone()],
        )
        .unwrap();

        let derived = new_v["derived_from"].as_array().unwrap();
        assert_eq!(derived.len(), 1);
        assert_eq!(derived[0].as_str().unwrap(), canon_label);

        let similar = new_v["similar_to"].as_array().unwrap();
        assert_eq!(similar.len(), 1);
        assert_eq!(similar[0].as_str().unwrap(), sibling_label);

        // The typed edges must actually exist in the graph.
        let new_uuid = db::get_item_by_handle(&conn, new_v["label"].as_str().unwrap())
            .unwrap()
            .uuid
            .to_string();
        let out = db::get_memory_links_from(&conn, &new_uuid).unwrap();
        assert!(out.iter().any(|l| l.relation == "derived_from"), "derived_from edge exists");
        assert!(out.iter().any(|l| l.relation == "similar_to"), "similar_to edge exists");
    }

    #[test]
    fn learn_unresolvable_link_warns_not_aborts() {
        use crate::infrastructure::{config::Config, db};
        let conn = db::open_in_memory_for_test();
        let cfg = Config::default();

        // Reference a memory label that does not exist — learn must still succeed.
        let v = super::learn_value(
            &conn,
            &cfg,
            "a memory with a dangling link",
            &["solo".to_string()],
            &[],
            &[],
            &[],
            false,
            true,
            &[],
            &["m9999".to_string()],
            &[],
        );
        assert!(v.is_ok(), "unresolvable --derived-from must not abort the learn");
        let v = v.unwrap();
        // The link was skipped, so the reported array is empty.
        assert_eq!(v["derived_from"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn overlap_suggests_typed_link() {
        // Near-duplicate → derived-from / supersedes; partial → similar-to.
        let near = super::near_dupe_suggestion("m26");
        assert!(near.contains("--derived-from m26"), "near-dupe offers --derived-from: {near}");
        assert!(near.contains("--supersedes m26"), "near-dupe offers --supersedes: {near}");

        let partial = super::partial_overlap_suggestion("m30");
        assert!(partial.contains("--similar-to m30"), "partial offers --similar-to: {partial}");
        // No longer the vague untyped "possible contradiction" wording.
        assert!(!partial.to_lowercase().contains("possible contradiction"));
    }
}
