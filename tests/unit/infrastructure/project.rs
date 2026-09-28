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

#[test]
fn file_link_relative_anchors_to_git_root_not_cwd() {
    // From a nested subdirectory, a repo-relative path must resolve against
    // the REPO ROOT, so learn-time storage and recall-time lookup agree no
    // matter which folder either was run from. (CWD-anchoring — the old
    // behaviour — would have produced "/repo/components/lib/server/game.ts".)
    let root = Path::new("/repo");
    let cwd = Path::new("/repo/components");
    assert_eq!(
        resolve_file_link("lib/server/game.ts", Some(root), cwd),
        "/repo/lib/server/game.ts"
    );
}

#[test]
fn file_link_absolute_passes_through() {
    assert_eq!(
        resolve_file_link("/x/y.ts", Some(Path::new("/repo")), Path::new("/repo/a")),
        "/x/y.ts"
    );
}

#[test]
fn file_link_outside_repo_falls_back_to_cwd() {
    assert_eq!(
        resolve_file_link("a/b.ts", None, Path::new("/tmp/work")),
        "/tmp/work/a/b.ts"
    );
}

#[test]
fn file_link_directory_prefix_slash_preserved() {
    assert_eq!(
        resolve_file_link("lib/", Some(Path::new("/repo")), Path::new("/repo/x")),
        "/repo/lib/"
    );
}
