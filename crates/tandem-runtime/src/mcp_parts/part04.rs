#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerDefinition {
    pub server_id: String,
    pub name: String,
    pub transport: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub auth_kind: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub purpose: String,
    #[serde(default)]
    pub grounding_required: bool,
}

impl McpServerDefinition {
    pub fn from_server(server_id: &str, server: &McpServer) -> Self {
        Self {
            server_id: server_id.trim().to_string(),
            name: server.name.clone(),
            transport: server.transport.clone(),
            auth_kind: server.auth_kind.clone(),
            enabled: server.enabled,
            allowed_tools: server.allowed_tools.clone(),
            purpose: server.purpose.clone(),
            grounding_required: server.grounding_required,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum McpPrincipalRef {
    HumanActor { actor_id: String },
    ServicePrincipal { principal_id: String },
    AutomationPrincipal { automation_id: String },
    SharedConnection { grant_id: String },
    LocalImplicit,
}

impl McpPrincipalRef {
    pub fn from_tenant_context(tenant_context: &TenantContext) -> Self {
        if let Some(actor_id) = tenant_context.actor_id.as_ref() {
            return Self::HumanActor {
                actor_id: actor_id.clone(),
            };
        }
        if tenant_context.is_local_implicit() {
            return Self::LocalImplicit;
        }
        Self::ServicePrincipal {
            principal_id: tenant_scoped_principal_id(tenant_context),
        }
    }

    fn stable_key(&self) -> String {
        match self {
            Self::HumanActor { actor_id } => format!("human:{actor_id}"),
            Self::ServicePrincipal { principal_id } => format!("service:{principal_id}"),
            Self::AutomationPrincipal { automation_id } => format!("automation:{automation_id}"),
            Self::SharedConnection { grant_id } => format!("shared:{grant_id}"),
            Self::LocalImplicit => "local:implicit".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum McpConnectionClass {
    UserOwned,
    ServiceAccount,
    SharedReadOnly,
    SharedReadWrite,
    AdminManaged,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpCredentialRef {
    pub provider: String,
    pub secret_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpUpstreamAccount {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub account_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_tenant_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpConnection {
    pub connection_id: String,
    pub server_id: String,
    pub tenant_context: TenantContext,
    pub owner: McpPrincipalRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<McpCredentialRef>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub secret_headers: HashMap<String, McpSecretRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpOAuthConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_account: Option<McpUpstreamAccount>,
    pub connection_class: McpConnectionClass,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl McpConnection {
    pub fn identity_key(&self) -> String {
        mcp_connection_identity_key(&self.server_id, &self.tenant_context, &self.owner)
    }

    fn local_compatibility_from_server(server_id: &str, server: &McpServer, now_ms: u64) -> Self {
        let tenant_context = local_tenant_context();
        let owner = McpPrincipalRef::LocalImplicit;
        let credential_ref = compatibility_credential_ref(server_id, server);
        Self {
            connection_id: mcp_connection_id(server_id, &tenant_context, &owner),
            server_id: server_id.trim().to_string(),
            tenant_context,
            owner,
            credential_ref,
            secret_headers: server.secret_headers.clone(),
            oauth: server.oauth.clone(),
            upstream_account: None,
            connection_class: McpConnectionClass::UserOwned,
            enabled: server.enabled,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }
}

impl McpRegistry {
    async fn connection_for_tenant(
        &self,
        server_id: &str,
        current_tenant: &TenantContext,
    ) -> Option<McpConnection> {
        let owner = McpPrincipalRef::from_tenant_context(current_tenant);
        let connection_id = mcp_connection_id(server_id, current_tenant, &owner);
        self.connections.read().await.get(&connection_id).cloned()
    }

    async fn upsert_compatibility_connection_for_server(
        &self,
        server_id: &str,
        current_tenant: &TenantContext,
    ) {
        let Some(server) = self.servers.read().await.get(server_id).cloned() else {
            return;
        };
        let owner = McpPrincipalRef::from_tenant_context(current_tenant);
        let connection_id = mcp_connection_id(server_id, current_tenant, &owner);
        let now = now_ms();
        let credential_ref = compatibility_credential_ref(server_id, &server);
        let mut connections = self.connections.write().await;
        if let Some(existing) = connections.get_mut(&connection_id) {
            existing.enabled = server.enabled;
            if current_tenant.is_local_implicit() {
                existing.credential_ref = credential_ref;
                existing.secret_headers = server.secret_headers.clone();
                existing.oauth = server.oauth.clone();
            } else if existing.credential_ref.is_none() {
                existing.credential_ref = credential_ref;
            }
            existing.updated_at_ms = now;
            return;
        }
        connections.insert(
            connection_id.clone(),
            McpConnection {
                connection_id,
                server_id: server_id.trim().to_string(),
                tenant_context: current_tenant.clone(),
                owner,
                credential_ref,
                secret_headers: server.secret_headers.clone(),
                oauth: server.oauth.clone(),
                upstream_account: None,
                connection_class: McpConnectionClass::UserOwned,
                enabled: server.enabled,
                created_at_ms: now,
                updated_at_ms: now,
            },
        );
    }

    async fn remove_connections_for_server(&self, server_id: &str) {
        self.connections
            .write()
            .await
            .retain(|_, connection| connection.server_id != server_id);
    }

    async fn update_connection_enabled_for_server(&self, server_id: &str, enabled: bool) {
        let now = now_ms();
        for connection in self
            .connections
            .write()
            .await
            .values_mut()
            .filter(|connection| connection.server_id == server_id)
        {
            connection.enabled = enabled;
            connection.updated_at_ms = now;
        }
    }

    async fn upsert_connection_secret_header_for_tenant(
        &self,
        server_id: &str,
        current_tenant: &TenantContext,
        header_name: &str,
        secret_ref: McpSecretRef,
    ) {
        let Some(server) = self.servers.read().await.get(server_id).cloned() else {
            return;
        };
        let owner = McpPrincipalRef::from_tenant_context(current_tenant);
        let connection_id = mcp_connection_id(server_id, current_tenant, &owner);
        let now = now_ms();
        let header_name = header_name.to_string();
        let header_credential_ref = McpCredentialRef {
            provider: "mcp_header".to_string(),
            secret_id: format!(
                "{}::{}::{}",
                server_id.trim(),
                header_name.to_ascii_lowercase(),
                secret_ref_stable_id(&secret_ref)
            ),
            credential_version: None,
            expires_at_ms: None,
        };
        let mut connections = self.connections.write().await;
        if let Some(existing) = connections.get_mut(&connection_id) {
            existing.enabled = server.enabled;
            existing
                .secret_headers
                .insert(header_name, secret_ref.clone());
            if existing.credential_ref.is_none() {
                existing.credential_ref = Some(header_credential_ref);
            }
            existing.updated_at_ms = now;
            return;
        }
        let mut secret_headers = HashMap::new();
        secret_headers.insert(header_name, secret_ref);
        connections.insert(
            connection_id.clone(),
            McpConnection {
                connection_id,
                server_id: server_id.trim().to_string(),
                tenant_context: current_tenant.clone(),
                owner,
                credential_ref: Some(header_credential_ref),
                secret_headers,
                oauth: None,
                upstream_account: None,
                connection_class: McpConnectionClass::UserOwned,
                enabled: server.enabled,
                created_at_ms: now,
                updated_at_ms: now,
            },
        );
    }

    async fn upsert_connection_oauth_for_tenant(
        &self,
        server_id: &str,
        current_tenant: &TenantContext,
        oauth: McpOAuthConfig,
    ) {
        let Some(server) = self.servers.read().await.get(server_id).cloned() else {
            return;
        };
        let owner = McpPrincipalRef::from_tenant_context(current_tenant);
        let connection_id = mcp_connection_id(server_id, current_tenant, &owner);
        let now = now_ms();
        let credential_ref = McpCredentialRef {
            provider: "mcp_oauth".to_string(),
            secret_id: oauth.provider_id.clone(),
            credential_version: None,
            expires_at_ms: None,
        };
        let mut connections = self.connections.write().await;
        if let Some(existing) = connections.get_mut(&connection_id) {
            existing.enabled = server.enabled;
            existing.credential_ref = Some(credential_ref);
            existing.oauth = Some(oauth);
            existing.updated_at_ms = now;
            return;
        }
        connections.insert(
            connection_id.clone(),
            McpConnection {
                connection_id,
                server_id: server_id.trim().to_string(),
                tenant_context: current_tenant.clone(),
                owner,
                credential_ref: Some(credential_ref),
                secret_headers: HashMap::new(),
                oauth: Some(oauth),
                upstream_account: None,
                connection_class: McpConnectionClass::UserOwned,
                enabled: server.enabled,
                created_at_ms: now,
                updated_at_ms: now,
            },
        );
    }

    async fn oauth_config_for_tenant(
        &self,
        server_id: &str,
        server: &McpServer,
        current_tenant: &TenantContext,
    ) -> Option<McpOAuthConfig> {
        if current_tenant.is_local_implicit() {
            return server.oauth.clone();
        }
        self.connection_for_tenant(server_id, current_tenant)
            .await
            .and_then(|connection| connection.oauth)
    }

    async fn effective_headers_for_current_tenant(
        &self,
        server_id: &str,
        server: &McpServer,
        current_tenant: &TenantContext,
    ) -> HashMap<String, String> {
        if current_tenant.is_local_implicit() {
            return effective_headers(server);
        }
        let mut headers = combine_headers(
            &server.headers,
            &resolve_secret_header_values(&server.secret_headers, current_tenant),
        );
        if let Some(connection) = self.connection_for_tenant(server_id, current_tenant).await {
            for (header_name, value) in
                resolve_secret_header_values(&connection.secret_headers, current_tenant)
            {
                if !value.trim().is_empty() {
                    headers.insert(header_name, value);
                }
            }
        }
        headers
    }
}
