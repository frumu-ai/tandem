use super::*;

use crate::app::state::{
    IdempotencyKeyOutcome, IdempotencyReservation, IdempotencyReservationInput,
};

const PROMPT_IDEMPOTENCY_OPERATION: &str = "session.prompt_async";

#[derive(Debug, Clone)]
pub(super) struct PromptSubmissionReservation {
    pub key: String,
    pub request_fingerprint: String,
}

#[derive(Debug, Clone)]
pub(super) enum PromptSubmissionDecision {
    Reserved(PromptSubmissionReservation),
    Replay(Value),
}

pub(super) async fn reserve_prompt_submission(
    state: &AppState,
    tenant: &TenantContext,
    session_id: &str,
    headers: &HeaderMap,
    req: &SendMessageRequest,
) -> Result<Option<PromptSubmissionDecision>, HttpError> {
    let Some(key) = prompt_idempotency_key(headers) else {
        return Ok(None);
    };
    let request_json = serde_json::to_string(req).map_err(|error| {
        persistence_error(format!("Failed to fingerprint prompt request: {error}"))
    })?;
    let fingerprint = crate::sha256_hex(&[session_id, &request_json]);
    let owner = tenant
        .actor_id
        .as_deref()
        .filter(|actor| !actor.trim().is_empty())
        .unwrap_or(session_id);
    let reservation = state
        .reserve_idempotency_key(IdempotencyReservationInput {
            tenant_context: tenant.clone(),
            operation: PROMPT_IDEMPOTENCY_OPERATION.to_string(),
            key: key.clone(),
            owner: owner.to_string(),
            request_fingerprint: fingerprint.clone(),
            first_seen_event_id: None,
            now_ms: crate::now_ms(),
            expires_at_ms: None,
        })
        .await
        .map_err(|error| {
            persistence_error(format!("Failed to reserve prompt idempotency key: {error}"))
        })?;
    match reservation {
        IdempotencyReservation::Reserved(_) => Ok(Some(PromptSubmissionDecision::Reserved(
            PromptSubmissionReservation {
                key,
                request_fingerprint: fingerprint,
            },
        ))),
        IdempotencyReservation::Duplicate(record) => {
            let Some(outcome) = record.outcome else {
                return Err(http_error(
                    StatusCode::CONFLICT,
                    "Prompt submission with this idempotency key is still initializing",
                    ErrorCode::SessionRunConflict,
                ));
            };
            Ok(Some(PromptSubmissionDecision::Replay(outcome.details)))
        }
        IdempotencyReservation::Conflict(_) => Err(http_error(
            StatusCode::CONFLICT,
            "Idempotency key is already bound to a different prompt request",
            ErrorCode::SessionRunConflict,
        )),
    }
}

pub(super) async fn release_prompt_submission(
    state: &AppState,
    tenant: &TenantContext,
    reservation: &PromptSubmissionReservation,
) -> Result<(), HttpError> {
    state
        .release_reserved_idempotency_key(
            tenant,
            PROMPT_IDEMPOTENCY_OPERATION,
            &reservation.key,
            &reservation.request_fingerprint,
        )
        .await
        .map_err(|error| {
            persistence_error(format!("Failed to release prompt idempotency key: {error}"))
        })?;
    Ok(())
}

pub(super) async fn complete_prompt_submission(
    state: &AppState,
    tenant: &TenantContext,
    reservation: &PromptSubmissionReservation,
    session_id: &str,
    run_id: &str,
    context_run_id: &str,
) -> Result<(), HttpError> {
    let details = json!({
        "runID": run_id,
        "contextRunID": context_run_id,
        "linked_context_run_id": context_run_id,
        "sessionID": session_id,
        "attachEventStream": super::sessions::attach_event_stream_path(session_id, run_id),
        "idempotentReplay": true,
    });
    state
        .complete_idempotency_key(
            tenant,
            PROMPT_IDEMPOTENCY_OPERATION,
            &reservation.key,
            IdempotencyKeyOutcome {
                outcome_kind: "accepted".to_string(),
                completed_at_ms: crate::now_ms(),
                primary_ref_kind: Some("session_run".to_string()),
                primary_ref_id: Some(run_id.to_string()),
                secondary_ref_kind: Some("context_run".to_string()),
                secondary_ref_id: Some(context_run_id.to_string()),
                details,
            },
            crate::now_ms(),
        )
        .await
        .map_err(|error| {
            persistence_error(format!(
                "Failed to complete prompt idempotency key: {error}"
            ))
        })?;
    Ok(())
}

pub(super) fn prompt_replay_response(payload: Value) -> Response {
    (StatusCode::ACCEPTED, Json(payload)).into_response()
}

fn prompt_idempotency_key(headers: &HeaderMap) -> Option<String> {
    headers
        .get("idempotency-key")
        .or_else(|| headers.get("x-tandem-idempotency-key"))
        .and_then(|value| value.to_str().ok())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn failure_category(status: &str, error: Option<&str>) -> Option<&'static str> {
    match status {
        "completed" => None,
        "cancelled" | "canceled" => Some("user_cancelled"),
        "timeout" => Some("timeout"),
        _ => {
            let error = error.unwrap_or_default().to_ascii_lowercase();
            if error.contains("validation")
                || error.contains("invalid")
                || error.contains("blocker")
            {
                Some("validation")
            } else if error.contains("permission")
                || error.contains("denied")
                || error.contains("forbidden")
                || error.contains("403")
            {
                Some("permission_denied")
            } else if error.contains("missing connection")
                || error.contains("not connected")
                || error.contains("not configured")
            {
                Some("missing_connection")
            } else if error.contains("auth") || error.contains("401") {
                Some("authentication")
            } else if error.contains("tool") {
                Some("tool_execution")
            } else if error.contains("provider") || error.contains("model") {
                Some("provider")
            } else {
                Some("runtime")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::failure_category;

    #[test]
    fn failure_categories_are_stable_and_actionable() {
        assert_eq!(failure_category("completed", None), None);
        assert_eq!(failure_category("cancelled", None), Some("user_cancelled"));
        assert_eq!(failure_category("timeout", None), Some("timeout"));
        assert_eq!(
            failure_category("error", Some("provider auth rejected with 401")),
            Some("authentication")
        );
        assert_eq!(
            failure_category("error", Some("tool invocation failed")),
            Some("tool_execution")
        );
        assert_eq!(
            failure_category("error", Some("validation blockers remain")),
            Some("validation")
        );
        assert_eq!(
            failure_category("error", Some("tool permission denied")),
            Some("permission_denied")
        );
        assert_eq!(
            failure_category("error", Some("Slack is not connected")),
            Some("missing_connection")
        );
    }
}
