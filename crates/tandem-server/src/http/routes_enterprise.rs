use axum::extract::Extension;
use axum::routing::{get, patch, post};
use axum::{Json, Router};
use serde::Serialize;
use tandem_enterprise_contract::{RequestPrincipal, TenantContext};

use crate::AppState;

#[derive(Debug, Serialize)]
struct EnterpriseAdminNoopResponse {
    tenant_context: TenantContext,
    request_principal: RequestPrincipal,
    bridge_state: &'static str,
    status: &'static str,
    message: &'static str,
}

#[derive(Debug, Serialize)]
struct EnterpriseOrgUnitsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminNoopResponse,
    org_units: Vec<serde_json::Value>,
    count: usize,
}

#[derive(Debug, Serialize)]
struct EnterpriseSourceBindingsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminNoopResponse,
    source_bindings: Vec<serde_json::Value>,
    count: usize,
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
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> Json<EnterpriseOrgUnitsResponse> {
    Json(EnterpriseOrgUnitsResponse {
        base: noop_base(tenant_context, request_principal),
        org_units: Vec::new(),
        count: 0,
    })
}

async fn create_org_unit(
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> Json<EnterpriseAdminNoopResponse> {
    Json(noop_base(tenant_context, request_principal))
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
) -> Json<EnterpriseAdminNoopResponse> {
    Json(noop_base(tenant_context, request_principal))
}

async fn update_source_binding(
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
) -> Json<EnterpriseAdminNoopResponse> {
    Json(noop_base(tenant_context, request_principal))
}

fn noop_base(
    tenant_context: TenantContext,
    request_principal: RequestPrincipal,
) -> EnterpriseAdminNoopResponse {
    EnterpriseAdminNoopResponse {
        tenant_context,
        request_principal,
        bridge_state: "absent",
        status: "noop",
        message: "enterprise admin storage is not configured",
    }
}
