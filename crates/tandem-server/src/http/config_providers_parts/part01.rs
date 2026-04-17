use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Digest;
use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
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
const OPENAI_CODEX_DEFAULT_MODEL: &str = "gpt-5.4";
const OPENAI_CODEX_API_BASE_URL: &str = "https://chatgpt.com/backend-api/codex";
const OPENAI_CODEX_OAUTH_REFRESH_SKEW_MS: u64 = 5 * 60 * 1000;
const OPENAI_CODEX_LOCAL_CALLBACK_ADDR: &str = "127.0.0.1:1455";
// Match the Codex CLI browser flow. auth.openai.com expects this localhost callback shape.
const OPENAI_CODEX_LOCAL_CALLBACK_URI: &str = "http://localhost:1455/auth/callback";

fn is_internal_secret_id(raw: &str) -> bool {
    let normalized = raw.trim().to_ascii_lowercase();
    normalized.starts_with("mcp_header::") || normalized.starts_with("channel::")
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
    let effective = match state
        .config
        .patch_project(normalize_config_patch_input(input))
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
    let providers = redacted(cfg.get("providers").cloned().unwrap_or_else(|| json!({})));
    let default_provider = cfg.get("default_provider").cloned().unwrap_or(Value::Null);
    Json(json!({
        "providers": providers,
        "default": default_provider
    }))
}

pub(super) async fn list_providers(State(state): State<AppState>) -> Json<Value> {
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
    let persisted_auth = tandem_core::load_provider_auth();
    let runtime_auth = state.auth.read().await.clone();
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

    for entry in &mut wire.all {
        match fetch_remote_provider_models(
            &entry.id,
            &effective_cfg,
            &runtime_auth,
            &persisted_auth,
        )
        .await
        {
            ProviderCatalogFetchResult::Remote { models } => {
                entry.models = models;
                entry.catalog_source = Some("remote".to_string());
                entry.catalog_status = Some("ok".to_string());
            }
            ProviderCatalogFetchResult::Static { models } => {
                entry.models = models;
                entry.catalog_source = Some("static".to_string());
                entry.catalog_status = Some("ok".to_string());
            }
            ProviderCatalogFetchResult::Unavailable { message } => {
                entry.catalog_source = Some("empty".to_string());
                entry.catalog_status = Some("unavailable".to_string());
                entry.catalog_message = Some(message);
            }
            ProviderCatalogFetchResult::Error { message } => {
                entry.catalog_source = Some("empty".to_string());
                entry.catalog_status = Some("error".to_string());
                entry.catalog_message = Some(message);
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

pub(super) async fn provider_auth(State(state): State<AppState>) -> Json<Value> {
    let _ = refresh_openai_codex_oauth_if_needed(&state).await;
    let cfg = state.config.get_effective_value().await;
    let providers_cfg = cfg
        .get("providers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let persisted = tandem_core::load_provider_auth();
    let persisted_credentials = tandem_core::load_provider_credentials();
    let persisted_ids = persisted
        .keys()
        .filter(|id| !is_internal_secret_id(id))
        .map(|id| id.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let runtime_auth = state.auth.read().await.clone();
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
        let has_runtime_key = provider_id_aliases(&provider_id).iter().any(|alias| {
            runtime_auth
                .get(*alias)
                .map(|token| !token.trim().is_empty())
                .unwrap_or(false)
        });
        let has_config_key = provider_config_value(&cfg, &provider_id)
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
        let has_env_key = provider_has_env_secret(&provider_id);
        let has_persisted_key = provider_id_aliases(&provider_id)
            .iter()
            .any(|alias| persisted_ids.contains(*alias));
        let oauth_credential = provider_id_aliases(&provider_id)
            .iter()
            .find_map(|alias| persisted_credentials.get(*alias))
            .or_else(|| persisted_credentials.get(&provider_id));
        let has_oauth_credential = oauth_credential.is_some();
        let local_session_available = provider_id == OPENAI_CODEX_PROVIDER_ID
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
    if let Err(error) = ensure_openai_codex_local_callback_server(state.clone()).await {
        return Json(json!({
            "ok": false,
            "error": format!("failed to start local Codex callback server: {error}"),
        }));
    }
    let redirect_uri = provider_oauth_redirect_uri(&state, &provider_id);
    let authorization_url =
        build_openai_codex_authorization_url(&redirect_uri, &code_challenge, &state_token);

    state.provider_oauth_sessions.write().await.insert(
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

    let _ = refresh_openai_codex_oauth_if_needed(&state).await;

    if let Some(session_id) = query.session_id.as_deref() {
        if let Some(session) = state.provider_oauth_sessions.read().await.get(session_id) {
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

    if let Some(tandem_core::ProviderCredential::OAuth(oauth)) =
        tandem_core::load_provider_credentials().remove(OPENAI_CODEX_PROVIDER_ID)
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
            "local_session_available": tandem_core::load_openai_codex_cli_oauth_credential().is_some(),
        }));
    }

    Json(json!({
        "ok": true,
        "status": "missing",
        "connected": false,
        "local_session_available": tandem_core::load_openai_codex_cli_oauth_credential().is_some(),
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
) -> Json<Value> {
    Json(finish_provider_oauth_callback(state, id, input).await)
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
) -> Json<Value> {
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
    Path(id): Path<String>,
) -> Json<Value> {
    let provider_id = canonical_provider_id(&id);
    if provider_id != OPENAI_CODEX_PROVIDER_ID {
        return Json(json!({
            "ok": false,
            "error": format!("oauth is not supported for provider `{provider_id}`"),
        }));
    }

    let persisted_removed =
        tandem_core::delete_provider_credential(OPENAI_CODEX_PROVIDER_ID).unwrap_or(false);
    let runtime_removed = state
        .config
        .delete_runtime_provider_key(OPENAI_CODEX_PROVIDER_ID)
        .await
        .is_ok();
    state.auth.write().await.remove(OPENAI_CODEX_PROVIDER_ID);
    stop_openai_codex_local_callback_server();

    if persisted_removed || runtime_removed {
        let _ = crate::audit::append_protected_audit_event(
            &state,
            "provider.oauth.deleted",
            &tandem_types::TenantContext::local_implicit(),
            None,
            json!({
                "providerID": OPENAI_CODEX_PROVIDER_ID,
                "runtimeRemoved": runtime_removed,
                "persistedRemoved": persisted_removed,
            }),
        )
        .await;
        ensure_openai_codex_runtime_provider(&state).await;
        state
            .providers
            .reload(state.config.get().await.into())
            .await;
    }

    Json(json!({
        "ok": persisted_removed || runtime_removed,
    }))
}

pub(super) async fn provider_oauth_local_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let provider_id = canonical_provider_id(&id);
    if provider_id != OPENAI_CODEX_PROVIDER_ID {
        return Json(json!({
            "ok": false,
            "error": format!("oauth is not supported for provider `{provider_id}`"),
        }));
    }

    let Some(credential) = tandem_core::load_openai_codex_cli_oauth_credential() else {
        return Json(json!({
            "ok": false,
            "error": "No local Codex CLI session was found. Sign in with the Codex CLI on this machine first.",
        }));
    };

    let backend = match tandem_core::set_provider_oauth_credential(
        OPENAI_CODEX_PROVIDER_ID,
        credential.clone(),
    ) {
        Ok(tandem_core::ProviderAuthBackend::Keychain) => "keychain",
        Ok(tandem_core::ProviderAuthBackend::File) => "file",
        Err(error) => {
            return Json(json!({
                "ok": false,
                "error": error.to_string(),
            }));
        }
    };

    ensure_openai_codex_runtime_provider(&state).await;
    state
        .providers
        .reload(state.config.get().await.into())
        .await;

    let _ = crate::audit::append_protected_audit_event(
        &state,
        "provider.oauth.updated",
        &tandem_types::TenantContext::local_implicit(),
        None,
        json!({
            "providerID": OPENAI_CODEX_PROVIDER_ID,
            "backend": backend,
            "managedBy": "codex-cli",
            "email": credential.email,
        }),
    )
    .await;

    Json(json!({
        "ok": true,
        "provider_id": OPENAI_CODEX_PROVIDER_ID,
        "managed_by": "codex-cli",
        "backend": backend,
        "email": credential.email,
        "display_name": credential.display_name,
        "expires_at_ms": credential.expires_at_ms,
    }))
}

pub(super) async fn set_auth(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AuthInput>,
) -> Json<Value> {
    let normalized_id = id.trim().to_ascii_lowercase();
    if normalized_id.is_empty() {
        return Json(json!({"ok": false, "error": "provider id cannot be empty"}));
    }
    let token = input.token.unwrap_or_default().trim().to_string();
    if token.is_empty() {
        return Json(json!({"ok": false, "error": "token cannot be empty"}));
    }

    let backend = match tandem_core::set_provider_auth(&normalized_id, &token) {
        Ok(tandem_core::ProviderAuthBackend::Keychain) => "keychain",
        Ok(tandem_core::ProviderAuthBackend::File) => "file",
        Err(err) => {
            return Json(json!({
                "ok": false,
                "id": normalized_id,
                "error": format!("failed to persist provider auth: {err}")
            }));
        }
    };

    // Keep legacy in-memory auth map for compatibility while runtime config
    // becomes the canonical provider-key source.
    state
        .auth
        .write()
        .await
        .insert(normalized_id.clone(), token.clone());

    let patch = json!({
        "providers": {
            normalized_id.clone(): {
                "api_key": token
            }
        }
    });
    let ok = state.config.patch_runtime(patch).await.is_ok();
    if ok {
        state
            .providers
            .reload(state.config.get().await.into())
            .await;
        let _ = crate::audit::append_protected_audit_event(
            &state,
            "provider.secret.updated",
            &tandem_types::TenantContext::local_implicit(),
            None,
            json!({
                "providerID": normalized_id,
                "backend": backend,
            }),
        )
        .await;
    }
    Json(json!({"ok": ok, "id": normalized_id, "backend": backend}))
}

pub(super) async fn delete_auth(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Json<Value> {
    let normalized_id = id.trim().to_ascii_lowercase();
    if normalized_id.is_empty() {
        return Json(json!({"ok": false, "error": "provider id cannot be empty"}));
    }
    let removed = state.auth.write().await.remove(&normalized_id).is_some();
    let persisted_removed = tandem_core::delete_provider_auth(&normalized_id)
        .map(|ok| ok)
        .unwrap_or(false);
    let runtime_removed = state
        .config
        .delete_runtime_provider_key(&normalized_id)
        .await
        .is_ok();
    if runtime_removed || persisted_removed || removed {
        let _ = crate::audit::append_protected_audit_event(
            &state,
            "provider.secret.deleted",
            &tandem_types::TenantContext::local_implicit(),
            None,
            json!({
                "providerID": normalized_id,
                "runtimeRemoved": runtime_removed,
                "persistedRemoved": persisted_removed,
            }),
        )
        .await;
    }
    if runtime_removed {
        state
            .providers
            .reload(state.config.get().await.into())
            .await;
    }
    Json(json!({"ok": removed || runtime_removed || persisted_removed}))
}

pub(super) async fn set_api_token(
    State(state): State<AppState>,
    Json(input): Json<ApiTokenInput>,
) -> Json<Value> {
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

pub(super) async fn clear_api_token(State(state): State<AppState>) -> Json<Value> {
    state.set_api_token(None).await;
    Json(json!({"ok": true}))
}

pub(super) async fn generate_api_token(State(state): State<AppState>) -> Json<Value> {
    let token = format!("tk_{}", Uuid::new_v4().simple());
    state.set_api_token(Some(token.clone())).await;
    Json(json!({
        "ok": true,
        "token": token
    }))
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

fn ensure_provider_entry<'a>(
    wire: &'a mut WireProviderCatalog,
    provider_id: &str,
    provider_name: Option<&str>,
) -> &'a mut WireProviderEntry {
    if let Some(idx) = wire.all.iter().position(|entry| entry.id == provider_id) {
        return &mut wire.all[idx];
    }

    wire.all.push(WireProviderEntry {
        id: provider_id.to_string(),
        name: provider_name.map(|s| s.to_string()),
        models: HashMap::new(),
        catalog_source: None,
        catalog_status: None,
        catalog_message: None,
    });
    wire.all.last_mut().expect("provider entry just inserted")
}

fn canonical_provider_id(provider_id: &str) -> String {
    match provider_id.trim().to_ascii_lowercase().as_str() {
        "llama.cpp" => "llama_cpp".to_string(),
        other => other.to_string(),
    }
}

fn provider_id_aliases(provider_id: &str) -> &'static [&'static str] {
    match canonical_provider_id(provider_id).as_str() {
        "llama_cpp" => &["llama_cpp", "llama.cpp"],
        _ => &[],
    }
}

fn known_provider_name(provider_id: &str) -> Option<&'static str> {
    match canonical_provider_id(provider_id).as_str() {
        "openrouter" => Some("OpenRouter"),
        "openai" => Some("OpenAI"),
        "openai-codex" => Some("OpenAI Codex"),
        "anthropic" => Some("Anthropic"),
        "ollama" => Some("Ollama"),
        "llama_cpp" => Some("llama.cpp"),
        "groq" => Some("Groq"),
        "mistral" => Some("Mistral"),
        "together" => Some("Together"),
        "cohere" => Some("Cohere"),
        "azure" => Some("Azure OpenAI-Compatible"),
        "bedrock" => Some("Bedrock-Compatible"),
        "vertex" => Some("Vertex-Compatible"),
        "copilot" => Some("GitHub Copilot-Compatible"),
        _ => None,
    }
}

fn ensure_known_provider_entries(wire: &mut WireProviderCatalog) {
    for provider_id in [
        "openrouter",
        "openai",
        "openai-codex",
        "anthropic",
        "ollama",
        "llama_cpp",
        "groq",
        "mistral",
        "together",
        "cohere",
        "azure",
        "bedrock",
        "vertex",
        "copilot",
    ] {
        ensure_provider_entry(wire, provider_id, known_provider_name(provider_id));
    }
}

fn merge_provider_model_map(
    wire: &mut WireProviderCatalog,
    provider_id: &str,
    provider_name: Option<&str>,
    models: HashMap<String, WireProviderModel>,
) {
    let entry = ensure_provider_entry(wire, provider_id, provider_name);
    for (model_id, model) in models {
        entry.models.insert(model_id, model);
    }
}

fn config_provider_root<'a>(cfg: &'a Value) -> Option<&'a serde_json::Map<String, Value>> {
    cfg.get("providers")
        .and_then(Value::as_object)
        .or_else(|| cfg.get("provider").and_then(Value::as_object))
}

fn provider_config_value<'a>(cfg: &'a Value, provider_id: &str) -> Option<&'a Value> {
    let root = config_provider_root(cfg)?;
    root.get(provider_id).or_else(|| {
        provider_id_aliases(provider_id)
            .iter()
            .find_map(|alias| root.get(*alias))
    })
}

fn merge_provider_models_from_config(wire: &mut WireProviderCatalog, cfg: &Value) -> Vec<String> {
    let Some(provider_root) = config_provider_root(cfg) else {
        return Vec::new();
    };

    let mut merged = Vec::new();
    for (provider_id, provider_value) in provider_root {
        let provider_id = canonical_provider_id(provider_id);
        let provider_name = provider_value
            .get("name")
            .and_then(|v| v.as_str())
            .or(known_provider_name(&provider_id))
            .or(Some(provider_id.as_str()));

        let mut model_map: HashMap<String, WireProviderModel> = HashMap::new();
        if let Some(models_obj) = provider_value.get("models").and_then(|v| v.as_object()) {
            for (model_id, model_value) in models_obj {
                let display_name = model_value
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| Some(model_id.to_string()));
                let context = model_value
                    .get("limit")
                    .and_then(|v| v.get("context"))
                    .and_then(|v| v.as_u64())
                    .or_else(|| model_value.get("context_length").and_then(|v| v.as_u64()))
                    .map(|v| v as u32);

                model_map.insert(
                    model_id.to_string(),
                    WireProviderModel {
                        name: display_name,
                        limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
                    },
                );
            }
        }

        if !model_map.is_empty() {
            merge_provider_model_map(wire, &provider_id, provider_name, model_map);
            merged.push(provider_id);
        }
    }

    merged
}

fn provider_default_url(provider_id: &str) -> Option<&'static str> {
    match canonical_provider_id(provider_id).as_str() {
        "openrouter" => Some("https://openrouter.ai/api/v1"),
        "openai" => Some("https://api.openai.com/v1"),
        "openai-codex" => Some(OPENAI_CODEX_API_BASE_URL),
        "llama_cpp" => Some("http://127.0.0.1:8080/v1"),
        "groq" => Some("https://api.groq.com/openai/v1"),
        "mistral" => Some("https://api.mistral.ai/v1"),
        "together" => Some("https://api.together.xyz/v1"),
        _ => None,
    }
}

fn codex_starter_models() -> HashMap<String, WireProviderModel> {
    let rows = [
        ("gpt-5.4", "GPT-5.4"),
        ("gpt-5.2-codex", "GPT-5.2-Codex"),
        ("gpt-5.1-codex-max", "GPT-5.1-Codex-Max"),
        ("gpt-5.4-mini", "GPT-5.4-Mini"),
        ("gpt-5.3-codex", "GPT-5.3-Codex"),
        ("gpt-5.3-codex-spark", "GPT-5.3-Codex-Spark"),
        ("gpt-5.1-codex-mini", "GPT-5.1-Codex-Mini"),
        ("gpt-5.4-pro", "GPT-5.4-Pro"),
    ];
    rows.into_iter()
        .map(|(id, name)| {
            (
                id.to_string(),
                WireProviderModel {
                    name: Some(name.to_string()),
                    limit: Some(WireProviderModelLimit {
                        context: Some(272_000),
                    }),
                },
            )
        })
        .collect()
}

fn remote_catalog_support_message(provider_id: &str) -> String {
    match provider_id {
        "openai-codex" => {
            "Codex account models use a Tandem starter catalog. Live discovery is not enabled here."
                .to_string()
        }
        "azure" | "vertex" | "bedrock" | "copilot" | "ollama" => {
            "Live model discovery is unavailable for this provider. Enter a model ID manually."
                .to_string()
        }
        "anthropic" | "cohere" => {
            "This provider does not currently expose a reliable live model catalog here. Enter a model ID manually."
                .to_string()
        }
        _ => "Live model discovery is unavailable. Enter a model ID manually.".to_string(),
    }
}

fn provider_config_api_key(
    cfg: &Value,
    provider_id: &str,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
) -> Option<String> {
    provider_config_value(cfg, provider_id)
        .and_then(|entry| entry.get("api_key").or_else(|| entry.get("apiKey")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "x")
        .map(str::to_string)
        .or_else(|| {
            runtime_auth
                .get(&provider_id.to_ascii_lowercase())
                .map(String::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            persisted_auth
                .get(&provider_id.to_ascii_lowercase())
                .map(String::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .or_else(|| {
            provider_env_candidates(provider_id)
                .into_iter()
                .find_map(|key| std::env::var(&key).ok())
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
}

fn provider_base_url(cfg: &Value, provider_id: &str) -> Option<String> {
    provider_config_value(cfg, provider_id)
        .and_then(|entry| entry.get("url"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| provider_default_url(provider_id).map(str::to_string))
}

fn normalize_openai_catalog_base(input: &str) -> String {
    let mut base = input.trim().trim_end_matches('/').to_string();
    for suffix in ["/chat/completions", "/completions", "/models"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/').to_string();
            break;
        }
    }
    while let Some(prefix) = base.strip_suffix("/v1") {
        if prefix.ends_with("/v1") {
            base = prefix.to_string();
            continue;
        }
        break;
    }
    if base.ends_with("/v1") {
        base
    } else {
        format!("{}/v1", base.trim_end_matches('/'))
    }
}

fn parse_openrouter_model_payload(body: &Value) -> Option<HashMap<String, WireProviderModel>> {
    let data = body.get("data").and_then(|v| v.as_array())?;
    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some(model_id.to_string()));
        let context = item
            .get("context_length")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                item.get("top_provider")
                    .and_then(|v| v.get("context_length"))
                    .and_then(|v| v.as_u64())
            })
            .map(|v| v as u32);

        out.insert(
            model_id.to_string(),
            WireProviderModel {
                name,
                limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
            },
        );
    }
    (!out.is_empty()).then_some(out)
}

fn parse_openai_compatible_model_payload(
    body: &Value,
) -> Option<HashMap<String, WireProviderModel>> {
    let data = body.get("data").and_then(|v| v.as_array())?;
    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some(model_id.to_string()));
        let context = item
            .get("context_window")
            .and_then(|v| v.as_u64())
            .or_else(|| item.get("context_length").and_then(|v| v.as_u64()))
            .map(|v| v as u32);
        out.insert(
            model_id.to_string(),
            WireProviderModel {
                name,
                limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
            },
        );
    }
    (!out.is_empty()).then_some(out)
}

async fn fetch_openrouter_models(
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let api_key = provider_config_api_key(cfg, "openrouter", runtime_auth, persisted_auth)
        .or_else(|| std::env::var("OPENCODE_OPENROUTER_API_KEY").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let Some(api_key) = api_key else {
        return Err(
            "OpenRouter requires an API key before live model discovery is available.".to_string(),
        );
    };

    let client = reqwest::Client::new();
    let mut req = client
        .get("https://openrouter.ai/api/v1/models")
        .timeout(Duration::from_secs(20));
    req = req.bearer_auth(api_key);
    let resp = req
        .send()
        .await
        .map_err(|err| format!("Failed to fetch OpenRouter models: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "OpenRouter model catalog request failed with status {}",
            resp.status()
        ));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|err| format!("Failed to decode OpenRouter models: {err}"))?;
    parse_openrouter_model_payload(&body)
        .ok_or_else(|| "OpenRouter returned an empty or invalid model catalog.".to_string())
}

async fn fetch_openai_compatible_models(
    provider_id: &str,
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let api_key = provider_config_api_key(cfg, provider_id, runtime_auth, persisted_auth);
    if api_key.is_none() && !provider_supports_optional_auth(provider_id) {
        return Err(format!(
            "{} requires an API key before live model discovery is available.",
            known_provider_name(provider_id).unwrap_or(provider_id)
        ));
    }
    let Some(base_url) = provider_base_url(cfg, provider_id) else {
        return Err("No provider base URL is configured for live model discovery.".to_string());
    };
    let url = format!("{}/models", normalize_openai_catalog_base(&base_url));
    let client = reqwest::Client::new();
    let request = client.get(&url).timeout(Duration::from_secs(20));
    let request = if let Some(api_key) = api_key {
        request.bearer_auth(api_key)
    } else {
        request
    };
    let resp = request
        .send()
        .await
        .map_err(|err| format!("Failed to fetch model catalog from {provider_id}: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "{} model catalog request failed with status {}",
            provider_id,
            resp.status()
        ));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|err| format!("Failed to decode model catalog from {provider_id}: {err}"))?;
    parse_openai_compatible_model_payload(&body).ok_or_else(|| {
        format!("{provider_id} returned an empty or invalid OpenAI-compatible model catalog.")
    })
}

fn normalize_cohere_catalog_base(input: &str) -> String {
    let mut base = input.trim().trim_end_matches('/').to_string();
    for suffix in ["/v1", "/v2", "/models"] {
        if let Some(stripped) = base.strip_suffix(suffix) {
            base = stripped.trim_end_matches('/').to_string();
            break;
        }
    }
    format!("{}/v1", base.trim_end_matches('/'))
}

fn parse_anthropic_model_payload(body: &Value) -> Option<HashMap<String, WireProviderModel>> {
    let data = body.get("data").and_then(|v| v.as_array())?;
    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = item
            .get("display_name")
            .or_else(|| item.get("name"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some(model_id.to_string()));
        out.insert(
            model_id.to_string(),
            WireProviderModel { name, limit: None },
        );
    }
    (!out.is_empty()).then_some(out)
}

async fn fetch_anthropic_models(
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let Some(api_key) = provider_config_api_key(cfg, "anthropic", runtime_auth, persisted_auth)
    else {
        return Err(
            "Anthropic requires an API key before live model discovery is available.".to_string(),
        );
    };
    let resp = reqwest::Client::new()
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|err| format!("Failed to fetch Anthropic models: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Anthropic model catalog request failed with status {}",
            resp.status()
        ));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|err| format!("Failed to decode Anthropic models: {err}"))?;
    parse_anthropic_model_payload(&body)
        .ok_or_else(|| "Anthropic returned an empty or invalid model catalog.".to_string())
}

fn parse_cohere_model_payload(body: &Value) -> Option<HashMap<String, WireProviderModel>> {
    let data = body
        .get("models")
        .and_then(|v| v.as_array())
        .or_else(|| body.get("data").and_then(|v| v.as_array()))?;
    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item
            .get("name")
            .and_then(|v| v.as_str())
            .or_else(|| item.get("id").and_then(|v| v.as_str()))
        else {
            continue;
        };
        let context = item
            .get("context_length")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        out.insert(
            model_id.to_string(),
            WireProviderModel {
                name: Some(model_id.to_string()),
                limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
            },
        );
    }
    (!out.is_empty()).then_some(out)
}

async fn fetch_cohere_models(
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
) -> Result<HashMap<String, WireProviderModel>, String> {
    let Some(api_key) = provider_config_api_key(cfg, "cohere", runtime_auth, persisted_auth) else {
        return Err(
            "Cohere requires an API key before live model discovery is available.".to_string(),
        );
    };
    let base_url = provider_config_value(cfg, "cohere")
        .and_then(|entry| entry.get("url"))
        .and_then(Value::as_str)
        .unwrap_or("https://api.cohere.com/v2");
    let url = format!("{}/models", normalize_cohere_catalog_base(base_url));
    let resp = reqwest::Client::new()
        .get(&url)
        .bearer_auth(api_key)
        .timeout(Duration::from_secs(20))
        .send()
        .await
        .map_err(|err| format!("Failed to fetch Cohere models: {err}"))?;
    if !resp.status().is_success() {
        return Err(format!(
            "Cohere model catalog request failed with status {}",
            resp.status()
        ));
    }
    let body: Value = resp
        .json()
        .await
        .map_err(|err| format!("Failed to decode Cohere models: {err}"))?;
    parse_cohere_model_payload(&body)
        .ok_or_else(|| "Cohere returned an empty or invalid model catalog.".to_string())
}

async fn fetch_remote_provider_models(
    provider_id: &str,
    cfg: &Value,
    runtime_auth: &HashMap<String, String>,
    persisted_auth: &HashMap<String, String>,
) -> ProviderCatalogFetchResult {
    match provider_id {
        "openai-codex" => ProviderCatalogFetchResult::Static {
            models: codex_starter_models(),
        },
        "openrouter" => match fetch_openrouter_models(cfg, runtime_auth, persisted_auth).await {
            Ok(models) => ProviderCatalogFetchResult::Remote { models },
            Err(message) => {
                if message.contains("requires an API key") {
                    ProviderCatalogFetchResult::Unavailable { message }
                } else {
                    tracing::debug!("openrouter catalog discovery failed: {message}");
                    ProviderCatalogFetchResult::Error { message }
                }
            }
        },
        "openai" | "llama_cpp" | "groq" | "mistral" | "together" => {
            match fetch_openai_compatible_models(provider_id, cfg, runtime_auth, persisted_auth)
                .await
            {
                Ok(models) => ProviderCatalogFetchResult::Remote { models },
                Err(message) => {
                    if message.contains("requires an API key") {
                        ProviderCatalogFetchResult::Unavailable { message }
                    } else {
                        tracing::warn!("{provider_id} catalog discovery failed: {message}");
                        ProviderCatalogFetchResult::Error { message }
                    }
                }
            }
        }
        "anthropic" => match fetch_anthropic_models(cfg, runtime_auth, persisted_auth).await {
            Ok(models) => ProviderCatalogFetchResult::Remote { models },
            Err(message) => {
                if message.contains("requires an API key") {
                    ProviderCatalogFetchResult::Unavailable { message }
                } else {
                    tracing::warn!("anthropic catalog discovery failed: {message}");
                    ProviderCatalogFetchResult::Error { message }
                }
            }
        },
        "cohere" => match fetch_cohere_models(cfg, runtime_auth, persisted_auth).await {
            Ok(models) => ProviderCatalogFetchResult::Remote { models },
            Err(message) => {
                if message.contains("requires an API key") {
                    ProviderCatalogFetchResult::Unavailable { message }
                } else {
                    tracing::warn!("cohere catalog discovery failed: {message}");
                    ProviderCatalogFetchResult::Error { message }
                }
            }
        },
        "azure" | "vertex" | "bedrock" | "copilot" | "ollama" => {
            ProviderCatalogFetchResult::Unavailable {
                message: remote_catalog_support_message(provider_id),
            }
        }
        _ => ProviderCatalogFetchResult::Unavailable {
            message: "Live model discovery is unavailable. Enter a model ID manually.".to_string(),
        },
    }
}

fn provider_supports_optional_auth(provider_id: &str) -> bool {
    matches!(canonical_provider_id(provider_id).as_str(), "llama_cpp")
}

fn provider_env_candidates(provider_id: &str) -> Vec<String> {
    let normalized = provider_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .to_ascii_uppercase();

    let mut out = vec![format!("{}_API_KEY", normalized)];
    match provider_id.to_ascii_lowercase().as_str() {
        "openai" => out.push("OPENAI_API_KEY".to_string()),
        "openrouter" => out.push("OPENROUTER_API_KEY".to_string()),
        "anthropic" => out.push("ANTHROPIC_API_KEY".to_string()),
        "groq" => out.push("GROQ_API_KEY".to_string()),
        "mistral" => out.push("MISTRAL_API_KEY".to_string()),
        "together" => out.push("TOGETHER_API_KEY".to_string()),
        "azure" => out.push("AZURE_OPENAI_API_KEY".to_string()),
        "vertex" => out.push("VERTEX_API_KEY".to_string()),
        "bedrock" => out.push("BEDROCK_API_KEY".to_string()),
        "copilot" => out.push("GITHUB_TOKEN".to_string()),
        "cohere" => out.push("COHERE_API_KEY".to_string()),
        _ => {}
    }
    out.sort();
    out.dedup();
    out
}

fn provider_has_env_secret(provider_id: &str) -> bool {
    provider_env_candidates(provider_id).into_iter().any(|key| {
        std::env::var(&key)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    })
}

fn provider_requires_api_key(provider_id: &str) -> bool {
    !matches!(
        canonical_provider_id(provider_id).as_str(),
        "local" | "ollama" | "llama_cpp"
    )
}

fn provider_oauth_redirect_uri(state: &AppState, provider_id: &str) -> String {
    if canonical_provider_id(provider_id) == OPENAI_CODEX_PROVIDER_ID {
        return OPENAI_CODEX_LOCAL_CALLBACK_URI.to_string();
    }
    let base = state
        .server_base_url
        .read()
        .ok()
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "http://127.0.0.1:39731".to_string());
    format!("{base}/provider/{provider_id}/oauth/callback")
}

fn generate_oauth_state() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(
        "{}:{}",
        Uuid::new_v4(),
        Uuid::new_v4()
    ))
}

fn generate_pkce_pair() -> (String, String) {
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(
        "{}:{}",
        Uuid::new_v4(),
        Uuid::new_v4()
    ));
    let digest = sha2::Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

fn build_openai_codex_authorization_url(
    redirect_uri: &str,
    code_challenge: &str,
    state: &str,
) -> String {
    let query = vec![
        ("response_type", "code".to_string()),
        ("client_id", OPENAI_CODEX_OAUTH_CLIENT_ID.to_string()),
        ("redirect_uri", redirect_uri.to_string()),
        ("scope", "openid profile email offline_access".to_string()),
        ("code_challenge", code_challenge.to_string()),
        ("code_challenge_method", "S256".to_string()),
        ("id_token_add_organizations", "true".to_string()),
        ("codex_cli_simplified_flow", "true".to_string()),
        ("state", state.to_string()),
    ]
    .into_iter()
    .map(|(key, value)| format!("{key}={}", urlencoding::encode(&value)))
    .collect::<Vec<_>>()
    .join("&");
    format!("{OPENAI_CODEX_OAUTH_ISSUER}/oauth/authorize?{query}")
}

fn openai_codex_local_callback_shutdown_slot() -> &'static Mutex<Option<oneshot::Sender<()>>> {
    static SLOT: OnceLock<Mutex<Option<oneshot::Sender<()>>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}
