use serde_json::Value;

use crate::commands::shared::json_strs;

const NOTHING_TO_CONSOLIDATE: &str =
    "Nothing to consolidate — no un-linked related clusters above the weight threshold.";

pub(super) fn print_applied(v: &Value) {
    let applied = v["applied"].as_u64().unwrap_or(0);
    let skipped = v["skipped"].as_array().map(|a| a.len()).unwrap_or(0);
    if applied == 0 && skipped == 0 {
        println!("{NOTHING_TO_CONSOLIDATE}");
        return;
    }
    println!(
        "Consolidated {applied} link(s) across {} cluster(s):",
        v["clusters"].as_u64().unwrap_or(0)
    );
    for link in v["links"].as_array().into_iter().flatten() {
        println!(
            "  {} derived_from {}",
            link["from"].as_str().unwrap_or("?"),
            link["to"].as_str().unwrap_or("?"),
        );
    }
    if skipped > 0 {
        println!("\nSkipped {skipped} link(s):");
        for s in v["skipped"].as_array().into_iter().flatten() {
            println!(
                "  {} -> {} : {}",
                s["from"].as_str().unwrap_or("?"),
                s["to"].as_str().unwrap_or("?"),
                s["reason"].as_str().unwrap_or(""),
            );
        }
    }
}

pub(super) fn print_proposal(v: &Value) {
    let count = v["count"].as_u64().unwrap_or(0);
    if count == 0 {
        println!("{NOTHING_TO_CONSOLIDATE}");
        return;
    }

    println!("{count} consolidation candidate(s) — related memories with no shared canonical:");
    println!();
    for c in v["clusters"].as_array().into_iter().flatten() {
        let canonical = c["suggested_canonical"].as_str().unwrap_or("?");
        let members: Vec<&str> = json_strs(&c["members"]);
        let tags: Vec<&str> = json_strs(&c["shared_tags"]);

        let tag_str = if tags.is_empty() {
            String::new()
        } else {
            format!("  [shared tags: {}]", tags.join(", "))
        };
        println!(
            "  {} members, canonical -> {canonical}{tag_str}",
            members.len()
        );
        println!("    cluster: {}", members.join(", "));
        for link in c["proposed_links"].as_array().into_iter().flatten() {
            let from = link["from"].as_str().unwrap_or("?");
            println!("    sara link-memory {from} derived_from {canonical}");
        }
        println!();
    }
    println!("Review each cluster, then run the printed `sara link-memory` lines to consolidate.");
}
