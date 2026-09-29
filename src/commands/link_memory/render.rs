use serde_json::Value;

/// Confirm a typed memory link was created.
pub(super) fn print_linked(v: &Value, from: &str, relation: &str, to: &str, weight: f64) {
    println!(
        "Linked: {} {} {} (weight: {})",
        v["from"].as_str().unwrap_or(from),
        v["relation"].as_str().unwrap_or(relation),
        v["to"].as_str().unwrap_or(to),
        v["weight"].as_f64().unwrap_or(weight),
    );
}

/// Report whether an unlink removed an existing edge.
pub(super) fn print_unlinked(v: &Value, from: &str, relation: &str, to: &str) {
    if v["removed"].as_bool().unwrap_or(false) {
        println!("Unlinked: {from} {relation} {to}");
    } else {
        println!("No such link: {from} {relation} {to}");
    }
}
