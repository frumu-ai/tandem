// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

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
            // The generic entry above synthesizes a "*" wildcard for a
            // missing allowlist — right for the poller-era channels, wrong
            // for Slack: signed Events ingress treats a missing/empty
            // `allowed_users` as DENY-ALL (`resolve_slack_connections`).
            // Report the stored value faithfully so a client echoing this
            // snapshot back can never widen deny-all into open-to-all.
            entry.insert(
                "allowed_users".to_string(),
                Value::Array(
                    channel
                        .and_then(|cfg| cfg.get("allowed_users"))
                        .and_then(Value::as_array)
                        .map(|arr| {
                            arr.iter()
                                .filter_map(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(|value| Value::String(value.to_string()))
                                .collect()
                        })
                        .unwrap_or_default(),
                ),
            );
            // Per-connection summary (TAN-763). Secrets are reported as
            // presence flags only, matching the top-level `token_masked`
            // contract — never the raw values. Deliberately NOT under the
            // real `connections` config key: clients echo this snapshot back
            // through `PUT /channels/slack` (the Channels page Reconnect),
            // and a lossy summary deserialized as connection config would
            // wipe per-connection secrets and tenant bindings.
            if let Some(cfg) = channel {
                let connections =
                    crate::config::channels::resolve_slack_connections(&Value::Object(cfg.clone()))
                        .into_iter()
                        .map(|connection| {
                            serde_json::json!({
                                "channel_id": connection.channel_id,
                                "team_id": connection.team_id,
                                "app_id": connection.app_id,
                                "has_token": connection.bot_token.is_some(),
                                "has_signing_secret": connection.signing_secret.is_some(),
                                "events_enabled": connection.events_enabled,
                                "events_capable": connection.events_capable(),
                                "mention_only": connection.mention_only,
                                "notify_approvals": connection.notify_approvals,
                                "tenant_org_id": connection.tenant_org_id,
                                "tenant_workspace_id": connection.tenant_workspace_id,
                                "org_units": connection.org_units,
                            })
                        })
                        .collect::<Vec<_>>();
                entry.insert("connections_summary".to_string(), Value::Array(connections));
            }
        }
        _ => {}
    }
    entry
}

/// One Slack sender observed on signed ingress, aggregated from the
/// protected audit ledger (TAN-765). Gives admins the exact principal to map
/// to a department without hand-composing `channel:slack:{team}:{app}:{user}`.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SlackSenderSummary {
    pub team_id: String,
    pub app_id: String,
    pub user_id: String,
    /// The exact principal string the membership APIs expect as `member_id`.
    pub principal: String,
    /// Channels this sender was observed in (denials that predate channel
    /// resolution may carry none).
    pub channels: Vec<String>,
    pub accepted_count: u64,
    pub denied_count: u64,
    pub last_seen_at_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_denial_reason: Option<String>,
    /// Whether the principal holds at least one active org-unit membership
    /// AND satisfies the department binding of every configured channel they
    /// were observed in — a sender denied by a department-bound channel they
    /// don't belong to is NOT mapped, even if they hold memberships elsewhere.
    pub mapped: bool,
    /// Active org-unit principal ids for this sender (empty when unmapped).
    pub org_units: Vec<String>,
    /// Per-observed-channel mapping state, so admins can see which channel's
    /// department binding a sender still needs.
    pub channel_access: Vec<SlackSenderChannelAccess>,
    pub tenant_org_id: String,
    pub tenant_workspace_id: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SlackSenderChannelAccess {
    pub channel_id: String,
    /// Departments the connection binds (empty = unbound channel).
    pub bound_org_units: Vec<String>,
    /// Whether this sender passes this channel's department gate: for a
    /// department-bound channel, an active membership in a bound unit; for an
    /// unbound channel, any active membership.
    pub mapped: bool,
    /// False when no configured connection currently claims this channel
    /// (e.g. denials recorded before the channel was removed from config).
    pub configured: bool,
}

#[derive(Debug, Default)]
struct SlackSenderAggregate {
    channels: std::collections::BTreeSet<String>,
    accepted_count: u64,
    denied_count: u64,
    last_seen_at_ms: u64,
    last_denied_at_ms: u64,
    last_denial_reason: Option<String>,
}

const SLACK_SENDERS_CAP: usize = 500;

/// `GET /channels/slack/senders` — recently seen Slack senders per bound
/// tenant, with mapped/unmapped department status (TAN-765). Data source is
/// the protected audit ledger (`channel.slack.ingress.accepted` / `.denied`),
/// so the fail-closed unmapped state is visible and actionable.
pub(crate) async fn slack_senders(State(state): State<AppState>) -> Response {
    let effective = state.config.get_effective_value().await;
    let connections = crate::config::channels::slack_connections_from_effective_config(&effective);
    let mut tenants = connections
        .iter()
        .filter_map(|connection| connection.bound_tenant())
        .collect::<Vec<_>>();
    tenants.sort();
    tenants.dedup();

    let mut senders: Vec<SlackSenderSummary> = Vec::new();
    for (org_id, workspace_id) in tenants {
        let tenant =
            tandem_types::TenantContext::explicit(org_id.clone(), workspace_id.clone(), None);
        let events = crate::audit::load_protected_audit_events_for_tenant(&state, &tenant).await;

        let mut aggregates: HashMap<(String, String, String), SlackSenderAggregate> =
            HashMap::new();
        for event in &events {
            let (dims, denial_reason) = match event.event_type.as_str() {
                "channel.slack.ingress.accepted" => {
                    (event.payload.pointer("/dimensions").cloned(), None)
                }
                "channel.slack.ingress.denied" => (
                    event.payload.pointer("/details").cloned(),
                    event
                        .payload
                        .pointer("/reason")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                ),
                _ => continue,
            };
            let Some(dims) = dims else { continue };
            let field = |key: &str| {
                dims.get(key)
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string)
            };
            let (Some(team_id), Some(app_id), Some(user_id)) = (
                field("slack_team_id"),
                field("slack_app_id"),
                field("slack_user_id"),
            ) else {
                continue;
            };
            let aggregate = aggregates.entry((team_id, app_id, user_id)).or_default();
            if let Some(channel_id) = field("slack_channel_id") {
                aggregate.channels.insert(channel_id);
            }
            aggregate.last_seen_at_ms = aggregate.last_seen_at_ms.max(event.created_at_ms);
            match denial_reason {
                Some(reason) => {
                    aggregate.denied_count += 1;
                    if event.created_at_ms >= aggregate.last_denied_at_ms {
                        aggregate.last_denied_at_ms = event.created_at_ms;
                        aggregate.last_denial_reason = Some(reason);
                    }
                }
                None => aggregate.accepted_count += 1,
            }
        }
        if aggregates.is_empty() {
            continue;
        }

        // Resolve mapped status against the tenant's authority graph once.
        let graph = state
            .build_intra_tenant_authority_graph(&tenant, Vec::new())
            .await;
        let now_ms = crate::now_ms();
        for ((team_id, app_id, user_id), aggregate) in aggregates {
            let principal = format!("channel:slack:{team_id}:{app_id}:{user_id}");
            let principal_ref = tandem_types::PrincipalRef::human_user(principal.clone());
            let resolved_units = graph.resolved_unit_principals(&principal_ref, now_ms);
            let active_units = graph
                .units
                .iter()
                .filter(|unit| {
                    unit.state.is_active() && resolved_units.contains(&unit.principal_ref())
                })
                .map(|unit| (unit.principal_ref().id, unit.unit_id.clone()))
                .collect::<Vec<_>>();
            let mut org_units = active_units
                .iter()
                .map(|(principal_id, _)| principal_id.clone())
                .collect::<Vec<_>>();
            org_units.sort();
            org_units.dedup();
            let has_membership = !org_units.is_empty();

            // Mapping is per channel: a department-bound channel only counts
            // as mapped when the sender belongs to one of ITS bound units
            // (the same gate the run-time intersection enforces), so a
            // sales-bound denial is not masked by an engineering membership.
            let channel_access = aggregate
                .channels
                .iter()
                .map(|channel_id| {
                    let connection = connections.iter().find(|connection| {
                        connection.channel_id == *channel_id
                            && connection.team_id.as_deref() == Some(team_id.as_str())
                            && connection.app_id.as_deref() == Some(app_id.as_str())
                            && connection.bound_tenant()
                                == Some((org_id.clone(), workspace_id.clone()))
                    });
                    let Some(connection) = connection else {
                        return SlackSenderChannelAccess {
                            channel_id: channel_id.clone(),
                            bound_org_units: Vec::new(),
                            mapped: false,
                            configured: false,
                        };
                    };
                    let bound_org_units = connection
                        .org_units
                        .iter()
                        .map(|entry| entry.trim().to_string())
                        .filter(|entry| !entry.is_empty())
                        .collect::<Vec<_>>();
                    let mapped = if connection.binds_departments() {
                        active_units.iter().any(|(principal_id, unit_id)| {
                            connection.binds_org_unit(principal_id, unit_id)
                        })
                    } else {
                        has_membership
                    };
                    SlackSenderChannelAccess {
                        channel_id: channel_id.clone(),
                        bound_org_units,
                        mapped,
                        configured: true,
                    }
                })
                .collect::<Vec<_>>();
            let mapped = has_membership
                && channel_access
                    .iter()
                    .filter(|access| access.configured)
                    .all(|access| access.mapped);
            senders.push(SlackSenderSummary {
                team_id,
                app_id,
                user_id,
                principal,
                channels: aggregate.channels.into_iter().collect(),
                accepted_count: aggregate.accepted_count,
                denied_count: aggregate.denied_count,
                last_seen_at_ms: aggregate.last_seen_at_ms,
                last_denial_reason: aggregate.last_denial_reason,
                mapped,
                org_units,
                channel_access,
                tenant_org_id: org_id.clone(),
                tenant_workspace_id: workspace_id.clone(),
            });
        }
    }

    senders.sort_by(|a, b| b.last_seen_at_ms.cmp(&a.last_seen_at_ms));
    let truncated = senders.len() > SLACK_SENDERS_CAP;
    senders.truncate(SLACK_SENDERS_CAP);

    Json(serde_json::json!({
        "senders": senders,
        "truncated": truncated,
    }))
    .into_response()
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
        "slack" => Ok(Json(slack_channel_verify(&state).await)),
        _ => Err(StatusCode::NOT_FOUND),
    }
}

/// Verify every configured Slack connection against the live installation
/// (TAN-766): the bot token must authenticate (`auth.test`), belong to the
/// connection's `team_id`, and — when `app_id` is configured — to the same
/// Slack app (`bots.info`). Mirrors the runtime outbound binding check, but
/// runs from config alone so the panel can verify before any event arrives.
async fn slack_channel_verify(state: &AppState) -> Value {
    let effective = state.config.get_effective_value().await;
    let connections = crate::config::channels::slack_connections_from_effective_config(&effective);
    if connections.is_empty() {
        return json!({
            "ok": false,
            "channel": "slack",
            "connections": [],
            "hints": ["Configure channels.slack (bot_token, channel_id) before verifying."],
        });
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build();
    let client = match client {
        Ok(client) => client,
        Err(error) => {
            return json!({
                "ok": false,
                "channel": "slack",
                "connections": [],
                "hints": [format!("HTTP client construction failed: {error}")],
            });
        }
    };

    let mut rows = Vec::new();
    let mut all_ok = true;
    for connection in &connections {
        let row = slack_connection_verify(&client, connection).await;
        if row.get("ok") != Some(&Value::Bool(true)) {
            all_ok = false;
        }
        rows.push(row);
    }
    json!({
        "ok": all_ok,
        "channel": "slack",
        "connections": rows,
    })
}

async fn slack_connection_verify(
    client: &reqwest::Client,
    connection: &crate::config::channels::ResolvedSlackConnection,
) -> Value {
    let base = json!({
        "channel_id": connection.channel_id,
        "team_id": connection.team_id,
        "app_id": connection.app_id,
        "events_capable": connection.events_capable(),
    });
    let mut row = base;
    let Some(bot_token) = connection.bot_token.as_deref() else {
        row["ok"] = json!(false);
        row["error"] = json!("bot token not configured");
        return row;
    };
    let api_base_url = connection
        .api_base_url
        .clone()
        .unwrap_or_else(|| "https://slack.com/api".to_string());
    let api_base_url = api_base_url.trim_end_matches('/');

    let auth = match client
        .get(format!("{api_base_url}/auth.test"))
        .bearer_auth(bot_token)
        .send()
        .await
    {
        Ok(response) => match response.json::<Value>().await {
            Ok(body) => body,
            Err(error) => {
                row["ok"] = json!(false);
                row["error"] = json!(format!("Slack auth.test response was not JSON: {error}"));
                return row;
            }
        },
        Err(error) => {
            row["ok"] = json!(false);
            row["error"] = json!(format!("Slack auth.test request failed: {error}"));
            return row;
        }
    };
    if auth.get("ok") != Some(&Value::Bool(true)) {
        row["ok"] = json!(false);
        row["error"] = json!(format!(
            "Slack auth.test rejected bot token: {}",
            auth.get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        ));
        return row;
    }
    row["token_ok"] = json!(true);

    let auth_team = auth.get("team_id").and_then(Value::as_str);
    if let Some(expected_team) = connection.team_id.as_deref() {
        let team_ok = auth_team == Some(expected_team);
        row["team_ok"] = json!(team_ok);
        if !team_ok {
            row["ok"] = json!(false);
            row["error"] = json!(format!(
                "bot token belongs to team {}, expected {expected_team}",
                auth_team.unwrap_or("unknown")
            ));
            return row;
        }
    }

    if let Some(expected_app) = connection.app_id.as_deref() {
        let Some(bot_id) = auth.get("bot_id").and_then(Value::as_str) else {
            row["ok"] = json!(false);
            row["error"] = json!("Slack auth.test token is not a bot identity");
            return row;
        };
        let bots_info = match client
            .get(format!("{api_base_url}/bots.info"))
            .bearer_auth(bot_token)
            .query(&[("bot", bot_id)])
            .send()
            .await
        {
            Ok(response) => response.json::<Value>().await.unwrap_or_default(),
            Err(error) => {
                row["ok"] = json!(false);
                row["error"] = json!(format!("Slack bots.info request failed: {error}"));
                return row;
            }
        };
        let app_ok = bots_info.get("ok") == Some(&Value::Bool(true))
            && bots_info.pointer("/bot/app_id").and_then(Value::as_str) == Some(expected_app);
        row["app_ok"] = json!(app_ok);
        if !app_ok {
            row["ok"] = json!(false);
            row["error"] = json!("bot token belongs to a different Slack app");
            return row;
        }
    }

    row["ok"] = json!(true);
    row
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
            let mut slack_identity_unchanged = true;
            if let Some(cfg) = input.as_object_mut() {
                let bot_token_provided = cfg
                    .get("bot_token")
                    .and_then(Value::as_str)
                    .is_some_and(|v| !v.trim().is_empty());
                if !bot_token_provided {
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
                // Connection-only configs have no top-level channel: GET
                // reports `channel_id: null`, and a client echoing that
                // snapshot back must not 400 on `SlackConfigFile` expecting a
                // string before the preserved `connections[]` can make the
                // config startable.
                if cfg.get("channel_id").is_some_and(Value::is_null) {
                    cfg.remove("channel_id");
                }
                // `GET /channels/config` returns a sanitized snapshot without
                // these fields (secrets are presence flags, connections a
                // summary). A client echoing that snapshot back — the
                // Channels page Reconnect does — must not wipe the Slack
                // installation identity, Events ingress, or per-connection /
                // governance bindings. Absent keys inherit the stored value;
                // explicitly provided values (including `[]`) still win.
                const PRESERVED_SLACK_KEYS: [&str; 9] = [
                    "team_id",
                    "app_id",
                    "events_enabled",
                    "tenant",
                    "org_units",
                    "require_approval_step_up",
                    "api_base_url",
                    "notify_approvals",
                    "allowed_users",
                ];
                if let Some(existing) = existing_channel_cfg(spec) {
                    for key in PRESERVED_SLACK_KEYS {
                        if !cfg.contains_key(key) {
                            if let Some(value) = existing.get(key) {
                                cfg.insert(key.to_string(), value.clone());
                            }
                        }
                    }
                    // Secret-BEARING keys are identity-gated: `existing`
                    // comes from the effective config, so it carries
                    // keystore-injected secrets. Carrying them into a save
                    // that CHANGES team/app would re-hoist app A's
                    // credentials under app B's installation ids and defeat
                    // the fail-closed migration semantics.
                    let installation = |map: &serde_json::Map<String, Value>| {
                        let raw = |keys: [&str; 2]| {
                            keys.iter()
                                .find_map(|key| map.get(*key))
                                .and_then(Value::as_str)
                                .map(str::trim)
                                .filter(|value| !value.is_empty())
                                .map(str::to_ascii_lowercase)
                        };
                        (
                            raw(["team_id", "workspace_id"]),
                            raw(["app_id", "api_app_id"]),
                        )
                    };
                    let identity_unchanged = installation(cfg) == installation(existing);
                    slack_identity_unchanged = identity_unchanged;
                    if identity_unchanged && !cfg.contains_key("signing_secret") {
                        if let Some(value) = existing.get("signing_secret") {
                            cfg.insert("signing_secret".to_string(), value.clone());
                        }
                    }
                    // The bot token filled above is the OLD installation's:
                    // a save that migrates team/app must supply its own
                    // token (or carry self-declared connections) rather
                    // than resolve the new installation with the old app's
                    // credentials — delivery would fail closed on binding
                    // checks while the save looked usable.
                    if !identity_unchanged && !bot_token_provided {
                        cfg.remove("bot_token");
                    }
                    if !cfg.contains_key("connections") {
                        if let Some(Value::Array(entries)) = existing.get("connections") {
                            let preserved = entries
                                .iter()
                                .map(|entry| {
                                    let Some(entry) = entry.as_object() else {
                                        return entry.clone();
                                    };
                                    let mut entry = entry.clone();
                                    // An entry that self-declares BOTH team
                                    // and app keeps its resolved identity
                                    // regardless of the top level; anything
                                    // inheriting resolves under the NEW
                                    // identity, so its old secrets must not
                                    // ride along.
                                    let (entry_team, entry_app) = installation(&entry);
                                    let self_declared = entry_team.is_some() && entry_app.is_some();
                                    if !identity_unchanged && !self_declared {
                                        entry.remove("bot_token");
                                        entry.remove("botToken");
                                        entry.remove("signing_secret");
                                        entry.remove("signingSecret");
                                    }
                                    Value::Object(entry)
                                })
                                .collect::<Vec<_>>();
                            cfg.insert("connections".to_string(), Value::Array(preserved));
                        }
                    }
                }
                // Still absent (nothing provided, nothing stored): pin the
                // allowlist to an explicit empty list so SlackConfigFile's
                // legacy `default_allow_all` serde default cannot turn a
                // fresh save into an open-to-all signed-ingress config.
                if !cfg.contains_key("allowed_users") {
                    cfg.insert("allowed_users".to_string(), Value::Array(Vec::new()));
                }
            }
            let mut cfg: SlackConfigFile =
                serde_json::from_value(input).map_err(|_| StatusCode::BAD_REQUEST)?;
            // Unlike telegram/discord, the Slack allowlist is stored
            // faithfully — NOT normalized to a "*" wildcard. Signed Events
            // ingress treats a missing/empty allowlist as deny-all, so
            // persisting a synthesized wildcard would silently open every
            // inheriting connection; the poller/notifier paths still
            // normalize at use time, keeping legacy behavior. Opening a
            // channel to everyone requires an explicit `["*"]`.
            cfg.allowed_users = cfg
                .allowed_users
                .into_iter()
                .map(|user| user.trim().to_string())
                .filter(|user| !user.is_empty())
                .collect();
            cfg.model_provider_id = trim_optional_string(cfg.model_provider_id);
            cfg.model_id = trim_optional_string(cfg.model_id);
            if cfg.bot_token.trim().is_empty() && slack_identity_unchanged {
                cfg.bot_token = existing_bot_token(spec).unwrap_or_default();
            }
            if cfg.channel_id.trim().is_empty() {
                cfg.channel_id = existing_channel_id(spec).unwrap_or_default();
            }
            // Startability is judged on the RESOLVED connections, not the
            // legacy top-level fields: a config that carries channel_id and
            // bot_token only inside `connections[]` is just as valid as the
            // single-channel shape (entries inherit unset fields, so the
            // legacy shape reduces to the same check).
            let resolved =
                crate::config::channels::resolve_slack_connections(&serde_json::json!(cfg));
            let has_usable_connection = resolved.iter().any(|connection| {
                !connection.channel_id.is_empty()
                    && connection
                        .bot_token
                        .as_deref()
                        .is_some_and(|token| !token.trim().is_empty())
            });
            if !has_usable_connection && !channel_is_connected(spec) {
                return Err(StatusCode::BAD_REQUEST);
            }
            // Purge stored credentials for connections this save REMOVES:
            // a binding dropped from `connections[]` must not keep a live
            // keystore entry that would silently resurrect its credentials
            // if the same (team, app, channel) binding is ever re-added.
            // Layer-safe because PUT replaces only the project-level object.
            let new_slack_value = serde_json::json!(cfg);
            let empty_map = serde_json::Map::new();
            let new_slack_obj = new_slack_value.as_object().unwrap_or(&empty_map);
            let kept_ids: std::collections::HashSet<String> = new_slack_obj
                .get("connections")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(Value::as_object)
                .flat_map(|entry| tandem_core::slack_connection_secret_ids(new_slack_obj, entry))
                .collect();
            if let Some(previous_obj) = channels_obj.get(spec.config_key).and_then(Value::as_object)
            {
                if let Some(previous_entries) =
                    previous_obj.get("connections").and_then(Value::as_array)
                {
                    for entry in previous_entries.iter().filter_map(Value::as_object) {
                        for id in tandem_core::slack_connection_secret_ids(previous_obj, entry) {
                            if !kept_ids.contains(&id) {
                                let _ = tandem_core::delete_provider_auth(&id);
                            }
                        }
                    }
                }
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
    if spec.name == "slack" {
        // Deleting the channel revokes ALL its stored credentials — the
        // top-level signing secret and every per-connection entry — so a
        // later re-add cannot silently resurrect them from the keystore.
        tandem_core::purge_slack_channel_secrets();
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
    let enabled_mcp_tools = filter_enabled_mcp_tools_by_enabled_servers(
        &enabled_mcp_servers,
        unique_strings(prefs.enabled_mcp_tools),
    );

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

fn mcp_namespace_segment(raw: &str) -> String {
    raw.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn mcp_tool_server_namespace(tool: &str) -> Option<String> {
    let rest = tool.strip_prefix("mcp.")?;
    let namespace = rest.split('.').next()?.trim();
    if namespace.is_empty() {
        return None;
    }
    Some(namespace.to_string())
}

fn filter_enabled_mcp_tools_by_enabled_servers(
    enabled_mcp_servers: &[String],
    enabled_mcp_tools: Vec<String>,
) -> Vec<String> {
    let enabled_namespaces = enabled_mcp_servers
        .iter()
        .map(|server| mcp_namespace_segment(server))
        .collect::<std::collections::HashSet<_>>();
    if enabled_namespaces.is_empty() {
        return Vec::new();
    }
    enabled_mcp_tools
        .into_iter()
        .filter(|tool| {
            mcp_tool_server_namespace(tool)
                .as_ref()
                .is_some_and(|namespace| enabled_namespaces.contains(namespace))
        })
        .collect()
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
    fn operator_drops_exact_mcp_tools_when_server_is_disabled() {
        let prefs = ChannelToolPreferences {
            enabled_tools: vec!["read".to_string()],
            disabled_tools: Vec::new(),
            enabled_mcp_servers: vec!["notion".to_string()],
            enabled_mcp_tools: vec![
                "mcp.github.create_issue".to_string(),
                "mcp.notion.search".to_string(),
            ],
        };

        let sanitized = sanitize_tool_preferences_for_security_profile(
            prefs,
            tandem_channels::config::ChannelSecurityProfile::Operator,
        );

        assert_eq!(sanitized.enabled_mcp_servers, vec!["notion".to_string()]);
        assert_eq!(
            sanitized.enabled_mcp_tools,
            vec!["mcp.notion.search".to_string()]
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
