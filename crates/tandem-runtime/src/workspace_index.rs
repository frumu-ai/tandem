use std::path::PathBuf;
use std::sync::Arc;

use ignore::WalkBuilder;
use serde::Serialize;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Serialize, Default)]
pub struct WorkspaceIndexSnapshot {
    pub root: String,
    pub file_count: usize,
    pub indexed_at: Option<String>,
    pub largest_files: Vec<IndexedFile>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IndexedFile {
    pub path: String,
    pub bytes: u64,
}

#[derive(Clone)]
pub struct WorkspaceIndex {
    root: Arc<PathBuf>,
    snapshot: Arc<RwLock<WorkspaceIndexSnapshot>>,
}

impl WorkspaceIndex {
    pub async fn new(root: impl Into<PathBuf>) -> Self {
        let root = root.into();
        let initial = WorkspaceIndexSnapshot {
            root: root.to_string_lossy().to_string(),
            ..WorkspaceIndexSnapshot::default()
        };
        let this = Self {
            root: Arc::new(root),
            snapshot: Arc::new(RwLock::new(initial)),
        };
        let clone = this.clone();
        tokio::spawn(async move {
            let _ = clone.refresh().await;
        });
        this
    }

    pub async fn refresh(&self) -> WorkspaceIndexSnapshot {
        let root = self.root.clone();
        let (mut files, count) = tokio::task::spawn_blocking(move || {
            let mut files = Vec::new();
            let mut count = 0usize;
            for entry in WalkBuilder::new(root.as_path()).build().flatten() {
                if !entry.file_type().map(|f| f.is_file()).unwrap_or(false) {
                    continue;
                }
                count += 1;
                if let Ok(meta) = entry.metadata() {
                    files.push(IndexedFile {
                        path: relativize(root.as_path(), entry.path()),
                        bytes: meta.len(),
                    });
                }
            }
            (files, count)
        })
        .await
        .unwrap_or_default();

        files.sort_by(|a, b| b.bytes.cmp(&a.bytes));
        let largest_files = files.into_iter().take(20).collect::<Vec<_>>();
        let snapshot = WorkspaceIndexSnapshot {
            root: self.root.to_string_lossy().to_string(),
            file_count: count,
            indexed_at: Some(chrono::Utc::now().to_rfc3339()),
            largest_files,
        };
        *self.snapshot.write().await = snapshot.clone();
        snapshot
    }

    pub async fn snapshot(&self) -> WorkspaceIndexSnapshot {
        self.snapshot.read().await.clone()
    }
}

fn relativize(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .map(|v| v.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}
