// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use serde_json::json;

use crate::{AppState, ExternalActionRecord};
use tandem_types::{AccessPermission, PrincipalKind, TenantContext, VerifiedTenantContext};

#[derive(Debug, Deserialize, Default)]
pub(super) struct ExternalActionsListQuery {
    pub(super) limit: Option<usize>,
}

pub(super) async fn list_external_actions(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Query(query): Query<ExternalActionsListQuery>,
) -> impl IntoResponse {
    let local_legacy = match external_action_read_scope(&state, &tenant, verified.as_deref()) {
        Ok(local_legacy) => local_legacy,
        Err(status) => return status.into_response(),
    };
    let actions = visible_external_actions(
        &state,
        &tenant,
        verified.as_deref(),
        local_legacy,
        query.limit.unwrap_or(50),
    )
    .await;
    // The map read above may have waited while hosted policy was revoked.
    // Recheck current read authority before serializing any receipts.
    let local_legacy = match external_action_read_scope(&state, &tenant, verified.as_deref()) {
        Ok(local_legacy) => local_legacy,
        Err(status) => return status.into_response(),
    };
    let actions = actions
        .into_iter()
        .filter(|action| {
            external_action_visible(&state, action, &tenant, verified.as_deref(), local_legacy)
        })
        .collect::<Vec<_>>();
    Json(json!({
        "count": actions.len(),
        "actions": actions,
    }))
    .into_response()
}

pub(super) async fn get_external_action(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Err(status) = external_action_read_scope(&state, &tenant, verified.as_deref()) {
        return status.into_response();
    }
    let action = state.get_external_action(&id).await;
    let local_legacy = match external_action_read_scope(&state, &tenant, verified.as_deref()) {
        Ok(local_legacy) => local_legacy,
        Err(status) => return status.into_response(),
    };
    match action {
        Some(action)
            if external_action_visible(
                &state,
                &action,
                &tenant,
                verified.as_deref(),
                local_legacy,
            ) =>
        {
            Json(json!({
                "action": action,
            }))
            .into_response()
        }
        Some(_) | None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "External action not found",
                "code": "EXTERNAL_ACTION_NOT_FOUND",
                "action_id": id,
            })),
        )
            .into_response(),
    }
}

fn external_action_read_scope(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
) -> Result<bool, StatusCode> {
    match state.enterprise.hosted_policy.current() {
        Ok(None) if tenant.is_local_implicit() && verified.is_none() => Ok(true),
        Ok(None) if verified.is_some_and(|verified| verified.policy_version.is_some()) => {
            Err(StatusCode::FORBIDDEN)
        }
        Ok(None) => Ok(false),
        Ok(Some(_)) => {
            let verified = verified.ok_or(StatusCode::FORBIDDEN)?;
            if verified.is_expired_at(crate::now_ms())
                || !super::tenant_matches(tenant, &verified.tenant_context)
                || tenant.actor_id.as_deref() != Some(verified.human_actor.actor_id.as_str())
            {
                return Err(StatusCode::FORBIDDEN);
            }
            state
                .enterprise
                .hosted_policy
                .authorize_permission(Some(verified), AccessPermission::HostedAutomationRead)
                .map_err(|_| StatusCode::FORBIDDEN)?;
            Ok(false)
        }
        Err(_) => Err(StatusCode::FORBIDDEN),
    }
}

pub(in crate::http) fn external_action_visible(
    state: &AppState,
    action: &ExternalActionRecord,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    local_legacy: bool,
) -> bool {
    if local_legacy {
        return true;
    }
    let (Some(provenance), Some(verified)) = (action.provenance.as_ref(), verified) else {
        return false;
    };
    if !super::tenant_matches(tenant, &provenance.tenant_context) {
        return false;
    }
    if verified.policy_version.is_some() && state.authorize_current_hosted_admin(verified).is_ok() {
        return true;
    }
    if verified.policy_version.is_none()
        && super::workflows::workflow_reviewer_is_eligible(tenant, Some(verified))
    {
        return true;
    }
    provenance.owner_principal.as_ref().is_some_and(|owner| {
        owner.kind == PrincipalKind::HumanUser && owner.id == verified.human_actor.actor_id
    })
}

async fn visible_external_actions(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    local_legacy: bool,
    limit: usize,
) -> Vec<ExternalActionRecord> {
    let mut rows = state
        .external_actions
        .read()
        .await
        .values()
        .filter(|action| external_action_visible(state, action, tenant, verified, local_legacy))
        .cloned()
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    rows.truncate(limit.clamp(1, 200));
    rows
}

pub(in crate::http) async fn external_actions_for_authority_inventory(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    limit: usize,
) -> Vec<ExternalActionRecord> {
    let Ok(local_legacy) = external_action_read_scope(state, tenant, verified) else {
        return Vec::new();
    };
    let rows = visible_external_actions(state, tenant, verified, local_legacy, limit).await;
    let Ok(local_legacy) = external_action_read_scope(state, tenant, verified) else {
        return Vec::new();
    };
    rows.into_iter()
        .filter(|action| external_action_visible(state, action, tenant, verified, local_legacy))
        .collect()
}
