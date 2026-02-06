use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::{Mutex, OwnedRwLockWriteGuard, RwLock};

pub struct PathLockManager {
    locks: Mutex<HashMap<String, Arc<RwLock<()>>>>,
}

impl PathLockManager {
    pub fn new() -> Self {
        Self {
            locks: Mutex::new(HashMap::new()),
        }
    }

    pub async fn write_lock(&self, path: &Path) -> OwnedRwLockWriteGuard<()> {
        let key = normalize_path(path);
        let lock = {
            let mut locks = self.locks.lock().await;
            locks
                .entry(key)
                .or_insert_with(|| Arc::new(RwLock::new(())))
                .clone()
        };
        lock.write_owned().await
    }
}

fn normalize_path(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/").to_lowercase()
}
