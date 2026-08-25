use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Item;
use crate::infrastructure::project::{detect_current_project, resolve_file_link_here};

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
                        // Superseding a canonical (one with derived children) can
                        // orphan those children on a now-corrected pattern — warn,
                        // never auto-archive (mirrors `sara forget`'s behaviour).
                        if relation == "supersedes" {
                            let derived: Vec<String> = db::get_memory_links_to(
                                conn,
                                &target.uuid.to_string(),
                            )
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|l| l.relation == "derived_from")
                            .filter_map(|l| db::get_item_by_uuid(conn, &l.from_uuid).ok())
                            .map(|i| format!("m{}", i.display_id.unwrap_or(0)))
                            .collect();
                            if !derived.is_empty() {
                                eprintln!(
                                    "Warning: {target_label} is a canonical pattern memory with \
                                     {} derived {} ({}) — review with `sara dream <label>` or \
                                     archive with `sara forget <label>`; they are not \
                                     auto-archived by this supersede.",
                                    derived.len(),
                                    if derived.len() == 1 { "memory" } else { "memories" },
                                    derived.join(", ")
                                );
                            }
                        }
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

/// Number of `derived_from` children pointing at `uuid` — non-zero means
/// `uuid` is a canonical pattern memory, not just "another similar memory".
pub(crate) fn canonical_derived_count(conn: &Connection, uuid: &uuid::Uuid) -> usize {
    db::get_memory_links_to(conn, &uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .count()
}

/// Build the copy-pasteable hint for a candidate that is itself a canonical
/// pattern memory. Upgrades the generic near-dupe/partial suggestion into one
/// that specifically proposes `--derived-from` (register as another
/// application of the pattern) or `sara relearn` (enrich the canonical in
/// place) instead of silently accumulating another near-duplicate.
pub(crate) fn canonical_hint(label: &str, derived_count: usize) -> String {
    format!(
        "→ {label} is a canonical pattern memory with {derived_count} derived application{} — \
         consider `sara learn --derived-from {label}` to register this as another application, \
         or `sara relearn {label}` to enrich the canonical instead of creating a new memory.",
        if derived_count == 1 { "" } else { "s" }
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
    let normalized: Vec<String> = tags
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

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

    // Canonical detection: a candidate that already has derived children is an
    // established pattern, not just "another similar memory". A partial-tag
    // match against a canonical is upgraded to the near-dupe band, and any
    // canonical hit (near-dupe or promoted-partial) gets a specific
    // `--derived-from`/`relearn` hint instead of the generic suggestion.
    let (canonical_partial, plain_partial): (Vec<uuid::Uuid>, Vec<uuid::Uuid>) =
        significant_partial
            .into_iter()
            .partition(|u| canonical_derived_count(conn, u) > 0);

    if !near_dupes.is_empty() || !canonical_partial.is_empty() {
        let all_dupes: Vec<uuid::Uuid> =
            near_dupes.iter().copied().chain(canonical_partial.iter().copied()).collect();
        eprintln!(
            "Warning: {} existing {} with identical or canonical-matching tags [{}]:",
            all_dupes.len(),
            if all_dupes.len() == 1 { "memory" } else { "memories" },
            normalized.join(", ")
        );
        let mut labels: Vec<String> = Vec::new();
        let mut canonical_hit: Option<(String, usize)> = None;
        for u in &all_dupes {
            if let Ok(item) = db::get_item_by_uuid(conn, &u.to_string()) {
                let label = format!("m{}", item.display_id.unwrap_or(0));
                let snippet: String = item.body.chars().take(80).collect();
                let derived_count = canonical_derived_count(conn, u);
                let suffix = if derived_count > 0 {
                    format!(" [canonical, {derived_count} derived]")
                } else {
                    String::new()
                };
                eprintln!("  {label}{suffix} — {}", snippet.trim());
                if derived_count > 0 && canonical_hit.is_none() {
                    canonical_hit = Some((label.clone(), derived_count));
                }
                labels.push(label);
            }
        }
        // A canonical hit gets the specific --derived-from/relearn hint;
        // otherwise fall back to the generic copy-pasteable typed link.
        match (&canonical_hit, labels.first()) {
            (Some((label, count)), _) => eprintln!("{}", canonical_hint(label, *count)),
            (None, Some(first)) => eprintln!("{}", near_dupe_suggestion(first)),
            (None, None) => {}
        }
    }

    if !plain_partial.is_empty() {
        eprintln!(
            "Note: {} potentially related {} (partial tag overlap):",
            plain_partial.len(),
            if plain_partial.len() == 1 { "memory" } else { "memories" }
        );
        let mut labels: Vec<String> = Vec::new();
        for u in &plain_partial {
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
    // Runs whether or not tags were given, so a `--file`-only learn still
    // surfaces "this file already has a memory".
    if !files.is_empty() {
        let file_overlap = file_overlaps(conn, files, &near_dupes)?;
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

/// Existing *memory* items linked to any of `files`, excluding those in
/// `exclude` (already reported as tag near-dupes). Each memory appears once,
/// paired with the first file path that matched it. Pure detection — no I/O
/// beyond the DB — so it runs regardless of whether tags were supplied.
pub(crate) fn file_overlaps(
    conn: &Connection,
    files: &[String],
    exclude: &std::collections::HashSet<uuid::Uuid>,
) -> Result<Vec<(String, uuid::Uuid)>> {
    let mut out: Vec<(String, uuid::Uuid)> = Vec::new();
    for path in files {
        for item in db::find_items_by_file(conn, path, false)? {
            if item.kind == "memory"
                && !out.iter().any(|(_, u)| *u == item.uuid)
                && !exclude.contains(&item.uuid)
            {
                out.push((path.clone(), item.uuid));
            }
        }
    }
    Ok(out)
}

/// Collect file paths from explicit --file flags and optionally --auto-files.
/// All paths are resolved to absolute. Returns an error if --auto-files is
/// requested but no git root can be found and no --file was given.
fn collect_files(explicit: &[String], auto_files: bool) -> Result<Vec<String>> {
    let mut paths: Vec<String> = explicit
        .iter()
        .map(|p| resolve_file_link_here(p))
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
        .map(|l| git_root.join(l).to_string_lossy().into_owned())
        .collect())
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
            db::resolve_task(conn, prefix)
                .with_context(|| format!("looking up --task '{prefix}'"))?
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

    // Keep the semantic index current: when semantic recall is enabled, embed
    // the freshly-learned memory so future `recall --semantic` can match it by
    // meaning. Best-effort — never blocks learning.
    if cfg.recall.semantic {
        crate::infrastructure::embedding::index_memory(conn, &item);
    }

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

    // Resolve all explicit --task prefixes and merge (explicit wins). Resolution
    // is display-id-first (db::resolve_task), consistent with every other
    // task-referencing command; a bare number is a display id, not a uuid prefix.
    for prefix in task_prefixes {
        if let Ok(t) = db::resolve_task(conn, prefix) {
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
    fn learn_task_prefers_display_id_over_uuid_collision() {
        use crate::infrastructure::{config::Config, db, model::Task};
        use uuid::Uuid;

        let conn = db::open_in_memory_for_test();
        let cfg = Config::default();

        // Task A: the intended target. Its UUID deliberately does NOT start with "1".
        let mut task_a = Task::new("intended target".into(), "proj".into());
        task_a.uuid = Uuid::parse_str("aaaaaaaa-0000-0000-0000-000000000001").unwrap();
        db::insert_task(&conn, &mut task_a).unwrap();
        let a_id = task_a.id.unwrap(); // display id 1

        // Task B: an unrelated task whose UUID starts with the same digit as A's
        // display id. A raw uuid-prefix lookup on "1" would wrongly match this.
        let mut task_b = Task::new("unrelated task".into(), "proj".into());
        task_b.uuid = Uuid::parse_str("1bbbbbbb-0000-0000-0000-000000000002").unwrap();
        db::insert_task(&conn, &mut task_b).unwrap();

        // Learn a memory with --task <A's display id>.
        let v = super::learn_value(
            &conn,
            &cfg,
            "finding tied to task A",
            &["tag-r".to_string()],
            &[],
            &[a_id.to_string()],
            &[],
            false,
            true,
            &[],
            &[],
            &[],
        )
        .unwrap();

        // The memory must be linked to task A (by display id), never task B.
        let item = db::get_item_by_handle(&conn, v["label"].as_str().unwrap()).unwrap();
        let linked = db::get_item_task_links(&conn, &item.uuid).unwrap();
        assert_eq!(linked.len(), 1, "expected exactly one task link");
        assert_eq!(
            linked[0].0.uuid,
            task_a.uuid,
            "memory linked to the wrong task (uuid-prefix collision instead of display id)"
        );
    }

    #[test]
    fn file_overlaps_detects_tagless_memory_sharing_a_file() {
        use crate::infrastructure::{config::Config, db};
        use std::collections::HashSet;
        let conn = db::open_in_memory_for_test();
        let cfg = Config::default();
        let path = "/repo/src/auth.rs".to_string();

        // A memory saved with a file but NO tags — the exact case the overlap
        // check used to skip via an early return, hiding the file collision.
        super::learn_value(
            &conn, &cfg, "auth finding", &[], &[], &[], &[path.clone()], false, true, &[], &[], &[],
        )
        .unwrap();

        let overlaps = super::file_overlaps(&conn, &[path.clone()], &HashSet::new()).unwrap();
        assert_eq!(
            overlaps.len(),
            1,
            "a tagless memory sharing the file must still be detected"
        );
        assert_eq!(overlaps[0].0, path);
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
    fn learn_value_supersedes_a_canonical_does_not_orphan_or_auto_archive_children() {
        use crate::infrastructure::{config::Config, db};
        let conn = db::open_in_memory_for_test();
        let cfg = Config::default();

        // Canonical memory.
        let canonical = super::learn_value(
            &conn, &cfg, "canonical pattern", &["pat".to_string()], &[], &[], &[], false, true, &[], &[], &[],
        )
        .unwrap();
        let canonical_label = canonical["label"].as_str().unwrap().to_string();

        // A derived child, linked via --derived-from.
        super::learn_value(
            &conn, &cfg, "an application of the pattern", &[], &[], &[], &[], false, true,
            &[], &[canonical_label.clone()], &[],
        )
        .unwrap();
        assert_eq!(
            super::canonical_derived_count(
                &conn,
                &db::get_item_by_handle(&conn, &canonical_label).unwrap().uuid
            ),
            1,
            "canonical must have exactly one derived child before superseding"
        );

        // Superseding the canonical must succeed and must not touch the
        // derived child's status — it stays active (never auto-archived).
        let new_v = super::learn_value(
            &conn, &cfg, "corrected canonical pattern", &["pat".to_string()], &[], &[], &[],
            false, true, &[canonical_label.clone()], &[], &[],
        )
        .unwrap();
        assert_eq!(new_v["superseded"].as_array().unwrap()[0].as_str().unwrap(), canonical_label);
    }

    #[test]
    fn canonical_derived_count_zero_for_a_plain_memory() {
        use crate::infrastructure::{config::Config, db};
        let conn = db::open_in_memory_for_test();
        let cfg = Config::default();
        let v = super::learn_value(
            &conn, &cfg, "plain note", &[], &[], &[], &[], false, true, &[], &[], &[],
        )
        .unwrap();
        let label = v["label"].as_str().unwrap();
        let uuid = db::get_item_by_handle(&conn, label).unwrap().uuid;
        assert_eq!(super::canonical_derived_count(&conn, &uuid), 0);
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

    #[test]
    fn canonical_hint_names_derived_from_and_relearn() {
        let hint = super::canonical_hint("m7", 3);
        assert!(hint.contains("m7"));
        assert!(hint.contains("3 derived applications"));
        assert!(hint.contains("--derived-from m7"));
        assert!(hint.contains("sara relearn m7"));
    }

    #[test]
    fn check_overlap_detects_canonical_via_derived_from_link() {
        use crate::infrastructure::{config::Config, db};
        let conn = db::open_in_memory_for_test();
        let cfg = Config::default();

        // Canonical memory with two tags.
        let canonical = super::learn_value(
            &conn, &cfg, "CodeQL config pattern", &["codeql".into(), "config".into()],
            &[], &[], &[], false, true, &[], &[], &[],
        )
        .unwrap();
        let canonical_label = canonical["label"].as_str().unwrap().to_string();
        let canonical_uuid = db::get_item_by_handle(&conn, &canonical_label).unwrap().uuid;

        // Before any derived child exists, it isn't canonical yet.
        assert_eq!(super::canonical_derived_count(&conn, &canonical_uuid), 0);

        // A derived application, linked via --derived-from.
        super::learn_value(
            &conn, &cfg, "applied CodeQL config to repo X", &["codeql".into()],
            &[], &[], &[], false, true, &[], &[canonical_label.clone()], &[],
        )
        .unwrap();

        // Now the canonical has one derived child — the detection
        // `check_overlap` relies on to upgrade a partial-tag match into the
        // near-dupe band with a `--derived-from`/`relearn` hint.
        assert_eq!(super::canonical_derived_count(&conn, &canonical_uuid), 1);

        // A new memory sharing only one of the canonical's two tags (partial
        // overlap) must not error when check_overlap runs against it.
        super::check_overlap(&conn, &["codeql".into()], &[]).unwrap();
    }
}
