use anyhow::Result;

use crate::config::Config;
use crate::memory;

pub fn run(cfg: &Config, words: &[String]) -> Result<()> {
    let text = words.join(" ").trim().to_string();
    if text.is_empty() {
        anyhow::bail!("Usage: sara remember \"something to store long-term\"");
    }
    memory::remember(cfg, &text)
}
