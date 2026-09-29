pub fn fzf_available() -> bool {
    std::process::Command::new("fzf")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

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
