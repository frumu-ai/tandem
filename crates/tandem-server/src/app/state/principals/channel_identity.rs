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
    fn channel_kind_str_matches_config_keys() {
        assert_eq!(ChannelKind::Slack.as_str(), "slack");
        assert_eq!(ChannelKind::Discord.as_str(), "discord");
        assert_eq!(ChannelKind::Telegram.as_str(), "telegram");
    }
}
