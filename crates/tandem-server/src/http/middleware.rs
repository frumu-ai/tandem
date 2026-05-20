use axum::extract::{Request, State};
use axum::http::header;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use std::collections::BTreeMap;
use tandem_types::{
    AccessPermission, DataBoundary, DataClass, GrantSource, HeaderTenantContextResolver,
    NoopRequestAuthorizationHook, PrincipalRef, RequestAuthorizationHook, RequestPrincipal,
    ResourceKind, ResourceRef, ResourceScope, RuntimeAuthMode, ScopedGrant, TenantContext,
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
            && !attach_enterprise_request_context_for_mode(&mut request, runtime_auth_mode)
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

    if !attach_enterprise_request_context_for_mode(&mut request, runtime_auth_mode) {
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

fn attach_enterprise_request_context_for_mode(
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

    if let Some(verified_tenant_context) = resolved.verified_tenant_context {
        request.extensions_mut().insert(verified_tenant_context);
    }
    request.extensions_mut().insert(resolved.tenant_context);
    request.extensions_mut().insert(resolved.request_principal);
    true
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

    extract_request_token(headers).as_deref() == Some(expected)
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
            Ok(ResolvedEnterpriseRequestContext::verified(
                verified_tenant_context,
            ))
        }
    }
}

fn resolve_local_enterprise_request_context(
    headers: &HeaderMap,
) -> ResolvedEnterpriseRequestContext {
    let resolver = HeaderTenantContextResolver;
    let tenant_context = resolver.resolve_tenant_context(
        first_header(headers, &["x-tandem-org-id", "x-tenant-org-id"]).as_deref(),
        first_header(headers, &["x-tandem-workspace-id", "x-tenant-workspace-id"]).as_deref(),
        first_header(headers, &["x-tandem-actor-id", "x-user-id"]).as_deref(),
    );
    let request_source = first_header(headers, &["x-tandem-request-source"])
        .unwrap_or_else(|| "api_token".to_string());
    let request_principal = RequestPrincipal {
        actor_id: tenant_context.actor_id.clone(),
        source: request_source,
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
    public_keys_by_id: BTreeMap<String, [u8; 32]>,
    legacy_public_key: Option<[u8; 32]>,
    issuer: String,
    audience: String,
    max_future_skew_ms: u64,
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

        let public_key = self
            .public_key_for_kid(&header.kid)
            .ok_or(TenantContextIngressError::ContextAssertionUntrusted)?;
        let verifying_key = VerifyingKey::from_bytes(public_key)
            .map_err(|_| TenantContextIngressError::ContextAssertionKeyNotConfigured)?;
        let signature = Signature::from_bytes(&signature_bytes);
        let signing_input = format!("{encoded_header}.{encoded_claims}");
        verifying_key
            .verify(signing_input.as_bytes(), &signature)
            .map_err(|_| TenantContextIngressError::ContextAssertionUntrusted)?;

        let claims: TenantContextAssertionClaims = serde_json::from_slice(&claims_bytes)
            .map_err(|_| TenantContextIngressError::ContextAssertionMalformed)?;
        self.validate_claims(&claims, now_ms)?;
        Ok(claims.into())
    }

    fn public_key_for_kid(&self, kid: &str) -> Option<&[u8; 32]> {
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

fn read_context_public_keyring_from_env(
) -> Result<BTreeMap<String, [u8; 32]>, TenantContextIngressError> {
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

fn read_legacy_context_public_key_from_env() -> Result<Option<[u8; 32]>, TenantContextIngressError>
{
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
        .map(Some)
        .ok_or(TenantContextIngressError::ContextAssertionKeyNotConfigured)
}

fn parse_context_public_keyring(raw: &str) -> Option<BTreeMap<String, [u8; 32]>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Some(BTreeMap::new());
    }
    if trimmed.starts_with('{') {
        let parsed = serde_json::from_str::<BTreeMap<String, String>>(trimmed).ok()?;
        return parse_context_public_keyring_entries(parsed);
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
) -> Option<BTreeMap<String, [u8; 32]>> {
    let mut decoded = BTreeMap::new();
    for (kid, raw_key) in entries {
        let kid = kid.trim();
        if kid.is_empty() {
            return None;
        }
        decoded.insert(kid.to_string(), decode_context_public_key(&raw_key)?);
    }
    Some(decoded)
}

fn validate_context_assertion_header(
    header: &TenantContextAssertionHeader,
) -> Result<(), TenantContextIngressError> {
    if header.alg != "EdDSA" || header.typ != "tandem-tenant-context+jws" || header.kid.is_empty() {
        return Err(TenantContextIngressError::ContextAssertionMalformed);
    }
    Ok(())
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
    use tandem_types::{AuthorityChain, HumanActor, TenantSource};

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
        assert_eq!(principal.source, "api_token");
    }

    #[test]
    fn resolve_enterprise_request_context_uses_tenant_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-tandem-org-id", HeaderValue::from_static("acme"));
        headers.insert("x-tandem-workspace-id", HeaderValue::from_static("north"));
        headers.insert("x-user-id", HeaderValue::from_static("user-1"));
        let resolved = resolve_enterprise_request_context(&headers);
        let tenant_context = resolved.tenant_context;
        let principal = resolved.request_principal;
        assert_eq!(tenant_context.org_id, "acme");
        assert_eq!(tenant_context.workspace_id, "north");
        assert_eq!(tenant_context.actor_id.as_deref(), Some("user-1"));
        assert_eq!(principal.actor_id.as_deref(), Some("user-1"));
        assert_eq!(tenant_context.source, TenantSource::Explicit);
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
    fn local_mode_continues_to_accept_tenant_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("x-tandem-org-id", HeaderValue::from_static("acme"));
        headers.insert("x-tandem-workspace-id", HeaderValue::from_static("north"));
        headers.insert("x-user-id", HeaderValue::from_static("user-1"));

        let resolved = resolve_enterprise_request_context_for_mode(
            &headers,
            RuntimeAuthMode::LocalSingleTenant,
        )
        .expect("local mode keeps legacy header behavior");
        let tenant_context = resolved.tenant_context;
        let principal = resolved.request_principal;

        assert_eq!(tenant_context.org_id, "acme");
        assert_eq!(tenant_context.workspace_id, "north");
        assert_eq!(principal.actor_id.as_deref(), Some("user-1"));
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
                ("old-key".to_string(), other_key.verifying_key().to_bytes()),
                (
                    "active-key".to_string(),
                    signing_key.verifying_key().to_bytes(),
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
                other_key.verifying_key().to_bytes(),
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
                .get("active-key"),
            Some(&signing_key.verifying_key().to_bytes())
        );
        assert_eq!(
            parse_context_public_keyring(&delimited_keyring)
                .expect("delimited keyring")
                .len(),
            2
        );
    }

    fn test_signing_key_and_verifier() -> (ed25519_dalek::SigningKey, TenantContextAssertionVerifier)
    {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let verifier = TenantContextAssertionVerifier {
            public_keys_by_id: BTreeMap::new(),
            legacy_public_key: Some(signing_key.verifying_key().to_bytes()),
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
