// Memory Context Types
// Type definitions and error types for the memory system

use crate::knowledge_scope::KnowledgeScopePolicy;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tandem_enterprise_contract::{
    AccessDecision, AccessEffect, AccessPermission, DataBoundary, DataClass, ResourceKind,
    ResourceRef, StrictTenantContext,
};
use tandem_orchestrator::{KnowledgeScope, KnowledgeTrustLevel};
use thiserror::Error;

/// Memory tier - determines persistence level
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryTier {
    /// Ephemeral session memory - cleared when session ends
    Session,
    /// Persistent project memory - survives across sessions
    Project,
    /// Cross-project global memory - user preferences and patterns
    Global,
}

impl MemoryTier {
    /// Get the table prefix for this tier
    pub fn table_prefix(&self) -> &'static str {
        match self {
            MemoryTier::Session => "session",
            MemoryTier::Project => "project",
            MemoryTier::Global => "global",
        }
    }
}

impl std::fmt::Display for MemoryTier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryTier::Session => write!(f, "session"),
            MemoryTier::Project => write!(f, "project"),
            MemoryTier::Global => write!(f, "global"),
        }
    }
}

/// Tenant partition for vector-backed memory rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryTenantScope {
    pub org_id: String,
    pub workspace_id: String,
    pub deployment_id: Option<String>,
}

impl MemoryTenantScope {
    pub fn local() -> Self {
        Self {
            org_id: "local".to_string(),
            workspace_id: "local".to_string(),
            deployment_id: None,
        }
    }

    /// True when this scope is the single-user local partition rather than an
    /// explicit hosted/enterprise tenant. Strict-mode stores reject this scope
    /// (see `MemoryDatabase::set_strict_tenant_enforcement`).
    pub fn is_local(&self) -> bool {
        self == &Self::local()
    }
}

impl Default for MemoryTenantScope {
    fn default() -> Self {
        Self::local()
    }
}

/// A memory chunk - unit of storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryChunk {
    pub id: String,
    pub content: String,
    pub tier: MemoryTier,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub source: String, // e.g., "user_message", "assistant_response", "file_content"
    // File-derived fields (only set when source == "file")
    pub source_path: Option<String>,
    pub source_mtime: Option<i64>,
    pub source_size: Option<i64>,
    pub source_hash: Option<String>,
    #[serde(default)]
    pub tenant_scope: MemoryTenantScope,
    /// Principal that owns this chunk, if user-restricted. Governed reads
    /// require the caller's memory subject to match; unset means shared within
    /// the chunk's tenant/tier scope (the pre-subject behavior).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub created_at: DateTime<Utc>,
    pub token_count: i64,
    pub metadata: Option<serde_json::Value>,
}

/// Search result with similarity score
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySearchResult {
    pub chunk: MemoryChunk,
    pub similarity: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GovernedReadMode {
    LocalNoop,
    GovernedStrict,
}

impl GovernedReadMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalNoop => "local_noop",
            Self::GovernedStrict => "governed_strict",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GovernedReadDecision {
    pub allowed: bool,
    pub reason: Option<String>,
}

impl GovernedReadDecision {
    pub fn allow(reason: impl Into<String>) -> Self {
        Self {
            allowed: true,
            reason: Some(reason.into()),
        }
    }

    pub fn deny(reason: impl Into<String>) -> Self {
        Self {
            allowed: false,
            reason: Some(reason.into()),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GovernedReadEvidence {
    SourceBinding,
    TenantLocalMemory,
}

impl GovernedReadEvidence {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SourceBinding => "source_binding",
            Self::TenantLocalMemory => "tenant_local_memory",
        }
    }

    fn requires_grant(self) -> bool {
        matches!(self, Self::SourceBinding)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GovernedReadTarget {
    pub resource_ref: ResourceRef,
    pub data_class: DataClass,
    pub source_binding_id: Option<String>,
    pub source_object_id: Option<String>,
    pub evidence: GovernedReadEvidence,
    /// Org-unit that owns this record, if department-restricted. Tenant-local
    /// reads require the caller to be a member of this unit; unset means
    /// tenant-wide visibility (the pre-org-unit behavior).
    pub owner_org_unit_id: Option<String>,
    /// Principal that owns this record, if user-restricted. Tenant-local reads
    /// require the caller's memory subject to match; unset means shared.
    pub owner_subject: Option<String>,
}

/// Optional enterprise access projection applied before memory ranking.
#[derive(Debug, Clone)]
pub struct MemoryAccessFilter {
    pub strict_context: Option<StrictTenantContext>,
    pub now_ms: u64,
    pub mode: GovernedReadMode,
    pub workflow_phase: Option<String>,
    /// Org-unit memberships of the calling principal (from the verified tenant
    /// context). `None` means no membership information is available: any
    /// org-unit-restricted record is denied, fail closed.
    pub caller_org_units: Option<std::collections::BTreeSet<String>>,
    /// Resolved memory subject of the calling principal. `None` means no
    /// subject information is available: any subject-restricted record is
    /// denied, fail closed.
    pub caller_subject: Option<String>,
}

impl MemoryAccessFilter {
    pub fn strict(strict_context: StrictTenantContext, now_ms: u64) -> Self {
        Self::governed(Some(strict_context), now_ms)
    }

    pub fn strict_with_workflow_phase(
        strict_context: StrictTenantContext,
        now_ms: u64,
        workflow_phase: impl Into<String>,
    ) -> Self {
        Self::strict(strict_context, now_ms).with_workflow_phase(workflow_phase)
    }

    pub fn governed(strict_context: Option<StrictTenantContext>, now_ms: u64) -> Self {
        Self {
            strict_context,
            now_ms,
            mode: GovernedReadMode::GovernedStrict,
            workflow_phase: None,
            caller_org_units: None,
            caller_subject: None,
        }
    }

    pub fn governed_with_workflow_phase(
        strict_context: Option<StrictTenantContext>,
        now_ms: u64,
        workflow_phase: impl Into<String>,
    ) -> Self {
        Self::governed(strict_context, now_ms).with_workflow_phase(workflow_phase)
    }

    pub fn local_noop(now_ms: u64) -> Self {
        Self {
            strict_context: None,
            now_ms,
            mode: GovernedReadMode::LocalNoop,
            workflow_phase: None,
            caller_org_units: None,
            caller_subject: None,
        }
    }

    pub fn with_workflow_phase(mut self, workflow_phase: impl Into<String>) -> Self {
        self.workflow_phase = Some(workflow_phase.into());
        self
    }

    pub fn with_caller_subject(mut self, subject: impl Into<String>) -> Self {
        let subject = subject.into().trim().to_string();
        if !subject.is_empty() {
            self.caller_subject = Some(subject);
        }
        self
    }

    pub fn with_caller_org_units(mut self, org_units: impl IntoIterator<Item = String>) -> Self {
        self.caller_org_units = Some(
            org_units
                .into_iter()
                .map(|unit| unit.trim().to_string())
                .filter(|unit| !unit.is_empty())
                .collect(),
        );
        self
    }

    pub fn allows_chunk(&self, chunk: &MemoryChunk) -> bool {
        self.decision_for_chunk(chunk).allowed
    }

    pub fn allows_source_target(&self, target: &MemorySourceAccessTarget) -> bool {
        self.decision_for_source_target(target).allowed
    }

    pub fn allows_global_record(&self, record: &GlobalMemoryRecord) -> bool {
        self.decision_for_global_record(record).allowed
    }

    pub fn decision_for_chunk(&self, chunk: &MemoryChunk) -> GovernedReadDecision {
        if let Some(decision) = self.decision_for_knowledge_scope_metadata(chunk.metadata.as_ref())
        {
            return decision;
        }
        if let Some(decision) =
            self.decision_for_missing_knowledge_scope_metadata(chunk.metadata.as_ref())
        {
            return decision;
        }
        match governed_read_target_from_chunk(chunk) {
            Ok(target) => self.decision_for_target(&target),
            Err(reason) => self.decision_for_missing_target(reason),
        }
    }

    pub fn decision_for_global_record(&self, record: &GlobalMemoryRecord) -> GovernedReadDecision {
        if let Some(decision) = self.decision_for_knowledge_scope_metadata(record.metadata.as_ref())
        {
            return decision;
        }
        if let Some(decision) =
            self.decision_for_missing_knowledge_scope_metadata(record.metadata.as_ref())
        {
            return decision;
        }
        match governed_read_target_from_global_record(record, self.strict_context.as_ref()) {
            Ok(target) => self.decision_for_target(&target),
            Err(reason) => self.decision_for_missing_target(reason),
        }
    }

    fn decision_for_knowledge_scope_metadata(
        &self,
        metadata: Option<&serde_json::Value>,
    ) -> Option<GovernedReadDecision> {
        let policy = match KnowledgeScopePolicy::from_metadata(metadata) {
            Ok(Some(policy)) => policy,
            Ok(None) => return None,
            Err(reason) => return Some(self.decision_for_missing_target_owned(reason)),
        };

        if self.mode == GovernedReadMode::LocalNoop {
            return Some(GovernedReadDecision::allow("local_noop"));
        }
        if let Some(reason) = policy.read_denial_reason(self.workflow_phase.as_deref(), self.now_ms)
        {
            return Some(GovernedReadDecision::deny(reason));
        }
        Some(self.decision_for_target(&policy.governed_read_target()))
    }

    fn decision_for_missing_knowledge_scope_metadata(
        &self,
        metadata: Option<&serde_json::Value>,
    ) -> Option<GovernedReadDecision> {
        if self.mode == GovernedReadMode::LocalNoop || self.workflow_phase.is_none() {
            return None;
        }
        if crate::knowledge_scope::metadata_has_enterprise_source_binding(metadata) {
            Some(GovernedReadDecision::deny(
                "knowledge_scope_registry_missing",
            ))
        } else {
            None
        }
    }

    pub fn decision_for_source_target(
        &self,
        target: &MemorySourceAccessTarget,
    ) -> GovernedReadDecision {
        self.decision_for_target(&GovernedReadTarget {
            resource_ref: target.resource_ref.clone(),
            data_class: target.data_class,
            source_binding_id: target.source_binding_id.clone(),
            source_object_id: target.source_object_id.clone(),
            evidence: GovernedReadEvidence::SourceBinding,
            owner_org_unit_id: None,
            owner_subject: None,
        })
    }

    fn decision_for_missing_target(&self, reason: &'static str) -> GovernedReadDecision {
        self.decision_for_missing_target_owned(reason.to_string())
    }

    fn decision_for_missing_target_owned(&self, reason: String) -> GovernedReadDecision {
        if self.mode == GovernedReadMode::LocalNoop {
            GovernedReadDecision::allow("local_noop")
        } else {
            GovernedReadDecision::deny(reason)
        }
    }

    fn decision_for_target(&self, target: &GovernedReadTarget) -> GovernedReadDecision {
        if self.mode == GovernedReadMode::LocalNoop {
            return GovernedReadDecision::allow("local_noop");
        }

        let Some(strict_context) = self.strict_context.as_ref() else {
            return GovernedReadDecision::deny("missing_strict_projection");
        };

        let effective_boundary =
            effective_data_boundary_for_governed_read(strict_context, self.now_ms);
        if !effective_boundary.allows(target.data_class) {
            return GovernedReadDecision::deny("data_class_denied_by_boundary");
        }

        if strict_context.is_expired_at(self.now_ms) {
            return GovernedReadDecision::deny("context_expired");
        }

        if !target.evidence.requires_grant() {
            if target.resource_ref.organization_id != strict_context.tenant_context.org_id
                || target.resource_ref.workspace_id != strict_context.tenant_context.workspace_id
            {
                return GovernedReadDecision::deny("tenant_scope_mismatch");
            }
            // Department restriction: a record owned by an org unit is readable
            // only by members of that unit. Membership comes from the verified
            // assertion; absent membership information denies, fail closed.
            // Source-bound records are governed by the grant path below instead,
            // where org-unit access grants already apply.
            if let Some(owner_org_unit_id) = target.owner_org_unit_id.as_deref() {
                let is_member = self
                    .caller_org_units
                    .as_ref()
                    .is_some_and(|units| units.contains(owner_org_unit_id));
                if !is_member {
                    return GovernedReadDecision::deny("org_unit_scope_mismatch");
                }
            }
            // Per-user restriction: a record owned by a subject is readable only
            // by that subject. As with org units, absent caller-subject
            // information denies, fail closed.
            if let Some(owner_subject) = target.owner_subject.as_deref() {
                let is_owner = self
                    .caller_subject
                    .as_deref()
                    .is_some_and(|subject| subject == owner_subject);
                if !is_owner {
                    return GovernedReadDecision::deny("subject_scope_mismatch");
                }
            }
            if strict_context
                .resource_scope
                .explicitly_denies(&target.resource_ref)
            {
                return GovernedReadDecision::deny("resource_explicitly_denied_by_scope");
            }
            if !strict_context.resource_scope.contains(&target.resource_ref) {
                return GovernedReadDecision::deny("resource_outside_projected_scope");
            }
            return GovernedReadDecision::allow("tenant_local_memory_allowed");
        }

        let evaluation = strict_context
            .clone()
            .with_data_boundary(effective_boundary)
            .evaluate_access(
                &target.resource_ref,
                AccessPermission::Read,
                target.data_class,
                self.now_ms,
            );
        if evaluation.decision == AccessDecision::Allow {
            GovernedReadDecision::allow(evaluation.reason)
        } else {
            GovernedReadDecision::deny(evaluation.reason)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemorySourceAccessTarget {
    pub resource_ref: ResourceRef,
    pub data_class: DataClass,
    pub source_binding_id: Option<String>,
    pub source_object_id: Option<String>,
}

impl MemorySourceAccessTarget {
    pub fn from_chunk(chunk: &MemoryChunk) -> Option<Self> {
        Self::from_metadata(chunk.metadata.as_ref())
    }

    pub fn from_metadata(metadata: Option<&serde_json::Value>) -> Option<Self> {
        let binding = metadata?.get("enterprise_source_binding")?;
        let resource_ref = serde_json::from_value(binding.get("resource_ref")?.clone()).ok()?;
        let data_class = serde_json::from_value(binding.get("data_class")?.clone()).ok()?;
        Some(Self {
            resource_ref,
            data_class,
            source_binding_id: binding
                .get("binding_id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
            source_object_id: binding
                .get("source_object_id")
                .and_then(serde_json::Value::as_str)
                .map(ToOwned::to_owned),
        })
    }
}

pub fn effective_data_boundary_for_governed_read(
    strict_context: &StrictTenantContext,
    now_ms: u64,
) -> DataBoundary {
    if strict_context.data_boundary.is_unrestricted() {
        let mut grant_data_classes = Vec::new();
        for grant in strict_context.grants.iter().filter(|grant| {
            grant.effect == AccessEffect::Allow
                && !grant.is_expired_at(now_ms)
                && grant.has_permission(AccessPermission::Read)
        }) {
            for data_class in &grant.data_classes {
                if !grant_data_classes.contains(data_class) {
                    grant_data_classes.push(*data_class);
                }
            }
        }
        if grant_data_classes.is_empty() {
            DataBoundary::governed_default()
        } else {
            DataBoundary::allow(grant_data_classes)
        }
    } else {
        strict_context.data_boundary.clone()
    }
}

fn governed_read_target_from_chunk(
    chunk: &MemoryChunk,
) -> Result<GovernedReadTarget, &'static str> {
    if let Some(target) = source_binding_target_from_metadata(chunk.metadata.as_ref())? {
        return Ok(target);
    }

    let mut resource_ref = ResourceRef::new(
        chunk.tenant_scope.org_id.clone(),
        chunk.tenant_scope.workspace_id.clone(),
        ResourceKind::MemorySpace,
        memory_chunk_resource_id(chunk),
    );
    if let Some(project_id) = chunk.project_id.as_ref() {
        resource_ref = resource_ref.with_project_id(project_id.clone());
    }

    Ok(GovernedReadTarget {
        resource_ref,
        data_class: data_class_from_metadata(chunk.metadata.as_ref())
            .unwrap_or(DataClass::Internal),
        source_binding_id: None,
        source_object_id: None,
        evidence: GovernedReadEvidence::TenantLocalMemory,
        owner_org_unit_id: owner_org_unit_id_from_metadata(chunk.metadata.as_ref()),
        owner_subject: chunk
            .subject
            .as_deref()
            .map(str::trim)
            .filter(|subject| !subject.is_empty())
            .map(ToString::to_string),
    })
}

fn governed_read_target_from_global_record(
    record: &GlobalMemoryRecord,
    strict_context: Option<&StrictTenantContext>,
) -> Result<GovernedReadTarget, &'static str> {
    if let Some(target) = source_binding_target_from_metadata(record.metadata.as_ref())? {
        return Ok(target);
    }

    let Some(strict_context) = strict_context else {
        return Err("missing_strict_projection");
    };

    let mut resource_ref = ResourceRef::new(
        strict_context.tenant_context.org_id.clone(),
        strict_context.tenant_context.workspace_id.clone(),
        ResourceKind::MemorySpace,
        global_memory_record_resource_id(record),
    );
    if let Some(project_id) = record.project_tag.as_ref() {
        resource_ref = resource_ref.with_project_id(project_id.clone());
    }

    Ok(GovernedReadTarget {
        resource_ref,
        data_class: data_class_from_metadata(record.metadata.as_ref())
            .unwrap_or(DataClass::Internal),
        source_binding_id: None,
        source_object_id: None,
        evidence: GovernedReadEvidence::TenantLocalMemory,
        owner_org_unit_id: owner_org_unit_id_from_metadata(record.metadata.as_ref()),
        // GlobalMemoryRecord subject scoping is enforced at the SQL layer
        // (user_id predicates per TAN-601); visibility="shared" records are
        // deliberately cross-subject, so no owner_subject is projected here.
        owner_subject: None,
    })
}

fn source_binding_target_from_metadata(
    metadata: Option<&serde_json::Value>,
) -> Result<Option<GovernedReadTarget>, &'static str> {
    let Some(metadata) = metadata else {
        return Ok(None);
    };
    let Some(binding) = metadata.get("enterprise_source_binding") else {
        return if memory_metadata_is_connector_sourced(Some(metadata)) {
            Err("missing_resource_ref")
        } else {
            Ok(None)
        };
    };

    let resource_value = binding.get("resource_ref").ok_or("missing_resource_ref")?;
    let data_class_value = binding.get("data_class").ok_or("missing_data_class")?;
    let resource_ref =
        serde_json::from_value(resource_value.clone()).map_err(|_| "missing_resource_ref")?;
    let data_class =
        serde_json::from_value(data_class_value.clone()).map_err(|_| "missing_data_class")?;

    Ok(Some(GovernedReadTarget {
        resource_ref,
        data_class,
        source_binding_id: binding
            .get("binding_id")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        source_object_id: binding
            .get("source_object_id")
            .and_then(serde_json::Value::as_str)
            .map(ToOwned::to_owned),
        evidence: GovernedReadEvidence::SourceBinding,
        owner_org_unit_id: None,
        owner_subject: None,
    }))
}

fn memory_metadata_is_connector_sourced(metadata: Option<&serde_json::Value>) -> bool {
    metadata
        .and_then(|value| value.get("memory_trust"))
        .and_then(|value| value.get("label"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|label| label == "connector_sourced")
}

fn data_class_from_metadata(metadata: Option<&serde_json::Value>) -> Option<DataClass> {
    metadata
        .and_then(|value| value.get("classification"))
        .and_then(serde_json::Value::as_str)
        .and_then(data_class_from_label)
}

/// Metadata key that department-restricts a record to members of one org unit.
pub const OWNER_ORG_UNIT_METADATA_KEY: &str = "owner_org_unit_id";

pub fn owner_org_unit_id_from_metadata(metadata: Option<&serde_json::Value>) -> Option<String> {
    metadata
        .and_then(|value| value.get(OWNER_ORG_UNIT_METADATA_KEY))
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn data_class_from_label(label: &str) -> Option<DataClass> {
    match label.trim().to_ascii_lowercase().replace('-', "_").as_str() {
        "public" => Some(DataClass::Public),
        "internal" => Some(DataClass::Internal),
        "confidential" => Some(DataClass::Confidential),
        "restricted" => Some(DataClass::Restricted),
        "executive" => Some(DataClass::Executive),
        "credential" => Some(DataClass::Credential),
        "regulated" => Some(DataClass::Regulated),
        "customer_data" | "customer" => Some(DataClass::CustomerData),
        "source_code" | "code" => Some(DataClass::SourceCode),
        "financial_record" | "financial" | "finance" => Some(DataClass::FinancialRecord),
        _ => None,
    }
}

fn memory_chunk_resource_id(chunk: &MemoryChunk) -> String {
    match chunk.tier {
        MemoryTier::Session => chunk
            .session_id
            .as_ref()
            .map(|session_id| format!("session:{session_id}"))
            .unwrap_or_else(|| "session:default".to_string()),
        MemoryTier::Project => chunk
            .project_id
            .as_ref()
            .map(|project_id| format!("project:{project_id}"))
            .unwrap_or_else(|| "project:default".to_string()),
        MemoryTier::Global => chunk
            .project_id
            .as_ref()
            .map(|project_id| format!("global:{project_id}"))
            .unwrap_or_else(|| "global".to_string()),
    }
}

fn global_memory_record_resource_id(record: &GlobalMemoryRecord) -> String {
    if record.visibility.eq_ignore_ascii_case("shared") {
        record
            .project_tag
            .as_ref()
            .map(|project_id| format!("project:{project_id}"))
            .unwrap_or_else(|| "project:default".to_string())
    } else {
        record
            .session_id
            .as_ref()
            .map(|session_id| format!("session:{session_id}"))
            .unwrap_or_else(|| "global".to_string())
    }
}

/// Memory configuration for a project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Maximum chunks to store per project
    pub max_chunks: i64,
    /// Chunk size in tokens
    pub chunk_size: i64,
    /// Number of chunks to retrieve
    pub retrieval_k: i64,
    /// Whether auto-cleanup is enabled
    pub auto_cleanup: bool,
    /// Session memory retention in days
    pub session_retention_days: i64,
    /// Token budget for memory context injection
    pub token_budget: i64,
    /// Overlap between chunks in tokens
    pub chunk_overlap: i64,
    /// Retention in days for raw chat exchange records
    /// (`user_message`/`assistant_final` memory records). 0 = keep forever.
    #[serde(default = "default_exchange_retention_days")]
    pub exchange_retention_days: i64,
    /// Retention in days for global-tier memory chunks. 0 = disabled
    /// (global chunks are user-facing archived memory, so age-pruning is opt-in).
    #[serde(default)]
    pub global_retention_days: i64,
}

fn default_exchange_retention_days() -> i64 {
    365
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_chunks: 10_000,
            chunk_size: 512,
            retrieval_k: 5,
            auto_cleanup: true,
            session_retention_days: 30,
            token_budget: 5000,
            chunk_overlap: 64,
            exchange_retention_days: default_exchange_retention_days(),
            global_retention_days: 0,
        }
    }
}

/// Memory storage statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryStats {
    /// Total number of chunks
    pub total_chunks: i64,
    /// Number of session chunks
    pub session_chunks: i64,
    /// Number of project chunks
    pub project_chunks: i64,
    /// Number of global chunks
    pub global_chunks: i64,
    /// Total size in bytes
    pub total_bytes: i64,
    /// Session memory size in bytes
    pub session_bytes: i64,
    /// Project memory size in bytes
    pub project_bytes: i64,
    /// Global memory size in bytes
    pub global_bytes: i64,
    /// Database file size in bytes
    pub file_size: i64,
    /// Last cleanup timestamp
    pub last_cleanup: Option<DateTime<Utc>>,
}

/// Context to inject into messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryContext {
    /// Recent messages from current session
    pub current_session: Vec<MemoryChunk>,
    /// Relevant historical chunks
    pub relevant_history: Vec<MemoryChunk>,
    /// Important project facts
    pub project_facts: Vec<MemoryChunk>,
    /// Total tokens in context
    pub total_tokens: i64,
}

/// Metadata describing how memory retrieval executed for a single query.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRetrievalMeta {
    pub used: bool,
    pub chunks_total: usize,
    pub session_chunks: usize,
    pub history_chunks: usize,
    pub project_fact_chunks: usize,
    pub score_min: Option<f64>,
    pub score_max: Option<f64>,
}

impl MemoryContext {
    /// Format the context for injection into a prompt
    pub fn format_for_injection(&self) -> String {
        let mut parts = Vec::new();

        if !self.current_session.is_empty() {
            parts.push("<current_session>".to_string());
            for chunk in &self.current_session {
                parts.push(format!("- {}", chunk.content));
            }
            parts.push("</current_session>".to_string());
        }

        if !self.relevant_history.is_empty() {
            parts.push("<relevant_history>".to_string());
            for chunk in &self.relevant_history {
                parts.push(format!("- {}", chunk.content));
            }
            parts.push("</relevant_history>".to_string());
        }

        if !self.project_facts.is_empty() {
            parts.push("<project_facts>".to_string());
            for chunk in &self.project_facts {
                parts.push(format!("- {}", chunk.content));
            }
            parts.push("</project_facts>".to_string());
        }

        if parts.is_empty() {
            String::new()
        } else {
            format!("<memory_context>\n{}\n</memory_context>", parts.join("\n"))
        }
    }
}

/// Request to store a message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoreMessageRequest {
    pub content: String,
    pub tier: MemoryTier,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub source: String,
    // File-derived fields (only set when source == "file")
    pub source_path: Option<String>,
    pub source_mtime: Option<i64>,
    pub source_size: Option<i64>,
    pub source_hash: Option<String>,
    #[serde(default)]
    pub tenant_scope: MemoryTenantScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    pub metadata: Option<serde_json::Value>,
}

/// Project-scoped memory statistics (filtered by project_id)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectMemoryStats {
    pub project_id: String,
    /// Total chunks stored under this project_id (all sources)
    pub project_chunks: i64,
    pub project_bytes: i64,
    /// Chunks/bytes that came from workspace file indexing (source == "file")
    pub file_index_chunks: i64,
    pub file_index_bytes: i64,
    /// Number of indexed files currently tracked for this project_id
    pub indexed_files: i64,
    /// Last time indexing completed for this project_id (if known)
    pub last_indexed_at: Option<DateTime<Utc>>,
    /// Last run totals (if known)
    pub last_total_files: Option<i64>,
    pub last_processed_files: Option<i64>,
    pub last_indexed_files: Option<i64>,
    pub last_skipped_files: Option<i64>,
    pub last_errors: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearFileIndexResult {
    pub chunks_deleted: i64,
    pub bytes_estimated: i64,
    pub did_vacuum: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryImportFormat {
    Directory,
    Openclaw,
}

impl std::fmt::Display for MemoryImportFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryImportFormat::Directory => write!(f, "directory"),
            MemoryImportFormat::Openclaw => write!(f, "openclaw"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryImportRequest {
    pub root_path: String,
    pub format: MemoryImportFormat,
    pub tier: MemoryTier,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    #[serde(default)]
    pub tenant_scope: MemoryTenantScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_binding: Option<MemoryImportSourceBinding>,
    pub sync_deletes: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_namespace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryImportSourceBinding {
    pub binding_id: String,
    pub connector_id: String,
    pub resource_ref: serde_json::Value,
    pub data_class: String,
    #[serde(default)]
    pub require_review: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceObjectLifecycleState {
    Active,
    Quarantined,
    Tombstoned,
    Deleted,
    Rescoped,
}

impl SourceObjectLifecycleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Quarantined => "quarantined",
            Self::Tombstoned => "tombstoned",
            Self::Deleted => "deleted",
            Self::Rescoped => "rescoped",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "quarantined" => Self::Quarantined,
            "tombstoned" => Self::Tombstoned,
            "deleted" => Self::Deleted,
            "rescoped" => Self::Rescoped,
            _ => Self::Active,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceObjectLifecycleRecord {
    pub source_object_id: String,
    pub tenant_scope: MemoryTenantScope,
    pub source_binding_id: String,
    pub connector_id: String,
    pub state: SourceObjectLifecycleState,
    pub tier: MemoryTier,
    pub session_id: Option<String>,
    pub project_id: Option<String>,
    pub import_namespace: String,
    pub indexed_path: String,
    pub native_object_id: String,
    pub resource_ref: serde_json::Value,
    pub data_class: String,
    pub content_hash: Option<String>,
    pub source_hash: Option<String>,
    pub first_seen_at_ms: u64,
    pub last_seen_at_ms: u64,
    pub tombstoned_at_ms: Option<u64>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MemoryImportStats {
    pub discovered_files: usize,
    pub files_processed: usize,
    pub indexed_files: usize,
    pub skipped_files: usize,
    pub deleted_files: usize,
    pub chunks_created: usize,
    pub errors: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryImportProgress {
    pub files_processed: usize,
    pub total_files: usize,
    pub indexed_files: usize,
    pub skipped_files: usize,
    pub deleted_files: usize,
    pub errors: usize,
    pub chunks_created: usize,
    pub current_file: String,
}

/// Request to search memory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchMemoryRequest {
    pub query: String,
    pub tier: Option<MemoryTier>,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub limit: Option<i64>,
}

/// Embedding backend health surfaced to UI/events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingHealth {
    /// "ok" when embeddings are available, "degraded_disabled" otherwise.
    pub status: String,
    /// Optional reason when degraded.
    pub reason: Option<String>,
}

/// Memory error types
#[derive(Error, Debug)]
pub enum MemoryError {
    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Embedding error: {0}")]
    Embedding(String),

    #[error("Chunking error: {0}")]
    Chunking(String),

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("Tenant scope violation: {0}")]
    TenantScopeViolation(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Tokenization error: {0}")]
    Tokenization(String),

    #[error("Lock error: {0}")]
    Lock(String),
}

impl From<String> for MemoryError {
    fn from(err: String) -> Self {
        MemoryError::InvalidConfig(err)
    }
}

impl From<&str> for MemoryError {
    fn from(err: &str) -> Self {
        MemoryError::InvalidConfig(err.to_string())
    }
}

// Implement serialization for Tauri commands
impl serde::Serialize for MemoryError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

pub type MemoryResult<T> = Result<T, MemoryError>;

/// Cleanup log entry for audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupLogEntry {
    pub id: String,
    pub cleanup_type: String,
    pub tier: MemoryTier,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub chunks_deleted: i64,
    pub bytes_reclaimed: i64,
    pub created_at: DateTime<Utc>,
}

/// Default embedding dimension for all-MiniLM-L6-v2
pub const DEFAULT_EMBEDDING_DIMENSION: usize = 384;

/// Default embedding model name
pub const DEFAULT_EMBEDDING_MODEL: &str = "all-MiniLM-L6-v2";

/// Maximum content length for a single chunk (in characters)
pub const MAX_CHUNK_LENGTH: usize = 4000;

/// Minimum content length for a chunk (in characters)
pub const MIN_CHUNK_LENGTH: usize = 50;

/// Persistent global memory record keyed by user identity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMemoryRecord {
    pub id: String,
    pub user_id: String,
    pub source_type: String,
    pub content: String,
    pub content_hash: String,
    pub run_id: String,
    pub session_id: Option<String>,
    pub message_id: Option<String>,
    pub tool_name: Option<String>,
    pub project_tag: Option<String>,
    pub channel_tag: Option<String>,
    pub host_tag: Option<String>,
    pub metadata: Option<serde_json::Value>,
    pub provenance: Option<serde_json::Value>,
    pub redaction_status: String,
    pub redaction_count: u32,
    pub visibility: String,
    pub demoted: bool,
    pub score_boost: f64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub expires_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMemoryWriteResult {
    pub id: String,
    pub stored: bool,
    pub deduped: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlobalMemorySearchHit {
    pub record: GlobalMemoryRecord,
    pub score: f64,
}

/// Status for a reusable knowledge item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeItemStatus {
    #[default]
    Working,
    Promoted,
    ApprovedDefault,
    Deprecated,
}

impl std::fmt::Display for KnowledgeItemStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Working => write!(f, "working"),
            Self::Promoted => write!(f, "promoted"),
            Self::ApprovedDefault => write!(f, "approved_default"),
            Self::Deprecated => write!(f, "deprecated"),
        }
    }
}

impl std::str::FromStr for KnowledgeItemStatus {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "working" => Ok(Self::Working),
            "promoted" => Ok(Self::Promoted),
            "approved_default" => Ok(Self::ApprovedDefault),
            "deprecated" => Ok(Self::Deprecated),
            other => Err(format!("unknown knowledge item status: {}", other)),
        }
    }
}

impl KnowledgeItemStatus {
    pub fn as_trust_level(self) -> Option<KnowledgeTrustLevel> {
        match self {
            Self::Working => Some(KnowledgeTrustLevel::Working),
            Self::Promoted => Some(KnowledgeTrustLevel::Promoted),
            Self::ApprovedDefault => Some(KnowledgeTrustLevel::ApprovedDefault),
            Self::Deprecated => None,
        }
    }

    pub fn is_active(self) -> bool {
        !matches!(self, Self::Deprecated)
    }
}

/// A reusable knowledge space scoped to a project, run, or global namespace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeSpaceRecord {
    pub id: String,
    pub scope: KnowledgeScope,
    pub project_id: Option<String>,
    pub namespace: Option<String>,
    pub title: Option<String>,
    pub description: Option<String>,
    pub trust_level: KnowledgeTrustLevel,
    pub metadata: Option<serde_json::Value>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// A reusable knowledge item promoted from run artifacts or validated memory.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeItemRecord {
    pub id: String,
    pub space_id: String,
    pub coverage_key: String,
    pub dedupe_key: String,
    pub item_type: String,
    pub title: String,
    pub summary: Option<String>,
    pub payload: serde_json::Value,
    pub trust_level: KnowledgeTrustLevel,
    pub status: KnowledgeItemStatus,
    pub run_id: Option<String>,
    pub artifact_refs: Vec<String>,
    pub source_memory_ids: Vec<String>,
    pub freshness_expires_at_ms: Option<u64>,
    pub metadata: Option<serde_json::Value>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Coverage state for a task/topic key within a knowledge space.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeCoverageRecord {
    pub coverage_key: String,
    pub space_id: String,
    pub latest_item_id: Option<String>,
    pub latest_dedupe_key: Option<String>,
    pub last_seen_at_ms: u64,
    pub last_promoted_at_ms: Option<u64>,
    pub freshness_expires_at_ms: Option<u64>,
    pub metadata: Option<serde_json::Value>,
}

/// Request to promote or retire a knowledge item.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePromotionRequest {
    pub item_id: String,
    pub target_status: KnowledgeItemStatus,
    pub promoted_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness_expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub approval_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Result of a knowledge item promotion or retirement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgePromotionResult {
    pub previous_status: KnowledgeItemStatus,
    pub previous_trust_level: KnowledgeTrustLevel,
    pub promoted: bool,
    pub item: KnowledgeItemRecord,
    pub coverage: KnowledgeCoverageRecord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeType {
    Directory,
    File,
}

impl std::fmt::Display for NodeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NodeType::Directory => write!(f, "directory"),
            NodeType::File => write!(f, "file"),
        }
    }
}

impl std::str::FromStr for NodeType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "directory" => Ok(NodeType::Directory),
            "file" => Ok(NodeType::File),
            _ => Err(format!("unknown node type: {}", s)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum LayerType {
    L0,
    L1,
    L2,
}

impl std::fmt::Display for LayerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LayerType::L0 => write!(f, "L0"),
            LayerType::L1 => write!(f, "L1"),
            LayerType::L2 => write!(f, "L2"),
        }
    }
}

impl std::str::FromStr for LayerType {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_uppercase().as_str() {
            "L0" | "L0_ABSTRACT" => Ok(LayerType::L0),
            "L1" | "L1_OVERVIEW" => Ok(LayerType::L1),
            "L2" | "L2_DETAIL" => Ok(LayerType::L2),
            _ => Err(format!("unknown layer type: {}", s)),
        }
    }
}

impl LayerType {
    pub fn default_tokens(&self) -> usize {
        match self {
            LayerType::L0 => 100,
            LayerType::L1 => 2000,
            LayerType::L2 => 4000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalStep {
    pub step_type: String,
    pub description: String,
    pub layer_accessed: Option<LayerType>,
    pub nodes_evaluated: usize,
    pub scores: std::collections::HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeVisit {
    pub uri: String,
    pub node_type: NodeType,
    pub score: f64,
    pub depth: usize,
    pub layer_loaded: Option<LayerType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalTrajectory {
    pub id: String,
    pub query: String,
    pub root_uri: String,
    pub steps: Vec<RetrievalStep>,
    pub visited_nodes: Vec<NodeVisit>,
    pub total_duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrievalResult {
    pub node_id: String,
    pub uri: String,
    pub content: String,
    pub layer_type: LayerType,
    pub score: f64,
    pub trajectory: RetrievalTrajectory,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryNode {
    pub id: String,
    pub uri: String,
    pub parent_uri: Option<String>,
    pub node_type: NodeType,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryLayer {
    pub id: String,
    pub node_id: String,
    pub layer_type: LayerType,
    pub content: String,
    pub token_count: i64,
    pub embedding_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub source_chunk_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeNode {
    pub node: MemoryNode,
    pub children: Vec<TreeNode>,
    pub layer_summary: Option<LayerSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayerSummary {
    pub l0_preview: Option<String>,
    pub l1_preview: Option<String>,
    pub has_l2: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryListing {
    pub uri: String,
    pub nodes: Vec<MemoryNode>,
    pub total_children: usize,
    pub directories: Vec<MemoryNode>,
    pub files: Vec<MemoryNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistilledFact {
    pub id: String,
    pub distillation_id: String,
    pub content: String,
    pub category: FactCategory,
    pub importance_score: f64,
    pub source_message_ids: Vec<String>,
    pub contradicts_fact_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactCategory {
    UserPreference,
    TaskOutcome,
    Learning,
    Fact,
}

impl std::fmt::Display for FactCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FactCategory::UserPreference => write!(f, "user_preference"),
            FactCategory::TaskOutcome => write!(f, "task_outcome"),
            FactCategory::Learning => write!(f, "learning"),
            FactCategory::Fact => write!(f, "fact"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistillationReport {
    pub distillation_id: String,
    pub session_id: String,
    pub distilled_at: DateTime<Utc>,
    pub facts_extracted: usize,
    pub importance_threshold: f64,
    pub user_memory_updated: bool,
    pub agent_memory_updated: bool,
    #[serde(default)]
    pub stored_count: usize,
    #[serde(default)]
    pub deduped_count: usize,
    #[serde(default)]
    pub memory_ids: Vec<String>,
    #[serde(default)]
    pub candidate_ids: Vec<String>,
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDistillation {
    pub id: String,
    pub session_id: String,
    pub distilled_at: DateTime<Utc>,
    pub input_token_count: i64,
    pub output_memory_count: usize,
    pub key_facts_extracted: usize,
    pub importance_threshold: f64,
}
