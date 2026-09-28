// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Hosted operation and per-goal authority for the long-running goal routes.
//! A deployment operation grant does not expose another actor's goal data.

use std::sync::atomic::Ordering;

use axum::{http::StatusCode, response::IntoResponse, response::Response, Json};
use serde_json::json;
use tandem_automation::LongRunningGoal;
use tandem_types::{
    AccessDecision, AccessPermission, DataClass, PrincipalRef, ResourceKind, ResourceRef,
    TenantContext, VerifiedTenantContext,
};

use crate::AppState;

fn forbidden(detail: &str) -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(json!({"error": "goal_forbidden", "detail": detail})),
    )
        .into_response()
}

pub(super) fn not_found() -> Response {
    (
        StatusCode::NOT_FOUND,
        Json(json!({"error": "goal_not_found"})),
    )
        .into_response()
}

fn request_identity_matches(tenant: &TenantContext, verified: &VerifiedTenantContext) -> bool {
    let actor = verified.human_actor.actor_id.trim();
    !actor.is_empty()
        && !verified.is_expired_at(crate::now_ms())
        && super::tenant_matches(tenant, &verified.tenant_context)
        && (tenant.is_local_implicit()
            || (tenant.actor_id.as_deref() == Some(actor)
                && verified.tenant_context.actor_id.as_deref() == Some(actor)))
}

fn unverified_local_access(state: &AppState, tenant: &TenantContext) -> bool {
    // Real local ingress resolves to the implicit tenant. Explicit tenant
    // headers are trusted only by the test fixture; neither path may bypass
    // a configured hosted policy, including one that has not synchronized.
    state
        .enterprise
        .hosted_policy
        .current()
        .is_ok_and(|policy| policy.is_none())
        && (tenant.is_local_implicit() || state.trust_test_tenant_headers.load(Ordering::Relaxed))
}

fn require_operation(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    permission: AccessPermission,
) -> Result<(), Response> {
    let Some(verified) = verified else {
        return if unverified_local_access(state, tenant) {
            Ok(())
        } else {
            Err(forbidden(
                "goal access requires a verified tenant principal",
            ))
        };
    };
    if !request_identity_matches(tenant, verified)
        || state
            .enterprise
            .hosted_policy
            .authorize_permission(Some(verified), permission)
            .is_err()
    {
        return Err(forbidden(
            "authenticated principal lacks current hosted goal authority",
        ));
    }
    Ok(())
}

/// Rebuild object grants after checking the live operation grant. Hosted
/// projection intentionally discards grants copied from an assertion; the
/// current membership/data-grant stores must be consulted again after awaits.
pub(super) async fn current_goal_context(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    operation: AccessPermission,
) -> Result<Option<VerifiedTenantContext>, Response> {
    require_operation(state, tenant, verified, operation)?;
    let Some(verified) = verified else {
        return Ok(None);
    };
    let mut current = verified.clone();
    let memberships = state
        .enterprise
        .hosted_policy
        .project(&mut current)
        .map_err(|_| forbidden("authenticated principal lacks current hosted goal authority"))?;
    if let Some(memberships) = memberships {
        super::middleware::enrich_verified_context_with_org_unit_grants(
            state,
            &mut current,
            Some(memberships),
        )
        .await;
        super::cross_tenant_grants::enrich_verified_context_with_inbound_cross_tenant_grants(
            state,
            &mut current,
        )
        .await;
    }
    // The data-grant lookups above await; do not use their result if the
    // deployment operation or signed identity changed meanwhile.
    require_operation(state, tenant, Some(&current), operation)?;
    Ok(Some(current))
}

/// The named reviewer capabilities remain separate from goal ownership, but
/// neither they nor a stale ingress snapshot can bypass the live use grant.
pub(super) fn require_goal_authority(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    required_capability: Option<&str>,
) -> Result<(), Response> {
    require_operation(state, tenant, verified, AccessPermission::HostedUse)?;
    if let (Some(verified), Some(capability)) = (verified, required_capability) {
        let authorized = verified
            .capabilities
            .iter()
            .any(|value| value == capability)
            || verified.roles.iter().any(|role| {
                matches!(
                    role.as_str(),
                    "owner" | "admin" | "hosted:owner" | "hosted:admin" | "enterprise:admin"
                )
            });
        if !authorized {
            return Err(forbidden(&format!(
                "authenticated principal lacks {capability} authority"
            )));
        }
    }
    Ok(())
}

pub(super) fn initiating_actor_id(goal: &LongRunningGoal) -> Option<&str> {
    goal.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("started_by"))
        .and_then(|value| {
            value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .or_else(|| value.as_str())
        })
}

/// Reproject before an admin-like owner bypass: signed roles/capabilities are
/// checked against current hosted policy, then existing control capabilities
/// retain their established administrative semantics.
pub(super) fn require_goal_owner(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    goal: &LongRunningGoal,
    actor: &PrincipalRef,
) -> Result<(), Response> {
    let Some(verified) = verified else {
        return if unverified_local_access(state, tenant) {
            Ok(())
        } else {
            Err(forbidden(
                "goal mutation requires a verified tenant principal",
            ))
        };
    };
    let mut current = verified.clone();
    if !request_identity_matches(tenant, verified)
        || state
            .enterprise
            .hosted_policy
            .project(&mut current)
            .is_err()
    {
        return Err(forbidden(
            "authenticated principal lacks current hosted goal authority",
        ));
    }
    if super::goals_api::verified_has_admin_authority(Some(&current))
        || initiating_actor_id(goal) == Some(actor.id.as_str())
    {
        Ok(())
    } else {
        Err(forbidden(
            "goal mutation requires its initiating actor or an authorized administrator",
        ))
    }
}

/// This is an object check, not an operation check. POST /goals idempotency
/// replay already has HostedUse and must not additionally require a read grant.
pub(super) fn can_inspect_goal(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    goal: &LongRunningGoal,
) -> bool {
    if !super::tenant_matches(tenant, &goal.tenant_context) {
        return false;
    }
    let Some(verified) = verified else {
        return unverified_local_access(state, tenant);
    };
    if !request_identity_matches(tenant, verified) {
        return false;
    }
    let actor = verified.human_actor.actor_id.trim();
    if initiating_actor_id(goal) == Some(actor)
        || super::goals_api::verified_has_admin_authority(Some(verified))
    {
        return true;
    }
    let Some(strict) = verified.strict_projection.as_ref() else {
        return false;
    };
    if strict.tenant_context != verified.tenant_context
        || strict.principal != PrincipalRef::human_user(actor)
    {
        return false;
    }
    let resource = ResourceRef::new(
        &tenant.org_id,
        &tenant.workspace_id,
        ResourceKind::Run,
        &goal.goal_id,
    );
    let now = crate::now_ms();
    [
        AccessPermission::Read,
        AccessPermission::View,
        AccessPermission::Edit,
        AccessPermission::Admin,
    ]
    .into_iter()
    .any(|permission| {
        strict
            .evaluate_access(&resource, permission, DataClass::Internal, now)
            .decision
            == AccessDecision::Allow
    })
}
