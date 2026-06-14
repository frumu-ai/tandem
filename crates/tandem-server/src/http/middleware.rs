use axum::extract::{Request, State};
use axum::http::header;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::Ordering;
use tandem_types::{
    AccessPermission, DataBoundary, DataClass, GrantSource, HeaderTenantContextResolver,
    NoopRequestAuthorizationHook, OrganizationUnitAccessGrant, OrganizationUnitMembership,
    PrincipalRef, RequestAuthorizationHook, RequestPrincipal, ResourceKind, ResourceRef,
    ResourceScope, RuntimeAuthMode, ScopedGrant, SigningKeyPurpose, TenantContext,
    TenantContextAssertionClaims, TenantContextAssertionHeader, TenantContextResolver,
    TenantSource, VerifiedTenantContext,
};

use crate::{AppState, StartupStatus};

use super::ErrorEnvelope;
use crate::config::env::resolve_runtime_auth_mode;

const DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS: u64 = 10_000;
const MAX_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS: u64 = 60_000;

pub(super) async fn auth_gate(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Response {
    if request.method() == Method::OPTIONS {
        return next.run(request).await;
    }
    let path = request.uri().path();
    if state.web_ui_enabled() && request.uri().path().starts_with(&state.web_ui_prefix()) {
        return next.run(request).await;
    }
    if path == "/global/health" {
        return next.run(request).await;
    }
    let runtime_auth_mode = resolve_runtime_auth_mode();
    if path == "/bug-monitor/intake/report" || path == "/failure-reporter/intake/report" {
        if !runtime_auth_mode_requires_transport_token(runtime_auth_mode)
            && !attach_enterprise_request_context_for_mode(&state, &mut request, runtime_auth_mode)
                .await
        {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorEnvelope {
                    error: "Unauthorized: tenant context denied".to_string(),
                    code: Some("TENANT_CONTEXT_DENIED".to_string()),
                }),
            )
                .into_response();
        }
        if !runtime_auth_mode_requires_transport_token(runtime_auth_mode) {
            return next.run(request).await;
        }
    }

    let required = state.api_token().await;
    if !request_transport_token_authorized(
        request.headers(),
        required.as_deref(),
        runtime_auth_mode,
    ) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(ErrorEnvelope {
                error: "Unauthorized: missing or invalid API token".to_string(),
                code: Some("AUTH_REQUIRED".to_string()),
            }),
        )
            .into_response();
    }

    if !attach_enterprise_request_context_for_mode(&state, &mut request, runtime_auth_mode).await {
        return (
            StatusCode::FORBIDDEN,
            Json(ErrorEnvelope {
                error: "Unauthorized: tenant context denied".to_string(),
                code: Some("TENANT_CONTEXT_DENIED".to_string()),
            }),
        )
            .into_response();
    }
    next.run(request).await
}

async fn attach_enterprise_request_context_for_mode(
    state: &AppState,
    request: &mut Request,
    mode: RuntimeAuthMode,
) -> bool {
    let headers = request.headers();
    let resolved = match resolve_enterprise_request_context_for_mode_with_denial(
        headers,
        mode,
        state.trust_test_tenant_headers.load(Ordering::Relaxed),
    ) {
        Ok(context) => context,
        Err(denial) => {
            tracing::warn!(
                "Authorization denied: tenant context ingress rejected - reason={}",
                denial.reason.as_str()
            );
            denial
                .append_protected_audit_event(state, mode, headers)
                .await;
            return false;
        }
    };

    if !authorize_request(&resolved.request_principal, &resolved.tenant_context) {
        tracing::warn!(
            "Authorization denied: principal={:?} tenant={} source={}",
            resolved.request_principal.actor_id,
            resolved.tenant_context.org_id,
            resolved.request_principal.source
        );
        append_authorization_denial_audit_event(state, &resolved).await;
        return false;
    }

    if let Some(mut verified_tenant_context) = resolved.verified_tenant_context {
        enrich_verified_context_with_org_unit_grants(state, &mut verified_tenant_context).await;
        super::cross_tenant_grants::enrich_verified_context_with_inbound_cross_tenant_grants(
            state,
            &mut verified_tenant_context,
        )
        .await;
        request.extensions_mut().insert(verified_tenant_context);
    }
    request.extensions_mut().insert(resolved.tenant_context);
    request.extensions_mut().insert(resolved.request_principal);
    true
}

async fn enrich_verified_context_with_org_unit_grants(
    state: &AppState,
    verified: &mut VerifiedTenantContext,
) {
    if verified.strict_projection.is_none() {
        return;
    }
    let memberships = state
        .enterprise
        .org_unit_memberships
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
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
        access_grants.iter(),
        crate::util::time::now_ms(),
    );
}

fn project_org_unit_grants_into_verified_context<'a>(
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
    issuer: Option<String>,
    audience: Option<String>,
}

impl TenantContextIngressDenial {
    fn untrusted(reason: TenantContextIngressError) -> Self {
        Self {
            reason,
            tenant_context: TenantContext::local_implicit(),
            actor: None,
            assertion_id: None,
            issuer: None,
            audience: None,
        }
    }

    fn verified(reason: TenantContextIngressError, verified: &VerifiedTenantContext) -> Self {
        Self {
            reason,
            tenant_context: verified.tenant_context.clone(),
            actor: Some(verified.human_actor.actor_id.clone()),
            assertion_id: Some(verified.assertion_id.clone()),
            issuer: Some(verified.issuer.clone()),
            audience: Some(verified.audience.clone()),
        }
    }

    fn event_type(&self) -> &'static str {
        match self.reason {
            TenantContextIngressError::ContextAssertionKeyNotConfigured
            | TenantContextIngressError::ContextAssertionMalformed
            | TenantContextIngressError::ContextAssertionUntrusted
            | TenantContextIngressError::ContextAssertionExpired
            | TenantContextIngressError::ContextAssertionReplayed
            | TenantContextIngressError::MissingVerifiedContext => "context_assertion.rejected",
            TenantContextIngressError::UnsignedTenantHeaders => "tenant_context.ingress.denied",
        }
    }

    async fn append_protected_audit_event(
        &self,
        state: &AppState,
        mode: RuntimeAuthMode,
        headers: &HeaderMap,
    ) {
        let _ = crate::audit::append_protected_audit_event(
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
                "issuer": self.issuer,
                "audience": self.audience,
            }),
        )
        .await;
    }
}

async fn append_authorization_denial_audit_event(
    state: &AppState,
    resolved: &ResolvedEnterpriseRequestContext,
) {
    let _ = crate::audit::append_protected_audit_event(
        state,
        "tenant_context.authorization.denied",
        &resolved.tenant_context,
        resolved.request_principal.actor_id.clone(),
        json!({
            "reason": "request_principal_tenant_mismatch",
            "request_principal": resolved.request_principal,
            "tenant_context": resolved.tenant_context,
        }),
    )
    .await;
}

fn resolve_enterprise_request_context_for_mode(
    headers: &HeaderMap,
    mode: RuntimeAuthMode,
) -> Result<ResolvedEnterpriseRequestContext, TenantContextIngressError> {
    resolve_enterprise_request_context_for_mode_with_denial(headers, mode, false)
        .map_err(|denial| denial.reason)
}

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
                .map_err(TenantContextIngressDenial::untrusted)?;
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
    let tenant_context = resolver.resolve_tenant_context(
        first_header(headers, &["x-tandem-org-id", "x-tenant-org-id"]).as_deref(),
        first_header(headers, &["x-tandem-workspace-id", "x-tenant-workspace-id"]).as_deref(),
        first_header(headers, &["x-tandem-actor-id", "x-user-id"]).as_deref(),
    );
    let request_principal = RequestPrincipal {
        actor_id: tenant_context.actor_id.clone(),
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct TenantContextAssertionVerifier {
    public_keys_by_id: BTreeMap<String, ContextAssertionPublicKey>,
    legacy_public_key: Option<ContextAssertionPublicKey>,
    issuer: String,
    audience: String,
    max_future_skew_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ContextAssertionPublicKey {
    public_key: [u8; 32],
    purpose: Option<SigningKeyPurpose>,
    organization_id: Option<String>,
    deployment_id: Option<String>,
    allowed_audiences: Vec<String>,
    allowed_resource_scope_prefixes: Vec<String>,
    not_before_ms: Option<u64>,
    not_after_ms: Option<u64>,
    status: Option<String>,
}

impl ContextAssertionPublicKey {
    fn legacy(public_key: [u8; 32]) -> Self {
        Self {
            public_key,
            purpose: None,
            organization_id: None,
            deployment_id: None,
            allowed_audiences: Vec::new(),
            allowed_resource_scope_prefixes: Vec::new(),
            not_before_ms: None,
            not_after_ms: None,
            status: None,
        }
    }
}

impl TenantContextAssertionVerifier {
    fn from_env() -> Result<Self, TenantContextIngressError> {
        let public_keys_by_id = read_context_public_keyring_from_env()?;
        let legacy_public_key = read_legacy_context_public_key_from_env()?;
        if public_keys_by_id.is_empty() && legacy_public_key.is_none() {
            return Err(TenantContextIngressError::ContextAssertionKeyNotConfigured);
        }
        let issuer = std::env::var("TANDEM_CONTEXT_ASSERTION_ISSUER")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "tandem-web".to_string());
        let audience = std::env::var("TANDEM_CONTEXT_ASSERTION_AUDIENCE")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "tandem-runtime".to_string());

        Ok(Self {
            public_keys_by_id,
            legacy_public_key,
            issuer,
            audience,
            max_future_skew_ms: resolve_context_assertion_max_future_skew_ms(),
        })
    }

    fn verify(&self, assertion: &str) -> Result<VerifiedTenantContext, TenantContextIngressError> {
        self.verify_at(assertion, current_unix_ms())
    }

    fn verify_at(
        &self,
        assertion: &str,
        now_ms: u64,
    ) -> Result<VerifiedTenantContext, TenantContextIngressError> {
        let claims = self.verify_signed_claims_at(assertion, now_ms)?;
        self.validate_claim_time(&claims, now_ms)?;
        Ok(claims.into())
    }

    fn denial_for_error(
        &self,
        assertion: &str,
        reason: TenantContextIngressError,
    ) -> TenantContextIngressDenial {
        if reason != TenantContextIngressError::ContextAssertionExpired {
            return TenantContextIngressDenial::untrusted(reason);
        }
        self.verify_signed_claims_at(assertion, current_unix_ms())
            .map(VerifiedTenantContext::from)
            .map(|verified| TenantContextIngressDenial::verified(reason, &verified))
            .unwrap_or_else(|_| TenantContextIngressDenial::untrusted(reason))
    }

    fn verify_signed_claims_at(
        &self,
        assertion: &str,
        now_ms: u64,
    ) -> Result<TenantContextAssertionClaims, TenantContextIngressError> {
        let assertion = assertion.trim();
        let mut parts = assertion.split('.');
        let encoded_header = parts
            .next()
            .filter(|part| !part.is_empty())
            .ok_or(TenantContextIngressError::ContextAssertionMalformed)?;
        let encoded_claims = parts
            .next()
            .filter(|part| !part.is_empty())
            .ok_or(TenantContextIngressError::ContextAssertionMalformed)?;
        let encoded_signature = parts
            .next()
            .filter(|part| !part.is_empty())
            .ok_or(TenantContextIngressError::ContextAssertionMalformed)?;
        if parts.next().is_some() {
            return Err(TenantContextIngressError::ContextAssertionMalformed);
        }

        let header_bytes = decode_base64url(encoded_header)
            .ok_or(TenantContextIngressError::ContextAssertionMalformed)?;
        let claims_bytes = decode_base64url(encoded_claims)
            .ok_or(TenantContextIngressError::ContextAssertionMalformed)?;
        let signature_bytes: [u8; 64] = decode_base64url(encoded_signature)
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(TenantContextIngressError::ContextAssertionMalformed)?;

        let header: TenantContextAssertionHeader = serde_json::from_slice(&header_bytes)
            .map_err(|_| TenantContextIngressError::ContextAssertionMalformed)?;
        validate_context_assertion_header(&header)?;

        let key = self
            .key_for_kid(&header.kid)
            .ok_or(TenantContextIngressError::ContextAssertionUntrusted)?;
        let verifying_key = VerifyingKey::from_bytes(&key.public_key)
            .map_err(|_| TenantContextIngressError::ContextAssertionKeyNotConfigured)?;
        let signature = Signature::from_bytes(&signature_bytes);
        let signing_input = format!("{encoded_header}.{encoded_claims}");
        verifying_key
            .verify(signing_input.as_bytes(), &signature)
            .map_err(|_| TenantContextIngressError::ContextAssertionUntrusted)?;

        let claims: TenantContextAssertionClaims = serde_json::from_slice(&claims_bytes)
            .map_err(|_| TenantContextIngressError::ContextAssertionMalformed)?;
        self.validate_claim_identity(&claims)?;
        validate_context_assertion_key_metadata(key, &claims, now_ms)?;
        Ok(claims)
    }

    fn key_for_kid(&self, kid: &str) -> Option<&ContextAssertionPublicKey> {
        self.public_keys_by_id
            .get(kid)
            .or(self.legacy_public_key.as_ref())
    }

    fn validate_claim_identity(
        &self,
        claims: &TenantContextAssertionClaims,
    ) -> Result<(), TenantContextIngressError> {
        if claims.version != "v1" {
            return Err(TenantContextIngressError::ContextAssertionMalformed);
        }
        if claims.issuer != self.issuer || claims.audience != self.audience {
            return Err(TenantContextIngressError::ContextAssertionUntrusted);
        }
        if claims.assertion_id.trim().is_empty()
            || claims.human_actor.actor_id.trim().is_empty()
            || claims.tenant_context.org_id.trim().is_empty()
            || claims.tenant_context.workspace_id.trim().is_empty()
        {
            return Err(TenantContextIngressError::ContextAssertionMalformed);
        }
        if claims.tenant_context.source != TenantSource::Explicit
            || claims
                .tenant_context
                .deployment_id
                .as_deref()
                .map(str::trim)
                .filter(|deployment_id| !deployment_id.is_empty())
                .is_none()
        {
            return Err(TenantContextIngressError::ContextAssertionMalformed);
        }
        if claims.tenant_context.actor_id.as_deref() != Some(claims.human_actor.actor_id.as_str()) {
            return Err(TenantContextIngressError::ContextAssertionUntrusted);
        }
        if claims.authority_chain.initiated_by.actor_id.as_deref()
            != Some(claims.human_actor.actor_id.as_str())
        {
            return Err(TenantContextIngressError::ContextAssertionUntrusted);
        }
        Ok(())
    }

    fn validate_claim_time(
        &self,
        claims: &TenantContextAssertionClaims,
        now_ms: u64,
    ) -> Result<(), TenantContextIngressError> {
        if claims.is_expired_at(now_ms) || claims.issued_at_ms > now_ms + self.max_future_skew_ms {
            return Err(TenantContextIngressError::ContextAssertionExpired);
        }
        Ok(())
    }
}

/// Replay handling for verified context assertions.
///
/// Assertions are bearer context that first-party clients legitimately reuse
/// across many requests within the expiry window (e.g. tandem-channels caches
/// one assertion per process), so the default cannot be one-shot:
///
/// - `bound` (default): the first use binds an `assertion_id` to the SHA-256
///   of the exact assertion bytes. Re-presenting the identical assertion is
///   allowed until expiry; a different assertion carrying the same
///   `assertion_id` is rejected as a replay/substitution.
/// - `one_shot`: an `assertion_id` is accepted exactly once. Requires the
///   issuing control plane to mint a fresh assertion per request.
/// - `off`: no replay tracking (unsafe; migration escape hatch only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AssertionReplayMode {
    Bound,
    OneShot,
    Off,
}

fn resolve_assertion_replay_mode() -> AssertionReplayMode {
    match std::env::var("TANDEM_CONTEXT_ASSERTION_REPLAY_MODE")
        .ok()
        .map(|value| value.trim().to_ascii_lowercase())
        .as_deref()
    {
        Some("one_shot") | Some("one-shot") | Some("oneshot") => AssertionReplayMode::OneShot,
        Some("off") => AssertionReplayMode::Off,
        _ => AssertionReplayMode::Bound,
    }
}

struct AssertionReplayEntry {
    fingerprint: [u8; 32],
    expires_at_ms: u64,
}

struct AssertionReplayGuard {
    entries: std::sync::Mutex<std::collections::HashMap<String, AssertionReplayEntry>>,
}

/// Sweep expired entries once the map grows past this size, bounding memory
/// without a background task.
const ASSERTION_REPLAY_SWEEP_THRESHOLD: usize = 1024;

/// Entries are retained slightly past assertion expiry so a clock-skewed
/// replay near the expiry boundary still hits the cache instead of slipping
/// through between sweep and expiry validation.
const ASSERTION_REPLAY_RETENTION_GRACE_MS: u64 = 60_000;

impl AssertionReplayGuard {
    fn new() -> Self {
        Self {
            entries: std::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }

    fn global() -> &'static Self {
        static GUARD: std::sync::OnceLock<AssertionReplayGuard> = std::sync::OnceLock::new();
        GUARD.get_or_init(AssertionReplayGuard::new)
    }

    fn check_and_record(
        &self,
        mode: AssertionReplayMode,
        assertion_id: &str,
        fingerprint: [u8; 32],
        expires_at_ms: u64,
        now_ms: u64,
    ) -> Result<(), TenantContextIngressError> {
        if mode == AssertionReplayMode::Off {
            return Ok(());
        }
        let mut entries = self
            .entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if entries.len() >= ASSERTION_REPLAY_SWEEP_THRESHOLD {
            entries.retain(|_, entry| {
                entry
                    .expires_at_ms
                    .saturating_add(ASSERTION_REPLAY_RETENTION_GRACE_MS)
                    > now_ms
            });
        }
        match entries.get(assertion_id) {
            None => {
                entries.insert(
                    assertion_id.to_string(),
                    AssertionReplayEntry {
                        fingerprint,
                        expires_at_ms,
                    },
                );
                Ok(())
            }
            Some(entry)
                if entry
                    .expires_at_ms
                    .saturating_add(ASSERTION_REPLAY_RETENTION_GRACE_MS)
                    <= now_ms =>
            {
                entries.insert(
                    assertion_id.to_string(),
                    AssertionReplayEntry {
                        fingerprint,
                        expires_at_ms,
                    },
                );
                Ok(())
            }
            Some(entry) => match mode {
                AssertionReplayMode::OneShot => {
                    Err(TenantContextIngressError::ContextAssertionReplayed)
                }
                AssertionReplayMode::Bound if entry.fingerprint == fingerprint => Ok(()),
                AssertionReplayMode::Bound => {
                    Err(TenantContextIngressError::ContextAssertionReplayed)
                }
                AssertionReplayMode::Off => Ok(()),
            },
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len()
    }
}

fn assertion_fingerprint(assertion: &str) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(assertion.trim().as_bytes());
    hasher.finalize().into()
}

fn enforce_context_assertion_replay_policy(
    assertion: &str,
    verified: &VerifiedTenantContext,
) -> Result<(), TenantContextIngressError> {
    let mode = resolve_assertion_replay_mode();
    let result = AssertionReplayGuard::global().check_and_record(
        mode,
        &verified.assertion_id,
        assertion_fingerprint(assertion),
        verified.expires_at_ms,
        current_unix_ms(),
    );
    if let Err(error) = result {
        tracing::warn!(
            assertion_id = %verified.assertion_id,
            org_id = %verified.tenant_context.org_id,
            replay_mode = ?mode,
            "Authorization denied: context assertion rejected as replayed - reason={}",
            error.as_str()
        );
    }
    result
}

fn resolve_context_assertion_max_future_skew_ms() -> u64 {
    std::env::var("TANDEM_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS)
        .clamp(
            DEFAULT_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS,
            MAX_CONTEXT_ASSERTION_MAX_FUTURE_SKEW_MS,
        )
}

fn read_context_public_keyring_from_env(
) -> Result<BTreeMap<String, ContextAssertionPublicKey>, TenantContextIngressError> {
    let Some(raw_keys) = std::env::var("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let path = std::env::var("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEYS_FILE")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())?;
            std::fs::read_to_string(path)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
    else {
        return Ok(BTreeMap::new());
    };
    parse_context_public_keyring(&raw_keys)
        .ok_or(TenantContextIngressError::ContextAssertionKeyNotConfigured)
}

fn read_legacy_context_public_key_from_env(
) -> Result<Option<ContextAssertionPublicKey>, TenantContextIngressError> {
    let Some(raw_key) = std::env::var("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            let path = std::env::var("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY_FILE")
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())?;
            std::fs::read_to_string(path)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
    else {
        return Ok(None);
    };
    decode_context_public_key(&raw_key)
        .map(ContextAssertionPublicKey::legacy)
        .map(Some)
        .ok_or(TenantContextIngressError::ContextAssertionKeyNotConfigured)
}

fn parse_context_public_keyring(raw: &str) -> Option<BTreeMap<String, ContextAssertionPublicKey>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some(BTreeMap::new());
    }
    if trimmed.starts_with('{') {
        let parsed = serde_json::from_str::<BTreeMap<String, serde_json::Value>>(trimmed).ok()?;
        return parse_context_public_keyring_json_entries(parsed);
    }

    let mut entries = BTreeMap::new();
    for entry in trimmed.split([',', '\n', ';']) {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (kid, key) = entry.split_once('=').or_else(|| entry.split_once(':'))?;
        entries.insert(kid.trim().to_string(), key.trim().to_string());
    }
    parse_context_public_keyring_entries(entries)
}

fn parse_context_public_keyring_entries(
    entries: BTreeMap<String, String>,
) -> Option<BTreeMap<String, ContextAssertionPublicKey>> {
    let mut decoded = BTreeMap::new();
    for (kid, raw_key) in entries {
        let kid = kid.trim();
        if kid.is_empty() {
            return None;
        }
        decoded.insert(
            kid.to_string(),
            ContextAssertionPublicKey::legacy(decode_context_public_key(&raw_key)?),
        );
    }
    Some(decoded)
}

fn parse_context_public_keyring_json_entries(
    entries: BTreeMap<String, serde_json::Value>,
) -> Option<BTreeMap<String, ContextAssertionPublicKey>> {
    let mut decoded = BTreeMap::new();
    for (kid, value) in entries {
        let kid = kid.trim();
        if kid.is_empty() {
            return None;
        }
        let key = match value {
            serde_json::Value::String(raw_key) => {
                ContextAssertionPublicKey::legacy(decode_context_public_key(&raw_key)?)
            }
            serde_json::Value::Object(mut object) => {
                let public_key = object
                    .remove("public_key")
                    .or_else(|| object.remove("publicKey"))
                    .and_then(|value| value.as_str().map(ToString::to_string))
                    .and_then(|raw_key| decode_context_public_key(&raw_key))?;
                let purpose = optional_string_field(&mut object, "purpose")
                    .map(|purpose| SigningKeyPurpose::parse(&purpose))
                    .transpose()
                    .ok()?;
                ContextAssertionPublicKey {
                    public_key,
                    purpose,
                    organization_id: optional_string_field(&mut object, "organization_id")
                        .or_else(|| optional_string_field(&mut object, "organizationId"))
                        .or_else(|| optional_string_field(&mut object, "org_id"))
                        .or_else(|| optional_string_field(&mut object, "orgId")),
                    deployment_id: optional_string_field(&mut object, "deployment_id")
                        .or_else(|| optional_string_field(&mut object, "deploymentId")),
                    allowed_audiences: string_vec_field(&mut object, "allowed_audiences")
                        .or_else(|| string_vec_field(&mut object, "allowedAudiences"))
                        .unwrap_or_default(),
                    allowed_resource_scope_prefixes: string_vec_field(
                        &mut object,
                        "allowed_resource_scope_prefixes",
                    )
                    .or_else(|| string_vec_field(&mut object, "allowedResourceScopePrefixes"))
                    .unwrap_or_default(),
                    not_before_ms: optional_u64_field(&mut object, "not_before_ms")
                        .or_else(|| optional_u64_field(&mut object, "notBeforeMs")),
                    not_after_ms: optional_u64_field(&mut object, "not_after_ms")
                        .or_else(|| optional_u64_field(&mut object, "notAfterMs")),
                    status: optional_string_field(&mut object, "status"),
                }
            }
            _ => return None,
        };
        decoded.insert(kid.to_string(), key);
    }
    Some(decoded)
}

fn optional_string_field(
    object: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<String> {
    object.remove(field).and_then(|value| match value {
        serde_json::Value::String(value) => {
            let value = value.trim().to_string();
            if value.is_empty() {
                None
            } else {
                Some(value)
            }
        }
        _ => None,
    })
}

fn optional_u64_field(
    object: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<u64> {
    object.remove(field).and_then(|value| value.as_u64())
}

fn string_vec_field(
    object: &mut serde_json::Map<String, serde_json::Value>,
    field: &str,
) -> Option<Vec<String>> {
    let value = object.remove(field)?;
    match value {
        serde_json::Value::Array(values) => Some(
            values
                .into_iter()
                .filter_map(|value| {
                    value
                        .as_str()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .map(ToString::to_string)
                })
                .collect(),
        ),
        serde_json::Value::String(value) => Some(
            value
                .split([',', ';', '\n'])
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToString::to_string)
                .collect(),
        ),
        _ => None,
    }
}

fn validate_context_assertion_header(
    header: &TenantContextAssertionHeader,
) -> Result<(), TenantContextIngressError> {
    if header.alg != "EdDSA" || header.typ != "tandem-tenant-context+jws" || header.kid.is_empty() {
        return Err(TenantContextIngressError::ContextAssertionMalformed);
    }
    Ok(())
}

fn validate_context_assertion_key_metadata(
    key: &ContextAssertionPublicKey,
    claims: &TenantContextAssertionClaims,
    now_ms: u64,
) -> Result<(), TenantContextIngressError> {
    if let Some(status) = key.status.as_deref() {
        if !status.eq_ignore_ascii_case("active") {
            return Err(TenantContextIngressError::ContextAssertionUntrusted);
        }
    }
    if let Some(purpose) = key.purpose {
        if purpose != SigningKeyPurpose::ContextAssertion {
            return Err(TenantContextIngressError::ContextAssertionUntrusted);
        }
    }
    if key
        .not_before_ms
        .map(|not_before_ms| now_ms < not_before_ms)
        .unwrap_or(false)
        || key
            .not_after_ms
            .map(|not_after_ms| now_ms >= not_after_ms)
            .unwrap_or(false)
    {
        return Err(TenantContextIngressError::ContextAssertionExpired);
    }
    if !key.allowed_audiences.is_empty()
        && !key
            .allowed_audiences
            .iter()
            .any(|audience| audience == &claims.audience)
    {
        return Err(TenantContextIngressError::ContextAssertionUntrusted);
    }
    if key
        .organization_id
        .as_deref()
        .map(|organization_id| organization_id != claims.tenant_context.org_id)
        .unwrap_or(false)
    {
        return Err(TenantContextIngressError::ContextAssertionUntrusted);
    }
    if key
        .deployment_id
        .as_deref()
        .map(|deployment_id| claims.tenant_context.deployment_id.as_deref() != Some(deployment_id))
        .unwrap_or(false)
    {
        return Err(TenantContextIngressError::ContextAssertionUntrusted);
    }
    if !key.allowed_resource_scope_prefixes.is_empty()
        && !context_assertion_scope_allowed(
            &key.allowed_resource_scope_prefixes,
            &context_assertion_scope_prefixes(claims),
        )
    {
        return Err(TenantContextIngressError::ContextAssertionUntrusted);
    }

    Ok(())
}

fn context_assertion_scope_allowed(
    allowed_prefixes: &[String],
    actual_prefixes: &[String],
) -> bool {
    actual_prefixes.iter().any(|actual| {
        allowed_prefixes.iter().any(|allowed| {
            let allowed = allowed.trim().trim_matches('/');
            !allowed.is_empty()
                && (actual == allowed
                    || actual
                        .strip_prefix(allowed)
                        .map(|suffix| suffix.starts_with('/'))
                        .unwrap_or(false))
        })
    })
}

fn context_assertion_scope_prefixes(claims: &TenantContextAssertionClaims) -> Vec<String> {
    let mut prefixes = vec![
        format!("org/{}", claims.tenant_context.org_id),
        format!(
            "org/{}/workspace/{}",
            claims.tenant_context.org_id, claims.tenant_context.workspace_id
        ),
    ];
    if let Some(resource_scope) = claims.resource_scope.as_ref() {
        push_resource_ref_prefixes(&mut prefixes, &resource_scope.root);
        for resource in &resource_scope.allowed_resources {
            push_resource_ref_prefixes(&mut prefixes, resource);
        }
        for resource in &resource_scope.denied_resources {
            push_resource_ref_prefixes(&mut prefixes, resource);
        }
    }
    for grant in &claims.grants {
        push_resource_ref_prefixes(&mut prefixes, &grant.resource);
    }
    prefixes.sort();
    prefixes.dedup();
    prefixes
}

fn push_resource_ref_prefixes(prefixes: &mut Vec<String>, resource: &ResourceRef) {
    prefixes.push(format!("org/{}", resource.organization_id));
    prefixes.push(format!(
        "org/{}/workspace/{}",
        resource.organization_id, resource.workspace_id
    ));
    let project_id = resource.project_id.as_deref().or_else(|| {
        (resource.resource_kind == ResourceKind::Project).then_some(resource.resource_id.as_str())
    });
    if let Some(project_id) = project_id {
        prefixes.push(format!(
            "org/{}/workspace/{}/project/{}",
            resource.organization_id, resource.workspace_id, project_id
        ));
    }
    let mut resource_prefix = format!(
        "org/{}/workspace/{}/resource/{}/{}",
        resource.organization_id,
        resource.workspace_id,
        resource_kind_scope_label(resource.resource_kind),
        resource.resource_id
    );
    if let Some(project_id) = project_id {
        resource_prefix = format!(
            "org/{}/workspace/{}/project/{}/resource/{}/{}",
            resource.organization_id,
            resource.workspace_id,
            project_id,
            resource_kind_scope_label(resource.resource_kind),
            resource.resource_id
        );
    }
    prefixes.push(resource_prefix.clone());
    if let Some(branch_id) = resource.branch_id.as_deref() {
        prefixes.push(format!("{resource_prefix}/branch/{branch_id}"));
    }
    if let Some(path_prefix) = resource.path_prefix.as_deref() {
        let path_prefix = path_prefix.trim_matches('/');
        if !path_prefix.is_empty() {
            prefixes.push(format!("{resource_prefix}/path/{path_prefix}"));
        }
    }
}

fn resource_kind_scope_label(kind: ResourceKind) -> &'static str {
    match kind {
        ResourceKind::Organization => "organization",
        ResourceKind::Workspace => "workspace",
        ResourceKind::OrganizationUnit => "organization_unit",
        ResourceKind::Department => "department",
        ResourceKind::Group => "group",
        ResourceKind::Project => "project",
        ResourceKind::DataRoom => "data_room",
        ResourceKind::SharedDrive => "shared_drive",
        ResourceKind::DocumentCollection => "document_collection",
        ResourceKind::DataStore => "data_store",
        ResourceKind::Dataset => "dataset",
        ResourceKind::Document => "document",
        ResourceKind::Repository => "repository",
        ResourceKind::Directory => "directory",
        ResourceKind::File => "file",
        ResourceKind::Artifact => "artifact",
        ResourceKind::MemorySpace => "memory_space",
        ResourceKind::KnowledgeSpace => "knowledge_space",
        ResourceKind::SecretProviderCredential => "secret_provider_credential",
        ResourceKind::Automation => "automation",
        ResourceKind::Run => "run",
        ResourceKind::Approval => "approval",
        ResourceKind::AuditExport => "audit_export",
        ResourceKind::McpServer => "mcp_server",
        ResourceKind::McpTool => "mcp_tool",
        ResourceKind::ConnectorInstance => "connector_instance",
        ResourceKind::SourceBinding => "source_binding",
        ResourceKind::SourceObject => "source_object",
        ResourceKind::IngestionJob => "ingestion_job",
        ResourceKind::ExternalIntegrationAccount => "external_integration_account",
    }
}

fn decode_context_public_key(raw: &str) -> Option<[u8; 32]> {
    decode_base64url(raw.trim())
        .or_else(|| {
            base64::engine::general_purpose::STANDARD
                .decode(raw.trim())
                .ok()
        })
        .and_then(|bytes| bytes.try_into().ok())
}

fn decode_base64url(raw: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(raw)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(raw))
        .ok()
}

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

pub(super) async fn startup_gate(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    if request.method() == Method::OPTIONS {
        return next.run(request).await;
    }
    if request.uri().path() == "/global/health" {
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
    let code = match snapshot.status {
        StartupStatus::Failed => "ENGINE_STARTUP_FAILED",
        _ => "ENGINE_STARTING",
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
        Json(ErrorEnvelope {
            error,
            code: Some(code.to_string()),
        }),
    )
        .into_response()
}
