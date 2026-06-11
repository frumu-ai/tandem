use crate::{EdgeId, Freshness, GraphSchemaVersion, GraphScope, NodeId, PolicyDecision};
use crate::{Provenance, Visibility};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphDomain {
    Repo,
    Workflow,
    Tool,
    Memory,
    Policy,
    Run,
    Artifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    Repository,
    File,
    Symbol,
    Import,
    TestTarget,
    ConfigEntry,
    DocSection,
    WorkflowTemplate,
    WorkflowVersion,
    WorkflowStep,
    WorkflowDependency,
    ApprovalGate,
    McpServer,
    ToolDefinition,
    Credential,
    ToolSchema,
    Authority,
    MemoryTier,
    MemoryCollection,
    RetrievedMemory,
    MemoryWriteCandidate,
    PolicyScope,
    PolicyBudget,
    SandboxLimit,
    DataBoundary,
    Run,
    ModelCall,
    ToolCall,
    Error,
    Retry,
    Output,
    Cost,
    Artifact,
}

impl NodeKind {
    pub fn stable_id(&self) -> &'static str {
        match self {
            Self::Repository => "repo.repository",
            Self::File => "repo.file",
            Self::Symbol => "repo.symbol",
            Self::Import => "repo.import",
            Self::TestTarget => "repo.test_target",
            Self::ConfigEntry => "repo.config_entry",
            Self::DocSection => "repo.doc_section",
            Self::WorkflowTemplate => "workflow.template",
            Self::WorkflowVersion => "workflow.version",
            Self::WorkflowStep => "workflow.step",
            Self::WorkflowDependency => "workflow.dependency",
            Self::ApprovalGate => "workflow.approval_gate",
            Self::McpServer => "tool.mcp_server",
            Self::ToolDefinition => "tool.definition",
            Self::Credential => "tool.credential",
            Self::ToolSchema => "tool.schema",
            Self::Authority => "tool.authority",
            Self::MemoryTier => "memory.tier",
            Self::MemoryCollection => "memory.collection",
            Self::RetrievedMemory => "memory.retrieved",
            Self::MemoryWriteCandidate => "memory.write_candidate",
            Self::PolicyScope => "policy.scope",
            Self::PolicyBudget => "policy.budget",
            Self::SandboxLimit => "policy.sandbox_limit",
            Self::DataBoundary => "policy.data_boundary",
            Self::Run => "run.run",
            Self::ModelCall => "run.model_call",
            Self::ToolCall => "run.tool_call",
            Self::Error => "run.error",
            Self::Retry => "run.retry",
            Self::Output => "run.output",
            Self::Cost => "run.cost",
            Self::Artifact => "artifact.artifact",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EdgeKind {
    Contains,
    Imports,
    Defines,
    References,
    Configures,
    Documents,
    Tests,
    LikelyRelated,
    ChangedWith,
    DependsOn,
    RequiresApproval,
    RequiresTool,
    RequiresMemory,
    GovernedBy,
    Produces,
    Consumes,
    ObservedIn,
    Blocks,
    Retries,
    Costs,
    HasCredential,
    HasSchema,
    HasAuthority,
    VisibleTo,
    FreshenedBy,
}

impl EdgeKind {
    pub fn stable_id(&self) -> &'static str {
        match self {
            Self::Contains => "contains",
            Self::Imports => "imports",
            Self::Defines => "defines",
            Self::References => "references",
            Self::Configures => "configures",
            Self::Documents => "documents",
            Self::Tests => "tests",
            Self::LikelyRelated => "likely_related",
            Self::ChangedWith => "changed_with",
            Self::DependsOn => "depends_on",
            Self::RequiresApproval => "requires_approval",
            Self::RequiresTool => "requires_tool",
            Self::RequiresMemory => "requires_memory",
            Self::GovernedBy => "governed_by",
            Self::Produces => "produces",
            Self::Consumes => "consumes",
            Self::ObservedIn => "observed_in",
            Self::Blocks => "blocks",
            Self::Retries => "retries",
            Self::Costs => "costs",
            Self::HasCredential => "has_credential",
            Self::HasSchema => "has_schema",
            Self::HasAuthority => "has_authority",
            Self::VisibleTo => "visible_to",
            Self::FreshenedBy => "freshened_by",
        }
    }
}

pub type GraphPayload = BTreeMap<String, String>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: NodeId,
    pub kind: NodeKind,
    pub label: String,
    pub payload: GraphPayload,
    pub provenance: Provenance,
    pub freshness: Freshness,
    pub visibility: Visibility,
    pub policy: PolicyDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphEdge {
    pub id: EdgeId,
    pub kind: EdgeKind,
    pub source: NodeId,
    pub target: NodeId,
    pub payload: GraphPayload,
    pub provenance: Provenance,
    pub freshness: Freshness,
    pub visibility: Visibility,
    pub policy: PolicyDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GraphFact {
    pub schema_version: GraphSchemaVersion,
    pub scope: GraphScope,
    pub domain: GraphDomain,
    pub source_key: String,
    pub target_key: String,
    pub edge_kind: EdgeKind,
    pub provenance: Provenance,
    pub freshness: Freshness,
    pub visibility: Visibility,
    pub policy: PolicyDecision,
    pub evidence: GraphPayload,
}

impl GraphFact {
    pub fn new(
        scope: GraphScope,
        domain: GraphDomain,
        source_key: impl Into<String>,
        target_key: impl Into<String>,
        edge_kind: EdgeKind,
        provenance: Provenance,
    ) -> Self {
        Self {
            schema_version: GraphSchemaVersion::default(),
            scope,
            domain,
            source_key: source_key.into(),
            target_key: target_key.into(),
            edge_kind,
            provenance,
            freshness: Freshness::unknown(),
            visibility: Visibility::default(),
            policy: PolicyDecision::Allowed,
            evidence: GraphPayload::new(),
        }
    }
}
