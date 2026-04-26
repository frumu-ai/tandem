//! External messaging channel integrations for Tandem.
//!
//! This crate provides adapters for Telegram, Discord, and Slack that route
//! incoming messages to Tandem sessions and deliver responses back to the sender.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use tandem_channels::{config::ChannelsConfig, start_channel_listeners};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let config = ChannelsConfig::from_env()?;
//!     let mut listeners = start_channel_listeners(config).await;
//!     listeners.join_all().await;
//!     Ok(())
//! }
//! ```

pub mod channel_registry;
pub mod config;
pub mod discord;
pub mod discord_blocks;
pub mod dispatcher;
pub mod signing;
pub mod slack;
pub mod slack_blocks;
pub mod telegram;
pub mod telegram_keyboards;
pub mod traits;

pub use channel_registry::{
    command_capabilities_for_channel, command_capabilities_for_profile, command_capability,
    find_channel, new_channel_runtime_diagnostics, registered_channels, slash_command_capabilities,
    ChannelCommandCapability, ChannelRuntimeDiagnostic, ChannelRuntimeDiagnostics, ChannelSpec,
};
pub use dispatcher::{start_channel_listeners, start_channel_listeners_with_diagnostics};
