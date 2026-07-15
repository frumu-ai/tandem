// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

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

mod claims;
mod runtime;

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock, Weak};

use anyhow::Context;
use axum::body::Bytes;
use axum::extract::{Extension, Path, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode};
use axum::response::Response;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_channels::config::SlackConfig;
use tandem_channels::dispatcher::{
    build_channel_session_permissions, channel_memory_subject_client_id,
};
use tandem_channels::redaction::redact_outbound;
use tandem_channels::signing::verify_slack_signature;
use tandem_channels::slack::SlackChannel;
use tandem_channels::traits::{Channel, ThreadReply};
use tandem_types::{
    AccessEffect, AssertionMetadata, AuthorityChain, CreateSessionRequest, DataBoundary,
    HumanActor, MessagePart, MessagePartInput, MessageRole, ModelSpec, OrganizationUnitKind,
    PrincipalRef, RequestPrincipal, ResourceKind, ResourceRef, ResourceScope, SamplingParams,
    SendMessageRequest, StrictTenantContext, TenantContext, VerifiedTenantContext,
};
use tokio_util::sync::CancellationToken;

use crate::app::rate_limit::{ChannelRateLimitKey, ChannelRateLimitKind};
use crate::app::state::channel_user_capabilities::{
    channel_requires_approval_step_up, channel_security_profile_from_config,
};
use crate::app::state::principals::channel_identity::{
    channel_bound_tenant, channel_is_open_to_all, resolve_slack_user_for_installation,
    ChannelIdentityResolution, ChannelKind,
};
use crate::AppState;

use claims::{
    checkpoint_slack_event_execution, claim_slack_event, compact_slack_event_claims,
    complete_slack_event_claim, mark_slack_event_response_audited,
    mark_slack_event_response_delivered, quarantine_slack_event_claim, recover_slack_event_claims,
    refresh_slack_event_claim, retry_slack_event_claim, stage_slack_event_response,
    RecoverableSlackEventClaim, SlackEventClaim, SlackEventClaimDecision, SlackEventClaimInput,
    CLAIM_HEARTBEAT, CLAIM_RECOVERY_SCAN_INTERVAL,
};
use runtime::{
    build_governed_slack_context, run_claimed_slack_event, run_slack_event_recovery_worker,
};

/// Bounded FIFO dedup for Slack interaction `(action_ts, action_id)` retries.
/// Gate decisions provide the durable idempotency boundary after entries expire.
const DEDUP_CAP: usize = 4096;
const DEDUP_TTL_SECS: u64 = 300; // 5 minutes — Slack retries within minutes

static SEEN_INTERACTIONS: OnceLock<Mutex<DedupRing>> = OnceLock::new();
static SLACK_EXECUTION_LOCKS: OnceLock<
    tokio::sync::Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>>,
> = OnceLock::new();

const SLACK_CONTEXT_TTL_MS: u64 = 60 * 60 * 1_000;
const SLACK_CONTEXT_ISSUER: &str = "tandem-server:slack-events";
const SLACK_CONTEXT_AUDIENCE: &str = "tandem-engine";

fn dedup_ring() -> &'static Mutex<DedupRing> {
    SEEN_INTERACTIONS.get_or_init(|| Mutex::new(DedupRing::new()))
}

fn slack_execution_locks(
) -> &'static tokio::sync::Mutex<HashMap<String, Weak<tokio::sync::Mutex<()>>>> {
    SLACK_EXECUTION_LOCKS.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()))
}

async fn slack_execution_lock(key: &str) -> Arc<tokio::sync::Mutex<()>> {
    let mut locks = slack_execution_locks().lock().await;
    locks.retain(|_, lock| lock.strong_count() > 0);
    if let Some(lock) = locks.get(key).and_then(Weak::upgrade) {
        return lock;
    }
    let lock = Arc::new(tokio::sync::Mutex::new(()));
    locks.insert(key.to_string(), Arc::downgrade(&lock));
    lock
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

    let effective_config = state.config.get_effective_value().await;
    let installation = match validate_slack_interaction_installation(&effective_config, &payload) {
        Ok(installation) => installation,
        Err(reason) => {
            tracing::warn!(target: "tandem_server::slack_interactions", %reason, "rejecting Slack interaction outside configured installation");
            return reject_forbidden(&reason);
        }
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
    let resolved_principal = match resolve_slack_user_for_installation(
        &effective_config,
        &installation.team_id,
        &installation.app_id,
        &action.user_id,
    ) {
        ChannelIdentityResolution::Resolved(principal) => principal,
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
    };
    let approval_identity = resolved_principal.actor_id.unwrap_or_else(|| {
        slack_installation_identity(&installation.team_id, &installation.app_id, &action.user_id)
    });
    let profile =
        channel_security_profile_from_config(&effective_config, ChannelKind::Slack.as_str());
    if !state
        .channel_user_can_approve(
            ChannelKind::Slack.as_str(),
            &approval_identity,
            profile,
            channel_is_open_to_all(&effective_config, ChannelKind::Slack),
        )
        .await
    {
        tracing::warn!(
            target: "tandem_server::slack_interactions",
            user_id = %action.user_id,
            "rejecting Slack interaction without approval capability"
        );
        return reject_forbidden("user lacks approval capability");
    }
    // GOV-B5b: on a channel that opts into step-up, an approval requires an active
    // per-identity step-up grant issued out-of-band by the control panel.
    if channel_requires_approval_step_up(&effective_config, ChannelKind::Slack.as_str())
        && !state
            .channel_step_up_active(ChannelKind::Slack.as_str(), &approval_identity)
            .await
    {
        tracing::warn!(
            target: "tandem_server::slack_interactions",
            user_id = %action.user_id,
            "rejecting Slack interaction without an active step-up"
        );
        return reject_forbidden("step-up required");
    }
    let rate_key = ChannelRateLimitKey {
        channel: ChannelKind::Slack.as_str().to_string(),
        user_id: approval_identity.clone(),
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
        approval_request_id: None,
        transition_id: None,
    };

    let tenant_context = state
        .get_automation_v2_run(&run_id)
        .await
        .map(|run| run.tenant_context)
        .unwrap_or_else(tandem_types::TenantContext::local_implicit);
    // GOV-B5c: if this channel is bound to a tenant, refuse to act on a run that
    // belongs to a different tenant (prevents a channel acting cross-tenant by run
    // id). An unbound channel (single-tenant/local) is unaffected.
    if let Some((org_id, workspace_id)) =
        channel_bound_tenant(&effective_config, ChannelKind::Slack)
    {
        if tenant_context.org_id != org_id || tenant_context.workspace_id != workspace_id {
            tracing::warn!(
                target: "tandem_server::slack_interactions",
                user_id = %action.user_id,
                "rejecting Slack interaction targeting a run outside the channel's bound tenant"
            );
            let channel_tenant = tandem_types::TenantContext::explicit_user_workspace(
                org_id,
                workspace_id,
                None,
                "slack",
            );
            if let Err(error) = crate::http::channel_interaction_audit::append_cross_tenant_denial(
                &state,
                "slack",
                &approval_identity,
                &run_id,
                channel_tenant,
                &tenant_context,
            )
            .await
            {
                return reject_forbidden(&format!(
                    "channel denied; required denial receipt persistence failed: {error}"
                ));
            }
            return reject_forbidden("channel not bound to this run's tenant");
        }
    }
    // GOV-B1: this user has already passed signature verification, allowlist, and
    // the Approve capability-tier check above, so record the decision as a verified
    // human approver attributed to the Slack identity.
    let decider = crate::automation_v2::governance::GovernanceActorRef::human(
        Some(approval_identity.clone()),
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
    config_string(&effective, "/channels/slack/signing_secret")
}

/// Slack Events API ingress. Slack's signature authenticates the event before a
/// server-owned principal is resolved and dispatched through the normal session
/// prompt path. The HTTP request is acknowledged before the model run so Slack's
/// three-second delivery deadline is not coupled to provider latency.
pub(crate) async fn slack_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let effective_config = state.config.get_effective_value().await;
    let signing_secret = match config_string(&effective_config, "/channels/slack/signing_secret") {
        Some(secret) => secret,
        None => {
            audit_slack_denial(
                &state,
                &effective_config,
                None,
                "slack signing secret not configured",
                json!({}),
            )
            .await;
            return reject_forbidden("slack signing secret not configured");
        }
    };
    let signature = headers
        .get("x-slack-signature")
        .and_then(|v| v.to_str().ok());
    let timestamp = headers
        .get("x-slack-request-timestamp")
        .and_then(|v| v.to_str().ok());
    let now = chrono::Utc::now().timestamp();
    if let Err(error) = verify_slack_signature(&body, signature, timestamp, &signing_secret, now) {
        tracing::warn!(target: "tandem_server::slack_events", ?error, "rejecting unsigned/forged Slack event");
        audit_slack_denial(
            &state,
            &effective_config,
            None,
            "Slack event signature verification failed",
            json!({ "request_timestamp": timestamp }),
        )
        .await;
        return reject_forbidden(&error.to_string());
    }

    let payload: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            audit_slack_denial(
                &state,
                &effective_config,
                None,
                "invalid Slack event JSON",
                json!({}),
            )
            .await;
            return reject_bad_request("invalid Slack event JSON");
        }
    };

    match payload.get("type").and_then(Value::as_str) {
        // Slack setup handshake: echo the challenge (signature already verified).
        Some("url_verification") => {
            if effective_config
                .pointer("/channels/slack/events_enabled")
                .and_then(Value::as_bool)
                != Some(true)
            {
                audit_slack_denial(
                    &state,
                    &effective_config,
                    None,
                    "slack events ingress not enabled",
                    json!({ "envelope_type": "url_verification" }),
                )
                .await;
                return reject_forbidden("slack events ingress not enabled");
            }
            let challenge = payload
                .get("challenge")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            (StatusCode::OK, challenge).into_response()
        }
        Some("event_callback") => {
            handle_slack_event_callback(&state, &effective_config, &payload).await
        }
        // Other envelope types are acknowledged so Slack does not retry.
        _ => ok_empty(),
    }
}

async fn handle_slack_event_callback(
    state: &AppState,
    effective_config: &Value,
    payload: &Value,
) -> Response {
    if effective_config
        .pointer("/channels/slack/events_enabled")
        .and_then(Value::as_bool)
        != Some(true)
    {
        audit_slack_denial(
            state,
            effective_config,
            None,
            "slack events ingress not enabled",
            json!({}),
        )
        .await;
        return reject_forbidden("slack events ingress not enabled");
    }
    let installation = match validate_slack_event_installation(effective_config, payload) {
        Ok(installation) => installation,
        Err(reason) => {
            tracing::warn!(target: "tandem_server::slack_events", %reason, "rejecting Slack event outside configured installation");
            audit_slack_denial(
                state,
                effective_config,
                None,
                &reason,
                json!({
                    "event_id": payload.get("event_id"),
                    "team_id": payload.get("team_id"),
                    "api_app_id": payload.get("api_app_id"),
                }),
            )
            .await;
            return reject_forbidden(&reason);
        }
    };
    let event = match parse_slack_message_event(payload) {
        Ok(Some(event)) => event,
        Ok(None) => return ok_empty(),
        Err(reason) => {
            audit_slack_denial(
                state,
                &effective_config,
                None,
                reason,
                json!({"event_id": payload.get("event_id")}),
            )
            .await;
            return reject_bad_request(reason);
        }
    };

    let Some(configured_channel_id) =
        config_string(&effective_config, "/channels/slack/channel_id")
    else {
        audit_slack_denial(
            state,
            &effective_config,
            None,
            "slack channel id not configured",
            json!({"event_id": event.event_id}),
        )
        .await;
        return reject_forbidden("slack channel id not configured");
    };
    if configured_channel_id != event.channel_id {
        tracing::warn!(target: "tandem_server::slack_events", channel_id = %event.channel_id, "rejecting Slack event outside configured channel");
        audit_slack_denial(
            state,
            &effective_config,
            None,
            "channel is not configured for this Slack app",
            json!({
                "event_id": event.event_id,
                "slack_channel_id": event.channel_id,
                "slack_team_id": installation.team_id,
                "slack_app_id": installation.app_id,
            }),
        )
        .await;
        return reject_forbidden("channel is not configured for this Slack app");
    }
    if config_string(&effective_config, "/channels/slack/bot_token").is_none() {
        audit_slack_denial(
            state,
            &effective_config,
            None,
            "slack bot token not configured",
            json!({"event_id": event.event_id}),
        )
        .await;
        return reject_forbidden("slack bot token not configured");
    }
    let mention_only = effective_config
        .pointer("/channels/slack/mention_only")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if mention_only
        && event.event_type != "app_mention"
        && event.channel_type.as_deref() != Some("im")
    {
        return ok_empty();
    }

    let request_principal = match resolve_slack_user_for_installation(
        &effective_config,
        &installation.team_id,
        &installation.app_id,
        &event.user_id,
    ) {
        ChannelIdentityResolution::Resolved(principal) => principal,
        ChannelIdentityResolution::Denied { .. } => {
            tracing::warn!(target: "tandem_server::slack_events", user_id = %event.user_id, "rejecting Slack message event from unauthorized user");
            audit_slack_denial(
                state,
                &effective_config,
                None,
                "user not in allowed_users",
                json!({
                    "event_id": event.event_id,
                    "slack_user_id": event.user_id,
                    "slack_team_id": installation.team_id,
                    "slack_app_id": installation.app_id,
                }),
            )
            .await;
            return reject_forbidden("user not in allowed_users");
        }
        ChannelIdentityResolution::ChannelNotConfigured(_) => {
            audit_slack_denial(
                state,
                &effective_config,
                None,
                "slack channel not configured",
                json!({"event_id": event.event_id}),
            )
            .await;
            return reject_forbidden("slack channel not configured");
        }
    };
    let actor_id = request_principal.actor_id.clone();

    let verified_tenant_context = match build_governed_slack_context(
        state,
        &effective_config,
        &event,
        &installation,
        request_principal,
    )
    .await
    {
        Ok(context) => context,
        Err(reason) => {
            tracing::warn!(target: "tandem_server::slack_events", user_id = %event.user_id, %reason, "rejecting Slack message without governed identity context");
            audit_slack_denial(
                state,
                &effective_config,
                actor_id,
                &reason,
                json!({
                    "event_id": event.event_id,
                    "slack_user_id": event.user_id,
                    "slack_team_id": installation.team_id,
                    "slack_app_id": installation.app_id,
                }),
            )
            .await;
            return reject_forbidden(&reason);
        }
    };

    let fingerprint = slack_event_fingerprint(&event, &installation);
    let recovery_payload = match serde_json::to_value(SlackEventRecoveryPayload {
        event: event.clone(),
        installation: installation.clone(),
    }) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::error!(target: "tandem_server::slack_events", %error, "failed to serialize Slack event recovery payload");
            return retry_slack_event_response("Could not prepare durable Slack event recovery");
        }
    };
    let claim = match claim_slack_event(
        state,
        SlackEventClaimInput {
            tenant_context: verified_tenant_context.tenant_context.clone(),
            team_id: installation.team_id.clone(),
            app_id: installation.app_id.clone(),
            event_id: event.event_id.clone(),
            fingerprint,
            recovery_payload,
            now_ms: crate::now_ms(),
        },
    )
    .await
    {
        Ok(SlackEventClaimDecision::Claimed(claim)) => claim,
        Ok(SlackEventClaimDecision::Completed) => {
            let _ = emit_slack_tenant_audit(
                state,
                &verified_tenant_context.tenant_context,
                verified_tenant_context.tenant_context.actor_id.clone(),
                "channel.slack.ingress.duplicate_completed",
                slack_audit_dimensions(&event, &installation, None),
            )
            .await;
            return ok_empty();
        }
        Ok(SlackEventClaimDecision::InFlight) => {
            super::sessions::publish_tenant_event(
                state,
                &verified_tenant_context.tenant_context,
                "channel.slack.ingress.duplicate_in_flight",
                slack_audit_dimensions(&event, &installation, None),
            );
            return retry_slack_event_response("Slack event is already processing");
        }
        Ok(SlackEventClaimDecision::RetryScheduled) => {
            super::sessions::publish_tenant_event(
                state,
                &verified_tenant_context.tenant_context,
                "channel.slack.ingress.retry_scheduled",
                slack_audit_dimensions(&event, &installation, None),
            );
            return retry_slack_event_response("Slack event retry is backoff-scheduled");
        }
        Ok(SlackEventClaimDecision::Quarantined) => {
            let _ = emit_slack_tenant_audit(
                state,
                &verified_tenant_context.tenant_context,
                verified_tenant_context.tenant_context.actor_id.clone(),
                "channel.slack.ingress.quarantined",
                slack_audit_dimensions(&event, &installation, None),
            )
            .await;
            return ok_empty();
        }
        Ok(SlackEventClaimDecision::Conflict) => {
            audit_slack_denial(
                state,
                &effective_config,
                verified_tenant_context.tenant_context.actor_id.clone(),
                "Slack event id was replayed with a conflicting payload",
                slack_audit_dimensions(&event, &installation, None),
            )
            .await;
            return reject_forbidden("Slack event id conflicts with an existing claim");
        }
        Err(error) => {
            tracing::error!(target: "tandem_server::slack_events", %error, "failed to reserve durable Slack event claim");
            audit_slack_denial(
                state,
                &effective_config,
                verified_tenant_context.tenant_context.actor_id.clone(),
                "durable Slack event claim failed",
                slack_audit_dimensions(&event, &installation, None),
            )
            .await;
            return retry_slack_event_response("Could not durably claim Slack event");
        }
    };

    if let Err(error) = emit_slack_tenant_audit(
        state,
        &verified_tenant_context.tenant_context,
        verified_tenant_context.tenant_context.actor_id.clone(),
        "channel.slack.ingress.accepted",
        json!({
            "attempt": claim.attempt,
            "claim_key": &claim.key,
            "dimensions": slack_audit_dimensions(&event, &installation, None),
        }),
    )
    .await
    {
        let _ = retry_slack_event_claim(&claim, &error.to_string(), crate::now_ms()).await;
        return retry_slack_event_response("Could not persist Slack ingress audit");
    }

    let task_state = state.clone();
    let task_config = effective_config.clone();
    let task_claim = claim.clone();
    let spawn_result = state
        .slack_event_tasks
        .spawn(move |cancel| async move {
            run_claimed_slack_event(
                task_state,
                task_config,
                event,
                installation,
                verified_tenant_context,
                task_claim,
                cancel,
            )
            .await;
        })
        .await;
    if let Err(error) = spawn_result {
        let _ = retry_slack_event_claim(&claim, &error.to_string(), crate::now_ms()).await;
        return retry_slack_event_response("Slack event runtime is shutting down");
    }
    ok_empty()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct SlackInstallationBinding {
    team_id: String,
    app_id: String,
}

fn slack_installation_identity(team_id: &str, app_id: &str, user_id: &str) -> String {
    format!("channel:slack:{team_id}:{app_id}:{user_id}")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlackEventRecoveryPayload {
    event: SlackMessageEvent,
    installation: SlackInstallationBinding,
}

pub(crate) async fn start_slack_event_recovery_worker(state: &AppState) -> anyhow::Result<bool> {
    let worker_state = state.clone();
    state
        .slack_event_tasks
        .start_recovery_worker(move |cancel| async move {
            run_slack_event_recovery_worker(worker_state, cancel).await;
        })
        .await
}

async fn emit_slack_tenant_audit(
    state: &AppState,
    tenant_context: &TenantContext,
    actor_id: Option<String>,
    event_type: &str,
    payload: Value,
) -> anyhow::Result<()> {
    super::sessions::publish_tenant_event(state, tenant_context, event_type, payload.clone());
    crate::audit::append_protected_audit_event(state, event_type, tenant_context, actor_id, payload)
        .await
}

fn configured_slack_tenant_context(
    effective_config: &Value,
    actor_id: Option<String>,
) -> Option<TenantContext> {
    let (org_id, workspace_id) = channel_bound_tenant(effective_config, ChannelKind::Slack)?;
    let mut tenant = TenantContext::explicit(org_id, workspace_id, actor_id);
    tenant.deployment_id = config_string(effective_config, "/channels/slack/tenant/deployment_id");
    Some(tenant)
}

async fn audit_slack_denial(
    state: &AppState,
    effective_config: &Value,
    actor_id: Option<String>,
    reason: &str,
    details: Value,
) {
    let Some(tenant_context) = configured_slack_tenant_context(effective_config, actor_id.clone())
    else {
        return;
    };
    let _ = emit_slack_tenant_audit(
        state,
        &tenant_context,
        actor_id,
        "channel.slack.ingress.denied",
        json!({
            "reason": reason,
            "details": details,
        }),
    )
    .await;
}

fn validate_slack_event_installation(
    effective_config: &Value,
    payload: &Value,
) -> Result<SlackInstallationBinding, String> {
    let expected_team_id = config_string(effective_config, "/channels/slack/team_id")
        .ok_or_else(|| "slack events require a configured team_id".to_string())?;
    let expected_app_id = config_string(effective_config, "/channels/slack/app_id")
        .ok_or_else(|| "slack events require a configured app_id".to_string())?;
    let envelope_team_id = slack_event_envelope_team_id(payload)?;
    let envelope_app_id = config_string(payload, "/api_app_id")
        .ok_or_else(|| "Slack event envelope missing api_app_id".to_string())?;

    if envelope_team_id != expected_team_id {
        return Err("Slack event team_id does not match configured workspace".to_string());
    }
    if envelope_app_id != expected_app_id {
        return Err("Slack event api_app_id does not match configured app".to_string());
    }

    Ok(SlackInstallationBinding {
        team_id: expected_team_id,
        app_id: expected_app_id,
    })
}

fn validate_slack_interaction_installation(
    effective_config: &Value,
    payload: &Value,
) -> Result<SlackInstallationBinding, String> {
    let expected_team_id = config_string(effective_config, "/channels/slack/team_id")
        .ok_or_else(|| "slack interactions require a configured team_id".to_string())?;
    let expected_app_id = config_string(effective_config, "/channels/slack/app_id")
        .ok_or_else(|| "slack interactions require a configured app_id".to_string())?;
    let expected_channel_id = config_string(effective_config, "/channels/slack/channel_id")
        .ok_or_else(|| "slack interactions require a configured channel_id".to_string())?;
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
        [channel_id] => channel_id,
        [] => return Err("Slack interaction payload missing channel id".to_string()),
        _ => return Err("Slack interaction payload has conflicting channel ids".to_string()),
    };

    if payload_team_id != expected_team_id {
        return Err("Slack interaction team does not match configured workspace".to_string());
    }
    if payload_app_id != expected_app_id {
        return Err("Slack interaction app does not match configured app".to_string());
    }
    if payload_channel_id != &expected_channel_id {
        return Err("Slack interaction channel does not match configured channel".to_string());
    }
    Ok(SlackInstallationBinding {
        team_id: expected_team_id,
        app_id: expected_app_id,
    })
}

fn slack_event_envelope_team_id(payload: &Value) -> Result<String, String> {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SlackMessageEvent {
    event_id: String,
    event_type: String,
    channel_type: Option<String>,
    user_id: String,
    channel_id: String,
    text: String,
    message_ts: String,
    thread_ts: Option<String>,
}

impl SlackMessageEvent {
    fn thread_anchor(&self) -> &str {
        self.thread_ts.as_deref().unwrap_or(&self.message_ts)
    }

    fn scope_id(&self, installation: &SlackInstallationBinding) -> String {
        format!(
            "thread:{}:{}:{}:{}",
            installation.team_id,
            installation.app_id,
            self.channel_id,
            self.thread_anchor()
        )
    }
}

fn slack_event_fingerprint(
    event: &SlackMessageEvent,
    installation: &SlackInstallationBinding,
) -> String {
    crate::sha256_hex(&[
        &installation.team_id,
        &installation.app_id,
        &event.event_id,
        &event.event_type,
        &event.user_id,
        &event.channel_id,
        &event.text,
        &event.message_ts,
        event.thread_ts.as_deref().unwrap_or_default(),
    ])
}

fn slack_audit_dimensions(
    event: &SlackMessageEvent,
    installation: &SlackInstallationBinding,
    session_id: Option<&str>,
) -> Value {
    json!({
        "slack_team_id": installation.team_id,
        "slack_app_id": installation.app_id,
        "slack_channel_id": event.channel_id,
        "slack_user_id": event.user_id,
        "slack_event_id": event.event_id,
        "slack_thread_ts": event.thread_anchor(),
        "session_id": session_id,
        "prompt_sha256": crate::sha256_hex(&[&event.text]),
    })
}

fn retry_slack_event_response(reason: &str) -> Response {
    let mut response = (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "SlackEventRetryable",
            "reason": reason,
        })),
    )
        .into_response();
    response.headers_mut().insert(
        axum::http::header::RETRY_AFTER,
        HeaderValue::from_static("1"),
    );
    response
}

fn parse_slack_message_event(payload: &Value) -> Result<Option<SlackMessageEvent>, &'static str> {
    let Some(user_id) = slack_event_message_user(payload) else {
        return Ok(None);
    };
    let event = payload
        .get("event")
        .and_then(Value::as_object)
        .ok_or("Slack event callback missing event object")?;
    let required = |field: &'static str| {
        event
            .get(field)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .ok_or(field)
    };
    let event_id = payload
        .get("event_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .ok_or("Slack event callback missing event_id")?;
    let text = required("text").map_err(|_| "Slack message event missing text")?;
    let channel_id = required("channel").map_err(|_| "Slack message event missing channel")?;
    let message_ts = required("ts").map_err(|_| "Slack message event missing ts")?;
    let thread_ts = event
        .get("thread_ts")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    Ok(Some(SlackMessageEvent {
        event_id,
        event_type: required("type").map_err(|_| "Slack message event missing type")?,
        channel_type: event
            .get("channel_type")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string),
        user_id,
        channel_id,
        text,
        message_ts,
        thread_ts,
    }))
}

fn config_string(config: &Value, pointer: &str) -> Option<String> {
    config
        .pointer(pointer)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Extract the sender of an actionable user message from an `event_callback`
/// payload, or `None` for bot / system / edited messages that must not be
/// dispatched.
fn slack_event_message_user(payload: &Value) -> Option<String> {
    let event = payload.get("event")?;
    if !matches!(
        event.get("type").and_then(Value::as_str),
        Some("message" | "app_mention")
    ) {
        return None;
    }
    // Ignore bot messages and message subtypes (edits, joins, deletions, …).
    if event.get("bot_id").is_some() || event.get("subtype").is_some() {
        return None;
    }
    event
        .get("user")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slack_event_message_user_extracts_plain_message_sender() {
        let payload = json!({
            "type": "event_callback",
            "event": { "type": "message", "user": "U123", "text": "hi" }
        });
        assert_eq!(slack_event_message_user(&payload).as_deref(), Some("U123"));
        let mention = json!({
            "event": { "type": "app_mention", "user": "U456", "text": "<@BOT> hi" }
        });
        assert_eq!(slack_event_message_user(&mention).as_deref(), Some("U456"));
    }

    #[test]
    fn slack_event_message_user_ignores_bot_subtype_and_non_message() {
        let bot = json!({"event": {"type": "message", "user": "U1", "bot_id": "B1"}});
        assert!(slack_event_message_user(&bot).is_none());
        let edited =
            json!({"event": {"type": "message", "user": "U1", "subtype": "message_changed"}});
        assert!(slack_event_message_user(&edited).is_none());
        let non_message = json!({"event": {"type": "reaction_added", "user": "U1"}});
        assert!(slack_event_message_user(&non_message).is_none());
    }

    #[test]
    fn parse_slack_message_event_uses_root_thread_as_session_scope() {
        let payload = json!({
            "type": "event_callback",
            "team_id": "T1",
            "api_app_id": "A1",
            "event_id": "Ev1",
            "event": {
                "type": "message",
                "user": "U1",
                "channel": "C1",
                "text": "hello",
                "ts": "100.2",
                "thread_ts": "100.1"
            }
        });
        let event = parse_slack_message_event(&payload)
            .expect("valid event")
            .expect("actionable message");
        let installation = SlackInstallationBinding {
            team_id: "T1".to_string(),
            app_id: "A1".to_string(),
        };
        assert_eq!(event.scope_id(&installation), "thread:T1:A1:C1:100.1");
        assert_eq!(event.thread_anchor(), "100.1");
    }

    #[test]
    fn slack_event_installation_requires_configured_matching_team_and_app() {
        let config = json!({
            "channels": {
                "slack": { "team_id": "T1", "app_id": "A1" }
            }
        });
        let payload = json!({ "team_id": "T1", "api_app_id": "A1" });
        assert_eq!(
            validate_slack_event_installation(&config, &payload).unwrap(),
            SlackInstallationBinding {
                team_id: "T1".to_string(),
                app_id: "A1".to_string(),
            }
        );

        let wrong_team = json!({ "team_id": "T2", "api_app_id": "A1" });
        assert!(validate_slack_event_installation(&config, &wrong_team).is_err());
        let wrong_app = json!({ "team_id": "T1", "api_app_id": "A2" });
        assert!(validate_slack_event_installation(&config, &wrong_app).is_err());
        assert!(validate_slack_event_installation(&config, &json!({})).is_err());
    }

    #[test]
    fn slack_interactions_require_matching_team_app_and_channel() {
        let config = json!({
            "channels": {
                "slack": { "team_id": "T1", "app_id": "A1", "channel_id": "C1" }
            }
        });
        let payload = json!({
            "team": { "id": "T1" },
            "api_app_id": "A1",
            "channel": { "id": "C1" },
            "container": { "channel_id": "C1" }
        });
        assert!(validate_slack_interaction_installation(&config, &payload).is_ok());

        for pointer in ["team", "app", "channel"] {
            let mut cross_installation = payload.clone();
            match pointer {
                "team" => cross_installation["team"]["id"] = json!("T2"),
                "app" => cross_installation["api_app_id"] = json!("A2"),
                "channel" => {
                    cross_installation["channel"]["id"] = json!("C2");
                    cross_installation["container"]["channel_id"] = json!("C2");
                }
                _ => unreachable!(),
            }
            assert!(
                validate_slack_interaction_installation(&config, &cross_installation).is_err(),
                "cross-installation {pointer} must fail"
            );
        }
    }

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
