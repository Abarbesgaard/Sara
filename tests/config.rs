//! Integration tests for `sara_tasks::infrastructure::config`.
//! Moved out of an inline mod tests block in src/infrastructure/config.rs.
//!
//! Mutates process-wide env vars (HOME/XDG_*) guarded by a local mutex --
//! keep all such tests in this one file so the mutex keeps working (each
//! tests/*.rs file is compiled as its own separate test binary/process).

use sara_tasks::infrastructure::config::*;
use std::fs;
use std::path::{Path, PathBuf};
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
