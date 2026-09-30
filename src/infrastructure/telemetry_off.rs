//! No-op telemetry for builds without the `telemetry` feature. Mirrors the call-site
//! API of `telemetry.rs` so callers compile unchanged; every function is empty, so
//! nothing is recorded, queued, or sent.

use crate::infrastructure::config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Cli,
    Mcp,
}

#[derive(Debug, Clone, Copy)]
pub struct Span<'a> {
    pub trace_id: &'a str,
    pub seq: u32,
    pub n: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct McpClient {
    pub name: String,
    pub version: String,
}

pub const NOT_COMPILED: &str = "telemetry not compiled into this build";

pub fn extract_cli_flags(_args: &[String]) -> Vec<String> {
    Vec::new()
}

pub fn maybe_show_notice(_cfg: &Config) {}

pub fn capture<T>(
    _cfg: &Config,
    _source: Source,
    _name: &str,
    _flags: &[String],
    _duration_ms: u64,
    _result: &anyhow::Result<T>,
    _client: Option<&McpClient>,
) {
}

pub fn spawn_flush(_cfg: &Config) {}
