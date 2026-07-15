// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Incident Monitor destination readiness helpers split from part02.rs for the
// touched-file size gate.

fn incident_monitor_destination_readiness(
    config: &IncidentMonitorConfig,
    status: &IncidentMonitorStatus,
    servers: &std::collections::HashMap<String, tandem_runtime::McpServer>,
) -> Vec<IncidentMonitorDestinationReadiness> {
    status
        .destinations
        .iter()
        .map(|destination| {
            let mut missing = Vec::new();
            let mut detail = None;
            let requires_approval =
                config.require_approval_for_new_issues || destination.require_approval;

            if !config.enabled {
                missing.push("Incident Monitor is disabled".to_string());
            }
            if config.paused {
                missing.push("Incident Monitor is paused".to_string());
            }
            if !destination.enabled {
                missing.push("Destination is disabled".to_string());
            }

            let publish_ready = match destination.kind {
                IncidentMonitorDestinationKind::GithubIssue => {
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

                    // `status.readiness.github_read_ready/write_ready` are resolved
                    // from the *global* selected server, so only gate on them when
                    // the destination actually uses the global server. A
                    // destination with its own `mcp_server` publishes through that
                    // server (`destination.publish_config(...)`); its capabilities
                    // are validated by connectedness here and by the adapter's tool
                    // resolution at execution, so gating it on the global flags
                    // would reject valid destination-specific routes (TAN-545).
                    let uses_global_server = destination
                        .mcp_server
                        .as_deref()
                        .map_or(true, |name| Some(name) == config.mcp_server.as_deref());
                    let global_capabilities_ok = !uses_global_server
                        || (status.readiness.github_read_ready
                            && status.readiness.github_write_ready);
                    if !global_capabilities_ok {
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
                        && global_capabilities_ok
                }
                IncidentMonitorDestinationKind::LinearIssue => {
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
                IncidentMonitorDestinationKind::Webhook => {
                    let (webhook_ready, webhook_missing, webhook_detail) =
                        crate::incident_monitor_webhook::webhook_destination_readiness(destination);
                    missing.extend(webhook_missing);
                    detail = webhook_detail;

                    config.enabled && !config.paused && destination.enabled && webhook_ready
                }
                IncidentMonitorDestinationKind::Telemetry => {
                    if destination
                        .telemetry_path
                        .as_deref()
                        .is_some_and(|value| value.trim().is_empty())
                    {
                        missing.push("Telemetry path is blank".to_string());
                    }
                    config.enabled
                        && !config.paused
                        && destination.enabled
                        && !destination
                            .telemetry_path
                            .as_deref()
                            .is_some_and(|value| value.trim().is_empty())
                }
                IncidentMonitorDestinationKind::McpTool => {
                    let (mcp_ready, mcp_missing, mcp_detail) =
                        crate::incident_monitor_mcp::mcp_tool_destination_readiness(
                            config,
                            destination,
                            servers,
                        );
                    missing.extend(mcp_missing);
                    detail = mcp_detail;

                    config.enabled && !config.paused && destination.enabled && mcp_ready
                }
                IncidentMonitorDestinationKind::InternalMemory => {
                    let category = destination
                        .memory_category
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or("failure_pattern");
                    if !crate::incident_monitor_local::is_supported_memory_category(category) {
                        missing.push(
                            "Memory category must be failure_pattern, recurrence, policy_gap, or safety_risk"
                                .to_string(),
                        );
                    }
                    config.enabled
                        && !config.paused
                        && destination.enabled
                        && crate::incident_monitor_local::is_supported_memory_category(category)
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

            IncidentMonitorDestinationReadiness {
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

#[cfg(test)]
mod tan545_destination_readiness_tests {
    use super::*;

    fn connected_server(name: &str) -> tandem_runtime::McpServer {
        serde_json::from_value(serde_json::json!({
            "name": name,
            "transport": "stdio",
            "connected": true,
        }))
        .expect("server")
    }

    fn github_destination(mcp_server: Option<&str>) -> IncidentMonitorDestinationConfig {
        IncidentMonitorDestinationConfig {
            destination_id: "gh".to_string(),
            name: "GitHub".to_string(),
            kind: IncidentMonitorDestinationKind::GithubIssue,
            enabled: true,
            repo: Some("acme/app".to_string()),
            mcp_server: mcp_server.map(str::to_string),
            ..IncidentMonitorDestinationConfig::default()
        }
    }

    fn readiness_for(
        destination: IncidentMonitorDestinationConfig,
        global_mcp_server: Option<&str>,
    ) -> IncidentMonitorDestinationReadiness {
        let config = IncidentMonitorConfig {
            enabled: true,
            mcp_server: global_mcp_server.map(str::to_string),
            destinations: vec![destination.clone()],
            ..IncidentMonitorConfig::default()
        };
        // Global GitHub capabilities are absent (resolved from the global server).
        let status = IncidentMonitorStatus {
            destinations: vec![destination],
            readiness: IncidentMonitorReadiness {
                github_read_ready: false,
                github_write_ready: false,
                ..IncidentMonitorReadiness::default()
            },
            ..IncidentMonitorStatus::default()
        };
        let mut servers = std::collections::HashMap::new();
        servers.insert("dest-gh".to_string(), connected_server("dest-gh"));
        incident_monitor_destination_readiness(&config, &status, &servers)
            .into_iter()
            .next()
            .expect("one readiness row")
    }

    #[test]
    fn tan545_github_destination_with_own_server_is_ready_despite_global_caps() {
        // A GitHub destination with its own connected MCP server publishes through
        // that server, so it must not be gated on the global capability flags.
        let row = readiness_for(github_destination(Some("dest-gh")), None);
        assert!(
            row.publish_ready,
            "destination-specific GitHub server must be ready: {:?}",
            row.missing
        );
    }

    #[test]
    fn tan545_github_destination_on_global_server_is_gated_on_global_caps() {
        // The same connected server, but reached as the *global* server, is still
        // gated on the global capability flags — so a genuinely unready global
        // deployment is blocked.
        let row = readiness_for(github_destination(None), Some("dest-gh"));
        assert!(!row.publish_ready);
        assert!(row
            .missing
            .iter()
            .any(|reason| reason.contains("GitHub capabilities are missing")));
    }
}
