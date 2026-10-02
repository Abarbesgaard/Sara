use anyhow::Result;
use rusqlite::Connection;

use crate::commands::shared::print_json;

mod apply;
mod cluster;
mod propose;
mod render;
mod types;

pub use apply::apply_value;
pub use propose::reflect_value;

pub const DEFAULT_MIN_WEIGHT: f64 = 0.5;

pub const DEFAULT_MAX_CLUSTER: usize = 8;

pub fn run(
    conn: &Connection,
    min_weight: f64,
    max_cluster: usize,
    json_output: bool,
    apply: bool,
) -> Result<()> {
    let v = if apply {
        apply_value(conn, min_weight, max_cluster)?
    } else {
        reflect_value(conn, min_weight, max_cluster)?
    };
    if json_output {
        return print_json(&v);
    }
    if apply {
        render::print_applied(&v);
    } else {
        render::print_proposal(&v);
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../../tests/unit/commands/reflect/mod.rs"]
mod tests;
