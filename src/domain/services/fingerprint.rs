//! Content fingerprints for file-anchored memories. When a memory is anchored
//! to a file, a bottom-k sketch of the file's line hashes is stored; at recall
//! the share of sketched lines no longer present in the file is its drift. This
//! is independent of git history, so the commit (or squash-merge) that lands
//! the very work a memory describes does not count as drift.

use std::collections::HashSet;

use crate::domain::value_objects::AnchorState;

/// How many line hashes a sketch keeps (the k smallest distinct hashes).
pub const SKETCH_SIZE: usize = 256;
/// Drift at or above this share of vanished lines flags an anchor as stale.
pub const STALE_DRIFT: f64 = 0.4;
const MIN_LINE_CHARS: usize = 3;

/// Judge the current `text` of an anchored file against its stored fingerprint.
pub fn judge(fingerprint: &[u8], text: &str) -> AnchorState {
    let d = drift(&decode(fingerprint), text);
    if d >= STALE_DRIFT {
        AnchorState::Drifted(d)
    } else {
        AnchorState::Fresh
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

#[cfg(test)]
#[path = "../../../tests/unit/domain/services/fingerprint.rs"]
mod tests;
