use super::super::*;
use crate::infrastructure::model::Item;
use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use uuid::Uuid;

#[derive(Debug)]
pub struct PruneCandidate {
    pub label: String,
    pub uuid: String,
    pub title: String,
    pub reason: &'static str,
}

pub fn prune_memories(
    conn: &Connection,
    weak_days: i64,
    provisional_days: i64,
    dry_run: bool,
) -> Result<Vec<PruneCandidate>> {
    let all = {
        let mut stmt = conn.prepare(
            "SELECT uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status, source_task_uuid
             FROM items WHERE kind='memory' AND status IN ('active','provisional') ORDER BY created ASC",
        )?;
        let rows = stmt.query_map([], row_to_item)?;
        rows.filter_map(|r| r.ok()).collect::<Vec<_>>()
    };

    let now = Utc::now();
    let strengths = item_strengths(conn, &all);
    let mut candidates: Vec<PruneCandidate> = vec![];
    let mut superseded_stmt = conn.prepare(
        "SELECT COUNT(*) FROM memory_links ml \
         JOIN items newer ON newer.uuid = ml.from_uuid \
            AND newer.status = 'active' AND newer.kind = 'memory' \
         WHERE ml.to_uuid = ?1 AND ml.relation = 'supersedes'",
    )?;

    for item in &all {
        let label = format!(
            "{}{}",
            item.kind.chars().next().unwrap_or('m'),
            item.display_id.unwrap_or(0)
        );
        let age_days = (now - item.created).num_days();

        let superseded = {
            let count: i64 = superseded_stmt.query_row([item.uuid.to_string()], |r| r.get(0))?;
            count > 0
        };
        if superseded {
            candidates.push(PruneCandidate {
                label,
                uuid: item.uuid.to_string(),
                title: item.title.clone(),
                reason: "superseded by a newer memory",
            });
            continue;
        }

        if item.status == "provisional" && age_days >= provisional_days {
            candidates.push(PruneCandidate {
                label,
                uuid: item.uuid.to_string(),
                title: item.title.clone(),
                reason: "provisional auto-memory not reviewed within time limit",
            });
            continue;
        }

        let strength = strengths.get(&item.uuid).copied().unwrap_or(1.0);
        if strength < 1.5 && age_days >= weak_days {
            candidates.push(PruneCandidate {
                label,
                uuid: item.uuid.to_string(),
                title: item.title.clone(),
                reason: "weak memory (no task link) older than age threshold",
            });
        }
    }

    if !dry_run {
        for c in &candidates {
            conn.execute(
                "UPDATE items SET status='archived', modified=?1 WHERE uuid=?2",
                rusqlite::params![dt_to_str(&now), c.uuid],
            )?;
        }
    }

    Ok(candidates)
}

#[derive(Debug, Default, Clone)]
pub struct ReviewSummary {
    pub count: usize,
    pub oldest_age_days: i64,
}

pub fn archive_superseded_memories(conn: &Connection) -> Result<Vec<PruneCandidate>> {
    let now = Utc::now();
    let mut stmt = conn.prepare(
        "SELECT old.uuid, old.kind, old.display_id, old.title, old.url, old.project, \
                old.tags_json, old.path, old.summary, old.body, old.created, old.modified, \
                old.status, old.source_task_uuid \
         FROM items old \
         JOIN memory_links ml ON ml.to_uuid = old.uuid AND ml.relation = 'supersedes' \
         JOIN items newer ON newer.uuid = ml.from_uuid AND newer.status = 'active' AND newer.kind = 'memory' \
         WHERE old.kind = 'memory' AND old.status IN ('active','provisional') \
         GROUP BY old.uuid",
    )?;
    let rows = stmt.query_map([], row_to_item)?;
    let items: Vec<_> = rows.filter_map(|r| r.ok()).collect();

    let mut archived: Vec<PruneCandidate> = vec![];
    for item in &items {
        let label = format!(
            "{}{}",
            item.kind.chars().next().unwrap_or('m'),
            item.display_id.unwrap_or(0)
        );
        conn.execute(
            "UPDATE items SET status='archived', modified=?1 WHERE uuid=?2",
            rusqlite::params![dt_to_str(&now), item.uuid.to_string()],
        )?;
        archived.push(PruneCandidate {
            label,
            uuid: item.uuid.to_string(),
            title: item.title.clone(),
            reason: "superseded by a newer memory",
        });
    }
    Ok(archived)
}

pub fn count_review_candidates(
    conn: &Connection,
    weak_days: i64,
    provisional_days: i64,
) -> Result<ReviewSummary> {
    let all = {
        let mut stmt = conn.prepare(
            "SELECT uuid, kind, display_id, title, url, project, tags_json, path, summary, body, created, modified, status, source_task_uuid
             FROM items WHERE kind='memory' AND status IN ('active','provisional') ORDER BY created ASC",
        )?;
        let rows = stmt.query_map([], row_to_item)?;
        rows.filter_map(|r| r.ok()).collect::<Vec<_>>()
    };

    let now = Utc::now();
    let strengths = item_strengths(conn, &all);
    let mut count = 0usize;
    let mut oldest_age_days = 0i64;
    for item in &all {
        let age_days = (now - item.created).num_days();
        let provisional_stale = item.status == "provisional" && age_days >= provisional_days;
        let strength = strengths.get(&item.uuid).copied().unwrap_or(1.0);
        let weak_old = strength < 1.5 && age_days >= weak_days;
        if provisional_stale || weak_old {
            count += 1;
            if age_days > oldest_age_days {
                oldest_age_days = age_days;
            }
        }
    }
    Ok(ReviewSummary {
        count,
        oldest_age_days,
    })
}

pub const AUTO_HYGIENE_WEAK_DAYS: i64 = 90;
pub const AUTO_HYGIENE_PROVISIONAL_DAYS: i64 = 30;

#[derive(Debug, Default, Clone)]
pub struct HygieneReport {
    pub archived: Vec<String>,
    pub review_pending: usize,
    pub oldest_age_days: i64,
}

pub fn hygiene_pass(conn: &Connection) -> Result<HygieneReport> {
    let archived = archive_superseded_memories(conn)?
        .into_iter()
        .map(|c| c.label)
        .collect();
    let review =
        count_review_candidates(conn, AUTO_HYGIENE_WEAK_DAYS, AUTO_HYGIENE_PROVISIONAL_DAYS)?;
    Ok(HygieneReport {
        archived,
        review_pending: review.count,
        oldest_age_days: review.oldest_age_days,
    })
}

pub fn synthesize_done_memory(
    conn: &Connection,
    task_uuid: &Uuid,
    project_name: &str,
) -> Result<Option<String>> {
    use crate::infrastructure::util::safety;

    let task = match get_task_by_uuid_prefix(conn, &task_uuid.to_string()[..8])? {
        Some(t) => t,
        None => return Ok(None),
    };

    let steps = get_checklist(conn, task_uuid).unwrap_or_default();
    let done_steps: Vec<&ChecklistItem> = steps
        .iter()
        .filter(|s| s.done && s.kind == STEP_KIND_STEP)
        .collect();
    let acceptance: Vec<&ChecklistItem> = steps
        .iter()
        .filter(|s| s.done && s.kind == STEP_KIND_ACCEPTANCE)
        .collect();

    let annotations = get_annotations(conn, task_uuid).unwrap_or_default();
    let key_annotations: Vec<&Annotation> = annotations
        .iter()
        .filter(|a| {
            matches!(
                a.kind.as_str(),
                "finding" | "decision" | "constraint" | "risk" | "pattern"
            )
        })
        .collect();

    let files = get_task_files(conn, task_uuid).unwrap_or_default();

    if done_steps.is_empty() && acceptance.is_empty() && key_annotations.is_empty() {
        return Ok(None);
    }

    let mut parts: Vec<String> = vec![];
    parts.push(format!("Task completed: {}", task.description.trim()));

    if !done_steps.is_empty() {
        let step_parts: Vec<String> = done_steps
            .iter()
            .map(|s| {
                if let Some(ref r) = s.result {
                    format!("\"{}\" → {}", s.text.trim(), r.trim())
                } else {
                    format!("\"{}\"", s.text.trim())
                }
            })
            .collect();
        parts.push(format!("Steps: {}", step_parts.join("; ")));
    }

    if !acceptance.is_empty() {
        let ac_parts: Vec<String> = acceptance
            .iter()
            .map(|s| s.text.trim().to_string())
            .collect();
        parts.push(format!("Acceptance: {}", ac_parts.join("; ")));
    }

    for kind in &["decision", "finding", "constraint", "risk", "pattern"] {
        let group: Vec<&str> = key_annotations
            .iter()
            .filter(|a| a.kind == *kind)
            .map(|a| a.text.trim())
            .collect();
        if !group.is_empty() {
            let cap = kind.chars().next().unwrap().to_uppercase().to_string() + &kind[1..];
            parts.push(format!("{}s: {}", cap, group.join("; ")));
        }
    }

    let body_raw = parts.join(". ");
    let body: String = if body_raw.chars().count() > 1900 {
        body_raw.chars().take(1900).collect::<String>() + "…"
    } else {
        body_raw
    };

    if safety::check_secrets(&body).is_err() {
        return Ok(None);
    }

    let title: String = {
        const MAX: usize = 80;
        let t = task.description.trim();
        if t.chars().count() <= MAX {
            t.to_string()
        } else {
            t.chars().take(MAX).collect::<String>() + "…"
        }
    };

    let mut item = crate::infrastructure::model::Item::new_memory(title, body, Some(*task_uuid));
    item.tags = task.tags.clone();
    item.status = "provisional".to_string();
    item.path = Some(String::new());

    insert_item(conn, &mut item)?;
    set_item_projects(conn, &item.uuid, &[project_name.to_string()])?;

    if !files.is_empty() {
        set_item_files(conn, &item.uuid, &files)?;
    }

    set_item_task_links(conn, &item.uuid, &[(*task_uuid, "explicit")])?;

    let label = format!("m{}", item.display_id.unwrap_or(0));
    Ok(Some(label))
}

pub fn find_similar_strong_memories(
    conn: &Connection,
    description: &str,
    tags: &[String],
) -> Result<Vec<Item>> {
    let mut candidates: std::collections::HashMap<uuid::Uuid, Item> =
        std::collections::HashMap::new();

    if !description.is_empty() {
        for hit in search_fts(conn, description, 20).unwrap_or_default() {
            if hit.ref_kind.starts_with("item_")
                && let Ok(uuid) = hit.task_uuid.parse::<uuid::Uuid>()
                && !candidates.contains_key(&uuid)
                && let Ok(item) = get_item_by_uuid(conn, &uuid.to_string())
                && item.kind == "memory"
            {
                candidates.insert(uuid, item);
            }
        }
    }

    if !tags.is_empty() {
        let mut tag_set: Option<std::collections::HashSet<uuid::Uuid>> = None;
        for tag in tags {
            let tag_norm = tag.trim().to_lowercase();
            if tag_norm.is_empty() {
                continue;
            }
            let uuids: std::collections::HashSet<uuid::Uuid> = find_items_by_tag(conn, &tag_norm)
                .unwrap_or_default()
                .into_iter()
                .filter(|i| i.kind == "memory")
                .map(|i| i.uuid)
                .collect();
            tag_set = Some(match tag_set {
                Some(existing) => existing.intersection(&uuids).copied().collect(),
                None => uuids,
            });
        }
        for uuid in tag_set.unwrap_or_default() {
            candidates
                .entry(uuid)
                .or_insert_with(|| get_item_by_uuid(conn, &uuid.to_string()).unwrap());
        }
    }

    let candidates: Vec<Item> = candidates.into_values().collect();
    let strengths = item_strengths(conn, &candidates);
    let mut strong: Vec<Item> = candidates
        .into_iter()
        .filter_map(|mut item| {
            let s = strengths.get(&item.uuid).copied().unwrap_or(1.0);
            if s >= 2.0 {
                item.files = get_item_files(conn, &item.uuid).unwrap_or_default();
                Some(item)
            } else {
                None
            }
        })
        .collect();
    strong.sort_by_key(|a| std::cmp::Reverse(a.modified));
    strong.truncate(5);
    Ok(strong)
}
