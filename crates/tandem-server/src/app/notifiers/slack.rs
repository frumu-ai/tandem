// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::sync::Arc;

use tandem_channels::{config::SlackConfig, slack::SlackChannel, traits::Channel};

use crate::app::state::approval_message_map::ApprovalMessageMap;

use super::ChannelApprovalNotifier;

pub type SlackApprovalNotifier = ChannelApprovalNotifier;

pub fn from_config(config: SlackConfig) -> SlackApprovalNotifier {
    let recipient = config.channel_id.clone();
    let channel: Arc<dyn Channel> = Arc::new(SlackChannel::new(config));
    ChannelApprovalNotifier::new("slack", recipient, channel)
}

pub fn from_config_with_message_map(
    config: SlackConfig,
    message_map: Arc<ApprovalMessageMap>,
) -> SlackApprovalNotifier {
    from_config_with_message_map_and_tenant(config, message_map, None)
}

/// TAN-764: a tenant-bound Slack connection only receives its own tenant's
/// approval cards; `tenant` is the connection's `bound_tenant()`.
pub fn from_config_with_message_map_and_tenant(
    config: SlackConfig,
    message_map: Arc<ApprovalMessageMap>,
    tenant: Option<(String, String)>,
) -> SlackApprovalNotifier {
    let recipient = config.channel_id.clone();
    let channel: Arc<dyn Channel> = Arc::new(SlackChannel::new(config));
    ChannelApprovalNotifier::new_with_message_map("slack", recipient, channel, Some(message_map))
        .with_tenant_filter(tenant)
}
