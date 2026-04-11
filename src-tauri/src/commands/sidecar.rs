// ============================================================================
// Sidecar Management
// ============================================================================

/// Start the tandem-engine sidecar
#[tauri::command]
pub async fn start_sidecar(app: AppHandle, state: State<'_, AppState>) -> Result<u16> {
    start_sidecar_inner(&app, state.inner()).await
}

async fn start_sidecar_inner(app: &AppHandle, state: &AppState) -> Result<u16> {
    let initial_state = state.sidecar.state().await;
    if initial_state == SidecarState::Running {
        state
            .stream_hub
            .start(app.clone(), state.sidecar.clone())
            .await?;
        crate::sidecar::emit_desktop_startup_progress(
            Some(app),
            crate::sidecar::DesktopStartupStatus::Ready,
            "sidecar_ready",
            "Tandem engine ready",
            100,
            Some("Sidecar already running".to_string()),
        );
        return state.sidecar.port().await.ok_or_else(|| {
            TandemError::Sidecar("Sidecar running but no port assigned".to_string())
        });
    }
    if initial_state == SidecarState::Starting {
        crate::sidecar::emit_desktop_startup_progress(
            Some(app),
            crate::sidecar::DesktopStartupStatus::Starting,
            "sidecar_waiting",
            "Waiting for Tandem engine",
            72,
            Some("Another startup is already in progress".to_string()),
        );
        // Another caller is already starting it; wait briefly for port assignment.
        for _ in 0..10 {
            tokio::time::sleep(Duration::from_millis(200)).await;
            if state.sidecar.state().await == SidecarState::Running {
                state
                    .stream_hub
                    .start(app.clone(), state.sidecar.clone())
                    .await?;
                crate::sidecar::emit_desktop_startup_progress(
                    Some(app),
                    crate::sidecar::DesktopStartupStatus::Ready,
                    "sidecar_ready",
                    "Tandem engine ready",
                    100,
                    Some("Startup completed by another caller".to_string()),
                );
                return state.sidecar.port().await.ok_or_else(|| {
                    TandemError::Sidecar("Sidecar running but no port assigned".to_string())
                });
            }
        }
    }

    // Get the sidecar path (checks AppData first, then resources)
    let sidecar_path = sidecar_manager::get_sidecar_binary_path(app)?;

    // Set workspace path on sidecar - clone before await
    let workspace_path = {
        let workspace = state.workspace_path.read().unwrap();
        workspace.clone()
    };
    if let Some(path) = workspace_path {
        state.sidecar.set_workspace(path).await;
    }

    // Get and set API keys as environment variables
    let providers = {
        let config = state.providers_config.read().unwrap();
        config.clone()
    };

    sync_provider_config_file(&providers)?;

    // Configure Ollama endpoint env (local models)
    sync_ollama_env(&state, &providers).await;

    // Set/remove API keys based on enabled providers.
    // (Important: remove_env only applies after restart, but we call this before start().)
    sync_provider_keys_env(app, state, &providers).await;
    sync_channel_tokens_env(app, state).await;

    // Start the sidecar
    state
        .sidecar
        .start_with_app(Some(app.clone()), sidecar_path.to_string_lossy().as_ref())
        .await?;

    // Push runtime-only provider auth immediately after sidecar startup.
    sync_provider_keys_runtime_auth(app, state, &providers).await;

    if let Ok(health) = state.sidecar.startup_health().await {
        let expected_build_id = expected_engine_build_id();
        let selected_binary = sidecar_path.to_string_lossy().to_string();
        let mut mismatch_reason: Option<String> = None;

        if let Some(actual_build_id) = health.build_id.clone() {
            if !expected_build_id.is_empty() && actual_build_id != expected_build_id {
                mismatch_reason = Some(format!(
                    "build_id mismatch expected={} actual={}",
                    expected_build_id, actual_build_id
                ));
            }
        }
        if mismatch_reason.is_none() {
            if let Some(actual_binary) = health.binary_path.clone() {
                if !same_binary_path(&selected_binary, &actual_binary) {
                    mismatch_reason = Some(format!(
                        "binary_path mismatch selected={} running={}",
                        selected_binary, actual_binary
                    ));
                }
            }
        }

        if let Some(reason) = mismatch_reason {
            let _ = app.emit(
                "sidecar-binary-mismatch",
                serde_json::json!({
                    "warning": "Running stale engine binary",
                    "reason": reason,
                    "selectedBinary": selected_binary,
                    "buildIDExpected": expected_build_id,
                    "buildIDActual": health.build_id,
                    "binaryPathActual": health.binary_path
                }),
            );
            emit_event(
                tracing::Level::WARN,
                ProcessKind::Desktop,
                ObservabilityEvent {
                    event: "sidecar.binary.mismatch",
                    component: "tauri.commands",
                    correlation_id: None,
                    session_id: None,
                    run_id: None,
                    message_id: None,
                    provider_id: None,
                    model_id: None,
                    status: Some("degraded"),
                    error_code: Some("STALE_ENGINE_BINARY"),
                    detail: Some("sidecar /global/health build/path mismatch detected"),
                },
            );
        }
    }

    state
        .stream_hub
        .start(app.clone(), state.sidecar.clone())
        .await?;

    let diagnostics_sidecar = state.sidecar.clone();
    tokio::spawn(async move {
        log_sidecar_catalog_diagnostics(diagnostics_sidecar).await;
    });

    // Return the port
    state
        .sidecar
        .port()
        .await
        .ok_or_else(|| TandemError::Sidecar("Sidecar started but no port assigned".to_string()))
}

async fn log_sidecar_catalog_diagnostics(sidecar: Arc<crate::sidecar::SidecarManager>) {
    match tokio::time::timeout(Duration::from_secs(5), sidecar.list_providers()).await {
        Ok(Ok(providers)) => {
            let provider_list: Vec<String> = providers
                .iter()
                .map(|p| format!("{} ({})", p.id, p.name))
                .collect();
            tracing::debug!("Sidecar providers: {}", provider_list.join(", "));
        }
        Ok(Err(e)) => {
            tracing::warn!("Failed to list sidecar providers: {}", e);
        }
        Err(_) => {
            tracing::warn!("Timed out while listing sidecar providers");
        }
    }

    match tokio::time::timeout(Duration::from_secs(5), sidecar.list_models()).await {
        Ok(Ok(models)) => {
            let openrouter_count = models
                .iter()
                .filter(|m| m.provider.as_deref() == Some("openrouter"))
                .count();
            tracing::debug!(
                "Sidecar models: total={} openrouter={}",
                models.len(),
                openrouter_count
            );
        }
        Ok(Err(e)) => {
            tracing::warn!("Failed to list sidecar models: {}", e);
        }
        Err(_) => {
            tracing::warn!("Timed out while listing sidecar models");
        }
    }
}

fn expected_engine_build_id() -> String {
    if let Some(explicit) = option_env!("TANDEM_BUILD_ID") {
        let trimmed = explicit.trim();
        if !trimmed.is_empty() {
            return trimmed.to_string();
        }
    }
    if let Some(git_sha) = option_env!("VERGEN_GIT_SHA") {
        let trimmed = git_sha.trim();
        if !trimmed.is_empty() {
            return format!("{}+{}", env!("CARGO_PKG_VERSION"), trimmed);
        }
    }
    env!("CARGO_PKG_VERSION").to_string()
}

fn same_binary_path(selected: &str, running: &str) -> bool {
    let selected_norm = selected.replace('\\', "/").to_ascii_lowercase();
    let running_norm = running.replace('\\', "/").to_ascii_lowercase();
    selected_norm == running_norm
}

/// Stop the tandem-engine sidecar
#[tauri::command]
pub async fn stop_sidecar(state: State<'_, AppState>) -> Result<()> {
    state.stream_hub.stop().await;
    state.sidecar.stop().await
}

/// Get the sidecar status
#[tauri::command]
pub async fn get_sidecar_status(state: State<'_, AppState>) -> Result<SidecarState> {
    Ok(state.sidecar.state().await)
}

#[tauri::command]
pub async fn get_sidecar_startup_health(
    state: State<'_, AppState>,
) -> Result<Option<crate::sidecar::SidecarStartupHealth>> {
    let sidecar_state = state.sidecar.state().await;
    if matches!(sidecar_state, SidecarState::Stopped | SidecarState::Failed) {
        return Ok(None);
    }
    match state.sidecar.startup_health().await {
        Ok(health) => Ok(Some(health)),
        Err(err) => {
            tracing::debug!("get_sidecar_startup_health unavailable: {}", err);
            Ok(None)
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeDiagnostics {
    pub sidecar: crate::sidecar::SidecarRuntimeSnapshot,
    pub stream: crate::stream_hub::StreamRuntimeSnapshot,
    pub lease_count: usize,
    pub logging: RuntimeLoggingDiagnostics,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeLoggingDiagnostics {
    pub initialized: bool,
    pub process: String,
    pub active_files: Vec<String>,
    pub last_write_ts_ms: Option<u64>,
    pub dropped_events: u64,
}

#[tauri::command]
pub async fn get_runtime_diagnostics(state: State<'_, AppState>) -> Result<RuntimeDiagnostics> {
    let sidecar = state.sidecar.runtime_snapshot().await;
    let stream = state.stream_hub.runtime_snapshot().await;
    let leases = state.engine_leases.lock().await;
    let logging = match resolve_shared_paths() {
        Ok(paths) => {
            let logs_dir = paths.canonical_root.join("logs");
            let files = logs::list_log_files(&logs_dir).unwrap_or_default();
            let active_files = files.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
            let last_write_ts_ms = files.iter().map(|f| f.modified_ms).max();
            RuntimeLoggingDiagnostics {
                initialized: true,
                process: "desktop".to_string(),
                active_files,
                last_write_ts_ms,
                dropped_events: 0,
            }
        }
        Err(_) => RuntimeLoggingDiagnostics {
            initialized: false,
            process: "desktop".to_string(),
            active_files: Vec::new(),
            last_write_ts_ms: None,
            dropped_events: 0,
        },
    };
    Ok(RuntimeDiagnostics {
        sidecar,
        stream,
        lease_count: leases.len(),
        logging,
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineApiTokenInfo {
    pub token_masked: String,
    pub token: Option<String>,
    pub path: String,
    pub storage_backend: String,
}

fn mask_engine_token(token: &str) -> String {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return "****".to_string();
    }
    if trimmed.len() <= 8 {
        return "****".to_string();
    }
    format!("{}****{}", &trimmed[..4], &trimmed[trimmed.len() - 4..])
}

#[tauri::command]
pub async fn get_engine_api_token(
    state: State<'_, AppState>,
    reveal: Option<bool>,
) -> Result<EngineApiTokenInfo> {
    let token = state.sidecar.api_token();
    let masked = mask_engine_token(&token);
    let path = state.sidecar.api_token_path().to_string_lossy().to_string();
    Ok(EngineApiTokenInfo {
        token_masked: masked,
        token: if reveal.unwrap_or(false) {
            Some(token)
        } else {
            None
        },
        path,
        storage_backend: state.sidecar.api_token_backend(),
    })
}

#[derive(Debug, Clone, Serialize)]
pub struct EngineLeaseInfo {
    pub lease_id: String,
    pub client_id: String,
    pub client_type: String,
    pub acquired_at_ms: u64,
    pub last_renewed_at_ms: u64,
    pub ttl_ms: u64,
}

fn prune_expired_leases(
    leases: &mut std::collections::HashMap<String, crate::state::EngineLease>,
    now: u64,
) {
    leases.retain(|_, lease| now.saturating_sub(lease.last_renewed_at_ms) <= lease.ttl_ms);
}

#[tauri::command]
pub async fn engine_acquire_lease(
    state: State<'_, AppState>,
    client_id: String,
    client_type: String,
    ttl_ms: Option<u64>,
) -> Result<EngineLeaseInfo> {
    let ttl_ms = ttl_ms.unwrap_or(45_000).clamp(10_000, 600_000);
    let now = now_ms();
    let lease_id = Uuid::new_v4().to_string();

    let mut leases = state.engine_leases.lock().await;
    prune_expired_leases(&mut leases, now);
    let lease = crate::state::EngineLease {
        lease_id: lease_id.clone(),
        client_id: client_id.clone(),
        client_type: client_type.clone(),
        acquired_at_ms: now,
        last_renewed_at_ms: now,
        ttl_ms,
    };
    leases.insert(lease_id.clone(), lease.clone());
    Ok(EngineLeaseInfo {
        lease_id,
        client_id,
        client_type,
        acquired_at_ms: lease.acquired_at_ms,
        last_renewed_at_ms: lease.last_renewed_at_ms,
        ttl_ms: lease.ttl_ms,
    })
}

#[tauri::command]
pub async fn engine_renew_lease(state: State<'_, AppState>, lease_id: String) -> Result<bool> {
    let now = now_ms();
    let mut leases = state.engine_leases.lock().await;
    prune_expired_leases(&mut leases, now);
    if let Some(lease) = leases.get_mut(&lease_id) {
        lease.last_renewed_at_ms = now;
        return Ok(true);
    }
    Ok(false)
}

#[tauri::command]
pub async fn engine_release_lease(state: State<'_, AppState>, lease_id: String) -> Result<bool> {
    let now = now_ms();
    let mut leases = state.engine_leases.lock().await;
    prune_expired_leases(&mut leases, now);
    let removed = leases.remove(&lease_id).is_some();
    let empty = leases.is_empty();
    drop(leases);

    // Shared-engine behavior: if no clients remain, stop sidecar + stream hub.
    if empty {
        state.stream_hub.stop().await;
        let _ = state.sidecar.stop().await;
    }
    Ok(removed)
}
