// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

pub(crate) struct McpOAuthEndpointAuthorization<'a> {
    state: &'a AppState,
    tenant_context: &'a TenantContext,
}

impl<'a> McpOAuthEndpointAuthorization<'a> {
    pub(crate) fn new(state: &'a AppState, tenant_context: &'a TenantContext) -> Self {
        Self {
            state,
            tenant_context,
        }
    }

    fn allows_private_endpoint(&self) -> bool {
        allow_private_mcp_oauth_endpoint(self.state, self.tenant_context)
    }
}

pub(crate) struct ResolvedMcpOAuthTarget {
    target: crate::outbound_http::ResolvedPublicHttpsTarget,
    requires_private_authorization: bool,
}

impl ResolvedMcpOAuthTarget {
    pub(crate) fn url(&self) -> &reqwest::Url {
        self.target.url()
    }

    pub(crate) fn client(&self, timeout: std::time::Duration) -> anyhow::Result<reqwest::Client> {
        self.target.client(timeout)
    }

    pub(crate) fn ensure_authorized(
        &self,
        authorization: &McpOAuthEndpointAuthorization<'_>,
    ) -> Result<(), String> {
        if self.requires_private_authorization && !authorization.allows_private_endpoint() {
            return Err("MCP private OAuth endpoint authorization was revoked".to_string());
        }
        Ok(())
    }
}

pub(crate) async fn resolve_mcp_oauth_target(
    raw: &str,
    authorization: &McpOAuthEndpointAuthorization<'_>,
) -> Result<ResolvedMcpOAuthTarget, String> {
    match crate::outbound_http::resolve_public_https_url(raw).await {
        Ok(target) => Ok(ResolvedMcpOAuthTarget {
            target,
            requires_private_authorization: false,
        }),
        Err(public_error) => {
            if !authorization.allows_private_endpoint() {
                return Err(format!("MCP OAuth URL rejected: {public_error}"));
            }
            let target = crate::outbound_http::resolve_standalone_provider_url(raw)
                .await
                .map_err(|error| format!("MCP OAuth URL rejected: {error}"))?;
            Ok(ResolvedMcpOAuthTarget {
                target,
                requires_private_authorization: true,
            })
        }
    }
}
