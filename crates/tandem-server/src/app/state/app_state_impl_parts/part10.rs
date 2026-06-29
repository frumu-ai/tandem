// Bug Monitor destination readiness helpers split from part02.rs for the
// touched-file size gate.

fn bug_monitor_destination_readiness(
    config: &BugMonitorConfig,
    status: &BugMonitorStatus,
    servers: &std::collections::HashMap<String, tandem_runtime::McpServer>,
) -> Vec<BugMonitorDestinationReadiness> {
    status
        .destinations
        .iter()
        .map(|destination| {
            let mut missing = Vec::new();
            let mut detail = None;
            let requires_approval =
                config.require_approval_for_new_issues || destination.require_approval;

            if !config.enabled {
                missing.push("Bug Monitor is disabled".to_string());
            }
            if config.paused {
                missing.push("Bug Monitor is paused".to_string());
            }
            if !destination.enabled {
                missing.push("Destination is disabled".to_string());
            }

            let publish_ready = match destination.kind {
                BugMonitorDestinationKind::GithubIssue => {
                    let destination_repo = destination.repo.as_deref().or(config.repo.as_deref());
                    let destination_repo_valid = destination_repo
                        .map(is_valid_owner_repo_slug)
                        .unwrap_or(false);
                    if destination_repo.is_none() {
                        missing.push("GitHub repo is missing".to_string());
                    } else if !destination_repo_valid {
                        missing.push("GitHub repo must be in owner/repo format".to_string());
                    }

                    let destination_server_name = destination
                        .mcp_server
                        .as_deref()
                        .or(config.mcp_server.as_deref());
                    let destination_server =
                        destination_server_name.and_then(|name| servers.get(name));
                    if destination_server_name.is_none() {
                        missing.push("MCP server is missing".to_string());
                    } else if destination_server.is_none() {
                        missing.push("MCP server is not configured".to_string());
                    } else if !destination_server
                        .as_ref()
                        .map(|row| row.connected)
                        .unwrap_or(false)
                    {
                        missing.push("MCP server is disconnected".to_string());
                    }

                    if !status.readiness.github_read_ready || !status.readiness.github_write_ready {
                        missing.push("GitHub capabilities are missing".to_string());
                    }

                    config.enabled
                        && !config.paused
                        && destination.enabled
                        && destination_repo_valid
                        && destination_server
                            .as_ref()
                            .map(|row| row.connected)
                            .unwrap_or(false)
                        && status.readiness.github_read_ready
                        && status.readiness.github_write_ready
                }
                BugMonitorDestinationKind::LinearIssue => {
                    let team_valid = destination
                        .linear_team
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|value| !value.is_empty());
                    let project_valid = destination
                        .linear_project
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|value| !value.is_empty());
                    if !team_valid {
                        missing.push("Linear team is missing".to_string());
                    }
                    if !project_valid {
                        missing.push("Linear project is missing".to_string());
                    }

                    let destination_server_name = destination
                        .mcp_server
                        .as_deref()
                        .or(config.mcp_server.as_deref());
                    let destination_server =
                        destination_server_name.and_then(|name| servers.get(name));
                    if destination_server_name.is_none() {
                        missing.push("MCP server is missing".to_string());
                    } else if destination_server.is_none() {
                        missing.push("MCP server is not configured".to_string());
                    } else if !destination_server
                        .as_ref()
                        .map(|row| row.connected)
                        .unwrap_or(false)
                    {
                        missing.push("MCP server is disconnected".to_string());
                    }

                    let linear_list_ready = destination_server
                        .as_ref()
                        .is_some_and(|server| linear_server_has_list_issues_tool(server));
                    let linear_create_ready = destination_server
                        .as_ref()
                        .is_some_and(|server| linear_server_has_create_issue_tool(server));
                    if destination_server.is_some() && !linear_list_ready {
                        missing.push("Linear list issues capability is missing".to_string());
                    }
                    if destination_server.is_some() && !linear_create_ready {
                        missing.push("Linear create issue capability is missing".to_string());
                    }

                    config.enabled
                        && !config.paused
                        && destination.enabled
                        && team_valid
                        && project_valid
                        && destination_server
                            .as_ref()
                            .map(|row| row.connected)
                            .unwrap_or(false)
                        && linear_list_ready
                        && linear_create_ready
                }
                BugMonitorDestinationKind::Webhook => {
                    let (webhook_ready, webhook_missing, webhook_detail) =
                        crate::bug_monitor_webhook::webhook_destination_readiness(destination);
                    missing.extend(webhook_missing);
                    detail = webhook_detail;

                    config.enabled && !config.paused && destination.enabled && webhook_ready
                }
                _ => {
                    detail = Some(
                        "Destination kind is configured but is not available in this phase"
                            .to_string(),
                    );
                    missing.push("Destination implementation is not available".to_string());
                    false
                }
            };

            BugMonitorDestinationReadiness {
                destination_id: destination.destination_id.clone(),
                kind: destination.kind.clone(),
                enabled: destination.enabled,
                ready: publish_ready,
                publish_ready,
                requires_approval,
                missing,
                detail,
            }
        })
        .collect()
}

fn linear_server_has_list_issues_tool(server: &tandem_runtime::McpServer) -> bool {
    linear_server_has_any_tool(
        server,
        &[
            "list_issues",
            "list_my_issues",
            "mcp.linear.list_issues",
            "mcp.linear.list_my_issues",
            "mcp.app_linear_linear.list_issues",
            "mcp.app_linear_linear.list_my_issues",
            "linear_list_issues",
        ],
    )
}

fn linear_server_has_create_issue_tool(server: &tandem_runtime::McpServer) -> bool {
    linear_server_has_any_tool(
        server,
        &[
            "create_issue",
            "save_issue",
            "update_issue",
            "mcp.linear.create_issue",
            "mcp.linear.save_issue",
            "mcp.linear.update_issue",
            "mcp.app_linear_linear.create_issue",
            "mcp.app_linear_linear.save_issue",
            "mcp.app_linear_linear.update_issue",
            "linear_create_issue",
            "linear_save_issue",
        ],
    )
}

fn linear_server_has_any_tool(server: &tandem_runtime::McpServer, candidates: &[&str]) -> bool {
    server.tool_cache.iter().any(|tool| {
        candidates.iter().any(|candidate| {
            tool.tool_name.eq_ignore_ascii_case(candidate)
                || format!("mcp.{}.{}", server.name, tool.tool_name).eq_ignore_ascii_case(candidate)
        })
    })
}
