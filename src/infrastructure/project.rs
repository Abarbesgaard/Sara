use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn find_git_root(start: &Path) -> Option<PathBuf> {
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

pub fn project_name_from_root(root: &Path) -> String {
    root.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("inbox")
        .to_string()
}

pub fn resolve_file_link(path: &str, git_root: Option<&Path>, cwd: &Path) -> String {
    let is_dir_prefix = path.ends_with('/') && path != "/";
    let core = if is_dir_prefix {
        path.trim_end_matches('/')
    } else {
        path
    };
    let is_absolute = core.starts_with('/') || Path::new(core).is_absolute();
    let resolved = if is_absolute {
        PathBuf::from(core)
    } else {
        git_root.unwrap_or(cwd).join(core)
    };
    let mut out = resolved.to_string_lossy().replace('\\', "/");
    if is_dir_prefix {
        out.push('/');
    }
    out
}

pub fn resolve_file_link_here(path: &str) -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let git_root = find_git_root(&cwd);
    resolve_file_link(path, git_root.as_deref(), &cwd)
}

pub fn project_identity_for_dir(
    dir: &Path,
    cfg: &crate::infrastructure::config::Config,
) -> (String, String) {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let git_root = find_git_root(&dir).map(|r| r.canonicalize().unwrap_or(r));
    let home = home_dir();
    let root = project_root_for(&dir, git_root.as_deref(), home.as_deref());
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .filter(|n| !n.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| cfg.default_project.clone());
    (name, root.to_string_lossy().to_string())
}

fn project_root_for(dir: &Path, git_root: Option<&Path>, home: Option<&Path>) -> PathBuf {
    match git_root {
        Some(root) if home.is_none_or(|h| !h.starts_with(root)) => root.to_path_buf(),
        _ => dir.to_path_buf(),
    }
}

fn home_dir() -> Option<PathBuf> {
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    Some(home.canonicalize().unwrap_or(home))
}

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
    let cleaned: Vec<&str> = args
        .iter()
        .filter(|a| !a.starts_with("--"))
        .map(|s| s.as_str())
        .collect();
    let mut remaining: Vec<&str> = cleaned;

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

pub fn detect_current_project(
    conn: &rusqlite::Connection,
    cfg: &crate::infrastructure::config::Config,
) -> Result<(String, Option<String>)> {
    let cwd = std::env::current_dir()?;
    let (derived_name, path_str) = project_identity_for_dir(&cwd, cfg);

    if let Some(existing) = crate::infrastructure::db::get_project_by_path(conn, &path_str)? {
        crate::infrastructure::db::upsert_project_seen(conn, &existing.name, Some(&path_str))?;
        return Ok((existing.name, Some(path_str)));
    }

    if let Some(existing) = crate::infrastructure::db::get_project(conn, &derived_name)?
        && let Some(ref existing_path) = existing.path
        && existing_path != &path_str
    {
        eprintln!(
            "Warning: project '{}' is already registered at {}. \
                     Using existing name. Use --project to override.",
            derived_name, existing_path
        );
    }

    crate::infrastructure::db::upsert_project_seen(conn, &derived_name, Some(&path_str))?;
    Ok((derived_name, Some(path_str)))
}

#[cfg(test)]
#[path = "../../tests/unit/infrastructure/project.rs"]
mod tests;
