use crate::commands::recall::enrich::stale::stale_text;
use crate::commands::recall::types::Hit;
use crate::commands::shared::{derived_from_suffix, rel_time};

pub(in crate::commands::recall) fn hit_line(h: &Hit) -> String {
    let age = h.modified.map(rel_time).unwrap_or_default();
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
    let stale_str = if h.stale.is_empty() {
        String::new()
    } else {
        format!(" ⚠ may be stale — re-validate:{}", stale_text(h))
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
    let derived_from_str = derived_from_suffix(&h.derived_from_labels);
    let nearest_str = h
        .cluster
        .as_ref()
        .and_then(|c| c.nearest.as_deref())
        .map(|n| format!(" — nearest {n}"))
        .unwrap_or_default();
    let cluster_str = match &h.cluster {
        Some(c) if c.collapsed_here > 0 => format!(
            " [cluster {} of {} — {} sibling(s) collapsed{}]",
            c.canonical_label, c.size, c.collapsed_here, nearest_str
        ),
        Some(c) => format!(" [cluster {} of {}]", c.canonical_label, c.size),
        None => String::new(),
    };
    format!(
        "  [{}] {} {} {}: {}{}{}{}{}{}{}{}{}{}",
        h.ref_kind,
        marker,
        h.label,
        h.description,
        h.snippet.trim(),
        files_str,
        tasks_str,
        superseded_str,
        stale_str,
        provisional_str,
        canonical_str,
        derived_from_str,
        cluster_str,
        if age.is_empty() {
            String::new()
        } else {
            format!(" ({age})")
        }
    )
}
