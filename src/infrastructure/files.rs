use ignore::WalkBuilder;
use std::path::Path;

const MAX_FILES: usize = 2000;
const MAX_FILE_SIZE: u64 = 1_000_000; // 1 MB

/// Walk a project root (gitignore-aware) and collect relative file paths.
pub fn collect_project_files(root: &Path) -> Vec<String> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .follow_links(false)
        .add_custom_ignore_filename(".saraignore")
        .add_custom_ignore_filename(".tkignore")
        .filter_entry(|e| e.file_name() != ".git")
        .build();

    let mut files = Vec::new();
    for entry in walker.flatten() {
        if files.len() >= MAX_FILES {
            break;
        }
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Skip large files
        if let Ok(meta) = path.metadata()
            && meta.len() > MAX_FILE_SIZE
        {
            continue;
        }
        // Skip binary-ish extensions
        if let Some(ext) = path.extension().and_then(|e| e.to_str())
            && matches!(
                ext.to_lowercase().as_str(),
                "png"
                    | "jpg"
                    | "jpeg"
                    | "gif"
                    | "webp"
                    | "ico"
                    | "svg"
                    | "woff"
                    | "woff2"
                    | "ttf"
                    | "eot"
                    | "mp4"
                    | "mp3"
                    | "wav"
                    | "zip"
                    | "tar"
                    | "gz"
                    | "pdf"
                    | "lock"
            )
        {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(root) {
            files.push(rel.to_string_lossy().to_string());
        }
    }
    files
}

/// Walk a project root and collect both files and directories as relative
/// paths, for the manual file/folder picker. Directories get a trailing `/`
/// so they're distinguishable when displayed or stored.
pub fn collect_project_entries(root: &Path) -> Vec<String> {
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .follow_links(false)
        .add_custom_ignore_filename(".saraignore")
        .add_custom_ignore_filename(".tkignore")
        .filter_entry(|e| e.file_name() != ".git")
        .build();

    let mut entries = Vec::new();
    for entry in walker.flatten() {
        if entries.len() >= MAX_FILES {
            break;
        }
        let path = entry.path();
        let is_dir = path.is_dir();
        if !is_dir && !path.is_file() {
            continue;
        }
        if let Ok(rel) = path.strip_prefix(root) {
            let mut s = rel.to_string_lossy().to_string();
            if s.is_empty() {
                continue; // the root itself
            }
            if is_dir {
                s.push('/');
            }
            entries.push(s);
        }
    }
    entries.sort();
    entries
}

/// Build a concise file-tree summary string (max ~100 lines).
pub fn build_tree_summary(root: &Path, files: &[String]) -> String {
    let root_name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");
    let mut lines = vec![format!("{root_name}/")];
    for f in files.iter().take(80) {
        lines.push(format!("  {f}"));
    }
    if files.len() > 80 {
        lines.push(format!("  ... ({} more files)", files.len() - 80));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

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
}
