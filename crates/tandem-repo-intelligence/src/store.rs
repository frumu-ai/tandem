use crate::error::{RepoIntelligenceError, Result};
use crate::extract_repo_facts;
use crate::model::RepoIndexSnapshot;
use crate::scanner::scan_repo;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct JsonRepoIndexStore {
    path: PathBuf,
}

impl JsonRepoIndexStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn index_repo(&self, root: impl AsRef<Path>) -> Result<RepoIndexSnapshot> {
        let root = root.as_ref();
        let mut manifest = scan_repo(root)?;
        if let Some(store_path) = relative_store_path(root, &self.path) {
            manifest.retain(|entry| entry.path != store_path);
        }
        let facts = extract_repo_facts(root, &manifest)?;
        let snapshot = RepoIndexSnapshot {
            root_label: root.to_string_lossy().to_string(),
            indexed_unix_ms: now_unix_ms(),
            manifest,
            facts,
        };
        self.save(&snapshot)?;
        Ok(snapshot)
    }

    pub fn save(&self, snapshot: &RepoIndexSnapshot) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| {
                RepoIntelligenceError::WriteStore {
                    path: parent.to_path_buf(),
                    source,
                }
            })?;
        }
        let body = serde_json::to_vec_pretty(snapshot).map_err(|source| {
            RepoIntelligenceError::EncodeStore {
                path: self.path.clone(),
                source,
            }
        })?;
        std::fs::write(&self.path, body).map_err(|source| RepoIntelligenceError::WriteStore {
            path: self.path.clone(),
            source,
        })
    }

    pub fn load(&self) -> Result<RepoIndexSnapshot> {
        let body =
            std::fs::read(&self.path).map_err(|source| RepoIntelligenceError::ReadStore {
                path: self.path.clone(),
                source,
            })?;
        serde_json::from_slice(&body).map_err(|source| RepoIntelligenceError::DecodeStore {
            path: self.path.clone(),
            source,
        })
    }
}

fn relative_store_path(root: &Path, store_path: &Path) -> Option<String> {
    let relative = if store_path.is_absolute() {
        store_path.strip_prefix(root).ok()?
    } else {
        store_path
    };
    let path = relative
        .to_string_lossy()
        .replace('\\', "/")
        .trim_start_matches("./")
        .trim_start_matches('/')
        .to_string();
    (!path.is_empty()).then_some(path)
}

fn now_unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}
