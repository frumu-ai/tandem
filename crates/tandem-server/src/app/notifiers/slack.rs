// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::sync::Arc;

use serde_json::Value;
use tandem_channels::{config::SlackConfig, slack::SlackChannel, traits::Channel};

use crate::app::approval_outbound::NotifierError;
use crate::app::state::approval_message_map::ApprovalMessageMap;
use crate::config::channels::ResolvedSlackConnection;

use super::{BindingCheck, ChannelApprovalNotifier};

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
    let recipient = config.channel_id.clone();
    let channel: Arc<dyn Channel> = Arc::new(SlackChannel::new(config));
    ChannelApprovalNotifier::new_with_message_map("slack", recipient, channel, Some(message_map))
}

/// Build the approval notifier for one resolved Slack connection (TAN-763/4):
/// posts through the connection's own token/API base, delivers only its bound
/// tenant's approvals, records the posting installation for decision-update
/// routing, and — when the connection declares an installation — verifies the
/// bot token actually belongs to it before the first card is posted, so a
/// token copied from another workspace can never leak approval cards there.
/// Returns `None` when the connection cannot notify (no token, no channel, or
/// approvals opted out).
pub fn from_resolved_connection(
    connection: &ResolvedSlackConnection,
    message_map: Arc<ApprovalMessageMap>,
) -> Option<SlackApprovalNotifier> {
    let bot_token = connection.bot_token.clone()?;
    if connection.channel_id.is_empty() || !connection.notify_approvals {
        return None;
    }
    let slack_config = SlackConfig {
        bot_token: bot_token.clone(),
        channel_id: connection.channel_id.clone(),
        allowed_users: crate::config::channels::normalize_allowed_users_or_wildcard(
            connection.allowed_users.clone(),
        ),
        mention_only: connection.mention_only,
        security_profile: connection.security_profile,
    };
    let recipient = connection.channel_id.clone();
    let api_base_url = connection
        .api_base_url
        .clone()
        .unwrap_or_else(|| "https://slack.com/api".to_string());
    let channel: Arc<dyn Channel> = Arc::new(SlackChannel::new_with_api_base_url(
        slack_config,
        api_base_url.clone(),
    ));
    let installation = connection.team_id.clone().zip(connection.app_id.clone());
    let binding_check = (connection.team_id.is_some() || connection.app_id.is_some()).then(|| {
        slack_binding_check(
            api_base_url,
            bot_token,
            connection.team_id.clone(),
            connection.app_id.clone(),
        )
    });
    Some(
        ChannelApprovalNotifier::new_with_message_map(
            "slack",
            recipient,
            channel,
            Some(message_map),
        )
        .with_tenant_filter(connection.bound_tenant())
        .with_installation(installation)
        .with_binding_check(binding_check),
    )
}

/// Prove the bot token belongs to the configured installation before any
/// card posts through it — the same `auth.test` team + `bots.info` app
/// checks the governed reply path and the verify endpoint use. A proven
/// mismatch is permanent (never post); an unreachable Slack API is transient
/// (re-checked on the next delivery).
fn slack_binding_check(
    api_base_url: String,
    bot_token: String,
    team_id: Option<String>,
    app_id: Option<String>,
) -> BindingCheck {
    Arc::new(move || {
        let api_base_url = api_base_url.trim_end_matches('/').to_string();
        let bot_token = bot_token.clone();
        let team_id = team_id.clone();
        let app_id = app_id.clone();
        Box::pin(async move {
            let client = reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .map_err(|error| {
                    NotifierError::Transient(format!("HTTP client construction failed: {error}"))
                })?;
            let auth = client
                .get(format!("{api_base_url}/auth.test"))
                .bearer_auth(&bot_token)
                .send()
                .await
                .map_err(|error| {
                    NotifierError::Transient(format!("Slack auth.test request failed: {error}"))
                })?
                .json::<Value>()
                .await
                .map_err(|error| {
                    NotifierError::Transient(format!(
                        "Slack auth.test response was not JSON: {error}"
                    ))
                })?;
            if auth.get("ok") != Some(&Value::Bool(true)) {
                return Err(NotifierError::Permanent(format!(
                    "Slack auth.test rejected the approval bot token: {}",
                    auth.get("error")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                )));
            }
            if let Some(expected_team) = team_id.as_deref() {
                let actual = auth.get("team_id").and_then(Value::as_str);
                if actual != Some(expected_team) {
                    return Err(NotifierError::Permanent(format!(
                        "approval bot token belongs to team {}, expected {expected_team}; \
                         approval cards suppressed to avoid posting into another workspace",
                        actual.unwrap_or("unknown")
                    )));
                }
            }
            if let Some(expected_app) = app_id.as_deref() {
                let Some(bot_id) = auth.get("bot_id").and_then(Value::as_str) else {
                    return Err(NotifierError::Permanent(
                        "Slack auth.test token is not a bot identity".to_string(),
                    ));
                };
                let bots_info = client
                    .get(format!("{api_base_url}/bots.info"))
                    .bearer_auth(&bot_token)
                    .query(&[("bot", bot_id)])
                    .send()
                    .await
                    .map_err(|error| {
                        NotifierError::Transient(format!("Slack bots.info request failed: {error}"))
                    })?
                    .json::<Value>()
                    .await
                    .unwrap_or_default();
                let app_ok = bots_info.get("ok") == Some(&Value::Bool(true))
                    && bots_info.pointer("/bot/app_id").and_then(Value::as_str)
                        == Some(expected_app);
                if !app_ok {
                    return Err(NotifierError::Permanent(format!(
                        "approval bot token belongs to a different Slack app than {expected_app}; \
                         approval cards suppressed"
                    )));
                }
            }
            Ok(())
        })
    })
}
