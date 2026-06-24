use super::*;

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
    state.oauth.mcp_sessions().write().await.insert(
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
        .oauth
        .mcp_sessions()
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
async fn mcp_delete_auth_clears_canonical_oauth_material_without_oauth_config() {
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

    let before = state
        .mcp
        .list()
        .await
        .get("notion")
        .cloned()
        .expect("notion row before delete");
    assert!(before.oauth.is_none());
    assert!(tandem_core::load_provider_oauth_credential_in_dir(
        &provider_auth_security_dir,
        &provider_id,
    )
    .is_some());
    assert!(tandem_core::load_provider_oauth_credential(&provider_id).is_some());

    let app = app_router(state.clone());
    let req = Request::builder()
        .method("DELETE")
        .uri("/mcp/notion/auth")
        .body(Body::empty())
        .expect("delete auth request");
    let resp = app.oneshot(req).await.expect("delete auth response");
    assert_eq!(resp.status(), StatusCode::OK);

    assert!(tandem_core::load_provider_oauth_credential_in_dir(
        &provider_auth_security_dir,
        &provider_id,
    )
    .is_none());
    assert!(tandem_core::load_provider_oauth_credential(&provider_id).is_none());
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
        .oauth
        .mcp_sessions()
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
    let callback_stored = tandem_core::load_provider_oauth_credential_in_dir(
        &provider_auth_security_dir,
        "mcp-oauth::notion",
    )
    .expect("callback writes the current credential to the registry-local store");
    assert_eq!(callback_stored.access_token, "access-token-123");
    assert_eq!(callback_stored.refresh_token, "refresh-token-123");
    assert!(tandem_core::delete_provider_credential_for_tenant_in_dir(
        &provider_auth_security_dir,
        &tandem_types::TenantContext::local_implicit(),
        "mcp-oauth::notion",
    )
    .expect("delete registry-local credential for fallback exercise"));
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
