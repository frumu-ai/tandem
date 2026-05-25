// ============================================================================
// API Key Management
// ============================================================================

/// Store an API key in the stronghold vault
#[tauri::command]
pub async fn store_api_key(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    key_type: String,
    api_key: String,
) -> Result<()> {
    // Validate inputs
    let key_type_enum = validate_key_type(&key_type)?;
    validate_api_key(&api_key)?;

    let key_name = key_type_enum.to_key_name();
    let api_key_value = api_key.clone();
    let _key_type_for_log = key_type.clone();

    // Clone app handle so we can move it into spawn_blocking
    let app_clone = app.clone();

    // Insert the key in memory first (fast)
    let keystore = app_clone
        .try_state::<SecureKeyStore>()
        .ok_or_else(|| TandemError::Vault("Keystore not initialized".to_string()))?;

    keystore.set(&key_name, &api_key_value)?;

    // Update environment variable immediately
    if let Some(env_key) = env_var_for_key(&key_type_enum) {
        // Never log secrets (even masked) to avoid accidental disclosure.
        tracing::info!("Setting environment variable {}", env_key);
        state.sidecar.set_env(env_key, &api_key).await;
    }

    tracing::info!("API key saved");

    {
        let mut providers = state.providers_config.write().unwrap();
        populate_provider_keys(&app, &mut providers);
    }

    // Restart sidecar if it's running to reload env vars
    if matches!(state.sidecar.state().await, SidecarState::Running) {
        let sidecar_path = sidecar_manager::get_sidecar_binary_path(&app)?;
        state
            .sidecar
            .restart(sidecar_path.to_string_lossy().as_ref())
            .await?;
    }

    Ok(())
}

/// Check if an API key exists for a provider
#[tauri::command]
pub async fn has_api_key(app: tauri::AppHandle, key_type: String) -> Result<bool> {
    let key_type_enum = validate_key_type(&key_type)?;
    let key_name = key_type_enum.to_key_name();

    let keystore = match app.try_state::<SecureKeyStore>() {
        Some(ks) => ks,
        None => return Ok(false),
    };

    Ok(keystore.has(&key_name))
}

/// Delete an API key from the vault
#[tauri::command]
pub async fn delete_api_key(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    key_type: String,
) -> Result<()> {
    let key_type_enum = validate_key_type(&key_type)?;
    let key_name = key_type_enum.to_key_name();

    let keystore = app
        .try_state::<SecureKeyStore>()
        .ok_or_else(|| TandemError::Vault("Keystore not initialized".to_string()))?;

    keystore.delete(&key_name)?;

    if let Some(env_key) = env_var_for_key(&key_type_enum) {
        state.sidecar.remove_env(env_key).await;
        {
            let mut providers = state.providers_config.write().unwrap();
            populate_provider_keys(&app, &mut providers);
        }
        if matches!(state.sidecar.state().await, SidecarState::Running) {
            let sidecar_path = sidecar_manager::get_sidecar_binary_path(&app)?;
            state
                .sidecar
                .restart(sidecar_path.to_string_lossy().as_ref())
                .await?;
        }
    }

    tracing::info!("API key deleted for provider: {}", key_type);
    Ok(())
}

/// Get an API key from the vault (internal use only)
async fn get_api_key(app: &AppHandle, key_type: &str) -> Result<Option<String>> {
    let key_type_enum = validate_key_type(key_type)?;
    let key_name = key_type_enum.to_key_name();

    let keystore = match app.try_state::<SecureKeyStore>() {
        Some(ks) => ks,
        None => return Ok(None),
    };

    keystore.get(&key_name)
}

// ============================================================================
