// ============================================================================
// Model & Provider Info
// ============================================================================

/// List available models from the sidecar
#[tauri::command]
pub async fn list_models(state: State<'_, AppState>) -> Result<Vec<ModelInfo>> {
    state.sidecar.list_models().await
}

/// List available providers from the sidecar
#[tauri::command]
pub async fn list_providers_from_sidecar(state: State<'_, AppState>) -> Result<Vec<ProviderInfo>> {
    state.sidecar.list_providers().await
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageStatus {
    pub canonical_root: String,
    pub legacy_root: String,
    pub migration_report_exists: bool,
    pub storage_version_exists: bool,
    pub migration_reason: Option<String>,
    pub migration_timestamp_ms: Option<u64>,
}

#[tauri::command]
pub fn get_storage_status() -> Result<StorageStatus> {
    let paths = resolve_shared_paths().map_err(|e| {
        TandemError::InvalidConfig(format!("Failed to resolve shared paths: {}", e))
    })?;

    let report_value = fs::read_to_string(&paths.migration_report_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok());

    let migration_reason = report_value
        .as_ref()
        .and_then(|v| v.get("reason"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let migration_timestamp_ms = report_value
        .as_ref()
        .and_then(|v| v.get("timestamp_ms"))
        .and_then(|v| v.as_u64());

    Ok(StorageStatus {
        canonical_root: paths.canonical_root.to_string_lossy().to_string(),
        legacy_root: paths.legacy_root.to_string_lossy().to_string(),
        migration_report_exists: paths.migration_report_path.exists(),
        storage_version_exists: paths.storage_version_path.exists(),
        migration_reason,
        migration_timestamp_ms,
    })
}

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageMigrationOptions {
    #[serde(default)]
    pub dry_run: bool,
    #[serde(default)]
    pub force: bool,
    #[serde(default)]
    pub include_workspace_scan: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageMigrationSource {
    pub id: String,
    pub path: String,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageMigrationStatus {
    pub canonical_root: String,
    pub migration_report_exists: bool,
    pub storage_version_exists: bool,
    pub migration_reason: Option<String>,
    pub migration_timestamp_ms: Option<u64>,
    pub migration_needed: bool,
    pub sources_detected: Vec<StorageMigrationSource>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageMigrationProgressEvent {
    pub phase: String,
    pub phase_percent: u8,
    pub overall_percent: u8,
    pub sessions_imported: u64,
    pub sessions_repaired: u64,
    pub messages_recovered: u64,
    pub parts_recovered: u64,
    pub conflicts_merged: u64,
    pub copied_count: usize,
    pub skipped_count: usize,
    pub error_count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct StorageMigrationRunResult {
    pub status: String,
    pub started_at_ms: u64,
    pub ended_at_ms: u64,
    pub duration_ms: u64,
    pub sources_detected: Vec<StorageMigrationSource>,
    pub copied: Vec<String>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
    pub sessions_imported: u64,
    pub sessions_repaired: u64,
    pub messages_recovered: u64,
    pub parts_recovered: u64,
    pub conflicts_merged: u64,
    pub tool_rows_upserted: u64,
    pub report_path: String,
    pub reason: String,
    pub dry_run: bool,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn detect_migration_sources() -> Vec<StorageMigrationSource> {
    let mut out = Vec::new();
    if let Ok(paths) = resolve_shared_paths() {
        out.push(StorageMigrationSource {
            id: "legacy_tandem_appdata".to_string(),
            path: paths.legacy_root.to_string_lossy().to_string(),
            exists: paths.legacy_root.exists(),
        });
    }
    if let Some(app_data) = dirs::data_dir() {
        let opencode_appdata = app_data.join("opencode");
        out.push(StorageMigrationSource {
            id: "opencode_appdata".to_string(),
            path: opencode_appdata.to_string_lossy().to_string(),
            exists: opencode_appdata.exists(),
        });
    }
    if let Some(home) = dirs::home_dir() {
        let opencode_local = home.join(".local").join("share").join("opencode");
        out.push(StorageMigrationSource {
            id: "opencode_local_share".to_string(),
            path: opencode_local.to_string_lossy().to_string(),
            exists: opencode_local.exists(),
        });
    }
    out
}

fn read_migration_report_value(paths: &tandem_core::SharedPaths) -> Option<serde_json::Value> {
    fs::read_to_string(&paths.migration_report_path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
}

#[tauri::command]
pub fn get_storage_migration_status() -> Result<StorageMigrationStatus> {
    let paths = resolve_shared_paths().map_err(|e| {
        TandemError::InvalidConfig(format!("Failed to resolve shared paths: {}", e))
    })?;
    let report_value = read_migration_report_value(&paths);
    let migration_reason = report_value
        .as_ref()
        .and_then(|v| v.get("reason"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    let migration_timestamp_ms = report_value
        .as_ref()
        .and_then(|v| v.get("timestamp_ms"))
        .and_then(|v| v.as_u64());
    let sources_detected = detect_migration_sources();
    let migration_needed = sources_detected.iter().any(|s| s.exists)
        && (!paths.migration_report_path.exists() || !paths.storage_version_path.exists());

    Ok(StorageMigrationStatus {
        canonical_root: paths.canonical_root.to_string_lossy().to_string(),
        migration_report_exists: paths.migration_report_path.exists(),
        storage_version_exists: paths.storage_version_path.exists(),
        migration_reason,
        migration_timestamp_ms,
        migration_needed,
        sources_detected,
    })
}

#[tauri::command]
pub async fn run_storage_migration(
    app: AppHandle,
    options: Option<StorageMigrationOptions>,
) -> Result<StorageMigrationRunResult> {
    let opts = options.unwrap_or(StorageMigrationOptions {
        dry_run: false,
        force: false,
        include_workspace_scan: false,
    });
    let started_at_ms = now_ms();
    let paths = resolve_shared_paths().map_err(|e| {
        TandemError::InvalidConfig(format!("Failed to resolve shared paths: {}", e))
    })?;
    let sources_detected = detect_migration_sources();

    let mut progress = StorageMigrationProgressEvent {
        phase: "scanning_sources".to_string(),
        phase_percent: 100,
        overall_percent: 10,
        sessions_imported: 0,
        sessions_repaired: 0,
        messages_recovered: 0,
        parts_recovered: 0,
        conflicts_merged: 0,
        copied_count: 0,
        skipped_count: 0,
        error_count: 0,
    };
    let _ = app.emit("storage-migration-progress", &progress);

    if opts.dry_run {
        let ended_at_ms = now_ms();
        let result = StorageMigrationRunResult {
            status: "success".to_string(),
            started_at_ms,
            ended_at_ms,
            duration_ms: ended_at_ms.saturating_sub(started_at_ms),
            sources_detected,
            copied: Vec::new(),
            skipped: Vec::new(),
            errors: Vec::new(),
            sessions_imported: 0,
            sessions_repaired: 0,
            messages_recovered: 0,
            parts_recovered: 0,
            conflicts_merged: 0,
            tool_rows_upserted: 0,
            report_path: paths.migration_report_path.to_string_lossy().to_string(),
            reason: "dry_run".to_string(),
            dry_run: true,
        };
        let _ = app.emit("storage-migration-complete", &result);
        return Ok(result);
    }

    progress.phase = "copying_secure_artifacts".to_string();
    progress.overall_percent = 35;
    let _ = app.emit("storage-migration-progress", &progress);

    let migration = migrate_legacy_storage_if_needed(&paths)
        .map_err(|e| TandemError::InvalidConfig(format!("Storage migration failed: {}", e)))?;

    progress.copied_count = migration.copied.len();
    progress.skipped_count = migration.skipped.len();
    progress.error_count = migration.errors.len();
    progress.phase = "rehydrating_chat_history".to_string();
    progress.overall_percent = 70;
    let _ = app.emit("storage-migration-progress", &progress);

    let storage_root = paths.engine_state_dir.join("storage");
    let storage = Storage::new(&storage_root)
        .await
        .map_err(|e| TandemError::InvalidConfig(format!("Storage open failed: {}", e)))?;
    let repair_stats: SessionRepairStats = storage
        .repair_sessions_from_file_store()
        .await
        .map_err(|e| TandemError::InvalidConfig(format!("Storage repair failed: {}", e)))?;
    let sessions_for_backfill = storage.list_sessions().await;
    let backfill = crate::tool_history::backfill_tool_executions_from_sessions(
        &app,
        &sessions_for_backfill,
    )
    .map_err(|e| TandemError::InvalidConfig(format!("Tool history backfill failed: {}", e)))?;

    progress.sessions_repaired = repair_stats.sessions_repaired;
    progress.messages_recovered = repair_stats.messages_recovered;
    progress.parts_recovered = repair_stats.parts_recovered;
    progress.conflicts_merged = repair_stats.conflicts_merged;
    progress.phase = "validating_and_finalizing".to_string();
    progress.overall_percent = 100;
    let _ = app.emit("storage-migration-progress", &progress);

    let status = if migration.errors.is_empty() {
        "success"
    } else {
        "partial"
    }
    .to_string();
    let ended_at_ms = now_ms();
    let result = StorageMigrationRunResult {
        status,
        started_at_ms,
        ended_at_ms,
        duration_ms: ended_at_ms.saturating_sub(started_at_ms),
        sources_detected,
        copied: migration.copied,
        skipped: migration.skipped,
        errors: migration.errors,
        sessions_imported: 0,
        sessions_repaired: repair_stats.sessions_repaired,
        messages_recovered: repair_stats.messages_recovered,
        parts_recovered: repair_stats.parts_recovered,
        conflicts_merged: repair_stats.conflicts_merged,
        tool_rows_upserted: backfill.tool_rows_upserted,
        report_path: paths.migration_report_path.to_string_lossy().to_string(),
        reason: migration.reason,
        dry_run: false,
    };
    let _ = app.emit("storage-migration-complete", &result);
    Ok(result)
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolHistoryBackfillResult {
    pub sessions_scanned: u64,
    pub tool_rows_upserted: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolHistoryBackfillStatus {
    pub tool_rows_total: u64,
    pub sessions_with_tool_rows: u64,
}

#[tauri::command]
pub async fn run_tool_history_backfill(app: AppHandle) -> Result<ToolHistoryBackfillResult> {
    let paths = resolve_shared_paths().map_err(|e| {
        TandemError::InvalidConfig(format!("Failed to resolve shared paths: {}", e))
    })?;
    let storage_root = paths.engine_state_dir.join("storage");
    let storage = Storage::new(&storage_root)
        .await
        .map_err(|e| TandemError::InvalidConfig(format!("Storage open failed: {}", e)))?;
    let sessions = storage.list_sessions().await;
    let stats = crate::tool_history::backfill_tool_executions_from_sessions(&app, &sessions)?;
    Ok(ToolHistoryBackfillResult {
        sessions_scanned: stats.sessions_scanned,
        tool_rows_upserted: stats.tool_rows_upserted,
    })
}

#[tauri::command]
pub fn get_tool_history_backfill_status(app: AppHandle) -> Result<ToolHistoryBackfillStatus> {
    let db_path = crate::tool_history::app_memory_db_path_for_commands(&app)?;
    let conn = rusqlite::Connection::open(db_path)
        .map_err(|e| TandemError::Memory(format!("open tool history db: {}", e)))?;
    let tool_rows_total: u64 = conn
        .query_row("SELECT COUNT(*) FROM tool_executions", [], |row| {
            row.get::<_, i64>(0)
        })
        .map(|v| u64::try_from(v).unwrap_or_default())
        .unwrap_or_default();
    let sessions_with_tool_rows: u64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT session_id) FROM tool_executions",
            [],
            |row| row.get::<_, i64>(0),
        )
        .map(|v| u64::try_from(v).unwrap_or_default())
        .unwrap_or_default();
    Ok(ToolHistoryBackfillStatus {
        tool_rows_total,
        sessions_with_tool_rows,
    })
}

// ============================================================================
// Logs (on-demand streaming)
// ============================================================================

#[derive(Debug, Clone, Serialize)]
pub struct LogStreamBatch {
    pub stream_id: String,
    pub source: String, // "tandem" | "sidecar"
    pub lines: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dropped: Option<u64>,
    pub ts_ms: u64,
}

#[tauri::command]
pub async fn list_app_log_files(app: AppHandle) -> Result<Vec<LogFileInfo>> {
    let app_data_dir = shared_app_data_dir(&app)?;
    let logs_dir = app_data_dir.join("logs");
    logs::list_log_files(&logs_dir)
}

#[tauri::command]
pub async fn start_log_stream(
    app: AppHandle,
    state: State<'_, AppState>,
    window_label: String,
    source: String,
    file_name: Option<String>,
    tail_lines: Option<u32>,
) -> Result<String> {
    let window = app
        .get_webview_window(&window_label)
        .ok_or_else(|| TandemError::InvalidConfig(format!("Window not found: {}", window_label)))?;

    let stream_id = format!("log_{}", Uuid::new_v4());
    let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel::<()>();

    state
        .active_log_streams
        .lock()
        .await
        .insert(stream_id.clone(), stop_tx);

    let tail_lines = tail_lines.unwrap_or(500).clamp(10, 5000) as usize;

    let stream_id_clone = stream_id.clone();
    let source_clone = source.clone();
    let sidecar = state.sidecar.clone();
    let active_map = state.active_log_streams.clone();

    tokio::spawn(async move {
        let source_kind = source_clone.clone();
        let stream_id_for_emit = stream_id_clone.clone();
        let source_for_emit = source_clone.clone();
        let window_for_emit = window.clone();

        let send_batch = |lines: Vec<String>, dropped: Option<u64>| {
            let window = window_for_emit.clone();
            let stream_id = stream_id_for_emit.clone();
            let source = source_for_emit.clone();
            async move {
                if lines.is_empty() {
                    return;
                }
                let payload = LogStreamBatch {
                    stream_id,
                    source,
                    lines,
                    dropped,
                    ts_ms: logs::now_ms(),
                };
                let _ = window.emit("log_stream_event", payload);
            }
        };

        match source_kind.as_str() {
            "sidecar" => {
                let (snap, dropped_total) = sidecar.sidecar_logs_snapshot(tail_lines);
                let mut last_seq = 0u64;
                let mut out = Vec::new();
                for (seq, text) in snap {
                    last_seq = last_seq.max(seq);
                    out.push(text);
                }
                send_batch(out, Some(dropped_total)).await;

                let mut tick = tokio::time::interval(Duration::from_millis(200));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                loop {
                    tokio::select! {
                        _ = &mut stop_rx => break,
                        _ = tick.tick() => {
                            let (lines, dropped_total) = sidecar.sidecar_logs_since(last_seq);
                            let mut out = Vec::new();
                            for (seq, text) in lines {
                                last_seq = last_seq.max(seq);
                                out.push(text);
                            }
                            for chunk in out.chunks(200) {
                                send_batch(chunk.to_vec(), Some(dropped_total)).await;
                            }
                        }
                    }
                }
            }
            "tandem" => {
                let app_data_dir = match shared_app_data_dir(&app) {
                    Ok(d) => d,
                    Err(e) => {
                        let _ = send_batch(
                            vec![format!("ERROR Failed to resolve app data dir: {e}")],
                            None,
                        )
                        .await;
                        let _ = active_map.lock().await.remove(&stream_id_clone);
                        return;
                    }
                };
                let logs_dir = app_data_dir.join("logs");

                let file_name = match file_name {
                    Some(n) => match logs::sanitize_log_file_name(&n) {
                        Ok(v) => v,
                        Err(e) => {
                            let _ = send_batch(vec![format!("ERROR {e}")], None).await;
                            let _ = active_map.lock().await.remove(&stream_id_clone);
                            return;
                        }
                    },
                    None => {
                        let _ =
                            send_batch(vec!["ERROR Missing log file name".to_string()], None).await;
                        let _ = active_map.lock().await.remove(&stream_id_clone);
                        return;
                    }
                };

                let path = logs::join_logs_dir(&logs_dir, &file_name);
                let (initial, mut offset) = match logs::tail_file(&path, tail_lines, 256 * 1024) {
                    Ok(v) => v,
                    Err(e) => {
                        let _ =
                            send_batch(vec![format!("ERROR Failed to open log: {e}")], None).await;
                        let _ = active_map.lock().await.remove(&stream_id_clone);
                        return;
                    }
                };
                send_batch(initial, None).await;

                let mut partial = String::new();
                let mut tick = tokio::time::interval(Duration::from_millis(200));
                tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

                loop {
                    tokio::select! {
                        _ = &mut stop_rx => break,
                        _ = tick.tick() => {
                            use std::io::{Read, Seek, SeekFrom};
                            let mut f = match std::fs::File::open(&path) {
                                Ok(f) => f,
                                Err(_) => continue,
                            };
                            let meta = match f.metadata() {
                                Ok(m) => m,
                                Err(_) => continue,
                            };
                            let len = meta.len();
                            if len < offset {
                                offset = 0;
                                partial.clear();
                            }
                            if f.seek(SeekFrom::Start(offset)).is_err() {
                                continue;
                            }
                            let mut buf = Vec::new();
                            if f.read_to_end(&mut buf).is_err() {
                                continue;
                            }
                            offset = len;
                            if buf.is_empty() {
                                continue;
                            }
                            partial.push_str(&String::from_utf8_lossy(&buf));

                            // Avoid borrowing issues by processing an owned copy of the buffer.
                            let data = std::mem::take(&mut partial);
                            let bytes = data.as_bytes();
                            let mut lines = Vec::new();
                            let mut start = 0usize;
                            for (i, b) in bytes.iter().enumerate() {
                                if *b == b'\n' {
                                    let mut slice = &data[start..i];
                                    if slice.ends_with('\r') {
                                        slice = &slice[..slice.len().saturating_sub(1)];
                                    }
                                    if !slice.is_empty() {
                                        lines.push(slice.to_string());
                                    }
                                    start = i + 1;
                                }
                            }
                            // Remainder (no trailing newline yet)
                            if start < data.len() {
                                partial = data[start..].to_string();
                            }
                            for chunk in lines.chunks(200) {
                                send_batch(chunk.to_vec(), None).await;
                            }
                        }
                    }
                }
            }
            other => {
                let _ = send_batch(vec![format!("ERROR Unknown log source: {other}")], None).await;
            }
        }

        // Best-effort cleanup
        active_map.lock().await.remove(&stream_id_clone);
    });

    Ok(stream_id)
}

#[tauri::command]
pub async fn stop_log_stream(state: State<'_, AppState>, stream_id: String) -> Result<()> {
    if let Some(tx) = state.active_log_streams.lock().await.remove(&stream_id) {
        let _ = tx.send(());
    }
    Ok(())
}

/// List models installed locally via Ollama
#[tauri::command]
pub async fn list_ollama_models() -> Result<Vec<ModelInfo>> {
    let output = Command::new("ollama").arg("list").output().map_err(|e| {
        TandemError::Sidecar(format!(
            "Failed to execute 'ollama list': {}. Is Ollama installed?",
            e
        ))
    })?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut models = Vec::new();

    // Skip header line
    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let name = parts[0].to_string();
        // Ollama names are often like 'llama3:latest' or just 'llama3'
        // We use the full name as the ID as well for simplify
        models.push(ModelInfo {
            id: name.clone(),
            name: name.clone(),
            provider: Some("ollama".to_string()),
            context_length: None,
        });
    }

    Ok(models)
}

/// List running Ollama models (ollama ps)
#[tauri::command]
pub async fn list_running_ollama_models() -> Result<Vec<ModelInfo>> {
    let output = Command::new("ollama")
        .arg("ps")
        .output()
        .map_err(|e| TandemError::Sidecar(format!("Failed to execute 'ollama ps': {}", e)))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut models = Vec::new();

    // Skip header line
    for line in stdout.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        let name = parts[0].to_string();
        models.push(ModelInfo {
            id: name.clone(),
            name: name.clone(),
            provider: Some("ollama".to_string()),
            context_length: None,
        });
    }

    Ok(models)
}

/// Stop a running Ollama model
#[tauri::command]
pub async fn stop_ollama_model(name: String) -> Result<()> {
    Command::new("ollama")
        .arg("stop")
        .arg(name)
        .output()
        .map_err(|e| TandemError::Sidecar(format!("Failed to execute 'ollama stop': {}", e)))?;
    Ok(())
}

/// Run (load) an Ollama model
#[tauri::command]
pub async fn run_ollama_model(name: String) -> Result<()> {
    // We run with an empty prompt to just trigger loading
    Command::new("ollama")
        .arg("run")
        .arg(name)
        .arg("")
        .output()
        .map_err(|e| TandemError::Sidecar(format!("Failed to execute 'ollama run': {}", e)))?;
    Ok(())
}
