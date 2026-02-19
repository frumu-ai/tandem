// Tandem - A local-first, zero-trust AI workspace application
// This is the main library entry point for the Tauri application

mod commands;
mod document_text;
mod error;
mod file_watcher;
mod keystore;
mod llm_router;
mod logs;
mod memory;
mod modes;
pub mod orchestrator;
mod packs;
mod presentation;
mod python_env;
mod ralph;
mod sidecar;
mod sidecar_manager;
mod skill_templates;
mod skills;
mod state;
mod stream_hub;
mod tandem_config;
mod tool_history;
mod tool_policy;
mod tool_proxy;
mod vault;

use std::sync::RwLock;
use std::time::{SystemTime, UNIX_EPOCH};
use tandem_core::resolve_shared_paths;
use tandem_observability::{emit_event, init_process_logging, ObservabilityEvent, ProcessKind};
use tauri::Manager;
use tauri_plugin_store::StoreExt;

static LOG_GUARD: std::sync::OnceLock<tracing_appender::non_blocking::WorkerGuard> =
    std::sync::OnceLock::new();

/// Vault state - tracks whether the vault is unlocked and stores the master key
pub struct VaultState {
    /// Whether the vault is unlocked
    pub unlocked: RwLock<bool>,
    /// The decrypted master key (only set when unlocked)
    pub master_key: RwLock<Option<Vec<u8>>>,
    /// Path to app data directory
    pub app_data_dir: std::path::PathBuf,
}

impl VaultState {
    pub fn new(app_data_dir: std::path::PathBuf) -> Self {
        Self {
            unlocked: RwLock::new(false),
            master_key: RwLock::new(None),
            app_data_dir,
        }
    }

    pub fn is_unlocked(&self) -> bool {
        *self.unlocked.read().unwrap()
    }

    pub fn get_master_key(&self) -> Option<Vec<u8>> {
        self.master_key.read().unwrap().clone()
    }

    pub fn set_master_key(&self, key: Vec<u8>) {
        *self.master_key.write().unwrap() = Some(key);
        *self.unlocked.write().unwrap() = true;
    }

    pub fn lock(&self) {
        *self.master_key.write().unwrap() = None;
        *self.unlocked.write().unwrap() = false;
    }

    pub fn get_status(&self) -> VaultStatus {
        if !vault::vault_exists(&self.app_data_dir) {
            VaultStatus::NotCreated
        } else if self.is_unlocked() {
            VaultStatus::Unlocked
        } else {
            VaultStatus::Locked
        }
    }
}

/// Initialize tracing for logging (console + file)
fn init_tracing(app_data_dir: &std::path::Path) {
    let logs_dir = app_data_dir.join("logs");
    if let Ok((guard, info)) = init_process_logging(ProcessKind::Desktop, &logs_dir, 14) {
        let _ = LOG_GUARD.set(guard);
        emit_event(
            tracing::Level::INFO,
            ProcessKind::Desktop,
            ObservabilityEvent {
                event: "logging.initialized",
                component: "desktop.main",
                correlation_id: None,
                session_id: None,
                run_id: None,
                message_id: None,
                provider_id: None,
                model_id: None,
                status: Some("ok"),
                error_code: None,
                detail: Some("desktop jsonl logging initialized"),
            },
        );
        tracing::info!("Logging initialized (logs directory: {:?})", logs_dir);
        tracing::info!("Desktop logging info: {:?}", info);
    }
}

fn is_recoverable_memory_error(err: &memory::types::MemoryError) -> bool {
    let msg = err.to_string().to_lowercase();
    msg.contains("vector blob")
        || msg.contains("project_memory_vectors_vector_chunks")
        || msg.contains("database disk image is malformed")
        || msg.contains("malformed")
}

fn memory_db_file_paths(db_path: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths = vec![db_path.to_path_buf()];
    if let (Some(parent), Some(name)) = (db_path.parent(), db_path.file_name()) {
        let stem = name.to_string_lossy().to_string();
        paths.push(parent.join(format!("{stem}-shm")));
        paths.push(parent.join(format!("{stem}-wal")));
    }
    paths
}

fn backup_and_reset_memory_db(
    app_data_dir: &std::path::Path,
    db_path: &std::path::Path,
) -> std::io::Result<Option<std::path::PathBuf>> {
    let memory_files = memory_db_file_paths(db_path)
        .into_iter()
        .filter(|p| p.exists())
        .collect::<Vec<_>>();

    if memory_files.is_empty() {
        return Ok(None);
    }

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let backup_dir = app_data_dir
        .join("memory_backups")
        .join(format!("memory-corrupt-{ts}"));
    std::fs::create_dir_all(&backup_dir)?;

    for file in &memory_files {
        if let Some(name) = file.file_name() {
            let _ = std::fs::copy(file, backup_dir.join(name));
        }
    }
    for file in &memory_files {
        let _ = std::fs::remove_file(file);
    }

    Ok(Some(backup_dir))
}

fn init_memory_manager_with_recovery(
    app_data_dir: &std::path::Path,
    db_path: &std::path::Path,
) -> Result<memory::MemoryManager, memory::types::MemoryError> {
    match tauri::async_runtime::block_on(memory::MemoryManager::new(db_path)) {
        Ok(manager) => return Ok(manager),
        Err(err) if is_recoverable_memory_error(&err) => {
            tracing::warn!(
                "Memory DB appears incompatible/corrupt, attempting self-heal: {}",
                err
            );
            match backup_and_reset_memory_db(app_data_dir, db_path) {
                Ok(Some(backup_dir)) => {
                    tracing::warn!(
                        "Backed up memory DB files before reset: {}",
                        backup_dir.display()
                    );
                }
                Ok(None) => {
                    tracing::warn!("No memory DB files found to back up before reset");
                }
                Err(e) => {
                    tracing::error!("Failed to back up/reset memory DB: {}", e);
                    return Err(err);
                }
            }

            tauri::async_runtime::block_on(memory::MemoryManager::new(db_path))
        }
        Err(err) => Err(err),
    }
}

/// Initialize keystore with the given master key and load API keys
fn initialize_keystore_and_keys(app: &tauri::AppHandle, master_key: &[u8]) {
    let app_data_dir = match resolve_shared_paths() {
        Ok(paths) => paths.canonical_root,
        Err(e) => dirs::data_dir()
            .map(|d| d.join("tandem"))
            .unwrap_or_else(|| {
                tracing::warn!(
                    "Failed to resolve canonical shared paths ({}); falling back to Tauri app_data_dir",
                    e
                );
                app.path()
                    .app_data_dir()
                    .expect("Failed to resolve app data directory")
            }),
    };
    let keystore_path = app_data_dir.join("tandem.keystore");

    // Create keystore (fast - no Argon2!)
    let keystore = match keystore::SecureKeyStore::new(&keystore_path, master_key.to_vec()) {
        Ok(ks) => ks,
        Err(e) => {
            tracing::error!("Failed to create keystore: {}", e);
            return;
        }
    };

    tracing::debug!("Keystore initialized");

    // Load API keys and set them in sidecar environment
    let app_state = app.state::<state::AppState>();
    let sidecar = app_state.sidecar.clone();

    let mappings: Vec<(keystore::ApiKeyType, &str)> = vec![
        (keystore::ApiKeyType::OpenRouter, "OPENROUTER_API_KEY"),
        (keystore::ApiKeyType::OpenCodeZen, "OPENCODE_ZEN_API_KEY"),
        (keystore::ApiKeyType::Anthropic, "ANTHROPIC_API_KEY"),
        (keystore::ApiKeyType::OpenAI, "OPENAI_API_KEY"),
        (keystore::ApiKeyType::Poe, "POE_API_KEY"),
    ];

    for (key_type, env_var) in mappings {
        let key_name = key_type.to_key_name();
        tracing::debug!("Checking for key: {}", key_name);

        match keystore.get(&key_name) {
            Ok(Some(key)) => {
                let masked = if key.len() > 8 {
                    format!("{}...{}", &key[..4], &key[key.len() - 4..])
                } else {
                    "***".to_string()
                };
                tracing::debug!("Loaded {} from vault ({})", env_var, masked);
                let sidecar_clone = sidecar.clone();
                tauri::async_runtime::spawn(async move {
                    sidecar_clone.set_env(env_var, &key).await;
                });
            }
            Ok(None) => {
                tracing::debug!("No key found for {}", key_name);
            }
            Err(e) => {
                tracing::warn!("Error reading key {}: {}", key_name, e);
            }
        }
    }

    // Manage the keystore
    app.manage(keystore);

    // Ensure channel bot tokens are restored from the vault as soon as the keystore is ready.
    // This avoids a startup race where sidecar restarts can occur before channel env vars exist.
    let app_clone = app.clone();
    let sidecar_for_restart = sidecar.clone();
    tauri::async_runtime::spawn(async move {
        if let Some(app_state) = app_clone.try_state::<state::AppState>() {
            commands::sync_channel_tokens_env(&app_clone, app_state.inner()).await;
        }

        // Restart sidecar to ensure it picks up newly loaded env vars (keys are applied on start).
        if sidecar_for_restart.state().await == sidecar::SidecarState::Running {
            tracing::debug!("Restarting sidecar to apply updated API keys");
            if let Err(e) = sidecar_for_restart.stop().await {
                tracing::warn!("Failed to stop sidecar for key refresh: {}", e);
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            match sidecar_manager::get_sidecar_binary_path(&app_clone) {
                Ok(path) => {
                    if let Err(e) = sidecar_for_restart
                        .start(path.to_string_lossy().as_ref())
                        .await
                    {
                        tracing::error!("Failed to restart sidecar for key refresh: {}", e);
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to resolve sidecar path for key refresh: {}", e);
                }
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // We'll initialize tracing inside the builder to avoid double initialization of GTK on Linux
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            // Use canonical shared storage root across Tauri, engine, and TUI.
            let app_data_dir = match resolve_shared_paths() {
                Ok(paths) => paths.canonical_root,
                Err(e) => dirs::data_dir()
                    .map(|d| d.join("tandem"))
                    .unwrap_or_else(|| {
                        eprintln!(
                            "Failed to resolve canonical shared paths ({}); falling back to Tauri app_data_dir",
                            e
                        );
                        app.path()
                            .app_data_dir()
                            .expect("Failed to get app data directory")
                    }),
            };

            std::fs::create_dir_all(&app_data_dir).ok();
            init_tracing(&app_data_dir);
            tracing::debug!("Starting Tandem application");
            tracing::info!("Canonical storage root: {}", app_data_dir.display());

            tracing::info!(
                "Storage migration is managed by frontend startup wizard (blocking overlay)."
            );

            // Initialize vault state (manages PIN-based encryption)
            let vault_state = VaultState::new(app_data_dir.clone());
            app.manage(vault_state);

            // Initialize application state (providers, workspace, sidecar)
            let mut app_state = state::AppState::new();

            // Initialize MemoryManager (Vector DB)
            let memory_db_path = app_data_dir.join("memory.sqlite");
            match init_memory_manager_with_recovery(&app_data_dir, &memory_db_path) {
                Ok(manager) => {
                    tracing::info!("Memory manager initialized at {:?}", memory_db_path);
                    app_state.memory_manager = Some(std::sync::Arc::new(manager));
                }
                Err(e) => {
                    tracing::error!("Failed to initialize memory manager: {}", e);
                }
            }

            // Load saved settings from store
            let store = app.store("settings.json").expect("Failed to create store");

            // Load providers config
            if let Some(config) = store.get("providers_config") {
                if let Ok(providers) =
                    serde_json::from_value::<state::ProvidersConfig>(config.clone())
                {
                    tracing::debug!("Loaded saved providers config");
                    *app_state.providers_config.write().unwrap() = providers;
                }
            }

            // Load workspace path
            if let Some(path) = store.get("workspace_path") {
                if let Some(path_str) = path.as_str() {
                    let path_buf = std::path::PathBuf::from(path_str);
                    if path_buf.exists() {
                        tracing::debug!("Loaded saved workspace: {}", path_str);
                        app_state.set_workspace(path_buf);
                    }
                }
            }

            // Load user projects
            if let Some(projects_value) = store.get("user_projects") {
                if let Ok(projects) =
                    serde_json::from_value::<Vec<state::UserProject>>(projects_value.clone())
                {
                    tracing::debug!("Loaded {} user projects", projects.len());
                    *app_state.user_projects.write().unwrap() = projects;
                }
            }

            // Load active project ID
            if let Some(active_id) = store.get("active_project_id") {
                if let Some(id_str) = active_id.as_str() {
                    tracing::debug!("Loaded active project ID: {}", id_str);
                    *app_state.active_project_id.write().unwrap() = Some(id_str.to_string());

                    // Set the active project's path as workspace
                    let projects = app_state.user_projects.read().unwrap();
                    if let Some(project) = projects.iter().find(|p| p.id == id_str) {
                        let path_buf = project.path_buf();
                        if path_buf.exists() {
                            app_state.set_workspace(path_buf);
                        }
                    }
                }
            }

            // Migration: If workspace_path exists but no user_projects, migrate it
            let needs_migration = {
                let projects = app_state.user_projects.read().unwrap();
                let workspace = app_state.workspace_path.read().unwrap();
                projects.is_empty() && workspace.is_some()
            };

            if needs_migration {
                tracing::info!("Migrating single workspace to user projects system");
                let workspace = app_state.workspace_path.read().unwrap();
                if let Some(path_buf) = workspace.as_ref() {
                    let project = state::UserProject::new(path_buf.clone(), None);
                    let project_id = project.id.clone();
                    let project_name = project.name.clone();

                    // Add to state
                    {
                        let mut projects = app_state.user_projects.write().unwrap();
                        projects.push(project.clone());
                    }

                    // Set as active
                    {
                        let mut active = app_state.active_project_id.write().unwrap();
                        *active = Some(project_id.clone());
                    }

                    // Save migration
                    store.set(
                        "user_projects",
                        serde_json::to_value(vec![project]).unwrap(),
                    );
                    store.set("active_project_id", serde_json::json!(project_id));
                    let _ = store.save();

                    tracing::info!("Migration complete: created project '{}'", project_name);
                }
            }

            if let Some(workspace_path) = app_state.get_workspace_path() {
                if let Err(e) =
                    commands::migrate_workspace_legacy_namespace_if_needed(&workspace_path)
                {
                    tracing::warn!(
                        "Workspace namespace migration check failed for {}: {}",
                        workspace_path.display(),
                        e
                    );
                }
            }

            app.manage(app_state);

            // Sync bundled skills (like Plan agent) to global OpenCode config
            match skills::sync_bundled_skills(app.handle()) {
                Ok(synced) => {
                    if !synced.is_empty() {
                        tracing::info!("Synced {} bundled skill(s): {:?}", synced.len(), synced);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to sync bundled skills: {}", e);
                }
            }

            // Sync bundled tools (like the Plan tool) to global OpenCode config
            match skills::sync_bundled_tools(app.handle()) {
                Ok(synced) => {
                    if !synced.is_empty() {
                        tracing::info!("Synced {} bundled tool(s): {:?}", synced.len(), synced);
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to sync bundled tools: {}", e);
                }
            }

            // Note: Keystore is NOT initialized here - it will be initialized
            // when the vault is unlocked via the unlock_vault command

            tracing::debug!("Tandem setup complete (vault locked, awaiting PIN)");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Updater helpers
            commands::get_updater_target,
            // Vault commands (must be called before other commands that need the keystore)
            commands::get_vault_status,
            commands::create_vault,
            commands::unlock_vault,
            commands::lock_vault,
            // Basic commands
            commands::greet,
            commands::log_frontend_error,
            commands::get_app_state,
            commands::get_storage_status,
            commands::get_storage_migration_status,
            commands::run_storage_migration,
            commands::run_tool_history_backfill,
            commands::get_tool_history_backfill_status,
            commands::set_workspace_path,
            commands::get_workspace_path,
            // Project management
            commands::is_git_repo,
            commands::is_git_installed,
            commands::initialize_git_repo,
            commands::check_git_status,
            commands::add_project,
            commands::remove_project,
            commands::get_user_projects,
            commands::set_active_project,
            commands::get_active_project,
            // API key management
            commands::store_api_key,
            commands::has_api_key,
            commands::delete_api_key,
            // Theme / appearance
            commands::get_user_theme,
            commands::set_user_theme,
            commands::get_custom_background,
            commands::set_custom_background_image,
            commands::set_custom_background_image_bytes,
            commands::set_custom_background_settings,
            commands::clear_custom_background_image,
            // Provider configuration
            commands::get_providers_config,
            commands::set_providers_config,
            // Channel connections
            commands::get_channel_connections,
            commands::set_channel_connection,
            commands::disable_channel_connection,
            commands::delete_channel_connection_token,
            // Sidecar management
            commands::start_sidecar,
            commands::stop_sidecar,
            commands::get_sidecar_status,
            commands::get_sidecar_startup_health,
            commands::get_runtime_diagnostics,
            commands::get_engine_api_token,
            commands::engine_acquire_lease,
            commands::engine_renew_lease,
            commands::engine_release_lease,
            // Session management
            commands::create_session,
            commands::get_session,
            commands::list_sessions,
            commands::get_session_active_run,
            commands::delete_session,
            commands::get_current_session_id,
            commands::set_current_session_id,
            commands::list_modes,
            commands::upsert_mode,
            commands::delete_mode,
            commands::import_modes,
            commands::export_modes,
            // Project & history
            commands::list_projects,
            commands::get_session_messages,
            commands::get_session_todos,
            commands::list_tool_executions,
            // Message handling
            commands::send_message,
            commands::send_message_and_start_run,
            commands::queue_message,
            commands::queue_list,
            commands::queue_remove,
            commands::queue_send_next,
            commands::queue_send_all,
            commands::cancel_generation,
            // Model & provider info
            commands::list_models,
            commands::list_providers_from_sidecar,
            commands::list_ollama_models,
            commands::list_running_ollama_models,
            commands::stop_ollama_model,
            commands::run_ollama_model,
            // Logs (on-demand)
            commands::list_app_log_files,
            commands::start_log_stream,
            commands::stop_log_stream,
            // File operation undo
            commands::can_undo_file_change,
            commands::undo_last_file_change,
            commands::get_recent_file_operations,
            // Conversation rewind
            commands::rewind_to_message,
            // Message undo/redo (OpenCode native)
            commands::undo_message,
            commands::undo_message_with_files,
            commands::redo_message,
            commands::undo_via_command,
            // File snapshot for undo
            commands::snapshot_file_for_message,
            // Tool approval
            commands::approve_tool,
            commands::deny_tool,
            commands::answer_question,
            commands::list_questions,
            commands::reply_question,
            commands::reject_question,
            // Routine controls
            commands::routines_list,
            commands::routines_create,
            commands::routines_patch,
            commands::routines_delete,
            commands::routines_run_now,
            commands::routines_history,
            // Engine mission controls
            commands::mission_list,
            commands::mission_create,
            commands::mission_get,
            commands::mission_apply_event,
            // Agent-team command center
            commands::agent_team_list_templates,
            commands::agent_team_list_instances,
            commands::agent_team_list_missions,
            commands::agent_team_list_approvals,
            commands::agent_team_spawn,
            commands::agent_team_cancel_instance,
            commands::agent_team_cancel_mission,
            commands::agent_team_approve_spawn,
            commands::agent_team_deny_spawn,
            // Execution planning / staging area
            commands::stage_tool_operation,
            commands::get_staged_operations,
            commands::execute_staged_plan,
            commands::remove_staged_operation,
            commands::clear_staging_area,
            commands::get_staged_count,
            // Sidecar binary management
            commands::check_sidecar_status,
            commands::download_sidecar,
            // Tool guidance
            commands::get_tool_guidance,
            // Presentation export
            commands::export_presentation,
            // File browser
            commands::read_directory,
            commands::read_file_content,
            commands::read_file_text,
            commands::read_binary_file,
            // File tree watcher (Files view)
            commands::start_file_tree_watcher,
            commands::stop_file_tree_watcher,
            // Python environment (workspace venv)
            commands::python_get_status,
            commands::python_create_venv,
            commands::python_install_requirements,
            // Skills management
            commands::list_skills,
            commands::list_skills,
            commands::import_skill,
            commands::skills_import_preview,
            commands::skills_import,
            commands::delete_skill,
            commands::skills_list_templates,
            commands::skills_install_template,
            // OpenCode config (Plugins + MCP)
            commands::opencode_list_plugins,
            commands::opencode_add_plugin,
            commands::opencode_remove_plugin,
            commands::opencode_list_mcp_servers,
            commands::opencode_add_mcp_server,
            commands::opencode_remove_mcp_server,
            commands::opencode_test_mcp_connection,
            // Packs (guided workflows)
            commands::packs_list,
            commands::packs_install,
            commands::packs_install_default,
            // Guaranteed Plan Mode
            commands::start_plan_session,
            commands::list_plans,
            commands::read_plan_content,
            // Ralph Loop commands
            commands::ralph_start,
            commands::ralph_cancel,
            commands::ralph_pause,
            commands::ralph_resume,
            commands::ralph_add_context,
            commands::ralph_status,
            commands::ralph_status,
            commands::ralph_history,
            // Orchestrator commands
            commands::orchestrator_create_run,
            commands::orchestrator_start,
            commands::orchestrator_get_run,
            commands::orchestrator_get_budget,
            commands::orchestrator_list_tasks,
            commands::orchestrator_get_config,
            commands::orchestrator_extend_budget,
            commands::orchestrator_get_run_model,
            commands::orchestrator_get_model_routing,
            commands::orchestrator_set_model_routing,
            commands::orchestrator_set_resume_model,
            commands::orchestrator_approve,
            commands::orchestrator_request_revision,
            commands::orchestrator_pause,
            commands::orchestrator_resume,
            commands::orchestrator_cancel,
            commands::orchestrator_list_runs,
            commands::orchestrator_load_run,
            commands::orchestrator_restart_run,
            commands::orchestrator_delete_run,
            // Memory Management
            commands::get_memory_stats,
            commands::get_memory_settings,
            commands::set_memory_settings,
            commands::get_project_memory_stats,
            commands::clear_project_file_index,
            commands::index_workspace_command,
            // Language Settings
            commands::get_language_setting,
            commands::set_language_setting,
        ]);

    // Add desktop-only plugins
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        builder = builder
            .plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {
                // Handle when another instance tries to launch
                tracing::info!("Another instance tried to launch");
            }))
            .plugin(tauri_plugin_updater::Builder::new().build())
            .plugin(tauri_plugin_process::init())
            .on_window_event(|window, event| {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    let app = window.app_handle();
                    if let Some(state) = app.try_state::<state::AppState>() {
                        tracing::info!("Window closing - stopping sidecar");
                        tauri::async_runtime::block_on(async {
                            if let Err(e) = state.sidecar.stop().await {
                                tracing::error!("Failed to stop sidecar on close: {}", e);
                            }
                        });
                    }
                }
            });
    }

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// Re-export for commands module
pub use crate::vault::VaultStatus;
pub fn init_keystore_and_keys(app: &tauri::AppHandle, master_key: &[u8]) {
    initialize_keystore_and_keys(app, master_key);
}
