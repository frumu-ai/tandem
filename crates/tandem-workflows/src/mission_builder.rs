use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ReviewStageKind {
    Review,
    Test,
    Approval,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approve,
    Rework,
    Cancel,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ValidationSeverity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ValidationMessage {
    pub severity: ValidationSeverity,
    pub code: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct MissionTeamBlueprint {
    #[serde(default)]
    pub allowed_template_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_model_policy: Option<Value>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_parallel_agents: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_budget: Option<Value>,
    #[serde(default)]
    pub orchestrator_only_tool_calls: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutputContractBlueprint {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_guidance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputRefBlueprint {
    pub from_step_id: String,
    pub alias: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionPhaseExecutionMode {
    Soft,
    Barrier,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionPhaseBlueprint {
    pub phase_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub execution_mode: Option<MissionPhaseExecutionMode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionMilestoneBlueprint {
    pub milestone_id: String,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    #[serde(default)]
    pub required_stage_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkstreamBlueprint {
    pub workstream_id: String,
    pub title: String,
    pub objective: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_override: Option<Value>,
    #[serde(default)]
    pub tool_allowlist_override: Vec<String>,
    #[serde(default)]
    pub mcp_servers_override: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub input_refs: Vec<InputRefBlueprint>,
    pub output_contract: OutputContractBlueprint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retry_policy: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HumanApprovalGate {
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub decisions: Vec<ApprovalDecision>,
    #[serde(default)]
    pub rework_targets: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ReviewStage {
    pub stage_id: String,
    pub stage_kind: ReviewStageKind,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub milestone: Option<String>,
    #[serde(default)]
    pub target_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    pub prompt: String,
    #[serde(default)]
    pub checklist: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_override: Option<Value>,
    #[serde(default)]
    pub tool_allowlist_override: Vec<String>,
    #[serde(default)]
    pub mcp_servers_override: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<HumanApprovalGate>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MissionBlueprint {
    pub mission_id: String,
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shared_context: Option<String>,
    pub workspace_root: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub orchestrator_template_id: Option<String>,
    #[serde(default)]
    pub phases: Vec<MissionPhaseBlueprint>,
    #[serde(default)]
    pub milestones: Vec<MissionMilestoneBlueprint>,
    #[serde(default)]
    pub team: MissionTeamBlueprint,
    #[serde(default)]
    pub workstreams: Vec<WorkstreamBlueprint>,
    #[serde(default)]
    pub review_stages: Vec<ReviewStage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
}

pub fn validate_mission_blueprint(blueprint: &MissionBlueprint) -> Vec<ValidationMessage> {
    let mut messages = Vec::new();
    if blueprint.title.trim().is_empty() {
        messages.push(error(
            "MISSION_TITLE_REQUIRED",
            "mission title is required",
            None,
        ));
    }
    if blueprint.goal.trim().is_empty() {
        messages.push(error(
            "MISSION_GOAL_REQUIRED",
            "mission goal is required",
            None,
        ));
    }
    if blueprint.workspace_root.trim().is_empty() {
        messages.push(error(
            "MISSION_WORKSPACE_REQUIRED",
            "mission workspace_root is required",
            None,
        ));
    }
    if blueprint.workstreams.is_empty() {
        messages.push(error(
            "MISSION_WORKSTREAMS_REQUIRED",
            "mission must include at least one workstream",
            None,
        ));
    }

    let mut phase_ids = HashSet::new();
    for phase in &blueprint.phases {
        let id = phase.phase_id.trim();
        if id.is_empty() {
            messages.push(error(
                "MISSION_PHASE_ID_REQUIRED",
                "mission phase_id is required",
                None,
            ));
            continue;
        }
        if !phase_ids.insert(id.to_string()) {
            messages.push(error(
                "MISSION_PHASE_DUPLICATE",
                "duplicate mission phase_id",
                Some(id.to_string()),
            ));
        }
        if phase.title.trim().is_empty() {
            messages.push(error(
                "MISSION_PHASE_TITLE_REQUIRED",
                "mission phase title is required",
                Some(id.to_string()),
            ));
        }
    }

    let mut milestone_ids = HashSet::new();
    for milestone in &blueprint.milestones {
        let id = milestone.milestone_id.trim();
        if id.is_empty() {
            messages.push(error(
                "MISSION_MILESTONE_ID_REQUIRED",
                "mission milestone_id is required",
                None,
            ));
            continue;
        }
        if !milestone_ids.insert(id.to_string()) {
            messages.push(error(
                "MISSION_MILESTONE_DUPLICATE",
                "duplicate mission milestone_id",
                Some(id.to_string()),
            ));
        }
        if milestone.title.trim().is_empty() {
            messages.push(error(
                "MISSION_MILESTONE_TITLE_REQUIRED",
                "mission milestone title is required",
                Some(id.to_string()),
            ));
        }
        if let Some(phase_id) = milestone.phase_id.as_deref() {
            if !phase_id.trim().is_empty() && !phase_ids.contains(phase_id.trim()) {
                messages.push(error(
                    "MISSION_MILESTONE_PHASE_UNKNOWN",
                    "mission milestone references unknown phase_id",
                    Some(id.to_string()),
                ));
            }
        }
    }

    let mut stage_ids = HashSet::new();
    let mut workstream_ids = HashSet::new();
    for workstream in &blueprint.workstreams {
        let id = workstream.workstream_id.trim();
        if id.is_empty() {
            messages.push(error(
                "WORKSTREAM_ID_REQUIRED",
                "workstream_id is required",
                None,
            ));
            continue;
        }
        if !stage_ids.insert(id.to_string()) {
            messages.push(error(
                "DUPLICATE_STAGE_ID",
                "duplicate stage/workstream id",
                Some(id.to_string()),
            ));
        }
        workstream_ids.insert(id.to_string());
        if workstream.title.trim().is_empty() {
            messages.push(error(
                "WORKSTREAM_TITLE_REQUIRED",
                "workstream title is required",
                Some(id.to_string()),
            ));
        }
        if workstream.objective.trim().is_empty() {
            messages.push(error(
                "WORKSTREAM_OBJECTIVE_REQUIRED",
                "workstream objective is required",
                Some(id.to_string()),
            ));
        }
        if workstream.role.trim().is_empty() {
            messages.push(error(
                "WORKSTREAM_ROLE_REQUIRED",
                "workstream role is required",
                Some(id.to_string()),
            ));
        }
        if workstream.prompt.trim().is_empty() {
            messages.push(error(
                "WORKSTREAM_PROMPT_REQUIRED",
                "workstream prompt is required",
                Some(id.to_string()),
            ));
        }
        if workstream.output_contract.kind.trim().is_empty() {
            messages.push(error(
                "WORKSTREAM_OUTPUT_REQUIRED",
                "workstream output_contract.kind is required",
                Some(id.to_string()),
            ));
        }
        if let Some(phase_id) = workstream.phase_id.as_deref() {
            if !phase_id.trim().is_empty() && !phase_ids.contains(phase_id.trim()) {
                messages.push(error(
                    "WORKSTREAM_PHASE_UNKNOWN",
                    "workstream phase_id references unknown mission phase",
                    Some(id.to_string()),
                ));
            }
        }
        if let Some(milestone) = workstream.milestone.as_deref() {
            if !milestone.trim().is_empty() && !milestone_ids.contains(milestone.trim()) {
                messages.push(error(
                    "WORKSTREAM_MILESTONE_UNKNOWN",
                    "workstream milestone references unknown mission milestone",
                    Some(id.to_string()),
                ));
            }
        }
    }

    for stage in &blueprint.review_stages {
        let id = stage.stage_id.trim();
        if id.is_empty() {
            messages.push(error(
                "REVIEW_STAGE_ID_REQUIRED",
                "stage_id is required",
                None,
            ));
            continue;
        }
        if !stage_ids.insert(id.to_string()) {
            messages.push(error(
                "DUPLICATE_STAGE_ID",
                "duplicate stage/workstream id",
                Some(id.to_string()),
            ));
        }
        if stage.title.trim().is_empty() {
            messages.push(error(
                "REVIEW_STAGE_TITLE_REQUIRED",
                "review stage title is required",
                Some(id.to_string()),
            ));
        }
        if stage.prompt.trim().is_empty() && stage.stage_kind != ReviewStageKind::Approval {
            messages.push(error(
                "REVIEW_STAGE_PROMPT_REQUIRED",
                "review/test stage prompt is required",
                Some(id.to_string()),
            ));
        }
        if stage.target_ids.is_empty() {
            messages.push(error(
                "REVIEW_STAGE_TARGETS_REQUIRED",
                "review stage must target at least one upstream stage",
                Some(id.to_string()),
            ));
        }
        if let Some(phase_id) = stage.phase_id.as_deref() {
            if !phase_id.trim().is_empty() && !phase_ids.contains(phase_id.trim()) {
                messages.push(error(
                    "REVIEW_STAGE_PHASE_UNKNOWN",
                    "review stage phase_id references unknown mission phase",
                    Some(id.to_string()),
                ));
            }
        }
        if let Some(milestone) = stage.milestone.as_deref() {
            if !milestone.trim().is_empty() && !milestone_ids.contains(milestone.trim()) {
                messages.push(error(
                    "REVIEW_STAGE_MILESTONE_UNKNOWN",
                    "review stage milestone references unknown mission milestone",
                    Some(id.to_string()),
                ));
            }
        }
        if stage.stage_kind == ReviewStageKind::Approval {
            let gate = stage.gate.as_ref();
            if !gate.map(|value| value.required).unwrap_or(false) {
                messages.push(error(
                    "APPROVAL_GATE_REQUIRED",
                    "approval stage must include a required gate",
                    Some(id.to_string()),
                ));
            }
            let decisions = gate.map(|value| value.decisions.as_slice()).unwrap_or(&[]);
            if !decisions.contains(&ApprovalDecision::Approve)
                || !decisions.contains(&ApprovalDecision::Rework)
                || !decisions.contains(&ApprovalDecision::Cancel)
            {
                messages.push(error(
                    "APPROVAL_GATE_DECISIONS_INVALID",
                    "approval stage must support approve, rework, and cancel",
                    Some(id.to_string()),
                ));
            }
        }
    }

    for workstream in &blueprint.workstreams {
        for dep in &workstream.depends_on {
            if !workstream_ids.contains(dep.trim()) {
                messages.push(error(
                    "WORKSTREAM_DEPENDENCY_UNKNOWN",
                    "workstream depends_on references unknown workstream",
                    Some(workstream.workstream_id.clone()),
                ));
            }
        }
        for input_ref in &workstream.input_refs {
            if !workstream_ids.contains(input_ref.from_step_id.trim())
                && !stage_ids.contains(input_ref.from_step_id.trim())
            {
                messages.push(error(
                    "WORKSTREAM_INPUT_UNKNOWN",
                    "workstream input_refs references unknown upstream stage",
                    Some(workstream.workstream_id.clone()),
                ));
            }
        }
    }

    for stage in &blueprint.review_stages {
        for target in &stage.target_ids {
            if !stage_ids.contains(target.trim()) {
                messages.push(error(
                    "REVIEW_STAGE_TARGET_UNKNOWN",
                    "review stage target_ids references unknown stage",
                    Some(stage.stage_id.clone()),
                ));
            }
        }
        if let Some(gate) = stage.gate.as_ref() {
            for target in &gate.rework_targets {
                if !stage_ids.contains(target.trim()) {
                    messages.push(error(
                        "APPROVAL_GATE_REWORK_UNKNOWN",
                        "approval gate rework_targets references unknown stage",
                        Some(stage.stage_id.clone()),
                    ));
                }
            }
        }
    }

    for milestone in &blueprint.milestones {
        if milestone.required_stage_ids.is_empty() {
            messages.push(warning(
                "MISSION_MILESTONE_EMPTY",
                "mission milestone does not currently reference any required stages",
                Some(milestone.milestone_id.clone()),
            ));
        }
        for stage_id in &milestone.required_stage_ids {
            if !stage_ids.contains(stage_id.trim()) {
                messages.push(error(
                    "MISSION_MILESTONE_STAGE_UNKNOWN",
                    "mission milestone required_stage_ids references unknown stage",
                    Some(milestone.milestone_id.clone()),
                ));
            }
        }
    }

    if messages
        .iter()
        .all(|message| message.code != "WORKSTREAM_DEPENDENCY_UNKNOWN")
    {
        messages.extend(validate_cycles(blueprint));
    }

    messages.extend(validate_phase_barriers(blueprint));

    messages
}

fn validate_cycles(blueprint: &MissionBlueprint) -> Vec<ValidationMessage> {
    let mut graph = HashMap::<String, Vec<String>>::new();
    for workstream in &blueprint.workstreams {
        graph.insert(
            workstream.workstream_id.clone(),
            workstream.depends_on.clone(),
        );
    }
    for stage in &blueprint.review_stages {
        graph.insert(stage.stage_id.clone(), stage.target_ids.clone());
    }
    let mut visiting = HashSet::new();
    let mut visited = HashSet::new();
    let mut messages = Vec::new();
    for node in graph.keys() {
        if has_cycle(node, &graph, &mut visiting, &mut visited) {
            messages.push(error(
                "MISSION_GRAPH_CYCLE",
                "mission graph contains a dependency cycle",
                Some(node.clone()),
            ));
            break;
        }
    }
    messages
}

fn has_cycle(
    node: &str,
    graph: &HashMap<String, Vec<String>>,
    visiting: &mut HashSet<String>,
    visited: &mut HashSet<String>,
) -> bool {
    if visited.contains(node) {
        return false;
    }
    if !visiting.insert(node.to_string()) {
        return true;
    }
    if let Some(deps) = graph.get(node) {
        for dep in deps {
            if graph.contains_key(dep) && has_cycle(dep, graph, visiting, visited) {
                return true;
            }
        }
    }
    visiting.remove(node);
    visited.insert(node.to_string());
    false
}

fn error(code: &str, message: &str, subject_id: Option<String>) -> ValidationMessage {
    ValidationMessage {
        severity: ValidationSeverity::Error,
        code: code.to_string(),
        message: message.to_string(),
        subject_id,
    }
}

fn warning(code: &str, message: &str, subject_id: Option<String>) -> ValidationMessage {
    ValidationMessage {
        severity: ValidationSeverity::Warning,
        code: code.to_string(),
        message: message.to_string(),
        subject_id,
    }
}

fn phase_rank_map(blueprint: &MissionBlueprint) -> HashMap<String, usize> {
    blueprint
        .phases
        .iter()
        .enumerate()
        .map(|(index, phase)| (phase.phase_id.clone(), index))
        .collect()
}

fn validate_phase_barriers(blueprint: &MissionBlueprint) -> Vec<ValidationMessage> {
    let phase_rank = phase_rank_map(blueprint);
    let barrier_phases = blueprint
        .phases
        .iter()
        .filter_map(|phase| {
            (phase.execution_mode == Some(MissionPhaseExecutionMode::Barrier))
                .then_some(phase.phase_id.clone())
        })
        .collect::<HashSet<_>>();
    let stage_phase = blueprint
        .workstreams
        .iter()
        .map(|workstream| {
            (
                workstream.workstream_id.clone(),
                workstream.phase_id.clone().unwrap_or_default(),
            )
        })
        .chain(blueprint.review_stages.iter().map(|stage| {
            (
                stage.stage_id.clone(),
                stage.phase_id.clone().unwrap_or_default(),
            )
        }))
        .collect::<HashMap<_, _>>();
    let mut messages = Vec::new();
    for workstream in &blueprint.workstreams {
        if let Some(phase_id) = workstream.phase_id.as_deref() {
            if let Some(&rank) = phase_rank.get(phase_id) {
                for dep in &workstream.depends_on {
                    if let Some(dep_phase) = stage_phase.get(dep) {
                        if let Some(&dep_rank) = phase_rank.get(dep_phase) {
                            if dep_rank > rank {
                                messages.push(error(
                                    "WORKSTREAM_PHASE_ORDER_INVALID",
                                    "workstream depends on a later phase",
                                    Some(workstream.workstream_id.clone()),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    for stage in &blueprint.review_stages {
        if let Some(phase_id) = stage.phase_id.as_deref() {
            if let Some(&rank) = phase_rank.get(phase_id) {
                for target in &stage.target_ids {
                    if let Some(dep_phase) = stage_phase.get(target) {
                        if let Some(&dep_rank) = phase_rank.get(dep_phase) {
                            if dep_rank > rank {
                                messages.push(error(
                                    "REVIEW_STAGE_PHASE_ORDER_INVALID",
                                    "review stage targets a later phase",
                                    Some(stage.stage_id.clone()),
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
    for phase in &blueprint.phases {
        if phase.execution_mode != Some(MissionPhaseExecutionMode::Barrier) {
            continue;
        }
        let Some(&rank) = phase_rank.get(&phase.phase_id) else {
            continue;
        };
        let has_prior = rank > 0;
        let stage_count = blueprint
            .workstreams
            .iter()
            .filter(|workstream| workstream.phase_id.as_deref() == Some(phase.phase_id.as_str()))
            .count()
            + blueprint
                .review_stages
                .iter()
                .filter(|stage| stage.phase_id.as_deref() == Some(phase.phase_id.as_str()))
                .count();
        if has_prior && stage_count == 0 {
            messages.push(warning(
                "MISSION_PHASE_BARRIER_EMPTY",
                "barrier phase is defined but currently has no stages assigned",
                Some(phase.phase_id.clone()),
            ));
        }
        if !has_prior {
            continue;
        }
        let prior_barrier =
            blueprint.phases.iter().take(rank).any(|candidate| {
                candidate.execution_mode == Some(MissionPhaseExecutionMode::Barrier)
            });
        if !prior_barrier {
            messages.push(warning(
                "MISSION_PHASE_BARRIER_SOFT_PREFIX",
                "barrier phase will compile as a full dependency barrier across all earlier phases",
                Some(phase.phase_id.clone()),
            ));
        }
    }
    if !blueprint.phases.is_empty() {
        for workstream in &blueprint.workstreams {
            if workstream
                .phase_id
                .as_deref()
                .unwrap_or("")
                .trim()
                .is_empty()
            {
                messages.push(warning(
                    "WORKSTREAM_PHASE_UNSET",
                    "workstream has no phase_id even though mission phases are defined",
                    Some(workstream.workstream_id.clone()),
                ));
            }
        }
    }
    let _ = barrier_phases;
    messages
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_blueprint() -> MissionBlueprint {
        MissionBlueprint {
            mission_id: "mission-demo".to_string(),
            title: "Mission".to_string(),
            goal: "Produce a useful deliverable".to_string(),
            success_criteria: vec!["Artifact delivered".to_string()],
            shared_context: Some("Shared context".to_string()),
            workspace_root: "/tmp/workspace".to_string(),
            orchestrator_template_id: Some("orchestrator-default".to_string()),
            phases: vec![
                MissionPhaseBlueprint {
                    phase_id: "discover".to_string(),
                    title: "Discover".to_string(),
                    description: None,
                    execution_mode: Some(MissionPhaseExecutionMode::Soft),
                },
                MissionPhaseBlueprint {
                    phase_id: "synthesize".to_string(),
                    title: "Synthesize".to_string(),
                    description: None,
                    execution_mode: Some(MissionPhaseExecutionMode::Barrier),
                },
            ],
            milestones: vec![MissionMilestoneBlueprint {
                milestone_id: "draft_ready".to_string(),
                title: "Draft ready".to_string(),
                description: None,
                phase_id: Some("synthesize".to_string()),
                required_stage_ids: vec!["synthesis".to_string(), "approval".to_string()],
            }],
            team: MissionTeamBlueprint::default(),
            workstreams: vec![
                WorkstreamBlueprint {
                    workstream_id: "research".to_string(),
                    title: "Research".to_string(),
                    objective: "Collect inputs".to_string(),
                    role: "researcher".to_string(),
                    priority: Some(10),
                    phase_id: Some("discover".to_string()),
                    lane: Some("research".to_string()),
                    milestone: None,
                    template_id: None,
                    prompt: "Research the topic".to_string(),
                    model_override: None,
                    tool_allowlist_override: Vec::new(),
                    mcp_servers_override: Vec::new(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: OutputContractBlueprint {
                        kind: "report_markdown".to_string(),
                        schema: None,
                        summary_guidance: None,
                    },
                    retry_policy: None,
                    timeout_ms: None,
                    metadata: None,
                },
                WorkstreamBlueprint {
                    workstream_id: "synthesis".to_string(),
                    title: "Synthesis".to_string(),
                    objective: "Combine research".to_string(),
                    role: "analyst".to_string(),
                    priority: Some(5),
                    phase_id: Some("synthesize".to_string()),
                    lane: Some("analysis".to_string()),
                    milestone: Some("draft_ready".to_string()),
                    template_id: None,
                    prompt: "Synthesize the report".to_string(),
                    model_override: None,
                    tool_allowlist_override: Vec::new(),
                    mcp_servers_override: Vec::new(),
                    depends_on: vec!["research".to_string()],
                    input_refs: vec![InputRefBlueprint {
                        from_step_id: "research".to_string(),
                        alias: "research_report".to_string(),
                    }],
                    output_contract: OutputContractBlueprint {
                        kind: "report_markdown".to_string(),
                        schema: None,
                        summary_guidance: None,
                    },
                    retry_policy: None,
                    timeout_ms: None,
                    metadata: None,
                },
            ],
            review_stages: vec![ReviewStage {
                stage_id: "approval".to_string(),
                stage_kind: ReviewStageKind::Approval,
                title: "Approve".to_string(),
                priority: Some(1),
                phase_id: Some("synthesize".to_string()),
                lane: Some("governance".to_string()),
                milestone: Some("draft_ready".to_string()),
                target_ids: vec!["synthesis".to_string()],
                role: None,
                template_id: None,
                prompt: String::new(),
                checklist: Vec::new(),
                model_override: None,
                tool_allowlist_override: Vec::new(),
                mcp_servers_override: Vec::new(),
                gate: Some(HumanApprovalGate {
                    required: true,
                    decisions: vec![
                        ApprovalDecision::Approve,
                        ApprovalDecision::Rework,
                        ApprovalDecision::Cancel,
                    ],
                    rework_targets: vec!["synthesis".to_string()],
                    instructions: None,
                }),
            }],
            metadata: None,
        }
    }

    #[test]
    fn sample_blueprint_validates_cleanly() {
        let messages = validate_mission_blueprint(&sample_blueprint());
        assert!(messages
            .iter()
            .all(|message| message.severity != ValidationSeverity::Error));
    }

    #[test]
    fn cycle_is_reported() {
        let mut blueprint = sample_blueprint();
        blueprint.workstreams[0]
            .depends_on
            .push("synthesis".to_string());
        let messages = validate_mission_blueprint(&blueprint);
        assert!(messages
            .iter()
            .any(|message| message.code == "MISSION_GRAPH_CYCLE"));
    }

    #[test]
    fn invalid_phase_reference_is_reported() {
        let mut blueprint = sample_blueprint();
        blueprint.workstreams[0].phase_id = Some("missing".to_string());
        let messages = validate_mission_blueprint(&blueprint);
        assert!(messages
            .iter()
            .any(|message| message.code == "WORKSTREAM_PHASE_UNKNOWN"));
    }

    #[test]
    fn later_phase_dependency_is_reported() {
        let mut blueprint = sample_blueprint();
        blueprint.workstreams[0]
            .depends_on
            .push("synthesis".to_string());
        let messages = validate_mission_blueprint(&blueprint);
        assert!(messages
            .iter()
            .any(|message| message.code == "WORKSTREAM_PHASE_ORDER_INVALID"));
    }
}
