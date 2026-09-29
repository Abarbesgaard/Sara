use serde_json::Value;

/// Confirm which parts of a memory were edited in place.
pub(super) fn print_relearned(v: &Value, handle: &str) {
    let updated: Vec<&str> = v["updated"]
        .as_array()
        .map(|a| a.iter().filter_map(|x| x.as_str()).collect())
        .unwrap_or_default();
    println!(
        "Relearned {} (updated: {}): {}",
        v["label"].as_str().unwrap_or(handle),
        updated.join(", "),
        v["title"].as_str().unwrap_or(""),
    );
}
