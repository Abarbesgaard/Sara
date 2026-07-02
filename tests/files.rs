//! Integration tests for `sara_tasks::infrastructure::files`.
//! Moved out of an inline mod tests block in src/infrastructure/files.rs.

use sara_tasks::infrastructure::files::*;
use std::fs;
use std::path::{Path, PathBuf};

fn unique_root(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let base = std::env::temp_dir().join(format!(
        "sara-files-test-{name}-{}-{nanos}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&base);
    fs::create_dir_all(&base).unwrap();
    base
}

fn write(root: &Path, rel: &str) {
    let p = root.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, b"x").unwrap();
}

#[test]
fn collect_entries_excludes_dot_git_but_keeps_other_dotdirs() {
    let root = unique_root("entries-git");
    write(&root, "src/main.rs");
    write(&root, "README.md");
    write(&root, ".git/config");
    write(&root, ".git/objects/ab/cdef0123");
    write(&root, ".git/hooks/pre-commit");
    write(&root, ".github/workflows/ci.yml");

    // Normalize separators so assertions hold on Windows (backslash) too.
    let entries: Vec<String> = collect_project_entries(&root)
        .into_iter()
        .map(|e| e.replace('\\', "/"))
        .collect();

    assert!(
        entries.iter().all(|e| !e.starts_with(".git/")),
        "entries leaked .git contents: {entries:?}"
    );
    assert!(
        entries.iter().any(|e| e == "src/main.rs"),
        "regular files should be present: {entries:?}"
    );
    assert!(
        entries.iter().any(|e| e == ".github/workflows/ci.yml"),
        "only .git is pruned, other dotdirs stay: {entries:?}"
    );

    let _ = fs::remove_dir_all(&root);
}

#[test]
fn collect_files_excludes_dot_git() {
    let root = unique_root("files-git");
    write(&root, "src/main.rs");
    write(&root, ".git/config");
    write(&root, ".git/objects/ab/cdef0123");

    // Normalize separators so assertions hold on Windows (backslash) too.
    let files: Vec<String> = collect_project_files(&root)
        .into_iter()
        .map(|f| f.replace('\\', "/"))
        .collect();

    assert!(
        files.iter().all(|f| !f.starts_with(".git/")),
        "files leaked .git contents: {files:?}"
    );
    assert!(
        files.iter().any(|f| f == "src/main.rs"),
        "regular files should be present: {files:?}"
    );

    let _ = fs::remove_dir_all(&root);
}
