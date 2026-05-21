use std::collections::HashMap;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_enterprise_contract::{
    DataClass, IngestionPolicy, OrganizationUnit, OrganizationUnitKind, OrganizationUnitState,
    PrincipalRef, RequestPrincipal, ResourceRef, SourceBinding, SourceBindingState, TenantContext,
    VerifiedTenantContext,
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
    source_bindings: Vec<SourceBinding>,
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

#[derive(Debug, Deserialize)]
struct CreateSourceBindingRequest {
    binding_id: String,
    connector_id: String,
    source_type: String,
    native_source_id: String,
    #[serde(default)]
    source_root_label: Option<String>,
    resource_ref: ResourceRef,
    data_class: DataClass,
    #[serde(default)]
    state: SourceBindingState,
    #[serde(default)]
    credential_ref_id: Option<String>,
    #[serde(default)]
    ingestion_policy: IngestionPolicy,
}

#[derive(Debug, Deserialize)]
struct UpdateSourceBindingRequest {
    #[serde(default)]
    state: Option<SourceBindingState>,
    #[serde(default)]
    source_root_label: Option<String>,
    #[serde(default)]
    credential_ref_id: Option<String>,
    #[serde(default)]
    ingestion_policy: Option<IngestionPolicy>,
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
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> Json<EnterpriseSourceBindingsResponse> {
    let mut source_bindings: Vec<_> = state
        .enterprise_source_bindings
        .read()
        .await
        .values()
        .filter(|binding| binding.tenant_matches(&tenant_context))
        .cloned()
        .collect();
    source_bindings.sort_by(|left, right| left.binding_id.cmp(&right.binding_id));

    Json(EnterpriseSourceBindingsResponse {
        base: storage_base(tenant_context, request_principal),
        count: source_bindings.len(),
        source_bindings,
    })
}

async fn create_source_binding(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<CreateSourceBindingRequest>,
) -> EnterpriseResult<EnterpriseAdminResponseBase> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;

    let binding_id = validate_enterprise_id("binding_id", &input.binding_id)?;
    let connector_id = validate_enterprise_id("connector_id", &input.connector_id)?;
    let source_type = validate_enterprise_id("source_type", &input.source_type)?;
    let native_source_id = validate_external_id("native_source_id", &input.native_source_id)?;
    validate_resource_ref_matches_tenant(&input.resource_ref, &tenant_context)?;
    let credential_ref_id = input
        .credential_ref_id
        .as_deref()
        .map(|value| validate_enterprise_id("credential_ref_id", value))
        .transpose()?;
    let actor_id = request_principal
        .actor_id
        .clone()
        .unwrap_or_else(|| request_principal.source.clone());
    let mut binding = SourceBinding::enabled(
        binding_id,
        tenant_context.clone(),
        connector_id,
        source_type,
        native_source_id,
        input.resource_ref,
        input.data_class,
        PrincipalRef::human_user(actor_id),
        now_ms(),
    )
    .with_state(input.state, now_ms())
    .with_ingestion_policy(input.ingestion_policy);
    binding.source_root_label = normalized_optional_label(input.source_root_label);
    if let Some(credential_ref_id) = credential_ref_id {
        binding = binding.with_credential_ref_id(credential_ref_id);
    }

    {
        let mut registry = state.enterprise_source_bindings.write().await;
        registry.insert(enterprise_source_binding_key(&binding), binding);
        persist_enterprise_source_bindings(&state.enterprise_source_bindings_path, &registry)
            .await?;
    }

    Ok(Json(EnterpriseAdminResponseBase {
        message: "enterprise source binding saved",
        ..storage_base(tenant_context, request_principal)
    }))
}

async fn update_source_binding(
    State(state): State<AppState>,
    Path(binding_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<UpdateSourceBindingRequest>,
) -> EnterpriseResult<EnterpriseAdminResponseBase> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let binding_id = validate_enterprise_id("binding_id", &binding_id)?;

    {
        let mut registry = state.enterprise_source_bindings.write().await;
        let Some(binding) = registry.values_mut().find(|binding| {
            binding.binding_id == binding_id && binding.tenant_matches(&tenant_context)
        }) else {
            return Err(not_found("ENTERPRISE_SOURCE_BINDING_NOT_FOUND"));
        };
        if let Some(state) = input.state {
            binding.state = state;
        }
        if let Some(label) = input.source_root_label {
            binding.source_root_label = normalized_optional_label(Some(label));
        }
        if let Some(credential_ref_id) = input.credential_ref_id {
            binding.credential_ref_id = Some(validate_enterprise_id(
                "credential_ref_id",
                &credential_ref_id,
            )?);
        }
        if let Some(ingestion_policy) = input.ingestion_policy {
            binding.ingestion_policy = ingestion_policy;
        }
        binding.updated_at_ms = now_ms();
        persist_enterprise_source_bindings(&state.enterprise_source_bindings_path, &registry)
            .await?;
    }

    Ok(Json(EnterpriseAdminResponseBase {
        message: "enterprise source binding updated",
        ..storage_base(tenant_context, request_principal)
    }))
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

fn validate_external_id(field: &str, value: &str) -> Result<String, (StatusCode, Json<Value>)> {
    let value = value.trim();
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return Err(bad_request(format!("ENTERPRISE_{field}_INVALID")));
    }
    Ok(value.to_string())
}

fn validate_resource_ref_matches_tenant(
    resource_ref: &ResourceRef,
    tenant_context: &TenantContext,
) -> Result<(), (StatusCode, Json<Value>)> {
    if resource_ref.organization_id != tenant_context.org_id
        || resource_ref.workspace_id != tenant_context.workspace_id
    {
        return Err(bad_request(
            "ENTERPRISE_SOURCE_BINDING_RESOURCE_TENANT_MISMATCH",
        ));
    }
    Ok(())
}

fn normalized_optional_label(label: Option<String>) -> Option<String> {
    label
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
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

fn enterprise_source_binding_key(binding: &SourceBinding) -> String {
    let deployment = binding
        .tenant_context
        .deployment_id
        .as_deref()
        .unwrap_or("local");
    format!(
        "{}::{}::{}::{}",
        binding.tenant_context.org_id,
        binding.tenant_context.workspace_id,
        deployment,
        binding.binding_id
    )
}

async fn persist_enterprise_source_bindings(
    path: &std::path::Path,
    registry: &HashMap<String, SourceBinding>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| internal_error("ENTERPRISE_SOURCE_BINDINGS_PERSIST_FAILED"))?;
    }
    let payload = serde_json::to_vec_pretty(registry)
        .map_err(|_| internal_error("ENTERPRISE_SOURCE_BINDINGS_PERSIST_FAILED"))?;
    tokio::fs::write(path, payload)
        .await
        .map_err(|_| internal_error("ENTERPRISE_SOURCE_BINDINGS_PERSIST_FAILED"))?;
    Ok(())
}

fn not_found(code: impl Into<String>) -> (StatusCode, Json<Value>) {
    let code = code.into();
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "code": code,
            "message": "enterprise resource was not found"
        })),
    )
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
