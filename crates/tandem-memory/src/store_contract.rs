use std::fmt;

use crate::types::{
    CleanupLogEntry, GlobalMemoryRecord, GlobalMemorySearchHit, GlobalMemoryWriteResult,
    KnowledgeCoverageRecord, KnowledgeItemRecord, KnowledgePromotionRequest,
    KnowledgePromotionResult, KnowledgeSpaceRecord, LayerType, MemoryChunk, MemoryConfig,
    MemoryError, MemoryLayer, MemoryNode, MemoryStats, MemoryTenantScope, MemoryTier, NodeType,
    ProjectMemoryStats, SourceObjectLifecycleRecord, TreeNode,
};

use super::{MemoryReadScope, MemoryWriteScope};

/// Backend-neutral result returned by the portable storage contract.
pub type MemoryStoreResult<T> = Result<T, MemoryStoreError>;

/// Stable error classes that business logic may branch on without knowing the
/// database driver used by the active backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryStoreErrorKind {
    InvalidRequest,
    ScopeViolation,
    NotFound,
    Conflict,
    Unsupported,
    Unavailable,
    CorruptData,
    Internal,
}

/// An error crossing the [`super::MemoryStore`] boundary.
///
/// Driver error values deliberately stop at the adapter. The message is useful
/// for diagnostics, while `kind` and `retryable` are the portable contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryStoreError {
    pub kind: MemoryStoreErrorKind,
    pub message: String,
    pub retryable: bool,
}

impl MemoryStoreError {
    pub fn new(kind: MemoryStoreErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            retryable: false,
        }
    }

    pub fn retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(MemoryStoreErrorKind::InvalidRequest, message)
    }

    pub fn unsupported(message: impl Into<String>) -> Self {
        Self::new(MemoryStoreErrorKind::Unsupported, message)
    }
}

impl fmt::Display for MemoryStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for MemoryStoreError {}

impl From<MemoryError> for MemoryStoreError {
    fn from(error: MemoryError) -> Self {
        let (kind, retryable) = match &error {
            MemoryError::InvalidConfig(_) => (MemoryStoreErrorKind::InvalidRequest, false),
            MemoryError::TenantScopeViolation(_) => (MemoryStoreErrorKind::ScopeViolation, false),
            MemoryError::NotFound(_) => (MemoryStoreErrorKind::NotFound, false),
            MemoryError::Io(_) => (MemoryStoreErrorKind::Unavailable, true),
            MemoryError::Serialization(_) => (MemoryStoreErrorKind::CorruptData, false),
            MemoryError::Database(_) => (MemoryStoreErrorKind::Internal, false),
            MemoryError::Embedding(_)
            | MemoryError::Chunking(_)
            | MemoryError::Tokenization(_)
            | MemoryError::Lock(_) => (MemoryStoreErrorKind::Internal, false),
        };
        Self::new(kind, error.to_string()).retryable(retryable)
    }
}

/// Coordinates selecting chunks in one memory tier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryChunkSelector {
    pub tier: MemoryTier,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
}

impl MemoryChunkSelector {
    pub fn session(session_id: impl Into<String>) -> Self {
        Self {
            tier: MemoryTier::Session,
            project_id: None,
            session_id: Some(session_id.into()),
        }
    }

    pub fn project(project_id: impl Into<String>) -> Self {
        Self {
            tier: MemoryTier::Project,
            project_id: Some(project_id.into()),
            session_id: None,
        }
    }

    /// Select every session-tier chunk visible within the request scope.
    ///
    /// This selector is valid for vector search only. Point/list operations
    /// continue to require a concrete session id.
    pub fn all_sessions() -> Self {
        Self {
            tier: MemoryTier::Session,
            project_id: None,
            session_id: None,
        }
    }

    pub fn global() -> Self {
        Self {
            tier: MemoryTier::Global,
            project_id: None,
            session_id: None,
        }
    }
}

/// Scoped point/list reads used by memory business logic.
#[derive(Debug, Clone)]
pub enum MemoryStoreReadRequest {
    Chunks {
        scope: MemoryReadScope,
        selector: MemoryChunkSelector,
        limit: Option<i64>,
    },
    GlobalRecord {
        scope: MemoryReadScope,
        id: String,
    },
    ProjectConfig {
        scope: MemoryReadScope,
        project_id: String,
    },
    Stats {
        scope: MemoryReadScope,
    },
    ProjectStats {
        scope: MemoryReadScope,
        project_id: String,
    },
    KnowledgeSpace {
        scope: MemoryReadScope,
        id: String,
    },
    KnowledgeItem {
        scope: MemoryReadScope,
        id: String,
    },
    KnowledgeCoverage {
        scope: MemoryReadScope,
        coverage_key: String,
        space_id: String,
    },
    ImportIndexEntry {
        scope: MemoryReadScope,
        selector: MemoryChunkSelector,
        path: String,
    },
    ContextNode {
        scope: MemoryReadScope,
        uri: String,
    },
    ContextLayer {
        scope: MemoryReadScope,
        node_id: String,
        layer_type: LayerType,
    },
}

/// Typed values returned by [`MemoryStoreReadRequest`].
#[derive(Debug, Clone)]
pub enum MemoryStoreReadResult {
    Chunks(Vec<MemoryChunk>),
    GlobalRecord(Option<GlobalMemoryRecord>),
    ProjectConfig(MemoryConfig),
    Stats(MemoryStats),
    ProjectStats(ProjectMemoryStats),
    KnowledgeSpace(Option<KnowledgeSpaceRecord>),
    KnowledgeItem(Option<KnowledgeItemRecord>),
    KnowledgeCoverage(Option<KnowledgeCoverageRecord>),
    ImportIndexEntry(Option<MemoryImportIndexEntry>),
    ContextNode(Option<MemoryNode>),
    ContextLayer(Option<MemoryLayer>),
}

/// Portable replacement for the concrete import-index tuple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryImportIndexEntry {
    pub modified_at: i64,
    pub size: i64,
    pub hash: String,
}

/// Scoped search and filtered-list operations.
#[derive(Debug, Clone)]
pub enum MemoryStoreQueryRequest {
    SimilarChunks {
        scope: MemoryReadScope,
        selector: MemoryChunkSelector,
        query_embedding: Vec<f32>,
        limit: i64,
    },
    SearchGlobalRecords {
        scope: MemoryReadScope,
        user_id: String,
        query: String,
        limit: i64,
        project_tag: Option<String>,
    },
    ListGlobalRecords {
        scope: MemoryReadScope,
        user_id: String,
        query: Option<String>,
        project_tag: Option<String>,
        channel_tag: Option<String>,
        limit: i64,
        offset: i64,
    },
    KnowledgeSpaces {
        scope: MemoryReadScope,
        project_id: Option<String>,
    },
    KnowledgeItems {
        scope: MemoryReadScope,
        space_id: String,
        coverage_key: Option<String>,
    },
    ImportIndexPaths {
        scope: MemoryReadScope,
        selector: MemoryChunkSelector,
    },
    CleanupLog {
        scope: MemoryReadScope,
        limit: i64,
    },
    ContextNodes {
        scope: MemoryReadScope,
        parent_uri: String,
    },
    ContextTree {
        scope: MemoryReadScope,
        parent_uri: String,
        max_depth: usize,
    },
    SourceObjectLifecyclesForBinding {
        scope: MemoryReadScope,
        source_binding_id: String,
    },
}

/// Typed values returned by [`MemoryStoreQueryRequest`].
#[derive(Debug, Clone)]
pub enum MemoryStoreQueryResult {
    SimilarChunks(Vec<(MemoryChunk, f64)>),
    GlobalSearchHits(Vec<GlobalMemorySearchHit>),
    GlobalRecords(Vec<GlobalMemoryRecord>),
    KnowledgeSpaces(Vec<KnowledgeSpaceRecord>),
    KnowledgeItems(Vec<KnowledgeItemRecord>),
    Paths(Vec<String>),
    CleanupLog(Vec<CleanupLogEntry>),
    ContextNodes(Vec<MemoryNode>),
    ContextTree(Vec<TreeNode>),
    SourceObjectLifecycles(Vec<SourceObjectLifecycleRecord>),
}

/// Backend-neutral cleanup audit row to append.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryCleanupLogWrite {
    pub cleanup_type: String,
    pub tier: MemoryTier,
    pub project_id: Option<String>,
    pub session_id: Option<String>,
    pub chunks_deleted: i64,
    pub bytes_reclaimed: i64,
}

/// Scoped inserts and upserts.
#[derive(Debug, Clone)]
pub enum MemoryStoreWriteRequest {
    Chunk {
        scope: MemoryWriteScope,
        chunk: MemoryChunk,
        embedding: Vec<f32>,
    },
    GlobalRecord {
        scope: MemoryWriteScope,
        record: GlobalMemoryRecord,
    },
    ProjectConfig {
        scope: MemoryWriteScope,
        project_id: String,
        config: MemoryConfig,
    },
    KnowledgeSpace {
        scope: MemoryWriteScope,
        record: KnowledgeSpaceRecord,
    },
    KnowledgeItem {
        scope: MemoryWriteScope,
        record: KnowledgeItemRecord,
    },
    KnowledgeCoverage {
        scope: MemoryWriteScope,
        record: KnowledgeCoverageRecord,
    },
    ImportIndexEntry {
        scope: MemoryWriteScope,
        selector: MemoryChunkSelector,
        path: String,
        entry: MemoryImportIndexEntry,
    },
    /// Insert a source object or refresh its active lifecycle state.
    SourceObjectLifecycle {
        scope: MemoryWriteScope,
        record: SourceObjectLifecycleRecord,
    },
    ContextNode {
        scope: MemoryWriteScope,
        uri: String,
        parent_uri: Option<String>,
        node_type: NodeType,
        metadata: Option<serde_json::Value>,
    },
    ContextLayer {
        scope: MemoryWriteScope,
        node_id: String,
        layer_type: LayerType,
        content: String,
        token_count: i64,
        source_chunk_id: Option<String>,
    },
    CleanupLog {
        scope: MemoryWriteScope,
        entry: MemoryCleanupLogWrite,
    },
}

/// Typed values returned by [`MemoryStoreWriteRequest`].
#[derive(Debug, Clone)]
pub enum MemoryStoreWriteResult {
    Stored,
    GlobalRecord(GlobalMemoryWriteResult),
    ContextNodeCreated(String),
    ContextLayerCreated(String),
}

/// Scoped updates, deletes, maintenance, and other state transitions.
#[derive(Debug, Clone)]
pub enum MemoryStoreMutationRequest {
    DeleteChunk {
        scope: MemoryReadScope,
        selector: MemoryChunkSelector,
        chunk_id: String,
    },
    ClearSession {
        scope: MemoryReadScope,
        session_id: String,
    },
    ClearProject {
        scope: MemoryReadScope,
        project_id: String,
    },
    DeleteGlobalRecord {
        scope: MemoryReadScope,
        id: String,
    },
    UpdateGlobalRecordContext {
        scope: MemoryReadScope,
        id: String,
        visibility: String,
        demoted: bool,
        metadata: Option<serde_json::Value>,
        provenance: Option<serde_json::Value>,
    },
    PromoteKnowledgeItem {
        scope: MemoryReadScope,
        request: KnowledgePromotionRequest,
    },
    DeleteImportIndexEntry {
        scope: MemoryReadScope,
        selector: MemoryChunkSelector,
        path: String,
    },
    /// Delete imported file chunks and their vectors for one source path.
    DeleteChunksBySourcePath {
        scope: MemoryReadScope,
        selector: MemoryChunkSelector,
        source_path: String,
    },
    /// Replace metadata on imported file chunks for one source path.
    UpdateChunkMetadataBySourcePath {
        scope: MemoryReadScope,
        selector: MemoryChunkSelector,
        source_path: String,
        metadata: serde_json::Value,
    },
    /// Mark a source object as no longer present in its source binding.
    TombstoneSourceObjectLifecycle {
        scope: MemoryReadScope,
        source_binding_id: String,
        native_object_id: String,
        tombstoned_at_ms: u64,
    },
    SetSourceObjectLifecycleState {
        scope: MemoryReadScope,
        source_binding_id: String,
        source_object_id: String,
        state: crate::types::SourceObjectLifecycleState,
        changed_at_ms: u64,
    },
    RunHygiene {
        scope: MemoryReadScope,
        retention_days: u32,
    },
    /// Run retention across every tenant partition. This is a backend-owned
    /// scheduled-maintenance operation, not a caller-supplied tenant action.
    RunHygieneAllTenants {
        retention_days: u32,
    },
    EnforceProjectChunkCap {
        scope: MemoryReadScope,
        project_id: String,
        max_chunks: i64,
    },
    /// Reclaim unused storage across the whole backend.
    Vacuum,
}

/// Typed values returned by [`MemoryStoreMutationRequest`].
#[derive(Debug, Clone)]
pub enum MemoryStoreMutationResult {
    Affected(u64),
    Changed(bool),
    SourcePathDelete(MemorySourcePathDeleteResult),
    Promotion(Option<KnowledgePromotionResult>),
    Completed,
}

/// Counts reported after deleting imported chunks for one source path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemorySourcePathDeleteResult {
    pub chunks_deleted: i64,
    pub bytes_reclaimed: i64,
}

/// Commit semantics requested for multiple storage operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryStoreBatchMode {
    /// All writes commit or all writes roll back.
    Atomic,
    /// Stop after the first failed item; already completed items remain applied.
    StopOnError,
    /// Attempt every item and report each outcome.
    BestEffort,
}

#[derive(Debug, Clone)]
pub enum MemoryStoreBatchOperation {
    Write(MemoryStoreWriteRequest),
    Mutation(MemoryStoreMutationRequest),
}

#[derive(Debug, Clone)]
pub struct MemoryStoreBatchRequest {
    pub mode: MemoryStoreBatchMode,
    pub operations: Vec<MemoryStoreBatchOperation>,
}

#[derive(Debug, Clone)]
pub enum MemoryStoreBatchValue {
    Write(MemoryStoreWriteResult),
    Mutation(MemoryStoreMutationResult),
}

#[derive(Debug, Clone)]
pub struct MemoryStoreBatchItemResult {
    pub index: usize,
    pub result: MemoryStoreResult<MemoryStoreBatchValue>,
}

#[derive(Debug, Clone)]
pub struct MemoryStoreBatchResult {
    /// True only when every requested operation completed successfully.
    pub completed: bool,
    pub items: Vec<MemoryStoreBatchItemResult>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoryBackendKind {
    Sqlite,
    Postgres,
    Other(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryBackendHealthStatus {
    Healthy,
    Degraded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryBackendHealthRequest {
    /// Permit the backend to repair its vector/index structures during probing.
    pub repair: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryBackendHealthResult {
    pub backend: MemoryBackendKind,
    pub status: MemoryBackendHealthStatus,
    pub repaired: bool,
    pub checks: Vec<MemoryBackendHealthCheck>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryBackendHealthCheck {
    pub name: String,
    pub healthy: bool,
    pub detail: Option<String>,
}

/// Explicit backend-wide recovery actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryBackendRecoveryAction {
    /// Repair backend-owned indexes without deleting memory records.
    RepairIndexes,
    /// Drop all memory data and recreate the backend schema.
    ResetAllData,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryBackendRecoveryRequest {
    pub action: MemoryBackendRecoveryAction,
    /// Must be true for [`MemoryBackendRecoveryAction::ResetAllData`].
    pub confirm_data_loss: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryBackendRecoveryResult {
    pub backend: MemoryBackendKind,
    pub action: MemoryBackendRecoveryAction,
    /// True when the backend changed persistent state.
    pub changed: bool,
}

/// Requirements a migration coordinator can ask a backend to satisfy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryMigrationCapabilityRequest {
    pub require_version_introspection: bool,
    pub require_transactional_apply: bool,
    pub require_online_apply: bool,
    pub require_dry_run: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryMigrationApplyMode {
    OnOpen,
    Explicit,
}

/// Backend migration features, separate from any backend's DDL syntax.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryMigrationCapabilityResult {
    pub backend: MemoryBackendKind,
    pub apply_mode: MemoryMigrationApplyMode,
    pub version_introspection: bool,
    pub transactional_apply: bool,
    pub online_apply: bool,
    pub dry_run: bool,
    pub requirements_satisfied: bool,
}

impl MemoryMigrationCapabilityResult {
    pub fn satisfies(&self, request: &MemoryMigrationCapabilityRequest) -> bool {
        (!request.require_version_introspection || self.version_introspection)
            && (!request.require_transactional_apply || self.transactional_apply)
            && (!request.require_online_apply || self.online_apply)
            && (!request.require_dry_run || self.dry_run)
    }
}

pub(crate) fn tenant_scope_from_global_record(record: &GlobalMemoryRecord) -> MemoryTenantScope {
    record
        .provenance
        .as_ref()
        .and_then(|value| value.get("tenant_context"))
        .and_then(|value| {
            Some(MemoryTenantScope {
                org_id: value.get("org_id")?.as_str()?.to_string(),
                workspace_id: value.get("workspace_id")?.as_str()?.to_string(),
                deployment_id: value
                    .get("deployment_id")
                    .and_then(serde_json::Value::as_str)
                    .map(ToString::to_string),
            })
        })
        .unwrap_or_default()
}
