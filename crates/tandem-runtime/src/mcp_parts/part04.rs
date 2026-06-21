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
            upstream_account: None,
            connection_class: McpConnectionClass::UserOwned,
            enabled: server.enabled,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }
}
