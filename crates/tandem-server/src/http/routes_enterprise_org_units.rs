use std::collections::HashMap;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_enterprise_contract::{
    OrganizationUnit, OrganizationUnitKind, OrganizationUnitMembership,
    OrganizationUnitMembershipSource, OrganizationUnitState, PrincipalKind, PrincipalRef,
    RequestPrincipal, TenantContext, VerifiedTenantContext,
};

use crate::{util::time::now_ms, AppState};

use super::routes_enterprise::{
    bad_request, internal_error, require_enterprise_admin, storage_base, validate_enterprise_id,
    validate_external_id, EnterpriseAdminResponseBase, EnterpriseResult,
};

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

fn default_member_kind() -> PrincipalKind {
    PrincipalKind::HumanUser
}

pub(super) async fn list_org_units(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> Json<EnterpriseOrgUnitsResponse> {
    let mut org_units: Vec<_> = state
        .enterprise_org_units
        .read()
        .await
        .values()
        .filter(|unit| organization_unit_tenant_matches(unit, &tenant_context))
        .cloned()
        .collect();
    org_units.sort_by(|left, right| {
        left.taxonomy_id
            .cmp(&right.taxonomy_id)
            .then_with(|| left.unit_id.cmp(&right.unit_id))
    });

    Json(EnterpriseOrgUnitsResponse {
        base: storage_base(tenant_context, request_principal),
        count: org_units.len(),
        org_units,
    })
}

pub(super) async fn list_org_unit_memberships(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> Json<EnterpriseOrgUnitMembershipsResponse> {
    let mut memberships: Vec<_> = state
        .enterprise_org_unit_memberships
        .read()
        .await
        .values()
        .filter(|membership| org_unit_membership_tenant_matches(membership, &tenant_context))
        .cloned()
        .collect();
    memberships.sort_by(|left, right| {
        left.unit
            .id
            .cmp(&right.unit.id)
            .then_with(|| left.member.id.cmp(&right.member.id))
            .then_with(|| left.membership_id.cmp(&right.membership_id))
    });

    Json(EnterpriseOrgUnitMembershipsResponse {
        base: storage_base(tenant_context, request_principal),
        count: memberships.len(),
        memberships,
    })
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
        let mut registry = state.enterprise_org_units.write().await;
        registry.insert(enterprise_org_unit_key(&unit), unit);
        persist_enterprise_org_units(&state.enterprise_org_units_path, &registry).await?;
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
        let mut registry = state.enterprise_org_unit_memberships.write().await;
        registry.insert(
            enterprise_org_unit_membership_key(&membership),
            membership.clone(),
        );
        persist_enterprise_org_unit_memberships(
            &state.enterprise_org_unit_memberships_path,
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
        let mut registry = state.enterprise_org_unit_memberships.write().await;
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
            &state.enterprise_org_unit_memberships_path,
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

async fn ensure_org_unit_for_tenant(
    state: &AppState,
    tenant_context: &TenantContext,
    taxonomy_id: &str,
    unit_id: &str,
) -> Result<(), (StatusCode, Json<Value>)> {
    if state
        .enterprise_org_units
        .read()
        .await
        .values()
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
