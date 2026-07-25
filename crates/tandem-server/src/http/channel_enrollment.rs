// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use tandem_types::{TenantContext, VerifiedTenantContext};

use crate::app::state::channel_user_capabilities::{
    ChannelEnrollmentCodeRecord, ChannelUserCapabilityRecord, StoredCommandTier,
};
use crate::AppState;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum ChannelEnrollRequest {
    Issue {
        channel: String,
        user_id: String,
        tier: StoredCommandTier,
        #[serde(default)]
        ttl_seconds: Option<u64>,
        #[serde(default)]
        issued_by: Option<String>,
        #[serde(default)]
        pinned_workspace_id: Option<String>,
        /// TAN-765: departments (org units, by bare id or `taxonomy/unit_id`)
        /// the redeeming identity should become a member of.
        #[serde(default)]
        org_units: Vec<String>,
        /// Tenant the `org_units` refs resolve in (set both or neither).
        /// Without it, refs that match units in multiple tenants are
        /// rejected rather than resolved to an arbitrary tenant.
        #[serde(default)]
        tenant_org_id: Option<String>,
        #[serde(default)]
        tenant_workspace_id: Option<String>,
    },
    Confirm {
        pairing_code: String,
        #[serde(default)]
        enrolled_by: Option<String>,
    },
}

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
enum ChannelEnrollResponse {
    CodeIssued {
        pairing_code: String,
        expires_at_ms: u64,
        enrollment: ChannelEnrollmentCodeRecord,
    },
    Enrolled {
        capability: ChannelUserCapabilityRecord,
    },
}

/// GOV-B5b: control-panel issuance of a per-identity, expiring channel step-up.
/// A channel configured with `require_approval_step_up` only honors an approval
/// from an identity that holds an active grant issued here.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct ChannelStepUpRequest {
    channel: String,
    user_id: String,
    #[serde(default)]
    ttl_seconds: Option<u64>,
    /// Optional tenant scope. A scoped grant only satisfies connections
    /// bound to this tenant; an unscoped grant satisfies any connection.
    #[serde(default)]
    tenant_org_id: Option<String>,
    #[serde(default)]
    tenant_workspace_id: Option<String>,
}

const DEFAULT_STEP_UP_TTL_MS: u64 = 5 * 60 * 1000;

pub(crate) async fn channel_step_up(
    State(state): State<AppState>,
    Extension(request_tenant): Extension<TenantContext>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<ChannelStepUpRequest>,
) -> Response {
    if input.channel.trim().is_empty() || input.user_id.trim().is_empty() {
        return enrollment_error(StatusCode::BAD_REQUEST, "channel and user_id are required");
    }
    let ttl_ms = input
        .ttl_seconds
        .map(|seconds| seconds.saturating_mul(1000))
        .filter(|ms| *ms > 0)
        .unwrap_or(DEFAULT_STEP_UP_TTL_MS);
    let tenant_org_id = input
        .tenant_org_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let tenant_workspace_id = input
        .tenant_workspace_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let tenant = match (tenant_org_id, tenant_workspace_id) {
        (Some(org_id), Some(workspace_id)) => Some((org_id, workspace_id)),
        (None, None) => None,
        // Half a tenant is a caller bug; refuse rather than silently issuing
        // an unscoped (works-everywhere) grant the operator meant to scope.
        _ => {
            return enrollment_error(
                StatusCode::BAD_REQUEST,
                "tenant_org_id and tenant_workspace_id must be provided together",
            );
        }
    };
    if let Err(response) = require_target_tenant_scope(
        &state,
        &request_tenant,
        verified.as_deref(),
        tenant,
        "channel_step_up",
    )
    .await
    {
        return response;
    }
    let channel = input.channel.trim().to_ascii_lowercase();
    let user_id = input.user_id.trim();
    let (grant, effect) = match crate::http::host_authority::authorize_administrative_effect(
        &state,
        &request_tenant,
        verified.as_deref(),
        locality,
        crate::action_authorization::HostAction::ChannelStepUpGrant,
        "channel_identity",
        format!("{channel}:{user_id}"),
        json!({
            "channel": &channel,
            "user_id": user_id,
            "ttl_ms": ttl_ms,
            "tenant_org_id": tenant.map(|(org_id, _)| org_id),
            "tenant_workspace_id": tenant.map(|(_, workspace_id)| workspace_id),
        }),
    )
    .await
    {
        Ok(authorized) => authorized,
        Err(status) => return status.into_response(),
    };
    if let Err(error) = grant.revalidate(&state, &effect) {
        return crate::http::host_authority::host_authorization_status(error).into_response();
    }
    let expires_at_ms = state
        .grant_channel_step_up(&channel, user_id, ttl_ms, tenant)
        .await;
    Json(json!({
        "status": "step_up_granted",
        "channel": channel,
        "user_id": user_id,
        "expires_at_ms": expires_at_ms,
        "tenant_org_id": tenant.map(|(org_id, _)| org_id),
        "tenant_workspace_id": tenant.map(|(_, workspace_id)| workspace_id),
    }))
    .into_response()
}

pub(crate) async fn channel_enroll(
    State(state): State<AppState>,
    Extension(request_tenant): Extension<TenantContext>,
    Extension(locality): Extension<crate::http::host_authority::RequestLocality>,
    verified: Option<Extension<VerifiedTenantContext>>,
    Json(mut input): Json<ChannelEnrollRequest>,
) -> Response {
    let (action, resource_id, target_tenant) = match &mut input {
        ChannelEnrollRequest::Issue {
            channel,
            user_id,
            issued_by,
            tenant_org_id,
            tenant_workspace_id,
            ..
        } => {
            if channel.trim().is_empty() || user_id.trim().is_empty() {
                return enrollment_error(
                    StatusCode::BAD_REQUEST,
                    "channel and user_id are required",
                );
            }
            let target = match normalized_target_tenant(
                tenant_org_id.as_deref(),
                tenant_workspace_id.as_deref(),
            ) {
                Ok(target) => target,
                Err(response) => return response,
            };
            if let Some(actor_id) = authority_actor(&request_tenant, verified.as_deref()) {
                *issued_by = Some(actor_id);
            }
            (
                crate::action_authorization::HostAction::ChannelEnrollmentIssue,
                format!("{}:{}", channel.trim().to_ascii_lowercase(), user_id.trim()),
                target,
            )
        }
        ChannelEnrollRequest::Confirm {
            pairing_code,
            enrolled_by,
        } => {
            let Some(pending) = state.pending_channel_enrollment_code(pairing_code).await else {
                return enrollment_error(StatusCode::NOT_FOUND, "pairing code not found");
            };
            let target = match normalized_target_tenant(
                pending.tenant_org_id.as_deref(),
                pending.tenant_workspace_id.as_deref(),
            ) {
                Ok(target) => target,
                Err(response) => return response,
            };
            if let Some(actor_id) = authority_actor(&request_tenant, verified.as_deref()) {
                *enrolled_by = Some(actor_id);
            }
            (
                crate::action_authorization::HostAction::ChannelEnrollmentConfirm,
                format!(
                    "code-sha256:{:x}",
                    Sha256::digest(pairing_code.trim().to_ascii_uppercase().as_bytes())
                ),
                target,
            )
        }
    };
    if let Err(response) = require_target_tenant_scope(
        &state,
        &request_tenant,
        verified.as_deref(),
        target_tenant
            .as_ref()
            .map(|(org, workspace)| (org.as_str(), workspace.as_str())),
        "channel_enrollment",
    )
    .await
    {
        return response;
    }
    let arguments = match serde_json::to_value(&input) {
        Ok(arguments) => arguments,
        Err(_) => {
            return enrollment_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "channel enrollment request could not be authorized",
            );
        }
    };
    let (grant, effect) = match crate::http::host_authority::authorize_administrative_effect(
        &state,
        &request_tenant,
        verified.as_deref(),
        locality,
        action,
        "channel_identity",
        resource_id,
        arguments,
    )
    .await
    {
        Ok(authorized) => authorized,
        Err(status) => return status.into_response(),
    };
    if let Err(error) = grant.revalidate(&state, &effect) {
        return crate::http::host_authority::host_authorization_status(error).into_response();
    }
    channel_enroll_inner(State(state), Json(input)).await
}

async fn channel_enroll_inner(
    State(state): State<AppState>,
    Json(input): Json<ChannelEnrollRequest>,
) -> Response {
    match input {
        ChannelEnrollRequest::Issue {
            channel,
            user_id,
            tier,
            ttl_seconds,
            issued_by,
            pinned_workspace_id,
            org_units,
            tenant_org_id,
            tenant_workspace_id,
        } => {
            if channel.trim().is_empty() || user_id.trim().is_empty() {
                return enrollment_error(
                    StatusCode::BAD_REQUEST,
                    "channel and user_id are required",
                );
            }
            let tenant = match (tenant_org_id, tenant_workspace_id) {
                (Some(org_id), Some(workspace_id))
                    if !org_id.trim().is_empty() && !workspace_id.trim().is_empty() =>
                {
                    Some((org_id.trim().to_string(), workspace_id.trim().to_string()))
                }
                (None, None) => None,
                _ => {
                    return enrollment_error(
                        StatusCode::BAD_REQUEST,
                        "tenant_org_id and tenant_workspace_id must be provided together",
                    );
                }
            };
            let enrollment = match state
                .issue_channel_enrollment_code(
                    channel.trim().to_ascii_lowercase(),
                    user_id.trim().to_string(),
                    tier,
                    ttl_seconds.map(|seconds| seconds.saturating_mul(1000)),
                    issued_by,
                    pinned_workspace_id
                        .as_deref()
                        .and_then(tandem_core::normalize_workspace_path),
                    org_units,
                    tenant,
                )
                .await
            {
                Ok(enrollment) => enrollment,
                // Unknown org-unit references fail at issue time so the
                // operator sees the typo immediately (TAN-765).
                Err(error) => return enrollment_error(StatusCode::BAD_REQUEST, &error.to_string()),
            };
            Json(ChannelEnrollResponse::CodeIssued {
                pairing_code: enrollment.code.clone(),
                expires_at_ms: enrollment.expires_at_ms,
                enrollment,
            })
            .into_response()
        }
        ChannelEnrollRequest::Confirm {
            pairing_code,
            enrolled_by,
        } => match state
            .confirm_channel_enrollment_code(&pairing_code, enrolled_by)
            .await
        {
            Ok(capability) => Json(ChannelEnrollResponse::Enrolled { capability }).into_response(),
            Err(error) if error.to_string().contains("expired") => {
                enrollment_error(StatusCode::GONE, &error.to_string())
            }
            Err(error) => enrollment_error(StatusCode::NOT_FOUND, &error.to_string()),
        },
    }
}

fn normalized_target_tenant(
    org_id: Option<&str>,
    workspace_id: Option<&str>,
) -> Result<Option<(String, String)>, Response> {
    match (org_id.map(str::trim), workspace_id.map(str::trim)) {
        (Some(org_id), Some(workspace_id)) if !org_id.is_empty() && !workspace_id.is_empty() => {
            Ok(Some((org_id.to_string(), workspace_id.to_string())))
        }
        (None, None) => Ok(None),
        _ => Err(enrollment_error(
            StatusCode::BAD_REQUEST,
            "tenant_org_id and tenant_workspace_id must be provided together",
        )),
    }
}

fn authority_actor(
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
) -> Option<String> {
    verified
        .map(|context| context.human_actor.actor_id.clone())
        .or_else(|| tenant.actor_id.clone())
}

async fn require_target_tenant_scope(
    state: &AppState,
    request_tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    target: Option<(&str, &str)>,
    operation: &'static str,
) -> Result<(), Response> {
    if verified.is_none()
        || target.is_some_and(|(org_id, workspace_id)| {
            org_id == request_tenant.org_id && workspace_id == request_tenant.workspace_id
        })
    {
        return Ok(());
    }
    crate::audit::append_protected_audit_event_best_effort(
        state,
        "authority.channel_scope.denied",
        request_tenant,
        verified.map(|context| context.human_actor.actor_id.clone()),
        json!({
            "operation": operation,
            "reason": if target.is_some() { "cross_tenant_target" } else { "unscoped_hosted_target" },
            "target_org_id": target.map(|(org_id, _)| org_id),
            "target_workspace_id": target.map(|(_, workspace_id)| workspace_id),
        }),
    )
    .await;
    Err(StatusCode::FORBIDDEN.into_response())
}

fn enrollment_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_channels::config::ChannelSecurityProfile;

    #[tokio::test]
    async fn issue_and_confirm_enrolls_telegram_user_for_approval() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = AppState::new_starting("test".to_string(), true);
        state.channel_user_capabilities_path = dir.path().join("channel_user_capabilities.json");

        let response = channel_enroll_inner(
            State(state.clone()),
            Json(ChannelEnrollRequest::Issue {
                channel: "telegram".to_string(),
                user_id: "4242".to_string(),
                tier: StoredCommandTier::Approve,
                ttl_seconds: Some(60),
                issued_by: Some("operator".to_string()),
                pinned_workspace_id: Some("/workspace/acme".to_string()),
                org_units: Vec::new(),
                tenant_org_id: None,
                tenant_workspace_id: None,
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);

        let issued = state
            .channel_enrollment_codes
            .read()
            .await
            .values()
            .next()
            .cloned()
            .expect("code stored");
        let response = channel_enroll_inner(
            State(state.clone()),
            Json(ChannelEnrollRequest::Confirm {
                pairing_code: issued.code,
                enrolled_by: Some("desktop".to_string()),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            state
                .channel_user_can_approve(
                    "telegram",
                    "4242",
                    ChannelSecurityProfile::PublicDemo,
                    false,
                    None
                )
                .await
        );
    }
}
