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
mod installation;
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
use tandem_channels::traits::{Channel, InteractiveCardReasonPrompt, ThreadReply};
use tandem_types::{
    AccessEffect, AssertionMetadata, AuthorityChain, CreateSessionRequest, DataBoundary,
    HumanActor, MessagePart, MessagePartInput, MessageRole, ModelSpec, OrganizationUnitKind,
    PrincipalRef, RequestPrincipal, ResourceKind, ResourceRef, ResourceScope, SamplingParams,
    SendMessageRequest, StrictTenantContext, TenantContext, VerifiedTenantContext,
};
use tokio_util::sync::CancellationToken;

use crate::app::rate_limit::{ChannelRateLimitKey, ChannelRateLimitKind};
use crate::app::state::principals::channel_identity::{
    channel_bound_tenant, resolve_slack_user_for_connection, ChannelIdentityResolution, ChannelKind,
};
use crate::config::channels::{slack_connections_from_effective_config, ResolvedSlackConnection};
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
    let effective_config = state.config.get_effective_value().await;
    let connections = slack_connections_from_effective_config(&effective_config);
    let all_signing_secrets = slack_signing_secrets(&connections);
    if all_signing_secrets.is_empty() {
        return reject_unauthorized("slack signing secret not configured");
    }

    let signature = headers
        .get("x-slack-signature")
        .and_then(|v| v.to_str().ok());
    let timestamp = headers
        .get("x-slack-request-timestamp")
        .and_then(|v| v.to_str().ok());
    let now = chrono::Utc::now().timestamp();

    // Parse before verifying ONLY to learn which installation the payload
    // claims, so the HMAC binds to that installation's secret. Nothing acts
    // on the payload until its signature verifies.
    let parsed = parse_slack_interaction_body(&body);
    let claimed_team = parsed.as_ref().ok().and_then(|payload| {
        config_string(payload, "/team/id").or_else(|| config_string(payload, "/team_id"))
    });
    let claimed_app = parsed
        .as_ref()
        .ok()
        .and_then(|payload| config_string(payload, "/api_app_id"));
    let signing_secrets = match slack_installation_signing_secrets(
        &connections,
        claimed_team.as_deref(),
        claimed_app.as_deref(),
    ) {
        InstallationSigningSecrets::Bound(secrets) => secrets,
        InstallationSigningSecrets::MissingSecret => {
            // A configured installation without its own secret fails closed:
            // verifying it against another app's secret would let that app
            // forge payloads for this one.
            tracing::warn!(target: "tandem_server::slack_interactions", "rejecting Slack interaction for installation without a signing secret");
            return reject_unauthorized(
                "slack signing secret not configured for this installation",
            );
        }
        InstallationSigningSecrets::Unclaimed => all_signing_secrets,
    };
    if let Err(error) =
        verify_slack_signature_any(&body, signature, timestamp, &signing_secrets, now)
    {
        tracing::warn!(target: "tandem_server::slack_interactions", %error, "rejecting unsigned/forged Slack interaction");
        return reject_unauthorized(&error);
    }

    let payload = match parsed {
        Ok(payload) => payload,
        Err(reason) => return reject_bad_request(&reason),
    };

    // Modal submissions (the rework-reason round-trip) carry no `actions`
    // array or channel container — route them to their own handler.
    if payload.get("type").and_then(Value::as_str) == Some("view_submission") {
        return handle_slack_view_submission(&state, &connections, &payload).await;
    }

    let (installation, connection) = match validate_slack_interaction_installation(
        &connections,
        &payload,
    ) {
        Ok(matched) => matched,
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

    // CRITICAL: Authorize the user against the connection's allowlist,
    // capability tier, step-up, rate limit, and department binding BEFORE
    // dispatching anything.
    let approval_identity =
        match authorize_slack_approver(&state, &connection, &installation, &action.user_id).await {
            Ok(identity) => identity,
            Err(response) => return response,
        };

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

    let tenant_context =
        match guard_slack_run_tenant(&state, &connection, &approval_identity, &run_id).await {
            Ok(tenant_context) => tenant_context,
            Err(response) => return response,
        };

    // Rework requires a reason, which Slack collects via a modal round-trip:
    // open the reason modal bound to this run; the decision dispatches when
    // the `view_submission` callback arrives. The click itself must still be
    // acked within Slack's 3-second window, and `trigger_id`s expire fast, so
    // the modal opens before the ack.
    if decision == "rework" {
        let Some(trigger_id) = config_string(&payload, "/trigger_id") else {
            return reject_bad_request("rework requires a trigger_id to open the reason modal");
        };
        let private_metadata = json!({
            "automation_v2_run_id": run_id,
            "channel_id": connection.channel_id,
        })
        .to_string();
        let modal = tandem_channels::slack_blocks::build_rework_modal_payload(
            &InteractiveCardReasonPrompt::default_rework(),
            &trigger_id,
            SLACK_REWORK_CALLBACK_ID,
            &private_metadata,
        );
        let Some(channel) = slack_channel_for_connection(&connection) else {
            return reject_forbidden("slack bot token not configured");
        };
        if let Err(error) = channel.open_view(&modal).await {
            tracing::warn!(
                target: "tandem_server::slack_interactions",
                run_id = %run_id,
                %error,
                "failed to open Slack rework modal"
            );
            return ok_with_payload(json!({
                "ok": false,
                "error": "could not open the rework reason modal; click Rework again",
            }));
        }
        tracing::info!(
            target: "tandem_server::slack_interactions",
            run_id = %run_id,
            user = %action.user_id,
            "opened Slack rework reason modal"
        );
        return ok_empty();
    }

    let input = crate::http::routines_automations::AutomationV2GateDecisionInput {
        decision: decision.to_string(),
        reason: None,
        approval_request_id: None,
        transition_id: None,
    };
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

/// `callback_id` of the rework-reason modal. Versioned so a future layout
/// change can coexist with in-flight modals.
const SLACK_REWORK_CALLBACK_ID: &str = "tandem_rework_v1";

/// Shared approver authorization for button clicks and modal submissions:
/// connection allowlist → Approve capability (GOV-B5a) → step-up (GOV-B5b) →
/// rate limit → department binding (TAN-764). Returns the installation-scoped
/// approval identity, or the rejection response to return as-is.
async fn authorize_slack_approver(
    state: &AppState,
    connection: &ResolvedSlackConnection,
    installation: &SlackInstallationBinding,
    surface_user_id: &str,
) -> Result<String, Response> {
    let resolved_principal = match resolve_slack_user_for_connection(
        &connection.allowed_users,
        &installation.team_id,
        &installation.app_id,
        surface_user_id,
    ) {
        ChannelIdentityResolution::Resolved(principal) => principal,
        ChannelIdentityResolution::Denied { .. } => {
            tracing::warn!(
                target: "tandem_server::slack_interactions",
                user_id = %surface_user_id,
                "rejecting Slack interaction from unauthorized user"
            );
            return Err(reject_forbidden("user not in allowed_users"));
        }
        ChannelIdentityResolution::ChannelNotConfigured(_) => {
            return Err(reject_bad_request("slack channel not properly configured"));
        }
    };
    let approval_identity = resolved_principal.actor_id.unwrap_or_else(|| {
        slack_installation_identity(&installation.team_id, &installation.app_id, surface_user_id)
    });
    let profile = connection.security_profile;
    let bound_tenant = connection.bound_tenant();
    if !state
        .channel_user_can_approve(
            ChannelKind::Slack.as_str(),
            &approval_identity,
            profile,
            connection.is_open_to_all(),
            bound_tenant
                .as_ref()
                .map(|(org_id, workspace_id)| (org_id.as_str(), workspace_id.as_str())),
        )
        .await
    {
        tracing::warn!(
            target: "tandem_server::slack_interactions",
            user_id = %surface_user_id,
            "rejecting Slack interaction without approval capability"
        );
        return Err(reject_forbidden("user lacks approval capability"));
    }
    // GOV-B5b: on a channel that opts into step-up, an approval requires an active
    // per-identity step-up grant issued out-of-band by the control panel.
    if connection.require_approval_step_up
        && !state
            .channel_step_up_active(ChannelKind::Slack.as_str(), &approval_identity)
            .await
    {
        tracing::warn!(
            target: "tandem_server::slack_interactions",
            user_id = %surface_user_id,
            "rejecting Slack interaction without an active step-up"
        );
        return Err(reject_forbidden("step-up required"));
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
        return Err(reject_rate_limited(rate_decision.retry_after_secs));
    }

    // TAN-764: on a department-bound connection, approval authority must not
    // exceed the channel's departmental scope — the approver needs an active
    // membership in one of the bound units. A department binding without a
    // tenant binding is a misconfiguration and fails closed (there is no
    // authority graph to resolve memberships against).
    if connection.binds_departments() {
        let Some((org_id, workspace_id)) = connection.bound_tenant() else {
            tracing::warn!(
                target: "tandem_server::slack_interactions",
                user_id = %surface_user_id,
                "rejecting Slack interaction: connection binds org units without a bound tenant"
            );
            return Err(reject_forbidden(
                "channel binds org units but has no bound tenant; failing closed",
            ));
        };
        let channel_tenant = TenantContext::explicit(org_id, workspace_id, None);
        let graph = state
            .build_intra_tenant_authority_graph(&channel_tenant, Vec::new())
            .await;
        let principal = PrincipalRef::human_user(approval_identity.clone());
        let now_ms = crate::now_ms();
        let resolved_units = graph.resolved_unit_principals(&principal, now_ms);
        let holds_bound_unit = graph.units.iter().any(|unit| {
            unit.state.is_active()
                && resolved_units.contains(&unit.principal_ref())
                && connection.binds_org_unit(&unit.principal_ref().id, &unit.unit_id)
        });
        if !holds_bound_unit {
            tracing::warn!(
                target: "tandem_server::slack_interactions",
                user_id = %surface_user_id,
                "rejecting Slack interaction outside the channel's bound departments"
            );
            return Err(reject_forbidden(
                "user has no membership in the channel's bound departments",
            ));
        }
    }
    Ok(approval_identity)
}

/// GOV-B5c: if this connection is bound to a tenant, refuse to act on a run
/// that belongs to a different tenant (prevents a channel acting cross-tenant
/// by run id). An unbound connection (single-tenant/local) is unaffected.
async fn guard_slack_run_tenant(
    state: &AppState,
    connection: &ResolvedSlackConnection,
    approval_identity: &str,
    run_id: &str,
) -> Result<TenantContext, Response> {
    let tenant_context = state
        .get_automation_v2_run(run_id)
        .await
        .map(|run| run.tenant_context)
        .unwrap_or_else(tandem_types::TenantContext::local_implicit);
    if let Some((org_id, workspace_id)) = connection.bound_tenant() {
        if tenant_context.org_id != org_id || tenant_context.workspace_id != workspace_id {
            tracing::warn!(
                target: "tandem_server::slack_interactions",
                run_id = %run_id,
                "rejecting Slack interaction targeting a run outside the channel's bound tenant"
            );
            let channel_tenant = tandem_types::TenantContext::explicit_user_workspace(
                org_id,
                workspace_id,
                None,
                "slack",
            );
            if let Err(error) = crate::http::channel_interaction_audit::append_cross_tenant_denial(
                state,
                "slack",
                approval_identity,
                run_id,
                channel_tenant,
                &tenant_context,
            )
            .await
            {
                return Err(reject_forbidden(&format!(
                    "channel denied; required denial receipt persistence failed: {error}"
                )));
            }
            return Err(reject_forbidden("channel not bound to this run's tenant"));
        }
    }
    Ok(tenant_context)
}

/// Build the outbound Slack client for a connection. `None` when the
/// connection has no bot token.
fn slack_channel_for_connection(connection: &ResolvedSlackConnection) -> Option<SlackChannel> {
    let slack_config = SlackConfig {
        bot_token: connection.bot_token.clone()?,
        channel_id: connection.channel_id.clone(),
        allowed_users: connection.allowed_users.clone(),
        mention_only: connection.mention_only,
        security_profile: connection.security_profile,
    };
    Some(match connection.api_base_url.clone() {
        Some(api_base_url) => SlackChannel::new_with_api_base_url(slack_config, api_base_url),
        None => SlackChannel::new(slack_config),
    })
}

/// Handle a `view_submission` callback — today only the rework-reason modal.
/// The submission carries no channel container; the connection binding rides
/// in `private_metadata`, which this server set when it opened the modal and
/// which Slack echoes back inside the signed payload.
async fn handle_slack_view_submission(
    state: &AppState,
    connections: &[ResolvedSlackConnection],
    payload: &Value,
) -> Response {
    if config_string(payload, "/view/callback_id").as_deref() != Some(SLACK_REWORK_CALLBACK_ID) {
        // Unknown modal — ack so Slack closes it rather than erroring the UI.
        return ok_empty();
    }

    let Some(payload_team_id) =
        config_string(payload, "/team/id").or_else(|| config_string(payload, "/team_id"))
    else {
        return reject_bad_request("Slack interaction payload missing team id");
    };
    let Some(payload_app_id) = config_string(payload, "/api_app_id") else {
        return reject_bad_request("Slack interaction payload missing api_app_id");
    };
    let Some(user_id) = config_string(payload, "/user/id") else {
        return reject_bad_request("payload missing user identification");
    };

    let metadata: Value = config_string(payload, "/view/private_metadata")
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    let Some(run_id) = config_string(&metadata, "/automation_v2_run_id") else {
        return reject_bad_request("rework submission missing automation_v2_run_id");
    };
    let Some(channel_id) = config_string(&metadata, "/channel_id") else {
        return reject_bad_request("rework submission missing channel binding");
    };

    let Some(connection) = connections.iter().find(|connection| {
        connection.team_id.as_deref() == Some(payload_team_id.as_str())
            && connection.app_id.as_deref() == Some(payload_app_id.as_str())
            && connection.channel_id == channel_id
    }) else {
        tracing::warn!(target: "tandem_server::slack_interactions", "rejecting Slack view submission outside configured installation");
        return reject_forbidden("Slack interaction channel does not match configured channel");
    };
    let installation = SlackInstallationBinding {
        team_id: payload_team_id,
        app_id: payload_app_id,
    };

    let approval_identity =
        match authorize_slack_approver(state, connection, &installation, &user_id).await {
            Ok(identity) => identity,
            Err(response) => return response,
        };

    let reason = config_string(
        payload,
        "/view/state/values/reason_block/reason_input/value",
    )
    .map(|value| value.trim().to_string())
    .filter(|value| !value.is_empty());
    let Some(reason) = reason else {
        // Slack renders this exact shape as inline validation on the modal.
        return ok_with_payload(json!({
            "response_action": "errors",
            "errors": {
                "reason_block": "Add the feedback the workflow should use."
            }
        }));
    };

    let tenant_context =
        match guard_slack_run_tenant(state, connection, &approval_identity, &run_id).await {
            Ok(tenant_context) => tenant_context,
            Err(response) => return response,
        };

    // Dedup double-submits by Slack view id — recorded only NOW, once the
    // submission is fully valid and about to dispatch. Recording earlier
    // would burn the id on a validation error (Slack keeps the same modal
    // open for `response_action: errors`), so the user's corrected resubmit
    // would be dropped as a duplicate. Gate decisions remain the durable
    // idempotency boundary after entries expire.
    if let Some(view_id) = config_string(payload, "/view/id") {
        let key = format!("view_submission:{view_id}");
        let mut guard = dedup_ring().lock().expect("dedup mutex poisoned");
        if !guard.record_new(&key) {
            tracing::debug!(target: "tandem_server::slack_interactions", %key, "duplicate Slack view submission — already processed");
            return ok_empty();
        }
    }

    let input = crate::http::routines_automations::AutomationV2GateDecisionInput {
        decision: "rework".to_string(),
        reason: Some(reason),
        approval_request_id: None,
        transition_id: None,
    };
    let decider = crate::automation_v2::governance::GovernanceActorRef::human(
        Some(approval_identity.clone()),
        "slack",
    );
    let result = crate::http::routines_automations::automations_v2_run_gate_decide_inner(
        state.clone(),
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
                user = %user_id,
                "Slack rework modal decided gate"
            );
            // Empty 200 closes the modal.
            ok_empty()
        }
        Err((status, body_json)) => {
            tracing::warn!(
                target: "tandem_server::slack_interactions",
                run_id = %run_id,
                status = %status,
                body = %body_json.0,
                "rework gate-decide returned non-success"
            );
            // Returning >200 would make Slack show a generic modal error and
            // invite retries of a decision that already resolved; surface the
            // conflict inline instead.
            ok_with_payload(json!({
                "response_action": "errors",
                "errors": {
                    "reason_block": "This gate was already decided or could not accept rework."
                }
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

/// Percent-decoding collects raw BYTES first and UTF-8 decodes at the end:
/// Slack form-encodes UTF-8 JSON, so a multi-byte sequence (emoji, accents,
/// CJK — e.g. a free-form rework reason) arrives as several `%xx` escapes
/// that only mean anything reassembled. Decoding each escape as its own
/// `char` would produce mojibake in persisted gate decisions.
fn url_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = hex_digit(bytes[i + 1]);
                let lo = hex_digit(bytes[i + 2]);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push(hi << 4 | lo);
                    i += 3;
                } else {
                    out.push(b'%');
                    i += 1;
                }
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
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

use installation::{
    slack_event_envelope_team_id, slack_installation_signing_secrets, slack_signing_secrets,
    validate_slack_event_installation, validate_slack_interaction_installation,
    verify_slack_signature_any, InstallationSigningSecrets,
};

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
    let connections = slack_connections_from_effective_config(&effective_config);
    let all_signing_secrets = slack_signing_secrets(&connections);
    if all_signing_secrets.is_empty() {
        audit_slack_denial(
            &state,
            &effective_config,
            None,
            None,
            "slack signing secret not configured",
            json!({}),
        )
        .await;
        return reject_forbidden("slack signing secret not configured");
    }
    let signature = headers
        .get("x-slack-signature")
        .and_then(|v| v.to_str().ok());
    let timestamp = headers
        .get("x-slack-request-timestamp")
        .and_then(|v| v.to_str().ok());
    let now = chrono::Utc::now().timestamp();

    // Parse before verifying ONLY to learn which installation the payload
    // claims, so the HMAC binds to that installation's secret (a payload for
    // connection B must never be admitted on connection A's secret). Nothing
    // acts on the payload until its signature verifies.
    let payload: Option<Value> = serde_json::from_slice(&body).ok();
    let claimed_team = payload
        .as_ref()
        .and_then(|payload| slack_event_envelope_team_id(payload).ok());
    let claimed_app = payload
        .as_ref()
        .and_then(|payload| config_string(payload, "/api_app_id"));
    let signing_secrets = match slack_installation_signing_secrets(
        &connections,
        claimed_team.as_deref(),
        claimed_app.as_deref(),
    ) {
        InstallationSigningSecrets::Bound(secrets) => secrets,
        InstallationSigningSecrets::MissingSecret => {
            // A configured installation without its own secret fails closed:
            // verifying it against another app's secret would let that app
            // forge events for this one.
            tracing::warn!(target: "tandem_server::slack_events", "rejecting Slack event for installation without a signing secret");
            audit_slack_denial(
                &state,
                &effective_config,
                None,
                None,
                "slack signing secret not configured for this installation",
                json!({
                    "team_id": claimed_team,
                    "api_app_id": claimed_app,
                }),
            )
            .await;
            return reject_forbidden("slack signing secret not configured for this installation");
        }
        // No matching installation (or a payload without one, e.g. the
        // url_verification handshake): any configured app's secret may vouch
        // for the request; installation validation still rejects mismatches
        // downstream before anything runs.
        InstallationSigningSecrets::Unclaimed => all_signing_secrets,
    };
    if let Err(error) =
        verify_slack_signature_any(&body, signature, timestamp, &signing_secrets, now)
    {
        tracing::warn!(target: "tandem_server::slack_events", %error, "rejecting unsigned/forged Slack event");
        audit_slack_denial(
            &state,
            &effective_config,
            None,
            None,
            "Slack event signature verification failed",
            json!({ "request_timestamp": timestamp }),
        )
        .await;
        return reject_forbidden(&error);
    }

    let Some(payload) = payload else {
        audit_slack_denial(
            &state,
            &effective_config,
            None,
            None,
            "invalid Slack event JSON",
            json!({}),
        )
        .await;
        return reject_bad_request("invalid Slack event JSON");
    };

    match payload.get("type").and_then(Value::as_str) {
        // Slack setup handshake: echo the challenge (signature already verified).
        Some("url_verification") => {
            let events_enabled_anywhere = connections
                .iter()
                .any(|connection| connection.events_enabled);
            if !events_enabled_anywhere {
                audit_slack_denial(
                    &state,
                    &effective_config,
                    None,
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
    let connections = slack_connections_from_effective_config(effective_config);
    if !connections
        .iter()
        .any(|connection| connection.events_enabled)
    {
        audit_slack_denial(
            state,
            effective_config,
            None,
            None,
            "slack events ingress not enabled",
            json!({}),
        )
        .await;
        return reject_forbidden("slack events ingress not enabled");
    }
    let installation = match validate_slack_event_installation(&connections, payload) {
        Ok(installation) => installation,
        Err(reason) => {
            tracing::warn!(target: "tandem_server::slack_events", %reason, "rejecting Slack event outside configured installation");
            audit_slack_denial(
                state,
                effective_config,
                None,
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
                None,
                reason,
                json!({"event_id": payload.get("event_id")}),
            )
            .await;
            return reject_bad_request(reason);
        }
    };

    // Route the event to the connection bound to its `(team, app, channel)`.
    let installation_connections = connections
        .iter()
        .filter(|connection| {
            connection.team_id.as_deref() == Some(installation.team_id.as_str())
                && connection.app_id.as_deref() == Some(installation.app_id.as_str())
        })
        .collect::<Vec<_>>();
    if installation_connections
        .iter()
        .all(|connection| connection.channel_id.is_empty())
    {
        audit_slack_denial(
            state,
            &effective_config,
            None,
            None,
            "slack channel id not configured",
            json!({"event_id": event.event_id}),
        )
        .await;
        return reject_forbidden("slack channel id not configured");
    }
    let Some(connection) = installation_connections
        .into_iter()
        .find(|connection| connection.channel_id == event.channel_id)
        .cloned()
    else {
        tracing::warn!(target: "tandem_server::slack_events", channel_id = %event.channel_id, "rejecting Slack event outside configured channel");
        audit_slack_denial(
            state,
            &effective_config,
            None,
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
    };
    if !connection.events_enabled {
        audit_slack_denial(
            state,
            &effective_config,
            Some(&connection),
            None,
            "slack events ingress not enabled",
            json!({
                "event_id": event.event_id,
                "slack_channel_id": event.channel_id,
            }),
        )
        .await;
        return reject_forbidden("slack events ingress not enabled");
    }
    if connection.bot_token.is_none() {
        audit_slack_denial(
            state,
            &effective_config,
            Some(&connection),
            None,
            "slack bot token not configured",
            json!({"event_id": event.event_id}),
        )
        .await;
        return reject_forbidden("slack bot token not configured");
    }
    if connection.mention_only
        && event.event_type != "app_mention"
        && event.channel_type.as_deref() != Some("im")
    {
        return ok_empty();
    }

    let request_principal = match resolve_slack_user_for_connection(
        &connection.allowed_users,
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
                Some(&connection),
                None,
                "user not in allowed_users",
                json!({
                    "event_id": event.event_id,
                    "slack_user_id": event.user_id,
                    "slack_channel_id": event.channel_id,
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
                Some(&connection),
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
        &connection,
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
                Some(&connection),
                actor_id,
                &reason,
                json!({
                    "event_id": event.event_id,
                    "slack_user_id": event.user_id,
                    "slack_channel_id": event.channel_id,
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
                Some(&connection),
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
                Some(&connection),
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
    let task_connection = connection.clone();
    let task_claim = claim.clone();
    let spawn_result = state
        .slack_event_tasks
        .spawn(move |cancel| async move {
            run_claimed_slack_event(
                task_state,
                task_connection,
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

/// Tenant attribution for a denial audit. The matched connection's bound
/// tenant wins, then the legacy top-level binding; when neither exists but
/// every tenant-bound connection agrees on a single tenant, that unambiguous
/// binding is used so configs that carry `tenant` only on `connections[]`
/// entries still record pre-routing denials (signature failures, unclaimed
/// channels). Denials stay unwritten only when tenant attribution would be a
/// guess between distinct tenants.
fn slack_denial_tenant_context(
    effective_config: &Value,
    connection: Option<&ResolvedSlackConnection>,
    actor_id: Option<String>,
) -> Option<TenantContext> {
    if let Some((org_id, workspace_id)) = connection.and_then(ResolvedSlackConnection::bound_tenant)
    {
        let mut tenant = TenantContext::explicit(org_id, workspace_id, actor_id);
        tenant.deployment_id = connection.and_then(|c| c.tenant_deployment_id.clone());
        return Some(tenant);
    }
    if let Some(tenant) = configured_slack_tenant_context(effective_config, actor_id.clone()) {
        return Some(tenant);
    }
    let mut bindings = slack_connections_from_effective_config(effective_config)
        .iter()
        .filter_map(|connection| {
            connection.bound_tenant().map(|(org_id, workspace_id)| {
                (
                    org_id,
                    workspace_id,
                    connection.tenant_deployment_id.clone(),
                )
            })
        })
        .collect::<Vec<_>>();
    bindings.sort();
    bindings.dedup();
    let [(org_id, workspace_id, deployment_id)] = bindings.as_slice() else {
        return None;
    };
    let mut tenant = TenantContext::explicit(org_id.clone(), workspace_id.clone(), actor_id);
    tenant.deployment_id = deployment_id.clone();
    Some(tenant)
}

async fn audit_slack_denial(
    state: &AppState,
    effective_config: &Value,
    connection: Option<&ResolvedSlackConnection>,
    actor_id: Option<String>,
    reason: &str,
    details: Value,
) {
    let Some(tenant_context) =
        slack_denial_tenant_context(effective_config, connection, actor_id.clone())
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
    fn url_decode_reassembles_multibyte_utf8() {
        // Percent escapes are UTF-8 bytes, not chars: emoji/CJK/accents in a
        // free-form rework reason must round-trip without mojibake.
        assert_eq!(url_decode("%F0%9F%94%A5+ship+it"), "🔥 ship it");
        assert_eq!(url_decode("%E4%B8%AD%E6%96%87"), "中文");
        assert_eq!(url_decode("caf%C3%A9"), "café");
        assert_eq!(url_decode("plain+ascii%21"), "plain ascii!");
        // Malformed escapes stay literal instead of corrupting the rest.
        assert_eq!(url_decode("100%+done"), "100% done");
    }

    #[test]
    fn parse_slack_interaction_body_preserves_unicode_payload_values() {
        let payload = json!({"type": "view_submission", "reason": "需要修改 🔥"}).to_string();
        let mut encoded = String::from("payload=");
        for byte in payload.as_bytes() {
            encoded.push_str(&format!("%{byte:02X}"));
        }
        let parsed = parse_slack_interaction_body(encoded.as_bytes()).expect("parse");
        assert_eq!(
            parsed.get("reason").and_then(Value::as_str),
            Some("需要修改 🔥")
        );
    }

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
        let connections = slack_connections_from_effective_config(&config);
        let payload = json!({ "team_id": "T1", "api_app_id": "A1" });
        assert_eq!(
            validate_slack_event_installation(&connections, &payload).unwrap(),
            SlackInstallationBinding {
                team_id: "T1".to_string(),
                app_id: "A1".to_string(),
            }
        );

        let wrong_team = json!({ "team_id": "T2", "api_app_id": "A1" });
        assert!(validate_slack_event_installation(&connections, &wrong_team).is_err());
        let wrong_app = json!({ "team_id": "T1", "api_app_id": "A2" });
        assert!(validate_slack_event_installation(&connections, &wrong_app).is_err());
        assert!(validate_slack_event_installation(&connections, &json!({})).is_err());
    }

    #[test]
    fn slack_interactions_require_matching_team_app_and_channel() {
        let config = json!({
            "channels": {
                "slack": { "team_id": "T1", "app_id": "A1", "channel_id": "C1" }
            }
        });
        let connections = slack_connections_from_effective_config(&config);
        let payload = json!({
            "team": { "id": "T1" },
            "api_app_id": "A1",
            "channel": { "id": "C1" },
            "container": { "channel_id": "C1" }
        });
        assert!(validate_slack_interaction_installation(&connections, &payload).is_ok());

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
                validate_slack_interaction_installation(&connections, &cross_installation).is_err(),
                "cross-installation {pointer} must fail"
            );
        }
    }

    #[test]
    fn slack_interaction_installation_resolves_per_connection() {
        let config = json!({
            "channels": {
                "slack": {
                    "team_id": "T1",
                    "app_id": "A1",
                    "allowed_users": ["U_SHARED"],
                    "connections": [
                        { "channel_id": "C_SALES", "allowed_users": ["U_SALES"] },
                        { "channel_id": "C_ENG" }
                    ]
                }
            }
        });
        let connections = slack_connections_from_effective_config(&config);
        let payload = json!({
            "team": { "id": "T1" },
            "api_app_id": "A1",
            "channel": { "id": "C_SALES" }
        });
        let (installation, connection) =
            validate_slack_interaction_installation(&connections, &payload).unwrap();
        assert_eq!(installation.team_id, "T1");
        assert_eq!(connection.channel_id, "C_SALES");
        assert_eq!(connection.allowed_users, vec!["U_SALES".to_string()]);

        let eng_payload = json!({
            "team": { "id": "T1" },
            "api_app_id": "A1",
            "channel": { "id": "C_ENG" }
        });
        let (_, eng_connection) =
            validate_slack_interaction_installation(&connections, &eng_payload).unwrap();
        assert_eq!(
            eng_connection.allowed_users,
            vec!["U_SHARED".to_string()],
            "connections without their own allowlist inherit the top-level one"
        );
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
