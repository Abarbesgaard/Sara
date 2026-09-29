use anyhow::{Context, Result};
use rusqlite::Connection;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Item;
use crate::infrastructure::project::{detect_current_project, resolve_file_link_here};

mod render;

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
    let v = learn_value(
        conn,
        cfg,
        text,
        tags,
        projects,
        tasks,
        files,
        auto_files,
        force,
        supersedes,
        derived_from,
        similar_to,
    )?;
    render::print_learned(&v);
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
    // Birth-time canonical attachment: canonical patterns this memory's tags
    // place it inside. Computed before the write (needs the pre-insert tag view)
    // and linked after, so a new instance auto-attaches to its pattern.
    let mut auto_canonical: Vec<uuid::Uuid> = Vec::new();
    if !force {
        crate::infrastructure::safety::check_size(text)?;
        crate::infrastructure::safety::check_secrets(text)?;
        // Scope the mandatory overlap band to the province this memory lands in
        // (explicit -p, else the current project) so common tags don't flag
        // other provinces' memories as must-resolve.
        let primary_project = projects.first().cloned().or_else(|| {
            crate::infrastructure::project::detect_current_project(conn, cfg)
                .ok()
                .map(|(p, _)| p)
        });
        check_overlap(conn, tags, &resolved_files, primary_project.as_deref())?;
        auto_canonical = canonical_overlap_candidates(conn, tags)?;
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
                            let derived: Vec<String> =
                                db::get_memory_links_to(conn, &target.uuid.to_string())
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
                                    if derived.len() == 1 {
                                        "memory"
                                    } else {
                                        "memories"
                                    },
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

    // Birth-time canonical attachment: auto-link `derived_from` to each canonical
    // pattern this memory's tags landed it inside, unless the author already
    // named that canonical in an explicit --derived-from/--supersedes/--similar-to
    // (an explicit intent — including a deliberate supersede — always wins). The
    // pattern strengthens as instances are learned, without a later `reflect`.
    // Reversible with `sara unlink-memory <this> derived_from <canonical>`.
    let mut auto_derived_labels: Vec<String> = Vec::new();
    {
        let already: std::collections::HashSet<&String> = superseded_labels
            .iter()
            .chain(derived_from_labels.iter())
            .chain(similar_to_labels.iter())
            .collect();
        for u in &auto_canonical {
            if let Ok(target) = db::get_item_by_uuid(conn, &u.to_string()) {
                let label = format!("m{}", target.display_id.unwrap_or(0));
                if already.contains(&label) || auto_derived_labels.contains(&label) {
                    continue;
                }
                db::insert_memory_link(conn, &new_uuid, &u.to_string(), "derived_from", 1.0)?;
                auto_derived_labels.push(label);
            }
        }
    }

    let files_json: Vec<Value> = resolved_files.iter().map(|f| json!(f)).collect();
    let tasks_json: Vec<Value> = item
        .linked_tasks
        .iter()
        .map(|(id, desc, src)| {
            json!({
                "id": id.parse::<i64>().unwrap_or(0),
                "description": desc,
                "source": src,
            })
        })
        .collect();

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
        "auto_derived_from": auto_derived_labels,
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

/// Canonical pattern memories the new memory's tags place it squarely inside:
/// near-duplicates (shares ALL tags) or canonical partial-tag matches (shares
/// ≥50% of tags with a memory that already has derived children). These are the
/// birth-time auto-attach targets — a new memory landing on an established
/// pattern is registered as another application (`derived_from`) automatically,
/// so the pattern strengthens as instances are learned instead of only during a
/// later `reflect` sweep.
///
/// Unlike `check_overlap`'s *mandatory warning* band, this deliberately does NOT
/// restrict to the current project: a canonical pattern is inherently
/// cross-project (the same fault recurring across repos is exactly what makes it
/// canonical), so a new repo's instance must be allowed to attach to it. The
/// confidence gate is instead "the overlap target is already a canonical".
/// Returns candidate uuids (deduped, order-stable).
pub(crate) fn canonical_overlap_candidates(
    conn: &Connection,
    tags: &[String],
) -> Result<Vec<uuid::Uuid>> {
    let normalized: Vec<String> = tags
        .iter()
        .map(|t| t.trim().to_lowercase())
        .filter(|t| !t.is_empty())
        .collect();
    if normalized.is_empty() {
        return Ok(vec![]);
    }

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

    let near_dupes: std::collections::HashSet<uuid::Uuid> = all_intersection.unwrap_or_default();
    let partial: std::collections::HashSet<uuid::Uuid> =
        any_union.difference(&near_dupes).copied().collect();
    let threshold = ((normalized.len() as f64) * 0.5).ceil() as usize;
    let significant_partial: Vec<uuid::Uuid> = if normalized.len() > 1 {
        partial
            .into_iter()
            .filter(|u| per_tag_sets.iter().filter(|set| set.contains(u)).count() >= threshold)
            .collect()
    } else {
        vec![]
    };

    // Candidates = near-dupes ∪ significant partials, filtered to established
    // canonicals (those with derived children).
    let mut out: Vec<uuid::Uuid> = Vec::new();
    for u in near_dupes.into_iter().chain(significant_partial) {
        if !out.contains(&u) && canonical_derived_count(conn, &u) > 0 {
            out.push(u);
        }
    }
    Ok(out)
}

/// Detect existing memories whose tags overlap significantly with the new one,
/// and memories that share any of the same file links (even with different tags).
/// Two tag-overlap bands:
///   - Near-duplicate: existing memory shares ALL given tags → warn prominently.
///   - Partial overlap: shares ≥50% (but not all) tags → softer note.
///
/// File-overlap: any memory already linked to one of the new memory's files → warn.
/// Prints warnings to stderr; never blocks the write (use --force to silence).
pub(crate) fn check_overlap(
    conn: &Connection,
    tags: &[String],
    files: &[String],
    current_project: Option<&str>,
) -> Result<()> {
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

    let near_dupes: std::collections::HashSet<uuid::Uuid> = all_intersection.unwrap_or_default();
    let partial: std::collections::HashSet<uuid::Uuid> =
        any_union.difference(&near_dupes).copied().collect();

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

    // p3: the mandatory-resolve band is only actionable for SAME-province
    // overlaps — a common tag (auth/api/db) otherwise floods it with other
    // provinces' memories the author cannot resolve. Keep same-project hits in
    // the mandatory band; demote cross-project ones to the soft "related" note.
    // With no project context (current_project = None) the old global behaviour
    // is preserved.
    let mut plain_partial = plain_partial;
    let same_project = |u: &uuid::Uuid| -> bool {
        match current_project {
            None => true,
            Some(p) => {
                db::get_item_by_uuid(conn, &u.to_string())
                    .ok()
                    .and_then(|i| i.project)
                    .as_deref()
                    == Some(p)
            }
        }
    };
    let (mandatory, cross_project): (Vec<uuid::Uuid>, Vec<uuid::Uuid>) = near_dupes
        .iter()
        .copied()
        .chain(canonical_partial.iter().copied())
        .partition(&same_project);
    plain_partial.extend(cross_project);

    if !mandatory.is_empty() {
        let all_dupes: Vec<uuid::Uuid> = mandatory;
        eprintln!(
            "Warning: {} existing {} with identical or canonical-matching tags [{}]:",
            all_dupes.len(),
            if all_dupes.len() == 1 {
                "memory"
            } else {
                "memories"
            },
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
            if plain_partial.len() == 1 {
                "memory"
            } else {
                "memories"
            }
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
                if file_overlap.len() == 1 {
                    "memory"
                } else {
                    "memories"
                }
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
    let mut paths: Vec<String> = explicit.iter().map(|p| resolve_file_link_here(p)).collect();

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

    // Keep the semantic index current: semantic recall is ALWAYS enabled
    // (SemanticOpts::from_cfg), so every learned memory must be embedded or it
    // could never match by meaning. Always index — do not gate on the
    // deprecated `recall.semantic` toggle, which left semantic recall silently
    // dead in every project that never flipped it. Best-effort — never blocks
    // learning.
    crate::infrastructure::embedding::index_memory(conn, &item);

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
#[path = "../../../tests/unit/commands/learn/mod.rs"]
mod tests;
