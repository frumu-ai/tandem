use tandem_types::TenantContext;

use crate::{AppState, AutomationMcpConnectionGrant, AutomationMcpRunAs};

pub(crate) fn automation_mcp_connection_grant_for_server<'a>(
    server_name: &str,
    allowed_connections: &'a [AutomationMcpConnectionGrant],
) -> Option<&'a AutomationMcpConnectionGrant> {
    allowed_connections
        .iter()
        .find(|grant| grant.server.eq_ignore_ascii_case(server_name))
}

pub(crate) fn automation_mcp_preflight_tenant_context(
    tenant_context: &TenantContext,
    connection_grant: Option<&AutomationMcpConnectionGrant>,
) -> Result<TenantContext, &'static str> {
    match connection_grant.and_then(|grant| grant.run_as.as_ref()) {
        Some(AutomationMcpRunAs::CurrentActor) | None => Ok(tenant_context.clone()),
        Some(AutomationMcpRunAs::ServicePrincipal { .. }) => {
            let mut service_tenant = tenant_context.clone();
            service_tenant.actor_id = None;
            Ok(service_tenant)
        }
        Some(
            AutomationMcpRunAs::AutomationPrincipal { .. }
            | AutomationMcpRunAs::SharedConnection { .. },
        ) => Err("grant_principal_not_executable"),
    }
}

pub(crate) async fn automation_mcp_remote_tool_names_for_tenant(
    state: &AppState,
    server_name: &str,
    tenant_context: &TenantContext,
) -> Vec<String> {
    let mut names = state
        .mcp
        .list_tools_for_tenant(tenant_context)
        .await
        .into_iter()
        .filter(|tool| tool.server_name == server_name)
        .map(|tool| {
            if tool.namespaced_name.trim().is_empty() {
                format!(
                    "mcp.{}.{}",
                    crate::http::mcp::mcp_namespace_segment(server_name),
                    tool.tool_name
                )
            } else {
                tool.namespaced_name
            }
        })
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn service_principal_grant_uses_service_tenant_context_for_preflight() {
        let tenant = TenantContext::explicit("org-a", "workspace-a", Some("alice".to_string()));
        let grant = AutomationMcpConnectionGrant {
            server: "notion".to_string(),
            connection_id: None,
            run_as: Some(AutomationMcpRunAs::ServicePrincipal {
                principal_id: "tenant-service".to_string(),
            }),
        };

        let resolved = automation_mcp_preflight_tenant_context(&tenant, Some(&grant))
            .expect("service principal preflight context");

        assert_eq!(resolved.org_id, tenant.org_id);
        assert_eq!(resolved.workspace_id, tenant.workspace_id);
        assert!(resolved.actor_id.is_none());
    }
}
