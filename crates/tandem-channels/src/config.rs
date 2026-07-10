//! Configuration for tandem-channels adapters.
//!
//! Config is loaded in priority order: environment variables > `config.json`.
//! Calling `ChannelsConfig::from_env()` reads the relevant `TANDEM_*` env vars
//! and returns `Err` only if *no* channels are configured.

use anyhow::bail;

/// Top-level channels configuration.
#[derive(Debug, Clone, Default)]
pub struct ChannelsConfig {
    pub telegram: Option<TelegramConfig>,
    pub discord: Option<DiscordConfig>,
    pub slack: Option<SlackConfig>,
    /// Base URL of the running tandem-server, e.g. `http://127.0.0.1:39731`.
    pub server_base_url: String,
    /// Value of `TANDEM_API_TOKEN` — used as `Authorization: Bearer <token>`.
    pub api_token: String,
    /// Optional signed tenant context assertion for channel-originated engine calls.
    pub context_assertion: Option<String>,
    /// Default policy for tool execution coming from channel commands
    pub tool_policy: ChannelToolPolicy,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelSecurityProfile {
    #[default]
    Operator,
    TrustedTeam,
    PublicDemo,
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelToolPolicy {
    #[default]
    RequireApproval,
    AllowAll,
    DenyAll,
}

#[derive(Debug, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    /// `["*"]` = allow everyone. Otherwise a list of usernames or user IDs.
    pub allowed_users: Vec<String>,
    /// Only respond when the bot is @-mentioned (useful in group chats).
    pub mention_only: bool,
    /// Presentation preset for outgoing Telegram messages.
    pub style_profile: TelegramStyleProfile,
    /// Security profile controlling what public channel sessions can do.
    pub security_profile: ChannelSecurityProfile,
}

#[derive(Debug, Clone, Copy, Default, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TelegramStyleProfile {
    #[default]
    Default,
    Compact,
    Friendly,
    Ops,
}

#[derive(Debug, Clone)]
pub struct DiscordConfig {
    pub bot_token: String,
    /// Optional: restrict listening to a specific guild/server.
    pub guild_id: Option<String>,
    /// `["*"]` = allow everyone.
    pub allowed_users: Vec<String>,
    /// Only respond to messages that @-mention the bot.
    pub mention_only: bool,
    /// Security profile controlling what public channel sessions can do.
    pub security_profile: ChannelSecurityProfile,
}

#[derive(Debug, Clone)]
pub struct SlackConfig {
    pub bot_token: String,
    /// Slack channel ID to poll (e.g. `C0XXXXXXXX`).
    pub channel_id: String,
    /// `["*"]` = allow everyone.
    pub allowed_users: Vec<String>,
    /// Only respond to messages that @-mention the bot.
    pub mention_only: bool,
    /// Security profile controlling what public channel sessions can do.
    pub security_profile: ChannelSecurityProfile,
}

/// Parse a comma-separated allowed_users string into a Vec.
/// `"*"` is kept as-is; leading/trailing whitespace is stripped per item.
pub fn parse_allowed_users(raw: &str) -> Vec<String> {
    if raw.trim() == "*" {
        return vec!["*".to_string()];
    }
    raw.split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Returns `true` if `user` is permitted based on `allowed_users`.
pub fn is_user_allowed(user: &str, allowed_users: &[String]) -> bool {
    if allowed_users.is_empty() {
        return false; // deny-by-default when list is empty
    }
    allowed_users.iter().any(|a| a == "*" || a == user)
}

impl ChannelsConfig {
    /// Build from environment variables. Returns `Err` if no channels are configured.
    pub fn from_env() -> anyhow::Result<Self> {
        let server_base_url = std::env::var("TANDEM_SERVER_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:39731".to_string());
        let api_token = std::env::var("TANDEM_API_TOKEN").unwrap_or_default();
        let context_assertion = std::env::var("TANDEM_CHANNEL_CONTEXT_ASSERTION")
            .ok()
            .or_else(|| std::env::var("TANDEM_CONTEXT_ASSERTION").ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let tool_policy = match std::env::var("TANDEM_CHANNEL_TOOL_POLICY").as_deref() {
            Ok("allow_all") => ChannelToolPolicy::AllowAll,
            Ok("deny_all") => ChannelToolPolicy::DenyAll,
            _ => ChannelToolPolicy::RequireApproval,
        };

        let telegram = Self::telegram_from_env();
        let discord = Self::discord_from_env();
        let slack = Self::slack_from_env();

        if telegram.is_none() && discord.is_none() && slack.is_none() {
            bail!(
                "no channels configured — set at least one of: \
                TANDEM_TELEGRAM_BOT_TOKEN, TANDEM_DISCORD_BOT_TOKEN, TANDEM_SLACK_BOT_TOKEN"
            );
        }

        Ok(Self {
            telegram,
            discord,
            slack,
            server_base_url,
            api_token,
            context_assertion,
            tool_policy,
        })
    }

    fn telegram_from_env() -> Option<TelegramConfig> {
        let bot_token = std::env::var("TANDEM_TELEGRAM_BOT_TOKEN").ok()?;
        if bot_token.trim().is_empty() {
            return None;
        }
        let allowed_users = std::env::var("TANDEM_TELEGRAM_ALLOWED_USERS")
            .map(|s| parse_allowed_users(&s))
            .unwrap_or_else(|_| vec!["*".to_string()]);
        let mention_only = std::env::var("TANDEM_TELEGRAM_MENTION_ONLY")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        let style_profile = std::env::var("TANDEM_TELEGRAM_STYLE_PROFILE")
            .ok()
            .map(|raw| match raw.trim().to_ascii_lowercase().as_str() {
                "compact" => TelegramStyleProfile::Compact,
                "friendly" => TelegramStyleProfile::Friendly,
                "ops" => TelegramStyleProfile::Ops,
                _ => TelegramStyleProfile::Default,
            })
            .unwrap_or_default();
        let security_profile = parse_channel_security_profile_env(
            std::env::var("TANDEM_TELEGRAM_SECURITY_PROFILE")
                .ok()
                .as_deref(),
        );
        Some(TelegramConfig {
            bot_token,
            allowed_users,
            mention_only,
            style_profile,
            security_profile,
        })
    }

    fn discord_from_env() -> Option<DiscordConfig> {
        let bot_token = std::env::var("TANDEM_DISCORD_BOT_TOKEN").ok()?;
        if bot_token.trim().is_empty() {
            return None;
        }
        let guild_id = std::env::var("TANDEM_DISCORD_GUILD_ID")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let allowed_users = std::env::var("TANDEM_DISCORD_ALLOWED_USERS")
            .map(|s| parse_allowed_users(&s))
            .unwrap_or_else(|_| vec!["*".to_string()]);
        let mention_only = std::env::var("TANDEM_DISCORD_MENTION_ONLY")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(true); // default true for Discord — avoids bots fighting each other
        let security_profile = parse_channel_security_profile_env(
            std::env::var("TANDEM_DISCORD_SECURITY_PROFILE")
                .ok()
                .as_deref(),
        );
        Some(DiscordConfig {
            bot_token,
            guild_id,
            allowed_users,
            mention_only,
            security_profile,
        })
    }

    fn slack_from_env() -> Option<SlackConfig> {
        let bot_token = std::env::var("TANDEM_SLACK_BOT_TOKEN").ok()?;
        if bot_token.trim().is_empty() {
            return None;
        }
        let channel_id = std::env::var("TANDEM_SLACK_CHANNEL_ID").ok()?;
        if channel_id.trim().is_empty() {
            return None;
        }
        let allowed_users = std::env::var("TANDEM_SLACK_ALLOWED_USERS")
            .map(|s| parse_allowed_users(&s))
            .unwrap_or_else(|_| vec!["*".to_string()]);
        let mention_only = std::env::var("TANDEM_SLACK_MENTION_ONLY")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);
        let security_profile = parse_channel_security_profile_env(
            std::env::var("TANDEM_SLACK_SECURITY_PROFILE")
                .ok()
                .as_deref(),
        );
        Some(SlackConfig {
            bot_token,
            channel_id,
            allowed_users,
            mention_only,
            security_profile,
        })
    }
}

fn parse_channel_security_profile_env(raw: Option<&str>) -> ChannelSecurityProfile {
    match raw.map(|value| value.trim().to_ascii_lowercase()) {
        Some(value) if value == "trusted_team" || value == "trusted-team" => {
            ChannelSecurityProfile::TrustedTeam
        }
        Some(value) if value == "public_demo" || value == "public-demo" => {
            ChannelSecurityProfile::PublicDemo
        }
        _ => ChannelSecurityProfile::Operator,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_user_allowed_wildcard() {
        assert!(is_user_allowed("anyone", &["*".to_string()]));
    }

    #[test]
    fn test_is_user_allowed_specific() {
        let list = vec!["@user123".to_string(), "@alice".to_string()];
        assert!(is_user_allowed("@user123", &list));
        assert!(!is_user_allowed("@mallory", &list));
    }

    #[test]
    fn test_is_user_allowed_empty_denies_all() {
        assert!(!is_user_allowed("@anyone", &[]));
    }

    #[test]
    fn test_parse_allowed_users_wildcard() {
        assert_eq!(parse_allowed_users("*"), vec!["*"]);
    }

    #[test]
    fn test_parse_allowed_users_list() {
        let result = parse_allowed_users("@user123, @alice, @bob");
        assert_eq!(result, vec!["@user123", "@alice", "@bob"]);
    }

    #[test]
    fn parse_tool_policy_from_env() {
        std::env::set_var("TANDEM_TELEGRAM_BOT_TOKEN", "test");

        std::env::set_var("TANDEM_CHANNEL_TOOL_POLICY", "allow_all");
        let config = ChannelsConfig::from_env().unwrap();
        assert!(matches!(config.tool_policy, ChannelToolPolicy::AllowAll));

        std::env::set_var("TANDEM_CHANNEL_TOOL_POLICY", "deny_all");
        let config = ChannelsConfig::from_env().unwrap();
        assert!(matches!(config.tool_policy, ChannelToolPolicy::DenyAll));

        std::env::set_var("TANDEM_CHANNEL_TOOL_POLICY", "unknown");
        let config = ChannelsConfig::from_env().unwrap();
        assert!(matches!(
            config.tool_policy,
            ChannelToolPolicy::RequireApproval
        ));

        std::env::remove_var("TANDEM_CHANNEL_TOOL_POLICY");
    }

    #[test]
    fn telegram_style_profile_from_env() {
        std::env::set_var("TANDEM_TELEGRAM_BOT_TOKEN", "test");
        std::env::set_var("TANDEM_TELEGRAM_STYLE_PROFILE", "friendly");
        let config = ChannelsConfig::from_env().expect("channels");
        assert_eq!(
            config.telegram.as_ref().map(|t| t.style_profile),
            Some(TelegramStyleProfile::Friendly)
        );
        std::env::remove_var("TANDEM_TELEGRAM_STYLE_PROFILE");
    }

    #[test]
    fn slack_mention_only_from_env() {
        std::env::set_var("TANDEM_SLACK_BOT_TOKEN", "test");
        std::env::set_var("TANDEM_SLACK_CHANNEL_ID", "C123");
        std::env::set_var("TANDEM_SLACK_MENTION_ONLY", "true");
        let config = ChannelsConfig::from_env().expect("channels");
        assert_eq!(config.slack.as_ref().map(|s| s.mention_only), Some(true));
        std::env::remove_var("TANDEM_SLACK_MENTION_ONLY");
    }

    #[test]
    fn security_profiles_parse_from_env() {
        std::env::set_var("TANDEM_TELEGRAM_BOT_TOKEN", "test");
        std::env::set_var("TANDEM_DISCORD_BOT_TOKEN", "test");
        std::env::set_var("TANDEM_SLACK_BOT_TOKEN", "test");
        std::env::set_var("TANDEM_SLACK_CHANNEL_ID", "C123");
        std::env::set_var("TANDEM_TELEGRAM_SECURITY_PROFILE", "public_demo");
        std::env::set_var("TANDEM_DISCORD_SECURITY_PROFILE", "trusted-team");
        std::env::set_var("TANDEM_SLACK_SECURITY_PROFILE", "unknown");

        let config = ChannelsConfig::from_env().expect("channels");
        assert_eq!(
            config.telegram.as_ref().map(|row| row.security_profile),
            Some(ChannelSecurityProfile::PublicDemo)
        );
        assert_eq!(
            config.discord.as_ref().map(|row| row.security_profile),
            Some(ChannelSecurityProfile::TrustedTeam)
        );
        assert_eq!(
            config.slack.as_ref().map(|row| row.security_profile),
            Some(ChannelSecurityProfile::Operator)
        );

        std::env::remove_var("TANDEM_TELEGRAM_SECURITY_PROFILE");
        std::env::remove_var("TANDEM_DISCORD_SECURITY_PROFILE");
        std::env::remove_var("TANDEM_SLACK_SECURITY_PROFILE");
    }
}
