use anyhow::Result;
use chrono::{DateTime, Utc};
use rusqlite::Connection;
use serde_json::json;
use std::collections::HashSet;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::{Item, Task};
use crate::infrastructure::project;

mod scoring;

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
    /// Short preview (≤160 chars) for the human terminal view.
    snippet: String,
    /// The complete, untruncated memory text — emitted in the JSON/MCP path so
    /// agents receive the full memory, not just the preview.
    body: String,
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
    /// Labels of memories that derive from this one (incoming `derived_from` edges).
    /// Non-empty means this is a canonical pattern memory; the labels are its
    /// per-application evidence cards, so recall can point straight at them.
    derived_children: Vec<String>,
    /// True when this hit was surfaced by semantic (embedding-cosine) matching
    /// rather than lexical FTS — it may share no literal token with the query.
    semantic: bool,
    /// Cosine similarity to the query for a semantic hit (`None` for lexical hits).
    cosine: Option<f32>,
    /// When this hit stands in for a whole canonical family (a pattern memory
    /// plus its per-application derived children), the family it represents.
    /// `Some` means recall collapsed the near-duplicate siblings into this one
    /// representative rather than returning every member — bounding the response
    /// and freeing the top-k for diverse memories. `None` for standalone hits.
    cluster: Option<ClusterInfo>,
}

/// A collapsed canonical family: the pattern memory (canonical) plus its
/// per-application derived children, represented in recall output by a single
/// highest-valued member instead of every near-duplicate. Lets the caller see
/// "one of a cluster of N related memories" and jump to the canonical, without
/// the whole family flooding the response.
#[derive(Clone, Debug)]
struct ClusterInfo {
    /// The canonical (root) memory's label, e.g. "m228" — the family's anchor.
    canonical_label: String,
    /// Total memories in the family: the canonical plus all its derived
    /// children (the *true* corpus size, not merely how many surfaced here).
    size: usize,
    /// How many sibling members of this family were collapsed OUT of this recall
    /// result to leave a single representative — for transparency.
    collapsed_here: usize,
}

/// Structured cross-task recall for the MCP `recall` tool and the `--json` CLI
/// path: keyword (FTS5) hits and exact tag/project hits.
pub fn recall_value(
    conn: &Connection,
    cfg: &Config,
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
    let files: Vec<String> = normalize(files)
        .iter()
        .map(|p| project::resolve_file_link_here(p))
        .collect();

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
        let patterns = detect_patterns(conn, &hits);
        return Ok(json!({
            "query": query,
            "tag": tags,
            "project": projects,
            "files": files,
            "keyword": keyword,
            "patterns": patterns,
            "associative": [],
            "confidence": "recent",
            "caveat": "Most recent memories (no query or filter given).",
            "recent": true,
        }));
    }

    // Drill-deeper: a bare memory handle like `m228` resolves that memory
    // directly and returns it in full plus its cluster as a guide — the "recall
    // deeper" affordance an agent uses after a lean recall surfaced the label.
    if let Some(item) = resolve_label_query(conn, query) {
        let _ = db::record_memory_recall(conn, &item.uuid);
        let hit = item_hit(conn, item, true);
        let related = spreading_related(conn, std::slice::from_ref(&hit))?;
        let associative = associative_guide(conn, &related);
        let label = hit.label.clone();
        return Ok(json!({
            "query": query,
            "tag": tags,
            "project": projects,
            "files": files,
            "keyword": keyword_json(std::slice::from_ref(&hit)),
            "associative": associative,
            "spread": "label",
            "confidence": "exact",
            "caveat": format!("Resolved memory {label} by label; `associative` maps its cluster."),
            "deep": true,
        }));
    }

    let hits = collect_hits(
        conn,
        query,
        &tags,
        &projects,
        &files,
        limit,
        &SemanticOpts::from_cfg(cfg),
    )?;
    let keyword = keyword_json(&hits);
    let patterns = detect_patterns(conn, &hits);

    // Match-confidence signal: distinguish "FTS found nothing" from "nothing exists".
    // Only meaningful when a free-text query drove the search (tag/file-only = high).
    let (confidence, caveat) = match_confidence(query, &tags, &hits);

    // Spreading activation: radiate from the direct memory hits across the graph
    // and return the associatively-related memories a keyword search misses, so
    // agents can pull in context that shares no literal term. Fires when either
    // explicitly requested (`--spread`) or a *free-text* query returned thin
    // literal hits (see `should_auto_spread`) — a plentiful lexical result stays
    // lexical, and a bare tag/file lookup (no query) is already precise so it
    // never auto-radiates. Surfaced memories are reinforced exactly like direct hits.
    let auto_spread = !spread && !query.trim().is_empty() && should_auto_spread(&hits);
    let do_spread = spread || auto_spread;
    let associative: Vec<_> = if do_spread {
        let related = spreading_related(conn, &hits)?;
        associative_guide(conn, &related)
    } else {
        vec![]
    };

    Ok(json!({
        "query": query,
        "tag": tags,
        "project": projects,
        "files": files,
        "keyword": keyword,
        "patterns": patterns,
        "associative": associative,
        "spread": if spread { "explicit" } else if auto_spread { "auto" } else { "off" },
        "confidence": confidence,
        "caveat": caveat,
    }))
}

/// Derive a match-confidence label and human-readable caveat from the search
/// inputs and results.
///
/// - `high`:   exact tag/project/file filters drove all hits (reliable index lookup).
/// - `medium`: some or all hits came from FTS keyword ranking (literal match only —
///   paraphrased or conceptually related content may not surface).
/// - `none`:   no hits at all AND a free-text query was involved — this does NOT
///   mean no similar work exists; it means no keywords overlapped.
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

    // Semantic hits present: recall matched by meaning (embedding cosine), so
    // the "literal-only, paraphrases may not surface" caveat no longer applies.
    if hits.iter().any(|h| h.semantic) {
        return (
            "semantic",
            "Includes semantic matches (embedding similarity, marked *): these \
             may share no literal keyword with the query — verify relevance.",
        );
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
    let files: Vec<String> = normalize(files)
        .iter()
        .map(|p| project::resolve_file_link_here(p))
        .collect();

    let recent = query.is_empty() && tags.is_empty() && projects.is_empty() && files.is_empty();

    // Drill-deeper: `sara recall m228` resolves the handle and prints that memory
    // in full plus its cluster, mirroring the JSON path's label lookup.
    if !recent && let Some(item) = resolve_label_query(conn, query) {
        let _ = db::record_memory_recall(conn, &item.uuid);
        let hit = item_hit(conn, item, true);
        println!("Memory {} (resolved by label):", hit.label);
        println!("  {}", hit.body.trim());
        let related = spreading_related(conn, std::slice::from_ref(&hit))?;
        if !related.is_empty() {
            println!("\nCluster (recall a label for its full text):");
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
                println!("  ~{label} ({:.2}): {}", r.activation, snippet.trim());
                let _ = db::record_memory_surfaced(conn, &r.item.uuid);
            }
        }
        return Ok(());
    }

    let hits = if recent {
        recent_hits(conn, limit)?
    } else {
        collect_hits(
            conn,
            query,
            &tags,
            &projects,
            &files,
            limit,
            &SemanticOpts::from_cfg(cfg),
        )?
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
        } else if h.semantic {
            "*"
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
                .map(|(t, src)| format!("#{} {} ({})", t.id.unwrap_or(0), t.description, src))
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
        let canonical_str = if h.derived_children.is_empty() {
            String::new()
        } else {
            format!(
                " [canonical, {} derived: {}]",
                h.derived_children.len(),
                h.derived_children.join(", ")
            )
        };
        let derived_from_str = if h.derived_from_labels.is_empty() {
            String::new()
        } else {
            format!(" [derived from: {}]", h.derived_from_labels.join(", "))
        };
        let cluster_str = match &h.cluster {
            Some(c) if c.collapsed_here > 0 => format!(
                " [cluster {} of {} — {} sibling(s) collapsed]",
                c.canonical_label, c.size, c.collapsed_here
            ),
            Some(c) => format!(" [cluster {} of {}]", c.canonical_label, c.size),
            None => String::new(),
        };
        println!(
            "  [{}] {} {} {}: {}{}{}{}{}{}{}{}{}",
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
            cluster_str,
            if age.is_empty() {
                String::new()
            } else {
                format!(" ({age})")
            }
        );
    }

    // Spreading activation: from the memories that matched directly, radiate
    // outward across the graph and surface the associatively-related memories a
    // flat keyword search would miss. Fires on explicit `--spread`, or
    // automatically when a non-bare query returned thin literal hits.
    let auto_spread = !spread && !recent && !query.trim().is_empty() && should_auto_spread(&hits);
    if spread || auto_spread {
        let related = spreading_related(conn, &hits)?;
        if !related.is_empty() {
            let header = if auto_spread {
                "Associatively related (auto-spread — literal hits were thin):"
            } else {
                "Associatively related (spreading activation):"
            };
            println!("\n{header}");
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
                // These surfaced only via spreading activation, not a deliberate
                // query — record them as *surfaced* so they still feed Hebbian
                // co-activation, but do NOT reinforce strength like a direct hit
                // would. Fire-and-forget.
                let _ = db::record_memory_surfaced(conn, &r.item.uuid);
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

/// Direct memory hits below this count are "thin" — too few for confidence that
/// the literal keyword search surfaced everything relevant. When recall is
/// *not* given an explicit `--spread`, thin results auto-radiate across the
/// graph so the caller still gets associatively-related context. Zero direct
/// memory hits cannot seed spreading activation, so auto-spread fires only with
/// `1..AUTO_SPREAD_HIT_FLOOR` seeds — a plentiful literal result set stays
/// lexical (and noise-free).
const AUTO_SPREAD_HIT_FLOOR: usize = 3;

/// Whether recall should auto-radiate: true when there are some (but few) direct
/// memory hits to seed from. Explicit `--spread` bypasses this and always spreads.
fn should_auto_spread(hits: &[Hit]) -> bool {
    let memory_hits = hits.iter().filter(|h| h.item_uuid.is_some()).count();
    (1..AUTO_SPREAD_HIT_FLOOR).contains(&memory_hits)
}

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
    "a", "an", "the", "is", "in", "it", "of", "to", "for", "on", "at", "by", "up", "as", "or",
    "do", "if", "be", "we", "he", "she", "they", "but", "and", "not", "with", "from", "this",
    "that", "are", "was", "has", "have", "how", "what", "does", "did",
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

/// Resolve query/tag/project/file inputs into a single ranked list of hits:
/// Strong (linkage-derived) memories first, then exact tag/project matches,
/// then plain FTS hits; ties broken by most-recently-modified.
/// Per-invocation semantic-recall settings. Semantic recall is **always on**:
/// `threshold`/`top_k` still come from `Config`, but `enabled` is never gated —
/// the legacy `[recall] semantic` toggle and `--semantic` flag are retained for
/// backward compatibility and no longer decide whether embeddings are ranked.
struct SemanticOpts {
    enabled: bool,
    threshold: f32,
    top_k: usize,
}

impl SemanticOpts {
    /// Semantic recall is ALWAYS enabled, so every free-text `sara recall` also
    /// ranks memories by embedding cosine and surfaces paraphrases that share no
    /// literal keyword. `threshold` and `top_k` are still read from config.
    fn from_cfg(cfg: &Config) -> Self {
        SemanticOpts {
            enabled: true,
            threshold: cfg.recall.semantic_threshold,
            top_k: cfg.recall.semantic_top_k,
        }
    }

    /// Lexical-only (semantic disabled) — the default used everywhere recall is
    /// not explicitly opted into semantic mode.
    fn off() -> Self {
        SemanticOpts {
            enabled: false,
            threshold: 1.0,
            top_k: 0,
        }
    }
}

/// Rank the stored memory embeddings against the query embedding and fold the
/// strongest matches into `hits` (deduped against memories already surfaced
/// lexically). This is what lets recall find a paraphrase that shares no literal
/// term with the query. Best-effort: any storage/embed hiccup leaves the lexical
/// hits untouched rather than breaking recall.
fn merge_semantic_hits(
    conn: &Connection,
    query: &str,
    opts: &SemanticOpts,
    allowlist: Option<&HashSet<uuid::Uuid>>,
    hits: &mut Vec<Hit>,
) -> Result<()> {
    use crate::infrastructure::embedding::{self, Embedder};

    let qv = embedding::bundled().embed(query);
    if qv.iter().all(|&x| x == 0.0) {
        return Ok(()); // query had no in-vocabulary content
    }

    let already: HashSet<String> = hits
        .iter()
        .filter_map(|h| h.item_uuid.map(|u| u.to_string()))
        .collect();

    // Drop lexical duplicates and any memory outside the exact-filter allowlist
    // *before* sorting/truncating, so `top_k` counts only memories that can
    // actually be surfaced. Filtering after `truncate` would let high-scoring
    // out-of-filter candidates consume slots and evict valid in-filter hits.
    let mut scored: Vec<(String, f32)> = db::active_embeddings(conn)?
        .into_iter()
        .filter(|(uuid, _)| {
            if already.contains(uuid) {
                return false;
            }
            match allowlist {
                Some(allow) => matches!(uuid::Uuid::parse_str(uuid), Ok(u) if allow.contains(&u)),
                None => true,
            }
        })
        .map(|(uuid, v)| (uuid, embedding::cosine(&qv, &v)))
        .filter(|(_, c)| *c >= opts.threshold)
        .collect();
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(opts.top_k);

    for (uuid, cos) in scored {
        if let Ok(item) = db::get_item_by_uuid(conn, &uuid) {
            let mut hit = item_hit(conn, item, false);
            hit.semantic = true;
            hit.cosine = Some(cos);
            hits.push(hit);
        }
    }
    Ok(())
}

fn collect_hits(
    conn: &Connection,
    query: &str,
    tags: &[String],
    projects: &[String],
    files: &[String],
    limit: i64,
    semantic: &SemanticOpts,
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
    let tag_project_uuids: Option<HashSet<uuid::Uuid>> = if !tags.is_empty() || !projects.is_empty()
    {
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
            let mut hit: HashSet<uuid::Uuid> = db::find_items_by_project(conn, project)?
                .into_iter()
                .map(|i| i.uuid)
                .collect();
            // Cross-project canonicals: a pattern memory with a derived
            // application scoped to `project` is relevant here too, even
            // though the canonical itself may live under a different
            // project (or none).
            hit.extend(db::find_cross_project_canonicals_for_project(
                conn, project,
            )?);
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

    // Build exact_items from the combined UUID set. Keep the UUID set itself as
    // the semantic allowlist: when exact filters are present, semantic recall
    // must stay inside them (AND semantics) rather than surfacing corpus-wide
    // paraphrases that carry none of the requested tag/project/file.
    let semantic_allowlist = exact_uuids.clone();
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
                    (
                        db::search_fts_tokens_or(conn, &tokens, limit.max(50))?,
                        true,
                    )
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
                    body: h.text.clone(),
                    strength: 1.0,
                    exact_match: false,
                    modified,
                    files: vec![],
                    linked_tasks: vec![],
                    superseded_by: vec![],
                    provisional: false,
                    item_uuid: None,
                    derived_from_labels: vec![],
                    derived_children: vec![],
                    fts_rank: Some(rank),
                    loose,
                    semantic: false,
                    cosine: None,
                    cluster: None,
                });
            }
        }
    }

    // Semantic (embedding) recall: rank the memory corpus by cosine to the
    // query embedding and fold in the strong matches a lexical search missed —
    // the whole point being to surface paraphrases sharing no literal token.
    // Off by default (config/flag), so lexical behaviour stays byte-identical.
    if semantic.enabled && !query.is_empty() {
        merge_semantic_hits(
            conn,
            query,
            semantic,
            semantic_allowlist.as_ref(),
            &mut hits,
        )?;
    }

    let hits = scoring::rank(hits);

    // Collapse canonical families: a pattern memory plus its per-application
    // derived children are near-duplicates; returning every one floods the
    // response (e.g. one dependabot fault with ~85 sibling memories) and eats
    // the top-k. Fold each surfaced family down to its single highest-valued
    // representative (already sorted first — the canonical, boosted by
    // item_strength's canonical_derived_bonus), tagging it with the family it
    // stands for. Done AFTER the sort (so the representative is the best member)
    // and BEFORE truncate (so `limit` counts distinct clusters, not siblings).
    let mut hits = collapse_clusters(conn, hits);
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

/// The canonical family a hit belongs to, or `None` if it stands alone.
/// A derived child is grouped under its canonical (its first `derived_from`
/// label); a canonical is grouped under itself; a plain memory (or task) has no
/// family and is never collapsed. When a memory is both derived and canonical
/// (a mid-tier), its parent link wins so it folds up, not down.
fn family_key(h: &Hit) -> Option<String> {
    if let Some(parent) = h.derived_from_labels.first() {
        Some(parent.clone())
    } else if !h.derived_children.is_empty() {
        Some(h.label.clone())
    } else {
        None
    }
}

/// Total members of a canonical family — the canonical plus all its derived
/// children (the *true* corpus size, independent of how many surfaced). When
/// the representative IS the canonical its `derived_children` already holds the
/// full set, so no query is needed; otherwise (only children surfaced) resolve
/// the canonical by label and count its incoming `derived_from` edges.
fn family_size(conn: &Connection, rep: &Hit, canonical_label: &str) -> usize {
    if rep.label == canonical_label {
        return 1 + rep.derived_children.len();
    }
    match db::get_item_by_handle(conn, canonical_label) {
        Ok(canon) => {
            let children = db::get_memory_links_to(conn, &canon.uuid.to_string())
                .unwrap_or_default()
                .into_iter()
                .filter(|l| l.relation == "derived_from")
                .count();
            1 + children
        }
        // Canonical not resolvable (shouldn't happen) — report only what we see.
        Err(_) => 1,
    }
}

/// Collapse each surfaced canonical family down to a single representative.
///
/// Walks the already-sorted hits, keeping the first (highest-valued) member of
/// each family and dropping the rest — but always promoting the canonical to be
/// the representative if it surfaces at all, since the canonical is the signal
/// (its derived children are per-application evidence). Standalone memories and
/// task hits pass through untouched. Each surviving representative of a family
/// with more than one member is tagged with `ClusterInfo` so the caller sees
/// "one of a cluster of N" and can jump to the canonical for the full family.
fn collapse_clusters(conn: &Connection, hits: Vec<Hit>) -> Vec<Hit> {
    let mut kept: Vec<Hit> = Vec::with_capacity(hits.len());
    // (family label, project signature) -> index of its representative in `kept`.
    // Keying on the project signature as well as the canonical family means a
    // member is only folded under a representative it actually shares a project
    // with — a foreign-project sibling (e.g. a legacy cross-repo canonical) is
    // never allowed to stand in for, or hide, a local memory.
    let mut rep_of: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
    // (family, project signature) -> how many members were folded out here.
    let mut collapsed: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();

    for h in hits {
        match family_key(&h) {
            None => kept.push(h),
            Some(fam) => {
                let key = (fam.clone(), hit_project_sig(conn, &h));
                match rep_of.get(&key).copied() {
                    None => {
                        rep_of.insert(key, kept.len());
                        kept.push(h);
                    }
                    Some(idx) => {
                        *collapsed.entry(key).or_insert(0) += 1;
                        // Promote the canonical to representative if it is the one
                        // arriving now and the current rep is merely a child.
                        if h.label == fam && kept[idx].label != fam {
                            kept[idx] = h;
                        }
                        // Otherwise the incoming member is dropped (folded in).
                    }
                }
            }
        }
    }

    // Tag each family representative with its cluster metadata.
    for h in kept.iter_mut() {
        if let Some(fam) = family_key(h) {
            let key = (fam.clone(), hit_project_sig(conn, h));
            let size = family_size(conn, h, &fam);
            if size > 1 {
                h.cluster = Some(ClusterInfo {
                    canonical_label: fam.clone(),
                    size,
                    collapsed_here: collapsed.get(&key).copied().unwrap_or(0),
                });
            }
        }
    }

    kept
}

/// Minimum number of prior applications (incoming `derived_from` edges) for a
/// canonical memory to be surfaced as a recurring *problem-solving pattern*.
/// One application is just a canonical-and-its-copy; a genuine reusable pattern
/// is one that has recurred, so require at least two.
const PATTERN_MIN_INSTANCES: usize = 2;

/// The full-body cap for a canonical memory's `text` in the patterns section.
/// Canonical recipes are the whole point here (the agent turns them into a
/// guide), so this is generous — it only guards against a pathological body.
const PATTERN_TEXT_CAP: usize = 4000;

/// Resolve a memory uuid to its `mN` handle.
fn label_of(item: &Item) -> String {
    format!(
        "{}{}",
        item.kind.chars().next().unwrap_or('m'),
        item.display_id.unwrap_or(0)
    )
}

/// Labels of the memories that derive from `uuid` (its incoming `derived_from`
/// edges) — i.e. the prior applications of a canonical pattern.
fn derived_child_labels(conn: &Connection, uuid: &str) -> Vec<String> {
    db::get_memory_links_to(conn, uuid)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .filter_map(|l| db::get_item_by_uuid(conn, &l.from_uuid).ok())
        .map(|i| label_of(&i))
        .collect()
}

/// Accumulator for one canonical anchor while scanning the hit set.
struct PatternAcc {
    item: Item,
    instances: Vec<String>,
    matched: Vec<String>,
}

/// From the resolved hit set, surface the canonical *problem-solving patterns*
/// the hits belong to.
///
/// A pattern is anchored on a **canonical memory** — one with
/// `>= PATTERN_MIN_INSTANCES` incoming `derived_from` edges — that is either a
/// direct hit or the parent of a hit. This promotes what is otherwise buried in
/// each hit's `derived_children`/`derived_from` flags into a dedicated,
/// actionable section: the canonical's full recipe body plus a `guide` string
/// telling the agent to turn it into a `sara add` task and link the outcome back
/// with `derived_from`, so recurring fixes are applied from a proven guide
/// instead of re-derived. Ranked most-recurrent first.
fn detect_patterns(conn: &Connection, hits: &[Hit]) -> Vec<serde_json::Value> {
    use std::collections::BTreeMap;
    let mut anchors: BTreeMap<String, PatternAcc> = BTreeMap::new();

    for h in hits {
        let Some(uuid) = h.item_uuid else { continue };
        // Candidate canonical anchors reachable from this hit:
        //  - the hit itself, when it is canonical (has derived children); and
        //  - each canonical this hit is derived from (its `derived_from` parents).
        let mut candidates: Vec<String> = Vec::new();
        if !h.derived_children.is_empty() {
            candidates.push(uuid.to_string());
        }
        for l in db::get_memory_links_from(conn, &uuid.to_string()).unwrap_or_default() {
            if l.relation == "derived_from" {
                candidates.push(l.to_uuid);
            }
        }

        for anchor_uuid in candidates {
            let entry = anchors.entry(anchor_uuid.clone());
            let acc = match entry {
                std::collections::btree_map::Entry::Occupied(o) => o.into_mut(),
                std::collections::btree_map::Entry::Vacant(v) => {
                    let Ok(item) = db::get_item_by_uuid(conn, &anchor_uuid) else {
                        continue;
                    };
                    let instances = derived_child_labels(conn, &anchor_uuid);
                    v.insert(PatternAcc {
                        item,
                        instances,
                        matched: Vec::new(),
                    })
                }
            };
            if !acc.matched.contains(&h.label) {
                acc.matched.push(h.label.clone());
            }
        }
    }

    let mut patterns: Vec<(usize, usize, serde_json::Value)> = anchors
        .into_values()
        .filter(|a| a.instances.len() >= PATTERN_MIN_INSTANCES)
        .map(|a| {
            let label = label_of(&a.item);
            let occurrences = a.instances.len();
            let title = a.item.title.trim().to_string();
            let text: String = a.item.body.chars().take(PATTERN_TEXT_CAP).collect();
            let strength = db::item_strength(conn, &a.item);
            let guide = format!(
                "Recurring pattern: '{label}' has been applied {occurrences} times before \
                 (see `instances`). The canonical `text` above is the proven approach — \
                 rather than re-deriving it, create a task with `add` using it as the \
                 step-by-step guide, then `learn` the outcome and link that memory \
                 `derived_from {label}` so the pattern keeps strengthening."
            );
            let matched = a.matched.clone();
            let v = json!({
                "canonical": label,
                "title": title,
                "text": text,
                "strength": strength,
                "occurrences": occurrences,
                "instances": a.instances,
                "matched_hits": matched,
                "guide": guide,
            });
            (occurrences, a.matched.len(), v)
        })
        .collect();

    // Most-recurrent first; break ties by how many of THIS recall's hits the
    // pattern claimed (more matched = more central to the query).
    patterns.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    patterns.into_iter().map(|(_, _, v)| v).collect()
}

/// A stable signature of the projects a memory hit belongs to, used to keep
/// canonical-family collapsing within a project boundary. Task hits and
/// project-less memories yield an empty signature.
fn hit_project_sig(conn: &Connection, h: &Hit) -> String {
    match h.item_uuid {
        Some(u) => {
            let mut ps = db::get_item_projects(conn, &u).unwrap_or_default();
            ps.sort();
            ps.join("\u{1f}")
        }
        None => String::new(),
    }
}

/// Serialize recall hits into the `keyword` JSON array shared by the bare-recall
/// and query-driven paths.
/// Resolve a query that is a bare memory handle (`m<NN>`) to its item, for the
/// "recall deeper" drill-in. Returns `None` for ordinary free-text queries so
/// they still go through keyword/associative search. Only memory handles (`m…`)
/// qualify — not `n`/`l` handles or uuid prefixes — since recall is memory-facing.
fn resolve_label_query(conn: &Connection, query: &str) -> Option<Item> {
    let q = query.trim();
    let is_memory_handle = q.len() >= 2
        && (q.starts_with('m') || q.starts_with('M'))
        && q[1..].chars().all(|c| c.is_ascii_digit());
    if !is_memory_handle {
        return None;
    }
    db::get_item_by_handle(conn, q).ok()
}

/// Render spreading-activation neighbours as a compact **guide** — label,
/// ≤160-char preview, activation, strength, and the synaptic path — with NO full
/// body. The associative array is a map to the cluster, not a payload; an agent
/// pulls any neighbour in full by re-calling `recall` with its `mNN` label.
fn associative_guide(conn: &Connection, related: &[Related]) -> Vec<serde_json::Value> {
    related
        .iter()
        .map(|r| {
            let _ = db::record_memory_surfaced(conn, &r.item.uuid);
            json!({
                "label": format!("m{}", r.item.display_id.unwrap_or(0)),
                "preview": r.item.summary.clone().unwrap_or_else(|| r.item.body.clone()).chars().take(160).collect::<String>(),
                "activation": r.activation,
                "strength": db::item_strength(conn, &r.item),
                "via": r.path,
            })
        })
        .collect()
}

/// Render the keyword hits as JSON. Only the **top hit** comes back in full —
/// its complete metadata envelope plus the untruncated `text`. Every other hit
/// collapses to a lean guide entry (`label`, ≤160-char `preview`, `strength`,
/// and — only when they carry signal — cluster/canonical/derivation and
/// linked-task handles), with none of the null/empty scaffolding fields. The
/// model gets one memory in full plus a compact map of the cluster it belongs
/// to, and can drill into any sibling by re-calling `recall` with that memory's
/// `mNN` label. This keeps the MCP payload lean instead of dumping every matched
/// memory's full envelope.
fn keyword_json(hits: &[Hit]) -> Vec<serde_json::Value> {
    hits.iter()
        .enumerate()
        .map(|(i, h)| {
            // Only the top hit is returned in full detail (all metadata + body);
            // every other hit collapses to a lean guide entry so the payload
            // stays small.
            if i == 0 {
                json!({
                    "ref_kind": h.ref_kind,
                    "label": h.label,
                    "description": h.description,
                    "preview": h.snippet,
                    "text": h.body,
                    "strength": h.strength,
                    "exact_match": h.exact_match,
                    "loose": h.loose,
                    "semantic": h.semantic,
                    "cosine": h.cosine,
                    "modified": h.modified.map(|m| m.to_rfc3339()),
                    "files": h.files,
                    "superseded_by": h.superseded_by,
                    "provisional": h.provisional,
                    "canonical": !h.derived_children.is_empty(),
                    "derived_count": h.derived_children.len(),
                    "derived_children": h.derived_children,
                    "derived_from": h.derived_from_labels,
                    "cluster": h.cluster.as_ref().map(|c| json!({
                        "canonical": c.canonical_label,
                        "size": c.size,
                        "collapsed_here": c.collapsed_here,
                    })),
                    "linked_tasks": h.linked_tasks.iter().map(|(t, src)| json!({
                        "id": t.id.unwrap_or(0),
                        "description": t.description,
                        "source": src,
                    })).collect::<Vec<_>>(),
                })
            } else {
                keyword_guide(h)
            }
        })
        .collect()
}

/// Compact guide rendering of a non-top keyword hit: enough to identify the
/// memory, gauge its weight, and know where it sits in its cluster so the model
/// can drill into it with `recall("mNN")` — but no full body and none of the
/// null/empty scaffolding fields that bloat the payload. Cluster/canonical,
/// derivation, and linked-task pointers are only emitted when they carry signal.
fn keyword_guide(h: &Hit) -> serde_json::Value {
    let mut o = json!({
        "label": h.label,
        "preview": h.snippet,
        "strength": h.strength,
    });
    let map = o.as_object_mut().expect("json object");
    if !h.derived_children.is_empty() {
        map.insert("canonical".into(), json!(true));
    }
    if !h.derived_from_labels.is_empty() {
        map.insert("derived_from".into(), json!(h.derived_from_labels));
    }
    if let Some(c) = h.cluster.as_ref() {
        map.insert(
            "cluster".into(),
            json!({
                "canonical": c.canonical_label,
                "size": c.size,
                "collapsed_here": c.collapsed_here,
            }),
        );
    }
    if !h.superseded_by.is_empty() {
        map.insert("superseded_by".into(), json!(h.superseded_by));
    }
    if !h.linked_tasks.is_empty() {
        map.insert(
            "linked_tasks".into(),
            json!(
                h.linked_tasks
                    .iter()
                    .map(|(t, _src)| t.id.unwrap_or(0))
                    .collect::<Vec<_>>()
            ),
        );
    }
    o
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
                .map(|i| {
                    format!(
                        "{}{}",
                        i.kind.chars().next().unwrap_or('m'),
                        i.display_id.unwrap_or(0)
                    )
                })
                .unwrap_or_else(|| l.from_uuid.chars().take(8).collect::<String>())
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
                .map(|i| {
                    format!(
                        "{}{}",
                        i.kind.chars().next().unwrap_or('m'),
                        i.display_id.unwrap_or(0)
                    )
                })
                .unwrap_or_else(|| l.to_uuid.chars().take(8).collect::<String>())
        })
        .collect();
    // Incoming `derived_from` edges — other memories derive from this canonical.
    let derived_children: Vec<String> = db::get_memory_links_to(conn, &item.uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .map(|l| {
            db::get_item_by_uuid(conn, &l.from_uuid)
                .ok()
                .map(|i| {
                    format!(
                        "{}{}",
                        i.kind.chars().next().unwrap_or('m'),
                        i.display_id.unwrap_or(0)
                    )
                })
                .unwrap_or_else(|| l.from_uuid.chars().take(8).collect::<String>())
        })
        .collect();
    Hit {
        ref_kind: format!("item_{}", item.kind),
        strength: db::item_strength(conn, &item),
        label: handle,
        description: item.title.clone(),
        snippet,
        body: item.body.clone(),
        exact_match,
        modified: Some(item.modified),
        files,
        linked_tasks,
        superseded_by,
        provisional: item.status == "provisional",
        item_uuid: Some(item.uuid),
        derived_from_labels,
        derived_children,
        fts_rank: None,
        loose: false,
        semantic: false,
        cosine: None,
        cluster: None,
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
#[path = "../../../tests/unit/commands/recall/mod.rs"]
mod tests;
