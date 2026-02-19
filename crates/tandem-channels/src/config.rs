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
}

#[derive(Debug, Clone)]
pub struct TelegramConfig {
    pub bot_token: String,
    /// `["*"]` = allow everyone. Otherwise a list of usernames or user IDs.
    pub allowed_users: Vec<String>,
    /// Only respond when the bot is @-mentioned (useful in group chats).
    pub mention_only: bool,
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
}

#[derive(Debug, Clone)]
pub struct SlackConfig {
    pub bot_token: String,
    /// Slack channel ID to poll (e.g. `C0XXXXXXXX`).
    pub channel_id: String,
    /// `["*"]` = allow everyone.
    pub allowed_users: Vec<String>,
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
        Some(TelegramConfig {
            bot_token,
            allowed_users,
            mention_only,
        })
    }

    fn discord_from_env() -> Option<DiscordConfig> {
        let bot_token = std::env::var("TANDEM_DISCORD_BOT_TOKEN").ok()?;
        if bot_token.trim().is_empty() {
            return None;
        }
        let guild_id = std::env::var("TANDEM_DISCORD_GUILD_ID").ok();
        let allowed_users = std::env::var("TANDEM_DISCORD_ALLOWED_USERS")
            .map(|s| parse_allowed_users(&s))
            .unwrap_or_else(|_| vec!["*".to_string()]);
        let mention_only = std::env::var("TANDEM_DISCORD_MENTION_ONLY")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(true); // default true for Discord — avoids bots fighting each other
        Some(DiscordConfig {
            bot_token,
            guild_id,
            allowed_users,
            mention_only,
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
        Some(SlackConfig {
            bot_token,
            channel_id,
            allowed_users,
        })
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
        let list = vec!["@evan".to_string(), "@alice".to_string()];
        assert!(is_user_allowed("@evan", &list));
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
        let result = parse_allowed_users("@evan, @alice, @bob");
        assert_eq!(result, vec!["@evan", "@alice", "@bob"]);
    }
}
