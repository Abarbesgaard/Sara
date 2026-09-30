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

struct Hit {
    ref_kind: String,
    label: String,
    description: String,
    snippet: String,
    body: String,
    strength: f64,
    exact_match: bool,
    loose: bool,
    fts_rank: Option<usize>,
    modified: Option<DateTime<Utc>>,
    files: Vec<String>,
    linked_tasks: Vec<(Task, String)>,
    superseded_by: Vec<String>,
    provisional: bool,
    item_uuid: Option<uuid::Uuid>,
    derived_from_labels: Vec<String>,
    derived_children: Vec<String>,
    semantic: bool,
    cosine: Option<f32>,
    cluster: Option<ClusterInfo>,
}

#[derive(Clone, Debug)]
struct ClusterInfo {
    canonical_label: String,
    size: usize,
    collapsed_here: usize,
}

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

    let (confidence, caveat) = match_confidence(query, &tags, &hits);

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
        return ("none", "");
    }

    let all_exact = hits.iter().all(|h| h.exact_match);
    if all_exact || (has_exact_filters && !has_query) {
        return ("high", "");
    }

    if hits.iter().any(|h| h.semantic) {
        return (
            "semantic",
            "Includes semantic matches (embedding similarity, marked *): these \
             may share no literal keyword with the query — verify relevance.",
        );
    }

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
        for h in &hits {
            if let Some(u) = h.item_uuid {
                let _ = db::record_memory_recall(conn, &u);
            }
        }
        println!("Recent memories (no query given):");
    } else {
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
                let _ = db::record_memory_surfaced(conn, &r.item.uuid);
            }
        }
    }
    Ok(())
}

struct Related {
    item: Item,
    activation: f64,
    path: Vec<String>,
}

const ASSOCIATIVE_CAP: usize = 5;

const AUTO_SPREAD_HIT_FLOOR: usize = 3;

fn should_auto_spread(hits: &[Hit]) -> bool {
    let memory_hits = hits.iter().filter(|h| h.item_uuid.is_some()).count();
    (1..AUTO_SPREAD_HIT_FLOOR).contains(&memory_hits)
}

fn spreading_related(conn: &Connection, hits: &[Hit]) -> Result<Vec<Related>> {
    let seeds: Vec<uuid::Uuid> = hits.iter().filter_map(|h| h.item_uuid).collect();
    if seeds.is_empty() {
        return Ok(vec![]);
    }
    let graph = crate::infrastructure::memory::graph::MemoryGraph::build(conn)?;
    if graph.is_empty() {
        return Ok(vec![]);
    }
    let seed_set: HashSet<uuid::Uuid> = seeds.iter().copied().collect();
    let mut out = vec![];
    for act in graph.spread_activation_explained(&seeds, 2, 0.6, 1e-6) {
        if seed_set.contains(&act.uuid) {
            continue;
        }
        if let Ok(item) = db::get_item_by_uuid(conn, &act.uuid.to_string()) {
            out.push(Related {
                item,
                activation: act.activation,
                path: act.path,
            });
        }
        if out.len() >= ASSOCIATIVE_CAP {
            break;
        }
    }
    let max = out.iter().map(|r| r.activation).fold(0.0_f64, f64::max);
    if max > 0.0 {
        for r in &mut out {
            r.activation /= max;
        }
    }
    Ok(out)
}

fn normalize(values: &[String]) -> Vec<String> {
    values
        .iter()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .collect()
}

const STOP_WORDS: &[&str] = &[
    "a", "an", "the", "is", "in", "it", "of", "to", "for", "on", "at", "by", "up", "as", "or",
    "do", "if", "be", "we", "he", "she", "they", "but", "and", "not", "with", "from", "this",
    "that", "are", "was", "has", "have", "how", "what", "does", "did",
];
const MAX_AND_TOKENS: usize = 6;

fn meaningful_tokens(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphabetic())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() >= 3 && !STOP_WORDS.contains(&w.as_str()))
        .take(MAX_AND_TOKENS)
        .collect()
}

struct SemanticOpts {
    enabled: bool,
    threshold: f32,
    top_k: usize,
}

impl SemanticOpts {
    fn from_cfg(cfg: &Config) -> Self {
        SemanticOpts {
            enabled: true,
            threshold: cfg.recall.semantic_threshold,
            top_k: cfg.recall.semantic_top_k,
        }
    }

    fn off() -> Self {
        SemanticOpts {
            enabled: false,
            threshold: 1.0,
            top_k: 0,
        }
    }
}

fn merge_semantic_hits(
    conn: &Connection,
    query: &str,
    opts: &SemanticOpts,
    allowlist: Option<&HashSet<uuid::Uuid>>,
    hits: &mut Vec<Hit>,
) -> Result<()> {
    use crate::infrastructure::memory::embedding::{self, Embedder};

    let qv = embedding::bundled().embed(query);
    if qv.iter().all(|&x| x == 0.0) {
        return Ok(());
    }

    let already: HashSet<String> = hits
        .iter()
        .filter_map(|h| h.item_uuid.map(|u| u.to_string()))
        .collect();

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

    let exact_uuids: Option<HashSet<uuid::Uuid>> = match (file_uuids, tag_project_uuids) {
        (Some(f), Some(tp)) => Some(f.intersection(&tp).copied().collect()),
        (Some(f), None) => Some(f),
        (None, Some(tp)) => Some(tp),
        (None, None) => None,
    };

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
            let tokens = meaningful_tokens(query);
            if tokens.is_empty() {
                (vec![], false)
            } else {
                let and_hits = db::search_fts_tokens(conn, &tokens, limit.max(50))?;
                if !and_hits.is_empty() || tokens.len() < 2 {
                    (and_hits, false)
                } else {
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

    let mut hits = collapse_clusters(conn, hits);
    hits.truncate(limit.max(0) as usize);

    for h in &hits {
        if let Some(u) = h.item_uuid {
            let _ = db::record_memory_recall(conn, &u);
        }
    }

    Ok(hits)
}

fn family_key(h: &Hit) -> Option<String> {
    if let Some(parent) = h.derived_from_labels.first() {
        Some(parent.clone())
    } else if !h.derived_children.is_empty() {
        Some(h.label.clone())
    } else {
        None
    }
}

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
        Err(_) => 1,
    }
}

fn collapse_clusters(conn: &Connection, hits: Vec<Hit>) -> Vec<Hit> {
    let mut kept: Vec<Hit> = Vec::with_capacity(hits.len());
    let mut rep_of: std::collections::HashMap<(String, String), usize> =
        std::collections::HashMap::new();
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
                        if h.label == fam && kept[idx].label != fam {
                            kept[idx] = h;
                        }
                    }
                }
            }
        }
    }

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

const PATTERN_MIN_INSTANCES: usize = 2;

const PATTERN_TEXT_CAP: usize = 4000;

fn label_of(item: &Item) -> String {
    format!(
        "{}{}",
        item.kind.chars().next().unwrap_or('m'),
        item.display_id.unwrap_or(0)
    )
}

fn derived_child_labels(conn: &Connection, uuid: &str) -> Vec<String> {
    db::get_memory_links_to(conn, uuid)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "derived_from")
        .filter_map(|l| db::get_item_by_uuid(conn, &l.from_uuid).ok())
        .map(|i| label_of(&i))
        .collect()
}

struct PatternAcc {
    item: Item,
    instances: Vec<String>,
    matched: Vec<String>,
}

fn detect_patterns(conn: &Connection, hits: &[Hit]) -> Vec<serde_json::Value> {
    use std::collections::BTreeMap;
    let mut anchors: BTreeMap<String, PatternAcc> = BTreeMap::new();

    for h in hits {
        let Some(uuid) = h.item_uuid else { continue };
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

    patterns.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    patterns.into_iter().map(|(_, _, v)| v).collect()
}

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

fn keyword_json(hits: &[Hit]) -> Vec<serde_json::Value> {
    hits.iter()
        .enumerate()
        .map(|(i, h)| {
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

fn keyword_guide(h: &Hit) -> serde_json::Value {
    let mut map = serde_json::Map::new();
    map.insert("label".into(), json!(h.label));
    map.insert("preview".into(), json!(h.snippet));
    map.insert("strength".into(), json!(h.strength));
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
    serde_json::Value::Object(map)
}

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
    let superseded_by: Vec<String> = db::get_memory_links_to(conn, &item.uuid.to_string())
        .unwrap_or_default()
        .into_iter()
        .filter(|l| l.relation == "supersedes")
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
