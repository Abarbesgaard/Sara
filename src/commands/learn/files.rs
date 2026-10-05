use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result};

use crate::commands::shared::resolve_files;
use crate::infrastructure::project::find_git_root;

pub(super) fn collect_files(explicit: &[String], auto_files: bool) -> Result<Vec<String>> {
    let mut paths: Vec<String> = resolve_files(explicit);

    if auto_files {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        match find_git_root(&cwd) {
            Some(root) => paths.extend(git_diff_files(&root)?),
            None if explicit.is_empty() => anyhow::bail!(
                "No git root found — cannot use --auto-files outside a git repository.\n\
                 Pass --file <path> explicitly instead."
            ),
            None => {}
        }
    }

    let mut seen = HashSet::new();
    paths.retain(|p| seen.insert(p.clone()));
    Ok(paths)
}

fn git_diff_files(git_root: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["diff", "--name-only", "HEAD"])
        .current_dir(git_root)
        .output()
        .context("running git diff --name-only HEAD")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    Ok(stdout
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(|l| git_root.join(l).to_string_lossy().into_owned())
        .collect())
}
