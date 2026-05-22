use serde_json::{json, Value};
use std::fs;
use std::path::Path;
use std::sync::OnceLock;
use tandem_core::tool_name_security_descriptor;
use tandem_types::{AccessPermission, DataClass, ResourceKind, ToolSecurityDescriptor};

const MCP_TOOL_SECURITY_OVERRIDES_PATH_ENV: &str = "TANDEM_MCP_TOOL_SECURITY_OVERRIDES_PATH";

mod generated {
    include!("mcp_catalog_generated.rs");
}

fn normalize_slug(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

pub fn index() -> Option<&'static Value> {
    static INDEX: OnceLock<Option<Value>> = OnceLock::new();
    INDEX
        .get_or_init(|| {
            serde_json::from_str::<Value>(generated::INDEX_JSON)
                .ok()
                .map(apply_operator_security_overrides)
                .map(augment_catalog_security)
        })
        .as_ref()
}

pub fn toml_for_slug(slug: &str) -> Option<&'static str> {
    let normalized = normalize_slug(slug);
    if normalized.is_empty() {
        return None;
    }
    generated::SERVERS
        .iter()
        .find(|(entry_slug, _)| *entry_slug == normalized)
        .map(|(_, toml)| *toml)
}

fn apply_operator_security_overrides(mut catalog: Value) -> Value {
    let Some(overrides) = load_operator_security_overrides() else {
        return catalog;
    };
    apply_operator_security_overrides_value(&mut catalog, &overrides);
    catalog
}

fn load_operator_security_overrides() -> Option<Value> {
    let path = std::env::var(MCP_TOOL_SECURITY_OVERRIDES_PATH_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let path = Path::new(&path);
    let raw = fs::read_to_string(path).ok()?;
    parse_operator_security_overrides(&raw)
}

fn parse_operator_security_overrides(raw: &str) -> Option<Value> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    serde_json::from_str::<Value>(trimmed)
        .ok()
        .or_else(|| serde_yaml::from_str::<Value>(trimmed).ok())
        .filter(Value::is_object)
}

fn apply_operator_security_overrides_value(catalog: &mut Value, overrides: &Value) {
    let Some(servers) = catalog.get_mut("servers").and_then(Value::as_array_mut) else {
        return;
    };
    let server_overrides = overrides
        .get("servers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let namespaced_tool_overrides = overrides
        .get("tools")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    for server in servers {
        let server_keys = catalog_entry_match_keys(server);
        let matched_server_override = server_keys
            .iter()
            .find_map(|key| server_overrides.get(key).cloned());
        if let Some(server_override) = matched_server_override {
            apply_server_security_override(server, &server_override);
        }
        apply_namespaced_tool_security_overrides(server, &namespaced_tool_overrides);
    }
}

fn apply_server_security_override(server: &mut Value, server_override: &Value) {
    let Some(server_obj) = server.as_object_mut() else {
        return;
    };
    if let Some(security) = server_override.get("security") {
        server_obj.insert("security".to_string(), security.clone());
    }

    let mut tool_overrides = server_obj
        .get("tool_security_overrides")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    if let Some(tools) = server_override
        .get("tools")
        .or_else(|| server_override.get("tool_security_overrides"))
        .and_then(Value::as_object)
    {
        for (tool_name, descriptor) in tools {
            tool_overrides.insert(tool_name.clone(), descriptor.clone());
        }
    }
    if !tool_overrides.is_empty() {
        server_obj.insert(
            "tool_security_overrides".to_string(),
            Value::Object(tool_overrides),
        );
    }
}

fn apply_namespaced_tool_security_overrides(
    server: &mut Value,
    namespaced_tool_overrides: &serde_json::Map<String, Value>,
) {
    if namespaced_tool_overrides.is_empty() {
        return;
    }
    let Some(server_obj) = server.as_object_mut() else {
        return;
    };
    let server_segment = server_namespace_segment_from_obj(server_obj);
    let tool_names = server_obj
        .get("tool_names")
        .and_then(Value::as_array)
        .map(|names| {
            names
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|tool_name| !tool_name.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let mut tool_overrides = server_obj
        .get("tool_security_overrides")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    for tool_name in tool_names {
        let namespaced_name = format!("mcp.{server_segment}.{tool_name}");
        if let Some(descriptor) = namespaced_tool_overrides.get(&namespaced_name) {
            tool_overrides.insert(tool_name, descriptor.clone());
        }
    }
    if !tool_overrides.is_empty() {
        server_obj.insert(
            "tool_security_overrides".to_string(),
            Value::Object(tool_overrides),
        );
    }
}

fn augment_catalog_security(mut catalog: Value) -> Value {
    if let Some(servers) = catalog.get_mut("servers").and_then(Value::as_array_mut) {
        for server in servers {
            augment_server_security(server);
        }
    }
    catalog
}

fn augment_server_security(server: &mut Value) {
    let base_descriptor = explicit_security_descriptor(server.get("security"))
        .unwrap_or_else(|| inferred_server_security_descriptor(server));
    let tool_overrides = server
        .get("tool_security_overrides")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let server_segment = server_namespace_segment(server);

    if let Some(obj) = server.as_object_mut() {
        obj.insert(
            "security".to_string(),
            serde_json::to_value(&base_descriptor).unwrap_or_else(|_| Value::Null),
        );

        let tool_names = obj
            .get("tool_names")
            .and_then(Value::as_array)
            .map(|names| {
                names
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::trim)
                    .filter(|tool_name| !tool_name.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut tool_security = serde_json::Map::new();
        for tool_name in tool_names {
            let namespaced_name = format!("mcp.{server_segment}.{tool_name}");
            let descriptor = tool_overrides
                .get(&tool_name)
                .and_then(|value| explicit_security_descriptor(Some(value)))
                .unwrap_or_else(|| {
                    let inferred = tool_name_security_descriptor(&namespaced_name);
                    if inferred.is_empty() {
                        base_descriptor.clone()
                    } else {
                        merge_server_tool_security(&base_descriptor, inferred)
                    }
                });
            tool_security.insert(
                tool_name.clone(),
                json!({
                    "tool_name": tool_name,
                    "namespaced_name": namespaced_name,
                    "security": descriptor,
                }),
            );
        }
        obj.insert("tool_security".to_string(), Value::Object(tool_security));
    }
}

fn explicit_security_descriptor(value: Option<&Value>) -> Option<ToolSecurityDescriptor> {
    value
        .cloned()
        .and_then(|value| serde_json::from_value::<ToolSecurityDescriptor>(value).ok())
        .filter(|descriptor| !descriptor.is_empty())
}

fn inferred_server_security_descriptor(server: &Value) -> ToolSecurityDescriptor {
    let mut descriptor = ToolSecurityDescriptor::new()
        .permission(AccessPermission::View)
        .permission(AccessPermission::Read)
        .resource_kind(ResourceKind::ExternalIntegrationAccount)
        .resource_kind(ResourceKind::McpServer)
        .resource_kind(ResourceKind::McpTool)
        .data_class(DataClass::Internal);

    let tokens = catalog_server_tokens(server);
    if tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "github" | "gitlab" | "repository" | "repositories" | "repo" | "repos" | "devtools"
        )
    }) {
        descriptor = descriptor
            .resource_kind(ResourceKind::Repository)
            .data_class(DataClass::SourceCode);
    }

    if tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "slack"
                | "gmail"
                | "email"
                | "outlook"
                | "mail"
                | "intercom"
                | "hubspot"
                | "customer"
                | "crm"
        )
    }) {
        descriptor = descriptor
            .resource_kind(ResourceKind::DocumentCollection)
            .data_class(DataClass::Confidential)
            .data_class(DataClass::CustomerData);
    }

    if tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "notion" | "confluence" | "docs" | "knowledge" | "document" | "documents" | "drive"
        )
    }) {
        descriptor = descriptor
            .resource_kind(ResourceKind::KnowledgeSpace)
            .resource_kind(ResourceKind::DocumentCollection)
            .resource_kind(ResourceKind::Document)
            .data_class(DataClass::Confidential);
    }

    if tokens.iter().any(|token| {
        matches!(
            token.as_str(),
            "stripe"
                | "ramp"
                | "paypal"
                | "mercury"
                | "netsuite"
                | "quickbooks"
                | "finance"
                | "financial"
                | "bank"
                | "banking"
        )
    }) {
        descriptor = descriptor
            .data_class(DataClass::FinancialRecord)
            .data_class(DataClass::Regulated);
    }

    descriptor
}

fn merge_server_tool_security(
    server: &ToolSecurityDescriptor,
    tool: ToolSecurityDescriptor,
) -> ToolSecurityDescriptor {
    let mut merged = tool;
    for resource_kind in &server.resource_kinds {
        merged = merged.resource_kind(*resource_kind);
    }
    for data_class in &server.data_classes {
        merged = merged.data_class(*data_class);
    }
    if server.admin_surface {
        merged = merged.admin_surface();
    }
    if server.credential_access {
        merged = merged.credential_access();
    }
    merged
}

fn server_namespace_segment(server: &Value) -> String {
    if let Some(obj) = server.as_object() {
        return server_namespace_segment_from_obj(obj);
    }
    "mcp".to_string()
}

fn server_namespace_segment_from_obj(server: &serde_json::Map<String, Value>) -> String {
    for field in ["server_config_name", "slug", "name"] {
        if let Some(value) = server.get(field).and_then(Value::as_str) {
            let normalized = normalize_namespace_segment(value);
            if !normalized.is_empty() {
                return normalized;
            }
        }
    }
    "mcp".to_string()
}

fn catalog_entry_match_keys(entry: &Value) -> Vec<String> {
    let mut keys = Vec::new();
    for field in [
        "slug",
        "name",
        "server_name",
        "server_config_name",
        "pack_id",
    ] {
        if let Some(value) = entry.get(field).and_then(Value::as_str) {
            let normalized = normalize_slug(value);
            if !normalized.is_empty() {
                keys.push(normalized);
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

fn normalize_namespace_segment(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '_' })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn catalog_server_tokens(server: &Value) -> Vec<String> {
    let mut text = String::new();
    for field in [
        "slug",
        "name",
        "description",
        "server_name",
        "server_config_name",
        "pack_id",
    ] {
        if let Some(value) = server.get(field).and_then(Value::as_str) {
            text.push(' ');
            text.push_str(value);
        }
    }
    for field in ["use_cases", "tool_names"] {
        if let Some(values) = server.get(field).and_then(Value::as_array) {
            for value in values {
                if let Some(value) = value.as_str() {
                    text.push(' ');
                    text.push_str(value);
                }
            }
        }
    }
    text.to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn catalog_security_derives_server_and_tool_metadata() {
        let catalog = augment_catalog_security(json!({
            "servers": [{
                "slug": "slack",
                "name": "Slack",
                "description": "Send messages and fetch Slack data",
                "server_config_name": "slack",
                "tool_names": ["slack_send_message", "slack_search_public_and_private"]
            }]
        }));

        let server = &catalog["servers"][0];
        assert_eq!(
            server["security"]["data_classes"],
            json!(["internal", "confidential", "customer_data"])
        );
        assert_eq!(
            server["tool_security"]["slack_send_message"]["namespaced_name"],
            "mcp.slack.slack_send_message"
        );
        assert_eq!(
            server["tool_security"]["slack_send_message"]["security"]["required_permissions"],
            json!(["execute"])
        );
        assert_eq!(
            server["tool_security"]["slack_send_message"]["security"]["external_side_effect"],
            true
        );
    }

    #[test]
    fn catalog_security_honors_explicit_overrides() {
        let catalog = augment_catalog_security(json!({
            "servers": [{
                "slug": "custom-admin",
                "name": "Custom Admin",
                "server_config_name": "custom_admin",
                "tool_names": ["safe_alias"],
                "security": {
                    "required_permissions": ["read"],
                    "resource_kinds": ["document"],
                    "data_classes": ["internal"]
                },
                "tool_security_overrides": {
                    "safe_alias": {
                        "required_permissions": ["admin", "execute"],
                        "resource_kinds": ["mcp_tool", "secret_provider_credential"],
                        "data_classes": ["credential"],
                        "admin_surface": true,
                        "credential_access": true,
                        "default_visibility": "hidden"
                    }
                }
            }]
        }));

        let security = &catalog["servers"][0]["tool_security"]["safe_alias"]["security"];
        assert_eq!(
            security["required_permissions"],
            json!(["admin", "execute"])
        );
        assert_eq!(
            security["resource_kinds"],
            json!(["mcp_tool", "secret_provider_credential"])
        );
        assert_eq!(security["data_classes"], json!(["credential"]));
        assert_eq!(security["admin_surface"], true);
        assert_eq!(security["credential_access"], true);
        assert_eq!(security["default_visibility"], "hidden");
    }

    #[test]
    fn operator_security_overrides_apply_by_server_and_namespaced_tool() {
        let mut catalog = json!({
            "servers": [
                {
                    "slug": "slack",
                    "name": "Slack",
                    "server_config_name": "slack",
                    "tool_names": ["slack_send_message", "slack_search_public_and_private"]
                },
                {
                    "slug": "notion",
                    "name": "Notion",
                    "server_config_name": "notion",
                    "tool_names": ["search"]
                }
            ]
        });
        let overrides = json!({
            "servers": {
                "slack": {
                    "security": {
                        "required_permissions": ["read"],
                        "resource_kinds": ["document_collection"],
                        "data_classes": ["confidential"]
                    },
                    "tools": {
                        "slack_send_message": {
                            "required_permissions": ["admin", "execute"],
                            "resource_kinds": ["mcp_tool"],
                            "data_classes": ["confidential"],
                            "admin_surface": true,
                            "default_visibility": "hidden"
                        }
                    }
                }
            },
            "tools": {
                "mcp.notion.search": {
                    "required_permissions": ["read"],
                    "resource_kinds": ["knowledge_space"],
                    "data_classes": ["internal"]
                }
            }
        });

        apply_operator_security_overrides_value(&mut catalog, &overrides);
        let catalog = augment_catalog_security(catalog);

        let slack_security = &catalog["servers"][0]["security"];
        assert_eq!(slack_security["data_classes"], json!(["confidential"]));
        let send_security =
            &catalog["servers"][0]["tool_security"]["slack_send_message"]["security"];
        assert_eq!(
            send_security["required_permissions"],
            json!(["admin", "execute"])
        );
        assert_eq!(send_security["admin_surface"], true);
        assert_eq!(send_security["default_visibility"], "hidden");

        let notion_security = &catalog["servers"][1]["tool_security"]["search"]["security"];
        assert_eq!(
            notion_security["resource_kinds"],
            json!(["knowledge_space"])
        );
        assert_eq!(notion_security["data_classes"], json!(["internal"]));
    }

    #[test]
    fn operator_security_overrides_parse_json_and_yaml() {
        let json_payload = r#"{
            "servers": {
                "slack": {
                    "security": {
                        "required_permissions": ["read"]
                    }
                }
            }
        }"#;
        let yaml_payload = r#"
servers:
  slack:
    security:
      required_permissions:
        - read
"#;

        assert!(parse_operator_security_overrides(json_payload).is_some());
        assert!(parse_operator_security_overrides(yaml_payload).is_some());
        assert!(parse_operator_security_overrides("").is_none());
    }
}
