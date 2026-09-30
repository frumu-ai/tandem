// A bearer write can fail after the shared rotating credential is saved.
// Keep only fingerprints on the existing connection so a later request can
// repair that write without consuming another refresh token.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpOAuthPendingPublication {
    server_policy: Value,
    connection_generation: String,
    oauth_digest: String,
    credential_digest: String,
}

impl McpOAuthPendingPublication {
    fn matches(
        &self,
        current: &McpOAuthRefreshPredecessor,
        credential_digest: &str,
    ) -> Result<bool, String> {
        Ok(self.server_policy == current.server_policy
            && current.connection_generation.as_ref() == Some(&self.connection_generation)
            && self.oauth_digest == oauth_config_digest(&current.oauth)?
            && self.credential_digest == credential_digest)
    }
}

struct McpOAuthRefreshParticipant {
    name: String,
    tenant: TenantContext,
    server_policy: Value,
    generation: Option<String>,
    oauth_digest: String,
}

impl McpOAuthRefreshParticipant {
    fn matches(
        &self,
        server: &McpServer,
        connection: Option<&McpConnection>,
    ) -> Result<bool, String> {
        let oauth = if self.tenant.is_local_implicit() {
            server.oauth.as_ref()
        } else {
            connection.and_then(|row| row.oauth.as_ref())
        };
        Ok(server.enabled
            && connection.is_none_or(|row| row.enabled && row.tenant_context == self.tenant)
            && server_dispatch_policy(server) == self.server_policy
            && connection.map(|row| &row.connection_generation) == self.generation.as_ref()
            && oauth.map(oauth_config_digest).transpose()?.as_ref() == Some(&self.oauth_digest))
    }
}

struct McpOAuthRefreshCapture {
    selected: McpOAuthRefreshPredecessor,
    credential_digest: String,
    participants: HashMap<String, McpOAuthRefreshParticipant>,
    save_credential: bool,
}

impl McpRegistry {
    fn load_oauth_refresh_credential(
        &self,
        tenant: &TenantContext,
        provider: &str,
    ) -> Option<tandem_core::OAuthProviderCredential> {
        if tenant.is_local_implicit() {
            tandem_core::load_provider_oauth_credential_in_dir(&self.oauth_security_dir, provider)
                .or_else(|| tandem_core::load_provider_oauth_credential(provider))
        } else {
            tandem_core::load_provider_oauth_credential_for_tenant_in_dir(
                &self.oauth_security_dir,
                tenant,
                provider,
            )
            .or_else(|| tandem_core::load_provider_oauth_credential_for_tenant(tenant, provider))
        }
    }

    async fn capture_oauth_refresh_participants(
        &self,
        key: &McpOAuthCredentialKey,
    ) -> Result<HashMap<String, McpOAuthRefreshParticipant>, String> {
        let servers = self.servers.read().await;
        let connections = self.connections.read().await;
        let mut participants = HashMap::new();
        for (id, connection) in connections.iter() {
            let Some(server) = servers.get(&connection.server_id) else {
                continue;
            };
            if !server.enabled || !connection.enabled {
                continue;
            }
            let oauth = if connection.tenant_context.is_local_implicit() {
                server.oauth.as_ref()
            } else {
                connection.oauth.as_ref()
            };
            let Some(oauth) = oauth else { continue };
            if McpOAuthCredentialKey::new(&connection.tenant_context, &oauth.provider_id) != *key {
                continue;
            }
            participants.insert(
                id.clone(),
                McpOAuthRefreshParticipant {
                    name: connection.server_id.clone(),
                    tenant: connection.tenant_context.clone(),
                    server_policy: server_dispatch_policy(server),
                    generation: Some(connection.connection_generation.clone()),
                    oauth_digest: oauth_config_digest(oauth)?,
                },
            );
        }
        Ok(participants)
    }

    async fn commit_oauth_refresh(
        &self,
        name: &str,
        tenant: &TenantContext,
        mut capture: McpOAuthRefreshCapture,
        refreshed: tandem_core::OAuthProviderCredential,
        binding: Option<&mut McpToolDispatchBinding>,
        coordinator: &McpOAuthRefreshState,
    ) -> Result<(), String> {
        let digest = oauth_credential_digest(&refreshed)?;
        let token = refreshed.access_token.trim().to_string();
        if token.is_empty() {
            return Err("oauth access token cannot be empty".into());
        }
        let selected_id = self.connection_id_for_tenant(name, tenant);
        capture.participants.insert(
            selected_id.clone(),
            McpOAuthRefreshParticipant {
                name: name.to_string(),
                tenant: tenant.clone(),
                server_policy: capture.selected.server_policy.clone(),
                generation: capture.selected.connection_generation.clone(),
                oauth_digest: oauth_config_digest(&capture.selected.oauth)?,
            },
        );
        let _credential_guard = self.credential_mutation_lock.lock().await;
        let mut servers = self.servers.write().await;
        let mut connections = self.connections.write().await;
        let selected = servers
            .get(name)
            .ok_or("MCP connector removed during OAuth refresh")?;
        if !capture.participants[&selected_id].matches(selected, connections.get(&selected_id))? {
            return Err("MCP authority changed during OAuth refresh".into());
        }
        if let Some(binding) = binding.as_ref() {
            binding.validate_snapshot(selected, connections.get(&selected_id))?;
        }
        // A sibling can delete or replace this same stored credential while the
        // endpoint is awaited. Do not resurrect it from the network response.
        let current = self
            .load_oauth_refresh_credential(tenant, &capture.selected.oauth.provider_id)
            .ok_or("MCP OAuth credential was removed during refresh")?;
        if oauth_credential_digest(&current)? != capture.credential_digest {
            return Err("MCP OAuth credential was replaced during refresh".into());
        }
        let mut targets = Vec::new();
        for (id, participant) in capture.participants {
            if let Some(server) = servers.get(&participant.name) {
                if participant.matches(server, connections.get(&id))? {
                    targets.push((id, participant));
                }
            }
        }
        let saved = if !capture.save_credential {
            Ok(())
        } else if tenant.is_local_implicit() {
            tandem_core::set_provider_oauth_credential_in_dir(
                &self.oauth_security_dir,
                &capture.selected.oauth.provider_id,
                refreshed,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
        } else {
            tandem_core::set_provider_oauth_credential_for_tenant_in_dir(
                &self.oauth_security_dir,
                tenant,
                &capture.selected.oauth.provider_id,
                refreshed,
            )
            .map(|_| ())
            .map_err(|error| error.to_string())
        };
        // The credential backend can save its value but fail updating its index.
        // If that happened, record repair intent but preserve the caller error.
        if saved.is_err()
            && self
                .load_oauth_refresh_credential(tenant, &capture.selected.oauth.provider_id)
                .as_ref()
                .map(oauth_credential_digest)
                .transpose()?
                .as_deref()
                != Some(digest.as_str())
        {
            return saved;
        }
        let mut published = Vec::new();
        let credential_save_failed = saved.is_err();
        let mut selected_error = saved.err();
        for (id, participant) in &targets {
            let server = servers
                .get_mut(&participant.name)
                .expect("captured server under write lock");
            let connection = connections.entry(id.clone()).or_insert_with(|| {
                McpConnection::tenant_connection_from_server(
                    &participant.name,
                    server,
                    participant.tenant.clone(),
                    McpPrincipalRef::from_tenant_context(&participant.tenant),
                    now_ms(),
                )
            });
            connection.oauth_publication_pending = Some(McpOAuthPendingPublication {
                server_policy: server_dispatch_policy(server),
                connection_generation: connection.connection_generation.clone(),
                oauth_digest: participant.oauth_digest.clone(),
                credential_digest: digest.clone(),
            });
            if credential_save_failed {
                continue;
            }
            // Never grant a receipt for a failed bearer write. Other valid
            // recipients may still finish; this connection retains repair intent.
            match publish_oauth_bearer(
                server,
                connection,
                &participant.name,
                &participant.tenant,
                &token,
            ) {
                Ok(()) => {
                    published.push(id.clone());
                }
                Err(error) => {
                    if id == &selected_id {
                        selected_error = Some(error);
                    }
                }
            }
        }
        let mut selected_transition = None;
        {
            let mut transitions = coordinator
                .transitions
                .lock()
                .expect("OAuth receipts poisoned");
            for (id, participant) in targets {
                let server = &servers[&participant.name];
                let connection = connections
                    .get_mut(&id)
                    .expect("captured connection under write lock");
                // Local compatibility publication can also change the shared server
                // header policy. Use the final policy for every resulting receipt.
                if let Some(pending) = connection.oauth_publication_pending.as_mut() {
                    pending.server_policy = server_dispatch_policy(server);
                }
                if !published.contains(&id) {
                    continue;
                }
                let transition = McpOAuthRefreshTransition {
                    predecessor_policy: participant.server_policy,
                    predecessor_generation: participant.generation,
                    oauth_digest: participant.oauth_digest,
                    server_policy: server_dispatch_policy(server),
                    connection_generation: Some(connection.connection_generation.clone()),
                    credential_digest: digest.clone(),
                };
                if id == selected_id {
                    selected_transition = Some(transition.clone());
                }
                transitions.insert(id, transition);
            }
        }
        if selected_error.is_none() {
            if let (Some(binding), Some(transition)) = (binding, selected_transition) {
                binding.server_policy = transition.server_policy;
                binding.connection_generation = transition.connection_generation;
            }
        }
        drop(connections);
        drop(servers);
        self.persist_state().await;
        match selected_error {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }
}

fn publish_oauth_bearer(
    server: &mut McpServer,
    connection: &mut McpConnection,
    name: &str,
    tenant: &TenantContext,
    token: &str,
) -> Result<(), String> {
    let header_name = "Authorization".to_string();
    let secret_id = mcp_header_secret_id_for_tenant(name, &header_name, tenant);
    let secret_ref = McpSecretRef::Store {
        secret_id: secret_id.clone(),
        tenant_context: tenant.clone(),
    };
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
    connection.updated_at_ms = now_ms();
    connection.oauth_publication_pending = None;
    Ok(())
}
