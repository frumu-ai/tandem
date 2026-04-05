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
    AgentInstanceStatus, DefaultMissionReducer, KnowledgeBinding, MissionEvent, MissionReducer,
    MissionSpec, NoopMissionReducer, SpawnRequest, SpawnSource, WorkItem, WorkItemStatus,
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
pub(super) struct AgentTeamTemplatesQuery {
    #[serde(rename = "workspaceRoot", alias = "workspace_root")]
    pub workspace_root: Option<String>,
}

async fn global_template_workspace_root(state: &AppState) -> String {
    state.workspace_index.snapshot().await.root
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
pub(super) struct StandupComposeInput {
    pub name: String,
    pub workspace_root: String,
    pub schedule: crate::AutomationV2Schedule,
    pub participant_template_ids: Vec<String>,
    #[serde(default)]
    pub report_path_template: Option<String>,
    #[serde(default)]
    pub model_policy: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MonitorComposeInput {
    pub name: String,
    pub workspace_root: String,
    pub schedule: crate::AutomationV2Schedule,
    /// What the triage agent should check for (e.g. "urgent unread emails").
    pub triage_objective: String,
    /// MCP server names the triage agent is allowed to use.
    #[serde(default)]
    pub triage_mcp_servers: Vec<String>,
    /// What to do when the triage finds work.
    pub work_objective: String,
    /// Optional agent template for the work agent.
    #[serde(default)]
    pub work_agent_template_id: Option<String>,
    /// Optional model policy override for the triage agent.
    /// Defaults to the workspace default (cheapest available).
    #[serde(default)]
    pub triage_model_policy: Option<Value>,
}

/// Generic descriptor used by all `compose_*` handlers to build an `AutomationV2Spec`.
pub(super) struct ComposedAutomationDescriptor {
    pub automation_id: String,
    pub name: String,
    pub description: Option<String>,
    pub workspace_root: String,
    pub schedule: crate::AutomationV2Schedule,
    pub agents: Vec<crate::AutomationAgentProfile>,
    pub nodes: Vec<crate::AutomationFlowNode>,
    pub max_parallel_agents: u32,
    pub output_targets: Vec<String>,
    pub creator_id: String,
    pub metadata: Option<Value>,
}

/// Shared factory: turns a `ComposedAutomationDescriptor` into an `AutomationV2Spec`.
/// All `compose_*` handlers should use this instead of duplicating the boilerplate.
pub(super) fn build_composed_automation(
    desc: ComposedAutomationDescriptor,
) -> crate::AutomationV2Spec {
    let now = crate::now_ms();
    crate::AutomationV2Spec {
        automation_id: desc.automation_id,
        name: desc.name,
        description: desc.description,
        status: crate::AutomationV2Status::Draft,
        schedule: desc.schedule,
        knowledge: KnowledgeBinding::default(),
        agents: desc.agents,
        flow: crate::AutomationFlowSpec { nodes: desc.nodes },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(desc.max_parallel_agents.clamp(1, 16)),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: desc.output_targets,
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: desc.creator_id,
        workspace_root: Some(desc.workspace_root),
        metadata: desc.metadata,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    }
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
        "You are {template_name}. Report your actual progress for this daily standup.

YOUR TASK:
Find concrete evidence of work done since the previous standup and report it. Search the workspace and memory, then write a JSON update.

HOW TO SEARCH:
1. Use `glob` to list these workspace-root directories first: `outputs/`, `content/`, `drafts/`, `research/`, `docs/` (excluding `docs/standups/`). These are where actual deliverables land.
2. Use `read` on any files in those directories that look relevant to your role. Check file dates or filenames to determine if they are new since the last standup.
3. Use `memory_search` to recall prior conversations, completed tasks, or notes from the project history.
4. Read the previous standup at `docs/standups/` to understand what was already reported — do not repeat it.
5. DO NOT search `.tandem/` — that directory contains run history and audit artifacts, not deliverables.

OUTPUT RULES:
- `yesterday`: List only work that is new since the previous standup. Name specific files, drafts, or deliverables. NEVER write tool observations like \"reviewed workspace context\" or \"identified relevant docs\".
- `today`: Name a specific deliverable or task you will work on next. Do not write \"prepare the standup\" as today's work.
- `blockers`: List only actual business blockers. \"No memory found\" is NOT a blocker. \"Evidence limited\" is NOT a blocker. Leave this empty if there are none.

STATUS RULES (critical):
- Always return `status: completed` in the final JSON — even if you found nothing.
- NEVER return `status: blocked`. If you have nothing to report, say so plainly in `yesterday` and still return `status: completed`.
- The only valid status values for a standup participant are `completed` and `needs_repair`.

BAD (reject these patterns entirely):
- \"Reviewed workspace context and approved findings\"
- \"Identified Tandem positioning docs as source of truth\"
- \"No prior work evidence was found in memory_search\"
- \"The update remains evidence-limited without additional source material\"
- \"Prepared the daily standup update\"

GOOD (write things like this):
- \"Drafted homepage headline copy in outputs/homepage-copy.md\"
- \"Updated campaign brief at content/briefs/q2-campaign.md with new audience segment\"
- \"No {template_name} deliverables found in workspace.\"

If you find no concrete role-specific work after a thorough search, say exactly: \"No {template_name} deliverables found in workspace.\" Do not pad with process commentary.

Return valid JSON with keys `yesterday`, `today`, `blockers`, and `status`. Set `status` to `completed`."
    )
}

fn standup_participant_enforcement() -> crate::AutomationOutputEnforcement {
    // The standup_update validation profile (enforcement.rs) handles gate
    // construction: it requires concrete_reads but not workspace_inspection,
    // and does not impose Files Reviewed sections or citation requirements.
    crate::AutomationOutputEnforcement {
        validation_profile: Some("standup_update".to_string()),
        required_tools: vec![
            "read".to_string(),
            "glob".to_string(),
            "grep".to_string(),
            "memory_search".to_string(),
        ],
        required_evidence: vec!["local_source_reads".to_string()],
        required_sections: Vec::new(),
        prewrite_gates: Vec::new(), // populated by enforcement.rs for standup_update
        retry_on_missing: Vec::new(), // populated by enforcement.rs
        terminal_on: Vec::new(),    // populated by enforcement.rs
        repair_budget: Some(3),
        session_text_recovery: Some("require_prewrite_satisfied".to_string()),
    }
}

fn standup_synthesis_objective(report_path_template: &str) -> String {
    format!(
        "You are the standup coordinator. Synthesize participant updates into a useful daily standup that a human team lead would want to read.

EDITORIAL RULES (apply these strictly before writing anything):
1. If a participant's update contains only meta-commentary — phrases like \"reviewed workspace context\", \"identified relevant docs\", \"evidence-limited\", \"prepared the standup\", or \"no prior work evidence was found\" — replace the entire section with: `⚠️ No concrete progress reported for [role name].`
2. Any \"blocker\" that is a tool observation (\"no memory found\", \"evidence limited\", \"artifacts truncated\") must be silently dropped. Only real business blockers belong in the Blockers section.
3. Remove duplicate items that appear across multiple participants.
4. Do not mention tool names (`glob`, `grep`, `memory_search`, `read`) anywhere in the final report.
5. The final report must read as if a human wrote it. Every item must be a concrete deliverable, decision, or action — not a description of the agent's process.

REPORT FORMAT:
Write a markdown report with these sections:
## Yesterday
[Per-participant bullets of concrete work done]

## Today
[Per-participant bullets of specific next actions or deliverables]

## Blockers
[Only real business blockers — omit this section entirely if there are none]

Write the final markdown report to `{report_path_template}` relative to the workspace root using the `write` tool.

After writing the report, store a concise standup summary in project memory using `memory_store` with `tier: \"project\"`, source `agent_standup_summary`, and metadata that includes the report path.

Then return a short confirmation with the report path and participant count."
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

pub(super) async fn agent_team_templates(
    State(state): State<AppState>,
    Query(_query): Query<AgentTeamTemplatesQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let workspace_root = global_template_workspace_root(&state).await;
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

    let templates = state.agent_teams.list_templates().await;
    Ok(Json(json!({
        "templates": templates,
        "count": templates.len(),
    })))
}

pub(super) async fn agent_team_template_create(
    State(state): State<AppState>,
    Query(_query): Query<AgentTeamTemplatesQuery>,
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
    let workspace_root = global_template_workspace_root(&state).await;
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
    Query(_query): Query<AgentTeamTemplatesQuery>,
    Path(id): Path<String>,
    Json(input): Json<AgentTeamTemplatePatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let workspace_root = global_template_workspace_root(&state).await;
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
    Query(_query): Query<AgentTeamTemplatesQuery>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let workspace_root = global_template_workspace_root(&state).await;
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

pub(super) async fn compose_standup(
    State(state): State<AppState>,
    Json(input): Json<StandupComposeInput>,
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
    if let Some(model_policy) = input.model_policy.as_ref() {
        crate::http::routines_automations::validate_model_policy(model_policy).map_err(
            |detail| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "ok": false,
                        "code": "INVALID_MODEL_POLICY",
                        "error": detail,
                    })),
                )
            },
        )?;
    }
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
    let template_workspace_root = global_template_workspace_root(&state).await;
    state
        .agent_teams
        .ensure_loaded_for_workspace(&template_workspace_root)
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
        // Standup participants must be worker-style agents. Reviewer, Orchestrator, and
        // Delegator role templates break the standup output contract because their output
        // handlers expect approval decisions rather than yesterday/today/blockers JSON.
        let role_name = format!("{:?}", template.role).to_ascii_lowercase();
        if matches!(
            role_name.as_str(),
            "reviewer" | "orchestrator" | "delegator"
        ) {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "ok": false,
                    "code": "INVALID_PARTICIPANT_ROLE",
                    "error": format!(
                        "template `{template_id}` has role `{role_name}` which cannot be a standup participant. Only Worker, Watcher, Tester, and Committer roles are supported."
                    ),
                    "templateID": template_id,
                    "role": role_name,
                })),
            ));
        }
        participants.push(template);
    }

    let automation_id = format!("standup-{}", Uuid::new_v4());
    let schedule_timezone = input.schedule.timezone.clone();
    let standup_model_policy = input.model_policy.clone();
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
            model_policy: standup_model_policy.clone(),
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
            knowledge: KnowledgeBinding::default(),
            depends_on: Vec::new(),
            input_refs: Vec::new(),
            output_contract: Some(crate::AutomationFlowOutputContract {
                kind: "standup_update".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::StandupUpdate),
                enforcement: Some(standup_participant_enforcement()),
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
        model_policy: standup_model_policy.clone(),
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
        knowledge: KnowledgeBinding::default(),
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
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: Some(json!({ "max_attempts": 2 })),
        timeout_ms: None,
        stage_kind: Some(crate::AutomationNodeStageKind::Orchestrator),
        gate: None,
        metadata: None,
    });

    let automation = build_composed_automation(ComposedAutomationDescriptor {
        automation_id,
        name: name.to_string(),
        description: Some("Agent standup automation".to_string()),
        workspace_root: workspace_root.clone(),
        schedule: input.schedule,
        agents,
        nodes,
        max_parallel_agents: participant_node_ids.len().clamp(1, 16) as u32,
        output_targets: vec![report_path_template.clone()],
        creator_id: "agent_standup".to_string(),
        metadata: Some(json!({
            "feature": "agent_standup",
            "standup": {
                "participant_template_ids": participant_ids,
                "report_path_template": report_path_template,
                "timezone": schedule_timezone,
            },
        })),
    });

    Ok(Json(json!({
        "ok": true,
        "automation": automation,
    })))
}

pub(super) async fn compose_monitor(
    State(_state): State<AppState>,
    Json(input): Json<MonitorComposeInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let name = input.name.trim();
    if name.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "code": "INVALID_MONITOR_NAME",
                "error": "name is required",
            })),
        ));
    }
    let triage_objective = input.triage_objective.trim();
    if triage_objective.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "code": "INVALID_TRIAGE_OBJECTIVE",
                "error": "triage_objective is required",
            })),
        ));
    }
    let work_objective = input.work_objective.trim();
    if work_objective.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "code": "INVALID_WORK_OBJECTIVE",
                "error": "work_objective is required",
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

    let automation_id = format!("monitor-{}", uuid::Uuid::new_v4());

    // ── Triage agent ──────────────────────────────────────────────────────────
    // Uses the cheapest/default model via triage_model_policy (or workspace default).
    let triage_agent_id = "monitor-triage".to_string();
    let triage_node_id = "monitor_triage".to_string();

    let triage_prompt = format!(
        "{triage_objective}\n\n\
        Use available MCP tools to check the data sources.\n\
        Respond ONLY with valid JSON matching this schema:\n\
        {{\"has_work\": true/false, \"summary\": \"...\", \"items\": [...]}}"
    );

    let triage_agent = crate::AutomationAgentProfile {
        agent_id: triage_agent_id.clone(),
        template_id: None,
        display_name: "Monitor Triage".to_string(),
        avatar_url: None,
        // triage_model_policy is set by the caller; None falls back to workspace default.
        model_policy: input.triage_model_policy.clone(),
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            // Triage only needs to call MCP tools — no workspace writes.
            allowlist: Vec::new(),
            denylist: vec!["write".to_string()],
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: input.triage_mcp_servers.clone(),
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let triage_node = crate::AutomationFlowNode {
        node_id: triage_node_id.clone(),
        agent_id: triage_agent_id,
        objective: triage_prompt,
        knowledge: KnowledgeBinding::default(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(crate::AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: Some(
                "Return {\"has_work\": bool, \"summary\": string, \"items\": array}".to_string(),
            ),
        }),
        retry_policy: Some(json!({ "max_attempts": 2 })),
        timeout_ms: None,
        stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
        gate: None,
        // triage_gate: true signals the executor to skip downstream nodes when
        // the output contains has_work: false.
        metadata: Some(json!({ "triage_gate": true })),
    };

    // ── Work agent ────────────────────────────────────────────────────────────
    let work_agent_id = "monitor-worker".to_string();
    let work_node_id = "monitor_work".to_string();

    let work_agent = crate::AutomationAgentProfile {
        agent_id: work_agent_id.clone(),
        template_id: input.work_agent_template_id.clone(),
        display_name: "Monitor Worker".to_string(),
        avatar_url: None,
        model_policy: None, // uses workspace default (full model)
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: Vec::new(),
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            // Worker can access all MCP servers the user has allowed.
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let work_node = crate::AutomationFlowNode {
        node_id: work_node_id.clone(),
        agent_id: work_agent_id,
        objective: work_objective.to_string(),
        knowledge: KnowledgeBinding::default(),
        depends_on: vec![triage_node_id.clone()],
        input_refs: vec![crate::AutomationFlowInputRef {
            from_step_id: triage_node_id.clone(),
            alias: "triage".to_string(),
        }],
        output_contract: Some(crate::AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: Some(json!({ "max_attempts": 2 })),
        timeout_ms: None,
        stage_kind: Some(crate::AutomationNodeStageKind::Orchestrator),
        gate: None,
        metadata: None,
    };

    let automation = build_composed_automation(ComposedAutomationDescriptor {
        automation_id,
        name: name.to_string(),
        description: Some(format!("Monitor: {triage_objective}")),
        workspace_root,
        schedule: input.schedule,
        agents: vec![triage_agent, work_agent],
        nodes: vec![triage_node, work_node],
        max_parallel_agents: 1,
        output_targets: Vec::new(),
        creator_id: "compose_monitor".to_string(),
        metadata: Some(json!({
            "feature": "monitor",
            "monitor": {
                "triage_objective": triage_objective,
                "work_objective": work_objective,
                "triage_mcp_servers": input.triage_mcp_servers,
            },
        })),
    });

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
