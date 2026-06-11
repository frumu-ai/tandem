use crate::{EdgeId, Freshness, GraphSchemaVersion, GraphScope, NodeId, PolicyDecision};
use crate::{Provenance, Visibility};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum GraphDomain {
    #[serde(rename = "repo")]
    Repo,
    #[serde(rename = "workflow")]
    Workflow,
    #[serde(rename = "tool")]
    Tool,
    #[serde(rename = "memory")]
    Memory,
    #[serde(rename = "policy")]
    Policy,
    #[serde(rename = "run")]
    Run,
    #[serde(rename = "artifact")]
    Artifact,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeKind {
    #[serde(rename = "repo.repository")]
    Repository,
    #[serde(rename = "repo.file")]
    File,
    #[serde(rename = "repo.symbol")]
    Symbol,
    #[serde(rename = "repo.import")]
    Import,
    #[serde(rename = "repo.test_target")]
    TestTarget,
    #[serde(rename = "repo.config_entry")]
    ConfigEntry,
    #[serde(rename = "repo.doc_section")]
    DocSection,
    #[serde(rename = "workflow.template")]
    WorkflowTemplate,
    #[serde(rename = "workflow.version")]
    WorkflowVersion,
    #[serde(rename = "workflow.step")]
    WorkflowStep,
    #[serde(rename = "workflow.dependency")]
    WorkflowDependency,
    #[serde(rename = "workflow.approval_gate")]
    ApprovalGate,
    #[serde(rename = "tool.mcp_server")]
    McpServer,
    #[serde(rename = "tool.definition")]
    ToolDefinition,
    #[serde(rename = "tool.credential")]
    Credential,
    #[serde(rename = "tool.schema")]
    ToolSchema,
    #[serde(rename = "tool.authority")]
    Authority,
    #[serde(rename = "memory.tier")]
    MemoryTier,
    #[serde(rename = "memory.collection")]
    MemoryCollection,
    #[serde(rename = "memory.retrieved")]
    RetrievedMemory,
    #[serde(rename = "memory.write_candidate")]
    MemoryWriteCandidate,
    #[serde(rename = "policy.scope")]
    PolicyScope,
    #[serde(rename = "policy.budget")]
    PolicyBudget,
    #[serde(rename = "policy.sandbox_limit")]
    SandboxLimit,
    #[serde(rename = "policy.data_boundary")]
    DataBoundary,
    #[serde(rename = "run.run")]
    Run,
    #[serde(rename = "run.model_call")]
    ModelCall,
    #[serde(rename = "run.tool_call")]
    ToolCall,
    #[serde(rename = "run.error")]
    Error,
    #[serde(rename = "run.retry")]
    Retry,
    #[serde(rename = "run.output")]
    Output,
    #[serde(rename = "run.cost")]
    Cost,
    #[serde(rename = "artifact.artifact")]
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
    #[serde(rename = "contains")]
    Contains,
    #[serde(rename = "imports")]
    Imports,
    #[serde(rename = "defines")]
    Defines,
    #[serde(rename = "references")]
    References,
    #[serde(rename = "configures")]
    Configures,
    #[serde(rename = "documents")]
    Documents,
    #[serde(rename = "tests")]
    Tests,
    #[serde(rename = "likely_related")]
    LikelyRelated,
    #[serde(rename = "changed_with")]
    ChangedWith,
    #[serde(rename = "depends_on")]
    DependsOn,
    #[serde(rename = "requires_approval")]
    RequiresApproval,
    #[serde(rename = "requires_tool")]
    RequiresTool,
    #[serde(rename = "requires_memory")]
    RequiresMemory,
    #[serde(rename = "governed_by")]
    GovernedBy,
    #[serde(rename = "produces")]
    Produces,
    #[serde(rename = "consumes")]
    Consumes,
    #[serde(rename = "observed_in")]
    ObservedIn,
    #[serde(rename = "blocks")]
    Blocks,
    #[serde(rename = "retries")]
    Retries,
    #[serde(rename = "costs")]
    Costs,
    #[serde(rename = "has_credential")]
    HasCredential,
    #[serde(rename = "has_schema")]
    HasSchema,
    #[serde(rename = "has_authority")]
    HasAuthority,
    #[serde(rename = "visible_to")]
    VisibleTo,
    #[serde(rename = "freshened_by")]
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
