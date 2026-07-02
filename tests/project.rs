//! Integration tests for `sara_tasks::infrastructure::project`.
//! Moved out of an inline mod tests block in src/infrastructure/project.rs.

use sara_tasks::infrastructure::project::*;
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
