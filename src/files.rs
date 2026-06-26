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
