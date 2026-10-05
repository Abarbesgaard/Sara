use serde_json::Value;

use crate::commands::shared::json_strs;

pub(in crate::commands::diagnose_memories) fn print_report(
    v: &Value,
    threshold: f32,
    project: Option<&str>,
) {
    let count = v["count"].as_u64().unwrap_or(0);
    let total = v["total"].as_u64().unwrap_or(count);
    let scope = project
        .map(|p| format!(" in project '{p}'"))
        .unwrap_or_default();

    if count == 0 {
        println!("No memories look like duplicates or contradictions{scope}.");
        println!(
            "Every active memory that shares a file or tag set is either already linked \
             or too different in meaning to worry about (needs cosine >= {threshold:.2})."
        );
        return;
    }

    println!(
        "Found {total} memory pair(s){scope} that may be saying the same thing — or contradicting each other."
    );
    println!(
        "Each pair is worded alike (similarity >= {threshold:.2}) and shares a file or an identical tag set, \
         yet nothing records that you've reconciled them."
    );
    if count < total {
        println!("Showing the {count} most similar; pass --limit to see the rest.");
    }
    println!();
    println!("Legend:  <memory-a> ↔ <memory-b>  [similarity 0-1]  [what they share]");
    println!("         followed by the opening line of each memory.");
    println!();
    if let Some(conflicts) = v["conflicts"].as_array() {
        for c in conflicts {
            print_conflict(c);
        }
    }
    print_reconcile_hints();
}

fn print_conflict(c: &Value) {
    let la = c["label_a"].as_str().unwrap_or("?");
    let lb = c["label_b"].as_str().unwrap_or("?");
    let score = c["cosine"]
        .as_f64()
        .map(|c| format!("{c:.2}"))
        .unwrap_or_else(|| "—".to_string());

    let shared_files = json_strs(&c["shared_files"]);
    if shared_files.is_empty() {
        let shared_tags = json_strs(&c["shared_tags"]);
        println!(
            "  {la} ↔ {lb}  [{score}] [tags: {}]",
            shared_tags.join(", ")
        );
    } else {
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
    }
    println!("    {la}: {}", c["snippet_a"].as_str().unwrap_or(""));
    println!("    {lb}: {}", c["snippet_b"].as_str().unwrap_or(""));
    println!();
}

fn print_reconcile_hints() {
    println!("How to reconcile each pair:");
    println!(
        "  • Same fact, one is better        → sara learn --supersedes <old-label>   (archives the weaker one)"
    );
    println!(
        "  • One is a case of a shared pattern → sara relearn <canonical-label>       (enrich it, don't duplicate)"
    );
    println!(
        "  • Both are correct but distinct   → sara link-memory <a> similar_to <b>    (marks reviewed, hides the pair)"
    );
    println!("Do nothing and the pair keeps showing up here until you act on it.");
}
