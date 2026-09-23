// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::HashMap;

use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::routing::{get, patch};
use axum::Json;
use axum::Router;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_enterprise_contract::{
    hosted_policy::HOSTED_TAXONOMY_ID, AccessEffect, AccessPermission, DataClass, OrganizationUnit,
    OrganizationUnitAccessGrant, OrganizationUnitKind, OrganizationUnitMembership,
    OrganizationUnitMembershipSource, OrganizationUnitState, PrincipalKind, PrincipalRef,
    RequestPrincipal, ResourceKind, ResourceRef, ScopedGrant, TenantContext, VerifiedTenantContext,
};

use tandem_server::{now_ms, AppState};

use super::routes_enterprise::{
    bad_request, internal_error, require_enterprise_admin, storage_base, validate_enterprise_id,
    validate_external_id, EnterpriseAdminResponseBase, EnterpriseResult,
};

async fn require_org_unit_grant_admin<'a>(
    state: &'a AppState,
    request_principal: &RequestPrincipal,
    verified: Option<&VerifiedTenantContext>,
) -> Result<Option<tokio::sync::MutexGuard<'a, ()>>, (StatusCode, Json<Value>)> {
    match state.hosted_org_unit_grant_mutation_guard(verified).await {
        Ok(Some(guard)) => Ok(Some(guard)),
        Ok(None) => require_enterprise_admin(request_principal, verified).map(|()| None),
        Err(_) => Err((
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"code": "HOSTED_ADMIN_REQUIRED"})),
        )),
    }
}

#[derive(Debug, Serialize)]
pub(super) struct EnterpriseOrgUnitsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    org_units: Vec<OrganizationUnit>,
    count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct EnterpriseOrgUnitMembershipsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    memberships: Vec<OrganizationUnitMembership>,
    count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct EnterpriseOrgUnitAccessGrantsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    access_grants: Vec<OrganizationUnitAccessGrant>,
    count: usize,
}

#[derive(Debug, Serialize)]
pub(super) struct EnterpriseOrgUnitEffectiveGrantsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    grants: Vec<ScopedGrant>,
    count: usize,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateOrganizationUnitRequest {
    unit_id: String,
    display_name: String,
    #[serde(default)]
    taxonomy_id: Option<String>,
    #[serde(default)]
    kind: OrganizationUnitKind,
    #[serde(default)]
    parent_unit_id: Option<String>,
    #[serde(default)]
    state: OrganizationUnitState,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    labels: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateOrganizationUnitMembershipRequest {
    #[serde(default)]
    membership_id: Option<String>,
    unit_id: String,
    #[serde(default)]
    taxonomy_id: Option<String>,
    #[serde(default = "default_member_kind")]
    member_kind: PrincipalKind,
    member_id: String,
    #[serde(default)]
    source: OrganizationUnitMembershipSource,
    #[serde(default)]
    state: OrganizationUnitState,
    #[serde(default)]
    expires_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UpdateOrganizationUnitMembershipRequest {
    state: OrganizationUnitState,
    #[serde(default)]
    expires_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct CreateOrganizationUnitAccessGrantRequest {
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
    state: OrganizationUnitState,
    #[serde(default)]
    expires_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct UpdateOrganizationUnitAccessGrantRequest {
    state: OrganizationUnitState,
    #[serde(default)]
    expires_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct EffectiveOrgUnitGrantsQuery {
    #[serde(default = "default_member_kind")]
    member_kind: PrincipalKind,
    member_id: String,
}

fn default_member_kind() -> PrincipalKind {
    PrincipalKind::HumanUser
}

pub(super) fn registry_error(reason: &'static str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(serde_json::json!({
            "code": "ENTERPRISE_HOSTED_REGISTRY_UNAVAILABLE", "reason": reason,
        })),
    )
}

fn require_local_registry_owner(
    taxonomy: &str,
    source: Option<OrganizationUnitMembershipSource>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if taxonomy == HOSTED_TAXONOMY_ID
        || source == Some(OrganizationUnitMembershipSource::HostedControlPlane)
    {
        return Err(bad_request("ENTERPRISE_HOSTED_REGISTRY_READ_ONLY"));
    }
    Ok(())
}

#[test]
fn hosted_policy_registry_ownership_cannot_be_claimed_by_local_authoring() {
    assert!(require_local_registry_owner("organization_unit", None).is_ok());
    assert!(require_local_registry_owner(
        "organization_unit",
        Some(OrganizationUnitMembershipSource::Scim)
    )
    .is_ok());
    assert!(require_local_registry_owner(HOSTED_TAXONOMY_ID, None).is_err());
    assert!(require_local_registry_owner(
        "organization_unit",
        Some(OrganizationUnitMembershipSource::HostedControlPlane)
    )
    .is_err());
}

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/enterprise/org-units",
            get(list_org_units).post(create_org_unit),
        )
        .route(
            "/enterprise/org-unit-memberships",
            get(list_org_unit_memberships).post(create_org_unit_membership),
        )
        .route(
            "/enterprise/org-unit-memberships/{membership_id}",
            patch(update_org_unit_membership),
        )
        .route(
            "/enterprise/org-unit-access-grants",
            get(list_org_unit_access_grants).post(create_org_unit_access_grant),
        )
        .route(
            "/enterprise/org-unit-access-grants/effective",
            get(list_effective_org_unit_grants),
        )
        .route(
            "/enterprise/org-unit-access-grants/{grant_id}",
            patch(update_org_unit_access_grant),
        )
}

pub(super) async fn list_org_units(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> EnterpriseResult<EnterpriseOrgUnitsResponse> {
    let mut org_units = state
        .enterprise_org_unit_view(&tenant_context)
        .await
        .map_err(registry_error)?
        .units;
    org_units.sort_by(|left, right| {
        left.taxonomy_id
            .cmp(&right.taxonomy_id)
            .then_with(|| left.unit_id.cmp(&right.unit_id))
    });

    Ok(Json(EnterpriseOrgUnitsResponse {
        base: storage_base(tenant_context, request_principal),
        count: org_units.len(),
        org_units,
    }))
}

pub(super) async fn list_org_unit_memberships(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> EnterpriseResult<EnterpriseOrgUnitMembershipsResponse> {
    let mut memberships = state
        .enterprise_org_unit_view(&tenant_context)
        .await
        .map_err(registry_error)?
        .memberships;
    memberships.sort_by(|left, right| {
        left.unit
            .id
            .cmp(&right.unit.id)
            .then_with(|| left.member.id.cmp(&right.member.id))
            .then_with(|| left.membership_id.cmp(&right.membership_id))
    });

    Ok(Json(EnterpriseOrgUnitMembershipsResponse {
        base: storage_base(tenant_context, request_principal),
        count: memberships.len(),
        memberships,
    }))
}

pub(super) async fn list_org_unit_access_grants(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> EnterpriseResult<EnterpriseOrgUnitAccessGrantsResponse> {
    let mut access_grants = state
        .enterprise_org_unit_view(&tenant_context)
        .await
        .map_err(registry_error)?
        .access_grants;
    access_grants.sort_by(|left, right| {
        left.unit
            .id
            .cmp(&right.unit.id)
            .then_with(|| left.resource.resource_id.cmp(&right.resource.resource_id))
            .then_with(|| left.grant_id.cmp(&right.grant_id))
    });

    Ok(Json(EnterpriseOrgUnitAccessGrantsResponse {
        base: storage_base(tenant_context, request_principal),
        count: access_grants.len(),
        access_grants,
    }))
}

pub(super) async fn list_effective_org_unit_grants(
    State(state): State<AppState>,
    Query(query): Query<EffectiveOrgUnitGrantsQuery>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> EnterpriseResult<EnterpriseOrgUnitEffectiveGrantsResponse> {
    let member_id = validate_external_id("member_id", &query.member_id)?;
    let member = PrincipalRef::new(query.member_kind, member_id);
    let now = now_ms();
    let view = state
        .enterprise_org_unit_view(&tenant_context)
        .await
        .map_err(registry_error)?;
    let memberships: Vec<_> = view
        .memberships
        .iter()
        .filter(|membership| {
            org_unit_membership_tenant_matches(membership, &tenant_context)
                && membership.member == member
                && membership.is_active_at(now)
        })
        .cloned()
        .collect();
    let mut grants = Vec::new();
    for access_grant in view
        .access_grants
        .iter()
        .filter(|grant| org_unit_access_grant_tenant_matches(grant, &tenant_context))
    {
        for membership in &memberships {
            if let Some(grant) = access_grant.to_scoped_grant_for_membership(membership, now) {
                grants.push(grant);
            }
        }
    }
    grants.sort_by(|left, right| left.grant_id.cmp(&right.grant_id));

    Ok(Json(EnterpriseOrgUnitEffectiveGrantsResponse {
        base: storage_base(tenant_context, request_principal),
        count: grants.len(),
        grants,
    }))
}

pub(super) async fn create_org_unit(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<CreateOrganizationUnitRequest>,
) -> EnterpriseResult<EnterpriseAdminResponseBase> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let unit_id = validate_enterprise_id("unit_id", &input.unit_id)?;
    let taxonomy_id = input
        .taxonomy_id
        .as_deref()
        .map(|value| validate_enterprise_id("taxonomy_id", value))
        .transpose()?
        .unwrap_or_else(|| "organization_unit".to_string());
    require_local_registry_owner(&taxonomy_id, None)?;
    let display_name = input.display_name.trim().to_string();
    if display_name.is_empty() {
        return Err(bad_request("ENTERPRISE_ORG_UNIT_DISPLAY_NAME_REQUIRED"));
    }
    let parent_unit_id = input
        .parent_unit_id
        .as_deref()
        .map(|value| validate_enterprise_id("parent_unit_id", value))
        .transpose()?;
    let labels = input
        .labels
        .into_iter()
        .map(|label| label.trim().to_string())
        .filter(|label| !label.is_empty())
        .take(32)
        .collect::<Vec<_>>();
    let actor_id = request_principal
        .actor_id
        .clone()
        .unwrap_or_else(|| request_principal.source.clone());
    let mut unit = OrganizationUnit::active(
        unit_id,
        tenant_context.clone(),
        display_name,
        input.kind,
        PrincipalRef::human_user(actor_id),
        now_ms(),
    )
    .with_taxonomy_id(taxonomy_id)
    .with_state(input.state, now_ms());
    unit.parent_unit_id = parent_unit_id;
    unit.description = input
        .description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned);
    unit.labels = labels;

    {
        let mut registry = state.enterprise.org_units.write().await;
        registry.insert(enterprise_org_unit_key(&unit), unit);
        persist_enterprise_org_units(&state.enterprise.org_units_path, &registry).await?;
    }

    Ok(Json(EnterpriseAdminResponseBase {
        message: "enterprise organization unit saved",
        ..storage_base(tenant_context, request_principal)
    }))
}

pub(super) async fn create_org_unit_membership(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<CreateOrganizationUnitMembershipRequest>,
) -> EnterpriseResult<EnterpriseOrgUnitMembershipsResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let unit_id = validate_enterprise_id("unit_id", &input.unit_id)?;
    let taxonomy_id = input
        .taxonomy_id
        .as_deref()
        .map(|value| validate_enterprise_id("taxonomy_id", value))
        .transpose()?
        .unwrap_or_else(|| "organization_unit".to_string());
    require_local_registry_owner(&taxonomy_id, Some(input.source))?;
    ensure_org_unit_for_tenant(&state, &tenant_context, &taxonomy_id, &unit_id).await?;
    let member_id = validate_external_id("member_id", &input.member_id)?;
    let membership_id = input
        .membership_id
        .as_deref()
        .map(|value| validate_enterprise_id("membership_id", value))
        .transpose()?
        .unwrap_or_else(|| {
            format!(
                "membership-{}-{}-{}",
                taxonomy_id,
                unit_id,
                compact_membership_id_segment(&member_id)
            )
        });
    let mut membership = OrganizationUnitMembership::active(
        membership_id,
        tenant_context.clone(),
        PrincipalRef::organization_unit(format!("{taxonomy_id}/{unit_id}")),
        PrincipalRef::new(input.member_kind, member_id),
        input.source,
        now_ms(),
    );
    membership.state = input.state;
    membership.expires_at_ms = input.expires_at_ms;

    {
        let mut registry = state.enterprise.org_unit_memberships.write().await;
        registry.insert(
            enterprise_org_unit_membership_key(&membership),
            membership.clone(),
        );
        persist_enterprise_org_unit_memberships(
            &state.enterprise.org_unit_memberships_path,
            &registry,
        )
        .await?;
    }

    Ok(Json(EnterpriseOrgUnitMembershipsResponse {
        base: storage_base(tenant_context, request_principal),
        count: 1,
        memberships: vec![membership],
    }))
}

pub(super) async fn create_org_unit_access_grant(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<CreateOrganizationUnitAccessGrantRequest>,
) -> EnterpriseResult<EnterpriseOrgUnitAccessGrantsResponse> {
    let unit_id = validate_enterprise_id("unit_id", &input.unit_id)?;
    let taxonomy_id = input
        .taxonomy_id
        .as_deref()
        .map(|value| validate_enterprise_id("taxonomy_id", value))
        .transpose()?
        .unwrap_or_else(|| "organization_unit".to_string());
    ensure_org_unit_for_tenant(&state, &tenant_context, &taxonomy_id, &unit_id).await?;
    let resource_id = validate_external_id("resource_id", &input.resource_id)?;
    let grant_id = input
        .grant_id
        .as_deref()
        .map(|value| validate_enterprise_id("grant_id", value))
        .transpose()?
        .unwrap_or_else(|| format!("grant-{taxonomy_id}-{unit_id}-{resource_id}"));
    let mut resource = ResourceRef::new(
        tenant_context.org_id.clone(),
        tenant_context.workspace_id.clone(),
        input.resource_kind,
        resource_id,
    );
    if let Some(project_id) = input
        .project_id
        .as_deref()
        .map(|value| validate_enterprise_id("project_id", value))
        .transpose()?
    {
        resource = resource.with_project_id(project_id);
    }
    if let Some(path_prefix) = input
        .path_prefix
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        resource = resource.with_path_prefix(path_prefix.to_string());
    }
    let now = now_ms();
    let mut grant = OrganizationUnitAccessGrant::active(
        grant_id,
        tenant_context.clone(),
        PrincipalRef::organization_unit(format!("{taxonomy_id}/{unit_id}")),
        resource,
        now,
    )
    .with_effect(input.effect)
    .with_permissions(input.permissions)
    .with_data_classes(input.data_classes)
    .with_tool_patterns(input.tool_patterns);
    grant.state = input.state;
    grant.expires_at_ms = input.expires_at_ms;

    let _policy_commit_guard = require_org_unit_grant_admin(
        &state,
        &request_principal,
        verified_tenant_context.as_deref(),
    )
    .await?;
    {
        let mut registry = state.enterprise.org_unit_access_grants.write().await;
        let mut next = registry.clone();
        next.insert(enterprise_org_unit_access_grant_key(&grant), grant.clone());
        state
            .persist_enterprise_org_unit_access_grants_snapshot(&next)
            .await
            .map_err(|_| internal_error("ENTERPRISE_ORG_UNIT_ACCESS_GRANTS_PERSIST_FAILED"))?;
        *registry = next;
    }

    Ok(Json(EnterpriseOrgUnitAccessGrantsResponse {
        base: storage_base(tenant_context, request_principal),
        count: 1,
        access_grants: vec![grant],
    }))
}

pub(super) async fn update_org_unit_membership(
    State(state): State<AppState>,
    Path(membership_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<UpdateOrganizationUnitMembershipRequest>,
) -> EnterpriseResult<EnterpriseOrgUnitMembershipsResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let membership_id = validate_enterprise_id("membership_id", &membership_id)?;
    let updated = {
        let mut registry = state.enterprise.org_unit_memberships.write().await;
        let Some(membership) = registry.values_mut().find(|membership| {
            membership.membership_id == membership_id
                && org_unit_membership_tenant_matches(membership, &tenant_context)
        }) else {
            return Err(super::routes_enterprise::not_found(
                "ENTERPRISE_ORG_UNIT_MEMBERSHIP_NOT_FOUND",
            ));
        };
        membership.state = input.state;
        membership.expires_at_ms = input.expires_at_ms;
        let updated = membership.clone();
        persist_enterprise_org_unit_memberships(
            &state.enterprise.org_unit_memberships_path,
            &registry,
        )
        .await?;
        updated
    };

    Ok(Json(EnterpriseOrgUnitMembershipsResponse {
        base: storage_base(tenant_context, request_principal),
        count: 1,
        memberships: vec![updated],
    }))
}

pub(super) async fn update_org_unit_access_grant(
    State(state): State<AppState>,
    Path(grant_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<UpdateOrganizationUnitAccessGrantRequest>,
) -> EnterpriseResult<EnterpriseOrgUnitAccessGrantsResponse> {
    let _policy_commit_guard = require_org_unit_grant_admin(
        &state,
        &request_principal,
        verified_tenant_context.as_deref(),
    )
    .await?;
    let grant_id = validate_enterprise_id("grant_id", &grant_id)?;
    let updated = {
        let mut registry = state.enterprise.org_unit_access_grants.write().await;
        let mut next = registry.clone();
        let Some(grant) = next.values_mut().find(|grant| {
            grant.grant_id == grant_id
                && org_unit_access_grant_tenant_matches(grant, &tenant_context)
        }) else {
            return Err(super::routes_enterprise::not_found(
                "ENTERPRISE_ORG_UNIT_ACCESS_GRANT_NOT_FOUND",
            ));
        };
        grant.state = input.state;
        grant.expires_at_ms = input.expires_at_ms;
        grant.updated_at_ms = now_ms();
        let updated = grant.clone();
        state
            .persist_enterprise_org_unit_access_grants_snapshot(&next)
            .await
            .map_err(|_| internal_error("ENTERPRISE_ORG_UNIT_ACCESS_GRANTS_PERSIST_FAILED"))?;
        *registry = next;
        updated
    };

    Ok(Json(EnterpriseOrgUnitAccessGrantsResponse {
        base: storage_base(tenant_context, request_principal),
        count: 1,
        access_grants: vec![updated],
    }))
}

fn organization_unit_tenant_matches(
    unit: &OrganizationUnit,
    tenant_context: &TenantContext,
) -> bool {
    unit.tenant_context.org_id == tenant_context.org_id
        && unit.tenant_context.workspace_id == tenant_context.workspace_id
        && unit.tenant_context.deployment_id == tenant_context.deployment_id
}

fn org_unit_membership_tenant_matches(
    membership: &OrganizationUnitMembership,
    tenant_context: &TenantContext,
) -> bool {
    membership.tenant_context.org_id == tenant_context.org_id
        && membership.tenant_context.workspace_id == tenant_context.workspace_id
        && membership.tenant_context.deployment_id == tenant_context.deployment_id
}

fn org_unit_access_grant_tenant_matches(
    grant: &OrganizationUnitAccessGrant,
    tenant_context: &TenantContext,
) -> bool {
    grant.tenant_context.org_id == tenant_context.org_id
        && grant.tenant_context.workspace_id == tenant_context.workspace_id
        && grant.tenant_context.deployment_id == tenant_context.deployment_id
}

async fn ensure_org_unit_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
    taxonomy_id: &str,
    unit_id: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    if state
        .enterprise_org_unit_view(tenant_context)
        .await
        .map_err(registry_error)?
        .units
        .iter()
        .any(|unit| {
            unit.taxonomy_id == taxonomy_id
                && unit.unit_id == unit_id
                && organization_unit_tenant_matches(unit, tenant_context)
        })
    {
        Ok(())
    } else {
        Err(super::routes_enterprise::not_found(
            "ENTERPRISE_ORG_UNIT_NOT_FOUND",
        ))
    }
}

fn enterprise_org_unit_key(unit: &OrganizationUnit) -> String {
    let deployment = unit
        .tenant_context
        .deployment_id
        .as_deref()
        .unwrap_or("local");
    format!(
        "{}::{}::{}::{}::{}",
        unit.tenant_context.org_id,
        unit.tenant_context.workspace_id,
        deployment,
        unit.taxonomy_id,
        unit.unit_id
    )
}

fn enterprise_org_unit_membership_key(membership: &OrganizationUnitMembership) -> String {
    let deployment = membership
        .tenant_context
        .deployment_id
        .as_deref()
        .unwrap_or("local");
    format!(
        "{}::{}::{}::{}",
        membership.tenant_context.org_id,
        membership.tenant_context.workspace_id,
        deployment,
        membership.membership_id
    )
}

fn enterprise_org_unit_access_grant_key(grant: &OrganizationUnitAccessGrant) -> String {
    let deployment = grant
        .tenant_context
        .deployment_id
        .as_deref()
        .unwrap_or("local");
    format!(
        "{}::{}::{}::{}",
        grant.tenant_context.org_id, grant.tenant_context.workspace_id, deployment, grant.grant_id
    )
}

fn compact_membership_id_segment(value: &str) -> String {
    let mut segment = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .chars()
        .take(48)
        .collect::<String>();
    if segment.is_empty() {
        segment = "member".to_string();
    }
    segment
}

async fn persist_enterprise_org_units(
    path: &std::path::Path,
    registry: &HashMap<String, OrganizationUnit>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| internal_error("ENTERPRISE_ORG_UNITS_PERSIST_FAILED"))?;
    }
    let payload = serde_json::to_vec_pretty(registry)
        .map_err(|_| internal_error("ENTERPRISE_ORG_UNITS_PERSIST_FAILED"))?;
    tokio::fs::write(path, payload)
        .await
        .map_err(|_| internal_error("ENTERPRISE_ORG_UNITS_PERSIST_FAILED"))?;
    Ok(())
}

async fn persist_enterprise_org_unit_memberships(
    path: &std::path::Path,
    registry: &HashMap<String, OrganizationUnitMembership>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| internal_error("ENTERPRISE_ORG_UNIT_MEMBERSHIPS_PERSIST_FAILED"))?;
    }
    let payload = serde_json::to_vec_pretty(registry)
        .map_err(|_| internal_error("ENTERPRISE_ORG_UNIT_MEMBERSHIPS_PERSIST_FAILED"))?;
    tokio::fs::write(path, payload)
        .await
        .map_err(|_| internal_error("ENTERPRISE_ORG_UNIT_MEMBERSHIPS_PERSIST_FAILED"))?;
    Ok(())
}

#[cfg(test)]
mod hosted_policy_grant_tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use base64::Engine as _;
    use ed25519_dalek::Signer;
    use serde_json::json;
    use tandem_enterprise_contract::{
        hosted_policy::role_capabilities, AuthorityChain, HumanActor, TenantContextAssertionClaims,
        TenantContextAssertionHeader,
    };
    use tower::ServiceExt;

    fn claims(actor: &str, role: &str, version: u64, nonce: &str) -> TenantContextAssertionClaims {
        let now = now_ms();
        let tenant =
            TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), actor);
        let principal = RequestPrincipal::authenticated_user(actor, "tandem-web");
        let mut claims = TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "tandem-runtime",
            now,
            now + 60_000,
            format!("grant-{actor}-{version}-{nonce}"),
            tenant,
            HumanActor::tandem_user(actor),
            AuthorityChain::from_request(principal),
            vec![format!("hosted:role:{role}")],
        );
        claims.policy_version = Some(version);
        claims.capabilities = role_capabilities(role)
            .into_iter()
            .map(str::to_string)
            .collect();
        claims
    }

    fn identity(actor: &str, role: &str, version: u64) -> VerifiedTenantContext {
        claims(actor, role, version, "direct").into()
    }

    fn policy(version: u64, alice_role: &str) -> Vec<u8> {
        let now = now_ms();
        serde_json::to_vec(&json!({
            "schema_version": 1, "policy_version": version,
            "organization_id": "org-a", "deployment_id": "dep-a",
            "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
            "users": [
                {"id": "alice", "email": null, "username": null, "role": alice_role,
                 "capabilities": role_capabilities(alice_role), "is_active": true,
                 "email_verified": true},
                {"id": "bob", "email": null, "username": null, "role": "member",
                 "capabilities": role_capabilities("member"), "is_active": true,
                 "email_verified": true}
            ],
            "org_units": [{"id": "eng", "slug": "eng", "display_name": "Engineering",
                           "kind": "department", "state": "active"}],
            "org_unit_memberships": [{"unit_id": "eng", "user_id": "alice"}],
            "deployment_grants": []
        }))
        .unwrap()
    }

    #[tokio::test]
    async fn hosted_admin_can_create_and_revoke_grant_but_member_and_downgraded_admin_cannot() {
        tandem_server::test_support::with_hosted_crypto_for_test(hosted_grant_handler_case()).await;
    }

    async fn hosted_grant_handler_case() {
        let state = tandem_server::test_support::test_state().await;
        state
            .install_hosted_policy_snapshot_for_test("org-a", "dep-a", &policy(4, "admin"))
            .unwrap();
        let alice = identity("alice", "admin", 4);
        let bob = identity("bob", "member", 4);
        let create = || -> CreateOrganizationUnitAccessGrantRequest {
            serde_json::from_value(json!({
                "grant_id": "grant-eng-private", "taxonomy_id": HOSTED_TAXONOMY_ID,
                "unit_id": "eng", "resource_kind": "document", "resource_id": "eng-private",
                "permissions": ["read"], "data_classes": ["customer_data"]
            }))
            .unwrap()
        };
        let call_create = |verified: VerifiedTenantContext| {
            let tenant = verified.tenant_context.clone();
            let principal =
                RequestPrincipal::authenticated_user(&verified.human_actor.actor_id, "tandem-web");
            create_org_unit_access_grant(
                State(state.clone()),
                Extension(tenant),
                Extension(principal),
                Some(Extension(verified)),
                Json(create()),
            )
        };
        assert_eq!(
            call_create(bob.clone()).await.unwrap_err().0,
            StatusCode::FORBIDDEN
        );
        assert!(call_create(alice.clone()).await.is_ok());
        assert_eq!(
            state.enterprise.org_unit_access_grants.read().await.len(),
            1
        );

        let revoke = |verified: VerifiedTenantContext| {
            let tenant = verified.tenant_context.clone();
            let principal =
                RequestPrincipal::authenticated_user(&verified.human_actor.actor_id, "tandem-web");
            update_org_unit_access_grant(
                State(state.clone()),
                Path("grant-eng-private".into()),
                Extension(tenant),
                Extension(principal),
                Some(Extension(verified)),
                Json(serde_json::from_value(json!({"state": "disabled"})).unwrap()),
            )
        };
        assert_eq!(revoke(bob).await.unwrap_err().0, StatusCode::FORBIDDEN);
        state
            .install_hosted_policy_snapshot_for_test("org-a", "dep-a", &policy(5, "owner"))
            .unwrap();
        let owner = identity("alice", "owner", 5);
        assert!(revoke(owner.clone()).await.is_ok());
        assert!(state
            .enterprise
            .org_unit_access_grants
            .read()
            .await
            .values()
            .all(|grant| { grant.state == OrganizationUnitState::Disabled }));
        assert!(call_create(owner.clone()).await.is_ok());
        state
            .install_hosted_policy_snapshot_for_test("org-a", "dep-a", &policy(6, "viewer"))
            .unwrap();
        assert_eq!(revoke(alice).await.unwrap_err().0, StatusCode::FORBIDDEN);
        assert_eq!(revoke(owner).await.unwrap_err().0, StatusCode::FORBIDDEN);
        assert_eq!(
            revoke(identity("alice", "viewer", 6)).await.unwrap_err().0,
            StatusCode::FORBIDDEN
        );
        let mut forged_enterprise_admin = identity("alice", "viewer", 6);
        forged_enterprise_admin.roles.push("admin".into());
        assert_eq!(
            revoke(forged_enterprise_admin).await.unwrap_err().0,
            StatusCode::FORBIDDEN
        );
        assert!(state
            .enterprise
            .org_unit_access_grants
            .read()
            .await
            .values()
            .all(|grant| { grant.state == OrganizationUnitState::Active }));
    }

    fn sign_claims(
        key: &ed25519_dalek::SigningKey,
        actor: &str,
        role: &str,
        version: u64,
        nonce: &str,
    ) -> String {
        let header = TenantContextAssertionHeader::ed25519("grant-key");
        let encoded_header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&header).unwrap());
        let encoded_claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims(actor, role, version, nonce)).unwrap());
        let input = format!("{encoded_header}.{encoded_claims}");
        let signature = key.sign(input.as_bytes());
        format!(
            "{input}.{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes())
        )
    }

    async fn signed_request(
        app: &Router,
        method: &str,
        path: &str,
        assertion: &str,
        body: Value,
    ) -> StatusCode {
        app.clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("content-type", "application/json")
                    .header("x-tandem-context-assertion", assertion)
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
            .status()
    }

    #[tokio::test]
    async fn signed_hosted_grant_http_rechecks_current_admin() {
        tandem_server::test_support::with_hosted_crypto_for_test(signed_hosted_grant_http_case())
            .await;
    }

    async fn signed_hosted_grant_http_case() {
        let state = tandem_server::test_support::test_state().await;
        let replay_path = state.enterprise.org_unit_access_grants_path.with_extension("replay.json");
        std::fs::create_dir_all(replay_path.parent().unwrap()).unwrap();
        let key = ed25519_dalek::SigningKey::from_bytes(&[41; 32]);
        let keyring = json!({"grant-key": {
            "purpose": "context_assertion",
            "public_key": base64::engine::general_purpose::URL_SAFE_NO_PAD
                .encode(key.verifying_key().to_bytes()),
            "organization_id": "org-a", "deployment_id": "dep-a",
            "allowed_audiences": ["tandem-runtime"], "status": "active"
        }}).to_string();
        tandem_server::test_support::install_hosted_assertion_security_for_test(
            &state, &keyring, &replay_path,
        );
        state.install_hosted_policy_snapshot_for_test("org-a", "dep-a", &policy(1, "admin"))
            .unwrap();
        let app = apply(Router::new())
            .layer(axum::middleware::from_fn_with_state(
                state.clone(), tandem_server::test_support::hosted_test_ingress,
            ))
            .with_state(state.clone());
        let create = json!({
            "grant_id": "signed-eng-private", "taxonomy_id": HOSTED_TAXONOMY_ID,
            "unit_id": "eng", "resource_kind": "document", "resource_id": "signed-private",
            "permissions": ["read"], "data_classes": ["customer_data"]
        });
        let update = json!({"state": "disabled"});
        let path = "/enterprise/org-unit-access-grants/signed-eng-private";
        assert_eq!(signed_request(&app, "POST", "/enterprise/org-unit-access-grants",
            &sign_claims(&key, "bob", "member", 1, "bob-create"), create.clone()).await,
            StatusCode::FORBIDDEN);
        assert_eq!(signed_request(&app, "POST", "/enterprise/org-unit-access-grants",
            &sign_claims(&key, "alice", "admin", 1, "alice-create"), create).await,
            StatusCode::OK);
        let forged_key = ed25519_dalek::SigningKey::from_bytes(&[42; 32]);
        assert_eq!(signed_request(&app, "PATCH", path,
            &sign_claims(&forged_key, "alice", "admin", 1, "forged-key"), update.clone()).await,
            StatusCode::FORBIDDEN);
        assert_eq!(signed_request(&app, "PATCH", path,
            &sign_claims(&key, "bob", "member", 1, "bob-update"), update.clone()).await,
            StatusCode::FORBIDDEN);
        assert_eq!(signed_request(&app, "PATCH", path,
            &sign_claims(&key, "alice", "admin", 1, "alice-update"), update.clone()).await,
            StatusCode::OK);
        state.install_hosted_policy_snapshot_for_test("org-a", "dep-a", &policy(2, "viewer"))
            .unwrap();
        assert_eq!(signed_request(&app, "PATCH", path,
            &sign_claims(&key, "alice", "admin", 1, "stale-admin"), update.clone()).await,
            StatusCode::FORBIDDEN);
        assert_eq!(signed_request(&app, "PATCH", path,
            &sign_claims(&key, "alice", "viewer", 2, "fresh-viewer"), update).await,
            StatusCode::FORBIDDEN);
        assert_eq!(state.enterprise.org_unit_access_grants.read().await.len(), 1);
        assert!(replay_path.is_file());
    }
}
