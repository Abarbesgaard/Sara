//! `fzf` subprocess integration for the review form's file picker.
//!
//! These are pure subprocess helpers with no coupling to form state; they live
//! here rather than in `review_form` so the form module stays about the form.

/// Whether the `fzf` binary is on PATH.
pub fn fzf_available() -> bool {
    std::process::Command::new("fzf")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Run fzf as a multi-select picker over `candidates`, pre-filling `query`.
/// Returns the chosen paths, or None if fzf was cancelled or failed.
pub fn run_fzf(candidates: &[String], query: &str) -> Option<Vec<String>> {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = Command::new("fzf")
        .args([
            "--multi", "--prompt", "files> ", "--height", "100%", "--border",
        ])
        .arg("--query")
        .arg(query)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;

    if let Some(stdin) = child.stdin.as_mut() {
        for c in candidates {
            let _ = writeln!(stdin, "{c}");
        }
    }

    let output = child.wait_with_output().ok()?;
    // fzf exits 130 when the user aborts (Esc/Ctrl-C); keep selection unchanged.
    if !output.status.success() {
        return None;
    }
    let selected: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Some(selected)
}
