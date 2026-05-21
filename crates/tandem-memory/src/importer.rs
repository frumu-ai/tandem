use crate::manager::MemoryManager;
use crate::types::{
    MemoryError, MemoryImportFormat, MemoryImportProgress, MemoryImportRequest, MemoryImportStats,
    MemoryTier, StoreMessageRequest,
};
use ignore::WalkBuilder;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const MAX_FILE_SIZE_BYTES: u64 = 2 * 1024 * 1024;

pub async fn import_files<F>(
    memory_manager: &MemoryManager,
    request: &MemoryImportRequest,
    mut on_progress: Option<F>,
) -> Result<MemoryImportStats, MemoryError>
where
    F: FnMut(&MemoryImportProgress),
{
    validate_scope(request)?;
    let embedding_health = memory_manager.embedding_health().await;
    if embedding_health.status != "ok" {
        return Err(MemoryError::Embedding(format!(
            "embeddings disabled for import: {}",
            embedding_health
                .reason
                .unwrap_or_else(|| embedding_health.status.clone())
        )));
    }

    let root_path = PathBuf::from(&request.root_path);
    let canonical_root = std::fs::canonicalize(&root_path)?;
    let namespace = import_namespace(&canonical_root, request);
    let files = match discover_files(&canonical_root, request.format) {
        Ok(files) => files,
        Err(MemoryError::InvalidConfig(message))
            if request.sync_deletes && message.contains("no importable files found") =>
        {
            Vec::new()
        }
        Err(err) => return Err(err),
    };
    let total_files = files.len();
    let db = memory_manager.db();

    let existing_indexed_paths: HashSet<String> = db
        .list_import_index_paths_for_tenant(
            request.tier,
            request.session_id.as_deref(),
            request.project_id.as_deref(),
            &request.tenant_scope,
        )
        .await?
        .into_iter()
        .filter(|path| path.starts_with(&format!("{namespace}/")))
        .collect();
    let mut seen_paths = HashSet::new();
    let mut stats = MemoryImportStats {
        discovered_files: total_files,
        ..MemoryImportStats::default()
    };

    for path in files {
        let relative_path = path
            .strip_prefix(&canonical_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        let indexed_path = format!("{namespace}/{relative_path}");
        seen_paths.insert(indexed_path.clone());

        let meta = match std::fs::metadata(&path) {
            Ok(meta) => meta,
            Err(_) => {
                stats.files_processed += 1;
                stats.skipped_files += 1;
                emit_progress(&mut on_progress, &stats, total_files, &relative_path);
                continue;
            }
        };

        let mtime = meta
            .modified()
            .ok()
            .and_then(|value| value.duration_since(UNIX_EPOCH).ok())
            .map(|value| value.as_secs() as i64)
            .unwrap_or(0);
        let size = meta.len() as i64;

        let existing = db
            .get_import_index_entry_for_tenant(
                request.tier,
                request.session_id.as_deref(),
                request.project_id.as_deref(),
                &indexed_path,
                &request.tenant_scope,
            )
            .await?;
        if let Some((existing_mtime, existing_size, _)) = &existing {
            if *existing_mtime == mtime && *existing_size == size {
                stats.files_processed += 1;
                stats.skipped_files += 1;
                emit_progress(&mut on_progress, &stats, total_files, &relative_path);
                continue;
            }
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(content) => content,
            Err(_) => {
                stats.files_processed += 1;
                stats.skipped_files += 1;
                emit_progress(&mut on_progress, &stats, total_files, &relative_path);
                continue;
            }
        };

        if content.trim().is_empty() {
            stats.files_processed += 1;
            stats.skipped_files += 1;
            emit_progress(&mut on_progress, &stats, total_files, &relative_path);
            continue;
        }

        let content_hash = sha256_hex(content.as_bytes());
        let hash = request
            .source_binding
            .as_ref()
            .map(|binding| scoped_source_hash(&content_hash, &indexed_path, binding))
            .unwrap_or_else(|| content_hash.clone());
        if let Some((_, _, existing_hash)) = &existing {
            if existing_hash == &hash {
                db.upsert_import_index_entry_for_tenant(
                    request.tier,
                    request.session_id.as_deref(),
                    request.project_id.as_deref(),
                    &indexed_path,
                    mtime,
                    size,
                    &hash,
                    &request.tenant_scope,
                )
                .await?;
                stats.files_processed += 1;
                stats.skipped_files += 1;
                emit_progress(&mut on_progress, &stats, total_files, &relative_path);
                continue;
            }
        }

        if let Err(err) = db
            .delete_file_chunks_by_path_for_tenant(
                request.tier,
                request.session_id.as_deref(),
                request.project_id.as_deref(),
                &indexed_path,
                &request.tenant_scope,
            )
            .await
        {
            tracing::warn!(
                "Failed to delete stale chunks for import path {} ({}). Attempting vector repair.",
                indexed_path,
                err
            );
            let _ = db.ensure_vector_tables_healthy().await;
            stats.files_processed += 1;
            stats.errors += 1;
            emit_progress(&mut on_progress, &stats, total_files, &relative_path);
            continue;
        }

        let mut request_metadata = serde_json::json!({
            "path": relative_path,
            "filename": path.file_name().and_then(|name| name.to_str()).unwrap_or(""),
            "import_format": request.format.to_string(),
            "import_root": canonical_root.display().to_string(),
            "import_namespace": namespace,
        });
        if let Some(binding) = request.source_binding.as_ref() {
            request_metadata["enterprise_source_binding"] = serde_json::json!({
                "binding_id": binding.binding_id,
                "connector_id": binding.connector_id,
                "resource_ref": binding.resource_ref,
                "data_class": binding.data_class,
                "source_object_id": source_object_id(request, binding, &indexed_path, &hash),
                "native_object_id": indexed_path,
                "content_hash": content_hash,
            });
        }
        let store_request = StoreMessageRequest {
            content,
            tier: request.tier,
            session_id: request.session_id.clone(),
            project_id: request.project_id.clone(),
            source: "file".to_string(),
            source_path: Some(indexed_path.clone()),
            source_mtime: Some(mtime),
            source_size: Some(size),
            source_hash: Some(hash.clone()),
            tenant_scope: request.tenant_scope.clone(),
            metadata: Some(request_metadata),
        };

        match memory_manager.store_message(store_request).await {
            Ok(chunks) => {
                db.upsert_import_index_entry_for_tenant(
                    request.tier,
                    request.session_id.as_deref(),
                    request.project_id.as_deref(),
                    &indexed_path,
                    mtime,
                    size,
                    &hash,
                    &request.tenant_scope,
                )
                .await?;
                stats.files_processed += 1;
                stats.indexed_files += 1;
                stats.chunks_created += chunks.len();
            }
            Err(err) => {
                tracing::warn!("Failed to store imported file {}: {}", relative_path, err);
                stats.files_processed += 1;
                stats.errors += 1;
            }
        }
        emit_progress(&mut on_progress, &stats, total_files, &relative_path);
    }

    if request.sync_deletes {
        let removed: Vec<String> = existing_indexed_paths
            .difference(&seen_paths)
            .cloned()
            .collect();
        for indexed_path in removed {
            if let Err(err) = db
                .delete_file_chunks_by_path_for_tenant(
                    request.tier,
                    request.session_id.as_deref(),
                    request.project_id.as_deref(),
                    &indexed_path,
                    &request.tenant_scope,
                )
                .await
            {
                tracing::warn!(
                    "Failed to delete removed imported chunks for {}: {}",
                    indexed_path,
                    err
                );
                let _ = db.ensure_vector_tables_healthy().await;
                stats.errors += 1;
                continue;
            }
            if let Err(err) = db
                .delete_import_index_entry_for_tenant(
                    request.tier,
                    request.session_id.as_deref(),
                    request.project_id.as_deref(),
                    &indexed_path,
                    &request.tenant_scope,
                )
                .await
            {
                tracing::warn!(
                    "Failed to delete removed import index entry for {}: {}",
                    indexed_path,
                    err
                );
                stats.errors += 1;
                continue;
            }
            stats.deleted_files += 1;
        }
    }

    Ok(stats)
}

fn validate_scope(request: &MemoryImportRequest) -> Result<(), MemoryError> {
    match request.tier {
        MemoryTier::Session
            if request
                .session_id
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty() =>
        {
            Err(MemoryError::InvalidConfig(
                "tier=session requires session_id".to_string(),
            ))
        }
        MemoryTier::Project
            if request
                .project_id
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty() =>
        {
            Err(MemoryError::InvalidConfig(
                "tier=project requires project_id".to_string(),
            ))
        }
        _ => Ok(()),
    }
}

fn emit_progress<F>(
    callback: &mut Option<F>,
    stats: &MemoryImportStats,
    total_files: usize,
    current_file: &str,
) where
    F: FnMut(&MemoryImportProgress),
{
    if let Some(callback) = callback.as_mut() {
        callback(&MemoryImportProgress {
            files_processed: stats.files_processed,
            total_files,
            indexed_files: stats.indexed_files,
            skipped_files: stats.skipped_files,
            deleted_files: stats.deleted_files,
            errors: stats.errors,
            chunks_created: stats.chunks_created,
            current_file: current_file.to_string(),
        });
    }
}

fn discover_files(
    root_path: &Path,
    format: MemoryImportFormat,
) -> Result<Vec<PathBuf>, MemoryError> {
    let mut files = Vec::new();
    match format {
        MemoryImportFormat::Openclaw => {
            let memory_md = root_path.join("MEMORY.md");
            if memory_md.is_file() {
                files.push(memory_md);
            }
            let memory_dir = root_path.join("memory");
            if memory_dir.is_dir() {
                let walker = WalkBuilder::new(&memory_dir)
                    .hidden(true)
                    .git_ignore(true)
                    .git_exclude(true)
                    .ignore(true)
                    .build();
                for result in walker {
                    let Ok(entry) = result else {
                        continue;
                    };
                    if entry.file_type().is_some_and(|ft| ft.is_file())
                        && is_supported_extension(entry.path())
                    {
                        files.push(entry.into_path());
                    }
                }
            }
        }
        MemoryImportFormat::Directory => {
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

            let walker = WalkBuilder::new(root_path)
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
            for result in walker {
                let Ok(entry) = result else {
                    continue;
                };
                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    continue;
                }
                if !is_supported_extension(entry.path()) {
                    continue;
                }
                if let Ok(meta) = std::fs::metadata(entry.path()) {
                    if meta.len() > MAX_FILE_SIZE_BYTES {
                        continue;
                    }
                }
                files.push(entry.into_path());
            }
        }
    }

    files.sort();
    files.dedup();
    if files.is_empty() {
        return Err(MemoryError::InvalidConfig(format!(
            "no importable files found for format={} under {}",
            format,
            root_path.display()
        )));
    }
    Ok(files)
}

fn is_supported_extension(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase())
            .as_deref(),
        Some("md" | "markdown" | "mdx" | "txt")
    )
}

fn import_namespace(root_path: &Path, request: &MemoryImportRequest) -> String {
    let mut hasher = Sha256::new();
    hasher.update(root_path.to_string_lossy().as_bytes());
    hasher.update(b"\n");
    hasher.update(request.format.to_string().as_bytes());
    hasher.update(b"\n");
    hasher.update(request.tier.to_string().as_bytes());
    hasher.update(b"\n");
    hasher.update(request.session_id.as_deref().unwrap_or("").as_bytes());
    hasher.update(b"\n");
    hasher.update(request.project_id.as_deref().unwrap_or("").as_bytes());
    let digest = format!("{:x}", hasher.finalize());
    format!("import-{}", &digest[..16])
}

fn scoped_source_hash(
    content_hash: &str,
    indexed_path: &str,
    binding: &crate::types::MemoryImportSourceBinding,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(binding.binding_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(binding.connector_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(binding.resource_ref.to_string().as_bytes());
    hasher.update(b"\n");
    hasher.update(binding.data_class.as_bytes());
    hasher.update(b"\n");
    hasher.update(indexed_path.as_bytes());
    hasher.update(b"\n");
    hasher.update(content_hash.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

fn source_object_id(
    request: &MemoryImportRequest,
    binding: &crate::types::MemoryImportSourceBinding,
    indexed_path: &str,
    source_hash: &str,
) -> String {
    let mut hasher = Sha256::new();
    hasher.update(request.tenant_scope.org_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(request.tenant_scope.workspace_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(
        request
            .tenant_scope
            .deployment_id
            .as_deref()
            .unwrap_or("")
            .as_bytes(),
    );
    hasher.update(b"\n");
    hasher.update(binding.binding_id.as_bytes());
    hasher.update(b"\n");
    hasher.update(indexed_path.as_bytes());
    hasher.update(b"\n");
    hasher.update(source_hash.as_bytes());
    format!(
        "source-object-{}",
        &format!("{:x}", hasher.finalize())[..24]
    )
}

fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MemoryTenantScope;
    use crate::MemoryManager;
    use tempfile::tempdir;

    fn is_embeddings_disabled(err: &crate::types::MemoryError) -> bool {
        matches!(err, crate::types::MemoryError::Embedding(msg) if msg.to_ascii_lowercase().contains("embeddings disabled"))
    }

    async fn setup_manager() -> (MemoryManager, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let db_path = dir.path().join("memory.sqlite");
        let manager = MemoryManager::new(&db_path).await.unwrap();
        (manager, dir)
    }

    #[tokio::test]
    async fn imports_openclaw_markdown_and_skips_unchanged_reimport() {
        let (manager, dir) = setup_manager().await;
        let root = dir.path().join("openclaw");
        std::fs::create_dir_all(root.join("memory")).unwrap();
        std::fs::write(root.join("MEMORY.md"), "# Root\nAlpha").unwrap();
        std::fs::write(root.join("memory").join("note.md"), "Beta").unwrap();

        let request = MemoryImportRequest {
            root_path: root.display().to_string(),
            format: MemoryImportFormat::Openclaw,
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            tenant_scope: MemoryTenantScope::local(),
            source_binding: None,
            sync_deletes: false,
        };

        let first = match import_files(&manager, &request, None::<fn(&MemoryImportProgress)>).await
        {
            Ok(stats) => stats,
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("import_files failed: {err}"),
        };
        assert_eq!(first.discovered_files, 2);
        assert_eq!(first.indexed_files, 2);

        let second = match import_files(&manager, &request, None::<fn(&MemoryImportProgress)>).await
        {
            Ok(stats) => stats,
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("import_files failed: {err}"),
        };
        assert_eq!(second.indexed_files, 0);
        assert_eq!(second.skipped_files, 2);
    }

    #[tokio::test]
    async fn sync_deletes_removes_missing_files_only_for_current_namespace() {
        let (manager, dir) = setup_manager().await;
        let root_a = dir.path().join("docs-a");
        let root_b = dir.path().join("docs-b");
        std::fs::create_dir_all(&root_a).unwrap();
        std::fs::create_dir_all(&root_b).unwrap();
        std::fs::write(root_a.join("note.md"), "Alpha").unwrap();
        std::fs::write(root_b.join("note.md"), "Beta").unwrap();

        let request_a = MemoryImportRequest {
            root_path: root_a.display().to_string(),
            format: MemoryImportFormat::Directory,
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            tenant_scope: MemoryTenantScope::local(),
            source_binding: None,
            sync_deletes: false,
        };
        let request_b = MemoryImportRequest {
            root_path: root_b.display().to_string(),
            format: MemoryImportFormat::Directory,
            tier: MemoryTier::Global,
            session_id: None,
            project_id: None,
            tenant_scope: MemoryTenantScope::local(),
            source_binding: None,
            sync_deletes: false,
        };

        match import_files(&manager, &request_a, None::<fn(&MemoryImportProgress)>).await {
            Ok(_) => {}
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("import_files failed: {err}"),
        }
        match import_files(&manager, &request_b, None::<fn(&MemoryImportProgress)>).await {
            Ok(_) => {}
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("import_files failed: {err}"),
        }

        std::fs::remove_file(root_a.join("note.md")).unwrap();
        let delete_stats = match import_files(
            &manager,
            &MemoryImportRequest {
                sync_deletes: true,
                ..request_a.clone()
            },
            None::<fn(&MemoryImportProgress)>,
        )
        .await
        {
            Ok(stats) => stats,
            Err(err) if is_embeddings_disabled(&err) => return,
            Err(err) => panic!("import_files failed: {err}"),
        };
        assert_eq!(delete_stats.deleted_files, 1);

        let chunks = manager.db().get_global_chunks(20).await.unwrap();
        assert_eq!(chunks.len(), 1);
        let remaining = chunks[0].metadata.clone().unwrap();
        assert_eq!(remaining["import_root"], root_b.display().to_string());
    }
}
