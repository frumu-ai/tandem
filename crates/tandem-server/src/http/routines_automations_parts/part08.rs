// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Channel approval-card decision updates: after a gate decision, edit every
// fanned-out card (one per channel connection, TAN-763) in place and post the
// decision as a thread reply. Split from part04 to keep it inside the file
// size envelope.

fn spawn_channel_approval_decision_update(
    state: AppState,
    request: tandem_types::ApprovalRequest,
    decision: String,
    reason: Option<String>,
) {
    tokio::spawn(async move {
        if let Err(error) = update_channel_approval_decision(state, request, decision, reason).await
        {
            tracing::warn!(
                target: "tandem_server::approval_outbound",
                %error,
                "failed to update channel approval card after gate decision"
            );
        }
    });
}

async fn update_channel_approval_decision(
    state: AppState,
    request: tandem_types::ApprovalRequest,
    decision: String,
    reason: Option<String>,
) -> anyhow::Result<()> {
    let message_map = crate::app::state::approval_message_map::ApprovalMessageMap::load_or_default(
        crate::config::paths::resolve_approval_message_map_path(),
    )
    .await;
    // Approval fan-out can post the same request as one card per channel
    // connection (TAN-763); a decision must edit EVERY card, or the ones not
    // updated stay stale and actionable. Per-card failures are logged and do
    // not stop the remaining cards; the first error is returned at the end.
    let records = message_map.get_deliveries(&request.request_id).await;
    if records.is_empty() {
        return Ok(());
    }

    let decided_by_display = format!("{} by Tandem operator", decision_label(&decision));
    let decision_summary = match reason.as_deref().filter(|value| !value.trim().is_empty()) {
        Some(reason) => format!(
            "*{}.*\nReason: {}",
            decision_label(&decision),
            reason.trim()
        ),
        None => format!("*{}.*", decision_label(&decision)),
    };
    let effective = state.config.get_effective_value().await;
    let mut first_error: Option<anyhow::Error> = None;
    for record in records {
        let result = update_single_channel_approval_card(
            &record,
            &request,
            &decision,
            reason.as_deref(),
            &effective,
            &decided_by_display,
            &decision_summary,
        )
        .await;
        if let Err(error) = result {
            tracing::warn!(
                target: "tandem_server::approval_outbound",
                channel = %record.channel,
                recipient = %record.recipient,
                %error,
                "failed to update one fanned-out approval card after gate decision"
            );
            if first_error.is_none() {
                first_error = Some(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[allow(clippy::too_many_arguments)]
async fn update_single_channel_approval_card(
    record: &crate::app::state::approval_message_map::ApprovalMessageRecord,
    request: &tandem_types::ApprovalRequest,
    decision: &str,
    reason: Option<&str>,
    effective: &serde_json::Value,
    decided_by_display: &str,
    decision_summary: &str,
) -> anyhow::Result<()> {
    let card = crate::app::notifiers::approval_request_to_card(request, record.recipient.clone());
    match record.channel.as_str() {
        "slack" => {
            let Some(slack_value) = effective.pointer("/channels/slack").cloned() else {
                return Ok(());
            };
            // Route by the recorded recipient AND the installation that
            // posted the card (channel-id strings can collide across
            // installations — editing app B's message with app A's token
            // fails or edits the wrong card). Legacy records without an
            // installation fall back to recipient-only matching, and an
            // unknown recipient to the default (first) resolved connection,
            // preserving the legacy single-channel behavior.
            let connections = crate::config::channels::resolve_slack_connections(&slack_value);
            let connection = connections
                .iter()
                .find(|connection| {
                    connection.channel_id == record.recipient
                        && record
                            .team_id
                            .as_deref()
                            .is_none_or(|team| connection.team_id.as_deref() == Some(team))
                        && record
                            .app_id
                            .as_deref()
                            .is_none_or(|app| connection.app_id.as_deref() == Some(app))
                })
                .or_else(|| {
                    connections
                        .iter()
                        .find(|connection| connection.channel_id == record.recipient)
                })
                .or_else(|| connections.first());
            let Some(bot_token) = connection.and_then(|connection| connection.bot_token.clone())
            else {
                return Ok(());
            };
            let connection = connection.expect("connection is Some when bot_token is Some");

            let slack_config = tandem_channels::config::SlackConfig {
                bot_token,
                channel_id: record.recipient.clone(),
                allowed_users: crate::config::channels::normalize_allowed_users_or_wildcard(
                    connection.allowed_users.clone(),
                ),
                mention_only: connection.mention_only,
                security_profile: connection.security_profile,
            };
            let channel = match connection.api_base_url.clone() {
                Some(api_base_url) => tandem_channels::slack::SlackChannel::new_with_api_base_url(
                    slack_config,
                    api_base_url,
                ),
                None => tandem_channels::slack::SlackChannel::new(slack_config),
            };
            channel
                .update_card_for_decision(
                    &card,
                    &record.message_id,
                    decided_by_display,
                    decision_summary,
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            send_approval_thread_reply(&channel, record, request, decision, reason).await?;
        }
        "discord" => {
            let Some(discord_value) = effective.pointer("/channels/discord").cloned() else {
                return Ok(());
            };
            let cfg: crate::DiscordConfigFile = serde_json::from_value(discord_value)?;
            if cfg.bot_token.trim().is_empty() {
                return Ok(());
            }

            let discord_config = tandem_channels::config::DiscordConfig {
                bot_token: cfg.bot_token,
                guild_id: cfg.guild_id,
                allowed_users: crate::config::channels::normalize_allowed_users_or_wildcard(
                    cfg.allowed_users,
                ),
                mention_only: cfg.mention_only,
                security_profile: cfg.security_profile,
            };
            let channel = tandem_channels::discord::DiscordChannel::new(discord_config);
            channel
                .update_card_for_decision(
                    &card,
                    &record.message_id,
                    discord_decision_outcome(decision),
                    decided_by_display,
                    decision_summary,
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            send_approval_thread_reply(&channel, record, request, decision, reason).await?;
        }
        "telegram" => {
            let Some(telegram_value) = effective.pointer("/channels/telegram").cloned() else {
                return Ok(());
            };
            let cfg: crate::TelegramConfigFile = serde_json::from_value(telegram_value)?;
            if cfg.bot_token.trim().is_empty() {
                return Ok(());
            }

            let telegram_config = tandem_channels::config::TelegramConfig {
                bot_token: cfg.bot_token,
                allowed_users: crate::config::channels::normalize_allowed_users_or_wildcard(
                    cfg.allowed_users,
                ),
                mention_only: cfg.mention_only,
                style_profile: cfg.style_profile,
                security_profile: cfg.security_profile,
            };
            let channel = tandem_channels::telegram::TelegramChannel::new(telegram_config);
            channel
                .update_card_for_decision(
                    &card,
                    &record.message_id,
                    decided_by_display,
                    decision_summary,
                )
                .await
                .map_err(|error| anyhow::anyhow!(error.to_string()))?;
            send_approval_thread_reply(&channel, record, request, decision, reason).await?;
        }
        _ => {}
    }
    Ok(())
}

async fn send_approval_thread_reply(
    channel: &dyn tandem_channels::traits::Channel,
    record: &crate::app::state::approval_message_map::ApprovalMessageRecord,
    request: &tandem_types::ApprovalRequest,
    decision: &str,
    reason: Option<&str>,
) -> anyhow::Result<()> {
    let thread_id = record
        .thread_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(record.message_id.as_str())
        .to_string();
    let node = request
        .node_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("approval gate");
    let mut content = format!(
        "{} `{}` for run `{}`.",
        decision_label(decision),
        node,
        request.run_id
    );
    if let Some(reason) = reason.map(str::trim).filter(|value| !value.is_empty()) {
        content.push_str(&format!("\nReason: {reason}"));
    }
    channel
        .send_thread_reply(&tandem_channels::traits::ThreadReply {
            content,
            recipient: record.recipient.clone(),
            thread_id,
        })
        .await
        .map_err(|error| anyhow::anyhow!(error.to_string()))
}

fn discord_decision_outcome(decision: &str) -> tandem_channels::discord_blocks::DecisionOutcome {
    match decision {
        "approve" => tandem_channels::discord_blocks::DecisionOutcome::Approved,
        "rework" => tandem_channels::discord_blocks::DecisionOutcome::Reworked,
        "cancel" => tandem_channels::discord_blocks::DecisionOutcome::Cancelled,
        _ => tandem_channels::discord_blocks::DecisionOutcome::Cancelled,
    }
}

fn decision_label(decision: &str) -> &'static str {
    match decision {
        "approve" => "Approved",
        "rework" => "Sent back for rework",
        "cancel" => "Cancelled",
        _ => "Decided",
    }
}
