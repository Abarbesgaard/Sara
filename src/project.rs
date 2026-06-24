use anyhow::Result;
use std::path::{Path, PathBuf};

/// Detect the git root for the given directory (or CWD).
/// Handles .git as either a directory (normal repo) or a file (worktree/submodule).
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    // Prefer asking git itself — handles all edge cases
    if let Ok(output) = std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .current_dir(start)
        .output()
        && output.status.success()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return Some(PathBuf::from(path));
        }
    }

    // Fallback: walk up looking for .git (dir or file)
    let mut cur = start.to_path_buf();
    loop {
        if cur.join(".git").exists() {
            return Some(cur);
        }
        if !cur.pop() {
            break;
        }
    }
    None
}

/// Extract a project name from the git root path.
pub fn project_name_from_root(root: &Path) -> String {
    root.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("inbox")
        .to_string()
}

/// Resolve the project identity (name, path) for a directory.
///
/// A git repo is its own project, named after the repo root. Otherwise the
/// directory itself is the project, named after the folder — Sara initializes
/// "inside the folder" rather than dumping into a catch-all. The legacy
/// `default_project` ("inbox") is only used as a last resort when the folder
/// has no usable name (e.g. the filesystem root).
pub fn project_identity_for_dir(dir: &Path, cfg: &crate::config::Config) -> (String, String) {
    let root = find_git_root(dir).unwrap_or_else(|| dir.to_path_buf());
    let canonical = root.canonicalize().unwrap_or(root);
    let name = canonical
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| cfg.default_project.clone());
    (name, canonical.to_string_lossy().to_string())
}

/// Parse inline Taskwarrior-style tokens from the raw add arguments.
/// Tokens are only extracted when they appear as leading or trailing words.
/// Everything between the first non-token and last non-token is the literal description.
///
/// Supported: `project:foo`, `+tag`, `pri:H`, `every:daily`
#[derive(Debug, Default)]
pub struct ParsedTokens {
    pub description: String,
    pub project: Option<String>,
    pub tags: Vec<String>,
    pub priority: Option<String>,
    pub recur: Option<String>,
}

pub fn parse_add_tokens(args: &[String]) -> ParsedTokens {
    let mut result = ParsedTokens::default();
    // Strip any --flag tokens that slipped in via trailing_var_arg before parsing
    let cleaned: Vec<&str> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(|s| s.as_str())
        .collect();
    let mut remaining: Vec<&str> = cleaned;

    // Strip leading tokens
    while let Some(&tok) = remaining.first() {
        if let Some(stripped) = tok.strip_prefix("project:") {
            result.project = Some(stripped.to_string());
            remaining.remove(0);
        } else if let Some(stripped) = tok.strip_prefix('+') {
            result.tags.push(stripped.to_string());
            remaining.remove(0);
        } else if let Some(stripped) = tok.to_lowercase().strip_prefix("pri:") {
            result.priority = Some(stripped.to_uppercase());
            remaining.remove(0);
        } else if let Some(stripped) = tok.to_lowercase().strip_prefix("every:") {
            result.recur = Some(stripped.to_string());
            remaining.remove(0);
        } else {
            break;
        }
    }

    // Strip trailing tokens
    while let Some(&tok) = remaining.last() {
        if let Some(stripped) = tok.strip_prefix("project:") {
            if result.project.is_none() {
                result.project = Some(stripped.to_string());
            }
            remaining.pop();
        } else if let Some(stripped) = tok.strip_prefix('+') {
            result.tags.push(stripped.to_string());
            remaining.pop();
        } else if let Some(stripped) = tok.to_lowercase().strip_prefix("pri:") {
            if result.priority.is_none() {
                result.priority = Some(stripped.to_uppercase());
            }
            remaining.pop();
        } else if let Some(stripped) = tok.to_lowercase().strip_prefix("every:") {
            if result.recur.is_none() {
                result.recur = Some(stripped.to_string());
            }
            remaining.pop();
        } else {
            break;
        }
    }

    result.description = remaining.join(" ");
    result
}

/// Detect the current project name and path.
/// Returns (name, Option<path_string>).
pub fn detect_current_project(
    conn: &rusqlite::Connection,
    cfg: &crate::config::Config,
) -> Result<(String, Option<String>)> {
    let cwd = std::env::current_dir()?;
    let (name, path_str) = project_identity_for_dir(&cwd, cfg);

    // Check for path collision (same name, different path)
    if let Some(existing) = crate::db::get_project(conn, &name)?
        && let Some(ref existing_path) = existing.path
        && existing_path != &path_str
    {
        eprintln!(
            "Warning: project '{}' is already registered at {}. \
                     Using existing name. Use --project to override.",
            name, existing_path
        );
    }

    crate::db::upsert_project_seen(conn, &name, Some(&path_str))?;
    Ok((name, Some(path_str)))
}
