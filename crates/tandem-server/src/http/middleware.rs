use axum::extract::{Request, State};
use axum::http::header;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
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
    let resolved = match resolve_enterprise_request_context_for_mode(headers, mode) {
        Ok(context) => context,
        Err(reason) => {
            tracing::warn!(
                "Authorization denied: tenant context ingress rejected - reason={}",
                reason.as_str()
            );
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
        .enterprise_org_unit_memberships
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    let access_grants = state
        .enterprise_org_unit_access_grants
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
    resolve_local_enterprise_request_context(headers)
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

fn resolve_enterprise_request_context_for_mode(
    headers: &HeaderMap,
    mode: RuntimeAuthMode,
) -> Result<ResolvedEnterpriseRequestContext, TenantContextIngressError> {
    match mode {
        RuntimeAuthMode::LocalSingleTenant => Ok(resolve_local_enterprise_request_context(headers)),
        RuntimeAuthMode::HostedSingleTenant | RuntimeAuthMode::EnterpriseRequired => {
            if has_raw_tenant_context_headers(headers) {
                return Err(TenantContextIngressError::UnsignedTenantHeaders);
            }
            let assertion = first_tandem_context_assertion(headers)
                .ok_or(TenantContextIngressError::MissingVerifiedContext)?;
            let verifier = TenantContextAssertionVerifier::from_env()?;
            let verified_tenant_context = verifier.verify(&assertion)?;
            enforce_context_assertion_replay_policy(&assertion, &verified_tenant_context)?;
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

#[cfg(not(test))]
fn resolve_local_enterprise_request_context(
    headers: &HeaderMap,
) -> ResolvedEnterpriseRequestContext {
    resolve_secure_local_enterprise_request_context(headers)
}

#[cfg(test)]
fn resolve_local_enterprise_request_context(
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
            max_future_skew_ms: 60_000,
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
        self.validate_claims(&claims, now_ms)?;
        validate_context_assertion_key_metadata(key, &claims, now_ms)?;
        Ok(claims.into())
    }

    fn key_for_kid(&self, kid: &str) -> Option<&ContextAssertionPublicKey> {
        self.public_keys_by_id
            .get(kid)
            .or(self.legacy_public_key.as_ref())
    }

    fn validate_claims(
        &self,
        claims: &TenantContextAssertionClaims,
        now_ms: u64,
    ) -> Result<(), TenantContextIngressError> {
        if claims.version != "v1" {
            return Err(TenantContextIngressError::ContextAssertionMalformed);
        }
        if claims.issuer != self.issuer || claims.audience != self.audience {
            return Err(TenantContextIngressError::ContextAssertionUntrusted);
        }
        if claims.is_expired_at(now_ms) || claims.issued_at_ms > now_ms + self.max_future_skew_ms {
            return Err(TenantContextIngressError::ContextAssertionExpired);
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
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use tandem_types::{AuthorityChain, HumanActor, OrganizationUnitState, TenantSource};

    #[test]
    fn resolve_enterprise_request_context_defaults_to_local_tenant() {
        let headers = HeaderMap::new();
        let resolved = resolve_enterprise_request_context(&headers);
        let tenant_context = resolved.tenant_context;
        let principal = resolved.request_principal;
        assert_eq!(tenant_context.org_id, "local");
        assert_eq!(tenant_context.workspace_id, "local");
        assert!(tenant_context.actor_id.is_none());
        assert_eq!(principal.actor_id, None);
        assert_eq!(principal.source, "local_control_panel");
    }

    #[test]
    fn resolve_enterprise_request_context_ignores_unsigned_tenant_headers_in_local_mode() {
        let mut headers = HeaderMap::new();
        headers.insert("x-tandem-org-id", HeaderValue::from_static("acme"));
        headers.insert("x-tandem-workspace-id", HeaderValue::from_static("north"));
        headers.insert("x-user-id", HeaderValue::from_static("user-1"));
        let resolved = resolve_secure_local_enterprise_request_context(&headers);
        let tenant_context = resolved.tenant_context;
        let principal = resolved.request_principal;
        assert_eq!(tenant_context.org_id, "local");
        assert_eq!(tenant_context.workspace_id, "local");
        assert_eq!(tenant_context.actor_id, None);
        assert_eq!(principal.actor_id, None);
        assert_eq!(tenant_context.source, TenantSource::LocalImplicit);
    }

    #[test]
    fn resolve_enterprise_request_context_uses_request_source_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-tandem-request-source",
            HeaderValue::from_static("control_panel"),
        );
        let resolved = resolve_enterprise_request_context(&headers);
        let principal = resolved.request_principal;
        assert_eq!(principal.source, "control_panel");
    }

    #[test]
    fn local_mode_transport_token_remains_optional_when_unconfigured() {
        let headers = HeaderMap::new();

        assert!(request_transport_token_authorized(
            &headers,
            None,
            RuntimeAuthMode::LocalSingleTenant
        ));
    }

    #[test]
    fn local_mode_rejects_missing_transport_token_when_configured() {
        let headers = HeaderMap::new();

        assert!(!request_transport_token_authorized(
            &headers,
            Some("tk_local"),
            RuntimeAuthMode::LocalSingleTenant
        ));
    }

    #[test]
    fn hosted_mode_requires_configured_transport_token() {
        let headers = HeaderMap::new();

        assert!(!request_transport_token_authorized(
            &headers,
            None,
            RuntimeAuthMode::HostedSingleTenant
        ));
    }

    #[test]
    fn hosted_mode_rejects_wrong_transport_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer wrong-token"),
        );

        assert!(!request_transport_token_authorized(
            &headers,
            Some("tk_hosted"),
            RuntimeAuthMode::HostedSingleTenant
        ));
    }

    #[test]
    fn hosted_mode_accepts_matching_transport_token() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer tk_hosted"),
        );

        assert!(request_transport_token_authorized(
            &headers,
            Some("tk_hosted"),
            RuntimeAuthMode::HostedSingleTenant
        ));
    }

    #[test]
    fn hosted_mode_rejects_unsigned_tenant_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-tandem-org-id", HeaderValue::from_static("acme"));
        headers.insert("x-tandem-workspace-id", HeaderValue::from_static("north"));

        let err = resolve_enterprise_request_context_for_mode(
            &headers,
            RuntimeAuthMode::HostedSingleTenant,
        )
        .expect_err("hosted mode must not trust raw tenant headers");

        assert_eq!(err, TenantContextIngressError::UnsignedTenantHeaders);
    }

    #[test]
    fn hosted_mode_requires_verified_context_even_without_raw_headers() {
        let headers = HeaderMap::new();

        let err = resolve_enterprise_request_context_for_mode(
            &headers,
            RuntimeAuthMode::HostedSingleTenant,
        )
        .expect_err("hosted mode requires signed context");

        assert_eq!(err, TenantContextIngressError::MissingVerifiedContext);
    }

    #[test]
    fn hosted_mode_rejects_context_assertion_without_configured_key() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-tandem-context-jws",
            HeaderValue::from_static("placeholder.assertion.signature"),
        );

        let err = resolve_enterprise_request_context_for_mode(
            &headers,
            RuntimeAuthMode::HostedSingleTenant,
        )
        .expect_err("hosted mode must fail closed without verifier key config");

        assert_eq!(
            err,
            TenantContextIngressError::ContextAssertionKeyNotConfigured
        );
    }

    #[test]
    fn local_mode_ignores_unsigned_tenant_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-tandem-org-id", HeaderValue::from_static("acme"));
        headers.insert("x-tandem-workspace-id", HeaderValue::from_static("north"));
        headers.insert("x-user-id", HeaderValue::from_static("user-1"));

        let resolved = resolve_secure_local_enterprise_request_context(&headers);
        let tenant_context = resolved.tenant_context;
        let principal = resolved.request_principal;

        assert_eq!(tenant_context.org_id, "local");
        assert_eq!(tenant_context.workspace_id, "local");
        assert_eq!(principal.actor_id, None);
    }

    #[test]
    fn verifier_accepts_valid_tandem_context_assertion() {
        let (signing_key, verifier) = test_signing_key_and_verifier();
        let assertion =
            sign_test_context_assertion(&signing_key, "test-key", test_claims(1_000, 2_000));

        let verified = verifier
            .verify_at(&assertion, 1_500)
            .expect("signed assertion should verify");

        assert_eq!(verified.issuer, "tandem-web");
        assert_eq!(verified.audience, "tandem-runtime");
        assert_eq!(verified.human_actor.actor_id, "user-a");
        assert_eq!(verified.tenant_context.org_id, "org-a");
        assert_eq!(verified.tenant_context.workspace_id, "workspace-a");
        assert_eq!(
            verified.tenant_context.deployment_id.as_deref(),
            Some("dep-a")
        );
    }

    #[test]
    fn verifier_accepts_signed_context_assertion_with_strict_projection() {
        let (signing_key, verifier) = test_signing_key_and_verifier();
        let principal = PrincipalRef::agent_worker("agent-platform").with_tenant_actor_id("user-a");
        let repo = ResourceRef::new("org-a", "workspace-a", ResourceKind::Repository, "tandem")
            .with_project_id("platform")
            .with_path_prefix("crates/tandem-enterprise-contract/");
        let grant = ScopedGrant::new(
            "grant-platform-read",
            principal.clone(),
            repo.clone(),
            GrantSource::Delegation,
        )
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::SourceCode]);
        let claims = test_claims(1_000, 2_000).with_strict_projection(
            principal,
            ResourceScope {
                root: ResourceRef::new("org-a", "workspace-a", ResourceKind::Project, "platform"),
                allowed_resources: vec![repo],
                denied_resources: Vec::new(),
                max_depth: Some(4),
            },
            vec![grant],
            DataBoundary::allow(vec![DataClass::SourceCode]),
        );
        let assertion = sign_test_context_assertion(&signing_key, "test-key", claims);

        let verified = verifier
            .verify_at(&assertion, 1_500)
            .expect("signed scoped assertion should verify");

        assert_eq!(verified.issuer, "tandem-web");
        assert_eq!(verified.tenant_context.org_id, "org-a");
        assert_eq!(verified.human_actor.actor_id, "user-a");
    }

    #[test]
    fn signed_strict_context_projects_active_org_unit_membership_grants() {
        let principal = PrincipalRef::agent_worker("agent-platform").with_tenant_actor_id("user-a");
        let patient_cases = ResourceRef::new(
            "org-a",
            "workspace-a",
            ResourceKind::DataStore,
            "patient-cases",
        );
        let mut verified =
            VerifiedTenantContext::from(test_claims(1_000, 4_000).with_strict_projection(
                principal,
                ResourceScope::root(ResourceRef::new(
                    "org-a",
                    "workspace-a",
                    ResourceKind::Workspace,
                    "workspace-a",
                )),
                Vec::new(),
                DataBoundary::allow(vec![DataClass::Regulated, DataClass::CustomerData]),
            ));
        let tenant = verified.tenant_context.clone();
        let doctors = PrincipalRef::organization_unit("clinical_role/doctors");
        let membership = OrganizationUnitMembership::active(
            "membership-doctor-user",
            tenant.clone(),
            doctors.clone(),
            PrincipalRef::human_user("user-a"),
            tandem_types::OrganizationUnitMembershipSource::HostedControlPlane,
            1_000,
        );
        let disabled_membership = OrganizationUnitMembership {
            membership_id: "membership-disabled".to_string(),
            state: OrganizationUnitState::Disabled,
            member: PrincipalRef::human_user("user-b"),
            ..membership.clone()
        };
        let access_grant = OrganizationUnitAccessGrant::active(
            "grant-doctors-patient-cases",
            tenant,
            doctors,
            patient_cases.clone(),
            1_000,
        )
        .with_permissions(vec![AccessPermission::View, AccessPermission::Read])
        .with_data_classes(vec![DataClass::Regulated, DataClass::CustomerData]);

        project_org_unit_grants_into_verified_context(
            &mut verified,
            [&membership, &disabled_membership].into_iter(),
            [&access_grant].into_iter(),
            1_500,
        );

        let strict = verified
            .strict_projection
            .as_ref()
            .expect("strict projection remains present");
        assert_eq!(strict.grants.len(), 1);
        assert_eq!(
            strict.grants[0].grant_source,
            GrantSource::OrganizationUnitMembership
        );
        assert_eq!(
            strict.grants[0].source_principal.as_ref(),
            Some(&access_grant.unit)
        );
        assert!(
            strict
                .evaluate_access(
                    &patient_cases,
                    AccessPermission::Read,
                    DataClass::Regulated,
                    1_500,
                )
                .decision
                == tandem_types::AccessDecision::Allow
        );
    }

    #[test]
    fn org_unit_projection_does_not_create_strict_context_or_cross_tenants() {
        let mut verified = VerifiedTenantContext::from(test_claims(1_000, 4_000));
        let other_tenant = TenantContext::explicit_user_workspace(
            "other-org",
            "workspace-a",
            Some("dep-a".to_string()),
            "user-a",
        );
        let doctors = PrincipalRef::organization_unit("clinical_role/doctors");
        let membership = OrganizationUnitMembership::active(
            "membership-doctor-user",
            other_tenant.clone(),
            doctors.clone(),
            PrincipalRef::human_user("user-a"),
            tandem_types::OrganizationUnitMembershipSource::HostedControlPlane,
            1_000,
        );
        let access_grant = OrganizationUnitAccessGrant::active(
            "grant-doctors-patient-cases",
            other_tenant,
            doctors,
            ResourceRef::new(
                "other-org",
                "workspace-a",
                ResourceKind::DataStore,
                "patient-cases",
            ),
            1_000,
        )
        .with_permissions(vec![AccessPermission::Read])
        .with_data_classes(vec![DataClass::Regulated]);

        project_org_unit_grants_into_verified_context(
            &mut verified,
            [&membership].into_iter(),
            [&access_grant].into_iter(),
            1_500,
        );

        assert!(verified.strict_projection.is_none());
    }

    #[test]
    fn hosted_mode_resolves_verified_context_as_tandem_web_principal() {
        let (signing_key, verifier) = test_signing_key_and_verifier();
        let assertion =
            sign_test_context_assertion(&signing_key, "test-key", test_claims(1_000, 2_000));
        let verified = verifier
            .verify_at(&assertion, 1_500)
            .expect("signed assertion should verify");

        let resolved = ResolvedEnterpriseRequestContext::verified(verified);

        assert_eq!(
            resolved.request_principal.actor_id.as_deref(),
            Some("user-a")
        );
        assert_eq!(resolved.request_principal.source, "tandem-web");
        assert_eq!(resolved.tenant_context.org_id, "org-a");
        assert_eq!(resolved.tenant_context.source, TenantSource::Explicit);
    }

    #[test]
    fn verifier_rejects_tampered_tandem_context_assertion() {
        let (signing_key, verifier) = test_signing_key_and_verifier();
        let assertion =
            sign_test_context_assertion(&signing_key, "test-key", test_claims(1_000, 2_000));
        let parts = assertion.split('.').collect::<Vec<_>>();
        let encoded_claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&test_claims(1_100, 2_100)).expect("claims json"));
        let assertion = format!("{}.{}.{}", parts[0], encoded_claims, parts[2]);

        let err = verifier
            .verify_at(&assertion, 1_500)
            .expect_err("tampered assertion must not verify");

        assert_eq!(err, TenantContextIngressError::ContextAssertionUntrusted);
    }

    #[test]
    fn verifier_rejects_expired_tandem_context_assertion() {
        let (signing_key, verifier) = test_signing_key_and_verifier();
        let assertion =
            sign_test_context_assertion(&signing_key, "test-key", test_claims(1_000, 2_000));

        let err = verifier
            .verify_at(&assertion, 2_000)
            .expect_err("expired assertion must fail closed");

        assert_eq!(err, TenantContextIngressError::ContextAssertionExpired);
    }

    #[test]
    fn verifier_rejects_local_implicit_tenant_context_assertion() {
        let (signing_key, verifier) = test_signing_key_and_verifier();
        let mut claims = test_claims(1_000, 2_000);
        claims.tenant_context = TenantContext::local_implicit();
        let assertion = sign_test_context_assertion(&signing_key, "test-key", claims);

        let err = verifier
            .verify_at(&assertion, 1_500)
            .expect_err("hosted assertions must carry explicit deployment tenant context");

        assert_eq!(err, TenantContextIngressError::ContextAssertionMalformed);
    }

    #[test]
    fn verifier_rejects_context_assertion_without_deployment_scope() {
        let (signing_key, verifier) = test_signing_key_and_verifier();
        let mut claims = test_claims(1_000, 2_000);
        claims.tenant_context.deployment_id = None;
        let assertion = sign_test_context_assertion(&signing_key, "test-key", claims);

        let err = verifier
            .verify_at(&assertion, 1_500)
            .expect_err("hosted assertions must bind to a deployment audience");

        assert_eq!(err, TenantContextIngressError::ContextAssertionMalformed);
    }

    #[test]
    fn verifier_rejects_context_assertion_with_mismatched_authority_actor() {
        let (signing_key, verifier) = test_signing_key_and_verifier();
        let mut claims = test_claims(1_000, 2_000);
        claims.authority_chain = AuthorityChain::from_request(
            RequestPrincipal::authenticated_user("user-b", "tandem-web"),
        );
        let assertion = sign_test_context_assertion(&signing_key, "test-key", claims);

        let err = verifier
            .verify_at(&assertion, 1_500)
            .expect_err("hosted assertions must bind authority to the human actor");

        assert_eq!(err, TenantContextIngressError::ContextAssertionUntrusted);
    }

    #[test]
    fn verifier_selects_context_assertion_key_by_kid() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
        let other_key = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
        let verifier = TenantContextAssertionVerifier {
            public_keys_by_id: BTreeMap::from([
                (
                    "old-key".to_string(),
                    ContextAssertionPublicKey::legacy(other_key.verifying_key().to_bytes()),
                ),
                (
                    "active-key".to_string(),
                    ContextAssertionPublicKey::legacy(signing_key.verifying_key().to_bytes()),
                ),
            ]),
            legacy_public_key: None,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            max_future_skew_ms: 60_000,
        };
        let assertion =
            sign_test_context_assertion(&signing_key, "active-key", test_claims(1_000, 2_000));

        let verified = verifier
            .verify_at(&assertion, 1_500)
            .expect("kid-selected key should verify");

        assert_eq!(verified.assertion_id, "assertion-a");
    }

    #[test]
    fn verifier_rejects_unknown_context_assertion_kid_when_keyring_is_configured() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
        let other_key = ed25519_dalek::SigningKey::from_bytes(&[9u8; 32]);
        let verifier = TenantContextAssertionVerifier {
            public_keys_by_id: BTreeMap::from([(
                "old-key".to_string(),
                ContextAssertionPublicKey::legacy(other_key.verifying_key().to_bytes()),
            )]),
            legacy_public_key: None,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            max_future_skew_ms: 60_000,
        };
        let assertion =
            sign_test_context_assertion(&signing_key, "active-key", test_claims(1_000, 2_000));

        let err = verifier
            .verify_at(&assertion, 1_500)
            .expect_err("unknown kid should not use the wrong key");

        assert_eq!(err, TenantContextIngressError::ContextAssertionUntrusted);
    }

    #[test]
    fn parse_context_public_keyring_accepts_json_and_delimited_forms() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
        let encoded =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signing_key.verifying_key());
        let json_keyring = format!(r#"{{"active-key":"{encoded}"}}"#);
        let delimited_keyring = format!("active-key={encoded};next-key={encoded}");

        assert_eq!(
            parse_context_public_keyring(&json_keyring)
                .expect("json keyring")
                .get("active-key")
                .map(|key| key.public_key),
            Some(signing_key.verifying_key().to_bytes())
        );
        assert_eq!(
            parse_context_public_keyring(&delimited_keyring)
                .expect("delimited keyring")
                .len(),
            2
        );
    }

    #[test]
    fn parse_context_public_keyring_accepts_metadata_objects() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
        let encoded =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signing_key.verifying_key());
        let json_keyring = format!(
            r#"{{
                "active-key": {{
                    "publicKey": "{encoded}",
                    "purpose": "context_assertion",
                    "organizationId": "org-a",
                    "deploymentId": "dep-a",
                    "allowedAudiences": ["tandem-runtime"],
                    "allowedResourceScopePrefixes": ["org/org-a/workspace/workspace-a/project/platform"],
                    "notBeforeMs": 1,
                    "notAfterMs": 5000,
                    "status": "active"
                }}
            }}"#
        );

        let keyring = parse_context_public_keyring(&json_keyring).expect("metadata keyring");
        let key = keyring.get("active-key").expect("active key metadata");

        assert_eq!(key.public_key, signing_key.verifying_key().to_bytes());
        assert_eq!(key.purpose, Some(SigningKeyPurpose::ContextAssertion));
        assert_eq!(key.organization_id.as_deref(), Some("org-a"));
        assert_eq!(key.deployment_id.as_deref(), Some("dep-a"));
        assert_eq!(key.allowed_audiences, vec!["tandem-runtime"]);
        assert_eq!(
            key.allowed_resource_scope_prefixes,
            vec!["org/org-a/workspace/workspace-a/project/platform"]
        );
        assert_eq!(key.not_before_ms, Some(1));
        assert_eq!(key.not_after_ms, Some(5000));
        assert_eq!(key.status.as_deref(), Some("active"));
    }

    #[test]
    fn verifier_enforces_context_assertion_key_metadata() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
        let verifier = TenantContextAssertionVerifier {
            public_keys_by_id: BTreeMap::from([(
                "active-key".to_string(),
                ContextAssertionPublicKey {
                    public_key: signing_key.verifying_key().to_bytes(),
                    purpose: Some(SigningKeyPurpose::ContextAssertion),
                    organization_id: Some("org-a".to_string()),
                    deployment_id: Some("dep-a".to_string()),
                    allowed_audiences: vec!["tandem-runtime".to_string()],
                    allowed_resource_scope_prefixes: vec![
                        "org/org-a/workspace/workspace-a/project/platform".to_string(),
                    ],
                    not_before_ms: Some(1_000),
                    not_after_ms: Some(2_000),
                    status: Some("active".to_string()),
                },
            )]),
            legacy_public_key: None,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            max_future_skew_ms: 60_000,
        };
        let claims = test_claims(1_000, 2_000).with_strict_projection(
            PrincipalRef::agent_worker("agent-platform").with_tenant_actor_id("user-a"),
            ResourceScope::root(ResourceRef::new(
                "org-a",
                "workspace-a",
                ResourceKind::Project,
                "platform",
            )),
            Vec::new(),
            DataBoundary::allow(vec![DataClass::SourceCode]),
        );
        let assertion = sign_test_context_assertion(&signing_key, "active-key", claims);

        let verified = verifier
            .verify_at(&assertion, 1_500)
            .expect("key metadata should match assertion scope");

        assert_eq!(verified.tenant_context.org_id, "org-a");
    }

    #[test]
    fn verifier_rejects_context_assertion_key_with_wrong_purpose() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
        let verifier = TenantContextAssertionVerifier {
            public_keys_by_id: BTreeMap::from([(
                "approval-key".to_string(),
                ContextAssertionPublicKey {
                    public_key: signing_key.verifying_key().to_bytes(),
                    purpose: Some(SigningKeyPurpose::ApprovalReceipt),
                    ..ContextAssertionPublicKey::legacy(signing_key.verifying_key().to_bytes())
                },
            )]),
            legacy_public_key: None,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            max_future_skew_ms: 60_000,
        };
        let assertion =
            sign_test_context_assertion(&signing_key, "approval-key", test_claims(1_000, 2_000));

        let err = verifier
            .verify_at(&assertion, 1_500)
            .expect_err("approval keys must not verify context assertions");

        assert_eq!(err, TenantContextIngressError::ContextAssertionUntrusted);
    }

    #[test]
    fn verifier_rejects_context_assertion_outside_key_scope() {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[8u8; 32]);
        let verifier = TenantContextAssertionVerifier {
            public_keys_by_id: BTreeMap::from([(
                "active-key".to_string(),
                ContextAssertionPublicKey {
                    public_key: signing_key.verifying_key().to_bytes(),
                    purpose: Some(SigningKeyPurpose::ContextAssertion),
                    allowed_resource_scope_prefixes: vec![
                        "org/org-a/workspace/workspace-a/project/finance".to_string(),
                    ],
                    ..ContextAssertionPublicKey::legacy(signing_key.verifying_key().to_bytes())
                },
            )]),
            legacy_public_key: None,
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            max_future_skew_ms: 60_000,
        };
        let claims = test_claims(1_000, 2_000).with_strict_projection(
            PrincipalRef::agent_worker("agent-platform").with_tenant_actor_id("user-a"),
            ResourceScope::root(ResourceRef::new(
                "org-a",
                "workspace-a",
                ResourceKind::Project,
                "platform",
            )),
            Vec::new(),
            DataBoundary::allow(vec![DataClass::SourceCode]),
        );
        let assertion = sign_test_context_assertion(&signing_key, "active-key", claims);

        let err = verifier
            .verify_at(&assertion, 1_500)
            .expect_err("key scope must constrain signed projection scope");

        assert_eq!(err, TenantContextIngressError::ContextAssertionUntrusted);
    }

    #[test]
    fn replay_guard_bound_mode_allows_identical_assertion_reuse() {
        let guard = AssertionReplayGuard::new();
        let fingerprint = assertion_fingerprint("assertion-bytes");

        for _ in 0..3 {
            guard
                .check_and_record(
                    AssertionReplayMode::Bound,
                    "assertion-a",
                    fingerprint,
                    2_000,
                    1_500,
                )
                .expect("identical assertion reuse is allowed in bound mode");
        }
    }

    #[test]
    fn replay_guard_bound_mode_rejects_same_id_with_different_bytes() {
        let guard = AssertionReplayGuard::new();
        guard
            .check_and_record(
                AssertionReplayMode::Bound,
                "assertion-a",
                assertion_fingerprint("original-bytes"),
                2_000,
                1_500,
            )
            .expect("first use binds the assertion id");

        let err = guard
            .check_and_record(
                AssertionReplayMode::Bound,
                "assertion-a",
                assertion_fingerprint("different-bytes"),
                2_500,
                1_600,
            )
            .expect_err("same assertion id with different bytes is a replay/substitution");

        assert_eq!(err, TenantContextIngressError::ContextAssertionReplayed);
    }

    #[test]
    fn replay_guard_one_shot_mode_rejects_second_use() {
        let guard = AssertionReplayGuard::new();
        let fingerprint = assertion_fingerprint("assertion-bytes");
        guard
            .check_and_record(
                AssertionReplayMode::OneShot,
                "assertion-a",
                fingerprint,
                2_000,
                1_500,
            )
            .expect("first use is accepted");

        let err = guard
            .check_and_record(
                AssertionReplayMode::OneShot,
                "assertion-a",
                fingerprint,
                2_000,
                1_600,
            )
            .expect_err("one-shot mode accepts an assertion id exactly once");

        assert_eq!(err, TenantContextIngressError::ContextAssertionReplayed);
    }

    #[test]
    fn replay_guard_releases_expired_assertion_ids() {
        let guard = AssertionReplayGuard::new();
        guard
            .check_and_record(
                AssertionReplayMode::OneShot,
                "assertion-a",
                assertion_fingerprint("old-bytes"),
                2_000,
                1_500,
            )
            .expect("first use is accepted");

        let past_retention = 2_000 + ASSERTION_REPLAY_RETENTION_GRACE_MS + 1;
        guard
            .check_and_record(
                AssertionReplayMode::OneShot,
                "assertion-a",
                assertion_fingerprint("new-bytes"),
                past_retention + 1_000,
                past_retention,
            )
            .expect("expired entries no longer block the assertion id");
    }

    #[test]
    fn replay_guard_sweeps_expired_entries_to_bound_memory() {
        let guard = AssertionReplayGuard::new();
        let fingerprint = assertion_fingerprint("assertion-bytes");
        for index in 0..ASSERTION_REPLAY_SWEEP_THRESHOLD {
            guard
                .check_and_record(
                    AssertionReplayMode::Bound,
                    &format!("assertion-{index}"),
                    fingerprint,
                    2_000,
                    1_500,
                )
                .expect("inserts succeed");
        }
        assert_eq!(guard.len(), ASSERTION_REPLAY_SWEEP_THRESHOLD);

        let past_retention = 2_000 + ASSERTION_REPLAY_RETENTION_GRACE_MS + 1;
        guard
            .check_and_record(
                AssertionReplayMode::Bound,
                "assertion-fresh",
                fingerprint,
                past_retention + 10_000,
                past_retention,
            )
            .expect("insert after sweep succeeds");

        assert_eq!(guard.len(), 1, "expired entries are swept at threshold");
    }

    #[test]
    fn replay_mode_defaults_to_bound() {
        assert_eq!(resolve_assertion_replay_mode(), AssertionReplayMode::Bound);
    }

    fn test_signing_key_and_verifier() -> (ed25519_dalek::SigningKey, TenantContextAssertionVerifier)
    {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let verifier = TenantContextAssertionVerifier {
            public_keys_by_id: BTreeMap::new(),
            legacy_public_key: Some(ContextAssertionPublicKey::legacy(
                signing_key.verifying_key().to_bytes(),
            )),
            issuer: "tandem-web".to_string(),
            audience: "tandem-runtime".to_string(),
            max_future_skew_ms: 60_000,
        };
        (signing_key, verifier)
    }

    fn test_claims(issued_at_ms: u64, expires_at_ms: u64) -> TenantContextAssertionClaims {
        let tenant_context = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            Some("dep-a".to_string()),
            "user-a",
        );
        let principal = RequestPrincipal::authenticated_user("user-a", "tandem-web");
        TenantContextAssertionClaims::new_v1(
            "tandem-web",
            "tandem-runtime",
            issued_at_ms,
            expires_at_ms,
            "assertion-a",
            tenant_context,
            HumanActor::tandem_user("user-a"),
            AuthorityChain::from_request(principal),
            vec!["workspace:admin".to_string()],
        )
    }

    fn sign_test_context_assertion(
        signing_key: &ed25519_dalek::SigningKey,
        kid: &str,
        claims: TenantContextAssertionClaims,
    ) -> String {
        use ed25519_dalek::Signer;

        let header = TenantContextAssertionHeader::ed25519(kid);
        let encoded_header = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&header).expect("header json"));
        let encoded_claims = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).expect("claims json"));
        let signing_input = format!("{encoded_header}.{encoded_claims}");
        let signature = signing_key.sign(signing_input.as_bytes());
        let encoded_signature =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(signature.to_bytes());
        format!("{signing_input}.{encoded_signature}")
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
