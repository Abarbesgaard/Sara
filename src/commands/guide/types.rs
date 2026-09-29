/// Outcome of running a task's acceptance criteria as a pass/fail gate.
pub struct AcceptanceGate {
    /// Total acceptance criteria on the task.
    pub total: usize,
    /// Criteria whose stored `verify_cmd` was executed.
    pub ran: usize,
    /// Criteria that ran and exited 0 (ticked as a side effect).
    pub passed: usize,
    /// Criteria proven green from cache — already `done` at the current HEAD with
    /// a clean tree — so their verify command was skipped this run.
    pub cached: usize,
    /// Text of criteria that ran but exited non-zero.
    pub failures: Vec<String>,
    /// Text of criteria that carry NO `verify_cmd` — unprovable, so they block.
    pub missing_verify: Vec<String>,
    /// Per-command record of the run. Populated in both modes; `output` is only
    /// filled under [`GateOutput::Capture`].
    pub transcript: Vec<GateRun>,
}

impl AcceptanceGate {
    /// The gate is green only when there is at least one acceptance criterion,
    /// every one carries a verify command, and every command passed. "No
    /// acceptance criteria" is a red gate: a task with no definition of done
    /// cannot be proven complete.
    pub fn is_green(&self) -> bool {
        self.total > 0 && self.failures.is_empty() && self.missing_verify.is_empty()
    }

    /// A human/agent-readable reason the gate is red.
    pub fn reason(&self) -> String {
        if self.total == 0 {
            return "no acceptance criteria to prove — add one with `sara check <id> \"…\" --kind acceptance --verify \"<cmd>\"`".to_string();
        }
        let mut parts = Vec::new();
        if !self.missing_verify.is_empty() {
            parts.push(format!(
                "{} acceptance criterion/criteria have no verify command: {}",
                self.missing_verify.len(),
                self.missing_verify.join("; ")
            ));
        }
        if !self.failures.is_empty() {
            parts.push(format!(
                "{} acceptance verify command(s) failed: {}",
                self.failures.len(),
                self.failures.join("; ")
            ));
        }
        parts.join(" | ")
    }

    /// The captured output of every command that failed, for callers that cannot
    /// print a live transcript. Empty when nothing was captured.
    pub fn failure_detail(&self) -> String {
        let mut out = String::new();
        for r in self.transcript.iter().filter(|r| !r.passed) {
            if r.output.trim().is_empty() {
                continue;
            }
            let code = r
                .exit_code
                .map(|c| c.to_string())
                .unwrap_or_else(|| "n/a".into());
            out.push_str(&format!("\n$ {} (exit {})\n{}", r.cmd, code, r.output));
        }
        out
    }
}

/// How the acceptance gate emits the output of the commands it runs.
///
/// This distinction is load-bearing for the MCP server: its stdio transport
/// carries JSON-RPC only, so neither our progress lines nor a verify command's
/// own output may reach stdout. `Capture` keeps the whole run off stdout and
/// records it in [`AcceptanceGate::transcript`] instead.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateOutput {
    /// CLI: announce each command and let it stream to the terminal live.
    Stream,
    /// MCP (and any other structured caller): capture output, print nothing.
    Capture,
}

impl GateOutput {
    /// Emit a progress line — but only when streaming to a human on the CLI.
    ///
    /// Every gate print must go through here. In `Capture` mode the caller owns
    /// stdout as a JSON-RPC transport, where a single stray line desynchronises
    /// the stream and crashes conformant clients. Funnelling all output through
    /// one mode-aware method makes that corruption impossible to reintroduce by
    /// accident; `tests/architecture.rs` pins the rule by asserting the gate
    /// contains no bare `println!` at all.
    pub(super) fn say(self, line: impl std::fmt::Display) {
        if self == GateOutput::Stream {
            println!("{line}");
        }
    }
}

/// One executed acceptance criterion, recorded so a caller that cannot print
/// (the MCP tool) can still report *why* the gate went red.
#[derive(Debug, Clone)]
pub struct GateRun {
    /// The criterion's text.
    pub text: String,
    /// The command that was run.
    pub cmd: String,
    /// Whether it exited 0.
    pub passed: bool,
    /// Exit code, or `None` if the command could not be spawned at all.
    pub exit_code: Option<i32>,
    /// Combined stdout+stderr. Empty under [`GateOutput::Stream`], where the
    /// child wrote straight to the terminal.
    pub output: String,
}
