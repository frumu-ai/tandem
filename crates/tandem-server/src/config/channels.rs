// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub(crate) fn normalize_slack_api_base_url(
    raw: Option<&str>,
) -> Result<Option<String>, &'static str> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    let parsed = reqwest::Url::parse(raw).map_err(|_| "Slack API base URL is invalid")?;
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err("Slack API base URL must not contain credentials, query, or fragment");
    }
    let normalized = raw.trim_end_matches('/');
    if normalized.eq_ignore_ascii_case("https://slack.com/api") {
        return Ok(None);
    }
    // The non-default `acme-demo` feature runs the production path against a
    // loopback Slack mock from a standalone CLI process. Keep the exception
    // compile-time scoped to tests/demo builds and loopback hosts only.
    if (cfg!(test) || cfg!(feature = "acme-demo"))
        && matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .trim_matches(['[', ']'])
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        })
    {
        return Ok(Some(normalized.to_string()));
    }
    Err("Slack API base URL overrides are disabled outside loopback test/demo builds")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfigFile {
    #[serde(default)]
    pub bot_token: String,
    /// Telegram chat ID where approval cards should be posted.
    #[serde(default)]
    pub approval_chat_id: Option<String>,
    #[serde(default = "default_allow_all")]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub mention_only: bool,
    #[serde(default)]
    pub strict_kb_grounding: bool,
    #[serde(default)]
    pub model_provider_id: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub style_profile: tandem_channels::config::TelegramStyleProfile,
    #[serde(default)]
    pub security_profile: tandem_channels::config::ChannelSecurityProfile,
    /// Telegram webhook secret token. When the bot's webhook is registered
    /// (via `setWebhook`) with a `secret_token` parameter, every callback
    /// POST from Telegram includes that exact value in the
    /// `x-telegram-bot-api-secret-token` header. Tandem rejects callback
    /// POSTs whose header does not match this value, preventing a third
    /// party from spoofing button clicks at the engine. Required when the
    /// Telegram interactions endpoint (`POST /channels/telegram/interactions`)
    /// is enabled.
    #[serde(default)]
    pub webhook_secret_token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordConfigFile {
    #[serde(default)]
    pub bot_token: String,
    /// Discord channel ID where approval cards should be posted.
    ///
    /// Reading/listening can still be scoped by guild and mention settings,
    /// but outbound approval delivery needs an explicit destination channel.
    #[serde(default)]
    pub approval_channel_id: Option<String>,
    #[serde(default)]
    pub guild_id: Option<String>,
    #[serde(default = "default_allow_all")]
    pub allowed_users: Vec<String>,
    #[serde(default = "default_discord_mention_only")]
    pub mention_only: bool,
    #[serde(default)]
    pub strict_kb_grounding: bool,
    #[serde(default)]
    pub model_provider_id: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub security_profile: tandem_channels::config::ChannelSecurityProfile,
    /// Discord application public key (32-byte hex). Required when the
    /// Discord interactions endpoint (`POST /channels/discord/interactions`)
    /// is enabled — every interaction POST from Discord is Ed25519-signed
    /// using this key. Discord disables the endpoint if even a single
    /// inbound interaction is unverified, so this is mandatory for any
    /// channel that wants approval cards. Configurable via
    /// `channels.discord.public_key` in `config.json`.
    #[serde(default)]
    pub public_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfigFile {
    #[serde(default)]
    pub bot_token: String,
    #[serde(default)]
    pub channel_id: String,
    /// Slack workspace identifier (the `T...` team ID) expected on every
    /// signed Events API callback. This is distinct from Tandem's tenant
    /// workspace ID under `tenant.workspace_id`.
    #[serde(default, alias = "workspace_id")]
    pub team_id: Option<String>,
    /// Slack application identifier (the `A...` API app ID) expected on every
    /// signed Events API callback.
    #[serde(default, alias = "api_app_id")]
    pub app_id: Option<String>,
    #[serde(default = "default_allow_all")]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub mention_only: bool,
    #[serde(default)]
    pub strict_kb_grounding: bool,
    #[serde(default)]
    pub model_provider_id: Option<String>,
    #[serde(default)]
    pub model_id: Option<String>,
    #[serde(default)]
    pub security_profile: tandem_channels::config::ChannelSecurityProfile,
    /// Slack app signing secret. Required for the Slack interactions and Events
    /// API endpoints; every payload is HMAC-SHA256 signed using this secret.
    /// Stored in the OS keystore in production; this field is the in-memory copy.
    #[serde(default)]
    pub signing_secret: Option<String>,
    /// Route signed Slack Events API message deliveries through the server.
    /// This disables the legacy history poller to prevent duplicate ingress.
    #[serde(default)]
    pub events_enabled: bool,
    /// Tandem tenant this channel is bound to (GOV-B5c). Channel-originated
    /// actions must target this tenant; unset means unbound (local default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<SlackTenantBindingFile>,
    /// GOV-B5b: require an active per-identity step-up grant before honoring
    /// an approval interaction from this channel. Default off.
    #[serde(default, skip_serializing_if = "is_false")]
    pub require_approval_step_up: bool,
    /// Override the Slack API base URL (tests/mocks). Defaults to
    /// `https://slack.com/api`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base_url: Option<String>,
    /// Organization units (departments) bound to this channel. Reserved for
    /// per-channel department scoping (TAN-764); carried through resolution
    /// but not yet enforced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub org_units: Vec<String>,
    /// Post approval cards to this channel. Default `true` (legacy behavior).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify_approvals: Option<bool>,
    /// Additional per-channel connections. Each entry inherits any field it
    /// does not set from the top-level config (installation identity, tokens,
    /// allowlist, profiles, tenant binding). When non-empty, the top-level
    /// `channel_id` still defines a connection of its own if it is non-empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub connections: Vec<SlackConnectionFile>,
}

/// Tandem tenant binding for a Slack channel connection.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct SlackTenantBindingFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deployment_id: Option<String>,
}

/// One Slack channel connection under `channels.slack.connections`.
///
/// Every field except `channel_id` is optional; unset fields inherit the
/// top-level `channels.slack` value, so a workspace-wide app configures its
/// installation identity and secrets once and lists per-department channels
/// as thin entries.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlackConnectionFile {
    pub channel_id: String,
    #[serde(
        default,
        alias = "workspace_id",
        skip_serializing_if = "Option::is_none"
    )]
    pub team_id: Option<String>,
    #[serde(default, alias = "api_app_id", skip_serializing_if = "Option::is_none")]
    pub app_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signing_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events_enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tenant: Option<SlackTenantBindingFile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_users: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention_only: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strict_kb_grounding: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_provider_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub security_profile: Option<tandem_channels::config::ChannelSecurityProfile>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub require_approval_step_up: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub org_units: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify_approvals: Option<bool>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfigFile {
    pub telegram: Option<TelegramConfigFile>,
    pub discord: Option<DiscordConfigFile>,
    pub slack: Option<SlackConfigFile>,
    #[serde(default)]
    pub tool_policy: tandem_channels::config::ChannelToolPolicy,
}

/// A fully resolved Slack channel connection: one authorized `(team, app,
/// channel)` binding with every effective setting applied (per-connection
/// override falling back to the top-level `channels.slack` value).
///
/// This is the single typed contract the server reads Slack config through.
/// Resolution intentionally works on the raw JSON value so field-missing
/// semantics stay identical to the historical pointer reads — notably, a
/// missing `allowed_users` resolves to an **empty list (deny-all)** here,
/// matching the events/interactions ingress paths, while the legacy poller
/// path keeps its own allow-all normalization on [`SlackConfigFile`].
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ResolvedSlackConnection {
    pub channel_id: String,
    pub team_id: Option<String>,
    pub app_id: Option<String>,
    pub bot_token: Option<String>,
    pub signing_secret: Option<String>,
    pub events_enabled: bool,
    pub tenant_org_id: Option<String>,
    pub tenant_workspace_id: Option<String>,
    pub tenant_deployment_id: Option<String>,
    pub allowed_users: Vec<String>,
    pub mention_only: bool,
    pub strict_kb_grounding: Option<bool>,
    pub model_provider_id: Option<String>,
    pub model_id: Option<String>,
    pub security_profile: tandem_channels::config::ChannelSecurityProfile,
    pub require_approval_step_up: bool,
    pub api_base_url: Option<String>,
    /// Departments bound to this connection (consumed by TAN-764; carried,
    /// not yet enforced).
    pub org_units: Vec<String>,
    pub notify_approvals: bool,
}

impl ResolvedSlackConnection {
    /// Events ingress is possible on this connection: opted in and signed.
    pub fn events_capable(&self) -> bool {
        self.events_enabled && self.signing_secret.is_some()
    }

    /// GOV-B5c bound tenant, `Some` only when both ids are set (a partial
    /// binding counts as unbound, matching `channel_bound_tenant`).
    pub fn bound_tenant(&self) -> Option<(String, String)> {
        match (&self.tenant_org_id, &self.tenant_workspace_id) {
            (Some(org), Some(workspace)) => Some((org.clone(), workspace.clone())),
            _ => None,
        }
    }

    /// GOV-B5a: the connection admits everyone via the `*` wildcard.
    pub fn is_open_to_all(&self) -> bool {
        self.allowed_users.iter().any(|entry| entry.trim() == "*")
    }

    /// TAN-764: whether this connection narrows runs to bound departments.
    pub fn binds_departments(&self) -> bool {
        self.org_units.iter().any(|entry| !entry.trim().is_empty())
    }

    /// TAN-764: whether a specific organization unit is bound to this
    /// connection. Entries match either the unit's principal id (e.g.
    /// `department/engineering`) or its bare unit id (`engineering`) so
    /// hand-written configs don't need to know the taxonomy prefix.
    pub fn binds_org_unit(&self, unit_principal_id: &str, unit_id: &str) -> bool {
        self.org_units.iter().any(|entry| {
            let entry = entry.trim();
            !entry.is_empty() && (entry == unit_principal_id || entry == unit_id)
        })
    }
}

fn raw_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_string)
}

fn raw_bool(value: &Value, key: &str) -> Option<bool> {
    value.get(key).and_then(Value::as_bool)
}

fn raw_string_list(value: &Value, key: &str) -> Option<Vec<String>> {
    value.get(key).and_then(Value::as_array).map(|arr| {
        arr.iter()
            .filter_map(Value::as_str)
            .map(|s| s.trim().to_string())
            .collect()
    })
}

fn raw_security_profile(
    value: &Value,
    key: &str,
) -> Option<tandem_channels::config::ChannelSecurityProfile> {
    value
        .get(key)
        .cloned()
        .and_then(|profile| serde_json::from_value(profile).ok())
}

fn raw_tenant(value: &Value) -> (Option<String>, Option<String>, Option<String>) {
    let Some(tenant) = value.get("tenant").filter(|t| !t.is_null()) else {
        return (None, None, None);
    };
    (
        raw_string(tenant, "org_id"),
        raw_string(tenant, "workspace_id"),
        raw_string(tenant, "deployment_id"),
    )
}

/// Resolve one connection entry against the top-level `channels.slack` value.
/// `entry == base` resolves the top-level (legacy) connection itself.
fn resolve_slack_connection_entry(base: &Value, entry: &Value) -> ResolvedSlackConnection {
    let pick_string = |key: &str| raw_string(entry, key).or_else(|| raw_string(base, key));
    let pick_bool = |key: &str| raw_bool(entry, key).or_else(|| raw_bool(base, key));
    let (tenant_org, tenant_workspace, tenant_deployment) = {
        let entry_tenant = raw_tenant(entry);
        if entry.get("tenant").map(|t| !t.is_null()).unwrap_or(false) {
            entry_tenant
        } else {
            raw_tenant(base)
        }
    };
    // `workspace_id`/`api_app_id` aliases from the serde contract also apply
    // to raw resolution so hand-written configs keep working.
    let team_id = raw_string(entry, "team_id")
        .or_else(|| raw_string(entry, "workspace_id"))
        .or_else(|| raw_string(base, "team_id"))
        .or_else(|| raw_string(base, "workspace_id"));
    let entry_app = raw_string(entry, "app_id").or_else(|| raw_string(entry, "api_app_id"));
    let base_app = raw_string(base, "app_id").or_else(|| raw_string(base, "api_app_id"));
    let app_id = entry_app.clone().or_else(|| base_app.clone());
    // A signing secret is a Slack APP credential: an entry that overrides
    // the app identity must NOT inherit the base app's secret — that would
    // let app A's secret verify payloads claiming app B (while app B's real
    // callbacks get rejected). Such an entry resolves secretless and fails
    // closed downstream unless it carries its own secret.
    let inherits_base_app = entry_app.is_none() || entry_app == base_app;
    let signing_secret = raw_string(entry, "signing_secret").or_else(|| {
        inherits_base_app
            .then(|| raw_string(base, "signing_secret"))
            .flatten()
    });

    ResolvedSlackConnection {
        channel_id: raw_string(entry, "channel_id").unwrap_or_default(),
        team_id,
        app_id,
        bot_token: pick_string("bot_token"),
        signing_secret,
        events_enabled: pick_bool("events_enabled").unwrap_or(false),
        tenant_org_id: tenant_org,
        tenant_workspace_id: tenant_workspace,
        tenant_deployment_id: tenant_deployment,
        allowed_users: raw_string_list(entry, "allowed_users")
            .or_else(|| raw_string_list(base, "allowed_users"))
            .unwrap_or_default(),
        mention_only: pick_bool("mention_only").unwrap_or(false),
        strict_kb_grounding: pick_bool("strict_kb_grounding"),
        model_provider_id: pick_string("model_provider_id"),
        model_id: pick_string("model_id"),
        security_profile: raw_security_profile(entry, "security_profile")
            .or_else(|| raw_security_profile(base, "security_profile"))
            .unwrap_or_default(),
        require_approval_step_up: pick_bool("require_approval_step_up").unwrap_or(false),
        api_base_url: normalize_slack_api_base_url(pick_string("api_base_url").as_deref())
            .ok()
            .flatten(),
        org_units: raw_string_list(entry, "org_units")
            .or_else(|| raw_string_list(base, "org_units"))
            .unwrap_or_default(),
        notify_approvals: pick_bool("notify_approvals").unwrap_or(true),
    }
}

/// Resolve every configured Slack connection from the raw `channels.slack`
/// value.
///
/// - No `connections` array (the legacy single-object shape): exactly one
///   connection resolved from the top-level fields, even when `channel_id`
///   is empty — downstream validation produces the same "not configured"
///   errors it always has.
/// - With `connections`: the top-level fields define a connection of their
///   own when `channel_id` is non-empty, followed by each entry (entries
///   with an empty `channel_id` are dropped; an entry that repeats an
///   earlier resolved `(team_id, app_id, channel_id)` binding replaces it —
///   the same tuple the runtime routes events and interactions by, so two
///   installations sharing a channel-id string never collapse into one).
pub fn resolve_slack_connections(slack: &Value) -> Vec<ResolvedSlackConnection> {
    if !slack.is_object() {
        return Vec::new();
    }
    let entries = slack
        .get("connections")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter(|entry| entry.is_object())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if entries.is_empty() {
        return vec![resolve_slack_connection_entry(slack, slack)];
    }

    let mut connections: Vec<ResolvedSlackConnection> = Vec::new();
    let top_level = resolve_slack_connection_entry(slack, slack);
    if !top_level.channel_id.is_empty() {
        connections.push(top_level);
    }
    for entry in entries {
        let resolved = resolve_slack_connection_entry(slack, entry);
        if resolved.channel_id.is_empty() {
            continue;
        }
        if let Some(existing) = connections.iter_mut().find(|existing| {
            existing.team_id == resolved.team_id
                && existing.app_id == resolved.app_id
                && existing.channel_id == resolved.channel_id
        }) {
            *existing = resolved;
        } else {
            connections.push(resolved);
        }
    }
    connections
}

/// Resolve Slack connections from an effective-config snapshot (the value
/// returned by `state.config.get_effective_value()`), i.e. from
/// `/channels/slack`. Returns an empty list when Slack is not configured.
pub fn slack_connections_from_effective_config(
    effective_config: &Value,
) -> Vec<ResolvedSlackConnection> {
    effective_config
        .pointer("/channels/slack")
        .map(resolve_slack_connections)
        .unwrap_or_default()
}

/// Find the connection bound to a specific Slack channel id (used to route
/// outbound messages such as approval-card updates by recipient).
pub fn find_slack_connection_by_channel(
    effective_config: &Value,
    channel_id: &str,
) -> Option<ResolvedSlackConnection> {
    slack_connections_from_effective_config(effective_config)
        .into_iter()
        .find(|connection| connection.channel_id == channel_id)
}

impl SlackConfigFile {
    /// True when any resolved connection can serve signed Events ingress.
    /// Mirrors the historical top-level `events_enabled && signing_secret`
    /// check, extended over `connections`.
    pub fn has_events_capable_connection(&self) -> bool {
        serde_json::to_value(self)
            .map(|value| {
                resolve_slack_connections(&value)
                    .iter()
                    .any(ResolvedSlackConnection::events_capable)
            })
            .unwrap_or(false)
    }

    /// TAN-762: true when any resolved connection carries a governed binding
    /// — a bound tenant (GOV-B5c) or bound departments (TAN-764). Governed
    /// Slack ingress is Events-only: the legacy poller carries no per-sender
    /// verified identity, so a governed binding must never run through it.
    pub fn has_governed_binding(&self) -> bool {
        serde_json::to_value(self)
            .map(|value| {
                resolve_slack_connections(&value).iter().any(|connection| {
                    connection.bound_tenant().is_some() || connection.binds_departments()
                })
            })
            .unwrap_or(false)
    }
}

pub fn normalize_allowed_users_or_wildcard(raw: Vec<String>) -> Vec<String> {
    let normalized = normalize_non_empty_list(raw);
    if normalized.is_empty() {
        return default_allow_all();
    }
    normalized
}

pub fn normalize_allowed_tools(raw: Vec<String>) -> Vec<String> {
    normalize_non_empty_list(raw)
}

fn default_allow_all() -> Vec<String> {
    vec!["*".to_string()]
}

fn default_discord_mention_only() -> bool {
    true
}

fn normalize_non_empty_list(raw: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for item in raw {
        let normalized = item.trim().to_string();
        if normalized.is_empty() {
            continue;
        }
        if seen.insert(normalized.clone()) {
            out.push(normalized);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn partial_channel_entries_without_tokens_still_deserialize() {
        let cfg: ChannelsConfigFile = serde_json::from_value(json!({
            "telegram": {
                "bot_token": "tg-secret",
                "allowed_users": ["123456789"],
                "security_profile": "trusted_team"
            },
            "discord": {
                "allowed_users": ["*"],
                "mention_only": true
            },
            "slack": {
                "channel_id": "C123",
                "allowed_users": ["U1"]
            }
        }))
        .expect("partial channel config should deserialize");

        assert_eq!(
            cfg.telegram
                .as_ref()
                .map(|telegram| telegram.bot_token.as_str()),
            Some("tg-secret")
        );
        assert_eq!(
            cfg.discord
                .as_ref()
                .map(|discord| discord.bot_token.as_str()),
            Some("")
        );
        assert_eq!(
            cfg.slack.as_ref().map(|slack| slack.bot_token.as_str()),
            Some("")
        );
        assert_eq!(
            cfg.slack.as_ref().map(|slack| slack.channel_id.as_str()),
            Some("C123")
        );
    }

    #[test]
    fn legacy_single_object_resolves_one_connection() {
        let slack = json!({
            "bot_token": "xoxb-1",
            "channel_id": "C1",
            "team_id": "T1",
            "app_id": "A1",
            "signing_secret": "shh",
            "events_enabled": true,
            "allowed_users": ["U1"],
            "mention_only": true,
            "tenant": { "org_id": "acme", "workspace_id": "hq" }
        });
        let connections = resolve_slack_connections(&slack);
        assert_eq!(connections.len(), 1);
        let c = &connections[0];
        assert_eq!(c.channel_id, "C1");
        assert_eq!(c.team_id.as_deref(), Some("T1"));
        assert_eq!(c.app_id.as_deref(), Some("A1"));
        assert_eq!(c.bot_token.as_deref(), Some("xoxb-1"));
        assert_eq!(c.signing_secret.as_deref(), Some("shh"));
        assert!(c.events_enabled);
        assert!(c.events_capable());
        assert!(c.mention_only);
        assert_eq!(c.allowed_users, vec!["U1".to_string()]);
        assert_eq!(
            c.bound_tenant(),
            Some(("acme".to_string(), "hq".to_string()))
        );
    }

    #[test]
    fn legacy_single_object_without_channel_id_still_resolves() {
        // Downstream validation owns the "channel id not configured" error;
        // resolution must not silently swallow the connection.
        let slack = json!({ "bot_token": "xoxb-1" });
        let connections = resolve_slack_connections(&slack);
        assert_eq!(connections.len(), 1);
        assert!(connections[0].channel_id.is_empty());
    }

    #[test]
    fn missing_allowed_users_resolves_to_deny_all() {
        // The events/interactions ingress treats a missing allowlist as
        // deny-all; resolution must not substitute the poller's allow-all
        // default here.
        let slack = json!({ "channel_id": "C1" });
        let connections = resolve_slack_connections(&slack);
        assert!(connections[0].allowed_users.is_empty());
        assert!(!connections[0].is_open_to_all());
    }

    #[test]
    fn connections_inherit_top_level_fields() {
        let slack = json!({
            "bot_token": "xoxb-shared",
            "team_id": "T1",
            "app_id": "A1",
            "signing_secret": "shh",
            "events_enabled": true,
            "allowed_users": ["*"],
            "security_profile": "trusted_team",
            "tenant": { "org_id": "acme", "workspace_id": "hq" },
            "connections": [
                { "channel_id": "C_SALES" },
                {
                    "channel_id": "C_ENG",
                    "allowed_users": ["U_ENG"],
                    "mention_only": true,
                    "tenant": { "org_id": "acme", "workspace_id": "eng" },
                    "events_enabled": false
                }
            ]
        });
        let connections = resolve_slack_connections(&slack);
        // Top-level has no channel_id, so only the two entries resolve.
        assert_eq!(connections.len(), 2);

        let sales = &connections[0];
        assert_eq!(sales.channel_id, "C_SALES");
        assert_eq!(sales.bot_token.as_deref(), Some("xoxb-shared"));
        assert_eq!(sales.team_id.as_deref(), Some("T1"));
        assert!(sales.events_capable());
        assert!(sales.is_open_to_all());
        assert_eq!(
            sales.bound_tenant(),
            Some(("acme".to_string(), "hq".to_string()))
        );
        assert_eq!(
            sales.security_profile,
            tandem_channels::config::ChannelSecurityProfile::TrustedTeam
        );

        let eng = &connections[1];
        assert_eq!(eng.channel_id, "C_ENG");
        assert_eq!(eng.allowed_users, vec!["U_ENG".to_string()]);
        assert!(eng.mention_only);
        assert!(!eng.events_enabled);
        assert_eq!(
            eng.bound_tenant(),
            Some(("acme".to_string(), "eng".to_string()))
        );
    }

    #[test]
    fn top_level_channel_becomes_connection_and_entries_can_override_it() {
        let slack = json!({
            "channel_id": "C_MAIN",
            "bot_token": "xoxb-shared",
            "connections": [
                { "channel_id": "C_MAIN", "mention_only": true },
                { "channel_id": "C_OTHER" },
                { "channel_id": "" }
            ]
        });
        let connections = resolve_slack_connections(&slack);
        assert_eq!(connections.len(), 2);
        assert_eq!(connections[0].channel_id, "C_MAIN");
        assert!(connections[0].mention_only, "entry overrides top-level");
        assert_eq!(connections[1].channel_id, "C_OTHER");
    }

    #[test]
    fn connections_sharing_a_channel_id_across_installations_both_survive() {
        // Two installations (different team/app) can legitimately carry the
        // same channel-id string; the runtime routes by the full
        // (team, app, channel) binding, so resolution must keep both.
        let slack = json!({
            "connections": [
                {
                    "channel_id": "C_SHARED",
                    "team_id": "T_A",
                    "app_id": "A_A",
                    "signing_secret": "secret-a",
                    "bot_token": "xoxb-a"
                },
                {
                    "channel_id": "C_SHARED",
                    "team_id": "T_B",
                    "app_id": "A_B",
                    "signing_secret": "secret-b",
                    "bot_token": "xoxb-b"
                }
            ]
        });
        let connections = resolve_slack_connections(&slack);
        assert_eq!(
            connections.len(),
            2,
            "a shared channel-id string must not collapse two installations"
        );
        assert_eq!(connections[0].team_id.as_deref(), Some("T_A"));
        assert_eq!(connections[0].signing_secret.as_deref(), Some("secret-a"));
        assert_eq!(connections[1].team_id.as_deref(), Some("T_B"));
        assert_eq!(connections[1].signing_secret.as_deref(), Some("secret-b"));

        // Same full binding: the later entry still replaces the earlier one.
        let slack = json!({
            "connections": [
                { "channel_id": "C_SHARED", "team_id": "T_A", "app_id": "A_A" },
                {
                    "channel_id": "C_SHARED",
                    "team_id": "T_A",
                    "app_id": "A_A",
                    "mention_only": true
                }
            ]
        });
        let connections = resolve_slack_connections(&slack);
        assert_eq!(connections.len(), 1);
        assert!(connections[0].mention_only);
    }

    #[test]
    fn signing_secret_does_not_inherit_across_app_overrides() {
        // The secret is an app credential: only entries that keep the base
        // app identity may inherit it. An entry overriding app_id without
        // its own secret resolves secretless (and fails closed downstream).
        let slack = json!({
            "team_id": "T1",
            "app_id": "A_BASE",
            "signing_secret": "secret-base",
            "connections": [
                { "channel_id": "C_INHERIT" },
                { "channel_id": "C_SAME_APP", "app_id": "A_BASE" },
                { "channel_id": "C_OTHER_APP", "app_id": "A_OTHER" },
                {
                    "channel_id": "C_OTHER_APP_OWN",
                    "app_id": "A_OTHER",
                    "signing_secret": "secret-other"
                }
            ]
        });
        let connections = resolve_slack_connections(&slack);
        let by_channel = |channel: &str| {
            connections
                .iter()
                .find(|connection| connection.channel_id == channel)
                .unwrap_or_else(|| panic!("connection {channel}"))
        };
        assert_eq!(
            by_channel("C_INHERIT").signing_secret.as_deref(),
            Some("secret-base"),
            "no app override: the base app's secret applies"
        );
        assert_eq!(
            by_channel("C_SAME_APP").signing_secret.as_deref(),
            Some("secret-base"),
            "explicitly the same app: the base app's secret applies"
        );
        assert_eq!(
            by_channel("C_OTHER_APP").signing_secret,
            None,
            "a different app must not inherit the base app's secret"
        );
        assert_eq!(
            by_channel("C_OTHER_APP_OWN").signing_secret.as_deref(),
            Some("secret-other")
        );
    }

    #[test]
    fn events_capable_detection_spans_connections() {
        let top_only: SlackConfigFile = serde_json::from_value(json!({
            "channel_id": "C1",
            "signing_secret": "shh",
            "events_enabled": true
        }))
        .unwrap();
        assert!(top_only.has_events_capable_connection());

        let connection_only: SlackConfigFile = serde_json::from_value(json!({
            "signing_secret": "shh",
            "connections": [ { "channel_id": "C1", "events_enabled": true } ]
        }))
        .unwrap();
        assert!(connection_only.has_events_capable_connection());

        let none: SlackConfigFile = serde_json::from_value(json!({
            "channel_id": "C1",
            "events_enabled": true
        }))
        .unwrap();
        assert!(
            !none.has_events_capable_connection(),
            "events without a signing secret must not count"
        );
    }

    #[test]
    fn find_connection_by_channel_routes_recipients() {
        let effective = json!({
            "channels": {
                "slack": {
                    "bot_token": "xoxb-shared",
                    "connections": [
                        { "channel_id": "C_SALES", "bot_token": "xoxb-sales" },
                        { "channel_id": "C_ENG" }
                    ]
                }
            }
        });
        let sales = find_slack_connection_by_channel(&effective, "C_SALES").unwrap();
        assert_eq!(sales.bot_token.as_deref(), Some("xoxb-sales"));
        let eng = find_slack_connection_by_channel(&effective, "C_ENG").unwrap();
        assert_eq!(eng.bot_token.as_deref(), Some("xoxb-shared"));
        assert!(find_slack_connection_by_channel(&effective, "C_NONE").is_none());
    }

    #[test]
    fn binds_org_unit_matches_principal_or_bare_unit_id() {
        let slack = json!({
            "channel_id": "C1",
            "org_units": ["department/sales", "engineering", "  "]
        });
        let connection = resolve_slack_connections(&slack).remove(0);
        assert!(connection.binds_departments());
        assert!(connection.binds_org_unit("department/sales", "sales"));
        assert!(
            connection.binds_org_unit("department/engineering", "engineering"),
            "bare unit ids must match without the taxonomy prefix"
        );
        assert!(!connection.binds_org_unit("department/finance", "finance"));

        let unbound = resolve_slack_connections(&json!({ "channel_id": "C1" })).remove(0);
        assert!(!unbound.binds_departments());
        let blank_only =
            resolve_slack_connections(&json!({ "channel_id": "C1", "org_units": ["  "] }))
                .remove(0);
        assert!(
            !blank_only.binds_departments(),
            "whitespace-only entries must not count as a department binding"
        );
    }

    #[test]
    fn governed_binding_detection_spans_connections() {
        let tenant_bound: SlackConfigFile = serde_json::from_value(json!({
            "channel_id": "C1",
            "tenant": { "org_id": "acme", "workspace_id": "hq" }
        }))
        .unwrap();
        assert!(tenant_bound.has_governed_binding());

        let department_bound: SlackConfigFile = serde_json::from_value(json!({
            "connections": [ { "channel_id": "C1", "org_units": ["department/sales"] } ]
        }))
        .unwrap();
        assert!(department_bound.has_governed_binding());

        let unbound: SlackConfigFile = serde_json::from_value(json!({
            "bot_token": "xoxb-1",
            "channel_id": "C1"
        }))
        .unwrap();
        assert!(!unbound.has_governed_binding());

        // A partial tenant (org without workspace) counts as unbound, matching
        // bound_tenant() semantics.
        let partial: SlackConfigFile = serde_json::from_value(json!({
            "channel_id": "C1",
            "tenant": { "org_id": "acme" }
        }))
        .unwrap();
        assert!(!partial.has_governed_binding());
    }

    #[test]
    fn new_slack_fields_do_not_serialize_when_unset() {
        let cfg: SlackConfigFile = serde_json::from_value(json!({
            "channel_id": "C1"
        }))
        .unwrap();
        let serialized = serde_json::to_value(&cfg).unwrap();
        for absent in [
            "tenant",
            "require_approval_step_up",
            "api_base_url",
            "org_units",
            "notify_approvals",
            "connections",
        ] {
            assert!(
                serialized.get(absent).is_none(),
                "unset field `{absent}` must not serialize"
            );
        }
    }
}
