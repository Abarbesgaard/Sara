use serde_json::Value;

use crate::commands::add::types::Created;

pub(in crate::commands::add) fn print_created(created: &Created) {
    match &created.similar {
        Ok(hits) if !hits.is_empty() => {
            println!("Similar past work found — consider reusing instead of starting fresh:");
            for hit in hits {
                print_similar_hit(hit);
            }
            println!();
        }
        Ok(_) => {}
        Err(e) => eprintln!("Warning: recall check failed: {e}"),
    }

    if let Some(dup) = &created.duplicate {
        println!(
            "⚠ An open task in this project already matches — reuse it instead of duplicating:\n  task {} ({}): {}\n",
            dup.id.unwrap_or(0),
            &dup.uuid.to_string()[..8],
            dup.description
        );
    }

    let task = &created.task;
    println!(
        "Created task {} [{}] ({}): {}",
        task.id.unwrap_or(0),
        task.project,
        &task.uuid.to_string()[..8],
        task.description
    );
    if let Some(branch) = &created.branch {
        println!(
            "Tied to branch '{branch}' — resolve by uuid across branches to stay unambiguous."
        );
    }
}

pub(in crate::commands::add) fn render_similar_body(confidence: &str, body: &str) -> String {
    if confidence == "semantic" {
        let snippet: String = body.chars().take(200).collect();
        let ellipsis = if body.chars().count() > 200 {
            " …"
        } else {
            ""
        };
        format!("      {}{}\n", snippet.trim(), ellipsis)
    } else {
        body.lines().map(|l| format!("      {l}\n")).collect()
    }
}

fn print_similar_hit(hit: &Value) {
    let confidence = hit["confidence"].as_str().unwrap_or("medium");
    if hit["ref_kind"].as_str() == Some("memory") {
        let label = hit["memory"].as_str().unwrap_or("memory");
        let provisional = if hit["provisional"].as_bool().unwrap_or(false) {
            " [provisional]"
        } else {
            ""
        };
        let cross = if hit["same_project"].as_bool().unwrap_or(true) {
            String::new()
        } else {
            format!(
                " [other project: {}]",
                hit["project"].as_str().unwrap_or("?")
            )
        };
        println!(
            "  [memory] [{}] {}{}{}: {}",
            confidence,
            label,
            provisional,
            cross,
            hit["title"].as_str().unwrap_or("")
        );
        let body = hit["body"].as_str().unwrap_or("");
        print!("{}", render_similar_body(confidence, body));
    } else {
        println!(
            "  [{}] [{}] task {}: {} — {}",
            hit["ref_kind"].as_str().unwrap_or(""),
            confidence,
            hit["task"].as_i64().unwrap_or(0),
            hit["description"].as_str().unwrap_or(""),
            hit["snippet"].as_str().unwrap_or("")
        );
    }
}
