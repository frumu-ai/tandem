// A refresh may advance only its own exact predecessor. Public credential
// replacement still rotates generation and invalidates all older requests.
#[derive(Clone)]
struct McpOAuthRefreshPredecessor {
    server_policy: Value,
    connection_generation: Option<String>,
    oauth: McpOAuthConfig,
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct McpOAuthCredentialKey(String);

impl McpOAuthCredentialKey {
    fn new(tenant: &TenantContext, provider: &str) -> Self {
        Self(tandem_core::provider_credential_storage_key(
            tenant, provider,
        ))
    }
}

type McpOAuthRefreshCoordinators = HashMap<McpOAuthCredentialKey, Arc<Mutex<McpOAuthRefreshState>>>;

#[derive(Default)]
struct McpOAuthRefreshState {
    transitions: HashMap<String, McpOAuthRefreshTransition>,
}

struct McpOAuthRefreshTransition {
    predecessor: McpOAuthRefreshPredecessor,
    server_policy: Value,
    connection_generation: Option<String>,
    credential_digest: String,
}

fn oauth_credential_digest(
    credential: &tandem_core::OAuthProviderCredential,
) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let bytes = serde_json::to_vec(&(
        &credential.access_token,
        &credential.refresh_token,
        credential.expires_at_ms,
    ))
    .map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

impl McpOAuthRefreshTransition {
    fn matches_successor(
        &self,
        current: &McpOAuthRefreshPredecessor,
        credential: &tandem_core::OAuthProviderCredential,
    ) -> Result<bool, String> {
        Ok(self.server_policy == current.server_policy
            && self.connection_generation == current.connection_generation
            && self.predecessor.oauth == current.oauth
            && self.credential_digest == oauth_credential_digest(credential)?)
    }
}

impl McpRegistry {
    async fn ensure_oauth_bearer_token_fresh(
        &self,
        name: &str,
        force: bool,
    ) -> Result<bool, String> {
        self.ensure_oauth_bearer_token_fresh_bound(name, &local_tenant_context(), force, None)
            .await
    }

    async fn ensure_oauth_bearer_token_fresh_bound(
        &self,
        name: &str,
        current_tenant: &TenantContext,
        force: bool,
        binding: Option<&mut McpToolDispatchBinding>,
    ) -> Result<bool, String> {
        let Some(admitted) = self
            .capture_oauth_refresh_predecessor(name, current_tenant)
            .await?
        else {
            return Ok(false);
        };
        let registry = self.clone();
        let name = name.to_string();
        let tenant = current_tenant.clone();
        let mut owned_binding = binding.as_deref().cloned();
        // A rotating-token exchange is not cancel-safe once sent. Let the
        // authorized operation finish even when one waiting caller disappears;
        // it retains both the keyed lock and the existing commit-time checks.
        let (result, successor) = tokio::spawn(async move {
            let result = registry
                .refresh_oauth_credential_serialized(
                    &name,
                    &tenant,
                    force,
                    admitted,
                    owned_binding.as_mut(),
                )
                .await;
            (result, owned_binding)
        })
        .await
        .map_err(|_| "MCP OAuth refresh task failed".to_string())?;
        if let (Some(binding), Some(successor)) = (binding, successor) {
            *binding = successor;
        }
        result
    }

    async fn refresh_oauth_credential_serialized(
        &self,
        name: &str,
        current_tenant: &TenantContext,
        force: bool,
        admitted: McpOAuthRefreshPredecessor,
        mut binding: Option<&mut McpToolDispatchBinding>,
    ) -> Result<bool, String> {
        let key = McpOAuthCredentialKey::new(current_tenant, &admitted.oauth.provider_id);
        let coordinator = self
            .oauth_refreshes
            .lock()
            .await
            .entry(key.clone())
            .or_default()
            .clone();
        // Serialize before loading the rotating token, but leave revocation and
        // unrelated credentials free to proceed while the endpoint is awaited.
        let mut refresh_state = coordinator.lock().await;
        let Some(predecessor) = self
            .capture_oauth_refresh_predecessor(name, current_tenant)
            .await?
        else {
            return Ok(false);
        };
        if McpOAuthCredentialKey::new(current_tenant, &predecessor.oauth.provider_id) != key {
            return Err("MCP OAuth configuration changed while awaiting refresh".into());
        }
        let oauth = &predecessor.oauth;
        let credential = if current_tenant.is_local_implicit() {
            tandem_core::load_provider_oauth_credential_in_dir(
                &self.oauth_security_dir,
                &oauth.provider_id,
            )
            .or_else(|| tandem_core::load_provider_oauth_credential(&oauth.provider_id))
        } else {
            tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
                &self.oauth_security_dir,
                current_tenant,
                &oauth.provider_id,
            )
            .or_else(|| {
                tandem_core::load_provider_oauth_credential_for_tenant(
                    current_tenant,
                    &oauth.provider_id,
                )
            })
        };
        let Some(credential) = credential else {
            return Ok(false);
        };

        let connection_id = self.connection_id_for_tenant(name, current_tenant);
        if let Some(transition) = refresh_state.transitions.get(&connection_id) {
            if transition.matches_successor(&predecessor, &credential)? {
                if let Some(binding) = binding.as_deref_mut() {
                    if binding.server_policy == transition.predecessor.server_policy
                        && binding.connection_generation
                            == transition.predecessor.connection_generation
                    {
                        let mut successor = binding.clone();
                        successor.server_policy = transition.server_policy.clone();
                        successor.connection_generation = transition.connection_generation.clone();
                        // Revalidate this waiter's own authority and the exact
                        // recorded successor, never blindly adopt current state.
                        successor.revalidate()?;
                        *binding = successor;
                        return Ok(true);
                    }
                } else if admitted.server_policy == transition.predecessor.server_policy
                    && admitted.connection_generation
                        == transition.predecessor.connection_generation
                    && admitted.oauth == transition.predecessor.oauth
                {
                    return Ok(true);
                }
            }
        }
        if let Some(binding) = binding.as_deref() {
            binding.revalidate()?;
        }

        let should_refresh = force
            || credential.expires_at_ms <= now_ms().saturating_add(60_000)
            || credential.access_token.trim().is_empty();
        if !should_refresh {
            return Ok(false);
        }
        let mut endpoint_authorization =
            McpEndpointAuthorization::for_registry(self, current_tenant);
        endpoint_authorization.tool_dispatch = binding.as_deref().cloned();

        let refreshed =
            refresh_mcp_oauth_credential(oauth, &credential, &endpoint_authorization).await?;
        let transition = self
            .commit_oauth_refresh(name, current_tenant, predecessor, refreshed, binding)
            .await?;
        refresh_state.transitions.insert(connection_id, transition);
        Ok(true)
    }

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
    ) -> Result<McpOAuthRefreshTransition, String> {
        let credential_digest = oauth_credential_digest(&refreshed)?;
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
        let transition = McpOAuthRefreshTransition {
            predecessor,
            server_policy: server_dispatch_policy(server),
            connection_generation: Some(connection.connection_generation.clone()),
            credential_digest,
        };
        if let Some(binding) = binding {
            // These are the deterministic changes made above, not a new
            // authorization captured after an uncontrolled network wait.
            binding.server_policy = transition.server_policy.clone();
            binding.connection_generation = transition.connection_generation.clone();
        }
        drop(connections);
        drop(servers);
        self.persist_state().await;
        Ok(transition)
    }
}
