// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use axum::http::StatusCode;
use std::net::IpAddr;
use tandem_types::{TenantContext, VerifiedTenantContext};
use url::Url;

use crate::AppState;

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
) -> Result<(), StatusCode> {
    if require_loopback_local_operator(state, tenant, verified).is_ok()
        || verified.is_some_and(verified_has_deployment_admin_authority)
    {
        Ok(())
    } else {
        Err(StatusCode::FORBIDDEN)
    }
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

fn server_base_url_is_loopback(value: &str) -> bool {
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
    use super::{is_loopback_local_operator, server_base_url_is_loopback};
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
}
