use crate::commands::shared::json_strs;
use serde_json::Value;

pub(super) fn print_relearned(v: &Value, handle: &str) {
    let updated: Vec<&str> = json_strs(&v["updated"]);
    println!(
        "Relearned {} (updated: {}): {}",
        v["label"].as_str().unwrap_or(handle),
        updated.join(", "),
        v["title"].as_str().unwrap_or(""),
    );
}
