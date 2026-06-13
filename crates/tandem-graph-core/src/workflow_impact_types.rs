use crate::{GraphScope, WorkflowBlocker};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowImpactQuery {
    pub changes: Vec<WorkflowImpactChange>,
    pub risk_hints: Vec<WorkflowImpactRiskHint>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkflowImpactChange {
    ToolSchemaChanged {
        tool_name: Option<String>,
    },
    McpServerChanged {
        server_id: Option<String>,
        tool_names: Vec<String>,
    },
    CredentialChanged {
        credential_ref: Option<String>,
        tool_name: Option<String>,
    },
    MemoryCollectionChanged {
        collection_id: Option<String>,
        tier: Option<String>,
        policy_scope: Option<String>,
    },
    PolicyScopeChanged {
        policy_scope: Option<String>,
    },
    ApprovalRuleChanged {
        approval_gate: Option<String>,
    },
    BudgetChanged {
        policy_scope: Option<String>,
    },
    WorkflowTemplateChanged {
        template_id: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowImpactRiskHint {
    pub target: String,
    pub authority_level: String,
    pub side_effect_boundary: String,
    pub checks_to_run: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowImpactReport {
    pub workflow_scope: GraphScope,
    pub affected_workflows: Vec<WorkflowImpactWorkflow>,
    pub affected_steps: Vec<WorkflowImpactStep>,
    pub risk_groups: Vec<WorkflowImpactRiskGroup>,
    pub checks_to_run: Vec<String>,
    pub blockers: Vec<WorkflowBlocker>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowImpactWorkflow {
    pub workflow_template_id: Option<String>,
    pub workflow_version_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowImpactStep {
    pub step_id: String,
    pub direct: bool,
    pub reasons: Vec<String>,
    pub required_tools: Vec<String>,
    pub memory_tiers: Vec<String>,
    pub policy_scopes: Vec<String>,
    pub approval_gates: Vec<String>,
    pub checks_to_run: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowImpactRiskGroup {
    pub authority_level: String,
    pub side_effect_boundary: String,
    pub affected_steps: Vec<String>,
    pub checks_to_run: Vec<String>,
}
