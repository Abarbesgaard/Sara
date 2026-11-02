use crate::domain::error::DomainError;

/// The text of a memory. A memory is a distilled insight, so it is short, and it
/// is stored and surfaced to agents, so it must never contain a secret.
#[derive(Debug, Clone, PartialEq)]
pub struct MemoryBody(String);

impl MemoryBody {
    pub const DEFAULT_SIZE_LIMIT: usize = 4000;

    pub fn new(text: &str, size_limit: usize) -> Result<Self, DomainError> {
        Self::check_size(text, size_limit)?;
        Self::check_secrets(text)?;
        Ok(Self(text.to_string()))
    }

    pub fn check_size(text: &str, size_limit: usize) -> Result<(), DomainError> {
        let len = text.chars().count();
        if len > size_limit {
            return Err(DomainError::MemoryBodyTooLarge {
                len,
                limit: size_limit,
            });
        }
        Ok(())
    }

    pub fn check_secrets(text: &str) -> Result<(), DomainError> {
        match Self::detect_secret(text) {
            Some(reason) => Err(DomainError::MemoryBodyContainsSecret { reason }),
            None => Ok(()),
        }
    }

    /// Why `text` looks like it contains a secret, if it does.
    pub fn detect_secret(text: &str) -> Option<&'static str> {
        detect_secret(text)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

fn detect_secret(text: &str) -> Option<&'static str> {
    let kv_patterns = [
        "api_key",
        "apikey",
        "api-key",
        "secret",
        "password",
        "passwd",
        "token",
        "private_key",
        "privatekey",
        "client_secret",
        "access_key",
        "accesskey",
        "auth_token",
        "bearer",
        "authorization",
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
            if rest.len() == 16
                && rest
                    .iter()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
            {
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
    if b.len() != 36 {
        return false;
    }
    let dashes = [8usize, 13, 18, 23];
    for (i, &byte) in b.iter().enumerate() {
        if dashes.contains(&i) {
            if byte != b'-' {
                return false;
            }
        } else if !byte.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

#[cfg(test)]
#[path = "../../../tests/unit/domain/value_objects/memory_body.rs"]
mod tests;
