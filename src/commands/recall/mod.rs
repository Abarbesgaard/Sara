use anyhow::Result;
use rusqlite::Connection;

use crate::commands::shared::print_json;
use crate::infrastructure::config::Config;

mod enrich;
mod render;
mod search;
mod types;

pub use render::value::recall_value;

#[cfg(test)]
use {
    crate::infrastructure::db, crate::infrastructure::model::Item,
    enrich::clusters::collapse_clusters, enrich::confidence::match_confidence,
    enrich::hit::item_hit, enrich::spread::spreading_related, enrich::stale::stale_text,
    render::keyword::keyword_json, search::collect::collect_hits, search::semantic::SemanticOpts,
    types::Hit,
};

pub fn run(
    conn: &Connection,
    cfg: &Config,
    query: &str,
    tags: &[String],
    projects: &[String],
    files: &[String],
    limit: i64,
    spread: bool,
    as_json: bool,
) -> Result<()> {
    if as_json {
        print_json(&recall_value(
            conn, cfg, query, tags, projects, files, limit, spread,
        )?)?;
        return Ok(());
    }
    render::text::print(conn, cfg, query, tags, projects, files, limit, spread)
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/recall/mod.rs"]
mod tests;
