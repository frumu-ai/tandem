// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Slack installation binding: signing-secret selection for the claimed
//! `(team_id, api_app_id)` and validation that a payload's installation and
//! channel match a configured connection. Split from `slack_interactions.rs`
//! so the endpoint module stays within the repository file-size policy.

use serde_json::Value;
use tandem_channels::signing::verify_slack_signature;

use super::{config_string, SlackInstallationBinding};
use crate::config::channels::ResolvedSlackConnection;

/// Distinct signing secrets across every configured Slack connection. An
/// empty result means "interactions/events are not enabled," never a silent
/// allow. Typically one secret (per Slack app), but connections spanning
/// installations may carry different ones.
pub(super) fn slack_signing_secrets(connections: &[ResolvedSlackConnection]) -> Vec<String> {
    let mut secrets = connections
        .iter()
        .filter_map(|connection| connection.signing_secret.clone())
        .collect::<Vec<_>>();
    secrets.sort();
    secrets.dedup();
    secrets
}

/// Signing secrets bound to one claimed installation `(team_id, api_app_id)`.
///
/// The signing secret is a Slack *app* credential, so a payload claiming
/// installation B must verify against B's secret — never against another
/// configured app's secret (that would break tenant/app isolation the moment
/// one installation's secret is compromised). Returns an empty list when the
/// claimed installation matches no configured connection or the matching
/// connections carry no secret; callers then fall back to
/// [`slack_signing_secrets`] so signed-but-unknown installations still reach
/// the installation-mismatch rejection instead of a misleading signature
/// error.
pub(super) fn slack_installation_signing_secrets(
    connections: &[ResolvedSlackConnection],
    team_id: Option<&str>,
    app_id: Option<&str>,
) -> Vec<String> {
    let (Some(team_id), Some(app_id)) = (team_id, app_id) else {
        return Vec::new();
    };
    let mut secrets = connections
        .iter()
        .filter(|connection| {
            connection.team_id.as_deref() == Some(team_id)
                && connection.app_id.as_deref() == Some(app_id)
        })
        .filter_map(|connection| connection.signing_secret.clone())
        .collect::<Vec<_>>();
    secrets.sort();
    secrets.dedup();
    secrets
}

/// Verify a Slack HMAC signature against every configured secret; success on
/// the first match. Each candidate check is constant-time internally.
pub(super) fn verify_slack_signature_any(
    body: &[u8],
    signature: Option<&str>,
    timestamp: Option<&str>,
    secrets: &[String],
    now: i64,
) -> Result<(), String> {
    let mut last_error = "slack signing secret not configured".to_string();
    for secret in secrets {
        match verify_slack_signature(body, signature, timestamp, secret, now) {
            Ok(()) => return Ok(()),
            Err(error) => last_error = error.to_string(),
        }
    }
    Err(last_error)
}

pub(super) fn validate_slack_event_installation(
    connections: &[ResolvedSlackConnection],
    payload: &Value,
) -> Result<SlackInstallationBinding, String> {
    if !connections
        .iter()
        .any(|connection| connection.team_id.is_some())
    {
        return Err("slack events require a configured team_id".to_string());
    }
    if !connections
        .iter()
        .any(|connection| connection.app_id.is_some())
    {
        return Err("slack events require a configured app_id".to_string());
    }
    let envelope_team_id = slack_event_envelope_team_id(payload)?;
    let envelope_app_id = config_string(payload, "/api_app_id")
        .ok_or_else(|| "Slack event envelope missing api_app_id".to_string())?;

    if !connections
        .iter()
        .any(|connection| connection.team_id.as_deref() == Some(envelope_team_id.as_str()))
    {
        return Err("Slack event team_id does not match configured workspace".to_string());
    }
    if !connections.iter().any(|connection| {
        connection.team_id.as_deref() == Some(envelope_team_id.as_str())
            && connection.app_id.as_deref() == Some(envelope_app_id.as_str())
    }) {
        return Err("Slack event api_app_id does not match configured app".to_string());
    }

    Ok(SlackInstallationBinding {
        team_id: envelope_team_id,
        app_id: envelope_app_id,
    })
}

pub(super) fn validate_slack_interaction_installation(
    connections: &[ResolvedSlackConnection],
    payload: &Value,
) -> Result<(SlackInstallationBinding, ResolvedSlackConnection), String> {
    if !connections
        .iter()
        .any(|connection| connection.team_id.is_some())
    {
        return Err("slack interactions require a configured team_id".to_string());
    }
    if !connections
        .iter()
        .any(|connection| connection.app_id.is_some())
    {
        return Err("slack interactions require a configured app_id".to_string());
    }
    if !connections
        .iter()
        .any(|connection| !connection.channel_id.is_empty())
    {
        return Err("slack interactions require a configured channel_id".to_string());
    }
    let payload_team_id = config_string(payload, "/team/id")
        .or_else(|| config_string(payload, "/team_id"))
        .ok_or_else(|| "Slack interaction payload missing team id".to_string())?;
    let payload_app_id = config_string(payload, "/api_app_id")
        .ok_or_else(|| "Slack interaction payload missing api_app_id".to_string())?;
    let mut channel_ids = [
        config_string(payload, "/channel/id"),
        config_string(payload, "/container/channel_id"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    channel_ids.sort();
    channel_ids.dedup();
    let payload_channel_id = match channel_ids.as_slice() {
        [channel_id] => channel_id.clone(),
        [] => return Err("Slack interaction payload missing channel id".to_string()),
        _ => return Err("Slack interaction payload has conflicting channel ids".to_string()),
    };

    if !connections
        .iter()
        .any(|connection| connection.team_id.as_deref() == Some(payload_team_id.as_str()))
    {
        return Err("Slack interaction team does not match configured workspace".to_string());
    }
    let installation_connections = connections
        .iter()
        .filter(|connection| {
            connection.team_id.as_deref() == Some(payload_team_id.as_str())
                && connection.app_id.as_deref() == Some(payload_app_id.as_str())
        })
        .collect::<Vec<_>>();
    if installation_connections.is_empty() {
        return Err("Slack interaction app does not match configured app".to_string());
    }
    let Some(connection) = installation_connections
        .into_iter()
        .find(|connection| connection.channel_id == payload_channel_id)
    else {
        return Err("Slack interaction channel does not match configured channel".to_string());
    };
    Ok((
        SlackInstallationBinding {
            team_id: payload_team_id,
            app_id: payload_app_id,
        },
        connection.clone(),
    ))
}

pub(super) fn slack_event_envelope_team_id(payload: &Value) -> Result<String, String> {
    if let Some(team_id) = config_string(payload, "/team_id") {
        return Ok(team_id);
    }
    if let Some(team_id) = config_string(payload, "/context_team_id") {
        return Ok(team_id);
    }

    let mut authorization_team_ids = payload
        .get("authorizations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|authorization| {
            authorization
                .get("team_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    authorization_team_ids.sort();
    authorization_team_ids.dedup();
    match authorization_team_ids.as_slice() {
        [team_id] => Ok(team_id.clone()),
        [] => Err("Slack event envelope missing team_id".to_string()),
        _ => Err("Slack event envelope has ambiguous team authorization".to_string()),
    }
}
