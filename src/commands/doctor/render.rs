use serde_json::Value;

use super::reuse::{WINDOW_DAYS, scope_line};

pub(super) fn print_report(v: &Value) {
    let color = std::env::var("NO_COLOR").is_err();
    let paint = |code: &str, s: &str| {
        if color {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    };

    println!(
        "sara doctor — {} memories",
        v["memories"].as_u64().unwrap_or(0)
    );
    for c in v["checks"].as_array().into_iter().flatten() {
        let status = c["status"].as_str().unwrap_or("");
        let mark = match status {
            "ok" => paint("32", "✓"),
            "warn" => paint("33", "!"),
            _ => paint("36", "i"),
        };
        println!(
            "  {mark} {:<20} {}",
            c["id"].as_str().unwrap_or(""),
            c["summary"].as_str().unwrap_or("")
        );
        if let Some(fix) = c["fix"].as_str() {
            println!("      fix: {fix}");
        }
    }
    let kpi = &v["knowledge_reuse"];
    println!("\nKnowledge reuse (verified tasks, last {WINDOW_DAYS} days):");
    if let Some(name) = kpi["project"]["name"].as_str() {
        println!("  {:<12} {}", name, scope_line(&kpi["project"]));
    }
    println!("  {:<12} {}", "all projects", scope_line(&kpi["global"]));

    let s = &v["summary"];
    println!(
        "\n{} ok, {} warn, {} info — {}",
        s["ok"].as_u64().unwrap_or(0),
        s["warn"].as_u64().unwrap_or(0),
        s["info"].as_u64().unwrap_or(0),
        if v["healthy"] == Value::Bool(true) {
            paint("32", "healthy")
        } else {
            paint("33", "needs attention")
        }
    );
}
