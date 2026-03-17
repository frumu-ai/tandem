use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde::Serialize;
use serde_json::{json, Value};
use tandem_orchestrator::{MissionSpec, WorkItem, WorkItemStatus};
use tandem_workflows::{
    validate_mission_blueprint, ApprovalDecision, MissionBlueprint, MissionPhaseExecutionMode,
    OutputContractBlueprint, ReviewStageKind, ValidationMessage, ValidationSeverity,
};
use uuid::Uuid;

use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct MissionBuilderPreviewRequest {
    pub blueprint: MissionBlueprint,
    #[serde(default)]
    pub schedule: Option<crate::AutomationV2Schedule>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MissionBuilderApplyRequest {
    pub blueprint: MissionBlueprint,
    #[serde(default)]
    pub creator_id: Option<String>,
    #[serde(default)]
    pub schedule: Option<crate::AutomationV2Schedule>,
}

#[derive(Debug, Clone, Serialize)]
struct CompiledNodePreview {
    node_id: String,
    title: String,
    agent_id: String,
    stage_kind: String,
    phase_id: Option<String>,
    lane: Option<String>,
    milestone: Option<String>,
    priority: Option<i32>,
    depends_on: Vec<String>,
    tool_allowlist: Vec<String>,
    mcp_servers: Vec<String>,
    inherited_brief: String,
}

#[derive(Debug, Clone, Serialize)]
struct MissionCompilePreview {
    blueprint: MissionBlueprint,
    automation: crate::AutomationV2Spec,
    mission_spec: MissionSpec,
    work_items: Vec<WorkItem>,
    node_previews: Vec<CompiledNodePreview>,
    validation: Vec<ValidationMessage>,
}

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

pub(super) async fn mission_builder_preview(
    State(_state): State<AppState>,
    Json(input): Json<MissionBuilderPreviewRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let preview = compile_blueprint_preview(input.blueprint, input.schedule, "mission_builder")?;
    Ok(Json(
        serde_json::to_value(preview).unwrap_or_else(|_| json!({})),
    ))
}

pub(super) async fn mission_builder_apply(
    State(state): State<AppState>,
    Json(input): Json<MissionBuilderApplyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let creator_id = input
        .creator_id
        .as_deref()
        .unwrap_or("mission_builder")
        .to_string();
    let preview = compile_blueprint_preview(input.blueprint, input.schedule, &creator_id)?;
    let stored = state
        .put_automation_v2(preview.automation.clone())
        .await
        .map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error.to_string(),
                    "code": "MISSION_BUILDER_APPLY_FAILED",
                })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "automation": stored,
        "mission_spec": preview.mission_spec,
        "work_items": preview.work_items,
        "node_previews": preview.node_previews,
        "validation": preview.validation,
    })))
}

fn compile_blueprint_preview(
    blueprint: MissionBlueprint,
    schedule: Option<crate::AutomationV2Schedule>,
    creator_id: &str,
) -> Result<MissionCompilePreview, (StatusCode, Json<Value>)> {
    let validation = validate_mission_blueprint(&blueprint);
    let has_errors = validation
        .iter()
        .any(|message| message.severity == ValidationSeverity::Error);
    if has_errors {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "mission blueprint validation failed",
                "code": "MISSION_BLUEPRINT_INVALID",
                "validation": validation,
            })),
        ));
    }

    let mission_spec = derive_mission_spec(&blueprint);
    let work_items = derive_work_items(&blueprint);
    let automation = compile_to_automation(blueprint.clone(), schedule, creator_id);
    let node_previews = derive_node_previews(&blueprint, &automation);

    Ok(MissionCompilePreview {
        blueprint,
        automation,
        mission_spec,
        work_items,
        node_previews,
        validation,
    })
}

fn derive_mission_spec(blueprint: &MissionBlueprint) -> MissionSpec {
    let mut spec = MissionSpec::new(blueprint.title.clone(), blueprint.goal.clone());
    spec.mission_id = blueprint.mission_id.clone();
    spec.success_criteria = blueprint.success_criteria.clone();
    spec.entrypoint = Some("automation_v2".to_string());
    spec.metadata = Some(json!({
        "builder_kind": "mission_blueprint",
        "shared_context": blueprint.shared_context,
        "orchestrator_template_id": blueprint.orchestrator_template_id,
        "phases": blueprint.phases,
        "milestones": blueprint.milestones,
    }));
    spec
}

fn derive_work_items(blueprint: &MissionBlueprint) -> Vec<WorkItem> {
    let mut items = blueprint
        .workstreams
        .iter()
        .map(|workstream| WorkItem {
            work_item_id: workstream.workstream_id.clone(),
            title: workstream.title.clone(),
            detail: Some(workstream.objective.clone()),
            status: WorkItemStatus::Todo,
            depends_on: workstream.depends_on.clone(),
            assigned_agent: Some(format!("agent_{}", workstream.workstream_id)),
            run_id: None,
            artifact_refs: Vec::new(),
            metadata: Some(json!({
                "role": workstream.role,
                "template_id": workstream.template_id,
                "stage_kind": "workstream",
                "priority": workstream.priority,
                "phase_id": workstream.phase_id,
                "lane": workstream.lane,
                "milestone": workstream.milestone,
            })),
        })
        .collect::<Vec<_>>();
    items.extend(blueprint.review_stages.iter().map(|stage| WorkItem {
        work_item_id: stage.stage_id.clone(),
        title: stage.title.clone(),
        detail: Some(stage.prompt.clone()),
        status: WorkItemStatus::Todo,
        depends_on: stage.target_ids.clone(),
        assigned_agent: Some(format!("agent_{}", stage.stage_id)),
        run_id: None,
        artifact_refs: Vec::new(),
        metadata: Some(json!({
            "stage_kind": format!("{:?}", stage.stage_kind).to_ascii_lowercase(),
            "template_id": stage.template_id,
            "role": stage.role,
            "priority": stage.priority,
            "phase_id": stage.phase_id,
            "lane": stage.lane,
            "milestone": stage.milestone,
        })),
    }));
    items
}

fn compile_to_automation(
    blueprint: MissionBlueprint,
    schedule: Option<crate::AutomationV2Schedule>,
    creator_id: &str,
) -> crate::AutomationV2Spec {
    let now = crate::now_ms();
    let mut agents = Vec::new();
    let orchestrator_agent_id = "mission_orchestrator".to_string();
    agents.push(crate::AutomationAgentProfile {
        agent_id: orchestrator_agent_id.clone(),
        template_id: blueprint.orchestrator_template_id.clone(),
        display_name: "Mission Orchestrator".to_string(),
        avatar_url: None,
        model_policy: blueprint.team.default_model_policy.clone(),
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["*".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: blueprint.team.allowed_mcp_servers.clone(),
            allowed_tools: None,
        },
        approval_policy: None,
    });

    let phase_rank = phase_rank_map(&blueprint);
    let barrier_deps = compile_barrier_dependencies(&blueprint, &phase_rank);
    let mut nodes = Vec::new();

    for workstream in &blueprint.workstreams {
        let agent_id = format!("agent_{}", workstream.workstream_id);
        agents.push(crate::AutomationAgentProfile {
            agent_id: agent_id.clone(),
            template_id: workstream.template_id.clone(),
            display_name: workstream.title.clone(),
            avatar_url: None,
            model_policy: merge_model_policy(
                blueprint.team.default_model_policy.as_ref(),
                workstream.model_override.as_ref(),
            ),
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: if workstream.tool_allowlist_override.is_empty() {
                    vec!["*".to_string()]
                } else {
                    workstream.tool_allowlist_override.clone()
                },
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: if workstream.mcp_servers_override.is_empty() {
                    blueprint.team.allowed_mcp_servers.clone()
                } else {
                    workstream.mcp_servers_override.clone()
                },
                allowed_tools: None,
            },
            approval_policy: None,
        });
        let mut input_refs = workstream
            .input_refs
            .iter()
            .map(|input| crate::AutomationFlowInputRef {
                from_step_id: input.from_step_id.clone(),
                alias: input.alias.clone(),
            })
            .collect::<Vec<_>>();
        for dep in &workstream.depends_on {
            if !input_refs.iter().any(|input| input.from_step_id == *dep) {
                input_refs.push(crate::AutomationFlowInputRef {
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
        nodes.push(crate::AutomationFlowNode {
            node_id: workstream.workstream_id.clone(),
            agent_id,
            objective: workstream.objective.clone(),
            depends_on: depends_on.clone(),
            input_refs,
            output_contract: Some(output_contract(&workstream.output_contract)),
            retry_policy: workstream.retry_policy.clone(),
            timeout_ms: workstream.timeout_ms,
            stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: Some(json!({
                "builder": {
                    "title": workstream.title,
                    "role": workstream.role,
                    "prompt": workstream.prompt,
                    "priority": workstream.priority,
                    "phase_id": workstream.phase_id,
                    "lane": workstream.lane,
                    "milestone": workstream.milestone,
                }
            })),
        });
    }

    for stage in &blueprint.review_stages {
        let stage_kind = match stage.stage_kind {
            ReviewStageKind::Review => crate::AutomationNodeStageKind::Review,
            ReviewStageKind::Test => crate::AutomationNodeStageKind::Test,
            ReviewStageKind::Approval => crate::AutomationNodeStageKind::Approval,
        };
        let agent_id = if stage.stage_kind == ReviewStageKind::Approval {
            orchestrator_agent_id.clone()
        } else {
            let stage_agent_id = format!("agent_{}", stage.stage_id);
            agents.push(crate::AutomationAgentProfile {
                agent_id: stage_agent_id.clone(),
                template_id: stage.template_id.clone(),
                display_name: stage.title.clone(),
                avatar_url: None,
                model_policy: merge_model_policy(
                    blueprint.team.default_model_policy.as_ref(),
                    stage.model_override.as_ref(),
                ),
                skills: Vec::new(),
                tool_policy: crate::AutomationAgentToolPolicy {
                    allowlist: vec!["*".to_string()],
                    denylist: Vec::new(),
                },
                mcp_policy: crate::AutomationAgentMcpPolicy {
                    allowed_servers: blueprint.team.allowed_mcp_servers.clone(),
                    allowed_tools: None,
                },
                approval_policy: None,
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
        if stage.stage_kind != ReviewStageKind::Approval {
            if let Some(agent) = agents.iter_mut().find(|agent| agent.agent_id == agent_id) {
                agent.tool_policy.allowlist = stage_tool_allowlist.clone();
                agent.mcp_policy.allowed_servers = stage_mcp_servers.clone();
            }
        }
        let mut depends_on = stage.target_ids.clone();
        if let Some(extra) = barrier_deps.get(&stage.stage_id) {
            for dep in extra {
                if !depends_on.contains(dep) {
                    depends_on.push(dep.clone());
                }
            }
        }
        nodes.push(crate::AutomationFlowNode {
            node_id: stage.stage_id.clone(),
            agent_id,
            objective: if stage.prompt.trim().is_empty() {
                stage.title.clone()
            } else {
                stage.prompt.clone()
            },
            depends_on: depends_on.clone(),
            input_refs: stage
                .target_ids
                .iter()
                .map(|target_id| crate::AutomationFlowInputRef {
                    from_step_id: target_id.clone(),
                    alias: target_id.clone(),
                })
                .collect(),
            output_contract: Some(output_contract(&OutputContractBlueprint {
                kind: if stage.stage_kind == ReviewStageKind::Approval {
                    "approval_gate".to_string()
                } else {
                    "review_summary".to_string()
                },
                schema: None,
                summary_guidance: Some(
                    "Summarize the review outcome and required follow-ups.".to_string(),
                ),
            })),
            retry_policy: Some(json!({ "max_attempts": 1 })),
            timeout_ms: None,
            stage_kind: Some(stage_kind),
            gate: stage
                .gate
                .as_ref()
                .map(|gate| crate::AutomationApprovalGate {
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
                }),
            metadata: Some(json!({
                "builder": {
                    "title": stage.title,
                    "checklist": stage.checklist,
                    "role": stage.role,
                    "tool_allowlist_override": stage_tool_allowlist,
                    "mcp_servers_override": stage_mcp_servers,
                    "priority": stage.priority,
                    "phase_id": stage.phase_id,
                    "lane": stage.lane,
                    "milestone": stage.milestone,
                }
            })),
        });
    }

    nodes.sort_by(|a, b| node_sort_key(a, &phase_rank).cmp(&node_sort_key(b, &phase_rank)));

    let typed_coder_metadata = extract_coder_metadata(&blueprint);
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

    crate::AutomationV2Spec {
        automation_id: format!("automation-v2-{}", Uuid::new_v4()),
        name: blueprint.title.clone(),
        description: Some(blueprint.goal.clone()),
        status: crate::AutomationV2Status::Draft,
        schedule: schedule.unwrap_or_else(default_manual_schedule),
        agents,
        flow: crate::AutomationFlowSpec { nodes },
        execution: crate::AutomationExecutionPolicy {
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
        output_targets: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: creator_id.to_string(),
        workspace_root: Some(blueprint.workspace_root.clone()),
        metadata: Some(Value::Object(metadata)),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    }
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

fn derive_node_previews(
    blueprint: &MissionBlueprint,
    automation: &crate::AutomationV2Spec,
) -> Vec<CompiledNodePreview> {
    automation
        .flow
        .nodes
        .iter()
        .map(|node| CompiledNodePreview {
            node_id: node.node_id.clone(),
            title: node
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("builder"))
                .and_then(|builder| builder.get("title"))
                .and_then(Value::as_str)
                .unwrap_or(node.node_id.as_str())
                .to_string(),
            agent_id: node.agent_id.clone(),
            stage_kind: node
                .stage_kind
                .as_ref()
                .map(|kind| format!("{kind:?}").to_ascii_lowercase())
                .unwrap_or_else(|| "workstream".to_string()),
            phase_id: node_builder_metadata(node, "phase_id"),
            lane: node_builder_metadata(node, "lane"),
            milestone: node_builder_metadata(node, "milestone"),
            priority: node_builder_priority(node),
            depends_on: node.depends_on.clone(),
            tool_allowlist: automation
                .agents
                .iter()
                .find(|agent| agent.agent_id == node.agent_id)
                .map(|agent| agent.tool_policy.allowlist.clone())
                .unwrap_or_default(),
            mcp_servers: automation
                .agents
                .iter()
                .find(|agent| agent.agent_id == node.agent_id)
                .map(|agent| agent.mcp_policy.allowed_servers.clone())
                .unwrap_or_default(),
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
        })
        .collect::<Vec<_>>()
}

fn output_contract(contract: &OutputContractBlueprint) -> crate::AutomationFlowOutputContract {
    crate::AutomationFlowOutputContract {
        kind: contract.kind.clone(),
        validator: Some(match contract.kind.trim().to_ascii_lowercase().as_str() {
            "brief" => crate::AutomationOutputValidatorKind::ResearchBrief,
            "review" | "review_summary" | "approval_gate" => {
                crate::AutomationOutputValidatorKind::ReviewDecision
            }
            "structured_json" => crate::AutomationOutputValidatorKind::StructuredJson,
            "report_markdown" | "text_summary" | "urls" | "citations" => {
                crate::AutomationOutputValidatorKind::GenericArtifact
            }
            _ => crate::AutomationOutputValidatorKind::GenericArtifact,
        }),
        schema: contract.schema.clone(),
        summary_guidance: contract.summary_guidance.clone(),
    }
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

fn default_manual_schedule() -> crate::AutomationV2Schedule {
    crate::AutomationV2Schedule {
        schedule_type: crate::AutomationV2ScheduleType::Manual,
        cron_expression: None,
        interval_seconds: None,
        timezone: "UTC".to_string(),
        misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
    }
}

fn phase_rank_map(blueprint: &MissionBlueprint) -> std::collections::HashMap<String, usize> {
    blueprint
        .phases
        .iter()
        .enumerate()
        .map(|(index, phase)| (phase.phase_id.clone(), index))
        .collect()
}

fn compile_barrier_dependencies(
    blueprint: &MissionBlueprint,
    phase_rank: &std::collections::HashMap<String, usize>,
) -> std::collections::HashMap<String, Vec<String>> {
    let mut stage_phase = std::collections::HashMap::<String, String>::new();
    for workstream in &blueprint.workstreams {
        if let Some(phase_id) = workstream.phase_id.as_ref() {
            stage_phase.insert(workstream.workstream_id.clone(), phase_id.clone());
        }
    }
    for stage in &blueprint.review_stages {
        if let Some(phase_id) = stage.phase_id.as_ref() {
            stage_phase.insert(stage.stage_id.clone(), phase_id.clone());
        }
    }
    let mut out = std::collections::HashMap::<String, Vec<String>>::new();
    for phase in &blueprint.phases {
        if phase.execution_mode != Some(MissionPhaseExecutionMode::Barrier) {
            continue;
        }
        let Some(&rank) = phase_rank.get(&phase.phase_id) else {
            continue;
        };
        let prior_stage_ids = stage_phase
            .iter()
            .filter_map(|(stage_id, stage_phase_id)| {
                phase_rank
                    .get(stage_phase_id)
                    .filter(|dep_rank| **dep_rank < rank)
                    .map(|_| stage_id.clone())
            })
            .collect::<Vec<_>>();
        for workstream in &blueprint.workstreams {
            if workstream.phase_id.as_deref() == Some(phase.phase_id.as_str()) {
                out.insert(workstream.workstream_id.clone(), prior_stage_ids.clone());
            }
        }
        for stage in &blueprint.review_stages {
            if stage.phase_id.as_deref() == Some(phase.phase_id.as_str()) {
                out.insert(stage.stage_id.clone(), prior_stage_ids.clone());
            }
        }
    }
    out
}

fn node_builder_metadata(node: &crate::AutomationFlowNode, key: &str) -> Option<String> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get(key))
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn node_builder_priority(node: &crate::AutomationFlowNode) -> Option<i32> {
    node.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("builder"))
        .and_then(|builder| builder.get("priority"))
        .and_then(Value::as_i64)
        .and_then(|value| i32::try_from(value).ok())
}

fn node_sort_key(
    node: &crate::AutomationFlowNode,
    phase_rank: &std::collections::HashMap<String, usize>,
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
