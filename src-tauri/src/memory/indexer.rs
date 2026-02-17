use crate::error::Result;
use crate::memory::manager::MemoryManager;
use crate::memory::types::{MemoryTier, StoreMessageRequest};
use ignore::WalkBuilder;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::UNIX_EPOCH;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone)]
pub struct IndexingProgress {
    pub project_id: String,
    pub files_processed: usize,
    pub total_files: usize,
    pub indexed_files: usize,
    pub skipped_files: usize,
    pub deleted_files: usize,
    pub errors: usize,
    pub chunks_created: usize,
    pub current_file: String,
}

#[derive(Serialize, Clone)]
pub struct IndexingStart {
    pub project_id: String,
    pub total_files: usize,
}

#[derive(Serialize, Clone)]
pub struct IndexingComplete {
    pub project_id: String,
    pub total_files: usize,
    pub files_processed: usize,
    pub indexed_files: usize,
    pub skipped_files: usize,
    pub deleted_files: usize,
    pub chunks_created: usize,
    pub errors: usize,
}

#[derive(Serialize)]
pub struct IndexingStats {
    pub total_files: usize,
    pub files_processed: usize,
    pub indexed_files: usize,
    pub skipped_files: usize,
    pub deleted_files: usize,
    pub chunks_created: usize,
    pub errors: usize,
}

pub async fn index_workspace(
    app: &AppHandle,
    workspace_path: &Path,
    project_id: &str,
    memory_manager: &Arc<MemoryManager>,
) -> Result<IndexingStats> {
    index_workspace_impl(Some(app), workspace_path, project_id, memory_manager).await
}

async fn index_workspace_impl(
    app: Option<&AppHandle>,
    workspace_path: &Path,
    project_id: &str,
    memory_manager: &Arc<MemoryManager>,
) -> Result<IndexingStats> {
    const MAX_FILE_SIZE_BYTES: u64 = 2 * 1024 * 1024; // 2MB cap to avoid giant blobs

    let excluded_dirs: HashSet<&'static str> = [
        ".git",
        "node_modules",
        "dist",
        "build",
        "target",
        ".next",
        ".turbo",
        ".cache",
    ]
    .into_iter()
    .collect();

    let excluded_exts: HashSet<&'static str> = [
        "exe", "dll", "so", "dylib", "bin", "obj", "iso", "zip", "png", "jpg", "jpeg", "gif",
        "ico", "pdf",
    ]
    .into_iter()
    .collect();

    let walker = WalkBuilder::new(workspace_path)
        .hidden(true)
        .git_ignore(true)
        .git_exclude(true)
        .ignore(true)
        .filter_entry(move |entry| {
            if entry.file_type().map(|ft| ft.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    return !excluded_dirs.contains(name);
                }
            }
            true
        })
        .build();

    // Build candidate file list so we can show total_files for progress.
    let mut files: Vec<PathBuf> = Vec::new();
    for result in walker {
        let Ok(entry) = result else {
            continue;
        };

        if !entry.file_type().map_or(false, |ft| ft.is_file()) {
            continue;
        }

        let path = entry.path().to_path_buf();

        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if excluded_exts.contains(&ext.to_lowercase().as_str()) {
                continue;
            }
        }

        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.len() > MAX_FILE_SIZE_BYTES {
                continue;
            }
        }

        files.push(path);
    }

    let total_files = files.len();
    if let Some(app) = app {
        let _ = app.emit(
            "indexing-start",
            IndexingStart {
                project_id: project_id.to_string(),
                total_files,
            },
        );
    }

    // One-time legacy cleanup: older versions may have file chunks but no file index, which
    // makes incremental indexing impossible and can cause duplication on re-index.
    let db = memory_manager.db();
    if db.ensure_vector_tables_healthy().await.unwrap_or(false) {
        tracing::warn!(
            "Repaired memory vector tables before indexing project {}. Resetting file index to force reindex.",
            project_id
        );
        let _ = db.clear_project_file_index(project_id, false).await;
    }
    if db.project_file_index_count(project_id).await? == 0
        && db.project_has_file_chunks(project_id).await?
    {
        let _ = db.clear_project_file_index(project_id, false).await?;
    }

    let stats = Arc::new(std::sync::Mutex::new(IndexingStats {
        total_files,
        files_processed: 0,
        indexed_files: 0,
        skipped_files: 0,
        deleted_files: 0,
        chunks_created: 0,
        errors: 0,
    }));

    let existing_indexed_paths: HashSet<String> = db
        .list_file_index_paths(project_id)
        .await?
        .into_iter()
        .collect();
    let mut seen_paths: HashSet<String> = HashSet::new();

    for path in files {
        let relative_path = path
            .strip_prefix(workspace_path)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");

        seen_paths.insert(relative_path.clone());

        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let size = meta.len() as i64;

        let existing = db.get_file_index_entry(project_id, &relative_path).await?;
        if let Some((existing_mtime, existing_size, _)) = &existing {
            if *existing_mtime == mtime && *existing_size == size {
                let mut s = stats.lock().unwrap();
                s.files_processed += 1;
                s.skipped_files += 1;
                if let Some(app) = app {
                    let _ = app.emit(
                        "indexing-progress",
                        IndexingProgress {
                            project_id: project_id.to_string(),
                            files_processed: s.files_processed,
                            total_files: s.total_files,
                            indexed_files: s.indexed_files,
                            skipped_files: s.skipped_files,
                            deleted_files: s.deleted_files,
                            errors: s.errors,
                            chunks_created: s.chunks_created,
                            current_file: relative_path.clone(),
                        },
                    );
                }
                continue;
            }
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => {
                let mut s = stats.lock().unwrap();
                s.files_processed += 1;
                s.skipped_files += 1;
                continue;
            }
        };

        if content.trim().is_empty() {
            let mut s = stats.lock().unwrap();
            s.files_processed += 1;
            s.skipped_files += 1;
            continue;
        }

        let mut hasher = Sha256::new();
        hasher.update(content.as_bytes());
        let hash = format!("{:x}", hasher.finalize());

        if let Some((_, _, existing_hash)) = &existing {
            if existing_hash == &hash {
                db.upsert_file_index_entry(project_id, &relative_path, mtime, size, &hash)
                    .await?;
                let mut s = stats.lock().unwrap();
                s.files_processed += 1;
                s.skipped_files += 1;
                if let Some(app) = app {
                    let _ = app.emit(
                        "indexing-progress",
                        IndexingProgress {
                            project_id: project_id.to_string(),
                            files_processed: s.files_processed,
                            total_files: s.total_files,
                            indexed_files: s.indexed_files,
                            skipped_files: s.skipped_files,
                            deleted_files: s.deleted_files,
                            errors: s.errors,
                            chunks_created: s.chunks_created,
                            current_file: relative_path.clone(),
                        },
                    );
                }
                continue;
            }
        }

        // Changed file: remove previous chunks for this path, then re-index and update the file index entry.
        if let Err(err) = db
            .delete_project_file_chunks_by_path(project_id, &relative_path)
            .await
        {
            tracing::warn!(
                "Failed to delete stale chunks for {} ({}). Attempting vector repair.",
                relative_path,
                err
            );
            let _ = db.ensure_vector_tables_healthy().await;
            let mut s = stats.lock().unwrap();
            s.files_processed += 1;
            s.errors += 1;
            continue;
        }

        let request = StoreMessageRequest {
            content,
            tier: MemoryTier::Project,
            session_id: None,
            project_id: Some(project_id.to_string()),
            source: "file".to_string(),
            source_path: Some(relative_path.clone()),
            source_mtime: Some(mtime),
            source_size: Some(size),
            source_hash: Some(hash.clone()),
            metadata: Some(serde_json::json!({
                "path": relative_path,
                "filename": path.file_name().and_then(|n| n.to_str()).unwrap_or(""),
            })),
        };

        match memory_manager.store_message(request).await {
            Ok(chunks) => {
                db.upsert_file_index_entry(project_id, &relative_path, mtime, size, &hash)
                    .await?;
                let mut s = stats.lock().unwrap();
                s.files_processed += 1;
                s.indexed_files += 1;
                s.chunks_created += chunks.len();
                if let Some(app) = app {
                    let _ = app.emit(
                        "indexing-progress",
                        IndexingProgress {
                            project_id: project_id.to_string(),
                            files_processed: s.files_processed,
                            total_files: s.total_files,
                            indexed_files: s.indexed_files,
                            skipped_files: s.skipped_files,
                            deleted_files: s.deleted_files,
                            errors: s.errors,
                            chunks_created: s.chunks_created,
                            current_file: relative_path.clone(),
                        },
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Failed to store file {}: {}", relative_path, e);
                let mut s = stats.lock().unwrap();
                s.files_processed += 1;
                s.errors += 1;
            }
        }
    }

    let final_stats = stats.lock().unwrap();

    // Handle deleted files: remove indexed entries that are no longer present.
    let removed: Vec<String> = existing_indexed_paths
        .difference(&seen_paths)
        .cloned()
        .collect();
    drop(final_stats);

    if !removed.is_empty() {
        for rel in removed {
            if let Err(err) = db
                .delete_project_file_chunks_by_path(project_id, &rel)
                .await
            {
                tracing::warn!("Failed to delete removed file chunks for {}: {}", rel, err);
                let _ = db.ensure_vector_tables_healthy().await;
            }
            if let Err(err) = db.delete_file_index_entry(project_id, &rel).await {
                tracing::warn!(
                    "Failed to delete removed file index entry for {}: {}",
                    rel,
                    err
                );
            }
            let mut s = stats.lock().unwrap();
            s.deleted_files += 1;
        }
    }

    let snapshot = {
        let s = stats.lock().unwrap();
        (
            s.total_files,
            s.files_processed,
            s.indexed_files,
            s.skipped_files,
            s.deleted_files,
            s.chunks_created,
            s.errors,
        )
    };

    // Persist last run summary for UI/cooldown.
    if let Err(err) = db
        .upsert_project_index_status(
            project_id,
            snapshot.0 as i64,
            snapshot.1 as i64,
            snapshot.2 as i64,
            snapshot.3 as i64,
            snapshot.6 as i64,
        )
        .await
    {
        tracing::warn!("Failed to persist index status for {}: {}", project_id, err);
    }

    if let Some(app) = app {
        let _ = app.emit(
            "indexing-complete",
            IndexingComplete {
                project_id: project_id.to_string(),
                total_files: snapshot.0,
                files_processed: snapshot.1,
                indexed_files: snapshot.2,
                skipped_files: snapshot.3,
                deleted_files: snapshot.4,
                chunks_created: snapshot.5,
                errors: snapshot.6,
            },
        );
    }

    Ok(IndexingStats {
        total_files: snapshot.0,
        files_processed: snapshot.1,
        indexed_files: snapshot.2,
        skipped_files: snapshot.3,
        deleted_files: snapshot.4,
        chunks_created: snapshot.5,
        errors: snapshot.6,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn setup_test_manager(db_dir: &TempDir) -> Arc<MemoryManager> {
        let db_path = db_dir.path().join("test_memory.sqlite");
        Arc::new(MemoryManager::new(&db_path).await.unwrap())
    }

    #[tokio::test]
    async fn test_incremental_indexing_skips_unchanged() {
        let workspace = TempDir::new().unwrap();
        std::fs::write(workspace.path().join("a.txt"), "hello world").unwrap();
        std::fs::write(workspace.path().join("b.ts"), "console.log('hi')").unwrap();
        std::fs::write(workspace.path().join("c.md"), "# title").unwrap();

        let db_dir = TempDir::new().unwrap();
        let manager = setup_test_manager(&db_dir).await;

        let s1 = index_workspace_impl(None, workspace.path(), "project-1", &manager)
            .await
            .unwrap();
        assert_eq!(s1.total_files, 3);
        assert_eq!(s1.indexed_files, 3);
        assert_eq!(s1.skipped_files, 0);

        let s2 = index_workspace_impl(None, workspace.path(), "project-1", &manager)
            .await
            .unwrap();
        assert_eq!(s2.total_files, 3);
        assert_eq!(s2.indexed_files, 0);
        assert_eq!(s2.skipped_files, 3);

        let stats = manager.db().get_project_stats("project-1").await.unwrap();
        assert_eq!(stats.indexed_files, 3);
        assert!(stats.file_index_chunks > 0);
    }

    #[tokio::test]
    async fn test_incremental_indexing_reindexes_changed_and_removes_deleted() {
        let workspace = TempDir::new().unwrap();
        std::fs::write(workspace.path().join("a.txt"), "hello world").unwrap();
        std::fs::write(workspace.path().join("b.ts"), "console.log('hi')").unwrap();

        let db_dir = TempDir::new().unwrap();
        let manager = setup_test_manager(&db_dir).await;

        let s1 = index_workspace_impl(None, workspace.path(), "project-2", &manager)
            .await
            .unwrap();
        assert_eq!(s1.total_files, 2);
        assert_eq!(s1.indexed_files, 2);

        // Modify one file
        std::fs::write(workspace.path().join("b.ts"), "console.log('changed')").unwrap();
        let s2 = index_workspace_impl(None, workspace.path(), "project-2", &manager)
            .await
            .unwrap();
        assert_eq!(s2.total_files, 2);
        assert_eq!(s2.indexed_files, 1);
        assert_eq!(s2.skipped_files, 1);

        // Delete one file
        std::fs::remove_file(workspace.path().join("a.txt")).unwrap();
        let s3 = index_workspace_impl(None, workspace.path(), "project-2", &manager)
            .await
            .unwrap();
        assert_eq!(s3.total_files, 1);
        assert_eq!(s3.deleted_files, 1);

        let stats = manager.db().get_project_stats("project-2").await.unwrap();
        assert_eq!(stats.indexed_files, 1);
    }

    #[tokio::test]
    async fn test_clear_file_index_only_removes_file_chunks() {
        let workspace = TempDir::new().unwrap();
        std::fs::write(workspace.path().join("a.txt"), "hello world").unwrap();

        let db_dir = TempDir::new().unwrap();
        let manager = setup_test_manager(&db_dir).await;

        let _ = index_workspace_impl(None, workspace.path(), "project-3", &manager)
            .await
            .unwrap();

        // Add a non-file chunk in the same project.
        let req = StoreMessageRequest {
            content: "user fact".to_string(),
            tier: MemoryTier::Project,
            session_id: None,
            project_id: Some("project-3".to_string()),
            source: "user_message".to_string(),
            source_path: None,
            source_mtime: None,
            source_size: None,
            source_hash: None,
            metadata: None,
        };
        manager.store_message(req).await.unwrap();

        let before = manager.db().get_project_stats("project-3").await.unwrap();
        assert!(before.file_index_chunks > 0);
        assert!(before.project_chunks > before.file_index_chunks);

        let _ = manager
            .db()
            .clear_project_file_index("project-3", false)
            .await
            .unwrap();

        let after = manager.db().get_project_stats("project-3").await.unwrap();
        assert_eq!(after.file_index_chunks, 0);
        assert_eq!(after.indexed_files, 0);
        // Non-file chunks should remain.
        assert!(after.project_chunks > 0);
    }
}
