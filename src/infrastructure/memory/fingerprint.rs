//! Reads anchored files from disk and judges them with the pure fingerprint
//! logic in `crate::domain::anchor_state`.

use std::path::Path;

use crate::domain::AnchorState;
use crate::domain::services::fingerprint::{encode, judge, sketch};

/// Files larger than this are not fingerprinted.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

fn read_text(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    String::from_utf8(bytes).ok()
}

/// Fingerprint the file at `path` for storage; `None` when it cannot be read as
/// text (missing, too large, binary) or has no meaningful lines.
pub fn fingerprint_file(path: &str) -> Option<Vec<u8>> {
    let s = sketch(&read_text(Path::new(path))?);
    (!s.is_empty()).then(|| encode(&s))
}

/// Judge an anchor against its stored fingerprint. A vanished file is only
/// `Missing` when its directory still exists — an anchor into a repo that is
/// not on this machine stays `Unknown` rather than raising a false alarm.
pub fn anchor_state(path: &str, fingerprint: Option<&[u8]>) -> AnchorState {
    let p = Path::new(path);
    if !p.exists() {
        return match p.parent() {
            Some(dir) if dir.is_dir() => AnchorState::Missing,
            _ => AnchorState::Unknown,
        };
    }
    let Some(fp) = fingerprint else {
        return AnchorState::Unknown;
    };
    let Some(text) = read_text(p) else {
        return AnchorState::Unknown;
    };
    judge(fp, &text)
}

#[cfg(test)]
#[path = "../../../tests/unit/infrastructure/fingerprint.rs"]
mod tests;
