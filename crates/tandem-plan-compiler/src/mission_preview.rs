// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tandem_orchestrator::{MissionSpec, WorkItem};
use tandem_workflows::{
    validate_mission_blueprint, MissionBlueprint, ReviewStageKind, ValidationMessage,
    ValidationSeverity,
};

use crate::mission_blueprint::{
    compile_barrier_dependencies, derive_mission_spec, derive_work_items, phase_rank_map,
    MISSION_EXECUTION_KIND_CODER_RUN, MISSION_EXECUTION_KIND_GOVERNANCE,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompiledNodePreview {
    pub node_id: String,
    pub title: String,
    pub agent_id: String,
    pub execution_kind: String,
    pub stage_kind: String,
    pub phase_id: Option<String>,
    pub lane: Option<String>,
    pub milestone: Option<String>,
    pub priority: Option<i32>,
    pub depends_on: Vec<String>,
    pub tool_allowlist: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub inherited_brief: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionBlueprintPreview {
    pub blueprint: MissionBlueprint,
    pub mission_spec: MissionSpec,
    pub work_items: Vec<WorkItem>,
    pub node_previews: Vec<CompiledNodePreview>,
    pub validation: Vec<ValidationMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MissionExecutionBoundarySummary {
    pub total_nodes: usize,
    pub coder_run_node_ids: Vec<String>,
    pub governance_node_ids: Vec<String>,
}

pub fn compile_mission_blueprint_preview(
    blueprint: MissionBlueprint,
) -> Result<MissionBlueprintPreview, Vec<ValidationMessage>> {
    let validation = validate_mission_blueprint(&blueprint);
    let has_errors = validation
        .iter()
        .any(|message| message.severity == ValidationSeverity::Error);
    if has_errors {
        return Err(validation);
    }

    let mission_spec = derive_mission_spec(&blueprint);
    let work_items = derive_work_items(&blueprint);
    let node_previews = derive_node_previews(&blueprint);

    Ok(MissionBlueprintPreview {
        blueprint,
        mission_spec,
        work_items,
        node_previews,
        validation,
    })
}

pub fn summarize_mission_execution_boundary(
    preview: &MissionBlueprintPreview,
) -> MissionExecutionBoundarySummary {
    let mut coder_run_node_ids = Vec::new();
    let mut governance_node_ids = Vec::new();

    for node in &preview.node_previews {
        if node.execution_kind == MISSION_EXECUTION_KIND_CODER_RUN {
            coder_run_node_ids.push(node.node_id.clone());
        } else {
            governance_node_ids.push(node.node_id.clone());
        }
    }

    MissionExecutionBoundarySummary {
        total_nodes: preview.node_previews.len(),
        coder_run_node_ids,
        governance_node_ids,
    }
}

fn derive_node_previews(blueprint: &MissionBlueprint) -> Vec<CompiledNodePreview> {
    let phase_rank = phase_rank_map(blueprint);
    let barrier_deps = compile_barrier_dependencies(blueprint, &phase_rank);
    let mut nodes = Vec::new();
    let orchestrator_agent_id = "mission_orchestrator".to_string();

    for workstream in &blueprint.workstreams {
        let agent_id = format!("agent_{}", workstream.workstream_id);
        let mut depends_on = workstream.depends_on.clone();
        if let Some(extra) = barrier_deps.get(&workstream.workstream_id) {
            for dep in extra {
                if !depends_on.contains(dep) {
                    depends_on.push(dep.clone());
                }
            }
        }
        nodes.push(CompiledNodePreview {
            node_id: workstream.workstream_id.clone(),
            title: workstream.title.clone(),
            agent_id,
            execution_kind: MISSION_EXECUTION_KIND_CODER_RUN.to_string(),
            stage_kind: "workstream".to_string(),
            phase_id: workstream.phase_id.clone(),
            lane: workstream.lane.clone(),
            milestone: workstream.milestone.clone(),
            priority: workstream.priority,
            depends_on,
            tool_allowlist: if workstream.tool_allowlist_override.is_empty() {
                vec!["*".to_string()]
            } else {
                workstream.tool_allowlist_override.clone()
            },
            mcp_servers: if workstream.mcp_servers_override.is_empty() {
                blueprint.team.allowed_mcp_servers.clone()
            } else {
                workstream.mcp_servers_override.clone()
            },
            inherited_brief: format!(
                "{}\n\nGoal: {}\n\nSuccess criteria:\n{}",
                blueprint.title,
                blueprint.goal,
                blueprint
                    .success_criteria
                    .iter()
                    .map(|criterion| format!("- {criterion}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        });
    }

    for stage in &blueprint.review_stages {
        let stage_kind = match stage.stage_kind {
            ReviewStageKind::Review => "review",
            ReviewStageKind::Test => "test",
            ReviewStageKind::Approval => "approval",
        }
        .to_string();
        let agent_id = if stage.stage_kind == ReviewStageKind::Approval {
            orchestrator_agent_id.clone()
        } else {
            format!("agent_{}", stage.stage_id)
        };
        let mut depends_on = stage.target_ids.clone();
        if let Some(extra) = barrier_deps.get(&stage.stage_id) {
            for dep in extra {
                if !depends_on.contains(dep) {
                    depends_on.push(dep.clone());
                }
            }
        }
        nodes.push(CompiledNodePreview {
            node_id: stage.stage_id.clone(),
            title: stage.title.clone(),
            agent_id,
            execution_kind: MISSION_EXECUTION_KIND_GOVERNANCE.to_string(),
            stage_kind,
            phase_id: stage.phase_id.clone(),
            lane: stage.lane.clone(),
            milestone: stage.milestone.clone(),
            priority: stage.priority,
            depends_on,
            tool_allowlist: if stage.tool_allowlist_override.is_empty() {
                vec!["*".to_string()]
            } else {
                stage.tool_allowlist_override.clone()
            },
            mcp_servers: if stage.mcp_servers_override.is_empty() {
                blueprint.team.allowed_mcp_servers.clone()
            } else {
                stage.mcp_servers_override.clone()
            },
            inherited_brief: format!(
                "{}\n\nGoal: {}\n\nSuccess criteria:\n{}",
                blueprint.title,
                blueprint.goal,
                blueprint
                    .success_criteria
                    .iter()
                    .map(|criterion| format!("- {criterion}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        });
    }

    nodes.sort_by(|a, b| node_sort_key(a, &phase_rank).cmp(&node_sort_key(b, &phase_rank)));
    nodes
}

fn node_sort_key(
    node: &CompiledNodePreview,
    phase_rank: &HashMap<String, usize>,
) -> (usize, i32, String) {
    let phase_order = node
        .phase_id
        .as_ref()
        .and_then(|phase_id| phase_rank.get(phase_id).copied())
        .unwrap_or(usize::MAX / 2);
    let priority = node.priority.unwrap_or(0);
    (phase_order, -priority, node.node_id.clone())
}
