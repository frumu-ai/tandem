// Tandem - A local-first, zero-trust AI workspace application
// This is the main library entry point for the Tauri application

mod commands;
mod error;
mod keystore;
mod llm_router;
mod sidecar;
mod sidecar_manager;
mod state;
mod stronghold;
mod tool_proxy;
mod vault;

use std::sync::RwLock;
use tauri::Manager;
use tauri_plugin_store::StoreExt;
use tauri_plugin_stronghold::stronghold::Stronghold;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::stronghold::ApiKeyType;

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

/// Initialize tracing for logging
fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "tandem=debug,tauri=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();
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

    tracing::info!("Keystore initialized");

    // Load API keys and set them in sidecar environment
    let app_state = app.state::<state::AppState>();
    let sidecar = app_state.sidecar.clone();

    let mappings: Vec<(stronghold::ApiKeyType, &str)> = vec![
        (stronghold::ApiKeyType::OpenRouter, "OPENROUTER_API_KEY"),
        (stronghold::ApiKeyType::Anthropic, "ANTHROPIC_API_KEY"),
        (stronghold::ApiKeyType::OpenAI, "OPENAI_API_KEY"),
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
                tracing::info!("Loaded {} from vault ({})", env_var, masked);
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
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_tracing();

    tracing::info!("Starting Tandem application");

    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(
            tauri_plugin_stronghold::Builder::new(|password| {
                // This callback is used when the plugin needs to derive a key
                // We pass through the master key directly since we already derived it
                password.as_bytes().to_vec()
            })
            .build(),
        )
        .setup(|app| {
            // Get app data directory
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");
            std::fs::create_dir_all(&app_data_dir).ok();

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
                    tracing::info!("Loaded saved providers config");
                    *app_state.providers_config.write().unwrap() = providers;
                }
            }

            // Load workspace path
            if let Some(path) = store.get("workspace_path") {
                if let Some(path_str) = path.as_str() {
                    let path_buf = std::path::PathBuf::from(path_str);
                    if path_buf.exists() {
                        tracing::info!("Loaded saved workspace: {}", path_str);
                        app_state.set_workspace(path_buf);
                    }
                }
            }

            app.manage(app_state);

            // Note: Stronghold is NOT initialized here - it will be initialized
            // when the vault is unlocked via the unlock_vault command

            tracing::info!("Tandem setup complete (vault locked, awaiting PIN)");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            // Vault commands (must be called before other commands that need Stronghold)
            commands::get_vault_status,
            commands::create_vault,
            commands::unlock_vault,
            commands::lock_vault,
            // Basic commands
            commands::greet,
            commands::get_app_state,
            commands::set_workspace_path,
            commands::get_workspace_path,
            // API key management
            commands::store_api_key,
            commands::has_api_key,
            commands::delete_api_key,
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
            // Message handling
            commands::send_message,
            commands::send_message_streaming,
            commands::cancel_generation,
            // Model & provider info
            commands::list_models,
            commands::list_providers_from_sidecar,
            // Tool approval
            commands::approve_tool,
            commands::deny_tool,
            // Sidecar binary management
            commands::check_sidecar_status,
            commands::download_sidecar,
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
            .plugin(tauri_plugin_process::init());
    }

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

// Re-export for commands module
pub use crate::vault::VaultStatus;
pub fn init_stronghold_and_keys(app: &tauri::AppHandle, master_key: &[u8]) {
    initialize_keystore_and_keys(app, master_key);
}
