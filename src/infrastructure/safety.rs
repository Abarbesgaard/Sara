//! Shared safety guardrails for all memory-ingestion paths.
//!
//! These checks apply to **every** path that writes to the `items` store —
//! explicit `sara learn`, auto-synthesis on `done`, and event-logging hooks.
//! Centralising here ensures new ingestion paths can't skip them.

use anyhow::Result;

/// Maximum body length in Unicode characters. Above this we assume the caller
/// is pasting a raw conversation rather than a distilled paragraph.
pub const SIZE_LIMIT_CHARS: usize = 2000;

/// Run the full guardrail suite: size check then secret detection.
///
/// Returns `Ok(())` when the content passes. Returns an `Err` with a
/// human-readable message that explains what to fix. The caller is responsible
/// for surfacing `--force` semantics (i.e. skip this call when force=true).
pub fn check_memory_body(text: &str) -> Result<()> {
    check_size(text)?;
    check_secrets(text)?;
    Ok(())
}

/// Body length guard. Rejects texts longer than [`SIZE_LIMIT_CHARS`].
pub fn check_size(text: &str) -> Result<()> {
    let len = text.chars().count();
    if len > SIZE_LIMIT_CHARS {
        anyhow::bail!(
            "Memory body is {len} characters — that looks like a raw conversation paste, \
not a distilled paragraph ({SIZE_LIMIT_CHARS} char limit).\n\
Distill the key insight into one short paragraph first, then run sara learn again.\n\
To save anyway (not recommended): add --force."
        );
    }
    Ok(())
}

/// Secret-pattern guard. Rejects text that contains recognisable credential
/// patterns (key=value assignments, AWS AKIA keys, high-entropy tokens).
pub fn check_secrets(text: &str) -> Result<()> {
    if let Some(reason) = detect_secret(text) {
        anyhow::bail!(
            "Memory body may contain a secret ({reason}).\n\
Remove the sensitive value before saving, or add --force to skip this check."
        );
    }
    Ok(())
}

/// Returns `Some(reason)` when a secret pattern is detected, `None` otherwise.
/// This is the low-level predicate — callers that want a `Result` should use
/// [`check_secrets`] instead.
pub fn detect_secret(text: &str) -> Option<&'static str> {
    let kv_patterns = [
        "api_key", "apikey", "api-key", "secret", "password", "passwd", "token",
        "private_key", "privatekey", "client_secret", "access_key", "accesskey",
        "auth_token", "bearer", "authorization",
    ];
    let lower = text.to_lowercase();
    for kw in &kv_patterns {
        for sep in ["=", ": ", ":\"", "=\""] {
            if lower.contains(&format!("{kw}{sep}")) {
                return Some("key=value assignment pattern");
            }
        }
    }
    if contains_aws_key(text) {
        return Some("AWS-style access key (AKIA…)");
    }
    if contains_high_entropy_token(text) {
        return Some("high-entropy token");
    }
    None
}

fn contains_aws_key(text: &str) -> bool {
    let bytes = text.as_bytes();
    for i in 0..bytes.len().saturating_sub(19) {
        if bytes[i..i + 4] == *b"AKIA" {
            let rest = &bytes[i + 4..i + 20];
            if rest.len() == 16 && rest.iter().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()) {
                return true;
            }
        }
    }
    false
}

fn contains_high_entropy_token(text: &str) -> bool {
    for word in text.split_whitespace() {
        let w = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
        if w.len() < 32 || !w.is_ascii() || is_uuid(w) {
            continue;
        }
        let hex_count = w.bytes().filter(|b| b.is_ascii_hexdigit()).count();
        if hex_count * 100 / w.len() > 55 && w.len() >= 32 {
            return true;
        }
    }
    false
}

fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 { return false; }
    let dashes = [8usize, 13, 18, 23];
    for (i, &byte) in b.iter().enumerate() {
        if dashes.contains(&i) {
            if byte != b'-' { return false; }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_check_passes_under_threshold() {
        assert!(check_size("short text").is_ok());
        assert!(check_size(&"x".repeat(SIZE_LIMIT_CHARS)).is_ok());
    }

    #[test]
    fn size_check_fails_over_threshold() {
        assert!(check_size(&"x".repeat(SIZE_LIMIT_CHARS + 1)).is_err());
    }

    #[test]
    fn secret_kv_detected() {
        assert!(detect_secret("api_key=abc123").is_some());
        assert!(detect_secret("password: hunter2").is_some());
        assert!(detect_secret("token=\"ghp_something\"").is_some());
    }

    #[test]
    fn secret_aws_key_detected() {
        assert!(detect_secret("AKIAIOSFODNN7EXAMPLE").is_some());
    }

    #[test]
    fn secret_high_entropy_detected() {
        // 40-char hex string — well above threshold
        assert!(detect_secret("a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2").is_some());
    }

    #[test]
    fn uuid_not_flagged_as_high_entropy() {
        // UUIDs look hex-heavy but should NOT trigger the entropy check
        assert!(detect_secret("ref: 831c4d6e-8fcc-4ca5-b516-21bc8236acb0").is_none());
    }

    #[test]
    fn clean_paragraph_passes() {
        assert!(detect_secret("Sara uses rusqlite for all DB access. The connection is created once in main.").is_none());
    }

    #[test]
    fn check_memory_body_combines_both() {
        assert!(check_memory_body("short clean text").is_ok());
        assert!(check_memory_body(&"x".repeat(SIZE_LIMIT_CHARS + 1)).is_err());
        assert!(check_memory_body("api_key=secret123").is_err());
    }
}
