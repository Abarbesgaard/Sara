use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
    pub api_key: Option<String>,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for LlmConfig {
    fn default() -> Self {
        LlmConfig {
            provider: "ollama".to_string(),
            model: "qwen2.5".to_string(),
            base_url: None,
            api_key: None,
            timeout_secs: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UrgencyConfig {
    pub priority_h: f64,
    pub priority_m: f64,
    pub priority_l: f64,
    pub due: f64,
    pub blocking: f64,
    pub blocked: f64,
    pub active: f64,
    pub has_tags: f64,
    pub project: f64,
    pub age_max: f64,
    pub age: f64,
}

impl Default for UrgencyConfig {
    fn default() -> Self {
        UrgencyConfig {
            priority_h: 6.0,
            priority_m: 3.9,
            priority_l: 1.8,
            due: 12.0,
            blocking: 8.0,
            blocked: -5.0,
            active: 4.0,
            has_tags: 1.0,
            project: 1.0,
            age_max: 365.0,
            age: 2.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub default_project: String,
    pub date_dialect: String,
    pub llm: LlmConfig,
    pub urgency: UrgencyConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_project: "inbox".to_string(),
            date_dialect: "uk".to_string(),
            llm: LlmConfig::default(),
            urgency: UrgencyConfig::default(),
        }
    }
}

pub fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", "tk")
        .context("Could not determine home directory")
}

pub fn config_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    Ok(dirs.config_dir().join("config.toml"))
}

pub fn db_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    Ok(dirs.data_dir().join("tasks.db"))
}

pub fn load() -> Result<Config> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;
    toml::from_str(&content)
        .with_context(|| format!("Failed to parse config: {}", path.display()))
}

pub fn save(cfg: &Config) -> Result<()> {
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = toml::to_string_pretty(cfg).context("Failed to serialize config")?;
    std::fs::write(&path, content)
        .with_context(|| format!("Failed to write config: {}", path.display()))
}
