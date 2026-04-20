use super::*;

async fn spawn_fake_notion_oauth_mcp_server() -> (String, tokio::task::JoinHandle<()>) {
    async fn handle(axum::Json(payload): axum::Json<Value>) -> axum::Json<Value> {
        let id = payload.get("id").cloned().unwrap_or_else(|| json!(1));
        let method = payload.get("method").and_then(Value::as_str).unwrap_or("");
        let response = match method {
            "initialize" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "serverInfo": {
                        "name": "fake-notion",
                        "version": "1.0.0"
                    }
                }
            }),
            "tools/list" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "tools": [
                        {
                            "name": "notion_search",
                            "description": "Search Notion",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "query": { "type": "string" }
                                }
                            }
                        }
                    ]
                }
            }),
            "tools/call" => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32001,
                    "message": "Authorization required",
                    "content": [
                        {
                            "type": "text",
                            "llm_instructions": "Authorize Notion access first.",
                            "authorization_url": "https://example.com/oauth/start"
                        }
                    ],
                    "structuredContent": {
                        "authorization_url": "https://example.com/oauth/start",
                        "message": "Authorize Notion access first."
                    }
                }
            }),
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": "Method not found"
                }
            }),
        };
        axum::Json(response)
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake notion mcp server");
    let endpoint = format!("http://{}/mcp", listener.local_addr().expect("local addr"));
    let app = axum::Router::new().route("/mcp", axum::routing::post(handle));
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve fake notion mcp server");
    });
    (endpoint, server)
}

#[tokio::test]
async fn mcp_list_returns_connected_inventory() {
    let state = test_state().await;

    let tool_names = state
        .tools
        .list()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .collect::<Vec<_>>();
    assert!(tool_names.iter().any(|name| name == "mcp_list"));

    let output = state
        .tools
        .execute("mcp_list", json!({}))
        .await
        .expect("execute mcp_list");
    let payload: Value = serde_json::from_str(&output.output).expect("inventory json");

    assert_eq!(
        payload.get("inventory_version").and_then(Value::as_u64),
        Some(1)
    );

    let servers = payload
        .get("servers")
        .and_then(Value::as_array)
        .expect("servers array");
    let github = servers
        .iter()
        .find(|row| row.get("name").and_then(Value::as_str) == Some("github"))
        .expect("github server row");
    assert_eq!(github.get("connected").and_then(Value::as_bool), Some(true));
    let remote_tools = github
        .get("remote_tools")
        .and_then(Value::as_array)
        .expect("remote tools array");
    assert!(!remote_tools.is_empty());
    assert_eq!(
        github.get("remote_tool_count").and_then(Value::as_u64),
        Some(remote_tools.len() as u64)
    );

    let connected_server_names = payload
        .get("connected_server_names")
        .and_then(Value::as_array)
        .expect("connected server names");
    assert!(connected_server_names
        .iter()
        .any(|server| server.as_str() == Some("github")));
}

#[tokio::test]
async fn mcp_list_filters_to_session_scoped_servers() {
    let state = test_state().await;

    state
        .mcp
        .add_or_update(
            "scoped-only".to_string(),
            "stdio".to_string(),
            std::collections::HashMap::new(),
            true,
        )
        .await;
    state
        .set_automation_v2_session_mcp_servers("automation-session-1", vec!["github".to_string()])
        .await;

    let unscoped = state
        .tools
        .execute("mcp_list", json!({}))
        .await
        .expect("execute unscoped mcp_list");
    let unscoped_payload: Value =
        serde_json::from_str(&unscoped.output).expect("unscoped inventory json");
    let unscoped_servers = unscoped_payload
        .get("servers")
        .and_then(Value::as_array)
        .expect("unscoped servers array");
    assert!(unscoped_servers
        .iter()
        .any(|row| row.get("name").and_then(Value::as_str) == Some("scoped-only")));

    let scoped = state
        .tools
        .execute(
            "mcp_list",
            json!({
                "__session_id": "automation-session-1"
            }),
        )
        .await
        .expect("execute scoped mcp_list");
    let payload: Value = serde_json::from_str(&scoped.output).expect("scoped inventory json");

    let servers = payload
        .get("servers")
        .and_then(Value::as_array)
        .expect("servers array");
    assert!(servers
        .iter()
        .all(|row| row.get("name").and_then(Value::as_str) == Some("github")));
    assert!(!servers
        .iter()
        .any(|row| row.get("name").and_then(Value::as_str) == Some("scoped-only")));

    let connected_server_names = payload
        .get("connected_server_names")
        .and_then(Value::as_array)
        .expect("connected server names");
    assert!(connected_server_names
        .iter()
        .all(|server| server.as_str() == Some("github")));

    let registered_tools = payload
        .get("registered_tools")
        .and_then(Value::as_array)
        .expect("registered tools");
    assert!(registered_tools
        .iter()
        .all(|tool| tool.as_str() == Some("mcp_list")
            || tool
                .as_str()
                .is_some_and(|name| name.starts_with("mcp.github."))));
}

#[tokio::test]
async fn mcp_inventory_preserves_oauth_auth_challenges() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_notion_oauth_mcp_server().await;

    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;

    let tools = state.mcp.refresh("notion").await.expect("refresh notion");
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].tool_name, "notion_search");

    let result = state
        .mcp
        .call_tool("notion", "notion_search", json!({"query": "workspace"}))
        .await
        .expect("call notion tool");
    assert!(result.output.contains("Authorize here:"));

    let listed = state.mcp.list().await;
    let server_row = listed.get("notion").expect("notion server row");
    let challenge = server_row
        .last_auth_challenge
        .as_ref()
        .expect("auth challenge should be preserved");
    assert_eq!(challenge.tool_name, "notion_search");
    assert_eq!(
        challenge.authorization_url,
        "https://example.com/oauth/start"
    );
    assert!(server_row
        .pending_auth_by_tool
        .contains_key("notion_search"));

    let output = state
        .tools
        .execute("mcp_list", json!({}))
        .await
        .expect("execute mcp_list");
    let payload: Value = serde_json::from_str(&output.output).expect("inventory json");
    let servers = payload
        .get("servers")
        .and_then(Value::as_array)
        .expect("servers array");
    let notion = servers
        .iter()
        .find(|row| row.get("name").and_then(Value::as_str) == Some("notion"))
        .expect("notion server row");
    assert_eq!(
        notion
            .get("last_auth_challenge")
            .and_then(|v| v.get("authorization_url"))
            .and_then(Value::as_str),
        Some("https://example.com/oauth/start")
    );
    let pending_auth_tools = notion
        .get("pending_auth_tools")
        .and_then(Value::as_array)
        .expect("pending auth tools array");
    assert!(pending_auth_tools
        .iter()
        .any(|tool| tool.as_str() == Some("notion_search")));

    drop(server);
}

#[tokio::test]
async fn mcp_authenticate_clears_pending_oauth_challenge() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_notion_oauth_mcp_server().await;

    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;

    let _ = state.mcp.refresh("notion").await.expect("initial refresh");
    let result = state
        .mcp
        .call_tool("notion", "notion_search", json!({"query": "workspace"}))
        .await
        .expect("call notion tool");
    assert!(result.output.contains("Authorize here:"));

    let Json(connected_payload) = authenticate_mcp(
        axum::extract::State(state.clone()),
        axum::extract::Path("notion".to_string()),
    )
    .await;
    assert!(connected_payload
        .get("ok")
        .and_then(Value::as_bool)
        .unwrap_or(false));
    assert!(connected_payload
        .get("authenticated")
        .and_then(Value::as_bool)
        .unwrap_or(false));
    assert!(connected_payload
        .get("connected")
        .and_then(Value::as_bool)
        .unwrap_or(false));
    assert!(connected_payload
        .get("pendingAuth")
        .and_then(Value::as_bool)
        .is_some_and(|value| !value));
    assert!(connected_payload
        .get("lastAuthChallenge")
        .is_some_and(|value| value.is_null()));

    let listed = state.mcp.list().await;
    let server_row = listed.get("notion").expect("notion server row");
    assert!(server_row.connected);
    assert!(server_row.last_auth_challenge.is_none());
    assert!(server_row.pending_auth_by_tool.is_empty());

    drop(server);
}
