use ignore::WalkBuilder;
use std::path::Path;

const MAX_FILES: usize = 2000;
const MAX_FILE_SIZE: u64 = 1_000_000;

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
        if let Ok(meta) = path.metadata()
            && meta.len() > MAX_FILE_SIZE
        {
            continue;
        }
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
                continue;
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
#[path = "../../tests/unit/infrastructure/files.rs"]
mod tests;
