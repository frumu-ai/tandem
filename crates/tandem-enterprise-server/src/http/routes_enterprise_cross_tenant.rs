// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

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
    bad_request, internal_error, require_current_enterprise_admin, require_enterprise_admin,
    storage_base, validate_enterprise_id, validate_external_id, EnterpriseAdminResponseBase,
    EnterpriseResult,
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
        .enterprise
        .cross_tenant_grants
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
        .enterprise
        .cross_tenant_grants
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
    let record = commit_cross_tenant_grant_change(
        &state,
        "enterprise.cross_tenant_grant.issued",
        &tenant_context,
        &request_principal,
        verified_tenant_context.as_deref(),
        move |candidate| {
            if candidate.contains_key(&storage_key) {
                return Err(bad_request("ENTERPRISE_CROSS_TENANT_GRANT_ALREADY_EXISTS"));
            }
            candidate.insert(storage_key, record.clone());
            Ok(record)
        },
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
    let issuer_tenant = tenant_context.clone();
    let revoked_by = principal_from_request(&request_principal);
    let updated = commit_cross_tenant_grant_change(
        &state,
        "enterprise.cross_tenant_grant.revoked",
        &tenant_context,
        &request_principal,
        verified_tenant_context.as_deref(),
        move |candidate| {
            let Some(record) = candidate.values_mut().find(|record| {
                record.grant.claims.grant_id == grant_id
                    && record
                        .grant
                        .claims
                        .issuer
                        .matches_tenant_context(&issuer_tenant)
            }) else {
                return Err(super::routes_enterprise::not_found(
                    "ENTERPRISE_CROSS_TENANT_GRANT_NOT_FOUND",
                ));
            };
            record.revoke(
                now_ms(),
                revoked_by,
                input.reason,
                input.source_policy_decision_id,
                input.source_audit_event_id,
            );
            Ok(record.clone())
        },
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

/// The hosted-policy publication lock precedes the grant writer lock: other
/// commits can read cross-tenant grants while holding the publication lock.
/// Check current authority only after both locks have been acquired, then
/// persist a candidate before making it visible to readers. The transaction
/// task owns both guards so HTTP cancellation cannot separate disk publication
/// from the live-map swap.
async fn commit_cross_tenant_grant_change<F>(
    state: &AppState,
    event_type: &'static str,
    tenant_context: &TenantContext,
    request_principal: &RequestPrincipal,
    verified_tenant_context: Option<&VerifiedTenantContext>,
    change: F,
) -> Result<CrossTenantGrantRecord, (StatusCode, Json<Value>)>
where
    F: FnOnce(
            &mut HashMap<String, CrossTenantGrantRecord>,
        ) -> Result<CrossTenantGrantRecord, (StatusCode, Json<Value>)>
        + Send
        + 'static,
{
    commit_cross_tenant_grant_change_inner(
        state.clone(),
        event_type,
        tenant_context.clone(),
        request_principal.clone(),
        verified_tenant_context.cloned(),
        change,
        None,
    )
    .await
}

struct GrantCommitPause {
    published: tokio::sync::oneshot::Sender<()>,
    resume: tokio::sync::oneshot::Receiver<()>,
}

async fn commit_cross_tenant_grant_change_inner<F>(
    state: AppState,
    event_type: &'static str,
    tenant_context: TenantContext,
    request_principal: RequestPrincipal,
    verified_tenant_context: Option<VerifiedTenantContext>,
    change: F,
    pause: Option<GrantCommitPause>,
) -> Result<CrossTenantGrantRecord, (StatusCode, Json<Value>)>
where
    F: FnOnce(
            &mut HashMap<String, CrossTenantGrantRecord>,
        ) -> Result<CrossTenantGrantRecord, (StatusCode, Json<Value>)>
        + Send
        + 'static,
{
    tokio::spawn(async move {
        let _policy_publication = state.lock_hosted_policy_publication().await;
        let mut registry = state.enterprise.cross_tenant_grants.write().await;
        require_current_enterprise_admin(
            &state,
            &request_principal,
            verified_tenant_context.as_ref(),
        )?;
        let mut candidate = registry.clone();
        let record = change(&mut candidate)?;
        persist_cross_tenant_grants(&state.enterprise.cross_tenant_grants_path, &candidate).await?;
        if let Some(pause) = pause {
            let _ = pause.published.send(());
            let _ = pause.resume.await;
        }
        *registry = candidate;
        drop(registry);
        drop(_policy_publication);
        // The grant file and protected audit are separate stores. Keep audit in
        // this owned task so caller cancellation cannot skip it after commit.
        append_cross_tenant_grant_audit(
            &state,
            event_type,
            &tenant_context,
            &request_principal,
            &record,
        )
        .await?;
        Ok(record)
    })
    .await
    .map_err(|_| internal_error("ENTERPRISE_CROSS_TENANT_GRANTS_PERSIST_FAILED"))?
}

async fn persist_cross_tenant_grants(
    path: &std::path::Path,
    registry: &HashMap<String, CrossTenantGrantRecord>,
) -> Result<(), (StatusCode, Json<Value>)> {
    let payload = serde_json::to_vec_pretty(registry)
        .map_err(|_| internal_error("ENTERPRISE_CROSS_TENANT_GRANTS_PERSIST_FAILED"))?;
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || -> std::io::Result<()> {
        use std::io::Write;

        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| std::path::Path::new("."));
        std::fs::create_dir_all(parent)?;
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("cross_tenant_grants.json");
        let temporary = parent.join(format!(".{file_name}.tmp-{}", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut options = std::fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temporary)?;
            file.write_all(&payload)?;
            file.sync_all()?;
            drop(file);
            replace_cross_tenant_grants_file(&temporary, &path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&temporary);
        } else {
            // Rename is the commit point. A later directory-sync error must
            // not report failure while leaving the new file but old live map.
            #[cfg(unix)]
            if let Err(error) = std::fs::File::open(parent).and_then(|dir| dir.sync_all()) {
                tracing::warn!(path = %path.display(), %error,
                    "cross-tenant grant replacement published but directory sync failed");
            }
        }
        result
    })
    .await
    .map_err(|_| internal_error("ENTERPRISE_CROSS_TENANT_GRANTS_PERSIST_FAILED"))?
    .map_err(|_| internal_error("ENTERPRISE_CROSS_TENANT_GRANTS_PERSIST_FAILED"))
}

#[cfg(not(windows))]
fn replace_cross_tenant_grants_file(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    std::fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_cross_tenant_grants_file(
    source: &std::path::Path,
    destination: &std::path::Path,
) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
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
    use std::path::Path;
    use std::time::Duration;
    use tandem_enterprise_contract::{
        hosted_policy::{role_capabilities, HostedPolicyBundle},
        AuthorityChain, HumanActor, ResourceKind, TenantContextAssertionClaims,
    };
    use tandem_server::test_support::{
        configure_hosted_policy_file_for_test, reload_hosted_policy_file_for_test, test_state,
    };

    fn hosted_policy_file(path: &Path, version: u64, role: Option<&str>) {
        let users = role
            .map(|role| {
                vec![json!({
                    "id": "admin", "email": null, "username": null, "role": role,
                    "capabilities": role_capabilities(role), "is_active": true,
                    "email_verified": true
                })]
            })
            .unwrap_or_default();
        std::fs::write(
            path,
            serde_json::to_vec(&json!({
                "schema_version": 1, "policy_version": version,
                "organization_id": "org-a", "deployment_id": "dep-a",
                "generated_at": chrono::DateTime::from_timestamp_millis(now_ms() as i64).unwrap(),
                "users": users, "org_units": [], "org_unit_memberships": [],
                "deployment_grants": []
            }))
            .unwrap(),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
        }
    }

    fn hosted_admin_identity(path: &Path) -> VerifiedTenantContext {
        let bytes = std::fs::read(path).unwrap();
        let bundle = HostedPolicyBundle::from_json(&bytes).unwrap();
        let now = now_ms();
        let tenant =
            TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".into()), "admin");
        let mut claims = TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "tandem-runtime",
            now,
            now + 60_000,
            uuid::Uuid::new_v4().to_string(),
            tenant,
            HumanActor::tandem_user("admin"),
            AuthorityChain::from_request(RequestPrincipal::authenticated_user(
                "admin",
                "tandem-web",
            )),
            vec!["hosted:role:admin".into()],
        );
        claims.policy_version = Some(bundle.policy_version);
        claims.capabilities = role_capabilities("admin")
            .into_iter()
            .map(str::to_string)
            .collect();
        let mut verified: VerifiedTenantContext = claims.into();
        verified.strict_projection = Some(
            bundle
                .validate("org-a", "dep-a", now, None)
                .unwrap()
                .project_identity(&verified, now)
                .unwrap(),
        );
        verified
    }

    async fn hosted_test_state() -> (AppState, std::path::PathBuf, VerifiedTenantContext) {
        let state = test_state().await;
        let policy_path = state
            .enterprise
            .cross_tenant_grants_path
            .with_file_name("hosted-policy.json");
        hosted_policy_file(&policy_path, 1, Some("admin"));
        configure_hosted_policy_file_for_test(&state, "org-a", "dep-a", policy_path.clone())
            .await
            .unwrap();
        let verified = hosted_admin_identity(&policy_path);
        (state, policy_path, verified)
    }

    fn test_record(tenant: &TenantContext, grant_id: &str) -> CrossTenantGrantRecord {
        let now = now_ms();
        let header = CrossTenantGrantHeader::ed25519("test-cross-tenant-key");
        let claims = CrossTenantGrantClaims::new_v1(
            grant_id,
            CrossTenantGrantParty::from_tenant_context(tenant),
            CrossTenantGrantParty {
                organization_id: "outside-org".into(),
                workspace_id: "outside-space".into(),
                deployment_id: None,
            },
            PrincipalRef::human_user("recipient"),
            ResourceScope::root(ResourceRef::new(
                &tenant.org_id,
                &tenant.workspace_id,
                ResourceKind::DocumentCollection,
                "shared-docs",
            )),
            vec![AccessPermission::Read],
            vec![DataClass::Internal],
            now,
            now + 60_000,
            PrincipalRef::human_user("admin"),
        );
        let signature =
            sign_cross_tenant_grant(&header, &claims, &SigningKey::from_bytes(&[7; 32])).unwrap();
        CrossTenantGrantRecord::active(CrossTenantGrant::new(header, claims, signature), now)
    }

    async fn commit_test_issue(
        state: &AppState,
        principal: &RequestPrincipal,
        verified: Option<&VerifiedTenantContext>,
        tenant: &TenantContext,
        record: CrossTenantGrantRecord,
    ) -> Result<CrossTenantGrantRecord, (StatusCode, Json<Value>)> {
        let key = cross_tenant_grant_key(&record);
        commit_cross_tenant_grant_change(
            state,
            "enterprise.cross_tenant_grant.issued",
            tenant,
            principal,
            verified,
            move |candidate| {
                if candidate.contains_key(&key) {
                    return Err(bad_request("ENTERPRISE_CROSS_TENANT_GRANT_ALREADY_EXISTS"));
                }
                candidate.insert(key, record.clone());
                Ok(record)
            },
        )
        .await
    }

    async fn commit_test_revoke(
        state: &AppState,
        principal: &RequestPrincipal,
        verified: Option<&VerifiedTenantContext>,
        tenant: &TenantContext,
        grant_id: &str,
    ) -> Result<CrossTenantGrantRecord, (StatusCode, Json<Value>)> {
        let issuer_tenant = tenant.clone();
        let grant_id = grant_id.to_string();
        let reviewer = principal_from_request(principal);
        commit_cross_tenant_grant_change(
            state,
            "enterprise.cross_tenant_grant.revoked",
            tenant,
            principal,
            verified,
            move |candidate| {
                let record = candidate
                    .values_mut()
                    .find(|record| {
                        record.grant.claims.grant_id == grant_id
                            && record
                                .grant
                                .claims
                                .issuer
                                .matches_tenant_context(&issuer_tenant)
                    })
                    .ok_or_else(|| {
                        super::super::routes_enterprise::not_found(
                            "ENTERPRISE_CROSS_TENANT_GRANT_NOT_FOUND",
                        )
                    })?;
                record.revoke(now_ms(), reviewer, None, None, None);
                Ok(record.clone())
            },
        )
        .await
    }

    async fn wait_for_queued_grant_writer(state: &AppState) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if state.enterprise.cross_tenant_grants.try_read().is_err() {
                    return;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("grant writer did not queue");
    }

    async fn wait_for_grant_audit(state: &AppState, event_type: &str, grant_id: &str) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Ok(audit) = tokio::fs::read_to_string(&state.protected_audit_path).await {
                    if audit.contains(event_type) && audit.contains(grant_id) {
                        return;
                    }
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("owned grant task did not append protected audit");
    }

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

    #[tokio::test]
    async fn revoked_hosted_admin_cannot_issue_or_revoke_after_grant_writer_wait() {
        for role in [Some("member"), None] {
            let (state, policy_path, stale_admin) = hosted_test_state().await;
            let principal = RequestPrincipal::authenticated_user("admin", "tandem-web");
            let tenant = stale_admin.tenant_context.clone();
            let existing = test_record(&tenant, "existing-grant");
            commit_test_issue(
                &state,
                &principal,
                Some(&stale_admin),
                &tenant,
                existing.clone(),
            )
            .await
            .unwrap();
            let before_disk = tokio::fs::read(&state.enterprise.cross_tenant_grants_path)
                .await
                .unwrap();

            hosted_policy_file(&policy_path, 2, role);
            reload_hosted_policy_file_for_test(&state).await.unwrap();

            let held = state.enterprise.cross_tenant_grants.read().await;
            let issue_state = state.clone();
            let issue_principal = principal.clone();
            let issue_admin = stale_admin.clone();
            let issue_tenant = tenant.clone();
            let new_record = test_record(&tenant, "forbidden-grant");
            let issue = tokio::spawn(async move {
                commit_test_issue(
                    &issue_state,
                    &issue_principal,
                    Some(&issue_admin),
                    &issue_tenant,
                    new_record,
                )
                .await
            });
            wait_for_queued_grant_writer(&state).await;
            drop(held);
            assert_eq!(issue.await.unwrap().unwrap_err().0, StatusCode::FORBIDDEN);

            let held = state.enterprise.cross_tenant_grants.read().await;
            let revoke_state = state.clone();
            let revoke_principal = principal.clone();
            let revoke_admin = stale_admin.clone();
            let revoke_tenant = tenant.clone();
            let revoke = tokio::spawn(async move {
                commit_test_revoke(
                    &revoke_state,
                    &revoke_principal,
                    Some(&revoke_admin),
                    &revoke_tenant,
                    "existing-grant",
                )
                .await
            });
            wait_for_queued_grant_writer(&state).await;
            drop(held);
            assert_eq!(revoke.await.unwrap().unwrap_err().0, StatusCode::FORBIDDEN);

            let registry = state.enterprise.cross_tenant_grants.read().await;
            assert_eq!(registry.len(), 1);
            assert_eq!(registry.values().next(), Some(&existing));
            assert_eq!(
                tokio::fs::read(&state.enterprise.cross_tenant_grants_path)
                    .await
                    .unwrap(),
                before_disk
            );
        }
    }

    #[tokio::test]
    async fn hosted_policy_publication_waits_for_grant_commit() {
        let (state, policy_path, admin) = hosted_test_state().await;
        let principal = RequestPrincipal::authenticated_user("admin", "tandem-web");
        let tenant = admin.tenant_context.clone();
        let held = state.enterprise.cross_tenant_grants.read().await;
        let issue_state = state.clone();
        let issue_principal = principal.clone();
        let issue_admin = admin.clone();
        let issue = tokio::spawn(async move {
            commit_test_issue(
                &issue_state,
                &issue_principal,
                Some(&issue_admin),
                &tenant,
                test_record(&tenant, "authorized-before-revocation"),
            )
            .await
        });
        wait_for_queued_grant_writer(&state).await;

        hosted_policy_file(&policy_path, 2, Some("member"));
        let reload_state = state.clone();
        let reload =
            tokio::spawn(async move { reload_hosted_policy_file_for_test(&reload_state).await });
        tokio::task::yield_now().await;
        assert!(
            !reload.is_finished(),
            "publication must wait for grant commit"
        );
        drop(held);

        assert!(issue.await.unwrap().is_ok());
        reload.await.unwrap().unwrap();
        assert!(state.authorize_current_hosted_admin(&admin).is_err());
        let registry = state.enterprise.cross_tenant_grants.read().await;
        assert_eq!(registry.len(), 1);
        let persisted: HashMap<String, CrossTenantGrantRecord> = serde_json::from_slice(
            &tokio::fs::read(&state.enterprise.cross_tenant_grants_path)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(*registry, persisted);
    }

    #[tokio::test]
    async fn canceled_caller_cannot_split_grant_file_and_live_registry() {
        let state = test_state().await;
        let principal = RequestPrincipal::authenticated_user("admin", "local_api_token");
        let tenant =
            TenantContext::explicit_user_workspace("local-org", "local-workspace", None, "admin");
        let record = test_record(&tenant, "cancel-safe-grant");
        let key = cross_tenant_grant_key(&record);
        let (published_tx, published_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
        let issue_state = state.clone();
        let issue_principal = principal.clone();
        let issue_tenant = tenant.clone();
        let issue_key = key.clone();
        let issue = tokio::spawn(async move {
            commit_cross_tenant_grant_change_inner(
                issue_state,
                "enterprise.cross_tenant_grant.issued",
                issue_tenant,
                issue_principal,
                None,
                move |candidate| {
                    candidate.insert(issue_key, record.clone());
                    Ok(record)
                },
                Some(GrantCommitPause {
                    published: published_tx,
                    resume: resume_rx,
                }),
            )
            .await
        });
        published_rx.await.expect("issue file published");
        assert!(state.enterprise.cross_tenant_grants.try_read().is_err());
        let disk_during_issue: HashMap<String, CrossTenantGrantRecord> = serde_json::from_slice(
            &tokio::fs::read(&state.enterprise.cross_tenant_grants_path)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(disk_during_issue.contains_key(&key));
        issue.abort();
        assert!(issue.await.unwrap_err().is_cancelled());
        resume_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if state.enterprise.cross_tenant_grants.read().await.len() == 1 {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("issued grant published in live registry");
        assert_eq!(
            *state.enterprise.cross_tenant_grants.read().await,
            disk_during_issue
        );
        wait_for_grant_audit(
            &state,
            "enterprise.cross_tenant_grant.issued",
            "cancel-safe-grant",
        )
        .await;

        let (published_tx, published_rx) = tokio::sync::oneshot::channel();
        let (resume_tx, resume_rx) = tokio::sync::oneshot::channel();
        let revoke_state = state.clone();
        let revoke_principal = principal.clone();
        let revoke_tenant = tenant.clone();
        let revoke_key = key.clone();
        let revoke = tokio::spawn(async move {
            commit_cross_tenant_grant_change_inner(
                revoke_state,
                "enterprise.cross_tenant_grant.revoked",
                revoke_tenant,
                revoke_principal,
                None,
                move |candidate| {
                    let record = candidate.get_mut(&revoke_key).unwrap();
                    record.revoke(
                        now_ms(),
                        PrincipalRef::human_user("admin"),
                        None,
                        None,
                        None,
                    );
                    Ok(record.clone())
                },
                Some(GrantCommitPause {
                    published: published_tx,
                    resume: resume_rx,
                }),
            )
            .await
        });
        published_rx.await.expect("revoke file published");
        assert!(state.enterprise.cross_tenant_grants.try_read().is_err());
        let disk_during_revoke: HashMap<String, CrossTenantGrantRecord> = serde_json::from_slice(
            &tokio::fs::read(&state.enterprise.cross_tenant_grants_path)
                .await
                .unwrap(),
        )
        .unwrap();
        assert!(disk_during_revoke[&key].revocation.is_some());
        revoke.abort();
        assert!(revoke.await.unwrap_err().is_cancelled());
        resume_tx.send(()).unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if state.enterprise.cross_tenant_grants.read().await[&key]
                    .revocation
                    .is_some()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("revocation published in live registry");
        assert_eq!(
            *state.enterprise.cross_tenant_grants.read().await,
            disk_during_revoke
        );
        wait_for_grant_audit(
            &state,
            "enterprise.cross_tenant_grant.revoked",
            "cancel-safe-grant",
        )
        .await;
    }

    #[tokio::test]
    async fn grant_persistence_failure_preserves_live_registry_and_local_admin_compatibility() {
        let state = test_state().await;
        let principal = RequestPrincipal::authenticated_user("admin", "local_api_token");
        let tenant =
            TenantContext::explicit_user_workspace("local-org", "local-workspace", None, "admin");
        let existing = test_record(&tenant, "local-grant");
        commit_test_issue(&state, &principal, None, &tenant, existing.clone())
            .await
            .expect("local admin issue remains available");
        let persisted_before = tokio::fs::read(&state.enterprise.cross_tenant_grants_path)
            .await
            .unwrap();
        let revoked = commit_test_revoke(&state, &principal, None, &tenant, "local-grant")
            .await
            .expect("local admin revoke remains available");
        assert!(revoked.revocation.is_some());
        let persisted_after = tokio::fs::read(&state.enterprise.cross_tenant_grants_path)
            .await
            .unwrap();
        assert_ne!(persisted_before, persisted_after);

        let mut failing_state = state.clone();
        let blocked_path = state
            .enterprise
            .cross_tenant_grants_path
            .with_file_name("blocked-cross-tenant-grants");
        tokio::fs::create_dir_all(&blocked_path).await.unwrap();
        failing_state.enterprise.cross_tenant_grants_path = blocked_path.clone();
        let before_registry = state.enterprise.cross_tenant_grants.read().await.clone();
        let issue_error = commit_test_issue(
            &failing_state,
            &principal,
            None,
            &tenant,
            test_record(&tenant, "failed-grant"),
        )
        .await
        .unwrap_err();
        assert_eq!(issue_error.0, StatusCode::INTERNAL_SERVER_ERROR);
        let revoke_error =
            commit_test_revoke(&failing_state, &principal, None, &tenant, "local-grant")
                .await
                .unwrap_err();
        assert_eq!(revoke_error.0, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(
            *state.enterprise.cross_tenant_grants.read().await,
            before_registry
        );
        assert_eq!(
            tokio::fs::read(&state.enterprise.cross_tenant_grants_path)
                .await
                .unwrap(),
            persisted_after
        );
        assert!(blocked_path.is_dir());
        assert_eq!(std::fs::read_dir(&blocked_path).unwrap().count(), 0);
        assert!(!std::fs::read_dir(blocked_path.parent().unwrap())
            .unwrap()
            .any(|entry| {
                entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".blocked-cross-tenant-grants.tmp-")
            }));
    }
}
