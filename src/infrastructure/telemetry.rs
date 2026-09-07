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

        unsafe {
            std::env::remove_var("SARA_NO_TELEMETRY");
        }
        assert!(enabled(&cfg));
        capture(&cfg, Source::Cli, "recall", 1, &Ok(()));
        assert_eq!(read_lines(&path).len(), 1, "capture writes when enabled");

        cfg.telemetry.enabled = false;
        assert!(!enabled(&cfg));

        unsafe {
            std::env::remove_var("SARA_TELEMETRY_QUEUE");
        }
        let _ = std::fs::remove_file(&path);
    }
}
