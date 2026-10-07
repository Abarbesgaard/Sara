//! Folded-operation trace for `begin`. With the `telemetry` feature, every internal
//! op `begin` composes emits its own ordered telemetry event (stamped with one
//! random `trace_id` and a `seq`) and is returned under the result's `trace` object (`id`, `ops`).
//! Without the feature this is a zero-sized no-op and the result is unchanged.

use std::time::Instant;

use serde_json::Value;

use crate::infrastructure::config::Config;
use crate::infrastructure::telemetry::Source;

#[cfg(feature = "telemetry")]
pub(super) struct Folded<'a> {
    cfg: &'a Config,
    source: Source,
    trace_id: String,
    log: Vec<Value>,
}

#[cfg(feature = "telemetry")]
impl<'a> Folded<'a> {
    pub(super) fn new(cfg: &'a Config, source: Source) -> Self {
        Self {
            cfg,
            source,
            trace_id: uuid::Uuid::new_v4().to_string(),
            log: Vec::new(),
        }
    }

    /// Record one folded op; `n` is an optional non-identifying outcome count.
    pub(super) fn emit(&mut self, op: &str, started: Instant, n: Option<u64>) {
        use crate::infrastructure::telemetry::{self, Span};
        use serde_json::json;

        let dur_ms = started.elapsed().as_millis() as u64;
        let seq = self.log.len() as u32;
        let name = match self.source {
            Source::Mcp => format!("mcp {op}"),
            Source::Cli => op.to_string(),
        };
        telemetry::capture_span(
            self.cfg,
            self.source,
            &name,
            &["via_begin".to_string()],
            dur_ms,
            &Ok::<(), anyhow::Error>(()),
            None,
            Some(Span {
                trace_id: &self.trace_id,
                seq,
                n,
            }),
        );
        let mut entry = json!({ "op": op, "seq": seq, "duration_ms": dur_ms });
        if let Some(n) = n {
            entry["n"] = json!(n);
        }
        self.log.push(entry);
    }

    pub(super) fn finish(self, out: &mut Value) {
        out["trace"] = serde_json::json!({ "id": self.trace_id, "ops": self.log });
    }
}

#[cfg(not(feature = "telemetry"))]
pub(super) struct Folded;

#[cfg(not(feature = "telemetry"))]
impl Folded {
    pub(super) fn new(_cfg: &Config, _source: Source) -> Self {
        Folded
    }

    pub(super) fn emit(&mut self, _op: &str, _started: Instant, _n: Option<u64>) {}

    pub(super) fn finish(self, _out: &mut Value) {}
}
