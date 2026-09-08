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
    /// The named options a call used, stored as a JSON array so a collector can
    /// `unroll` it for per-name aggregation. For CLI records these are flag
    /// NAMES (e.g. `["--json","--tag","-p"]`, see [`extract_cli_flags`]); for
    /// MCP records they are the tool's argument NAMES (e.g.
    /// `["dry_run","project_path"]`, see [`extract_mcp_params`]). Values are
    /// never recorded, and the field is omitted entirely when empty.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub flags: Vec<String>,
    pub duration_ms: u64,
    pub ok: bool,
    pub err_code: Option<&'static str>,
    pub version: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    /// For MCP records, the calling client's reported name from the `initialize`
    /// handshake (e.g. `claude-ai`, `cursor`, `Copilot`) — the *origin* of the
    /// tool call. `None` for CLI records and for MCP clients that sent no name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
    /// For MCP records, the calling client's reported version, paired with
    /// [`client`]. `None` for CLI records.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_version: Option<String>,
}

/// Identity of an MCP client, taken from the `initialize` handshake's
/// `clientInfo`. Used to record *which agent* invoked a tool.
#[derive(Debug, Clone)]
pub struct McpClient {
    pub name: String,
    pub version: String,
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

/// Extract parameter NAMES (never values) from an MCP tool call's raw JSON
/// arguments, for telemetry. The MCP analog of [`extract_cli_flags`]: returns
/// the sorted, deduplicated names of the arguments the client actually sent
/// (e.g. `["dry_run","project_path","tag"]`), with all *values* discarded so
/// task ids, queries, file paths and bodies never reach the collector. A key
/// whose value is JSON `null` is treated as absent.
pub fn extract_mcp_params(
    arguments: Option<&serde_json::Map<String, serde_json::Value>>,
) -> Vec<String> {
    let Some(map) = arguments else {
        return Vec::new();
    };
    let mut names: Vec<String> = map
        .iter()
        .filter(|(_, v)| !v.is_null())
        .map(|(k, _)| k.clone())
        .collect();
    names.sort();
    names.dedup();
    names
}

pub fn build_record<T>(
    install_id: &str,
    source: Source,
    name: &str,
    flags: &[String],
    duration_ms: u64,
    result: &anyhow::Result<T>,
    client: Option<&McpClient>,
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
        client: client.map(|c| c.name.clone()),
        client_version: client.map(|c| c.version.clone()),
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
    client: Option<&McpClient>,
) {
    // The test binary must never emit telemetry: seed/setup helpers would
    // otherwise write bogus records (e.g. "seed") that later flush to the
    // collector and pollute the dashboards. Unit tests exercise the real
    // gating/writing path through `capture_impl` directly.
    if cfg!(test) {
        return;
    }
    capture_impl(cfg, source, name, flags, duration_ms, result, client);
}

fn capture_impl<T>(
    cfg: &Config,
    source: Source,
    name: &str,
    flags: &[String],
    duration_ms: u64,
    result: &anyhow::Result<T>,
    client: Option<&McpClient>,
) {
    if !enabled(cfg) {
        return;
    }
    let Ok(path) = queue_path() else {
        return;
    };
    let rec = build_record(&install_id(), source, name, flags, duration_ms, result, client);
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
     ?_time_field=ts&_msg_field=name&_stream_fields=install_id,source,version,client",
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
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard, OnceLock};

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
        let flags = [String::from("--json"), String::from("--tag")];
        let rec = build_record("iid-1", Source::Cli, "recall", &flags, 12, &Ok(()), None);
        append(&path, &rec).unwrap();

        let rows = read_lines(&path);
        assert_eq!(rows.len(), 1, "exactly one record appended");
        let r = &rows[0];
        assert_eq!(r["source"], "cli");
        assert_eq!(r["name"], "recall");
        assert_eq!(r["flags"], serde_json::json!(["--json", "--tag"]));
        assert_eq!(r["ok"], true);
        assert_eq!(r["duration_ms"], 12);
        assert_eq!(r["install_id"], "iid-1");
        assert!(r["err_code"].is_null());
        let keys: std::collections::BTreeSet<&str> =
            r.as_object().unwrap().keys().map(String::as_str).collect();
        let expected: std::collections::BTreeSet<&str> = [
            "install_id",
            "ts",
            "source",
            "name",
            "flags",
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

        append(
            &path,
            &build_record("iid-1", Source::Cli, "list", &[], 3, &Ok(()), None),
        )
        .unwrap();
        let rows = read_lines(&path);
        assert_eq!(rows.len(), 2);
        assert!(
            rows[1].as_object().unwrap().get("flags").is_none(),
            "flags omitted entirely when empty"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn records_mcp_invocation() {
        let _g = lock();
        let path = temp_queue("mcp");
        let rec = build_record("iid-2", Source::Mcp, "mcp add", &[], 7, &Ok(()), None);
        append(&path, &rec).unwrap();

        let rows = read_lines(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["source"], "mcp");
        assert_eq!(rows[0]["name"], "mcp add");
        // No client info supplied → the origin fields are omitted entirely.
        assert!(rows[0].as_object().unwrap().get("client").is_none());
        assert!(
            rows[0]
                .as_object()
                .unwrap()
                .get("client_version")
                .is_none()
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn records_mcp_client_origin() {
        let _g = lock();
        let path = temp_queue("mcp-client");
        let client = McpClient {
            name: "claude-ai".into(),
            version: "0.1.0".into(),
        };
        let rec = build_record("iid-3", Source::Mcp, "mcp recall", &[], 4, &Ok(()), Some(&client));
        append(&path, &rec).unwrap();

        let rows = read_lines(&path);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["client"], "claude-ai", "records the calling agent");
        assert_eq!(rows[0]["client_version"], "0.1.0");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn extract_cli_flags_names_only_sorted_deduped() {
        let a = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();

        // values are excluded; long+short captured; sorted + deduped
        assert_eq!(
            extract_cli_flags(&a("recall --tag herdr -p pling --json")),
            vec!["--json", "--tag", "-p"]
        );
        // `=value` form keeps only the name; duplicates collapse
        assert_eq!(
            extract_cli_flags(&a("learn --tag=a --tag=b --auto-files")),
            vec!["--auto-files", "--tag"]
        );
        // bare `--`, negative numbers, and a lone `-` are not flags
        assert!(extract_cli_flags(&a("modify 5 -- -3 -")).is_empty());
        // no flags at all
        assert!(extract_cli_flags(&a("done 3f45")).is_empty());
    }

    #[test]
    fn extract_mcp_params_names_only_sorted_deduped() {
        use serde_json::json;

        // argument NAMES are captured, sorted; values (a query, a path) are
        // never included in the output.
        let args = json!({
            "query": "how did I fix the auth bug",
            "project_path": "/Users/secret/repo",
            "spread": true,
        });
        assert_eq!(
            extract_mcp_params(args.as_object()),
            vec!["project_path", "query", "spread"]
        );

        // a key whose value is JSON null counts as absent
        let with_null = json!({ "id": "3f45", "project_path": null });
        assert_eq!(extract_mcp_params(with_null.as_object()), vec!["id"]);

        // no arguments at all
        assert!(extract_mcp_params(None).is_empty());
        assert!(extract_mcp_params(json!({}).as_object()).is_empty());
    }

    #[test]
    fn err_code_is_type_derived() {
        let _g = lock();

        let io_err: anyhow::Error = anyhow::Error::new(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "/Users/secret/tasks.db missing",
        ))
        .context("Failed to write /Users/secret/queue.jsonl");
        assert_eq!(err_code(&io_err), "io");

        let db_err: anyhow::Error =
            anyhow::Error::new(rusqlite::Error::QueryReturnedNoRows).context("loading task");
        assert_eq!(err_code(&db_err), "db");

        let serde_err: anyhow::Error =
            anyhow::Error::new(serde_json::from_str::<i32>("nope").unwrap_err());
        assert_eq!(err_code(&serde_err), "serde");

        let other: anyhow::Error = anyhow::anyhow!("something bespoke");
        assert_eq!(err_code(&other), "other");

        let failed: anyhow::Result<()> = Err(io_err);
        let rec = build_record("iid", Source::Cli, "validate", &[], 5, &failed, None);
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

        unsafe {
            std::env::set_var("SARA_NO_TELEMETRY", "1");
            std::env::set_var("SARA_TELEMETRY_QUEUE", &path);
        }
        assert!(!enabled(&cfg));
        capture_impl(&cfg, Source::Cli, "recall", &[], 1, &Ok(()), None);
        assert!(
            !path.exists(),
            "no queue file created while disabled by env"
        );

        unsafe {
            std::env::remove_var("SARA_NO_TELEMETRY");
        }
        assert!(enabled(&cfg));
        capture_impl(&cfg, Source::Cli, "recall", &[], 1, &Ok(()), None);
        assert_eq!(read_lines(&path).len(), 1, "capture writes when enabled");

        cfg.telemetry.enabled = false;
        assert!(!enabled(&cfg));

        unsafe {
            std::env::remove_var("SARA_TELEMETRY_QUEUE");
        }
        let _ = std::fs::remove_file(&path);
    }

    // ── flush tests ──────────────────────────────────────────────────────────
    use std::io::Read as _;
    use std::net::TcpListener;
    use std::sync::mpsc;
    use std::thread;

    fn temp_dir_isolated(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "sara-flush-{name}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn write_queue(path: &Path, n: usize) {
        let mut body = String::new();
        for i in 0..n {
            body.push_str(&format!("{{\"name\":\"cmd{i}\",\"ok\":true}}\n"));
        }
        std::fs::write(path, body).unwrap();
    }

    /// One-shot HTTP server: returns (url, receiver-of-request-body). `status_line`
    /// e.g. "HTTP/1.1 200 OK" or "HTTP/1.1 500 Internal Server Error".
    fn mock_server(status_line: &'static str) -> (String, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf: Vec<u8> = Vec::new();
                let mut tmp = [0u8; 2048];
                loop {
                    let n = stream.read(&mut tmp).unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&tmp[..n]);
                    if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        let headers = String::from_utf8_lossy(&buf[..pos]).to_lowercase();
                        let clen = headers
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length:"))
                            .and_then(|v| v.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        let body_start = pos + 4;
                        while buf.len() < body_start + clen {
                            let n = stream.read(&mut tmp).unwrap_or(0);
                            if n == 0 {
                                break;
                            }
                            buf.extend_from_slice(&tmp[..n]);
                        }
                        let end = (body_start + clen).min(buf.len());
                        let body = String::from_utf8_lossy(&buf[body_start..end]).to_string();
                        let _ = tx.send(body);
                        break;
                    }
                }
                use std::io::Write as _;
                let resp =
                    format!("{status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                let _ = stream.write_all(resp.as_bytes());
            }
        });
        (format!("http://{addr}/insert"), rx)
    }

    fn flush_cfg() -> Config {
        let mut cfg = Config::default();
        cfg.telemetry.enabled = true;
        cfg
    }

    fn set_flush_env(dir: &Path, endpoint: &str) {
        unsafe {
            std::env::remove_var("SARA_NO_TELEMETRY");
            std::env::set_var("SARA_TELEMETRY_QUEUE", dir.join("queue.jsonl"));
            std::env::set_var("SARA_TELEMETRY_ENDPOINT", endpoint);
            std::env::set_var("SARA_TELEMETRY_FLUSH_INTERVAL", "0");
        }
    }

    fn clear_flush_env() {
        unsafe {
            std::env::remove_var("SARA_TELEMETRY_QUEUE");
            std::env::remove_var("SARA_TELEMETRY_ENDPOINT");
            std::env::remove_var("SARA_TELEMETRY_FLUSH_INTERVAL");
            std::env::remove_var("SARA_NO_TELEMETRY");
        }
    }

    #[test]
    fn flush_sends_and_truncates_queue() {
        let _g = lock();
        let dir = temp_dir_isolated("send");
        let (url, rx) = mock_server("HTTP/1.1 200 OK");
        set_flush_env(&dir, &url);
        write_queue(&dir.join("queue.jsonl"), 3);

        let out = flush(&flush_cfg()).unwrap();
        assert_eq!(out, FlushOutcome::Sent(3));

        let body = rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(body.lines().filter(|l| !l.trim().is_empty()).count(), 3);
        assert_eq!(
            super::read_lines(&dir.join("queue.jsonl")).unwrap().len(),
            0
        );

        clear_flush_env();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flush_leaves_queue_on_failure() {
        let _g = lock();
        let dir = temp_dir_isolated("fail");
        let (url, _rx) = mock_server("HTTP/1.1 500 Internal Server Error");
        set_flush_env(&dir, &url);
        write_queue(&dir.join("queue.jsonl"), 4);

        let out = flush(&flush_cfg()).unwrap();
        assert_eq!(out, FlushOutcome::Failed);
        assert_eq!(
            super::read_lines(&dir.join("queue.jsonl")).unwrap().len(),
            4,
            "no records dropped when the collector rejects the push"
        );

        clear_flush_env();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flush_respects_optout() {
        let _g = lock();
        let dir = temp_dir_isolated("optout");
        let (url, _rx) = mock_server("HTTP/1.1 200 OK");
        set_flush_env(&dir, &url);
        write_queue(&dir.join("queue.jsonl"), 2);
        unsafe {
            std::env::set_var("SARA_NO_TELEMETRY", "1");
        }

        let out = flush(&flush_cfg()).unwrap();
        assert_eq!(out, FlushOutcome::Skipped);
        assert_eq!(
            super::read_lines(&dir.join("queue.jsonl")).unwrap().len(),
            2,
            "opt-out leaves the queue untouched and sends nothing"
        );

        clear_flush_env();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flush_min_interval_gates_repeat_sends() {
        let _g = lock();
        let dir = temp_dir_isolated("interval");
        let (url, _rx) = mock_server("HTTP/1.1 200 OK");
        set_flush_env(&dir, &url);
        // A big interval + a fresh last-flush marker means: skip despite data.
        unsafe {
            std::env::set_var("SARA_TELEMETRY_FLUSH_INTERVAL", "3600");
        }
        std::fs::write(dir.join("telemetry-last-flush"), b"1").unwrap();
        write_queue(&dir.join("queue.jsonl"), 2);

        let out = flush(&flush_cfg()).unwrap();
        assert_eq!(out, FlushOutcome::Skipped);
        assert_eq!(
            super::read_lines(&dir.join("queue.jsonl")).unwrap().len(),
            2
        );

        clear_flush_env();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn remove_prefix_lines_keeps_later_appends() {
        let _g = lock();
        let dir = temp_dir_isolated("prefix");
        let path = dir.join("queue.jsonl");
        write_queue(&path, 5);

        remove_prefix_lines(&path, 3).unwrap();

        let rows = super::read_lines(&path).unwrap();
        assert_eq!(rows.len(), 2, "only the first 3 sent lines are dropped");
        assert!(rows[0].contains("cmd3"));
        assert!(rows[1].contains("cmd4"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn flush_single_sender_lock_blocks_second() {
        let _g = lock();
        let dir = temp_dir_isolated("lock");
        set_flush_env(&dir, "http://127.0.0.1:9/insert");
        // Hold the lock, then a flush attempt must skip rather than double-send.
        let held = FlushLock::try_acquire().unwrap();
        assert!(held.is_some(), "first acquire succeeds");
        write_queue(&dir.join("queue.jsonl"), 1);

        let out = flush(&flush_cfg()).unwrap();
        assert_eq!(out, FlushOutcome::Skipped);

        drop(held);
        clear_flush_env();
        let _ = std::fs::remove_dir_all(&dir);
    }
}
