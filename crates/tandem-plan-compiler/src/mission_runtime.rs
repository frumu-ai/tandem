// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use tandem_workflows::{
    ApprovalDecision, MissionBlueprint, OutputContractBlueprint, ReviewStage, ReviewStageKind,
};

use crate::automation_projection::{
    ProjectedAutomationAgentProfile, ProjectedAutomationApprovalGate, ProjectedAutomationDraft,
    ProjectedAutomationExecutionPolicy, ProjectedAutomationNode, ProjectedAutomationStageKind,
};
use crate::contracts::{
    approval_gate_output_contract_seed, output_contract_seed, review_summary_output_contract_seed,
    OutputContractSeed,
};
use crate::materialization::{
    materialization_seed_from_projection, ProjectedAutomationMaterializationSeed,
};
use crate::mission_blueprint::{
    compile_barrier_dependencies, mission_workstream_enforcement_defaults,
    mission_workstream_node_metadata, phase_rank_map, MISSION_EXECUTION_KIND_GOVERNANCE,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CoderAutomationBranchContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    current_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    head_branch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    base_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CoderAutomationMetadata {
    surface: String,
    workflow_kind: String,
    preset_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    repo_binding: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    github_ref: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    branch_context: Option<CoderAutomationBranchContext>,
    launch_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectedMissionInputRef {
    pub from_step_id: String,
    pub alias: String,
}

pub fn compile_mission_runtime_projection(
    blueprint: &MissionBlueprint,
) -> ProjectedAutomationDraft<ProjectedMissionInputRef, OutputContractSeed> {
    let mut agents = Vec::new();
    let orchestrator_agent_id = "mission_orchestrator".to_string();
    let phase_rank = phase_rank_map(blueprint);
    let barrier_deps = compile_barrier_dependencies(blueprint, &phase_rank);

    agents.push(ProjectedAutomationAgentProfile {
        agent_id: orchestrator_agent_id.clone(),
        template_id: blueprint.orchestrator_template_id.clone(),
        display_name: "Mission Orchestrator".to_string(),
        model_policy: blueprint.team.default_model_policy.clone(),
        tool_allowlist: vec!["*".to_string()],
        allowed_mcp_servers: blueprint.team.allowed_mcp_servers.clone(),
    });

    let mut nodes = Vec::new();

    for workstream in &blueprint.workstreams {
        let agent_id = format!("agent_{}", workstream.workstream_id);
        agents.push(ProjectedAutomationAgentProfile {
            agent_id: agent_id.clone(),
            template_id: workstream.template_id.clone(),
            display_name: workstream.title.clone(),
            model_policy: merge_model_policy(
                blueprint.team.default_model_policy.as_ref(),
                workstream.model_override.as_ref(),
            ),
            tool_allowlist: if workstream.tool_allowlist_override.is_empty() {
                vec!["*".to_string()]
            } else {
                workstream.tool_allowlist_override.clone()
            },
            allowed_mcp_servers: if workstream.mcp_servers_override.is_empty() {
                blueprint.team.allowed_mcp_servers.clone()
            } else {
                workstream.mcp_servers_override.clone()
            },
        });
        let mut input_refs = workstream
            .input_refs
            .iter()
            .map(|input| ProjectedMissionInputRef {
                from_step_id: input.from_step_id.clone(),
                alias: input.alias.clone(),
            })
            .collect::<Vec<_>>();
        for dep in &workstream.depends_on {
            if !input_refs.iter().any(|input| input.from_step_id == *dep) {
                input_refs.push(ProjectedMissionInputRef {
                    from_step_id: dep.clone(),
                    alias: dep.clone(),
                });
            }
        }
        let mut depends_on = workstream.depends_on.clone();
        if let Some(extra) = barrier_deps.get(&workstream.workstream_id) {
            for dep in extra {
                if !depends_on.contains(dep) {
                    depends_on.push(dep.clone());
                }
            }
        }
        let mut output_contract = projected_output_contract(&workstream.output_contract);
        if output_contract.enforcement.is_none() {
            output_contract.enforcement = mission_workstream_enforcement_defaults(workstream);
        }
        nodes.push(ProjectedAutomationNode {
            node_id: workstream.workstream_id.clone(),
            agent_id,
            objective: workstream.objective.clone(),
            depends_on,
            input_refs,
            output_contract: Some(output_contract),
            retry_policy: workstream.retry_policy.clone(),
            timeout_ms: workstream.timeout_ms,
            stage_kind: Some(ProjectedAutomationStageKind::Workstream),
            gate: None,
            metadata: mission_workstream_node_metadata(workstream),
        });
    }

    for stage in &blueprint.review_stages {
        let stage_kind = review_stage_kind_key(stage.stage_kind.clone());
        let agent_id = if stage.stage_kind == ReviewStageKind::Approval {
            orchestrator_agent_id.clone()
        } else {
            let stage_agent_id = format!("agent_{}", stage.stage_id);
            agents.push(ProjectedAutomationAgentProfile {
                agent_id: stage_agent_id.clone(),
                template_id: stage.template_id.clone(),
                display_name: stage.title.clone(),
                model_policy: merge_model_policy(
                    blueprint.team.default_model_policy.as_ref(),
                    stage.model_override.as_ref(),
                ),
                tool_allowlist: if stage.tool_allowlist_override.is_empty() {
                    vec!["*".to_string()]
                } else {
                    stage.tool_allowlist_override.clone()
                },
                allowed_mcp_servers: if stage.mcp_servers_override.is_empty() {
                    blueprint.team.allowed_mcp_servers.clone()
                } else {
                    stage.mcp_servers_override.clone()
                },
            });
            stage_agent_id
        };
        let stage_tool_allowlist = if stage.tool_allowlist_override.is_empty() {
            vec!["*".to_string()]
        } else {
            stage.tool_allowlist_override.clone()
        };
        let stage_mcp_servers = if stage.mcp_servers_override.is_empty() {
            blueprint.team.allowed_mcp_servers.clone()
        } else {
            stage.mcp_servers_override.clone()
        };
        let mut depends_on = stage.target_ids.clone();
        if let Some(extra) = barrier_deps.get(&stage.stage_id) {
            for dep in extra {
                if !depends_on.contains(dep) {
                    depends_on.push(dep.clone());
                }
            }
        }
        nodes.push(ProjectedAutomationNode {
            node_id: stage.stage_id.clone(),
            agent_id,
            objective: if stage.prompt.trim().is_empty() {
                stage.title.clone()
            } else {
                stage.prompt.clone()
            },
            depends_on,
            input_refs: stage
                .target_ids
                .iter()
                .map(|target_id| ProjectedMissionInputRef {
                    from_step_id: target_id.clone(),
                    alias: target_id.clone(),
                })
                .collect(),
            output_contract: Some(if stage.stage_kind == ReviewStageKind::Approval {
                approval_gate_output_contract_seed()
            } else {
                review_summary_output_contract_seed()
            }),
            retry_policy: Some(json!({ "max_attempts": 1 })),
            timeout_ms: None,
            stage_kind: Some(stage_kind),
            gate: stage.gate.as_ref().map(projected_gate),
            metadata: Some(review_stage_metadata(
                stage,
                &stage_tool_allowlist,
                &stage_mcp_servers,
            )),
        });
    }

    nodes.sort_by(|a, b| node_sort_key(a, &phase_rank).cmp(&node_sort_key(b, &phase_rank)));

    // Default-on approval gates: walk the projected nodes and wrap any
    // workstream whose bound agent's tool allowlist includes a
    // `RequiresApproval` action (CRM writes, outbound emails, payments, etc.).
    // Approval/Review stages already carry explicit gates from the blueprint
    // and are skipped.
    inject_default_approval_gates(&mut nodes, &agents);

    let typed_coder_metadata = extract_coder_metadata(blueprint);
    let mut metadata = serde_json::Map::from_iter([
        ("builder_kind".to_string(), json!("mission_blueprint")),
        ("mission_blueprint".to_string(), json!(blueprint.clone())),
        (
            "mission".to_string(),
            json!({
                "mission_id": blueprint.mission_id,
                "title": blueprint.title,
                "goal": blueprint.goal,
                "success_criteria": blueprint.success_criteria,
                "shared_context": blueprint.shared_context,
                "orchestrator_template_id": blueprint.orchestrator_template_id,
                "phases": blueprint.phases,
                "milestones": blueprint.milestones,
                "team": blueprint.team,
            }),
        ),
    ]);
    if let Some(extra_metadata) = blueprint.metadata.as_ref().and_then(Value::as_object) {
        for (key, value) in extra_metadata {
            metadata.insert(key.clone(), value.clone());
        }
    }
    if let Some(coder) = typed_coder_metadata {
        metadata.insert(
            "coder".to_string(),
            serde_json::to_value(coder).unwrap_or_else(|_| json!({})),
        );
    }

    ProjectedAutomationDraft {
        name: blueprint.title.clone(),
        description: Some(blueprint.goal.clone()),
        workspace_root: Some(blueprint.workspace_root.clone()),
        output_targets: Vec::new(),
        agents,
        nodes,
        execution: ProjectedAutomationExecutionPolicy {
            max_parallel_agents: blueprint.team.max_parallel_agents.or(Some(4)),
            max_total_runtime_ms: blueprint
                .team
                .mission_budget
                .as_ref()
                .and_then(|value| value.get("max_duration_ms"))
                .and_then(Value::as_u64),
            max_total_tool_calls: blueprint
                .team
                .mission_budget
                .as_ref()
                .and_then(|value| value.get("max_tool_calls"))
                .and_then(Value::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
            max_total_tokens: blueprint
                .team
                .mission_budget
                .as_ref()
                .and_then(|value| value.get("max_tokens"))
                .and_then(Value::as_u64),
            max_total_cost_usd: blueprint
                .team
                .mission_budget
                .as_ref()
                .and_then(|value| value.get("max_cost_usd"))
                .and_then(Value::as_f64),
        },
        context: None,
        metadata: Value::Object(metadata),
    }
}

pub fn project_mission_runtime_materialization_seed(
    blueprint: &MissionBlueprint,
) -> ProjectedAutomationMaterializationSeed<ProjectedMissionInputRef, OutputContractSeed> {
    materialization_seed_from_projection(compile_mission_runtime_projection(blueprint))
}

fn extract_coder_metadata(blueprint: &MissionBlueprint) -> Option<CoderAutomationMetadata> {
    let coder = blueprint
        .metadata
        .as_ref()
        .and_then(Value::as_object)
        .and_then(|metadata| metadata.get("coder"))
        .cloned()?;
    serde_json::from_value(coder).ok()
}

fn projected_output_contract(contract: &OutputContractBlueprint) -> OutputContractSeed {
    output_contract_seed(
        contract.kind.clone(),
        contract.schema.clone(),
        contract.summary_guidance.clone(),
    )
}

fn projected_gate(gate: &tandem_workflows::HumanApprovalGate) -> ProjectedAutomationApprovalGate {
    ProjectedAutomationApprovalGate {
        required: gate.required,
        decisions: gate
            .decisions
            .iter()
            .map(|decision| match decision {
                ApprovalDecision::Approve => "approve".to_string(),
                ApprovalDecision::Rework => "rework".to_string(),
                ApprovalDecision::Cancel => "cancel".to_string(),
            })
            .collect(),
        rework_targets: gate.rework_targets.clone(),
        instructions: gate.instructions.clone(),
    }
}

/// Walk the projected nodes and wrap workstream nodes whose tool allowlist
/// includes a `RequiresApproval` action with a default
/// `ProjectedAutomationApprovalGate`. Idempotent: nodes with an explicit
/// `gate` are left untouched. Approval/Review stages already carry their
/// blueprint gates and are skipped.
///
/// This is the default-on policy that makes the agent-owned-workflows pitch
/// real: even if the planner agent forgets to add a gate, the compiler adds
/// one for any node that touches an external mutation. Operators can override
/// per-step at scope-review time (`SkipApproval` toggle in `ScopeInspector`)
/// for ordinary workflows. Fintech strict nodes intentionally ignore that UI
/// hint because protected-action approval must be enforced by runtime policy,
/// not by planner metadata or artifact text.
pub(crate) fn inject_default_approval_gates(
    nodes: &mut [ProjectedAutomationNode<ProjectedMissionInputRef, OutputContractSeed>],
    agents: &[ProjectedAutomationAgentProfile],
) {
    let agent_lookup: HashMap<&str, &ProjectedAutomationAgentProfile> =
        agents.iter().map(|a| (a.agent_id.as_str(), a)).collect();

    for node in nodes.iter_mut() {
        if node.gate.is_some() {
            continue;
        }
        // Approval/Review stage gates are blueprint-owned; skip.
        if matches!(
            node.stage_kind,
            Some(ProjectedAutomationStageKind::Approval)
                | Some(ProjectedAutomationStageKind::Review)
        ) {
            continue;
        }
        if node_skip_approval_override(node) {
            continue;
        }

        let allowlist = node_tool_allowlist(node, &agent_lookup);
        if tandem_tools::approval_classifier::allowlist_is_wildcard(&allowlist) {
            // Wildcard allowlists default to a gate: we cannot reason about
            // which tool the agent will pick at runtime.
            node.gate = Some(default_injected_gate(&format!(
                "Wildcard tool allowlist: agent may invoke any registered tool. \
                 Approve before this step runs."
            )));
            continue;
        }
        match tandem_tools::approval_classifier::classify_node_allowlist(&allowlist) {
            tandem_tools::approval_classifier::ApprovalClassification::RequiresApproval => {
                node.gate = Some(default_injected_gate(&format!(
                    "Step `{}` will invoke a tool that mutates an external system. \
                     Review the proposed action before it runs.",
                    node.objective.lines().next().unwrap_or(&node.node_id)
                )));
            }
            tandem_tools::approval_classifier::ApprovalClassification::UserConfigurable => {
                // Doubt-case: the allowlist mixes safe tools with unknown
                // ones. Fail closed but mark the gate as configurable so
                // scope-review can offer a "skip approval" toggle.
                let mut gate = default_injected_gate(&format!(
                    "Step `{}` may invoke an unrecognized tool. Approve once, \
                     or open scope review to mark this step auto-approved.",
                    node.objective.lines().next().unwrap_or(&node.node_id)
                ));
                gate.required = true;
                node.gate = Some(gate);
            }
            tandem_tools::approval_classifier::ApprovalClassification::NoApproval => {
                // No external mutation; do not add a gate.
            }
        }
    }
}

fn default_injected_gate(instructions: &str) -> ProjectedAutomationApprovalGate {
    ProjectedAutomationApprovalGate {
        required: true,
        decisions: vec![
            "approve".to_string(),
            "rework".to_string(),
            "cancel".to_string(),
        ],
        rework_targets: Vec::new(),
        instructions: Some(instructions.to_string()),
    }
}

/// Resolve the effective tool allowlist for a node by consulting:
///   1. The node's `metadata.builder.tool_allowlist_override` if present
///      (review-stage nodes carry this).
///   2. The bound agent's `tool_allowlist` (workstream nodes get their
///      allowlist from the agent the compiler created for them).
fn node_tool_allowlist<I, O>(
    node: &ProjectedAutomationNode<I, O>,
    agents: &HashMap<&str, &ProjectedAutomationAgentProfile>,
) -> Vec<String> {
    if let Some(metadata) = node.metadata.as_ref() {
        if let Some(values) = metadata
            .pointer("/builder/tool_allowlist_override")
            .and_then(Value::as_array)
        {
            let allowlist: Vec<String> = values
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            if !allowlist.is_empty() {
                return allowlist;
            }
        }
    }
    if let Some(agent) = agents.get(node.agent_id.as_str()) {
        return agent.tool_allowlist.clone();
    }
    Vec::new()
}

/// Operators can pin an ordinary step as auto-approved at scope-review time.
/// The override lives at `metadata.approval.skip_approval = true` (a
/// UI-emitted hint). Fintech strict nodes never honor this hint.
fn node_skip_approval_override<I, O>(node: &ProjectedAutomationNode<I, O>) -> bool {
    if tandem_core::metadata_enables_fintech_strict(node.metadata.as_ref()) {
        return false;
    }
    node.metadata
        .as_ref()
        .and_then(|m| m.pointer("/approval/skip_approval"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn review_stage_metadata(
    stage: &ReviewStage,
    tool_allowlist: &[String],
    mcp_servers: &[String],
) -> Value {
    json!({
        "execution_kind": MISSION_EXECUTION_KIND_GOVERNANCE,
        "builder": {
            "title": stage.title,
            "checklist": stage.checklist,
            "role": stage.role,
            "tool_allowlist_override": tool_allowlist,
            "mcp_servers_override": mcp_servers,
            "priority": stage.priority,
            "phase_id": stage.phase_id,
            "lane": stage.lane,
            "milestone": stage.milestone,
        }
    })
}

fn review_stage_kind_key(kind: ReviewStageKind) -> ProjectedAutomationStageKind {
    match kind {
        ReviewStageKind::Review => ProjectedAutomationStageKind::Review,
        ReviewStageKind::Test => ProjectedAutomationStageKind::Test,
        ReviewStageKind::Approval => ProjectedAutomationStageKind::Approval,
    }
}

fn node_builder_metadata(
    node: &ProjectedAutomationNode<ProjectedMissionInputRef, OutputContractSeed>,
    key: &str,
) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn node_builder_priority(
    node: &ProjectedAutomationNode<ProjectedMissionInputRef, OutputContractSeed>,
) -> Option<i32> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get("priority"))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn node_sort_key(
    node: &ProjectedAutomationNode<ProjectedMissionInputRef, OutputContractSeed>,
    phase_rank: &HashMap<String, usize>,
) -> (usize, i32, String) {
    let phase = node_builder_metadata(node, "phase_id");
    let priority = node_builder_priority(node).unwrap_or(0);
    let phase_order = phase
        .as_ref()
        .and_then(|phase_id| phase_rank.get(phase_id))
        .copied()
        .unwrap_or(usize::MAX / 2);
    (phase_order, -priority, node.node_id.clone())
}

fn merge_model_policy(
    default_policy: Option<&Value>,
    override_policy: Option<&Value>,
) -> Option<Value> {
    match (default_policy, override_policy) {
        (Some(default_policy), Some(override_policy)) => {
            let mut merged = default_policy.as_object().cloned().unwrap_or_default();
            if let Some(override_map) = override_policy.as_object() {
                for (key, value) in override_map {
                    merged.insert(key.clone(), value.clone());
                }
            }
            Some(Value::Object(merged))
        }
        (Some(default_policy), None) => Some(default_policy.clone()),
        (None, Some(override_policy)) => Some(override_policy.clone()),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tandem_workflows::{MissionTeamBlueprint, OutputContractBlueprint, WorkstreamBlueprint};

    #[test]
    fn compile_mission_runtime_projection_builds_agents_and_nodes() {
        let blueprint = MissionBlueprint {
            mission_id: "mission-1".to_string(),
            title: "Mission".to_string(),
            goal: "Do a thing".to_string(),
            success_criteria: vec!["done".to_string()],
            shared_context: None,
            workspace_root: "/tmp/project".to_string(),
            orchestrator_template_id: None,
            phases: Vec::new(),
            milestones: Vec::new(),
            team: MissionTeamBlueprint {
                allowed_template_ids: Vec::new(),
                default_model_policy: Some(json!({"provider_id":"test","model_id":"model"})),
                allowed_mcp_servers: vec!["github".to_string()],
                max_parallel_agents: Some(3),
                mission_budget: None,
                orchestrator_only_tool_calls: false,
            },
            workstreams: vec![WorkstreamBlueprint {
                workstream_id: "research".to_string(),
                title: "Research".to_string(),
                objective: "Investigate".to_string(),
                role: "researcher".to_string(),
                priority: None,
                phase_id: None,
                lane: None,
                milestone: None,
                template_id: None,
                prompt: "Research it".to_string(),
                model_override: None,
                tool_allowlist_override: Vec::new(),
                mcp_servers_override: Vec::new(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: OutputContractBlueprint {
                    kind: "brief".to_string(),
                    schema: None,
                    summary_guidance: None,
                },
                retry_policy: None,
                timeout_ms: None,
                metadata: None,
            }],
            review_stages: Vec::new(),
            metadata: None,
        };

        let projection = compile_mission_runtime_projection(&blueprint);
        assert_eq!(projection.agents.len(), 2);
        assert_eq!(projection.nodes.len(), 1);
        assert_eq!(
            projection.nodes[0].stage_kind,
            Some(ProjectedAutomationStageKind::Workstream)
        );
        assert_eq!(
            projection.nodes[0]
                .output_contract
                .as_ref()
                .and_then(|contract| contract.validator_kind),
            Some(crate::contracts::ProjectedOutputValidatorKind::ResearchBrief)
        );
        assert_eq!(
            projection.nodes[0]
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("builder"))
                .and_then(|builder| builder.get("execution_kind"))
                .and_then(Value::as_str),
            Some("coder_run")
        );
    }

    fn workstream_with_tools(id: &str, tools: Vec<String>) -> WorkstreamBlueprint {
        WorkstreamBlueprint {
            workstream_id: id.to_string(),
            title: id.to_string(),
            objective: format!("objective for {id}"),
            role: "worker".to_string(),
            priority: None,
            phase_id: None,
            lane: None,
            milestone: None,
            template_id: None,
            prompt: format!("Do {id}"),
            model_override: None,
            tool_allowlist_override: tools,
            mcp_servers_override: Vec::new(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: OutputContractBlueprint {
                kind: "brief".to_string(),
                schema: None,
                summary_guidance: None,
            },
            retry_policy: None,
            timeout_ms: None,
            metadata: None,
        }
    }

    fn blueprint_with_workstreams(workstreams: Vec<WorkstreamBlueprint>) -> MissionBlueprint {
        MissionBlueprint {
            mission_id: "mission-gates".to_string(),
            title: "Gate test mission".to_string(),
            goal: "Validate compiler-injected gates".to_string(),
            success_criteria: vec!["passes".to_string()],
            shared_context: None,
            workspace_root: "/tmp/project".to_string(),
            orchestrator_template_id: None,
            phases: Vec::new(),
            milestones: Vec::new(),
            team: MissionTeamBlueprint {
                allowed_template_ids: Vec::new(),
                default_model_policy: Some(json!({"provider_id":"test","model_id":"model"})),
                allowed_mcp_servers: vec!["github".to_string()],
                max_parallel_agents: Some(3),
                mission_budget: None,
                orchestrator_only_tool_calls: false,
            },
            workstreams,
            review_stages: Vec::new(),
            metadata: None,
        }
    }

    fn worker_node<'a>(
        projection: &'a ProjectedAutomationDraft<ProjectedMissionInputRef, OutputContractSeed>,
        node_id: &str,
    ) -> &'a ProjectedAutomationNode<ProjectedMissionInputRef, OutputContractSeed> {
        projection
            .nodes
            .iter()
            .find(|n| n.node_id == node_id)
            .unwrap_or_else(|| panic!("node `{node_id}` missing"))
    }

    #[test]
    fn injects_gate_when_workstream_uses_crm_write_tool() {
        let blueprint = blueprint_with_workstreams(vec![workstream_with_tools(
            "outreach",
            vec!["mcp.hubspot.create_contact".to_string()],
        )]);
        let projection = compile_mission_runtime_projection(&blueprint);
        let node = worker_node(&projection, "outreach");
        let gate = node.gate.as_ref().expect("gate auto-injected");
        assert!(gate.required);
        assert_eq!(
            gate.decisions,
            vec![
                "approve".to_string(),
                "rework".to_string(),
                "cancel".to_string()
            ]
        );
        assert!(gate.instructions.is_some());
    }

    #[test]
    fn injects_gate_when_workstream_uses_outbound_email() {
        let blueprint = blueprint_with_workstreams(vec![workstream_with_tools(
            "send",
            vec!["send_email".to_string()],
        )]);
        let projection = compile_mission_runtime_projection(&blueprint);
        let gate = worker_node(&projection, "send")
            .gate
            .as_ref()
            .expect("gate auto-injected for outbound email");
        assert!(gate.required);
    }

    #[test]
    fn does_not_inject_gate_for_pure_read_workstream() {
        let blueprint = blueprint_with_workstreams(vec![workstream_with_tools(
            "research",
            vec![
                "read".to_string(),
                "websearch".to_string(),
                "mcp.github.list_issues".to_string(),
            ],
        )]);
        let projection = compile_mission_runtime_projection(&blueprint);
        assert!(
            worker_node(&projection, "research").gate.is_none(),
            "pure read-only workstream should not gate"
        );
    }

    #[test]
    fn injects_gate_for_wildcard_allowlist() {
        // Empty tool_allowlist_override → compiler defaults the agent to `*`,
        // which is the "I can call anything" case. Compiler must gate.
        let blueprint =
            blueprint_with_workstreams(vec![workstream_with_tools("freeform", Vec::new())]);
        let projection = compile_mission_runtime_projection(&blueprint);
        let gate = worker_node(&projection, "freeform")
            .gate
            .as_ref()
            .expect("wildcard allowlist must gate");
        assert!(gate
            .instructions
            .as_deref()
            .unwrap_or_default()
            .to_lowercase()
            .contains("wildcard"));
    }

    #[test]
    fn injects_gate_for_unknown_tool_failing_closed() {
        let blueprint = blueprint_with_workstreams(vec![workstream_with_tools(
            "mystery",
            vec![
                "read".to_string(),
                "mcp.unknown_vendor.do_something".to_string(),
            ],
        )]);
        let projection = compile_mission_runtime_projection(&blueprint);
        let gate = worker_node(&projection, "mystery")
            .gate
            .as_ref()
            .expect("unknown tool should fail closed");
        assert!(gate.required);
    }

    #[test]
    fn respects_explicit_skip_approval_metadata_override() {
        // Build the projection normally (which would inject a gate),
        // then run inject_default_approval_gates on a manually-prepared
        // node carrying the skip-approval override to verify it is honored.
        let mut projection = compile_mission_runtime_projection(&blueprint_with_workstreams(vec![
            workstream_with_tools("outreach", vec!["mcp.hubspot.create_contact".to_string()]),
        ]));
        // Strip the auto-injected gate, set the override, re-run injection.
        projection.nodes[0].gate = None;
        let mut metadata = projection.nodes[0]
            .metadata
            .clone()
            .unwrap_or_else(|| json!({}));
        metadata["approval"] = json!({ "skip_approval": true });
        projection.nodes[0].metadata = Some(metadata);

        inject_default_approval_gates(&mut projection.nodes, &projection.agents);
        assert!(
            projection.nodes[0].gate.is_none(),
            "skip_approval override must prevent re-injection"
        );
    }

    #[test]
    fn fintech_strict_ignores_skip_approval_metadata_override() {
        let mut projection = compile_mission_runtime_projection(&blueprint_with_workstreams(vec![
            workstream_with_tools("release_funds", vec!["mcp.bank.release_funds".to_string()]),
        ]));
        projection.nodes[0].gate = None;
        let mut metadata = projection.nodes[0]
            .metadata
            .clone()
            .unwrap_or_else(|| json!({}));
        metadata["runtime_profile"] = json!("fintech_strict");
        metadata["approval"] = json!({ "skip_approval": true });
        projection.nodes[0].metadata = Some(metadata);

        inject_default_approval_gates(&mut projection.nodes, &projection.agents);
        assert!(
            projection.nodes[0].gate.is_some(),
            "fintech_strict must not allow metadata to suppress approval injection"
        );
    }

    #[test]
    fn does_not_overwrite_explicit_blueprint_gate() {
        let mut projection = compile_mission_runtime_projection(&blueprint_with_workstreams(vec![
            workstream_with_tools("outreach", vec!["mcp.hubspot.create_contact".to_string()]),
        ]));
        // Replace the auto-injected gate with a custom one, then re-run
        // injection. The custom gate must survive untouched.
        projection.nodes[0].gate = Some(ProjectedAutomationApprovalGate {
            required: true,
            decisions: vec!["approve".to_string()],
            rework_targets: vec!["draft".to_string()],
            instructions: Some("custom per-blueprint instructions".to_string()),
        });
        inject_default_approval_gates(&mut projection.nodes, &projection.agents);
        let gate = projection.nodes[0].gate.as_ref().expect("gate retained");
        assert_eq!(
            gate.instructions.as_deref(),
            Some("custom per-blueprint instructions")
        );
        assert_eq!(gate.decisions, vec!["approve".to_string()]);
        assert_eq!(gate.rework_targets, vec!["draft".to_string()]);
    }
}
