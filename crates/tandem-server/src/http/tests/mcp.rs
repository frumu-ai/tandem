use super::*;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

mod inventory;
mod run_as;

fn mcp_env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct McpEnvGuard {
    _guard: MutexGuard<'static, ()>,
    saved: Vec<(&'static str, Option<String>)>,
}

impl McpEnvGuard {
    fn new(vars: &[&'static str]) -> Self {
        let guard = mcp_env_lock().lock().expect("mcp env lock");
        let saved = vars
            .iter()
            .copied()
            .map(|key| (key, std::env::var(key).ok()))
            .collect::<Vec<_>>();
        Self {
            _guard: guard,
            saved,
        }
    }

    fn set(&self, key: &'static str, value: impl AsRef<str>) {
        std::env::set_var(key, value.as_ref());
    }
}

impl Drop for McpEnvGuard {
    fn drop(&mut self) {
        for (key, value) in self.saved.drain(..) {
            if let Some(value) = value {
                std::env::set_var(key, value);
            } else {
                std::env::remove_var(key);
            }
        }
    }
}

#[test]
fn mcp_public_base_url_from_config_trims_trailing_slash() {
    let cfg = json!({
        "hosted": {
            "public_url": "https://test.tandem.ac/"
        }
    });

    assert_eq!(
        crate::http::mcp::mcp_public_base_url_from_config(&cfg).as_deref(),
        Some("https://test.tandem.ac")
    );
}

#[test]
fn mcp_public_base_url_from_env_uses_control_panel_public_url() {
    let guard = McpEnvGuard::new(&[
        "TANDEM_CONTROL_PANEL_PUBLIC_URL",
        "HOSTED_CONTROL_PANEL_PUBLIC_URL",
        "HOSTED_PUBLIC_URL",
    ]);
    guard.set("TANDEM_CONTROL_PANEL_PUBLIC_URL", "https://test.tandem.ac/");
    std::env::remove_var("HOSTED_CONTROL_PANEL_PUBLIC_URL");
    std::env::remove_var("HOSTED_PUBLIC_URL");

    assert_eq!(
        crate::http::mcp::mcp_public_base_url_from_env().as_deref(),
        Some("https://test.tandem.ac")
    );
}

fn strict_context_with_grant(
    permissions: Vec<tandem_types::AccessPermission>,
    data_classes: Vec<tandem_types::DataClass>,
) -> tandem_types::StrictTenantContext {
    let tenant_context = tandem_types::TenantContext::explicit_user_workspace(
        "acme",
        "dev",
        Some("deployment-test".to_string()),
        "user-1",
    );
    let principal = tandem_types::PrincipalRef::human_user("user-1");
    let root = tandem_types::ResourceRef::new(
        "acme",
        "*",
        tandem_types::ResourceKind::Organization,
        "acme",
    );
    let grant = tandem_types::ScopedGrant::new(
        "grant-mcp-discovery",
        principal.clone(),
        root.clone(),
        tandem_types::GrantSource::Direct,
    )
    .with_permissions(permissions)
    .with_data_classes(data_classes.clone());

    tandem_types::StrictTenantContext::new(
        tenant_context,
        principal.clone(),
        tandem_types::AuthorityChain::from_request(
            tandem_types::RequestPrincipal::authenticated_user(principal.id, "test"),
        ),
        tandem_types::ResourceScope::root(root),
        tandem_types::AssertionMetadata::new(
            "test",
            "tandem-runtime",
            1_000,
            9_999_999_999_999,
            "assertion-mcp-discovery",
        ),
    )
    .with_grants(vec![grant])
    .with_data_boundary(tandem_types::DataBoundary::allow(data_classes))
}

fn tenant_request(
    method: &str,
    uri: impl AsRef<str>,
    org_id: &str,
    workspace_id: &str,
    actor_id: &str,
) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri.as_ref())
        .header("x-tandem-org-id", org_id)
        .header("x-tandem-workspace-id", workspace_id)
        .header("x-tandem-actor-id", actor_id)
        .body(Body::empty())
        .expect("tenant request")
}

async fn spawn_fake_notion_oauth_mcp_server() -> (String, tokio::task::JoinHandle<()>) {
    async fn handle(
        headers: axum::http::HeaderMap,
        axum::Json(payload): axum::Json<Value>,
    ) -> axum::Json<Value> {
        let id = payload.get("id").cloned().unwrap_or_else(|| json!(1));
        let method = payload.get("method").and_then(Value::as_str).unwrap_or("");
        let auth = headers
            .get("authorization")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("");
        let tool_name = if auth.contains("alice-union-token") {
            "alice_search"
        } else if auth.contains("bob-union-token") {
            "bob_search"
        } else {
            "notion_search"
        };
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
                            "name": tool_name,
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
async fn bootstrap_mcp_servers_installs_builtin_tandem_docs_server() {
    let _env = McpEnvGuard::new(&["TANDEM_DOCS_MCP_TRANSPORT_URL"]);
    let (endpoint, server) = spawn_fake_notion_oauth_mcp_server().await;
    _env.set("TANDEM_DOCS_MCP_TRANSPORT_URL", &endpoint);

    let state = test_state().await;
    super::super::bootstrap_mcp_servers(&state).await;

    let server_row = state
        .mcp
        .list()
        .await
        .get("tandem-mcp")
        .cloned()
        .expect("builtin tandem docs mcp server");
    assert_eq!(server_row.transport, endpoint);
    assert!(server_row.enabled);
    assert!(server_row.connected);
    assert!(server_row
        .tool_cache
        .iter()
        .any(|tool| tool.tool_name == "notion_search"));

    server.abort();
}

#[tokio::test]
async fn mcp_secret_tenant_mismatch_writes_sanitized_protected_audit() {
    let state = test_state().await;
    let tenant_a = tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "workspace-a",
        Some("deployment-a".to_string()),
        "user-a",
    );
    let tenant_b = tandem_types::TenantContext::explicit_user_workspace(
        "org-b",
        "workspace-b",
        Some("deployment-b".to_string()),
        "user-b",
    );
    state
        .mcp
        .add_or_update_with_secret_refs(
            "tenant-server".to_string(),
            "http://127.0.0.1:9/mcp".to_string(),
            HashMap::new(),
            HashMap::from([(
                "Authorization".to_string(),
                tandem_runtime::McpSecretRef::Store {
                    secret_id: "super-secret-canary".to_string(),
                    tenant_context: tenant_a,
                },
            )]),
            &tenant_b,
            true,
        )
        .await;

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_audit(
        &state,
        "tenant-server",
        "get_me",
        json!({}),
        &tenant_b,
    )
    .await
    .expect_err("tenant B cannot use tenant A's store-backed secret");
    assert!(err.contains("ToolDenied { reason: TenantScope }"));

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"mcp.secret_tenant_mismatch\""));
    assert!(audit.contains("\"Authorization\""));
    assert!(audit.contains("\"server_name\":\"tenant-server\""));
    assert!(
        !audit.contains("super-secret-canary"),
        "secret identifiers must not be written to protected audit payloads"
    );

    let events = crate::audit::load_protected_audit_events_for_tenant(&state, &tenant_b).await;
    let event = events
        .iter()
        .find(|event| event.event_type == "mcp.secret_tenant_mismatch")
        .expect("mcp secret tenant mismatch audit event");
    assert_eq!(event.actor.as_deref(), Some("user-b"));
    assert_eq!(
        event.payload["header_names"]
            .as_array()
            .and_then(|headers| headers.first())
            .and_then(Value::as_str),
        Some("Authorization")
    );
}

#[tokio::test]
async fn disabled_mcp_server_with_foreign_secret_does_not_emit_mismatch_audit() {
    let state = test_state().await;
    let tenant_a = tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "workspace-a",
        Some("deployment-a".to_string()),
        "user-a",
    );
    let tenant_b = tandem_types::TenantContext::explicit_user_workspace(
        "org-b",
        "workspace-b",
        Some("deployment-b".to_string()),
        "user-b",
    );
    state
        .mcp
        .add_or_update_with_secret_refs(
            "disabled-tenant-server".to_string(),
            "http://127.0.0.1:9/mcp".to_string(),
            HashMap::new(),
            HashMap::from([(
                "Authorization".to_string(),
                tandem_runtime::McpSecretRef::Store {
                    secret_id: "super-secret-canary".to_string(),
                    tenant_context: tenant_a,
                },
            )]),
            &tenant_b,
            false,
        )
        .await;

    let err = crate::http::mcp::call_mcp_tool_for_tenant_with_audit(
        &state,
        "disabled-tenant-server",
        "get_me",
        json!({}),
        &tenant_b,
    )
    .await
    .expect_err("disabled server should fail before tenant secret validation");
    assert!(err.contains("is disabled"));

    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .unwrap_or_default();
    assert!(!audit.contains("mcp.secret_tenant_mismatch"));
    assert!(!audit.contains("super-secret-canary"));
}

#[cfg(not(feature = "premium-governance"))]
#[tokio::test]
async fn mcp_catalog_stays_available_but_capability_requests_fail_closed_without_premium() {
    let state = test_state().await;
    let app = app_router(state.clone());

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
    let catalog_resp = app
        .clone()
        .oneshot(catalog_req)
        .await
        .expect("catalog response");
    assert_eq!(catalog_resp.status(), StatusCode::OK);
    let catalog_body = to_bytes(catalog_resp.into_body(), usize::MAX)
        .await
        .expect("catalog body");
    let catalog_payload: Value = serde_json::from_slice(&catalog_body).expect("catalog json");
    assert!(catalog_payload
        .get("servers")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty()));

    let catalog_output = state
        .tools
        .execute("mcp_list_catalog", json!({}))
        .await
        .expect("execute mcp_list_catalog");
    let tool_payload: Value =
        serde_json::from_str(&catalog_output.output).expect("catalog tool json");
    assert!(tool_payload
        .get("servers")
        .and_then(Value::as_array)
        .is_some_and(|rows| !rows.is_empty()));

    let req = Request::builder()
        .method("POST")
        .uri("/mcp/request-capability")
        .header("content-type", "application/json")
        .header("x-tandem-request-source", "agent")
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
    assert_eq!(resp.status(), StatusCode::NOT_IMPLEMENTED);
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("capability body");
    let payload: Value = serde_json::from_slice(&body).expect("capability json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("PREMIUM_FEATURE_REQUIRED")
    );
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
async fn mcp_list_redacts_execute_tools_without_strict_grant() {
    let state = test_state().await;
    let strict_context = strict_context_with_grant(
        vec![
            tandem_types::AccessPermission::View,
            tandem_types::AccessPermission::Read,
        ],
        vec![
            tandem_types::DataClass::Internal,
            tandem_types::DataClass::SourceCode,
        ],
    );

    let output = state
        .tools
        .execute(
            "mcp_list",
            json!({
                "__strict_tenant_context": strict_context
            }),
        )
        .await
        .expect("execute mcp_list");
    let payload: Value = serde_json::from_str(&output.output).expect("inventory json");
    let github = payload
        .get("servers")
        .and_then(Value::as_array)
        .and_then(|servers| {
            servers
                .iter()
                .find(|row| row.get("name").and_then(Value::as_str) == Some("github"))
        })
        .expect("github server remains visible");
    let remote_tools = github
        .get("remote_tools")
        .and_then(Value::as_array)
        .expect("remote tools");

    assert!(remote_tools
        .iter()
        .any(|tool| tool.as_str() == Some("mcp.github.list_repository_issues")));
    assert!(!remote_tools
        .iter()
        .any(|tool| tool.as_str() == Some("mcp.github.create_pull_request")));
    let all_remote_tools = payload
        .get("remote_tools")
        .and_then(Value::as_array)
        .expect("all remote tools");
    assert!(!all_remote_tools
        .iter()
        .any(|tool| tool.as_str() == Some("mcp.github.create_pull_request")));
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
        axum::extract::Extension(tandem_types::TenantContext::local_implicit()),
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
    let callback_status = callback_resp.status();
    let callback_body = to_bytes(callback_resp.into_body(), usize::MAX)
        .await
        .expect("callback body");
    assert_eq!(
        callback_status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&callback_body)
    );

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
async fn mcp_oauth_session_records_tenant_actor_connection_identity() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_hosted_mcp_oauth_server().await;
    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    assert!(state.mcp.set_auth_kind("notion", "oauth".to_string()).await);

    let app = app_router(state.clone());
    let resp = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/mcp/notion/connect",
            "org-a",
            "workspace-a",
            "alice",
        ))
        .await
        .expect("connect response");
    assert_eq!(resp.status(), StatusCode::OK);

    let session = state
        .mcp_oauth_sessions
        .read()
        .await
        .values()
        .find(|session| session.server_name == "notion")
        .cloned()
        .expect("mcp oauth session");
    let tenant =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");
    assert_eq!(session.tenant_context, tenant);
    assert_eq!(
        session.principal,
        tandem_runtime::McpPrincipalRef::HumanActor {
            actor_id: "alice".to_string()
        }
    );
    assert_eq!(
        session.connection_id,
        state.mcp.connection_id_for_tenant("notion", &tenant)
    );
    assert_ne!(session.provider_id, "mcp-oauth::notion");
    assert!(session.provider_id.ends_with(&session.connection_id));
    let server_row_after_start = state
        .mcp
        .list()
        .await
        .get("notion")
        .cloned()
        .expect("notion row after oauth start");
    assert!(
        server_row_after_start.last_auth_challenge.is_none(),
        "explicit tenant OAuth challenges must not be written into the shared server row"
    );
    let connections_after_start = state.mcp.list_connections().await;
    let alice_pending_connection = connections_after_start
        .get(&session.connection_id)
        .expect("alice pending scoped connection");
    assert_eq!(
        alice_pending_connection
            .last_auth_challenge
            .as_ref()
            .map(|challenge| challenge.authorization_url.as_str()),
        Some(session.authorization_url.as_str())
    );

    let bob_resp = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/mcp/notion/connect",
            "org-a",
            "workspace-a",
            "bob",
        ))
        .await
        .expect("bob connect response");
    assert_eq!(bob_resp.status(), StatusCode::OK);
    let bob_tenant =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "bob");
    let bob_session = state
        .mcp_oauth_sessions
        .read()
        .await
        .values()
        .find(|candidate| candidate.tenant_context == bob_tenant)
        .cloned()
        .expect("bob mcp oauth session");
    assert_ne!(bob_session.connection_id, session.connection_id);
    assert_ne!(bob_session.provider_id, session.provider_id);
    assert_ne!(bob_session.authorization_url, session.authorization_url);

    let alice_authenticate_resp = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/mcp/notion/auth/authenticate",
            "org-a",
            "workspace-a",
            "alice",
        ))
        .await
        .expect("alice authenticate response");
    assert_eq!(alice_authenticate_resp.status(), StatusCode::OK);
    let alice_authenticate_body = to_bytes(alice_authenticate_resp.into_body(), usize::MAX)
        .await
        .expect("alice authenticate body");
    let alice_authenticate_payload: Value =
        serde_json::from_slice(&alice_authenticate_body).expect("alice authenticate json");
    assert_eq!(
        alice_authenticate_payload
            .get("authorizationUrl")
            .and_then(Value::as_str),
        Some(session.authorization_url.as_str())
    );
    assert_ne!(
        alice_authenticate_payload
            .get("authorizationUrl")
            .and_then(Value::as_str),
        Some(bob_session.authorization_url.as_str())
    );

    let callback_resp = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/mcp/notion/auth/callback?code=test-code&state={}",
                    urlencoding::encode(&session.state)
                ))
                .body(Body::empty())
                .expect("callback request"),
        )
        .await
        .expect("callback response");
    assert_eq!(callback_resp.status(), StatusCode::OK);
    let server_row = state
        .mcp
        .list()
        .await
        .get("notion")
        .cloned()
        .expect("notion row");
    assert!(
        !server_row.secret_headers.contains_key("Authorization"),
        "explicit tenant OAuth must not write bearer refs into the shared server row"
    );
    assert!(
        server_row.tool_cache.is_empty(),
        "explicit tenant OAuth discovery must not write authenticated tools into the shared server row"
    );
    let connections = state.mcp.list_connections().await;
    let alice_connection = connections
        .get(&session.connection_id)
        .expect("alice scoped connection");
    assert!(alice_connection.connected);
    assert!(alice_connection
        .tool_cache
        .iter()
        .any(|tool| tool.tool_name == "notion_search"));
    assert!(alice_connection
        .secret_headers
        .contains_key("Authorization"));
    assert_eq!(
        alice_connection
            .oauth
            .as_ref()
            .map(|oauth| oauth.provider_id.as_str()),
        Some(session.provider_id.as_str())
    );

    drop(server);
}

#[tokio::test]
async fn tenant_tool_sync_preserves_other_actor_bridge_tools() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_notion_oauth_mcp_server().await;
    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    let alice =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "alice");
    let bob =
        tandem_types::TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "bob");
    state
        .mcp
        .set_bearer_token_for_tenant("notion", "alice-union-token", &alice)
        .await
        .expect("store alice token");
    state
        .mcp
        .refresh_for_tenant("notion", &alice)
        .await
        .expect("refresh alice tools");
    assert_eq!(
        crate::http::mcp::sync_mcp_tools_for_server_for_tenant(&state, "notion", &alice).await,
        1
    );
    state
        .mcp
        .set_bearer_token_for_tenant("notion", "bob-union-token", &bob)
        .await
        .expect("store bob token");
    state
        .mcp
        .refresh_for_tenant("notion", &bob)
        .await
        .expect("refresh bob tools");
    assert_eq!(
        crate::http::mcp::sync_mcp_tools_for_server_for_tenant(&state, "notion", &bob).await,
        1
    );
    let registered = state
        .tools
        .list()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .collect::<Vec<_>>();
    assert!(registered.contains(&"mcp.notion.alice_search".to_string()));
    assert!(registered.contains(&"mcp.notion.bob_search".to_string()));
    drop(server);
}

#[tokio::test]
async fn mcp_oauth_callback_rejects_cross_actor_context() {
    let state = test_state().await;
    let (endpoint, server) = spawn_fake_hosted_mcp_oauth_server().await;
    state
        .mcp
        .add_or_update("notion".to_string(), endpoint, HashMap::new(), true)
        .await;
    assert!(state.mcp.set_auth_kind("notion", "oauth".to_string()).await);

    let app = app_router(state.clone());
    let connect_resp = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/mcp/notion/connect",
            "org-a",
            "workspace-a",
            "alice",
        ))
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

    let callback_app = axum::Router::new()
        .route(
            "/mcp/{name}/auth/callback",
            axum::routing::get(crate::http::mcp::callback_mcp_get),
        )
        .layer(axum::extract::Extension(
            tandem_types::TenantContext::explicit_user_workspace(
                "org-a",
                "workspace-a",
                None,
                "bob",
            ),
        ))
        .with_state(state.clone());
    let callback_resp = callback_app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/mcp/notion/auth/callback?code=test-code&state={}",
                    urlencoding::encode(&session.state)
                ))
                .body(Body::empty())
                .expect("callback request"),
        )
        .await
        .expect("callback response");
    assert_eq!(callback_resp.status(), StatusCode::OK);
    let callback_body = to_bytes(callback_resp.into_body(), usize::MAX)
        .await
        .expect("callback body");
    assert!(String::from_utf8_lossy(&callback_body).contains("tenant context did not match"));
    let session_after = state
        .mcp_oauth_sessions
        .read()
        .await
        .get(&session.session_id)
        .cloned()
        .expect("session remains recorded");
    assert_eq!(session_after.status, "pending");
    assert!(
        !state
            .mcp
            .list()
            .await
            .get("notion")
            .expect("notion row")
            .connected
    );
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"mcp.connection.oauth_denied\""));
    assert!(audit.contains("tenant_context_mismatch"));
    assert!(audit.contains(&session.connection_id));
    assert!(!audit.contains("test-code"));
    assert!(!audit.contains(&session.code_verifier));

    drop(server);
}

#[tokio::test]
async fn mcp_delete_auth_clears_stale_oauth_material() {
    let state = test_state().await;

    state
        .mcp
        .add_or_update(
            "notion".to_string(),
            "https://example.test/mcp".to_string(),
            HashMap::new(),
            true,
        )
        .await;
    assert!(state.mcp.set_auth_kind("notion", "oauth".to_string()).await);
    state
        .mcp
        .set_bearer_token("notion", "stale-access-token")
        .await
        .expect("store bearer token");
    state
        .mcp
        .set_oauth_refresh_config(
            "notion",
            "mcp-oauth::notion".to_string(),
            "https://example.test/token".to_string(),
            "stale-client-id".to_string(),
            Some("stale-client-secret".to_string()),
        )
        .await
        .expect("store oauth refresh config");
    let provider_id = "mcp-oauth::notion".to_string();
    let provider_auth_security_dir = state
        .shared_resources_path
        .parent()
        .expect("state root")
        .join("security");
    let stale_credential = tandem_core::OAuthProviderCredential {
        provider_id: provider_id.clone(),
        access_token: "stale-oauth-access-token".to_string(),
        refresh_token: "stale-oauth-refresh-token".to_string(),
        expires_at_ms: crate::now_ms().saturating_add(60_000),
        account_id: None,
        email: None,
        display_name: None,
        managed_by: "tandem".to_string(),
        api_key: None,
    };
    tandem_core::set_provider_oauth_credential_in_dir(
        &provider_auth_security_dir,
        &provider_id,
        stale_credential.clone(),
    )
    .expect("store registry oauth credential");
    tandem_core::set_provider_oauth_credential(&provider_id, stale_credential)
        .expect("store fallback oauth credential");
    state.mcp_oauth_sessions.write().await.insert(
        "pending-session-1".to_string(),
        McpOAuthSessionRecord {
            session_id: "pending-session-1".to_string(),
            server_name: "notion".to_string(),
            tenant_context: tandem_types::TenantContext::local_implicit(),
            principal: tandem_runtime::McpPrincipalRef::LocalImplicit,
            connection_id: state
                .mcp
                .connection_id_for_tenant("notion", &tandem_types::TenantContext::local_implicit()),
            provider_id: "mcp-oauth::notion".to_string(),
            status: "pending".to_string(),
            created_at_ms: crate::now_ms(),
            expires_at_ms: crate::now_ms().saturating_add(60_000),
            redirect_uri: "https://panel.test/api/engine/mcp/notion/auth/callback".to_string(),
            state: "stale-state-token".to_string(),
            code_verifier: "stale-code-verifier".to_string(),
            authorization_url: "https://example.test/authorize?state=stale-state-token".to_string(),
            token_endpoint: "https://example.test/token".to_string(),
            client_id: "stale-client-id".to_string(),
            client_secret: Some("stale-client-secret".to_string()),
            error: None,
        },
    );
    let challenge = tandem_runtime::McpAuthChallenge {
        challenge_id: "challenge-1".to_string(),
        tool_name: "notion_search".to_string(),
        authorization_url: "https://example.test/authorize".to_string(),
        message: "Authorization required.".to_string(),
        requested_at_ms: crate::now_ms(),
        status: "pending".to_string(),
    };
    assert!(
        state
            .mcp
            .record_server_auth_challenge("notion", challenge, None)
            .await
    );

    let before = state
        .mcp
        .list()
        .await
        .get("notion")
        .cloned()
        .expect("notion row before delete");
    assert!(before.secret_headers.contains_key("Authorization"));
    assert!(before.secret_header_values.contains_key("Authorization"));
    assert!(before.oauth.is_some());
    assert!(before.last_auth_challenge.is_some());
    assert!(!before.pending_auth_by_tool.is_empty());

    let app = app_router(state.clone());
    let req = Request::builder()
        .method("DELETE")
        .uri("/mcp/notion/auth")
        .body(Body::empty())
        .expect("delete auth request");
    let resp = app.oneshot(req).await.expect("delete auth response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("delete auth body");
    let payload: Value = serde_json::from_slice(&body).expect("delete auth json");
    assert_eq!(payload.get("ok").and_then(Value::as_bool), Some(true));
    assert_eq!(
        payload
            .get("removedOauthSessionCount")
            .and_then(Value::as_u64),
        Some(1)
    );

    let notion = state
        .mcp
        .list()
        .await
        .get("notion")
        .cloned()
        .expect("notion row after delete");
    assert!(!notion.connected);
    assert!(notion.secret_headers.is_empty());
    assert!(notion.secret_header_values.is_empty());
    assert!(notion.oauth.is_none());
    assert!(notion.last_auth_challenge.is_none());
    assert!(notion.pending_auth_by_tool.is_empty());
    assert!(notion.tool_cache.is_empty());
    assert!(notion.tools_fetched_at_ms.is_none());
    assert!(state
        .mcp_oauth_sessions
        .read()
        .await
        .values()
        .all(|session| session.server_name != "notion"));
    assert!(tandem_core::load_provider_oauth_credential_in_dir(
        &provider_auth_security_dir,
        &provider_id,
    )
    .is_none());
    assert!(tandem_core::load_provider_oauth_credential(&provider_id).is_none());
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

    let provider_auth_security_dir = state
        .shared_resources_path
        .parent()
        .expect("state root")
        .join("security");
    tandem_core::set_provider_oauth_credential_in_dir(
        &provider_auth_security_dir,
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
    let stored = tandem_core::load_provider_oauth_credential_in_dir(
        &provider_auth_security_dir,
        "mcp-oauth::notion",
    )
    .expect("refreshed oauth credential");
    assert_eq!(stored.access_token, "access-token-456");
    assert_eq!(stored.refresh_token, "refresh-token-456");

    drop(server);
}

#[tokio::test]
async fn mcp_refresh_falls_back_to_global_oauth_credential() {
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

    let provider_auth_security_dir = state
        .shared_resources_path
        .parent()
        .expect("state root")
        .join("security");
    assert!(
        tandem_core::load_provider_oauth_credential_in_dir(
            &provider_auth_security_dir,
            "mcp-oauth::notion",
        )
        .is_none(),
        "callback writes the current global provider credential, not the registry-local copy"
    );
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
    .expect("store expired global oauth credential");

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
    let stored = tandem_core::load_provider_oauth_credential_in_dir(
        &provider_auth_security_dir,
        "mcp-oauth::notion",
    )
    .expect("refreshed oauth credential should be backfilled locally");
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
        .header("x-tandem-request-source", "agent")
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
