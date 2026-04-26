use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::config::{ChannelSecurityProfile, ChannelsConfig};
use crate::discord::DiscordChannel;
use crate::slack::SlackChannel;
use crate::telegram::TelegramChannel;
use crate::traits::Channel;

/// Runtime diagnostics snapshot for a listener lifecycle.
#[derive(Debug, Clone)]
pub struct ChannelRuntimeDiagnostic {
    /// Human-readable lifecycle state.
    pub state: &'static str,
    /// Last observed listener error, if any.
    pub last_error: Option<String>,
    /// Last error code, if available (`listener_error`, `startup_error`, etc).
    pub last_error_code: Option<&'static str>,
    /// Timestamp (ms since epoch) when the listener last reconnected.
    pub last_reconnect_at: Option<u64>,
    /// Number of listener starts (including restarts).
    pub listener_start_count: u64,
}

impl Default for ChannelRuntimeDiagnostic {
    fn default() -> Self {
        Self {
            state: "stopped",
            last_error: None,
            last_error_code: None,
            last_reconnect_at: None,
            listener_start_count: 0,
        }
    }
}

pub type ChannelRuntimeDiagnostics = Arc<RwLock<HashMap<String, ChannelRuntimeDiagnostic>>>;

pub fn new_channel_runtime_diagnostics() -> ChannelRuntimeDiagnostics {
    Arc::new(RwLock::new(HashMap::new()))
}

/// Static metadata for a built-in channel adapter.
#[derive(Debug)]
pub struct ChannelSpec {
    /// Adapter name, e.g. "telegram".
    pub name: &'static str,
    /// Config key under `channels` in the server payload.
    pub config_key: &'static str,
    /// Environment variable used for token fallback.
    pub token_env_key: &'static str,
    /// Optional secondary env key for channel IDs.
    pub channel_id_env_key: Option<&'static str>,
    /// Human label surfaced in diagnostics and status views.
    pub status_label: &'static str,
    /// Supported top-level slash commands for this channel.
    pub supported_commands: &'static [&'static str],
    /// Build a runtime channel instance from runtime config.
    pub constructor: fn(&ChannelsConfig) -> Option<Arc<dyn Channel>>,
    /// Read the configured security profile for this channel.
    pub security_profile: fn(&ChannelsConfig) -> Option<ChannelSecurityProfile>,
}

/// Command capability metadata shared by all built-in adapters in v1.
#[derive(Debug, Clone, Copy)]
pub struct ChannelCommandCapability {
    pub name: &'static str,
    pub args: &'static str,
    pub audience: &'static str,
    pub description: &'static str,
    pub enabled_for_operator: bool,
    pub enabled_for_trusted_team: bool,
    pub enabled_for_public_demo: bool,
    pub public_demo_reason: Option<&'static str>,
}

impl ChannelCommandCapability {
    pub const fn enabled_for(self, profile: ChannelSecurityProfile) -> bool {
        match profile {
            ChannelSecurityProfile::Operator => self.enabled_for_operator,
            ChannelSecurityProfile::TrustedTeam => self.enabled_for_trusted_team,
            ChannelSecurityProfile::PublicDemo => self.enabled_for_public_demo,
        }
    }
}

const PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON: &str =
    "Public demo channels do not expose operator controls, workspace access, MCP access, or runtime reconfiguration.";
const PUBLIC_DEMO_QUEUE_VISIBILITY_DISABLED_REASON: &str =
    "Public demo channels keep internal execution and approval queues hidden to avoid leaking runtime details.";

pub const BUILTIN_CHANNEL_COMMANDS: &[ChannelCommandCapability] = &[
    ChannelCommandCapability {
        name: "new",
        args: "[name]",
        audience: "session",
        description: "start a fresh session",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: true,
        public_demo_reason: None,
    },
    ChannelCommandCapability {
        name: "sessions",
        args: "",
        audience: "session",
        description: "list your recent sessions",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: true,
        public_demo_reason: None,
    },
    ChannelCommandCapability {
        name: "resume",
        args: "<id or name>",
        audience: "session",
        description: "switch to a previous session",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: true,
        public_demo_reason: None,
    },
    ChannelCommandCapability {
        name: "rename",
        args: "<name>",
        audience: "session",
        description: "rename the current session",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: true,
        public_demo_reason: None,
    },
    ChannelCommandCapability {
        name: "status",
        args: "",
        audience: "session",
        description: "show current session info",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: true,
        public_demo_reason: None,
    },
    ChannelCommandCapability {
        name: "run",
        args: "",
        audience: "session",
        description: "show active run state",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: true,
        public_demo_reason: None,
    },
    ChannelCommandCapability {
        name: "cancel",
        args: "",
        audience: "session",
        description: "cancel the active run",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: true,
        public_demo_reason: None,
    },
    ChannelCommandCapability {
        name: "todos",
        args: "",
        audience: "operator",
        description: "list current session todos",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_QUEUE_VISIBILITY_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "requests",
        args: "",
        audience: "operator",
        description: "list pending tool/question requests",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_QUEUE_VISIBILITY_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "answer",
        args: "<question_id> <text>",
        audience: "approval",
        description: "answer a pending question",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "approve",
        args: "<tool_call_id>",
        audience: "approval",
        description: "approve a pending tool call",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "deny",
        args: "<tool_call_id>",
        audience: "approval",
        description: "deny a pending tool call",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "pending",
        args: "",
        audience: "approval",
        description: "list outstanding workflow approval gates",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "rework",
        args: "<run_id> <feedback>",
        audience: "approval",
        description: "send a paused workflow gate back for rework with feedback",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "providers",
        args: "",
        audience: "model",
        description: "list available providers",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "models",
        args: "[provider]",
        audience: "model",
        description: "list models",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "model",
        args: "<model_id>",
        audience: "model",
        description: "set model for current provider",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "help",
        args: "[topic]",
        audience: "meta",
        description: "show command help",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: true,
        public_demo_reason: None,
    },
    ChannelCommandCapability {
        name: "schedule",
        args: "help | plan <prompt> | show <plan_id> | edit <plan_id> <text> | reset <plan_id> | apply <plan_id>",
        audience: "automation",
        description: "plan and scheduling workflow commands",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "automations",
        args: "[help|list|show <id>|runs <id> [limit]|run <id>|pause <id>|resume <id>|delete <id> [--yes]]",
        audience: "automation",
        description: "automation control commands",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "runs",
        args: "[help|automations [limit]|show <run_id>|pause <run_id>|resume <run_id>|cancel <run_id>|artifacts <run_id>]",
        audience: "automation",
        description: "automation run commands",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "memory",
        args: "[help|search <query>|recent [limit]|save <text>|scopes|delete <id> [--yes]]",
        audience: "operator",
        description: "memory-related commands",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: true,
        public_demo_reason: None,
    },
    ChannelCommandCapability {
        name: "workspace",
        args: "[help|show|status|branch|files <query>]",
        audience: "operator",
        description: "workspace introspection commands",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "tools",
        args: "[help|list|enable|disable|reset]",
        audience: "operator",
        description: "tool allowlist management",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "mcp",
        args: "[help|list|tools|resources|status|connect|disconnect|enable|disable|refresh]",
        audience: "operator",
        description: "MCP server control commands",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "packs",
        args: "[help|list|show <pack>|updates <pack>|install <target>|uninstall <pack> [--yes]]",
        audience: "operator",
        description: "pack management commands",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
    ChannelCommandCapability {
        name: "config",
        args: "[help|show|providers|channels|model <model_id>]",
        audience: "operator",
        description: "runtime config inspection and tweaks",
        enabled_for_operator: true,
        enabled_for_trusted_team: true,
        enabled_for_public_demo: false,
        public_demo_reason: Some(PUBLIC_DEMO_TOOL_CMD_DISABLED_REASON),
    },
];

pub fn slash_command_capabilities() -> &'static [ChannelCommandCapability] {
    BUILTIN_CHANNEL_COMMANDS
}

pub fn command_capability(name: &str) -> Option<&'static ChannelCommandCapability> {
    let requested = name.trim().to_ascii_lowercase();
    BUILTIN_CHANNEL_COMMANDS
        .iter()
        .find(|cmd| cmd.name == requested)
}

pub fn command_capabilities_for_profile(
    profile: ChannelSecurityProfile,
) -> Vec<&'static ChannelCommandCapability> {
    BUILTIN_CHANNEL_COMMANDS
        .iter()
        .filter(|cmd| cmd.enabled_for(profile))
        .collect()
}

pub fn command_capabilities_for_channel(
    channel_name: &str,
    profile: ChannelSecurityProfile,
) -> Vec<&'static ChannelCommandCapability> {
    if find_channel(channel_name).is_none() {
        return Vec::new();
    }
    command_capabilities_for_profile(profile)
}

/// Supported top-level command names for all built-in adapters.
pub const DEFAULT_CHANNEL_COMMANDS: &[&str] = &[
    "new",
    "sessions",
    "resume",
    "rename",
    "status",
    "run",
    "cancel",
    "todos",
    "requests",
    "answer",
    "approve",
    "deny",
    "providers",
    "models",
    "model",
    "help",
    "schedule",
    "automations",
    "runs",
    "memory",
    "workspace",
    "tools",
    "mcp",
    "packs",
    "config",
];

pub fn registered_channels() -> &'static [ChannelSpec] {
    static SPECS: &[ChannelSpec] = &[
        ChannelSpec {
            name: "telegram",
            config_key: "telegram",
            token_env_key: "TANDEM_TELEGRAM_BOT_TOKEN",
            channel_id_env_key: None,
            status_label: "Telegram",
            supported_commands: DEFAULT_CHANNEL_COMMANDS,
            constructor: |config: &ChannelsConfig| {
                config
                    .telegram
                    .clone()
                    .map(|cfg| Arc::new(TelegramChannel::new(cfg)) as Arc<dyn Channel>)
            },
            security_profile: |config| config.telegram.as_ref().map(|cfg| cfg.security_profile),
        },
        ChannelSpec {
            name: "discord",
            config_key: "discord",
            token_env_key: "TANDEM_DISCORD_BOT_TOKEN",
            channel_id_env_key: None,
            status_label: "Discord",
            supported_commands: DEFAULT_CHANNEL_COMMANDS,
            constructor: |config: &ChannelsConfig| {
                config
                    .discord
                    .clone()
                    .map(|cfg| Arc::new(DiscordChannel::new(cfg)) as Arc<dyn Channel>)
            },
            security_profile: |config| config.discord.as_ref().map(|cfg| cfg.security_profile),
        },
        ChannelSpec {
            name: "slack",
            config_key: "slack",
            token_env_key: "TANDEM_SLACK_BOT_TOKEN",
            channel_id_env_key: Some("TANDEM_SLACK_CHANNEL_ID"),
            status_label: "Slack",
            supported_commands: DEFAULT_CHANNEL_COMMANDS,
            constructor: |config: &ChannelsConfig| {
                config
                    .slack
                    .clone()
                    .map(|cfg| Arc::new(SlackChannel::new(cfg)) as Arc<dyn Channel>)
            },
            security_profile: |config| config.slack.as_ref().map(|cfg| cfg.security_profile),
        },
    ];

    SPECS
}

pub fn find_channel(name: &str) -> Option<&'static ChannelSpec> {
    let normalized = name.trim().to_ascii_lowercase();
    registered_channels()
        .iter()
        .find(|spec| spec.name == normalized)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn registered_channels_have_unique_names_and_config_keys() {
        let mut names = HashSet::new();
        let mut keys = HashSet::new();
        for spec in registered_channels() {
            assert!(
                !spec.name.trim().is_empty(),
                "channel name must not be empty"
            );
            assert!(
                !spec.config_key.trim().is_empty(),
                "channel config key must not be empty"
            );
            assert!(
                names.insert(spec.name),
                "duplicate channel registration for {}",
                spec.name
            );
            assert!(
                keys.insert(spec.config_key),
                "duplicate channel config key registration for {}",
                spec.config_key
            );
        }
    }

    #[test]
    fn command_capability_lookup_is_case_insensitive() {
        let help = command_capability("HELP").expect("expected help capability");
        assert_eq!(help.name, "help");
        assert!(command_capability("unknown").is_none());
    }

    #[test]
    fn channel_scoped_command_lookup_returns_empty_for_unknown_channel() {
        assert!(
            command_capabilities_for_channel("missing", ChannelSecurityProfile::Operator)
                .is_empty()
        );
    }
}
