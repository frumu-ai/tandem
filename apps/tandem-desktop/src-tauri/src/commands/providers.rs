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

    let has_codex_oauth = tandem_core::load_provider_oauth_credential("openai-codex")
        .is_some_and(|credential| credential.expires_at_ms > crate::logs::now_ms())
        || tandem_core::load_openai_codex_cli_oauth_credential()
            .is_some_and(|credential| credential.expires_at_ms > crate::logs::now_ms());

    if let Some(keystore) = app.try_state::<SecureKeyStore>() {
        let has_key = |id: &str| keystore.has(&ApiKeyType::from_str(id).to_key_name());

        config.openrouter.has_key = has_key("openrouter");
        config.opencode_zen.has_key = has_key("opencode_zen");
        config.openai_codex.has_key = has_key("openai-codex") || has_codex_oauth;
        config.anthropic.has_key = has_key("anthropic");
        config.openai.has_key = has_key("openai");
        config.poe.has_key = has_key("poe");
        config.groq.has_key = has_key("groq");
        config.mistral.has_key = has_key("mistral");
        config.together.has_key = has_key("together");
        config.cohere.has_key = has_key("cohere");
        config.azure.has_key = has_key("azure");
        config.bedrock.has_key = has_key("bedrock");
        config.vertex.has_key = has_key("vertex");
        config.copilot.has_key = has_key("copilot");
        config.llama_cpp.has_key = true;
        config.ollama.has_key = true;
    } else {
        tracing::debug!("[populate_provider_keys] Keystore not available (vault locked?)");
        config.openrouter.has_key = false;
        config.opencode_zen.has_key = false;
        config.openai_codex.has_key = has_codex_oauth;
        config.anthropic.has_key = false;
        config.openai.has_key = false;
        config.poe.has_key = false;
        config.groq.has_key = false;
        config.mistral.has_key = false;
        config.together.has_key = false;
        config.cohere.has_key = false;
        config.azure.has_key = false;
        config.bedrock.has_key = false;
        config.vertex.has_key = false;
        config.copilot.has_key = false;
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

fn provider_config_for_slot<'a>(
    config: &'a ProvidersConfig,
    slot: &str,
) -> Option<&'a crate::state::ProviderConfig> {
    match slot {
        "openrouter" => Some(&config.openrouter),
        "opencode_zen" => Some(&config.opencode_zen),
        "openai-codex" | "openai_codex" => Some(&config.openai_codex),
        "anthropic" => Some(&config.anthropic),
        "openai" => Some(&config.openai),
        "llama_cpp" | "llama.cpp" => Some(&config.llama_cpp),
        "ollama" => Some(&config.ollama),
        "poe" => Some(&config.poe),
        "groq" => Some(&config.groq),
        "mistral" => Some(&config.mistral),
        "together" => Some(&config.together),
        "cohere" => Some(&config.cohere),
        "azure" => Some(&config.azure),
        "bedrock" => Some(&config.bedrock),
        "vertex" => Some(&config.vertex),
        "copilot" => Some(&config.copilot),
        _ => None,
    }
}

fn provider_settings_selected_slot(config: &ProvidersConfig) -> Option<&'static str> {
    let provider_id = config
        .selected_model
        .as_ref()?
        .provider_id
        .trim()
        .to_ascii_lowercase();
    match provider_id.as_str() {
        "openrouter" => Some("openrouter"),
        "openai-codex" | "openai_codex" => Some("openai-codex"),
        "openai" => Some("openai"),
        "anthropic" => Some("anthropic"),
        "poe" => Some("poe"),
        "opencode" | "opencode_zen" | "zen" => Some("opencode_zen"),
        "llama_cpp" | "llama.cpp" => Some("llama_cpp"),
        "ollama" => Some("ollama"),
        "groq" => Some("groq"),
        "mistral" => Some("mistral"),
        "together" => Some("together"),
        "cohere" => Some("cohere"),
        "azure" => Some("azure"),
        "bedrock" => Some("bedrock"),
        "vertex" => Some("vertex"),
        "copilot" => Some("copilot"),
        "custom" => Some("custom"),
        _ => None,
    }
}

fn provider_settings_slot_active(config: &ProvidersConfig, slot: &str) -> bool {
    let selected_slot = provider_settings_selected_slot(config);
    let selected_active = selected_slot.is_some_and(|selected| selected == slot);
    if slot == "custom" {
        return config.custom.iter().any(|provider| provider.enabled) || selected_active;
    }
    provider_config_for_slot(config, slot)
        .map(|provider| provider.enabled || selected_active)
        .unwrap_or(selected_active)
}

fn configurable_provider_slots<'a>(
    config: &'a ProvidersConfig,
) -> Vec<(&'static str, &'a crate::state::ProviderConfig)> {
    vec![
        ("openai-codex", &config.openai_codex),
        ("openrouter", &config.openrouter),
        ("anthropic", &config.anthropic),
        ("openai", &config.openai),
        ("llama_cpp", &config.llama_cpp),
        ("ollama", &config.ollama),
        ("poe", &config.poe),
        ("groq", &config.groq),
        ("mistral", &config.mistral),
        ("together", &config.together),
        ("cohere", &config.cohere),
        ("azure", &config.azure),
        ("bedrock", &config.bedrock),
        ("vertex", &config.vertex),
        ("copilot", &config.copilot),
    ]
}

fn provider_env_var(provider_id: &str) -> Option<&'static str> {
    match provider_id {
        "openai-codex" => Some("OPENAI_CODEX_API_KEY"),
        "openrouter" => Some("OPENROUTER_API_KEY"),
        "opencode_zen" => Some("OPENCODE_ZEN_API_KEY"),
        "anthropic" => Some("ANTHROPIC_API_KEY"),
        "openai" => Some("OPENAI_API_KEY"),
        "llama_cpp" => Some("LLAMA_CPP_API_KEY"),
        "poe" => Some("POE_API_KEY"),
        "groq" => Some("GROQ_API_KEY"),
        "mistral" => Some("MISTRAL_API_KEY"),
        "together" => Some("TOGETHER_API_KEY"),
        "cohere" => Some("COHERE_API_KEY"),
        "azure" => Some("AZURE_OPENAI_API_KEY"),
        "bedrock" => Some("BEDROCK_API_KEY"),
        "vertex" => Some("VERTEX_API_KEY"),
        "copilot" => Some("GITHUB_TOKEN"),
        _ => None,
    }
}

fn provider_slots_signature(config: &ProvidersConfig) -> serde_json::Value {
    let entries = configurable_provider_slots(config)
        .into_iter()
        .map(|(provider_id, provider)| {
            (
                provider_id.to_string(),
                serde_json::json!({
                    "enabled": provider.enabled,
                    "default": provider.default,
                    "endpoint": provider.endpoint,
                    "model": provider.model,
                }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    serde_json::Value::Object(entries)
}

fn sync_provider_config_file(config: &ProvidersConfig) -> Result<()> {
    let custom_provider = config
        .custom
        .iter()
        .find(|c| c.enabled && !c.endpoint.trim().is_empty());
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

        for (provider_id, provider) in configurable_provider_slots(config) {
            if provider.enabled && !provider.endpoint.trim().is_empty() {
                let mut provider_cfg = serde_json::Map::new();
                provider_cfg.insert(
                    "url".to_string(),
                    serde_json::Value::String(provider.endpoint.trim().to_string()),
                );
                if let Some(model) = provider
                    .model
                    .as_ref()
                    .map(|m| m.trim().to_string())
                    .filter(|m| !m.is_empty())
                {
                    provider_cfg.insert(
                        "default_model".to_string(),
                        serde_json::Value::String(model),
                    );
                }
                providers.insert(
                    provider_id.to_string(),
                    serde_json::Value::Object(provider_cfg),
                );
            } else {
                providers.remove(provider_id);
            }
        }
        providers.remove("llama.cpp");
        providers.remove("openai_codex");

        let default_provider = if custom_provider.is_some_and(|provider| provider.default)
            || selected_provider_model_signature(config, &["custom"]).is_some()
        {
            Some("custom")
        } else {
            configurable_provider_slots(config)
                .into_iter()
                .find_map(|(provider_id, provider)| {
                    let selected = selected_provider_model_signature(
                        config,
                        &[provider_id, &provider_id.replace('-', "_")],
                    )
                    .is_some();
                    (provider.enabled && (provider.default || selected)).then_some(provider_id)
                })
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
                            || configurable_provider_slots(config)
                                .iter()
                                .any(|(provider_id, _)| v.eq_ignore_ascii_case(provider_id))
                            || v.eq_ignore_ascii_case("llama.cpp")
                            || v.eq_ignore_ascii_case("openai_codex")
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
    for provider_id in [
        "openai-codex",
        "openrouter",
        "opencode_zen",
        "anthropic",
        "openai",
        "llama_cpp",
        "poe",
        "groq",
        "mistral",
        "together",
        "cohere",
        "azure",
        "bedrock",
        "vertex",
        "copilot",
    ] {
        let Some(env_var) = provider_env_var(provider_id) else {
            continue;
        };
        if provider_settings_slot_active(config, provider_id) {
            if let Ok(Some(key)) = get_api_key(app, provider_id).await {
                state.sidecar.set_env(env_var, &key).await;
            } else {
                state.sidecar.remove_env(env_var).await;
            }
        } else {
            state.sidecar.remove_env(env_var).await;
        }
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

    for (slot, runtime_id) in [
        ("openai-codex", "openai-codex"),
        ("openrouter", "openrouter"),
        ("opencode_zen", "zen"),
        ("anthropic", "anthropic"),
        ("openai", "openai"),
        ("llama_cpp", "llama_cpp"),
        ("poe", "poe"),
        ("groq", "groq"),
        ("mistral", "mistral"),
        ("together", "together"),
        ("cohere", "cohere"),
        ("azure", "azure"),
        ("bedrock", "bedrock"),
        ("vertex", "vertex"),
        ("copilot", "copilot"),
    ] {
        if provider_settings_slot_active(config, slot) {
            if let Ok(Some(key)) = get_api_key(app, slot).await {
                let _ = state.sidecar.set_provider_auth(runtime_id, &key).await;
            }
        }
    }
    if provider_settings_slot_active(config, "custom") {
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

    let key_providers_changed = provider_slots_signature(&previous_config)
        != provider_slots_signature(&config)
        || previous_config.opencode_zen.enabled != config.opencode_zen.enabled
        || previous_config.opencode_zen.default != config.opencode_zen.default
        || previous_config.opencode_zen.endpoint != config.opencode_zen.endpoint
        || previous_config.opencode_zen.model != config.opencode_zen.model
        || provider_settings_selected_slot(&previous_config)
            != provider_settings_selected_slot(&config);

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
pub async fn provider_oauth_authorize(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<serde_json::Value> {
    state.sidecar.provider_oauth_authorize(&provider_id).await
}

#[tauri::command]
pub async fn provider_oauth_status(
    state: State<'_, AppState>,
    provider_id: String,
    session_id: Option<String>,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .provider_oauth_status(&provider_id, session_id.as_deref())
        .await
}

#[tauri::command]
pub async fn provider_oauth_import_local(
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<serde_json::Value> {
    state
        .sidecar
        .provider_oauth_import_local(&provider_id)
        .await
}

#[tauri::command]
pub async fn delete_provider_oauth_session(
    app: AppHandle,
    state: State<'_, AppState>,
    provider_id: String,
) -> Result<()> {
    match state
        .sidecar
        .delete_provider_oauth_session(&provider_id)
        .await
    {
        Ok(()) => Ok(()),
        Err(error) if provider_id == "openai-codex" || provider_id == "openai_codex" => {
            tracing::warn!(
                "Sidecar OAuth disconnect failed for {}; falling back to local credential delete: {}",
                provider_id,
                error
            );
            let app_data_dir = shared_app_data_dir(&app)?;
            let security_dir = app_data_dir.join("security");
            let tenant_context = tandem_types::TenantContext::local_implicit();
            let removed_from_security_dir =
                tandem_core::delete_provider_credential_for_tenant_in_dir(
                    &security_dir,
                    &tenant_context,
                    "openai-codex",
                )
                .map_err(|delete_error| {
                    crate::error::TandemError::Sidecar(format!(
                        "Failed to delete OpenAI Codex OAuth session from sidecar ({error}) and local security store ({delete_error})"
                    ))
                })?;
            let removed_from_global =
                tandem_core::delete_provider_credential("openai-codex").map_err(|delete_error| {
                    crate::error::TandemError::Sidecar(format!(
                        "Failed to delete OpenAI Codex OAuth session from sidecar ({error}) and global store ({delete_error})"
                    ))
                })?;
            tracing::info!(
                "Deleted OpenAI Codex OAuth session via fallback: security_dir={} global={}",
                removed_from_security_dir,
                removed_from_global
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
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
