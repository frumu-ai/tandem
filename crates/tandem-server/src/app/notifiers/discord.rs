use std::sync::Arc;

use tandem_channels::{config::DiscordConfig, discord::DiscordChannel, traits::Channel};

use crate::app::state::approval_message_map::ApprovalMessageMap;

use super::ChannelApprovalNotifier;

pub type DiscordApprovalNotifier = ChannelApprovalNotifier;

pub fn from_config(config: DiscordConfig, recipient: impl Into<String>) -> DiscordApprovalNotifier {
    let channel: Arc<dyn Channel> = Arc::new(DiscordChannel::new(config));
    ChannelApprovalNotifier::new("discord", recipient, channel)
}

pub fn from_config_with_message_map(
    config: DiscordConfig,
    recipient: impl Into<String>,
    message_map: Arc<ApprovalMessageMap>,
) -> DiscordApprovalNotifier {
    let channel: Arc<dyn Channel> = Arc::new(DiscordChannel::new(config));
    ChannelApprovalNotifier::new_with_message_map("discord", recipient, channel, Some(message_map))
}
