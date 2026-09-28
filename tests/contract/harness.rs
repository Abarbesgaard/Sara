//! Shared fixtures for the contract suite: an isolated `sara` environment, a
//! deterministic seed, a volatile-field redactor, and a minimal MCP stdio
//! client that speaks newline-delimited JSON-RPC to `sara mcp`.

#![allow(dead_code)] // helpers are shared across sibling modules; not all used by each.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use serde_json::{Value, json};
use tempfile::TempDir;

/// An isolated sara install: a temp `HOME`/`XDG_*` root plus a project folder,
/// so tests never touch the developer's real `~/.local/share/sara`.
pub struct Sara {
    home: TempDir,
    project: PathBuf,
}

impl Sara {
    /// Fresh, empty install with one initialised project ("Demo").
    pub fn new() -> Self {
        let home = tempfile::tempdir().expect("tempdir");
        let project = home.path().join("project");
        std::fs::create_dir_all(&project).expect("mkdir project");
        let s = Sara { home, project };
        s.run(&["init", "--name", "Demo", "-y"]);
        s
    }

    /// The project directory sara commands operate in.
    pub fn project(&self) -> &Path {
        &self.project
    }

    /// A `Command` for the compiled `sara` binary, pinned to this install and
    /// run from the project directory.
    pub fn cmd(&self) -> Command {
        let mut c = Command::new(bin());
        c.current_dir(&self.project)
            .env("HOME", self.home.path())
            .env("XDG_DATA_HOME", self.home.path().join("data"))
            .env("XDG_CONFIG_HOME", self.home.path().join("config"))
            .env("SARA_NO_TELEMETRY", "1")
            .env("NO_COLOR", "1");
        c
    }

    /// Run a sara subcommand to completion, asserting success, and return stdout.
    pub fn run(&self, args: &[&str]) -> String {
        let out = self.cmd().args(args).output().expect("spawn sara");
        assert!(
            out.status.success(),
            "`sara {}` failed ({}):\n{}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr),
        );
        String::from_utf8(out.stdout).expect("utf8 stdout")
    }

    /// Run a sara subcommand whose stdout is JSON and return the parsed value.
    pub fn json(&self, args: &[&str]) -> Value {
        serde_json::from_str(&self.run(args)).expect("stdout is valid json")
    }

    /// Start an MCP session (`sara mcp`) against this install and complete the
    /// `initialize` handshake so `tools/call` requests are accepted.
    pub fn mcp(&self) -> Mcp {
        let mut child = Command::new(bin())
            .arg("mcp")
            .current_dir(&self.project)
            .env("HOME", self.home.path())
            .env("XDG_DATA_HOME", self.home.path().join("data"))
            .env("XDG_CONFIG_HOME", self.home.path().join("config"))
            .env("SARA_NO_TELEMETRY", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sara mcp");
        let stdin = child.stdin.take().expect("stdin");
        let reader = BufReader::new(child.stdout.take().expect("stdout"));
        let mut mcp = Mcp {
            child,
            stdin,
            reader,
            next_id: 1,
        };
        mcp.initialize();
        mcp
    }
}

/// Path to the `sara` binary built for this test run.
fn bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin("sara")
}

/// A minimal MCP stdio client: writes newline-delimited JSON-RPC requests and
/// reads the matching responses.
pub struct Mcp {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
}

impl Mcp {
    fn send(&mut self, msg: &Value) {
        let line = serde_json::to_string(msg).expect("serialize");
        self.stdin.write_all(line.as_bytes()).expect("write");
        self.stdin.write_all(b"\n").expect("write nl");
        self.stdin.flush().expect("flush");
    }

    /// Read lines until one carries the given response id.
    fn read_response(&mut self, id: u64) -> Value {
        loop {
            let mut line = String::new();
            let n = self.reader.read_line(&mut line).expect("read");
            assert!(n > 0, "mcp server closed the stream before id {id}");
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let v: Value = serde_json::from_str(trimmed).expect("json-rpc line");
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                return v;
            }
        }
    }

    fn initialize(&mut self) {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-06-18",
                "capabilities": {},
                "clientInfo": {"name": "contract-tests", "version": "0"}
            }
        }));
        let _ = self.read_response(id);
        self.send(&json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        }));
    }

    /// Call a tool and return the full JSON-RPC response envelope.
    pub fn call(&mut self, name: &str, arguments: Value) -> Value {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": arguments}
        }));
        self.read_response(id)
    }

    /// Call a tool and return the parsed JSON payload the tool produced
    /// (the text content of the response envelope).
    pub fn call_result(&mut self, name: &str, arguments: Value) -> Value {
        let env = self.call(name, arguments);
        let text = env["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_else(|| panic!("{name}: no text content in {env}"));
        serde_json::from_str(text).expect("tool payload is json")
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        // Killing the child lets the server exit; then reap it.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Keys whose values are non-deterministic (random ids, wall-clock times,
/// age-derived scores, git commits). Replaced with a stable placeholder so
/// snapshots pin structure, not run-specific noise.
const VOLATILE: &[&str] = &[
    "uuid",
    "entry",
    "modified",
    "created",
    "updated",
    "validated_at",
    "validated_commit",
    "head",
    "urgency",
    "step_id",
];

/// Recursively replace volatile field values with `"[redacted]"`.
pub fn redact(mut value: Value) -> Value {
    scrub(&mut value);
    value
}

fn scrub(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (k, v) in map.iter_mut() {
                if VOLATILE.contains(&k.as_str()) && !v.is_null() {
                    *v = Value::String("[redacted]".into());
                } else {
                    scrub(v);
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(scrub),
        _ => {}
    }
}
