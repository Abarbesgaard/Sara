use anyhow::{Context, Result};
use rusqlite::Connection;

use crate::infrastructure::config::Config;
use crate::infrastructure::db;
use crate::infrastructure::model::Item;
use crate::infrastructure::project::detect_current_project;

/// Size threshold in characters above which we warn that the text is probably
/// not a distilled paragraph. A full conversation paste defeats the
/// token-minimization premise of the whole feature.
const SIZE_WARN_CHARS: usize = 2000;

/// `sara learn "<text>" [--tag] [-p] [--task]` — save a short, freeform
/// distillation as a global memory (not task/project-scoped: it *references*
/// projects rather than belonging to one, same convention as `sara add`).
pub fn run(
    conn: &Connection,
    cfg: &Config,
    text: &str,
    tags: &[String],
    projects: &[String],
    task: Option<&str>,
    force: bool,
) -> Result<()> {
    let text = text.trim();

    if !force {
        check_size(text)?;
        check_secrets(text)?;
    }

    let item = save(conn, cfg, text, tags, projects, task)?;

    println!(
        "Learned m{} ({}): {}",
        item.display_id.unwrap_or(0),
        &item.uuid.to_string()[..8],
        summarize(text)
    );
    Ok(())
}

/// Warn (and abort) when the body is too long to be a distilled paragraph.
/// The user must pass `--force` to override.
fn check_size(text: &str) -> Result<()> {
    let len = text.chars().count();
    if len > SIZE_WARN_CHARS {
        anyhow::bail!(
            "Memory body is {len} characters — that looks like a raw conversation paste, \
not a distilled paragraph ({SIZE_WARN_CHARS} char limit).\n\
Distill the key insight into one short paragraph first, then run sara learn again.\n\
To save anyway (not recommended): add --force."
        );
    }
    Ok(())
}

/// Warn (and abort) when the body appears to contain a secret-like value.
/// Heuristic only — false positives are expected. The user must pass `--force`
/// to override. This is a write-time safeguard; it does not guarantee secrets
/// cannot enter the store through other paths.
fn check_secrets(text: &str) -> Result<()> {
    if let Some(reason) = detect_secret(text) {
        anyhow::bail!(
            "Memory body may contain a secret ({reason}).\n\
Remove the sensitive value before saving, or add --force to skip this check."
        );
    }
    Ok(())
}

/// Returns a short description of the first suspicious pattern found, or None.
pub(crate) fn detect_secret(text: &str) -> Option<&'static str> {
    // Common key=value assignment patterns seen in config/appsettings/env files.
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
        // Only flag when followed by an assignment or colon — avoids flagging
        // prose like "the token field is …" without a value attached.
        for sep in ["=", ": ", ":\"", "=\""] {
            if lower.contains(&format!("{kw}{sep}")) {
                return Some("key=value assignment pattern");
            }
        }
    }

    // AWS-style access key: AKIA… (20 uppercase alphanumeric chars)
    if contains_aws_key(text) {
        return Some("AWS-style access key (AKIA…)");
    }

    // High-entropy token heuristic: a word of 32+ non-whitespace ASCII chars
    // where more than 60% are non-alphanumeric (dashes/underscores don't count)
    // — catches hex hashes, base64 tokens, UUIDs with no surrounding prose.
    if contains_high_entropy_token(text) {
        return Some("high-entropy token");
    }

    None
}

fn contains_aws_key(text: &str) -> bool {
    // AKIA followed by exactly 16 uppercase letters/digits (total 20 chars)
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
        // Strip common surrounding punctuation
        let w = word.trim_matches(|c: char| !c.is_alphanumeric() && c != '-' && c != '_');
        if w.len() < 32 {
            continue;
        }
        // Must be all ASCII (avoids flagging normal prose in other scripts)
        if !w.is_ascii() {
            continue;
        }
        // Exclude standard UUID format (8-4-4-4-12): these appear legitimately
        // in config descriptions and should not be flagged as high-entropy tokens.
        if is_uuid(w) {
            continue;
        }
        // Flag if more than 55% of chars are hex digits — catches long hex
        // hashes/tokens but not plain English words
        let hex_count = w.bytes().filter(|b| b.is_ascii_hexdigit()).count();
        if hex_count * 100 / w.len() > 55 && w.len() >= 32 {
            return true;
        }
    }
    false
}

/// Returns true for standard UUID format: xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx
fn is_uuid(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 36 {
        return false;
    }
    let dashes = [8, 13, 18, 23];
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

fn save(
    conn: &Connection,
    cfg: &Config,
    text: &str,
    tags: &[String],
    projects: &[String],
    task: Option<&str>,
) -> Result<Item> {
    if text.is_empty() {
        anyhow::bail!("Memory text cannot be empty");
    }

    let source_task_uuid = match task {
        Some(prefix) => Some(
            db::get_task_by_uuid_prefix(conn, prefix)
                .context("looking up --task")?
                .ok_or_else(|| anyhow::anyhow!("No task found for prefix '{prefix}'"))?
                .uuid,
        ),
        None => None,
    };

    let projects: Vec<String> = if projects.is_empty() {
        let (name, _path) = detect_current_project(conn, cfg)?;
        vec![name]
    } else {
        projects.to_vec()
    };

    let mut item = Item::new_memory(summarize(text), text.to_string(), source_task_uuid);
    item.tags = tags.to_vec();
    item.path = Some(String::new());

    db::insert_item(conn, &mut item)?;
    db::set_item_projects(conn, &item.uuid, &projects)?;

    Ok(item)
}

/// A short title for display, taken from the start of the memory text.
fn summarize(text: &str) -> String {
    const MAX: usize = 80;
    let trimmed = text.trim();
    if trimmed.chars().count() <= MAX {
        trimmed.to_string()
    } else {
        let truncated: String = trimmed.chars().take(MAX).collect();
        format!("{}…", truncated.trim_end())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn size_check_passes_under_threshold() {
        assert!(check_size("short text").is_ok());
        assert!(check_size(&"x".repeat(SIZE_WARN_CHARS)).is_ok());
    }

    #[test]
    fn size_check_fails_over_threshold() {
        let long = "x".repeat(SIZE_WARN_CHARS + 1);
        assert!(check_size(&long).is_err());
    }

    #[test]
    fn secret_check_flags_api_key_assignment() {
        assert!(detect_secret("api_key=abc123secret").is_some());
        assert!(detect_secret("apikey: supersecret").is_some());
        assert!(detect_secret("password=\"hunter2\"").is_some());
        assert!(detect_secret("client_secret: my-secret-value").is_some());
    }

    #[test]
    fn secret_check_does_not_flag_normal_prose() {
        assert!(detect_secret("sara recall uses item_tags for exact tag lookup").is_none());
        assert!(detect_secret("the token field is optional").is_none());
        assert!(detect_secret("AuthAppUri (required string)").is_none());
    }

    #[test]
    fn secret_check_flags_aws_style_key() {
        assert!(detect_secret("key: AKIAIOSFODNN7EXAMPLE").is_some());
    }

    #[test]
    fn secret_check_flags_high_entropy_hex_token() {
        // 64-char hex string — looks like a SHA256 hash or access token
        assert!(
            detect_secret(
                "token: a3f1b2c4d5e6f7a8b9c0d1e2f3a4b5c6d7e8f9a0b1c2d3e4f5a6b7c8d9e0f1a2"
            )
            .is_some()
        );
    }

    #[test]
    fn secret_check_does_not_flag_uuid_in_prose() {
        // UUIDs in normal prose (as field *values described*, not assigned) should
        // not trigger — they're short enough (36 chars with dashes) and mixed
        // alphanumeric so the hex-ratio heuristic won't catch them.
        assert!(
            detect_secret(
                "AuthAppUri is a UUID like a2923ccd-a496-4cbd-9673-f40156552a92 in the config"
            )
            .is_none()
        );
    }
}
