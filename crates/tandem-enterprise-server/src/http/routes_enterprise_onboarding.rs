// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::{HashMap, HashSet};

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_enterprise_contract::{
    AccessEffect, AccessPermission, ConnectorCredentialClass, ConnectorLifecycleState, DataClass,
    IngestionPolicy, OrganizationUnitState, PrincipalKind, RequestPrincipal, ResourceKind,
    ResourceRef, SecretRef, SourceBindingState, TenantContext, VerifiedTenantContext,
};

use tandem_server::automation_v2::governance::GovernanceApprovalStatus;
use tandem_server::AppState;

use super::routes_enterprise::{
    ingestion_quarantine_tenant_matches, storage_base, validate_enterprise_id,
    validate_external_id, validate_resource_ref_matches_tenant, EnterpriseAdminResponseBase,
    EnterpriseResult,
};

const GOOGLE_DRIVE_PROVIDER: &str = "google_drive";
const GOOGLE_DRIVE_SOURCE_TYPE: &str = "google_drive";

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route("/enterprise/readiness", get(get_enterprise_readiness))
        .route(
            "/enterprise/onboarding-plans/preview",
            post(preview_enterprise_onboarding_plan),
        )
}

#[derive(Debug, Serialize)]
struct EnterpriseReadinessResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    overall_status: &'static str,
    counts: EnterpriseReadinessCounts,
    checks: Vec<EnterpriseReadinessCheck>,
}

#[derive(Debug, Serialize, Default)]
struct EnterpriseReadinessCounts {
    org_units: usize,
    memberships: usize,
    access_grants: usize,
    connectors: usize,
    connectors_by_state: HashMap<String, usize>,
    source_bindings: usize,
    source_bindings_by_state: HashMap<String, usize>,
    pending_quarantines: usize,
    pending_approvals: usize,
}

#[derive(Debug, Serialize)]
struct EnterpriseReadinessCheck {
    id: &'static str,
    status: &'static str,
    summary: String,
    recommended_action: Option<&'static str>,
}

#[derive(Debug, Deserialize, Default)]
struct EnterpriseOnboardingPlanPreviewRequest {
    #[serde(default)]
    org_units: Vec<PreviewOrgUnit>,
    #[serde(default)]
    memberships: Vec<PreviewMembership>,
    #[serde(default)]
    grants: Vec<PreviewGrant>,
    #[serde(default)]
    connectors: Vec<PreviewConnector>,
    #[serde(default)]
    credential_refs: Vec<PreviewCredentialRef>,
    #[serde(default)]
    source_bindings: Vec<PreviewSourceBinding>,
    #[serde(default)]
    mcp_requirements: Vec<PreviewMcpRequirement>,
}

#[derive(Debug, Deserialize)]
struct PreviewOrgUnit {
    unit_id: String,
    #[serde(default)]
    taxonomy_id: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    action: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PreviewMembership {
    #[serde(default)]
    membership_id: Option<String>,
    unit_id: String,
    #[serde(default)]
    taxonomy_id: Option<String>,
    #[serde(default = "default_preview_member_kind")]
    member_kind: PrincipalKind,
    member_id: String,
    #[serde(default)]
    action: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PreviewGrant {
    #[serde(default)]
    grant_id: Option<String>,
    unit_id: String,
    #[serde(default)]
    taxonomy_id: Option<String>,
    resource_kind: ResourceKind,
    resource_id: String,
    #[serde(default)]
    project_id: Option<String>,
    #[serde(default)]
    path_prefix: Option<String>,
    #[serde(default)]
    effect: AccessEffect,
    #[serde(default)]
    permissions: Vec<AccessPermission>,
    #[serde(default)]
    data_classes: Vec<DataClass>,
    #[serde(default)]
    tool_patterns: Vec<String>,
    #[serde(default)]
    action: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PreviewConnector {
    connector_id: String,
    provider: String,
    #[serde(default)]
    action: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PreviewCredentialRef {
    connector_id: String,
    credential_id: String,
    #[serde(default)]
    credential_class: ConnectorCredentialClass,
    secret_ref: SecretRef,
    #[serde(default)]
    source_bound_resource: Option<ResourceRef>,
    #[serde(default)]
    credential_value: Option<Value>,
    #[serde(default)]
    action: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PreviewSourceBinding {
    binding_id: String,
    connector_id: String,
    source_type: String,
    native_source_id: String,
    resource_ref: ResourceRef,
    data_class: DataClass,
    #[serde(default)]
    state: SourceBindingState,
    #[serde(default)]
    credential_ref_id: Option<String>,
    #[serde(default)]
    ingestion_policy: IngestionPolicy,
    #[serde(default)]
    action: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PreviewMcpRequirement {
    name: String,
    #[serde(default)]
    required_tools: Vec<String>,
    #[serde(default)]
    action: Option<String>,
}

fn default_preview_member_kind() -> PrincipalKind {
    PrincipalKind::HumanUser
}

#[derive(Debug, Serialize)]
struct EnterpriseOnboardingPlanPreviewResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    valid: bool,
    operations: Vec<EnterprisePreviewOperation>,
    warnings: Vec<EnterprisePreviewMessage>,
    blocking_errors: Vec<EnterprisePreviewMessage>,
    required_human_actions: Vec<&'static str>,
    private_control_plane_prerequisites: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
struct EnterprisePreviewOperation {
    kind: &'static str,
    id: String,
    action: &'static str,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct EnterprisePreviewMessage {
    code: &'static str,
    path: String,
    message: String,
}

async fn get_enterprise_readiness(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<EnterpriseReadinessResponse> {
    require_enterprise_read_access(&request_principal, verified_tenant_context.as_deref())?;

    let org_units = state
        .enterprise
        .org_units
        .read()
        .await
        .values()
        .filter(|unit| {
            unit.tenant_context.org_id == tenant_context.org_id
                && unit.tenant_context.workspace_id == tenant_context.workspace_id
                && unit.tenant_context.deployment_id == tenant_context.deployment_id
        })
        .cloned()
        .collect::<Vec<_>>();
    let memberships = state
        .enterprise
        .org_unit_memberships
        .read()
        .await
        .values()
        .filter(|membership| {
            membership.tenant_context.org_id == tenant_context.org_id
                && membership.tenant_context.workspace_id == tenant_context.workspace_id
                && membership.tenant_context.deployment_id == tenant_context.deployment_id
        })
        .cloned()
        .collect::<Vec<_>>();
    let grants = state
        .enterprise
        .org_unit_access_grants
        .read()
        .await
        .values()
        .filter(|grant| {
            grant.tenant_context.org_id == tenant_context.org_id
                && grant.tenant_context.workspace_id == tenant_context.workspace_id
                && grant.tenant_context.deployment_id == tenant_context.deployment_id
        })
        .cloned()
        .collect::<Vec<_>>();
    let connectors = state
        .enterprise
        .connectors
        .read()
        .await
        .values()
        .filter(|connector| connector.tenant_matches(&tenant_context))
        .cloned()
        .collect::<Vec<_>>();
    let source_bindings = state
        .enterprise
        .source_bindings
        .read()
        .await
        .values()
        .filter(|binding| binding.tenant_matches(&tenant_context))
        .cloned()
        .collect::<Vec<_>>();
    let pending_quarantines = state
        .enterprise
        .ingestion_quarantines
        .read()
        .await
        .values()
        .filter(|quarantine| {
            ingestion_quarantine_tenant_matches(quarantine, &tenant_context)
                && quarantine.disposition.is_none()
        })
        .count();
    let pending_approvals = state
        .list_approval_requests(None, Some(GovernanceApprovalStatus::Pending))
        .await
        .len();

    let mut counts = EnterpriseReadinessCounts {
        org_units: org_units.len(),
        memberships: memberships.len(),
        access_grants: grants.len(),
        connectors: connectors.len(),
        source_bindings: source_bindings.len(),
        pending_quarantines,
        pending_approvals,
        ..EnterpriseReadinessCounts::default()
    };
    for connector in &connectors {
        *counts
            .connectors_by_state
            .entry(serialized_enum_key(connector.state))
            .or_insert(0) += 1;
    }
    for binding in &source_bindings {
        *counts
            .source_bindings_by_state
            .entry(serialized_enum_key(binding.state))
            .or_insert(0) += 1;
    }

    let active_org_units = org_units
        .iter()
        .filter(|unit| unit.state == OrganizationUnitState::Active)
        .count();
    let active_connectors = connectors
        .iter()
        .filter(|connector| connector.state == ConnectorLifecycleState::Active)
        .count();
    let enabled_bindings = source_bindings
        .iter()
        .filter(|binding| binding.state == SourceBindingState::Enabled)
        .count();
    let memory_context_policy =
        tandem_server::memory::policy_status::current_memory_context_policy_status();
    let memory_policy_details = serde_json::to_string(&memory_context_policy)
        .unwrap_or_else(|_| "{\"startup_ready\":false}".to_string());

    let checks = vec![
        readiness_check(
            "tenant_context",
            "ready",
            format!(
                "tenant `{}` workspace `{}` is attached to the request",
                tenant_context.org_id, tenant_context.workspace_id
            ),
            None,
        ),
        readiness_check(
            "governance_skeleton",
            if active_org_units > 0 && !memberships.is_empty() && !grants.is_empty() {
                "ready"
            } else {
                "attention"
            },
            format!(
                "{} active org units, {} memberships, {} access grants",
                active_org_units,
                memberships.len(),
                grants.len()
            ),
            Some("create a minimal org unit, membership, and access grant for the pilot"),
        ),
        readiness_check(
            "connectors",
            if active_connectors > 0 {
                "ready"
            } else {
                "attention"
            },
            format!(
                "{} active connectors out of {} total",
                active_connectors,
                connectors.len()
            ),
            Some("create or resume one read-only pilot connector"),
        ),
        readiness_check(
            "source_bindings",
            if enabled_bindings > 0 {
                "ready"
            } else {
                "attention"
            },
            format!(
                "{} enabled source bindings out of {} total",
                enabled_bindings,
                source_bindings.len()
            ),
            Some("create one enabled source binding and run preflight before import"),
        ),
        readiness_check(
            "quarantine",
            if pending_quarantines == 0 {
                "ready"
            } else {
                "blocked"
            },
            format!("{} quarantine records require review", pending_quarantines),
            Some("review, release, delete, or reindex pending quarantine records"),
        ),
        readiness_check(
            "approvals",
            if pending_approvals == 0 {
                "ready"
            } else {
                "attention"
            },
            format!("{} governance approvals are pending", pending_approvals),
            Some("resolve pending approval requests before go-live"),
        ),
        readiness_check(
            "memory_context_policy",
            if memory_context_policy.startup_ready {
                "ready"
            } else {
                "blocked"
            },
            memory_policy_details,
            Some(
                "enterprise or hosted memory requires strict tenant enforcement before serving prompts",
            ),
        ),
    ];
    let overall_status = if checks.iter().any(|check| check.status == "blocked") {
        "blocked"
    } else if checks.iter().any(|check| check.status == "attention") {
        "attention"
    } else {
        "ready"
    };

    Ok(Json(EnterpriseReadinessResponse {
        base: storage_base(tenant_context, request_principal),
        overall_status,
        counts,
        checks,
    }))
}

async fn preview_enterprise_onboarding_plan(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<EnterpriseOnboardingPlanPreviewRequest>,
) -> EnterpriseResult<EnterpriseOnboardingPlanPreviewResponse> {
    require_enterprise_read_access(&request_principal, verified_tenant_context.as_deref())?;

    let mut operations = Vec::new();
    let mut warnings = Vec::new();
    let mut blocking_errors = Vec::new();
    let mut planned_org_units = HashSet::<String>::new();
    let mut planned_connectors = HashSet::<String>::new();
    let mut planned_credentials = HashSet::<String>::new();

    for (index, item) in input.org_units.iter().enumerate() {
        let path = format!("org_units[{index}]");
        if reject_preview_destructive_action(item.action.as_deref(), &path, &mut blocking_errors) {
            continue;
        }
        let Ok(unit_id) = validate_enterprise_id("unit_id", &item.unit_id) else {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_INVALID_ORG_UNIT",
                path,
                "org unit id is invalid",
            );
            continue;
        };
        let taxonomy_id = match preview_taxonomy_id(item.taxonomy_id.as_deref()) {
            Ok(value) => value,
            Err(_) => {
                push_preview_error(
                    &mut blocking_errors,
                    "ENTERPRISE_PREVIEW_INVALID_TAXONOMY",
                    path,
                    "taxonomy id is invalid",
                );
                continue;
            }
        };
        if item
            .display_name
            .as_deref()
            .map(str::trim)
            .unwrap_or_default()
            .is_empty()
        {
            push_preview_warning(
                &mut warnings,
                "ENTERPRISE_PREVIEW_ORG_UNIT_LABEL_MISSING",
                path.clone(),
                "org unit display_name should be filled before apply",
            );
        }
        planned_org_units.insert(format!("{taxonomy_id}/{unit_id}"));
        let exists =
            enterprise_org_unit_exists(&state, &tenant_context, &taxonomy_id, &unit_id).await;
        operations.push(preview_operation(
            "org_unit",
            format!("{taxonomy_id}/{unit_id}"),
            exists,
        ));
    }

    for (index, item) in input.connectors.iter().enumerate() {
        let path = format!("connectors[{index}]");
        if reject_preview_destructive_action(item.action.as_deref(), &path, &mut blocking_errors) {
            continue;
        }
        let Ok(connector_id) = validate_enterprise_id("connector_id", &item.connector_id) else {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_INVALID_CONNECTOR",
                path,
                "connector id is invalid",
            );
            continue;
        };
        if validate_enterprise_id("provider", &item.provider).is_err() {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_INVALID_PROVIDER",
                path,
                "connector provider is invalid",
            );
            continue;
        }
        planned_connectors.insert(connector_id.clone());
        let exists = connector_exists_for_tenant(&state, &tenant_context, &connector_id).await;
        operations.push(preview_operation("connector", connector_id, exists));
    }

    for (index, item) in input.memberships.iter().enumerate() {
        let path = format!("memberships[{index}]");
        if reject_preview_destructive_action(item.action.as_deref(), &path, &mut blocking_errors) {
            continue;
        }
        let Ok(unit_id) = validate_enterprise_id("unit_id", &item.unit_id) else {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_INVALID_MEMBERSHIP",
                path,
                "membership unit id is invalid",
            );
            continue;
        };
        let taxonomy_id = match preview_taxonomy_id(item.taxonomy_id.as_deref()) {
            Ok(value) => value,
            Err(_) => {
                push_preview_error(
                    &mut blocking_errors,
                    "ENTERPRISE_PREVIEW_INVALID_TAXONOMY",
                    path,
                    "taxonomy id is invalid",
                );
                continue;
            }
        };
        if validate_external_id("member_id", &item.member_id).is_err() {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_INVALID_MEMBER",
                path,
                "member id is invalid",
            );
            continue;
        }
        let membership_id = match item.membership_id.as_deref() {
            Some(value) => match validate_enterprise_id("membership_id", value) {
                Ok(value) => value,
                Err(_) => {
                    push_preview_error(
                        &mut blocking_errors,
                        "ENTERPRISE_PREVIEW_INVALID_MEMBERSHIP",
                        path,
                        "membership id is invalid",
                    );
                    continue;
                }
            },
            None => format!(
                "membership-{taxonomy_id}-{unit_id}-{}",
                compact_preview_id_segment(&item.member_id)
            ),
        };
        let unit_ref = format!("{taxonomy_id}/{unit_id}");
        if !planned_org_units.contains(&unit_ref)
            && !enterprise_org_unit_exists(&state, &tenant_context, &taxonomy_id, &unit_id).await
        {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_ORG_UNIT_MISSING",
                path,
                format!("org unit `{unit_ref}` must exist or be included in this plan"),
            );
        }
        let exists = enterprise_membership_exists(&state, &tenant_context, &membership_id).await;
        operations.push(preview_operation(
            "org_unit_membership",
            membership_id,
            exists,
        ));
    }

    for (index, item) in input.grants.iter().enumerate() {
        let path = format!("grants[{index}]");
        if reject_preview_destructive_action(item.action.as_deref(), &path, &mut blocking_errors) {
            continue;
        }
        let Ok(unit_id) = validate_enterprise_id("unit_id", &item.unit_id) else {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_INVALID_GRANT",
                path,
                "grant unit id is invalid",
            );
            continue;
        };
        let taxonomy_id = match preview_taxonomy_id(item.taxonomy_id.as_deref()) {
            Ok(value) => value,
            Err(_) => {
                push_preview_error(
                    &mut blocking_errors,
                    "ENTERPRISE_PREVIEW_INVALID_TAXONOMY",
                    path,
                    "taxonomy id is invalid",
                );
                continue;
            }
        };
        let resource_id = match validate_external_id("resource_id", &item.resource_id) {
            Ok(value) => value,
            Err(_) => {
                push_preview_error(
                    &mut blocking_errors,
                    "ENTERPRISE_PREVIEW_INVALID_RESOURCE",
                    path,
                    "resource id is invalid",
                );
                continue;
            }
        };
        let grant_id = match item.grant_id.as_deref() {
            Some(value) => match validate_enterprise_id("grant_id", value) {
                Ok(value) => value,
                Err(_) => {
                    push_preview_error(
                        &mut blocking_errors,
                        "ENTERPRISE_PREVIEW_INVALID_GRANT",
                        path,
                        "grant id is invalid",
                    );
                    continue;
                }
            },
            None => format!("grant-{taxonomy_id}-{unit_id}-{resource_id}"),
        };
        if let Some(project_id) = item.project_id.as_deref() {
            if validate_enterprise_id("project_id", project_id).is_err() {
                push_preview_error(
                    &mut blocking_errors,
                    "ENTERPRISE_PREVIEW_INVALID_PROJECT",
                    path.clone(),
                    "project id is invalid",
                );
            }
        }
        let unit_ref = format!("{taxonomy_id}/{unit_id}");
        if !planned_org_units.contains(&unit_ref)
            && !enterprise_org_unit_exists(&state, &tenant_context, &taxonomy_id, &unit_id).await
        {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_ORG_UNIT_MISSING",
                path.clone(),
                format!("org unit `{unit_ref}` must exist or be included in this plan"),
            );
        }
        if item.data_classes.is_empty() || item.permissions.iter().any(access_permission_is_admin) {
            push_preview_warning(
                &mut warnings,
                "ENTERPRISE_PREVIEW_BROAD_GRANT",
                path,
                "grant is broad; confirm the pilot really needs this scope",
            );
        }
        if matches!(item.effect, AccessEffect::Deny) {
            push_preview_warning(
                &mut warnings,
                "ENTERPRISE_PREVIEW_DENY_GRANT",
                format!("grants[{index}]"),
                "deny grants can block pilot users; verify intent before apply",
            );
        }
        if !item.tool_patterns.is_empty() {
            push_preview_warning(
                &mut warnings,
                "ENTERPRISE_PREVIEW_TOOL_GRANT",
                format!("grants[{index}]"),
                "tool-pattern grants should be paired with narrow MCP allowlists",
            );
        }
        let exists = enterprise_grant_exists(&state, &tenant_context, &grant_id).await;
        operations.push(preview_operation("org_unit_access_grant", grant_id, exists));
    }

    for (index, item) in input.credential_refs.iter().enumerate() {
        let path = format!("credential_refs[{index}]");
        if reject_preview_destructive_action(item.action.as_deref(), &path, &mut blocking_errors) {
            continue;
        }
        if item.credential_value.is_some() {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_CONNECTOR_CREDENTIAL_VALUE_NOT_ALLOWED",
                path.clone(),
                "raw credential values must stay out of runtime onboarding plans",
            );
            continue;
        }
        let Ok(connector_id) = validate_enterprise_id("connector_id", &item.connector_id) else {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_INVALID_CONNECTOR",
                path,
                "connector id is invalid",
            );
            continue;
        };
        let Ok(credential_id) = validate_enterprise_id("credential_id", &item.credential_id) else {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_INVALID_CREDENTIAL",
                path,
                "credential id is invalid",
            );
            continue;
        };
        if normalize_secret_ref_for_tenant(&item.secret_ref, &tenant_context).is_err() {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_CONNECTOR_CREDENTIAL_TENANT_MISMATCH",
                path.clone(),
                "secret ref must match the request tenant",
            );
        }
        if let Some(resource) = item.source_bound_resource.as_ref() {
            if validate_resource_ref_matches_tenant(resource, &tenant_context).is_err() {
                push_preview_error(
                    &mut blocking_errors,
                    "ENTERPRISE_SOURCE_BINDING_RESOURCE_TENANT_MISMATCH",
                    path.clone(),
                    "credential source-bound resource must match the request tenant",
                );
            }
        }
        if !planned_connectors.contains(&connector_id)
            && !connector_exists_for_tenant(&state, &tenant_context, &connector_id).await
        {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_CONNECTOR_MISSING",
                path.clone(),
                format!("connector `{connector_id}` must exist or be included in this plan"),
            );
        }
        planned_credentials.insert(format!("{connector_id}/{credential_id}"));
        let exists = connector_credential_exists_for_tenant(
            &state,
            &tenant_context,
            &connector_id,
            &credential_id,
        )
        .await;
        operations.push(preview_operation(
            "connector_credential_ref",
            format!("{connector_id}/{credential_id}"),
            exists,
        ));
    }

    for (index, item) in input.source_bindings.iter().enumerate() {
        let path = format!("source_bindings[{index}]");
        if reject_preview_destructive_action(item.action.as_deref(), &path, &mut blocking_errors) {
            continue;
        }
        let Ok(binding_id) = validate_enterprise_id("binding_id", &item.binding_id) else {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_INVALID_SOURCE_BINDING",
                path,
                "binding id is invalid",
            );
            continue;
        };
        let Ok(connector_id) = validate_enterprise_id("connector_id", &item.connector_id) else {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_INVALID_CONNECTOR",
                path,
                "connector id is invalid",
            );
            continue;
        };
        if validate_enterprise_id("source_type", &item.source_type).is_err()
            || validate_external_id("native_source_id", &item.native_source_id).is_err()
        {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_INVALID_SOURCE",
                path.clone(),
                "source type or native source id is invalid",
            );
        }
        if validate_resource_ref_matches_tenant(&item.resource_ref, &tenant_context).is_err() {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_SOURCE_BINDING_RESOURCE_TENANT_MISMATCH",
                path.clone(),
                "source binding resource must match the request tenant",
            );
        }
        if validate_google_drive_source_binding_policy(
            &connector_id,
            &item.source_type,
            &item.ingestion_policy,
        )
        .is_err()
        {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_GOOGLE_DRIVE_POLICY_INVALID",
                path.clone(),
                "Google Drive bindings must use the constrained read-only policy shape",
            );
        }
        if !item.state.allows_ingestion() {
            push_preview_warning(
                &mut warnings,
                "ENTERPRISE_PREVIEW_SOURCE_BINDING_NOT_ENABLED",
                path.clone(),
                "source binding is not enabled, so pilot import will not run",
            );
        }
        if let Some(credential_ref_id) = item.credential_ref_id.as_deref() {
            if validate_enterprise_id("credential_ref_id", credential_ref_id).is_err() {
                push_preview_error(
                    &mut blocking_errors,
                    "ENTERPRISE_PREVIEW_INVALID_CREDENTIAL",
                    path.clone(),
                    "credential ref id is invalid",
                );
            } else if !planned_credentials.contains(&format!("{connector_id}/{credential_ref_id}"))
                && !connector_credential_exists_for_tenant(
                    &state,
                    &tenant_context,
                    &connector_id,
                    credential_ref_id,
                )
                .await
            {
                push_preview_error(&mut blocking_errors, "ENTERPRISE_PREVIEW_CREDENTIAL_MISSING", path.clone(), format!("credential ref `{connector_id}/{credential_ref_id}` must exist or be included in this plan"));
            }
        } else {
            push_preview_warning(
                &mut warnings,
                "ENTERPRISE_PREVIEW_CREDENTIAL_REF_MISSING",
                path.clone(),
                "source binding should name a source-bound credential ref before import",
            );
        }
        if !planned_connectors.contains(&connector_id)
            && !connector_exists_for_tenant(&state, &tenant_context, &connector_id).await
        {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_CONNECTOR_MISSING",
                path,
                format!("connector `{connector_id}` must exist or be included in this plan"),
            );
        }
        let exists = source_binding_exists_for_tenant(&state, &tenant_context, &binding_id).await;
        operations.push(preview_operation("source_binding", binding_id, exists));
    }

    for (index, item) in input.mcp_requirements.iter().enumerate() {
        let path = format!("mcp_requirements[{index}]");
        if reject_preview_destructive_action(item.action.as_deref(), &path, &mut blocking_errors) {
            continue;
        }
        let Ok(name) = validate_enterprise_id("mcp_name", &item.name) else {
            push_preview_error(
                &mut blocking_errors,
                "ENTERPRISE_PREVIEW_INVALID_MCP",
                path,
                "MCP name is invalid",
            );
            continue;
        };
        if item.required_tools.is_empty() {
            push_preview_warning(
                &mut warnings,
                "ENTERPRISE_PREVIEW_MCP_TOOLS_MISSING",
                path,
                "MCP requirement should list exact required tools for allowlist review",
            );
        }
        operations.push(EnterprisePreviewOperation {
            kind: "mcp_requirement",
            id: name,
            action: "operator_connect_or_verify",
            status: "requires_human_action",
        });
    }

    let valid = blocking_errors.is_empty();
    Ok(Json(EnterpriseOnboardingPlanPreviewResponse {
        base: storage_base(tenant_context, request_principal),
        valid,
        operations,
        warnings,
        blocking_errors,
        required_human_actions: vec![
            "private_control_plane_provisioning_complete",
            "operator_supplies_secret_refs_without_raw_values",
            "operator_connects_or_approves_required_mcp_servers",
            "operator_runs_preflight_import_and_quarantine_review_after_apply",
        ],
        private_control_plane_prerequisites: vec![
            "tenant_and_workspace_provisioned",
            "invites_and_user_lifecycle_complete",
            "sso_oidc_scim_billing_and_account_ownership_configured_privately",
        ],
    }))
}

fn readiness_check(
    id: &'static str,
    status: &'static str,
    summary: String,
    recommended_action: Option<&'static str>,
) -> EnterpriseReadinessCheck {
    EnterpriseReadinessCheck {
        id,
        status,
        summary,
        recommended_action: if status == "ready" {
            None
        } else {
            recommended_action
        },
    }
}

fn serialized_enum_key<T: Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(ToOwned::to_owned))
        .unwrap_or_else(|| "unknown".to_string())
}

fn require_enterprise_read_access(
    request_principal: &RequestPrincipal,
    verified_tenant_context: Option<&VerifiedTenantContext>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if let Some(verified) = verified_tenant_context {
        if verified
            .roles
            .iter()
            .any(|role| enterprise_admin_role(role))
        {
            return Ok(());
        }
    } else if matches!(
        request_principal.source.as_str(),
        "api_token" | "control_panel" | "local_api_token" | "local_control_panel"
    ) {
        return Ok(());
    }
    Err((
        StatusCode::FORBIDDEN,
        Json(json!({
            "code": "ENTERPRISE_READ_ACCESS_REQUIRED",
            "message": "enterprise read access is required for this request"
        })),
    ))
}

async fn enterprise_org_unit_exists(
    state: &AppState,
    tenant_context: &TenantContext,
    taxonomy_id: &str,
    unit_id: &str,
) -> bool {
    state
        .enterprise
        .org_units
        .read()
        .await
        .values()
        .any(|unit| {
            unit.tenant_context.org_id == tenant_context.org_id
                && unit.tenant_context.workspace_id == tenant_context.workspace_id
                && unit.tenant_context.deployment_id == tenant_context.deployment_id
                && unit.unit_id == unit_id
                && unit.taxonomy_id == taxonomy_id
        })
}

async fn enterprise_membership_exists(
    state: &AppState,
    tenant_context: &TenantContext,
    membership_id: &str,
) -> bool {
    state
        .enterprise
        .org_unit_memberships
        .read()
        .await
        .values()
        .any(|membership| {
            membership.tenant_context.org_id == tenant_context.org_id
                && membership.tenant_context.workspace_id == tenant_context.workspace_id
                && membership.tenant_context.deployment_id == tenant_context.deployment_id
                && membership.membership_id == membership_id
        })
}

async fn enterprise_grant_exists(
    state: &AppState,
    tenant_context: &TenantContext,
    grant_id: &str,
) -> bool {
    state
        .enterprise
        .org_unit_access_grants
        .read()
        .await
        .values()
        .any(|grant| {
            grant.tenant_context.org_id == tenant_context.org_id
                && grant.tenant_context.workspace_id == tenant_context.workspace_id
                && grant.tenant_context.deployment_id == tenant_context.deployment_id
                && grant.grant_id == grant_id
        })
}

async fn connector_exists_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
    connector_id: &str,
) -> bool {
    state
        .enterprise
        .connectors
        .read()
        .await
        .values()
        .any(|connector| {
            connector.connector_id == connector_id && connector.tenant_matches(tenant_context)
        })
}

async fn connector_credential_exists_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
    connector_id: &str,
    credential_id: &str,
) -> bool {
    state
        .enterprise
        .connectors
        .read()
        .await
        .values()
        .find(|connector| {
            connector.connector_id == connector_id && connector.tenant_matches(tenant_context)
        })
        .is_some_and(|connector| {
            connector
                .credential_refs
                .iter()
                .any(|credential| credential.credential_id == credential_id)
        })
}

async fn source_binding_exists_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
    binding_id: &str,
) -> bool {
    state
        .enterprise
        .source_bindings
        .read()
        .await
        .values()
        .any(|binding| binding.binding_id == binding_id && binding.tenant_matches(tenant_context))
}

fn preview_taxonomy_id(value: Option<&str>) -> Result<String, (StatusCode, Json<Value>)> {
    value
        .map(|value| validate_enterprise_id("taxonomy_id", value))
        .transpose()
        .map(|value| value.unwrap_or_else(|| "organization_unit".to_string()))
}

fn reject_preview_destructive_action(
    action: Option<&str>,
    path: &str,
    blocking_errors: &mut Vec<EnterprisePreviewMessage>,
) -> bool {
    let Some(action) = action else {
        return false;
    };
    let normalized = action.trim().to_ascii_lowercase();
    if matches!(
        normalized.as_str(),
        "delete" | "hard_delete" | "destroy" | "purge" | "remove"
    ) {
        push_preview_error(
            blocking_errors,
            "ENTERPRISE_PREVIEW_DESTRUCTIVE_ACTION_NOT_ALLOWED",
            path.to_string(),
            "onboarding preview does not allow destructive runtime actions",
        );
        return true;
    }
    false
}

fn preview_operation(kind: &'static str, id: String, exists: bool) -> EnterprisePreviewOperation {
    EnterprisePreviewOperation {
        kind,
        id,
        action: if exists { "noop" } else { "create" },
        status: if exists {
            "already_exists"
        } else {
            "would_create"
        },
    }
}

fn push_preview_error(
    messages: &mut Vec<EnterprisePreviewMessage>,
    code: &'static str,
    path: impl Into<String>,
    message: impl Into<String>,
) {
    messages.push(EnterprisePreviewMessage {
        code,
        path: path.into(),
        message: message.into(),
    });
}

fn push_preview_warning(
    messages: &mut Vec<EnterprisePreviewMessage>,
    code: &'static str,
    path: impl Into<String>,
    message: impl Into<String>,
) {
    messages.push(EnterprisePreviewMessage {
        code,
        path: path.into(),
        message: message.into(),
    });
}

fn compact_preview_id_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(48)
        .collect::<String>()
}

fn access_permission_is_admin(permission: &AccessPermission) -> bool {
    serialized_enum_key(*permission) == "admin"
}

fn normalize_secret_ref_for_tenant(
    secret_ref: &SecretRef,
    tenant_context: &TenantContext,
) -> Result<SecretRef, (StatusCode, Json<Value>)> {
    if secret_ref.org_id != tenant_context.org_id
        || secret_ref.workspace_id != tenant_context.workspace_id
    {
        return Err(preview_bad_request(
            "ENTERPRISE_CONNECTOR_CREDENTIAL_TENANT_MISMATCH",
        ));
    }
    let provider = validate_enterprise_id("secret_provider", &secret_ref.provider)?;
    let secret_id = validate_external_id("secret_id", &secret_ref.secret_id)?;
    let name = secret_ref.name.trim();
    if name.is_empty() {
        return Err(preview_bad_request("ENTERPRISE_SECRET_NAME_INVALID"));
    }
    Ok(SecretRef {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        provider,
        secret_id,
        name: name.to_string(),
    })
}

fn validate_google_drive_source_binding_policy(
    connector_id: &str,
    source_type: &str,
    ingestion_policy: &IngestionPolicy,
) -> Result<(), (StatusCode, Json<Value>)> {
    if connector_id != GOOGLE_DRIVE_PROVIDER && source_type != GOOGLE_DRIVE_SOURCE_TYPE {
        return Ok(());
    }
    if source_type != GOOGLE_DRIVE_SOURCE_TYPE {
        return Err(preview_bad_request(
            "ENTERPRISE_GOOGLE_DRIVE_SOURCE_TYPE_REQUIRED",
        ));
    }
    if !ingestion_policy.allow_prompt_context && ingestion_policy.allow_indexing {
        return Err(preview_bad_request(
            "ENTERPRISE_GOOGLE_DRIVE_INDEXING_REQUIRES_PROMPT_CONTEXT_POLICY",
        ));
    }
    Ok(())
}

fn preview_bad_request(code: impl Into<String>) -> (StatusCode, Json<Value>) {
    let code = code.into();
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "code": code,
            "message": "enterprise onboarding preview validation failed"
        })),
    )
}

fn enterprise_admin_role(role: &str) -> bool {
    matches!(
        role.trim().to_ascii_lowercase().as_str(),
        "admin"
            | "owner"
            | "org:admin"
            | "organization:admin"
            | "workspace:admin"
            | "enterprise:admin"
            | "reconfigure"
    )
}
