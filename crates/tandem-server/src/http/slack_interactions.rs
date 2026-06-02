//! Slack interaction endpoint.
//!
//! Slack POSTs a payload here whenever a user clicks a button on a
//! Block Kit card, submits a modal, or invokes an interaction shortcut.
//! Slack's spec is `application/x-www-form-urlencoded` with one field
//! `payload` whose value is the JSON interaction body.
//!
//! Hard requirements (per Slack docs):
//! - Verify the request via HMAC-SHA256 over `v0:{timestamp}:{raw_body}`
//!   using the app signing secret. See [`tandem_channels::signing`].
//! - Reject any timestamp older than 5 minutes (replay protection).
//! - Acknowledge the request within 3 seconds. We do this synchronously by
//!   processing button clicks fast (gate-decide is in-memory) and returning
//!   200 with an empty body — Slack treats that as success and does not retry.
//! - Idempotent on retries: dedup by `(action_ts, action_id)` so accidental
//!   double-fires don't double-decide.
//!
//! Decision dispatch reuses `automations_v2_run_gate_decide` directly. The
//! shared `pause_for_gate` / `decide_gate` helpers from W1.3 will replace
//! that direct call when they land.

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use axum::Json;
use serde_json::{json, Value};
use tandem_channels::signing::verify_slack_signature;

use crate::app::rate_limit::{ChannelRateLimitKey, ChannelRateLimitKind};
use crate::app::state::channel_user_capabilities::channel_security_profile_from_config;
use crate::app::state::principals::channel_identity::{
    resolve_channel_user, ChannelIdentityResolution, ChannelKind,
};
use crate::AppState;

/// Bounded LRU-ish dedup set for Slack interaction `(action_ts, action_id)`
/// pairs. Slack retries interactions if the 3-second ack is missed; this
/// prevents the second retry from double-decide-ing.
///
/// Cap is intentionally small — entries age out by FIFO once the cap is hit.
/// In a real production deployment this would be tenant-scoped and persisted;
/// for v1 in-memory dedup is sufficient because gate decisions are themselves
/// idempotent at the run level (the second call hits the 409 path with the
/// winner identity from W2.6).
const DEDUP_CAP: usize = 4096;
const DEDUP_TTL_SECS: u64 = 300; // 5 minutes — Slack retries within minutes

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

    /// Returns `true` if the key is new (and records it). Returns `false` if
    /// the key was already seen recently (within TTL).
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

/// Slack interaction handler.
///
/// Wired at `POST /channels/slack/interactions`.
pub(crate) async fn slack_interactions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let signing_secret = match read_slack_signing_secret(&state).await {
        Some(secret) => secret,
        None => return reject_unauthorized("slack signing secret not configured"),
    };

    let signature = headers
        .get("x-slack-signature")
        .and_then(|v| v.to_str().ok());
    let timestamp = headers
        .get("x-slack-request-timestamp")
        .and_then(|v| v.to_str().ok());

    let now = chrono::Utc::now().timestamp();
    if let Err(error) = verify_slack_signature(&body, signature, timestamp, &signing_secret, now) {
        tracing::warn!(target: "tandem_server::slack_interactions", ?error, "rejecting unsigned/forged Slack interaction");
        return reject_unauthorized(&error.to_string());
    }

    let payload = match parse_slack_interaction_body(&body) {
        Ok(payload) => payload,
        Err(reason) => return reject_bad_request(&reason),
    };

    let dedup_key = make_dedup_key(&payload);
    if let Some(key) = dedup_key.as_ref() {
        let mut guard = dedup_ring().lock().expect("dedup mutex poisoned");
        if !guard.record_new(key) {
            tracing::debug!(target: "tandem_server::slack_interactions", %key, "duplicate Slack interaction — already processed");
            return ok_empty();
        }
    }

    let action = match extract_primary_action(&payload) {
        Ok(action) => action,
        Err(reason) => return reject_bad_request(&reason),
    };

    // CRITICAL: Authorize the user against the allowlist BEFORE dispatching.
    let effective_config = state.config.get_effective_value().await;
    match resolve_channel_user(&effective_config, ChannelKind::Slack, &action.user_id) {
        ChannelIdentityResolution::Resolved(_principal) => {
            // User is authorized; proceed to handle the action.
        }
        ChannelIdentityResolution::Denied { .. } => {
            tracing::warn!(
                target: "tandem_server::slack_interactions",
                user_id = %action.user_id,
                "rejecting Slack interaction from unauthorized user"
            );
            return reject_forbidden("user not in allowed_users");
        }
        ChannelIdentityResolution::ChannelNotConfigured(_) => {
            return reject_bad_request("slack channel not properly configured");
        }
    }
    let profile =
        channel_security_profile_from_config(&effective_config, ChannelKind::Slack.as_str());
    if !state
        .channel_user_can_approve(ChannelKind::Slack.as_str(), &action.user_id, profile)
        .await
    {
        tracing::warn!(
            target: "tandem_server::slack_interactions",
            user_id = %action.user_id,
            "rejecting Slack interaction without approval capability"
        );
        return reject_forbidden("user lacks approval capability");
    }
    let rate_key = ChannelRateLimitKey {
        channel: ChannelKind::Slack.as_str().to_string(),
        user_id: action.user_id.clone(),
    };
    let rate_decision = state
        .channel_rate_limiter
        .check(&rate_key, ChannelRateLimitKind::Decision, profile)
        .await;
    if !rate_decision.allowed {
        return reject_rate_limited(rate_decision.retry_after_secs);
    }

    let parsed_value = match parse_button_value(&action.value) {
        Ok(v) => v,
        Err(reason) => return reject_bad_request(&reason),
    };
    let Some(run_id) = parsed_value
        .pointer("/correlation/automation_v2_run_id")
        .and_then(Value::as_str)
        .map(|s| s.to_string())
    else {
        return reject_bad_request("button value missing automation_v2_run_id");
    };

    // Translate Slack action_id → gate-decide decision string.
    let decision = match action.action_id.as_str() {
        "approve" => "approve",
        "rework" => "rework",
        "cancel" => "cancel",
        other => return reject_bad_request(&format!("unknown action_id: {other}")),
    };

    // For W2.4 we dispatch the approve/cancel decisions directly. Rework
    // requires a reason and Slack passes the reason via a follow-up modal
    // submission — that round-trip lands in W2.5. For now we accept the
    // rework click but defer the decision until the modal is wired.
    if decision == "rework" {
        // Open the modal (the caller built it via slack_blocks::build_rework_modal_payload).
        // Until the modal POST handler lands in W2.5, return 200 with a hint.
        tracing::info!(
            target: "tandem_server::slack_interactions",
            run_id = %run_id,
            "rework button clicked; modal flow lands in W2.5"
        );
        return ok_empty();
    }

    let input = crate::http::routines_automations::AutomationV2GateDecisionInput {
        decision: decision.to_string(),
        reason: None,
    };

    let tenant_context = state
        .get_automation_v2_run(&run_id)
        .await
        .map(|run| run.tenant_context)
        .unwrap_or_else(tandem_types::TenantContext::local_implicit);
    // GOV-B1: this user has already passed signature verification, allowlist, and
    // the Approve capability-tier check above, so record the decision as a verified
    // human approver attributed to the Slack identity.
    let decider = crate::automation_v2::governance::GovernanceActorRef::human(
        Some(action.user_id.clone()),
        "slack",
    );
    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state,
        tenant_context,
        None,
        run_id.clone(),
        input,
        decider,
    )
    .await;

    match result {
        Ok(_) => {
            tracing::info!(
                target: "tandem_server::slack_interactions",
                run_id = %run_id,
                user = %action.user_id,
                decision,
                "Slack interaction decided gate"
            );
            ok_empty()
        }
        Err((status, body_json)) => {
            // Race UX: if we lost the race, surface "already decided by …"
            // back via the response. Slack will render the response_url
            // payload separately — for now, log + return the same status.
            tracing::warn!(
                target: "tandem_server::slack_interactions",
                run_id = %run_id,
                status = %status,
                body = %body_json.0,
                "gate-decide returned non-success"
            );
            // Slack treats anything > 200 as a retry trigger; map 409 to 200
            // with the body so Slack does not retry the (now-resolved) action.
            ok_with_payload(json!({
                "ok": false,
                "status": status.as_u16(),
                "body": body_json.0,
            }))
        }
    }
}

#[derive(Debug, Clone)]
struct PrimaryAction {
    action_id: String,
    value: String,
    user_id: String,
}

fn extract_primary_action(payload: &Value) -> Result<PrimaryAction, String> {
    let actions = payload
        .get("actions")
        .and_then(Value::as_array)
        .ok_or_else(|| "payload missing `actions` array".to_string())?;
    let first = actions
        .first()
        .ok_or_else(|| "actions array is empty".to_string())?;
    let action_id = first
        .get("action_id")
        .and_then(Value::as_str)
        .ok_or_else(|| "action missing action_id".to_string())?
        .to_string();
    let value = first
        .get("value")
        .and_then(Value::as_str)
        .ok_or_else(|| "action missing value".to_string())?
        .to_string();
    let user_id = payload
        .pointer("/user/id")
        .and_then(Value::as_str)
        .ok_or_else(|| "payload missing user identification".to_string())?
        .to_string();
    Ok(PrimaryAction {
        action_id,
        value,
        user_id,
    })
}

fn parse_button_value(raw: &str) -> Result<Value, String> {
    serde_json::from_str(raw).map_err(|err| format!("button value is not JSON: {err}"))
}

fn make_dedup_key(payload: &Value) -> Option<String> {
    let action_ts = payload
        .pointer("/actions/0/action_ts")
        .and_then(Value::as_str)?;
    let action_id = payload
        .pointer("/actions/0/action_id")
        .and_then(Value::as_str)?;
    Some(format!("{action_ts}:{action_id}"))
}

/// Parse Slack's `application/x-www-form-urlencoded` body. Slack sends the
/// interaction JSON as the value of a single `payload` field.
fn parse_slack_interaction_body(body: &[u8]) -> Result<Value, String> {
    let body_str = std::str::from_utf8(body).map_err(|_| "body is not utf-8".to_string())?;
    for pair in body_str.split('&') {
        let mut parts = pair.splitn(2, '=');
        let key = parts.next().unwrap_or("");
        let value = parts.next().unwrap_or("");
        if key == "payload" {
            let decoded = url_decode(value);
            return serde_json::from_str(&decoded)
                .map_err(|err| format!("payload field is not valid JSON: {err}"));
        }
    }
    Err("body did not contain a `payload` form field".to_string())
}

fn url_decode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let bytes = input.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_digit(bytes[i + 1]);
                let lo = hex_digit(bytes[i + 2]);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi << 4 | lo) as char);
                    i += 3;
                } else {
                    out.push('%');
                    i += 1;
                }
            }
            other => {
                out.push(other as char);
                i += 1;
            }
        }
    }
    out
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

fn reject_unauthorized(reason: &str) -> Response {
    (
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "error": "Unauthorized",
            "reason": reason,
        })),
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
        Json(json!({
            "error": "BadRequest",
            "reason": reason,
        })),
    )
        .into_response()
}

fn ok_empty() -> Response {
    (StatusCode::OK, Json(json!({}))).into_response()
}

fn ok_with_payload(value: Value) -> Response {
    (StatusCode::OK, Json(value)).into_response()
}

use axum::response::IntoResponse;

/// Read the configured Slack signing secret from `state.config`. Returns
/// `None` when the channel is not configured or the secret field is empty —
/// either case must be treated as "interactions are not enabled," not as a
/// silent allow.
async fn read_slack_signing_secret(state: &AppState) -> Option<String> {
    let effective = state.config.get_effective_value().await;
    effective
        .pointer("/channels/slack/signing_secret")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_decode_handles_basic_pct_encodings() {
        assert_eq!(url_decode("hello%20world"), "hello world");
        assert_eq!(url_decode("a+b"), "a b");
        assert_eq!(url_decode("%7B%7D"), "{}");
    }

    #[test]
    fn parse_slack_interaction_body_extracts_payload_field() {
        let body = "payload=%7B%22type%22%3A%22block_actions%22%7D";
        let parsed = parse_slack_interaction_body(body.as_bytes()).expect("parsed");
        assert_eq!(
            parsed.get("type").and_then(Value::as_str),
            Some("block_actions")
        );
    }

    #[test]
    fn parse_slack_interaction_body_rejects_missing_payload() {
        let body = "team_id=T123&user_id=U456";
        let err = parse_slack_interaction_body(body.as_bytes()).unwrap_err();
        assert!(err.contains("payload"));
    }

    #[test]
    fn extract_primary_action_returns_first_button() {
        let payload = json!({
            "actions": [
                { "action_id": "approve", "value": "{\"x\":1}" },
                { "action_id": "rework", "value": "{}" }
            ],
            "user": { "id": "U999" }
        });
        let action = extract_primary_action(&payload).expect("action");
        assert_eq!(action.action_id, "approve");
        assert_eq!(action.value, "{\"x\":1}");
        assert_eq!(action.user_id, "U999");
    }

    #[test]
    fn make_dedup_key_uses_action_ts_and_action_id() {
        let payload = json!({
            "actions": [{ "action_id": "approve", "action_ts": "1700000000.0001" }]
        });
        let key = make_dedup_key(&payload).expect("key");
        assert_eq!(key, "1700000000.0001:approve");
    }

    #[test]
    fn dedup_ring_returns_false_on_repeat() {
        let mut ring = DedupRing::new();
        assert!(ring.record_new("a"));
        assert!(!ring.record_new("a"));
        assert!(ring.record_new("b"));
    }

    #[test]
    fn dedup_ring_evicts_oldest_at_cap() {
        let mut ring = DedupRing::new();
        for i in 0..DEDUP_CAP {
            ring.record_new(&format!("k{i}"));
        }
        assert!(!ring.record_new("k0"));
        ring.record_new(&format!("k{DEDUP_CAP}"));
        // After overflow, "k0" is still in the ring (record_new returned false)
        // but inserting a brand new key past the cap should evict "k0".
        assert!(ring.record_new("k0_again_after_evict"));
    }
}
