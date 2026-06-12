use std::collections::{HashMap, HashSet};

use serde_json::{json, Value};
use tandem_core::{tool_name_security_descriptor, tool_risk_tier_from_name_and_descriptor};
use tandem_runtime::{McpSecretRef, McpServer};
use tandem_types::{
    AccessDecision, AccessPermission, DataClass, ResourceKind, ResourceRef, StrictTenantContext,
    TenantContext, ToolDefaultVisibility, ToolSecurityDescriptor,
};

use crate::AppState;

pub(crate) async fn mcp_inventory_snapshot(state: &AppState) -> Value {
    let mut server_rows = state.mcp.list().await.into_values().collect::<Vec<_>>();
    server_rows.sort_by(|a, b| a.name.cmp(&b.name));

    let remote_tools = state.mcp.list_tools().await;
    let registered_schemas = state.tools.list().await;
    let registered_tool_names = registered_schemas
        .iter()
        .map(|schema| schema.name.clone())
        .collect::<Vec<_>>();
    let registered_security_by_name = registered_schemas
        .into_iter()
        .map(|schema| {
            (
                schema.name.clone(),
                serde_json::to_value(schema.security).unwrap_or(Value::Null),
            )
        })
        .collect::<HashMap<_, _>>();
    let catalog_tool_security = crate::mcp_catalog::index()
        .and_then(|catalog| catalog.get("servers"))
        .and_then(Value::as_array)
        .map(|servers| catalog_tool_security_by_namespaced_name(servers.as_slice()))
        .unwrap_or_default();

    let mut connected_server_names = Vec::new();
    let mut enabled_server_names = Vec::new();
    let mut all_remote_tool_names = Vec::new();
    let mut all_registered_tool_names = Vec::new();
    let mut governed_tool_registry = Vec::new();
    let mut servers = Vec::new();

    for server in server_rows {
        let mut remote_tool_names = remote_tools
            .iter()
            .filter(|tool| tool.server_name == server.name)
            .map(|tool| tool.namespaced_name.trim().to_string())
            .filter(|tool_name| !tool_name.is_empty())
            .collect::<Vec<_>>();
        remote_tool_names.sort();
        remote_tool_names.dedup();

        let registered_names = mcp_tool_names_for_server(&registered_tool_names, &server.name);
        let (tool_security, mut server_governed_tools) = mcp_tool_security_for_inventory_server(
            &server,
            &remote_tool_names,
            &registered_names,
            &catalog_tool_security,
            &registered_security_by_name,
        );
        governed_tool_registry.append(&mut server_governed_tools);

        if server.enabled {
            enabled_server_names.push(server.name.clone());
        }
        if server.connected {
            connected_server_names.push(server.name.clone());
        }
        all_remote_tool_names.extend(remote_tool_names.clone());
        all_registered_tool_names.extend(registered_names.clone());

        let mut pending_auth_tools = server
            .pending_auth_by_tool
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        pending_auth_tools.sort();
        pending_auth_tools.dedup();

        servers.push(json!({
            "name": server.name,
            "transport": server.transport,
            "enabled": server.enabled,
            "connected": server.connected,
            "last_error": server.last_error,
            "last_auth_challenge": server.last_auth_challenge,
            "pending_auth_tools": pending_auth_tools,
            "remote_tool_count": remote_tool_names.len(),
            "registered_tool_count": registered_names.len(),
            "allowed_tool_count": server.allowed_tools.as_ref().map(|tools| tools.len()).unwrap_or(remote_tool_names.len()),
            "allowed_tools": server.allowed_tools.clone(),
            "discovered_tool_count": server.tool_cache.len(),
            "remote_tools": remote_tool_names,
            "registered_tools": registered_names,
            "tool_security": tool_security,
        }));
    }

    connected_server_names.sort();
    connected_server_names.dedup();
    enabled_server_names.sort();
    enabled_server_names.dedup();
    all_remote_tool_names.sort();
    all_remote_tool_names.dedup();
    all_registered_tool_names.sort();
    all_registered_tool_names.dedup();
    governed_tool_registry.sort_by(|a, b| {
        a.get("namespaced_name")
            .and_then(Value::as_str)
            .cmp(&b.get("namespaced_name").and_then(Value::as_str))
    });

    json!({
        "inventory_version": 1,
        "registry_version": 1,
        "connected_server_names": connected_server_names,
        "enabled_server_names": enabled_server_names,
        "remote_tools": all_remote_tool_names,
        "registered_tools": all_registered_tool_names,
        "governed_tool_registry": governed_tool_registry,
        "servers": servers,
    })
}

fn mcp_tool_names_for_server(tool_names: &[String], server_name: &str) -> Vec<String> {
    let prefix = format!("mcp.{}.", mcp_namespace_segment(server_name));
    let mut tools = tool_names
        .iter()
        .filter(|tool_name| tool_name.starts_with(&prefix))
        .cloned()
        .collect::<Vec<_>>();
    tools.sort();
    tools.dedup();
    tools
}

fn catalog_tool_security_by_namespaced_name(catalog_servers: &[Value]) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    for server in catalog_servers {
        let Some(tool_security) = server.get("tool_security").and_then(Value::as_object) else {
            continue;
        };
        for value in tool_security.values() {
            let Some(namespaced_name) = value.get("namespaced_name").and_then(Value::as_str) else {
                continue;
            };
            if let Some(security) = value.get("security") {
                out.insert(namespaced_name.to_string(), security.clone());
            }
        }
    }
    out
}

fn mcp_tool_security_for_inventory_server(
    server: &McpServer,
    remote_tool_names: &[String],
    registered_tool_names: &[String],
    catalog_tool_security: &HashMap<String, Value>,
    registered_security: &HashMap<String, Value>,
) -> (Value, Vec<Value>) {
    let server_segment = mcp_namespace_segment(&server.name);
    let mut tool_names = remote_tool_names
        .iter()
        .chain(registered_tool_names.iter())
        .cloned()
        .collect::<Vec<_>>();
    tool_names.sort();
    tool_names.dedup();

    let mut out = serde_json::Map::new();
    let mut registry = Vec::new();
    for tool_name in tool_names {
        let short_name = tool_name
            .strip_prefix(&format!("mcp.{server_segment}."))
            .unwrap_or(&tool_name)
            .to_string();
        let security = catalog_tool_security
            .get(&tool_name)
            .cloned()
            .filter(security_value_non_empty)
            .or_else(|| {
                registered_security
                    .get(&tool_name)
                    .cloned()
                    .filter(security_value_non_empty)
            })
            .unwrap_or_else(|| {
                serde_json::to_value(tool_name_security_descriptor(&tool_name))
                    .unwrap_or(Value::Null)
            });
        let governance =
            governed_tool_registry_metadata(server, &short_name, &tool_name, &security);
        registry.push(governance.clone());
        out.insert(
            short_name.clone(),
            json!({
                "tool_name": short_name,
                "namespaced_name": tool_name,
                "security": security,
                "governance": governance,
            }),
        );
    }
    (Value::Object(out), registry)
}

fn governed_tool_registry_metadata(
    server: &McpServer,
    short_name: &str,
    namespaced_name: &str,
    security: &Value,
) -> Value {
    let descriptor = security_descriptor_for_tool(namespaced_name, security);
    let risk_tier = tool_risk_tier_from_name_and_descriptor(namespaced_name, &descriptor);
    let hidden_without_grant =
        matches!(descriptor.default_visibility, ToolDefaultVisibility::Hidden)
            || descriptor.admin_surface
            || descriptor.credential_access
            || risk_tier.hidden_without_grant_by_default();
    let approval_required = risk_tier.approval_required_by_default()
        || descriptor.external_side_effect
        || descriptor.required_permissions.iter().any(|permission| {
            matches!(
                permission,
                AccessPermission::Admin | AccessPermission::Execute
            )
        });
    let (default_access, default_policy) = if hidden_without_grant {
        ("hidden", "hidden_without_grant")
    } else if approval_required {
        ("gated", "approval_required")
    } else {
        ("visible", "allow")
    };
    let reasons = governed_tool_reasons(&descriptor, hidden_without_grant, approval_required);
    let tenant_binding = first_store_tenant_context(server);

    json!({
        "registry_version": 1,
        "tool_name": short_name,
        "namespaced_name": namespaced_name,
        "server_name": server.name,
        "server_segment": mcp_namespace_segment(&server.name),
        "owner": {
            "kind": "mcp_server",
            "id": server.name,
        },
        "tenant_binding": tenant_binding,
        "resource_scope": {
            "required_permissions": descriptor.required_permissions,
            "resource_kinds": descriptor.resource_kinds,
            "data_classes": descriptor.data_classes,
        },
        "risk_tier": risk_tier.as_str(),
        "default_visibility": descriptor.default_visibility,
        "default_access": default_access,
        "default_policy": default_policy,
        "approval_required_by_default": approval_required,
        "hidden_without_grant_by_default": hidden_without_grant,
        "admin_surface": descriptor.admin_surface,
        "credential_access": descriptor.credential_access,
        "external_side_effect": descriptor.external_side_effect,
        "credential_binding": credential_binding_metadata(server),
        "last_discovered_at_ms": server.tools_fetched_at_ms,
        "last_used_at_ms": Value::Null,
        "reasons": reasons,
    })
}

fn security_descriptor_for_tool(namespaced_name: &str, security: &Value) -> ToolSecurityDescriptor {
    serde_json::from_value::<ToolSecurityDescriptor>(security.clone())
        .ok()
        .filter(|descriptor| !descriptor.is_empty())
        .unwrap_or_else(|| tool_name_security_descriptor(namespaced_name))
}

fn governed_tool_reasons(
    descriptor: &ToolSecurityDescriptor,
    hidden_without_grant: bool,
    approval_required: bool,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if matches!(descriptor.default_visibility, ToolDefaultVisibility::Hidden) {
        reasons.push("descriptor_hidden_by_default");
    }
    if descriptor.admin_surface {
        reasons.push("admin_surface");
    }
    if descriptor.credential_access {
        reasons.push("credential_access");
    }
    if descriptor.external_side_effect {
        reasons.push("external_side_effect");
    }
    if hidden_without_grant {
        reasons.push("hidden_without_grant");
    } else if approval_required {
        reasons.push("approval_required");
    } else {
        reasons.push("read_discover_visible");
    }
    reasons
}

fn credential_binding_metadata(server: &McpServer) -> Value {
    let mut refs = Vec::new();
    for (header, secret_ref) in &server.secret_headers {
        refs.push(redacted_secret_ref_metadata(
            Some(header.as_str()),
            secret_ref,
        ));
    }
    if let Some(oauth) = &server.oauth {
        if let Some(secret_ref) = &oauth.client_secret_ref {
            refs.push(redacted_secret_ref_metadata(
                Some("oauth.client_secret"),
                secret_ref,
            ));
        }
    }
    refs.sort_by(|a, b| {
        a.get("binding")
            .and_then(Value::as_str)
            .cmp(&b.get("binding").and_then(Value::as_str))
    });
    json!({
        "auth_kind": server.auth_kind,
        "uses_oauth": server.oauth.is_some() || server.auth_kind.trim().eq_ignore_ascii_case("oauth"),
        "has_static_headers": !server.headers.is_empty(),
        "has_secret_refs": !refs.is_empty(),
        "secret_ref_count": refs.len(),
        "secret_refs": refs,
    })
}

fn redacted_secret_ref_metadata(binding: Option<&str>, secret_ref: &McpSecretRef) -> Value {
    match secret_ref {
        McpSecretRef::Store {
            secret_id,
            tenant_context,
        } => json!({
            "binding": binding,
            "kind": "store",
            "secret_id": secret_id,
            "tenant_context": tenant_context,
        }),
        McpSecretRef::Env { env } => json!({
            "binding": binding,
            "kind": "env",
            "env": env,
        }),
        McpSecretRef::BearerEnv { env } => json!({
            "binding": binding,
            "kind": "bearer_env",
            "env": env,
        }),
    }
}

fn first_store_tenant_context(server: &McpServer) -> Value {
    server
        .secret_headers
        .values()
        .find_map(secret_ref_tenant_context)
        .or_else(|| {
            server
                .oauth
                .as_ref()
                .and_then(|oauth| oauth.client_secret_ref.as_ref())
                .and_then(secret_ref_tenant_context)
        })
        .map(|tenant_context| json!(tenant_context))
        .unwrap_or(Value::Null)
}

fn secret_ref_tenant_context(secret_ref: &McpSecretRef) -> Option<&TenantContext> {
    match secret_ref {
        McpSecretRef::Store { tenant_context, .. } => Some(tenant_context),
        McpSecretRef::Env { .. } | McpSecretRef::BearerEnv { .. } => None,
    }
}

fn security_value_non_empty(value: &Value) -> bool {
    serde_json::from_value::<ToolSecurityDescriptor>(value.clone())
        .map(|descriptor| !descriptor.is_empty())
        .unwrap_or(false)
}

#[derive(Default)]
pub(super) struct McpToolScopeFilter {
    pub(super) wildcard_server_segments: HashSet<String>,
    pub(super) exact_tool_names: HashSet<String>,
}

fn parse_mcp_tool_scope_filter(tool_names: &[String]) -> McpToolScopeFilter {
    let mut filter = McpToolScopeFilter::default();
    for raw in tool_names {
        let tool_name = raw.trim();
        if tool_name.is_empty() {
            continue;
        }
        if let Some(rest) = tool_name.strip_prefix("mcp.") {
            if let Some((server_segment, tool_segment)) = rest.split_once('.') {
                if tool_segment == "*" {
                    filter
                        .wildcard_server_segments
                        .insert(server_segment.to_string());
                } else {
                    filter
                        .exact_tool_names
                        .insert(format!("mcp.{server_segment}.{tool_segment}"));
                }
            }
        }
    }
    filter
}

pub(super) fn filter_mcp_inventory_snapshot_to_servers(
    snapshot: Value,
    allowed_servers: &[String],
) -> Value {
    let mut snapshot = snapshot;
    let allowed_servers = allowed_servers
        .iter()
        .map(|server| server.trim().to_string())
        .filter(|server| !server.is_empty())
        .collect::<HashSet<_>>();
    if allowed_servers.is_empty() {
        return snapshot;
    }
    let allowed_tool_prefixes = allowed_servers
        .iter()
        .map(|server| format!("mcp.{}.", mcp_namespace_segment(server)))
        .collect::<Vec<_>>();

    let keep_server = |name: &str| allowed_servers.contains(name);

    if let Some(root) = snapshot.as_object_mut() {
        retain_servers(root, keep_server);
        retain_tool_rows(root, |row| {
            row.get("server_name")
                .and_then(Value::as_str)
                .is_some_and(keep_server)
        });
        retain_registered_tools(root, |tool_name| {
            tool_name == "mcp_list"
                || allowed_tool_prefixes
                    .iter()
                    .any(|prefix| tool_name.starts_with(prefix))
        });
        retain_governed_tools(root, |row| {
            row.get("server_name")
                .and_then(Value::as_str)
                .is_some_and(keep_server)
        });
    }

    snapshot
}

pub(super) fn filter_mcp_snapshot_by_tool_scope(
    snapshot: Value,
    filter: &McpToolScopeFilter,
) -> Value {
    let mut snapshot = snapshot;
    if filter.wildcard_server_segments.is_empty() && filter.exact_tool_names.is_empty() {
        return snapshot;
    }

    if let Some(root) = snapshot.as_object_mut() {
        if let Some(Value::Array(rows)) = root.get_mut("servers") {
            rows.retain(|row| {
                let server_name = row.get("name").and_then(Value::as_str).unwrap_or("");
                let server_segment = mcp_namespace_segment(server_name);
                if filter.wildcard_server_segments.contains(&server_segment) {
                    return true;
                }
                row_tools(row).any(|tool_name| filter.exact_tool_names.contains(&tool_name))
            });
        }
        retain_server_names(root, |server| {
            let segment = mcp_namespace_segment(server);
            filter.wildcard_server_segments.contains(&segment)
                || filter
                    .exact_tool_names
                    .iter()
                    .any(|tool| tool.starts_with(&format!("mcp.{segment}.")))
        });
        retain_tool_rows(root, |row| {
            row.get("server_name")
                .and_then(Value::as_str)
                .is_some_and(|server| {
                    let segment = mcp_namespace_segment(server);
                    filter.wildcard_server_segments.contains(&segment)
                        || row
                            .get("namespaced_name")
                            .and_then(Value::as_str)
                            .is_some_and(|tool_name| filter.exact_tool_names.contains(tool_name))
                })
        });
        retain_registered_tools(root, |tool_name| {
            tool_name == "mcp_list"
                || filter.exact_tool_names.contains(tool_name)
                || filter
                    .wildcard_server_segments
                    .iter()
                    .any(|segment| tool_name.starts_with(&format!("mcp.{segment}.")))
        });
        retain_governed_tools(root, |row| {
            row.get("server_segment")
                .and_then(Value::as_str)
                .is_some_and(|segment| {
                    filter.wildcard_server_segments.contains(segment)
                        || row
                            .get("namespaced_name")
                            .and_then(Value::as_str)
                            .is_some_and(|tool_name| filter.exact_tool_names.contains(tool_name))
                })
        });
    }

    snapshot
}

/// Locate the security descriptor for an invoked `mcp.{server}.{tool}`
/// name in the live inventory snapshot. Returns `None` when the invocation
/// does not name a known MCP tool (built-in tools are not discovery-filtered
/// and stay on their existing policy paths).
/// Split an invoked tool name into `(server_segment, bare_tool)` when it
/// names an MCP tool (`mcp.{server}.{tool}`).
fn parse_mcp_invocation(invoked_tool: &str) -> Option<(&str, &str)> {
    let rest = invoked_tool.strip_prefix("mcp.")?;
    let (segment, bare_tool) = rest.split_once('.')?;
    if segment.is_empty() || bare_tool.is_empty() {
        return None;
    }
    Some((segment, bare_tool))
}

pub(crate) async fn mcp_tool_security_for_invocation(
    state: &AppState,
    invoked_tool: &str,
) -> Option<(String, Option<Value>)> {
    let (segment, bare_tool) = parse_mcp_invocation(invoked_tool)?;
    let snapshot = mcp_inventory_snapshot(state).await;
    let servers = snapshot.get("servers")?.as_array()?;
    for row in servers {
        let server_name = row.get("name").and_then(Value::as_str).unwrap_or("");
        if mcp_namespace_segment(server_name) != segment {
            continue;
        }
        let security = row
            .get("tool_security")
            .and_then(Value::as_object)
            .and_then(|map| map.get(bare_tool))
            .cloned();
        return Some((bare_tool.to_string(), security));
    }
    None
}

pub(super) fn filter_mcp_snapshot_by_discovery_authorization(
    snapshot: Value,
    strict_context: Option<&StrictTenantContext>,
    now_ms: u64,
) -> Value {
    if strict_context.is_none() {
        return snapshot;
    }
    let mut snapshot = snapshot;
    if let Some(root) = snapshot.as_object_mut() {
        let mut allowed_tools = HashSet::<String>::new();
        let mut allowed_server_segments = HashSet::<String>::new();

        if let Some(Value::Array(rows)) = root.get_mut("servers") {
            rows.retain_mut(|row| {
                let server_name = row.get("name").and_then(Value::as_str).unwrap_or("");
                let server_segment = mcp_namespace_segment(server_name);
                let tool_security = row
                    .get("tool_security")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();

                let mut server_allowed_tools = HashSet::<String>::new();
                for field in ["remote_tools", "registered_tools"] {
                    if let Some(Value::Array(tools)) = row.get_mut(field) {
                        tools.retain(|tool| {
                            let Some(tool_name) = tool_name_from_inventory_value(tool) else {
                                return false;
                            };
                            if tool_name == "mcp_list" {
                                return true;
                            }
                            let short_name = tool_name
                                .strip_prefix(&format!("mcp.{server_segment}."))
                                .unwrap_or(&tool_name);
                            let security = tool_security
                                .get(short_name)
                                .and_then(|value| value.get("security"));
                            let allowed = mcp_tool_authorized_for_discovery(
                                strict_context,
                                &tool_name,
                                security,
                                now_ms,
                            );
                            if allowed {
                                allowed_tools.insert(tool_name.clone());
                                server_allowed_tools.insert(tool_name);
                            }
                            allowed
                        });
                    }
                }

                let keep = !server_allowed_tools.is_empty();
                if keep {
                    allowed_server_segments.insert(server_segment);
                }
                keep
            });
        }

        retain_tool_rows(root, |row| {
            tool_name_from_inventory_value(row)
                .is_some_and(|tool_name| allowed_tools.contains(&tool_name))
        });
        retain_registered_tools(root, |tool_name| {
            tool_name == "mcp_list" || allowed_tools.contains(tool_name)
        });
        retain_server_names(root, |server| {
            allowed_server_segments.contains(&mcp_namespace_segment(server))
        });
        retain_governed_tools(root, |row| {
            row.get("namespaced_name")
                .and_then(Value::as_str)
                .is_some_and(|tool_name| allowed_tools.contains(tool_name))
        });
    }
    snapshot
}

pub(super) fn session_mcp_tool_filter(session_tools: &[String]) -> McpToolScopeFilter {
    parse_mcp_tool_scope_filter(session_tools)
}

/// Shared authorization decision for an MCP tool against a strict tenant
/// principal. Used by BOTH discovery filtering and execution-time
/// re-checking (EAA-02 / TAN-27): a tool hidden from the offered list is
/// denied with the exact same normalized descriptor and grant evaluation
/// when named directly by a hand-crafted or model-generated invocation.
pub(crate) fn mcp_tool_authorized_for_discovery(
    strict_context: Option<&StrictTenantContext>,
    tool_name: &str,
    security: Option<&Value>,
    now_ms: u64,
) -> bool {
    let Some(strict_context) = strict_context else {
        return true;
    };
    let descriptor = security
        .cloned()
        .and_then(|value| serde_json::from_value::<ToolSecurityDescriptor>(value).ok())
        .filter(|descriptor| !descriptor.is_empty())
        .unwrap_or_else(|| tool_name_security_descriptor(tool_name));

    if descriptor.is_empty() {
        return true;
    }
    if strict_context.is_expired_at(now_ms) {
        return false;
    }

    let required_permissions = if descriptor.required_permissions.is_empty() {
        vec![AccessPermission::View]
    } else {
        descriptor.required_permissions.clone()
    };
    let data_classes = if descriptor.data_classes.is_empty() {
        vec![DataClass::Internal]
    } else {
        descriptor.data_classes.clone()
    };
    let resource_kinds = if descriptor.resource_kinds.is_empty() {
        vec![ResourceKind::McpTool]
    } else {
        descriptor.resource_kinds.clone()
    };

    let risk_tier = tool_risk_tier_from_name_and_descriptor(tool_name, &descriptor);
    let hidden_by_default = matches!(descriptor.default_visibility, ToolDefaultVisibility::Hidden)
        || descriptor.admin_surface
        || descriptor.credential_access
        || risk_tier.hidden_without_grant_by_default();

    let all_permissions_allowed = required_permissions.iter().all(|permission| {
        resource_kinds.iter().any(|resource_kind| {
            let resource = mcp_tool_resource_ref(strict_context, *resource_kind, tool_name);
            data_classes.iter().any(|data_class| {
                matches!(
                    strict_context
                        .evaluate_access(&resource, *permission, *data_class, now_ms)
                        .decision,
                    AccessDecision::Allow
                )
            })
        })
    });

    if all_permissions_allowed {
        return true;
    }

    !hidden_by_default
        && required_permissions
            .iter()
            .all(|permission| matches!(permission, AccessPermission::View | AccessPermission::Read))
        && resource_kinds.iter().any(|resource_kind| {
            let resource = mcp_tool_resource_ref(strict_context, *resource_kind, tool_name);
            data_classes.iter().any(|data_class| {
                matches!(
                    strict_context
                        .evaluate_access(&resource, AccessPermission::Read, *data_class, now_ms)
                        .decision,
                    AccessDecision::Allow
                ) || matches!(
                    strict_context
                        .evaluate_access(&resource, AccessPermission::View, *data_class, now_ms)
                        .decision,
                    AccessDecision::Allow
                )
            })
        })
}

fn mcp_tool_resource_ref(
    strict_context: &StrictTenantContext,
    resource_kind: ResourceKind,
    tool_name: &str,
) -> ResourceRef {
    let resource_id = match resource_kind {
        ResourceKind::McpServer => mcp_server_segment_from_tool_name(tool_name),
        _ => tool_name.to_string(),
    };
    ResourceRef::new(
        strict_context.tenant_context.org_id.clone(),
        strict_context.tenant_context.workspace_id.clone(),
        resource_kind,
        resource_id,
    )
}

fn mcp_server_segment_from_tool_name(tool_name: &str) -> String {
    let mut parts = tool_name.split('.');
    match (parts.next(), parts.next()) {
        (Some("mcp"), Some(server)) if !server.trim().is_empty() => server.to_string(),
        _ => "mcp".to_string(),
    }
}

fn tool_name_from_inventory_value(value: &Value) -> Option<String> {
    value
        .as_str()
        .map(str::to_string)
        .or_else(|| {
            value
                .get("namespaced_name")
                .or_else(|| value.get("name"))
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn retain_servers(root: &mut serde_json::Map<String, Value>, keep: impl Fn(&str) -> bool) {
    if let Some(Value::Array(rows)) = root.get_mut("servers") {
        rows.retain(|row| row.get("name").and_then(Value::as_str).is_some_and(&keep));
    }
    retain_server_names(root, keep);
}

fn retain_server_names(root: &mut serde_json::Map<String, Value>, keep: impl Fn(&str) -> bool) {
    if let Some(Value::Array(rows)) = root.get_mut("connected_server_names") {
        rows.retain(|row| row.as_str().is_some_and(&keep));
    }
    if let Some(Value::Array(rows)) = root.get_mut("enabled_server_names") {
        rows.retain(|row| row.as_str().is_some_and(&keep));
    }
}

fn retain_tool_rows(root: &mut serde_json::Map<String, Value>, keep: impl Fn(&Value) -> bool) {
    if let Some(Value::Array(rows)) = root.get_mut("remote_tools") {
        rows.retain(|row| keep(row));
    }
}

fn retain_registered_tools(root: &mut serde_json::Map<String, Value>, keep: impl Fn(&str) -> bool) {
    if let Some(Value::Array(rows)) = root.get_mut("registered_tools") {
        rows.retain(|row| row.as_str().is_some_and(&keep));
    }
}

fn retain_governed_tools(root: &mut serde_json::Map<String, Value>, keep: impl Fn(&Value) -> bool) {
    if let Some(Value::Array(rows)) = root.get_mut("governed_tool_registry") {
        rows.retain(|row| keep(row));
    }
}

fn row_tools(row: &Value) -> impl Iterator<Item = String> + '_ {
    ["remote_tools", "registered_tools"]
        .into_iter()
        .filter_map(|field| row.get(field).and_then(Value::as_array))
        .flat_map(|tools| tools.iter().filter_map(tool_name_from_inventory_value))
}

fn mcp_namespace_segment(raw: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    let cleaned = out.trim_matches('_');
    if cleaned.is_empty() {
        "server".to_string()
    } else {
        cleaned.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_runtime::{McpOAuthConfig, PendingMcpAuth};
    use tandem_types::{AccessPermission, DataClass, ResourceKind, ToolRiskTier};

    fn test_server() -> McpServer {
        let mut secret_headers = HashMap::new();
        secret_headers.insert(
            "authorization".to_string(),
            McpSecretRef::Store {
                secret_id: "tenant-secret-1".to_string(),
                tenant_context: TenantContext::explicit_user_workspace(
                    "org-a",
                    "workspace-a",
                    None,
                    "actor-a",
                ),
            },
        );
        McpServer {
            name: "Enterprise Admin".to_string(),
            transport: "https://mcp.example.test".to_string(),
            auth_kind: "oauth".to_string(),
            enabled: true,
            connected: true,
            pid: None,
            last_error: None,
            last_auth_challenge: None,
            mcp_session_id: None,
            headers: HashMap::from([("x-static".to_string(), "redacted".to_string())]),
            secret_headers,
            tool_cache: Vec::new(),
            tools_fetched_at_ms: Some(42),
            pending_auth_by_tool: HashMap::<String, PendingMcpAuth>::new(),
            allowed_tools: None,
            purpose: String::new(),
            grounding_required: false,
            secret_header_values: HashMap::from([(
                "authorization".to_string(),
                "Bearer should-not-leak".to_string(),
            )]),
            oauth: Some(McpOAuthConfig {
                provider_id: "provider".to_string(),
                token_endpoint: "https://auth.example.test/token".to_string(),
                client_id: "client".to_string(),
                client_secret_ref: None,
                client_secret_value: Some("should-not-leak".to_string()),
            }),
        }
    }

    #[test]
    fn governed_metadata_hides_credential_admin_tools_and_redacts_values() {
        let server = test_server();
        let descriptor = ToolSecurityDescriptor::new()
            .permission(AccessPermission::Admin)
            .resource_kind(ResourceKind::SecretProviderCredential)
            .data_class(DataClass::Credential)
            .credential_access()
            .admin_surface()
            .hidden_by_default()
            .risk_tier(ToolRiskTier::CredentialAdmin);
        let security = serde_json::to_value(descriptor).unwrap();

        let metadata = governed_tool_registry_metadata(
            &server,
            "rotate_credential",
            "mcp.enterprise_admin.rotate_credential",
            &security,
        );

        assert_eq!(metadata["risk_tier"], "credential_admin");
        assert_eq!(metadata["default_access"], "hidden");
        assert_eq!(metadata["default_policy"], "hidden_without_grant");
        assert_eq!(metadata["owner"]["id"], "Enterprise Admin");
        assert_eq!(metadata["tenant_binding"]["org_id"], "org-a");
        assert_eq!(metadata["credential_binding"]["secret_ref_count"], 1);
        let rendered = metadata.to_string();
        assert!(!rendered.contains("should-not-leak"));
    }

    #[test]
    fn governed_metadata_gates_external_send_tools_by_default() {
        let server = test_server();
        let descriptor = ToolSecurityDescriptor::new()
            .permission(AccessPermission::Execute)
            .resource_kind(ResourceKind::McpTool)
            .data_class(DataClass::CustomerData)
            .external_side_effect()
            .risk_tier(ToolRiskTier::ExternalSend);
        let security = serde_json::to_value(descriptor).unwrap();

        let metadata = governed_tool_registry_metadata(
            &server,
            "send_message",
            "mcp.enterprise_admin.send_message",
            &security,
        );

        assert_eq!(metadata["risk_tier"], "external_send");
        assert_eq!(metadata["default_access"], "gated");
        assert_eq!(metadata["default_policy"], "approval_required");
        assert_eq!(metadata["approval_required_by_default"], true);
        assert_eq!(metadata["hidden_without_grant_by_default"], false);
    }

    #[test]
    fn discovery_hides_risk_tier_only_credential_admin_without_grant() {
        let tool_name = "mcp.enterprise_admin.rotate_credential";
        let resource = tandem_types::ResourceRef::new(
            "org-a",
            "workspace-a",
            tandem_types::ResourceKind::McpTool,
            tool_name,
        );
        let tenant_context =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "actor-a");
        let principal = tandem_types::PrincipalRef::human_user("actor-a");
        let grant = tandem_types::ScopedGrant::new(
            "grant-read-only",
            principal.clone(),
            resource.clone(),
            tandem_types::GrantSource::Direct,
        )
        .with_permissions(vec![tandem_types::AccessPermission::Read])
        .with_data_classes(vec![tandem_types::DataClass::Internal]);
        let strict_context = tandem_types::StrictTenantContext::new(
            tenant_context,
            principal.clone(),
            tandem_types::AuthorityChain::from_request(
                tandem_types::RequestPrincipal::authenticated_user(principal.id, "tandem-web"),
            ),
            tandem_types::ResourceScope::root(resource),
            tandem_types::AssertionMetadata::new(
                "tandem-web",
                "tandem-runtime",
                1_000,
                9_999_999_999_999,
                "assertion-risk-tier-only",
            ),
        )
        .with_grants(vec![grant]);
        let descriptor =
            ToolSecurityDescriptor::new().risk_tier(tandem_types::ToolRiskTier::CredentialAdmin);
        let security = serde_json::to_value(descriptor).unwrap();

        assert!(!mcp_tool_authorized_for_discovery(
            Some(&strict_context),
            tool_name,
            Some(&security),
            2_000,
        ));
    }

    // ── EAA-02 (TAN-27): execution-time authorization ───────────────────

    fn strict_context_with_permissions(
        permissions: Vec<tandem_types::AccessPermission>,
        tool_name: &str,
    ) -> tandem_types::StrictTenantContext {
        let resource = tandem_types::ResourceRef::new(
            "org-a",
            "workspace-a",
            tandem_types::ResourceKind::McpTool,
            tool_name,
        );
        let tenant_context =
            TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "actor-a");
        let principal = tandem_types::PrincipalRef::human_user("actor-a");
        let grant = tandem_types::ScopedGrant::new(
            "grant-exec",
            principal.clone(),
            resource.clone(),
            tandem_types::GrantSource::Direct,
        )
        .with_permissions(permissions)
        .with_data_classes(vec![
            tandem_types::DataClass::Internal,
            tandem_types::DataClass::Credential,
        ]);
        tandem_types::StrictTenantContext::new(
            tenant_context,
            principal.clone(),
            tandem_types::AuthorityChain::from_request(
                tandem_types::RequestPrincipal::authenticated_user(principal.id, "tandem-web"),
            ),
            tandem_types::ResourceScope::root(resource),
            tandem_types::AssertionMetadata::new(
                "tandem-web",
                "tandem-runtime",
                1_000,
                9_999_999_999_999,
                "assertion-execution",
            ),
        )
        .with_grants(vec![grant])
    }

    #[test]
    fn execution_authorization_is_the_same_decision_as_discovery() {
        // A discovery-hidden tool must be equally denied when named directly
        // by a hand-crafted invocation: both paths call the same function
        // with the same descriptor.
        let tool_name = "mcp.enterprise_admin.rotate_credential";
        let descriptor = ToolSecurityDescriptor::new()
            .permission(AccessPermission::Admin)
            .resource_kind(ResourceKind::McpTool)
            .data_class(DataClass::Internal)
            .admin_surface()
            .hidden_by_default()
            .risk_tier(ToolRiskTier::CredentialAdmin);
        let security = serde_json::to_value(descriptor).unwrap();

        let read_only =
            strict_context_with_permissions(vec![tandem_types::AccessPermission::Read], tool_name);
        assert!(
            !mcp_tool_authorized_for_discovery(Some(&read_only), tool_name, Some(&security), 2_000),
            "read-only principal must be denied execution of a hidden admin tool"
        );

        // The required Admin grant flips both discovery and execution to allow.
        let admin =
            strict_context_with_permissions(vec![tandem_types::AccessPermission::Admin], tool_name);
        assert!(
            mcp_tool_authorized_for_discovery(Some(&admin), tool_name, Some(&security), 2_000),
            "an explicit admin grant must authorize execution"
        );

        // Local/unscoped sessions are unchanged: no strict context, no denial.
        assert!(mcp_tool_authorized_for_discovery(
            None,
            tool_name,
            Some(&security),
            2_000
        ));
    }

    #[test]
    fn invocation_parsing_matches_only_mcp_tool_names() {
        assert_eq!(
            parse_mcp_invocation("mcp.github.create_issue"),
            Some(("github", "create_issue"))
        );
        // Built-in tools are not discovery-filtered and must not match the
        // execution-time re-check.
        assert_eq!(parse_mcp_invocation("read"), None);
        assert_eq!(parse_mcp_invocation("bash"), None);
        assert_eq!(parse_mcp_invocation("mcp."), None);
        assert_eq!(parse_mcp_invocation("mcp.github."), None);
        assert_eq!(parse_mcp_invocation("mcp..tool"), None);
    }
}
