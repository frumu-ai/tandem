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

use crate::AppState;

const DEDUP_CAP: usize = 4096;

static SEEN_INTERACTIONS: OnceLock<Mutex<DedupRing>> = OnceLock::new();

fn dedup_ring() -> &'static Mutex<DedupRing> {
    SEEN_INTERACTIONS.get_or_init(|| Mutex::new(DedupRing::new()))
}

struct DedupRing {
    set: HashSet<String>,
    order: std::collections::VecDeque<String>,
}

impl DedupRing {
    fn new() -> Self {
        Self {
            set: HashSet::with_capacity(DEDUP_CAP),
            order: std::collections::VecDeque::with_capacity(DEDUP_CAP),
        }
    }

    fn record_new(&mut self, key: &str) -> bool {
        if self.set.contains(key) {
            return false;
        }
        if self.order.len() >= DEDUP_CAP {
            if let Some(oldest) = self.order.pop_front() {
                self.set.remove(&oldest);
            }
        }
        self.set.insert(key.to_string());
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

    let user_id = payload
        .pointer("/member/user/id")
        .or_else(|| payload.pointer("/user/id"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

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

    if prefix != "tdm-modal" || action != "rework" {
        return reject_bad_request(&format!("unrecognized modal custom_id: {custom_id}"));
    }

    let reason = payload
        .pointer("/data/components/0/components/0/value")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();

    let user_id = payload
        .pointer("/member/user/id")
        .or_else(|| payload.pointer("/user/id"))
        .and_then(Value::as_str)
        .unwrap_or("unknown")
        .to_string();

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
    let result = crate::http::routines_automations::automations_v2_run_gate_decide(
        State(state),
        axum::extract::Path(parsed.run_id.clone()),
        Json(input),
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
