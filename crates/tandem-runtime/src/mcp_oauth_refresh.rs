// A refresh may advance only its own exact predecessor. Public credential
// replacement still rotates generation and invalidates all older requests.
struct McpOAuthRefreshPredecessor {
    server_policy: Value,
    connection_generation: Option<String>,
    oauth: McpOAuthConfig,
}

impl McpRegistry {
    async fn capture_oauth_refresh_predecessor(
        &self,
        name: &str,
        tenant: &TenantContext,
    ) -> Result<Option<McpOAuthRefreshPredecessor>, String> {
        let servers = self.servers.read().await;
        let connections = self.connections.read().await;
        let server = servers
            .get(name)
            .ok_or_else(|| format!("MCP server '{name}' not found"))?;
        let owner = McpPrincipalRef::from_tenant_context(tenant);
        let connection = connections.get(&mcp_connection_id(name, tenant, &owner));
        let oauth = if tenant.is_local_implicit() {
            server.oauth.clone()
        } else {
            connection.and_then(|row| row.oauth.clone())
        };
        Ok(oauth.map(|oauth| McpOAuthRefreshPredecessor {
            server_policy: server_dispatch_policy(server),
            connection_generation: connection.map(|row| row.connection_generation.clone()),
            oauth,
        }))
    }

    async fn commit_oauth_refresh(
        &self,
        name: &str,
        tenant: &TenantContext,
        predecessor: McpOAuthRefreshPredecessor,
        refreshed: tandem_core::OAuthProviderCredential,
        binding: Option<&mut McpToolDispatchBinding>,
    ) -> Result<(), String> {
        let _credential_guard = self.credential_mutation_lock.lock().await;
        // Keep both authority registries stable through comparison, credential
        // writes and successor construction. No network I/O under these locks.
        let mut servers = self.servers.write().await;
        let mut connections = self.connections.write().await;
        let server = servers
            .get_mut(name)
            .ok_or("MCP connector removed during OAuth refresh")?;
        let owner = McpPrincipalRef::from_tenant_context(tenant);
        let connection_id = mcp_connection_id(name, tenant, &owner);
        let connection = connections.get(&connection_id);
        let current_oauth = if tenant.is_local_implicit() {
            server.oauth.as_ref()
        } else {
            connection.and_then(|row| row.oauth.as_ref())
        };
        if !server.enabled
            || connection.is_some_and(|row| !row.enabled)
            || server_dispatch_policy(server) != predecessor.server_policy
            || connection.map(|row| &row.connection_generation)
                != predecessor.connection_generation.as_ref()
            || current_oauth != Some(&predecessor.oauth)
        {
            return Err("MCP authority changed during OAuth refresh".into());
        }
        if let Some(binding) = binding.as_ref() {
            binding.validate_snapshot(server, connection)?;
        }
        let token = refreshed.access_token.trim().to_string();
        if token.is_empty() {
            return Err("oauth access token cannot be empty".into());
        }
        let header_name = "Authorization".to_string();
        let secret_id = mcp_header_secret_id_for_tenant(name, &header_name, tenant);
        let secret_ref = McpSecretRef::Store {
            secret_id: secret_id.clone(),
            tenant_context: tenant.clone(),
        };
        // Both records belong to the verified predecessor. A failed write
        // returns without granting the request a successor dispatch binding.
        if tenant.is_local_implicit() {
            tandem_core::set_provider_oauth_credential_in_dir(
                &self.oauth_security_dir,
                &predecessor.oauth.provider_id,
                refreshed,
            )
            .map_err(|error| error.to_string())?;
        } else {
            tandem_core::set_provider_oauth_credential_for_tenant_in_dir(
                &self.oauth_security_dir,
                tenant,
                &predecessor.oauth.provider_id,
                refreshed,
            )
            .map_err(|error| error.to_string())?;
        }
        tandem_core::set_provider_auth_for_tenant(tenant, &secret_id, &format!("Bearer {token}"))
            .map_err(|error| error.to_string())?;
        if tenant.is_local_implicit() {
            server
                .secret_headers
                .insert(header_name.clone(), secret_ref.clone());
            server
                .secret_header_values
                .insert(header_name.clone(), format!("Bearer {token}"));
            server.headers.remove(&header_name);
        }
        let now = now_ms();
        let connection = connections.entry(connection_id).or_insert_with(|| {
            McpConnection::tenant_connection_from_server(name, server, tenant.clone(), owner, now)
        });
        if tenant.is_local_implicit() {
            connection.secret_headers = server.secret_headers.clone();
            connection.credential_ref = compatibility_credential_ref(name, server);
        } else {
            connection
                .secret_headers
                .insert(header_name.clone(), secret_ref.clone());
            if connection.credential_ref.is_none() {
                connection.credential_ref = Some(McpCredentialRef {
                    provider: "mcp_header".into(),
                    secret_id: format!(
                        "{}::{}::{}",
                        name.trim(),
                        header_name.to_ascii_lowercase(),
                        secret_ref_stable_id(&secret_ref)
                    ),
                    credential_version: None,
                    expires_at_ms: None,
                });
            }
        }
        connection.connection_generation = new_mcp_connection_generation();
        connection.updated_at_ms = now;
        if let Some(binding) = binding {
            // These are the deterministic changes made above, not a new
            // authorization captured after an uncontrolled network wait.
            binding.server_policy = server_dispatch_policy(server);
            binding.connection_generation = Some(connection.connection_generation.clone());
        }
        drop(connections);
        drop(servers);
        self.persist_state().await;
        Ok(())
    }
}
