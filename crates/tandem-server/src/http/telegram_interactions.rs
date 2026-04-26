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

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use tandem_channels::signing::verify_telegram_secret_token;
use tandem_channels::telegram_keyboards::{parse_callback_data, ParsedCallbackData};

use crate::AppState;

const DEDUP_CAP: usize = 4096;

static SEEN_UPDATES: OnceLock<Mutex<DedupRing>> = OnceLock::new();

fn dedup_ring() -> &'static Mutex<DedupRing> {
    SEEN_UPDATES.get_or_init(|| Mutex::new(DedupRing::new()))
}

struct DedupRing {
    set: HashSet<i64>,
    order: std::collections::VecDeque<i64>,
}

impl DedupRing {
    fn new() -> Self {
        Self {
            set: HashSet::with_capacity(DEDUP_CAP),
            order: std::collections::VecDeque::with_capacity(DEDUP_CAP),
        }
    }

    fn record_new(&mut self, key: i64) -> bool {
        if self.set.contains(&key) {
            return false;
        }
        if self.order.len() >= DEDUP_CAP {
            if let Some(oldest) = self.order.pop_front() {
                self.set.remove(&oldest);
            }
        }
        self.set.insert(key);
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

    // We only handle callback_query updates here; the dispatcher's listener
    // owns regular `message` updates and the rework `force_reply` capture
    // (W5 wiring).
    let Some(callback_query) = update.get("callback_query") else {
        return ok_empty();
    };

    let callback_data = match callback_query.get("data").and_then(Value::as_str) {
        Some(d) => d,
        None => return reject_bad_request("callback_query missing data"),
    };

    let parsed = match parse_callback_data(callback_data) {
        Some(p) => p,
        None => return reject_bad_request(&format!("unrecognized callback_data: {callback_data}")),
    };

    if parsed.was_truncated {
        // W5 wiring: the dispatcher would resolve the full identifier from a
        // short-lived cache here. For now, refuse rather than dispatch a
        // partial run_id and risk wrong-run mutations.
        tracing::warn!(
            target: "tandem_server::telegram_interactions",
            "callback_data was truncated; full ID resolution lands in W5"
        );
        return reject_bad_request("callback identifier truncated; cache resolution not yet wired");
    }

    let user_id = callback_query
        .pointer("/from/id")
        .and_then(Value::as_i64)
        .map(|id| id.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    match parsed.action.as_str() {
        "approve" | "cancel" => dispatch_decision(state, parsed, &user_id, None).await,
        "rework" => {
            // Telegram has no modal. The dispatcher will capture the user's
            // next message via `force_reply` (built by
            // telegram_keyboards::build_force_reply_for_rework). For now,
            // ack the callback so the loading spinner stops and instruct
            // callers to wire the force-reply state machine in W5.
            tracing::info!(
                target: "tandem_server::telegram_interactions",
                run_id = %parsed.run_id,
                "rework button tapped; force-reply capture lands in W5"
            );
            ok_empty()
        }
        other => reject_bad_request(&format!("unknown action: {other}")),
    }
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
    let result = crate::http::routines_automations::automations_v2_run_gate_decide(
        State(state),
        axum::extract::Path(parsed.run_id.clone()),
        Json(input),
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
}
