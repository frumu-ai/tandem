use super::*;

use std::collections::{HashMap, HashSet};

use tandem_channels::channel_registry::{find_channel, registered_channels, ChannelSpec};

fn parse_allowed_users(value: Option<&Value>) -> Vec<String> {
    let mut users = value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if users.is_empty() {
        users.push("*".to_string());
    }
    users
}

fn mask_saved_token(has_token: bool) -> Option<&'static str> {
    if has_token {
        Some("****")
    } else {
        None
    }
}

fn trim_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn find_channel_spec(name: &str) -> Option<&'static ChannelSpec> {
    let normalized = name.trim().to_ascii_lowercase();
    find_channel(&normalized)
}

fn state_data_dir() -> PathBuf {
    std::env::var("TANDEM_STATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            if let Some(data_dir) = dirs::data_dir() {
                return data_dir.join("tandem").join("data");
            }
            dirs::home_dir()
                .map(|home| home.join(".tandem").join("data"))
                .unwrap_or_else(|| PathBuf::from(".tandem"))
        })
}

fn normalize_channel_config_obj<'a>(
    channels: Option<&'a serde_json::Map<String, Value>>,
    spec: &'static ChannelSpec,
) -> serde_json::Map<String, Value> {
    let mut entry = serde_json::Map::new();
    let channel = channels
        .and_then(|channels| channels.get(spec.config_key))
        .and_then(Value::as_object);

    let has_token = channel
        .and_then(|cfg| cfg.get("bot_token"))
        .and_then(Value::as_str)
        .map(|value| !value.trim().is_empty())
        .unwrap_or(false);
    entry.insert("has_token".to_string(), serde_json::Value::Bool(has_token));
    entry.insert(
        "token_masked".to_string(),
        mask_saved_token(has_token).map_or(Value::Null, |value| Value::String(value.to_string())),
    );
    entry.insert(
        "allowed_users".to_string(),
        Value::Array(
            parse_allowed_users(channel.and_then(|cfg| cfg.get("allowed_users")))
                .into_iter()
                .map(Value::String)
                .collect(),
        ),
    );

    let mention_only = channel
        .and_then(|cfg| cfg.get("mention_only"))
        .and_then(Value::as_bool)
        .unwrap_or(match spec.name {
            "discord" => true,
            _ => false,
        });
    entry.insert("mention_only".to_string(), Value::Bool(mention_only));
    entry.insert(
        "strict_kb_grounding".to_string(),
        Value::Bool(
            channel
                .and_then(|cfg| cfg.get("strict_kb_grounding"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        ),
    );

    entry.insert(
        "model_provider_id".to_string(),
        channel
            .and_then(|cfg| cfg.get("model_provider_id"))
            .and_then(Value::as_str)
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
    );
    entry.insert(
        "model_id".to_string(),
        channel
            .and_then(|cfg| cfg.get("model_id"))
            .and_then(Value::as_str)
            .map(|value| Value::String(value.to_string()))
            .unwrap_or(Value::Null),
    );
    entry.insert(
        "security_profile".to_string(),
        Value::String(
            channel
                .and_then(|cfg| cfg.get("security_profile"))
                .and_then(Value::as_str)
                .unwrap_or("operator")
                .to_string(),
        ),
    );

    match spec.name {
        "telegram" => {
            entry.insert(
                "style_profile".to_string(),
                Value::String(
                    channel
                        .and_then(|cfg| cfg.get("style_profile"))
                        .and_then(Value::as_str)
                        .unwrap_or("default")
                        .to_string(),
                ),
            );
        }
        "discord" => {
            entry.insert(
                "guild_id".to_string(),
                channel
                    .and_then(|cfg| cfg.get("guild_id"))
                    .and_then(Value::as_str)
                    .map(|value| Value::String(value.to_string()))
                    .unwrap_or(Value::Null),
            );
        }
        "slack" => {
            entry.insert(
                "channel_id".to_string(),
                channel
                    .and_then(|cfg| cfg.get("channel_id"))
                    .and_then(Value::as_str)
                    .map(|value| Value::String(value.to_string()))
                    .unwrap_or(Value::Null),
            );
        }
        _ => {}
    }
    entry
}

#[derive(Debug, Clone, serde::Deserialize)]
struct ChannelSessionRecord {
    session_id: String,
    created_at_ms: u64,
    last_seen_at_ms: u64,
    channel: String,
    sender: String,
    #[serde(default)]
    scope_id: Option<String>,
    #[serde(default)]
    scope_kind: Option<String>,
    #[serde(default)]
    tool_preferences: Option<ChannelToolPreferences>,
    #[serde(default)]
    workflow_planner_session_id: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChannelScopeSummary {
    pub scope_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope_kind: Option<String>,
    pub session_count: usize,
    pub sender_count: usize,
    pub last_seen_at_ms: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ChannelScopesResponse {
    pub channel: String,
    pub scopes: Vec<ChannelScopeSummary>,
}

#[derive(Debug, Clone, serde::Deserialize)]
pub struct ChannelToolPreferencesQuery {
    #[serde(default)]
    pub scope_id: Option<String>,
}

fn existing_channel_value(
    channels: Option<&serde_json::Map<String, Value>>,
    spec: &ChannelSpec,
    key: &str,
) -> Option<String> {
    channels
        .and_then(|obj| obj.get(spec.config_key))
        .and_then(Value::as_object)
        .and_then(|cfg| cfg.get(key))
        .and_then(Value::as_str)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn existing_channel_token(
    channels: Option<&serde_json::Map<String, Value>>,
    spec: &ChannelSpec,
) -> Option<String> {
    existing_channel_value(channels, spec, "bot_token").or_else(|| {
        std::env::var(spec.token_env_key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn existing_channel_id(
    channels: Option<&serde_json::Map<String, Value>>,
    spec: &ChannelSpec,
) -> Option<String> {
    let env_key = match spec.channel_id_env_key {
        Some(env_key) => env_key,
        None => return None,
    };
    existing_channel_value(channels, spec, "channel_id").or_else(|| {
        std::env::var(env_key)
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    })
}

fn channel_sessions_path() -> PathBuf {
    state_data_dir().join("channel_sessions.json")
}

async fn load_channel_session_map() -> HashMap<String, ChannelSessionRecord> {
    let path = channel_sessions_path();
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return HashMap::new();
    };

    if let Ok(map) = serde_json::from_slice::<HashMap<String, ChannelSessionRecord>>(&bytes) {
        return map;
    }

    if let Ok(old_map) = serde_json::from_slice::<HashMap<String, String>>(&bytes) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_millis() as u64)
            .unwrap_or_default();
        return old_map
            .into_iter()
            .map(|(key, session_id)| {
                let mut parts = key.splitn(2, ':');
                let channel = parts.next().unwrap_or("unknown").to_string();
                let sender = parts.next().unwrap_or("unknown").to_string();
                (
                    key,
                    ChannelSessionRecord {
                        session_id,
                        created_at_ms: now,
                        last_seen_at_ms: now,
                        channel,
                        sender,
                        scope_id: None,
                        scope_kind: None,
                        tool_preferences: None,
                        workflow_planner_session_id: None,
                    },
                )
            })
            .collect();
    }

    HashMap::new()
}

fn group_channel_scope_summaries(
    channel: &str,
    session_map: &HashMap<String, ChannelSessionRecord>,
) -> Vec<ChannelScopeSummary> {
    let mut grouped: HashMap<String, ChannelScopeSummary> = HashMap::new();
    let mut senders: HashMap<String, HashSet<String>> = HashMap::new();

    for record in session_map.values() {
        if record.channel != channel {
            continue;
        }
        let Some(scope_id) = record
            .scope_id
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
        else {
            continue;
        };

        let entry = grouped
            .entry(scope_id.clone())
            .or_insert_with(|| ChannelScopeSummary {
                scope_id: scope_id.clone(),
                scope_kind: record
                    .scope_kind
                    .clone()
                    .filter(|value| !value.trim().is_empty()),
                session_count: 0,
                sender_count: 0,
                last_seen_at_ms: record.last_seen_at_ms,
            });
        entry.session_count += 1;
        entry.last_seen_at_ms = entry.last_seen_at_ms.max(record.last_seen_at_ms);
        if entry.scope_kind.is_none() {
            entry.scope_kind = record
                .scope_kind
                .clone()
                .filter(|value| !value.trim().is_empty());
        }

        senders
            .entry(scope_id)
            .or_default()
            .insert(record.sender.clone());
    }

    for (scope_id, entry) in grouped.iter_mut() {
        entry.sender_count = senders.get(scope_id).map(|set| set.len()).unwrap_or(0);
    }

    let mut scopes = grouped.into_values().collect::<Vec<_>>();
    scopes.sort_by(|left, right| {
        right
            .last_seen_at_ms
            .cmp(&left.last_seen_at_ms)
            .then_with(|| left.scope_id.cmp(&right.scope_id))
    });
    scopes
}

async fn load_channel_scope_summaries(channel: &str) -> Vec<ChannelScopeSummary> {
    let session_map = load_channel_session_map().await;
    group_channel_scope_summaries(channel, &session_map)
}

pub(super) async fn channels_config(State(state): State<AppState>) -> Json<Value> {
    let effective = state.config.get_effective_value().await;
    let channels = effective.get("channels").and_then(Value::as_object);
    let mut entries = serde_json::Map::new();
    for spec in registered_channels() {
        entries.insert(
            spec.config_key.to_string(),
            Value::Object(normalize_channel_config_obj(channels, spec)),
        );
    }
    Json(Value::Object(entries))
}

pub(super) async fn channels_status(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.channel_statuses().await))
}

pub(super) async fn channel_scopes_get(
    Path(name): Path<String>,
) -> Result<Json<ChannelScopesResponse>, StatusCode> {
    let channel = name.trim().to_ascii_lowercase();
    if find_channel_spec(&channel).is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    let scopes = load_channel_scope_summaries(&channel).await;
    Ok(Json(ChannelScopesResponse { channel, scopes }))
}

pub(super) async fn channels_verify(
    State(state): State<AppState>,
    Path(name): Path<String>,
    input: Option<Json<Value>>,
) -> Result<Json<Value>, StatusCode> {
    let normalized = name.to_ascii_lowercase();
    let Some(spec) = find_channel_spec(&normalized) else {
        return Err(StatusCode::NOT_FOUND);
    };
    let payload = input.map(|Json(v)| v).unwrap_or_else(|| json!({}));

    match spec.name {
        "discord" => Ok(Json(discord_channel_verify(&state, &payload).await)),
        _ => Err(StatusCode::NOT_FOUND),
    }
}

const DISCORD_FLAG_GATEWAY_PRESENCE: u64 = 1 << 12;
const DISCORD_FLAG_GATEWAY_PRESENCE_LIMITED: u64 = 1 << 13;
const DISCORD_FLAG_GATEWAY_MEMBERS: u64 = 1 << 14;
const DISCORD_FLAG_GATEWAY_MEMBERS_LIMITED: u64 = 1 << 15;
const DISCORD_FLAG_GATEWAY_MESSAGE_CONTENT: u64 = 1 << 18;
const DISCORD_FLAG_GATEWAY_MESSAGE_CONTENT_LIMITED: u64 = 1 << 19;

async fn discord_channel_verify(state: &AppState, payload: &Value) -> Value {
    let provided_token = payload
        .get("bot_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    let effective = state.config.get_effective_value().await;
    let saved_token = effective
        .get("channels")
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("discord"))
        .and_then(Value::as_object)
        .and_then(|cfg| cfg.get("bot_token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    let token = provided_token.or(saved_token).unwrap_or_default();
    let has_token = !token.is_empty();
    let mut hints: Vec<String> = Vec::new();
    if !has_token {
        hints.push("Add your Discord bot token, then click Save or Verify again.".to_string());
        return json!({
            "ok": false,
            "channel": "discord",
            "checks": {
                "has_token": false,
                "token_auth_ok": false,
                "gateway_ok": false,
                "message_content_intent_ok": false
            },
            "status_codes": {
                "users_me": null,
                "gateway_bot": null,
                "application_me": null
            },
            "hints": hints,
            "details": {}
        });
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "ok": false,
                "channel": "discord",
                "checks": {
                    "has_token": true,
                    "token_auth_ok": false,
                    "gateway_ok": false,
                    "message_content_intent_ok": false
                },
                "status_codes": {
                    "users_me": null,
                    "gateway_bot": null,
                    "application_me": null
                },
                "hints": ["Local HTTP client setup failed. Restart Tandem and retry verification."],
                "details": {
                    "error": e.to_string()
                }
            });
        }
    };
    let auth_header = format!("Bot {token}");

    let users_resp = client
        .get("https://discord.com/api/v10/users/@me")
        .header("Authorization", auth_header.clone())
        .send()
        .await;
    let gateway_resp = client
        .get("https://discord.com/api/v10/gateway/bot")
        .header("Authorization", auth_header.clone())
        .send()
        .await;
    let app_resp = client
        .get("https://discord.com/api/v10/applications/@me")
        .header("Authorization", auth_header)
        .send()
        .await;

    let users_status = users_resp.as_ref().ok().map(|r| r.status().as_u16());
    let gateway_status = gateway_resp.as_ref().ok().map(|r| r.status().as_u16());
    let app_status = app_resp.as_ref().ok().map(|r| r.status().as_u16());

    let token_auth_ok = users_status == Some(200);
    let gateway_ok = gateway_status == Some(200);

    let mut bot_username: Option<String> = None;
    let mut bot_id: Option<String> = None;
    if let Ok(resp) = users_resp {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<Value>().await {
                bot_username = v
                    .get("username")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                bot_id = v.get("id").and_then(Value::as_str).map(ToString::to_string);
            }
        }
    }

    let mut app_flags: Option<u64> = None;
    if let Ok(resp) = app_resp {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<Value>().await {
                app_flags = v.get("flags").and_then(Value::as_u64);
            }
        }
    }

    let message_content_intent_ok = app_flags.is_some_and(|flags| {
        flags
            & (DISCORD_FLAG_GATEWAY_MESSAGE_CONTENT | DISCORD_FLAG_GATEWAY_MESSAGE_CONTENT_LIMITED)
            != 0
    });
    let presence_intent_enabled = app_flags.is_some_and(|flags| {
        flags & (DISCORD_FLAG_GATEWAY_PRESENCE | DISCORD_FLAG_GATEWAY_PRESENCE_LIMITED) != 0
    });
    let server_members_intent_enabled = app_flags.is_some_and(|flags| {
        flags & (DISCORD_FLAG_GATEWAY_MEMBERS | DISCORD_FLAG_GATEWAY_MEMBERS_LIMITED) != 0
    });

    if !token_auth_ok {
        if users_status == Some(401) {
            hints.push("Discord rejected this token (401). Regenerate bot token in Developer Portal -> Bot and update Tandem.".to_string());
        } else {
            hints.push("Could not authenticate bot token with Discord `/users/@me`.".to_string());
        }
    }
    if !gateway_ok {
        if gateway_status == Some(429) {
            hints.push("Discord gateway verification is rate-limited right now. Wait a few seconds and verify again.".to_string());
        } else {
            hints.push("Discord `/gateway/bot` check failed. Verify outbound network access to discord.com.".to_string());
        }
    }
    if token_auth_ok && gateway_ok && !message_content_intent_ok {
        hints.push("Enable `Message Content Intent` in Discord Developer Portal -> Bot -> Privileged Gateway Intents.".to_string());
    }
    if hints.is_empty() {
        hints.push("Discord checks passed. If replies are still missing, verify channel/thread permissions: View Channel, Send Messages, Read Message History, Send Messages in Threads.".to_string());
    }

    let ok = token_auth_ok && gateway_ok && message_content_intent_ok;
    json!({
        "ok": ok,
        "channel": "discord",
        "checks": {
            "has_token": has_token,
            "token_auth_ok": token_auth_ok,
            "gateway_ok": gateway_ok,
            "message_content_intent_ok": message_content_intent_ok,
            "presence_intent_enabled": presence_intent_enabled,
            "server_members_intent_enabled": server_members_intent_enabled
        },
        "status_codes": {
            "users_me": users_status,
            "gateway_bot": gateway_status,
            "application_me": app_status
        },
        "hints": hints,
        "details": {
            "bot_username": bot_username,
            "bot_id": bot_id,
            "application_flags": app_flags
        }
    })
}

pub(super) async fn channels_put(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(mut input): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let normalized = name.to_ascii_lowercase();
    let Some(spec) = find_channel_spec(&normalized) else {
        return Err(StatusCode::NOT_FOUND);
    };
    let effective = state.config.get_effective_value().await;
    let statuses = state.channel_statuses().await;
    let existing_channel_cfg = |spec: &ChannelSpec| -> Option<&serde_json::Map<String, Value>> {
        effective
            .get("channels")
            .and_then(Value::as_object)
            .and_then(|obj| obj.get(spec.config_key))
            .and_then(Value::as_object)
    };
    let channel_is_connected = |spec: &ChannelSpec| -> bool {
        statuses
            .get(spec.name)
            .map(|status| status.connected)
            .unwrap_or(false)
    };
    let existing_bot_token = |spec: &ChannelSpec| -> Option<String> {
        existing_channel_cfg(spec)
            .and_then(|cfg| cfg.get("bot_token"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::env::var(spec.token_env_key)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
    };
    let existing_channel_id = |spec: &ChannelSpec| -> Option<String> {
        let Some(env_key) = spec.channel_id_env_key else {
            return None;
        };
        existing_channel_cfg(spec)
            .and_then(|cfg| cfg.get("channel_id"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                std::env::var(env_key)
                    .ok()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            })
    };

    let mut project = state.config.get_project_value().await;
    let Some(root) = project.as_object_mut() else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let channels = root
        .entry("channels".to_string())
        .or_insert_with(|| json!({}));
    let Some(channels_obj) = channels.as_object_mut() else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    match spec.name {
        "telegram" => {
            if let Some(cfg) = input.as_object_mut() {
                if cfg
                    .get("bot_token")
                    .and_then(Value::as_str)
                    .is_none_or(|v| v.trim().is_empty())
                {
                    if let Some(existing) = existing_bot_token(spec) {
                        cfg.insert("bot_token".to_string(), Value::String(existing));
                    }
                }
            }
            let mut cfg: TelegramConfigFile =
                serde_json::from_value(input).map_err(|_| StatusCode::BAD_REQUEST)?;
            cfg.allowed_users = crate::normalize_allowed_users_or_wildcard(cfg.allowed_users);
            cfg.model_provider_id = trim_optional_string(cfg.model_provider_id);
            cfg.model_id = trim_optional_string(cfg.model_id);
            if cfg.bot_token.trim().is_empty() {
                cfg.bot_token = existing_bot_token(spec).unwrap_or_default();
            }
            if cfg.bot_token.trim().is_empty() && !channel_is_connected(spec) {
                return Err(StatusCode::BAD_REQUEST);
            }
            channels_obj.insert(spec.config_key.to_string(), json!(cfg));
        }
        "discord" => {
            if let Some(cfg) = input.as_object_mut() {
                if cfg
                    .get("bot_token")
                    .and_then(Value::as_str)
                    .is_none_or(|v| v.trim().is_empty())
                {
                    if let Some(existing) = existing_bot_token(spec) {
                        cfg.insert("bot_token".to_string(), Value::String(existing));
                    }
                }
            }
            let mut cfg: DiscordConfigFile =
                serde_json::from_value(input).map_err(|_| StatusCode::BAD_REQUEST)?;
            cfg.allowed_users = crate::normalize_allowed_users_or_wildcard(cfg.allowed_users);
            cfg.guild_id = trim_optional_string(cfg.guild_id);
            cfg.model_provider_id = trim_optional_string(cfg.model_provider_id);
            cfg.model_id = trim_optional_string(cfg.model_id);
            if cfg.bot_token.trim().is_empty() {
                cfg.bot_token = existing_bot_token(spec).unwrap_or_default();
            }
            if cfg.bot_token.trim().is_empty() && !channel_is_connected(spec) {
                return Err(StatusCode::BAD_REQUEST);
            }
            channels_obj.insert(spec.config_key.to_string(), json!(cfg));
        }
        "slack" => {
            if let Some(cfg) = input.as_object_mut() {
                if cfg
                    .get("bot_token")
                    .and_then(Value::as_str)
                    .is_none_or(|v| v.trim().is_empty())
                {
                    if let Some(existing) = existing_bot_token(spec) {
                        cfg.insert("bot_token".to_string(), Value::String(existing));
                    }
                }
                if cfg
                    .get("channel_id")
                    .and_then(Value::as_str)
                    .is_none_or(|v| v.trim().is_empty())
                {
                    if let Some(existing) = existing_channel_id(spec) {
                        cfg.insert("channel_id".to_string(), Value::String(existing));
                    }
                }
            }
            let mut cfg: SlackConfigFile =
                serde_json::from_value(input).map_err(|_| StatusCode::BAD_REQUEST)?;
            cfg.allowed_users = crate::normalize_allowed_users_or_wildcard(cfg.allowed_users);
            cfg.model_provider_id = trim_optional_string(cfg.model_provider_id);
            cfg.model_id = trim_optional_string(cfg.model_id);
            if cfg.bot_token.trim().is_empty() {
                cfg.bot_token = existing_bot_token(spec).unwrap_or_default();
            }
            if cfg.channel_id.trim().is_empty() {
                cfg.channel_id = existing_channel_id(spec).unwrap_or_default();
            }
            if (cfg.bot_token.trim().is_empty() || cfg.channel_id.trim().is_empty())
                && !channel_is_connected(spec)
            {
                return Err(StatusCode::BAD_REQUEST);
            }
            channels_obj.insert(spec.config_key.to_string(), json!(cfg));
        }
        _ => return Err(StatusCode::NOT_FOUND),
    }
    state
        .config
        .replace_project_value(project)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    state
        .restart_channel_listeners()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": true})))
}

pub(super) async fn channels_delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let Some(spec) = find_channel_spec(&name.to_ascii_lowercase()) else {
        return Err(StatusCode::NOT_FOUND);
    };
    if let Some(secret_id) = tandem_core::channel_secret_store_id(spec.name) {
        let _ = tandem_core::delete_provider_auth(&secret_id);
    }
    let mut project = state.config.get_project_value().await;
    let Some(root) = project.as_object_mut() else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let channels = root
        .entry("channels".to_string())
        .or_insert_with(|| json!({}));
    let Some(channels_obj) = channels.as_object_mut() else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    channels_obj.remove(spec.config_key);
    state
        .config
        .replace_project_value(project)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    state
        .restart_channel_listeners()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": true})))
}

pub(super) async fn admin_reload_config(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    state
        .restart_channel_listeners()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": true})))
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct ChannelToolPreferences {
    #[serde(default)]
    pub enabled_tools: Vec<String>,
    #[serde(default)]
    pub disabled_tools: Vec<String>,
    #[serde(default)]
    pub enabled_mcp_servers: Vec<String>,
    #[serde(default)]
    pub enabled_mcp_tools: Vec<String>,
}

const WORKFLOW_PLANNER_PSEUDO_TOOL: &str = "tandem.workflow_planner";
const PUBLIC_DEMO_ALLOWED_TOOLS: &[&str] = &[
    "websearch",
    "webfetch",
    "webfetch_html",
    "memory_search",
    "memory_store",
    "memory_list",
];

fn unique_strings(values: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    values
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .filter(|value| seen.insert(value.clone()))
        .collect()
}

fn parse_channel_security_profile(
    raw: Option<&str>,
) -> tandem_channels::config::ChannelSecurityProfile {
    match raw.map(|value| value.trim().to_ascii_lowercase()) {
        Some(value) if value == "trusted_team" || value == "trusted-team" => {
            tandem_channels::config::ChannelSecurityProfile::TrustedTeam
        }
        Some(value) if value == "public_demo" || value == "public-demo" => {
            tandem_channels::config::ChannelSecurityProfile::PublicDemo
        }
        _ => tandem_channels::config::ChannelSecurityProfile::Operator,
    }
}

fn channel_security_profile_from_config(
    effective: &Value,
    channel: &str,
) -> tandem_channels::config::ChannelSecurityProfile {
    let raw = effective
        .get("channels")
        .and_then(Value::as_object)
        .and_then(|channels| channels.get(channel))
        .and_then(Value::as_object)
        .and_then(|cfg| cfg.get("security_profile"))
        .and_then(Value::as_str);
    parse_channel_security_profile(raw)
}

fn sanitize_tool_preferences_for_security_profile(
    prefs: ChannelToolPreferences,
    security_profile: tandem_channels::config::ChannelSecurityProfile,
) -> ChannelToolPreferences {
    let enabled_tools = unique_strings(prefs.enabled_tools);
    let disabled_tools = unique_strings(prefs.disabled_tools);
    let enabled_mcp_servers = unique_strings(prefs.enabled_mcp_servers);
    let enabled_mcp_tools = unique_strings(prefs.enabled_mcp_tools);

    if security_profile != tandem_channels::config::ChannelSecurityProfile::PublicDemo {
        return ChannelToolPreferences {
            enabled_tools,
            disabled_tools,
            enabled_mcp_servers,
            enabled_mcp_tools,
        };
    }

    ChannelToolPreferences {
        enabled_tools: enabled_tools
            .into_iter()
            .filter(|tool| {
                PUBLIC_DEMO_ALLOWED_TOOLS
                    .iter()
                    .any(|allowed| allowed == tool)
            })
            .collect(),
        disabled_tools: disabled_tools
            .into_iter()
            .filter(|tool| tool != WORKFLOW_PLANNER_PSEUDO_TOOL)
            .collect(),
        enabled_mcp_servers: Vec::new(),
        enabled_mcp_tools: Vec::new(),
    }
}

fn merge_channel_tool_preferences(
    base: ChannelToolPreferences,
    scoped: ChannelToolPreferences,
) -> ChannelToolPreferences {
    ChannelToolPreferences {
        enabled_tools: merge_unique_strings(base.enabled_tools, scoped.enabled_tools),
        disabled_tools: merge_unique_strings(base.disabled_tools, scoped.disabled_tools),
        enabled_mcp_servers: merge_unique_strings(
            base.enabled_mcp_servers,
            scoped.enabled_mcp_servers,
        ),
        enabled_mcp_tools: merge_unique_strings(base.enabled_mcp_tools, scoped.enabled_mcp_tools),
    }
}

fn merge_unique_strings(mut base: Vec<String>, overlay: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut merged = Vec::new();

    for value in base.drain(..).chain(overlay.into_iter()) {
        let value = value.trim().to_string();
        if value.is_empty() || !seen.insert(value.clone()) {
            continue;
        }
        merged.push(value);
    }

    merged
}

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

async fn load_tool_preferences_map() -> std::collections::HashMap<String, ChannelToolPreferences> {
    let path = tool_preferences_path();
    let Ok(bytes) = tokio::fs::read(&path).await else {
        return std::collections::HashMap::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

async fn save_tool_preferences_map(map: &ToolPreferencesMap) {
    let path = tool_preferences_path();
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    if let Ok(json) = serde_json::to_vec_pretty(map) {
        let _ = tokio::fs::write(&path, json).await;
    }
}

pub(super) async fn channel_tool_preferences_get(
    State(state): State<AppState>,
    Path(channel): Path<String>,
    Query(query): Query<ChannelToolPreferencesQuery>,
) -> Result<Json<ChannelToolPreferences>, StatusCode> {
    let key = channel.to_string();
    let mut map = load_tool_preferences_map().await;
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
    let sanitized = sanitize_tool_preferences_for_security_profile(prefs.clone(), security_profile);
    if sanitized != prefs {
        if let Some(scoped_key) = scoped_key {
            if map.contains_key(&scoped_key) {
                map.insert(scoped_key, sanitized.clone());
                save_tool_preferences_map(&map).await;
            }
        } else {
            map.insert(key, sanitized.clone());
            save_tool_preferences_map(&map).await;
        }
    }
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
    Path(channel): Path<String>,
    Query(query): Query<ChannelToolPreferencesQuery>,
    Json(input): Json<ChannelToolPreferencesInput>,
) -> Result<Json<ChannelToolPreferences>, StatusCode> {
    let mut map = load_tool_preferences_map().await;
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
    save_tool_preferences_map(&map).await;
    Ok(Json(new_prefs))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_demo_sanitizes_enabled_tools_and_mcp_servers() {
        let prefs = ChannelToolPreferences {
            enabled_tools: vec![
                "websearch".to_string(),
                WORKFLOW_PLANNER_PSEUDO_TOOL.to_string(),
                "bash".to_string(),
                "webfetch_html".to_string(),
                "bash".to_string(),
            ],
            disabled_tools: vec![
                "read".to_string(),
                WORKFLOW_PLANNER_PSEUDO_TOOL.to_string(),
                "read".to_string(),
            ],
            enabled_mcp_servers: vec!["github".to_string(), "slack".to_string()],
            enabled_mcp_tools: vec![
                "mcp.github.create_issue".to_string(),
                "mcp.slack.post_message".to_string(),
            ],
        };

        let sanitized = sanitize_tool_preferences_for_security_profile(
            prefs,
            tandem_channels::config::ChannelSecurityProfile::PublicDemo,
        );

        assert_eq!(
            sanitized.enabled_tools,
            vec!["websearch".to_string(), "webfetch_html".to_string()]
        );
        assert_eq!(sanitized.disabled_tools, vec!["read".to_string()]);
        assert!(sanitized.enabled_mcp_servers.is_empty());
        assert!(sanitized.enabled_mcp_tools.is_empty());
    }

    #[test]
    fn operator_keeps_existing_tool_preferences() {
        let prefs = ChannelToolPreferences {
            enabled_tools: vec!["bash".to_string(), "bash".to_string()],
            disabled_tools: vec!["read".to_string(), "".to_string()],
            enabled_mcp_servers: vec!["github".to_string(), "github".to_string()],
            enabled_mcp_tools: vec![
                "mcp.github.create_issue".to_string(),
                "mcp.github.create_issue".to_string(),
            ],
        };

        let sanitized = sanitize_tool_preferences_for_security_profile(
            prefs,
            tandem_channels::config::ChannelSecurityProfile::Operator,
        );

        assert_eq!(sanitized.enabled_tools, vec!["bash".to_string()]);
        assert_eq!(sanitized.disabled_tools, vec!["read".to_string()]);
        assert_eq!(sanitized.enabled_mcp_servers, vec!["github".to_string()]);
        assert_eq!(
            sanitized.enabled_mcp_tools,
            vec!["mcp.github.create_issue".to_string()]
        );
    }

    #[test]
    fn group_channel_scope_summaries_groups_by_scope_and_orders_by_recency() {
        let mut map = HashMap::new();
        map.insert(
            "telegram:chat:123:alice".to_string(),
            ChannelSessionRecord {
                session_id: "s1".to_string(),
                created_at_ms: 1,
                last_seen_at_ms: 10,
                channel: "telegram".to_string(),
                sender: "alice".to_string(),
                scope_id: Some("chat:123".to_string()),
                scope_kind: Some("room".to_string()),
                tool_preferences: None,
                workflow_planner_session_id: None,
            },
        );
        map.insert(
            "telegram:chat:123:bob".to_string(),
            ChannelSessionRecord {
                session_id: "s2".to_string(),
                created_at_ms: 2,
                last_seen_at_ms: 30,
                channel: "telegram".to_string(),
                sender: "bob".to_string(),
                scope_id: Some("chat:123".to_string()),
                scope_kind: Some("room".to_string()),
                tool_preferences: None,
                workflow_planner_session_id: None,
            },
        );
        map.insert(
            "telegram:topic:1:2:carol".to_string(),
            ChannelSessionRecord {
                session_id: "s3".to_string(),
                created_at_ms: 3,
                last_seen_at_ms: 20,
                channel: "telegram".to_string(),
                sender: "carol".to_string(),
                scope_id: Some("topic:1:2".to_string()),
                scope_kind: Some("topic".to_string()),
                tool_preferences: None,
                workflow_planner_session_id: None,
            },
        );
        map.insert(
            "discord:channel:9:dave".to_string(),
            ChannelSessionRecord {
                session_id: "s4".to_string(),
                created_at_ms: 4,
                last_seen_at_ms: 40,
                channel: "discord".to_string(),
                sender: "dave".to_string(),
                scope_id: Some("channel:9".to_string()),
                scope_kind: Some("room".to_string()),
                tool_preferences: None,
                workflow_planner_session_id: None,
            },
        );

        let scopes = group_channel_scope_summaries("telegram", &map);
        assert_eq!(scopes.len(), 2);
        assert_eq!(scopes[0].scope_id, "chat:123");
        assert_eq!(scopes[0].session_count, 2);
        assert_eq!(scopes[0].sender_count, 2);
        assert_eq!(scopes[0].last_seen_at_ms, 30);
        assert_eq!(scopes[1].scope_id, "topic:1:2");
        assert_eq!(scopes[1].session_count, 1);
        assert_eq!(scopes[1].sender_count, 1);
    }

    #[test]
    fn merge_channel_tool_preferences_layers_scope_over_base() {
        let base = ChannelToolPreferences {
            enabled_tools: vec!["read".to_string(), "grep".to_string()],
            disabled_tools: vec!["write".to_string()],
            enabled_mcp_servers: vec!["github".to_string()],
            enabled_mcp_tools: vec!["mcp.github.get_issue".to_string()],
        };
        let scoped = ChannelToolPreferences {
            enabled_tools: vec!["search".to_string(), "read".to_string()],
            disabled_tools: vec!["write".to_string(), "edit".to_string()],
            enabled_mcp_servers: vec!["notion".to_string(), "github".to_string()],
            enabled_mcp_tools: vec![
                "mcp.github.create_issue".to_string(),
                "mcp.notion.search_pages".to_string(),
            ],
        };

        let merged = merge_channel_tool_preferences(base, scoped);

        assert_eq!(
            merged.enabled_tools,
            vec!["read".to_string(), "grep".to_string(), "search".to_string()]
        );
        assert_eq!(
            merged.disabled_tools,
            vec!["write".to_string(), "edit".to_string()]
        );
        assert_eq!(
            merged.enabled_mcp_servers,
            vec!["github".to_string(), "notion".to_string()]
        );
        assert_eq!(
            merged.enabled_mcp_tools,
            vec![
                "mcp.github.get_issue".to_string(),
                "mcp.github.create_issue".to_string(),
                "mcp.notion.search_pages".to_string()
            ]
        );
    }
}
