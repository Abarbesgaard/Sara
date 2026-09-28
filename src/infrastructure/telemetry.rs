use crate::infrastructure::config::{self, Config};
use serde::Serialize;
use std::io::Write;
use std::path::{Path, PathBuf};

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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TelemetryRecord {
    pub install_id: String,
    pub ts: String,
    pub source: &'static str,
    pub name: String,
    /// Flag NAMES supplied on the CLI (e.g. `["--json","--tag","-p"]`), sorted and
    /// deduped. Stored as a JSON array so VictoriaLogs `unroll by (flags)` can
    /// expand it for per-flag aggregation. Flag *values* are never recorded, and
    /// the field is omitted entirely when empty (all MCP calls, flagless CLI calls).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    pub duration_ms: u64,
    pub ok: bool,
    pub err_code: Option<&'static str>,
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
}

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

/// Extract flag NAMES (never values) from CLI argv, for telemetry.
///
/// Keeps only tokens that begin with `-` (short or long), strips any `=value`
/// suffix, drops the bare `--` end-of-options marker and negative-number
/// operands, then returns a sorted, deduplicated list. Value tokens
/// (`--tag foo`, `-p proj`) are naturally excluded because they do not start
/// with `-`, so free-text and secrets never reach the collector.
pub fn extract_cli_flags(args: &[String]) -> Vec<String> {
    let mut flags: Vec<String> = args
        .iter()
        .filter(|a| {
            a.len() > 1
                && a.starts_with('-')
                && a.as_str() != "--"
                && !a.as_bytes().get(1).is_some_and(u8::is_ascii_digit)
        })
        .map(|a| a.split('=').next().unwrap_or(a).to_string())
        .collect();
    flags.sort();
    flags.dedup();
    flags
}

pub fn build_record<T>(
    install_id: &str,
    source: Source,
    name: &str,
    flags: &[String],
    duration_ms: u64,
    result: &anyhow::Result<T>,
) -> TelemetryRecord {
    TelemetryRecord {
        install_id: install_id.to_string(),
        ts: chrono::Utc::now().to_rfc3339(),
        source: source.as_str(),
        name: name.to_string(),
        flags: flags.to_vec(),
        duration_ms,
        ok: result.is_ok(),
        err_code: result.as_ref().err().map(err_code),
        version: env!("CARGO_PKG_VERSION"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
    }
}

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

pub fn queue_path() -> anyhow::Result<PathBuf> {
    if let Ok(p) = std::env::var("SARA_TELEMETRY_QUEUE")
        && !p.is_empty()
    {
        return Ok(PathBuf::from(p));
    }
    let dirs = config::project_dirs()?;
    Ok(dirs.data_dir().join("telemetry-queue.jsonl"))
}

fn install_id_path() -> anyhow::Result<PathBuf> {
    let dirs = config::project_dirs()?;
    Ok(dirs.data_dir().join("install_id"))
}

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

pub fn enabled(cfg: &Config) -> bool {
    if let Ok(v) = std::env::var("SARA_NO_TELEMETRY")
        && !v.is_empty()
    {
        return false;
    }
    cfg.telemetry.enabled
}

fn notice_marker_path() -> anyhow::Result<PathBuf> {
    let dirs = config::project_dirs()?;
    Ok(dirs.data_dir().join("telemetry_notice_shown"))
}

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

pub fn capture<T>(
    cfg: &Config,
    source: Source,
    name: &str,
    flags: &[String],
    duration_ms: u64,
    result: &anyhow::Result<T>,
) {
    if !enabled(cfg) {
        return;
    }
    let Ok(path) = queue_path() else {
        return;
    };
    let rec = build_record(&install_id(), source, name, flags, duration_ms, result);
    let _ = append(&path, &rec);
}

// ── Auto-flush (nightly) ─────────────────────────────────────────────────────
// Capture writes records to the local queue; the flush ships them to a collector
// and removes exactly the records it sent. It runs in a DETACHED child so the
// short-lived CLI never blocks, is serialised by a single-sender lock, and is
// throttled so it does not POST on every invocation.

/// Compiled-in default collector (the nightly self-hosted VictoriaLogs). Overridable
/// by `SARA_TELEMETRY_ENDPOINT` or `config.telemetry.endpoint`.
pub const DEFAULT_ENDPOINT: Option<&str> = Some(
    "http://100.72.1.121:9428/insert/jsonline\
     ?_time_field=ts&_msg_field=name&_stream_fields=install_id,source,version",
);

const DEFAULT_FLUSH_INTERVAL_SECS: u64 = 60;

/// Resolve the collector endpoint: env > config > compiled default.
pub fn resolve_endpoint(cfg: &Config) -> Option<String> {
    if let Ok(e) = std::env::var("SARA_TELEMETRY_ENDPOINT")
        && !e.is_empty()
    {
        return Some(e);
    }
    if let Some(e) = &cfg.telemetry.endpoint
        && !e.is_empty()
    {
        return Some(e.clone());
    }
    DEFAULT_ENDPOINT.map(str::to_string)
}

fn resolve_token(cfg: &Config) -> Option<String> {
    if let Ok(t) = std::env::var("SARA_TELEMETRY_TOKEN")
        && !t.is_empty()
    {
        return Some(t);
    }
    cfg.telemetry.token.clone().filter(|t| !t.is_empty())
}

fn flush_interval() -> std::time::Duration {
    let secs = std::env::var("SARA_TELEMETRY_FLUSH_INTERVAL")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(DEFAULT_FLUSH_INTERVAL_SECS);
    std::time::Duration::from_secs(secs)
}

/// A file living beside the queue so `SARA_TELEMETRY_QUEUE` redirects it too
/// (keeping tests off the real data dir).
fn queue_sibling(name: &str) -> anyhow::Result<PathBuf> {
    let q = queue_path()?;
    let parent = q.parent().unwrap_or_else(|| Path::new("."));
    Ok(parent.join(name))
}

/// Exclusive single-sender lock. `create_new` is atomic; a lingering lock from a
/// crashed flush older than the flush interval is treated as stale and stolen.
struct FlushLock(PathBuf);

impl FlushLock {
    fn try_acquire() -> anyhow::Result<Option<FlushLock>> {
        let path = queue_sibling("telemetry-flush.lock")?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(_) => Ok(Some(FlushLock(path))),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                let stale = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.elapsed().ok())
                    .map(|age| age > flush_interval().max(std::time::Duration::from_secs(60)))
                    .unwrap_or(false);
                if stale {
                    let _ = std::fs::remove_file(&path);
                    match std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&path)
                    {
                        Ok(_) => Ok(Some(FlushLock(path))),
                        Err(_) => Ok(None),
                    }
                } else {
                    Ok(None)
                }
            }
            Err(e) => Err(e.into()),
        }
    }
}

impl Drop for FlushLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn within_min_interval() -> bool {
    let Ok(marker) = queue_sibling("telemetry-last-flush") else {
        return false;
    };
    std::fs::metadata(&marker)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .map(|age| age < flush_interval())
        .unwrap_or(false)
}

fn touch_last_flush() {
    if let Ok(marker) = queue_sibling("telemetry-last-flush") {
        if let Some(parent) = marker.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&marker, b"1");
    }
}

fn read_lines(path: &Path) -> std::io::Result<Vec<String>> {
    match std::fs::read_to_string(path) {
        Ok(t) => Ok(t
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(String::from)
            .collect()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

/// Drop the first `n` lines, keeping any lines appended after they were read.
/// Append-only + the single-sender lock guarantee the first `n` lines are exactly
/// what was sent. Rewrites atomically via temp-file + rename.
fn remove_prefix_lines(path: &Path, n: usize) -> std::io::Result<()> {
    let remaining = read_lines(path)?;
    let kept = if n >= remaining.len() {
        Vec::new()
    } else {
        remaining[n..].to_vec()
    };
    let tmp = path.with_extension("jsonl.tmp");
    if kept.is_empty() {
        std::fs::write(&tmp, b"")?;
    } else {
        let mut body = kept.join("\n");
        body.push('\n');
        std::fs::write(&tmp, body.as_bytes())?;
    }
    std::fs::rename(&tmp, path)
}

/// POST the JSONL body. Ok(true) on HTTP 2xx, Ok(false) on any other status,
/// Err on a transport failure. Non-2xx and transport failures both leave the queue.
fn post_jsonl(endpoint: &str, token: Option<&str>, body: &str) -> anyhow::Result<bool> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let mut req = agent
        .post(endpoint)
        .set("Content-Type", "application/stream+json");
    if let Some(t) = token {
        req = req.set("Authorization", &format!("Bearer {t}"));
    }
    match req.send_string(body) {
        Ok(resp) => Ok((200..300).contains(&resp.status())),
        Err(ureq::Error::Status(code, _)) => Ok((200..300).contains(&code)),
        Err(e) => Err(anyhow::anyhow!("telemetry transport error: {e}")),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum FlushOutcome {
    Skipped,
    Empty,
    Sent(usize),
    Failed,
}

/// Ship the queued records to the collector and remove exactly those sent.
/// Never panics; every failure mode leaves the queue intact.
pub fn flush(cfg: &Config) -> anyhow::Result<FlushOutcome> {
    if !enabled(cfg) {
        return Ok(FlushOutcome::Skipped);
    }
    let Some(endpoint) = resolve_endpoint(cfg) else {
        return Ok(FlushOutcome::Skipped);
    };
    let Some(_lock) = FlushLock::try_acquire()? else {
        return Ok(FlushOutcome::Skipped);
    };
    if within_min_interval() {
        return Ok(FlushOutcome::Skipped);
    }
    let path = queue_path()?;
    let lines = read_lines(&path)?;
    if lines.is_empty() {
        return Ok(FlushOutcome::Empty);
    }
    let n = lines.len();
    let mut body = lines.join("\n");
    body.push('\n');
    let sent = post_jsonl(&endpoint, resolve_token(cfg).as_deref(), &body)?;
    if !sent {
        return Ok(FlushOutcome::Failed);
    }
    remove_prefix_lines(&path, n)?;
    touch_last_flush();
    Ok(FlushOutcome::Sent(n))
}

/// Spawn the flush as a fully-detached child so the caller never blocks.
pub fn spawn_flush(cfg: &Config) {
    if !enabled(cfg) || resolve_endpoint(cfg).is_none() {
        return;
    }
    let Ok(exe) = std::env::current_exe() else {
        return;
    };
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("__telemetry_flush")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    let _ = cmd.spawn();
}

#[cfg(test)]
#[path = "../../tests/unit/infrastructure/telemetry.rs"]
mod tests;
