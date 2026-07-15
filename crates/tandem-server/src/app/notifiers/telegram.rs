// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::sync::Arc;

use tandem_channels::{config::TelegramConfig, telegram::TelegramChannel, traits::Channel};

use crate::app::state::approval_message_map::ApprovalMessageMap;

use super::ChannelApprovalNotifier;

pub type TelegramApprovalNotifier = ChannelApprovalNotifier;

pub fn from_config(
    config: TelegramConfig,
    recipient: impl Into<String>,
) -> TelegramApprovalNotifier {
    let channel: Arc<dyn Channel> = Arc::new(TelegramChannel::new(config));
    ChannelApprovalNotifier::new("telegram", recipient, channel)
}

pub fn from_config_with_message_map(
    config: TelegramConfig,
    recipient: impl Into<String>,
    message_map: Arc<ApprovalMessageMap>,
) -> TelegramApprovalNotifier {
    let channel: Arc<dyn Channel> = Arc::new(TelegramChannel::new(config));
    ChannelApprovalNotifier::new_with_message_map("telegram", recipient, channel, Some(message_map))
}
