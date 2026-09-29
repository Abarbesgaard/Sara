use anyhow::Result;
use serde_json::json;

/// `sara tags` output — the memory tag vocabulary with usage counts, most-used
/// first, or a hint when none exist yet. `--json` emits the structured form.
pub(super) fn print_tags(tags: &[(String, i64)], as_json: bool) -> Result<()> {
    if as_json {
        let v: Vec<_> = tags
            .iter()
            .map(|(tag, count)| json!({ "tag": tag, "count": count }))
            .collect();
        println!("{}", serde_json::to_string_pretty(&json!({ "tags": v }))?);
        return Ok(());
    }

    if tags.is_empty() {
        println!("No tags recorded yet. Use `sara learn --tag <topic> \"...\"` to tag a memory.");
        return Ok(());
    }

    println!("Memory tags (most used first):");
    for (tag, count) in tags {
        println!("  {:>4}  {tag}", count);
    }
    Ok(())
}
