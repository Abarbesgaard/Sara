//! Local telemetry capture.
//!
//! Records one line per CLI command and per MCP tool call into an append-only
//! JSONL queue file so Sara can answer basic questions about its own usage —
//! which commands run, how long they take, and whether they fail. This module
//! is deliberately *capture only*: nothing is transmitted here. A dependent
//! task adds a detached background flush to a self-hosted collector; keeping
//! capture separate lets the exact payload be proven (via `sara telemetry
//! --show`) before a single byte ever leaves a machine.
//!
//! Design rules this module must never break (see memories m493/m494):
//!   * **Separate store.** The queue is its own JSONL file, never the `events`
//!     table — that table is single-purpose for memory strength and every
//!     `FROM events` query in `db.rs` depends on its shape.
//!   * **Allowlist scalars only.** A record carries version, os/arch, the
//!     command *name*, duration, ok/err and a coarse error *code*. It must
//!     never carry argv, file paths, project names, cwd, config, memory bodies
//!     or error message strings.
//!   * **Type-derived error codes.** The crate has no typed error enum (all
//!     `anyhow`), so `err_code` is derived by downcasting to known error
//!     *types*. Deriving it from `err.to_string()` would exfiltrate the very
//!     paths and task text the allowlist exists to keep out.
//!   * **Never block, never mutate a read path.** Capture is a single, cheap,
//!     best-effort file append; any failure is swallowed so telemetry can
//!     never break a real command.

use crate::infrastructure::config::{self, Config};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Where a captured invocation originated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Cli,
    Mcp,
}

impl Source {
    fn as_str(self) -> &'static str {
        match self {
            Source::Cli => "cli",
            Source::Mcp => "mcp",
        }
    }
}

/// One captured invocation. Every field here is an intentional allowlist entry;
/// adding a field is a privacy decision, not a mechanical one.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TelemetryRecord {
    /// Random per-install identifier — stable across invocations, never a user
    /// name or anything that identifies a person.
    pub install_id: String,
    /// RFC3339 UTC timestamp.
    pub ts: String,
    /// "cli" or "mcp".
    pub source: &'static str,
    /// The command / tool *name* only (e.g. "recall", "mcp add") — never argv.
    pub name: String,
    /// Wall-clock duration of the dispatch, in milliseconds.
    pub duration_ms: u64,
    /// Whether the invocation succeeded.
    pub ok: bool,
    /// Coarse, type-derived error class on failure (never a message).
    pub err_code: Option<&'static str>,
    /// Sara version.
    pub version: &'static str,
    /// Target OS (e.g. "macos", "linux").
    pub os: &'static str,
    /// Target architecture (e.g. "x86_64", "aarch64").
    pub arch: &'static str,
}

/// Classify a failure into a coarse, allowlist-safe code by inspecting the
/// error *chain's types* — never its text. Walks the whole `anyhow` chain so a
/// context-wrapped root cause (e.g. an `io::Error` behind a "Failed to write …"
/// context) is still classified.
pub fn err_code(err: &anyhow::Error) -> &'static str {
    for cause in err.chain() {
        if cause.downcast_ref::<std::io::Error>().is_some() {
            return "io";
        }
        if cause.downcast_ref::<rusqlite::Error>().is_some() {
            return "db";
        }
        if cause.downcast_ref::<toml::de::Error>().is_some()
            || cause.downcast_ref::<toml::ser::Error>().is_some()
        {
            return "config";
        }
        if cause.downcast_ref::<serde_json::Error>().is_some() {
            return "serde";
        }
    }
    "other"
}

/// Build a record from a completed invocation's result. Pure and total: no I/O,
/// no environment reads beyond compile-time constants, so it is trivially
/// testable and can never fail.
pub fn build_record<T>(
    install_id: &str,
    source: Source,
    name: &str,
    duration_ms: u64,
    result: &anyhow::Result<T>,
) -> TelemetryRecord {
    TelemetryRecord {
        install_id: install_id.to_string(),
        ts: chrono::Utc::now().to_rfc3339(),
        source: source.as_str(),
        name: name.to_string(),
        duration_ms,
        ok: result.is_ok(),
        err_code: result.as_ref().err().map(err_code),
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

/// Append one record as a single JSON line to the queue file. Pure w.r.t. the
/// environment — the caller supplies the path, which makes it directly testable
/// and keeps the real-path resolution in one place.
pub fn append(queue_path: &Path, rec: &TelemetryRecord) -> std::io::Result<()> {
    if let Some(parent) = queue_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut line = serde_json::to_string(rec).map_err(std::io::Error::other)?;
    line.push('\n');
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(queue_path)?;
    f.write_all(line.as_bytes())
}

/// Absolute path to the append-only telemetry queue, beside the task DB but a
/// distinct file. Honours `SARA_TELEMETRY_QUEUE` as an override (used by tests
/// and by anyone relocating the buffer).
pub fn queue_path() -> anyhow::Result<PathBuf> {
    if let Ok(p) = std::env::var("SARA_TELEMETRY_QUEUE")
        && !p.is_empty()
    {
        return Ok(PathBuf::from(p));
    }
    let dirs = config::project_dirs()?;
    Ok(dirs.data_dir().join("telemetry-queue.jsonl"))
}

/// Path to the persisted install-id file.
fn install_id_path() -> anyhow::Result<PathBuf> {
    let dirs = config::project_dirs()?;
    Ok(dirs.data_dir().join("install_id"))
}

/// Read the stable per-install id, generating and persisting a random one on
/// first use. Best-effort: a read/write failure falls back to an ephemeral id
/// rather than aborting a command.
pub fn install_id() -> String {
    match install_id_path() {
        Ok(path) => {
            if let Ok(existing) = std::fs::read_to_string(&path) {
                let trimmed = existing.trim();
                if !trimmed.is_empty() {
                    return trimmed.to_string();
                }
            }
            let id = uuid::Uuid::new_v4().to_string();
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(&path, &id);
            id
        }
        Err(_) => uuid::Uuid::new_v4().to_string(),
    }
}

/// Whether capture is active. Opt-out precedence: the `SARA_NO_TELEMETRY`
/// environment variable (set to any non-empty value) wins over config, which
/// defaults to enabled.
pub fn enabled(cfg: &Config) -> bool {
    if let Ok(v) = std::env::var("SARA_NO_TELEMETRY")
        && !v.is_empty()
    {
        return false;
    }
    cfg.telemetry.enabled
}

/// Path to the marker written once the first-run notice has been shown. A
/// marker FILE (not a config field) is used deliberately: the ambient
/// per-command path must never rewrite `config.toml`, whose full-config save is
/// lossy w.r.t. sections Sara's `Config` struct does not model.
fn notice_marker_path() -> anyhow::Result<PathBuf> {
    let dirs = config::project_dirs()?;
    Ok(dirs.data_dir().join("telemetry_notice_shown"))
}

/// Print the one-time first-run telemetry notice, then mark it shown. Runs on
/// the hot path of every command, so it is strictly best-effort and never
/// writes config. No-op when telemetry is disabled or the notice already ran.
pub fn maybe_show_notice(cfg: &Config) {
    if !enabled(cfg) {
        return;
    }
    let Ok(marker) = notice_marker_path() else {
        return;
    };
    if marker.exists() {
        return;
    }
    eprintln!(
        "sara records anonymous local usage telemetry (command name, duration, \
         ok/error — never arguments, paths or content). Inspect it with \
         `sara telemetry --show`; disable with `sara telemetry off` or \
         SARA_NO_TELEMETRY=1."
    );
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&marker, b"1");
}

/// Read back every queued record as parsed JSON, for `sara telemetry --show`.
/// Returns an empty vec if the queue does not yet exist.
pub fn read_queue() -> anyhow::Result<Vec<serde_json::Value>> {
    let path = queue_path()?;
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim().is_empty() {
            continue;
        }
        out.push(serde_json::from_str(line)?);
    }
    Ok(out)
}

/// Capture one completed invocation. Best-effort and side-effect-isolated: when
/// disabled it does nothing, and any I/O failure is swallowed so telemetry can
/// never break or slow a real command.
pub fn capture<T>(
    cfg: &Config,
    source: Source,
    name: &str,
    duration_ms: u64,
    result: &anyhow::Result<T>,
) {
    if !enabled(cfg) {
        return;
    }
    let Ok(path) = queue_path() else {
        return;
    };
    let rec = build_record(&install_id(), source, name, duration_ms, result);
    let _ = append(&path, &rec);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    // The tests below mutate process-wide state (env vars, HOME-derived paths),
    // so they must not run concurrently.
    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    fn lock() -> MutexGuard<'static, ()> {
        TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn temp_queue(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "sara-tel-{name}-{}-{:?}.jsonl",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_file(&p);
        p
    }

    fn read_lines(path: &Path) -> Vec<serde_json::Value> {
        let text = std::fs::read_to_string(path).unwrap_or_default();
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect()
    }

    #[test]
    fn records_cli_invocation() {
        let _g = lock();
        let path = temp_queue("cli");
        let rec = build_record("iid-1", Source::Cli, "recall", 12, &Ok(()));
        append(&path, &rec).unwrap();

        let rows = read_lines(&path);
        assert_eq!(rows.len(), 1, "exactly one record appended");
        let r = &rows[0];
        assert_eq!(r["source"], "cli");
        assert_eq!(r["name"], "recall");
        assert_eq!(r["ok"], true);
        assert_eq!(r["duration_ms"], 12);
        assert_eq!(r["install_id"], "iid-1");
        assert!(r["err_code"].is_null());
        // Allowlist guard: only the intended scalar keys, nothing else.
        let keys: std::collections::BTreeSet<&str> =
            r.as_object().unwrap().keys().map(String::as_str).collect();
        let expected: std::collections::BTreeSet<&str> = [
            "install_id",
            "ts",
            "source",
            "name",
            "duration_ms",
            "ok",
            "err_code",
            "version",
            "os",
            "arch",
        ]
        .into_iter()
        .collect();
        assert_eq!(keys, expected, "payload is exactly the allowlist, no leaks");

        // A second append accumulates rather than overwriting.
        append(
            &path,
            &build_record("iid-1", Source::Cli, "list", 3, &Ok(())),
        )
        .unwrap();
        assert_eq!(read_lines(&path).len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn records_mcp_invocation() {
        let _g = lock();
        let path = temp_queue("mcp");
        let rec = build_record("iid-2", Source::Mcp, "mcp add", 7, &Ok(()));
        append(&path, &rec).unwrap();

        let rows = read_lines(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["source"], "mcp");
        assert_eq!(rows[0]["name"], "mcp add");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn err_code_is_type_derived() {
        let _g = lock();

        // io::Error, even when wrapped in an anyhow context string, classifies
        // as "io" — and the message (which could hold a path) is never read.
        let io_err: anyhow::Error = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "/Users/secret/tasks.db missing",
        ))
        .context("Failed to write /Users/secret/queue.jsonl");
        assert_eq!(err_code(&io_err), "io");

        // A rusqlite error classifies as "db".
        let db_err: anyhow::Error =
            anyhow::Error::new(rusqlite::Error::QueryReturnedNoRows).context("loading task");
        assert_eq!(err_code(&db_err), "db");

        // serde_json error classifies as "serde".
        let serde_err: anyhow::Error =
            anyhow::Error::new(serde_json::from_str::<i32>("nope").unwrap_err());
        assert_eq!(err_code(&serde_err), "serde");

        // An error of no known type falls back to the generic bucket.
        let other: anyhow::Error = anyhow::anyhow!("something bespoke");
        assert_eq!(err_code(&other), "other");

        // The code carried on a failing record is the type code, and the record
        // never contains the (path-bearing) message.
        let failed: anyhow::Result<()> = Err(io_err);
        let rec = build_record("iid", Source::Cli, "validate", 5, &failed);
        assert!(!rec.ok);
        assert_eq!(rec.err_code, Some("io"));
        let json = serde_json::to_string(&rec).unwrap();
        assert!(
            !json.contains("secret"),
            "record must not embed the error message"
        );
    }

    #[test]
    fn disabled_writes_nothing() {
        let _g = lock();
        let path = temp_queue("disabled");

        let mut cfg = Config::default();
        cfg.telemetry.enabled = true;

        // SARA_NO_TELEMETRY wins over an enabled config.
        // SAFETY: guarded by the test lock; no other thread reads env here.
        unsafe {
            std::env::set_var("SARA_NO_TELEMETRY", "1");
            std::env::set_var("SARA_TELEMETRY_QUEUE", &path);
        }
        assert!(!enabled(&cfg));
        capture(&cfg, Source::Cli, "recall", 1, &Ok(()));
        assert!(
            !path.exists(),
            "no queue file created while disabled by env"
        );

        // With the env cleared and config enabled, capture writes.
        unsafe {
            std::env::remove_var("SARA_NO_TELEMETRY");
        }
        assert!(enabled(&cfg));
        capture(&cfg, Source::Cli, "recall", 1, &Ok(()));
        assert_eq!(read_lines(&path).len(), 1, "capture writes when enabled");

        // Config opt-out also suppresses, regardless of env.
        cfg.telemetry.enabled = false;
        assert!(!enabled(&cfg));

        unsafe {
            std::env::remove_var("SARA_TELEMETRY_QUEUE");
        }
        let _ = std::fs::remove_file(&path);
    }
}
