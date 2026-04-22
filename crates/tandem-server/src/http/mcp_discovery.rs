use super::*;
use tandem_types::RequestPrincipal;

#[derive(Debug, Deserialize)]
pub(super) struct McpCapabilityRequestInput {
    pub agent_id: String,
    pub mcp_name: String,
    pub rationale: String,
    #[serde(default)]
    pub catalog_slug: Option<String>,
    #[serde(default)]
    pub requested_tools: Option<Vec<String>>,
    #[serde(default)]
    pub context: Option<Value>,
    #[serde(default)]
    pub expires_at_ms: Option<u64>,
}

fn normalize_catalog_match_key(raw: &str) -> String {
    raw.trim()
        .to_ascii_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
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
            let normalized = normalize_catalog_match_key(value);
            if !normalized.is_empty() {
                keys.push(normalized);
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

fn inventory_server_match_keys(server: &Value) -> Vec<String> {
    let mut keys = Vec::new();
    for field in ["name", "transport"] {
        if let Some(value) = server.get(field).and_then(Value::as_str) {
            let normalized = normalize_catalog_match_key(value);
            if !normalized.is_empty() {
                keys.push(normalized);
            }
        }
    }
    keys.sort();
    keys.dedup();
    keys
}

fn annotate_catalog_entry(entry: &Value, inventory: Option<&Value>) -> Value {
    let mut annotated = entry.clone();
    let (enabled, connected, server_name, last_error, pending_auth_tools, registered_tool_count) =
        if let Some(inventory) = inventory {
            let enabled = inventory
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let connected = inventory
                .get("connected")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let server_name = inventory
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let last_error = inventory.get("last_error").cloned().unwrap_or(Value::Null);
            let pending_auth_tools = inventory
                .get("pending_auth_tools")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let registered_tool_count = inventory
                .get("registered_tool_count")
                .and_then(Value::as_u64)
                .unwrap_or(0);
            (
                enabled,
                connected,
                server_name,
                last_error,
                pending_auth_tools,
                registered_tool_count,
            )
        } else {
            (false, false, String::new(), Value::Null, json!([]), 0)
        };
    let connection_status = match (enabled, connected, inventory.is_some()) {
        (true, true, true) => "connected_enabled",
        (false, true, true) => "connected_disabled",
        (false, false, true) => "cataloged_disabled",
        (true, false, true) => "cataloged_not_connected",
        (_, _, false) => "cataloged_not_connected",
    };
    if let Some(obj) = annotated.as_object_mut() {
        obj.insert("enabled".to_string(), Value::Bool(enabled));
        obj.insert("connected".to_string(), Value::Bool(connected));
        obj.insert(
            "connection_status".to_string(),
            Value::String(connection_status.to_string()),
        );
        if server_name.is_empty() {
            obj.insert("matched_server_name".to_string(), Value::Null);
        } else {
            obj.insert(
                "matched_server_name".to_string(),
                Value::String(server_name),
            );
        }
        obj.insert("last_error".to_string(), last_error);
        obj.insert("pending_auth_tools".to_string(), pending_auth_tools);
        obj.insert(
            "registered_tool_count".to_string(),
            Value::Number(serde_json::Number::from(registered_tool_count)),
        );
        obj.insert(
            "execution_available".to_string(),
            Value::Bool(enabled && connected),
        );
    }
    annotated
}

pub(crate) async fn mcp_catalog_overlay_snapshot(state: &AppState) -> Option<Value> {
    let mut catalog = mcp_catalog::index()?.clone();
    let inventory = super::mcp::mcp_inventory_snapshot(state).await;
    let inventory_servers = inventory
        .get("servers")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut inventory_by_key: HashMap<String, Value> = HashMap::new();
    let mut matched_inventory_names = std::collections::HashSet::<String>::new();
    for server in &inventory_servers {
        for key in inventory_server_match_keys(server) {
            inventory_by_key.insert(key, server.clone());
        }
    }

    let mut annotated_servers = Vec::new();
    let mut connected_catalog_server_names = Vec::new();
    let mut connected_disabled_server_names = Vec::new();
    let mut cataloged_not_connected_server_names = Vec::new();
    let mut cataloged_disabled_server_names = Vec::new();
    let mut uncataloged_connected_servers = Vec::new();

    if let Some(servers) = catalog.get("servers").and_then(Value::as_array).cloned() {
        for entry in servers {
            let matched_inventory = catalog_entry_match_keys(&entry)
                .into_iter()
                .find_map(|key| inventory_by_key.get(&key).cloned());
            if let Some(ref inventory_row) = matched_inventory {
                if let Some(name) = inventory_row.get("name").and_then(Value::as_str) {
                    matched_inventory_names.insert(normalize_catalog_match_key(name));
                }
            }

            let annotated = annotate_catalog_entry(&entry, matched_inventory.as_ref());
            let catalog_name = entry
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            match annotated
                .get("connection_status")
                .and_then(Value::as_str)
                .unwrap_or("cataloged_not_connected")
            {
                "connected_enabled" => connected_catalog_server_names.push(catalog_name),
                "connected_disabled" => connected_disabled_server_names.push(catalog_name),
                "cataloged_disabled" => cataloged_disabled_server_names.push(catalog_name),
                _ => cataloged_not_connected_server_names.push(catalog_name),
            }
            annotated_servers.push(annotated);
        }
    }

    for server in &inventory_servers {
        let connected = server
            .get("connected")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !connected {
            continue;
        }
        let matched = inventory_server_match_keys(server)
            .iter()
            .any(|key| matched_inventory_names.contains(key));
        if matched {
            continue;
        }
        let mut row = server.clone();
        if let Some(obj) = row.as_object_mut() {
            obj.insert(
                "connection_status".to_string(),
                Value::String("uncataloged_connected".to_string()),
            );
        }
        uncataloged_connected_servers.push(row);
    }

    let status_counts = json!({
        "connected_enabled": connected_catalog_server_names.len(),
        "connected_disabled": connected_disabled_server_names.len(),
        "cataloged_not_connected": cataloged_not_connected_server_names.len(),
        "cataloged_disabled": cataloged_disabled_server_names.len(),
        "uncataloged_connected": uncataloged_connected_servers.len(),
    });

    if let Some(obj) = catalog.as_object_mut() {
        obj.insert("servers".to_string(), Value::Array(annotated_servers));
        obj.insert(
            "overlay".to_string(),
            json!({
                "inventory_version": inventory.get("inventory_version").cloned().unwrap_or_else(|| json!(1)),
                "connected_server_names": inventory.get("connected_server_names").cloned().unwrap_or_else(|| json!([])),
                "enabled_server_names": inventory.get("enabled_server_names").cloned().unwrap_or_else(|| json!([])),
                "connected_catalog_server_names": connected_catalog_server_names,
                "connected_disabled_server_names": connected_disabled_server_names,
                "cataloged_not_connected_server_names": cataloged_not_connected_server_names,
                "cataloged_disabled_server_names": cataloged_disabled_server_names,
                "uncataloged_connected_servers": uncataloged_connected_servers,
                "status_counts": status_counts,
            }),
        );
    }
    Some(catalog)
}

async fn mcp_request_capability_impl(
    state: &AppState,
    requested_by: crate::automation_v2::governance::GovernanceActorRef,
    input: McpCapabilityRequestInput,
) -> anyhow::Result<Value> {
    if !state.premium_governance_enabled() {
        anyhow::bail!(
            "PREMIUM_FEATURE_REQUIRED: premium governance is not available in this build"
        );
    }
    let agent_id = input.agent_id.trim().to_string();
    if agent_id.is_empty() {
        anyhow::bail!("agent_id is required");
    }
    let mcp_name = input.mcp_name.trim().to_string();
    if mcp_name.is_empty() {
        anyhow::bail!("mcp_name is required");
    }
    let rationale = input.rationale.trim().to_string();
    if rationale.is_empty() {
        anyhow::bail!("rationale is required");
    }
    let catalog = mcp_catalog_overlay_snapshot(state).await;
    let requested_slug = input
        .catalog_slug
        .as_deref()
        .map(normalize_catalog_match_key)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| normalize_catalog_match_key(&mcp_name));
    let catalog_entry = catalog.as_ref().and_then(|catalog| {
        catalog
            .get("servers")
            .and_then(Value::as_array)
            .and_then(|servers| {
                servers.iter().find(|server| {
                    catalog_entry_match_keys(server)
                        .iter()
                        .any(|key| key == &requested_slug)
                })
            })
            .cloned()
    });
    let approval_context = json!({
        "agent_id": agent_id,
        "mcp_name": mcp_name,
        "catalog_slug": input.catalog_slug,
        "requested_tools": input.requested_tools,
        "catalog_match": catalog_entry.is_some(),
        "catalog_entry": catalog_entry,
        "capability_key": format!("mcp:{}", requested_slug),
        "context": input.context,
    });
    let request = state
        .request_approval(
            crate::automation_v2::governance::GovernanceApprovalRequestType::CapabilityRequest,
            requested_by.clone(),
            crate::automation_v2::governance::GovernanceResourceRef {
                resource_type: "agent".to_string(),
                id: agent_id,
            },
            rationale,
            approval_context,
            input.expires_at_ms,
        )
        .await
        .map_err(anyhow::Error::msg)?;
    let approval = super::governance::approval_request_wire(&request);
    Ok(json!({
        "ok": true,
        "approval": approval,
        "requested_by": requested_by,
    }))
}

#[derive(Clone)]
pub(crate) struct McpListCatalogTool {
    state: AppState,
}

impl McpListCatalogTool {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Tool for McpListCatalogTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "mcp_list_catalog",
            "List the embedded MCP catalog with connection overlay for discovery and gap analysis",
            json!({
                "type": "object",
                "properties": {},
                "additionalProperties": false,
            }),
        )
    }

    async fn execute(&self, _args: Value) -> anyhow::Result<ToolResult> {
        let Some(snapshot) = mcp_catalog_overlay_snapshot(&self.state).await else {
            anyhow::bail!("MCP catalog is unavailable");
        };
        let output =
            serde_json::to_string_pretty(&snapshot).unwrap_or_else(|_| snapshot.to_string());
        Ok(ToolResult {
            output,
            metadata: snapshot,
        })
    }
}

#[derive(Clone)]
pub(crate) struct McpRequestCapabilityTool {
    state: AppState,
}

impl McpRequestCapabilityTool {
    pub fn new(state: AppState) -> Self {
        Self { state }
    }
}

#[async_trait]
impl Tool for McpRequestCapabilityTool {
    fn schema(&self) -> ToolSchema {
        ToolSchema::new(
            "mcp_request_capability",
            "Request human approval for an MCP capability gap without attempting to connect or execute it directly",
            json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string" },
                    "mcp_name": { "type": "string" },
                    "catalog_slug": { "type": "string" },
                    "rationale": { "type": "string" },
                    "requested_tools": {
                        "type": "array",
                        "items": { "type": "string" }
                    },
                    "context": { "type": "object" },
                    "expires_at_ms": { "type": "integer" }
                },
                "required": ["agent_id", "mcp_name", "rationale"],
                "additionalProperties": false
            }),
        )
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        let input: McpCapabilityRequestInput = serde_json::from_value(args.clone())
            .map_err(|error| anyhow::anyhow!("invalid capability request input: {error}"))?;
        let session_source = args
            .get("__session_id")
            .and_then(Value::as_str)
            .map(|value| format!("tool:{value}"))
            .unwrap_or_else(|| "tool:mcp_request_capability".to_string());
        let requested_by = crate::automation_v2::governance::GovernanceActorRef::agent(
            Some(input.agent_id.clone()),
            session_source,
        );
        let output = mcp_request_capability_impl(&self.state, requested_by, input).await?;
        Ok(ToolResult {
            output: serde_json::to_string_pretty(&output).unwrap_or_else(|_| output.to_string()),
            metadata: output,
        })
    }
}

pub(super) async fn mcp_catalog_index(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    if let Some(index) = mcp_catalog_overlay_snapshot(&state).await {
        return Ok(Json(index));
    }
    Err(StatusCode::SERVICE_UNAVAILABLE)
}

pub(super) async fn mcp_request_capability(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Extension(request_principal): Extension<RequestPrincipal>,
    headers: HeaderMap,
    Json(input): Json<McpCapabilityRequestInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let requested_by =
        super::governance::resolve_governance_actor(&headers, &tenant_context, &request_principal);
    let response = mcp_request_capability_impl(&state, requested_by, input)
        .await
        .map_err(|error| {
            let message = error.to_string();
            let (status, code) = if message.contains("PREMIUM_FEATURE_REQUIRED") {
                (StatusCode::NOT_IMPLEMENTED, "PREMIUM_FEATURE_REQUIRED")
            } else if message.contains("is required") {
                (StatusCode::BAD_REQUEST, "MCP_CAPABILITY_REQUEST_FAILED")
            } else {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "MCP_CAPABILITY_REQUEST_FAILED",
                )
            };
            (
                status,
                Json(json!({
                    "error": message,
                    "code": code,
                })),
            )
        })?;
    Ok(Json(response))
}

pub(super) async fn mcp_catalog_toml(Path(slug): Path<String>) -> Result<Response, StatusCode> {
    if let Some(toml) = mcp_catalog::toml_for_slug(&slug) {
        return Ok((
            [(header::CONTENT_TYPE, "application/toml; charset=utf-8")],
            toml,
        )
            .into_response());
    }
    Err(StatusCode::NOT_FOUND)
}
