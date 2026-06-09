use std::collections::HashMap;

use axum::extract::{Extension, Path, State};
use axum::http::StatusCode;
use axum::routing::{get, post};
use axum::{Json, Router};
use base64::Engine;
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_enterprise_contract::{
    AccessPermission, CrossTenantGrant, CrossTenantGrantClaims, CrossTenantGrantHeader,
    CrossTenantGrantParty, CrossTenantGrantRecord, DataClass, PrincipalRef, RequestPrincipal,
    ResourceRef, ResourceScope, TenantContext, VerifiedTenantContext,
};
use tandem_server::{now_ms, AppState};

use super::routes_enterprise::{
    bad_request, internal_error, require_enterprise_admin, storage_base, validate_enterprise_id,
    validate_external_id, EnterpriseAdminResponseBase, EnterpriseResult,
};

#[derive(Debug, Serialize)]
struct EnterpriseCrossTenantGrantsResponse {
    #[serde(flatten)]
    base: EnterpriseAdminResponseBase,
    grants: Vec<CrossTenantGrantRecord>,
    count: usize,
}

#[derive(Debug, Deserialize)]
struct IssueCrossTenantGrantRequest {
    grant_id: String,
    audience: CrossTenantGrantParty,
    subject: PrincipalRef,
    resource_scope: ResourceScope,
    #[serde(default)]
    permissions: Vec<AccessPermission>,
    #[serde(default)]
    data_classes: Vec<DataClass>,
    #[serde(default)]
    tool_patterns: Vec<String>,
    #[serde(default)]
    issued_at_ms: Option<u64>,
    #[serde(default)]
    not_before_ms: Option<u64>,
    expires_at_ms: u64,
    #[serde(default)]
    source_policy_decision_id: Option<String>,
    #[serde(default)]
    source_audit_event_id: Option<String>,
    #[serde(default)]
    approval_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RevokeCrossTenantGrantRequest {
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    source_policy_decision_id: Option<String>,
    #[serde(default)]
    source_audit_event_id: Option<String>,
}

pub(super) fn apply(router: Router<AppState>) -> Router<AppState> {
    router
        .route(
            "/enterprise/cross-tenant-grants",
            get(list_issued_cross_tenant_grants).post(issue_cross_tenant_grant),
        )
        .route(
            "/enterprise/cross-tenant-grants/inbound",
            get(list_inbound_cross_tenant_grants),
        )
        .route(
            "/enterprise/cross-tenant-grants/{grant_id}/revoke",
            post(revoke_cross_tenant_grant),
        )
}

async fn list_issued_cross_tenant_grants(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<EnterpriseCrossTenantGrantsResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let mut grants = state
        .enterprise_cross_tenant_grants
        .read()
        .await
        .values()
        .filter(|record| {
            record
                .grant
                .claims
                .issuer
                .matches_tenant_context(&tenant_context)
        })
        .cloned()
        .collect::<Vec<_>>();
    grants.sort_by(|left, right| left.grant.claims.grant_id.cmp(&right.grant.claims.grant_id));
    Ok(Json(EnterpriseCrossTenantGrantsResponse {
        count: grants.len(),
        grants,
        base: storage_base(tenant_context, request_principal),
    }))
}

async fn list_inbound_cross_tenant_grants(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
) -> EnterpriseResult<EnterpriseCrossTenantGrantsResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let mut grants = state
        .enterprise_cross_tenant_grants
        .read()
        .await
        .values()
        .filter(|record| {
            record
                .grant
                .claims
                .audience
                .matches_tenant_context(&tenant_context)
        })
        .cloned()
        .collect::<Vec<_>>();
    grants.sort_by(|left, right| left.grant.claims.grant_id.cmp(&right.grant.claims.grant_id));
    Ok(Json(EnterpriseCrossTenantGrantsResponse {
        count: grants.len(),
        grants,
        base: storage_base(tenant_context, request_principal),
    }))
}

async fn issue_cross_tenant_grant(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<IssueCrossTenantGrantRequest>,
) -> EnterpriseResult<EnterpriseCrossTenantGrantsResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let grant_id = validate_enterprise_id("cross_tenant_grant_id", &input.grant_id)?;
    validate_cross_tenant_party(&input.audience)?;
    if input.audience.matches_tenant_context(&tenant_context) {
        return Err(bad_request(
            "ENTERPRISE_CROSS_TENANT_GRANT_AUDIENCE_MUST_DIFFER",
        ));
    }
    if input.permissions.is_empty() {
        return Err(bad_request(
            "ENTERPRISE_CROSS_TENANT_GRANT_PERMISSIONS_REQUIRED",
        ));
    }
    if input.data_classes.is_empty() {
        return Err(bad_request(
            "ENTERPRISE_CROSS_TENANT_GRANT_DATA_CLASSES_REQUIRED",
        ));
    }
    validate_resource_scope_matches_tenant(&input.resource_scope, &tenant_context)?;

    let now = now_ms();
    let issued_at_ms = input.issued_at_ms.unwrap_or(now);
    let not_before_ms = input.not_before_ms.unwrap_or(issued_at_ms);
    if not_before_ms < issued_at_ms || input.expires_at_ms <= not_before_ms {
        return Err(bad_request(
            "ENTERPRISE_CROSS_TENANT_GRANT_VALIDITY_WINDOW_INVALID",
        ));
    }

    let issued_by = principal_from_request(&request_principal);
    let mut claims = CrossTenantGrantClaims::new_v1(
        grant_id,
        CrossTenantGrantParty::from_tenant_context(&tenant_context),
        input.audience,
        input.subject,
        input.resource_scope,
        input.permissions,
        input.data_classes,
        issued_at_ms,
        input.expires_at_ms,
        issued_by,
    );
    claims.not_before_ms = not_before_ms;
    claims.tool_patterns = input.tool_patterns;
    claims.source_policy_decision_id = input.source_policy_decision_id;
    claims.source_audit_event_id = input.source_audit_event_id;
    claims.approval_id = input.approval_id;

    let (key_id, signing_key) = cross_tenant_grant_signing_key()?;
    let header = CrossTenantGrantHeader::ed25519(key_id);
    let signature = sign_cross_tenant_grant(&header, &claims, &signing_key)?;
    let record =
        CrossTenantGrantRecord::active(CrossTenantGrant::new(header, claims, signature), now);
    let storage_key = cross_tenant_grant_key(&record);
    {
        let mut registry = state.enterprise_cross_tenant_grants.write().await;
        if registry.contains_key(&storage_key) {
            return Err(bad_request("ENTERPRISE_CROSS_TENANT_GRANT_ALREADY_EXISTS"));
        }
        registry.insert(storage_key, record.clone());
        persist_cross_tenant_grants(&state.enterprise_cross_tenant_grants_path, &registry).await?;
    }
    append_cross_tenant_grant_audit(
        &state,
        "enterprise.cross_tenant_grant.issued",
        &tenant_context,
        &request_principal,
        &record,
    )
    .await?;

    Ok(Json(EnterpriseCrossTenantGrantsResponse {
        count: 1,
        grants: vec![record],
        base: storage_base(tenant_context, request_principal),
    }))
}

async fn revoke_cross_tenant_grant(
    State(state): State<AppState>,
    Path(grant_id): Path<String>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    verified_tenant_context: Option<Extension<VerifiedTenantContext>>,
    Json(input): Json<RevokeCrossTenantGrantRequest>,
) -> EnterpriseResult<EnterpriseCrossTenantGrantsResponse> {
    require_enterprise_admin(&request_principal, verified_tenant_context.as_deref())?;
    let grant_id = validate_enterprise_id("cross_tenant_grant_id", &grant_id)?;
    let updated = {
        let mut registry = state.enterprise_cross_tenant_grants.write().await;
        let Some(record) = registry.values_mut().find(|record| {
            record.grant.claims.grant_id == grant_id
                && record
                    .grant
                    .claims
                    .issuer
                    .matches_tenant_context(&tenant_context)
        }) else {
            return Err(super::routes_enterprise::not_found(
                "ENTERPRISE_CROSS_TENANT_GRANT_NOT_FOUND",
            ));
        };
        record.revoke(
            now_ms(),
            principal_from_request(&request_principal),
            input.reason,
            input.source_policy_decision_id,
            input.source_audit_event_id,
        );
        let updated = record.clone();
        persist_cross_tenant_grants(&state.enterprise_cross_tenant_grants_path, &registry).await?;
        updated
    };
    append_cross_tenant_grant_audit(
        &state,
        "enterprise.cross_tenant_grant.revoked",
        &tenant_context,
        &request_principal,
        &updated,
    )
    .await?;

    Ok(Json(EnterpriseCrossTenantGrantsResponse {
        count: 1,
        grants: vec![updated],
        base: storage_base(tenant_context, request_principal),
    }))
}

fn validate_cross_tenant_party(
    party: &CrossTenantGrantParty,
) -> Result<(), (StatusCode, Json<Value>)> {
    validate_external_id("audience_organization_id", &party.organization_id)?;
    validate_external_id("audience_workspace_id", &party.workspace_id)?;
    if let Some(deployment_id) = party.deployment_id.as_deref() {
        validate_external_id("audience_deployment_id", deployment_id)?;
    }
    Ok(())
}

fn validate_resource_scope_matches_tenant(
    scope: &ResourceScope,
    tenant_context: &TenantContext,
) -> Result<(), (StatusCode, Json<Value>)> {
    for resource in std::iter::once(&scope.root)
        .chain(scope.allowed_resources.iter())
        .chain(scope.denied_resources.iter())
    {
        validate_resource_matches_tenant(resource, tenant_context)?;
    }
    Ok(())
}

fn validate_resource_matches_tenant(
    resource: &ResourceRef,
    tenant_context: &TenantContext,
) -> Result<(), (StatusCode, Json<Value>)> {
    if resource.organization_id != tenant_context.org_id
        || resource.workspace_id != tenant_context.workspace_id
    {
        return Err(bad_request(
            "ENTERPRISE_CROSS_TENANT_GRANT_RESOURCE_TENANT_MISMATCH",
        ));
    }
    Ok(())
}

fn principal_from_request(request_principal: &RequestPrincipal) -> PrincipalRef {
    PrincipalRef::human_user(
        request_principal
            .actor_id
            .clone()
            .unwrap_or_else(|| request_principal.source.clone()),
    )
}

fn cross_tenant_grant_key(record: &CrossTenantGrantRecord) -> String {
    let issuer = &record.grant.claims.issuer;
    let deployment = issuer.deployment_id.as_deref().unwrap_or("local");
    format!(
        "{}::{}::{}::{}",
        issuer.organization_id, issuer.workspace_id, deployment, record.grant.claims.grant_id
    )
}

async fn persist_cross_tenant_grants(
    path: &std::path::Path,
    registry: &HashMap<String, CrossTenantGrantRecord>,
) -> Result<(), (StatusCode, Json<Value>)> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| internal_error("ENTERPRISE_CROSS_TENANT_GRANTS_PERSIST_FAILED"))?;
    }
    let payload = serde_json::to_vec_pretty(registry)
        .map_err(|_| internal_error("ENTERPRISE_CROSS_TENANT_GRANTS_PERSIST_FAILED"))?;
    tokio::fs::write(path, payload)
        .await
        .map_err(|_| internal_error("ENTERPRISE_CROSS_TENANT_GRANTS_PERSIST_FAILED"))?;
    Ok(())
}

async fn append_cross_tenant_grant_audit(
    state: &AppState,
    event_type: &'static str,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    record: &CrossTenantGrantRecord,
) -> Result<(), (StatusCode, Json<Value>)> {
    tandem_server::audit::append_protected_audit_event(
        state,
        event_type,
        tenant_context,
        request_principal
            .actor_id
            .clone()
            .or_else(|| Some(request_principal.source.clone())),
        json!({
            "grant_id": record.grant.claims.grant_id,
            "issuer_tenant": record.grant.claims.issuer,
            "audience_tenant": record.grant.claims.audience,
            "subject": record.grant.claims.subject,
            "resource_scope": record.grant.claims.resource_scope,
            "permissions": record.grant.claims.permissions,
            "data_classes": record.grant.claims.data_classes,
            "state": record.state,
            "revocation": record.revocation,
            "source_policy_decision_id": record.grant.claims.source_policy_decision_id,
            "source_audit_event_id": record.grant.claims.source_audit_event_id,
            "approval_id": record.grant.claims.approval_id,
        }),
    )
    .await
    .map_err(|_| internal_error("ENTERPRISE_CROSS_TENANT_GRANT_AUDIT_FAILED"))
}

fn cross_tenant_grant_signing_key() -> Result<(String, SigningKey), (StatusCode, Json<Value>)> {
    let raw_key = std::env::var("TANDEM_CROSS_TENANT_GRANT_SIGNING_KEY")
        .ok()
        .or_else(|| {
            let path = std::env::var("TANDEM_CROSS_TENANT_GRANT_SIGNING_KEY_FILE").ok()?;
            std::fs::read_to_string(path).ok()
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| service_unavailable("ENTERPRISE_CROSS_TENANT_GRANT_SIGNING_KEY_REQUIRED"))?;
    let key_bytes = decode_signing_key(&raw_key)
        .ok_or_else(|| bad_request("ENTERPRISE_CROSS_TENANT_GRANT_SIGNING_KEY_INVALID"))?;
    let key_id = std::env::var("TANDEM_CROSS_TENANT_GRANT_SIGNING_KEY_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "cross-tenant-grant-local".to_string());
    Ok((key_id, SigningKey::from_bytes(&key_bytes)))
}

fn decode_signing_key(raw: &str) -> Option<[u8; 32]> {
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(raw))
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(raw))
        .ok()
        .or_else(|| decode_hex(raw))?;
    decoded.as_slice().try_into().ok()
}

fn decode_hex(raw: &str) -> Option<Vec<u8>> {
    let raw = raw.trim();
    if raw.len() % 2 != 0 {
        return None;
    }
    (0..raw.len())
        .step_by(2)
        .map(|idx| u8::from_str_radix(&raw[idx..idx + 2], 16).ok())
        .collect()
}

fn sign_cross_tenant_grant(
    header: &CrossTenantGrantHeader,
    claims: &CrossTenantGrantClaims,
    signing_key: &SigningKey,
) -> Result<String, (StatusCode, Json<Value>)> {
    let encoded_header = encode_json_base64url(header)?;
    let encoded_claims = encode_json_base64url(claims)?;
    let signing_input = format!("{encoded_header}.{encoded_claims}");
    let signature = signing_key.sign(signing_input.as_bytes());
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes()))
}

fn encode_json_base64url<T: Serialize>(value: &T) -> Result<String, (StatusCode, Json<Value>)> {
    let bytes = serde_json::to_vec(value)
        .map_err(|_| internal_error("ENTERPRISE_CROSS_TENANT_GRANT_SIGN_FAILED"))?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes))
}

fn service_unavailable(code: impl Into<String>) -> (StatusCode, Json<Value>) {
    let code = code.into();
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "code": code,
            "message": "enterprise signing key is not configured"
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_enterprise_contract::ResourceKind;

    #[test]
    fn grant_issuer_scope_rejects_wildcard_workspace() {
        let tenant_context =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "admin-a");
        let resource = ResourceRef::new(
            "org-a",
            "*",
            ResourceKind::DocumentCollection,
            "all-workspaces",
        );

        assert!(validate_resource_matches_tenant(&resource, &tenant_context).is_err());
    }
}
