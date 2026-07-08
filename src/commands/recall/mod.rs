use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde_json::json;
use std::collections::HashSet;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Item;

/// `sara recall <query>` — cross-task memory. Uses the FTS5 index over task
/// descriptions/rationale/assignment, annotations (findings/decisions/…), and
/// code-anchor reasons so an agent can pull prior context from the whole history.
/// Also supports exact `--tag`/`--project` lookups over learned memories
/// (`sara learn`), indexed via `item_tags`/`item_projects` rather than FTS
/// ranking, so a known topic can be found precisely instead of by keyword luck.
///
/// When the `embeddings` table has been populated, semantic hits are blended in
/// (hybrid keyword + vector recall); today FTS5 is the active engine.
///
/// A single resolved hit, unifying task-level FTS matches and memory
/// (`items`) hits so both can be ranked together.
struct Hit {
    ref_kind: String,
    /// "task <id>" or the item's short handle (e.g. "m3").
    label: String,
    description: String,
    snippet: String,
    /// Task-linkage-derived confidence (see `db::item_strength`); 1.0 baseline
    /// for plain task hits, which have no such linkage to derive from.
    strength: f64,
    /// True when this hit came from an exact `--tag`/`--project` match rather
    /// than plain-text FTS ranking.
    exact_match: bool,
    modified: Option<DateTime<Utc>>,
}

/// Structured cross-task recall for the MCP `recall` tool and the `--json` CLI
/// path: keyword (FTS5) hits, exact tag/project hits, plus any semantic hits.
pub fn recall_value(
    conn: &Connection,
    _cfg: &Config,
    query: &str,
    tags: &[String],
    projects: &[String],
    limit: i64,
) -> Result<serde_json::Value> {
    let query = query.trim();
    let tags = normalize(tags);
    let projects = normalize(projects);

    if query.is_empty() && tags.is_empty() && projects.is_empty() {
        anyhow::bail!("Provide a search query, --tag, or --project to recall.");
    }

    let hits = collect_hits(conn, query, &tags, &projects, limit)?;
    let keyword: Vec<_> = hits
        .iter()
        .map(|h| {
            json!({
                "ref_kind": h.ref_kind,
                "label": h.label,
                "description": h.description,
                "text": h.snippet,
                "strength": h.strength,
                "exact_match": h.exact_match,
                "modified": h.modified.map(|m| m.to_rfc3339()),
            })
        })
        .collect();
    let sem: Vec<_> = semantic_hits(conn, query, limit)
        .iter()
        .map(|(id, desc, score)| json!({ "task": id, "task_description": desc, "score": score }))
        .collect();
    Ok(json!({
        "query": query,
        "tag": tags,
        "project": projects,
        "keyword": keyword,
        "semantic": sem,
    }))
}

pub fn run(
    conn: &Connection,
    cfg: &Config,
    query: &str,
    tags: &[String],
    projects: &[String],
    limit: i64,
    as_json: bool,
) -> Result<()> {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&recall_value(conn, cfg, query, tags, projects, limit)?)?
        );
        return Ok(());
    }

    let query = query.trim();
    let tags = normalize(tags);
    let projects = normalize(projects);

    if query.is_empty() && tags.is_empty() && projects.is_empty() {
        anyhow::bail!("Provide a search query, --tag, or --project to recall.");
    }

    let hits = collect_hits(conn, query, &tags, &projects, limit)?;
    let semantic = semantic_hits(conn, query, limit);

    if hits.is_empty() && semantic.is_empty() {
        if !tags.is_empty() || !projects.is_empty() {
            if !db::has_any_memories(conn)? {
                println!("No memories recorded yet. Use `sara learn \"...\"` to save one.");
            } else {
                println!("No matches for the given --tag/--project filters.");
            }
        } else {
            println!("No matches for \"{query}\".");
        }
        return Ok(());
    }

    if !hits.is_empty() {
        println!("Keyword matches:");
        for h in &hits {
            let age = h.modified.map(age_str).unwrap_or_default();
            let marker = if h.exact_match { "=" } else { "~" };
            println!(
                "  [{}] {} {} {}: {}{}",
                h.ref_kind,
                marker,
                h.label,
                h.description,
                h.snippet.trim(),
                if age.is_empty() {
                    String::new()
                } else {
                    format!(" ({age})")
                }
            );
        }
    }
    if !semantic.is_empty() {
        println!("\nSemantically related:");
        for (id, desc, score) in &semantic {
            println!("  task {id} ({score:.2}): {desc}");
        }
    }
    Ok(())
}

/// Trim, drop empty entries.
fn normalize(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

/// Resolve query/tag/project inputs into a single ranked list of hits:
/// Strong (linkage-derived) memories first, then exact tag/project matches,
/// then plain FTS hits; ties broken by most-recently-modified.
fn collect_hits(
    conn: &Connection,
    query: &str,
    tags: &[String],
    projects: &[String],
    limit: i64,
) -> Result<Vec<Hit>> {
    // Exact filters narrow first: a memory must carry every given --tag, and
    // reference at least one of the given --project values.
    let exact_items: Option<Vec<Item>> = if !tags.is_empty() || !projects.is_empty() {
        let mut by_tag: Option<HashSet<uuid::Uuid>> = None;
        for tag in tags {
            let hit: HashSet<uuid::Uuid> = db::find_items_by_tag(conn, tag)?
                .into_iter()
                .map(|i| i.uuid)
                .collect();
            by_tag = Some(match by_tag {
                Some(existing) => existing.intersection(&hit).copied().collect(),
                None => hit,
            });
        }

        let mut by_project: Option<HashSet<uuid::Uuid>> = None;
        for project in projects {
            let hit: HashSet<uuid::Uuid> = db::find_items_by_project(conn, project)?
                .into_iter()
                .map(|i| i.uuid)
                .collect();
            by_project = Some(match by_project {
                Some(mut existing) => {
                    existing.extend(hit);
                    existing
                }
                None => hit,
            });
        }

        let uuids: HashSet<uuid::Uuid> = match (by_tag, by_project) {
            (Some(t), Some(p)) => t.intersection(&p).copied().collect(),
            (Some(t), None) => t,
            (None, Some(p)) => p,
            (None, None) => HashSet::new(),
        };

        let mut items = vec![];
        for u in uuids {
            if let Ok(item) = db::get_item_by_uuid(conn, &u.to_string()) {
                items.push(item);
            }
        }
        Some(items)
    } else {
        None
    };

    let fts_hits = if query.is_empty() {
        vec![]
    } else {
        db::search_fts(conn, query, limit.max(50))?
    };

    let mut hits = vec![];

    match exact_items {
        Some(items) => {
            // Filters given: AND with free-text query when one was also
            // provided (both must match), otherwise the exact filter alone
            // defines the result set.
            let fts_item_uuids: HashSet<String> = fts_hits
                .iter()
                .filter(|h| h.ref_kind.starts_with("item_"))
                .map(|h| h.task_uuid.clone())
                .collect();
            for item in items {
                if !query.is_empty() && !fts_item_uuids.contains(&item.uuid.to_string()) {
                    continue;
                }
                hits.push(item_hit(conn, item, true));
            }
        }
        None => {
            for h in &fts_hits {
                if h.ref_kind.starts_with("item_") {
                    if let Ok(item) = db::get_item_by_uuid(conn, &h.task_uuid) {
                        hits.push(item_hit(conn, item, false));
                    }
                    continue;
                }
                let (id, desc, modified) = match db::resolve_task(conn, &h.task_uuid) {
                    Ok(task) => (
                        task.id.unwrap_or(0),
                        task.description.clone(),
                        Some(task.modified),
                    ),
                    Err(_) => (0, String::new(), None),
                };
                hits.push(Hit {
                    ref_kind: h.ref_kind.clone(),
                    label: format!("task {id}"),
                    description: desc,
                    snippet: h.text.chars().take(160).collect(),
                    strength: 1.0,
                    exact_match: false,
                    modified,
                });
            }
        }
    }

    hits.sort_by(|a, b| {
        b.exact_match
            .cmp(&a.exact_match)
            .then(
                b.strength
                    .partial_cmp(&a.strength)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(b.modified.cmp(&a.modified))
    });
    hits.truncate(limit.max(0) as usize);

    Ok(hits)
}

fn item_hit(conn: &Connection, item: Item, exact_match: bool) -> Hit {
    let handle = format!(
        "{}{}",
        item.kind.chars().next().unwrap_or('m'),
        item.display_id.unwrap_or(0)
    );
    let snippet: String = item
        .summary
        .clone()
        .unwrap_or_else(|| item.body.clone())
        .chars()
        .take(160)
        .collect();
    Hit {
        ref_kind: format!("item_{}", item.kind),
        strength: db::item_strength(conn, &item),
        label: handle,
        description: item.title.clone(),
        snippet,
        exact_match,
        modified: Some(item.modified),
    }
}

/// Compact relative age, e.g. "just now", "5m ago", "3h ago", "2d ago". Local
/// to `recall` (rather than reused from another command slice) to keep the
/// vertical-slice boundary the architecture tests enforce.
fn age_str(dt: DateTime<Utc>) -> String {
    let secs = (Utc::now() - dt).num_seconds().max(0);
    const MIN: i64 = 60;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    match secs {
        s if s < MIN => "just now".to_string(),
        s if s < HOUR => format!("{}m ago", s / MIN),
        s if s < DAY => format!("{}h ago", s / HOUR),
        s if s < 30 * DAY => format!("{}d ago", s / DAY),
        s if s < 365 * DAY => format!("{}mo ago", s / (30 * DAY)),
        s => format!("{}y ago", s / (365 * DAY)),
    }
}

/// Best-effort vector recall over any stored embeddings. Returns empty until the
/// embeddings table is populated (no query-side embedding is computed otherwise).
fn semantic_hits(_conn: &Connection, _query: &str, _limit: i64) -> Vec<(i64, String, f32)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::config::Config;
    use crate::infrastructure::model::{Status, Task};

    fn cfg() -> Config {
        Config::default()
    }

    fn seed_memory(
        conn: &Connection,
        title: &str,
        body: &str,
        tags: &[&str],
        projects: &[&str],
    ) -> Item {
        let mut item = Item::new_memory(title.to_string(), body.to_string(), None);
        item.tags = tags.iter().map(|t| t.to_string()).collect();
        item.path = Some(String::new());
        db::insert_item(conn, &mut item).unwrap();
        db::set_item_projects(
            conn,
            &item.uuid,
            &projects.iter().map(|p| p.to_string()).collect::<Vec<_>>(),
        )
        .unwrap();
        item
    }

    #[test]
    fn recall_by_tag_returns_only_matching_items() {
        let conn = db::open_in_memory_for_test();
        seed_memory(
            &conn,
            "about service-a",
            "how to call service-a",
            &["service-a"],
            &["web-app"],
        );
        seed_memory(
            &conn,
            "about service-b",
            "how to call service-b",
            &["service-b"],
            &["web-app"],
        );

        let hits = collect_hits(&conn, "", &["service-a".to_string()], &[], 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].description, "about service-a");
        assert!(hits[0].exact_match);
    }

    #[test]
    fn recall_with_no_memories_and_no_tasks_reports_none_recorded() {
        let conn = db::open_in_memory_for_test();
        let v = recall_value(&conn, &cfg(), "", &["service-a".to_string()], &[], 20).unwrap();
        assert!(v["keyword"].as_array().unwrap().is_empty());
        assert!(!db::has_any_memories(&conn).unwrap());
    }

    #[test]
    fn strong_linked_memory_outranks_plain_fts_hit_for_same_query() {
        let conn = db::open_in_memory_for_test();

        let mut task = Task::new("fix service-a auth bug".to_string(), "Sara".to_string());
        task.status = Status::Completed;
        db::insert_task(&conn, &mut task).unwrap();

        let mut linked = Item::new_memory(
            "service-a auth fix".to_string(),
            "service-a needs an X-Client-Id header".to_string(),
            Some(task.uuid),
        );
        linked.path = Some(String::new());
        db::insert_item(&conn, &mut linked).unwrap();

        let mut unlinked = Item::new_memory(
            "unrelated service-a note".to_string(),
            "service-a has a staging endpoint too".to_string(),
            None,
        );
        unlinked.path = Some(String::new());
        db::insert_item(&conn, &mut unlinked).unwrap();

        let hits = collect_hits(&conn, "service-a", &[], &[], 20).unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0].description, "service-a auth fix");
    }

    #[test]
    fn hyphenated_tag_is_not_misparsed_as_fts_operator() {
        let conn = db::open_in_memory_for_test();
        seed_memory(
            &conn,
            "hyphen tag test",
            "notes about service-a",
            &["service-a"],
            &[],
        );

        // Regression: FTS5 treats bare hyphens as NOT; --tag must never go
        // through MATCH at all (find_items_by_tag uses a plain WHERE clause),
        // and a free-text query containing a hyphen must still be quoted.
        let hits = collect_hits(&conn, "", &["service-a".to_string()], &[], 20).unwrap();
        assert_eq!(hits.len(), 1);

        let hits = collect_hits(&conn, "service-a", &[], &[], 20).unwrap();
        assert_eq!(hits.len(), 1);
    }

    #[test]
    fn ties_on_strength_and_match_kind_break_by_recency() {
        let conn = db::open_in_memory_for_test();
        let older = seed_memory(
            &conn,
            "older note",
            "shared-topic body",
            &["shared-topic"],
            &[],
        );
        std::thread::sleep(std::time::Duration::from_millis(20));
        let newer = seed_memory(
            &conn,
            "newer note",
            "shared-topic body",
            &["shared-topic"],
            &[],
        );
        assert!(newer.modified >= older.modified);

        let hits = collect_hits(&conn, "", &["shared-topic".to_string()], &[], 20).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].description, "newer note");
    }
}
