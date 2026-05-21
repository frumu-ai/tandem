use std::collections::HashMap;

use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_enterprise_contract::{
    OrganizationUnit, OrganizationUnitKind, OrganizationUnitState, PrincipalRef, RequestPrincipal,
    TenantContext, VerifiedTenantContext,
};

use crate::{util::time::now_ms, AppState};

type EnterpriseResult<T> = Result<Json<T>, (StatusCode, Json<Value>)>;

#[derive(Debug, Serialize)]
struct EnterpriseAdminResponseBase {
    tenant_context: TenantContext,
    request_principal: RequestPrincipal,
    bridge_state: &'static str,
    status: &'static str,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct EnterpriseOrgUnitsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    org_units: Vec<OrganizationUnit>,
    count: usize,
}

#[derive(Debug, Serialize)]
struct EnterpriseSourceBindingsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    source_bindings: Vec<serde_json::Value>,
    count: usize,
}

#[derive(Debug, Deserialize)]
struct CreateOrganizationUnitRequest {
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

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/enterprise/org-units",
            get(list_org_units).post(create_org_unit),
        )
        .route(
            "/enterprise/source-bindings",
            get(list_source_bindings).post(create_source_binding),
        )
        .route(
            "/enterprise/source-bindings/{binding_id}",
            patch(update_source_binding),
        )
}

async fn list_org_units(
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

async fn create_org_unit(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<CreateOrganizationUnitRequest>,
) -> EnterpriseResult<EnterpriseAdminResponseBase> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;

    let unit_id = validate_enterprise_id("unit_id", &input.unit_id)?;
    let taxonomy_id = validate_enterprise_id(
        "taxonomy_id",
        input.taxonomy_id.as_deref().unwrap_or("organization_unit"),
    )?;
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

async fn list_source_bindings(
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> Json<EnterpriseSourceBindingsResponse> {
    Json(EnterpriseSourceBindingsResponse {
        base: noop_base(tenant_context, request_principal),
        source_bindings: Vec::new(),
        count: 0,
    })
}

async fn create_source_binding(
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<EnterpriseAdminResponseBase> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    Ok(Json(noop_base(tenant_context, request_principal)))
}

async fn update_source_binding(
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<EnterpriseAdminResponseBase> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    Ok(Json(noop_base(tenant_context, request_principal)))
}

fn storage_base(
    tenant_context: TenantContext,
    request_principal: RequestPrincipal,
) -> EnterpriseAdminResponseBase {
    EnterpriseAdminResponseBase {
        tenant_context,
        request_principal,
        bridge_state: "storage_backed",
        status: "ok",
        message: "enterprise admin storage is configured",
    }
}

fn noop_base(
    tenant_context: TenantContext,
    request_principal: RequestPrincipal,
) -> EnterpriseAdminResponseBase {
    EnterpriseAdminResponseBase {
        tenant_context,
        request_principal,
        bridge_state: "absent",
        status: "noop",
        message: "enterprise admin storage is not configured",
    }
}

fn require_enterprise_admin(
    request_principal: &RequestPrincipal,
    verified_tenant_context: Option<&VerifiedTenantContext>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if enterprise_admin_allowed_for_mutation(request_principal, verified_tenant_context) {
        return Ok(());
    }
    Err((
        StatusCode::FORBIDDEN,
        Json(json!({
            "code": "ENTERPRISE_ADMIN_REQUIRED",
            "message": "enterprise admin access is required for this mutation"
        })),
    ))
}

fn enterprise_admin_allowed_for_mutation(
    request_principal: &RequestPrincipal,
    verified_tenant_context: Option<&VerifiedTenantContext>,
) -> bool {
    if let Some(verified) = verified_tenant_context {
        return verified
            .roles
            .iter()
            .any(|role| is_enterprise_admin_role(role));
    }

    matches!(
        request_principal.source.as_str(),
        "api_token" | "control_panel" | "local_api_token" | "local_control_panel"
    )
}

fn is_enterprise_admin_role(role: &str) -> bool {
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

fn validate_enterprise_id(field: &str, value: &str) -> Result<String, (StatusCode, Json<Value>)> {
    let value = value.trim();
    if value.is_empty() || value.len() > 96 {
        return Err(bad_request(format!("ENTERPRISE_{field}_INVALID")));
    }
    if !value
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '-')
    {
        return Err(bad_request(format!("ENTERPRISE_{field}_INVALID")));
    }
    Ok(value.to_string())
}

fn bad_request(code: impl Into<String>) -> (StatusCode, Json<Value>) {
    let code = code.into();
    (
        StatusCode::BAD_REQUEST,
        Json(json!({
            "code": code,
            "message": "enterprise request validation failed"
        })),
    )
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

fn internal_error(code: impl Into<String>) -> (StatusCode, Json<Value>) {
    let code = code.into();
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "code": code,
            "message": "enterprise storage operation failed"
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_enterprise_contract::{AuthorityChain, HumanActor};

    fn verified_with_roles(roles: Vec<&str>) -> VerifiedTenantContext {
        let request_principal = RequestPrincipal::authenticated_user("user-a", "tandem-web");
        VerifiedTenantContext {
            tenant_context: TenantContext::explicit_user_workspace(
                "org-a",
                "workspace-a",
                Some("deployment-a".to_string()),
                "user-a",
            ),
            human_actor: HumanActor::tandem_user("user-a"),
            authority_chain: AuthorityChain::from_request(request_principal),
            roles: roles.into_iter().map(ToOwned::to_owned).collect(),
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            issued_at_ms: 1_000,
            expires_at_ms: 2_000,
            assertion_id: "assertion-a".to_string(),
        }
    }

    #[test]
    fn hosted_enterprise_mutations_require_signed_admin_role() {
        let local = RequestPrincipal::authenticated_user("user-a", "api_token");
        assert!(enterprise_admin_allowed_for_mutation(&local, None));

        let member = RequestPrincipal::authenticated_user("user-a", "tandem-web");
        assert!(!enterprise_admin_allowed_for_mutation(
            &member,
            Some(&verified_with_roles(vec!["member"]))
        ));
        assert!(enterprise_admin_allowed_for_mutation(
            &member,
            Some(&verified_with_roles(vec!["workspace:admin"]))
        ));
    }
}
