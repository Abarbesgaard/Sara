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
fn span_stamps_trace_correlation_fields() {
    // A folded sub-op of a composite operation carries its trace context:
    // the shared `trace_id`, an ordinal `seq`, and an optional outcome count
    // `n`. Plain records (no span) omit all three, so the allowlist for a
    // normal invocation is unaffected.
    let _g = lock();
    let path = temp_queue("span");
    let span = Span {
        trace_id: "trace-xyz",
        seq: 4,
        n: Some(3),
    };
    let rec = build_record_span(
        "iid-s",
        Source::Mcp,
        "mcp recall",
        &["via_begin".to_string()],
        13,
        &Ok(()),
        None,
        Some(span),
    );
    append(&path, &rec).unwrap();

    // A record with no span omits the trace fields entirely.
    let plain = build_record("iid-s", Source::Mcp, "mcp next", &[], 1, &Ok(()), None);
    append(&path, &plain).unwrap();

    let rows = read_lines(&path);
    assert_eq!(rows[0]["trace_id"], "trace-xyz");
    assert_eq!(rows[0]["seq"], 4);
    assert_eq!(rows[0]["n"], 3);
    let plain_obj = rows[1].as_object().unwrap();
    assert!(
        plain_obj.get("trace_id").is_none()
            && plain_obj.get("seq").is_none()
            && plain_obj.get("n").is_none(),
        "records without a span carry no trace fields: {}",
        rows[1]
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
    assert!(rows[0].as_object().unwrap().get("client_version").is_none());
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
    let rec = build_record(
        "iid-3",
        Source::Mcp,
        "mcp recall",
        &[],
        4,
        &Ok(()),
        Some(&client),
    );
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
    capture_impl(&cfg, Source::Cli, "recall", &[], 1, &Ok(()), None, None);
    assert!(
        !path.exists(),
        "no queue file created while disabled by env"
    );

    unsafe {
        std::env::remove_var("SARA_NO_TELEMETRY");
    }
    assert!(enabled(&cfg));
    capture_impl(&cfg, Source::Cli, "recall", &[], 1, &Ok(()), None, None);
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
            let resp = format!("{status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
            let _ = stream.write_all(resp.as_bytes());
        }
    });
    (format!("http://{addr}/insert"), rx)
}

/// Multi-request mock: handles one connection per entry in `statuses`,
/// replying with that status line (in order) and forwarding each request
/// body to the channel. Lets a test assert how a backlog is batched.
fn mock_server_multi(statuses: Vec<&'static str>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        for status_line in statuses {
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
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
            let resp = format!("{status_line}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
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
fn flush_chunks_oversized_queue_into_sublimit_batches() {
    let _g = lock();
    let dir = temp_dir_isolated("chunk");
    // ~25-byte records; a 100-byte cap forces ~3 records per POST.
    write_queue(&dir.join("queue.jsonl"), 10);
    let (url, rx) = mock_server_multi(vec!["HTTP/1.1 200 OK"; 6]);
    set_flush_env(&dir, &url);
    unsafe {
        std::env::set_var("SARA_TELEMETRY_MAX_BATCH_BYTES", "100");
    }

    let out = flush(&flush_cfg()).unwrap();
    assert_eq!(out, FlushOutcome::Sent(10), "whole backlog drains");
    assert_eq!(
        super::read_lines(&dir.join("queue.jsonl")).unwrap().len(),
        0,
        "queue fully emptied via multiple sub-cap requests"
    );

    let mut total = 0usize;
    let mut requests = 0usize;
    while let Ok(body) = rx.recv_timeout(std::time::Duration::from_secs(2)) {
        let n = body.lines().filter(|l| !l.trim().is_empty()).count();
        assert!(n >= 1);
        assert!(
            body.len() <= 100 + 30,
            "each POST stays near the cap, got {} bytes",
            body.len()
        );
        total += n;
        requests += 1;
    }
    assert_eq!(total, 10, "every record shipped exactly once");
    assert!(
        requests >= 3,
        "oversized backlog split across POSTs, got {requests}"
    );

    unsafe {
        std::env::remove_var("SARA_TELEMETRY_MAX_BATCH_BYTES");
    }
    clear_flush_env();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn flush_keeps_remainder_when_a_later_chunk_is_rejected() {
    let _g = lock();
    let dir = temp_dir_isolated("chunk-fail");
    write_queue(&dir.join("queue.jsonl"), 6);
    // First batch accepted, second rejected (e.g. a 413) — progress is kept
    // and the unsent tail stays queued, so the queue can never wedge.
    let (url, rx) = mock_server_multi(vec![
        "HTTP/1.1 200 OK",
        "HTTP/1.1 413 Request Entity Too Large",
    ]);
    set_flush_env(&dir, &url);
    unsafe {
        std::env::set_var("SARA_TELEMETRY_MAX_BATCH_BYTES", "100");
    }

    let out = flush(&flush_cfg()).unwrap();
    assert_eq!(out, FlushOutcome::Sent(3), "only the accepted batch drains");
    assert_eq!(
        super::read_lines(&dir.join("queue.jsonl")).unwrap().len(),
        3,
        "the rejected tail remains queued for a later flush"
    );

    let first = rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    assert_eq!(first.lines().filter(|l| !l.trim().is_empty()).count(), 3);

    unsafe {
        std::env::remove_var("SARA_TELEMETRY_MAX_BATCH_BYTES");
    }
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
