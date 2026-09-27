use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

async fn concurrent_refresh_case(explicit: bool, force: bool) {
    concurrent_refresh_case_with_revocation(explicit, force, false).await;
}

async fn concurrent_refresh_case_with_revocation(explicit: bool, force: bool, revoke_waiter: bool) {
    concurrent_refresh_case_with_cancellation(explicit, force, revoke_waiter, false).await;
}

async fn concurrent_refresh_case_with_cancellation(
    explicit: bool,
    force: bool,
    revoke_waiter: bool,
    cancel_first: bool,
) {
    let _auth_guard = super::tests::provider_auth_test_guard().await;
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/token", listener.local_addr().unwrap());
    let sent = Arc::new(AtomicUsize::new(0));
    let entered = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let server_sent = sent.clone();
    let server_entered = entered.clone();
    let server_release = release.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let sent = server_sent.clone();
            let entered = server_entered.clone();
            let release = server_release.clone();
            tokio::spawn(async move {
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
                let first = sent.fetch_add(1, Ordering::SeqCst) == 0;
                let (status, body) = if first {
                    entered.notify_one();
                    release.notified().await;
                    (
                        "200 OK",
                        json!({"access_token":"renewed", "refresh_token":"rotated", "expires_in":3600}),
                    )
                } else {
                    // Model a provider rejecting replay of its single-use token.
                    (
                        "400 Bad Request",
                        json!({"error":"refresh_token_already_used"}),
                    )
                };
                let body = body.to_string();
                let response = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = socket.write_all(response.as_bytes()).await;
            });
        }
    });
    let directory = std::env::temp_dir().join(format!("mcp-refresh-race-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let registry = McpRegistry::new_with_state_file(directory.join("state.json"));
    registry.allow_private_endpoints_for_tests();
    let name = format!("refresh-race-{}", uuid::Uuid::new_v4());
    let tenant = if explicit {
        TenantContext::explicit("refresh-race-org", "workspace", Some(name.clone()))
    } else {
        TenantContext::local_implicit()
    };
    // A refresh for an unrelated stored credential must not block this one.
    let other_coordinator = registry
        .oauth_refreshes
        .lock()
        .await
        .entry(McpOAuthCredentialKey::new(&tenant, "unrelated-provider"))
        .or_default()
        .clone();
    let _unrelated_refresh = other_coordinator.lock().await;
    registry
        .add_or_update(name.clone(), endpoint.clone(), HashMap::new(), true)
        .await;
    registry.set_auth_kind(&name, "oauth".into()).await;
    registry
        .set_oauth_refresh_config_for_tenant(
            &name,
            name.clone(),
            endpoint,
            "test-client".into(),
            None,
            &tenant,
        )
        .await
        .unwrap();
    let credential = tandem_core::OAuthProviderCredential {
        provider_id: name.clone(),
        access_token: "old".into(),
        refresh_token: "single-use".into(),
        expires_at_ms: if force { now_ms() + 3_600_000 } else { 1 },
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
    let snapshot = registry.servers.read().await.get(&name).unwrap().clone();
    let binding = McpToolDispatchBinding {
        registry: registry.clone(),
        server_name: name.clone(),
        tool_name: "get_me".into(),
        server_policy: server_dispatch_policy(&snapshot),
        tenant: tenant.clone(),
        connection_generation: registry
            .connection_for_tenant(&name, &tenant)
            .await
            .map(|row| row.connection_generation),
        request_authority: None,
    };
    let launch = |mut binding: McpToolDispatchBinding| {
        let registry = registry.clone();
        let name = name.clone();
        let tenant = tenant.clone();
        tokio::spawn(async move {
            let result = registry
                .ensure_oauth_bearer_token_fresh_bound(&name, &tenant, force, Some(&mut binding))
                .await;
            (result, binding.revalidate())
        })
    };
    let mut stale_binding = binding.clone();
    let first = launch(binding.clone());
    tokio::time::timeout(std::time::Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    let first = if cancel_first {
        first.abort();
        assert!(first.await.unwrap_err().is_cancelled());
        None
    } else {
        Some(first)
    };
    let revoked = Arc::new(AtomicBool::new(false));
    let authority_revoked = revoked.clone();
    let mut waiter = binding;
    waiter.request_authority = Some(crate::McpRequestAuthority::new(move || {
        if authority_revoked.load(Ordering::SeqCst) {
            Err("waiter revoked".into())
        } else {
            Ok(())
        }
    }));
    let mut second = launch(waiter);
    // Keep the first response in flight while the concurrent caller has time to
    // reach the provider. A serialized waiter must stay pending, not replay.
    let early = tokio::time::timeout(std::time::Duration::from_millis(500), &mut second)
        .await
        .ok();
    revoked.store(revoke_waiter, Ordering::SeqCst);
    release.notify_one();
    let first = if let Some(first) = first {
        Some(
            tokio::time::timeout(std::time::Duration::from_secs(5), first)
                .await
                .unwrap()
                .unwrap(),
        )
    } else {
        None
    };
    let second = match early {
        Some(result) => result.unwrap(),
        None => tokio::time::timeout(std::time::Duration::from_secs(5), second)
            .await
            .unwrap()
            .unwrap(),
    };
    // A receipt must never let an old binding adopt a later public replacement.
    registry
        .set_bearer_token_for_tenant(&name, "external-replacement", &tenant)
        .await
        .unwrap();
    let replaced = registry
        .ensure_oauth_bearer_token_fresh_bound(&name, &tenant, force, Some(&mut stale_binding))
        .await;
    registry
        .clear_auth_material_for_tenant(&name, &tenant)
        .await;
    server.abort();
    std::fs::remove_dir_all(&directory).unwrap();
    assert_eq!(
        sent.load(Ordering::SeqCst),
        1,
        "a rotating credential must be sent once"
    );
    if let Some(first) = first {
        assert!(first.0.is_ok() && first.1.is_ok(), "first: {first:?}");
    }
    assert!(replaced.is_err(), "external replacement cannot be adopted");
    if revoke_waiter {
        assert!(
            second.0.is_err() && second.1.is_err(),
            "revoked waiter: {second:?}"
        );
    } else {
        assert!(second.0.is_ok() && second.1.is_ok(), "waiter: {second:?}");
    }
    if force && !revoke_waiter {
        assert!(second.0.unwrap(), "coalesced 401 refresh must enable retry");
    }
}

#[tokio::test]
async fn concurrent_oauth_proactive_refresh_retains_both_dispatch_bindings() {
    concurrent_refresh_case(false, false).await;
}

#[tokio::test]
async fn concurrent_oauth_forced_refresh_retains_both_dispatch_bindings() {
    concurrent_refresh_case(false, true).await;
}

#[tokio::test]
async fn concurrent_oauth_hosted_proactive_refresh_retains_both_dispatch_bindings() {
    concurrent_refresh_case(true, false).await;
}

#[tokio::test]
async fn concurrent_oauth_hosted_forced_refresh_retains_both_dispatch_bindings() {
    concurrent_refresh_case(true, true).await;
}

#[tokio::test]
async fn concurrent_oauth_waiter_revalidates_its_own_authority() {
    for explicit in [false, true] {
        for force in [false, true] {
            concurrent_refresh_case_with_revocation(explicit, force, true).await;
        }
    }
}

#[test]
fn oauth_refresh_key_matches_stored_credential_scope() {
    let alice = TenantContext::explicit_user_workspace("org", "workspace", None, "alice");
    let bob =
        TenantContext::explicit_user_workspace("org", "workspace", Some(String::new()), "bob");
    assert!(
        McpOAuthCredentialKey::new(&alice, " Provider ")
            == McpOAuthCredentialKey::new(&bob, "provider")
    );
    let other = TenantContext::explicit_user_workspace("org", "other-workspace", None, "alice");
    assert!(
        McpOAuthCredentialKey::new(&alice, "provider")
            != McpOAuthCredentialKey::new(&other, "provider")
    );
    assert!(
        McpOAuthCredentialKey::new(&alice, "provider")
            != McpOAuthCredentialKey::new(&TenantContext::local_implicit(), "provider")
    );
}

#[test]
fn oauth_refresh_key_coalesces_flat_storage_alias() {
    let explicit = TenantContext::explicit_user_workspace("o", "w", None, "alice");
    let local = TenantContext::local_implicit();
    assert!(
        McpOAuthCredentialKey::new(&explicit, "p")
            == McpOAuthCredentialKey::new(&local, "__tenant__::6f::77::::p")
    );
}

#[tokio::test]
async fn concurrent_oauth_cancellation_does_not_replay_consumed_token() {
    for explicit in [false, true] {
        for force in [false, true] {
            concurrent_refresh_case_with_cancellation(explicit, force, false, true).await;
        }
    }
}
