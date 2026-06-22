// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::plan_package::PartialFailureMode;

use crate::materialization::ProjectedAutomationContextMaterialization;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedAutomationStageKind {
    Workstream,
    Review,
    Test,
    Approval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedAutomationAgentProfile {
    pub agent_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_policy: Option<Value>,
    #[serde(default)]
    pub tool_allowlist: Vec<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedAutomationApprovalGate {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub decisions: Vec<String>,
    #[serde(default)]
    pub rework_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry_policy: Option<ProjectedAutomationGateExpiryPolicy>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProjectedAutomationGateExpiryAction {
    Cancel,
    Escalate,
    Remind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProjectedAutomationGateExpiryPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_after_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub on_expiry: Option<ProjectedAutomationGateExpiryAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub escalate_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remind_every_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedAutomationNode<I, O> {
    pub node_id: String,
    pub agent_id: String,
    pub objective: String,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub input_refs: Vec<I>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_contract: Option<O>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stage_kind: Option<ProjectedAutomationStageKind>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<ProjectedAutomationApprovalGate>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_failure_mode: Option<PartialFailureMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedAutomationExecutionPolicy {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel_agents: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_runtime_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_tool_calls: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedAutomationDraft<I, O> {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub output_targets: Vec<String>,
    #[serde(default)]
    pub agents: Vec<ProjectedAutomationAgentProfile>,
    #[serde(default)]
    pub nodes: Vec<ProjectedAutomationNode<I, O>>,
    pub execution: ProjectedAutomationExecutionPolicy,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ProjectedAutomationContextMaterialization>,
    pub metadata: Value,
}

// These types are the wire format between the plan compiler and the
// automation runtime; the tests below pin the serialized shape so a field
// rename or a dropped `skip_serializing_if` cannot silently break drafts
// persisted by older engines.
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn stage_kind_serializes_snake_case() {
        assert_eq!(
            serde_json::to_value(ProjectedAutomationStageKind::Workstream).expect("serialize"),
            json!("workstream")
        );
        assert_eq!(
            serde_json::to_value(ProjectedAutomationStageKind::Approval).expect("serialize"),
            json!("approval")
        );
        let parsed: ProjectedAutomationStageKind =
            serde_json::from_value(json!("review")).expect("deserialize");
        assert_eq!(parsed, ProjectedAutomationStageKind::Review);
    }

    #[test]
    fn minimal_node_deserializes_with_defaults() {
        let node: ProjectedAutomationNode<Value, Value> = serde_json::from_value(json!({
            "node_id": "compose",
            "agent_id": "agent_compose",
            "objective": "Compose the weekly email",
        }))
        .expect("minimal node deserializes");

        assert!(node.depends_on.is_empty());
        assert!(node.input_refs.is_empty());
        assert!(node.output_contract.is_none());
        assert!(node.gate.is_none());
        assert!(node.stage_kind.is_none());
        assert!(node.partial_failure_mode.is_none());
        assert!(node.metadata.is_none());
    }

    #[test]
    fn node_serialization_omits_unset_optional_fields() {
        let node = ProjectedAutomationNode::<Value, Value> {
            node_id: "compose".to_string(),
            agent_id: "agent_compose".to_string(),
            objective: "Compose the weekly email".to_string(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: None,
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            partial_failure_mode: None,
            metadata: None,
        };

        let serialized = serde_json::to_value(&node).expect("serialize");
        let keys = serialized
            .as_object()
            .expect("object")
            .keys()
            .cloned()
            .collect::<std::collections::BTreeSet<_>>();
        let expected = [
            "node_id",
            "agent_id",
            "objective",
            "depends_on",
            "input_refs",
        ]
        .into_iter()
        .map(str::to_string)
        .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(keys, expected);
    }

    #[test]
    fn gate_deserializes_with_defaults_and_round_trips() {
        let gate: ProjectedAutomationApprovalGate =
            serde_json::from_value(json!({ "required": true })).expect("gate deserializes");
        assert!(gate.required);
        assert!(gate.decisions.is_empty());
        assert!(gate.rework_targets.is_empty());
        assert!(gate.instructions.is_none());
        assert!(gate.expiry_policy.is_none());

        let full = ProjectedAutomationApprovalGate {
            required: true,
            decisions: vec!["approve".to_string(), "rework".to_string()],
            rework_targets: vec!["compose".to_string()],
            instructions: Some("Review before sending".to_string()),
            expiry_policy: Some(ProjectedAutomationGateExpiryPolicy {
                expires_after_ms: Some(60_000),
                on_expiry: Some(ProjectedAutomationGateExpiryAction::Escalate),
                escalate_to: Some("ops-lead".to_string()),
                remind_every_ms: Some(15_000),
            }),
        };
        let round_tripped: ProjectedAutomationApprovalGate =
            serde_json::from_value(serde_json::to_value(&full).expect("serialize"))
                .expect("deserialize");
        assert_eq!(round_tripped.decisions, full.decisions);
        assert_eq!(round_tripped.rework_targets, full.rework_targets);
        assert_eq!(round_tripped.instructions, full.instructions);
        assert_eq!(round_tripped.expiry_policy, full.expiry_policy);
    }

    #[test]
    fn minimal_draft_deserializes_with_empty_collections() {
        let draft: ProjectedAutomationDraft<Value, Value> = serde_json::from_value(json!({
            "name": "Weekly email",
            "execution": {},
            "metadata": {},
        }))
        .expect("minimal draft deserializes");

        assert!(draft.agents.is_empty());
        assert!(draft.nodes.is_empty());
        assert!(draft.output_targets.is_empty());
        assert!(draft.execution.max_parallel_agents.is_none());
        assert!(draft.execution.max_total_cost_usd.is_none());
        assert!(draft.context.is_none());
    }
}
