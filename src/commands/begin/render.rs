use serde_json::Value;

use super::types::RECALL_STEP_TEXT;

pub(super) fn print_begin(v: &Value) {
    let task = v["task"].as_i64().unwrap_or_default();
    let project = v["project"].as_str().unwrap_or("");
    println!(
        "Started task {task} in {project}: {}",
        v["description"].as_str().unwrap_or("")
    );

    match v["acceptance"].as_object() {
        Some(a) => println!("  acceptance: {}", a["text"].as_str().unwrap_or("")),
        None => println!("  acceptance: (none — add one with `sara check`)"),
    }

    println!("  step 1: {RECALL_STEP_TEXT} — run `sara recall` before you act");

    if let Some(next) = v["next"].as_object() {
        if next.get("done").and_then(Value::as_bool) == Some(true) {
            println!("  next: no steps yet — add them with `sara check <id> \"…\"`");
        } else if let Some(text) = next.get("text").and_then(Value::as_str) {
            println!("  next: {text}");
        } else {
            println!("  next: `sara next {task}`");
        }
    }

    for w in v["warnings"].as_array().cloned().unwrap_or_default() {
        if let Some(w) = w.as_str() {
            eprintln!("warning: {w}");
        }
    }
}
