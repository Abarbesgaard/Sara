#![allow(dead_code)]

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};

use serde_json::{Value, json};
use tempfile::TempDir;

pub struct Sara {
    home: TempDir,
    project: PathBuf,
}

impl Sara {
    pub fn new() -> Self {
        let home = tempfile::tempdir().expect("tempdir");
        let project = home.path().join("project");
        std::fs::create_dir_all(&project).expect("mkdir project");
        let s = Sara { home, project };
        s.run(&["init", "--name", "Demo", "-y"]);
        s
    }

    pub fn project(&self) -> &Path {
        &self.project
    }

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

    pub fn json(&self, args: &[&str]) -> Value {
        serde_json::from_str(&self.run(args)).expect("stdout is valid json")
    }

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

fn bin() -> PathBuf {
    assert_cmd::cargo::cargo_bin("sara")
}

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
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

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
