// Memory Management
// ============================================================================

/// Get statistics about the vector database memory usage
#[tauri::command]
pub async fn get_memory_stats(state: State<'_, AppState>) -> Result<MemoryStats> {
    if let Some(manager) = &state.memory_manager {
        manager
            .get_stats()
            .await
            .map_err(|e| TandemError::Memory(e.to_string()))
    } else {
        Err(TandemError::Memory(
            "Memory manager not initialized".to_string(),
        ))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct MemorySettings {
    pub auto_index_on_project_load: bool,
    #[serde(default = "default_memory_embedding_status")]
    pub embedding_status: String,
    #[serde(default)]
    pub embedding_reason: Option<String>,
}

#[tauri::command]
pub fn get_memory_settings(app: AppHandle, state: State<'_, AppState>) -> MemorySettings {
    let mut settings = MemorySettings {
        auto_index_on_project_load: true,
        embedding_status: default_memory_embedding_status(),
        embedding_reason: None,
    };

    if let Ok(store) = app.store("settings.json") {
        if let Some(value) = store.get("memory_auto_index_on_project_load") {
            if let Some(b) = value.as_bool() {
                settings.auto_index_on_project_load = b;
            }
        }
    }

    let embedding_health = if let Some(manager) = &state.memory_manager {
        tauri::async_runtime::block_on(manager.embedding_health())
    } else {
        EmbeddingHealth {
            status: "unavailable".to_string(),
            reason: Some("memory manager not initialized".to_string()),
        }
    };
    settings.embedding_status = embedding_health.status;
    settings.embedding_reason = embedding_health.reason;

    settings
}

fn default_memory_embedding_status() -> String {
    "unknown".to_string()
}

#[tauri::command]
pub fn set_memory_settings(app: AppHandle, settings: MemorySettings) -> Result<()> {
    if let Ok(store) = app.store("settings.json") {
        store.set(
            "memory_auto_index_on_project_load",
            serde_json::json!(settings.auto_index_on_project_load),
        );
        let _ = store.save();
    }
    Ok(())
}

#[tauri::command]
pub async fn get_project_memory_stats(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<ProjectMemoryStats> {
    if let Some(manager) = &state.memory_manager {
        manager
            .db()
            .get_project_stats(&project_id)
            .await
            .map_err(|e| TandemError::Memory(e.to_string()))
    } else {
        Err(TandemError::Memory(
            "Memory manager not initialized".to_string(),
        ))
    }
}

#[tauri::command]
pub async fn clear_project_file_index(
    state: State<'_, AppState>,
    project_id: String,
    vacuum: bool,
) -> Result<ClearFileIndexResult> {
    if let Some(manager) = &state.memory_manager {
        manager
            .db()
            .clear_project_file_index(&project_id, vacuum)
            .await
            .map_err(|e| TandemError::Memory(e.to_string()))
    } else {
        Err(TandemError::Memory(
            "Memory manager not initialized".to_string(),
        ))
    }
}

/// Index the current workspace
#[tauri::command]
pub async fn index_workspace_command(
    app: AppHandle,
    state: State<'_, AppState>,
    project_id: String,
) -> Result<IndexingStats> {
    let correlation_id = Uuid::new_v4().to_string();
    emit_event(
        tracing::Level::INFO,
        ProcessKind::Desktop,
        ObservabilityEvent {
            event: "index.workspace.start",
            component: "tauri.commands",
            correlation_id: Some(&correlation_id),
            session_id: None,
            run_id: None,
            message_id: None,
            provider_id: None,
            model_id: None,
            status: Some("start"),
            error_code: None,
            detail: Some("index_workspace_command"),
        },
    );
    if let Some(manager) = &state.memory_manager {
        let workspace_path = state
            .get_workspace_path()
            .ok_or_else(|| TandemError::IoError("No workspace selected".to_string()))?;
        match index_workspace(&app, &workspace_path, &project_id, manager).await {
            Ok(stats) => {
                emit_event(
                    tracing::Level::INFO,
                    ProcessKind::Desktop,
                    ObservabilityEvent {
                        event: "index.workspace.complete",
                        component: "tauri.commands",
                        correlation_id: Some(&correlation_id),
                        session_id: None,
                        run_id: None,
                        message_id: None,
                        provider_id: None,
                        model_id: None,
                        status: Some("ok"),
                        error_code: None,
                        detail: Some("index complete"),
                    },
                );
                Ok(stats)
            }
            Err(err) => {
                emit_event(
                    tracing::Level::ERROR,
                    ProcessKind::Desktop,
                    ObservabilityEvent {
                        event: "index.workspace.failed",
                        component: "tauri.commands",
                        correlation_id: Some(&correlation_id),
                        session_id: None,
                        run_id: None,
                        message_id: None,
                        provider_id: None,
                        model_id: None,
                        status: Some("failed"),
                        error_code: Some("INDEX_WORKSPACE_FAILED"),
                        detail: Some("index command failed"),
                    },
                );
                Err(err)
            }
        }
    } else {
        Err(TandemError::Memory(
            "Memory manager not initialized".to_string(),
        ))
    }
}

/// Unlock an existing vault with a PIN
#[tauri::command]
pub async fn unlock_vault(
    app: AppHandle,
    vault_state: State<'_, VaultState>,
    state: State<'_, AppState>,
    pin: String,
) -> Result<()> {
    // Check if vault exists
    if !vault::vault_exists(&vault_state.app_data_dir) {
        return Err(TandemError::Vault(
            "No vault exists. Create one first.".to_string(),
        ));
    }

    // Check if already unlocked
    if vault_state.is_unlocked() {
        return Ok(());
    }

    // Load encrypted key
    let vault_key_path = vault::get_vault_key_path(&vault_state.app_data_dir);
    let encrypted_key = EncryptedVaultKey::load(&vault_key_path)?;

    // Decrypt master key (this validates the PIN)
    let master_key = encrypted_key.decrypt(&pin)?;

    tracing::info!("Vault unlocked successfully");

    // Store master key and mark as unlocked
    vault_state.set_master_key(master_key.clone());

    crate::sidecar::emit_desktop_startup_progress(
        Some(&app),
        crate::sidecar::DesktopStartupStatus::Starting,
        "vault_unlocked",
        "Vault unlocked",
        12,
        Some("Master key restored".to_string()),
    );

    // Initialize keystore in the background so vault unlock can return immediately.
    initialize_keystore_after_unlock(app.clone(), master_key.clone());

    // Start the sidecar as part of lock-screen unlock flow.
    // Startup failures must not block vault unlock.
    let _ = state;
    spawn_sidecar_start_in_background(app.clone());

    Ok(())
}

/// Lock the vault (clears master key from memory)
#[tauri::command]
pub fn lock_vault(vault_state: State<'_, VaultState>) -> Result<()> {
    vault_state.lock();
    tracing::info!("Vault locked");
    Ok(())
}

fn resolve_default_model_spec(config: &ProvidersConfig) -> Option<ModelSpec> {
    // Strict routing: only use the explicit selected model/provider.
    // Do not silently fall back to enabled/default provider slots.
    if let Some(sel) = &config.selected_model {
        let provider_id = if sel.provider_id == "opencode_zen" {
            // Back-compat: frontend uses "opencode_zen", sidecar expects "opencode".
            "opencode".to_string()
        } else {
            sel.provider_id.clone()
        };

        if !provider_id.trim().is_empty() && !sel.model_id.trim().is_empty() {
            return Some(ModelSpec {
                provider_id,
                model_id: sel.model_id.clone(),
            });
        }
    }
    None
}

fn resolve_required_model_spec(
    config: &ProvidersConfig,
    model: Option<String>,
    provider: Option<String>,
    context: &str,
) -> Result<ModelSpec> {
    let explicit_provider = normalize_provider_id_for_sidecar(provider)
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());
    let explicit_model = model
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty());

    match (explicit_provider, explicit_model) {
        (Some(provider_id), Some(model_id)) => Ok(ModelSpec {
            provider_id,
            model_id,
        }),
        (Some(_), None) | (None, Some(_)) => Err(TandemError::InvalidConfig(format!(
            "{} requires both provider and model to be set together.",
            context
        ))),
        (None, None) => resolve_default_model_spec(config).ok_or_else(|| {
            TandemError::InvalidConfig(format!(
                "{} could not resolve a model/provider. Select one in the model picker.",
                context
            ))
        }),
    }
}

fn resolve_default_provider_and_model(
    config: &ProvidersConfig,
) -> (Option<String>, Option<String>) {
    if let Some(sel) = &config.selected_model {
        let provider_id = if sel.provider_id == "opencode_zen" {
            "opencode".to_string()
        } else {
            sel.provider_id.clone()
        };

        if !provider_id.trim().is_empty() && !sel.model_id.trim().is_empty() {
            return (Some(provider_id), Some(sel.model_id.clone()));
        }
    }

    let candidates: Vec<(&str, &crate::state::ProviderConfig)> = vec![
        ("openrouter", &config.openrouter),
        ("opencode", &config.opencode_zen), // OpenCode expects "opencode" not "opencode_zen"
        ("anthropic", &config.anthropic),
        ("openai", &config.openai),
        ("llama_cpp", &config.llama_cpp),
        ("ollama", &config.ollama),
        ("poe", &config.poe),
    ];
    let custom_default = config.custom.iter().find(|c| c.enabled && c.default);
    let custom_enabled = config.custom.iter().find(|c| c.enabled);

    if let Some((provider_id, provider)) = candidates
        .iter()
        .find(|(_, p)| p.enabled && p.default)
        .map(|(id, p)| (*id, *p))
    {
        return (Some(provider_id.to_string()), provider.model.clone());
    }

    if let Some(provider) = custom_default {
        return (Some("custom".to_string()), provider.model.clone());
    }

    for (provider_id, provider) in candidates {
        if provider.enabled {
            return (Some(provider_id.to_string()), provider.model.clone());
        }
    }

    if let Some(provider) = custom_enabled {
        return (Some("custom".to_string()), provider.model.clone());
    }

    (None, None)
}

fn selected_provider_slot(config: &ProvidersConfig) -> Option<&'static str> {
    let provider_id = config
        .selected_model
        .as_ref()?
        .provider_id
        .trim()
        .to_lowercase();
    match provider_id.as_str() {
        "openrouter" => Some("openrouter"),
        "openai" => Some("openai"),
        "anthropic" => Some("anthropic"),
        "poe" => Some("poe"),
        "opencode" | "opencode_zen" | "zen" => Some("opencode_zen"),
        "llama_cpp" | "llama.cpp" => Some("llama_cpp"),
        "ollama" => Some("ollama"),
        "custom" => Some("custom"),
        _ => None,
    }
}

fn provider_slot_active(config: &ProvidersConfig, slot: &str) -> bool {
    let selected_slot = selected_provider_slot(config);
    let selected_active = selected_slot.is_some_and(|s| s == slot);
    match slot {
        "openrouter" => config.openrouter.enabled || selected_active,
        "opencode_zen" => config.opencode_zen.enabled || selected_active,
        "anthropic" => config.anthropic.enabled || selected_active,
        "openai" => config.openai.enabled || selected_active,
        "llama_cpp" => config.llama_cpp.enabled || selected_active,
        "poe" => config.poe.enabled || selected_active,
        "ollama" => config.ollama.enabled || selected_active,
        "custom" => config.custom.iter().any(|provider| provider.enabled) || selected_active,
        _ => selected_active,
    }
}

async fn validate_model_provider_auth_if_required(
    app: &AppHandle,
    config: &ProvidersConfig,
    model: Option<&str>,
    provider: Option<&str>,
) -> Result<()> {
    let provider_id = provider.map(|p| p.trim().to_lowercase()).or_else(|| {
        if model.is_some() {
            resolve_default_provider_and_model(config)
                .0
                .map(|p| p.trim().to_lowercase())
        } else {
            None
        }
    });

    let Some(provider_id) = provider_id else {
        return Ok(());
    };

    let key_type = match provider_id.as_str() {
        "openrouter" => Some("openrouter"),
        "openai" => Some("openai"),
        "anthropic" => Some("anthropic"),
        "poe" => Some("poe"),
        _ => None,
    };

    let Some(key_type) = key_type else {
        return Ok(());
    };

    let has_key = get_api_key(app, key_type)
        .await
        .ok()
        .flatten()
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);

    if !has_key {
        return Err(TandemError::InvalidConfig(format!(
            "Provider '{}' is selected but no API key is configured. Add the key in Settings > Providers.",
            provider_id
        )));
    }

    Ok(())
}

async fn wait_for_sidecar_api_ready(state: &AppState, timeout: Duration) -> Result<()> {
    let started = Instant::now();
    loop {
        let sidecar_state = state.sidecar.state().await;
        match sidecar_state {
            SidecarState::Running => match state.sidecar.startup_health().await {
                Ok(health) => {
                    if health.ready {
                        return Ok(());
                    }
                    if started.elapsed() >= timeout {
                        return Err(TandemError::Sidecar(format!(
                            "Engine still starting: phase={} attempt_id={} elapsed_ms={}",
                            health.phase, health.startup_attempt_id, health.startup_elapsed_ms
                        )));
                    }
                }
                Err(_) => {
                    // Older engine builds may not expose /global/health consistently; if sidecar
                    // reports running, allow request path retries to handle transient readiness.
                    return Ok(());
                }
            },
            SidecarState::Starting => {
                if started.elapsed() >= timeout {
                    return Err(TandemError::Sidecar(
                        "Engine is still starting; please retry in a moment.".to_string(),
                    ));
                }
            }
            SidecarState::Stopped | SidecarState::Failed | SidecarState::Stopping => {
                return Err(TandemError::Sidecar(format!(
                    "Engine is not ready (state={:?}). Start/reconnect the engine and retry.",
                    sidecar_state
                )));
            }
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

async fn validate_model_provider_in_sidecar_catalog(
    state: &AppState,
    model: Option<&str>,
    provider: Option<&str>,
) -> Result<()> {
    let model = model.map(str::trim).filter(|m| !m.is_empty());
    let provider = provider.map(str::trim).filter(|p| !p.is_empty());

    match (model, provider) {
        (None, None) => return Ok(()),
        (Some(_), None) | (None, Some(_)) => {
            return Err(TandemError::InvalidConfig(
                "Model/provider selection is incomplete. Select both a provider and model."
                    .to_string(),
            ));
        }
        _ => {}
    }

    let model = model.unwrap();
    let provider = provider.unwrap().to_lowercase();
    if provider == "custom" || provider == "llama_cpp" || provider == "llama.cpp" {
        return Ok(());
    }

    let models = state.sidecar.list_models().await.map_err(|e| {
        TandemError::Sidecar(format!(
            "Failed to validate selected model/provider against sidecar catalog: {}",
            e
        ))
    })?;

    let provider_models: Vec<&crate::sidecar::ModelInfo> = models
        .iter()
        .filter(|m| {
            m.provider
                .as_deref()
                .map(|p| p.eq_ignore_ascii_case(provider.as_str()))
                .unwrap_or(false)
        })
        .collect();

    if provider_models.is_empty() {
        return Err(TandemError::InvalidConfig(format!(
            "Selected provider '{}' is not available in the current engine catalog.",
            provider
        )));
    }

    let exact_match = provider_models.iter().any(|m| m.id == model);
    let name_match = provider_models.iter().any(|m| m.name == model);
    if !exact_match && !name_match {
        let examples = provider_models
            .iter()
            .take(8)
            .map(|m| m.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(TandemError::InvalidConfig(format!(
            "Model '{}' is not available for provider '{}'. Available examples: {}",
            model, provider, examples
        )));
    }

    Ok(())
}

fn resolve_effective_mode(
    app: &AppHandle,
    state: &AppState,
    mode_id: Option<&str>,
    legacy_agent: Option<&str>,
) -> Result<ModeResolution> {
    crate::modes::resolve_mode_for_request(
        app,
        state.get_workspace_path().as_deref(),
        mode_id,
        legacy_agent,
    )
}

fn set_session_mode(state: &AppState, session_id: &str, mode: ResolvedMode) {
    let mut guard = state.session_modes.write().unwrap();
    guard.insert(session_id.to_string(), mode);
}

fn get_session_mode(state: &AppState, session_id: &str) -> Option<ResolvedMode> {
    let guard = state.session_modes.read().unwrap();
    guard.get(session_id).cloned()
}

fn sidecar_permissions_for_mode(
    mode: &ResolvedMode,
) -> Option<Vec<crate::sidecar::PermissionRule>> {
    let permissions = crate::modes::build_permission_rules(mode);
    if permissions.is_empty() {
        None
    } else {
        Some(permissions)
    }
}

fn env_var_for_key(key_type: &ApiKeyType) -> Option<&'static str> {
    match key_type {
        ApiKeyType::OpenRouter => Some("OPENROUTER_API_KEY"),
        ApiKeyType::OpenCodeZen => Some("OPENCODE_ZEN_API_KEY"),
        ApiKeyType::Anthropic => Some("ANTHROPIC_API_KEY"),
        ApiKeyType::OpenAI => Some("OPENAI_API_KEY"),
        ApiKeyType::Poe => Some("POE_API_KEY"),
        ApiKeyType::BraveSearch => Some("TANDEM_BRAVE_SEARCH_API_KEY"),
        ApiKeyType::ExaSearch => Some("TANDEM_EXA_API_KEY"),
        ApiKeyType::Custom(_) => None,
    }
}

const CHANNEL_NAMES: [&str; 3] = ["telegram", "discord", "slack"];

fn normalize_channel_name(raw: &str) -> Result<&'static str> {
    let normalized = raw.trim().to_ascii_lowercase();
    match normalized.as_str() {
        "telegram" => Ok("telegram"),
        "discord" => Ok("discord"),
        "slack" => Ok("slack"),
        _ => Err(TandemError::InvalidConfig(format!(
            "Unsupported channel: {}",
            raw
        ))),
    }
}

fn channel_token_env_var(channel: &str) -> &'static str {
    match channel {
        "telegram" => "TANDEM_TELEGRAM_BOT_TOKEN",
        "discord" => "TANDEM_DISCORD_BOT_TOKEN",
        "slack" => "TANDEM_SLACK_BOT_TOKEN",
        _ => "TANDEM_CHANNEL_BOT_TOKEN",
    }
}

fn channel_token_storage_key(project_id: &str, channel: &str) -> String {
    format!("channel::{project_id}::{channel}::bot_token")
}

fn active_project_id(state: &AppState) -> Result<String> {
    state
        .active_project_id
        .read()
        .unwrap()
        .clone()
        .ok_or_else(|| {
            TandemError::InvalidConfig(
                "No active project selected for channel connections".to_string(),
            )
        })
}

fn workspace_channel_enabled(workspace_path: &Path, channel: &str) -> bool {
    let config_path = workspace_path.join(".tandem").join("config.json");
    let Ok(raw) = fs::read_to_string(config_path) else {
        return false;
    };
    let Ok(root) = serde_json::from_str::<serde_json::Value>(&raw) else {
        return false;
    };
    root.get("channels")
        .and_then(serde_json::Value::as_object)
        .is_some_and(|channels| channels.contains_key(channel))
}

/// Check if a file operation should be auto-approved based on path
/// Auto-approve writes to .tandem/plans/ (canonical) and legacy .opencode/plans/.
fn workspace_plans_dirs(workspace_path: &Path) -> (PathBuf, PathBuf) {
    (
        workspace_path.join(".tandem").join("plans"),
        workspace_path.join(".opencode").join("plans"),
    )
}

fn workspace_skill_dirs(workspace_path: &Path) -> (PathBuf, PathBuf) {
    (
        workspace_path.join(".tandem").join("skill"),
        workspace_path.join(".opencode").join("skill"),
    )
}

fn normalize_path_for_match(path: &str) -> String {
    path.replace('\\', "/")
}

fn copy_workspace_tree_if_missing(src: &Path, dst: &Path) -> Result<(usize, usize)> {
    if !src.exists() {
        return Ok((0, 0));
    }
    fs::create_dir_all(dst).map_err(TandemError::Io)?;
    let mut copied = 0usize;
    let mut skipped = 0usize;

    for entry in fs::read_dir(src).map_err(TandemError::Io)? {
        let entry = entry.map_err(TandemError::Io)?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let ty = entry.file_type().map_err(TandemError::Io)?;
        if ty.is_dir() {
            let (c, s) = copy_workspace_tree_if_missing(&src_path, &dst_path)?;
            copied += c;
            skipped += s;
        } else if ty.is_file() {
            if dst_path.exists() {
                skipped += 1;
                continue;
            }
            if let Some(parent) = dst_path.parent() {
                fs::create_dir_all(parent).map_err(TandemError::Io)?;
            }
            fs::copy(&src_path, &dst_path).map_err(TandemError::Io)?;
            copied += 1;
        }
    }

    Ok((copied, skipped))
}

pub(crate) fn migrate_workspace_legacy_namespace_if_needed(workspace_path: &Path) -> Result<()> {
    let canonical_root = workspace_path.join(".tandem");
    let legacy_root = workspace_path.join(".opencode");
    if !legacy_root.exists() {
        return Ok(());
    }

    fs::create_dir_all(&canonical_root).map_err(TandemError::Io)?;

    let (canonical_plans, legacy_plans) = workspace_plans_dirs(workspace_path);
    let (copied_plans, skipped_plans) =
        copy_workspace_tree_if_missing(&legacy_plans, &canonical_plans)?;

    let (canonical_skills, legacy_skills) = workspace_skill_dirs(workspace_path);
    let (copied_skills, skipped_skills) =
        copy_workspace_tree_if_missing(&legacy_skills, &canonical_skills)?;

    let legacy_python_cfg = legacy_root
        .join("tandem")
        .join("python")
        .join("config.json");
    let canonical_python_cfg = canonical_root
        .join("tandem")
        .join("python")
        .join("config.json");
    let mut copied_python = 0usize;
    let mut skipped_python = 0usize;
    if legacy_python_cfg.exists() {
        if canonical_python_cfg.exists() {
            skipped_python = 1;
        } else {
            if let Some(parent) = canonical_python_cfg.parent() {
                fs::create_dir_all(parent).map_err(TandemError::Io)?;
            }
            fs::copy(&legacy_python_cfg, &canonical_python_cfg).map_err(TandemError::Io)?;
            copied_python = 1;
        }
    }

    if copied_plans + copied_skills + copied_python > 0 {
        tracing::info!(
            "Workspace namespace migration (.opencode -> .tandem): copied plans={} skills={} python_cfg={} skipped plans={} skills={} python_cfg={} workspace={}",
            copied_plans,
            copied_skills,
            copied_python,
            skipped_plans,
            skipped_skills,
            skipped_python,
            workspace_path.display()
        );
    }

    Ok(())
}

fn is_plan_file_operation(path: &str, tool: &str) -> bool {
    // Only auto-approve write operations
    if tool != "write" && tool != "write_file" {
        return false;
    }

    let normalized_path = normalize_path_for_match(path);

    normalized_path.contains("/.tandem/plans/")
        || normalized_path.starts_with(".tandem/plans/")
        || normalized_path.contains("/.opencode/plans/")
        || normalized_path.starts_with(".opencode/plans/")
}
