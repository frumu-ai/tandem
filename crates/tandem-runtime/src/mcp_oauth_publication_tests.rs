use super::*;

fn credential(token: &str) -> tandem_core::OAuthProviderCredential {
    tandem_core::OAuthProviderCredential {
        provider_id: "shared".into(),
        access_token: token.into(),
        refresh_token: format!("refresh-{token}"),
        expires_at_ms: now_ms() + 3_600_000,
        account_id: None,
        email: None,
        display_name: None,
        managed_by: "test".into(),
        api_key: None,
    }
}

async fn fixture() -> (McpRegistry, PathBuf, TenantContext, TenantContext) {
    let directory = PathBuf::from(std::env::var_os("TANDEM_HOME").unwrap())
        .join(format!("publication-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&directory).unwrap();
    let file = directory.join("state.json");
    let registry = McpRegistry::new_with_state_file(file.clone());
    let alice =
        TenantContext::explicit_user_workspace("publication-org", "workspace", None, "alice");
    let bob = TenantContext::explicit_user_workspace("publication-org", "workspace", None, "bob");
    registry
        .add_or_update(
            "shared-server".into(),
            "https://example.invalid/mcp".into(),
            HashMap::new(),
            true,
        )
        .await;
    registry
        .set_auth_kind("shared-server", "oauth".into())
        .await;
    for (tenant, provider) in [(&alice, "shared"), (&bob, " SHARED ")] {
        registry
            .set_oauth_refresh_config_for_tenant(
                "shared-server",
                provider.into(),
                "https://example.invalid/token".into(),
                "client".into(),
                Some("client-secret".into()),
                tenant,
            )
            .await
            .unwrap();
        registry
            .set_bearer_token_for_tenant("shared-server", "old", tenant)
            .await
            .unwrap();
    }
    registry
        .set_oauth_credential_for_tenant("shared", credential("old"), &alice)
        .await
        .unwrap();
    (registry, file, alice, bob)
}

async fn capture(registry: &McpRegistry, tenant: &TenantContext) -> McpOAuthRefreshCapture {
    McpOAuthRefreshCapture {
        selected: registry
            .capture_oauth_refresh_predecessor("shared-server", tenant)
            .await
            .unwrap()
            .unwrap(),
        credential_digest: oauth_credential_digest(
            &registry
                .load_oauth_refresh_credential(tenant, "shared")
                .unwrap(),
        )
        .unwrap(),
        participants: registry
            .capture_oauth_refresh_participants(&McpOAuthCredentialKey::new(tenant, "shared"))
            .await
            .unwrap(),
        save_credential: true,
    }
}

#[tokio::test]
async fn oauth_publication_failed_bearer_repairs_after_reload_without_exchange() {
    let _guard = super::tests::provider_auth_test_guard().await;
    let (registry, file, alice, bob) = fixture().await;
    let captured = capture(&registry, &alice).await;
    let security = PathBuf::from(std::env::var_os("TANDEM_HOME").unwrap()).join("security");
    let index = security.join("provider_auth_index.json");
    let backup = security.join("provider_auth_index.saved");
    std::fs::rename(&index, &backup).unwrap();
    std::fs::create_dir(&index).unwrap();
    let coordinator = McpOAuthRefreshState::default();
    let result = registry
        .commit_oauth_refresh(
            "shared-server",
            &alice,
            captured,
            credential("new"),
            None,
            &coordinator,
        )
        .await;
    assert!(
        result.is_err(),
        "bearer index failure must reach the caller"
    );
    assert!(
        coordinator.transitions.lock().unwrap().is_empty(),
        "failed writes grant no receipt"
    );
    assert_eq!(
        registry
            .load_oauth_refresh_credential(&alice, "shared")
            .unwrap()
            .access_token,
        "new"
    );
    for tenant in [&alice, &bob] {
        assert!(registry
            .connection_for_tenant("shared-server", tenant)
            .await
            .unwrap()
            .oauth_publication_pending
            .is_some());
    }
    std::fs::remove_dir(&index).unwrap();
    std::fs::rename(&backup, &index).unwrap();
    drop(registry);
    let reloaded = McpRegistry::new_with_state_file(file);
    for tenant in [&alice, &bob] {
        // The endpoint is deliberately invalid: success requires repairing the
        // saved credential, never sending a second refresh request.
        assert!(reloaded
            .ensure_oauth_bearer_token_fresh_bound("shared-server", tenant, true, None)
            .await
            .unwrap());
        let connection = reloaded
            .connection_for_tenant("shared-server", tenant)
            .await
            .unwrap();
        assert!(connection.oauth_publication_pending.is_none());
        assert_eq!(
            resolve_secret_ref_value(&connection.secret_headers["Authorization"], tenant)
                .as_deref(),
            Some("Bearer new")
        );
    }
}

#[tokio::test]
async fn oauth_publication_sibling_replacement_or_clear_invalidates_exchange() {
    let _guard = super::tests::provider_auth_test_guard().await;
    for clear in [false, true] {
        let (registry, _, alice, bob) = fixture().await;
        let captured = capture(&registry, &alice).await;
        if clear {
            assert!(
                registry
                    .clear_auth_material_for_tenant("shared-server", &bob)
                    .await
            );
        } else {
            registry
                .set_oauth_credential_for_tenant(" SHARED ", credential("replacement"), &bob)
                .await
                .unwrap();
        }
        let coordinator = McpOAuthRefreshState::default();
        assert!(registry
            .commit_oauth_refresh(
                "shared-server",
                &alice,
                captured,
                credential("stale-response"),
                None,
                &coordinator
            )
            .await
            .is_err());
        assert!(coordinator.transitions.lock().unwrap().is_empty());
        let stored = registry.load_oauth_refresh_credential(&alice, "shared");
        if clear {
            assert!(stored.is_none());
        } else {
            assert_eq!(stored.unwrap().access_token, "replacement");
        }
    }
}

#[tokio::test]
async fn oauth_publication_does_not_overwrite_changed_sibling() {
    let _guard = super::tests::provider_auth_test_guard().await;
    let (registry, _, alice, bob) = fixture().await;
    let captured = capture(&registry, &alice).await;
    registry
        .set_bearer_token_for_tenant("shared-server", "manual", &bob)
        .await
        .unwrap();
    let coordinator = McpOAuthRefreshState::default();
    registry
        .commit_oauth_refresh(
            "shared-server",
            &alice,
            captured,
            credential("new"),
            None,
            &coordinator,
        )
        .await
        .unwrap();
    let bob_connection = registry
        .connection_for_tenant("shared-server", &bob)
        .await
        .unwrap();
    assert_eq!(
        resolve_secret_ref_value(&bob_connection.secret_headers["Authorization"], &bob).as_deref(),
        Some("Bearer manual")
    );
    assert!(bob_connection.oauth_publication_pending.is_none());
    let transitions = coordinator.transitions.lock().unwrap();
    assert!(transitions.contains_key(&registry.connection_id_for_tenant("shared-server", &alice)));
    assert!(!transitions.contains_key(&registry.connection_id_for_tenant("shared-server", &bob)));
}
