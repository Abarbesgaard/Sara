use anyhow::Result;
use rusqlite::Connection;
use serde_json::json;

use crate::commands::doctor::types::{Check, DETAIL_LIMIT, Labels, Status};
use crate::infrastructure::db;
use crate::infrastructure::memory::embedding;
use crate::infrastructure::model::Item;

pub(in crate::commands::doctor) fn embedding_check(
    conn: &Connection,
    memories: &[Item],
    labels: &Labels,
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
        "missing": missing.iter().take(DETAIL_LIMIT).map(|u| labels.of(u)).collect::<Vec<_>>(),
    });
    Ok(vec![c])
}
