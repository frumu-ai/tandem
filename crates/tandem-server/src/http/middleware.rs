use axum::extract::{Request, State};
use axum::http::header;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;

use tandem_types::{
    HeaderTenantContextResolver, NoopRequestAuthorizationHook, RequestAuthorizationHook,
    RequestPrincipal, TenantContext, TenantContextResolver,
};

use crate::{AppState, StartupStatus};

use super::ErrorEnvelope;

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
    let (tenant_context, request_principal) = resolve_enterprise_request_context(headers);

    if !authorize_request(&request_principal, &tenant_context) {
        tracing::warn!(
            "Authorization denied: principal={:?} tenant={} source={}",
            request_principal.actor_id,
            tenant_context.org_id,
            request_principal.source
        );
        return false;
    }

    request.extensions_mut().insert(tenant_context);
    request.extensions_mut().insert(request_principal);
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

fn resolve_enterprise_request_context(headers: &HeaderMap) -> (TenantContext, RequestPrincipal) {
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
    (tenant_context, request_principal)
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

    #[test]
    fn resolve_enterprise_request_context_defaults_to_local_tenant() {
        let headers = HeaderMap::new();
        let (tenant_context, principal) = resolve_enterprise_request_context(&headers);
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
        let (tenant_context, principal) = resolve_enterprise_request_context(&headers);
        assert_eq!(tenant_context.org_id, "acme");
        assert_eq!(tenant_context.workspace_id, "north");
        assert_eq!(tenant_context.actor_id.as_deref(), Some("user-1"));
        assert_eq!(principal.actor_id.as_deref(), Some("user-1"));
        assert_eq!(tenant_context.source, tandem_types::TenantSource::Explicit);
    }

    #[test]
    fn resolve_enterprise_request_context_uses_request_source_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-tandem-request-source",
            HeaderValue::from_static("control_panel"),
        );
        let (_, principal) = resolve_enterprise_request_context(&headers);
        assert_eq!(principal.source, "control_panel");
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
