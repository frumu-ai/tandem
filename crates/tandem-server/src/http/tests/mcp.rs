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

#[derive(Clone)]
struct FakeHostedMcpOauthState {
    base_url: String,
    valid_access_token: std::sync::Arc<tokio::sync::RwLock<String>>,
}

async fn spawn_fake_hosted_mcp_oauth_server() -> (String, tokio::task::JoinHandle<()>) {
    async fn protected_resource(
        axum::extract::State(state): axum::extract::State<FakeHostedMcpOauthState>,
    ) -> axum::Json<Value> {
        axum::Json(json!({
            "resource": format!("{}/mcp", state.base_url),
            "authorization_servers": [state.base_url],
            "bearer_methods_supported": ["header"],
            "resource_name": "Fake Notion MCP"
        }))
    }

    async fn authorization_server(
        axum::extract::State(state): axum::extract::State<FakeHostedMcpOauthState>,
    ) -> axum::Json<Value> {
        axum::Json(json!({
            "issuer": state.base_url,
            "authorization_endpoint": format!("{}/authorize", state.base_url),
            "token_endpoint": format!("{}/token", state.base_url),
            "registration_endpoint": format!("{}/register", state.base_url),
            "response_types_supported": ["code"],
            "grant_types_supported": ["authorization_code", "refresh_token"],
            "token_endpoint_auth_methods_supported": ["none"],
            "code_challenge_methods_supported": ["S256"]
        }))
    }

    async fn register_client() -> axum::Json<Value> {
        axum::Json(json!({
            "client_id": "fake-mcp-client"
        }))
    }

    async fn token_exchange(
        axum::extract::State(state): axum::extract::State<FakeHostedMcpOauthState>,
        axum::Form(params): axum::Form<std::collections::HashMap<String, String>>,
    ) -> axum::Json<Value> {
        let grant_type = params
            .get("grant_type")
            .map(String::as_str)
            .unwrap_or_default();
        let (access_token, refresh_token) = if grant_type == "refresh_token" {
            ("access-token-456", "refresh-token-456")
        } else {
            ("access-token-123", "refresh-token-123")
        };
        *state.valid_access_token.write().await = access_token.to_string();
        axum::Json(json!({
            "access_token": access_token,
            "refresh_token": refresh_token,
            "expires_in": 3600,
            "token_type": "Bearer"
        }))
    }

    async fn handle_mcp(
        axum::extract::State(state): axum::extract::State<FakeHostedMcpOauthState>,
        headers: axum::http::HeaderMap,
        axum::Json(payload): axum::Json<Value>,
    ) -> impl axum::response::IntoResponse {
        let auth = headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let expected = state.valid_access_token.read().await.clone();
        if auth != format!("Bearer {expected}") {
            let www_authenticate = format!(
                "Bearer realm=\"OAuth\", resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\", error=\"invalid_token\", error_description=\"Missing or invalid access token\"",
                state.base_url
            );
            return (
                StatusCode::UNAUTHORIZED,
                [("www-authenticate", www_authenticate)],
                Json(json!({
                    "error": "invalid_token",
                    "error_description": "Missing or invalid access token"
                })),
            )
                .into_response();
        }
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
                        "name": "fake-hosted-notion",
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
            _ => json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": {
                    "code": -32601,
                    "message": "Method not found"
                }
            }),
        };
        (StatusCode::OK, Json(response)).into_response()
    }

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake hosted mcp oauth server");
    let base_url = format!("http://{}", listener.local_addr().expect("local addr"));
    let state = FakeHostedMcpOauthState {
        base_url: base_url.clone(),
        valid_access_token: std::sync::Arc::new(tokio::sync::RwLock::new(
            "access-token-123".to_string(),
        )),
    };
    let app = axum::Router::new()
        .route("/mcp", axum::routing::post(handle_mcp))
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            axum::routing::get(protected_resource),
        )
        .route(
            "/.well-known/oauth-authorization-server",
            axum::routing::get(authorization_server),
        )
        .route("/register", axum::routing::post(register_client))
        .route("/token", axum::routing::post(token_exchange))
        .with_state(state);
    let endpoint = format!("{base_url}/mcp");
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve fake hosted mcp oauth server");
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
        HeaderMap::new(),
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

#[tokio::test]
async fn mcp_connect_discovers_www_authenticate_oauth_and_callback_connects_server() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_hosted_mcp_oauth_server().await;

    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    assert!(state.mcp.set_auth_kind("notion", "oauth".to_string()).await);

    let app = app_router(state.clone());
    let connect_req = Request::builder()
        .method("POST")
        .uri("/mcp/notion/connect")
        .body(Body::empty())
        .expect("connect request");
    let connect_resp = app
        .clone()
        .oneshot(connect_req)
        .await
        .expect("connect response");
    assert_eq!(connect_resp.status(), StatusCode::OK);
    let connect_body = to_bytes(connect_resp.into_body(), usize::MAX)
        .await
        .expect("connect body");
    let connect_payload: Value = serde_json::from_slice(&connect_body).expect("connect json");
    assert_eq!(
        connect_payload.get("ok").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        connect_payload.get("pendingAuth").and_then(Value::as_bool),
        Some(true)
    );
    let authorization_url = connect_payload
        .get("authorizationUrl")
        .and_then(Value::as_str)
        .expect("authorizationUrl");
    assert!(authorization_url.starts_with("http://127.0.0.1:"));
    assert!(authorization_url.contains("/authorize?"));
    assert!(authorization_url.contains("redirect_uri="));
    assert!(authorization_url.contains("%2Fapi%2Fengine%2Fmcp%2Fnotion%2Fauth%2Fcallback"));

    let pending_challenge = state
        .mcp
        .list()
        .await
        .get("notion")
        .and_then(|row| row.last_auth_challenge.clone())
        .expect("pending auth challenge");
    assert_eq!(pending_challenge.authorization_url, authorization_url);

    let session = state
        .mcp_oauth_sessions
        .read()
        .await
        .values()
        .find(|session| session.server_name == "notion")
        .cloned()
        .expect("mcp oauth session");

    let callback_req = Request::builder()
        .method("GET")
        .uri(format!(
            "/mcp/notion/auth/callback?code=test-code&state={}",
            urlencoding::encode(&session.state)
        ))
        .body(Body::empty())
        .expect("callback request");
    let callback_resp = app.oneshot(callback_req).await.expect("callback response");
    assert_eq!(callback_resp.status(), StatusCode::OK);

    let notion = state
        .mcp
        .list()
        .await
        .get("notion")
        .cloned()
        .expect("notion row");
    assert!(notion.connected);
    assert!(notion.last_auth_challenge.is_none());
    assert!(notion
        .tool_cache
        .iter()
        .any(|tool| tool.tool_name == "notion_search"));
    assert_eq!(
        notion
            .secret_header_values
            .get("Authorization")
            .map(String::as_str),
        Some("Bearer access-token-123")
    );

    drop(server);
}

#[tokio::test]
async fn mcp_refresh_silently_renews_expired_oauth_token() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_hosted_mcp_oauth_server().await;

    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    assert!(state.mcp.set_auth_kind("notion", "oauth".to_string()).await);

    let app = app_router(state.clone());
    let connect_req = Request::builder()
        .method("POST")
        .uri("/mcp/notion/connect")
        .body(Body::empty())
        .expect("connect request");
    let connect_resp = app
        .clone()
        .oneshot(connect_req)
        .await
        .expect("connect response");
    assert_eq!(connect_resp.status(), StatusCode::OK);

    let session = state
        .mcp_oauth_sessions
        .read()
        .await
        .values()
        .find(|session| session.server_name == "notion")
        .cloned()
        .expect("mcp oauth session");

    let callback_req = Request::builder()
        .method("GET")
        .uri(format!(
            "/mcp/notion/auth/callback?code=test-code&state={}",
            urlencoding::encode(&session.state)
        ))
        .body(Body::empty())
        .expect("callback request");
    let callback_resp = app.oneshot(callback_req).await.expect("callback response");
    assert_eq!(callback_resp.status(), StatusCode::OK);

    tandem_core::set_provider_oauth_credential(
        "mcp-oauth::notion",
        tandem_core::OAuthProviderCredential {
            provider_id: "mcp-oauth::notion".to_string(),
            access_token: "access-token-123".to_string(),
            refresh_token: "refresh-token-123".to_string(),
            expires_at_ms: crate::now_ms().saturating_sub(1_000),
            account_id: None,
            email: None,
            display_name: None,
            managed_by: "tandem".to_string(),
            api_key: None,
        },
    )
    .expect("store expired oauth credential");

    let tools = state.mcp.refresh("notion").await.expect("refresh notion");
    assert!(!tools.is_empty());

    let notion = state
        .mcp
        .list()
        .await
        .get("notion")
        .cloned()
        .expect("notion row");
    assert_eq!(
        notion
            .secret_header_values
            .get("Authorization")
            .map(String::as_str),
        Some("Bearer access-token-456")
    );
    let stored = tandem_core::load_provider_oauth_credential("mcp-oauth::notion")
        .expect("refreshed oauth credential");
    assert_eq!(stored.access_token, "access-token-456");
    assert_eq!(stored.refresh_token, "refresh-token-456");

    drop(server);
}

#[tokio::test]
async fn mcp_catalog_overlay_surfaces_connected_and_uncataloged_states() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_notion_oauth_mcp_server().await;
    let app = app_router(state.clone());

    state
        .mcp
        .add_or_update("orphaned".to_string(), endpoint, HashMap::new(), true)
        .await;
    assert!(state.mcp.connect("orphaned").await);

    let tool_names = state
        .tools
        .list()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .collect::<Vec<_>>();
    assert!(tool_names.iter().any(|name| name == "mcp_list_catalog"));
    assert!(tool_names
        .iter()
        .any(|name| name == "mcp_request_capability"));

    let catalog_req = Request::builder()
        .method("GET")
        .uri("/mcp/catalog")
        .body(Body::empty())
        .expect("catalog request");
    let catalog_resp = app.oneshot(catalog_req).await.expect("catalog response");
    assert_eq!(catalog_resp.status(), StatusCode::OK);
    let catalog_body = to_bytes(catalog_resp.into_body(), usize::MAX)
        .await
        .expect("catalog body");
    let catalog_payload: Value = serde_json::from_slice(&catalog_body).expect("catalog json");

    let servers = catalog_payload
        .get("servers")
        .and_then(Value::as_array)
        .expect("servers array");
    let github = servers
        .iter()
        .find(|row| row.get("slug").and_then(Value::as_str) == Some("github"))
        .expect("github server row");
    assert_eq!(
        github.get("connection_status").and_then(Value::as_str),
        Some("connected_enabled")
    );
    assert_eq!(
        github.get("execution_available").and_then(Value::as_bool),
        Some(true)
    );

    let notion = servers
        .iter()
        .find(|row| row.get("slug").and_then(Value::as_str) == Some("notion"))
        .expect("notion server row");
    assert_eq!(
        notion.get("connection_status").and_then(Value::as_str),
        Some("cataloged_not_connected")
    );

    let overlay = catalog_payload
        .get("overlay")
        .and_then(Value::as_object)
        .expect("overlay object");
    let status_counts = overlay
        .get("status_counts")
        .and_then(Value::as_object)
        .expect("status counts");
    assert_eq!(
        status_counts
            .get("connected_enabled")
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        status_counts
            .get("uncataloged_connected")
            .and_then(Value::as_u64),
        Some(1)
    );

    let uncataloged_connected_servers = overlay
        .get("uncataloged_connected_servers")
        .and_then(Value::as_array)
        .expect("uncataloged connected servers");
    let orphaned = uncataloged_connected_servers
        .iter()
        .find(|row| row.get("name").and_then(Value::as_str) == Some("orphaned"))
        .expect("orphaned server row");
    assert_eq!(
        orphaned.get("connection_status").and_then(Value::as_str),
        Some("uncataloged_connected")
    );

    let catalog_output = state
        .tools
        .execute("mcp_list_catalog", json!({}))
        .await
        .expect("execute mcp_list_catalog");
    let tool_payload: Value =
        serde_json::from_str(&catalog_output.output).expect("catalog tool json");
    assert_eq!(
        tool_payload
            .get("overlay")
            .and_then(|row| row.get("status_counts"))
            .and_then(|row| row.get("uncataloged_connected"))
            .and_then(Value::as_u64),
        Some(1)
    );

    drop(server);
}

#[tokio::test]
async fn mcp_request_capability_route_creates_approval_request() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let req = Request::builder()
        .method("POST")
        .uri("/mcp/request-capability")
        .header("content-type", "application/json")
        .header("x-tandem-agent-id", "agent-self-operator")
        .body(Body::from(
            json!({
                "agent_id": "agent-self-operator",
                "mcp_name": "notion",
                "catalog_slug": "notion",
                "rationale": "Need Notion access for the weekly competitor pulse.",
                "requested_tools": ["notion_search"],
                "context": {
                    "gap": "daily competitor pulse",
                    "source": "self-operator"
                }
            })
            .to_string(),
        ))
        .expect("capability request");
    let resp = app.oneshot(req).await.expect("capability response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("capability body");
    let payload: Value = serde_json::from_slice(&body).expect("capability json");

    let approval = payload
        .get("approval")
        .and_then(Value::as_object)
        .expect("approval object");
    let approval_id = approval
        .get("approval_id")
        .and_then(Value::as_str)
        .expect("approval id");
    assert_eq!(
        approval.get("request_type").and_then(Value::as_str),
        Some("capability_request")
    );
    assert_eq!(
        approval
            .get("target_resource")
            .and_then(|row| row.get("type"))
            .and_then(Value::as_str),
        Some("agent")
    );
    assert_eq!(
        approval
            .get("target_resource")
            .and_then(|row| row.get("id"))
            .and_then(Value::as_str),
        Some("agent-self-operator")
    );
    assert_eq!(
        approval
            .get("requested_by")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("agent")
    );
    assert_eq!(
        approval
            .get("context")
            .and_then(|row| row.get("capability_key"))
            .and_then(Value::as_str),
        Some("mcp:notion")
    );
    assert!(approval
        .get("context")
        .and_then(|row| row.get("catalog_match"))
        .and_then(Value::as_bool)
        .unwrap_or(false));

    let approvals = state.list_approval_requests(None, None).await;
    assert!(approvals.iter().any(|row| row.approval_id == approval_id));
}

#[tokio::test]
async fn mcp_request_capability_tool_creates_approval_request() {
    let state = test_state().await;

    let output = state
        .tools
        .execute(
            "mcp_request_capability",
            json!({
                "__session_id": "self-operator-session",
                "agent_id": "agent-self-operator",
                "mcp_name": "notion",
                "rationale": "Need Notion access for the weekly competitor pulse.",
                "requested_tools": ["notion_search"],
                "context": {
                    "gap": "daily competitor pulse"
                }
            }),
        )
        .await
        .expect("execute mcp_request_capability");
    let payload: Value = serde_json::from_str(&output.output).expect("capability tool json");

    assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(true));
    let approval = payload
        .get("approval")
        .and_then(Value::as_object)
        .expect("approval object");
    assert_eq!(
        approval.get("request_type").and_then(Value::as_str),
        Some("capability_request")
    );
    assert_eq!(
        approval
            .get("requested_by")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("agent")
    );
    assert_eq!(
        approval
            .get("requested_by")
            .and_then(|row| row.get("actor_id"))
            .and_then(Value::as_str),
        Some("agent-self-operator")
    );
    assert!(approval
        .get("requested_by")
        .and_then(|row| row.get("source"))
        .and_then(Value::as_str)
        .is_some_and(|source| source.contains("tool:self-operator-session")));
    assert_eq!(
        approval
            .get("context")
            .and_then(|row| row.get("capability_key"))
            .and_then(Value::as_str),
        Some("mcp:notion")
    );
}
