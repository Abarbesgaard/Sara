use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde_json::json;
use std::collections::HashSet;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::{Item, Task};

/// `sara recall <query>` — cross-task memory. Uses the FTS5 index over task
/// descriptions/rationale/assignment, annotations (findings/decisions/…), and
/// code-anchor reasons so an agent can pull prior context from the whole history.
/// Also supports exact `--tag`/`--project` lookups over learned memories
/// (`sara learn`), indexed via `item_tags`/`item_projects` rather than FTS
/// ranking, so a known topic can be found precisely instead of by keyword luck.
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
    /// True when this hit came from the loose Tier-3 token-OR fallback (only
    /// SOME query terms matched, not the full phrase or every token). Signals
    /// lower confidence so callers treat it as "maybe related", not exact.
    loose: bool,
    /// bm25 relevance rank for free-text FTS hits: the hit's 0-based position in
    /// `db::search_fts`'s `ORDER BY rank` result (lower = better match). `None`
    /// for exact-filter hits, which are ordered by strength/recency instead.
    /// Used as the primary tie-break among equal-strength FTS hits so the most
    /// query-relevant memory leads (matters for LLM retrieval accuracy).
    fts_rank: Option<usize>,
    modified: Option<DateTime<Utc>>,
    /// File paths the memory is associated with (from `item_files`).
    files: Vec<String>,
    /// Tasks linked to this memory: (task, source "auto"|"explicit").
    linked_tasks: Vec<(Task, String)>,
    /// Labels of memories that supersede this one (incoming `supersedes` edges).
    /// Non-empty means this memory may be stale — the superseding memory is more current.
    superseded_by: Vec<String>,
    /// True when this memory is auto-generated (status=provisional) and not yet reviewed.
    provisional: bool,
    /// The item's own uuid for memory hits (None for plain task hits) — used
    /// to record usage-reinforcement events after the final hit list is known.
    item_uuid: Option<uuid::Uuid>,
    /// Labels of memories this one is derived from (outgoing `derived_from` edges).
    /// Non-empty means this is a per-application copy of a canonical pattern memory.
    derived_from_labels: Vec<String>,
    /// Number of memories that derive from this one (incoming `derived_from` edges).
    /// Non-zero means this is a canonical pattern memory.
    derived_count: usize,
}

/// Structured cross-task recall for the MCP `recall` tool and the `--json` CLI
/// path: keyword (FTS5) hits and exact tag/project hits.
pub fn recall_value(
    conn: &Connection,
    _cfg: &Config,
    query: &str,
    tags: &[String],
    projects: &[String],
    files: &[String],
    limit: i64,
    spread: bool,
) -> Result<serde_json::Value> {
    let query = query.trim();
    let tags = normalize(tags);
    let projects = normalize(projects);
    let files: Vec<String> = normalize(files).iter().map(|p| resolve_file_path(p)).collect();

    if query.is_empty() && tags.is_empty() && projects.is_empty() && files.is_empty() {
        // Bare recall: surface the most recent memories instead of erroring, so
        // the cheap exploratory "what do I know?" call just works.
        let hits = recent_hits(conn, limit)?;
        for h in &hits {
            if let Some(u) = h.item_uuid {
                let _ = db::record_memory_recall(conn, &u);
            }
        }
        let keyword = keyword_json(&hits);
        return Ok(json!({
            "query": query,
            "tag": tags,
            "project": projects,
            "files": files,
            "keyword": keyword,
            "associative": [],
            "confidence": "recent",
            "caveat": "Most recent memories (no query or filter given).",
            "recent": true,
        }));
    }

    let hits = collect_hits(conn, query, &tags, &projects, &files, limit)?;
    let keyword = keyword_json(&hits);

    // Match-confidence signal: distinguish "FTS found nothing" from "nothing exists".
    // Only meaningful when a free-text query drove the search (tag/file-only = high).
    let (confidence, caveat) = match_confidence(query, &tags, &hits);

    // Spreading activation (opt-in): radiate from the direct memory hits across
    // the graph and return the associatively-related memories a keyword search
    // misses, so agents can pull in context that shares no literal term. These
    // surfaced to the caller, so reinforce them exactly like direct hits.
    let associative: Vec<_> = if spread {
        let related = spreading_related(conn, &hits)?;
        related
            .iter()
            .map(|r| {
                let _ = db::record_memory_recall(conn, &r.item.uuid);
                json!({
                    "label": format!("m{}", r.item.display_id.unwrap_or(0)),
                    "text": r.item.summary.clone().unwrap_or_else(|| r.item.body.clone()),
                    "activation": r.activation,
                    "strength": db::item_strength(conn, &r.item),
                    "via": r.path,
                })
            })
            .collect()
    } else {
        vec![]
    };

    Ok(json!({
        "query": query,
        "tag": tags,
        "project": projects,
        "files": files,
        "keyword": keyword,
        "associative": associative,
        "confidence": confidence,
        "caveat": caveat,
    }))
}

/// Derive a match-confidence label and human-readable caveat from the search
/// inputs and results.
///
/// - `high`:   exact tag/project/file filters drove all hits (reliable index lookup).
/// - `medium`: some or all hits came from FTS keyword ranking (literal match only —
///             paraphrased or conceptually related content may not surface).
/// - `none`:   no hits at all AND a free-text query was involved — this does NOT
///             mean no similar work exists; it means no keywords overlapped.
///
/// Tag/file-only searches with zero results emit `"none"` without a caveat (the
/// absence of a tagged memory is meaningful — the tag simply doesn't exist).
fn match_confidence(query: &str, tags: &[String], hits: &[Hit]) -> (&'static str, &'static str) {
    let has_query = !query.is_empty();
    let has_exact_filters = !tags.is_empty();

    if hits.is_empty() {
        if has_query {
            return (
                "none",
                "No keyword matches found. Sara uses literal FTS only — \
                 paraphrased or conceptually related content may not surface. \
                 Try --tag, different keywords, or --file to broaden the search.",
            );
        }
        // Tag/file-only miss — meaningful absence, no misleading caveat needed.
        return ("none", "");
    }

    // Hits exist. Confidence is high only when every hit came from an exact
    // tag/project/file filter (no FTS ranking involved).
    let all_exact = hits.iter().all(|h| h.exact_match);
    if all_exact || (has_exact_filters && !has_query) {
        return ("high", "");
    }

    // Loose Tier-3 (token-OR) hits: only some query terms overlapped, so these
    // are weaker signals than a phrase or token-AND match. Flag them distinctly
    // so callers don't treat a tangential hit as a confident one.
    if hits.iter().any(|h| h.loose) {
        return (
            "low",
            "Loose match: only SOME query terms overlapped (token-OR fallback). \
             Results may be tangential and the best match need not be first — \
             refine the query or use --tag to narrow.",
        );
    }

    (
        "medium",
        "Keyword-match only (literal FTS). Paraphrased or conceptually \
         similar content with different wording may not appear.",
    )
}

pub fn run(
    conn: &Connection,
    cfg: &Config,
    query: &str,
    tags: &[String],
    projects: &[String],
    files: &[String],
    limit: i64,
    spread: bool,
    as_json: bool,
) -> Result<()> {
    if as_json {
        println!(
            "{}",
            serde_json::to_string_pretty(&recall_value(
                conn, cfg, query, tags, projects, files, limit, spread
            )?)?
        );
        return Ok(());
    }

    let query = query.trim();
    let tags = normalize(tags);
    let projects = normalize(projects);
    let files: Vec<String> = normalize(files).iter().map(|p| resolve_file_path(p)).collect();

    let recent = query.is_empty() && tags.is_empty() && projects.is_empty() && files.is_empty();

    let hits = if recent {
        recent_hits(conn, limit)?
    } else {
        collect_hits(conn, query, &tags, &projects, &files, limit)?
    };

    if hits.is_empty() {
        if recent {
            println!("No memories recorded yet. Use `sara learn \"...\"` to save one.");
        } else if !files.is_empty() {
            println!("No memories tied to the given file(s).");
        } else if !tags.is_empty() || !projects.is_empty() {
            if !db::has_any_memories(conn)? {
                println!("No memories recorded yet. Use `sara learn \"...\"` to save one.");
            } else {
                println!("No matches for the given --tag/--project filters.");
            }
        } else {
            println!("No matches for \"{query}\".");
            println!(
                "Note: Sara uses literal keyword search only — paraphrased or \
                 conceptually related content may not surface. Try --tag or different keywords."
            );
        }
        return Ok(());
    }

    if recent {
        // Bare recall reinforces the surfaced memories, mirroring collect_hits.
        for h in &hits {
            if let Some(u) = h.item_uuid {
                let _ = db::record_memory_recall(conn, &u);
            }
        }
        println!("Recent memories (no query given):");
    } else {
        // Show confidence caveat for FTS-only results so callers know the absence
        // of further hits is not a guarantee that nothing similar exists.
        let (_, caveat) = match_confidence(query, &tags, &hits);
        if !caveat.is_empty() {
            println!("Note: {caveat}");
        }
        println!("Keyword matches:");
    }

    for h in &hits {
            let age = h.modified.map(age_str).unwrap_or_default();
            let marker = if h.exact_match {
                "="
            } else if h.loose {
                "≈"
            } else {
                "~"
            };
            let files_str = if h.files.is_empty() {
                String::new()
            } else {
                format!(" [files: {}]", h.files.join(", "))
            };
            let tasks_str = if h.linked_tasks.is_empty() {
                String::new()
            } else {
                let parts: Vec<String> = h
                    .linked_tasks
                    .iter()
                    .map(|(t, src)| {
                        format!("#{} {} ({})", t.id.unwrap_or(0), t.description, src)
                    })
                    .collect();
                format!(" [via tasks: {}]", parts.join(", "))
            };
            let superseded_str = if h.superseded_by.is_empty() {
                String::new()
            } else {
                format!(" ⚠ superseded by: {}", h.superseded_by.join(", "))
            };
            let provisional_str = if h.provisional {
                " [provisional — unreviewed auto-memory]".to_string()
            } else {
                String::new()
            };
            let canonical_str = if h.derived_count > 0 {
                format!(" [canonical, {} derived]", h.derived_count)
            } else {
                String::new()
            };
            let derived_from_str = if h.derived_from_labels.is_empty() {
                String::new()
            } else {
                format!(" [derived from: {}]", h.derived_from_labels.join(", "))
            };
            println!(
                "  [{}] {} {} {}: {}{}{}{}{}{}{}{}",
                h.ref_kind,
                marker,
                h.label,
                h.description,
                h.snippet.trim(),
                files_str,
                tasks_str,
                superseded_str,
                provisional_str,
                canonical_str,
                derived_from_str,
                if age.is_empty() {
                    String::new()
                } else {
                    format!(" ({age})")
                }
            );
        }

    // Spreading activation: from the memories that matched directly, radiate
    // outward across the graph and surface the associatively-related memories a
    // flat keyword search would miss. Opt-in so default recall is unchanged.
    if spread {
        let related = spreading_related(conn, &hits)?;
        if !related.is_empty() {
            println!("\nAssociatively related (spreading activation):");
            for r in &related {
                let label = format!("m{}", r.item.display_id.unwrap_or(0));
                let snippet: String = r
                    .item
                    .summary
                    .clone()
                    .unwrap_or_else(|| r.item.body.clone())
                    .chars()
                    .take(100)
                    .collect();
                // Show the path minus the node itself: seed → … → (this).
                let via = if r.path.len() > 1 {
                    format!("  [via {}]", r.path[..r.path.len() - 1].join(" → "))
                } else {
                    String::new()
                };
                println!(
                    "  ~{label} ({:.2}): {}{}",
                    r.activation,
                    snippet.trim(),
                    via
                );
                // These surfaced to the caller — reinforce, exactly like direct
                // hits, so they feed future Hebbian consolidation. Fire-and-forget.
                let _ = db::record_memory_recall(conn, &r.item.uuid);
            }
        }
    }
    Ok(())
}

/// A memory reached by spreading activation, with the dominant synaptic path
/// (`seed → … → this`, as labels) that explains why it lit up.
struct Related {
    item: Item,
    activation: f64,
    path: Vec<String>,
}

/// Upper bound on associatively-surfaced memories. Spreading activation is only
/// useful to a reader (often an LLM) as a *small* set of the strongest links —
/// beyond a handful it becomes context-flooding noise.
const ASSOCIATIVE_CAP: usize = 5;

/// Radiate activation from the memories that matched directly and return the
/// *other* memories the network lights up, ranked by accumulated activation.
/// Results are capped to [`ASSOCIATIVE_CAP`] and their activation normalized to
/// `0..1` (relative to the strongest) so the caller — often an LLM — gets a
/// small, calibrated set instead of a global-centrality dump. Empty when
/// nothing matched a memory (e.g. task-only hits) or the graph is disconnected.
fn spreading_related(conn: &Connection, hits: &[Hit]) -> Result<Vec<Related>> {
    let seeds: Vec<uuid::Uuid> = hits.iter().filter_map(|h| h.item_uuid).collect();
    if seeds.is_empty() {
        return Ok(vec![]);
    }
    let graph = crate::infrastructure::memory_graph::MemoryGraph::build(conn)?;
    if graph.is_empty() {
        return Ok(vec![]);
    }
    let seed_set: HashSet<uuid::Uuid> = seeds.iter().copied().collect();
    let mut out = vec![];
    for act in graph.spread_activation_explained(&seeds, 2, 0.6, 1e-6) {
        if seed_set.contains(&act.uuid) {
            continue; // already shown as a direct hit
        }
        if let Ok(item) = db::get_item_by_uuid(conn, &act.uuid.to_string()) {
            out.push(Related {
                item,
                activation: act.activation,
                path: act.path,
            });
        }
        if out.len() >= ASSOCIATIVE_CAP {
            break; // a small, bounded set — not a global-centrality dump
        }
    }
    // Normalize activation to 0..1 relative to the strongest, so the score is a
    // calibrated relative signal rather than an unbounded raw sum.
    let max = out.iter().map(|r| r.activation).fold(0.0_f64, f64::max);
    if max > 0.0 {
        for r in &mut out {
            r.activation /= max;
        }
    }
    Ok(out)
}

/// Trim, drop empty entries.
fn normalize(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

/// Stop words + max-token cap for the token-AND fallback. Duplicated locally
/// (rather than reused from `add::similar`) to keep the vertical-slice
/// boundary the architecture tests enforce.
const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "in", "it", "of", "to", "for", "on", "at",
    "by", "up", "as", "or", "do", "if", "be", "we", "he", "she", "they",
    "but", "and", "not", "with", "from", "this", "that", "are", "was",
    "has", "have", "how", "what", "does", "did",
];
const MAX_AND_TOKENS: usize = 6;

/// Extract meaningful search tokens from free text: lowercase, alpha-only,
/// ≥3 chars, not a stop word, capped at MAX_AND_TOKENS.
fn meaningful_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphabetic())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() >= 3 && !STOP_WORDS.contains(&w.as_str()))
        .take(MAX_AND_TOKENS)
        .collect()
}

/// Resolve a file path (or directory prefix ending with '/') to absolute form.
/// The trailing '/' is preserved for prefix matching.
fn resolve_file_path(path: &str) -> String {
    if path.ends_with('/') {
        // Directory prefix: resolve the dir part, re-append the slash.
        let dir = path.trim_end_matches('/');
        let resolved = if std::path::Path::new(dir).is_absolute() {
            dir.to_string()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(dir).to_string_lossy().into_owned())
                .unwrap_or_else(|_| dir.to_string())
        };
        format!("{resolved}/")
    } else if std::path::Path::new(path).is_absolute() {
        path.to_string()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path).to_string_lossy().into_owned())
            .unwrap_or_else(|_| path.to_string())
    }
}

/// Resolve query/tag/project/file inputs into a single ranked list of hits:
/// Strong (linkage-derived) memories first, then exact tag/project matches,
/// then plain FTS hits; ties broken by most-recently-modified.
fn collect_hits(
    conn: &Connection,
    query: &str,
    tags: &[String],
    projects: &[String],
    files: &[String],
    limit: i64,
) -> Result<Vec<Hit>> {
    // File filter: intersect items across all --file values (AND semantics).
    let file_uuids: Option<HashSet<uuid::Uuid>> = if !files.is_empty() {
        let mut combined: Option<HashSet<uuid::Uuid>> = None;
        for path in files {
            let prefix = path.ends_with('/');
            let items = db::find_items_by_file(conn, path, prefix)?;
            let uuids: HashSet<uuid::Uuid> = items.into_iter().map(|i| i.uuid).collect();
            combined = Some(match combined {
                Some(existing) => existing.intersection(&uuids).copied().collect(),
                None => uuids,
            });
        }
        combined
    } else {
        None
    };

    // Exact filters narrow first: a memory must carry every given --tag, and
    // reference at least one of the given --project values.
    let tag_project_uuids: Option<HashSet<uuid::Uuid>> =
        if !tags.is_empty() || !projects.is_empty() {
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

            Some(match (by_tag, by_project) {
                (Some(t), Some(p)) => t.intersection(&p).copied().collect(),
                (Some(t), None) => t,
                (None, Some(p)) => p,
                (None, None) => HashSet::new(),
            })
        } else {
            None
        };

    // Combined exact filter: AND of file + tag/project sets when both provided.
    let exact_uuids: Option<HashSet<uuid::Uuid>> = match (file_uuids, tag_project_uuids) {
        (Some(f), Some(tp)) => Some(f.intersection(&tp).copied().collect()),
        (Some(f), None) => Some(f),
        (None, Some(tp)) => Some(tp),
        (None, None) => None,
    };

    // Build exact_items from the combined UUID set.
    let exact_items: Option<Vec<Item>> = if let Some(uuids) = exact_uuids {
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

    let (fts_hits, loose) = if query.is_empty() {
        (vec![], false)
    } else {
        let phrase = db::search_fts(conn, query, limit.max(50))?;
        if !phrase.is_empty() {
            (phrase, false)
        } else {
            // Phrase literal missed — fall back to token-AND (order-independent,
            // stop-word-stripped) so paraphrased queries still surface hits.
            let tokens = meaningful_tokens(query);
            if tokens.is_empty() {
                (vec![], false)
            } else {
                let and_hits = db::search_fts_tokens(conn, &tokens, limit.max(50))?;
                if !and_hits.is_empty() || tokens.len() < 2 {
                    // token-AND found something, or a single token (where OR == AND
                    // so the fallback would add nothing).
                    (and_hits, false)
                } else {
                    // Tier 3: loose token-OR fallback — match ANY meaningful
                    // token so a partial-vocabulary query returns candidates
                    // instead of nothing. Flagged loose (lower confidence).
                    (db::search_fts_tokens_or(conn, &tokens, limit.max(50))?, true)
                }
            }
        }
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
            for (rank, h) in fts_hits.iter().enumerate() {
                if h.ref_kind.starts_with("item_") {
                    if let Ok(item) = db::get_item_by_uuid(conn, &h.task_uuid) {
                        let mut hit = item_hit(conn, item, false);
                        hit.fts_rank = Some(rank);
                        hit.loose = loose;
                        hits.push(hit);
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
                    files: vec![],
                    linked_tasks: vec![],
                    superseded_by: vec![],
                    provisional: false,
                    item_uuid: None,
                    derived_from_labels: vec![],
                    derived_count: 0,
                    fts_rank: Some(rank),
                    loose,
                });
            }
        }
    }

    hits.sort_by(|a, b| {
        // Exact tag/file matches lead; then linkage-derived strength; then bm25
        // relevance (lower fts_rank = better match, `None` sorts last so exact
        // hits fall through to recency); finally most-recently-modified.
        b.exact_match
            .cmp(&a.exact_match)
            .then(
                b.strength
                    .partial_cmp(&a.strength)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(
                a.fts_rank
                    .unwrap_or(usize::MAX)
                    .cmp(&b.fts_rank.unwrap_or(usize::MAX)),
            )
            .then(b.modified.cmp(&a.modified))
    });
    hits.truncate(limit.max(0) as usize);

    // Usage reinforcement: log each memory that actually surfaced, so
    // frequently-recalled memories gain strength (see db::item_strength).
    // Fire-and-forget — a failed event write must never break recall.
    for h in &hits {
        if let Some(u) = h.item_uuid {
            let _ = db::record_memory_recall(conn, &u);
        }
    }

    Ok(hits)
}

/// Serialize recall hits into the `keyword` JSON array shared by the bare-recall
/// and query-driven paths.
fn keyword_json(hits: &[Hit]) -> Vec<serde_json::Value> {
    hits.iter()
        .map(|h| {
            json!({
                "ref_kind": h.ref_kind,
                "label": h.label,
                "description": h.description,
                "text": h.snippet,
                "strength": h.strength,
                "exact_match": h.exact_match,
                "loose": h.loose,
                "modified": h.modified.map(|m| m.to_rfc3339()),
                "files": h.files,
                "superseded_by": h.superseded_by,
                "provisional": h.provisional,
                "canonical": h.derived_count > 0,
                "derived_count": h.derived_count,
                "derived_from": h.derived_from_labels,
                "linked_tasks": h.linked_tasks.iter().map(|(t, src)| json!({
                    "id": t.id.unwrap_or(0),
                    "description": t.description,
                    "source": src,
                })).collect::<Vec<_>>(),
            })
        })
        .collect()
}

/// Recent memories for a bare `sara recall` (no query, no filters): newest
/// memories first, truncated to `limit`. Lets the cheap exploratory recall
/// just work instead of erroring, so an agent can survey what it knows.
fn recent_hits(conn: &Connection, limit: i64) -> Result<Vec<Hit>> {
    let mut memories = db::list_memories(conn)?;
    memories.truncate(limit.max(0) as usize);
    Ok(memories
        .into_iter()
        .map(|m| item_hit(conn, m, false))
        .collect())
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
    let files = db::get_item_files(conn, &item.uuid).unwrap_or_default();
    let linked_tasks = db::get_item_task_links(conn, &item.uuid).unwrap_or_default();
    // Check for incoming `supersedes` edges — means this memory may be stale.
    let superseded_by: Vec<String> = db::get_memory_links_to(conn, &item.uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "supersedes")
        .map(|l| {
            // Resolve the from_uuid to a label like "m12".
            db::get_item_by_uuid(conn, &l.from_uuid)
                .ok()
                .map(|i| format!("{}{}", i.kind.chars().next().unwrap_or('m'), i.display_id.unwrap_or(0)))
                .unwrap_or_else(|| l.from_uuid[..8].to_string())
        })
        .collect();
    // Outgoing `derived_from` edges — this memory is derived from a canonical.
    let derived_from_labels: Vec<String> = db::get_memory_links_from(conn, &item.uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .map(|l| {
            db::get_item_by_uuid(conn, &l.to_uuid)
                .ok()
                .map(|i| format!("{}{}", i.kind.chars().next().unwrap_or('m'), i.display_id.unwrap_or(0)))
                .unwrap_or_else(|| l.to_uuid[..8].to_string())
        })
        .collect();
    // Incoming `derived_from` edges — other memories derive from this canonical.
    let derived_count = db::get_memory_links_to(conn, &item.uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .count();
    Hit {
        ref_kind: format!("item_{}", item.kind),
        strength: db::item_strength(conn, &item),
        label: handle,
        description: item.title.clone(),
        snippet,
        exact_match,
        modified: Some(item.modified),
        files,
        linked_tasks,
        superseded_by,
        provisional: item.status == "provisional",
        item_uuid: Some(item.uuid),
        derived_from_labels,
        derived_count,
        fts_rank: None,
        loose: false,
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
    fn recall_falls_back_to_token_and_when_phrase_misses() {
        let conn = db::open_in_memory_for_test();
        seed_memory(
            &conn,
            "MudTable pagination gotcha",
            "MudBlazor MudTable runs pagination server-side when using ServerData",
            &[],
            &["web-app"],
        );

        // Word order differs from the stored text → phrase literal misses,
        // token-AND fallback should still surface the memory.
        let hits = collect_hits(&conn, "pagination MudTable", &[], &[], &[], 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].description, "MudTable pagination gotcha");
        assert!(!hits[0].exact_match);
    }

    #[test]
    fn recall_surfaces_provisional_memories_with_flag() {
        let conn = db::open_in_memory_for_test();
        let mut item = Item::new_memory(
            "auto memory".to_string(),
            "synthesised frobnicator pattern".to_string(),
            None,
        );
        item.status = "provisional".to_string();
        item.path = Some(String::new());
        db::insert_item(&conn, &mut item).unwrap();

        let hits = collect_hits(&conn, "frobnicator", &[], &[], &[], 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].provisional, "provisional flag must be set");
    }

    #[test]
    fn recall_value_associative_array_is_empty_by_default_and_populated_with_spread() {
        let conn = db::open_in_memory_for_test();
        let seed = seed_memory(
            &conn,
            "redis eviction policy",
            "set maxmemory-policy allkeys-lru",
            &[],
            &["web-app"],
        );
        let neighbour = seed_memory(
            &conn,
            "cache stampede guard",
            "add a mutex around cold-cache fills",
            &[],
            &["web-app"],
        );
        db::insert_memory_link(
            &conn,
            &seed.uuid.to_string(),
            &neighbour.uuid.to_string(),
            "similar_to",
            1.0,
        )
        .unwrap();

        // Default: no associative expansion, existing shape preserved.
        let v = recall_value(&conn, &cfg(), "maxmemory-policy", &[], &[], &[], 20, false).unwrap();
        assert!(v["associative"].as_array().unwrap().is_empty());
        assert_eq!(v["keyword"].as_array().unwrap().len(), 1);

        // With spread: the linked neighbour appears associatively, not in keyword.
        let v = recall_value(&conn, &cfg(), "maxmemory-policy", &[], &[], &[], 20, true).unwrap();
        let assoc = v["associative"].as_array().unwrap();
        assert!(
            assoc
                .iter()
                .any(|a| a["text"].as_str().unwrap().contains("mutex")),
            "spread should surface the linked neighbour in the associative array"
        );
        assert!(
            assoc.iter().all(|a| a["activation"].is_number()),
            "each associative hit carries an activation score"
        );
        assert!(
            assoc
                .iter()
                .all(|a| a["via"].as_array().is_some_and(|p| !p.is_empty())),
            "each associative hit carries a non-empty 'via' synaptic path"
        );
    }

    #[test]
    fn spreading_related_surfaces_linked_neighbour_not_in_direct_hits() {
        let conn = db::open_in_memory_for_test();
        // Direct hit on the query, and a linked neighbour that shares no query term.
        let seed = seed_memory(
            &conn,
            "postgres connection pooling",
            "use pgbouncer in front of postgres",
            &[],
            &["web-app"],
        );
        let neighbour = seed_memory(
            &conn,
            "supavisor tuning",
            "raise pool_size for burst traffic",
            &[],
            &["web-app"],
        );
        db::insert_memory_link(
            &conn,
            &seed.uuid.to_string(),
            &neighbour.uuid.to_string(),
            "similar_to",
            1.0,
        )
        .unwrap();

        let hits = collect_hits(&conn, "pgbouncer", &[], &[], &[], 20).unwrap();
        assert_eq!(hits.len(), 1, "only the seed matches the query directly");

        let related = spreading_related(&conn, &hits).unwrap();
        assert!(
            related.iter().any(|r| r.item.uuid == neighbour.uuid),
            "spreading activation should surface the linked neighbour"
        );
        assert!(
            related.iter().all(|r| r.item.uuid != seed.uuid),
            "direct hits must not be repeated in the associative section"
        );
        // The neighbour's path explains the hop: it ends at the neighbour and
        // starts at the seed memory.
        let n = related
            .iter()
            .find(|r| r.item.uuid == neighbour.uuid)
            .unwrap();
        let seed_lbl = format!("m{}", seed.display_id.unwrap_or(0));
        let neighbour_lbl = format!("m{}", neighbour.display_id.unwrap_or(0));
        assert_eq!(n.path.first(), Some(&seed_lbl));
        assert_eq!(n.path.last(), Some(&neighbour_lbl));
    }

    #[test]
    fn spreading_related_is_empty_without_memory_seeds() {
        let conn = db::open_in_memory_for_test();
        // No memory hits → no seeds → nothing to spread from.
        let related = spreading_related(&conn, &[]).unwrap();
        assert!(related.is_empty());
    }

    #[test]
    fn associative_output_is_capped_and_normalized() {
        let conn = db::open_in_memory_for_test();
        let hub = seed_memory(&conn, "hub topic", "central memory", &["hub"], &["p"]);
        // Many neighbours linked to the seed — without a cap, all would surface.
        for i in 0..12 {
            let n = seed_memory(&conn, &format!("n{i}"), &format!("body {i}"), &[], &["p"]);
            db::insert_memory_link(
                &conn,
                &hub.uuid.to_string(),
                &n.uuid.to_string(),
                "similar_to",
                1.0,
            )
            .unwrap();
        }
        let hits = collect_hits(&conn, "central", &[], &[], &[], 20).unwrap();
        let related = spreading_related(&conn, &hits).unwrap();

        assert!(
            related.len() <= 5,
            "associative results must be capped to ~5, got {}",
            related.len()
        );
        assert!(
            related.iter().all(|r| r.activation <= 1.0 + 1e-9),
            "activation must be normalized to <= 1.0"
        );
        assert!(
            related.iter().any(|r| r.activation >= 0.999),
            "the strongest associative hit must normalize to 1.0"
        );
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

        let hits = collect_hits(&conn, "", &["service-a".to_string()], &[], &[], 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].description, "about service-a");
        assert!(hits[0].exact_match);
    }

    #[test]
    fn recall_with_no_memories_and_no_tasks_reports_none_recorded() {
        let conn = db::open_in_memory_for_test();
        let v =
            recall_value(&conn, &cfg(), "", &["service-a".to_string()], &[], &[], 20, false).unwrap();
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

        let hits = collect_hits(&conn, "service-a", &[], &[], &[], 20).unwrap();
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
        let hits = collect_hits(&conn, "", &["service-a".to_string()], &[], &[], 20).unwrap();
        assert_eq!(hits.len(), 1);

        let hits = collect_hits(&conn, "service-a", &[], &[], &[], 20).unwrap();
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

        let hits = collect_hits(&conn, "", &["shared-topic".to_string()], &[], &[], 20).unwrap();
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].description, "newer note");
    }

    #[test]
    fn token_or_fallback_surfaces_partial_overlap_and_flags_it_loose() {
        // A memory about pruning. The query shares SOME terms ("pruning",
        // "decay") but adds terms the memory lacks ("archive", "policy" absent
        // from body) so token-AND requires all four and misses. The Tier-3
        // token-OR fallback should still surface it, flagged loose / low.
        let conn = db::open_in_memory_for_test();
        seed_memory(
            &conn,
            "prune stale entries",
            "pruning removes memories that decay over time",
            &[],
            &["engine"],
        );

        // token-AND over all four tokens finds nothing (memory lacks
        // "archive"/"garbage"), so recall would previously be empty.
        let and_only = db::search_fts_tokens(
            &conn,
            &["pruning".into(), "decay".into(), "archive".into(), "garbage".into()],
            50,
        )
        .unwrap();
        assert!(and_only.is_empty(), "precondition: token-AND misses");

        let hits =
            collect_hits(&conn, "pruning decay archive garbage", &[], &[], &[], 20).unwrap();
        assert_eq!(hits.len(), 1, "OR fallback surfaces the partial-overlap memory");
        assert_eq!(hits[0].description, "prune stale entries");
        assert!(hits[0].loose, "OR-fallback hit must be flagged loose");

        // Confidence must reflect the loose match, not medium/high.
        let (confidence, caveat) = match_confidence("pruning decay archive garbage", &[], &hits);
        assert_eq!(confidence, "low");
        assert!(caveat.contains("Loose match"));
    }

    #[test]
    fn token_or_fallback_does_not_fire_when_stricter_tiers_match() {
        // When token-AND (or phrase) already matches, the loose fallback must
        // NOT fire and hits must NOT be flagged loose (no displacement).
        let conn = db::open_in_memory_for_test();
        seed_memory(
            &conn,
            "kafka consumer lag",
            "monitor consumer group lag with burrow",
            &[],
            &["platform"],
        );
        // Both tokens present → token-AND matches → not loose.
        let hits = collect_hits(&conn, "consumer lag", &[], &[], &[], 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(!hits[0].loose, "token-AND hit must not be flagged loose");
        let (confidence, _) = match_confidence("consumer lag", &[], &hits);
        assert_eq!(confidence, "medium");
    }

    #[test]
    fn bare_recall_returns_recent_memories_instead_of_erroring() {
        // A `sara recall` with no query/tag/project/file must not error — it
        // surfaces the most recent memories so exploratory recall just works.
        let conn = db::open_in_memory_for_test();
        seed_memory(&conn, "alpha note", "first insight", &[], &["proj"]);
        seed_memory(&conn, "beta note", "second insight", &[], &["proj"]);

        let v = recall_value(&conn, &cfg(), "", &[], &[], &[], 20, false).unwrap();
        assert_eq!(v["confidence"], "recent");
        assert_eq!(v["recent"], true);
        let keyword = v["keyword"].as_array().unwrap();
        assert_eq!(keyword.len(), 2, "both memories surface as recent hits");
    }

    #[test]
    fn recall_value_keyword_and_confidence_are_stable() {
        // Pins the observable recall_value contract that must survive the
        // removal of the dead semantic scaffold: a matching free-text query
        // yields its keyword hit and a medium confidence signal.
        let conn = db::open_in_memory_for_test();
        seed_memory(
            &conn,
            "kafka consumer lag",
            "monitor consumer group lag with burrow",
            &[],
            &["platform"],
        );
        let v = recall_value(&conn, &cfg(), "burrow", &[], &[], &[], 20, false).unwrap();
        let keyword = v["keyword"].as_array().unwrap();
        assert_eq!(keyword.len(), 1, "the matching memory surfaces as a keyword hit");
        assert_eq!(keyword[0]["description"], "kafka consumer lag");
        assert_eq!(v["confidence"], "medium", "free-text FTS hit => medium confidence");
    }

    #[test]
    fn bm25_relevance_orders_equal_strength_fts_hits_over_recency() {
        let conn = db::open_in_memory_for_test();
        // Strong bm25 match: term-dense, short document. Plain memory (1.0).
        let relevant = seed_memory(&conn, "widget widget widget", "widget", &[], &[]);
        std::thread::sleep(std::time::Duration::from_millis(20));
        // Weak bm25 match: the term appears once in a long, mostly-unrelated
        // body. Inserted later, so it is MORE RECENT — under the old recency
        // tie-break it wrongly led despite being the poorer match.
        let recent_but_weak = seed_memory(
            &conn,
            "misc note",
            "this is a long note about many unrelated things and it merely \
             mentions a widget once among lots of filler words and more filler",
            &[],
            &[],
        );
        assert!(recent_but_weak.modified >= relevant.modified);

        let hits = collect_hits(&conn, "widget", &[], &[], &[], 20).unwrap();
        assert_eq!(hits.len(), 2, "both memories match the query");
        assert_eq!(
            hits[0].description, "widget widget widget",
            "the stronger bm25 match must lead over the more-recent weaker match"
        );
    }

    #[test]
    fn recall_by_file_returns_associated_memories() {
        let conn = db::open_in_memory_for_test();

        let item = seed_memory(&conn, "auth notes", "JWT details", &[], &[]);
        db::set_item_files(&conn, &item.uuid, &["src/auth.rs".to_string()]).unwrap();

        seed_memory(&conn, "unrelated notes", "something else", &[], &[]);

        let hits =
            collect_hits(&conn, "", &[], &[], &["src/auth.rs".to_string()], 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].description, "auth notes");
        assert_eq!(hits[0].files, vec!["src/auth.rs"]);
    }

    #[test]
    fn recall_by_file_prefix_returns_all_files_under_dir() {
        let conn = db::open_in_memory_for_test();

        let a = seed_memory(&conn, "auth notes", "JWT details", &[], &[]);
        db::set_item_files(&conn, &a.uuid, &["src/auth.rs".to_string()]).unwrap();

        let b = seed_memory(&conn, "model notes", "model details", &[], &[]);
        db::set_item_files(&conn, &b.uuid, &["src/model.rs".to_string()]).unwrap();

        seed_memory(&conn, "unrelated", "outside src", &[], &[]);

        let hits = collect_hits(&conn, "", &[], &[], &["src/".to_string()], 20).unwrap();
        assert_eq!(hits.len(), 2);
    }

    #[test]
    fn recall_file_and_tag_are_anded() {
        let conn = db::open_in_memory_for_test();

        let a = seed_memory(&conn, "auth notes", "JWT details", &["auth"], &[]);
        db::set_item_files(&conn, &a.uuid, &["src/auth.rs".to_string()]).unwrap();

        let b = seed_memory(&conn, "other auth notes", "other", &["auth"], &[]);
        db::set_item_files(&conn, &b.uuid, &["src/other.rs".to_string()]).unwrap();

        let hits = collect_hits(
            &conn,
            "",
            &["auth".to_string()],
            &[],
            &["src/auth.rs".to_string()],
            20,
        )
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].description, "auth notes");
    }

    #[test]
    fn recall_linked_tasks_are_surfaced_on_item_hit() {
        let conn = db::open_in_memory_for_test();

        let mut task = Task::new("fix auth".to_string(), "Sara".to_string());
        task.status = Status::Completed;
        db::insert_task(&conn, &mut task).unwrap();

        let item = seed_memory(&conn, "auth memory", "auth body", &["auth"], &[]);
        db::set_item_task_links(&conn, &item.uuid, &[(task.uuid, "explicit")]).unwrap();

        let hits = collect_hits(&conn, "", &["auth".to_string()], &[], &[], 20).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].linked_tasks.len(), 1);
        assert_eq!(hits[0].linked_tasks[0].0.description, "fix auth");
        assert_eq!(hits[0].linked_tasks[0].1, "explicit");
    }

    #[test]
    fn recall_surfaces_canonical_and_derived_memory_distinction() {
        let conn = db::open_in_memory_for_test();

        // Canonical: the pattern memory.
        let canonical = seed_memory(
            &conn,
            "CodeQL config pattern",
            "extract to codeql-config.yml and use query-filters",
            &["codeql"],
            &[],
        );
        // Derived: per-repo application memories.
        let derived_a = seed_memory(
            &conn,
            "CodeQL config applied to repo-a",
            "applied codeql pattern to repo-a",
            &["codeql"],
            &[],
        );
        let derived_b = seed_memory(
            &conn,
            "CodeQL config applied to repo-b",
            "applied codeql pattern to repo-b",
            &["codeql"],
            &[],
        );

        // Link derived memories to canonical.
        db::insert_memory_link(
            &conn,
            &derived_a.uuid.to_string(),
            &canonical.uuid.to_string(),
            "derived_from",
            1.0,
        )
        .unwrap();
        db::insert_memory_link(
            &conn,
            &derived_b.uuid.to_string(),
            &canonical.uuid.to_string(),
            "derived_from",
            1.0,
        )
        .unwrap();

        let hits = collect_hits(&conn, "", &["codeql".to_string()], &[], &[], 20).unwrap();
        assert_eq!(hits.len(), 3);

        // Find each hit by description.
        let canonical_hit = hits.iter().find(|h| h.description == "CodeQL config pattern").unwrap();
        let derived_a_hit = hits.iter().find(|h| h.description == "CodeQL config applied to repo-a").unwrap();
        let derived_b_hit = hits.iter().find(|h| h.description == "CodeQL config applied to repo-b").unwrap();

        // Canonical: has derived_count=2, no derived_from_labels.
        assert_eq!(canonical_hit.derived_count, 2, "canonical must report 2 derived memories");
        assert!(canonical_hit.derived_from_labels.is_empty(), "canonical must not be derived from anything");

        // Derived: has derived_count=0, derived_from_labels pointing to canonical.
        assert_eq!(derived_a_hit.derived_count, 0);
        assert_eq!(derived_a_hit.derived_from_labels.len(), 1);
        assert_eq!(derived_b_hit.derived_count, 0);
        assert_eq!(derived_b_hit.derived_from_labels.len(), 1);

        // Both derived memories point to the same canonical label.
        assert_eq!(derived_a_hit.derived_from_labels[0], derived_b_hit.derived_from_labels[0]);
    }
}

