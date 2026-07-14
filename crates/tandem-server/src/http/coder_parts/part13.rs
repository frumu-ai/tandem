fn with_coder_mcp_phase_authority(
    mut args: Value,
    server_name: &str,
    tool_name: &str,
    phase: &str,
) -> Value {
    let allowed_tool = format!(
        "mcp.{}.{}",
        crate::http::mcp::mcp_namespace_segment(server_name),
        crate::http::mcp::mcp_namespace_segment(tool_name)
    );
    args["__phase_tool_authority"] = json!({
        "phase": phase,
        "allowed_tools": [allowed_tool],
        "source": "coder_server_dispatch",
        "policy_id": "coder_endpoint_tool_authority",
    });
    args
}

async fn call_create_pull_request(
    state: &AppState,
    tenant_context: &tandem_types::TenantContext,
    verified_tenant_context: Option<&tandem_types::VerifiedTenantContext>,
    server_name: &str,
    tool_name: &str,
    owner: &str,
    repo: &str,
    title: &str,
    body: &str,
    base_branch: &str,
    head_branch: &str,
) -> Result<tandem_types::ToolResult, StatusCode> {
    let preferred = with_coder_mcp_phase_authority(
        json!({
            "method": "create",
            "owner": owner,
            "repo": repo,
            "title": title,
            "body": body,
            "base": base_branch,
            "head": head_branch,
            "draft": true,
        }),
        server_name,
        tool_name,
        "coder_pr_submit",
    );
    let fallback = with_coder_mcp_phase_authority(
        json!({
            "owner": owner,
            "repo": repo,
            "title": title,
            "body": body,
            "base": base_branch,
            "head": head_branch,
            "draft": true,
        }),
        server_name,
        tool_name,
        "coder_pr_submit",
    );
    let first = crate::http::mcp_run_as::call_mcp_tool_for_tenant_with_verified_context(
        state,
        server_name,
        tool_name,
        preferred,
        tenant_context,
        verified_tenant_context,
    )
    .await;
    match first {
        Ok(result) => Ok(result),
        Err(_) => crate::http::mcp_run_as::call_mcp_tool_for_tenant_with_verified_context(
            state,
            server_name,
            tool_name,
            fallback,
            tenant_context,
            verified_tenant_context,
        )
        .await
        .map_err(|_| StatusCode::BAD_GATEWAY),
    }
}
