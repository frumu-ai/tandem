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

type McpOAuthRefreshCoordinators = HashMap<McpOAuthCredentialKey, McpOAuthRefreshEntry>;

struct McpOAuthRefreshEntry {
    coordinator: Arc<McpOAuthRefreshState>,
    admitted: usize,
}

// Registered before capturing the admitted predecessor and owned by the
// detached exchange. Drop also covers cancellation before the task is spawned.
struct McpOAuthRefreshAdmission {
    index: Arc<std::sync::Mutex<McpOAuthRefreshCoordinators>>,
    key: McpOAuthCredentialKey,
    coordinator: Arc<McpOAuthRefreshState>,
}

impl Drop for McpOAuthRefreshAdmission {
    fn drop(&mut self) {
        let mut index = self.index.lock().expect("OAuth refresh index poisoned");
        if let Some(entry) = index.get_mut(&self.key) {
            if Arc::ptr_eq(&entry.coordinator, &self.coordinator) {
                entry.admitted -= 1;
                if entry.admitted == 0 {
                    index.remove(&self.key);
                }
            }
        }
    }
}

#[derive(Default)]
struct McpOAuthRefreshState {
    exchange: Mutex<()>,
    transitions: std::sync::Mutex<HashMap<String, McpOAuthRefreshTransition>>,
}

#[derive(Clone)]
struct McpOAuthRefreshTransition {
    predecessor_policy: Value,
    predecessor_generation: Option<String>,
    oauth_digest: String,
    server_policy: Value,
    connection_generation: Option<String>,
    credential_digest: String,
}

fn oauth_config_digest(oauth: &McpOAuthConfig) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    // The secret value is skipped by McpOAuthConfig's serializer, but remains
    // part of its equality contract. Include it explicitly in the receipt.
    let bytes = serde_json::to_vec(&(oauth, &oauth.client_secret_value))
        .map_err(|error| error.to_string())?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
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
            && self.oauth_digest == oauth_config_digest(&current.oauth)?
            && self.credential_digest == oauth_credential_digest(credential)?)
    }
}

impl McpRegistry {
    fn admit_oauth_refresh(&self, key: McpOAuthCredentialKey) -> McpOAuthRefreshAdmission {
        let mut index = self
            .oauth_refreshes
            .lock()
            .expect("OAuth refresh index poisoned");
        let entry = index
            .entry(key.clone())
            .or_insert_with(|| McpOAuthRefreshEntry {
                coordinator: Arc::new(McpOAuthRefreshState::default()),
                admitted: 0,
            });
        entry.admitted += 1;
        McpOAuthRefreshAdmission {
            index: self.oauth_refreshes.clone(),
            key,
            coordinator: entry.coordinator.clone(),
        }
    }

    // Never await the exchange mutex here: credential deletion must remain free
    // to revoke an exchange while its endpoint is blocked. Keep active keyed
    // coordinators in place so aliases cannot open a second exchange lock.
    fn invalidate_oauth_refresh_connection(&self, connection_id: &str) {
        let index = self
            .oauth_refreshes
            .lock()
            .expect("OAuth refresh index poisoned");
        for entry in index.values() {
            entry
                .coordinator
                .transitions
                .lock()
                .expect("OAuth receipts poisoned")
                .remove(connection_id);
        }
    }

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
        let Some(initial) = self
            .capture_oauth_refresh_predecessor(name, current_tenant)
            .await?
        else {
            return Ok(false);
        };
        let admission = self.admit_oauth_refresh(McpOAuthCredentialKey::new(
            current_tenant,
            &initial.oauth.provider_id,
        ));
        drop(initial);
        let Some(admitted) = self
            .capture_oauth_refresh_predecessor(name, current_tenant)
            .await?
        else {
            return Ok(false);
        };
        if McpOAuthCredentialKey::new(current_tenant, &admitted.oauth.provider_id) != admission.key
        {
            return Err("MCP OAuth configuration changed during refresh admission".into());
        }
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
                    &admission,
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
        admission: &McpOAuthRefreshAdmission,
        mut binding: Option<&mut McpToolDispatchBinding>,
    ) -> Result<bool, String> {
        let key = McpOAuthCredentialKey::new(current_tenant, &admitted.oauth.provider_id);
        let coordinator = &admission.coordinator;
        // Serialize before loading the rotating token, but leave revocation and
        // unrelated credentials free to proceed while the endpoint is awaited.
        let _exchange = coordinator.exchange.lock().await;
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
        let credential = self.load_oauth_refresh_credential(current_tenant, &oauth.provider_id);
        let Some(credential) = credential else {
            return Ok(false);
        };

        let connection_id = self.connection_id_for_tenant(name, current_tenant);
        let transition = coordinator
            .transitions
            .lock()
            .expect("OAuth receipts poisoned")
            .get(&connection_id)
            .cloned();
        if let Some(transition) = transition {
            if transition.matches_successor(&predecessor, &credential)? {
                if let Some(binding) = binding.as_deref_mut() {
                    if binding.server_policy == transition.predecessor_policy
                        && binding.connection_generation == transition.predecessor_generation
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
                } else if admitted.server_policy == transition.predecessor_policy
                    && admitted.connection_generation == transition.predecessor_generation
                    && oauth_config_digest(&admitted.oauth)? == transition.oauth_digest
                {
                    return Ok(true);
                }
            }
        }
        if let Some(binding) = binding.as_deref() {
            binding.revalidate()?;
        }

        let credential_digest = oauth_credential_digest(&credential)?;
        let pending = self
            .connections
            .read()
            .await
            .get(&connection_id)
            .and_then(|connection| connection.oauth_publication_pending.clone());
        if pending
            .as_ref()
            .map(|pending| pending.matches(&predecessor, &credential_digest))
            .transpose()?
            .unwrap_or(false)
        {
            self.commit_oauth_refresh(
                name,
                current_tenant,
                McpOAuthRefreshCapture {
                    selected: predecessor,
                    credential_digest,
                    participants: HashMap::new(),
                    save_credential: false,
                },
                credential,
                binding,
                coordinator,
            )
            .await?;
            return Ok(true);
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

        let participants = self.capture_oauth_refresh_participants(&key).await?;

        let refreshed =
            refresh_mcp_oauth_credential(oauth, &credential, &endpoint_authorization).await?;
        self.commit_oauth_refresh(
            name,
            current_tenant,
            McpOAuthRefreshCapture {
                selected: predecessor,
                credential_digest,
                participants,
                save_credential: true,
            },
            refreshed,
            binding,
            coordinator,
        )
        .await?;
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
}
