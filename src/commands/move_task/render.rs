use serde_json::Value;

pub(super) fn print_moved(v: &Value, project: &str) {
    let display_id = v["task"].as_i64().unwrap_or(0);
    let to = v["to"].as_str().unwrap_or(project);
    let from = v["from"].as_str().unwrap_or("");
    if v["changed"].as_bool().unwrap_or(false) {
        println!("Moved task {display_id} to project '{to}' (was '{from}').");
    } else {
        println!("Task {display_id} is already in project '{to}'.");
    }
}
