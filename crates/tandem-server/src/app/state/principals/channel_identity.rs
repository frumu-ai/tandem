//! Resolve a channel surface user (Slack/Discord/Telegram user ID) to a
//! Tandem-owned [`RequestPrincipal`] suitable for audit and authorization.
//!
//! Used by the channel interaction endpoints
//! (`http/slack_interactions.rs`, `http/discord_interactions.rs`,
//! `http/telegram_interactions.rs`) and the future approval-fan-out task
//! before they call into `automations_v2_run_gate_decide`.

use serde_json::Value;
use tandem_types::RequestPrincipal;

/// Which channel surface a click came from. Mirrors the channel adapter
/// names: `"slack"`, `"discord"`, `"telegram"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelKind {
    Slack,
    Discord,
    Telegram,
}

impl ChannelKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ChannelKind::Slack => "slack",
            ChannelKind::Discord => "discord",
            ChannelKind::Telegram => "telegram",
        }
    }
}

/// Outcome of an identity-resolution attempt. Callers MUST treat
/// [`ChannelIdentityResolution::Denied`] as a hard reject — never silently
/// approve as `RequestPrincipal::anonymous()` because the audit trail would
/// then carry no actor for an external mutation.
#[derive(Debug, Clone)]
pub enum ChannelIdentityResolution {
    /// The surface user is allowed and a principal was constructed for them.
    Resolved(RequestPrincipal),
    /// Channel config is missing for this kind. Caller should refuse the
    /// action with a clear error rather than dispatching anonymously.
    ChannelNotConfigured(ChannelKind),
    /// Channel is configured but the surface user is not in `allowed_users`.
    /// Caller should respond with a forbidden status, not 200.
    Denied { kind: ChannelKind, user_id: String },
}

/// Resolve a channel surface user against the configured channel allowlist.
///
/// `effective_config` is the engine's effective config snapshot
/// (`state.config.get_effective_value().await`). The function reads
/// `channels.{kind}.allowed_users` and returns the appropriate resolution.
pub fn resolve_channel_user(
    effective_config: &Value,
    kind: ChannelKind,
    surface_user_id: &str,
) -> ChannelIdentityResolution {
    let user_id = surface_user_id.trim();
    if user_id.is_empty() {
        return ChannelIdentityResolution::Denied {
            kind,
            user_id: String::new(),
        };
    }

    let channel_config = match effective_config.pointer(&format!("/channels/{}", kind.as_str())) {
        Some(c) if !c.is_null() => c,
        _ => return ChannelIdentityResolution::ChannelNotConfigured(kind),
    };

    let allowed_users = channel_config
        .get("allowed_users")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if !user_is_allowed(&allowed_users, user_id) {
        return ChannelIdentityResolution::Denied {
            kind,
            user_id: user_id.to_string(),
        };
    }

    ChannelIdentityResolution::Resolved(build_principal(kind, user_id))
}

/// Resolve a Slack user within one explicitly bound Slack installation.
///
/// Slack user IDs are workspace-scoped, and the same app can be installed in
/// multiple workspaces. Including both the team and app IDs in the Tandem actor
/// ID prevents two installations from collapsing onto one governed principal.
/// The normal Slack allowlist remains the authorization source.
pub fn resolve_slack_user_for_installation(
    effective_config: &Value,
    team_id: &str,
    app_id: &str,
    surface_user_id: &str,
) -> ChannelIdentityResolution {
    let team_id = team_id.trim();
    let app_id = app_id.trim();
    let user_id = surface_user_id.trim();
    if team_id.is_empty() || app_id.is_empty() || user_id.is_empty() {
        return ChannelIdentityResolution::Denied {
            kind: ChannelKind::Slack,
            user_id: user_id.to_string(),
        };
    }

    match resolve_channel_user(effective_config, ChannelKind::Slack, user_id) {
        ChannelIdentityResolution::Resolved(_) => {
            ChannelIdentityResolution::Resolved(RequestPrincipal {
                actor_id: Some(format!("channel:slack:{team_id}:{app_id}:{user_id}")),
                source: "channel:slack".to_string(),
            })
        }
        other => other,
    }
}

/// GOV-B5a: a channel is "open" when its `allowed_users` admits everyone via the
/// `*` wildcard. On such a channel, being allowed to *talk* must not imply approval
/// authority — approval there requires an explicit per-identity capability grant.
pub fn channel_is_open_to_all(effective_config: &Value, kind: ChannelKind) -> bool {
    effective_config
        .pointer(&format!("/channels/{}/allowed_users", kind.as_str()))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .any(|entry| entry.trim() == "*")
        })
        .unwrap_or(false)
}

/// GOV-B5c: the `(org_id, workspace_id)` a channel is bound to, if configured via
/// `/channels/{channel}/tenant`. When unset — the single-tenant/local default — the
/// channel is unbound and adopts the acted-upon run's tenant unchanged, so local
/// operation is unaffected. When set, channel-originated actions must target that
/// tenant, preventing a channel from acting on another tenant's run by id.
pub fn channel_bound_tenant(
    effective_config: &Value,
    kind: ChannelKind,
) -> Option<(String, String)> {
    let tenant = effective_config.pointer(&format!("/channels/{}/tenant", kind.as_str()))?;
    let org_id = tenant
        .get("org_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    let workspace_id = tenant
        .get("workspace_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?
        .to_string();
    Some((org_id, workspace_id))
}

fn user_is_allowed(allowlist: &[String], user_id: &str) -> bool {
    if allowlist.is_empty() {
        // An empty `allowed_users` list is treated as "deny all" — the
        // configured channel must explicitly opt users in. Channel adapters
        // that want "everyone in this room" use `["*"]`.
        return false;
    }
    if allowlist.iter().any(|u| u == "*") {
        return true;
    }
    allowlist.iter().any(|allowed| {
        let allowed = allowed.trim();
        allowed.eq_ignore_ascii_case(user_id)
            || allowed.eq_ignore_ascii_case(&format!("@{user_id}"))
    })
}

fn build_principal(kind: ChannelKind, user_id: &str) -> RequestPrincipal {
    RequestPrincipal {
        actor_id: Some(format!("channel:{}:{}", kind.as_str(), user_id)),
        source: format!("channel:{}", kind.as_str()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolves_slack_user_in_allowlist() {
        let cfg = json!({
            "channels": {
                "slack": { "allowed_users": ["U12345", "U67890"] }
            }
        });
        let result = resolve_channel_user(&cfg, ChannelKind::Slack, "U12345");
        match result {
            ChannelIdentityResolution::Resolved(principal) => {
                assert_eq!(principal.actor_id.as_deref(), Some("channel:slack:U12345"));
                assert_eq!(principal.source, "channel:slack");
            }
            other => panic!("expected Resolved, got {other:?}"),
        }
    }

    #[test]
    fn denies_slack_user_not_in_allowlist() {
        let cfg = json!({
            "channels": {
                "slack": { "allowed_users": ["U12345"] }
            }
        });
        let result = resolve_channel_user(&cfg, ChannelKind::Slack, "U99999");
        assert!(matches!(result, ChannelIdentityResolution::Denied { .. }));
    }

    #[test]
    fn allows_wildcard_allowlist() {
        let cfg = json!({
            "channels": {
                "discord": { "allowed_users": ["*"] }
            }
        });
        let result = resolve_channel_user(&cfg, ChannelKind::Discord, "1234567890");
        assert!(matches!(result, ChannelIdentityResolution::Resolved(_)));
    }

    #[test]
    fn empty_allowlist_denies_everyone() {
        let cfg = json!({
            "channels": {
                "telegram": { "allowed_users": [] }
            }
        });
        let result = resolve_channel_user(&cfg, ChannelKind::Telegram, "12345");
        assert!(matches!(result, ChannelIdentityResolution::Denied { .. }));
    }

    #[test]
    fn missing_allowlist_denies_everyone() {
        let cfg = json!({
            "channels": {
                "slack": { "bot_token": "xoxb-..." }
            }
        });
        let result = resolve_channel_user(&cfg, ChannelKind::Slack, "U12345");
        assert!(matches!(result, ChannelIdentityResolution::Denied { .. }));
    }

    #[test]
    fn returns_channel_not_configured_when_section_missing() {
        let cfg = json!({});
        let result = resolve_channel_user(&cfg, ChannelKind::Slack, "U12345");
        assert!(matches!(
            result,
            ChannelIdentityResolution::ChannelNotConfigured(ChannelKind::Slack)
        ));
    }

    #[test]
    fn empty_user_id_is_denied() {
        let cfg = json!({
            "channels": {
                "slack": { "allowed_users": ["*"] }
            }
        });
        let result = resolve_channel_user(&cfg, ChannelKind::Slack, "");
        assert!(matches!(result, ChannelIdentityResolution::Denied { .. }));
    }

    #[test]
    fn whitespace_user_id_is_trimmed_and_resolved() {
        let cfg = json!({
            "channels": {
                "slack": { "allowed_users": ["U12345"] }
            }
        });
        let result = resolve_channel_user(&cfg, ChannelKind::Slack, "  U12345  ");
        assert!(matches!(result, ChannelIdentityResolution::Resolved(_)));
    }

    #[test]
    fn allowlist_with_at_prefix_matches_unprefixed_user() {
        // Telegram username allowlists are commonly stored as `@evan` —
        // resolve_channel_user must recognize either form.
        let cfg = json!({
            "channels": {
                "telegram": { "allowed_users": ["@evan"] }
            }
        });
        let result = resolve_channel_user(&cfg, ChannelKind::Telegram, "evan");
        assert!(matches!(result, ChannelIdentityResolution::Resolved(_)));
    }

    #[test]
    fn case_insensitive_match() {
        let cfg = json!({
            "channels": {
                "discord": { "allowed_users": ["AliceBot"] }
            }
        });
        let result = resolve_channel_user(&cfg, ChannelKind::Discord, "alicebot");
        assert!(matches!(result, ChannelIdentityResolution::Resolved(_)));
    }

    #[test]
    fn principal_actor_id_distinguishes_channel_kinds() {
        let cfg = json!({
            "channels": {
                "slack": { "allowed_users": ["U12345"] },
                "discord": { "allowed_users": ["U12345"] }
            }
        });
        let slack = resolve_channel_user(&cfg, ChannelKind::Slack, "U12345");
        let discord = resolve_channel_user(&cfg, ChannelKind::Discord, "U12345");
        let slack_id = match slack {
            ChannelIdentityResolution::Resolved(p) => p.actor_id.unwrap(),
            _ => panic!("expected Resolved"),
        };
        let discord_id = match discord {
            ChannelIdentityResolution::Resolved(p) => p.actor_id.unwrap(),
            _ => panic!("expected Resolved"),
        };
        assert_ne!(slack_id, discord_id);
        assert!(slack_id.starts_with("channel:slack:"));
        assert!(discord_id.starts_with("channel:discord:"));
    }

    #[test]
    fn slack_installation_identity_scopes_actor_by_team_and_app() {
        let cfg = json!({
            "channels": {
                "slack": { "allowed_users": ["U12345"] }
            }
        });
        let first = resolve_slack_user_for_installation(&cfg, "T1", "A1", "U12345");
        let second = resolve_slack_user_for_installation(&cfg, "T2", "A1", "U12345");
        let third = resolve_slack_user_for_installation(&cfg, "T1", "A2", "U12345");
        let actor_id = |resolution| match resolution {
            ChannelIdentityResolution::Resolved(principal) => principal.actor_id.unwrap(),
            other => panic!("expected resolved Slack installation identity, got {other:?}"),
        };

        assert_eq!(actor_id(first), "channel:slack:T1:A1:U12345");
        assert_eq!(actor_id(second), "channel:slack:T2:A1:U12345");
        assert_eq!(actor_id(third), "channel:slack:T1:A2:U12345");
    }

    #[test]
    fn slack_installation_identity_rejects_missing_dimensions() {
        let cfg = json!({
            "channels": {
                "slack": { "allowed_users": ["U12345"] }
            }
        });

        assert!(matches!(
            resolve_slack_user_for_installation(&cfg, "", "A1", "U12345"),
            ChannelIdentityResolution::Denied { .. }
        ));
        assert!(matches!(
            resolve_slack_user_for_installation(&cfg, "T1", "", "U12345"),
            ChannelIdentityResolution::Denied { .. }
        ));
    }

    #[test]
    fn channel_kind_str_matches_config_keys() {
        assert_eq!(ChannelKind::Slack.as_str(), "slack");
        assert_eq!(ChannelKind::Discord.as_str(), "discord");
        assert_eq!(ChannelKind::Telegram.as_str(), "telegram");
    }

    #[test]
    fn channel_is_open_to_all_detects_wildcard() {
        let cfg = json!({
            "channels": {
                "slack": { "allowed_users": ["*"] },
                "discord": { "allowed_users": ["U1", "U2"] },
                "telegram": {}
            }
        });
        assert!(channel_is_open_to_all(&cfg, ChannelKind::Slack));
        assert!(!channel_is_open_to_all(&cfg, ChannelKind::Discord));
        assert!(!channel_is_open_to_all(&cfg, ChannelKind::Telegram));
    }

    #[test]
    fn channel_bound_tenant_parses_config_and_defaults_unbound() {
        let cfg = json!({
            "channels": {
                "slack": { "tenant": { "org_id": "org-a", "workspace_id": "ws-a" } },
                "discord": { "tenant": { "org_id": "", "workspace_id": "ws" } },
                "telegram": {}
            }
        });
        assert_eq!(
            channel_bound_tenant(&cfg, ChannelKind::Slack),
            Some(("org-a".to_string(), "ws-a".to_string()))
        );
        // An empty org_id is treated as unbound (not a partial binding).
        assert_eq!(channel_bound_tenant(&cfg, ChannelKind::Discord), None);
        // No `tenant` key -> unbound (single-tenant/local default).
        assert_eq!(channel_bound_tenant(&cfg, ChannelKind::Telegram), None);
    }
}
