use rusqlite::Connection;
use uuid::Uuid;

use crate::commands::shared::{derived_count, plural};

pub(in crate::commands::learn) fn near_dupe_suggestion(label: &str) -> String {
    format!(
        "→ Link it typed: re-run with `--derived-from {label}` if this specialises it, \
         or `--supersedes {label}` if it replaces it (or --force to keep both)."
    )
}

pub(in crate::commands::learn) fn partial_overlap_suggestion(label: &str) -> String {
    format!(
        "→ Link it typed: re-run with `--similar-to {label}` to connect them in the memory graph."
    )
}

pub(in crate::commands::learn) fn canonical_derived_count(conn: &Connection, uuid: &Uuid) -> usize {
    derived_count(conn, &uuid.to_string())
}

pub(in crate::commands::learn) fn canonical_hint(label: &str, derived_count: usize) -> String {
    format!(
        "→ {label} is a canonical pattern memory with {derived_count} derived application{} — \
         consider `sara learn --derived-from {label}` to register this as another application, \
         or `sara relearn {label}` to enrich the canonical instead of creating a new memory.",
        plural(derived_count)
    )
}
