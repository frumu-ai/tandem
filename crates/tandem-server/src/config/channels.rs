use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelegramConfigFile {
    pub bot_token: String,
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
    pub bot_token: String,
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
    pub bot_token: String,
    pub channel_id: String,
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
    /// Slack app signing secret. Required when the Slack interactions endpoint
    /// (`POST /channels/slack/interactions`) is enabled — every interaction
    /// payload from Slack is HMAC-SHA256 signed using this secret. Stored in
    /// the OS keystore in production; this field is the in-memory copy.
    #[serde(default)]
    pub signing_secret: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfigFile {
    pub telegram: Option<TelegramConfigFile>,
    pub discord: Option<DiscordConfigFile>,
    pub slack: Option<SlackConfigFile>,
    #[serde(default)]
    pub tool_policy: tandem_channels::config::ChannelToolPolicy,
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
