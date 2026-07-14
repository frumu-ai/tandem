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
    #[serde(default = "new_mcp_connection_generation")]
    pub connection_generation: String,
    pub server_id: String,
    pub tenant_context: TenantContext,
    pub owner: McpPrincipalRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref: Option<McpCredentialRef>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub secret_headers: HashMap<String, McpSecretRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oauth: Option<McpOAuthConfig>,
    #[serde(default)]
    pub connected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_auth_challenge: Option<McpAuthChallenge>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_session_id: Option<String>,
    #[serde(default)]
    pub tool_cache: Vec<McpToolCacheEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tools_fetched_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub pending_auth_by_tool: HashMap<String, PendingMcpAuth>,
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

    pub(crate) fn reset_transient_runtime_state(&mut self) {
        self.connected = false;
        self.last_error = None;
        self.last_auth_challenge = None;
        self.mcp_session_id = None;
        self.tool_cache.clear();
        self.tools_fetched_at_ms = None;
        self.pending_auth_by_tool.clear();
    }

    fn local_compatibility_from_server(server_id: &str, server: &McpServer, now_ms: u64) -> Self {
        let tenant_context = local_tenant_context();
        let owner = McpPrincipalRef::LocalImplicit;
        let credential_ref = compatibility_credential_ref(server_id, server);
        Self {
            connection_id: mcp_connection_id(server_id, &tenant_context, &owner),
            connection_generation: new_mcp_connection_generation(),
            server_id: server_id.trim().to_string(),
            tenant_context,
            owner,
            credential_ref,
            secret_headers: server.secret_headers.clone(),
            oauth: server.oauth.clone(),
            connected: server.connected,
            last_error: server.last_error.clone(),
            last_auth_challenge: server.last_auth_challenge.clone(),
            mcp_session_id: server.mcp_session_id.clone(),
            tool_cache: server.tool_cache.clone(),
            tools_fetched_at_ms: server.tools_fetched_at_ms,
            pending_auth_by_tool: server.pending_auth_by_tool.clone(),
            upstream_account: None,
            connection_class: McpConnectionClass::UserOwned,
            enabled: server.enabled,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }

    fn tenant_connection_from_server(
        server_id: &str,
        server: &McpServer,
        tenant_context: TenantContext,
        owner: McpPrincipalRef,
        now_ms: u64,
    ) -> Self {
        let credential_ref = compatibility_credential_ref(server_id, server);
        let is_local = tenant_context.is_local_implicit();
        Self {
            connection_id: mcp_connection_id(server_id, &tenant_context, &owner),
            connection_generation: new_mcp_connection_generation(),
            server_id: server_id.trim().to_string(),
            tenant_context,
            owner,
            credential_ref,
            secret_headers: server.secret_headers.clone(),
            oauth: server.oauth.clone(),
            connected: is_local && server.connected,
            last_error: is_local.then(|| server.last_error.clone()).flatten(),
            last_auth_challenge: is_local
                .then(|| server.last_auth_challenge.clone())
                .flatten(),
            mcp_session_id: is_local.then(|| server.mcp_session_id.clone()).flatten(),
            tool_cache: if is_local {
                server.tool_cache.clone()
            } else {
                Vec::new()
            },
            tools_fetched_at_ms: is_local.then_some(server.tools_fetched_at_ms).flatten(),
            pending_auth_by_tool: if is_local {
                server.pending_auth_by_tool.clone()
            } else {
                HashMap::new()
            },
            upstream_account: None,
            connection_class: McpConnectionClass::UserOwned,
            enabled: server.enabled,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct McpRuntimeState {
    connected: bool,
    last_error: Option<String>,
    last_auth_challenge: Option<McpAuthChallenge>,
    mcp_session_id: Option<String>,
    tool_cache: Vec<McpToolCacheEntry>,
    tools_fetched_at_ms: Option<u64>,
    pending_auth_by_tool: HashMap<String, PendingMcpAuth>,
}

impl McpRuntimeState {
    fn from_server(server: &McpServer) -> Self {
        Self {
            connected: server.connected,
            last_error: server.last_error.clone(),
            last_auth_challenge: server.last_auth_challenge.clone(),
            mcp_session_id: server.mcp_session_id.clone(),
            tool_cache: server.tool_cache.clone(),
            tools_fetched_at_ms: server.tools_fetched_at_ms,
            pending_auth_by_tool: server.pending_auth_by_tool.clone(),
        }
    }

    fn from_connection(connection: &McpConnection) -> Self {
        Self {
            connected: connection.connected,
            last_error: connection.last_error.clone(),
            last_auth_challenge: connection.last_auth_challenge.clone(),
            mcp_session_id: connection.mcp_session_id.clone(),
            tool_cache: connection.tool_cache.clone(),
            tools_fetched_at_ms: connection.tools_fetched_at_ms,
            pending_auth_by_tool: connection.pending_auth_by_tool.clone(),
        }
    }

    fn connected(
        session_id: Option<String>,
        tool_cache: Vec<McpToolCacheEntry>,
        now_ms: u64,
    ) -> Self {
        Self {
            connected: true,
            last_error: None,
            last_auth_challenge: None,
            mcp_session_id: session_id,
            tool_cache,
            tools_fetched_at_ms: Some(now_ms),
            pending_auth_by_tool: HashMap::new(),
        }
    }

    fn disconnected(last_error: Option<String>, auth_challenge: Option<McpAuthChallenge>) -> Self {
        let mut pending_auth_by_tool = HashMap::new();
        if let Some(challenge) = auth_challenge.as_ref() {
            pending_auth_by_tool.insert(
                canonical_tool_key(&challenge.tool_name),
                pending_auth_from_challenge(challenge),
            );
        }
        Self {
            connected: false,
            last_error,
            last_auth_challenge: auth_challenge,
            mcp_session_id: None,
            tool_cache: Vec::new(),
            tools_fetched_at_ms: None,
            pending_auth_by_tool,
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
                existing.connected = server.connected;
                existing.last_error = server.last_error.clone();
                existing.last_auth_challenge = server.last_auth_challenge.clone();
                existing.mcp_session_id = server.mcp_session_id.clone();
                existing.tool_cache = server.tool_cache.clone();
                existing.tools_fetched_at_ms = server.tools_fetched_at_ms;
                existing.pending_auth_by_tool = server.pending_auth_by_tool.clone();
            } else if existing.credential_ref.is_none() {
                existing.credential_ref = credential_ref;
            }
            existing.updated_at_ms = now;
            return;
        }
        connections.insert(
            connection_id,
            McpConnection::tenant_connection_from_server(
                server_id,
                &server,
                current_tenant.clone(),
                owner,
                now,
            ),
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
            existing.connection_generation = new_mcp_connection_generation();
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
                connection_generation: new_mcp_connection_generation(),
                server_id: server_id.trim().to_string(),
                tenant_context: current_tenant.clone(),
                owner,
                credential_ref: Some(header_credential_ref),
                secret_headers,
                oauth: None,
                connected: false,
                last_error: None,
                last_auth_challenge: None,
                mcp_session_id: None,
                tool_cache: Vec::new(),
                tools_fetched_at_ms: None,
                pending_auth_by_tool: HashMap::new(),
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
            existing.connection_generation = new_mcp_connection_generation();
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
                connection_generation: new_mcp_connection_generation(),
                server_id: server_id.trim().to_string(),
                tenant_context: current_tenant.clone(),
                owner,
                credential_ref: Some(credential_ref),
                secret_headers: HashMap::new(),
                oauth: Some(oauth),
                connected: false,
                last_error: None,
                last_auth_challenge: None,
                mcp_session_id: None,
                tool_cache: Vec::new(),
                tools_fetched_at_ms: None,
                pending_auth_by_tool: HashMap::new(),
                upstream_account: None,
                connection_class: McpConnectionClass::UserOwned,
                enabled: server.enabled,
                created_at_ms: now,
                updated_at_ms: now,
            },
        );
    }

    async fn rotate_connection_generation_for_tenant(
        &self,
        server_id: &str,
        current_tenant: &TenantContext,
    ) {
        let owner = McpPrincipalRef::from_tenant_context(current_tenant);
        let connection_id = mcp_connection_id(server_id, current_tenant, &owner);
        if let Some(connection) = self.connections.write().await.get_mut(&connection_id) {
            connection.connection_generation = new_mcp_connection_generation();
            connection.updated_at_ms = now_ms();
        }
    }

    async fn rotate_connection_generations_for_oauth_provider(
        &self,
        provider_id: &str,
        current_tenant: &TenantContext,
    ) {
        let now = now_ms();
        for connection in self
            .connections
            .write()
            .await
            .values_mut()
            .filter(|connection| {
                connection.tenant_context == *current_tenant
                    && connection
                        .oauth
                        .as_ref()
                        .is_some_and(|oauth| oauth.provider_id == provider_id)
            })
        {
            connection.connection_generation = new_mcp_connection_generation();
            connection.updated_at_ms = now;
        }
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

    async fn runtime_state_for_current_tenant(
        &self,
        server_id: &str,
        server: &McpServer,
        current_tenant: &TenantContext,
    ) -> McpRuntimeState {
        if current_tenant.is_local_implicit() {
            return McpRuntimeState::from_server(server);
        }
        self.connection_for_tenant(server_id, current_tenant)
            .await
            .as_ref()
            .map(McpRuntimeState::from_connection)
            .unwrap_or_default()
    }

    async fn set_runtime_state_for_current_tenant(
        &self,
        server_id: &str,
        current_tenant: &TenantContext,
        runtime: McpRuntimeState,
    ) {
        let Some(server) = self.servers.read().await.get(server_id).cloned() else {
            return;
        };
        let now = now_ms();
        if current_tenant.is_local_implicit() {
            {
                let mut servers = self.servers.write().await;
                if let Some(entry) = servers.get_mut(server_id) {
                    entry.connected = runtime.connected;
                    entry.pid = None;
                    entry.last_error = runtime.last_error.clone();
                    entry.last_auth_challenge = runtime.last_auth_challenge.clone();
                    entry.mcp_session_id = runtime.mcp_session_id.clone();
                    entry.tool_cache = runtime.tool_cache.clone();
                    entry.tools_fetched_at_ms = runtime.tools_fetched_at_ms;
                    entry.pending_auth_by_tool = runtime.pending_auth_by_tool.clone();
                }
            }
            self.upsert_compatibility_connection_for_server(server_id, current_tenant)
                .await;
            return;
        }

        let owner = McpPrincipalRef::from_tenant_context(current_tenant);
        let connection_id = mcp_connection_id(server_id, current_tenant, &owner);
        let mut connections = self.connections.write().await;
        let connection = connections.entry(connection_id).or_insert_with(|| {
            McpConnection::tenant_connection_from_server(
                server_id,
                &server,
                current_tenant.clone(),
                owner,
                now,
            )
        });
        connection.enabled = server.enabled;
        connection.connected = runtime.connected;
        connection.last_error = runtime.last_error;
        connection.last_auth_challenge = runtime.last_auth_challenge;
        connection.mcp_session_id = runtime.mcp_session_id;
        connection.tool_cache = runtime.tool_cache;
        connection.tools_fetched_at_ms = runtime.tools_fetched_at_ms;
        connection.pending_auth_by_tool = runtime.pending_auth_by_tool;
        connection.updated_at_ms = now;
    }

    pub async fn auth_challenge_for_tenant(
        &self,
        server_id: &str,
        current_tenant: &TenantContext,
    ) -> Option<McpAuthChallenge> {
        if current_tenant.is_local_implicit() {
            return self
                .servers
                .read()
                .await
                .get(server_id)
                .and_then(|server| server.last_auth_challenge.clone());
        }
        self.connection_for_tenant(server_id, current_tenant)
            .await
            .and_then(|connection| connection.last_auth_challenge)
    }

    pub async fn clear_auth_challenge_for_tenant(
        &self,
        server_id: &str,
        current_tenant: &TenantContext,
    ) -> bool {
        if current_tenant.is_local_implicit() {
            return self.clear_server_auth_challenge(server_id).await;
        }
        let Some(server) = self.servers.read().await.get(server_id).cloned() else {
            return false;
        };
        let mut runtime = self
            .runtime_state_for_current_tenant(server_id, &server, current_tenant)
            .await;
        runtime.last_auth_challenge = None;
        runtime.pending_auth_by_tool.clear();
        self.set_runtime_state_for_current_tenant(server_id, current_tenant, runtime)
            .await;
        self.persist_state().await;
        true
    }

    pub async fn record_auth_challenge_for_tenant(
        &self,
        server_id: &str,
        current_tenant: &TenantContext,
        challenge: McpAuthChallenge,
        last_error: Option<String>,
    ) -> bool {
        if current_tenant.is_local_implicit() {
            return self
                .record_server_auth_challenge(server_id, challenge, last_error)
                .await;
        }
        let Some(server) = self.servers.read().await.get(server_id).cloned() else {
            return false;
        };
        let tool_key = canonical_tool_key(&challenge.tool_name);
        let mut runtime = self
            .runtime_state_for_current_tenant(server_id, &server, current_tenant)
            .await;
        runtime.connected = false;
        runtime.last_error = last_error.or_else(|| Some(challenge.message.clone()));
        runtime.last_auth_challenge = Some(challenge.clone());
        runtime.mcp_session_id = None;
        runtime.pending_auth_by_tool.clear();
        runtime
            .pending_auth_by_tool
            .insert(tool_key, pending_auth_from_challenge(&challenge));
        self.set_runtime_state_for_current_tenant(server_id, current_tenant, runtime)
            .await;
        self.persist_state().await;
        true
    }

    pub async fn runtime_connected_for_tenant(
        &self,
        server_id: &str,
        server: &McpServer,
        current_tenant: &TenantContext,
    ) -> bool {
        self.runtime_state_for_current_tenant(server_id, server, current_tenant)
            .await
            .connected
    }

    pub(crate) async fn runtime_last_error_for_tenant(
        &self,
        server_id: &str,
        server: &McpServer,
        current_tenant: &TenantContext,
    ) -> Option<String> {
        self.runtime_state_for_current_tenant(server_id, server, current_tenant)
            .await
            .last_error
            .filter(|error| !error.trim().is_empty())
    }

    pub async fn server_tools_for_tenant(
        &self,
        server_id: &str,
        current_tenant: &TenantContext,
    ) -> Vec<McpRemoteTool> {
        if current_tenant.is_local_implicit() {
            return self.server_tools(server_id).await;
        }
        let Some(server) = self.servers.read().await.get(server_id).cloned() else {
            return Vec::new();
        };
        let Some(connection) = self.connection_for_tenant(server_id, current_tenant).await else {
            return Vec::new();
        };
        if !server.enabled || !connection.enabled || !connection.connected {
            return Vec::new();
        }
        let mut rows = tool_cache_rows(&server, &connection.tool_cache);
        rows.sort_by(|a, b| a.namespaced_name.cmp(&b.namespaced_name));
        rows
    }

    pub async fn bridge_tools_for_server(&self, server_id: &str) -> Vec<McpRemoteTool> {
        let Some(server) = self.servers.read().await.get(server_id).cloned() else {
            return Vec::new();
        };
        if !server.enabled {
            return Vec::new();
        }
        let mut by_name = HashMap::new();
        if server.connected {
            for row in server_tool_rows(&server) {
                by_name.entry(row.namespaced_name.clone()).or_insert(row);
            }
        }
        for connection in self.connections.read().await.values().filter(|connection| {
            connection.server_id == server_id && connection.enabled && connection.connected
        }) {
            for row in tool_cache_rows(&server, &connection.tool_cache) {
                by_name.entry(row.namespaced_name.clone()).or_insert(row);
            }
        }
        let mut rows = by_name.into_values().collect::<Vec<_>>();
        rows.sort_by(|a, b| a.namespaced_name.cmp(&b.namespaced_name));
        rows
    }

    pub async fn list_tools_for_tenant(
        &self,
        current_tenant: &TenantContext,
    ) -> Vec<McpRemoteTool> {
        if current_tenant.is_local_implicit() {
            return self.list_tools().await;
        }
        let servers = self.servers.read().await.clone();
        let mut out = Vec::new();
        for (server_id, server) in servers {
            if !server.enabled {
                continue;
            }
            let Some(connection) = self.connection_for_tenant(&server_id, current_tenant).await
            else {
                continue;
            };
            if !connection.enabled || !connection.connected {
                continue;
            }
            out.extend(tool_cache_rows(&server, &connection.tool_cache));
        }
        out.sort_by(|a, b| a.namespaced_name.cmp(&b.namespaced_name));
        out
    }
}
