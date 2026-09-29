pub struct AcceptanceGate {
    pub total: usize,
    pub ran: usize,
    pub passed: usize,
    pub cached: usize,
    pub failures: Vec<String>,
    pub missing_verify: Vec<String>,
    pub transcript: Vec<GateRun>,
}

impl AcceptanceGate {
    pub fn is_green(&self) -> bool {
        self.total > 0 && self.failures.is_empty() && self.missing_verify.is_empty()
    }

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

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GateOutput {
    Stream,
    Capture,
}

impl GateOutput {
    pub(super) fn say(self, line: impl std::fmt::Display) {
        if self == GateOutput::Stream {
            println!("{line}");
        }
    }
}

#[derive(Debug, Clone)]
pub struct GateRun {
    pub text: String,
    pub cmd: String,
    pub passed: bool,
    pub exit_code: Option<i32>,
    pub output: String,
}
