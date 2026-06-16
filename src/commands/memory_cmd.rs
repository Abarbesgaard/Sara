use anyhow::Result;

use crate::config::Config;
use crate::memory;
use rusqlite::Connection;

pub fn run(conn: &Connection, cfg: &Config, action: &crate::cli::MemoryAction, query: Option<&str>) -> Result<()> {
    match action {
        crate::cli::MemoryAction::Show => {
            let content = memory::read_long_term(cfg);
            if content.trim().is_empty() {
                println!("No long-term memory yet. Use `sara remember \"...\"` or `sara learn`.");
            } else {
                println!("{content}");
            }
        }
        crate::cli::MemoryAction::Today => {
            let notes = memory::recent_daily_notes(cfg, 1);
            if let Some((date, body)) = notes.first() {
                println!("# {date}\n\n{body}");
            } else {
                println!("No daily notes for today yet.");
            }
        }
        crate::cli::MemoryAction::Search => {
            let q = query.ok_or_else(|| anyhow::anyhow!("Provide a query: sara memory search <query>"))?;
            let hits = memory::search(cfg, q);
            if hits.is_empty() {
                println!("No memory matches for \"{q}\".");
            } else {
                println!("Memory matches for \"{q}\":\n");
                for hit in hits {
                    println!(
                        "  [{:.0}%] {} — {}",
                        hit.score * 100.0,
                        hit.source,
                        hit.excerpt
                    );
                }
            }
        }
        crate::cli::MemoryAction::Index => {
            let count = memory::index_embeddings(conn, cfg)?;
            println!("Indexed {count} memory chunks for semantic search.");
        }
    }
    Ok(())
}
