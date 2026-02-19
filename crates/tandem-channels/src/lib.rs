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

pub mod config;
pub mod discord;
pub mod dispatcher;
pub mod slack;
pub mod telegram;
pub mod traits;

pub use dispatcher::start_channel_listeners;
