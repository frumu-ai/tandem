// Tandem - A local-first, zero-trust AI workspace application
// This is the main library entry point for the Tauri application

mod commands;
mod error;
mod file_watcher;
mod keystore;
mod llm_router;
mod presentation;
mod sidecar;
mod sidecar_manager;
mod skills;
mod state;
mod tool_proxy;
mod vault;

use std::sync::RwLock;
use tauri::Manager;
use tauri_plugin_store::StoreExt;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
    use std::fs;
    use tracing_appender::rolling;

    // Create logs directory
    let logs_dir = app_data_dir.join("logs");
    fs::create_dir_all(&logs_dir).ok();

    // Use daily rotation with size limit (tracing-appender will handle rotation)
    // Each file will be named tandem-YYYY-MM-DD.log
    // Old files are kept for a few days before being deleted
    let file_appender = rolling::daily(&logs_dir, "tandem");

    // Set up both console and file logging
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(file_appender)
        .with_ansi(false); // No ANSI colors in file

    let console_layer = tracing_subscriber::fmt::layer();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                // Changed from "tandem=debug,tauri=info" to reduce log size
                .unwrap_or_else(|_| "tandem=info,tauri=info".into()),
        )
        .with(console_layer)
        .with(file_layer)
        .init();

    tracing::info!("Logging initialized (logs directory: {:?})", logs_dir);
    tracing::info!("Log level: INFO (use RUST_LOG env var to change)");
}

/// Initialize keystore with the given master key and load API keys
fn initialize_keystore_and_keys(app: &tauri::AppHandle, master_key: &[u8]) {
    let app_data_dir = app
        .path()
        .app_data_dir()
        .expect("Failed to get app data directory");
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

    // Restart sidecar to ensure it picks up new env vars (keys are only applied on start).
    let app_clone = app.clone();
    let sidecar_for_restart = sidecar.clone();
    tauri::async_runtime::spawn(async move {
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

    // Manage the keystore
    app.manage(keystore);
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
            // Get app data directory for logging and state
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");

            std::fs::create_dir_all(&app_data_dir).ok();
            init_tracing(&app_data_dir);
            tracing::debug!("Starting Tandem application");

            // Initialize vault state (manages PIN-based encryption)
            let vault_state = VaultState::new(app_data_dir.clone());
            app.manage(vault_state);

            // Initialize application state (providers, workspace, sidecar)
            let app_state = state::AppState::new();

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
            // Provider configuration
            commands::get_providers_config,
            commands::set_providers_config,
            // Sidecar management
            commands::start_sidecar,
            commands::stop_sidecar,
            commands::get_sidecar_status,
            // Session management
            commands::create_session,
            commands::get_session,
            commands::list_sessions,
            commands::delete_session,
            commands::get_current_session_id,
            commands::set_current_session_id,
            // Project & history
            commands::list_projects,
            commands::get_session_messages,
            commands::get_session_todos,
            // Message handling
            commands::send_message,
            commands::send_message_streaming,
            commands::cancel_generation,
            // Model & provider info
            commands::list_models,
            commands::list_providers_from_sidecar,
            commands::list_ollama_models,
            commands::list_running_ollama_models,
            commands::stop_ollama_model,
            commands::run_ollama_model,
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
            commands::read_binary_file,
            // Skills management
            commands::list_skills,
            commands::list_skills,
            commands::import_skill,
            commands::delete_skill,
            // Guaranteed Plan Mode
            commands::start_plan_session,
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
