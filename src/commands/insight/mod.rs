//! Surfacing a task's own prior findings that are semantically close to a new
//! result or finding — so an agent is reconnected to what it already concluded
//! the moment it records something that touches the same ground. This is the
//! guard against the "recorded a finding early, then contradicted it later
//! without noticing" failure: the earlier finding is pushed back into view (with
//! a reconsider prompt) exactly when the new text overlaps it in meaning.
//!
//! Deliberately embedding-based, not keyword-based: a contradiction rarely
//! shares surface wording ("no coupled NightOwl fault" vs "reverted NightOwl for
//! the coupled NU1605"), but it is semantically adjacent — which cosine catches
//! and FTS does not.

use rusqlite::Connection;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::infrastructure::db;
use crate::infrastructure::embedding::{self, Embedder};

/// Cosine floor for calling a prior finding "related enough to reconsider".
/// Higher than the general recall threshold (0.30) because a false reconsider
/// prompt is noise the agent must burn a thought on — precision over recall.
const RELATED_THRESHOLD: f32 = 0.55;

/// At most this many prior findings are surfaced, strongest first — enough to
/// catch a contradiction without drowning the agent in its own backlog.
const MAX_RELATED: usize = 2;

/// A prior finding surfaced as related to some new text, with its similarity.
pub struct Related {
    pub id: i64,
    pub text: String,
    pub cosine: f32,
}

/// Find the task's own prior `finding` annotations semantically closest to
/// `new_text`, above [`RELATED_THRESHOLD`], strongest first, capped at
/// [`MAX_RELATED`]. `exclude_id` skips the just-inserted annotation so a finding
/// never matches itself. Best-effort: returns empty on any embed hiccup.
pub fn related_findings(
    conn: &Connection,
    task_uuid: &Uuid,
    new_text: &str,
    exclude_id: Option<i64>,
) -> Vec<Related> {
    let qv = embedding::bundled().embed(new_text);
    if qv.iter().all(|&x| x == 0.0) {
        return Vec::new();
    }
    let Ok(anns) = db::get_annotations(conn, task_uuid) else {
        return Vec::new();
    };
    let mut scored: Vec<Related> = anns
        .into_iter()
        .filter(|a| a.kind == "finding" && Some(a.id) != exclude_id)
        .filter_map(|a| {
            let v = embedding::bundled().embed(&a.text);
            let c = embedding::cosine(&qv, &v);
            (c >= RELATED_THRESHOLD).then_some(Related {
                id: a.id,
                text: a.text,
                cosine: c,
            })
        })
        .collect();
    scored.sort_by(|a, b| {
        b.cosine
            .partial_cmp(&a.cosine)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    scored.truncate(MAX_RELATED);
    scored
}

/// JSON form for MCP/`--json` callers.
pub fn related_findings_json(related: &[Related]) -> Vec<Value> {
    related
        .iter()
        .map(|r| json!({ "annotation_id": r.id, "cosine": r.cosine, "text": r.text }))
        .collect()
}

/// Print a reconsider prompt for related prior findings, if any. No-op on empty.
pub fn print_related_findings(related: &[Related]) {
    if related.is_empty() {
        return;
    }
    eprintln!("⟳ reconsider — related prior finding(s) on this task:");
    for r in related {
        eprintln!("    (~{:.2}) #{}: {}", r.cosine, r.id, r.text);
    }
    eprintln!(
        "  If your new note revises or contradicts one, correct it (denotate / re-annotate)."
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::db;
    use crate::infrastructure::model::Task;

    fn seed_task(conn: &Connection) -> Task {
        let mut task = Task::new("host task".into(), "proj".into());
        db::insert_task(conn, &mut task).unwrap();
        task
    }

    fn add_finding(conn: &Connection, task: &Task, text: &str) -> i64 {
        db::add_annotation_full(conn, &task.uuid, text, "finding", "ai", None, None, false).unwrap()
    }

    #[test]
    fn surfaces_a_semantically_close_prior_finding() {
        let conn = db::open_in_memory_for_test();
        let task = seed_task(&conn);
        add_finding(
            &conn,
            &task,
            "dependabot bump broke the restore step; pin the lockfile version back",
        );
        let related = related_findings(
            &conn,
            &task.uuid,
            "the dependency update caused the restore to fail; revert the version bump",
            None,
        );
        assert!(
            !related.is_empty(),
            "a semantically adjacent prior finding must resurface"
        );
    }

    #[test]
    fn ignores_an_unrelated_prior_finding() {
        let conn = db::open_in_memory_for_test();
        let task = seed_task(&conn);
        add_finding(&conn, &task, "how to bake sourdough bread at home");
        let related = related_findings(
            &conn,
            &task.uuid,
            "the dependency update caused the restore to fail; revert the version bump",
            None,
        );
        assert!(
            related.is_empty(),
            "an unrelated finding must not resurface (precision over recall)"
        );
    }

    #[test]
    fn excludes_the_just_inserted_finding() {
        let conn = db::open_in_memory_for_test();
        let task = seed_task(&conn);
        let id = add_finding(
            &conn,
            &task,
            "restore broke after the dependabot version bump",
        );
        // Querying with the same text but excluding that id yields nothing —
        // a finding must never match itself.
        let related = related_findings(
            &conn,
            &task.uuid,
            "restore broke after the dependabot version bump",
            Some(id),
        );
        assert!(related.is_empty(), "self-match is excluded");
    }

    #[test]
    fn only_findings_resurface_not_other_note_kinds() {
        let conn = db::open_in_memory_for_test();
        let task = seed_task(&conn);
        db::add_annotation_full(
            &conn,
            &task.uuid,
            "restore broke after the dependabot version bump",
            "comment",
            "ai",
            None,
            None,
            false,
        )
        .unwrap();
        let related = related_findings(
            &conn,
            &task.uuid,
            "restore broke after the dependabot version bump",
            None,
        );
        assert!(related.is_empty(), "comments are not findings");
    }
}
