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
///
/// `$HOME` (and anything above it) is never treated as a project root: a
/// dotfiles repo living at `$HOME` would otherwise capture every non-git
/// subfolder (e.g. `~/workspace`) as a project named after the home folder.
pub fn project_identity_for_dir(dir: &Path, cfg: &crate::config::Config) -> (String, String) {
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

/// Pick the project root for `dir`: the detected `git_root`, unless that root is
/// `$HOME` or an ancestor of it (too broad to be a project), in which case fall
/// back to `dir` itself. All inputs are assumed already canonicalized.
fn project_root_for(dir: &Path, git_root: Option<&Path>, home: Option<&Path>) -> PathBuf {
    match git_root {
        Some(root) if home.is_none_or(|h| !h.starts_with(root)) => root.to_path_buf(),
        _ => dir.to_path_buf(),
    }
}

/// The user's home directory, canonicalized when possible.
fn home_dir() -> Option<PathBuf> {
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    Some(home.canonicalize().unwrap_or(home))
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
///
/// Path-aware: if a project profile is registered at the current canonical path,
/// its stored name wins. This honors `sara init --name X`, whose chosen name
/// need not match the folder basename. Only when no profile is registered for
/// this path do we fall back to the name derived from the folder.
pub fn detect_current_project(
    conn: &rusqlite::Connection,
    cfg: &crate::config::Config,
) -> Result<(String, Option<String>)> {
    let cwd = std::env::current_dir()?;
    let (derived_name, path_str) = project_identity_for_dir(&cwd, cfg);

    // Honor a project registered at this exact path, whatever it is named.
    if let Some(existing) = crate::db::get_project_by_path(conn, &path_str)? {
        crate::db::upsert_project_seen(conn, &existing.name, Some(&path_str))?;
        return Ok((existing.name, Some(path_str)));
    }

    // No profile at this path — fall back to the derived name, warning if that
    // name is already registered at a *different* path (an ambiguous collision).
    if let Some(existing) = crate::db::get_project(conn, &derived_name)?
        && let Some(ref existing_path) = existing.path
        && existing_path != &path_str
    {
        eprintln!(
            "Warning: project '{}' is already registered at {}. \
                     Using existing name. Use --project to override.",
            derived_name, existing_path
        );
    }

    crate::db::upsert_project_seen(conn, &derived_name, Some(&path_str))?;
    Ok((derived_name, Some(path_str)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn home_dotfiles_repo_does_not_capture_subfolder() {
        // $HOME is itself a git repo (dotfiles); a non-git subfolder must
        // resolve to the subfolder, not to $HOME.
        let home = Path::new("/home/u");
        let dir = Path::new("/home/u/workspace");
        assert_eq!(
            project_root_for(dir, Some(home), Some(home)),
            PathBuf::from("/home/u/workspace")
        );
    }

    #[test]
    fn real_repo_under_home_is_used_as_root() {
        let home = Path::new("/home/u");
        let repo = Path::new("/home/u/projects/myrepo");
        let dir = Path::new("/home/u/projects/myrepo/src");
        assert_eq!(
            project_root_for(dir, Some(repo), Some(home)),
            PathBuf::from("/home/u/projects/myrepo")
        );
    }

    #[test]
    fn no_git_root_falls_back_to_dir() {
        let dir = Path::new("/home/u/workspace");
        assert_eq!(
            project_root_for(dir, None, Some(Path::new("/home/u"))),
            PathBuf::from("/home/u/workspace")
        );
    }

    #[test]
    fn git_root_above_home_is_rejected() {
        // A repo at the filesystem root (or any ancestor of $HOME) is too broad.
        let dir = Path::new("/home/u/workspace");
        assert_eq!(
            project_root_for(dir, Some(Path::new("/")), Some(Path::new("/home/u"))),
            PathBuf::from("/home/u/workspace")
        );
    }

    #[test]
    fn git_root_equal_to_dir_is_used() {
        let home = Path::new("/home/u");
        let dir = Path::new("/home/u/projects/myrepo");
        assert_eq!(
            project_root_for(dir, Some(dir), Some(home)),
            PathBuf::from("/home/u/projects/myrepo")
        );
    }
}
