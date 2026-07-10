use base64::Engine;
use futures::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Cursor;
use std::net::IpAddr;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tandem_providers::openai_codex_supported_model_rows;
use tandem_wire::{
    WireProviderCatalog, WireProviderEntry, WireProviderModel, WireProviderModelLimit,
};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use uuid::Uuid;

#[derive(Debug, Deserialize, Default)]
pub(super) struct AuthInput {
    #[serde(alias = "apiKey", alias = "api_key")]
    pub token: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ApiTokenInput {
    #[serde(alias = "apiToken", alias = "api_token")]
    pub token: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ProviderOAuthCallbackInput {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
    pub error_description: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ProviderOAuthStatusQuery {
    pub session_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct ProviderOAuthSessionImportInput {
    pub auth_json: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ProviderOAuthSessionRecord {
    pub session_id: String,
    pub provider_id: String,
    pub status: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub redirect_uri: String,
    pub state: String,
    pub code_verifier: String,
    pub authorization_url: String,
    pub error: Option<String>,
    pub email: Option<String>,
    #[serde(default = "TenantContext::local_implicit")]
    pub tenant_context: TenantContext,
}

#[derive(Debug, Serialize)]
pub(super) struct LegacyProviderInfo {
    pub id: String,
    pub name: String,
    pub models: Vec<String>,
    pub configured: bool,
}

#[derive(Debug, Deserialize)]
struct OpenAiCodexTokenExchangeResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    id_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OpenAiCodexApiKeyExchangeResponse {
    access_token: String,
}

const OPENAI_CODEX_OAUTH_CLIENT_ID: &str = "app_EMoamEEZ73f0CkXaXp7hrann";
const OPENAI_CODEX_OAUTH_ISSUER: &str = "https://auth.openai.com";
const OPENAI_CODEX_PROVIDER_ID: &str = "openai-codex";
const OPENAI_CODEX_DEFAULT_MODEL: &str = tandem_providers::OPENAI_CODEX_DEFAULT_MODEL;
const OPENAI_CODEX_API_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const OPENAI_CODEX_OAUTH_REFRESH_SKEW_MS: u64 = 5 * 60 * 1000;
const OPENAI_CODEX_LOCAL_CALLBACK_ADDR: &str = "127.0.0.1:1455";
// Match the Codex CLI browser flow. auth.openai.com expects this localhost callback shape.
const OPENAI_CODEX_LOCAL_CALLBACK_URI: &str = "http://localhost:1455/auth/callback";

fn is_internal_secret_id(raw: &str) -> bool {
    let normalized = raw.trim().to_ascii_lowercase();
    normalized.starts_with("mcp_header::")
        || normalized.starts_with("channel::")
        || normalized.starts_with("__tenant__::")
}

fn request_actor(
    request_principal: &RequestPrincipal,
    tenant_context: &TenantContext,
) -> Option<String> {
    request_principal
        .actor_id
        .clone()
        .or_else(|| tenant_context.actor_id.clone())
}

pub(super) async fn get_config(State(state): State<AppState>) -> Json<Value> {
    let effective = normalize_effective_config_with_identity(redacted(
        state.config.get_effective_value().await,
    ));
    let layers = normalize_layers_with_identity(redacted(state.config.get_layers_value().await));
    Json(json!({
        "effective": effective,
        "layers": layers
    }))
}

pub(super) async fn patch_config(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response {
    if contains_secret_config_fields(&input) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
            "error": "Secret provider keys are not accepted in config patches.",
            "code": "CONFIG_SECRET_REJECTED",
            "hint": "Use PUT /auth/{provider} or environment variables."
            })),
        )
            .into_response();
    }
    let normalized_input = normalize_config_patch_input(input);
    let updates_openai_codex_default = config_patch_updates_openai_codex_default(&normalized_input);
    let effective = match state.config.patch_project(normalized_input).await {
        Ok(effective) => effective,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    let effective = if updates_openai_codex_default {
        ensure_openai_codex_runtime_provider(&state).await;
        state.config.get_effective_value().await
    } else {
        effective
    };
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    Json(json!({ "effective": normalize_effective_config_with_identity(redacted(effective)) }))
        .into_response()
}

pub(super) async fn global_config(State(state): State<AppState>) -> Json<Value> {
    let global =
        normalize_effective_config_with_identity(redacted(state.config.get_global_value().await));
    let effective = normalize_effective_config_with_identity(redacted(
        state.config.get_effective_value().await,
    ));
    Json(json!({
        "global": global,
        "effective": effective
    }))
}

pub(super) async fn global_config_patch(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response {
    if contains_secret_config_fields(&input) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
            "error": "Secret provider keys are not accepted in global config patches.",
            "code": "CONFIG_SECRET_REJECTED",
            "hint": "Use PUT /auth/{provider} or environment variables."
            })),
        )
            .into_response();
    }
    let effective = match state
        .config
        .patch_global(normalize_config_patch_input(input))
        .await
    {
        Ok(effective) => effective,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    Json(json!({ "effective": normalize_effective_config_with_identity(redacted(effective)) }))
        .into_response()
}

pub(super) async fn get_config_identity(State(state): State<AppState>) -> Json<Value> {
    let effective = normalize_effective_config_with_identity(redacted(
        state.config.get_effective_value().await,
    ));
    let identity = effective
        .get("identity")
        .cloned()
        .unwrap_or_else(identity_default_value);
    Json(json!({
        "identity": identity,
        "presets": personality_presets_catalog()
    }))
}

pub(super) async fn patch_config_identity(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response {
    if contains_secret_config_fields(&input) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
            "error": "Secret provider keys are not accepted in config patches.",
            "code": "CONFIG_SECRET_REJECTED",
            "hint": "Use PUT /auth/{provider} or environment variables."
            })),
        )
            .into_response();
    }

    let patch = if input.get("identity").is_some() {
        normalize_config_patch_input(input)
    } else {
        normalize_config_patch_input(json!({ "identity": input }))
    };
    let effective = match state.config.patch_project(patch).await {
        Ok(effective) => effective,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    Json(json!({
        "identity": normalize_effective_config_with_identity(redacted(effective))
            .get("identity")
            .cloned()
            .unwrap_or_else(identity_default_value),
        "presets": personality_presets_catalog()
    }))
    .into_response()
}

pub(super) async fn config_providers(State(state): State<AppState>) -> Json<Value> {
    let cfg = state.config.get_effective_value().await;
    let mut providers = redacted(cfg.get("providers").cloned().unwrap_or_else(|| json!({})));
    // Heal a persisted openai-codex default that has been retired from the
    // catalog. Channel dispatchers fetch this endpoint fresh on every run and
    // send the returned default as the request model, so an unsupported value
    // here makes every channel reply fail with a provider 400.
    if let Some(codex) = providers
        .get_mut(OPENAI_CODEX_PROVIDER_ID)
        .and_then(Value::as_object_mut)
    {
        let resolved = tandem_providers::openai_codex_effective_default_model(
            codex.get("default_model").and_then(Value::as_str),
        );
        codex.insert("default_model".to_string(), Value::String(resolved));
    }
    let default_provider = cfg.get("default_provider").cloned().unwrap_or(Value::Null);
    Json(json!({
        "providers": providers,
        "default": default_provider
    }))
}

pub(crate) fn provider_auth_security_dir_for_state(state: &AppState) -> std::path::PathBuf {
    state
        .memory_db_path
        .parent()
        .map(|parent| parent.join("security"))
        .unwrap_or_else(|| std::path::PathBuf::from("security"))
}

pub(super) async fn list_providers(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
) -> Json<Value> {
    let cfg = state.config.get().await;
    let default = cfg.default_provider.unwrap_or_else(|| "local".to_string());
    let connected = state
        .providers
        .list()
        .await
        .into_iter()
        .map(|p| p.id)
        .collect::<Vec<_>>();
    let all = state.providers.list().await;
    let mut wire = WireProviderCatalog::from_providers(all, connected);
    let effective_cfg = state.config.get_effective_value().await;
    let provider_auth_security_dir = provider_auth_security_dir_for_state(&state);
    let persisted_auth = tandem_core::load_provider_auth_for_tenant_in_dir(
        &provider_auth_security_dir,
        &tenant_context,
    );
    let runtime_auth = if tenant_context.is_local_implicit() {
        state.auth.read().await.clone()
    } else {
        HashMap::new()
    };
    let allow_shared_auth_sources = tenant_context.is_local_implicit();
    let config_model_provider_set = merge_provider_models_from_config(&mut wire, &effective_cfg)
        .into_iter()
        .collect::<std::collections::HashSet<_>>();

    for entry in &mut wire.all {
        entry.models.clear();
        entry.catalog_source = None;
        entry.catalog_status = None;
        entry.catalog_message = None;
    }
    wire.all.retain(|entry| entry.id != "local");
    ensure_known_provider_entries(&mut wire);

    // Remote model discovery can be slow, so overlap the provider requests.
    let provider_catalog_results = join_all(wire.all.iter().map(|entry| {
        let provider_id = entry.id.clone();
        let effective_cfg = &effective_cfg;
        let runtime_auth = &runtime_auth;
        let persisted_auth = &persisted_auth;
        let allow_shared_auth_sources = allow_shared_auth_sources;
        async move {
            let has_discovery_key = provider_config_api_key(
                effective_cfg,
                &provider_id,
                runtime_auth,
                persisted_auth,
                allow_shared_auth_sources,
            )
            .is_some();
            let result = if provider_requires_api_key(&provider_id) && !has_discovery_key {
                if canonical_provider_id(&provider_id) == OPENAI_CODEX_PROVIDER_ID {
                    ProviderCatalogFetchResult::Static {
                        models: codex_starter_models(),
                    }
                } else {
                    ProviderCatalogFetchResult::Unavailable {
                        message: format!(
                            "{} requires an API key before live model discovery is available.",
                            known_provider_name(&provider_id).unwrap_or(provider_id.as_str())
                        ),
                    }
                }
            } else {
                fetch_remote_provider_models(
                    &provider_id,
                    &effective_cfg,
                    &runtime_auth,
                    &persisted_auth,
                    allow_shared_auth_sources,
                )
                .await
            };
            (provider_id, result)
        }
    }))
    .await
    .into_iter()
    .collect::<HashMap<_, _>>();

    for entry in &mut wire.all {
        match provider_catalog_results.get(&entry.id) {
            Some(ProviderCatalogFetchResult::Remote { models }) => {
                entry.models = models.clone();
                entry.catalog_source = Some("remote".to_string());
                entry.catalog_status = Some("ok".to_string());
            }
            Some(ProviderCatalogFetchResult::Static { models }) => {
                entry.models = models.clone();
                entry.catalog_source = Some("static".to_string());
                entry.catalog_status = Some("ok".to_string());
            }
            Some(ProviderCatalogFetchResult::Unavailable { message }) => {
                entry.catalog_source = Some("empty".to_string());
                entry.catalog_status = Some("unavailable".to_string());
                entry.catalog_message = Some(message.clone());
            }
            Some(ProviderCatalogFetchResult::Error { message }) => {
                entry.catalog_source = Some("empty".to_string());
                entry.catalog_status = Some("error".to_string());
                entry.catalog_message = Some(message.clone());
            }
            None => {
                entry.catalog_source = Some("empty".to_string());
                entry.catalog_status = Some("unavailable".to_string());
                entry.catalog_message = Some(
                    "Live model discovery is unavailable. Enter a model ID manually.".to_string(),
                );
            }
        }
    }

    merge_provider_models_from_config(&mut wire, &effective_cfg);

    for entry in &mut wire.all {
        if config_model_provider_set.contains(&entry.id)
            && entry.catalog_source.as_deref() != Some("remote")
        {
            entry.catalog_source = Some("config".to_string());
            entry.catalog_status = Some("ok".to_string());
            entry.catalog_message = None;
            continue;
        }
        if entry.models.is_empty() && entry.catalog_status.is_none() {
            entry.catalog_source = Some("empty".to_string());
            entry.catalog_status = Some("unavailable".to_string());
            entry.catalog_message =
                Some("Live model discovery is unavailable. Enter a model ID manually.".to_string());
        } else if entry.catalog_source.is_none() {
            entry.catalog_source = Some("config".to_string());
            entry.catalog_status = Some("ok".to_string());
            entry.catalog_message = None;
        }
    }

    Json(json!({
        "all": wire.all,
        "connected": wire.connected,
        "default": default
    }))
}

pub(super) async fn list_providers_legacy(
    State(state): State<AppState>,
) -> Json<Vec<LegacyProviderInfo>> {
    let connected_ids = state
        .providers
        .list()
        .await
        .into_iter()
        .map(|p| p.id)
        .collect::<std::collections::HashSet<_>>();
    let providers = state
        .providers
        .list()
        .await
        .into_iter()
        .map(|p| LegacyProviderInfo {
            id: p.id.clone(),
            name: p.name,
            models: p.models.into_iter().map(|m| m.id).collect(),
            configured: connected_ids.contains(&p.id),
        })
        .collect::<Vec<_>>();
    Json(providers)
}

pub(super) async fn provider_auth(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
) -> Json<Value> {
    let _ = refresh_openai_codex_oauth_if_needed(&state, &tenant_context).await;
    let cfg = state.config.get_effective_value().await;
    let providers_cfg = cfg
        .get("providers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let provider_auth_security_dir = provider_auth_security_dir_for_state(&state);
    let persisted = tandem_core::load_provider_auth_for_tenant_in_dir(
        &provider_auth_security_dir,
        &tenant_context,
    );
    let persisted_credentials = tandem_core::load_provider_credentials_for_tenant_in_dir(
        &provider_auth_security_dir,
        &tenant_context,
    );
    let persisted_ids = persisted
        .keys()
        .filter(|id| !is_internal_secret_id(id))
        .map(|id| id.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let local_implicit = tenant_context.is_local_implicit();
    let runtime_auth = if local_implicit {
        state.auth.read().await.clone()
    } else {
        HashMap::new()
    };
    let connected = state
        .providers
        .list()
        .await
        .into_iter()
        .map(|provider| provider.id.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let mut known_ids = std::collections::HashSet::new();

    if let Some(default_id) = cfg.get("default_provider").and_then(Value::as_str) {
        let normalized = canonical_provider_id(default_id);
        if !normalized.is_empty() {
            known_ids.insert(normalized);
        }
    }
    known_ids.insert(OPENAI_CODEX_PROVIDER_ID.to_string());
    known_ids.extend(providers_cfg.keys().map(|id| canonical_provider_id(id)));
    known_ids.extend(connected.iter().map(|id| canonical_provider_id(id)));
    known_ids.extend(runtime_auth.keys().map(|id| canonical_provider_id(id)));
    known_ids.extend(persisted_ids.iter().map(|id| canonical_provider_id(id)));
    known_ids.extend(
        persisted_credentials
            .keys()
            .map(|id| canonical_provider_id(id)),
    );

    let mut ids = known_ids.into_iter().collect::<Vec<_>>();
    ids.sort();

    let mut providers = serde_json::Map::new();
    for provider_id in ids {
        let has_runtime_key = runtime_auth
            .get(&provider_id)
            .map(|token| !token.trim().is_empty())
            .unwrap_or(false)
            || provider_id_aliases(&provider_id).iter().any(|alias| {
                runtime_auth
                    .get(*alias)
                    .map(|token| !token.trim().is_empty())
                    .unwrap_or(false)
            });
        let has_config_key = local_implicit
            && provider_config_value(&cfg, &provider_id)
                .and_then(Value::as_object)
                .map(|entry| {
                    entry
                        .get("api_key")
                        .or_else(|| entry.get("apiKey"))
                        .and_then(Value::as_str)
                        .map(|token| !token.trim().is_empty())
                        .unwrap_or(false)
                })
                .unwrap_or(false);
        let has_env_key = local_implicit && provider_has_env_secret(&provider_id);
        let has_persisted_key = persisted_ids.contains(&provider_id)
            || provider_id_aliases(&provider_id)
                .iter()
                .any(|alias| persisted_ids.contains(*alias));
        let oauth_credential = provider_id_aliases(&provider_id)
            .iter()
            .find_map(|alias| persisted_credentials.get(*alias))
            .or_else(|| persisted_credentials.get(&provider_id));
        let has_oauth_credential = oauth_credential.is_some();
        let local_session_available = local_implicit
            && provider_id == OPENAI_CODEX_PROVIDER_ID
            && tandem_core::load_openai_codex_cli_oauth_credential().is_some();
        let has_key = has_env_key || has_runtime_key || has_config_key || has_persisted_key;
        let source = if has_oauth_credential {
            "oauth"
        } else if has_env_key {
            "env"
        } else if has_persisted_key {
            "persisted"
        } else if has_runtime_key || has_config_key {
            "runtime"
        } else {
            "none"
        };
        let configured = connected.contains(&provider_id);
        let auth_kind = match oauth_credential {
            Some(tandem_core::ProviderCredential::OAuth(_)) => "oauth",
            _ => "api_key",
        };
        let mut payload = json!({
            "has_key": has_key || has_oauth_credential,
            "configured": configured,
            "connected": configured && (has_key || has_oauth_credential || !provider_requires_api_key(&provider_id)),
            "source": source,
            "auth_kind": auth_kind,
            "status": if has_oauth_credential { "connected" } else if has_key { "configured" } else { "missing" },
            "local_session_available": local_session_available,
        });
        if let Some(tandem_core::ProviderCredential::OAuth(oauth)) = oauth_credential {
            payload["expires_at_ms"] = json!(oauth.expires_at_ms);
            payload["email"] = json!(oauth.email);
            payload["display_name"] = json!(oauth.display_name);
            payload["managed_by"] = json!(oauth.managed_by);
            payload["account_id"] = json!(oauth.account_id);
            if oauth.expires_at_ms <= crate::now_ms() {
                payload["status"] = json!("reauth_required");
                payload["connected"] = json!(false);
            }
        }
        providers.insert(provider_id, payload);
    }

    Json(json!({ "providers": providers }))
}

pub(super) async fn provider_oauth_authorize(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Json<Value> {
    let provider_id = canonical_provider_id(&id);
    if provider_id != OPENAI_CODEX_PROVIDER_ID {
        return Json(json!({
            "ok": false,
            "error": format!("oauth is not supported for provider `{provider_id}`"),
        }));
    }

    let session_id = Uuid::new_v4().to_string();
    let created_at_ms = crate::now_ms();
    let expires_at_ms = created_at_ms.saturating_add(10 * 60 * 1000);
    let (code_verifier, code_challenge) = generate_pkce_pair();
    let state_token = generate_oauth_state();
    let effective_cfg = state.config.get_effective_value().await;
    let hosted_managed = hosted_managed_from_config(&effective_cfg);
    let public_panel_base_url = provider_oauth_public_panel_base_url(&effective_cfg, &headers);
    let engine_base_url = provider_oauth_engine_base_url(&state);
    let callback_base_url = public_panel_base_url
        .as_deref()
        .unwrap_or(engine_base_url.as_str());
    let use_control_panel_callback = public_panel_base_url.is_some();
    let use_hosted_codex_callback = hosted_managed
        || use_control_panel_callback
        || !provider_oauth_base_url_is_loopback(&engine_base_url);

    if provider_id == OPENAI_CODEX_PROVIDER_ID && !use_hosted_codex_callback {
        if let Err(error) = ensure_openai_codex_local_callback_server(state.clone()).await {
            return Json(json!({
                "ok": false,
                "error": format!("failed to start local Codex callback server: {error}"),
            }));
        }
    }
    let redirect_uri = if provider_id == OPENAI_CODEX_PROVIDER_ID && !use_hosted_codex_callback {
        OPENAI_CODEX_LOCAL_CALLBACK_URI.to_string()
    } else if use_control_panel_callback {
        provider_oauth_redirect_uri_for_control_panel_base(callback_base_url, &provider_id)
    } else {
        provider_oauth_redirect_uri_for_base(callback_base_url, &provider_id)
    };
    let authorization_url =
        build_openai_codex_authorization_url(&redirect_uri, &code_challenge, &state_token);

    state.oauth.provider_sessions().write().await.insert(
        session_id.clone(),
        ProviderOAuthSessionRecord {
            session_id: session_id.clone(),
            provider_id,
            status: "pending".to_string(),
            created_at_ms,
            expires_at_ms,
            redirect_uri,
            state: state_token,
            code_verifier,
            authorization_url: authorization_url.clone(),
            error: None,
            email: None,
            tenant_context,
        },
    );

    Json(json!({
        "ok": true,
        "provider_id": OPENAI_CODEX_PROVIDER_ID,
        "session_id": session_id,
        "authorizationUrl": authorization_url,
        "expires_at_ms": expires_at_ms,
    }))
}

pub(super) async fn provider_oauth_status(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
    Query(query): Query<ProviderOAuthStatusQuery>,
) -> Json<Value> {
    let provider_id = canonical_provider_id(&id);
    if provider_id != OPENAI_CODEX_PROVIDER_ID {
        return Json(json!({
            "ok": false,
            "error": format!("oauth is not supported for provider `{provider_id}`"),
        }));
    }

    let _ = refresh_openai_codex_oauth_if_needed(&state, &tenant_context).await;

    if let Some(session_id) = query.session_id.as_deref() {
        if let Some(session) = state.oauth.provider_sessions().read().await.get(session_id) {
            if session.tenant_context != tenant_context {
                return Json(json!({
                    "ok": true,
                    "status": "missing",
                    "connected": false,
                    "local_session_available": tenant_context.is_local_implicit()
                        && tandem_core::load_openai_codex_cli_oauth_credential().is_some(),
                }));
            }
            return Json(json!({
                "ok": true,
                "session_id": session.session_id,
                "status": session.status,
                "error": session.error,
                "email": session.email,
                "expires_at_ms": session.expires_at_ms,
            }));
        }
    }

    let provider_auth_security_dir = provider_auth_security_dir_for_state(&state);
    if let Some(tandem_core::ProviderCredential::OAuth(oauth)) =
        tandem_core::load_provider_credentials_for_tenant_in_dir(
            &provider_auth_security_dir,
            &tenant_context,
        )
        .remove(OPENAI_CODEX_PROVIDER_ID)
    {
        return Json(json!({
            "ok": true,
            "status": if oauth.expires_at_ms <= crate::now_ms() { "reauth_required" } else { "connected" },
            "connected": oauth.expires_at_ms > crate::now_ms(),
            "email": oauth.email,
            "display_name": oauth.display_name,
            "managed_by": oauth.managed_by,
            "account_id": oauth.account_id,
            "expires_at_ms": oauth.expires_at_ms,
            "local_session_available": tenant_context.is_local_implicit()
                && tandem_core::load_openai_codex_cli_oauth_credential().is_some(),
        }));
    }

    Json(json!({
        "ok": true,
        "status": "missing",
        "connected": false,
        "local_session_available": tenant_context.is_local_implicit()
            && tandem_core::load_openai_codex_cli_oauth_credential().is_some(),
    }))
}

pub(super) async fn provider_oauth_callback_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(input): Query<ProviderOAuthCallbackInput>,
) -> Response {
    let payload = finish_provider_oauth_callback(state, id, input).await;
    let ok = payload.get("ok").and_then(Value::as_bool).unwrap_or(false);
    let title = if ok {
        "Codex Account Connected"
    } else {
        "Codex Account Connection Failed"
    };
    let detail = if ok {
        payload
            .get("email")
            .and_then(Value::as_str)
            .map(|email| {
                format!("Connected as {email}. You can close this tab and return to Tandem.")
            })
            .unwrap_or_else(|| {
                "Codex account connected. You can close this tab and return to Tandem.".to_string()
            })
    } else {
        payload
            .get("error")
            .and_then(Value::as_str)
            .unwrap_or("The OAuth callback could not be completed.")
            .to_string()
    };

    Html(render_provider_oauth_result_page(&title, &detail, ok)).into_response()
}

fn escape_html(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn render_provider_oauth_result_page(title: &str, detail: &str, ok: bool) -> String {
    let title = escape_html(title);
    let detail = escape_html(detail);
    let status_text = if ok {
        "Connected"
    } else {
        "Connection needs attention"
    };
    let status_tone = if ok { "#4ade80" } else { "#fb7185" };
    let status_border = if ok {
        "rgba(74, 222, 128, 0.35)"
    } else {
        "rgba(251, 113, 133, 0.35)"
    };
    let accent_glow = if ok {
        "rgba(59, 130, 246, 0.28)"
    } else {
        "rgba(244, 63, 94, 0.28)"
    };
    let icon_path = if ok {
        r#"<path d="M12 2a10 10 0 1 0 10 10A10 10 0 0 0 12 2Zm4.3 7.7-4.9 5a1 1 0 0 1-.7.3 1 1 0 0 1-.7-.3l-2.4-2.4a1 1 0 1 1 1.4-1.4l1.7 1.7 4.2-4.3a1 1 0 1 1 1.4 1.4Z"/>"#
    } else {
        r#"<path d="M12 2a10 10 0 1 0 10 10A10 10 0 0 0 12 2Zm4.2 12.8a1 1 0 1 1-1.4 1.4L12 13.4l-2.8 2.8a1 1 0 0 1-1.4-1.4l2.8-2.8-2.8-2.8a1 1 0 0 1 1.4-1.4l2.8 2.8 2.8-2.8a1 1 0 1 1 1.4 1.4L13.4 12l2.8 2.8Z"/>"#
    };

    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <style>
    :root {{
      color-scheme: dark;
      --bg: #030712;
      --bg-2: #0a1020;
      --card: rgba(8, 12, 24, 0.98);
      --border: rgba(148, 163, 184, 0.15);
      --text: #ecf2ff;
      --muted: #a9b8d4;
      --accent: {status_tone};
      --accent-bg: rgba(15, 23, 42, 0.82);
      --accent-border: {status_border};
      --accent-glow: {accent_glow};
    }}
    * {{ box-sizing: border-box; }}
    html, body {{ min-height: 100%; }}
    body {{
      margin: 0;
      min-height: 100vh;
      display: grid;
      place-items: center;
      padding: 24px;
      font-family: Inter, ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
      color: var(--text);
      background:
        radial-gradient(circle at top left, rgba(37, 99, 235, 0.12), transparent 24%),
        radial-gradient(circle at top right, rgba(79, 70, 229, 0.12), transparent 28%),
        linear-gradient(180deg, var(--bg) 0%, var(--bg-2) 100%);
    }}
    body::before {{
      content: "";
      position: fixed;
      inset: 0;
      pointer-events: none;
      background-image:
        linear-gradient(rgba(148, 163, 184, 0.035) 1px, transparent 1px),
        linear-gradient(90deg, rgba(148, 163, 184, 0.035) 1px, transparent 1px);
      background-size: 40px 40px;
      mask-image: linear-gradient(to bottom, rgba(0, 0, 0, 0.65), transparent);
      opacity: 0.5;
    }}
    .shell {{
      position: relative;
      width: min(720px, 100%);
    }}
    .card {{
      position: relative;
      overflow: hidden;
      border-radius: 0;
      border: 1px solid var(--border);
      background: linear-gradient(180deg, rgba(8, 12, 24, 0.98), rgba(5, 8, 16, 0.98));
      box-shadow: 0 28px 70px rgba(0, 0, 0, 0.5);
      padding: 32px;
      border-top: 3px solid var(--accent);
    }}
    .card::before {{
      content: "";
      position: absolute;
      left: 0;
      top: 0;
      right: 0;
      height: 1px;
      background: linear-gradient(90deg, transparent, var(--accent-glow), transparent);
      pointer-events: none;
    }}
    .brand {{
      display: flex;
      align-items: center;
      gap: 14px;
      margin-bottom: 28px;
      position: relative;
      z-index: 1;
    }}
    .mark {{
      width: 46px;
      height: 46px;
      border-radius: 0;
      display: grid;
      place-items: center;
      flex: none;
      background: rgba(37, 99, 235, 0.14);
      border: 1px solid rgba(59, 130, 246, 0.4);
    }}
    .mark svg {{
      width: 28px;
      height: 28px;
      fill: white;
    }}
    .brand-copy {{
      display: flex;
      flex-direction: column;
      gap: 4px;
      min-width: 0;
    }}
    .eyebrow {{
      font-size: 12px;
      letter-spacing: 0.16em;
      text-transform: uppercase;
      color: var(--muted);
    }}
    .brand-copy strong {{
      font-size: 18px;
      font-weight: 700;
      letter-spacing: -0.01em;
    }}
    .badge {{
      margin-left: auto;
      padding: 8px 12px;
      border-radius: 0;
      border: 1px solid var(--accent-border);
      background: var(--accent-bg);
      color: var(--accent);
      font-size: 13px;
      font-weight: 700;
      white-space: nowrap;
    }}
    h1 {{
      position: relative;
      z-index: 1;
      margin: 0 0 14px;
      font-size: clamp(28px, 4vw, 40px);
      line-height: 1.08;
      letter-spacing: -0.04em;
    }}
    p {{
      position: relative;
      z-index: 1;
      margin: 0;
      max-width: 60ch;
      font-size: 16px;
      line-height: 1.7;
      color: var(--muted);
    }}
    .foot {{
      position: relative;
      z-index: 1;
      margin-top: 28px;
      display: flex;
      flex-wrap: wrap;
      gap: 12px;
      color: #93a5c7;
      font-size: 13px;
      border-top: 1px solid rgba(148, 163, 184, 0.12);
      padding-top: 16px;
    }}
    .pill {{
      display: inline-flex;
      align-items: center;
      gap: 8px;
      padding: 10px 14px;
      border-radius: 0;
      border: 1px solid var(--border);
      background: rgba(15, 23, 42, 0.56);
    }}
    .actions {{
      position: relative;
      z-index: 1;
      margin-top: 28px;
      display: flex;
      flex-wrap: wrap;
      gap: 12px;
    }}
    .button {{
      display: inline-flex;
      align-items: center;
      justify-content: center;
      min-height: 44px;
      padding: 0 16px;
      border-radius: 0;
      text-decoration: none;
      font-weight: 700;
      font-size: 14px;
    }}
    .button.primary {{
      color: white;
      background: linear-gradient(135deg, #2563eb, #4f46e5);
      box-shadow: 0 12px 24px rgba(37, 99, 235, 0.24);
    }}
    .button.secondary {{
      color: var(--text);
      border: 1px solid var(--border);
      background: rgba(15, 23, 42, 0.58);
    }}
    @media (max-width: 640px) {{
      body {{ padding: 16px; }}
      .card {{ padding: 24px; border-radius: 0; }}
      .brand {{ align-items: flex-start; flex-direction: column; }}
      .badge {{ margin-left: 0; }}
    }}
  </style>
</head>
<body>
  <main class="shell">
    <section class="card" aria-live="polite">
      <div class="brand">
        <div class="mark" aria-hidden="true">
          <svg viewBox="0 0 24 24" role="presentation" focusable="false">{icon_path}</svg>
        </div>
        <div class="brand-copy">
          <div class="eyebrow">Tandem</div>
          <strong>Codex account</strong>
        </div>
        <div class="badge">{status_text}</div>
      </div>
      <h1>{title}</h1>
      <p>{detail}</p>
      <div class="actions">
        <div class="pill">You can close this tab and return to Tandem.</div>
        <div class="pill">Browser sign-in completed through Tandem's local auth bridge.</div>
      </div>
      <div class="foot">
        <span>Secure browser handoff</span>
        <span>•</span>
        <span>Local OAuth callback</span>
      </div>
    </section>
  </main>
</body>
</html>"#,
        title = title,
        detail = detail,
        status_text = status_text,
        status_tone = status_tone,
        accent_glow = accent_glow,
        icon_path = icon_path
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_codex_authorization_url_matches_codex_cli_flow() {
        let url = build_openai_codex_authorization_url(
            "http://localhost:1455/auth/callback",
            "challenge123",
            "state123",
        );
        assert!(url.starts_with("https://auth.openai.com/oauth/authorize?"));
        assert!(url.contains("redirect_uri=http%3A%2F%2Flocalhost%3A1455%2Fauth%2Fcallback"));
        assert!(url.contains("scope=openid%20profile%20email%20offline_access"));
        assert!(url.contains("code_challenge=challenge123"));
        assert!(url.contains("code_challenge_method=S256"));
        assert!(url.contains("id_token_add_organizations=true"));
        assert!(url.contains("codex_cli_simplified_flow=true"));
        assert!(url.contains("state=state123"));
        assert!(!url.contains("originator="));
        assert!(!url.contains("api.connectors.read"));
        assert!(!url.contains("api.connectors.invoke"));
    }

    #[test]
    fn hosted_codex_redirect_uses_control_panel_callback_route() {
        let url = provider_oauth_redirect_uri_for_control_panel_base(
            "https://test.hosted.tandem.ac",
            OPENAI_CODEX_PROVIDER_ID,
        );
        assert_eq!(
            url,
            "https://test.hosted.tandem.ac/api/engine/provider/openai-codex/oauth/callback"
        );
    }

    #[test]
    fn oauth_callback_page_escapes_untrusted_text() {
        let html = render_provider_oauth_result_page(
            "Connected <script>",
            "Email: user@example.com & friends",
            true,
        );
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("&amp; friends"));
        assert!(!html.contains("<script>"));
    }
}

pub(super) async fn provider_oauth_callback_post(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProviderOAuthCallbackInput>,
) -> Response {
    let payload = finish_provider_oauth_callback(state, id, input).await;
    let status = if payload.get("code").and_then(Value::as_str)
        == Some("PROTECTED_AUDIT_PERSISTENCE_FAILED")
    {
        StatusCode::INTERNAL_SERVER_ERROR
    } else {
        StatusCode::OK
    };
    (status, Json(payload)).into_response()
}

async fn openai_codex_local_callback_get(
    State(state): State<AppState>,
    Query(input): Query<ProviderOAuthCallbackInput>,
) -> Response {
    let response = provider_oauth_callback_get(
        State(state),
        Path(OPENAI_CODEX_PROVIDER_ID.to_string()),
        Query(input),
    )
    .await;
    stop_openai_codex_local_callback_server();
    response
}

async fn openai_codex_local_callback_post(
    State(state): State<AppState>,
    Json(input): Json<ProviderOAuthCallbackInput>,
) -> Response {
    let response = provider_oauth_callback_post(
        State(state),
        Path(OPENAI_CODEX_PROVIDER_ID.to_string()),
        Json(input),
    )
    .await;
    stop_openai_codex_local_callback_server();
    response
}

pub(super) async fn provider_oauth_disconnect(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    Path(id): Path<String>,
) -> Response {
    let provider_id = canonical_provider_id(&id);
    if provider_id != OPENAI_CODEX_PROVIDER_ID {
        return Json(json!({
            "ok": false,
            "error": format!("oauth is not supported for provider `{provider_id}`"),
        }))
        .into_response();
    }

    let mut credential_guard = state
        .oauth
        .provider_credential_guard(&tenant_context, OPENAI_CODEX_PROVIDER_ID)
        .await;
    let snapshot = snapshot_openai_codex_oauth_mutation(&state, &tenant_context).await;
    credential_guard.advance_generation();
    let persisted_removed =
        match delete_openai_codex_oauth_credential(&state, &tenant_context).await {
            Ok(removed) => removed,
            Err(error) => {
                return Json(json!({ "ok": false, "error": error.to_string() })).into_response();
            }
        };
    state
        .providers
        .clear_tenant_provider_bearer_token(&tenant_context, OPENAI_CODEX_PROVIDER_ID)
        .await;
    let mut runtime_removed = false;
    if tenant_context.is_local_implicit() {
        runtime_removed = state
            .config
            .delete_runtime_provider_key(OPENAI_CODEX_PROVIDER_ID)
            .await
            .is_ok();
        state.auth.write().await.remove(OPENAI_CODEX_PROVIDER_ID);
        stop_openai_codex_local_callback_server();
    }

    if persisted_removed || runtime_removed {
        if let Err(error) = crate::audit::append_protected_audit_event(
            &state,
            "provider.oauth.deleted",
            &tenant_context,
            request_actor(&request_principal, &tenant_context),
            json!({
                "providerID": OPENAI_CODEX_PROVIDER_ID,
                "runtimeRemoved": runtime_removed,
                "persistedRemoved": persisted_removed,
            }),
        )
        .await
        {
            if let Err(rollback_error) =
                restore_openai_codex_oauth_mutation(&state, &tenant_context, &snapshot).await
            {
                tracing::error!(
                    error = ?rollback_error,
                    "failed to restore provider OAuth state after protected audit failure"
                );
            }
            return super::protected_audit_error_response(error).into_response();
        }
        ensure_openai_codex_runtime_provider(&state).await;
        state
            .providers
            .reload(state.config.get().await.into())
            .await;
    }

    Json(json!({
        "ok": persisted_removed || runtime_removed,
    }))
    .into_response()
}

pub(super) async fn provider_oauth_local_session(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    Path(id): Path<String>,
) -> Response {
    if !tenant_context.is_local_implicit() {
        return (
            StatusCode::FORBIDDEN,
            Json(json!({
                "ok": false,
                "code": "LOCAL_PROVIDER_SESSION_REQUIRES_LOCAL_IMPLICIT_TENANT",
                "error": "Local provider sessions are available only to the local implicit tenant.",
            })),
        )
            .into_response();
    }

    let provider_id = canonical_provider_id(&id);
    if provider_id != OPENAI_CODEX_PROVIDER_ID {
        return Json(json!({
            "ok": false,
            "error": format!("oauth is not supported for provider `{provider_id}`"),
        }))
        .into_response();
    }

    let mut credential_guard = state
        .oauth
        .provider_credential_guard(&tenant_context, OPENAI_CODEX_PROVIDER_ID)
        .await;
    let Some(credential) = tandem_core::load_openai_codex_cli_oauth_credential() else {
        return Json(json!({
            "ok": false,
            "error": "No local Codex CLI session was found. Sign in with the Codex CLI on this machine first.",
        }))
        .into_response();
    };

    let snapshot = snapshot_openai_codex_oauth_mutation(&state, &tenant_context).await;
    credential_guard.advance_generation();
    let backend =
        match persist_openai_codex_oauth_credential(&state, &tenant_context, credential.clone())
            .await
        {
            Ok(tandem_core::ProviderAuthBackend::Keychain) => "keychain",
            Ok(tandem_core::ProviderAuthBackend::File) => "file",
            Err(error) => {
                let _ =
                    restore_openai_codex_oauth_mutation(&state, &tenant_context, &snapshot).await;
                return Json(json!({
                    "ok": false,
                    "error": error.to_string(),
                }))
                .into_response();
            }
        };
    apply_openai_codex_runtime_credential(&state, &tenant_context, Some(&credential), true).await;

    if let Err(error) = crate::audit::append_protected_audit_event(
        &state,
        "provider.oauth.updated",
        &tenant_context,
        request_actor(&request_principal, &tenant_context),
        json!({
            "providerID": OPENAI_CODEX_PROVIDER_ID,
            "backend": backend,
            "managedBy": "codex-cli",
            "email": credential.email,
        }),
    )
    .await
    {
        if let Err(rollback_error) =
            restore_openai_codex_oauth_mutation(&state, &tenant_context, &snapshot).await
        {
            tracing::error!(
                error = ?rollback_error,
                "failed to restore provider OAuth state after protected audit failure"
            );
        }
        return super::protected_audit_error_response(error).into_response();
    }

    Json(json!({
        "ok": true,
        "provider_id": OPENAI_CODEX_PROVIDER_ID,
        "managed_by": "codex-cli",
        "backend": backend,
        "email": credential.email,
        "display_name": credential.display_name,
        "expires_at_ms": credential.expires_at_ms,
    }))
    .into_response()
}

pub(super) async fn provider_oauth_session_import(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    Path(id): Path<String>,
    Json(input): Json<ProviderOAuthSessionImportInput>,
) -> Response {
    let provider_id = canonical_provider_id(&id);
    if provider_id != OPENAI_CODEX_PROVIDER_ID {
        return Json(json!({
            "ok": false,
            "error": format!("oauth is not supported for provider `{provider_id}`"),
        }))
        .into_response();
    }

    let raw_auth_json = input.auth_json.unwrap_or_default();
    let trimmed_auth_json = raw_auth_json.trim();
    if trimmed_auth_json.is_empty() {
        return Json(json!({
            "ok": false,
            "error": "Codex auth.json content cannot be empty.",
        }))
        .into_response();
    }

    let auth_json: Value = match serde_json::from_str(trimmed_auth_json) {
        Ok(value) => value,
        Err(error) => {
            return Json(json!({
                "ok": false,
                "error": format!("Codex auth.json is not valid JSON: {error}"),
            }))
            .into_response();
        }
    };

    let Some(mut credential) = tandem_core::oauth_credential_from_codex_auth_json(&auth_json)
    else {
        return Json(json!({
            "ok": false,
            "error": "The imported Codex auth.json could not be parsed into a credential.",
        }))
        .into_response();
    };
    credential.managed_by = "codex-upload".to_string();

    let mut credential_guard = state
        .oauth
        .provider_credential_guard(&tenant_context, OPENAI_CODEX_PROVIDER_ID)
        .await;
    let snapshot = snapshot_openai_codex_oauth_mutation(&state, &tenant_context).await;
    let cli_auth_snapshot = tenant_context
        .is_local_implicit()
        .then(tandem_core::load_openai_codex_cli_auth_json)
        .flatten();
    credential_guard.advance_generation();
    if tenant_context.is_local_implicit() {
        if let Err(error) = tandem_core::write_openai_codex_cli_auth_json(&auth_json) {
            return Json(json!({
                "ok": false,
                "error": format!("failed to persist Codex auth.json: {error}"),
            }))
            .into_response();
        }
    }
    let backend =
        match persist_openai_codex_oauth_credential(&state, &tenant_context, credential.clone())
            .await
        {
            Ok(tandem_core::ProviderAuthBackend::Keychain) => "keychain",
            Ok(tandem_core::ProviderAuthBackend::File) => "file",
            Err(error) => {
                let _ =
                    restore_openai_codex_oauth_mutation(&state, &tenant_context, &snapshot).await;
                if tenant_context.is_local_implicit() {
                    let _ =
                        tandem_core::restore_openai_codex_cli_auth_json(cli_auth_snapshot.as_ref());
                }
                return Json(json!({
                    "ok": false,
                    "error": error.to_string(),
                }))
                .into_response();
            }
        };
    apply_openai_codex_runtime_credential(&state, &tenant_context, Some(&credential), true).await;

    if let Err(error) = crate::audit::append_protected_audit_event(
        &state,
        "provider.oauth.updated",
        &tenant_context,
        request_actor(&request_principal, &tenant_context),
        json!({
            "providerID": OPENAI_CODEX_PROVIDER_ID,
            "backend": backend,
            "managedBy": "codex-upload",
            "email": credential.email,
        }),
    )
    .await
    {
        if let Err(rollback_error) =
            restore_openai_codex_oauth_mutation(&state, &tenant_context, &snapshot).await
        {
            tracing::error!(
                error = ?rollback_error,
                "failed to restore provider OAuth state after protected audit failure"
            );
        }
        if tenant_context.is_local_implicit() {
            if let Err(rollback_error) =
                tandem_core::restore_openai_codex_cli_auth_json(cli_auth_snapshot.as_ref())
            {
                tracing::error!(
                    error = ?rollback_error,
                    "failed to restore Codex CLI auth after protected audit failure"
                );
            }
        }
        return super::protected_audit_error_response(error).into_response();
    }

    Json(json!({
        "ok": true,
        "provider_id": OPENAI_CODEX_PROVIDER_ID,
        "managed_by": "codex-upload",
        "backend": backend,
        "email": credential.email,
        "display_name": credential.display_name,
        "expires_at_ms": credential.expires_at_ms,
    }))
    .into_response()
}

pub(super) async fn set_auth(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    Path(id): Path<String>,
    Json(input): Json<AuthInput>,
) -> Response {
    let normalized_id = id.trim().to_ascii_lowercase();
    if normalized_id.is_empty() {
        return Json(json!({"ok": false, "error": "provider id cannot be empty"})).into_response();
    }
    let token = input.token.unwrap_or_default().trim().to_string();
    if token.is_empty() {
        return Json(json!({"ok": false, "error": "token cannot be empty"})).into_response();
    }

    let mut credential_guard = state
        .oauth
        .provider_credential_guard(&tenant_context, &normalized_id)
        .await;
    let _persistence_guard = state.oauth.provider_credential_persistence_guard().await;
    let provider_auth_security_dir = provider_auth_security_dir_for_state(&state);
    let snapshot = snapshot_api_key_mutation(
        &state,
        &provider_auth_security_dir,
        &tenant_context,
        &normalized_id,
    )
    .await;
    credential_guard.advance_generation();
    let backend = match tandem_core::set_provider_auth_for_tenant_in_dir(
        &provider_auth_security_dir,
        &tenant_context,
        &normalized_id,
        &token,
    ) {
        Ok(tandem_core::ProviderAuthBackend::Keychain) => "keychain",
        Ok(tandem_core::ProviderAuthBackend::File) => "file",
        Err(err) => {
            rollback_api_key_mutation(
                &state,
                &provider_auth_security_dir,
                &tenant_context,
                &normalized_id,
                &snapshot,
                "provider API-key persistence failure",
            )
            .await;
            return provider_auth_mutation_error_response(
                &normalized_id,
                "PROVIDER_AUTH_PERSISTENCE_FAILED",
                format!("failed to persist provider auth: {err}"),
            );
        }
    };

    if tenant_context.is_local_implicit() {
        let patch = json!({
            "providers": {
                normalized_id.clone(): {
                    "api_key": token.clone()
                }
            }
        });
        if let Err(error) = state.config.patch_runtime(patch).await {
            rollback_api_key_mutation(
                &state,
                &provider_auth_security_dir,
                &tenant_context,
                &normalized_id,
                &snapshot,
                "provider API-key runtime patch failure",
            )
            .await;
            return provider_auth_mutation_error_response(
                &normalized_id,
                "PROVIDER_AUTH_RUNTIME_PATCH_FAILED",
                format!("failed to update provider runtime auth: {error}"),
            );
        }
        // Keep the legacy in-memory map in sync with the runtime config.
        state
            .auth
            .write()
            .await
            .insert(normalized_id.clone(), token);
        state
            .providers
            .reload(state.config.get().await.into())
            .await;
    }
    if let Err(error) = crate::audit::append_protected_audit_event(
        &state,
        "provider.secret.updated",
        &tenant_context,
        request_actor(&request_principal, &tenant_context),
        json!({
            "providerID": normalized_id,
            "backend": backend,
        }),
    )
    .await
    {
        rollback_api_key_mutation(
            &state,
            &provider_auth_security_dir,
            &tenant_context,
            &normalized_id,
            &snapshot,
            "provider API-key audit failure",
        )
        .await;
        return super::protected_audit_error_response(error).into_response();
    }
    Json(json!({"ok": true, "id": normalized_id, "backend": backend})).into_response()
}

pub(super) async fn delete_auth(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    Path(id): Path<String>,
) -> Response {
    let normalized_id = id.trim().to_ascii_lowercase();
    if normalized_id.is_empty() {
        return Json(json!({"ok": false, "error": "provider id cannot be empty"})).into_response();
    }

    let mut credential_guard = state
        .oauth
        .provider_credential_guard(&tenant_context, &normalized_id)
        .await;
    let _persistence_guard = state.oauth.provider_credential_persistence_guard().await;
    let provider_auth_security_dir = provider_auth_security_dir_for_state(&state);
    let snapshot = snapshot_api_key_mutation(
        &state,
        &provider_auth_security_dir,
        &tenant_context,
        &normalized_id,
    )
    .await;
    credential_guard.advance_generation();
    let persisted_removed = match tandem_core::delete_provider_auth_for_tenant_in_dir(
        &provider_auth_security_dir,
        &tenant_context,
        &normalized_id,
    ) {
        Ok(removed) => removed,
        Err(error) => {
            rollback_api_key_mutation(
                &state,
                &provider_auth_security_dir,
                &tenant_context,
                &normalized_id,
                &snapshot,
                "provider API-key delete persistence failure",
            )
            .await;
            return provider_auth_mutation_error_response(
                &normalized_id,
                "PROVIDER_AUTH_PERSISTENCE_FAILED",
                format!("failed to delete persisted provider auth: {error}"),
            );
        }
    };

    let runtime_removed = tenant_context.is_local_implicit()
        && (snapshot.local_auth.is_some() || snapshot.runtime_api_key.is_some());
    if tenant_context.is_local_implicit() {
        if let Err(error) = state
            .config
            .delete_runtime_provider_key(&normalized_id)
            .await
        {
            rollback_api_key_mutation(
                &state,
                &provider_auth_security_dir,
                &tenant_context,
                &normalized_id,
                &snapshot,
                "provider API-key runtime delete failure",
            )
            .await;
            return provider_auth_mutation_error_response(
                &normalized_id,
                "PROVIDER_AUTH_RUNTIME_PATCH_FAILED",
                format!("failed to delete provider runtime auth: {error}"),
            );
        }
        state.auth.write().await.remove(&normalized_id);
    }
    if runtime_removed || persisted_removed {
        if let Err(error) = crate::audit::append_protected_audit_event(
            &state,
            "provider.secret.deleted",
            &tenant_context,
            request_actor(&request_principal, &tenant_context),
            json!({
                "providerID": normalized_id,
                "runtimeRemoved": runtime_removed,
                "persistedRemoved": persisted_removed,
            }),
        )
        .await
        {
            rollback_api_key_mutation(
                &state,
                &provider_auth_security_dir,
                &tenant_context,
                &normalized_id,
                &snapshot,
                "provider API-key delete audit failure",
            )
            .await;
            return super::protected_audit_error_response(error).into_response();
        }
    }
    if runtime_removed {
        state
            .providers
            .reload(state.config.get().await.into())
            .await;
    }
    Json(json!({"ok": runtime_removed || persisted_removed})).into_response()
}

struct ApiKeyMutationSnapshot {
    persisted: Option<String>,
    local_auth: Option<String>,
    runtime_api_key: Option<String>,
}

async fn snapshot_api_key_mutation(
    state: &AppState,
    security_dir: &std::path::Path,
    tenant_context: &TenantContext,
    provider_id: &str,
) -> ApiKeyMutationSnapshot {
    let persisted = tandem_core::load_provider_auth_for_tenant_in_dir(security_dir, tenant_context)
        .remove(provider_id);
    if !tenant_context.is_local_implicit() {
        return ApiKeyMutationSnapshot {
            persisted,
            local_auth: None,
            runtime_api_key: None,
        };
    }

    let local_auth = state.auth.read().await.get(provider_id).cloned();
    let layers = state.config.get_layers_value().await;
    let runtime_api_key = layers
        .get("runtime")
        .and_then(|runtime| runtime.get("providers"))
        .and_then(Value::as_object)
        .and_then(|providers| {
            providers
                .get(provider_id)
                .or_else(|| {
                    providers
                        .iter()
                        .find(|(id, _)| id.eq_ignore_ascii_case(provider_id))
                        .map(|(_, value)| value)
                })
                .and_then(Value::as_object)
        })
        .and_then(|provider| provider.get("api_key").or_else(|| provider.get("apiKey")))
        .and_then(Value::as_str)
        .map(str::to_string);
    ApiKeyMutationSnapshot {
        persisted,
        local_auth,
        runtime_api_key,
    }
}

async fn restore_api_key_mutation(
    state: &AppState,
    security_dir: &std::path::Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    snapshot: &ApiKeyMutationSnapshot,
) -> anyhow::Result<()> {
    let persistence_result = if let Some(token) = snapshot.persisted.as_deref() {
        tandem_core::set_provider_auth_for_tenant_in_dir(
            security_dir,
            tenant_context,
            provider_id,
            token,
        )
        .map(|_| ())
    } else {
        tandem_core::delete_provider_auth_for_tenant_in_dir(
            security_dir,
            tenant_context,
            provider_id,
        )
        .map(|_| ())
    };

    let runtime_result = if tenant_context.is_local_implicit() {
        let result = async {
            state
                .config
                .delete_runtime_provider_key(provider_id)
                .await?;
            if let Some(token) = snapshot.runtime_api_key.as_deref() {
                state
                    .config
                    .patch_runtime(json!({
                        "providers": {
                            provider_id.to_string(): { "api_key": token }
                        }
                    }))
                    .await?;
            }
            let mut auth = state.auth.write().await;
            match snapshot.local_auth.as_ref() {
                Some(token) => {
                    auth.insert(provider_id.to_string(), token.clone());
                }
                None => {
                    auth.remove(provider_id);
                }
            }
            drop(auth);
            state
                .providers
                .reload(state.config.get().await.into())
                .await;
            Ok(())
        }
        .await;
        result
    } else {
        Ok(())
    };

    persistence_result?;
    runtime_result
}

async fn rollback_api_key_mutation(
    state: &AppState,
    security_dir: &std::path::Path,
    tenant_context: &TenantContext,
    provider_id: &str,
    snapshot: &ApiKeyMutationSnapshot,
    context: &'static str,
) {
    if let Err(error) =
        restore_api_key_mutation(state, security_dir, tenant_context, provider_id, snapshot).await
    {
        tracing::error!(error = ?error, provider_id, context, "failed to restore provider API-key mutation");
    }
}

fn provider_auth_mutation_error_response(
    provider_id: &str,
    code: &'static str,
    error: String,
) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "ok": false,
            "id": provider_id,
            "code": code,
            "error": error,
        })),
    )
        .into_response()
}

pub(super) async fn set_api_token(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Json(input): Json<ApiTokenInput>,
) -> Json<Value> {
    if let Err(error) = authorize_api_token_management(&state, &headers).await {
        return Json(error);
    }
    let token = input.token.unwrap_or_default().trim().to_string();
    if token.is_empty() {
        return Json(json!({
            "ok": false,
            "error": "token cannot be empty"
        }));
    }
    state.set_api_token(Some(token)).await;
    Json(json!({"ok": true}))
}

pub(super) async fn clear_api_token(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Json<Value> {
    if let Err(error) = authorize_api_token_management(&state, &headers).await {
        return Json(error);
    }
    Json(json!({
        "ok": false,
        "error": "clearing the API token is disabled because it would reopen the HTTP API; use /auth/token/generate to rotate it"
    }))
}

pub(super) async fn generate_api_token(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Json<Value> {
    if let Err(error) = authorize_api_token_management(&state, &headers).await {
        return Json(error);
    }
    let token = format!("tk_{}", Uuid::new_v4().simple());
    state.set_api_token(Some(token.clone())).await;
    Json(json!({
        "ok": true,
        "token": token
    }))
}

async fn authorize_api_token_management(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Result<(), Value> {
    if state.api_token().await.is_some() {
        return Ok(());
    }
    let expected = std::env::var("TANDEM_TOKEN_BOOTSTRAP_SECRET")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| json!({
            "ok": false,
            "error": "api token management requires TANDEM_TOKEN_BOOTSTRAP_SECRET before a token exists",
            "code": "TOKEN_BOOTSTRAP_REQUIRED"
        }))?;
    let provided = headers
        .get("x-tandem-bootstrap-token")
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            json!({
                "ok": false,
                "error": "missing bootstrap token",
                "code": "TOKEN_BOOTSTRAP_REQUIRED"
            })
        })?;
    if constant_time_token_eq(provided, &expected) {
        Ok(())
    } else {
        Err(json!({
            "ok": false,
            "error": "invalid bootstrap token",
            "code": "TOKEN_BOOTSTRAP_DENIED"
        }))
    }
}

fn constant_time_token_eq(provided: &str, expected: &str) -> bool {
    let provided_hash = Sha256::digest(provided.as_bytes());
    let expected_hash = Sha256::digest(expected.as_bytes());
    let mut diff = 0u8;
    for (left, right) in provided_hash.iter().zip(expected_hash.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

#[derive(Debug)]
enum ProviderCatalogFetchResult {
    Remote {
        models: HashMap<String, WireProviderModel>,
    },
    Static {
        models: HashMap<String, WireProviderModel>,
    },
    Unavailable {
        message: String,
    },
    Error {
        message: String,
    },
}
