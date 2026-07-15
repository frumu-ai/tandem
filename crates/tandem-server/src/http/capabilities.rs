// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use std::collections::HashMap;

pub(super) async fn evaluate_capability_readiness(
    state: &AppState,
    input: &CapabilityReadinessInput,
) -> Result<CapabilityReadinessOutput, StatusCode> {
    let runtime_mcp_tools = state.mcp.list_tools().await;
    let discovered = state
        .capability_resolver
        .discover_from_runtime(runtime_mcp_tools, state.tools.list().await)
        .await;
    let available_for_readiness = if input.available_tools.is_empty() {
        discovered.clone()
    } else {
        input.available_tools.clone()
    };
    let resolve_input = CapabilityResolveInput {
        workflow_id: input.workflow_id.clone(),
        required_capabilities: input.required_capabilities.clone(),
        optional_capabilities: input.optional_capabilities.clone(),
        provider_preference: input.provider_preference.clone(),
        available_tools: input.available_tools.clone(),
    };
    let result = state
        .capability_resolver
        .resolve(resolve_input, discovered)
        .await
        .map_err(|err| {
            tracing::warn!("capability readiness resolve failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;

    let bindings = state
        .capability_resolver
        .list_bindings()
        .await
        .unwrap_or_else(|_| CapabilityBindingsFile::default());
    let (missing_required_capabilities, unbound_capabilities) =
        classify_missing_required(&bindings, &result.missing_required);

    let mcp_servers = state.mcp.list().await;
    let enabled_servers = mcp_servers
        .values()
        .filter(|server| server.enabled)
        .collect::<Vec<_>>();
    let connected_servers = enabled_servers
        .iter()
        .filter(|server| server.connected)
        .map(|server| server.name.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();

    let mut required_providers = unbound_capabilities
        .iter()
        .flat_map(|capability_id| providers_for_capability(&bindings, capability_id))
        .collect::<Vec<_>>();
    required_providers.sort();
    required_providers.dedup();

    let mut missing_servers = Vec::new();
    let mut disconnected_servers = Vec::new();
    for provider in &required_providers {
        match provider.as_str() {
            "custom" => {}
            "mcp" => {
                if enabled_servers.is_empty() {
                    missing_servers.push(provider.clone());
                } else if connected_servers.is_empty() {
                    disconnected_servers.push(provider.clone());
                }
            }
            name => {
                let any_enabled = enabled_servers
                    .iter()
                    .any(|server| server.name.eq_ignore_ascii_case(name));
                if !any_enabled {
                    missing_servers.push(provider.clone());
                    continue;
                }
                let any_connected = connected_servers.contains(name);
                if !any_connected {
                    disconnected_servers.push(provider.clone());
                }
            }
        }
    }
    missing_servers.sort();
    missing_servers.dedup();
    disconnected_servers.sort();
    disconnected_servers.dedup();

    let mut auth_pending_tools = mcp_servers
        .values()
        .filter(|server| server.connected)
        .flat_map(|server| {
            server.pending_auth_by_tool.keys().map(move |tool| {
                format!(
                    "mcp.{}.{}",
                    mcp_namespace_segment(&server.name),
                    mcp_namespace_segment(tool)
                )
            })
        })
        .collect::<Vec<_>>();
    auth_pending_tools.sort();
    auth_pending_tools.dedup();

    let missing_secret_refs = Vec::<String>::new();
    let mcp_connector_states = mcp_connector_readiness_states(
        input,
        &available_for_readiness,
        &mcp_servers,
        &result.resolved,
    );
    let mut blocking_issues = Vec::<CapabilityBlockingIssue>::new();
    if !missing_required_capabilities.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "missing_required_capabilities".to_string(),
            message: "Some required capabilities do not have any bindings.".to_string(),
            capability_ids: missing_required_capabilities.clone(),
            providers: Vec::new(),
            tools: Vec::new(),
        });
    }
    if !unbound_capabilities.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "unbound_capabilities".to_string(),
            message: "Some required capabilities have bindings, but no available runtime tools."
                .to_string(),
            capability_ids: unbound_capabilities.clone(),
            providers: required_providers.clone(),
            tools: Vec::new(),
        });
    }
    if !missing_servers.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "missing_mcp_servers".to_string(),
            message: "Required provider servers are not configured.".to_string(),
            capability_ids: Vec::new(),
            providers: missing_servers.clone(),
            tools: Vec::new(),
        });
    }
    if !disconnected_servers.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "disconnected_mcp_servers".to_string(),
            message: "Required provider servers are configured but disconnected.".to_string(),
            capability_ids: Vec::new(),
            providers: disconnected_servers.clone(),
            tools: Vec::new(),
        });
    }
    if !auth_pending_tools.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "auth_pending_tools".to_string(),
            message: "At least one MCP tool still requires authorization.".to_string(),
            capability_ids: Vec::new(),
            providers: Vec::new(),
            tools: auth_pending_tools.clone(),
        });
    }

    let mut recommendations = Vec::<String>::new();
    if !missing_required_capabilities.is_empty() {
        recommendations.push(
            "Add capability bindings for each missing required capability in /capabilities/bindings."
                .to_string(),
        );
    }
    if !unbound_capabilities.is_empty() {
        recommendations.push(
            "Connect/refresh MCP servers so required capability bindings match discovered tools."
                .to_string(),
        );
    }
    if !missing_servers.is_empty() {
        recommendations.push("Configure missing MCP servers in /mcp and reconnect.".to_string());
    }
    if !disconnected_servers.is_empty() {
        recommendations.push("Connect and refresh disconnected MCP servers.".to_string());
    }
    if !auth_pending_tools.is_empty() {
        recommendations.push(
            "Complete MCP authorization flow for pending tools, then refresh server tools."
                .to_string(),
        );
    }

    Ok(CapabilityReadinessOutput {
        workflow_id: input
            .workflow_id
            .clone()
            .unwrap_or_else(|| "unknown_workflow".to_string()),
        runnable: blocking_issues.is_empty() || input.allow_unbound,
        resolved: result.resolved,
        missing_required_capabilities,
        unbound_capabilities,
        missing_optional_capabilities: result.missing_optional,
        missing_servers,
        disconnected_servers,
        auth_pending_tools,
        missing_secret_refs,
        mcp_connector_states,
        considered_bindings: result.considered_bindings,
        recommendations,
        blocking_issues,
    })
}

fn capability_connector_id(capability_id: &str) -> Option<&'static str> {
    if capability_id.starts_with("linear.") {
        Some("linear")
    } else {
        None
    }
}

const LINEAR_MCP_TOOL_PREFIXES: &[&str] = &["mcp.linear.", "mcp.app_linear_linear."];

fn mcp_tool_is_connector_tool(tool_name: &str, connector: &str) -> bool {
    if connector == "linear" {
        return linear_tool_slug(tool_name).is_some();
    }
    let prefix = format!("mcp.{connector}.");
    tool_name
        .trim()
        .to_ascii_lowercase()
        .starts_with(prefix.as_str())
}

fn mcp_server_is_connector(server_name: &str, connector: &str) -> bool {
    if connector == "linear" {
        let name = server_name.trim().to_ascii_lowercase();
        return name == "linear" || name == "app.linear/linear" || name == "app_linear_linear";
    }
    server_name.eq_ignore_ascii_case(connector)
}

fn linear_tool_slug(tool_name: &str) -> Option<String> {
    let tool = tool_name.trim().to_ascii_lowercase();
    LINEAR_MCP_TOOL_PREFIXES
        .iter()
        .find_map(|prefix| tool.strip_prefix(prefix).map(ToOwned::to_owned))
}

fn linear_tool_is_read(tool_name: &str) -> bool {
    let Some(slug) = linear_tool_slug(tool_name) else {
        return false;
    };
    slug.starts_with("list")
        || slug.starts_with("get")
        || slug.starts_with("search")
        || slug == "viewer"
}

fn linear_tool_is_write(tool_name: &str) -> bool {
    mcp_tool_is_connector_tool(tool_name, "linear") && !linear_tool_is_read(tool_name)
}

fn mcp_connector_readiness_states(
    input: &CapabilityReadinessInput,
    available_tools: &[crate::capability_resolver::CapabilityToolAvailability],
    mcp_servers: &HashMap<String, tandem_runtime::McpServer>,
    resolved: &[crate::capability_resolver::CapabilityResolution],
) -> Vec<crate::capability_resolver::McpConnectorReadiness> {
    let mut connectors = input
        .required_capabilities
        .iter()
        .chain(input.optional_capabilities.iter())
        .filter_map(|capability| capability_connector_id(capability))
        .collect::<Vec<_>>();
    connectors.sort();
    connectors.dedup();

    connectors
        .into_iter()
        .map(|connector| {
            let configured_server = mcp_servers
                .values()
                .find(|server| mcp_server_is_connector(&server.name, connector));
            let configured = configured_server.is_some()
                || available_tools
                    .iter()
                    .any(|tool| mcp_tool_is_connector_tool(&tool.tool_name, connector));
            let connected = configured_server.is_some_and(|server| server.connected);
            let mut read_tools = Vec::new();
            let mut write_tools = Vec::new();
            for tool in available_tools {
                if connector == "linear" && linear_tool_is_read(&tool.tool_name) {
                    read_tools.push(tool.tool_name.clone());
                } else if connector == "linear" && linear_tool_is_write(&tool.tool_name) {
                    write_tools.push(tool.tool_name.clone());
                }
            }
            for row in resolved {
                if connector == "linear" && row.capability_id.starts_with("linear.") {
                    if row.capability_id.contains("list") || row.capability_id.contains("get") {
                        read_tools.push(row.tool_name.clone());
                    } else {
                        write_tools.push(row.tool_name.clone());
                    }
                }
            }
            read_tools.sort();
            read_tools.dedup();
            write_tools.sort();
            write_tools.dedup();
            let access = if !configured {
                "missing_connector"
            } else if !write_tools.is_empty() {
                "write_capable"
            } else if !read_tools.is_empty() {
                "read_only"
            } else if !connected {
                "disconnected"
            } else {
                "connected_no_matching_tools"
            };
            crate::capability_resolver::McpConnectorReadiness {
                provider: connector.to_string(),
                configured,
                connected,
                access: access.to_string(),
                read_tools,
                write_tools,
            }
        })
        .collect()
}

pub(super) async fn capabilities_bindings_get(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let bindings = state
        .capability_resolver
        .list_bindings()
        .await
        .map_err(|err| {
            tracing::warn!("capability bindings get failed: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(json!({ "bindings": bindings })))
}

pub(super) async fn capabilities_bindings_put(
    State(state): State<AppState>,
    Json(file): Json<CapabilityBindingsFile>,
) -> Result<Json<Value>, StatusCode> {
    state
        .capability_resolver
        .set_bindings(file)
        .await
        .map_err(|err| {
            tracing::warn!("capability bindings put failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;
    Ok(Json(json!({ "ok": true })))
}

pub(super) async fn capabilities_bindings_refresh_builtins(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let summary = state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .map_err(|err| {
            tracing::warn!("capability bindings refresh failed: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(json!({ "ok": true, "summary": summary })))
}

pub(super) async fn capabilities_bindings_reset_to_builtins(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let summary = state
        .capability_resolver
        .reset_to_builtin_bindings()
        .await
        .map_err(|err| {
            tracing::warn!("capability bindings reset failed: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(json!({ "ok": true, "summary": summary })))
}

pub(super) async fn capabilities_discovery(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let discovered = state
        .capability_resolver
        .discover_from_runtime(state.mcp.list_tools().await, state.tools.list().await)
        .await;
    Ok(Json(json!({ "tools": discovered })))
}

pub(super) async fn capabilities_resolve(
    State(state): State<AppState>,
    Json(input): Json<CapabilityResolveInput>,
) -> Result<Response, StatusCode> {
    let discovered = state
        .capability_resolver
        .discover_from_runtime(state.mcp.list_tools().await, state.tools.list().await)
        .await;
    let result = state
        .capability_resolver
        .resolve(input.clone(), discovered)
        .await
        .map_err(|err| {
            tracing::warn!("capability resolve failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;
    if !result.missing_required.is_empty() {
        let bindings = state
            .capability_resolver
            .list_bindings()
            .await
            .unwrap_or_else(|_| CapabilityBindingsFile::default());
        let mut suggestions = HashMap::<String, Vec<String>>::new();
        for missing in &result.missing_required {
            let rows = bindings
                .bindings
                .iter()
                .filter(|row| row.capability_id == *missing)
                .map(|row| format!("{}:{}", row.provider, row.tool_name))
                .collect::<Vec<_>>();
            suggestions.insert(missing.clone(), rows);
        }
        let workflow_id = input
            .workflow_id
            .clone()
            .unwrap_or_else(|| "unknown_workflow".to_string());
        let payload = crate::capability_resolver::CapabilityResolver::missing_capability_error(
            &workflow_id,
            &result.missing_required,
            &suggestions,
        );
        return Ok((StatusCode::CONFLICT, Json(payload)).into_response());
    }
    Ok(Json(json!({ "resolution": result })).into_response())
}

pub(super) async fn capabilities_readiness(
    State(state): State<AppState>,
    Json(input): Json<CapabilityReadinessInput>,
) -> Result<Response, StatusCode> {
    let output = evaluate_capability_readiness(&state, &input).await?;
    let status = if output.runnable {
        StatusCode::OK
    } else {
        StatusCode::CONFLICT
    };
    state.event_bus.publish(EngineEvent::new(
        "capabilities.readiness.evaluated",
        json!({
            "workflow_id": output.workflow_id,
            "runnable": output.runnable,
            "blocking_issue_count": output.blocking_issues.len(),
        }),
    ));
    Ok((status, Json(json!({ "readiness": output }))).into_response())
}
