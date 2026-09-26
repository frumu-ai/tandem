// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::http::StatusCode;
use tandem_types::{TenantContext, VerifiedTenantContext};

use super::tenant_matches;

fn tenant_actor_id(tenant_context: &TenantContext) -> Option<&str> {
    tenant_context
        .actor_id
        .as_deref()
        .map(str::trim)
        .filter(|actor_id| !actor_id.is_empty())
}

pub(super) fn session_visible_to_actor(
    request_tenant: &TenantContext,
    session_tenant: &TenantContext,
) -> bool {
    if !tenant_matches(request_tenant, session_tenant) {
        return false;
    }
    if session_tenant.is_local_implicit() {
        return true;
    }
    matches!(
        (
            tenant_actor_id(request_tenant),
            tenant_actor_id(session_tenant)
        ),
        (Some(request_actor), Some(session_actor)) if request_actor == session_actor
    )
}

pub(super) fn ensure_same_session_actor(
    request_tenant: &TenantContext,
    session_tenant: &TenantContext,
) -> Result<(), StatusCode> {
    if session_visible_to_actor(request_tenant, session_tenant) {
        Ok(())
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

/// Called only after acquiring the idle session's run slot. A rejected or
/// replayed submission must never replace authority used by an active run.
pub(super) async fn refresh_prompt_authority(
    state: &super::AppState,
    session_id: &str,
    request_tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
) -> Result<(), super::HttpError> {
    let session = state
        .storage
        .get_session(session_id)
        .await
        .ok_or_else(super::session_not_found_error)?;
    ensure_same_session_actor(request_tenant, &session.tenant_context)
        .map_err(|_| super::session_not_found_error())?;
    let denied = || {
        super::http_error(
            StatusCode::FORBIDDEN,
            "Current verified session authority is required",
            super::ErrorCode::TenantContextDenied,
        )
    };
    state
        .enterprise
        .hosted_policy
        .authorize_execution(verified)
        .map_err(|_| denied())?;
    let Some(verified) = verified else {
        if !request_tenant.is_local_implicit() && session.verified_tenant_context.is_some() {
            return Err(denied());
        }
        return Ok(());
    };
    ensure_same_session_actor(&verified.tenant_context, &session.tenant_context)
        .map_err(|_| super::session_not_found_error())?;
    let now = crate::now_ms();
    if verified.issued_at_ms > now
        || verified.is_expired_at(now)
        || (!request_tenant.is_local_implicit()
            && tenant_actor_id(&verified.tenant_context)
                != Some(verified.human_actor.actor_id.trim()))
    {
        return Err(denied());
    }
    let mut current = verified.clone();
    state
        .enterprise
        .hosted_policy
        .project(&mut current)
        .map_err(|_| denied())?;
    // Hosted reprojection replaces the strict context. Restore only currently
    // valid, signed inbound grants, just as authenticated ingress does.
    super::cross_tenant_grants::enrich_verified_context_with_inbound_cross_tenant_grants(
        state,
        &mut current,
    )
    .await;
    let updated = state
        .storage
        .update_session_authority(session_id, session.tenant_context, Some(current))
        .await
        .map_err(|_| super::persistence_error("Failed to refresh session authority"))?;
    if !updated {
        return Err(super::session_not_found_error());
    }
    Ok(())
}
