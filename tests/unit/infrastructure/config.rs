use super::*;
use std::fs;
use std::path::Path;

use crate::test_support::{env_guard, temp_dir};

fn with_home<F: FnOnce()>(f: F) {
    let mut env = env_guard();
    let home = temp_dir();
    env.set("HOME", home.path())
        .remove("XDG_CONFIG_HOME")
        .remove("XDG_DATA_HOME");
    f();
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
    with_home(|| {
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
    with_home(|| {
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
