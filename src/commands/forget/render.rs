use crate::commands::shared::json_strs;
use serde_json::Value;

pub(super) fn print_forgotten(v: &Value, handle: &str, cascade: bool) {
    println!(
        "Forgot {}: archived.",
        v["label"].as_str().unwrap_or(handle),
    );
    let derived: Vec<&str> = json_strs(&v["derived"]);
    let cascaded: Vec<&str> = json_strs(&v["cascaded"]);
    if !derived.is_empty() {
        if cascade {
            println!(
                "  ↳ cascaded: also archived {} derived {} ({})",
                cascaded.len(),
                if cascaded.len() == 1 {
                    "memory"
                } else {
                    "memories"
                },
                cascaded.join(", ")
            );
        } else {
            println!(
                "Warning: {} derived {} exist ({}) — review with `sara dream <label>` \
                 or archive with `sara forget <label>`, or re-run with --cascade.",
                derived.len(),
                if derived.len() == 1 {
                    "memory"
                } else {
                    "memories"
                },
                derived.join(", ")
            );
        }
    }
}
