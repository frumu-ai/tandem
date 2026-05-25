// ============================================================================
// Channel Connections (Telegram / Discord / Slack)
// ============================================================================

#[tauri::command]
pub async fn get_channel_connections(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ChannelConnectionsView> {
    get_channel_connections_inner(&app, state.inner()).await
}

#[tauri::command]
pub async fn set_channel_connection(
    app: AppHandle,
    state: State<'_, AppState>,
    channel: String,
    input: ChannelConnectionInput,
) -> Result<ChannelConnectionsView> {
    let channel = normalize_channel_name(&channel)?;
    let project_id = active_project_id(state.inner())?;
    let keystore = app
        .try_state::<SecureKeyStore>()
        .ok_or_else(|| TandemError::Vault("Keystore not initialized".to_string()))?;

    if let Some(raw_token) = input.token.as_deref() {
        let token = raw_token.trim();
        if !token.is_empty() {
            validate_api_key(token)?;
            let key = channel_token_storage_key(&project_id, channel);
            keystore.set(&key, token)?;
            state
                .sidecar
                .set_env(channel_token_env_var(channel), token)
                .await;
        }
    }

    let token = {
        let key = channel_token_storage_key(&project_id, channel);
        keystore.get(&key)?.ok_or_else(|| {
            TandemError::InvalidConfig(format!("No saved {} bot token for active project", channel))
        })?
    };

    let existing_cfg = state.sidecar.channels_config().await.unwrap_or_default();
    let payload = match channel {
        "telegram" => {
            let allowed_users =
                normalize_allowed_users(input.allowed_users, &existing_cfg.telegram.allowed_users);
            let mention_only = input
                .mention_only
                .unwrap_or(existing_cfg.telegram.mention_only);
            let security_profile = trim_to_option(input.security_profile)
                .unwrap_or_else(|| existing_cfg.telegram.security_profile.clone());
            serde_json::json!({
                "bot_token": token,
                "allowed_users": allowed_users,
                "mention_only": mention_only,
                "style_profile": existing_cfg.telegram.style_profile,
                "security_profile": security_profile,
            })
        }
        "discord" => {
            let allowed_users =
                normalize_allowed_users(input.allowed_users, &existing_cfg.discord.allowed_users);
            let mention_only = input
                .mention_only
                .unwrap_or(existing_cfg.discord.mention_only);
            let guild_id = trim_to_option(input.guild_id).or(existing_cfg.discord.guild_id);
            let security_profile = trim_to_option(input.security_profile)
                .unwrap_or_else(|| existing_cfg.discord.security_profile.clone());
            serde_json::json!({
                "bot_token": token,
                "allowed_users": allowed_users,
                "mention_only": mention_only,
                "guild_id": guild_id,
                "security_profile": security_profile,
            })
        }
        "slack" => {
            let allowed_users =
                normalize_allowed_users(input.allowed_users, &existing_cfg.slack.allowed_users);
            let channel_id = trim_to_option(input.channel_id).or(existing_cfg.slack.channel_id);
            let security_profile = trim_to_option(input.security_profile)
                .unwrap_or_else(|| existing_cfg.slack.security_profile.clone());
            let channel_id = channel_id.ok_or_else(|| {
                TandemError::InvalidConfig("Slack channel_id is required".to_string())
            })?;
            serde_json::json!({
                "bot_token": token,
                "allowed_users": allowed_users,
                "channel_id": channel_id,
                "security_profile": security_profile,
            })
        }
        _ => {
            return Err(TandemError::InvalidConfig(format!(
                "Unsupported channel: {}",
                channel
            )))
        }
    };

    state.sidecar.channels_put(channel, payload).await?;
    get_channel_connections_inner(&app, state.inner()).await
}

#[tauri::command]
pub async fn verify_channel_connection(
    app: AppHandle,
    state: State<'_, AppState>,
    channel: String,
    input: Option<ChannelConnectionInput>,
) -> Result<serde_json::Value> {
    let channel = normalize_channel_name(&channel)?;
    let input = input.unwrap_or_default();

    let mut token = trim_to_option(input.token.clone());
    if token.is_none() {
        if let Some(keystore) = app.try_state::<SecureKeyStore>() {
            if let Ok(project_id) = active_project_id(state.inner()) {
                let key = channel_token_storage_key(&project_id, channel);
                if let Some(saved) = keystore.get(&key)? {
                    token = trim_to_option(Some(saved));
                }
            }
        }
    }

    let existing_cfg = state.sidecar.channels_config().await.unwrap_or_default();
    let payload = match channel {
        "telegram" => {
            let allowed_users =
                normalize_allowed_users(input.allowed_users, &existing_cfg.telegram.allowed_users);
            let mention_only = input
                .mention_only
                .unwrap_or(existing_cfg.telegram.mention_only);
            serde_json::json!({
                "bot_token": token.unwrap_or_default(),
                "allowed_users": allowed_users,
                "mention_only": mention_only,
            })
        }
        "discord" => {
            let allowed_users =
                normalize_allowed_users(input.allowed_users, &existing_cfg.discord.allowed_users);
            let mention_only = input
                .mention_only
                .unwrap_or(existing_cfg.discord.mention_only);
            let guild_id = trim_to_option(input.guild_id).or(existing_cfg.discord.guild_id);
            serde_json::json!({
                "bot_token": token.unwrap_or_default(),
                "allowed_users": allowed_users,
                "mention_only": mention_only,
                "guild_id": guild_id,
            })
        }
        "slack" => {
            let allowed_users =
                normalize_allowed_users(input.allowed_users, &existing_cfg.slack.allowed_users);
            let channel_id = trim_to_option(input.channel_id).or(existing_cfg.slack.channel_id);
            serde_json::json!({
                "bot_token": token.unwrap_or_default(),
                "allowed_users": allowed_users,
                "channel_id": channel_id,
            })
        }
        _ => {
            return Err(TandemError::InvalidConfig(format!(
                "Unsupported channel: {}",
                channel
            )))
        }
    };

    state.sidecar.channels_verify(channel, payload).await
}

#[tauri::command]
pub async fn disable_channel_connection(
    app: AppHandle,
    state: State<'_, AppState>,
    channel: String,
) -> Result<ChannelConnectionsView> {
    let channel = normalize_channel_name(&channel)?;
    state.sidecar.channels_delete(channel).await?;
    state
        .sidecar
        .remove_env(channel_token_env_var(channel))
        .await;
    get_channel_connections_inner(&app, state.inner()).await
}

#[tauri::command]
pub async fn delete_channel_connection_token(
    app: AppHandle,
    state: State<'_, AppState>,
    channel: String,
) -> Result<ChannelConnectionsView> {
    let channel = normalize_channel_name(&channel)?;
    let project_id = active_project_id(state.inner())?;
    let keystore = app
        .try_state::<SecureKeyStore>()
        .ok_or_else(|| TandemError::Vault("Keystore not initialized".to_string()))?;
    let key = channel_token_storage_key(&project_id, channel);
    keystore.delete(&key)?;
    state
        .sidecar
        .remove_env(channel_token_env_var(channel))
        .await;
    get_channel_connections_inner(&app, state.inner()).await
}

// ============================================================================
// Channel Tool Preferences
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ChannelToolPreferencesView {
    pub enabled_tools: Vec<String>,
    pub disabled_tools: Vec<String>,
    pub enabled_mcp_servers: Vec<String>,
}

#[tauri::command]
pub async fn get_channel_tool_preferences(
    state: State<'_, AppState>,
    channel: String,
) -> Result<ChannelToolPreferencesView> {
    let normalized = normalize_channel_name(&channel)?;
    let json = state.sidecar.channel_tool_preferences(normalized).await?;
    let prefs: ChannelToolPreferencesView = serde_json::from_value(json).unwrap_or_default();
    Ok(prefs)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SetChannelToolPreferencesInput {
    pub enabled_tools: Option<Vec<String>>,
    pub disabled_tools: Option<Vec<String>>,
    pub enabled_mcp_servers: Option<Vec<String>>,
    pub reset: Option<bool>,
}

#[tauri::command]
pub async fn set_channel_tool_preferences(
    state: State<'_, AppState>,
    channel: String,
    input: SetChannelToolPreferencesInput,
) -> Result<ChannelToolPreferencesView> {
    let normalized = normalize_channel_name(&channel)?;
    let body = serde_json::to_value(&input)
        .map_err(|e| TandemError::InvalidConfig(format!("Failed to serialize input: {}", e)))?;
    let json = state
        .sidecar
        .set_channel_tool_preferences(normalized, body)
        .await?;
    let prefs: ChannelToolPreferencesView = serde_json::from_value(json).unwrap_or_default();
    Ok(prefs)
}
