use axum::http::StatusCode;
use tandem_types::TenantContext;

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
