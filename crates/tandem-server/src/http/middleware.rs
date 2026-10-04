// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::extract::{ConnectInfo, Request, State};
use axum::http::header;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use base64::Engine;
#[cfg(test)]
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::json;
use sha2::{Digest, Sha256};
#[cfg(test)]
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::net::SocketAddr;
use std::sync::atomic::Ordering;
use tandem_types::{
    AccessPermission, DataBoundary, DataClass, GrantSource, HeaderTenantContextResolver,
    NoopRequestAuthorizationHook, OrganizationUnitAccessGrant, OrganizationUnitMembership,
    PrincipalRef, RequestAuthorizationHook, RequestPrincipal, ResourceScope, RuntimeAuthMode,
    ScopedGrant, TenantContext, TenantContextAssertionClaims, TenantContextAssertionHeader,
    TenantContextResolver, TenantSource, VerifiedTenantContext,
};
#[cfg(test)]
use tandem_types::{ResourceKind, ResourceRef, SigningKeyPurpose};

use crate::{AppState, StartupStatus};

use super::{ErrorCode, ErrorEnvelope};
use crate::memory::policy_status::resolve_memory_context_runtime_auth_mode;

#[cfg(test)]
const DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS: u64 = 10_000;
#[cfg(test)]
const MAX_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS: u64 = 60_000;

pub(super) async fn auth_gate(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|connect_info| connect_info.0);
    let locality =
        super::host_authority::RequestLocality::from_peer_and_headers(peer, request.headers());
    request.extensions_mut().insert(locality);
    if request.method() == Method::OPTIONS {
        return next.run(request).await;
    }
    let path = request.uri().path();
    if state.web_ui_enabled()
        && is_public_web_ui_request(request.method(), path, &state.web_ui_prefix())
    {
        return next.run(request).await;
    }
    if is_public_health_request(request.method(), path) {
        return next.run(request).await;
    }
    if is_public_oauth_callback_path(path) {
        return next.run(request).await;
    }
    if request.method() == Method::POST {
        if let Some(public_path_token) =
            super::webhook_rate_limit::public_automation_webhook_token(path)
        {
            match super::webhook_rate_limit::global().check(
                &public_path_token,
                peer,
                crate::now_ms(),
            ) {
                super::webhook_rate_limit::RateDecision::Allowed => {
                    return next.run(request).await;
                }
                super::webhook_rate_limit::RateDecision::Limited { retry_after_secs } => {
                    tandem_observability::record_webhook_intake_throttled("capability_network");
                    tracing::warn!(
                        target: "tandem_server::webhook_intake",
                        retry_after_secs,
                        "public automation webhook pre-auth rate limit exceeded"
                    );
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        [(header::RETRY_AFTER, retry_after_secs.to_string())],
                        Json(json!({ "ok": false, "status": "throttled" })),
                    )
                        .into_response();
                }
            }
        }
    }
    if request.method() == Method::POST && is_public_slack_events_path(path) {
        return next.run(request).await;
    }
    let runtime_auth_mode = resolve_memory_context_runtime_auth_mode();
    if path == "/incident-monitor/intake/report" {
        if !runtime_auth_mode_requires_transport_token(runtime_auth_mode) {
            match attach_enterprise_request_context_for_mode(
                &state,
                &mut request,
                runtime_auth_mode,
            )
            .await
            {
                Ok(true) => {}
                Ok(false) => return tenant_context_denied_response(),
                Err(error) => return denial_audit_failure_response(&error),
            }
        }
        if !runtime_auth_mode_requires_transport_token(runtime_auth_mode) {
            return next.run(request).await;
        }
    }

    let token_required = state.api_token_required().await;
    let token_authorized = match extract_request_token(request.headers()) {
        Some(provided) => state.api_token_matches(&provided).await,
        None => !token_required && !runtime_auth_mode_requires_transport_token(runtime_auth_mode),
    };
    if !token_authorized {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorEnvelope::new(
                "Unauthorized: missing or invalid API token",
                ErrorCode::AuthRequired,
            )),
        )
            .into_response();
    }

    match attach_enterprise_request_context_for_mode(&state, &mut request, runtime_auth_mode).await
    {
        Ok(true) => {}
        Ok(false) => return tenant_context_denied_response(),
        Err(error) => return denial_audit_failure_response(&error),
    }

    // Per-tenant inbound rate limiting (TAN2-11). Enforced only once the tenant
    // context has been resolved, so the quota is keyed to the actual tenant.
    // Disabled by default; controlled by TANDEM_TENANT_RATE_LIMIT_PER_MIN.
    let limiter = super::tenant_rate_limit::global();
    if limiter.is_enabled() {
        if let Some(tenant) = request.extensions().get::<TenantContext>() {
            let key = super::tenant_rate_limit::tenant_key(&tenant.org_id, &tenant.workspace_id);
            if let super::tenant_rate_limit::RateDecision::Limited { retry_after_secs } =
                limiter.check(&key, crate::now_ms())
            {
                return (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(header::RETRY_AFTER, retry_after_secs.to_string())],
                    Json(ErrorEnvelope::new(
                        "Rate limit exceeded for tenant",
                        ErrorCode::RateLimited,
                    )),
                )
                    .into_response();
            }
        }
    }

    next.run(request).await
}

async fn attach_enterprise_request_context_for_mode(
    state: &AppState,
    request: &mut Request,
    mode: RuntimeAuthMode,
) -> Result<bool, String> {
    let headers = request.headers();
    let assertion_security = state.context_assertion_security_snapshot().ok();
    let mut resolved = match resolve_enterprise_request_context_for_mode_with_cached_security(
        headers,
        mode,
        state.trust_test_tenant_headers.load(Ordering::Relaxed),
        assertion_security.as_deref(),
    ) {
        Ok(context) => context,
        Err(denial) => {
            tracing::warn!(
                "Authorization denied: tenant context ingress rejected - reason={}",
                denial.reason.as_str()
            );
            denial
                .append_required_audit_event(state, mode, headers)
                .await?;
            return Ok(false);
        }
    };

    if !authorize_request(&resolved.request_principal, &resolved.tenant_context) {
        tracing::warn!(
            "Authorization denied: principal={:?} tenant={} source={}",
            resolved.request_principal.actor_id,
            resolved.tenant_context.org_id,
            resolved.request_principal.source
        );
        append_authorization_denial_audit_event(state, &resolved).await?;
        return Ok(false);
    }

    if let Err(reason) = state
        .enterprise
        .hosted_policy
        .authorize(resolved.verified_tenant_context.as_ref())
    {
        tracing::warn!(target: "tandem_server::hosted_policy", reason, "hosted request authority denied");
        append_authorization_denial_audit_event(state, &resolved).await?;
        return Ok(false);
    }
    let hosted_memberships = if let Some(verified) = resolved.verified_tenant_context.as_mut() {
        match state.enterprise.hosted_policy.project(verified) {
            Ok(memberships) => memberships,
            Err(reason) => {
                tracing::warn!(target: "tandem_server::hosted_policy", reason, "hosted projection denied");
                append_authorization_denial_audit_event(state, &resolved).await?;
                return Ok(false);
            }
        }
    } else {
        None
    };
    if let Some(permission) = super::hosted_route_authority::required_permission(request) {
        if let Err(reason) = state
            .enterprise
            .hosted_policy
            .authorize_permission(resolved.verified_tenant_context.as_ref(), permission)
        {
            tracing::warn!(target: "tandem_server::hosted_policy", reason, "hosted operation denied");
            append_authorization_denial_audit_event(state, &resolved).await?;
            return Ok(false);
        }
    }
    if let Some(mut verified_tenant_context) = resolved.verified_tenant_context {
        enrich_verified_context_with_org_unit_grants(
            state,
            &mut verified_tenant_context,
            hosted_memberships,
        )
        .await;
        super::cross_tenant_grants::enrich_verified_context_with_inbound_cross_tenant_grants(
            state,
            &mut verified_tenant_context,
        )
        .await;
        request.extensions_mut().insert(verified_tenant_context);
    }
    request.extensions_mut().insert(resolved.tenant_context);
    request.extensions_mut().insert(resolved.request_principal);
    Ok(true)
}

#[cfg(feature = "test-support")]
pub async fn hosted_test_ingress(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    match attach_enterprise_request_context_for_mode(
        &state,
        &mut request,
        RuntimeAuthMode::HostedSingleTenant,
    )
    .await
    {
        Ok(true) => next.run(request).await,
        Ok(false) => StatusCode::FORBIDDEN.into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn tenant_context_denied_response() -> Response {
    (
        StatusCode::FORBIDDEN,
        Json(ErrorEnvelope::new(
            "Unauthorized: tenant context denied",
            ErrorCode::TenantContextDenied,
        )),
    )
        .into_response()
}

const REQUIRED_DENIAL_RECEIPT_PUBLIC_ERROR: &str =
    "request remained denied because its required denial receipt could not be written";

fn denial_audit_failure_response(_error: &str) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "error": REQUIRED_DENIAL_RECEIPT_PUBLIC_ERROR,
            "code": "AUDIT_PERSISTENCE_FAILED",
        })),
    )
        .into_response()
}

// A failed required receipt must remain a 500. Keep diagnostic output to
// fixed categories and OS error numbers: an audit/KMS error chain may contain
// installation paths, assertion metadata, or command output in the future.
fn required_denial_receipt_category(error: &anyhow::Error) -> (&'static str, &'static str) {
    let causes = error
        .chain()
        .map(|cause| cause.to_string())
        .collect::<Vec<_>>();
    let contains = |needle: &str| causes.iter().any(|cause| cause.contains(needle));
    let phase = if contains("read governance JSONL store") {
        "read"
    } else if contains("append governance JSONL store") {
        "append"
    } else {
        "other"
    };
    let category = if contains("failed to spawn google cloud kms decrypt command") {
        "kms_decrypt_spawn"
    } else if contains("google cloud kms decrypt command timed out") {
        "kms_decrypt_timeout"
    } else if contains("google cloud kms decrypt command exited with status") {
        "kms_decrypt_exit"
    } else if contains("failed to spawn google cloud kms encrypt command") {
        "kms_encrypt_spawn"
    } else if contains("google cloud kms encrypt command timed out") {
        "kms_encrypt_timeout"
    } else if contains("google cloud kms encrypt command exited with status") {
        "kms_encrypt_exit"
    } else if contains("protected JSONL") {
        "protected_jsonl"
    } else if contains("external integrity anchor") {
        "external_anchor"
    } else {
        "other"
    };
    (phase, category)
}

fn required_denial_receipt_error(error: anyhow::Error) -> String {
    let (phase, category) = required_denial_receipt_category(&error);
    let io_error = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<std::io::Error>());
    tracing::error!(
        target: "tandem_server::audit",
        phase,
        category,
        io_kind = ?io_error.map(std::io::Error::kind),
        os_error = ?io_error.and_then(std::io::Error::raw_os_error),
        "required denial receipt failed"
    );
    REQUIRED_DENIAL_RECEIPT_PUBLIC_ERROR.to_string()
}

#[cfg(test)]
#[tokio::test]
async fn denial_receipt_diagnostic_and_response_hide_error_content() {
    let error = anyhow::anyhow!(
        "failed to spawn google cloud kms decrypt command secret-token: Resource temporarily unavailable (os error 11)"
    )
    .context("read governance JSONL store secret-path");
    assert_eq!(
        required_denial_receipt_category(&error),
        ("read", "kms_decrypt_spawn")
    );
    let raw_error = format!("{error:#}");
    let safe_error = required_denial_receipt_error(error);
    assert_eq!(safe_error, REQUIRED_DENIAL_RECEIPT_PUBLIC_ERROR);
    let response = denial_audit_failure_response(&raw_error);
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("denial body");
    let encoded = String::from_utf8(body.to_vec()).expect("JSON body");
    assert!(!encoded.contains("secret-token") && !encoded.contains("secret-path"));
    let json: serde_json::Value = serde_json::from_str(&encoded).expect("JSON response");
    assert_eq!(json["code"], "AUDIT_PERSISTENCE_FAILED");
    assert_eq!(json["error"], REQUIRED_DENIAL_RECEIPT_PUBLIC_ERROR);
}

pub(crate) async fn enrich_verified_context_with_org_unit_grants(
    state: &AppState,
    verified: &mut VerifiedTenantContext,
    hosted_memberships: Option<Vec<OrganizationUnitMembership>>,
) {
    if verified.strict_projection.is_none() {
        return;
    }
    // An empty hosted membership set is authoritative; local state cannot restore it.
    let hosted = hosted_memberships.is_some();
    let memberships = match hosted_memberships {
        Some(memberships) => memberships,
        None => state
            .enterprise
            .org_unit_memberships
            .read()
            .await
            .values()
            .cloned()
            .collect(),
    };
    let access_grants = state
        .enterprise
        .org_unit_access_grants
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    project_org_unit_grants_into_verified_context(
        verified,
        memberships.iter(),
        access_grants
            .iter()
            .filter(|grant| !hosted || local_hosted_data_grant(grant)),
        crate::util::time::now_ms(),
    );
}

pub(super) fn local_hosted_data_grant(grant: &OrganizationUnitAccessGrant) -> bool {
    // Deployment operations are control-plane authored; reject mixed permission grants.
    grant.resource.resource_kind != tandem_types::ResourceKind::HostedDeployment
        && grant.permissions.iter().all(|permission| {
            matches!(
                permission,
                AccessPermission::View
                    | AccessPermission::Read
                    | AccessPermission::Edit
                    | AccessPermission::Execute
                    | AccessPermission::Delegate
                    | AccessPermission::Admin
            )
        })
}

pub(super) fn project_org_unit_grants_into_verified_context<'a>(
    verified: &mut VerifiedTenantContext,
    memberships: impl Iterator<Item = &'a OrganizationUnitMembership>,
    access_grants: impl Iterator<Item = &'a OrganizationUnitAccessGrant>,
    now_ms: u64,
) {
    let Some(strict_principal) = verified
        .strict_projection
        .as_ref()
        .map(|projection| projection.principal.clone())
    else {
        return;
    };
    let candidate_principals = org_unit_grant_candidate_principals(verified, &strict_principal);
    let memberships = memberships
        .filter(|membership| {
            organization_unit_membership_matches_verified_context(membership, verified)
                && membership.is_active_at(now_ms)
                && candidate_principals.contains(&membership.member)
        })
        .cloned()
        .collect::<Vec<_>>();
    if memberships.is_empty() {
        return;
    }
    let access_grants = access_grants
        .filter(|grant| {
            organization_unit_access_grant_matches_verified_context(grant, verified)
                && grant.is_active_at(now_ms)
        })
        .cloned()
        .collect::<Vec<_>>();

    let Some(strict_projection) = verified.strict_projection.as_mut() else {
        return;
    };
    let mut existing_grant_ids = strict_projection
        .grants
        .iter()
        .map(|grant| grant.grant_id.clone())
        .collect::<BTreeSet<_>>();
    for access_grant in &access_grants {
        for membership in &memberships {
            let Some(scoped_grant) =
                access_grant.to_scoped_grant_for_membership(membership, now_ms)
            else {
                continue;
            };
            if existing_grant_ids.insert(scoped_grant.grant_id.clone()) {
                strict_projection.grants.push(scoped_grant);
            }
        }
    }
}

fn org_unit_grant_candidate_principals(
    verified: &VerifiedTenantContext,
    strict_principal: &PrincipalRef,
) -> Vec<PrincipalRef> {
    let mut principals = vec![strict_principal.clone()];
    principals.push(PrincipalRef::human_user(
        verified.human_actor.actor_id.clone(),
    ));
    if let Some(actor_id) = verified.tenant_context.actor_id.as_ref() {
        principals.push(PrincipalRef::human_user(actor_id.clone()));
    }
    if let Some(tenant_actor_id) = strict_principal.tenant_actor_id.as_ref() {
        principals.push(PrincipalRef::human_user(tenant_actor_id.clone()));
    }
    principals.sort_by(|left, right| {
        format!("{:?}:{}", left.kind, left.id).cmp(&format!("{:?}:{}", right.kind, right.id))
    });
    principals.dedup();
    principals
}

fn organization_unit_membership_matches_verified_context(
    membership: &OrganizationUnitMembership,
    verified: &VerifiedTenantContext,
) -> bool {
    membership.tenant_context.org_id == verified.tenant_context.org_id
        && membership.tenant_context.workspace_id == verified.tenant_context.workspace_id
        && membership.tenant_context.deployment_id == verified.tenant_context.deployment_id
}

fn organization_unit_access_grant_matches_verified_context(
    grant: &OrganizationUnitAccessGrant,
    verified: &VerifiedTenantContext,
) -> bool {
    grant.tenant_context.org_id == verified.tenant_context.org_id
        && grant.tenant_context.workspace_id == verified.tenant_context.workspace_id
        && grant.tenant_context.deployment_id == verified.tenant_context.deployment_id
}

fn runtime_auth_mode_requires_transport_token(mode: RuntimeAuthMode) -> bool {
    matches!(
        mode,
        RuntimeAuthMode::HostedSingleTenant | RuntimeAuthMode::EnterpriseRequired
    )
}

fn is_public_oauth_callback_path(path: &str) -> bool {
    let trimmed = path
        .strip_prefix("/api/engine")
        .filter(|suffix| suffix.starts_with('/'))
        .unwrap_or(path);
    let parts = trimmed
        .trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();

    matches!(
        parts.as_slice(),
        ["mcp", _, "auth", "callback"] | ["provider", _, "oauth", "callback"]
    )
}

#[cfg(test)]
fn is_public_automation_webhook_path(path: &str) -> bool {
    super::webhook_rate_limit::public_automation_webhook_token(path).is_some()
}

fn is_public_web_ui_request(method: &Method, path: &str, prefix: &str) -> bool {
    if !matches!(*method, Method::GET | Method::HEAD) {
        return false;
    }
    path == prefix
        || path
            .strip_prefix(prefix)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn is_public_health_request(method: &Method, path: &str) -> bool {
    matches!(*method, Method::GET | Method::HEAD) && path == "/global/health"
}

/// The signed Slack Events webhook (`/channels/slack/events`) authenticates via the
/// Slack request signature (verified first thing in `slack_events`), **not** the
/// Tandem API token — Slack's `url_verification` handshake and event deliveries
/// never carry it. Exempt it from the transport-token gate, like the other signed
/// webhook ingresses, so the endpoint is reachable on token-protected deployments
/// instead of 401-ing before signature verification (TAN-654).
fn is_public_slack_events_path(path: &str) -> bool {
    let trimmed = path
        .strip_prefix("/api/engine")
        .filter(|suffix| suffix.starts_with('/'))
        .unwrap_or(path);
    trimmed.trim_end_matches('/') == "/channels/slack/events"
}

fn request_transport_token_authorized(
    headers: &HeaderMap,
    expected: Option<&str>,
    mode: RuntimeAuthMode,
) -> bool {
    let Some(expected) = expected
        .map(str::trim)
        .filter(|expected| !expected.is_empty())
    else {
        return !runtime_auth_mode_requires_transport_token(mode);
    };

    extract_request_token(headers)
        .as_deref()
        .is_some_and(|provided| constant_time_token_eq(provided, expected))
}

fn constant_time_token_eq(provided: &str, expected: &str) -> bool {
    let provided_hash = Sha256::digest(provided.as_bytes());
    let expected_hash = Sha256::digest(expected.as_bytes());
    let mut diff = 0u8;
    for (left, right) in provided_hash.iter().zip(expected_hash.iter()) {
        diff |= left ^ right;
    }
    diff == 0
}

fn authorize_request(principal: &RequestPrincipal, tenant: &TenantContext) -> bool {
    if tenant.org_id.is_empty() || tenant.workspace_id.is_empty() {
        tracing::warn!(
            "Authorization denied: invalid tenant context - org_id={} workspace_id={}",
            tenant.org_id,
            tenant.workspace_id
        );
        return false;
    }

    if let Some(principal_actor) = &principal.actor_id {
        if principal_actor.is_empty() {
            tracing::warn!("Authorization denied: actor_id is empty string");
            return false;
        }

        if let Some(tenant_actor) = &tenant.actor_id {
            if principal_actor != tenant_actor {
                tracing::warn!(
                    "Authorization denied: actor mismatch - principal={} tenant={}",
                    principal_actor,
                    tenant_actor
                );
                return false;
            }
        }
    }

    true
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedEnterpriseRequestContext {
    tenant_context: TenantContext,
    request_principal: RequestPrincipal,
    verified_tenant_context: Option<VerifiedTenantContext>,
}

impl ResolvedEnterpriseRequestContext {
    fn local(tenant_context: TenantContext, request_principal: RequestPrincipal) -> Self {
        Self {
            tenant_context,
            request_principal,
            verified_tenant_context: None,
        }
    }

    fn verified(verified_tenant_context: VerifiedTenantContext) -> Self {
        let tenant_context = verified_tenant_context.tenant_context.clone();
        let request_principal = RequestPrincipal::authenticated_user(
            verified_tenant_context.human_actor.actor_id.clone(),
            verified_tenant_context.issuer.clone(),
        );
        Self {
            tenant_context,
            request_principal,
            verified_tenant_context: Some(verified_tenant_context),
        }
    }
}

fn resolve_enterprise_request_context(headers: &HeaderMap) -> ResolvedEnterpriseRequestContext {
    resolve_local_enterprise_request_context(headers, false)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TenantContextIngressError {
    MissingVerifiedContext,
    ContextAssertionKeyNotConfigured,
    ContextAssertionMalformed,
    ContextAssertionUntrusted,
    ContextAssertionExpired,
    ContextAssertionReplayed,
    UnsignedTenantHeaders,
}

impl TenantContextIngressError {
    fn as_str(self) -> &'static str {
        match self {
            Self::MissingVerifiedContext => "missing_verified_context",
            Self::ContextAssertionKeyNotConfigured => "context_assertion_key_not_configured",
            Self::ContextAssertionMalformed => "context_assertion_malformed",
            Self::ContextAssertionUntrusted => "context_assertion_untrusted",
            Self::ContextAssertionExpired => "context_assertion_expired",
            Self::ContextAssertionReplayed => "context_assertion_replayed",
            Self::UnsignedTenantHeaders => "unsigned_tenant_headers",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TenantContextIngressDenial {
    reason: TenantContextIngressError,
    tenant_context: TenantContext,
    actor: Option<String>,
    assertion_id: Option<String>,
    assertion_key_id: Option<String>,
    issuer: Option<String>,
    org_claim: Option<String>,
    workspace_claim: Option<String>,
    deployment_claim: Option<String>,
}

impl TenantContextIngressDenial {
    fn untrusted(reason: TenantContextIngressError) -> Self {
        Self {
            reason,
            tenant_context: TenantContext::local_implicit(),
            actor: None,
            assertion_id: None,
            assertion_key_id: None,
            issuer: None,
            org_claim: None,
            workspace_claim: None,
            deployment_claim: None,
        }
    }

    fn from_assertion(reason: TenantContextIngressError, assertion: &str) -> Self {
        let Some((key_id, claims)) = preview_context_assertion_for_audit(assertion) else {
            return Self::untrusted(reason);
        };
        let tenant_context = claims.tenant_context;
        Self {
            reason,
            tenant_context: TenantContext::local_implicit(),
            actor: None,
            assertion_id: Some(claims.assertion_id),
            assertion_key_id: Some(key_id),
            issuer: Some(claims.issuer),
            org_claim: Some(tenant_context.org_id),
            workspace_claim: Some(tenant_context.workspace_id),
            deployment_claim: tenant_context.deployment_id,
        }
    }

    fn verified(reason: TenantContextIngressError, verified: &VerifiedTenantContext) -> Self {
        Self {
            reason,
            tenant_context: verified.tenant_context.clone(),
            actor: Some(verified.human_actor.actor_id.clone()),
            assertion_id: Some(verified.assertion_id.clone()),
            assertion_key_id: verified.assertion_key_id.clone(),
            issuer: Some(verified.issuer.clone()),
            org_claim: Some(verified.tenant_context.org_id.clone()),
            workspace_claim: Some(verified.tenant_context.workspace_id.clone()),
            deployment_claim: verified.tenant_context.deployment_id.clone(),
        }
    }

    fn event_type(&self) -> &'static str {
        match self.reason {
            TenantContextIngressError::ContextAssertionKeyNotConfigured
            | TenantContextIngressError::ContextAssertionMalformed
            | TenantContextIngressError::ContextAssertionUntrusted
            | TenantContextIngressError::ContextAssertionExpired
            | TenantContextIngressError::ContextAssertionReplayed
            | TenantContextIngressError::MissingVerifiedContext => "context.assertion_rejected",
            TenantContextIngressError::UnsignedTenantHeaders => "tenant_context.ingress.denied",
        }
    }

    async fn append_required_audit_event(
        &self,
        state: &AppState,
        mode: RuntimeAuthMode,
        headers: &HeaderMap,
    ) -> Result<(), String> {
        crate::audit::append_protected_audit_event(
            state,
            self.event_type(),
            &self.tenant_context,
            self.actor.clone(),
            json!({
                "reason": self.reason.as_str(),
                "runtime_auth_mode": format!("{mode:?}"),
                "request_source": first_header(headers, &["x-tandem-request-source"]),
                "assertion_present": first_tandem_context_assertion(headers).is_some(),
                "raw_tenant_headers_present": has_raw_tenant_context_headers(headers),
                "assertion_id": self.assertion_id,
                "kid": self.assertion_key_id,
                "issuer": self.issuer,
                "org_claim": self.org_claim,
                "workspace_claim": self.workspace_claim,
                "deployment_claim": self.deployment_claim,
            }),
        )
        .await
        .map(|_| ())
        .map_err(required_denial_receipt_error)
    }
}
fn preview_context_assertion_for_audit(
    assertion: &str,
) -> Option<(String, TenantContextAssertionClaims)> {
    let mut parts = assertion.trim().split('.');
    let header = decode_base64url(parts.next()?)?;
    let claims = decode_base64url(parts.next()?)?;
    let _signature = parts.next()?;
    if parts.next().is_some() {
        return None;
    }
    let header: TenantContextAssertionHeader = serde_json::from_slice(&header).ok()?;
    let claims: TenantContextAssertionClaims = serde_json::from_slice(&claims).ok()?;
    Some((header.kid, claims))
}

async fn append_authorization_denial_audit_event(
    state: &AppState,
    resolved: &ResolvedEnterpriseRequestContext,
) -> Result<(), String> {
    crate::audit::append_protected_audit_event(
        state,
        "authority.cross_tenant_denied",
        &resolved.tenant_context,
        resolved.request_principal.actor_id.clone(),
        json!({
            "reason": "request_principal_tenant_mismatch",
            "principal_actor": resolved.request_principal.actor_id,
            "principal_source": resolved.request_principal.source,
            "resource_ref": {
                "kind": "tenant_context",
                "org_id": resolved.tenant_context.org_id,
                "workspace_id": resolved.tenant_context.workspace_id,
                "deployment_id": resolved.tenant_context.deployment_id,
            },
            "tenant_context": resolved.tenant_context,
        }),
    )
    .await
    .map(|_| ())
    .map_err(required_denial_receipt_error)
}

#[cfg(test)]
fn resolve_enterprise_request_context_for_mode(
    headers: &HeaderMap,
    mode: RuntimeAuthMode,
) -> Result<ResolvedEnterpriseRequestContext, TenantContextIngressError> {
    resolve_enterprise_request_context_for_mode_with_denial(headers, mode, false)
        .map_err(|denial| denial.reason)
}

#[cfg(test)]
fn resolve_enterprise_request_context_for_mode_with_denial(
    headers: &HeaderMap,
    mode: RuntimeAuthMode,
    trust_test_tenant_headers: bool,
) -> Result<ResolvedEnterpriseRequestContext, TenantContextIngressDenial> {
    match mode {
        RuntimeAuthMode::LocalSingleTenant => Ok(resolve_local_enterprise_request_context(
            headers,
            trust_test_tenant_headers,
        )),
        RuntimeAuthMode::HostedSingleTenant | RuntimeAuthMode::EnterpriseRequired => {
            if has_raw_tenant_context_headers(headers) {
                return Err(TenantContextIngressDenial::untrusted(
                    TenantContextIngressError::UnsignedTenantHeaders,
                ));
            }
            let assertion = first_tandem_context_assertion(headers).ok_or_else(|| {
                TenantContextIngressDenial::untrusted(
                    TenantContextIngressError::MissingVerifiedContext,
                )
            })?;
            let verifier = TenantContextAssertionVerifier::from_env()
                .map_err(|reason| TenantContextIngressDenial::from_assertion(reason, &assertion))?;
            let verified_tenant_context = verifier
                .verify(&assertion)
                .map_err(|reason| verifier.denial_for_error(&assertion, reason))?;
            enforce_context_assertion_replay_policy(&assertion, &verified_tenant_context).map_err(
                |reason| TenantContextIngressDenial::verified(reason, &verified_tenant_context),
            )?;
            Ok(ResolvedEnterpriseRequestContext::verified(
                verified_tenant_context,
            ))
        }
    }
}

fn resolve_enterprise_request_context_for_mode_with_cached_security(
    headers: &HeaderMap,
    mode: RuntimeAuthMode,
    trust_test_tenant_headers: bool,
    assertion_security: Option<&crate::context_assertion_security::RuntimeContextAssertionSecurity>,
) -> Result<ResolvedEnterpriseRequestContext, TenantContextIngressDenial> {
    match mode {
        RuntimeAuthMode::LocalSingleTenant => Ok(resolve_local_enterprise_request_context(
            headers,
            trust_test_tenant_headers,
        )),
        RuntimeAuthMode::HostedSingleTenant | RuntimeAuthMode::EnterpriseRequired => {
            if has_raw_tenant_context_headers(headers) {
                return Err(TenantContextIngressDenial::untrusted(
                    TenantContextIngressError::UnsignedTenantHeaders,
                ));
            }
            let assertion = first_tandem_context_assertion(headers).ok_or_else(|| {
                TenantContextIngressDenial::untrusted(
                    TenantContextIngressError::MissingVerifiedContext,
                )
            })?;
            let verifier = assertion_security.ok_or_else(|| {
                TenantContextIngressDenial::from_assertion(
                    TenantContextIngressError::ContextAssertionKeyNotConfigured,
                    &assertion,
                )
            })?;
            let verified_tenant_context = verifier.verify(&assertion).map_err(|error| {
                TenantContextIngressDenial::from_assertion(
                    map_shared_context_assertion_error(error),
                    &assertion,
                )
            })?;
            Ok(ResolvedEnterpriseRequestContext::verified(
                verified_tenant_context,
            ))
        }
    }
}

fn map_shared_context_assertion_error(
    error: tandem_enterprise_contract::ContextAssertionError,
) -> TenantContextIngressError {
    use tandem_enterprise_contract::ContextAssertionError as Shared;
    match error {
        Shared::KeyNotConfigured | Shared::ReplayBackendUnavailable => {
            TenantContextIngressError::ContextAssertionKeyNotConfigured
        }
        Shared::MalformedAssertion
        | Shared::MalformedHeader
        | Shared::UnsupportedVersion
        | Shared::InvalidIdentity
        | Shared::InvalidPolicy => TenantContextIngressError::ContextAssertionMalformed,
        Shared::Expired | Shared::NotYetValid | Shared::LifetimeExceeded | Shared::TimeOverflow => {
            TenantContextIngressError::ContextAssertionExpired
        }
        Shared::Replayed | Shared::ReplayCapacityExceeded => {
            TenantContextIngressError::ContextAssertionReplayed
        }
        Shared::BadSignature
        | Shared::BadIssuer
        | Shared::BadAudience
        | Shared::UnknownKey
        | Shared::KeyringDenied(_) => TenantContextIngressError::ContextAssertionUntrusted,
    }
}

fn local_request_source(headers: &HeaderMap) -> String {
    first_header(headers, &["x-tandem-request-source"]).unwrap_or_else(|| {
        if extract_request_token(headers).is_some() {
            "api_token".to_string()
        } else {
            "local_control_panel".to_string()
        }
    })
}

fn resolve_secure_local_enterprise_request_context(
    headers: &HeaderMap,
) -> ResolvedEnterpriseRequestContext {
    let tenant_context = TenantContext::local_implicit();
    let request_principal = RequestPrincipal {
        actor_id: None,
        source: local_request_source(headers),
    };
    ResolvedEnterpriseRequestContext::local(tenant_context, request_principal)
}

fn resolve_local_enterprise_request_context(
    headers: &HeaderMap,
    trust_test_tenant_headers: bool,
) -> ResolvedEnterpriseRequestContext {
    if trust_test_tenant_headers {
        resolve_test_header_local_enterprise_request_context(headers)
    } else {
        resolve_secure_local_enterprise_request_context(headers)
    }
}

fn resolve_test_header_local_enterprise_request_context(
    headers: &HeaderMap,
) -> ResolvedEnterpriseRequestContext {
    let resolver = HeaderTenantContextResolver;
    let org_id = first_header(headers, &["x-tandem-org-id", "x-tenant-org-id"]);
    let workspace_id = first_header(headers, &["x-tandem-workspace-id", "x-tenant-workspace-id"]);
    let actor_id = first_header(headers, &["x-tandem-actor-id", "x-user-id"]);
    // Actor identity does not change tenancy in real standalone mode. Keep
    // actor-only test requests production-faithful while still allowing tests
    // with explicit org/workspace headers to exercise hosted tenant contexts.
    let tenant_context = if org_id.is_none() && workspace_id.is_none() {
        TenantContext::local_implicit()
    } else {
        resolver.resolve_tenant_context(
            org_id.as_deref(),
            workspace_id.as_deref(),
            actor_id.as_deref(),
        )
    };
    let request_principal = RequestPrincipal {
        actor_id,
        source: local_request_source(headers),
    };
    ResolvedEnterpriseRequestContext::local(tenant_context, request_principal)
}

fn first_tandem_context_assertion(headers: &HeaderMap) -> Option<String> {
    first_header(
        headers,
        &[
            "x-tandem-context-assertion",
            "x-tandem-context-jws",
            "x-tandem-tenant-context-jws",
        ],
    )
}

fn has_raw_tenant_context_headers(headers: &HeaderMap) -> bool {
    first_header(
        headers,
        &[
            "x-tandem-org-id",
            "x-tenant-org-id",
            "x-tandem-workspace-id",
            "x-tenant-workspace-id",
            "x-tandem-actor-id",
            "x-user-id",
        ],
    )
    .is_some()
}

fn first_header(headers: &HeaderMap, names: &[&str]) -> Option<String> {
    for name in names {
        if let Some(value) = headers
            .get(*name)
            .and_then(|v| v.to_str().ok())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }
    None
}

#[cfg(test)]
include!("middleware_parts/context_assertion_test_helpers.rs");

fn decode_base64url(raw: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(raw))
        .ok()
}

#[cfg(test)]
fn current_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn extract_request_token(headers: &HeaderMap) -> Option<String> {
    if let Some(token) = headers
        .get("x-agent-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(token.to_string());
    }
    if let Some(token) = headers
        .get("x-tandem-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(token.to_string());
    }

    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())?;
    let trimmed = auth.trim();
    let bearer = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))?;
    let token = bearer.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

#[cfg(test)]
#[path = "middleware_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "middleware_hosted_policy_tests.rs"]
mod hosted_policy_tests;

#[cfg(test)]
#[path = "tests/middleware_hosted_signed_tests.rs"]
mod hosted_signed_tests;
#[cfg(test)]
#[path = "tests/middleware_hosted_workflow_hook_tests.rs"]
mod hosted_workflow_hook_tests;
#[cfg(test)]
mod slack_events_bypass_tests {
    use super::is_public_slack_events_path;

    #[test]
    fn slack_events_path_bypasses_token_gate() {
        assert!(is_public_slack_events_path("/channels/slack/events"));
        assert!(is_public_slack_events_path(
            "/api/engine/channels/slack/events"
        ));
        assert!(is_public_slack_events_path("/channels/slack/events/"));
        // Sibling routes and other channels are not exempted by this predicate.
        assert!(!is_public_slack_events_path("/channels/slack/interactions"));
        assert!(!is_public_slack_events_path("/channels/discord/events"));
        assert!(!is_public_slack_events_path("/global/health"));
    }
}

#[cfg(test)]
mod web_ui_bypass_tests {
    use super::is_public_web_ui_request;
    use axum::http::Method;

    #[test]
    fn web_ui_bypass_is_method_and_segment_exact() {
        assert!(is_public_web_ui_request(&Method::GET, "/admin", "/admin"));
        assert!(is_public_web_ui_request(
            &Method::HEAD,
            "/ui/operators/assets/app.js",
            "/ui/operators"
        ));
        assert!(!is_public_web_ui_request(
            &Method::POST,
            "/admin/token/generate",
            "/admin"
        ));
        assert!(!is_public_web_ui_request(
            &Method::GET,
            "/administrator",
            "/admin"
        ));
        assert!(!is_public_web_ui_request(
            &Method::GET,
            "/admin%2fapi",
            "/admin"
        ));
    }
}

#[cfg(test)]
mod health_bypass_tests {
    use super::is_public_health_request;
    use axum::http::Method;

    #[test]
    fn health_bypass_is_method_and_path_exact() {
        assert!(is_public_health_request(&Method::GET, "/global/health"));
        assert!(is_public_health_request(&Method::HEAD, "/global/health"));
        assert!(!is_public_health_request(&Method::POST, "/global/health"));
        assert!(!is_public_health_request(&Method::PATCH, "/global/health"));
        assert!(!is_public_health_request(&Method::GET, "/global/health/"));
    }
}

pub(super) async fn startup_gate(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() == Method::OPTIONS {
        return next.run(request).await;
    }
    if is_public_health_request(request.method(), request.uri().path()) {
        return next.run(request).await;
    }
    if state.is_ready() {
        return next.run(request).await;
    }

    let snapshot = state.startup_snapshot().await;
    let status_text = match snapshot.status {
        StartupStatus::Starting => "starting",
        StartupStatus::Ready => "ready",
        StartupStatus::Failed => "failed",
    };
    let error = format!(
        "Engine {}: phase={} attempt_id={} elapsed_ms={}{}",
        status_text,
        snapshot.phase,
        snapshot.attempt_id,
        snapshot.elapsed_ms,
        snapshot
            .last_error
            .as_ref()
            .map(|e| format!(" error={}", e))
            .unwrap_or_default()
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorEnvelope::new(
            error,
            match snapshot.status {
                StartupStatus::Failed => ErrorCode::EngineStartupFailed,
                _ => ErrorCode::EngineStarting,
            },
        )),
    )
        .into_response()
}
