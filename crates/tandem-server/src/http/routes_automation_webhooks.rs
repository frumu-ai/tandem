use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::app::state::{
    automation_webhook_body_digest, sanitize_automation_webhook_preview,
    AutomationWebhookFeedbackLoopCandidate, AutomationWebhookQueueResult,
    AutomationWebhookRawEventCreateInput, AutomationWebhookSignatureHeaders,
    AutomationWebhookVerificationDecision, AutomationWebhookVerificationError,
};
use crate::automation_v2::types::automation_webhook_provider_event_id_headers;
use crate::{
    AppState, AutomationWebhookDeliveryRecord, AutomationWebhookDeliveryStatus,
    AutomationWebhookRawEventRecord, AutomationWebhookTriggerRecord,
};

const AUTOMATION_WEBHOOK_MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
const AUTOMATION_WEBHOOK_SIGNATURE_TOLERANCE_MS: u64 = 5 * 60 * 1000;
const AUTOMATION_WEBHOOK_SIGNATURE_HEADER: &str = "x-tandem-webhook-signature";
const AUTOMATION_WEBHOOK_LEGACY_SIGNATURE_HEADER: &str = "x-tandem-signature";
const AUTOMATION_WEBHOOK_GITHUB_SIGNATURE_HEADER: &str = "x-hub-signature-256";
const AUTOMATION_WEBHOOK_SHARED_SECRET_HEADER: &str = "x-tandem-webhook-secret";
const AUTOMATION_WEBHOOK_ORIGIN_ACTION_HEADER: &str = "x-tandem-origin-action-id";
const AUTOMATION_WEBHOOK_ORIGIN_RUN_HEADER: &str = "x-tandem-origin-run-id";
const AUTOMATION_WEBHOOK_ORIGIN_NODE_HEADER: &str = "x-tandem-origin-node-id";
const AUTOMATION_WEBHOOK_ORIGIN_IDEMPOTENCY_HEADER: &str = "x-tandem-origin-idempotency-key";
const AUTOMATION_WEBHOOK_ORIGIN_RESOURCE_HEADER: &str = "x-tandem-origin-resource-id";
const AUTOMATION_WEBHOOK_ALLOW_SELF_FEEDBACK_HEADER: &str = "x-tandem-allow-self-feedback";

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/webhooks/automations/{public_path_token}",
            post(automation_webhook_intake),
        )
        .route(
            "/api/engine/webhooks/automations/{public_path_token}",
            post(automation_webhook_intake),
        )
}

async fn automation_webhook_intake(
    State(state): State<AppState>,
    Path(public_path_token): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let received_at_ms = crate::now_ms();
    if body.len() > AUTOMATION_WEBHOOK_MAX_PAYLOAD_BYTES {
        return webhook_public_response(StatusCode::PAYLOAD_TOO_LARGE, "rejected");
    }
    if !is_json_content_type(&headers) {
        return webhook_public_response(StatusCode::UNSUPPORTED_MEDIA_TYPE, "rejected");
    }

    let advisory_provider_event_id =
        advisory_provider_event_id(&state, &public_path_token, &headers).await;
    let body_digest = automation_webhook_body_digest(body.as_ref());
    let signature_headers = signature_headers_from_request(&headers);
    let verified = match state
        .verify_automation_webhook_request_with_headers(
            &public_path_token,
            signature_headers,
            body.as_ref(),
            advisory_provider_event_id.clone(),
            received_at_ms,
            AUTOMATION_WEBHOOK_SIGNATURE_TOLERANCE_MS,
        )
        .await
    {
        Ok(verified) => verified,
        Err(error) => {
            let preview = preview_for_rejected_body(body.as_ref(), &body_digest);
            record_verification_rejection(
                &state,
                &public_path_token,
                &error,
                advisory_provider_event_id,
                body_digest,
                &headers,
                body.as_ref(),
                received_at_ms,
                preview,
            )
            .await;
            return verification_error_response(&error);
        }
    };
    let raw_event = match record_raw_event_for_trigger(
        &state,
        &verified.trigger,
        verified.provider_event_id.clone(),
        verified.body_digest.clone(),
        &headers,
        body.as_ref(),
        verified.received_at_ms,
    )
    .await
    {
        Ok(raw_event) => raw_event,
        Err(error) => {
            tracing::warn!(error = %error, "failed to persist automation webhook raw event");
            return webhook_public_response(StatusCode::INTERNAL_SERVER_ERROR, "rejected");
        }
    };
    let raw_event_tenant = verified.trigger.tenant_context.clone();

    let payload = match serde_json::from_slice::<Value>(body.as_ref()) {
        Ok(payload) => payload,
        Err(_) => {
            if let Ok(delivery) = state
                .record_automation_webhook_rejection(
                    &verified.trigger,
                    verified.provider_event_id,
                    verified.body_digest,
                    AutomationWebhookDeliveryStatus::Rejected,
                    "invalid_json",
                    verified.received_at_ms,
                    json!({ "body_digest": body_digest }),
                    Some(verified.verification),
                )
                .await
            {
                update_raw_event_from_delivery(&state, &raw_event_tenant, &raw_event, &delivery)
                    .await;
            }
            return webhook_public_response(StatusCode::BAD_REQUEST, "rejected");
        }
    };
    let sanitized_preview = sanitize_automation_webhook_preview(&payload);
    let feedback_loop_candidate = automation_webhook_feedback_loop_candidate(&headers, &payload);

    match state
        .queue_automation_v2_run_from_webhook_delivery_with_feedback_loop(
            verified,
            sanitized_preview,
            feedback_loop_candidate,
        )
        .await
    {
        Ok(AutomationWebhookQueueResult::Accepted { delivery, .. }) => {
            update_raw_event_from_delivery(&state, &raw_event_tenant, &raw_event, &delivery).await;
            webhook_public_response(StatusCode::ACCEPTED, "accepted")
        }
        Ok(AutomationWebhookQueueResult::Duplicate { delivery }) => {
            update_raw_event_from_delivery(&state, &raw_event_tenant, &raw_event, &delivery).await;
            webhook_public_response(StatusCode::ACCEPTED, "accepted")
        }
        Ok(AutomationWebhookQueueResult::Woken { delivery, .. }) => {
            update_raw_event_from_delivery(&state, &raw_event_tenant, &raw_event, &delivery).await;
            webhook_public_response(StatusCode::ACCEPTED, "accepted")
        }
        Ok(AutomationWebhookQueueResult::Suppressed { delivery }) => {
            update_raw_event_from_delivery(&state, &raw_event_tenant, &raw_event, &delivery).await;
            webhook_public_response(StatusCode::ACCEPTED, "accepted")
        }
        Ok(AutomationWebhookQueueResult::Rejected { delivery, .. }) => {
            update_raw_event_from_delivery(&state, &raw_event_tenant, &raw_event, &delivery).await;
            webhook_public_response(StatusCode::CONFLICT, "rejected")
        }
        Err(error) => {
            tracing::warn!(error = %error, "automation webhook intake failed");
            webhook_public_response(StatusCode::INTERNAL_SERVER_ERROR, "rejected")
        }
    }
}

fn header_str<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn is_json_content_type(headers: &HeaderMap) -> bool {
    let Some(value) = header_str(headers, header::CONTENT_TYPE.as_str()) else {
        return false;
    };
    value
        .split(';')
        .next()
        .is_some_and(|media_type| media_type.trim().eq_ignore_ascii_case("application/json"))
}

fn signature_headers_from_request(headers: &HeaderMap) -> AutomationWebhookSignatureHeaders {
    AutomationWebhookSignatureHeaders::from_headers(
        header_str(headers, AUTOMATION_WEBHOOK_SIGNATURE_HEADER),
        header_str(headers, AUTOMATION_WEBHOOK_LEGACY_SIGNATURE_HEADER),
        header_str(headers, AUTOMATION_WEBHOOK_GITHUB_SIGNATURE_HEADER),
        header_str(headers, AUTOMATION_WEBHOOK_SHARED_SECRET_HEADER),
    )
}

async fn advisory_provider_event_id(
    state: &AppState,
    public_path_token: &str,
    headers: &HeaderMap,
) -> Option<String> {
    if let Some(trigger) = state
        .get_automation_webhook_trigger_by_public_token(public_path_token)
        .await
    {
        return provider_event_id_from_headers(
            headers,
            automation_webhook_provider_event_id_headers(&trigger.provider),
        );
    }
    provider_event_id_from_headers(
        headers,
        automation_webhook_provider_event_id_headers("generic"),
    )
}

fn truthy_header(value: Option<&str>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "allow" | "allowed"
        )
    })
}

fn json_path_string(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(512).collect::<String>())
}

fn json_path_bool(value: &Value, path: &[&str]) -> bool {
    let mut current = value;
    for key in path {
        let Some(next) = current.get(*key) else {
            return false;
        };
        current = next;
    }
    current.as_bool().unwrap_or(false)
        || current.as_str().is_some_and(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "allow" | "allowed"
            )
        })
}

fn automation_webhook_feedback_loop_candidate(
    headers: &HeaderMap,
    payload: &Value,
) -> Option<AutomationWebhookFeedbackLoopCandidate> {
    let candidate = AutomationWebhookFeedbackLoopCandidate {
        source_action_id: header_str(headers, AUTOMATION_WEBHOOK_ORIGIN_ACTION_HEADER)
            .map(str::to_string)
            .or_else(|| json_path_string(payload, &["tandem_origin", "action_id"]))
            .or_else(|| json_path_string(payload, &["tandemOrigin", "actionID"])),
        source_run_id: header_str(headers, AUTOMATION_WEBHOOK_ORIGIN_RUN_HEADER)
            .map(str::to_string)
            .or_else(|| json_path_string(payload, &["tandem_origin", "run_id"]))
            .or_else(|| json_path_string(payload, &["tandemOrigin", "runID"])),
        source_node_id: header_str(headers, AUTOMATION_WEBHOOK_ORIGIN_NODE_HEADER)
            .map(str::to_string)
            .or_else(|| json_path_string(payload, &["tandem_origin", "node_id"]))
            .or_else(|| json_path_string(payload, &["tandemOrigin", "nodeID"])),
        source_idempotency_key: header_str(headers, AUTOMATION_WEBHOOK_ORIGIN_IDEMPOTENCY_HEADER)
            .map(str::to_string)
            .or_else(|| json_path_string(payload, &["tandem_origin", "idempotency_key"]))
            .or_else(|| json_path_string(payload, &["tandemOrigin", "idempotencyKey"])),
        provider_resource_id: header_str(headers, AUTOMATION_WEBHOOK_ORIGIN_RESOURCE_HEADER)
            .map(str::to_string)
            .or_else(|| json_path_string(payload, &["tandem_origin", "resource_id"]))
            .or_else(|| json_path_string(payload, &["tandemOrigin", "resourceID"])),
        allow_self_feedback: truthy_header(header_str(
            headers,
            AUTOMATION_WEBHOOK_ALLOW_SELF_FEEDBACK_HEADER,
        )) || json_path_bool(
            payload,
            &["tandem_origin", "allow_self_feedback"],
        ) || json_path_bool(payload, &["tandemOrigin", "allowSelfFeedback"]),
    };
    (!candidate.is_empty()).then_some(candidate)
}

fn provider_event_id_from_headers(
    headers: &HeaderMap,
    event_id_headers: &[&str],
) -> Option<String> {
    event_id_headers
        .iter()
        .find_map(|name| header_str(headers, name))
        .map(|value| value.chars().take(256).collect::<String>())
}

fn webhook_header_is_sensitive(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase();
    normalized.contains("authorization")
        || normalized.contains("cookie")
        || normalized.contains("api-key")
        || normalized.contains("apikey")
        || normalized.contains("password")
        || normalized.contains("passwd")
        || normalized.contains("credential")
        || normalized.contains("secret")
        || normalized.contains("signature")
        || normalized.contains("token")
}

fn automation_webhook_headers_digest(headers: &HeaderMap) -> String {
    let mut rows = headers
        .iter()
        .map(|(name, value)| {
            let value = value.to_str().unwrap_or("[non_utf8]");
            format!("{}:{value}", name.as_str().to_ascii_lowercase())
        })
        .collect::<Vec<_>>();
    rows.sort();
    automation_webhook_body_digest(rows.join("\n").as_bytes())
}

fn redacted_automation_webhook_headers(headers: &HeaderMap) -> Value {
    let mut map = serde_json::Map::new();
    for (name, value) in headers.iter() {
        let key = name.as_str().to_ascii_lowercase();
        let value = if webhook_header_is_sensitive(&key) {
            Value::String("[redacted]".to_string())
        } else {
            Value::String(
                value
                    .to_str()
                    .map(|value| value.chars().take(512).collect::<String>())
                    .unwrap_or_else(|_| "[non_utf8]".to_string()),
            )
        };
        match map.get_mut(&key) {
            Some(Value::Array(items)) => items.push(value),
            Some(existing) => {
                let previous = std::mem::replace(existing, Value::Null);
                *existing = Value::Array(vec![previous, value]);
            }
            None => {
                map.insert(key, value);
            }
        }
    }
    Value::Object(map)
}

fn preview_for_rejected_body(body: &[u8], body_digest: &str) -> Value {
    serde_json::from_slice::<Value>(body)
        .map(|value| sanitize_automation_webhook_preview(&value))
        .unwrap_or_else(|_| json!({ "body_digest": body_digest }))
}

fn verification_error_allows_raw_payload_persistence(
    error: &AutomationWebhookVerificationError,
) -> bool {
    matches!(error, AutomationWebhookVerificationError::ReplayDetected)
}

async fn record_raw_event_for_trigger(
    state: &AppState,
    trigger: &AutomationWebhookTriggerRecord,
    provider_event_id: Option<String>,
    body_digest: String,
    headers: &HeaderMap,
    body: &[u8],
    received_at_ms: u64,
) -> anyhow::Result<AutomationWebhookRawEventRecord> {
    state
        .record_automation_webhook_raw_event(AutomationWebhookRawEventCreateInput {
            trigger: trigger.clone(),
            provider_event_id,
            body_digest,
            headers_digest: automation_webhook_headers_digest(headers),
            headers_redacted: redacted_automation_webhook_headers(headers),
            content_type: header_str(headers, header::CONTENT_TYPE.as_str()).map(str::to_string),
            payload: body.to_vec(),
            received_at_ms,
        })
        .await
}

async fn update_raw_event_from_delivery(
    state: &AppState,
    tenant_context: &tandem_types::TenantContext,
    raw_event: &AutomationWebhookRawEventRecord,
    delivery: &AutomationWebhookDeliveryRecord,
) {
    if let Err(error) = state
        .update_automation_webhook_raw_event_outcome(
            tenant_context,
            &raw_event.event_id,
            delivery,
            crate::now_ms(),
        )
        .await
    {
        tracing::warn!(
            error = %error,
            event_id = %raw_event.event_id,
            delivery_id = %delivery.delivery_id,
            "failed to update automation webhook raw event outcome"
        );
    }
}

async fn record_verification_rejection(
    state: &AppState,
    public_path_token: &str,
    error: &AutomationWebhookVerificationError,
    provider_event_id: Option<String>,
    body_digest: String,
    headers: &HeaderMap,
    body: &[u8],
    received_at_ms: u64,
    sanitized_preview: Value,
) {
    let Some((status, reason_code)) = verification_rejection_delivery(error) else {
        return;
    };
    let Some(trigger) = state
        .get_automation_webhook_trigger_by_public_token(public_path_token)
        .await
    else {
        return;
    };
    let raw_event = if verification_error_allows_raw_payload_persistence(error) {
        match record_raw_event_for_trigger(
            state,
            &trigger,
            provider_event_id.clone(),
            body_digest.clone(),
            headers,
            body,
            received_at_ms,
        )
        .await
        {
            Ok(raw_event) => Some(raw_event),
            Err(error) => {
                tracing::warn!(
                    error = %error,
                    trigger_id = %trigger.trigger_id,
                    "failed to persist rejected automation webhook raw event"
                );
                None
            }
        }
    } else {
        None
    };
    let verification = Some(AutomationWebhookVerificationDecision::rejected_for_trigger(
        &trigger,
        reason_code,
    ));
    if let Ok(delivery) = state
        .record_automation_webhook_rejection(
            &trigger,
            provider_event_id,
            body_digest,
            status,
            reason_code,
            received_at_ms,
            sanitized_preview,
            verification,
        )
        .await
    {
        if let Some(raw_event) = raw_event {
            update_raw_event_from_delivery(state, &trigger.tenant_context, &raw_event, &delivery)
                .await;
        }
    }
}

fn verification_rejection_delivery(
    error: &AutomationWebhookVerificationError,
) -> Option<(AutomationWebhookDeliveryStatus, &'static str)> {
    match error {
        AutomationWebhookVerificationError::UnknownTrigger => None,
        AutomationWebhookVerificationError::DisabledTrigger => Some((
            AutomationWebhookDeliveryStatus::Disabled,
            "trigger_disabled",
        )),
        AutomationWebhookVerificationError::MissingSignature => Some((
            AutomationWebhookDeliveryStatus::Rejected,
            "missing_signature",
        )),
        AutomationWebhookVerificationError::MalformedSignature => Some((
            AutomationWebhookDeliveryStatus::Rejected,
            "malformed_signature",
        )),
        AutomationWebhookVerificationError::StaleTimestamp => Some((
            AutomationWebhookDeliveryStatus::Rejected,
            "stale_signature_timestamp",
        )),
        AutomationWebhookVerificationError::BadSignature => {
            Some((AutomationWebhookDeliveryStatus::Rejected, "bad_signature"))
        }
        AutomationWebhookVerificationError::MissingSecretMaterial => Some((
            AutomationWebhookDeliveryStatus::Failed,
            "missing_secret_material",
        )),
        AutomationWebhookVerificationError::ReplayDetected => Some((
            AutomationWebhookDeliveryStatus::Duplicate,
            "duplicate_delivery",
        )),
    }
}

fn verification_error_response(error: &AutomationWebhookVerificationError) -> Response {
    match error {
        AutomationWebhookVerificationError::UnknownTrigger
        | AutomationWebhookVerificationError::MissingSignature
        | AutomationWebhookVerificationError::MalformedSignature
        | AutomationWebhookVerificationError::StaleTimestamp
        | AutomationWebhookVerificationError::BadSignature
        | AutomationWebhookVerificationError::MissingSecretMaterial => {
            webhook_public_response(StatusCode::UNAUTHORIZED, "rejected")
        }
        AutomationWebhookVerificationError::DisabledTrigger => {
            webhook_public_response(StatusCode::GONE, "rejected")
        }
        AutomationWebhookVerificationError::ReplayDetected => {
            webhook_public_response(StatusCode::ACCEPTED, "accepted")
        }
    }
}

fn webhook_public_response(status: StatusCode, public_status: &'static str) -> Response {
    (
        status,
        Json(json!({
            "ok": status.is_success(),
            "status": public_status,
        })),
    )
        .into_response()
}
