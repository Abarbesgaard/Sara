use serde_json::Value;

pub(super) fn print_pruned(v: &Value, dry_run: bool) {
    let count = v["archived"].as_u64().unwrap_or(0);
    let mode = if dry_run { "Would archive" } else { "Archived" };

    if count == 0 {
        println!("No low-value memories found.");
        return;
    }

    println!("{mode} {count} memories:");
    if let Some(memories) = v["memories"].as_array() {
        for m in memories {
            println!(
                "  {} {} — {}",
                m["label"].as_str().unwrap_or("?"),
                m["title"].as_str().unwrap_or(""),
                m["reason"].as_str().unwrap_or(""),
            );
        }
    }
    if dry_run {
        println!("\nRun without --dry-run to apply.");
    }
}
