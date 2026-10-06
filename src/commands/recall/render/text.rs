use anyhow::Result;
use rusqlite::Connection;

use crate::commands::recall::enrich::confidence::match_confidence;
use crate::commands::recall::enrich::hit::{item_hit, recent_hits, record_recalled};
use crate::commands::recall::enrich::provenance::mark_provenance;
use crate::commands::recall::enrich::spread::{should_auto_spread, spreading_related};
use crate::commands::recall::enrich::stale::{mark_stale, stale_text};
use crate::commands::recall::render::hit_line::hit_line;
use crate::commands::recall::search::collect::collect_hits;
use crate::commands::recall::search::query::{RecallInput, resolve_label_query};
use crate::commands::recall::search::semantic::SemanticOpts;

use crate::commands::shared::{item_snippet, memory_handle};
use crate::infrastructure::config::Config;
use crate::infrastructure::db;

pub(in crate::commands::recall) fn print(
    conn: &Connection,
    cfg: &Config,
    query: &str,
    tags: &[String],
    projects: &[String],
    files: &[String],
    limit: i64,
    spread: bool,
) -> Result<()> {
    let input = RecallInput::new(query, tags, projects, files);
    let recent = input.is_recent();
    let RecallInput {
        query,
        tags,
        projects,
        files,
    } = input;

    if !recent && let Some(item) = resolve_label_query(conn, query) {
        let _ = db::record_memory_recall(conn, &item.uuid);
        let mut hit = item_hit(conn, item, true);
        mark_stale(conn, std::slice::from_mut(&mut hit));
        mark_provenance(conn, std::slice::from_mut(&mut hit));
        println!("Memory {} (resolved by label):", hit.label);
        println!("  {}", hit.body.trim());
        if !hit.stale.is_empty() {
            println!("  ⚠ may be stale — re-validate:{}", stale_text(&hit));
        }
        if !hit.provenance.is_empty() {
            println!("  Track record: {}", hit.provenance.summary());
        }
        let related = spreading_related(conn, std::slice::from_ref(&hit))?;
        if !related.is_empty() {
            println!("\nCluster (recall a label for its full text):");
            for r in &related {
                let label = memory_handle(&r.item);
                let snippet = item_snippet(&r.item, 100);
                println!("  ~{label} ({:.2}): {}", r.activation, snippet.trim());
                let _ = db::record_memory_surfaced(conn, &r.item.uuid);
            }
        }
        return Ok(());
    }

    let hits = if recent {
        recent_hits(conn, limit)?
    } else {
        collect_hits(
            conn,
            query,
            &tags,
            &projects,
            &files,
            limit,
            &SemanticOpts::from_cfg(cfg),
        )?
    };

    if hits.is_empty() {
        if recent {
            println!("No memories recorded yet. Use `sara learn \"...\"` to save one.");
        } else if !files.is_empty() {
            println!("No memories tied to the given file(s).");
        } else if !tags.is_empty() || !projects.is_empty() {
            if !db::has_any_memories(conn)? {
                println!("No memories recorded yet. Use `sara learn \"...\"` to save one.");
            } else {
                println!("No matches for the given --tag/--project filters.");
            }
        } else {
            println!("No matches for \"{query}\".");
            println!(
                "Note: Sara uses literal keyword search only — paraphrased or \
                 conceptually related content may not surface. Try --tag or different keywords."
            );
        }
        return Ok(());
    }

    if recent {
        record_recalled(conn, &hits);
        println!("Recent memories (no query given):");
    } else {
        let (_, caveat) = match_confidence(query, &tags, &hits);
        if !caveat.is_empty() {
            println!("Note: {caveat}");
        }
        println!("Keyword matches:");
    }

    for h in &hits {
        println!("{}", hit_line(h));
    }

    let auto_spread = !spread && !recent && !query.trim().is_empty() && should_auto_spread(&hits);
    if spread || auto_spread {
        let related = spreading_related(conn, &hits)?;
        if !related.is_empty() {
            let header = if auto_spread {
                "Associatively related (auto-spread — literal hits were thin):"
            } else {
                "Associatively related (spreading activation):"
            };
            println!("\n{header}");
            for r in &related {
                let label = memory_handle(&r.item);
                let snippet = item_snippet(&r.item, 100);
                let via = if r.path.len() > 1 {
                    format!("  [via {}]", r.path[..r.path.len() - 1].join(" → "))
                } else {
                    String::new()
                };
                println!(
                    "  ~{label} ({:.2}): {}{}",
                    r.activation,
                    snippet.trim(),
                    via
                );
                let _ = db::record_memory_surfaced(conn, &r.item.uuid);
            }
        }
    }
    Ok(())
}
