use std::ffi::OsString;
use std::sync::{Mutex, MutexGuard, OnceLock};

pub(crate) fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(crate) struct EnvRestore(Vec<(&'static str, Option<OsString>)>);

impl EnvRestore {
    pub(crate) fn set(values: &[(&'static str, &str)]) -> Self {
        let restore = Self(
            values
                .iter()
                .map(|(name, _)| (*name, std::env::var_os(name)))
                .collect(),
        );
        for (name, value) in values {
            std::env::set_var(name, value);
        }
        restore
    }
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        for (name, value) in self.0.drain(..) {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    }
}
