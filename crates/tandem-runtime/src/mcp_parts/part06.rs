// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[cfg(test)]
mod credential_ordering_tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn private_endpoints_require_host_posture_and_local_identity() {
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file.clone());
        let local = TenantContext::local_implicit();
        let explicit = TenantContext::explicit_user_workspace(
            "org-a",
            "workspace-a",
            None,
            "alice",
        );

        assert!(
            !registry.allow_private_endpoint_for(&local),
            "local identity without verified host posture must fail closed"
        );
        registry.set_standalone_private_endpoint_access(true);
        assert!(registry.allow_private_endpoint_for(&local));
        assert!(
            !registry.allow_private_endpoint_for(&explicit),
            "host posture must not grant private egress to an explicit hosted tenant"
        );
        registry.set_strict_tenant_enforcement(true);
        assert!(
            !registry.allow_private_endpoint_for(&local),
            "hosted strict-tenant mode must fail closed even on a loopback bind"
        );
        registry.set_strict_tenant_enforcement(false);
        registry.set_standalone_private_endpoint_access(false);
        assert!(!registry.allow_private_endpoint_for(&local));
        let _ = std::fs::remove_file(file);
    }

    #[tokio::test]
    async fn auth_clear_and_replacement_share_one_mutation_order() {
        let _provider_auth_guard = super::tests::provider_auth_test_guard().await;
        let file = std::env::temp_dir().join(format!("mcp-test-{}.json", Uuid::new_v4()));
        let registry = McpRegistry::new_with_state_file(file.clone());
        let tenant = TenantContext::explicit_user_workspace(
            format!("credential-order-org-{}", Uuid::new_v4()),
            "workspace-a",
            None,
            "alice",
        );
        registry
            .add_or_update(
                "notion".to_string(),
                "https://example.com/mcp".to_string(),
                HashMap::new(),
                true,
            )
            .await;
        registry
            .set_bearer_token_for_tenant("notion", "old-token", &tenant)
            .await
            .expect("set initial token");

        let mutation_guard = registry.credential_mutation_lock.lock().await;
        let clear_registry = registry.clone();
        let clear_tenant = tenant.clone();
        let clear_task = tokio::spawn(async move {
            clear_registry
                .clear_auth_material_for_tenant("notion", &clear_tenant)
                .await
        });
        let replace_registry = registry.clone();
        let replace_tenant = tenant.clone();
        let replace_task = tokio::spawn(async move {
            replace_registry
                .set_bearer_token_for_tenant("notion", "replacement-token", &replace_tenant)
                .await
        });
        tokio::task::yield_now().await;
        assert!(
            !clear_task.is_finished() && !replace_task.is_finished(),
            "clear and replacement must both wait for the credential mutation order"
        );
        drop(mutation_guard);

        assert!(
            clear_task.await.expect("clear task"),
            "clear must find the tenant connection"
        );
        assert!(
            replace_task
                .await
                .expect("replacement task")
                .expect("replace token"),
            "replacement must find the MCP server"
        );
        let connection_id = registry.connection_id_for_tenant("notion", &tenant);
        let connections = registry.list_connections().await;
        let connection = connections
            .get(&connection_id)
            .expect("tenant connection remains present");
        if let Some(secret_ref) = connection.secret_headers.get("Authorization") {
            assert_eq!(
                resolve_secret_ref_value(secret_ref, &tenant).as_deref(),
                Some("Bearer replacement-token"),
                "a surviving replacement ref must retain its backing credential"
            );
        } else {
            assert!(
                connection.credential_ref.is_none(),
                "a winning clear must leave no credential reference"
            );
        }

        let _ = registry
            .clear_auth_material_for_tenant("notion", &tenant)
            .await;
        let _ = std::fs::remove_file(file);
    }
}
