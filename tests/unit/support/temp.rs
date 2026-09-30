use tempfile::TempDir;

pub fn temp_dir() -> TempDir {
    tempfile::tempdir().unwrap()
}
