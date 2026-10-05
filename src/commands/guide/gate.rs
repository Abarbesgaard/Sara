use std::collections::HashMap;
use std::path::Path;

use anyhow::Result;
use rusqlite::Connection;

use super::support::{shell, working_dir};
use super::types::{AcceptanceGate, GateOutput, GateRun};
use crate::commands::shared::project_head;
use crate::infrastructure::db::{self, ChecklistItem};
use crate::infrastructure::git;

const CAPTURE_TAIL_CHARS: usize = 4000;

#[derive(Clone)]
struct Outcome {
    passed: bool,
    exit_code: Option<i32>,
    output: String,
}

pub(super) fn run_acceptance_gate(
    conn: &Connection,
    task_id_or_uuid: &str,
    mode: GateOutput,
    fresh: bool,
) -> Result<AcceptanceGate> {
    let task = db::resolve_task(conn, task_id_or_uuid)?;
    let acceptance = db::get_steps(conn, &task.uuid, db::STEP_KIND_ACCEPTANCE)?;
    let dir = working_dir(conn, &task.project);
    let commit = project_head(conn, &task.project);

    let tree_clean = !fresh
        && dir
            .as_deref()
            .and_then(|d| git::is_clean(Path::new(d)))
            .unwrap_or(false);
    let cache_ok = !fresh && tree_clean && commit.is_some();
    let commit_label = commit.as_deref().unwrap_or("?");

    let mut gate = AcceptanceGate {
        total: acceptance.len(),
        ran: 0,
        passed: 0,
        cached: 0,
        failures: Vec::new(),
        missing_verify: Vec::new(),
        transcript: Vec::new(),
    };
    let mut ran_cmds: HashMap<String, Outcome> = HashMap::new();

    for s in &acceptance {
        let Some(cmd) = &s.verify_cmd else {
            gate.missing_verify.push(s.text.clone());
            continue;
        };

        if cache_ok && s.done && s.done_commit.as_deref() == commit.as_deref() {
            gate.cached += 1;
            mode.say(format!("$ {cmd}"));
            mode.say(format!(
                "  ⏭ cached — already proven at {commit_label} (clean tree), skipped \"{}\"",
                s.text
            ));
            let cached = Outcome {
                passed: true,
                exit_code: Some(0),
                output: format!("cached: proven at {commit_label}"),
            };
            gate.record(s, cmd, cached);
            continue;
        }

        if let Some(prev) = ran_cmds.get(cmd).cloned() {
            let verdict = if prev.passed { "pass" } else { "failure" };
            gate.settle(conn, s, cmd, prev, commit.as_deref())?;
            mode.say(format!(
                "  ↻ same command already run — reusing {verdict} for \"{}\"",
                s.text
            ));
            continue;
        }

        gate.ran += 1;
        mode.say(format!("$ {cmd}"));
        let (outcome, line) = match execute(cmd, dir.as_deref(), mode) {
            Ok(o) if o.passed => (o, format!("  ✓ passed — ticked \"{}\"", s.text)),
            Ok(o) => {
                let line = format!("  ✗ exit {} — \"{}\"", o.exit_code.unwrap_or(-1), s.text);
                (o, line)
            }
            Err(e) => (
                Outcome {
                    passed: false,
                    exit_code: None,
                    output: format!("failed to run: {e}"),
                },
                format!("  ✗ failed to run ({e}) — \"{}\"", s.text),
            ),
        };
        ran_cmds.insert(cmd.clone(), outcome.clone());
        gate.settle(conn, s, cmd, outcome, commit.as_deref())?;
        mode.say(line);
    }
    Ok(gate)
}

impl AcceptanceGate {
    fn settle(
        &mut self,
        conn: &Connection,
        s: &ChecklistItem,
        cmd: &str,
        outcome: Outcome,
        commit: Option<&str>,
    ) -> Result<()> {
        if outcome.passed {
            let note = format!("verify passed: {cmd}");
            db::set_step_done(conn, s.id, true, Some(&note), commit)?;
            self.passed += 1;
        } else {
            self.failures.push(s.text.clone());
        }
        self.record(s, cmd, outcome);
        Ok(())
    }

    fn record(&mut self, s: &ChecklistItem, cmd: &str, outcome: Outcome) {
        self.transcript.push(GateRun {
            text: s.text.clone(),
            cmd: cmd.to_string(),
            passed: outcome.passed,
            exit_code: outcome.exit_code,
            output: outcome.output,
        });
    }
}

fn execute(cmd: &str, dir: Option<&str>, mode: GateOutput) -> std::io::Result<Outcome> {
    let mut command = shell(cmd, dir);
    let (status, output) = match mode {
        GateOutput::Stream => (command.status()?, String::new()),
        GateOutput::Capture => {
            let o = command.output()?;
            (o.status, tail_limited(&combined(&o), CAPTURE_TAIL_CHARS))
        }
    };
    Ok(Outcome {
        passed: status.success(),
        exit_code: status.code(),
        output,
    })
}

fn combined(o: &std::process::Output) -> String {
    let mut buf = String::from_utf8_lossy(&o.stdout).into_owned();
    let err = String::from_utf8_lossy(&o.stderr);
    if !err.is_empty() {
        if !buf.is_empty() && !buf.ends_with('\n') {
            buf.push('\n');
        }
        buf.push_str(&err);
    }
    buf
}

pub(super) fn tail_limited(s: &str, max_chars: usize) -> String {
    let total = s.chars().count();
    if total <= max_chars {
        return s.to_string();
    }
    let skip = total - max_chars;
    let start = s
        .char_indices()
        .nth(skip)
        .map(|(i, _)| i)
        .unwrap_or(s.len());
    format!("…[{skip} chars truncated]\n{}", &s[start..])
}
