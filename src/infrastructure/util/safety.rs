//! Adapts the domain's `MemoryBody` rules for the CLI: reads the size limit from
//! the environment and phrases violations with the flags that bypass them.

use anyhow::Result;

use crate::domain::{DomainError, MemoryBody};

pub const SIZE_LIMIT_CHARS: usize = MemoryBody::DEFAULT_SIZE_LIMIT;

pub const SIZE_LIMIT_ENV: &str = "SARA_MEMORY_CHAR_LIMIT";

pub fn size_limit() -> usize {
    std::env::var(SIZE_LIMIT_ENV)
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(SIZE_LIMIT_CHARS)
}

pub fn check_memory_body(text: &str) -> Result<()> {
    MemoryBody::new(text, size_limit()).map_err(explain)?;
    Ok(())
}

pub fn check_size(text: &str) -> Result<()> {
    MemoryBody::check_size(text, size_limit()).map_err(explain)
}

pub fn check_secrets(text: &str) -> Result<()> {
    MemoryBody::check_secrets(text).map_err(explain)
}

fn explain(e: DomainError) -> anyhow::Error {
    match e {
        DomainError::MemoryBodyTooLarge { len, limit } => anyhow::anyhow!(
            "Memory body is {len} characters — that looks like a raw conversation paste, \
not a distilled paragraph ({limit} char limit).\n\
Distill the key insight into one short paragraph first, then run sara learn again.\n\
Raise the limit with {SIZE_LIMIT_ENV}=<n>, or save anyway (not recommended) with --force."
        ),
        DomainError::MemoryBodyContainsSecret { reason } => anyhow::anyhow!(
            "Memory body may contain a secret ({reason}).\n\
Remove the sensitive value before saving, or add --force to skip this check."
        ),
        other => other.into(),
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/infrastructure/safety.rs"]
mod tests;
