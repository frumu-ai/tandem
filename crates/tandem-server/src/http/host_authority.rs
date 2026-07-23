// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::extract::Request;
use axum::http::{HeaderMap, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use std::net::{IpAddr, SocketAddr};
use tandem_types::{TenantContext, VerifiedTenantContext};
use url::Url;

use crate::action_authorization::{
    authorize_host_effect, AuthorizedHostEffect, CanonicalHostResource, HostAction,
    HostAuthorizationError, HostEffectRequest,
};
use crate::AppState;

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct RequestLocality {
    direct_loopback: bool,
}

impl RequestLocality {
    pub(super) fn from_peer_and_headers(peer: Option<SocketAddr>, headers: &HeaderMap) -> Self {
        Self {
            direct_loopback: peer.is_some_and(|peer| peer.ip().is_loopback())
                && !has_proxy_forwarding_headers(headers),
        }
    }

    pub(super) fn is_direct_loopback(self) -> bool {
        self.direct_loopback
    }
}

pub(super) async fn require_direct_loopback_request(request: Request, next: Next) -> Response {
    if request
        .extensions()
        .get::<RequestLocality>()
        .is_some_and(|locality| locality.is_direct_loopback())
    {
        next.run(request).await
    } else {
        StatusCode::FORBIDDEN.into_response()
    }
}

pub(super) fn require_loopback_local_operator(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
) -> Result<(), StatusCode> {
    if is_loopback_local_operator(
        state.host_operations_loopback_only(),
        &state.server_base_url(),
        tenant,
        verified.is_some(),
    ) {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

pub(crate) fn standalone_local_runtime_posture(state: &AppState, tenant: &TenantContext) -> bool {
    let runtime_allows_local_implicit = state
        .runtime
        .get()
        .is_some_and(|runtime| !runtime.mcp.strict_tenant_enforcement_enabled());
    if !state.http_listener_bound_loopback_only() || !runtime_allows_local_implicit {
        return false;
    }
    is_loopback_local_operator(
        state.host_operations_loopback_only(),
        &state.server_base_url(),
        tenant,
        false,
    )
}

fn is_loopback_local_operator(
    listener_is_loopback: bool,
    server_base_url: &str,
    tenant: &TenantContext,
    has_verified_context: bool,
) -> bool {
    listener_is_loopback
        && !has_verified_context
        && tenant.is_local_implicit()
        && server_base_url_is_loopback(server_base_url)
}

pub(super) fn require_diagnostics_admin(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    locality: RequestLocality,
) -> Result<(), StatusCode> {
    if (locality.is_direct_loopback()
        && require_loopback_local_operator(state, tenant, verified).is_ok())
        || verified.is_some_and(verified_has_deployment_admin_authority)
    {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
}

pub(super) fn host_authorization_status(error: HostAuthorizationError) -> StatusCode {
    match error {
        HostAuthorizationError::AuditPersistenceFailed => StatusCode::INTERNAL_SERVER_ERROR,
        HostAuthorizationError::InvalidEffectArguments => StatusCode::BAD_REQUEST,
        _ => StatusCode::FORBIDDEN,
    }
}

/// Issue an exact-request, tenant-bound grant for an administrative state
/// change. Shared deployment actions declare that policy on `HostAction`;
/// tenant-scoped actions can instead require their exact capability.
pub(super) async fn authorize_administrative_effect(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&VerifiedTenantContext>,
    locality: RequestLocality,
    action: HostAction,
    resource_kind: &'static str,
    resource_id: impl Into<String>,
    arguments: serde_json::Value,
) -> Result<(AuthorizedHostEffect, HostEffectRequest), StatusCode> {
    let effect = HostEffectRequest::new(
        action,
        CanonicalHostResource::new(resource_kind, resource_id, tenant.clone()),
        arguments,
    );
    let grant = authorize_host_effect(
        state,
        tenant,
        verified,
        locality.is_direct_loopback(),
        &effect,
    )
    .await
    .map_err(host_authorization_status)?;
    Ok((grant, effect))
}

fn verified_has_deployment_admin_authority(context: &VerifiedTenantContext) -> bool {
    context.roles.iter().any(|role| {
        matches!(
            role.as_str(),
            "owner"
                | "admin"
                | "hosted:owner"
                | "hosted:admin"
                | "enterprise:admin"
                | "workspace:admin"
                | "organization:admin"
        )
    }) || context.capabilities.iter().any(|capability| {
        matches!(
            capability.as_str(),
            "hosted.owner" | "hosted.admin" | "deployment.admin" | "diagnostics.read"
        )
    })
}

fn has_proxy_forwarding_headers(headers: &HeaderMap) -> bool {
    [
        "forwarded",
        "x-forwarded-for",
        "x-forwarded-host",
        "x-forwarded-proto",
        "x-real-ip",
        "cf-connecting-ip",
        "true-client-ip",
    ]
    .iter()
    .any(|name| headers.contains_key(*name))
}

pub(crate) fn server_base_url_is_loopback(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

#[cfg(test)]
mod tests {
    use super::{is_loopback_local_operator, server_base_url_is_loopback, RequestLocality};
    use axum::http::HeaderMap;
    use std::net::SocketAddr;
    use tandem_types::TenantContext;

    #[test]
    fn loopback_base_url_check_fails_closed() {
        assert!(server_base_url_is_loopback("http://127.0.0.1:39731"));
        assert!(server_base_url_is_loopback("http://[::1]:39731"));
        assert!(server_base_url_is_loopback("http://localhost:39731"));
        assert!(!server_base_url_is_loopback("http://0.0.0.0:39731"));
        assert!(!server_base_url_is_loopback("https://engine.example.test"));
        assert!(!server_base_url_is_loopback("not a url"));
    }

    #[test]
    fn host_operator_is_only_unverified_loopback_local_context() {
        let local = TenantContext::local_implicit();
        assert!(is_loopback_local_operator(
            true,
            "http://127.0.0.1:39731",
            &local,
            false
        ));
        assert!(!is_loopback_local_operator(
            false,
            "http://127.0.0.1:39731",
            &local,
            false
        ));
        assert!(!is_loopback_local_operator(
            true,
            "http://0.0.0.0:39731",
            &local,
            false
        ));
        assert!(!is_loopback_local_operator(
            true,
            "http://127.0.0.1:39731",
            &local,
            true
        ));
        let hosted = TenantContext::explicit("org", "workspace", Some("actor".to_string()));
        assert!(!is_loopback_local_operator(
            true,
            "http://127.0.0.1:39731",
            &hosted,
            false
        ));
    }
    #[test]
    fn direct_loopback_locality_fails_closed_for_missing_remote_or_proxied_peers() {
        let headers = HeaderMap::new();
        let loopback: SocketAddr = "127.0.0.1:43123".parse().expect("loopback peer");
        let remote: SocketAddr = "192.0.2.10:43123".parse().expect("remote peer");
        assert!(
            RequestLocality::from_peer_and_headers(Some(loopback), &headers).is_direct_loopback()
        );
        assert!(
            !RequestLocality::from_peer_and_headers(Some(remote), &headers).is_direct_loopback()
        );
        assert!(!RequestLocality::from_peer_and_headers(None, &headers).is_direct_loopback());

        let mut forwarded = HeaderMap::new();
        forwarded.insert("x-forwarded-for", "198.51.100.9".parse().expect("header"));
        assert!(
            !RequestLocality::from_peer_and_headers(Some(loopback), &forwarded)
                .is_direct_loopback()
        );
    }
}
