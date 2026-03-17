use crate::agent_teams::{emit_spawn_approved, emit_spawn_denied, emit_spawn_requested};
use crate::http::AppState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_orchestrator::{
    AgentInstanceStatus, DefaultMissionReducer, MissionEvent, MissionReducer, MissionSpec,
    NoopMissionReducer, SpawnRequest, SpawnSource, WorkItem, WorkItemStatus,
};
use tandem_types::EngineEvent;
use uuid::Uuid;

fn instance_metadata_field(instance: &tandem_orchestrator::AgentInstance, key: &str) -> Value {
    instance
        .metadata
        .as_ref()
        .and_then(|row| row.get(key))
        .cloned()
        .unwrap_or(Value::Null)
}

#[derive(Debug, Serialize)]
pub(super) struct AgentTeamToolApprovalOutput {
    #[serde(rename = "approvalID")]
    pub approval_id: String,
    #[serde(rename = "sessionID", skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(rename = "toolCallID")]
    pub tool_call_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    pub status: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct MissionCreateInput {
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub work_items: Vec<MissionCreateWorkItem>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MissionCreateWorkItem {
    #[serde(default)]
    pub work_item_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub assigned_agent: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MissionEventInput {
    pub event: MissionEvent,
}

#[derive(Debug, Deserialize)]
pub(super) struct AgentTeamSpawnInput {
    #[serde(rename = "missionID")]
    pub mission_id: Option<String>,
    #[serde(rename = "parentInstanceID")]
    pub parent_instance_id: Option<String>,
    #[serde(rename = "templateID")]
    pub template_id: Option<String>,
    pub role: tandem_orchestrator::AgentRole,
    pub source: Option<SpawnSource>,
    pub justification: String,
    #[serde(default)]
    pub budget_override: Option<tandem_orchestrator::BudgetLimit>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AgentTeamInstancesQuery {
    #[serde(rename = "missionID")]
    pub mission_id: Option<String>,
    #[serde(rename = "parentInstanceID")]
    pub parent_instance_id: Option<String>,
    pub status: Option<AgentInstanceStatus>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AgentTeamCancelInput {
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AgentTeamTemplateCreateInput {
    pub template: tandem_orchestrator::AgentTemplate,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct AgentTeamTemplatePatchInput {
    pub display_name: Option<String>,
    pub avatar_url: Option<String>,
    pub role: Option<tandem_orchestrator::AgentRole>,
    pub system_prompt: Option<String>,
    pub default_model: Option<Value>,
    pub skills: Option<Vec<tandem_orchestrator::SkillRef>>,
    pub default_budget: Option<tandem_orchestrator::BudgetLimit>,
    pub capabilities: Option<tandem_orchestrator::CapabilitySpec>,
}

#[derive(Debug, Deserialize)]
pub(super) struct AgentStandupComposeInput {
    pub name: String,
    pub workspace_root: String,
    pub schedule: crate::AutomationV2Schedule,
    pub participant_template_ids: Vec<String>,
    #[serde(default)]
    pub report_path_template: Option<String>,
}

pub(super) fn mission_event_id(event: &MissionEvent) -> &str {
    match event {
        MissionEvent::MissionStarted { mission_id }
        | MissionEvent::MissionPaused { mission_id, .. }
        | MissionEvent::MissionResumed { mission_id }
        | MissionEvent::MissionCanceled { mission_id, .. }
        | MissionEvent::RunStarted { mission_id, .. }
        | MissionEvent::RunFinished { mission_id, .. }
        | MissionEvent::ToolObserved { mission_id, .. }
        | MissionEvent::ApprovalGranted { mission_id, .. }
        | MissionEvent::ApprovalDenied { mission_id, .. }
        | MissionEvent::TimerFired { mission_id, .. }
        | MissionEvent::ResourceChanged { mission_id, .. } => mission_id,
    }
}

fn standup_slug(raw: &str, fallback: &str) -> String {
    let mut out = String::new();
    let mut previous_dash = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_dash = false;
        } else if !previous_dash {
            out.push('-');
            previous_dash = true;
        }
    }
    let cleaned = out.trim_matches('-').to_string();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

fn validate_standup_report_path(raw: &str) -> Result<String, &'static str> {
    let value = raw.trim();
    if value.is_empty() {
        return Err("report_path_template is required");
    }
    if value.starts_with('/') {
        return Err("report_path_template must be workspace-relative");
    }
    if value.contains("..") {
        return Err("report_path_template must not traverse parent directories");
    }
    Ok(value.to_string())
}

fn standup_participant_objective(template_name: &str) -> String {
    format!(
        "You are preparing your daily standup update for {template_name}. Review relevant workspace context and use `memory_search` for prior conversations and history. `memory_search` defaults to the current session, current workspace/project, and global Tandem memory, so use it directly unless you need to narrow scope. Use `glob` to enumerate files and directories, `grep` to find relevant text, and `read` only on concrete files. Return valid JSON with keys `yesterday`, `today`, and `blockers`. Keep each field concise and evidence-based. If evidence is unavailable, say so plainly instead of guessing."
    )
}

fn standup_synthesis_objective(report_path_template: &str) -> String {
    format!(
        "Synthesize all participant standup updates into a clear markdown engineering standup. Include sections for Yesterday, Today, and Blockers grouped by participant. Write the final markdown report to `{report_path_template}` relative to the workspace root. After writing the report, store a concise standup summary in project memory with `memory_store`, using `tier: \"project\"`, source `agent_standup_summary`, and metadata that includes the report path. Then return a short confirmation summary."
    )
}

pub(super) async fn mission_create(
    State(state): State<AppState>,
    Json(input): Json<MissionCreateInput>,
) -> Json<Value> {
    let spec = MissionSpec::new(input.title, input.goal);
    let mission_id = spec.mission_id.clone();
    let mut mission = NoopMissionReducer::init(spec);
    mission.work_items = input
        .work_items
        .into_iter()
        .map(|item| WorkItem {
            work_item_id: item
                .work_item_id
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            title: item.title,
            detail: item.detail,
            status: WorkItemStatus::Todo,
            depends_on: Vec::new(),
            assigned_agent: item.assigned_agent,
            run_id: None,
            artifact_refs: Vec::new(),
            metadata: None,
        })
        .collect();

    state
        .missions
        .write()
        .await
        .insert(mission_id.clone(), mission.clone());
    state.event_bus.publish(EngineEvent::new(
        "mission.created",
        json!({
            "missionID": mission_id,
            "workItemCount": mission.work_items.len(),
        }),
    ));

    Json(json!({
        "mission": mission,
    }))
}

pub(super) async fn mission_list(State(state): State<AppState>) -> Json<Value> {
    let mut missions = state
        .missions
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    missions.sort_by(|a, b| a.mission_id.cmp(&b.mission_id));
    Json(json!({
        "missions": missions,
        "count": missions.len(),
    }))
}

pub(super) async fn mission_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mission = state
        .missions
        .read()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Mission not found",
                    "code": "MISSION_NOT_FOUND",
                    "missionID": id,
                })),
            )
        })?;
    Ok(Json(json!({
        "mission": mission,
    })))
}

pub(super) async fn mission_apply_event(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<MissionEventInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let event = input.event;
    let event_for_runtime = event.clone();
    if mission_event_id(&event) != id {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Mission event mission_id mismatch",
                "code": "MISSION_EVENT_MISMATCH",
                "missionID": id,
            })),
        ));
    }

    let current = state
        .missions
        .read()
        .await
        .get(&id)
        .cloned()
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "Mission not found",
                    "code": "MISSION_NOT_FOUND",
                    "missionID": id,
                })),
            )
        })?;

    let (next, commands) = DefaultMissionReducer::reduce(&current, event);
    let next_revision = next.revision;
    let next_status = next.status.clone();
    state
        .missions
        .write()
        .await
        .insert(id.clone(), next.clone());

    state.event_bus.publish(EngineEvent::new(
        "mission.updated",
        json!({
            "missionID": id,
            "revision": next_revision,
            "status": next_status,
            "commandCount": commands.len(),
        }),
    ));
    let orchestrator_spawns =
        run_orchestrator_runtime_spawns(&state, &next, &event_for_runtime).await;
    let orchestrator_cancellations =
        run_orchestrator_runtime_cancellations(&state, &next, &event_for_runtime).await;

    Ok(Json(json!({
        "mission": next,
        "commands": commands,
        "orchestratorSpawns": orchestrator_spawns,
        "orchestratorCancellations": orchestrator_cancellations,
    })))
}

async fn run_orchestrator_runtime_spawns(
    state: &AppState,
    mission: &tandem_orchestrator::MissionState,
    event: &MissionEvent,
) -> Vec<Value> {
    let MissionEvent::MissionStarted { mission_id } = event else {
        return Vec::new();
    };
    if mission_id != &mission.mission_id {
        return Vec::new();
    }
    let mut rows = Vec::new();
    for item in &mission.work_items {
        let Some(agent_name) = item.assigned_agent.as_deref() else {
            continue;
        };
        let Some(role) = parse_agent_role(agent_name) else {
            rows.push(json!({
                "workItemID": item.work_item_id,
                "agent": agent_name,
                "ok": false,
                "code": "UNSUPPORTED_ASSIGNED_AGENT",
                "error": "assigned_agent does not map to an agent-team role"
            }));
            continue;
        };
        let req = SpawnRequest {
            mission_id: Some(mission.mission_id.clone()),
            parent_instance_id: None,
            source: SpawnSource::OrchestratorRuntime,
            parent_role: Some(tandem_orchestrator::AgentRole::Orchestrator),
            role,
            template_id: None,
            justification: format!("mission work item {}", item.work_item_id),
            budget_override: None,
        };
        emit_spawn_requested(state, &req);
        let result = state.agent_teams.spawn(state, req.clone()).await;
        if !result.decision.allowed || result.instance.is_none() {
            emit_spawn_denied(state, &req, &result.decision);
            rows.push(json!({
                "workItemID": item.work_item_id,
                "agent": agent_name,
                "ok": false,
                "code": result.decision.code,
                "error": result.decision.reason,
            }));
            continue;
        }
        let instance = result.instance.expect("checked is_some");
        emit_spawn_approved(state, &req, &instance);
        rows.push(json!({
            "workItemID": item.work_item_id,
            "agent": agent_name,
            "ok": true,
            "instanceID": instance.instance_id,
            "sessionID": instance.session_id,
            "status": instance.status,
        }));
    }
    rows
}

fn parse_agent_role(agent_name: &str) -> Option<tandem_orchestrator::AgentRole> {
    match agent_name.trim().to_ascii_lowercase().as_str() {
        "orchestrator" => Some(tandem_orchestrator::AgentRole::Orchestrator),
        "delegator" => Some(tandem_orchestrator::AgentRole::Delegator),
        "worker" => Some(tandem_orchestrator::AgentRole::Worker),
        "watcher" => Some(tandem_orchestrator::AgentRole::Watcher),
        "reviewer" => Some(tandem_orchestrator::AgentRole::Reviewer),
        "tester" => Some(tandem_orchestrator::AgentRole::Tester),
        "committer" => Some(tandem_orchestrator::AgentRole::Committer),
        _ => None,
    }
}

async fn run_orchestrator_runtime_cancellations(
    state: &AppState,
    mission: &tandem_orchestrator::MissionState,
    event: &MissionEvent,
) -> Value {
    let MissionEvent::MissionCanceled { mission_id, reason } = event else {
        return json!({
            "triggered": false,
            "cancelledInstances": 0u64
        });
    };
    if mission_id != &mission.mission_id {
        return json!({
            "triggered": false,
            "cancelledInstances": 0u64
        });
    }
    let cancelled = state
        .agent_teams
        .cancel_mission(state, &mission.mission_id, reason)
        .await;
    json!({
        "triggered": true,
        "reason": reason,
        "cancelledInstances": cancelled,
    })
}

pub(super) async fn agent_team_templates(State(state): State<AppState>) -> Json<Value> {
    let templates = state.agent_teams.list_templates().await;
    Json(json!({
        "templates": templates,
        "count": templates.len(),
    }))
}

pub(super) async fn agent_team_template_create(
    State(state): State<AppState>,
    Json(input): Json<AgentTeamTemplateCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if input.template.template_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "code": "INVALID_TEMPLATE_ID",
                "error": "template_id is required"
            })),
        ));
    }
    let workspace_root = state.workspace_index.snapshot().await.root;
    let template = state
        .agent_teams
        .upsert_template(&workspace_root, input.template)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "ok": false,
                    "code": "TEMPLATE_PERSIST_FAILED",
                    "error": error.to_string(),
                })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "template": template,
    })))
}

pub(super) async fn agent_team_template_patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AgentTeamTemplatePatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let existing = state
        .agent_teams
        .list_templates()
        .await
        .into_iter()
        .find(|template| template.template_id == id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "ok": false,
                    "code": "TEMPLATE_NOT_FOUND",
                    "error": "template not found",
                    "templateID": id,
                })),
            )
        })?;
    let mut updated = existing;
    if let Some(display_name) = input.display_name {
        updated.display_name = Some(display_name);
    }
    if let Some(avatar_url) = input.avatar_url {
        updated.avatar_url = Some(avatar_url);
    }
    if let Some(role) = input.role {
        updated.role = role;
    }
    if let Some(system_prompt) = input.system_prompt {
        updated.system_prompt = Some(system_prompt);
    }
    if let Some(default_model) = input.default_model {
        updated.default_model = Some(default_model);
    }
    if let Some(skills) = input.skills {
        updated.skills = skills;
    }
    if let Some(default_budget) = input.default_budget {
        updated.default_budget = default_budget;
    }
    if let Some(capabilities) = input.capabilities {
        updated.capabilities = capabilities;
    }

    let workspace_root = state.workspace_index.snapshot().await.root;
    let template = state
        .agent_teams
        .upsert_template(&workspace_root, updated)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "ok": false,
                    "code": "TEMPLATE_PERSIST_FAILED",
                    "error": error.to_string(),
                })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "template": template,
    })))
}

pub(super) async fn agent_team_template_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let workspace_root = state.workspace_index.snapshot().await.root;
    let deleted = state
        .agent_teams
        .delete_template(&workspace_root, &id)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "ok": false,
                    "code": "TEMPLATE_DELETE_FAILED",
                    "error": error.to_string(),
                })),
            )
        })?;
    if !deleted {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "code": "TEMPLATE_NOT_FOUND",
                "error": "template not found",
                "templateID": id,
            })),
        ));
    }
    Ok(Json(json!({
        "ok": true,
        "deleted": true,
        "templateID": id,
    })))
}

pub(super) async fn agent_standup_compose(
    State(state): State<AppState>,
    Json(input): Json<AgentStandupComposeInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "code": "INVALID_STANDUP_NAME",
                "error": "name is required",
            })),
        ));
    }
    let workspace_root =
        crate::normalize_absolute_workspace_root(&input.workspace_root).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "code": "INVALID_WORKSPACE_ROOT",
                    "error": error,
                })),
            )
        })?;
    let report_path_template = validate_standup_report_path(
        input
            .report_path_template
            .as_deref()
            .unwrap_or("docs/standups/{{date}}.md"),
    )
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "code": "INVALID_REPORT_PATH",
                "error": error,
            })),
        )
    })?;
    state
        .agent_teams
        .ensure_loaded_for_workspace(&workspace_root)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "ok": false,
                    "code": "TEMPLATE_LOAD_FAILED",
                    "error": error.to_string(),
                })),
            )
        })?;

    let participant_ids = input
        .participant_template_ids
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    if participant_ids.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "code": "EMPTY_PARTICIPANTS",
                "error": "at least one participant template is required",
            })),
        ));
    }

    let templates = state.agent_teams.list_templates().await;
    let template_map = templates
        .into_iter()
        .map(|template| (template.template_id.clone(), template))
        .collect::<std::collections::HashMap<_, _>>();
    let mut participants = Vec::new();
    for template_id in &participant_ids {
        let Some(template) = template_map.get(template_id).cloned() else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "code": "TEMPLATE_NOT_FOUND",
                    "error": format!("unknown participant template `{template_id}`"),
                })),
            ));
        };
        participants.push(template);
    }

    let now = crate::now_ms();
    let automation_id = format!("standup-{}", Uuid::new_v4());
    let schedule_timezone = input.schedule.timezone.clone();
    let mut agents = Vec::new();
    let mut nodes = Vec::new();
    let mut participant_node_ids = Vec::new();
    for (index, template) in participants.iter().enumerate() {
        let participant_slug = standup_slug(
            template
                .display_name
                .as_deref()
                .unwrap_or(template.template_id.as_str()),
            "participant",
        );
        let agent_id = format!("standup-agent-{}-{}", index + 1, participant_slug);
        let node_id = format!("standup-participant-{}-{}", index + 1, participant_slug);
        let allowlist = {
            let mut tools = vec![
                "read".to_string(),
                "glob".to_string(),
                "grep".to_string(),
                "memory_search".to_string(),
            ];
            tools.extend(template.capabilities.tool_allowlist.clone());
            tools.sort();
            tools.dedup();
            tools
        };
        agents.push(crate::AutomationAgentProfile {
            agent_id: agent_id.clone(),
            template_id: Some(template.template_id.clone()),
            display_name: template
                .display_name
                .clone()
                .unwrap_or_else(|| template.template_id.clone()),
            avatar_url: template.avatar_url.clone(),
            model_policy: None,
            skills: template
                .skills
                .iter()
                .map(|skill| {
                    skill
                        .id
                        .clone()
                        .or_else(|| skill.path.clone())
                        .unwrap_or_default()
                })
                .filter(|value| !value.is_empty())
                .collect(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist,
                denylist: template.capabilities.tool_denylist.clone(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
            },
            approval_policy: None,
        });
        nodes.push(crate::AutomationFlowNode {
            node_id: node_id.clone(),
            agent_id,
            objective: standup_participant_objective(
                template
                    .display_name
                    .as_deref()
                    .unwrap_or(template.template_id.as_str()),
            ),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(crate::AutomationFlowOutputContract {
                kind: "structured_json".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: Some(json!({ "max_attempts": 2 })),
            timeout_ms: None,
            stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
            gate: None,
            metadata: None,
        });
        participant_node_ids.push(node_id);
    }

    let coordinator_agent_id = "standup-coordinator".to_string();
    agents.push(crate::AutomationAgentProfile {
        agent_id: coordinator_agent_id.clone(),
        template_id: None,
        display_name: "Standup Coordinator".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec![
                "read".to_string(),
                "write".to_string(),
                "memory_store".to_string(),
            ],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    });
    nodes.push(crate::AutomationFlowNode {
        node_id: "standup_synthesis".to_string(),
        agent_id: coordinator_agent_id,
        objective: standup_synthesis_objective(&report_path_template),
        depends_on: participant_node_ids.clone(),
        input_refs: participant_node_ids
            .iter()
            .map(|node_id| crate::AutomationFlowInputRef {
                from_step_id: node_id.clone(),
                alias: node_id.clone(),
            })
            .collect(),
        output_contract: Some(crate::AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: Some(json!({ "max_attempts": 2 })),
        timeout_ms: None,
        stage_kind: Some(crate::AutomationNodeStageKind::Orchestrator),
        gate: None,
        metadata: None,
    });

    let automation = crate::AutomationV2Spec {
        automation_id,
        name: name.to_string(),
        description: Some("Agent standup automation".to_string()),
        status: crate::AutomationV2Status::Draft,
        schedule: input.schedule,
        agents,
        flow: crate::AutomationFlowSpec { nodes },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(participant_node_ids.len().clamp(1, 16) as u32),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: vec![report_path_template.clone()],
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: "agent_standup".to_string(),
        workspace_root: Some(workspace_root.clone()),
        metadata: Some(json!({
            "feature": "agent_standup",
            "standup": {
                "participant_template_ids": participant_ids,
                "report_path_template": report_path_template,
                "timezone": schedule_timezone,
            },
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };

    Ok(Json(json!({
        "ok": true,
        "automation": automation,
    })))
}

pub(super) async fn agent_team_instances(
    State(state): State<AppState>,
    Query(query): Query<AgentTeamInstancesQuery>,
) -> Json<Value> {
    let instances = state
        .agent_teams
        .list_instances(
            query.mission_id.as_deref(),
            query.parent_instance_id.as_deref(),
            query.status,
        )
        .await;
    Json(json!({
        "instances": instances,
        "count": instances.len(),
    }))
}

pub(super) async fn agent_team_missions(State(state): State<AppState>) -> Json<Value> {
    let missions = state.agent_teams.list_mission_summaries().await;
    Json(json!({
        "missions": missions,
        "count": missions.len(),
    }))
}

pub(super) async fn agent_team_approvals(State(state): State<AppState>) -> Json<Value> {
    let spawn = state.agent_teams.list_spawn_approvals().await;
    let session_ids = state
        .agent_teams
        .list_instances(None, None, None)
        .await
        .into_iter()
        .map(|instance| instance.session_id)
        .collect::<std::collections::HashSet<_>>();
    let permissions = state
        .permissions
        .list()
        .await
        .into_iter()
        .filter(|req| {
            req.session_id
                .as_ref()
                .map(|sid| session_ids.contains(sid))
                .unwrap_or(false)
        })
        .map(|req| AgentTeamToolApprovalOutput {
            approval_id: req.id.clone(),
            session_id: req.session_id.clone(),
            tool_call_id: req.id,
            tool: req.tool,
            args: req.args,
            status: req.status,
        })
        .collect::<Vec<_>>();
    Json(json!({
        "spawnApprovals": spawn,
        "toolApprovals": permissions,
        "count": spawn.len() + permissions.len(),
    }))
}

pub(super) async fn agent_team_spawn(
    State(state): State<AppState>,
    Json(input): Json<AgentTeamSpawnInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let req = SpawnRequest {
        mission_id: input.mission_id.clone(),
        parent_instance_id: input.parent_instance_id.clone(),
        source: input.source.unwrap_or(SpawnSource::UiAction),
        parent_role: None,
        role: input.role,
        template_id: input.template_id.clone(),
        justification: input.justification.clone(),
        budget_override: input.budget_override,
    };
    emit_spawn_requested(&state, &req);
    let result = state.agent_teams.spawn(&state, req.clone()).await;
    if !result.decision.allowed || result.instance.is_none() {
        emit_spawn_denied(&state, &req, &result.decision);
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "ok": false,
                "code": result.decision.code,
                "error": result.decision.reason,
                "requiresUserApproval": result.decision.requires_user_approval,
            })),
        ));
    }
    let instance = result.instance.expect("checked is_some");
    emit_spawn_approved(&state, &req, &instance);
    Ok(Json(json!({
        "ok": true,
        "missionID": instance.mission_id,
        "instanceID": instance.instance_id,
        "sessionID": instance.session_id,
        "runID": instance.run_id,
        "status": instance.status,
        "skillHash": instance.skill_hash,
        "workspaceRoot": instance_metadata_field(&instance, "workspaceRoot"),
        "workspaceRepoRoot": instance_metadata_field(&instance, "workspaceRepoRoot"),
        "managedWorktree": instance_metadata_field(&instance, "managedWorktree"),
    })))
}

pub(super) async fn agent_team_approve_spawn(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AgentTeamCancelInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let reason = input
        .reason
        .unwrap_or_else(|| "approved by user".to_string());
    let Some(result) = state
        .agent_teams
        .approve_spawn_approval(&state, &id, Some(reason.as_str()))
        .await
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "code": "APPROVAL_NOT_FOUND",
                "error": "Spawn approval not found",
                "approvalID": id,
            })),
        ));
    };
    if !result.decision.allowed || result.instance.is_none() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({
                "ok": false,
                "code": result.decision.code,
                "error": result.decision.reason,
                "approvalID": id,
            })),
        ));
    }
    let instance = result.instance.expect("checked is_some");
    Ok(Json(json!({
        "ok": true,
        "approvalID": id,
        "decision": "approved",
        "instanceID": instance.instance_id,
        "sessionID": instance.session_id,
        "missionID": instance.mission_id,
        "status": instance.status,
        "workspaceRoot": instance_metadata_field(&instance, "workspaceRoot"),
        "workspaceRepoRoot": instance_metadata_field(&instance, "workspaceRepoRoot"),
        "managedWorktree": instance_metadata_field(&instance, "managedWorktree"),
    })))
}

pub(super) async fn agent_team_deny_spawn(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AgentTeamCancelInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let reason = input.reason.unwrap_or_else(|| "denied by user".to_string());
    let Some(approval) = state
        .agent_teams
        .deny_spawn_approval(&id, Some(reason.as_str()))
        .await
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "code": "APPROVAL_NOT_FOUND",
                "error": "Spawn approval not found",
                "approvalID": id,
            })),
        ));
    };
    let denied_decision = tandem_orchestrator::SpawnDecision {
        allowed: false,
        code: approval.decision_code,
        reason: Some(reason.clone()),
        requires_user_approval: false,
    };
    emit_spawn_denied(&state, &approval.request, &denied_decision);
    Ok(Json(json!({
        "ok": true,
        "approvalID": id,
        "decision": "denied",
        "reason": reason,
    })))
}

pub(super) async fn agent_team_cancel_instance(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AgentTeamCancelInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let reason = input
        .reason
        .unwrap_or_else(|| "cancelled by user".to_string());
    let Some(instance) = state
        .agent_teams
        .cancel_instance(&state, &id, &reason)
        .await
    else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "code": "INSTANCE_NOT_FOUND",
                "error": "Agent instance not found",
                "instanceID": id,
            })),
        ));
    };
    Ok(Json(json!({
        "ok": true,
        "instanceID": instance.instance_id,
        "sessionID": instance.session_id,
        "status": instance.status,
    })))
}

pub(super) async fn agent_team_cancel_mission(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AgentTeamCancelInput>,
) -> Json<Value> {
    let reason = input
        .reason
        .unwrap_or_else(|| "mission cancelled by user".to_string());
    let cancelled = state.agent_teams.cancel_mission(&state, &id, &reason).await;
    Json(json!({
        "ok": true,
        "missionID": id,
        "cancelledInstances": cancelled,
    }))
}
