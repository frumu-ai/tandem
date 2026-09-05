// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Reload only operator-configured verifier material; never accept caller keys.
use axum::{extract::State, http::StatusCode, Extension, Json};
use serde_json::{json, Value};
use std::sync::Arc;
use tandem_types::{TenantContext, VerifiedTenantContext};

use crate::AppState;

pub(super) async fn reload(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    verified: Option<Extension<VerifiedTenantContext>>,
) -> Result<Json<Value>, StatusCode> {
    let verified = verified.as_deref().ok_or(StatusCode::FORBIDDEN)?;
    let (grant, effect) = super::host_authority::authorize_administrative_effect(
        &state,
        &tenant,
        Some(verified),
        locality,
        crate::action_authorization::HostAction::ContextAssertionReload,
        "context_assertion_keyring",
        tenant
            .deployment_id
            .as_deref()
            .ok_or(StatusCode::FORBIDDEN)?,
        json!({"operation": "reload_operator_keyring"}),
    )
    .await?;
    let previous = state
        .context_assertion_security_snapshot()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    // Hosted metadata and durable replay remain mandatory even if unrelated
    // environment settings are changed. No request body/path selects material.
    let next = previous
        .load_hosted_keyring_for_reload()
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    next.validate_hosted_key_transition(&previous, &tenant)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let next = Arc::new(next);
    let receipt = json!({
        "previous_keyring_fingerprint": previous.keyring_fingerprint(),
        "current_keyring_fingerprint": next.keyring_fingerprint(),
        "key_count": next.key_count(),
    });
    // Persist the exact intended snapshot before publication. A lost response
    // can be reconciled by another reload and its returned fingerprint. Failure
    // to persist this audit record leaves the last-known-good snapshot live.
    crate::audit::append_protected_audit_event(
        &state,
        "context_assertion.verifier_reload_prepared",
        &tenant,
        tenant.actor_id.clone(),
        receipt.clone(),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    {
        let mut current = state
            .context_assertion_security
            .write()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        if !current
            .as_ref()
            .is_some_and(|value| Arc::ptr_eq(value, &previous))
        {
            return Err(StatusCode::CONFLICT);
        }
        grant
            .revalidate(&state, &effect)
            .map_err(super::host_authority::host_authorization_status)?;
        *current = Some(next);
    }
    // This operation does not reload provider credentials or channel listeners.
    Ok(Json(json!({"ok": true, "verifier": receipt})))
}
