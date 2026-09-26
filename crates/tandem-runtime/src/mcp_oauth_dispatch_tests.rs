use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn oauth_dispatch_control(explicit: bool, connected: bool, force_401: bool) {
    oauth_dispatch_case(explicit, connected, force_401, None).await;
}

async fn oauth_dispatch_case(
    explicit: bool,
    connected: bool,
    force_401: bool,
    mutation: Option<&str>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let refreshes = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let server_refreshes = refreshes.clone();
    let server_calls = calls.clone();
    let refreshing = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let server_refreshing = refreshing.clone();
    let server_release = release.clone();
    let pause_refresh = mutation.is_some();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            loop {
                let mut chunk = [0; 4096];
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
            let (head, body) = text.split_once("\r\n\r\n").unwrap();
            let (status, response) = if head.starts_with("POST /token ") {
                server_refreshes.fetch_add(1, Ordering::SeqCst);
                if pause_refresh {
                    server_refreshing.notify_one();
                    server_release.notified().await;
                }
                (
                    "200 OK",
                    json!({"access_token":"renewed-test-token", "expires_in":3600}),
                )
            } else {
                let request: Value = serde_json::from_str(body).unwrap();
                let method = request["method"].as_str().unwrap();
                if method == "tools/call"
                    && force_401
                    && !head
                        .to_ascii_lowercase()
                        .contains("authorization: bearer renewed-test-token")
                {
                    ("401 Unauthorized", json!({"error":"expired token"}))
                } else {
                    let result = match method {
                        "initialize" => {
                            json!({"protocolVersion":MCP_PROTOCOL_VERSION, "capabilities":{},
                            "serverInfo":{"name":"oauth-test", "version":"1"}})
                        }
                        "tools/list" => {
                            json!({"tools":[{"name":"get_me", "inputSchema":{"type":"object"}}]})
                        }
                        "tools/call" => {
                            server_calls.fetch_add(1, Ordering::SeqCst);
                            json!({"content":[{"type":"text", "text":"success"}]})
                        }
                        _ => json!({}),
                    };
                    (
                        "200 OK",
                        json!({"jsonrpc":"2.0", "id":request["id"], "result":result}),
                    )
                }
            };
            let response = response.to_string();
            let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len());
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let directory =
        std::env::temp_dir().join(format!("tandem-oauth-dispatch-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let registry = McpRegistry::new_with_state_file(directory.join("state.json"));
    registry.allow_private_endpoints_for_tests();
    let name = format!("oauth-{}", uuid::Uuid::new_v4());
    let tenant = if explicit {
        TenantContext::explicit("oauth-test-org", "oauth-test-workspace", Some(name.clone()))
    } else {
        TenantContext::local_implicit()
    };
    registry
        .add_or_update(name.clone(), format!("{origin}/mcp"), HashMap::new(), true)
        .await;
    assert!(registry.set_auth_kind(&name, "oauth".into()).await);
    registry
        .set_oauth_refresh_config_for_tenant(
            &name,
            name.clone(),
            format!("{origin}/token"),
            "test-client".into(),
            None,
            &tenant,
        )
        .await
        .unwrap();
    // Deliberately no bearer header: configuring OAuth independently is a
    // supported path, and first renewal must materialize that header safely.
    let credential = tandem_core::OAuthProviderCredential {
        provider_id: name.clone(),
        access_token: "old-test-token".into(),
        refresh_token: "test-refresh".into(),
        expires_at_ms: if force_401 { now_ms() + 3_600_000 } else { 1 },
        account_id: None,
        email: None,
        display_name: None,
        managed_by: "test".into(),
        api_key: None,
    };
    if explicit {
        tandem_core::set_provider_oauth_credential_for_tenant_in_dir(
            &registry.oauth_security_dir,
            &tenant,
            &name,
            credential,
        )
        .unwrap();
    } else {
        tandem_core::set_provider_oauth_credential_in_dir(
            &registry.oauth_security_dir,
            &name,
            credential,
        )
        .unwrap();
    }
    if connected {
        registry
            .set_runtime_state_for_current_tenant(
                &name,
                &tenant,
                McpRuntimeState::connected(None, Vec::new(), now_ms()),
            )
            .await;
    }
    let revoked = Arc::new(AtomicBool::new(false));
    let request_revoked = revoked.clone();
    let request_registry = registry.clone();
    let request_name = name.clone();
    let request_tenant = tenant.clone();
    let request = tokio::spawn(async move {
        request_registry
            .call_tool_for_tenant_with_authority(
                &request_name,
                "get_me",
                json!({}),
                &request_tenant,
                Some(crate::McpRequestAuthority::new(move || {
                    if request_revoked.load(Ordering::SeqCst) {
                        Err("request revoked".into())
                    } else {
                        Ok(())
                    }
                })),
            )
            .await
    });
    if let Some(mutation) = mutation {
        tokio::time::timeout(std::time::Duration::from_secs(10), refreshing.notified())
            .await
            .unwrap();
        match mutation {
            "clear" => {
                assert!(
                    registry
                        .clear_auth_material_for_tenant(&name, &tenant)
                        .await
                );
            }
            "replace" => {
                assert!(registry
                    .set_bearer_token_for_tenant(&name, "replacement-test-token", &tenant)
                    .await
                    .unwrap());
            }
            "remove" => {
                assert!(registry.remove_for_tenant(&name, &tenant).await);
            }
            "revoke" => {
                revoked.store(true, Ordering::SeqCst);
            }
            _ => unreachable!(),
        }
        release.notify_one();
    }
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(10), request)
        .await
        .unwrap()
        .unwrap();
    let persisted = if explicit {
        tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
            &registry.oauth_security_dir,
            &tenant,
            &name,
        )
    } else {
        tandem_core::load_provider_oauth_credential_in_dir(&registry.oauth_security_dir, &name)
    };
    let auth = tandem_core::load_provider_auth_for_tenant(&tenant);
    let bearer = auth
        .get(&mcp_header_secret_id_for_tenant(
            &name,
            "Authorization",
            &tenant,
        ))
        .cloned();
    registry
        .clear_auth_material_for_tenant(&name, &tenant)
        .await;
    server.abort();
    std::fs::remove_dir_all(&directory).unwrap();
    assert_eq!(refreshes.load(Ordering::SeqCst), 1);
    if let Some(mutation) = mutation {
        assert!(outcome.is_err(), "{mutation}: {outcome:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 0, "{mutation}");
        assert!(
            persisted.is_none_or(|credential| credential.access_token != "renewed-test-token"),
            "{mutation}: stale refresh was saved"
        );
        assert_ne!(
            bearer.as_deref(),
            Some("Bearer renewed-test-token"),
            "{mutation}"
        );
        if mutation == "replace" {
            assert_eq!(bearer.as_deref(), Some("Bearer replacement-test-token"));
        }
    } else {
        assert!(outcome.is_ok(), "{outcome:?}");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }
}

#[tokio::test]
async fn oauth_dispatch_local_refresh_rejects_concurrent_authority_changes() {
    for (connected, force_401) in [(true, false), (false, false), (true, true)] {
        for mutation in ["clear", "replace", "remove", "revoke"] {
            oauth_dispatch_case(false, connected, force_401, Some(mutation)).await;
        }
    }
}

#[tokio::test]
async fn oauth_dispatch_hosted_refresh_rejects_concurrent_authority_changes() {
    for (connected, force_401) in [(true, false), (false, false), (true, true)] {
        for mutation in ["clear", "replace", "remove", "revoke"] {
            oauth_dispatch_case(true, connected, force_401, Some(mutation)).await;
        }
    }
}

#[tokio::test]
async fn oauth_dispatch_local_proactive_refresh() {
    oauth_dispatch_control(false, true, false).await;
}
#[tokio::test]
async fn oauth_dispatch_hosted_proactive_refresh() {
    oauth_dispatch_control(true, true, false).await;
}
#[tokio::test]
async fn oauth_dispatch_local_readiness_refresh() {
    oauth_dispatch_control(false, false, false).await;
}
#[tokio::test]
async fn oauth_dispatch_hosted_readiness_refresh() {
    oauth_dispatch_control(true, false, false).await;
}
#[tokio::test]
async fn oauth_dispatch_local_401_refresh() {
    oauth_dispatch_control(false, true, true).await;
}
#[tokio::test]
async fn oauth_dispatch_hosted_401_refresh() {
    oauth_dispatch_control(true, true, true).await;
}
