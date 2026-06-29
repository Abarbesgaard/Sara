use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    pub urgency: UrgencyConfig,
    /// Absolute path to Sara's private knowledge store (markdown notes/links).
    pub vault_path: Option<PathBuf>,
    /// Embeddings provider settings (optional, for semantic search).
    pub embeddings: EmbeddingsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingsConfig {
    pub provider: String,
    pub model: String,
    pub base_url: Option<String>,
}

impl Default for EmbeddingsConfig {
    fn default() -> Self {
        EmbeddingsConfig {
            provider: "ollama".to_string(),
            model: "nomic-embed-text".to_string(),
            base_url: None,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            default_project: "inbox".to_string(),
            date_dialect: "uk".to_string(),
            urgency: UrgencyConfig::default(),
            vault_path: None,
            embeddings: EmbeddingsConfig::default(),
        }
    }
}

impl Config {
    /// True when [embeddings] was never customized (still Ollama defaults).
    pub fn embeddings_at_default(&self) -> bool {
        self.embeddings.provider == "ollama" && self.embeddings.model == "nomic-embed-text"
    }

    /// Embeddings provider.
    pub fn effective_embeddings_provider(&self) -> String {
        self.embeddings.provider.clone()
    }

    /// Deployment/model name for embeddings API calls.
    pub fn effective_embeddings_model(&self) -> String {
        self.embeddings.model.clone()
    }
}

pub fn project_dirs() -> Result<ProjectDirs> {
    ProjectDirs::from("", "", "sara").context("Could not determine home directory")
}

fn legacy_tk_project_dirs() -> Option<ProjectDirs> {
    ProjectDirs::from("", "", "tk")
}

pub fn config_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    Ok(dirs.config_dir().join("config.toml"))
}

pub fn db_path() -> Result<PathBuf> {
    let dirs = project_dirs()?;
    Ok(dirs.data_dir().join("tasks.db"))
}

pub fn vault_path(cfg: &Config) -> Result<PathBuf> {
    if let Some(ref p) = cfg.vault_path {
        return Ok(p.clone());
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    Ok(cwd.join("Sara"))
}

pub fn set_vault_path(cfg: &mut Config, path: PathBuf) -> Result<()> {
    cfg.vault_path = Some(path);
    save(cfg)
}

fn legacy_tk_config_path() -> Option<PathBuf> {
    legacy_tk_project_dirs().map(|d| d.config_dir().join("config.toml"))
}

fn legacy_tk_db_path() -> Option<PathBuf> {
    legacy_tk_project_dirs().map(|d| d.data_dir().join("tasks.db"))
}

/// Copy legacy tk config/data into sara locations on first run.
pub fn migrate_from_tk_if_needed() -> Result<bool> {
    let sara_cfg = config_path()?;
    let sara_db = db_path()?;
    let mut migrated = false;

    if let Some(tk_cfg) = legacy_tk_config_path()
        && tk_cfg.exists()
        && !sara_cfg.exists()
    {
        if let Some(parent) = sara_cfg.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&tk_cfg, &sara_cfg)
            .with_context(|| format!("Failed to copy config from {}", tk_cfg.display()))?;
        migrated = true;
    }

    if let Some(tk_db) = legacy_tk_db_path()
        && tk_db.exists()
        && !sara_db.exists()
    {
        if let Some(parent) = sara_db.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(&tk_db, &sara_db)
            .with_context(|| format!("Failed to copy database from {}", tk_db.display()))?;
        migrated = true;
    }

    if migrated {
        eprintln!(
            "Imported existing tk config and tasks into sara.\n\
             You can remove the old tk binary with: cargo uninstall tk"
        );
    }

    Ok(migrated)
}

pub fn load() -> Result<Config> {
    migrate_from_tk_if_needed()?;
    let path = config_path()?;
    if !path.exists() {
        return Ok(Config::default());
    }
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read config: {}", path.display()))?;
    toml::from_str(&content).with_context(|| format!("Failed to parse config: {}", path.display()))
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::sync::{Mutex, OnceLock};

    static TEST_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        TEST_LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn temp_home(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("sara-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn with_home<F: FnOnce()>(name: &str, f: F) {
        let _guard = test_lock();
        let home = temp_home(name);
        let old_home = std::env::var("HOME").ok();
        let old_xdg = std::env::var("XDG_CONFIG_HOME").ok();
        let old_data = std::env::var("XDG_DATA_HOME").ok();
        unsafe {
            std::env::set_var("HOME", &home);
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::remove_var("XDG_DATA_HOME");
        }
        f();
        if let Some(h) = old_home {
            unsafe {
                std::env::set_var("HOME", h);
            }
        }
        if let Some(x) = old_xdg {
            unsafe {
                std::env::set_var("XDG_CONFIG_HOME", x);
            }
        } else {
            unsafe {
                std::env::remove_var("XDG_CONFIG_HOME");
            }
        }
        if let Some(d) = old_data {
            unsafe {
                std::env::set_var("XDG_DATA_HOME", d);
            }
        } else {
            unsafe {
                std::env::remove_var("XDG_DATA_HOME");
            }
        }
        let _ = fs::remove_dir_all(&home);
    }

    fn tk_config_dir(home: &Path) -> PathBuf {
        home.join("Library/Application Support/tk")
    }

    fn sara_config_dir(home: &Path) -> PathBuf {
        home.join("Library/Application Support/sara")
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn migrate_copies_tk_config_and_db_when_sara_missing() {
        with_home("migrate", || {
            let home = std::env::var("HOME").unwrap();
            let home = PathBuf::from(home);

            let tk_dir = tk_config_dir(&home);
            fs::create_dir_all(&tk_dir).unwrap();
            fs::write(tk_dir.join("config.toml"), "default_project = \"inbox\"\n").unwrap();
            fs::write(tk_dir.join("tasks.db"), b"sqlite-demo").unwrap();

            let migrated = migrate_from_tk_if_needed().unwrap();
            assert!(migrated);

            let sara_dir = sara_config_dir(&home);
            assert!(sara_dir.join("config.toml").exists());
            assert!(sara_dir.join("tasks.db").exists());
            assert_eq!(
                fs::read_to_string(sara_dir.join("config.toml")).unwrap(),
                "default_project = \"inbox\"\n"
            );
        });
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn migrate_skips_when_sara_already_exists() {
        with_home("migrate-skip", || {
            let home = PathBuf::from(std::env::var("HOME").unwrap());

            let tk_dir = tk_config_dir(&home);
            fs::create_dir_all(&tk_dir).unwrap();
            fs::write(
                tk_dir.join("config.toml"),
                "default_project = \"tk-only\"\n",
            )
            .unwrap();

            let sara_dir = sara_config_dir(&home);
            fs::create_dir_all(&sara_dir).unwrap();
            fs::write(sara_dir.join("config.toml"), "default_project = \"sara\"\n").unwrap();

            let migrated = migrate_from_tk_if_needed().unwrap();
            assert!(!migrated);
            assert_eq!(
                fs::read_to_string(sara_dir.join("config.toml")).unwrap(),
                "default_project = \"sara\"\n"
            );
        });
    }
}
