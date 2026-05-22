use std::collections::HashMap;

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tandem_enterprise_contract::{
    OrganizationUnit, OrganizationUnitKind, OrganizationUnitState, PrincipalRef, RequestPrincipal,
    TenantContext, VerifiedTenantContext,
};

use crate::{util::time::now_ms, AppState};

use super::routes_enterprise::{
    bad_request, internal_error, require_enterprise_admin, storage_base, validate_enterprise_id,
    EnterpriseAdminResponseBase, EnterpriseResult,
};

#[derive(Debug, Serialize)]
pub(super) struct EnterpriseOrgUnitsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    org_units: Vec<OrganizationUnit>,
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

fn organization_unit_tenant_matches(
    unit: &OrganizationUnit,
    tenant_context: &TenantContext,
) -> bool {
    unit.tenant_context.org_id == tenant_context.org_id
        && unit.tenant_context.workspace_id == tenant_context.workspace_id
        && unit.tenant_context.deployment_id == tenant_context.deployment_id
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
