//! Shared context graph primitives for Tandem.
//!
//! This crate is intentionally dependency-light. Repo, workflow, memory, policy,
//! and run adapters can share these types without pulling in runtime services.

mod audit;
mod context_payloads;
mod graph_build;
mod hash;
mod ids;
mod query_envelope;
mod run_trace;
mod storage;
mod taxonomy;
mod trust;
mod workflow_graph;
mod workflow_impact;
mod workflow_impact_types;
mod workflow_memory;
mod workflow_rerun;
mod workflow_runtime;
mod workflow_runtime_topology;

pub use audit::{
    GraphAuditDecision, GraphAuditEvent, GraphAuditEventType, GraphAuditMetrics, GraphAuditTarget,
};
pub use context_payloads::{
    ApprovalGateNode, ArtifactNode, ContextNodePayload, DataBoundaryNode, McpServerNode,
    MemoryCollectionNode, MemoryTierNode, MemoryWriteCandidateNode, PolicyBudgetNode,
    PolicyScopeNode, RetrievedMemoryNode, SandboxLimitNode, ToolAuthorityNode, ToolCredentialNode,
    ToolDefinitionNode, ToolSchemaNode,
};
pub use hash::{stable_graph_hash, StableGraphHash, StableGraphHashError};
pub use ids::{EdgeId, GraphSchemaVersion, GraphScope, NodeId};
pub use query_envelope::{
    GraphQueryAudit, GraphQueryEnvelope, GraphQueryEnvelopeError, GraphQueryOutput,
};
pub use run_trace::{RunTraceEvent, RunTraceEventKind, RunTraceGraph, RunTraceGraphSpec};
pub use storage::{
    GraphPartitionKind, GraphRetentionClass, GraphRetentionPolicy, GraphStoragePartition,
};
pub use taxonomy::{
    EdgeKind, GraphDomain, GraphEdge, GraphFact, GraphNode, GraphPayload, NodeKind,
};
pub use trust::{Freshness, FreshnessSource, PolicyDecision, Provenance, Visibility};
pub use workflow_graph::{
    WorkflowGraph, WorkflowGraphSpec, WorkflowStepDependencySummary, WorkflowStepGraphNode,
    WorkflowTemplateGraphNode, WorkflowVersionGraphNode,
};
pub use workflow_impact_types::{
    WorkflowImpactChange, WorkflowImpactQuery, WorkflowImpactReport, WorkflowImpactRiskGroup,
    WorkflowImpactRiskHint, WorkflowImpactStep, WorkflowImpactWorkflow,
};
pub use workflow_memory::{
    WorkflowMemoryBundle, WorkflowMemoryCandidate, WorkflowMemoryMatch, WorkflowMemoryQuery,
};
pub use workflow_rerun::{
    WorkflowRerunChange, WorkflowRerunPlan, WorkflowRerunStep, WorkflowStepCacheKey,
};
pub use workflow_runtime::{
    WorkflowBlockedNode, WorkflowBlocker, WorkflowBlockerKind, WorkflowPreflightReport,
    WorkflowPromptPruningMetrics, WorkflowReadyNode, WorkflowRuntimePlan, WorkflowRuntimeState,
    WorkflowToolCandidate, WorkflowToolSelection,
};

#[cfg(test)]
mod tests;
#[cfg(test)]
mod workflow_impact_tests;
#[cfg(test)]
mod workflow_memory_rerun_tests;
#[cfg(test)]
mod workflow_run_tests;
