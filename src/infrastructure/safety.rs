use anyhow::Result;

pub const SIZE_LIMIT_CHARS: usize = 4000;

pub const SIZE_LIMIT_ENV: &str = "SARA_MEMORY_CHAR_LIMIT";

pub fn size_limit() -> usize {
    std::env::var(SIZE_LIMIT_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(SIZE_LIMIT_CHARS)
}

pub fn check_memory_body(text: &str) -> Result<()> {
    check_size(text)?;
    check_secrets(text)?;
    Ok(())
}

pub fn check_size(text: &str) -> Result<()> {
    let len = text.chars().count();
    let limit = size_limit();
    if len > limit {
        anyhow::bail!(
            "Memory body is {len} characters — that looks like a raw conversation paste, \
not a distilled paragraph ({limit} char limit).\n\
Distill the key insight into one short paragraph first, then run sara learn again.\n\
Raise the limit with {SIZE_LIMIT_ENV}=<n>, or save anyway (not recommended) with --force."
        );
    }
    Ok(())
}

pub fn check_secrets(text: &str) -> Result<()> {
    if let Some(reason) = detect_secret(text) {
        anyhow::bail!(
            "Memory body may contain a secret ({reason}).\n\
Remove the sensitive value before saving, or add --force to skip this check."
        );
    }
    Ok(())
}

pub fn detect_secret(text: &str) -> Option<&'static str> {
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
#[path = "../../tests/unit/infrastructure/safety.rs"]
mod tests;
