// Tandem - A local-first, zero-trust AI workspace application
// This is the main library entry point for the Tauri application

mod commands;
mod error;
mod llm_router;
mod sidecar;
mod state;
mod stronghold;
mod tool_proxy;

use tauri::Manager;
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
            // Initialize application state
            let state = state::AppState::new();
            app.manage(state);

            // Initialize Stronghold with a snapshot path in app data directory
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("Failed to get app data directory");
            std::fs::create_dir_all(&app_data_dir).ok();
            let snapshot_path = app_data_dir.join("tandem.stronghold");

            // Create stronghold instance with a 32-byte key
            // The password needs to be exactly 32 bytes for AES-256
            let password = b"tandem-secure-vault-key-32bytes!".to_vec();
            let stronghold = Stronghold::new(snapshot_path, password)
                .expect("Failed to create Stronghold");
            app.manage(stronghold);

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
