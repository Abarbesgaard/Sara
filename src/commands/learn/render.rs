use serde_json::Value;

/// Print the outcome of a `learn` — the saved memory, its file/task suffixes,
/// and any typed links (supersedes / derived-from / auto-derived / similar-to).
pub(super) fn print_learned(v: &Value) {
    let label = v["label"].as_str().unwrap_or("m?");
    let uuid = v["uuid"].as_str().unwrap_or("");
    let body = v["text"].as_str().unwrap_or("");

    let file_suffix = {
        let fs: Vec<&str> = v["files"]
            .as_array()
            .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
            .unwrap_or_default();
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
    if let Some(links) = v["superseded"].as_array() {
        for link in links {
            if let Some(old_label) = link.as_str() {
                println!("  ↳ supersedes {old_label}");
            }
        }
    }
    if let Some(links) = v["derived_from"].as_array() {
        for link in links {
            if let Some(canon_label) = link.as_str() {
                println!("  ↳ derived from {canon_label}");
            }
        }
    }
    if let Some(links) = v["auto_derived_from"].as_array() {
        for link in links {
            if let Some(canon_label) = link.as_str() {
                println!(
                    "  ↳ auto-linked as an application of canonical pattern {canon_label} \
                     (unlink with `sara unlink-memory {label} derived_from {canon_label}`)"
                );
            }
        }
    }
    if let Some(links) = v["similar_to"].as_array() {
        for link in links {
            if let Some(other_label) = link.as_str() {
                println!("  ↳ similar to {other_label}");
            }
        }
    }
}
