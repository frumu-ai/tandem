//! Telegram callback_query webhook handler.
//!
//! Telegram POSTs an `Update` object here whenever a user taps an inline
//! keyboard button. The `Update.callback_query` field carries the
//! `callback_data` we built via `tandem_channels::telegram_keyboards`.
//!
//! Hard requirements:
//! - Verify `x-telegram-bot-api-secret-token` against the configured
//!   `webhook_secret_token` on every request via
//!   `tandem_channels::signing::verify_telegram_secret_token`.
//! - Acknowledge the callback fast — the Telegram client shows a loading
//!   spinner on the user's tapped button until the bot calls
//!   `answerCallbackQuery`. We respond 200 within milliseconds; the bot
//!   library calls answerCallbackQuery in the background once the gate
//!   decision lands.
//! - Idempotent on retries by `update_id` (Telegram retries when our 200 is
//!   slow or absent).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use tandem_channels::signing::verify_telegram_secret_token;
use tandem_channels::telegram_keyboards::{parse_callback_data, ParsedCallbackData};

use crate::app::rate_limit::{ChannelRateLimitKey, ChannelRateLimitKind};
use crate::app::state::channel_user_capabilities::{
    channel_requires_approval_step_up, channel_security_profile_from_config,
};
use crate::app::state::principals::channel_identity::{
    channel_bound_tenant, channel_is_open_to_all, resolve_channel_user, ChannelIdentityResolution,
    ChannelKind,
};
use crate::AppState;

const DEDUP_CAP: usize = 4096;
const DEDUP_TTL_SECS: u64 = 300; // 5 minutes — Telegram retries within minutes
const PENDING_REWORK_TTL_SECS: u64 = 10 * 60;
const TELEGRAM_API: &str = "https://api.telegram.org/bot";

static SEEN_UPDATES: OnceLock<Mutex<DedupRing>> = OnceLock::new();
static PENDING_REWORK: OnceLock<Mutex<HashMap<String, PendingTelegramRework>>> = OnceLock::new();

fn dedup_ring() -> &'static Mutex<DedupRing> {
    SEEN_UPDATES.get_or_init(|| Mutex::new(DedupRing::new()))
}

fn pending_rework() -> &'static Mutex<HashMap<String, PendingTelegramRework>> {
    PENDING_REWORK.get_or_init(|| Mutex::new(HashMap::new()))
}

#[derive(Debug, Clone)]
struct PendingTelegramRework {
    run_id: String,
    node_id: String,
    prompt_message_id: Option<i64>,
    inserted_at_secs: u64,
}

impl PendingTelegramRework {
    fn is_expired(&self, now_secs: u64) -> bool {
        now_secs.saturating_sub(self.inserted_at_secs) > PENDING_REWORK_TTL_SECS
    }
}

struct DedupEntry {
    inserted_at_secs: u64,
}

struct DedupRing {
    set: std::collections::HashMap<i64, DedupEntry>,
    order: std::collections::VecDeque<i64>,
}

impl DedupRing {
    fn new() -> Self {
        Self {
            set: std::collections::HashMap::with_capacity(DEDUP_CAP),
            order: std::collections::VecDeque::with_capacity(DEDUP_CAP),
        }
    }

    fn record_new(&mut self, key: i64) -> bool {
        let now_secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Check if key exists and hasn't expired.
        if let Some(entry) = self.set.get(&key) {
            if now_secs.saturating_sub(entry.inserted_at_secs) < DEDUP_TTL_SECS {
                return false; // Duplicate within TTL window.
            }
            // Entry exists but expired; will be reinserted below.
            self.set.remove(&key);
        }

        // Evict oldest entry if at capacity.
        if self.order.len() >= DEDUP_CAP {
            if let Some(oldest) = self.order.pop_front() {
                self.set.remove(&oldest);
            }
        }

        self.set.insert(
            key,
            DedupEntry {
                inserted_at_secs: now_secs,
            },
        );
        self.order.push_back(key);
        true
    }
}

/// Telegram interaction handler. Wired at
/// `POST /channels/telegram/interactions`.
pub(crate) async fn telegram_interactions(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let secret = match read_telegram_secret(&state).await {
        Some(s) => s,
        None => return reject_unauthorized("telegram webhook secret not configured"),
    };
    let header_value = headers
        .get("x-telegram-bot-api-secret-token")
        .and_then(|v| v.to_str().ok());
    if let Err(error) = verify_telegram_secret_token(header_value, &secret) {
        tracing::warn!(
            target: "tandem_server::telegram_interactions",
            ?error,
            "rejecting Telegram update with bad/missing secret token"
        );
        return reject_unauthorized(&error.to_string());
    }

    let update: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(err) => return reject_bad_request(&format!("update is not JSON: {err}")),
    };

    if let Some(update_id) = update.get("update_id").and_then(Value::as_i64) {
        let mut guard = dedup_ring().lock().expect("dedup mutex poisoned");
        if !guard.record_new(update_id) {
            tracing::debug!(
                target: "tandem_server::telegram_interactions",
                update_id,
                "duplicate Telegram update — already processed"
            );
            return ok_empty();
        }
    }

    if let Some(message) = update.get("message") {
        return handle_message_update(state, message).await;
    }

    let Some(callback_query) = update.get("callback_query") else {
        return ok_empty();
    };

    let callback_data = match callback_query.get("data").and_then(Value::as_str) {
        Some(d) => d,
        None => return reject_bad_request("callback_query missing data"),
    };

    let mut parsed = match parse_callback_data(callback_data) {
        Some(p) => p,
        None => return reject_bad_request(&format!("unrecognized callback_data: {callback_data}")),
    };

    if parsed.was_truncated {
        tracing::warn!(
            target: "tandem_server::telegram_interactions",
            "legacy truncated callback_data refused"
        );
        return reject_bad_request("callback identifier truncated and could not be resolved");
    }

    if let Some(callback_id) = parsed.callback_id.as_deref() {
        match resolve_callback_token(callback_id).await {
            Some(record) => {
                parsed.run_id = record.run_id;
                parsed.node_id = record.node_id.unwrap_or_default();
            }
            None => {
                tracing::warn!(
                    target: "tandem_server::telegram_interactions",
                    callback_id,
                    "Telegram callback token did not resolve"
                );
                return reject_bad_request("callback identifier not found");
            }
        }
    }

    let user_id = match callback_query
        .pointer("/from/id")
        .and_then(Value::as_i64)
        .map(|id| id.to_string())
    {
        Some(id) => id,
        None => return reject_bad_request("callback_query missing user identification"),
    };

    // CRITICAL: Authorize the user against the allowlist BEFORE dispatching.
    let effective_config = state.config.get_effective_value().await;
    match resolve_channel_user(&effective_config, ChannelKind::Telegram, &user_id) {
        ChannelIdentityResolution::Resolved(_principal) => {
            // User is authorized; proceed to handle the action.
        }
        ChannelIdentityResolution::Denied { .. } => {
            tracing::warn!(
                target: "tandem_server::telegram_interactions",
                user_id = %user_id,
                "rejecting Telegram interaction from unauthorized user"
            );
            return reject_forbidden("user not in allowed_users");
        }
        ChannelIdentityResolution::ChannelNotConfigured(_) => {
            return reject_bad_request("telegram channel not properly configured");
        }
    }
    let profile =
        channel_security_profile_from_config(&effective_config, ChannelKind::Telegram.as_str());
    if !state
        .channel_user_can_approve(
            ChannelKind::Telegram.as_str(),
            &user_id,
            profile,
            channel_is_open_to_all(&effective_config, ChannelKind::Telegram),
        )
        .await
    {
        tracing::warn!(
            target: "tandem_server::telegram_interactions",
            user_id = %user_id,
            "rejecting Telegram interaction without approval capability"
        );
        return reject_forbidden("user lacks approval capability");
    }
    // GOV-B5b: on a channel that opts into step-up, an approval requires an active
    // per-identity step-up grant issued out-of-band by the control panel.
    if channel_requires_approval_step_up(&effective_config, ChannelKind::Telegram.as_str())
        && !state
            .channel_step_up_active(ChannelKind::Telegram.as_str(), &user_id)
            .await
    {
        tracing::warn!(
            target: "tandem_server::telegram_interactions",
            user_id = %user_id,
            "rejecting Telegram interaction without an active step-up"
        );
        return reject_forbidden("step-up required");
    }
    let rate_key = ChannelRateLimitKey {
        channel: ChannelKind::Telegram.as_str().to_string(),
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
            let chat_id = callback_query
                .pointer("/message/chat/id")
                .and_then(Value::as_i64)
                .map(|id| id.to_string())
                .or_else(|| {
                    callback_query
                        .pointer("/message/chat/id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            if let Some(chat_id) = chat_id {
                let prompt_message_id =
                    send_rework_force_reply(&state, &chat_id, &user_id, &parsed).await;
                record_pending_rework(
                    &chat_id,
                    &user_id,
                    PendingTelegramRework {
                        run_id: parsed.run_id.clone(),
                        node_id: parsed.node_id.clone(),
                        prompt_message_id,
                        inserted_at_secs: now_secs(),
                    },
                );
            } else {
                tracing::warn!(
                    target: "tandem_server::telegram_interactions",
                    run_id = %parsed.run_id,
                    "Telegram rework callback missing chat id"
                );
            }
            ok_empty()
        }
        other => reject_bad_request(&format!("unknown action: {other}")),
    }
}

async fn handle_message_update(state: AppState, message: &Value) -> Response {
    let user_id = match message
        .pointer("/from/id")
        .and_then(Value::as_i64)
        .map(|id| id.to_string())
    {
        Some(id) => id,
        None => return ok_empty(),
    };
    let chat_id = message
        .pointer("/chat/id")
        .and_then(Value::as_i64)
        .map(|id| id.to_string())
        .or_else(|| {
            message
                .pointer("/chat/id")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let Some(chat_id) = chat_id else {
        return ok_empty();
    };

    let Some(pending) = take_pending_rework(&chat_id, &user_id, message) else {
        return ok_empty();
    };
    let reason = match message.get("text").and_then(Value::as_str) {
        Some(text) if !text.trim().is_empty() => text.trim().to_string(),
        _ => {
            record_pending_rework(&chat_id, &user_id, pending);
            return ok_empty();
        }
    };

    dispatch_decision(
        state,
        ParsedCallbackData {
            action: "rework".to_string(),
            run_id: pending.run_id,
            node_id: pending.node_id,
            callback_id: None,
            was_truncated: false,
        },
        &user_id,
        Some(reason),
    )
    .await
}

async fn dispatch_decision(
    state: AppState,
    parsed: ParsedCallbackData,
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
    // GOV-B5c: if this channel is bound to a tenant, refuse to act on a run that
    // belongs to a different tenant. An unbound channel (single-tenant/local) is
    // unaffected.
    let effective_config = state.config.get_effective_value().await;
    if let Some((org_id, workspace_id)) =
        channel_bound_tenant(&effective_config, ChannelKind::Telegram)
    {
        if tenant_context.org_id != org_id || tenant_context.workspace_id != workspace_id {
            tracing::warn!(
                target: "tandem_server::telegram_interactions",
                user_id = %user_id,
                "rejecting Telegram interaction targeting a run outside the channel's bound tenant"
            );
            return reject_forbidden("channel not bound to this run's tenant");
        }
    }
    // GOV-B1: caller is verified (secret-token + allowlist + Approve tier);
    // attribute the decision to the Telegram identity as a human approver.
    let decider = crate::automation_v2::governance::GovernanceActorRef::human(
        Some(user_id.to_string()),
        "telegram",
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
                target: "tandem_server::telegram_interactions",
                run_id = %parsed.run_id,
                user = %user_id,
                action = %parsed.action,
                "Telegram interaction decided gate"
            );
            ok_empty()
        }
        Err((status, body)) => {
            tracing::warn!(
                target: "tandem_server::telegram_interactions",
                run_id = %parsed.run_id,
                status = %status,
                body = %body.0,
                "gate-decide returned non-success"
            );
            // Telegram treats non-200 as an error and may retry. Map
            // application-level failures (409 race, etc.) to 200 + log so
            // Telegram doesn't double-fire. The dispatcher's
            // answerCallbackQuery (W5 wiring) will surface the conflict to
            // the user with a brief toast.
            ok_empty()
        }
    }
}

async fn read_telegram_secret(state: &AppState) -> Option<String> {
    let effective = state.config.get_effective_value().await;
    effective
        .pointer("/channels/telegram/webhook_secret_token")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn read_telegram_bot_token(state: &AppState) -> Option<String> {
    let effective = state.config.get_effective_value().await;
    effective
        .pointer("/channels/telegram/bot_token")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn resolve_callback_token(
    callback_id: &str,
) -> Option<crate::app::state::approval_message_map::ApprovalCallbackRecord> {
    let map = crate::app::state::approval_message_map::ApprovalMessageMap::load_or_default(
        crate::config::paths::resolve_approval_message_map_path(),
    )
    .await;
    map.get_telegram_callback(callback_id).await
}

async fn send_rework_force_reply(
    state: &AppState,
    chat_id: &str,
    user_id: &str,
    parsed: &ParsedCallbackData,
) -> Option<i64> {
    let token = read_telegram_bot_token(state).await?;
    let payload = json!({
        "chat_id": chat_id,
        "text": format!("@user{user_id} What should change before this can be approved?"),
        "reply_markup": {
            "force_reply": true,
            "selective": true,
            "input_field_placeholder": "Type your rework feedback...",
        },
    });
    let url = format!("{TELEGRAM_API}{token}/sendMessage");
    let response = match reqwest::Client::new().post(url).json(&payload).send().await {
        Ok(response) => response,
        Err(error) => {
            tracing::warn!(
                target: "tandem_server::telegram_interactions",
                run_id = %parsed.run_id,
                ?error,
                "failed to send Telegram rework force-reply prompt"
            );
            return None;
        }
    };
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    if !status.is_success() {
        tracing::warn!(
            target: "tandem_server::telegram_interactions",
            run_id = %parsed.run_id,
            status = %status,
            "Telegram rework force-reply prompt failed"
        );
        return None;
    }
    serde_json::from_str::<Value>(&body)
        .ok()
        .and_then(|value| value.pointer("/result/message_id").and_then(Value::as_i64))
}

fn pending_key(chat_id: &str, user_id: &str) -> String {
    format!("{}:{}", chat_id.trim(), user_id.trim())
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn record_pending_rework(chat_id: &str, user_id: &str, pending: PendingTelegramRework) {
    let now = now_secs();
    let mut guard = pending_rework()
        .lock()
        .expect("pending rework mutex poisoned");
    guard.retain(|_, value| !value.is_expired(now));
    guard.insert(pending_key(chat_id, user_id), pending);
}

fn take_pending_rework(
    chat_id: &str,
    user_id: &str,
    message: &Value,
) -> Option<PendingTelegramRework> {
    let mut guard = pending_rework()
        .lock()
        .expect("pending rework mutex poisoned");
    let key = pending_key(chat_id, user_id);
    let pending = guard.get(&key)?.clone();
    if pending.is_expired(now_secs()) {
        guard.remove(&key);
        return None;
    }
    if let Some(prompt_message_id) = pending.prompt_message_id {
        let replied_to_prompt = message
            .pointer("/reply_to_message/message_id")
            .and_then(Value::as_i64)
            == Some(prompt_message_id);
        if !replied_to_prompt {
            return None;
        }
    }
    guard.remove(&key)
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

fn ok_empty() -> Response {
    (StatusCode::OK, Json(json!({}))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_ring_returns_false_on_repeat_update_id() {
        let mut ring = DedupRing::new();
        assert!(ring.record_new(100));
        assert!(!ring.record_new(100));
        assert!(ring.record_new(101));
    }

    #[test]
    fn dedup_ring_evicts_oldest_at_cap() {
        let mut ring = DedupRing::new();
        for i in 0..(DEDUP_CAP as i64) {
            ring.record_new(i);
        }
        assert!(!ring.record_new(0));
        ring.record_new(DEDUP_CAP as i64);
        // After overflow, an older entry can be re-inserted.
        assert!(ring.record_new(0));
    }

    /// Sanity check: the callback_data we expect from the renderer parses
    /// cleanly and has the shape the dispatch path relies on.
    #[test]
    fn callback_data_format_round_trips() {
        let raw = "tdm:approve:auto-v2-run-abc:send_email";
        let parsed = parse_callback_data(raw).expect("parses");
        assert_eq!(parsed.action, "approve");
        assert_eq!(parsed.run_id, "auto-v2-run-abc");
        assert_eq!(parsed.node_id, "send_email");
        assert!(!parsed.was_truncated);
    }

    #[test]
    fn pending_rework_requires_reply_to_prompt_when_prompt_id_exists() {
        let pending = PendingTelegramRework {
            run_id: "run-1".to_string(),
            node_id: "send_email".to_string(),
            prompt_message_id: Some(42),
            inserted_at_secs: now_secs(),
        };
        record_pending_rework("123", "456", pending);

        let unrelated = json!({
            "chat": { "id": 123 },
            "from": { "id": 456 },
            "text": "try again"
        });
        assert!(take_pending_rework("123", "456", &unrelated).is_none());

        let reply = json!({
            "chat": { "id": 123 },
            "from": { "id": 456 },
            "text": "try again",
            "reply_to_message": { "message_id": 42 }
        });
        let taken = take_pending_rework("123", "456", &reply).expect("pending rework");
        assert_eq!(taken.run_id, "run-1");
        assert_eq!(taken.node_id, "send_email");
    }

    #[test]
    fn pending_rework_expires() {
        let pending = PendingTelegramRework {
            run_id: "run-expired".to_string(),
            node_id: "send_email".to_string(),
            prompt_message_id: None,
            inserted_at_secs: now_secs().saturating_sub(PENDING_REWORK_TTL_SECS + 1),
        };
        record_pending_rework("expired-chat", "expired-user", pending);

        let message = json!({
            "chat": { "id": "expired-chat" },
            "from": { "id": "expired-user" },
            "text": "try again"
        });
        assert!(take_pending_rework("expired-chat", "expired-user", &message).is_none());
    }
}
