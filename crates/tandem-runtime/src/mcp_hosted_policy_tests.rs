use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[tokio::test]
async fn hosted_policy_revocation_during_mcp_readiness_sends_no_tool_request() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
    let revoked = Arc::new(AtomicBool::new(false));
    let effects = Arc::new(AtomicUsize::new(0));
    let server_revoked = revoked.clone();
    let server_effects = effects.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0u8; 4096];
                let count = socket.read(&mut chunk).await.unwrap();
                assert!(count > 0 && bytes.len() < 16384);
                bytes.extend_from_slice(&chunk[..count]);
                let text = String::from_utf8_lossy(&bytes);
                if let Some((head, body)) = text.split_once("\r\n\r\n") {
                    let length = head
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if body.len() >= length {
                        break;
                    }
                }
            }
            let text = String::from_utf8(bytes).unwrap();
            let (_, body) = text.split_once("\r\n\r\n").unwrap();
            let request: Value = serde_json::from_str(body).unwrap();
            let result = match request["method"].as_str().unwrap() {
                "initialize" => json!({"protocolVersion": MCP_PROTOCOL_VERSION, "capabilities": {},
                    "serverInfo": {"name": "synthetic", "version": "1"}}),
                "tools/list" => {
                    server_revoked.store(true, Ordering::SeqCst);
                    json!({"tools": [{"name": "get_me", "description": "test", "inputSchema": {"type": "object"}}]})
                }
                "tools/call" => {
                    server_effects.fetch_add(1, Ordering::SeqCst);
                    json!({"content": []})
                }
                _ => json!({}),
            };
            let response =
                json!({"jsonrpc": "2.0", "id": request["id"], "result": result}).to_string();
            let response = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response.len(), response);
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let file =
        std::env::temp_dir().join(format!("mcp-hosted-policy-{}.json", uuid::Uuid::new_v4()));
    let registry = McpRegistry::new_with_state_file(file.clone());
    registry.allow_private_endpoints_for_tests();
    registry
        .add_or_update("test".into(), endpoint, HashMap::new(), true)
        .await;
    let authority = crate::McpRequestAuthority::new(move || {
        if revoked.load(Ordering::SeqCst) {
            Err("hosted membership revoked".into())
        } else {
            Ok(())
        }
    });
    let error = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        registry.call_tool_for_tenant_with_authority(
            "test",
            "get_me",
            json!({}),
            &TenantContext::local_implicit(),
            Some(authority),
        ),
    )
    .await
    .unwrap()
    .unwrap_err();
    assert!(error.contains("hosted membership revoked"), "{error}");
    assert_eq!(effects.load(Ordering::SeqCst), 0);
    server.abort();
    let _ = std::fs::remove_file(file);
}

#[tokio::test]
async fn hosted_policy_mcp_send_rechecks_connector_and_connection_changes() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/mcp", listener.local_addr().unwrap());
    for mutation in ["disable", "remove", "transport", "tool", "connection"] {
        let file =
            std::env::temp_dir().join(format!("mcp-hosted-policy-{}.json", uuid::Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file.clone());
        registry.allow_private_endpoints_for_tests();
        registry
            .add_or_update("test".into(), endpoint.clone(), HashMap::new(), true)
            .await;
        let tenant = TenantContext::local_implicit();
        let row = registry.servers.read().await["test"].clone();
        let connection = registry
            .connection_for_tenant("test", &tenant)
            .await
            .unwrap();
        let mut authorization = McpEndpointAuthorization::for_registry(&registry, &tenant);
        authorization.tool_dispatch = Some(McpToolDispatchBinding {
            registry: registry.clone(),
            server_name: "test".into(),
            tool_name: "get_me".into(),
            server_policy: server_dispatch_policy(&row),
            tenant,
            connection_generation: Some(connection.connection_generation),
            request_authority: None,
        });
        match mutation {
            "remove" => {
                registry.servers.write().await.remove("test");
            }
            "connection" => {
                registry
                    .connections
                    .write()
                    .await
                    .remove(&connection.connection_id);
            }
            other => {
                let mut servers = registry.servers.write().await;
                let row = servers.get_mut("test").unwrap();
                match other {
                    "disable" => row.enabled = false,
                    "transport" => row.transport = "https://changed.example.com/mcp".into(),
                    "tool" => row.allowed_tools = Some(vec![]),
                    _ => unreachable!(),
                }
            }
        }
        let error = post_json_rpc_with_session(
            &endpoint,
            &HashMap::new(),
            json!({"method": "tools/call", "params": {"name": "get_me"}}),
            None,
            &authorization,
        )
        .await
        .unwrap_err();
        assert!(error.contains("before dispatch"), "{mutation}: {error}");
        let _ = std::fs::remove_file(file);
    }
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(20), listener.accept())
            .await
            .is_err(),
        "revoked dispatches must never open a tool connection"
    );
}
