use anyhow::Result;
use rusqlite::Connection;
use uuid::Uuid;

use super::files::file_overlaps;
use super::hints::{
    canonical_derived_count, canonical_hint, near_dupe_suggestion, partial_overlap_suggestion,
};
use super::tags::TagOverlap;
use crate::commands::shared::memory_handle;
use crate::infrastructure::db;
use crate::infrastructure::model::Item;

const SNIPPET_CHARS: usize = 80;

#[cfg(test)]
pub(in crate::commands::learn) fn check_overlap(
    conn: &Connection,
    tags: &[String],
    files: &[String],
    current_project: Option<&str>,
) -> Result<()> {
    let overlap = TagOverlap::compute(conn, tags)?;
    warn_overlap(conn, &overlap, files, current_project)
}

pub(in crate::commands::learn) fn warn_overlap(
    conn: &Connection,
    overlap: &TagOverlap,
    files: &[String],
    current_project: Option<&str>,
) -> Result<()> {
    let (canonical_partial, mut plain_partial): (Vec<Uuid>, Vec<Uuid>) = overlap
        .partial
        .iter()
        .partition(|u| canonical_derived_count(conn, u) > 0);

    let same_project = |u: &Uuid| -> bool {
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
    let (mandatory, cross_project): (Vec<Uuid>, Vec<Uuid>) = overlap
        .near_dupes
        .iter()
        .copied()
        .chain(canonical_partial)
        .partition(&same_project);
    plain_partial.extend(cross_project);

    warn_dupes(conn, &mandatory, &overlap.tags);
    warn_partial(conn, &plain_partial);
    if !files.is_empty() {
        warn_shared_files(conn, &file_overlaps(conn, files, &overlap.near_dupes)?);
    }
    Ok(())
}

fn warn_dupes(conn: &Connection, dupes: &[Uuid], tags: &[String]) {
    if dupes.is_empty() {
        return;
    }
    eprintln!(
        "Warning: {} existing {} with identical or canonical-matching tags [{}]:",
        dupes.len(),
        memories_word(dupes.len()),
        tags.join(", ")
    );
    let mut first: Option<String> = None;
    let mut canonical_hit: Option<(String, usize)> = None;
    for u in dupes {
        let Some(item) = load(conn, u) else { continue };
        let label = memory_handle(&item);
        let derived_count = canonical_derived_count(conn, u);
        let suffix = if derived_count > 0 {
            format!(" [canonical, {derived_count} derived]")
        } else {
            String::new()
        };
        eprintln!("  {label}{suffix} — {}", snippet(&item));
        if derived_count > 0 && canonical_hit.is_none() {
            canonical_hit = Some((label.clone(), derived_count));
        }
        first.get_or_insert(label);
    }
    match (canonical_hit, first) {
        (Some((label, count)), _) => eprintln!("{}", canonical_hint(&label, count)),
        (None, Some(first)) => eprintln!("{}", near_dupe_suggestion(&first)),
        (None, None) => {}
    }
}

fn warn_partial(conn: &Connection, partial: &[Uuid]) {
    if partial.is_empty() {
        return;
    }
    eprintln!(
        "Note: {} potentially related {} (partial tag overlap):",
        partial.len(),
        memories_word(partial.len())
    );
    let mut first: Option<String> = None;
    for u in partial {
        let Some(item) = load(conn, u) else { continue };
        let label = memory_handle(&item);
        eprintln!("  {label} — {}", snippet(&item));
        first.get_or_insert(label);
    }
    if let Some(first) = first {
        eprintln!("{}", partial_overlap_suggestion(&first));
    }
}

fn warn_shared_files(conn: &Connection, overlaps: &[(String, Uuid)]) {
    if overlaps.is_empty() {
        return;
    }
    eprintln!(
        "Note: {} existing {} linked to the same file(s) — consider --supersedes or sara relearn:",
        overlaps.len(),
        memories_word(overlaps.len())
    );
    for (path, u) in overlaps {
        let Some(item) = load(conn, u) else { continue };
        let short_path = std::path::Path::new(path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(path.as_str());
        eprintln!(
            "  {} [{short_path}] — {}",
            memory_handle(&item),
            snippet(&item)
        );
    }
}

fn load(conn: &Connection, u: &Uuid) -> Option<Item> {
    db::get_item_by_uuid(conn, &u.to_string()).ok()
}

fn snippet(item: &Item) -> String {
    item.body
        .chars()
        .take(SNIPPET_CHARS)
        .collect::<String>()
        .trim()
        .to_string()
}

fn memories_word(n: usize) -> &'static str {
    if n == 1 { "memory" } else { "memories" }
}
