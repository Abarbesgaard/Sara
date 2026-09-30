use std::ffi::{OsStr, OsString};
use std::sync::{Mutex, MutexGuard, OnceLock};

pub struct EnvGuard {
    saved: Vec<(String, Option<OsString>)>,
    _lock: MutexGuard<'static, ()>,
}

pub fn env_guard() -> EnvGuard {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let lock = LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    EnvGuard {
        saved: Vec::new(),
        _lock: lock,
    }
}

impl EnvGuard {
    pub fn set(&mut self, key: &str, value: impl AsRef<OsStr>) -> &mut Self {
        self.save(key);
        unsafe { std::env::set_var(key, value) };
        self
    }

    pub fn remove(&mut self, key: &str) -> &mut Self {
        self.save(key);
        unsafe { std::env::remove_var(key) };
        self
    }

    fn save(&mut self, key: &str) {
        if !self.saved.iter().any(|(k, _)| k == key) {
            self.saved.push((key.to_string(), std::env::var_os(key)));
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..).rev() {
            match value {
                Some(v) => unsafe { std::env::set_var(&key, v) },
                None => unsafe { std::env::remove_var(&key) },
            }
        }
    }
}

#[test]
fn env_guard_restores_the_original_value_on_drop() {
    const KEY: &str = "SARA_TEST_SUPPORT_ENV_GUARD";
    {
        let mut env = env_guard();
        env.remove(KEY);
    }
    {
        let mut env = env_guard();
        env.set(KEY, "first").set(KEY, "second");
        assert_eq!(std::env::var(KEY).unwrap(), "second");
    }
    let _env = env_guard();
    assert!(std::env::var_os(KEY).is_none());
}

#[test]
fn env_guard_restores_even_when_the_test_panics() {
    const KEY: &str = "SARA_TEST_SUPPORT_ENV_GUARD_PANIC";
    let result = std::panic::catch_unwind(|| {
        let mut env = env_guard();
        env.set(KEY, "leaked?");
        panic!("boom");
    });
    assert!(result.is_err());
    let _env = env_guard();
    assert!(std::env::var_os(KEY).is_none());
}
