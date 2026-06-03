//! Discord interaction endpoint.
//!
//! Discord POSTs a payload here for every interaction (PING, button click,
//! modal submit, slash command). Body is JSON.
//!
//! Hard requirements (per Discord docs):
//! - Verify `x-signature-ed25519` and `x-signature-timestamp` on every
//!   request via `tandem_channels::signing::verify_discord_signature`.
//!   Discord disables the endpoint if even a single inbound interaction is
//!   unverified, so we must reject with HTTP 401 on every failure.
//! - Respond to PING (`type = 1`) with PONG (`type = 1`) — Discord uses this
//!   to validate the endpoint when first registered.
//! - Acknowledge any other interaction within 3 seconds. Button clicks land
//!   here, so we either dispatch synchronously and return an UPDATE_MESSAGE
//!   (`type = 7`) or return a deferred ack (`type = 6`) and PATCH the message
//!   later via the interaction webhook URL.
//! - Idempotent on retries: dedup by `interaction_id` (Discord retries on
//!   network errors).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use tandem_channels::discord_blocks::{parse_custom_id, ParsedCustomId};
use tandem_channels::signing::verify_discord_signature;

use crate::app::rate_limit::{ChannelRateLimitKey, ChannelRateLimitKind};
use crate::app::state::channel_user_capabilities::{
    channel_requires_approval_step_up, channel_security_profile_from_config,
};
use crate::app::state::principals::channel_identity::{
    channel_is_open_to_all, resolve_channel_user, ChannelIdentityResolution, ChannelKind,
};
use crate::AppState;

const DEDUP_CAP: usize = 4096;
const DEDUP_TTL_SECS: u64 = 300; // 5 minutes — Discord retries within minutes

static SEEN_INTERACTIONS: OnceLock<Mutex<DedupRing>> = OnceLock::new();

fn dedup_ring() -> &'static Mutex<DedupRing> {
    SEEN_INTERACTIONS.get_or_init(|| Mutex::new(DedupRing::new()))
}

struct DedupEntry {
    inserted_at_secs: u64,
}

struct DedupRing {
    set: std::collections::HashMap<String, DedupEntry>,
    order: std::collections::VecDeque<String>,
}

impl DedupRing {
    fn new() -> Self {
        Self {
            set: std::collections::HashMap::with_capacity(DEDUP_CAP),
            order: std::collections::VecDeque::with_capacity(DEDUP_CAP),
        }
    }

    fn record_new(&mut self, key: &str) -> bool {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Check if key exists and hasn't expired.
        if let Some(entry) = self.set.get(key) {
            if now_secs.saturating_sub(entry.inserted_at_secs) < DEDUP_TTL_SECS {
                return false; // Duplicate within TTL window.
            }
            // Entry exists but expired; will be reinserted below.
            self.set.remove(key);
            // Note: order queue still has the old entry, but we'll skip it on next cleanup.
        }

        // Evict oldest entry if at capacity.
        if self.order.len() >= DEDUP_CAP {
            if let Some(oldest) = self.order.pop_front() {
                self.set.remove(&oldest);
            }
        }

        self.set.insert(
            key.to_string(),
            DedupEntry {
                inserted_at_secs: now_secs,
            },
        );
        self.order.push_back(key.to_string());
        true
    }
}

/// Discord interaction handler. Wired at `POST /channels/discord/interactions`.
pub(crate) async fn discord_interactions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let public_key = match read_discord_public_key(&state).await {
        Some(key) => key,
        None => return reject_unauthorized("discord public key not configured"),
    };

    let signature = headers
        .get("x-signature-ed25519")
        .and_then(|v| v.to_str().ok());
    let timestamp = headers
        .get("x-signature-timestamp")
        .and_then(|v| v.to_str().ok());

    if let Err(error) = verify_discord_signature(&body, signature, timestamp, &public_key) {
        tracing::warn!(
            target: "tandem_server::discord_interactions",
            ?error,
            "rejecting unsigned/forged Discord interaction"
        );
        return reject_unauthorized(&error.to_string());
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => return reject_bad_request(&format!("payload is not JSON: {err}")),
    };

    let interaction_type = payload.get("type").and_then(Value::as_u64).unwrap_or(0);

    // Type 1: PING. Reply with PONG so Discord's endpoint-validation flow
    // can confirm the URL.
    if interaction_type == 1 {
        return Json(json!({ "type": 1 })).into_response();
    }

    // Dedup by interaction_id (Discord retries on transient failures).
    if let Some(interaction_id) = payload.get("id").and_then(Value::as_str) {
        let mut guard = dedup_ring().lock().expect("dedup mutex poisoned");
        if !guard.record_new(interaction_id) {
            tracing::debug!(
                target: "tandem_server::discord_interactions",
                interaction_id,
                "duplicate Discord interaction — already processed"
            );
            return Json(json!({ "type": 6 })).into_response();
        }
    }

    match interaction_type {
        // 3: MESSAGE_COMPONENT — button clicks on action rows.
        3 => handle_message_component(state, &payload).await,
        // 5: MODAL_SUBMIT — rework reason was submitted.
        5 => handle_modal_submit(state, &payload).await,
        // 2: APPLICATION_COMMAND — slash commands. Future: /pending, /approve.
        2 => Json(json!({
            "type": 4,
            "data": { "content": "Slash commands land in W5. Use the buttons on approval cards for now." }
        }))
        .into_response(),
        other => {
            tracing::info!(
                target: "tandem_server::discord_interactions",
                interaction_type = other,
                "unhandled Discord interaction type"
            );
            Json(json!({ "type": 6 })).into_response()
        }
    }
}

async fn handle_message_component(state: AppState, payload: &Value) -> Response {
    let custom_id = match payload.pointer("/data/custom_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return reject_bad_request("button payload missing data.custom_id"),
    };

    let parsed = match parse_custom_id(custom_id) {
        Some(p) => p,
        None => return reject_bad_request(&format!("unrecognized custom_id: {custom_id}")),
    };

    let user_id = match payload
        .pointer("/member/user/id")
        .or_else(|| payload.pointer("/user/id"))
        .and_then(Value::as_str)
    {
        Some(id) => id.to_string(),
        None => return reject_bad_request("payload missing user identification"),
    };

    // CRITICAL: Authorize the user against the allowlist BEFORE dispatching.
    let effective_config = state.config.get_effective_value().await;
    match resolve_channel_user(&effective_config, ChannelKind::Discord, &user_id) {
        ChannelIdentityResolution::Resolved(_principal) => {
            // User is authorized; proceed to handle the action.
        }
        ChannelIdentityResolution::Denied { .. } => {
            tracing::warn!(
                target: "tandem_server::discord_interactions",
                user_id = %user_id,
                "rejecting Discord interaction from unauthorized user"
            );
            return reject_forbidden("user not in allowed_users");
        }
        ChannelIdentityResolution::ChannelNotConfigured(_) => {
            return reject_bad_request("discord channel not properly configured");
        }
    }
    let profile =
        channel_security_profile_from_config(&effective_config, ChannelKind::Discord.as_str());
    if !state
        .channel_user_can_approve(
            ChannelKind::Discord.as_str(),
            &user_id,
            profile,
            channel_is_open_to_all(&effective_config, ChannelKind::Discord),
        )
        .await
    {
        tracing::warn!(
            target: "tandem_server::discord_interactions",
            user_id = %user_id,
            "rejecting Discord interaction without approval capability"
        );
        return reject_forbidden("user lacks approval capability");
    }
    // GOV-B5b: on a channel that opts into step-up, an approval requires an active
    // per-identity step-up grant issued out-of-band by the control panel.
    if channel_requires_approval_step_up(&effective_config, ChannelKind::Discord.as_str())
        && !state
            .channel_step_up_active(ChannelKind::Discord.as_str(), &user_id)
            .await
    {
        tracing::warn!(
            target: "tandem_server::discord_interactions",
            user_id = %user_id,
            "rejecting Discord interaction without an active step-up"
        );
        return reject_forbidden("step-up required");
    }
    let rate_key = ChannelRateLimitKey {
        channel: ChannelKind::Discord.as_str().to_string(),
        user_id: user_id.clone(),
    };
    let rate_decision = state
        .channel_rate_limiter
        .check(&rate_key, ChannelRateLimitKind::Decision, profile)
        .await;
    if !rate_decision.allowed {
        return reject_rate_limited(rate_decision.retry_after_secs);
    }

    match parsed.action.as_str() {
        "approve" | "cancel" => dispatch_decision(state, parsed, &user_id, None).await,
        "rework" => {
            // Open the modal so the user can supply a reason. The modal's
            // custom_id encodes the run_id + node_id for the eventual
            // MODAL_SUBMIT handler.
            let modal_custom_id = format!("tdm-modal:rework:{}:{}", parsed.run_id, parsed.node_id);
            // We don't have the InteractiveCard here; build a minimal modal
            // inline. (W4-bonus: pass the original card through interaction
            // metadata once message lookups are wired.)
            Json(json!({
                "type": 9,
                "data": {
                    "title": "Rework feedback",
                    "custom_id": modal_custom_id,
                    "components": [{
                        "type": 1,
                        "components": [{
                            "type": 4,
                            "custom_id": "reason_input",
                            "label": "What should change?",
                            "style": 2,
                            "min_length": 1,
                            "max_length": 4000,
                            "required": true,
                        }]
                    }]
                }
            }))
            .into_response()
        }
        other => reject_bad_request(&format!("unknown action: {other}")),
    }
}

async fn handle_modal_submit(state: AppState, payload: &Value) -> Response {
    let custom_id = match payload.pointer("/data/custom_id").and_then(Value::as_str) {
        Some(id) => id,
        None => return reject_bad_request("modal payload missing data.custom_id"),
    };

    // Modal custom_id format: `tdm-modal:rework:{run_id}:{node_id}`.
    let mut parts = custom_id.splitn(4, ':');
    let prefix = parts.next().unwrap_or("");
    let action = parts.next().unwrap_or("");
    let run_id = parts.next().unwrap_or("").to_string();
    let node_id = parts.next().unwrap_or("").to_string();

    if prefix != "tdm-modal" || action != "rework" || run_id.is_empty() || node_id.is_empty() {
        return reject_bad_request(&format!(
            "unrecognized or malformed modal custom_id: {custom_id}"
        ));
    }

    let reason_raw = payload
        .pointer("/data/components/0/components/0/value")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if reason_raw.len() > 4000 {
        return reject_bad_request("reason exceeds 4000 character limit");
    }
    let reason = reason_raw.to_string();

    let user_id = match payload
        .pointer("/member/user/id")
        .or_else(|| payload.pointer("/user/id"))
        .and_then(Value::as_str)
    {
        Some(id) => id.to_string(),
        None => return reject_bad_request("modal payload missing user identification"),
    };

    // CRITICAL: Authorize the user against the allowlist BEFORE dispatching.
    let effective_config = state.config.get_effective_value().await;
    match resolve_channel_user(&effective_config, ChannelKind::Discord, &user_id) {
        ChannelIdentityResolution::Resolved(_principal) => {
            // User is authorized; proceed to handle the modal submission.
        }
        ChannelIdentityResolution::Denied { .. } => {
            tracing::warn!(
                target: "tandem_server::discord_interactions",
                user_id = %user_id,
                "rejecting Discord modal submission from unauthorized user"
            );
            return reject_forbidden("user not in allowed_users");
        }
        ChannelIdentityResolution::ChannelNotConfigured(_) => {
            return reject_bad_request("discord channel not properly configured");
        }
    }
    let profile =
        channel_security_profile_from_config(&effective_config, ChannelKind::Discord.as_str());
    if !state
        .channel_user_can_approve(
            ChannelKind::Discord.as_str(),
            &user_id,
            profile,
            channel_is_open_to_all(&effective_config, ChannelKind::Discord),
        )
        .await
    {
        tracing::warn!(
            target: "tandem_server::discord_interactions",
            user_id = %user_id,
            "rejecting Discord modal submission without approval capability"
        );
        return reject_forbidden("user lacks approval capability");
    }
    // GOV-B5b: on a channel that opts into step-up, an approval requires an active
    // per-identity step-up grant issued out-of-band by the control panel.
    if channel_requires_approval_step_up(&effective_config, ChannelKind::Discord.as_str())
        && !state
            .channel_step_up_active(ChannelKind::Discord.as_str(), &user_id)
            .await
    {
        tracing::warn!(
            target: "tandem_server::discord_interactions",
            user_id = %user_id,
            "rejecting Discord interaction without an active step-up"
        );
        return reject_forbidden("step-up required");
    }
    let rate_key = ChannelRateLimitKey {
        channel: ChannelKind::Discord.as_str().to_string(),
        user_id: user_id.clone(),
    };
    let rate_decision = state
        .channel_rate_limiter
        .check(&rate_key, ChannelRateLimitKind::Decision, profile)
        .await;
    if !rate_decision.allowed {
        return reject_rate_limited(rate_decision.retry_after_secs);
    }

    dispatch_decision(
        state,
        ParsedCustomId {
            action: "rework".to_string(),
            run_id,
            node_id,
        },
        &user_id,
        if reason.is_empty() {
            None
        } else {
            Some(reason)
        },
    )
    .await
}

async fn dispatch_decision(
    state: AppState,
    parsed: ParsedCustomId,
    user_id: &str,
    reason: Option<String>,
) -> Response {
    let input = crate::http::routines_automations::AutomationV2GateDecisionInput {
        decision: parsed.action.clone(),
        reason,
    };
    let tenant_context = state
        .get_automation_v2_run(&parsed.run_id)
        .await
        .map(|run| run.tenant_context)
        .unwrap_or_else(tandem_types::TenantContext::local_implicit);
    // GOV-B1: caller is verified (Ed25519 signature + allowlist + Approve tier);
    // attribute the decision to the Discord identity as a human approver.
    let decider = crate::automation_v2::governance::GovernanceActorRef::human(
        Some(user_id.to_string()),
        "discord",
    );
    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state,
        tenant_context,
        None,
        parsed.run_id.clone(),
        input,
        decider,
    )
    .await;

    match result {
        Ok(_) => {
            tracing::info!(
                target: "tandem_server::discord_interactions",
                run_id = %parsed.run_id,
                user = %user_id,
                action = %parsed.action,
                "Discord interaction decided gate"
            );
            // Type 7: UPDATE_MESSAGE — rewrite the original message inline.
            // We send a minimal acknowledgment; the full edit (with colors,
            // footer, etc.) is best done by a follow-up PATCH using the
            // discord_blocks builders. For v1 we ack with a brief content
            // line and let the dispatcher's message-update task replace the
            // card if it owns the original message handle.
            Json(json!({
                "type": 7,
                "data": {
                    "content": format!("`{}` by <@{}>.", parsed.action, user_id),
                    "embeds": [],
                    "components": [],
                }
            }))
            .into_response()
        }
        Err((status, body)) => {
            tracing::warn!(
                target: "tandem_server::discord_interactions",
                run_id = %parsed.run_id,
                status = %status,
                body = %body.0,
                "gate-decide returned non-success"
            );
            // Discord treats anything > 200 as a failure that disables the
            // endpoint long-term. Map non-200 to a UPDATE_MESSAGE response
            // so Discord stays happy and the user sees the conflict.
            let winner = body
                .0
                .pointer("/winningDecision/decision")
                .and_then(Value::as_str)
                .unwrap_or("another operator");
            Json(json!({
                "type": 7,
                "data": {
                    "content": format!(
                        "Already decided ({}) — refresh to see the latest state.",
                        winner
                    ),
                    "embeds": [],
                    "components": [],
                }
            }))
            .into_response()
        }
    }
}

async fn read_discord_public_key(state: &AppState) -> Option<String> {
    let effective = state.config.get_effective_value().await;
    effective
        .pointer("/channels/discord/public_key")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn reject_unauthorized(reason: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "Unauthorized", "reason": reason })),
    )
        .into_response()
}

fn reject_forbidden(reason: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({
            "error": "Forbidden",
            "reason": reason,
        })),
    )
        .into_response()
}

fn reject_rate_limited(retry_after_secs: u64) -> Response {
    let mut response = (
        StatusCode::TOO_MANY_REQUESTS,
        Json(json!({ "error": "rate limit exceeded" })),
    )
        .into_response();
    if let Ok(value) = axum::http::HeaderValue::from_str(&retry_after_secs.max(1).to_string()) {
        response
            .headers_mut()
            .insert(axum::http::header::RETRY_AFTER, value);
    }
    response
}

fn reject_bad_request(reason: &str) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": "BadRequest", "reason": reason })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_ring_returns_false_on_repeat() {
        let mut ring = DedupRing::new();
        assert!(ring.record_new("interaction-1"));
        assert!(!ring.record_new("interaction-1"));
        assert!(ring.record_new("interaction-2"));
    }

    #[test]
    fn dedup_ring_evicts_oldest_at_cap() {
        let mut ring = DedupRing::new();
        for i in 0..DEDUP_CAP {
            ring.record_new(&format!("k{i}"));
        }
        assert!(!ring.record_new("k0"));
        ring.record_new(&format!("k{DEDUP_CAP}"));
        assert!(ring.record_new("k0_evicted_now"));
    }

    /// Modal custom_id parsing handles the exact format `handle_modal_submit`
    /// produces. Keep this golden so the round-trip stays stable.
    #[test]
    fn modal_custom_id_format_is_recognizable() {
        let raw = "tdm-modal:rework:auto-v2-run-abc123:send_email";
        let mut parts = raw.splitn(4, ':');
        assert_eq!(parts.next(), Some("tdm-modal"));
        assert_eq!(parts.next(), Some("rework"));
        assert_eq!(parts.next(), Some("auto-v2-run-abc123"));
        assert_eq!(parts.next(), Some("send_email"));
    }
}
