// Tandem - A local-first, zero-trust AI workspace application
// This is the main library entry point for the Tauri application

mod commands;
mod error;
mod llm_router;
mod sidecar;
mod sidecar_manager;
mod state;
mod stronghold;
mod tool_proxy;

use tauri::Manager;
use tauri_plugin_store::StoreExt;
use tauri_plugin_stronghold::stronghold::Stronghold;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

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
                // Derive key from password using simple hash
                // In production, use a proper KDF like Argon2
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                password.hash(&mut hasher);
                let hash = hasher.finish();
                // Expand to 32 bytes for AES-256
                let mut key = Vec::with_capacity(32);
                key.extend_from_slice(&hash.to_le_bytes());
                key.extend_from_slice(&hash.to_be_bytes());
                key.extend_from_slice(&hash.to_le_bytes());
                key.extend_from_slice(&hash.to_be_bytes());
                key
            })
            .build(),
        )
        .setup(|app| {
            // Initialize Stronghold with a snapshot path in app data directory
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");
            std::fs::create_dir_all(&app_data_dir).ok();
            let snapshot_path = app_data_dir.join("tandem.stronghold");

            // Create stronghold instance with a 32-byte key
            let password = b"tandem-secure-vault-key-32bytes!".to_vec();
            let stronghold = Stronghold::new(snapshot_path, password)
                .expect("Failed to create Stronghold");
            app.manage(stronghold);

            // Initialize application state
            let app_state = state::AppState::new();

            // Load saved settings from store
            let store = app.store("settings.json").expect("Failed to create store");
            
            // Load providers config
            if let Some(config) = store.get("providers_config") {
                if let Ok(providers) = serde_json::from_value::<state::ProvidersConfig>(config.clone()) {
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

            // Load API keys from Stronghold and set them in sidecar environment
            let stronghold = app.state::<Stronghold>();
            let client_path = b"tandem";
            if let Ok(client) = stronghold.get_client(client_path) {
                let store = client.store();
                
                // Load OpenRouter API key
                if let Ok(key_bytes) = store.get(b"openrouter_api_key") {
                    if let Some(bytes) = key_bytes {
                        if let Ok(key) = String::from_utf8(bytes) {
                            tracing::info!("Loaded OpenRouter API key from vault");
                            let sidecar = &app_state.sidecar;
                            // Use tokio runtime to set env var
                            let rt = tokio::runtime::Handle::current();
                            let sidecar_clone = sidecar.clone();
                            rt.spawn(async move {
                                sidecar_clone.set_env("OPENROUTER_API_KEY", &key).await;
                            });
                        }
                    }
                }
                
                // Load Anthropic API key
                if let Ok(key_bytes) = store.get(b"anthropic_api_key") {
                    if let Some(bytes) = key_bytes {
                        if let Ok(key) = String::from_utf8(bytes) {
                            tracing::info!("Loaded Anthropic API key from vault");
                            let sidecar = &app_state.sidecar;
                            let rt = tokio::runtime::Handle::current();
                            let sidecar_clone = sidecar.clone();
                            rt.spawn(async move {
                                sidecar_clone.set_env("ANTHROPIC_API_KEY", &key).await;
                            });
                        }
                    }
                }
                
                // Load OpenAI API key
                if let Ok(key_bytes) = store.get(b"openai_api_key") {
                    if let Some(bytes) = key_bytes {
                        if let Ok(key) = String::from_utf8(bytes) {
                            tracing::info!("Loaded OpenAI API key from vault");
                            let sidecar = &app_state.sidecar;
                            let rt = tokio::runtime::Handle::current();
                            let sidecar_clone = sidecar.clone();
                            rt.spawn(async move {
                                sidecar_clone.set_env("OPENAI_API_KEY", &key).await;
                            });
                        }
                    }
                }
            }

            app.manage(app_state);

            tracing::info!("Tandem setup complete");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
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

    // Add single instance plugin on desktop platforms
    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        builder = builder.plugin(tauri_plugin_single_instance::init(|_app, _args, _cwd| {
            // Handle when another instance tries to launch
            tracing::info!("Another instance tried to launch");
        }));
    }

    builder
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
