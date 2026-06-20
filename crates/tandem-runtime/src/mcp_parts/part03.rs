#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use uuid::Uuid;

    async fn spawn_fake_http_mcp_server() -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake mcp server");
        let addr = listener.local_addr().expect("fake mcp addr");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = vec![0_u8; 8192];
                    let Ok(n) = socket.read(&mut buf).await else {
                        return;
                    };
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let body = if request.contains("\"initialize\"") {
                        json!({
                            "jsonrpc": "2.0",
                            "id": "initialize-1",
                            "result": {
                                "protocolVersion": MCP_PROTOCOL_VERSION,
                                "capabilities": {},
                                "serverInfo": {"name": "fake", "version": "test"}
                            }
                        })
                    } else if request.contains("\"tools/list\"") {
                        json!({
                            "jsonrpc": "2.0",
                            "id": "tools-list-1",
                            "result": {
                                "tools": [{
                                    "name": "get_me",
                                    "description": "Get authenticated user",
                                    "inputSchema": {"type": "object", "properties": {}}
                                }]
                            }
                        })
                    } else if request.contains("\"tools/call\"") {
                        json!({
                            "jsonrpc": "2.0",
                            "id": "call-1",
                            "result": {
                                "content": [{"type": "text", "text": "authenticated"}]
                            }
                        })
                    } else {
                        json!({
                            "jsonrpc": "2.0",
                            "id": "unknown",
                            "error": {"code": -32601, "message": "unknown method"}
                        })
                    };
                    let payload = body.to_string();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nmcp-session-id: test-session\r\nconnection: close\r\n\r\n{}",
                        payload.len(),
                        payload
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}/mcp"), handle)
    }

    async fn spawn_auth_required_http_mcp_server(
        expected_authorization: &'static str,
    ) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fake auth mcp server");
        let addr = listener.local_addr().expect("fake auth mcp addr");
        let handle = tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                tokio::spawn(async move {
                    let mut buf = vec![0_u8; 8192];
                    let Ok(n) = socket.read(&mut buf).await else {
                        return;
                    };
                    let request = String::from_utf8_lossy(&buf[..n]);
                    let request_lower = request.to_ascii_lowercase();
                    let authorized = request_lower.contains(&format!(
                        "authorization: {}",
                        expected_authorization.to_ascii_lowercase()
                    ));
                    let body = if request.contains("\"initialize\"") {
                        json!({
                            "jsonrpc": "2.0",
                            "id": "initialize-1",
                            "result": {
                                "protocolVersion": MCP_PROTOCOL_VERSION,
                                "capabilities": {},
                                "serverInfo": {"name": "fake", "version": "test"}
                            }
                        })
                    } else if request.contains("\"tools/list\"") {
                        json!({
                            "jsonrpc": "2.0",
                            "id": "tools-list-1",
                            "result": {
                                "tools": [{
                                    "name": "get_me",
                                    "description": "Get authenticated user",
                                    "inputSchema": {"type": "object", "properties": {}}
                                }]
                            }
                        })
                    } else if request.contains("\"tools/call\"") && authorized {
                        json!({
                            "jsonrpc": "2.0",
                            "id": "call-1",
                            "result": {
                                "content": [{"type": "text", "text": "tenant-authenticated"}]
                            }
                        })
                    } else if request.contains("\"tools/call\"") {
                        json!({
                            "jsonrpc": "2.0",
                            "id": "call-1",
                            "error": {"code": -32001, "message": "unauthorized"}
                        })
                    } else {
                        json!({
                            "jsonrpc": "2.0",
                            "id": "unknown",
                            "error": {"code": -32601, "message": "unknown method"}
                        })
                    };
                    let payload = body.to_string();
                    let response = format!(
                        "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nmcp-session-id: test-session\r\nconnection: close\r\n\r\n{}",
                        payload.len(),
                        payload
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        (format!("http://{addr}/mcp"), handle)
    }

    #[tokio::test]
    async fn add_connect_disconnect_non_stdio_server() {
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        registry
            .add("example".to_string(), "sse:https://example.com".to_string())
            .await;
        assert!(registry.connect("example").await);
        let listed = registry.list().await;
        assert!(listed.get("example").map(|s| s.connected).unwrap_or(false));
        assert!(registry.disconnect("example").await);
    }

    #[tokio::test]
    async fn call_tool_reconnects_enabled_remote_server_before_execution() {
        let (endpoint, server) = spawn_fake_http_mcp_server().await;
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        registry
            .add("githubcopilot".to_string(), endpoint.to_string())
            .await;

        assert!(registry.connect("githubcopilot").await);
        assert!(registry.disconnect("githubcopilot").await);
        assert!(
            !registry
                .list()
                .await
                .get("githubcopilot")
                .expect("server")
                .connected
        );

        let result = registry
            .call_tool("githubcopilot", "get_me", json!({}))
            .await
            .expect("call should reconnect and execute");

        assert!(result.output.contains("authenticated"));
        assert!(
            registry
                .list()
                .await
                .get("githubcopilot")
                .expect("server")
                .connected
        );
        server.abort();
    }

    #[tokio::test]
    async fn ensure_ready_rejects_unknown_server_with_typed_error() {
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        let err = registry
            .ensure_ready("nope", EnsureReadyPolicy::default())
            .await
            .expect_err("missing server should error");
        assert_eq!(err, McpReadyError::NotFound);
    }

    #[tokio::test]
    async fn ensure_ready_rejects_disabled_server_with_typed_error() {
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        registry
            .add("example".to_string(), "sse:https://example.com".to_string())
            .await;
        registry.set_enabled("example", false).await;
        let err = registry
            .ensure_ready("example", EnsureReadyPolicy::default())
            .await
            .expect_err("disabled server should error");
        assert_eq!(err, McpReadyError::Disabled);
    }

    #[tokio::test]
    async fn ensure_ready_returns_already_connected_server_without_reconnecting() {
        let (endpoint, server) = spawn_fake_http_mcp_server().await;
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        registry
            .add("githubcopilot".to_string(), endpoint.to_string())
            .await;
        assert!(registry.connect("githubcopilot").await);

        let ready = registry
            .ensure_ready("githubcopilot", EnsureReadyPolicy::default())
            .await
            .expect("connected server should be ready");
        assert!(ready.connected);
        server.abort();
    }

    #[tokio::test]
    async fn ensure_ready_reconnects_when_disconnected() {
        let (endpoint, server) = spawn_fake_http_mcp_server().await;
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        registry
            .add("githubcopilot".to_string(), endpoint.to_string())
            .await;
        assert!(registry.connect("githubcopilot").await);
        assert!(registry.disconnect("githubcopilot").await);

        let ready = registry
            .ensure_ready("githubcopilot", EnsureReadyPolicy::default())
            .await
            .expect("ensure_ready should reconnect");
        assert!(ready.connected);
        server.abort();
    }

    #[tokio::test]
    async fn ensure_ready_returns_permanently_failed_when_endpoint_unreachable() {
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        registry
            .add(
                "broken".to_string(),
                "https://127.0.0.1:1/unreachable".to_string(),
            )
            .await;

        let err = registry
            .ensure_ready("broken", EnsureReadyPolicy::default())
            .await
            .expect_err("unreachable endpoint should permanently fail");
        assert!(matches!(err, McpReadyError::PermanentlyFailed { .. }));
    }

    #[test]
    fn parse_remote_endpoint_supports_http_prefixes() {
        assert_eq!(
            parse_remote_endpoint("https://mcp.example.com/mcp"),
            Some("https://mcp.example.com/mcp".to_string())
        );
        assert_eq!(
            parse_remote_endpoint("http:https://mcp.example.com/mcp"),
            Some("https://mcp.example.com/mcp".to_string())
        );
    }

    #[test]
    fn normalize_schema_removes_non_string_enums_recursively() {
        let mut schema = json!({
            "type": "object",
            "properties": {
                "good": { "type": "string", "enum": ["a", "b"] },
                "good_nullable": { "type": ["string", "null"], "enum": ["asc", "desc"] },
                "bad_object": { "type": "object", "enum": ["asc", "desc"] },
                "bad_array": { "type": "array", "enum": ["asc", "desc"] },
                "bad_number": { "type": "number", "enum": [1, 2] },
                "bad_mixed": { "enum": ["ok", 1] },
                "nested": {
                    "type": "object",
                    "properties": {
                        "child": { "enum": [true, false] }
                    }
                }
            }
        });

        normalize_tool_input_schema(&mut schema);

        assert!(
            schema["properties"]["good"]["enum"].is_array(),
            "string enums should be preserved"
        );
        assert!(
            schema["properties"]["good_nullable"]["enum"].is_array(),
            "string|null enums should be preserved"
        );
        assert!(
            schema["properties"]["bad_object"]["enum"].is_null(),
            "object enums should be dropped"
        );
        assert!(
            schema["properties"]["bad_array"]["enum"].is_null(),
            "array enums should be dropped"
        );
        assert!(
            schema["properties"]["bad_number"]["enum"].is_null(),
            "non-string enums should be dropped"
        );
        assert!(
            schema["properties"]["bad_mixed"]["enum"].is_null(),
            "mixed enums should be dropped"
        );
        assert!(
            schema["properties"]["nested"]["properties"]["child"]["enum"].is_null(),
            "recursive non-string enums should be dropped"
        );
    }

    #[test]
    fn extract_auth_challenge_from_result_payload() {
        let payload = json!({
            "content": [
                {
                    "type": "text",
                    "llm_instructions": "Authorize Gmail access first.",
                    "authorization_url": "https://example.com/oauth/start"
                }
            ]
        });
        let challenge = extract_auth_challenge(&payload, "gmail_whoami")
            .expect("auth challenge should be detected");
        assert_eq!(challenge.tool_name, "gmail_whoami");
        assert_eq!(
            challenge.authorization_url,
            "https://example.com/oauth/start"
        );
        assert_eq!(challenge.status, "pending");
    }

    #[test]
    fn extract_auth_challenge_returns_none_without_url() {
        let payload = json!({
            "content": [
                {"type":"text","text":"No authorization needed"}
            ]
        });
        assert!(extract_auth_challenge(&payload, "gmail_whoami").is_none());
    }

    #[test]
    fn extract_auth_challenge_prefers_structured_content_message() {
        let payload = json!({
            "content": [
                {
                    "type": "text",
                    "text": "{\"authorization_url\":\"https://example.com/oauth\",\"message\":\"json blob\"}"
                }
            ],
            "structuredContent": {
                "authorization_url": "https://example.com/oauth",
                "message": "Authorize Reddit access first."
            }
        });
        let challenge = extract_auth_challenge(&payload, "reddit_getmyusername")
            .expect("auth challenge should be detected");
        assert_eq!(challenge.message, "Authorize Reddit access first.");
    }

    #[test]
    fn sanitize_auth_message_compacts_llm_instructions() {
        let raw = "Please show the following link to the end user formatted as markdown: https://example.com/auth\nInform the end user that this tool requires authorization.";
        let message = sanitize_auth_message(raw);
        assert!(!message.contains('\n'));
        assert!(message.len() <= 283);
    }

    #[test]
    fn normalize_mcp_tool_args_maps_clickup_aliases() {
        let server = McpServer {
            name: "arcade".to_string(),
            transport: "https://example.com/mcp".to_string(),
            auth_kind: String::new(),
            enabled: true,
            connected: true,
            pid: None,
            last_error: None,
            last_auth_challenge: None,
            mcp_session_id: None,
            headers: HashMap::new(),
            secret_headers: HashMap::new(),
            tool_cache: vec![McpToolCacheEntry {
                tool_name: "Clickup_CreateTask".to_string(),
                description: "Create task".to_string(),
                input_schema: json!({
                    "type":"object",
                    "properties":{
                        "list_id":{"type":"string"},
                        "task_title":{"type":"string"}
                    },
                    "required":["list_id","task_title"]
                }),
                fetched_at_ms: 0,
                schema_hash: "x".to_string(),
            }],
            tools_fetched_at_ms: None,
            pending_auth_by_tool: HashMap::new(),
            allowed_tools: None,
            purpose: String::new(),
            grounding_required: false,
            secret_header_values: HashMap::new(),
            oauth: None,
        };

        let normalized = normalize_mcp_tool_args(
            &server,
            "Clickup_CreateTask",
            json!({
                "listId": "123",
                "name": "Prep fish"
            }),
        );
        assert_eq!(
            normalized.get("list_id").and_then(|v| v.as_str()),
            Some("123")
        );
        assert_eq!(
            normalized.get("task_title").and_then(|v| v.as_str()),
            Some("Prep fish")
        );
    }

    #[test]
    fn normalize_arg_key_ignores_case_and_separators() {
        assert_eq!(normalize_arg_key("task_title"), "tasktitle");
        assert_eq!(normalize_arg_key("taskTitle"), "tasktitle");
        assert_eq!(normalize_arg_key("task-title"), "tasktitle");
    }

    #[test]
    fn pending_auth_blocks_retries_within_cooldown() {
        let mut server = McpServer {
            name: "arcade".to_string(),
            transport: "https://example.com/mcp".to_string(),
            auth_kind: String::new(),
            enabled: true,
            connected: true,
            pid: None,
            last_error: None,
            last_auth_challenge: None,
            mcp_session_id: None,
            headers: HashMap::new(),
            secret_headers: HashMap::new(),
            tool_cache: Vec::new(),
            tools_fetched_at_ms: None,
            pending_auth_by_tool: HashMap::new(),
            allowed_tools: None,
            purpose: String::new(),
            grounding_required: false,
            secret_header_values: HashMap::new(),
            oauth: None,
        };
        server.pending_auth_by_tool.insert(
            "clickup_whoami".to_string(),
            PendingMcpAuth {
                challenge_id: "abc".to_string(),
                authorization_url: "https://example.com/auth".to_string(),
                message: "Authorize ClickUp access.".to_string(),
                status: "pending".to_string(),
                first_seen_ms: 1_000,
                last_probe_ms: 2_000,
            },
        );
        let blocked =
            pending_auth_short_circuit(&server, "clickup_whoami", "Clickup_WhoAmI", 10_000, 15_000)
                .expect("should block");
        assert!(blocked.output.contains("Authorization pending"));
        assert!(blocked
            .mcp_auth
            .get("pending")
            .and_then(|v| v.as_bool())
            .unwrap_or(false));
    }

    #[test]
    fn pending_auth_allows_probe_after_cooldown() {
        let mut server = McpServer {
            name: "arcade".to_string(),
            transport: "https://example.com/mcp".to_string(),
            auth_kind: String::new(),
            enabled: true,
            connected: true,
            pid: None,
            last_error: None,
            last_auth_challenge: None,
            mcp_session_id: None,
            headers: HashMap::new(),
            secret_headers: HashMap::new(),
            tool_cache: Vec::new(),
            tools_fetched_at_ms: None,
            pending_auth_by_tool: HashMap::new(),
            allowed_tools: None,
            purpose: String::new(),
            grounding_required: false,
            secret_header_values: HashMap::new(),
            oauth: None,
        };
        server.pending_auth_by_tool.insert(
            "clickup_whoami".to_string(),
            PendingMcpAuth {
                challenge_id: "abc".to_string(),
                authorization_url: "https://example.com/auth".to_string(),
                message: "Authorize ClickUp access.".to_string(),
                status: "pending".to_string(),
                first_seen_ms: 1_000,
                last_probe_ms: 2_000,
            },
        );
        assert!(
            pending_auth_short_circuit(&server, "clickup_whoami", "Clickup_WhoAmI", 17_001, 15_000)
                .is_none(),
            "cooldown elapsed should allow re-probe"
        );
    }

    #[test]
    fn pending_auth_is_tool_scoped() {
        let mut server = McpServer {
            name: "arcade".to_string(),
            transport: "https://example.com/mcp".to_string(),
            auth_kind: String::new(),
            enabled: true,
            connected: true,
            pid: None,
            last_error: None,
            last_auth_challenge: None,
            mcp_session_id: None,
            headers: HashMap::new(),
            secret_headers: HashMap::new(),
            tool_cache: Vec::new(),
            tools_fetched_at_ms: None,
            pending_auth_by_tool: HashMap::new(),
            allowed_tools: None,
            purpose: String::new(),
            grounding_required: false,
            secret_header_values: HashMap::new(),
            oauth: None,
        };
        server.pending_auth_by_tool.insert(
            "gmail_sendemail".to_string(),
            PendingMcpAuth {
                challenge_id: "abc".to_string(),
                authorization_url: "https://example.com/auth".to_string(),
                message: "Authorize Gmail access.".to_string(),
                status: "pending".to_string(),
                first_seen_ms: 1_000,
                last_probe_ms: 2_000,
            },
        );
        assert!(pending_auth_short_circuit(
            &server,
            "gmail_sendemail",
            "Gmail_SendEmail",
            2_100,
            15_000
        )
        .is_some());
        assert!(pending_auth_short_circuit(
            &server,
            "clickup_whoami",
            "Clickup_WhoAmI",
            2_100,
            15_000
        )
        .is_none());
    }

    #[test]
    fn store_secret_ref_requires_matching_tenant_context() {
        let suffix = Uuid::new_v4();
        let security_dir = std::env::temp_dir().join(format!("mcp-auth-test-{suffix}"));
        let secret_id = format!("mcp_header::tenant::{suffix}::authorization");

        let current_tenant =
            TenantContext::explicit(format!("tenant-{suffix}"), "workspace", None);
        tandem_core::set_provider_auth_for_tenant_in_dir(
            &security_dir,
            &current_tenant,
            &secret_id,
            "tenant-secret",
        )
        .expect("store secret");
        let matching_ref = McpSecretRef::Store {
            secret_id: secret_id.clone(),
            tenant_context: current_tenant.clone(),
        };
        assert_eq!(
            resolve_secret_ref_value_with_loader(&matching_ref, &current_tenant, |tenant_context| {
                tandem_core::load_provider_auth_for_tenant_in_dir(&security_dir, tenant_context)
            })
            .as_deref(),
            Some("tenant-secret")
        );

        let mismatched_tenant = TenantContext::explicit("tenant", "other-workspace", None);
        assert!(
            resolve_secret_ref_value_with_loader(
                &matching_ref,
                &mismatched_tenant,
                |tenant_context| {
                    tandem_core::load_provider_auth_for_tenant_in_dir(&security_dir, tenant_context)
                },
            )
            .is_none(),
            "tenant mismatch should block secret lookup"
        );

        let _ = tandem_core::delete_provider_auth_for_tenant_in_dir(
            &security_dir,
            &current_tenant,
            &secret_id,
        );
        let _ = std::fs::remove_dir_all(security_dir);
    }

    #[test]
    fn env_secret_refs_are_local_only() {
        std::env::set_var("TANDEM_TEST_MCP_SECRET", "env-secret");
        let secret_ref = McpSecretRef::Env {
            env: "TANDEM_TEST_MCP_SECRET".to_string(),
        };

        assert_eq!(
            resolve_secret_ref_value(&secret_ref, &TenantContext::local_implicit()).as_deref(),
            Some("env-secret")
        );
        assert!(
            resolve_secret_ref_value(
                &secret_ref,
                &TenantContext::explicit("tenant", "workspace", None)
            )
            .is_none(),
            "explicit hosted tenants must not resolve process env-backed MCP secrets"
        );
        std::env::remove_var("TANDEM_TEST_MCP_SECRET");
    }

    #[tokio::test]
    async fn strict_mode_denies_local_implicit_tool_calls_before_dispatch() {
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        registry.set_strict_tenant_enforcement(true);

        // Denied before server lookup: no server named "any-server" exists,
        // so reaching "not found" would mean the guard did not fire first.
        let err = registry
            .call_tool_for_tenant(
                "any-server",
                "any_tool",
                json!({}),
                &TenantContext::local_implicit(),
            )
            .await
            .expect_err("strict mode must deny local-implicit tool calls");
        assert!(err.contains("ToolDenied { reason: TenantScope }"));
        assert!(err.contains("local-implicit"));

        // Explicit tenants pass the strict guard (and then fail later on the
        // missing server, proving the guard is scope-specific).
        let err = registry
            .call_tool_for_tenant(
                "any-server",
                "any_tool",
                json!({}),
                &TenantContext::explicit("org-a", "workspace-a", None),
            )
            .await
            .expect_err("server does not exist");
        assert!(err.contains("not found"));
    }

    #[test]
    fn mismatched_store_secret_headers_flags_only_foreign_store_refs() {
        let tenant_a = TenantContext::explicit("tenant-a", "workspace-a", None);
        let tenant_b = TenantContext::explicit("tenant-b", "workspace-b", None);

        let secret_headers = HashMap::from([
            (
                "Authorization".to_string(),
                McpSecretRef::Store {
                    secret_id: "secret-a".to_string(),
                    tenant_context: tenant_a.clone(),
                },
            ),
            (
                "X-Api-Key".to_string(),
                McpSecretRef::Store {
                    secret_id: "secret-b".to_string(),
                    tenant_context: tenant_b.clone(),
                },
            ),
            (
                "X-Env-Token".to_string(),
                McpSecretRef::Env {
                    env: "SOME_ENV".to_string(),
                },
            ),
        ]);

        let mismatched = mismatched_store_secret_headers(&secret_headers, &tenant_b);
        assert_eq!(
            mismatched,
            vec!["Authorization".to_string()],
            "only store refs owned by a different tenant are flagged; env refs are not"
        );

        let denial = store_secret_tenant_denial_error("server", "tool", &mismatched);
        assert!(denial.contains("ToolDenied { reason: TenantScope }"));
        assert!(denial.contains("Authorization"));

        assert!(
            mismatched_store_secret_headers(&secret_headers, &TenantContext::local_implicit())
                .is_empty(),
            "local implicit tenants skip the store-secret tenant check"
        );
    }

    #[test]
    fn mismatched_store_secret_headers_treats_deployments_as_distinct_tenants() {
        let mut deployment_one = TenantContext::explicit("org-a", "workspace-a", None);
        deployment_one.deployment_id = Some("deployment-1".to_string());
        let mut deployment_two = deployment_one.clone();
        deployment_two.deployment_id = Some("deployment-2".to_string());

        let secret_headers = HashMap::from([(
            "Authorization".to_string(),
            McpSecretRef::Store {
                secret_id: "secret-one".to_string(),
                tenant_context: deployment_one.clone(),
            },
        )]);

        assert!(
            mismatched_store_secret_headers(&secret_headers, &deployment_one).is_empty(),
            "same deployment resolves its own secret"
        );
        assert_eq!(
            mismatched_store_secret_headers(&secret_headers, &deployment_two),
            vec!["Authorization".to_string()],
            "a different deployment in the same org/workspace must be denied"
        );
    }

    #[test]
    fn explicit_tenant_mcp_secret_headers_are_not_cached_globally() {
        let current_tenant = TenantContext::explicit("tenant", "workspace", None);
        let (headers, secret_headers, secret_values) = split_headers_for_storage(
            "tenant-server",
            HashMap::from([(
                "Authorization".to_string(),
                "Bearer tenant-secret".to_string(),
            )]),
            HashMap::new(),
            &current_tenant,
        );

        assert!(headers.is_empty());
        assert!(secret_headers.contains_key("Authorization"));
        assert!(
            secret_values.is_empty(),
            "explicit hosted MCP secrets must not be cached in shared server rows"
        );

        let _ = tandem_core::delete_provider_auth_for_tenant(
            &current_tenant,
            "mcp_header::tenant-server::authorization",
        );
    }

    #[tokio::test]
    async fn call_tool_for_tenant_resolves_matching_tenant_store_secret() {
        let (endpoint, server) =
            spawn_auth_required_http_mcp_server("Bearer tenant-a-secret").await;
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        let server_name = "tenant-server-matching";
        let tenant_a = TenantContext::explicit("tenant-a", "workspace-a", None);
        registry
            .add_or_update_with_secret_refs(
                server_name.to_string(),
                endpoint,
                HashMap::from([(
                    "Authorization".to_string(),
                    "Bearer tenant-a-secret".to_string(),
                )]),
                HashMap::new(),
                &tenant_a,
                true,
            )
            .await;

        assert!(registry.connect(server_name).await);
        let result = registry
            .call_tool_for_tenant(server_name, "get_me", json!({}), &tenant_a)
            .await
            .expect("tenant A should execute with tenant A secret");
        assert!(result.output.contains("tenant-authenticated"));
        server.abort();
        let _ = tandem_core::delete_provider_auth_for_tenant(
            &tenant_a,
            &format!("mcp_header::{server_name}::authorization"),
        );
    }

    #[tokio::test]
    async fn call_tool_for_tenant_rejects_mismatched_tenant_store_secret() {
        let (endpoint, server) =
            spawn_auth_required_http_mcp_server("Bearer tenant-a-secret").await;
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        let server_name = "tenant-server-mismatch-connected";
        let tenant_a = TenantContext::explicit("tenant-a", "workspace-a", None);
        let tenant_b = TenantContext::explicit("tenant-b", "workspace-b", None);
        registry
            .add_or_update_with_secret_refs(
                server_name.to_string(),
                endpoint,
                HashMap::from([(
                    "Authorization".to_string(),
                    "Bearer tenant-a-secret".to_string(),
                )]),
                HashMap::new(),
                &tenant_a,
                true,
            )
            .await;

        assert!(registry.connect(server_name).await);
        let err = registry
            .call_tool_for_tenant(server_name, "get_me", json!({}), &tenant_b)
            .await
            .expect_err("tenant B must not execute with tenant A secret");
        assert!(err.contains("ToolDenied { reason: TenantScope }"));
        assert!(err.contains("Authorization"));
        server.abort();
        let _ = tandem_core::delete_provider_auth_for_tenant(
            &tenant_a,
            &format!("mcp_header::{server_name}::authorization"),
        );
    }

    #[tokio::test]
    async fn call_tool_for_tenant_rejects_mismatched_secret_before_reconnect() {
        let (endpoint, server) =
            spawn_auth_required_http_mcp_server("Bearer tenant-a-secret").await;
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file);
        let server_name = "tenant-server-mismatch-before-reconnect";
        let tenant_a = TenantContext::explicit("tenant-a", "workspace-a", None);
        let tenant_b = TenantContext::explicit("tenant-b", "workspace-b", None);
        registry
            .add_or_update_with_secret_refs(
                server_name.to_string(),
                endpoint,
                HashMap::from([(
                    "Authorization".to_string(),
                    "Bearer tenant-a-secret".to_string(),
                )]),
                HashMap::new(),
                &tenant_a,
                true,
            )
            .await;

        let err = registry
            .call_tool_for_tenant(server_name, "get_me", json!({}), &tenant_b)
            .await
            .expect_err("tenant B must be denied before reconnecting tenant A server");
        assert!(err.contains("ToolDenied { reason: TenantScope }"));
        let tenant_server = registry
            .list()
            .await
            .get(server_name)
            .cloned()
            .expect("tenant server row");
        assert!(
            !tenant_server.connected,
            "tenant mismatch should deny before readiness reconnect"
        );
        server.abort();
        let _ = tandem_core::delete_provider_auth_for_tenant(
            &tenant_a,
            &format!("mcp_header::{server_name}::authorization"),
        );
    }
}
