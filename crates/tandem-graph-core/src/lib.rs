//! Shared context graph primitives for Tandem.
//!
//! This crate is intentionally dependency-light. Repo, workflow, memory, policy,
//! and run adapters can share these types without pulling in runtime services.

mod context_payloads;
mod hash;
mod ids;
mod query_envelope;
mod taxonomy;
mod trust;

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
pub use taxonomy::{
    EdgeKind, GraphDomain, GraphEdge, GraphFact, GraphNode, GraphPayload, NodeKind,
};
pub use trust::{Freshness, FreshnessSource, PolicyDecision, Provenance, Visibility};

#[cfg(test)]
mod tests;
