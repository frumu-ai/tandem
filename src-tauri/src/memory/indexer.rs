use crate::error::Result;
use crate::memory::manager::MemoryManager;
use crate::memory::types::{MemoryTier, StoreMessageRequest};
use ignore::WalkBuilder;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;
use tauri::{AppHandle, Emitter};

#[derive(Serialize, Clone)]
pub struct IndexingProgress {
    pub files_processed: usize,
    pub current_file: String,
}

#[derive(Serialize)]
pub struct IndexingStats {
    pub files_processed: usize,
    pub chunks_created: usize,
    pub errors: usize,
}

pub async fn index_workspace(
    app: &AppHandle,
    workspace_path: &Path,
    project_id: &str,
    memory_manager: &Arc<MemoryManager>,
) -> Result<IndexingStats> {
    let walker = WalkBuilder::new(workspace_path)
        .hidden(true)
        .git_ignore(true)
        .build();

    let stats = Arc::new(std::sync::Mutex::new(IndexingStats {
        files_processed: 0,
        chunks_created: 0,
        errors: 0,
    }));

    for result in walker {
        match result {
            Ok(entry) => {
                if entry.file_type().map_or(false, |ft| ft.is_file()) {
                    let path = entry.path().to_path_buf();
                    let relative_path = path
                        .strip_prefix(workspace_path)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .to_string();

                    // Skip likely binary or large files based on extension
                    // This is a basic filter, can be improved
                    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                        if [
                            "exe", "dll", "so", "dylib", "bin", "obj", "iso", "zip", "png", "jpg",
                            "jpeg", "gif", "ico", "pdf",
                        ]
                        .contains(&ext.to_lowercase().as_str())
                        {
                            continue;
                        }
                    }

                    let manager = memory_manager.clone();
                    let pid = project_id.to_string();
                    let stats_clone = stats.clone();

                    // Spawn a task for each file to process in parallel (bounded by semaphore ideally, but for now relies on Tokio)
                    // We'll process sequentially here to avoid overwhelming the DB/runtime for this first pass
                    // or use a semaphore if we want parallelism.
                    // Let's do sequential for safety and simplicity first.

                    match std::fs::read_to_string(&path) {
                        Ok(content) => {
                            if content.trim().is_empty() {
                                continue;
                            }

                            let request = StoreMessageRequest {
                                content,
                                tier: MemoryTier::Project,
                                session_id: None,
                                project_id: Some(pid),
                                source: "file".to_string(),
                                metadata: Some(serde_json::json!({
                                    "path": relative_path,
                                    "filename": entry.file_name().to_string_lossy()
                                })),
                            };

                            match manager.store_message(request).await {
                                Ok(chunks) => {
                                    let mut s = stats_clone.lock().unwrap();
                                    s.files_processed += 1;
                                    s.chunks_created += chunks.len();

                                    let _ = app.emit(
                                        "indexing-progress",
                                        IndexingProgress {
                                            files_processed: s.files_processed,
                                            current_file: relative_path.clone(),
                                        },
                                    );
                                }
                                Err(e) => {
                                    tracing::warn!("Failed to store file {}: {}", relative_path, e);
                                    let mut s = stats_clone.lock().unwrap();
                                    s.errors += 1;
                                }
                            }
                        }
                        Err(_) => {
                            // Likely binary or unreadable
                            continue;
                        }
                    }
                }
            }
            Err(err) => {
                tracing::warn!("Error walking directory: {}", err);
            }
        }
    }

    let final_stats = stats.lock().unwrap();
    Ok(IndexingStats {
        files_processed: final_stats.files_processed,
        chunks_created: final_stats.chunks_created,
        errors: final_stats.errors,
    })
}
