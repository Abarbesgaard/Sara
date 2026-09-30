//! `sara doctor` — a read-only health report over the memory store. It composes
//! the existing diagnostics (duplicate detection, the prune evaluation, the
//! embedding index, link integrity, recall activity, anchored-file drift) into one checklist, each
//! finding paired with the command that fixes it. Nothing here writes.

use std::collections::HashMap;

use anyhow::Result;
use chrono::Utc;
use rusqlite::Connection;
use serde_json::{Value, json};

use crate::infrastructure::db;
use crate::infrastructure::memory::embedding;
use crate::infrastructure::memory::fingerprint::{self, AnchorState};

mod render;

/// Recall events older than this are pruned, so decay is judged within it.
pub const RECALL_WINDOW_DAYS: i64 = 90;
const OVER_REINFORCED_MIN_RECALLS: usize = 20;
const OVER_REINFORCED_MEAN_FACTOR: f64 = 5.0;
const DETAIL_LIMIT: usize = 5;

const REASON_SUPERSEDED: &str = "superseded by a newer memory";
const REASON_PROVISIONAL: &str = "provisional auto-memory not reviewed within time limit";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok,
    Warn,
    Info,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Warn => "warn",
            Status::Info => "info",
        }
    }
}

struct Check {
    id: &'static str,
    status: Status,
    count: usize,
    summary: String,
    fix: Option<&'static str>,
    details: Value,
}

impl Check {
    fn new(id: &'static str, flagged: Status, count: usize, fix: &'static str) -> Self {
        let status = if count == 0 { Status::Ok } else { flagged };
        Check {
            id,
            status,
            count,
            summary: String::new(),
            fix: (count > 0).then_some(fix),
            details: Value::Null,
        }
    }

    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "status": self.status.as_str(),
            "count": self.count,
            "summary": self.summary,
            "fix": self.fix,
            "details": self.details,
        })
    }
}

pub fn doctor_value(conn: &Connection) -> Result<Value> {
    let memories = db::list_memories(conn)?;
    let labels: HashMap<String, String> = memories
        .iter()
        .map(|m| {
            let uuid = m.uuid.to_string();
            let label = m
                .display_id
                .map(|id| format!("m{id}"))
                .unwrap_or_else(|| uuid[..8].to_string());
            (uuid, label)
        })
        .collect();
    let label_of = |uuid: &str| {
        labels
            .get(uuid)
            .cloned()
            .unwrap_or_else(|| uuid.chars().take(8).collect())
    };

    let checks = vec![
        embedding_check(conn, &memories, &label_of)?,
        orphan_check(conn, &label_of)?,
        duplicate_check(conn)?,
        superseded_and_backlog_checks(conn)?,
        decay_check(conn, &memories, &label_of)?,
        stale_anchor_check(conn, &label_of)?,
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<Check>>();

    let tally = |s: Status| checks.iter().filter(|c| c.status == s).count();
    let warn = tally(Status::Warn);
    Ok(json!({
        "healthy": warn == 0,
        "memories": memories.len(),
        "summary": { "ok": tally(Status::Ok), "warn": warn, "info": tally(Status::Info) },
        "checks": checks.iter().map(Check::to_json).collect::<Vec<_>>(),
    }))
}

fn embedding_check(
    conn: &Connection,
    memories: &[crate::infrastructure::model::Item],
    label_of: &dyn Fn(&str) -> String,
) -> Result<Vec<Check>> {
    let embedded: std::collections::HashSet<String> = db::active_embeddings(conn)?
        .into_iter()
        .map(|(uuid, _)| uuid)
        .collect();
    let missing: Vec<String> = memories
        .iter()
        .map(|m| m.uuid.to_string())
        .filter(|u| !embedded.contains(u))
        .collect();
    let current = embedding::index_is_current(conn)?;
    let total = memories.len();
    let covered = total - missing.len();

    let flagged = missing.len() + usize::from(!current);
    let mut c = Check::new(
        "embedding_coverage",
        Status::Warn,
        flagged,
        "sara reindex-embeddings",
    );
    c.count = missing.len();
    c.summary = match (missing.is_empty(), current) {
        (true, true) => format!("{covered}/{total} memories embedded with the current scheme"),
        (_, false) => format!(
            "{covered}/{total} memories embedded, but the index was built with an outdated scheme"
        ),
        (false, true) => format!(
            "{covered}/{total} memories embedded — {} missing a vector",
            missing.len()
        ),
    };
    c.details = json!({
        "total": total,
        "embedded": covered,
        "scheme_current": current,
        "missing": missing.iter().take(DETAIL_LIMIT).map(|u| label_of(u)).collect::<Vec<_>>(),
    });
    Ok(vec![c])
}

fn orphan_check(conn: &Connection, label_of: &dyn Fn(&str) -> String) -> Result<Vec<Check>> {
    let orphans = db::orphaned_memory_links(conn)?;
    let mut c = Check::new(
        "orphaned_links",
        Status::Warn,
        orphans.len(),
        "sara unlink-memory <from> <relation> <to>",
    );
    c.summary = if orphans.is_empty() {
        "every memory link points at a live memory".to_string()
    } else {
        format!(
            "{} memory link(s) point at an archived or missing memory",
            orphans.len()
        )
    };
    c.details = json!(
        orphans
            .iter()
            .take(DETAIL_LIMIT)
            .map(|l| json!({
                "from": label_of(&l.from_uuid),
                "relation": l.relation,
                "to": label_of(&l.to_uuid),
            }))
            .collect::<Vec<_>>()
    );
    Ok(vec![c])
}

fn duplicate_check(conn: &Connection) -> Result<Vec<Check>> {
    let diag = crate::commands::diagnose_memories::diagnose_value(
        conn,
        crate::commands::diagnose_memories::DEFAULT_CONFLICT_THRESHOLD,
        None,
        Some(DETAIL_LIMIT),
    )?;
    let total = diag["total"].as_u64().unwrap_or(0) as usize;
    let mut c = Check::new("duplicates", Status::Warn, total, "sara diagnose-memories");
    c.summary = if total == 0 {
        "no near-duplicate or contradictory memory pairs".to_string()
    } else {
        format!("{total} unlinked memory pair(s) look like duplicates or contradictions")
    };
    c.details = json!(
        diag["conflicts"]
            .as_array()
            .map(|pairs| pairs
                .iter()
                .map(|p| json!({ "a": p["label_a"], "b": p["label_b"], "cosine": p["cosine"] }))
                .collect::<Vec<_>>())
            .unwrap_or_default()
    );
    Ok(vec![c])
}

fn superseded_and_backlog_checks(conn: &Connection) -> Result<Vec<Check>> {
    let candidates = db::prune_memories(
        conn,
        db::AUTO_HYGIENE_WEAK_DAYS,
        db::AUTO_HYGIENE_PROVISIONAL_DAYS,
        true,
    )?;
    let labels_for = |reason: &str| -> Vec<String> {
        candidates
            .iter()
            .filter(|c| c.reason == reason)
            .map(|c| c.label.clone())
            .collect()
    };

    let superseded = labels_for(REASON_SUPERSEDED);
    let mut s = Check::new(
        "superseded",
        Status::Warn,
        superseded.len(),
        "sara prune-memories --apply",
    );
    s.summary = if superseded.is_empty() {
        "no superseded memory is still active".to_string()
    } else {
        format!(
            "{} superseded memory(ies) still active and competing in recall",
            superseded.len()
        )
    };
    s.details = json!(superseded.iter().take(DETAIL_LIMIT).collect::<Vec<_>>());

    let backlog = labels_for(REASON_PROVISIONAL);
    let mut b = Check::new(
        "provisional_backlog",
        Status::Warn,
        backlog.len(),
        "sara promote <label> (keep) or sara prune-memories --apply (archive)",
    );
    b.summary = if backlog.is_empty() {
        format!(
            "no provisional memory older than {} days awaits review",
            db::AUTO_HYGIENE_PROVISIONAL_DAYS
        )
    } else {
        format!(
            "{} provisional memory(ies) older than {} days await promote or forget",
            backlog.len(),
            db::AUTO_HYGIENE_PROVISIONAL_DAYS
        )
    };
    b.details = json!(backlog.iter().take(DETAIL_LIMIT).collect::<Vec<_>>());
    Ok(vec![s, b])
}

fn decay_check(
    conn: &Connection,
    memories: &[crate::infrastructure::model::Item],
    label_of: &dyn Fn(&str) -> String,
) -> Result<Vec<Check>> {
    let now = Utc::now();
    let cutoff = now - chrono::Duration::days(RECALL_WINDOW_DAYS);
    let mut recalls: HashMap<uuid::Uuid, usize> = HashMap::new();
    for (uuid, _) in db::memory_recall_events_since(conn, &cutoff)? {
        *recalls.entry(uuid).or_default() += 1;
    }

    let dead: Vec<String> = memories
        .iter()
        .filter(|m| m.created <= cutoff && !recalls.contains_key(&m.uuid))
        .map(|m| label_of(&m.uuid.to_string()))
        .collect();

    let recalled: Vec<(&uuid::Uuid, usize)> = recalls
        .iter()
        .filter(|(u, _)| memories.iter().any(|m| &m.uuid == *u))
        .map(|(u, n)| (u, *n))
        .collect();
    let mean = if recalled.is_empty() {
        0.0
    } else {
        recalled.iter().map(|(_, n)| *n as f64).sum::<f64>() / recalled.len() as f64
    };
    let mut hot: Vec<(String, usize)> = recalled
        .iter()
        .filter(|(_, n)| {
            *n >= OVER_REINFORCED_MIN_RECALLS && *n as f64 >= OVER_REINFORCED_MEAN_FACTOR * mean
        })
        .map(|(u, n)| (label_of(&u.to_string()), *n))
        .collect();
    hot.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    let mut c = Check::new(
        "decay_outliers",
        Status::Info,
        dead.len() + hot.len(),
        "sara forget <label> (dead weight) or sara relearn <label> (split an over-reinforced memory)",
    );
    c.summary = format!(
        "{} memory(ies) not recalled in {RECALL_WINDOW_DAYS} days, {} over-reinforced",
        dead.len(),
        hot.len()
    );
    c.details = json!({
        "window_days": RECALL_WINDOW_DAYS,
        "dead_weight": dead.iter().take(DETAIL_LIMIT).collect::<Vec<_>>(),
        "over_reinforced": hot
            .iter()
            .take(DETAIL_LIMIT)
            .map(|(l, n)| json!({ "label": l, "recalls": n }))
            .collect::<Vec<_>>(),
    });
    Ok(vec![c])
}

fn stale_anchor_check(conn: &Connection, label_of: &dyn Fn(&str) -> String) -> Result<Vec<Check>> {
    let anchors = db::all_active_item_file_fingerprints(conn)?;
    let mut stale: Vec<(String, String, AnchorState)> = anchors
        .into_iter()
        .filter_map(|(uuid, file, fp)| {
            let state = fingerprint::anchor_state(&file, fp.as_deref());
            state.is_stale().then(|| (label_of(&uuid), file, state))
        })
        .collect();
    stale.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    let memories: std::collections::BTreeSet<&str> = stale.iter().map(|s| s.0.as_str()).collect();

    let mut c = Check::new(
        "stale_anchors",
        Status::Info,
        memories.len(),
        "sara relearn <label> <text> (re-validate) or sara forget <label>",
    );
    c.summary = if stale.is_empty() {
        "no anchored file has drifted or vanished since its memory was learned".to_string()
    } else {
        format!(
            "{} memory(ies) anchored to {} file(s) that drifted or vanished since learn",
            memories.len(),
            stale.len()
        )
    };
    c.details = json!(
        stale
            .iter()
            .take(DETAIL_LIMIT)
            .map(|(label, file, state)| match state {
                AnchorState::Drifted(d) => json!({
                    "label": label,
                    "file": file,
                    "reason": "drifted",
                    "drift": (d * 100.0).round() / 100.0,
                }),
                _ => json!({ "label": label, "file": file, "reason": "missing" }),
            })
            .collect::<Vec<_>>()
    );
    Ok(vec![c])
}

pub fn run(conn: &Connection, json: bool, strict: bool) -> Result<()> {
    let v = doctor_value(conn)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&v)?);
    } else {
        render::print_report(&v);
    }
    if strict && v["healthy"] == Value::Bool(false) {
        let warn = v["summary"]["warn"].as_u64().unwrap_or(0);
        anyhow::bail!("doctor found {warn} warning(s)");
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/doctor/mod.rs"]
mod tests;
