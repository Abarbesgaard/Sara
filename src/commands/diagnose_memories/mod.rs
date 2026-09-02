use anyhow::Result;
use rusqlite::Connection;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

use crate::infrastructure::{db, embedding};

/// Cosine floor for calling two memories a genuine conflict candidate.
///
/// Sharing a file or a tag proves *co-occurrence*, not contradiction: on a real
/// store 84% of file-overlap pairs share exactly one path, and the pass is
/// quadratic in memories-per-file, so a single hub file drags in hundreds of
/// unrelated pairs. Scoring those pairs by embedding cosine gives a median of
/// ~0.55 — the median "conflict" is barely on topic. A 0.75 floor removes ~94%
/// of them while keeping every hand-verified duplicate.
///
/// Deliberately higher than `recall`'s general threshold (0.30) and than
/// `insight`'s `RELATED_THRESHOLD` (0.55) for the same reason those were
/// chosen: a false candidate is noise a human or agent must burn attention on,
/// so this pass buys precision with recall.
pub const DEFAULT_CONFLICT_THRESHOLD: f32 = 0.75;

/// A pair of memories that may be in conflict (no memory_links edge between them).
#[derive(Debug)]
pub struct ConflictCandidate {
    pub label_a: String,
    pub label_b: String,
    pub snippet_a: String,
    pub snippet_b: String,
    pub shared_files: Vec<String>,
    pub shared_tags: Vec<String>,
    /// Embedding cosine between the two bodies. `None` when either memory has
    /// no embedding row yet — such a pair is kept (fail-open) so a lagging
    /// index never silently hides a candidate.
    pub cosine: Option<f32>,
}

/// Core shared by CLI and MCP. Scans active memories and returns conflict
/// candidates: pairs sharing file-path links OR full tag sets, with no
/// `memory_links` edge between them in either direction, **and** whose bodies
/// are semantically close enough (`cosine >= threshold`) to be worth review.
///
/// `project` scopes the scan to memories linked to that project (both sides of
/// a pair must belong to it), so a scan run in one province does not report
/// another's. `limit` caps the returned list — candidates are always sorted by
/// cosine descending, so the worst offender is first and a cap is a clean
/// truncation rather than an arbitrary sample.
pub fn diagnose_value(
    conn: &Connection,
    threshold: f32,
    project: Option<&str>,
    limit: Option<usize>,
) -> Result<Value> {
    let memories = db::list_memories(conn)?;

    // Build a set of (uuid, uuid) pairs that already have a link edge.
    let all_links = db::all_memory_links(conn)?;
    let linked_pairs: HashSet<(String, String)> = all_links
        .iter()
        .flat_map(|l| {
            // Treat edges as undirected for conflict-suppression purposes.
            [
                (l.from_uuid.clone(), l.to_uuid.clone()),
                (l.to_uuid.clone(), l.from_uuid.clone()),
            ]
        })
        .collect();

    // Embeddings, keyed by uuid — the topical signal that separates a real
    // conflict from two memories that merely touched the same file.
    let vectors: HashMap<String, Vec<f32>> = db::active_embeddings(conn)?.into_iter().collect();

    // For each memory, collect its files and normalised tags.
    struct MemInfo {
        uuid: String,
        label: String,
        body: String,
        files: Vec<String>,
        tags: Vec<String>,
    }

    let mut infos: Vec<MemInfo> = Vec::new();
    for m in &memories {
        if let Some(scope) = project {
            let projects = db::get_item_projects(conn, &m.uuid).unwrap_or_default();
            if !projects.iter().any(|p| p.eq_ignore_ascii_case(scope)) {
                continue;
            }
        }
        let uuid = m.uuid.to_string();
        let files = db::get_item_files(conn, &m.uuid).unwrap_or_default();
        let label = m
            .display_id
            .map(|id| format!("m{id}"))
            .unwrap_or_else(|| uuid[..8].to_string());
        let tags: Vec<String> = m.tags.iter().map(|t| t.to_lowercase()).collect();
        infos.push(MemInfo {
            uuid,
            label,
            body: m.body.clone(),
            files,
            tags,
        });
    }

    // Index memories by file path.
    let mut by_file: HashMap<String, Vec<usize>> = HashMap::new();
    for (i, info) in infos.iter().enumerate() {
        for f in &info.files {
            by_file.entry(f.clone()).or_default().push(i);
        }
    }

    // Cosine between two memories, or None when either lacks an embedding.
    let score = |a: &MemInfo, b: &MemInfo| -> Option<f32> {
        let va = vectors.get(&a.uuid)?;
        let vb = vectors.get(&b.uuid)?;
        Some(embedding::cosine(va, vb))
    };

    // Collect conflict candidates — deduplicate with a seen set.
    let mut seen: HashSet<(usize, usize)> = HashSet::new();
    let mut candidates: Vec<ConflictCandidate> = Vec::new();

    // 1. File-path overlap.
    for indices in by_file.values() {
        for &i in indices {
            for &j in indices {
                if i >= j {
                    continue;
                }
                let key = (i.min(j), i.max(j));
                if seen.contains(&key) {
                    continue;
                }
                let a = &infos[i];
                let b = &infos[j];
                if linked_pairs.contains(&(a.uuid.clone(), b.uuid.clone())) {
                    seen.insert(key);
                    continue;
                }
                // Fail-open: an unscorable pair (missing embedding) is kept.
                let cosine = score(a, b);
                if cosine.is_some_and(|c| c < threshold) {
                    seen.insert(key);
                    continue;
                }
                let shared_files: Vec<String> = a
                    .files
                    .iter()
                    .filter(|f| b.files.contains(f))
                    .cloned()
                    .collect();
                candidates.push(ConflictCandidate {
                    label_a: a.label.clone(),
                    label_b: b.label.clone(),
                    snippet_a: a.body.chars().take(80).collect(),
                    snippet_b: b.body.chars().take(80).collect(),
                    shared_files,
                    shared_tags: vec![],
                    cosine,
                });
                seen.insert(key);
            }
        }
    }

    // 2. Full tag-set overlap (all tags identical, no link).
    for i in 0..infos.len() {
        for j in (i + 1)..infos.len() {
            let key = (i, j);
            if seen.contains(&key) {
                continue;
            }
            let a = &infos[i];
            let b = &infos[j];
            if a.tags.is_empty() || b.tags.is_empty() {
                continue;
            }
            let a_set: HashSet<&String> = a.tags.iter().collect();
            let b_set: HashSet<&String> = b.tags.iter().collect();
            if a_set != b_set {
                continue;
            }
            if linked_pairs.contains(&(a.uuid.clone(), b.uuid.clone())) {
                seen.insert(key);
                continue;
            }
            let cosine = score(a, b);
            if cosine.is_some_and(|c| c < threshold) {
                seen.insert(key);
                continue;
            }
            candidates.push(ConflictCandidate {
                label_a: a.label.clone(),
                label_b: b.label.clone(),
                snippet_a: a.body.chars().take(80).collect(),
                snippet_b: b.body.chars().take(80).collect(),
                shared_files: vec![],
                shared_tags: a.tags.clone(),
                cosine,
            });
            seen.insert(key);
        }
    }

    // Worst offender first. Unscorable pairs (no embedding) sort last rather
    // than masquerading as top hits. The previous order was `HashMap`
    // iteration order, i.e. reshuffled between runs.
    candidates.sort_by(|a, b| {
        let ka = a.cosine.unwrap_or(f32::NEG_INFINITY);
        let kb = b.cosine.unwrap_or(f32::NEG_INFINITY);
        kb.partial_cmp(&ka)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.label_a.cmp(&b.label_a))
            .then_with(|| a.label_b.cmp(&b.label_b))
    });

    let total = candidates.len();
    if let Some(n) = limit {
        candidates.truncate(n);
    }

    let items: Vec<Value> = candidates
        .iter()
        .map(|c| {
            json!({
                "label_a":      c.label_a,
                "label_b":      c.label_b,
                "snippet_a":    c.snippet_a,
                "snippet_b":    c.snippet_b,
                "shared_files": c.shared_files,
                "shared_tags":  c.shared_tags,
                "cosine":       c.cosine,
            })
        })
        .collect();

    Ok(json!({
        "conflicts": items,
        "count": items.len(),
        "total": total,
        "threshold": threshold,
        "project": project,
    }))
}

/// `sara diagnose-memories` (alias: `sara conflicts`)
pub fn run(
    conn: &Connection,
    json_output: bool,
    threshold: f32,
    project: Option<&str>,
    limit: Option<usize>,
) -> Result<()> {
    let v = diagnose_value(conn, threshold, project, limit)?;
    let count = v["count"].as_u64().unwrap_or(0);
    let total = v["total"].as_u64().unwrap_or(count);

    if json_output {
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }

    if count == 0 {
        println!("No unlinked conflict candidates found (cosine >= {threshold:.2}).");
        return Ok(());
    }

    let scope = project
        .map(|p| format!(" in project '{p}'"))
        .unwrap_or_default();
    println!(
        "{total} potential conflict(s){scope} — related memories (cosine >= {threshold:.2}) sharing files or tags with no link:"
    );
    if count < total {
        println!("Showing the {count} closest; pass --limit to widen.");
    }
    println!();
    if let Some(conflicts) = v["conflicts"].as_array() {
        for c in conflicts {
            let la = c["label_a"].as_str().unwrap_or("?");
            let lb = c["label_b"].as_str().unwrap_or("?");
            let sa = c["snippet_a"].as_str().unwrap_or("");
            let sb = c["snippet_b"].as_str().unwrap_or("");
            let score = c["cosine"]
                .as_f64()
                .map(|c| format!("{c:.2}"))
                .unwrap_or_else(|| "—".to_string());

            let shared_files: Vec<&str> = c["shared_files"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();
            let shared_tags: Vec<&str> = c["shared_tags"]
                .as_array()
                .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
                .unwrap_or_default();

            if !shared_files.is_empty() {
                let names: Vec<&str> = shared_files
                    .iter()
                    .map(|p| {
                        std::path::Path::new(p)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(p)
                    })
                    .collect();
                println!("  {la} ↔ {lb}  [{score}] [file: {}]", names.join(", "));
            } else {
                println!(
                    "  {la} ↔ {lb}  [{score}] [tags: {}]",
                    shared_tags.join(", ")
                );
            }
            println!("    {la}: {sa}");
            println!("    {lb}: {sb}");
            println!();
        }
    }

    println!(
        "Resolve with: sara relearn <label> / sara learn --supersedes <label> / sara link-memory <a> similar_to <b>"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::infrastructure::{db, embedding, model::Item};
    use uuid::Uuid;

    /// Seed a memory with a file link, a tag and a real embedding, so the
    /// cosine gate is exercised rather than bypassed by the fail-open path.
    fn insert_memory_with_file(
        conn: &rusqlite::Connection,
        body: &str,
        tag: &str,
        file: &str,
    ) -> Uuid {
        let mut item = Item::new_memory(body.to_string(), body.to_string(), None);
        item.tags = vec![tag.to_string()];
        item.path = Some(String::new());
        db::insert_item(conn, &mut item).unwrap();
        db::set_item_projects(conn, &item.uuid, &["test".to_string()]).unwrap();
        db::set_item_files(conn, &item.uuid, &[file.to_string()]).unwrap();
        embedding::index_memory(conn, &item);
        item.uuid
    }

    const NEAR_A: &str =
        "dependabot bumped NSubstitute to 6.2.0 which broke dotnet restore with NU1608";
    const NEAR_B: &str =
        "the dependabot NSubstitute 6.2.0 bump broke the dotnet restore build with NU1608";
    const FAR: &str = "sourdough bread proofs best overnight in a cold refrigerator";

    #[test]
    fn near_duplicate_pair_on_same_file_is_reported() {
        let conn = db::open_in_memory_for_test();
        let file = "/tmp/sara_diag_near.rs".to_string();
        insert_memory_with_file(&conn, NEAR_A, "tag-a", &file);
        insert_memory_with_file(&conn, NEAR_B, "tag-b", &file);

        let v =
            super::diagnose_value(&conn, super::DEFAULT_CONFLICT_THRESHOLD, None, None).unwrap();
        assert_eq!(
            v["count"].as_u64().unwrap(),
            1,
            "a topically near-identical pair on one file is a real candidate"
        );
        let c = v["conflicts"][0]["cosine"].as_f64().unwrap();
        assert!(
            c >= super::DEFAULT_CONFLICT_THRESHOLD as f64,
            "reported cosine {c} must clear the threshold"
        );
    }

    #[test]
    fn unrelated_pair_on_same_file_is_not_reported() {
        let conn = db::open_in_memory_for_test();
        let file = "/tmp/sara_diag_far.rs".to_string();
        insert_memory_with_file(&conn, NEAR_A, "tag-a", &file);
        insert_memory_with_file(&conn, FAR, "tag-b", &file);

        let v =
            super::diagnose_value(&conn, super::DEFAULT_CONFLICT_THRESHOLD, None, None).unwrap();
        assert_eq!(
            v["count"].as_u64().unwrap(),
            0,
            "sharing a file is co-occurrence, not conflict — the cosine gate must drop this"
        );
    }

    #[test]
    fn pair_without_embeddings_is_kept_fail_open() {
        let conn = db::open_in_memory_for_test();
        let file = "/tmp/sara_diag_noemb.rs".to_string();
        let a = insert_memory_with_file(&conn, NEAR_A, "tag-a", &file);
        let b = insert_memory_with_file(&conn, FAR, "tag-b", &file);
        // Simulate a lagging index: drop both embedding rows.
        for u in [a, b] {
            db::delete_embedding(&conn, &u.to_string()).unwrap();
        }

        let v =
            super::diagnose_value(&conn, super::DEFAULT_CONFLICT_THRESHOLD, None, None).unwrap();
        assert_eq!(
            v["count"].as_u64().unwrap(),
            1,
            "an unscorable pair must be kept, never silently hidden"
        );
        assert!(v["conflicts"][0]["cosine"].is_null());
    }

    #[test]
    fn candidates_are_sorted_by_cosine_descending() {
        let conn = db::open_in_memory_for_test();
        let file = "/tmp/sara_diag_sort.rs".to_string();
        insert_memory_with_file(&conn, NEAR_A, "tag-a", &file);
        insert_memory_with_file(&conn, NEAR_B, "tag-b", &file);
        insert_memory_with_file(
            &conn,
            "NSubstitute version pinning is handled in Directory.Packages.props",
            "tag-c",
            &file,
        );

        // Threshold 0 keeps every pair so the ordering itself is under test.
        let v = super::diagnose_value(&conn, 0.0, None, None).unwrap();
        let scores: Vec<f64> = v["conflicts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["cosine"].as_f64().unwrap())
            .collect();
        assert!(scores.len() >= 2, "expected several pairs to order");
        assert!(
            scores.windows(2).all(|w| w[0] >= w[1]),
            "worst offender must come first, got {scores:?}"
        );
    }

    #[test]
    fn limit_truncates_but_total_reports_the_full_count() {
        let conn = db::open_in_memory_for_test();
        let file = "/tmp/sara_diag_limit.rs".to_string();
        insert_memory_with_file(&conn, NEAR_A, "tag-a", &file);
        insert_memory_with_file(&conn, NEAR_B, "tag-b", &file);
        insert_memory_with_file(&conn, FAR, "tag-c", &file);

        let v = super::diagnose_value(&conn, 0.0, None, Some(1)).unwrap();
        assert_eq!(v["count"].as_u64().unwrap(), 1);
        assert!(
            v["total"].as_u64().unwrap() > 1,
            "total must report the untruncated count"
        );
    }

    #[test]
    fn project_scope_excludes_other_provinces() {
        let conn = db::open_in_memory_for_test();
        let file = "/tmp/sara_diag_scope.rs".to_string();
        let a = insert_memory_with_file(&conn, NEAR_A, "tag-a", &file);
        insert_memory_with_file(&conn, NEAR_B, "tag-b", &file);
        // Move one side of the pair into a different project.
        db::set_item_projects(&conn, &a, &["elsewhere".to_string()]).unwrap();

        let scoped =
            super::diagnose_value(&conn, super::DEFAULT_CONFLICT_THRESHOLD, Some("test"), None)
                .unwrap();
        assert_eq!(
            scoped["count"].as_u64().unwrap(),
            0,
            "a pair straddling two projects must not surface in either scope"
        );

        let unscoped =
            super::diagnose_value(&conn, super::DEFAULT_CONFLICT_THRESHOLD, None, None).unwrap();
        assert_eq!(unscoped["count"].as_u64().unwrap(), 1);
    }

    #[test]
    fn linked_memories_do_not_appear_in_diagnose() {
        let conn = db::open_in_memory_for_test();
        let file = "/tmp/sara_diag_linked_test.rs".to_string();
        let uuid_a = insert_memory_with_file(&conn, NEAR_A, "tag-c", &file);
        let uuid_b = insert_memory_with_file(&conn, NEAR_B, "tag-d", &file);

        // Link them — they should disappear from diagnose output.
        db::insert_memory_link(
            &conn,
            &uuid_a.to_string(),
            &uuid_b.to_string(),
            "supersedes",
            1.0,
        )
        .unwrap();

        let v =
            super::diagnose_value(&conn, super::DEFAULT_CONFLICT_THRESHOLD, None, None).unwrap();
        assert_eq!(v["count"].as_u64().unwrap(), 0);
    }
}
