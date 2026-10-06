use rusqlite::Connection;

use crate::commands::recall::types::Hit;
use crate::infrastructure::db;

/// Attach each memory hit's track record; one query for any number of hits.
pub(in crate::commands::recall) fn mark_provenance(conn: &Connection, hits: &mut [Hit]) {
    match hits {
        [] => {}
        [h] => {
            if let Some(u) = h.item_uuid {
                h.provenance = db::memory_provenance(conn, &u);
            }
        }
        _ => {
            let all = db::memory_provenance_all(conn);
            for h in hits.iter_mut() {
                if let Some(p) = h.item_uuid.and_then(|u| all.get(&u)) {
                    h.provenance = *p;
                }
            }
        }
    }
}
