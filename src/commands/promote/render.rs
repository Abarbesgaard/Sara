use serde_json::Value;

pub(super) fn print_promoted(v: &Value, handle: &str) {
    println!(
        "Promoted {}: now an active (reviewed) memory.",
        v["label"].as_str().unwrap_or(handle),
    );
}
