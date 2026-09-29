use serde_json::Value;

use crate::infrastructure::model::format_duration;

pub(super) fn print_started(v: &Value) {
    if v["already_active"].as_bool().unwrap_or(false) {
        println!(
            "Task {} is already active (running for {}).",
            v["task"].as_i64().unwrap_or(0),
            format_duration(v["elapsed_seconds"].as_i64().unwrap_or(0))
        );
    } else {
        println!(
            "Started task {}: {}",
            v["task"].as_i64().unwrap_or(0),
            v["description"].as_str().unwrap_or_default()
        );
    }
}

pub(super) fn print_stopped(v: &Value) {
    if !v["stopped"].as_bool().unwrap_or(false) {
        println!("Task {} is not active.", v["task"].as_i64().unwrap_or(0));
        return;
    }

    println!(
        "Stopped task {} (this session: {}, total: {})",
        v["task"].as_i64().unwrap_or(0),
        format_duration(v["session_seconds"].as_i64().unwrap_or(0)),
        format_duration(v["total_seconds"].as_i64().unwrap_or(0))
    );

    if let Some(bl) = v.get("branch_log").filter(|b| !b.is_null()) {
        let n = bl["files_logged"].as_i64().unwrap_or(0);
        println!(
            "Logged {} changed file{} on branch '{}'.",
            n,
            if n == 1 { "" } else { "s" },
            bl["branch"].as_str().unwrap_or_default()
        );
    }
}
