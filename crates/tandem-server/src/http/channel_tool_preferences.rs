// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn tool_preferences_path() -> PathBuf {
    let base = std::env::var("TANDEM_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if let Some(data_dir) = dirs::data_dir() {
                return data_dir.join("tandem").join("data");
            }
            dirs::home_dir()
                .map(|home| home.join(".tandem").join("data"))
                .unwrap_or_else(|| PathBuf::from(".tandem"))
        });
    base.join("channel_tool_preferences.json")
}

type ToolPreferencesMap = std::collections::HashMap<String, ChannelToolPreferences>;

fn authorize_tool_preferences_write(
    state: &AppState,
    verified: Option<&tandem_types::VerifiedTenantContext>,
) -> Result<(), StatusCode> {
    state
        .enterprise
        .hosted_policy
        .authorize_permission(verified, tandem_types::AccessPermission::HostedAdmin)
        .map_err(|_| StatusCode::FORBIDDEN)
}

async fn load_tool_preferences_map(path: &std::path::Path) -> ToolPreferencesMap {
    let Ok(bytes) = tokio::fs::read(path).await else {
        return std::collections::HashMap::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

async fn save_tool_preferences_map(
    path: PathBuf,
    map: &ToolPreferencesMap,
    state: AppState,
    verified: Option<tandem_types::VerifiedTenantContext>,
) -> Result<(), StatusCode> {
    save_tool_preferences_map_with_hook(path, map, state, verified, || {}).await
}

async fn save_tool_preferences_map_with_hook(
    path: PathBuf,
    map: &ToolPreferencesMap,
    state: AppState,
    verified: Option<tandem_types::VerifiedTenantContext>,
    before_write: impl FnOnce() + Send + 'static,
) -> Result<(), StatusCode> {
    let json = serde_json::to_vec_pretty(map).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::task::spawn_blocking(move || {
        // The blocking worker owns this entire commit. Dropping the HTTP
        // future cannot release the policy guard while a queued write runs.
        state
            .enterprise
            .hosted_policy
            .with_current_policy(|policy| {
                let authorize = || {
                    if let Some(policy) = policy {
                        let verified = verified.as_ref().ok_or(StatusCode::FORBIDDEN)?;
                        let now = crate::now_ms();
                        let projection = policy
                            .project_identity(verified, now)
                            .map_err(|_| StatusCode::FORBIDDEN)?;
                        if projection
                            .evaluate_access(
                                &policy.deployment_resource(),
                                tandem_types::AccessPermission::HostedAdmin,
                                tandem_enterprise_contract::DataClass::Internal,
                                now,
                            )
                            .decision
                            != tandem_enterprise_contract::AccessDecision::Allow
                        {
                            return Err(StatusCode::FORBIDDEN);
                        }
                    }
                    Ok(())
                };
                authorize()?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
                }
                before_write();
                // A claim or policy can expire while directory preparation (or
                // the test pause) runs even though publication is held.
                authorize()?;
                std::fs::write(&path, json).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
            })
            .map_err(|_| StatusCode::FORBIDDEN)?
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
}

pub(super) async fn channel_tool_preferences_get(
    State(state): State<AppState>,
    Path(channel): Path<String>,
    Query(query): Query<ChannelToolPreferencesQuery>,
) -> Result<Json<ChannelToolPreferences>, StatusCode> {
    channel_tool_preferences_get_at_path(&state, channel, query, &tool_preferences_path()).await
}

async fn channel_tool_preferences_get_at_path(
    state: &AppState,
    channel: String,
    query: ChannelToolPreferencesQuery,
    path: &std::path::Path,
) -> Result<Json<ChannelToolPreferences>, StatusCode> {
    let key = channel.to_string();
    let map = load_tool_preferences_map(path).await;
    let scope_id = query
        .scope_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let scoped_key = scope_id.map(|scope_id| format!("{}:{}", key, scope_id));
    let prefs = if let Some(scoped_key) = scoped_key.as_ref() {
        let base = map.get(&key).cloned().unwrap_or_default();
        map.get(scoped_key)
            .cloned()
            .map(|overlay| merge_channel_tool_preferences(base.clone(), overlay))
            .unwrap_or(base)
    } else {
        map.get(&key).cloned().unwrap_or_default()
    };
    let effective = state.config.get_effective_value().await;
    let security_profile = channel_security_profile_from_config(&effective, &key);
    let sanitized = sanitize_tool_preferences_for_security_profile(prefs, security_profile);
    Ok(Json(sanitized))
}

#[derive(Debug, serde::Deserialize)]
pub struct ChannelToolPreferencesInput {
    pub enabled_tools: Option<Vec<String>>,
    pub disabled_tools: Option<Vec<String>>,
    pub enabled_mcp_servers: Option<Vec<String>>,
    pub enabled_mcp_tools: Option<Vec<String>>,
    pub reset: Option<bool>,
}

pub(super) async fn channel_tool_preferences_put(
    State(state): State<AppState>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Path(channel): Path<String>,
    Query(query): Query<ChannelToolPreferencesQuery>,
    Json(input): Json<ChannelToolPreferencesInput>,
) -> Result<Json<ChannelToolPreferences>, StatusCode> {
    channel_tool_preferences_put_at_path(
        &state,
        verified.as_deref(),
        channel,
        query,
        input,
        tool_preferences_path(),
    )
    .await
}

async fn channel_tool_preferences_put_at_path(
    state: &AppState,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    channel: String,
    query: ChannelToolPreferencesQuery,
    input: ChannelToolPreferencesInput,
    path: PathBuf,
) -> Result<Json<ChannelToolPreferences>, StatusCode> {
    authorize_tool_preferences_write(state, verified)?;
    let mut map = load_tool_preferences_map(&path).await;
    let key = channel.to_string();
    let scope_id = query
        .scope_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let scoped_key = scope_id.map(|scope_id| format!("{}:{}", key, scope_id));
    let effective = state.config.get_effective_value().await;
    let security_profile = channel_security_profile_from_config(&effective, &key);

    let new_prefs = if input.reset.unwrap_or(false) {
        ChannelToolPreferences::default()
    } else {
        let existing = if let Some(scoped_key) = scoped_key.as_ref() {
            let base = map.get(&key).cloned().unwrap_or_default();
            map.get(scoped_key)
                .cloned()
                .map(|overlay| merge_channel_tool_preferences(base.clone(), overlay))
                .unwrap_or(base)
        } else {
            map.get(&key).cloned().unwrap_or_default()
        };
        ChannelToolPreferences {
            enabled_tools: input.enabled_tools.unwrap_or(existing.enabled_tools),
            disabled_tools: input.disabled_tools.unwrap_or(existing.disabled_tools),
            enabled_mcp_servers: input
                .enabled_mcp_servers
                .unwrap_or(existing.enabled_mcp_servers),
            enabled_mcp_tools: input
                .enabled_mcp_tools
                .unwrap_or(existing.enabled_mcp_tools),
        }
    };
    let new_prefs = sanitize_tool_preferences_for_security_profile(new_prefs, security_profile);

    if let Some(scoped_key) = scoped_key {
        map.insert(scoped_key, new_prefs.clone());
    } else {
        map.insert(key, new_prefs.clone());
    }
    save_tool_preferences_map(path, &map, state.clone(), verified.cloned()).await?;
    Ok(Json(new_prefs))
}

#[cfg(test)]
#[path = "channel_tool_preferences_authority_tests.rs"]
mod preference_authority_tests;
