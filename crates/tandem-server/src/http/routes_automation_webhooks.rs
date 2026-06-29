use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use axum::{Json, Router};
use serde_json::{json, Value};

use crate::app::state::{
    automation_webhook_body_digest, sanitize_automation_webhook_preview,
    AutomationWebhookQueueResult, AutomationWebhookSignatureHeaders,
    AutomationWebhookVerificationDecision, AutomationWebhookVerificationError,
};
use crate::automation_v2::types::automation_webhook_provider_event_id_headers;
use crate::{AppState, AutomationWebhookDeliveryStatus};

const AUTOMATION_WEBHOOK_MAX_PAYLOAD_BYTES: usize = 1024 * 1024;
const AUTOMATION_WEBHOOK_SIGNATURE_TOLERANCE_MS: u64 = 5 * 60 * 1000;
const AUTOMATION_WEBHOOK_SIGNATURE_HEADER: &str = "x-tandem-webhook-signature";
const AUTOMATION_WEBHOOK_LEGACY_SIGNATURE_HEADER: &str = "x-tandem-signature";
const AUTOMATION_WEBHOOK_GITHUB_SIGNATURE_HEADER: &str = "x-hub-signature-256";
const AUTOMATION_WEBHOOK_SHARED_SECRET_HEADER: &str = "x-tandem-webhook-secret";

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
                received_at_ms,
                preview,
            )
            .await;
            return verification_error_response(&error);
        }
    };

    let payload = match serde_json::from_slice::<Value>(body.as_ref()) {
        Ok(payload) => payload,
        Err(_) => {
            let _ = state
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
                .await;
            return webhook_public_response(StatusCode::BAD_REQUEST, "rejected");
        }
    };
    let sanitized_preview = sanitize_automation_webhook_preview(&payload);

    match state
        .queue_automation_v2_run_from_webhook_delivery(verified, sanitized_preview)
        .await
    {
        Ok(AutomationWebhookQueueResult::Accepted { .. }) => {
            webhook_public_response(StatusCode::ACCEPTED, "accepted")
        }
        Ok(AutomationWebhookQueueResult::Duplicate { .. }) => {
            webhook_public_response(StatusCode::ACCEPTED, "accepted")
        }
        Ok(AutomationWebhookQueueResult::Woken { .. }) => {
            webhook_public_response(StatusCode::ACCEPTED, "accepted")
        }
        Ok(AutomationWebhookQueueResult::Rejected { .. }) => {
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

fn provider_event_id_from_headers(
    headers: &HeaderMap,
    event_id_headers: &[&str],
) -> Option<String> {
    event_id_headers
        .iter()
        .find_map(|name| header_str(headers, name))
        .map(|value| value.chars().take(256).collect::<String>())
}

fn preview_for_rejected_body(body: &[u8], body_digest: &str) -> Value {
    serde_json::from_slice::<Value>(body)
        .map(|value| sanitize_automation_webhook_preview(&value))
        .unwrap_or_else(|_| json!({ "body_digest": body_digest }))
}

async fn record_verification_rejection(
    state: &AppState,
    public_path_token: &str,
    error: &AutomationWebhookVerificationError,
    provider_event_id: Option<String>,
    body_digest: String,
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
    let verification = Some(AutomationWebhookVerificationDecision::rejected_for_trigger(
        &trigger,
        reason_code,
    ));
    let _ = state
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
        .await;
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
