#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConnectorInstance {
    pub connector_id: String,
    pub tenant_context: TenantContext,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default)]
    pub state: ConnectorLifecycleState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub credential_refs: Vec<ConnectorCredentialRef>,
    pub created_by: PrincipalRef,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl ConnectorInstance {
    pub fn active(
        connector_id: impl Into<String>,
        tenant_context: TenantContext,
        provider: impl Into<String>,
        created_by: PrincipalRef,
        now_ms: u64,
    ) -> Self {
        Self {
            connector_id: connector_id.into(),
            tenant_context,
            provider: provider.into(),
            display_name: None,
            state: ConnectorLifecycleState::Active,
            credential_refs: Vec::new(),
            created_by,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }

    pub fn with_state(mut self, state: ConnectorLifecycleState, updated_at_ms: u64) -> Self {
        self.state = state;
        self.updated_at_ms = updated_at_ms;
        self
    }

    pub fn with_credential_refs(mut self, credential_refs: Vec<ConnectorCredentialRef>) -> Self {
        self.credential_refs = credential_refs;
        self
    }

    pub fn tenant_matches(&self, tenant: &TenantContext) -> bool {
        self.tenant_context.org_id == tenant.org_id
            && self.tenant_context.workspace_id == tenant.workspace_id
            && self.tenant_context.deployment_id == tenant.deployment_id
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SourceBindingState {
    #[default]
    Enabled,
    Disabled,
    Quarantined,
}

impl SourceBindingState {
    pub fn allows_ingestion(self) -> bool {
        matches!(self, Self::Enabled)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestionPolicy {
    #[serde(default = "default_true")]
    pub allow_indexing: bool,
    #[serde(default = "default_true")]
    pub allow_prompt_context: bool,
    #[serde(default)]
    pub require_review: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_depth: Option<u32>,
}

impl Default for IngestionPolicy {
    fn default() -> Self {
        Self {
            allow_indexing: true,
            allow_prompt_context: true,
            require_review: false,
            max_depth: None,
        }
    }
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceBinding {
    pub binding_id: String,
    pub tenant_context: TenantContext,
    pub connector_id: String,
    pub source_type: String,
    pub native_source_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_root_label: Option<String>,
    pub resource_ref: ResourceRef,
    pub data_class: DataClass,
    #[serde(default)]
    pub state: SourceBindingState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential_ref_id: Option<String>,
    #[serde(default)]
    pub ingestion_policy: IngestionPolicy,
    pub created_by: PrincipalRef,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

impl SourceBinding {
    #[allow(clippy::too_many_arguments)]
    pub fn enabled(
        binding_id: impl Into<String>,
        tenant_context: TenantContext,
        connector_id: impl Into<String>,
        source_type: impl Into<String>,
        native_source_id: impl Into<String>,
        resource_ref: ResourceRef,
        data_class: DataClass,
        created_by: PrincipalRef,
        now_ms: u64,
    ) -> Self {
        Self {
            binding_id: binding_id.into(),
            tenant_context,
            connector_id: connector_id.into(),
            source_type: source_type.into(),
            native_source_id: native_source_id.into(),
            source_root_label: None,
            resource_ref,
            data_class,
            state: SourceBindingState::Enabled,
            credential_ref_id: None,
            ingestion_policy: IngestionPolicy::default(),
            created_by,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
        }
    }

    pub fn with_state(mut self, state: SourceBindingState, updated_at_ms: u64) -> Self {
        self.state = state;
        self.updated_at_ms = updated_at_ms;
        self
    }

    pub fn with_credential_ref_id(mut self, credential_ref_id: impl Into<String>) -> Self {
        self.credential_ref_id = Some(credential_ref_id.into());
        self
    }

    pub fn with_ingestion_policy(mut self, ingestion_policy: IngestionPolicy) -> Self {
        self.ingestion_policy = ingestion_policy;
        self
    }

    pub fn tenant_matches(&self, tenant: &TenantContext) -> bool {
        self.tenant_context.org_id == tenant.org_id
            && self.tenant_context.workspace_id == tenant.workspace_id
            && self.tenant_context.deployment_id == tenant.deployment_id
    }

    pub fn can_ingest_with(&self, connector: &ConnectorInstance) -> bool {
        self.connector_id == connector.connector_id
            && connector.tenant_matches(&self.tenant_context)
            && connector.state.allows_ingestion()
            && self.state.allows_ingestion()
            && self.ingestion_policy.allow_indexing
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceObject {
    pub source_object_id: String,
    pub tenant_context: TenantContext,
    pub binding_id: String,
    pub connector_id: String,
    pub native_object_id: String,
    pub resource_ref: ResourceRef,
    pub data_class: DataClass,
    #[serde(default)]
    pub lifecycle_state: SourceObjectLifecycleState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_object_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_source_object_id: Option<String>,
    #[serde(default)]
    pub created_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_changed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub superseded_by_source_object_id: Option<String>,
}

impl SourceObject {
    pub fn tenant_matches(&self, tenant: &TenantContext) -> bool {
        self.tenant_context.org_id == tenant.org_id
            && self.tenant_context.workspace_id == tenant.workspace_id
            && self.tenant_context.deployment_id == tenant.deployment_id
    }

    pub fn is_active(&self) -> bool {
        self.lifecycle_state == SourceObjectLifecycleState::Active
    }

    pub fn allows_prompt_context(&self) -> bool {
        self.is_active()
    }

    pub fn with_lifecycle_state(
        mut self,
        lifecycle_state: SourceObjectLifecycleState,
        updated_at_ms: u64,
    ) -> Self {
        self.lifecycle_state = lifecycle_state;
        self.updated_at_ms = updated_at_ms;
        self.lifecycle_changed_at_ms = Some(updated_at_ms);
        self
    }

    pub fn dedupe_scope_key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}",
            self.tenant_context.org_id,
            self.tenant_context.workspace_id,
            self.resource_ref.resource_kind as u8,
            self.resource_ref.resource_id,
            self.binding_id,
            self.native_object_id
        )
    }

    pub fn lifecycle_identity_key(&self) -> String {
        format!(
            "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            self.tenant_context.org_id,
            self.tenant_context.workspace_id,
            self.tenant_context.deployment_id.as_deref().unwrap_or(""),
            self.binding_id,
            self.connector_id,
            self.resource_ref.resource_kind as u8,
            self.resource_ref.resource_id,
            self.resource_ref.path_prefix.as_deref().unwrap_or(""),
            self.data_class as u8,
            self.native_object_id,
            self.native_object_path.as_deref().unwrap_or("")
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum SourceObjectLifecycleState {
    #[default]
    Active,
    Quarantined,
    Tombstoned,
    Deleted,
    Rescoped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum IngestionJobState {
    #[default]
    Queued,
    Running,
    Completed,
    Failed,
    Skipped,
    Quarantined,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestionJob {
    pub job_id: String,
    pub tenant_context: TenantContext,
    pub connector_id: String,
    pub binding_id: String,
    #[serde(default)]
    pub state: IngestionJobState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_object_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quarantine_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuarantineDisposition {
    Release,
    Delete,
    Reindex,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IngestionQuarantine {
    pub quarantine_id: String,
    pub tenant_context: TenantContext,
    pub connector_id: String,
    pub binding_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_object_ids: Vec<String>,
    pub reason: String,
    pub created_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_by: Option<PrincipalRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewed_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disposition: Option<QuarantineDisposition>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScopedMemoryChunkRef {
    pub chunk_id: String,
    pub tenant_context: TenantContext,
    pub source_object_id: String,
    pub resource_ref: ResourceRef,
    pub data_class: DataClass,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_hash: Option<String>,
}
