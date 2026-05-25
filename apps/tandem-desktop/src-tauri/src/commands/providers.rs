// ============================================================================
// Scheduler Configuration
// ============================================================================

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SchedulerSettings {
    #[serde(default)]
    pub mode: String,
    #[serde(default)]
    pub max_concurrent_runs: Option<usize>,
}

impl Default for SchedulerSettings {
    fn default() -> Self {
        Self {
            mode: "multi".to_string(),
            max_concurrent_runs: None,
        }
    }
}

impl SchedulerSettings {
    fn normalized(&self) -> Self {
        let mode = match self.mode.trim().to_ascii_lowercase().as_str() {
            "single" => "single",
            "multi" => "multi",
            _ => "multi",
        };
        Self {
            mode: mode.to_string(),
            max_concurrent_runs: self.max_concurrent_runs.filter(|&v| v > 0),
        }
    }
}

pub(crate) fn load_saved_scheduler_settings(app: &AppHandle) -> SchedulerSettings {
    if let Ok(store) = app.store("settings.json") {
        if let Some(value) = store.get("scheduler_settings") {
            if let Ok(settings) = serde_json::from_value::<SchedulerSettings>(value.clone()) {
                return settings.normalized();
            }
        }
    }
    SchedulerSettings::default()
}

pub(crate) async fn sync_scheduler_settings_env(state: &AppState, settings: &SchedulerSettings) {
    state
        .sidecar
        .set_env("TANDEM_SCHEDULER_MODE", &settings.mode)
        .await;
    if let Some(max) = settings.max_concurrent_runs {
        state
            .sidecar
            .set_env("TANDEM_SCHEDULER_MAX_CONCURRENT_RUNS", &max.to_string())
            .await;
    } else {
        state
            .sidecar
            .remove_env("TANDEM_SCHEDULER_MAX_CONCURRENT_RUNS")
            .await;
    }
}

#[tauri::command]
pub async fn get_scheduler_settings(app: AppHandle) -> Result<SchedulerSettings> {
    Ok(load_saved_scheduler_settings(&app))
}

#[tauri::command]
pub async fn set_scheduler_settings(
    app: AppHandle,
    settings: SchedulerSettings,
    state: State<'_, AppState>,
) -> Result<SchedulerSettings> {
    let normalized = settings.normalized();
    if let Ok(store) = app.store("settings.json") {
        store.set(
            "scheduler_settings",
            serde_json::to_value(&normalized).unwrap_or_default(),
        );
        let _ = store.save();
    }
    sync_scheduler_settings_env(&state, &normalized).await;
    Ok(normalized)
}

// ============================================================================
// Provider Configuration
// ============================================================================

fn default_search_backend() -> String {
    "auto".to_string()
}

fn default_search_timeout_ms() -> u64 {
    10_000
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchSettings {
    #[serde(default = "default_search_backend")]
    pub backend: String,
    #[serde(default)]
    pub tandem_url: Option<String>,
    #[serde(default)]
    pub searxng_url: Option<String>,
    #[serde(default = "default_search_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for SearchSettings {
    fn default() -> Self {
        Self {
            backend: default_search_backend(),
            tandem_url: None,
            searxng_url: None,
            timeout_ms: default_search_timeout_ms(),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SearchSettingsView {
    pub backend: String,
    pub tandem_url: Option<String>,
    pub searxng_url: Option<String>,
    pub timeout_ms: u64,
    pub has_brave_key: bool,
    pub has_exa_key: bool,
}

fn normalize_search_backend(raw: &str) -> String {
    match raw.trim().to_ascii_lowercase().as_str() {
        "auto" | "" => "auto".to_string(),
        "tandem" => "tandem".to_string(),
        "brave" => "brave".to_string(),
        "exa" => "exa".to_string(),
        "searxng" => "searxng".to_string(),
        "none" | "disabled" => "none".to_string(),
        _ => "auto".to_string(),
    }
}

fn normalize_search_url(value: Option<String>) -> Option<String> {
    value
        .map(|raw| raw.trim().trim_end_matches('/').to_string())
        .filter(|raw| !raw.is_empty())
}

fn normalize_search_settings(settings: SearchSettings) -> SearchSettings {
    SearchSettings {
        backend: normalize_search_backend(&settings.backend),
        tandem_url: normalize_search_url(settings.tandem_url),
        searxng_url: normalize_search_url(settings.searxng_url),
        timeout_ms: settings.timeout_ms.clamp(1_000, 120_000),
    }
}

pub(crate) fn load_saved_search_settings(app: &AppHandle) -> SearchSettings {
    if let Ok(store) = app.store("settings.json") {
        if let Some(value) = store.get("search_settings") {
            if let Ok(settings) = serde_json::from_value::<SearchSettings>(value.clone()) {
                return normalize_search_settings(settings);
            }
        }
    }
    SearchSettings::default()
}

fn search_key_presence(app: &AppHandle) -> (bool, bool) {
    let Some(keystore) = app.try_state::<SecureKeyStore>() else {
        return (false, false);
    };
    (
        keystore.has(&ApiKeyType::BraveSearch.to_key_name()),
        keystore.has(&ApiKeyType::ExaSearch.to_key_name()),
    )
}

fn search_settings_view(app: &AppHandle, settings: SearchSettings) -> SearchSettingsView {
    let (has_brave_key, has_exa_key) = search_key_presence(app);
    SearchSettingsView {
        backend: settings.backend,
        tandem_url: settings.tandem_url,
        searxng_url: settings.searxng_url,
        timeout_ms: settings.timeout_ms,
        has_brave_key,
        has_exa_key,
    }
}

pub(crate) async fn sync_search_settings_env(state: &AppState, settings: &SearchSettings) {
    state
        .sidecar
        .set_env("TANDEM_SEARCH_BACKEND", &settings.backend)
        .await;
    state
        .sidecar
        .set_env("TANDEM_SEARCH_TIMEOUT_MS", &settings.timeout_ms.to_string())
        .await;
    if let Some(url) = settings.tandem_url.as_deref() {
        state.sidecar.set_env("TANDEM_SEARCH_URL", url).await;
    } else {
        state.sidecar.remove_env("TANDEM_SEARCH_URL").await;
    }
    if let Some(url) = settings.searxng_url.as_deref() {
        state.sidecar.set_env("TANDEM_SEARXNG_URL", url).await;
    } else {
        state.sidecar.remove_env("TANDEM_SEARXNG_URL").await;
    }
}

#[tauri::command]
pub async fn get_search_settings(app: AppHandle) -> Result<SearchSettingsView> {
    Ok(search_settings_view(&app, load_saved_search_settings(&app)))
}

#[tauri::command]
pub async fn set_search_settings(
    app: AppHandle,
    settings: SearchSettings,
    state: State<'_, AppState>,
) -> Result<SearchSettingsView> {
    let normalized = normalize_search_settings(settings);
    if let Ok(store) = app.store("settings.json") {
        store.set(
            "search_settings",
            serde_json::to_value(&normalized).unwrap_or_default(),
        );
        let _ = store.save();
    }

    sync_search_settings_env(&state, &normalized).await;

    if matches!(state.sidecar.state().await, SidecarState::Running) {
        let sidecar_path = sidecar_manager::get_sidecar_binary_path(&app)?;
        state
            .sidecar
            .restart(sidecar_path.to_string_lossy().as_ref())
            .await?;
    }

    Ok(search_settings_view(&app, normalized))
}

/// Get the providers configuration
/// Get the providers configuration (with key status)
#[tauri::command]
pub async fn get_providers_config(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<ProvidersConfig> {
    let mut config = state.providers_config.read().unwrap().clone();

    // Dynamically populate has_key status
    populate_provider_keys(&app, &mut config);

    Ok(config)
}

/// Helper to populate has_key status from keystore
// This function is local to commands but we need to ensure keys are populated on load too.
// Actually, `lib.rs` initializes keys into env vars via `init_keystore_and_keys`.
// `populate_provider_keys` here updates the *config object* in memory to say `has_key = true`.
// We need to make sure this happens on app startup after loading config.
pub fn populate_provider_keys(app: &AppHandle, config: &mut ProvidersConfig) {
    use crate::keystore::ApiKeyType;

    if let Some(keystore) = app.try_state::<SecureKeyStore>() {
        let openrouter_key = ApiKeyType::OpenRouter.to_key_name();
        let opencode_zen_key = ApiKeyType::OpenCodeZen.to_key_name();
        let anthropic_key = ApiKeyType::Anthropic.to_key_name();
        let openai_key = ApiKeyType::OpenAI.to_key_name();
        let poe_key = ApiKeyType::Poe.to_key_name();

        tracing::info!("[populate_provider_keys] Checking for keys:");
        tracing::info!(
            "  OpenRouter key '{}': {}",
            openrouter_key,
            keystore.has(&openrouter_key)
        );
        tracing::info!(
            "  TandemZen key '{}': {}",
            opencode_zen_key,
            keystore.has(&opencode_zen_key)
        );
        tracing::info!(
            "  Anthropic key '{}': {}",
            anthropic_key,
            keystore.has(&anthropic_key)
        );
        tracing::info!(
            "  OpenAI key '{}': {}",
            openai_key,
            keystore.has(&openai_key)
        );
        tracing::info!("  Poe key '{}': {}", poe_key, keystore.has(&poe_key));

        config.openrouter.has_key = keystore.has(&openrouter_key);
        config.opencode_zen.has_key = keystore.has(&opencode_zen_key);
        config.anthropic.has_key = keystore.has(&anthropic_key);
        config.openai.has_key = keystore.has(&openai_key);
        config.poe.has_key = keystore.has(&poe_key);
        config.llama_cpp.has_key = true;
        config.ollama.has_key = true;
    } else {
        tracing::debug!("[populate_provider_keys] Keystore not available (vault locked?)");
        config.openrouter.has_key = false;
        config.opencode_zen.has_key = false;
        config.anthropic.has_key = false;
        config.openai.has_key = false;
        config.poe.has_key = false;
        config.llama_cpp.has_key = true;
        config.ollama.has_key = true;
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ChannelConnectionInput {
    pub token: Option<String>,
    pub allowed_users: Option<Vec<String>>,
    pub mention_only: Option<bool>,
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub security_profile: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ChannelConnectionConfigView {
    pub has_token: bool,
    pub token_masked: Option<String>,
    pub allowed_users: Vec<String>,
    pub mention_only: Option<bool>,
    pub guild_id: Option<String>,
    pub channel_id: Option<String>,
    pub style_profile: Option<String>,
    pub security_profile: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ChannelConnectionView {
    pub status: crate::sidecar::ChannelRuntimeStatus,
    pub config: ChannelConnectionConfigView,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct ChannelConnectionsView {
    pub telegram: ChannelConnectionView,
    pub discord: ChannelConnectionView,
    pub slack: ChannelConnectionView,
}

fn normalize_allowed_users(input: Option<Vec<String>>, fallback: &[String]) -> Vec<String> {
    let mut users = input.unwrap_or_else(|| fallback.to_vec());
    users = users
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if users.is_empty() {
        users.push("*".to_string());
    }
    users
}

fn trim_to_option(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn merge_channel_views(
    statuses: ChannelsStatusResponse,
    configs: ChannelsConfigResponse,
    project_token_presence: Option<&std::collections::HashMap<&'static str, bool>>,
) -> ChannelConnectionsView {
    let has_token_for = |channel: &'static str, fallback: bool| -> bool {
        project_token_presence
            .and_then(|map| map.get(channel))
            .copied()
            .unwrap_or(fallback)
    };
    let masked_token_for = |channel: &'static str, fallback: bool| -> Option<String> {
        if has_token_for(channel, fallback) {
            Some("********".to_string())
        } else {
            None
        }
    };

    ChannelConnectionsView {
        telegram: ChannelConnectionView {
            status: statuses.telegram,
            config: ChannelConnectionConfigView {
                has_token: has_token_for("telegram", configs.telegram.has_token),
                token_masked: masked_token_for("telegram", configs.telegram.has_token),
                allowed_users: normalize_allowed_users(Some(configs.telegram.allowed_users), &[]),
                mention_only: Some(configs.telegram.mention_only),
                guild_id: None,
                channel_id: None,
                style_profile: Some(configs.telegram.style_profile),
                security_profile: trim_to_option(Some(configs.telegram.security_profile)),
            },
        },
        discord: ChannelConnectionView {
            status: statuses.discord,
            config: ChannelConnectionConfigView {
                has_token: has_token_for("discord", configs.discord.has_token),
                token_masked: masked_token_for("discord", configs.discord.has_token),
                allowed_users: normalize_allowed_users(Some(configs.discord.allowed_users), &[]),
                mention_only: Some(configs.discord.mention_only),
                guild_id: trim_to_option(configs.discord.guild_id),
                channel_id: None,
                style_profile: None,
                security_profile: trim_to_option(Some(configs.discord.security_profile)),
            },
        },
        slack: ChannelConnectionView {
            status: statuses.slack,
            config: ChannelConnectionConfigView {
                has_token: has_token_for("slack", configs.slack.has_token),
                token_masked: masked_token_for("slack", configs.slack.has_token),
                allowed_users: normalize_allowed_users(Some(configs.slack.allowed_users), &[]),
                mention_only: None,
                guild_id: None,
                channel_id: trim_to_option(configs.slack.channel_id),
                style_profile: None,
                security_profile: trim_to_option(Some(configs.slack.security_profile)),
            },
        },
    }
}

async fn get_channel_connections_inner(
    app: &AppHandle,
    state: &AppState,
) -> Result<ChannelConnectionsView> {
    let project_id = active_project_id(state)?;
    let sidecar_running = matches!(state.sidecar.state().await, SidecarState::Running);

    let statuses = if sidecar_running {
        state.sidecar.channels_status().await.unwrap_or_default()
    } else {
        ChannelsStatusResponse::default()
    };

    let configs = if sidecar_running {
        state.sidecar.channels_config().await.unwrap_or_default()
    } else {
        ChannelsConfigResponse::default()
    };

    let token_presence = app.try_state::<SecureKeyStore>().map(|keystore| {
        let mut map = std::collections::HashMap::new();
        for channel in CHANNEL_NAMES {
            let key = channel_token_storage_key(&project_id, channel);
            map.insert(channel, keystore.has(&key));
        }
        map
    });

    Ok(merge_channel_views(
        statuses,
        configs,
        token_presence.as_ref(),
    ))
}

fn selected_custom_model_signature(config: &ProvidersConfig) -> Option<String> {
    let selected = config.selected_model.as_ref()?;
    if selected.provider_id.trim().eq_ignore_ascii_case("custom") {
        let model = selected.model_id.trim();
        if !model.is_empty() {
            return Some(model.to_string());
        }
    }
    None
}

fn selected_provider_model_signature(
    config: &ProvidersConfig,
    provider_ids: &[&str],
) -> Option<String> {
    let selected = config.selected_model.as_ref()?;
    if provider_ids.iter().any(|provider_id| {
        selected
            .provider_id
            .trim()
            .eq_ignore_ascii_case(provider_id)
    }) {
        let model = selected.model_id.trim();
        if !model.is_empty() {
            return Some(model.to_string());
        }
    }
    None
}

fn sync_provider_config_file(config: &ProvidersConfig) -> Result<()> {
    let custom_provider = config
        .custom
        .iter()
        .find(|c| c.enabled && !c.endpoint.trim().is_empty());
    let llama_cpp_provider = config
        .llama_cpp
        .enabled
        .then_some(&config.llama_cpp)
        .filter(|provider| !provider.endpoint.trim().is_empty());
    let config_path = crate::tandem_config::global_config_path()?;
    crate::tandem_config::update_config_at(&config_path, |cfg| {
        let root = if let Some(root) = cfg.as_object_mut() {
            root
        } else {
            *cfg = serde_json::Value::Object(serde_json::Map::new());
            cfg.as_object_mut().expect("config must be object")
        };

        let providers_value = root
            .entry("providers".to_string())
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let providers = if let Some(obj) = providers_value.as_object_mut() {
            obj
        } else {
            *providers_value = serde_json::Value::Object(serde_json::Map::new());
            providers_value
                .as_object_mut()
                .expect("providers must be object")
        };

        if let Some(custom) = custom_provider {
            let endpoint = custom.endpoint.trim();
            let default_model = custom
                .model
                .as_ref()
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty());

            let mut custom_cfg = serde_json::Map::new();
            custom_cfg.insert(
                "url".to_string(),
                serde_json::Value::String(endpoint.to_string()),
            );
            if let Some(model) = default_model {
                custom_cfg.insert(
                    "default_model".to_string(),
                    serde_json::Value::String(model),
                );
            }
            providers.insert("custom".to_string(), serde_json::Value::Object(custom_cfg));
        } else {
            providers.remove("custom");
        }

        if let Some(llama_cpp) = llama_cpp_provider {
            let endpoint = llama_cpp.endpoint.trim();
            let default_model = llama_cpp
                .model
                .as_ref()
                .map(|m| m.trim().to_string())
                .filter(|m| !m.is_empty());

            let mut llama_cpp_cfg = serde_json::Map::new();
            llama_cpp_cfg.insert(
                "url".to_string(),
                serde_json::Value::String(endpoint.to_string()),
            );
            if let Some(model) = default_model {
                llama_cpp_cfg.insert(
                    "default_model".to_string(),
                    serde_json::Value::String(model),
                );
            }
            providers.insert(
                "llama_cpp".to_string(),
                serde_json::Value::Object(llama_cpp_cfg),
            );
            providers.remove("llama.cpp");
        } else {
            providers.remove("llama_cpp");
            providers.remove("llama.cpp");
        }

        let selected_custom = selected_provider_model_signature(config, &["custom"]).is_some();
        let selected_llama_cpp =
            selected_provider_model_signature(config, &["llama_cpp", "llama.cpp"]).is_some();
        let default_provider = if custom_provider.is_some_and(|provider| provider.default)
            || selected_custom
        {
            Some("custom")
        } else if llama_cpp_provider.is_some_and(|provider| provider.default) || selected_llama_cpp
        {
            Some("llama_cpp")
        } else {
            None
        };

        match default_provider {
            Some(provider_id) => {
                root.insert(
                    "default_provider".to_string(),
                    serde_json::Value::String(provider_id.to_string()),
                );
            }
            None => {
                let should_clear_default = root
                    .get("default_provider")
                    .and_then(|v| v.as_str())
                    .map(|v| {
                        v.eq_ignore_ascii_case("custom")
                            || v.eq_ignore_ascii_case("llama_cpp")
                            || v.eq_ignore_ascii_case("llama.cpp")
                    })
                    .unwrap_or(false);
                if should_clear_default {
                    root.remove("default_provider");
                }
            }
        }

        Ok(())
    })?;
    Ok(())
}

async fn sync_ollama_env(state: &AppState, config: &ProvidersConfig) {
    if config.ollama.enabled {
        let endpoint = config.ollama.endpoint.trim();
        if !endpoint.is_empty() {
            state.sidecar.set_env("OLLAMA_HOST", endpoint).await;
        }
    } else {
        state.sidecar.remove_env("OLLAMA_HOST").await;
    }
}

pub(crate) async fn sync_channel_tokens_env(app: &AppHandle, state: &AppState) {
    let workspace = state.get_workspace_path();
    let project_id = state.active_project_id.read().unwrap().clone();
    let Some(project_id) = project_id else {
        for channel in CHANNEL_NAMES {
            state
                .sidecar
                .remove_env(channel_token_env_var(channel))
                .await;
        }
        return;
    };
    let Some(workspace) = workspace else {
        for channel in CHANNEL_NAMES {
            state
                .sidecar
                .remove_env(channel_token_env_var(channel))
                .await;
        }
        return;
    };
    let Some(keystore) = app.try_state::<SecureKeyStore>() else {
        for channel in CHANNEL_NAMES {
            state
                .sidecar
                .remove_env(channel_token_env_var(channel))
                .await;
        }
        return;
    };

    for channel in CHANNEL_NAMES {
        if !workspace_channel_enabled(&workspace, channel) {
            state
                .sidecar
                .remove_env(channel_token_env_var(channel))
                .await;
            continue;
        }

        let storage_key = channel_token_storage_key(&project_id, channel);
        match keystore.get(&storage_key) {
            Ok(Some(token)) if !token.trim().is_empty() => {
                state
                    .sidecar
                    .set_env(channel_token_env_var(channel), token.trim())
                    .await;
            }
            _ => {
                state
                    .sidecar
                    .remove_env(channel_token_env_var(channel))
                    .await;
            }
        }
    }
}

async fn sync_provider_keys_env(app: &AppHandle, state: &AppState, config: &ProvidersConfig) {
    // OPENROUTER
    if provider_slot_active(config, "openrouter") {
        if let Ok(Some(key)) = get_api_key(app, "openrouter").await {
            state.sidecar.set_env("OPENROUTER_API_KEY", &key).await;
        } else {
            state.sidecar.remove_env("OPENROUTER_API_KEY").await;
        }
    } else {
        state.sidecar.remove_env("OPENROUTER_API_KEY").await;
    }

    // OpenCode Zen
    if provider_slot_active(config, "opencode_zen") {
        if let Ok(Some(key)) = get_api_key(app, "opencode_zen").await {
            state.sidecar.set_env("OPENCODE_ZEN_API_KEY", &key).await;
        } else {
            state.sidecar.remove_env("OPENCODE_ZEN_API_KEY").await;
        }
    } else {
        state.sidecar.remove_env("OPENCODE_ZEN_API_KEY").await;
    }

    // Anthropic
    if provider_slot_active(config, "anthropic") {
        if let Ok(Some(key)) = get_api_key(app, "anthropic").await {
            state.sidecar.set_env("ANTHROPIC_API_KEY", &key).await;
        } else {
            state.sidecar.remove_env("ANTHROPIC_API_KEY").await;
        }
    } else {
        state.sidecar.remove_env("ANTHROPIC_API_KEY").await;
    }

    // OpenAI
    if provider_slot_active(config, "openai") {
        if let Ok(Some(key)) = get_api_key(app, "openai").await {
            state.sidecar.set_env("OPENAI_API_KEY", &key).await;
        } else {
            state.sidecar.remove_env("OPENAI_API_KEY").await;
        }
    } else {
        state.sidecar.remove_env("OPENAI_API_KEY").await;
    }

    if provider_slot_active(config, "llama_cpp") {
        if let Ok(Some(key)) = get_api_key(app, "llama_cpp").await {
            state.sidecar.set_env("LLAMA_CPP_API_KEY", &key).await;
        } else {
            state.sidecar.remove_env("LLAMA_CPP_API_KEY").await;
        }
    } else {
        state.sidecar.remove_env("LLAMA_CPP_API_KEY").await;
    }

    // Poe
    if provider_slot_active(config, "poe") {
        if let Ok(Some(key)) = get_api_key(app, "poe").await {
            state.sidecar.set_env("POE_API_KEY", &key).await;
        } else {
            state.sidecar.remove_env("POE_API_KEY").await;
        }
    } else {
        state.sidecar.remove_env("POE_API_KEY").await;
    }
}

async fn sync_provider_keys_runtime_auth(
    app: &AppHandle,
    state: &AppState,
    config: &ProvidersConfig,
) {
    if !matches!(state.sidecar.state().await, SidecarState::Running) {
        return;
    }

    if provider_slot_active(config, "openrouter") {
        if let Ok(Some(key)) = get_api_key(app, "openrouter").await {
            let _ = state.sidecar.set_provider_auth("openrouter", &key).await;
        }
    }
    if provider_slot_active(config, "opencode_zen") {
        if let Ok(Some(key)) = get_api_key(app, "opencode_zen").await {
            let _ = state.sidecar.set_provider_auth("zen", &key).await;
        }
    }
    if provider_slot_active(config, "anthropic") {
        if let Ok(Some(key)) = get_api_key(app, "anthropic").await {
            let _ = state.sidecar.set_provider_auth("anthropic", &key).await;
        }
    }
    if provider_slot_active(config, "openai") {
        if let Ok(Some(key)) = get_api_key(app, "openai").await {
            let _ = state.sidecar.set_provider_auth("openai", &key).await;
        }
    }
    if provider_slot_active(config, "llama_cpp") {
        if let Ok(Some(key)) = get_api_key(app, "llama_cpp").await {
            let _ = state.sidecar.set_provider_auth("llama_cpp", &key).await;
        }
    }
    if provider_slot_active(config, "poe") {
        if let Ok(Some(key)) = get_api_key(app, "poe").await {
            let _ = state.sidecar.set_provider_auth("poe", &key).await;
        }
    }
    if provider_slot_active(config, "custom") {
        if let Ok(Some(key)) = get_api_key(app, "custom_provider").await {
            let _ = state.sidecar.set_provider_auth("custom", &key).await;
        }
    }
}

/// Set the providers configuration
#[tauri::command]
pub async fn set_providers_config(
    app: AppHandle,
    config: ProvidersConfig,
    state: State<'_, AppState>,
) -> Result<()> {
    let previous_config = {
        let providers = state.providers_config.read().unwrap();
        providers.clone()
    };

    {
        let mut providers = state.providers_config.write().unwrap();
        *providers = config.clone();
    }

    tracing::info!("Providers configuration updated");

    // Save to store for persistence
    if let Ok(store) = app.store("settings.json") {
        store.set(
            "providers_config",
            serde_json::to_value(&config).unwrap_or_default(),
        );
        let _ = store.save();
    }

    sync_provider_config_file(&config)?;

    let ollama_changed = previous_config.ollama.enabled != config.ollama.enabled
        || previous_config.ollama.endpoint != config.ollama.endpoint;
    let llama_cpp_changed = previous_config.llama_cpp.enabled != config.llama_cpp.enabled
        || previous_config.llama_cpp.endpoint != config.llama_cpp.endpoint
        || previous_config.llama_cpp.model != config.llama_cpp.model
        || previous_config.llama_cpp.default != config.llama_cpp.default;
    let custom_changed = serde_json::to_value(&previous_config.custom).ok()
        != serde_json::to_value(&config.custom).ok()
        || selected_custom_model_signature(&previous_config)
            != selected_custom_model_signature(&config);

    let key_providers_changed = previous_config.openrouter.enabled != config.openrouter.enabled
        || previous_config.opencode_zen.enabled != config.opencode_zen.enabled
        || previous_config.anthropic.enabled != config.anthropic.enabled
        || previous_config.openai.enabled != config.openai.enabled
        || previous_config.llama_cpp.enabled != config.llama_cpp.enabled
        || previous_config.poe.enabled != config.poe.enabled
        || selected_provider_slot(&previous_config) != selected_provider_slot(&config);

    if ollama_changed || llama_cpp_changed || key_providers_changed || custom_changed {
        sync_ollama_env(&state, &config).await;
        sync_provider_keys_env(&app, &state, &config).await;

        if matches!(state.sidecar.state().await, SidecarState::Running) {
            let sidecar_path = sidecar_manager::get_sidecar_binary_path(&app)?;
            state
                .sidecar
                .restart(sidecar_path.to_string_lossy().as_ref())
                .await?;
            sync_provider_keys_runtime_auth(&app, &state, &config).await;
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn get_identity_config(state: State<'_, AppState>) -> Result<serde_json::Value> {
    state.sidecar.identity_config().await
}

#[tauri::command]
pub async fn patch_identity_config(
    state: State<'_, AppState>,
    patch: serde_json::Value,
) -> Result<serde_json::Value> {
    state.sidecar.patch_identity_config(patch).await
}
