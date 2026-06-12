use crate::{GraphPayload, NodeKind};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContextNodePayload {
    McpServer(McpServerNode),
    ToolDefinition(ToolDefinitionNode),
    ToolCredential(ToolCredentialNode),
    ToolSchema(ToolSchemaNode),
    ToolAuthority(ToolAuthorityNode),
    MemoryTier(MemoryTierNode),
    MemoryCollection(MemoryCollectionNode),
    RetrievedMemory(RetrievedMemoryNode),
    MemoryWriteCandidate(MemoryWriteCandidateNode),
    PolicyScope(PolicyScopeNode),
    PolicyBudget(PolicyBudgetNode),
    SandboxLimit(SandboxLimitNode),
    DataBoundary(DataBoundaryNode),
    ApprovalGate(ApprovalGateNode),
    Artifact(ArtifactNode),
}

impl ContextNodePayload {
    pub fn node_kind(&self) -> NodeKind {
        match self {
            Self::McpServer(_) => NodeKind::McpServer,
            Self::ToolDefinition(_) => NodeKind::ToolDefinition,
            Self::ToolCredential(_) => NodeKind::Credential,
            Self::ToolSchema(_) => NodeKind::ToolSchema,
            Self::ToolAuthority(_) => NodeKind::Authority,
            Self::MemoryTier(_) => NodeKind::MemoryTier,
            Self::MemoryCollection(_) => NodeKind::MemoryCollection,
            Self::RetrievedMemory(_) => NodeKind::RetrievedMemory,
            Self::MemoryWriteCandidate(_) => NodeKind::MemoryWriteCandidate,
            Self::PolicyScope(_) => NodeKind::PolicyScope,
            Self::PolicyBudget(_) => NodeKind::PolicyBudget,
            Self::SandboxLimit(_) => NodeKind::SandboxLimit,
            Self::DataBoundary(_) => NodeKind::DataBoundary,
            Self::ApprovalGate(_) => NodeKind::ApprovalGate,
            Self::Artifact(_) => NodeKind::Artifact,
        }
    }

    pub fn display_safe_payload(&self) -> GraphPayload {
        match self {
            Self::McpServer(node) => node.display_safe_payload(),
            Self::ToolDefinition(node) => node.display_safe_payload(),
            Self::ToolCredential(node) => node.display_safe_payload(),
            Self::ToolSchema(node) => node.display_safe_payload(),
            Self::ToolAuthority(node) => node.display_safe_payload(),
            Self::MemoryTier(node) => node.display_safe_payload(),
            Self::MemoryCollection(node) => node.display_safe_payload(),
            Self::RetrievedMemory(node) => node.display_safe_payload(),
            Self::MemoryWriteCandidate(node) => node.display_safe_payload(),
            Self::PolicyScope(node) => node.display_safe_payload(),
            Self::PolicyBudget(node) => node.display_safe_payload(),
            Self::SandboxLimit(node) => node.display_safe_payload(),
            Self::DataBoundary(node) => node.display_safe_payload(),
            Self::ApprovalGate(node) => node.display_safe_payload(),
            Self::Artifact(node) => node.display_safe_payload(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerNode {
    pub server_id: String,
    pub display_name: String,
    pub transport: String,
    pub enabled: bool,
    pub tool_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDefinitionNode {
    pub tool_name: String,
    pub server_id: Option<String>,
    pub schema_hash: Option<String>,
    pub authority_level: String,
    pub read_only: bool,
    pub side_effects: bool,
    pub credential_ref: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolCredentialNode {
    pub provider: String,
    pub credential_ref: String,
    pub status: String,
    pub scopes: Vec<String>,
    pub expires_at_unix_ms: Option<u64>,
    pub secret_material_present: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolSchemaNode {
    pub schema_hash: String,
    pub version: Option<String>,
    pub input_summary: String,
    pub output_summary: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolAuthorityNode {
    pub authority_id: String,
    pub risk_tier: String,
    pub data_classes: Vec<String>,
    pub approval_required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryTierNode {
    pub tier: String,
    pub retention: String,
    pub write_requires_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryCollectionNode {
    pub collection_id: String,
    pub tier: String,
    pub policy_scope: String,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetrievedMemoryNode {
    pub memory_id: String,
    pub collection_id: String,
    pub reason: String,
    pub score: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryWriteCandidateNode {
    pub candidate_id: String,
    pub target_tier: String,
    pub summary_hash: String,
    pub requires_approval: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyScopeNode {
    pub scope_id: String,
    pub data_classes: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub readable_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyBudgetNode {
    pub budget_id: String,
    pub unit: String,
    pub limit: String,
    pub window: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandboxLimitNode {
    pub sandbox_id: String,
    pub network: String,
    pub filesystem: String,
    pub command_policy: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataBoundaryNode {
    pub boundary_id: String,
    pub data_class: String,
    pub residency: Option<String>,
    pub export_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalGateNode {
    pub gate_id: String,
    pub approver_role: String,
    pub decisions: Vec<String>,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactNode {
    pub artifact_id: String,
    pub artifact_type: String,
    pub display_name: String,
    pub path_ref: Option<String>,
    pub content_hash: Option<String>,
    pub produced_by_run: Option<String>,
}

trait DisplaySafePayload {
    fn display_safe_payload(&self) -> GraphPayload;
}

impl DisplaySafePayload for McpServerNode {
    fn display_safe_payload(&self) -> GraphPayload {
        payload([
            ("server_id", self.server_id.clone()),
            ("display_name", self.display_name.clone()),
            ("transport", self.transport.clone()),
            ("enabled", self.enabled.to_string()),
            ("tool_count", self.tool_count.to_string()),
        ])
    }
}

impl DisplaySafePayload for ToolDefinitionNode {
    fn display_safe_payload(&self) -> GraphPayload {
        let mut out = payload([
            ("tool_name", self.tool_name.clone()),
            ("authority_level", self.authority_level.clone()),
            ("read_only", self.read_only.to_string()),
            ("side_effects", self.side_effects.to_string()),
        ]);
        insert_optional(&mut out, "server_id", self.server_id.as_deref());
        insert_optional(&mut out, "schema_hash", self.schema_hash.as_deref());
        insert_optional(&mut out, "credential_ref", self.credential_ref.as_deref());
        out
    }
}

impl DisplaySafePayload for ToolCredentialNode {
    fn display_safe_payload(&self) -> GraphPayload {
        let mut out = payload([
            ("provider", self.provider.clone()),
            ("credential_ref", self.credential_ref.clone()),
            ("status", self.status.clone()),
            ("scopes", self.scopes.join(",")),
            (
                "secret_material_present",
                self.secret_material_present.to_string(),
            ),
        ]);
        if let Some(expires_at) = self.expires_at_unix_ms {
            out.insert("expires_at_unix_ms".to_string(), expires_at.to_string());
        }
        out
    }
}

impl DisplaySafePayload for ToolSchemaNode {
    fn display_safe_payload(&self) -> GraphPayload {
        let mut out = payload([
            ("schema_hash", self.schema_hash.clone()),
            ("input_summary", self.input_summary.clone()),
        ]);
        insert_optional(&mut out, "version", self.version.as_deref());
        insert_optional(&mut out, "output_summary", self.output_summary.as_deref());
        out
    }
}

impl DisplaySafePayload for ToolAuthorityNode {
    fn display_safe_payload(&self) -> GraphPayload {
        payload([
            ("authority_id", self.authority_id.clone()),
            ("risk_tier", self.risk_tier.clone()),
            ("data_classes", self.data_classes.join(",")),
            ("approval_required", self.approval_required.to_string()),
        ])
    }
}

impl DisplaySafePayload for MemoryTierNode {
    fn display_safe_payload(&self) -> GraphPayload {
        payload([
            ("tier", self.tier.clone()),
            ("retention", self.retention.clone()),
            (
                "write_requires_approval",
                self.write_requires_approval.to_string(),
            ),
        ])
    }
}

impl DisplaySafePayload for MemoryCollectionNode {
    fn display_safe_payload(&self) -> GraphPayload {
        payload([
            ("collection_id", self.collection_id.clone()),
            ("tier", self.tier.clone()),
            ("policy_scope", self.policy_scope.clone()),
            ("summary", self.summary.clone()),
        ])
    }
}

impl DisplaySafePayload for RetrievedMemoryNode {
    fn display_safe_payload(&self) -> GraphPayload {
        let mut out = payload([
            ("memory_id", self.memory_id.clone()),
            ("collection_id", self.collection_id.clone()),
            ("reason", self.reason.clone()),
        ]);
        insert_optional(&mut out, "score", self.score.as_deref());
        out
    }
}

impl DisplaySafePayload for MemoryWriteCandidateNode {
    fn display_safe_payload(&self) -> GraphPayload {
        payload([
            ("candidate_id", self.candidate_id.clone()),
            ("target_tier", self.target_tier.clone()),
            ("summary_hash", self.summary_hash.clone()),
            ("requires_approval", self.requires_approval.to_string()),
        ])
    }
}

impl DisplaySafePayload for PolicyScopeNode {
    fn display_safe_payload(&self) -> GraphPayload {
        payload([
            ("scope_id", self.scope_id.clone()),
            ("data_classes", self.data_classes.join(",")),
            ("allowed_tools", self.allowed_tools.join(",")),
            ("readable_paths", self.readable_paths.join(",")),
        ])
    }
}

impl DisplaySafePayload for PolicyBudgetNode {
    fn display_safe_payload(&self) -> GraphPayload {
        let mut out = payload([
            ("budget_id", self.budget_id.clone()),
            ("unit", self.unit.clone()),
            ("limit", self.limit.clone()),
        ]);
        insert_optional(&mut out, "window", self.window.as_deref());
        out
    }
}

impl DisplaySafePayload for SandboxLimitNode {
    fn display_safe_payload(&self) -> GraphPayload {
        payload([
            ("sandbox_id", self.sandbox_id.clone()),
            ("network", self.network.clone()),
            ("filesystem", self.filesystem.clone()),
            ("command_policy", self.command_policy.clone()),
        ])
    }
}

impl DisplaySafePayload for DataBoundaryNode {
    fn display_safe_payload(&self) -> GraphPayload {
        let mut out = payload([
            ("boundary_id", self.boundary_id.clone()),
            ("data_class", self.data_class.clone()),
            ("export_allowed", self.export_allowed.to_string()),
        ]);
        insert_optional(&mut out, "residency", self.residency.as_deref());
        out
    }
}

impl DisplaySafePayload for ApprovalGateNode {
    fn display_safe_payload(&self) -> GraphPayload {
        payload([
            ("gate_id", self.gate_id.clone()),
            ("approver_role", self.approver_role.clone()),
            ("decisions", self.decisions.join(",")),
            ("required", self.required.to_string()),
        ])
    }
}

impl DisplaySafePayload for ArtifactNode {
    fn display_safe_payload(&self) -> GraphPayload {
        let mut out = payload([
            ("artifact_id", self.artifact_id.clone()),
            ("artifact_type", self.artifact_type.clone()),
            ("display_name", self.display_name.clone()),
        ]);
        insert_optional(&mut out, "path_ref", self.path_ref.as_deref());
        insert_optional(&mut out, "content_hash", self.content_hash.as_deref());
        insert_optional(&mut out, "produced_by_run", self.produced_by_run.as_deref());
        out
    }
}

fn payload(items: impl IntoIterator<Item = (&'static str, String)>) -> GraphPayload {
    items
        .into_iter()
        .map(|(key, value)| (key.to_string(), value))
        .collect()
}

fn insert_optional(payload: &mut GraphPayload, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        payload.insert(key.to_string(), value.to_string());
    }
}
