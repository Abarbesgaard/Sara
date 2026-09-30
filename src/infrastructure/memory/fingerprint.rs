//! Content fingerprints for file-anchored memories. When a memory is anchored
//! to a file, a bottom-k sketch of the file's line hashes is stored; at recall
//! the share of sketched lines no longer present in the file is its drift. This
//! is independent of git history, so the commit (or squash-merge) that lands
//! the very work a memory describes does not count as drift.

use std::collections::HashSet;
use std::path::Path;

/// How many line hashes a sketch keeps (the k smallest distinct hashes).
pub const SKETCH_SIZE: usize = 256;
/// Drift at or above this share of vanished lines flags an anchor as stale.
pub const STALE_DRIFT: f64 = 0.4;
/// Files larger than this are not fingerprinted.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MIN_LINE_CHARS: usize = 3;

#[derive(Clone, Debug, PartialEq)]
pub enum AnchorState {
    Fresh,
    Drifted(f64),
    Missing,
    Unknown,
}

impl AnchorState {
    pub fn is_stale(&self) -> bool {
        matches!(self, AnchorState::Missing | AnchorState::Drifted(_))
    }
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn line_hashes(text: &str) -> HashSet<u64> {
    text.lines()
        .map(str::trim)
        .filter(|l| l.chars().count() >= MIN_LINE_CHARS)
        .map(|l| fnv1a(l.as_bytes()))
        .collect()
}

pub fn sketch(text: &str) -> Vec<u64> {
    let mut hashes: Vec<u64> = line_hashes(text).into_iter().collect();
    hashes.sort_unstable();
    hashes.truncate(SKETCH_SIZE);
    hashes
}

pub fn encode(sketch: &[u64]) -> Vec<u8> {
    sketch.iter().flat_map(|h| h.to_le_bytes()).collect()
}

pub fn decode(bytes: &[u8]) -> Vec<u64> {
    bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|c| u64::from_le_bytes(*c))
        .collect()
}

/// Share (0.0–1.0) of the sketched lines that no longer appear in `current`.
pub fn drift(sketch: &[u64], current: &str) -> f64 {
    if sketch.is_empty() {
        return 0.0;
    }
    let now = line_hashes(current);
    let gone = sketch.iter().filter(|h| !now.contains(h)).count();
    gone as f64 / sketch.len() as f64
}

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
    let d = drift(&decode(fp), &text);
    if d >= STALE_DRIFT {
        AnchorState::Drifted(d)
    } else {
        AnchorState::Fresh
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/infrastructure/fingerprint.rs"]
mod tests;
