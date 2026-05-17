use axum::extract::{Request, State};
use axum::http::header;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use base64::Engine;
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use tandem_types::{
    HeaderTenantContextResolver, NoopRequestAuthorizationHook, RequestAuthorizationHook,
    RequestPrincipal, RuntimeAuthMode, TenantContext, TenantContextAssertionClaims,
    TenantContextAssertionHeader, TenantContextResolver, VerifiedTenantContext,
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
    if path == "/bug-monitor/intake/report" || path == "/failure-reporter/intake/report" {
        if !attach_enterprise_request_context(&mut request) {
            return (
                StatusCode::FORBIDDEN,
                Json(ErrorEnvelope {
                    error: "Unauthorized: tenant context denied".to_string(),
                    code: Some("TENANT_CONTEXT_DENIED".to_string()),
                }),
            )
                .into_response();
        }
        return next.run(request).await;
    }

    let required = state.api_token().await;
    if let Some(expected) = required {
        let provided = extract_request_token(request.headers());
        if provided.as_deref() != Some(expected.as_str()) {
            return (
                StatusCode::UNAUTHORIZED,
                Json(ErrorEnvelope {
                    error: "Unauthorized: missing or invalid API token".to_string(),
                    code: Some("AUTH_REQUIRED".to_string()),
                }),
            )
                .into_response();
        }
    }

    if !attach_enterprise_request_context(&mut request) {
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

fn attach_enterprise_request_context(request: &mut Request) -> bool {
    let headers = request.headers();
    let resolved =
        match resolve_enterprise_request_context_for_mode(headers, resolve_runtime_auth_mode()) {
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
            "tandem_context_assertion",
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
    public_key: [u8; 32],
    issuer: String,
    audience: String,
    max_future_skew_ms: u64,
}

impl TenantContextAssertionVerifier {
    fn from_env() -> Result<Self, TenantContextIngressError> {
        let raw_key = std::env::var("TANDEM_CONTEXT_ASSERTION_PUBLIC_KEY")
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
            .ok_or(TenantContextIngressError::ContextAssertionKeyNotConfigured)?;
        let public_key = decode_context_public_key(&raw_key)
            .ok_or(TenantContextIngressError::ContextAssertionKeyNotConfigured)?;
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
            public_key,
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

        let verifying_key = VerifyingKey::from_bytes(&self.public_key)
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
        if claims.tenant_context.actor_id.as_deref() != Some(claims.human_actor.actor_id.as_str()) {
            return Err(TenantContextIngressError::ContextAssertionUntrusted);
        }
        Ok(())
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
        let assertion = sign_test_context_assertion(&signing_key, test_claims(1_000, 2_000));

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
    fn verifier_rejects_tampered_tandem_context_assertion() {
        let (signing_key, verifier) = test_signing_key_and_verifier();
        let assertion = sign_test_context_assertion(&signing_key, test_claims(1_000, 2_000));
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
        let assertion = sign_test_context_assertion(&signing_key, test_claims(1_000, 2_000));

        let err = verifier
            .verify_at(&assertion, 2_000)
            .expect_err("expired assertion must fail closed");

        assert_eq!(err, TenantContextIngressError::ContextAssertionExpired);
    }

    fn test_signing_key_and_verifier() -> (ed25519_dalek::SigningKey, TenantContextAssertionVerifier)
    {
        let signing_key = ed25519_dalek::SigningKey::from_bytes(&[7u8; 32]);
        let verifier = TenantContextAssertionVerifier {
            public_key: signing_key.verifying_key().to_bytes(),
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
        claims: TenantContextAssertionClaims,
    ) -> String {
        use ed25519_dalek::Signer;

        let header = TenantContextAssertionHeader::ed25519("test-key");
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
