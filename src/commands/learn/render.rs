use crate::commands::shared::json_strs;
use serde_json::Value;

pub(super) fn print_learned(v: &Value) {
    let label = v["label"].as_str().unwrap_or("m?");
    let uuid = v["uuid"].as_str().unwrap_or("");
    let body = v["text"].as_str().unwrap_or("");

    let file_suffix = {
        let fs: Vec<&str> = json_strs(&v["files"]);
        if fs.is_empty() {
            String::new()
        } else {
            format!(" (files: {})", fs.join(", "))
        }
    };
    let task_suffix = {
        let ts = v["linked_tasks"].as_array();
        let parts: Vec<String> = ts
            .map(|a| {
                a.iter()
                    .filter_map(|t| {
                        let id = t["id"].as_i64()?;
                        let desc = t["description"].as_str()?;
                        let src = t["source"].as_str().unwrap_or("auto");
                        Some(format!("#{id} {desc} [{src}]"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if parts.is_empty() {
            String::new()
        } else {
            format!(" (tasks: {})", parts.join(", "))
        }
    };

    println!("Learned {label} ({uuid}): {body}{file_suffix}{task_suffix}");
    for old in json_strs(&v["superseded"]) {
        println!("  ↳ supersedes {old}");
    }
    for canon in json_strs(&v["derived_from"]) {
        println!("  ↳ derived from {canon}");
    }
    for canon in json_strs(&v["auto_derived_from"]) {
        println!(
            "  ↳ auto-linked as an application of canonical pattern {canon} \
             (unlink with `sara unlink-memory {label} derived_from {canon}`)"
        );
    }
    for other in json_strs(&v["similar_to"]) {
        println!("  ↳ similar to {other}");
    }
}
