use async_trait::async_trait;
use axum::response::IntoResponse;
use tandem_memory::types::{DistilledFact, MemoryResult};
use tandem_plan_compiler::api as compiler_api;
use tandem_plan_compiler::api::schedule_from_value;
use tandem_skills::SkillContent;

#[derive(Debug, Deserialize)]
pub(super) struct SkillLocationQuery {
    location: Option<SkillLocation>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SkillsImportRequest {
    content: Option<String>,
    file_or_path: Option<String>,
    location: SkillLocation,
    namespace: Option<String>,
    conflict_policy: Option<SkillsConflictPolicy>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SkillsTemplateInstallRequest {
    location: SkillLocation,
}

#[derive(Debug, Deserialize)]
pub(super) struct SkillsValidateRequest {
    content: Option<String>,
    file_or_path: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct SkillsRouterMatchRequest {
    goal: Option<String>,
    max_matches: Option<usize>,
    threshold: Option<f64>,
    #[serde(default)]
    context_run_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct SkillsCompileRequest {
    skill_name: Option<String>,
    goal: Option<String>,
    threshold: Option<f64>,
    max_matches: Option<usize>,
    schedule: Option<Value>,
    #[serde(default)]
    context_run_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct SkillsGenerateRequest {
    prompt: Option<String>,
    threshold: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct SkillsGenerateInstallRequest {
    prompt: Option<String>,
    threshold: Option<f64>,
    location: Option<SkillLocation>,
    conflict_policy: Option<SkillsConflictPolicy>,
    artifacts: Option<SkillsGenerateArtifactsInput>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct SkillsGenerateArtifactsInput {
    #[serde(rename = "SKILL.md")]
    skill_md: Option<String>,
    #[serde(rename = "workflow.yaml")]
    workflow_yaml: Option<String>,
    #[serde(rename = "automation.example.yaml")]
    automation_example_yaml: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct SkillEvalCaseInput {
    prompt: Option<String>,
    expected_skill: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct SkillsEvalBenchmarkRequest {
    cases: Option<Vec<SkillEvalCaseInput>>,
    threshold: Option<f64>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct SkillsEvalTriggersRequest {
    skill_name: Option<String>,
    prompts: Option<Vec<String>>,
    threshold: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MemoryPutInput {
    #[serde(flatten)]
    request: MemoryPutRequest,
    capability: Option<MemoryCapabilityToken>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MemoryPromoteInput {
    #[serde(flatten)]
    request: MemoryPromoteRequest,
    capability: Option<MemoryCapabilityToken>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MemoryDemoteInput {
    id: String,
    run_id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct MemorySearchInput {
    #[serde(flatten)]
    request: MemorySearchRequest,
    capability: Option<MemoryCapabilityToken>,
}

#[derive(Debug, Deserialize)]
pub(super) struct MemoryImportPathSourceInput {
    kind: String,
    path: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct MemoryImportInput {
    source: MemoryImportPathSourceInput,
    #[serde(default = "default_memory_import_format")]
    format: MemoryImportFormat,
    #[serde(default = "default_memory_import_tier")]
    tier: MemoryTier,
    project_id: Option<String>,
    session_id: Option<String>,
    #[serde(default)]
    sync_deletes: bool,
}

#[derive(Debug, Serialize)]
pub(super) struct MemoryImportPathSourceResponse {
    kind: &'static str,
    path: String,
}

#[derive(Debug, Serialize)]
pub(super) struct MemoryImportResponse {
    ok: bool,
    source: MemoryImportPathSourceResponse,
    format: MemoryImportFormat,
    tier: MemoryTier,
    project_id: Option<String>,
    session_id: Option<String>,
    sync_deletes: bool,
    discovered_files: usize,
    files_processed: usize,
    indexed_files: usize,
    skipped_files: usize,
    deleted_files: usize,
    chunks_created: usize,
    errors: usize,
}

fn default_memory_import_format() -> MemoryImportFormat {
    MemoryImportFormat::Directory
}

fn default_memory_import_tier() -> MemoryTier {
    MemoryTier::Project
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct MemoryAuditQuery {
    run_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct MemoryListQuery {
    q: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
    user_id: Option<String>,
    project_id: Option<String>,
    channel_tag: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct MemoryDeleteQuery {
    project_id: Option<String>,
    channel_tag: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowLearningCandidateListQuery {
    workflow_id: Option<String>,
    project_id: Option<String>,
    status: Option<String>,
    kind: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowLearningCandidateReviewRequest {
    action: Option<String>,
    reviewer_id: Option<String>,
    note: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowLearningCandidatePromoteRequest {
    reviewer_id: Option<String>,
    approval_id: Option<String>,
    run_id: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowLearningCandidateSpawnRevisionRequest {
    reviewer_id: Option<String>,
    title: Option<String>,
}

fn tenant_context_event_value(tenant_context: &TenantContext) -> Value {
    serde_json::to_value(tenant_context).unwrap_or_else(|_| json!(tenant_context))
}

fn with_tenant_context(mut properties: Value, tenant_context: &TenantContext) -> Value {
    if let Some(map) = properties.as_object_mut() {
        map.insert(
            "tenantContext".to_string(),
            tenant_context_event_value(tenant_context),
        );
    }
    properties
}

fn publish_tenant_event(
    state: &AppState,
    tenant_context: &TenantContext,
    event_type: &str,
    properties: Value,
) {
    state.event_bus.publish(EngineEvent::new(
        event_type,
        with_tenant_context(properties, tenant_context),
    ));
}

fn event_tenant_context(event: &EngineEvent) -> Option<TenantContext> {
    event
        .properties
        .get("tenantContext")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
}

fn record_tenant_context(record: &GlobalMemoryRecord) -> TenantContext {
    record
        .provenance
        .as_ref()
        .and_then(|value| value.get("tenant_context").cloned())
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

pub(super) fn skills_service() -> SkillService {
    SkillService::for_workspace(std::env::current_dir().ok())
}

pub(super) fn skill_error(
    status: StatusCode,
    message: impl Into<String>,
) -> (StatusCode, Json<ErrorEnvelope>) {
    (
        status,
        Json(ErrorEnvelope {
            error: message.into(),
            code: Some("skills_error".to_string()),
        }),
    )
}

pub(super) async fn ensure_skill_router_context_run(
    state: &AppState,
    run_id: &str,
    goal: Option<&str>,
) -> Result<(), StatusCode> {
    if load_context_run_state(state, run_id).await.is_ok() {
        return Ok(());
    }
    let now = crate::now_ms();
    let run = ContextRunState {
        run_id: run_id.to_string(),
        run_type: "skill_router".to_string(),
        tenant_context: TenantContext::local_implicit(),
        source_client: Some("skills_api".to_string()),
        model_provider: None,
        model_id: None,
        mcp_servers: Vec::new(),
        status: ContextRunStatus::Running,
        objective: goal
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| "Skill routing workflow".to_string()),
        workspace: ContextWorkspaceLease::default(),
        steps: Vec::new(),
        tasks: Vec::new(),
        why_next_step: Some("Resolve skill workflow from user goal".to_string()),
        revision: 1,
        last_event_seq: 0,
        created_at_ms: now,
        started_at_ms: Some(now),
        ended_at_ms: None,
        last_error: None,
        updated_at_ms: now,
    };
    save_context_run_state(state, &run).await
}

pub(super) async fn emit_skill_router_task(
    state: &AppState,
    run_id: &str,
    task_id: &str,
    task_type: &str,
    task_payload: Value,
    status: ContextBlackboardTaskStatus,
) -> Result<(), StatusCode> {
    let run = load_context_run_state(state, run_id).await?;
    let existing = run.tasks.iter().find(|row| row.id == task_id).cloned();
    let now = crate::now_ms();

    if existing.is_none() {
        let task = ContextBlackboardTask {
            id: task_id.to_string(),
            task_type: task_type.to_string(),
            payload: task_payload.clone(),
            status: ContextBlackboardTaskStatus::Pending,
            workflow_id: Some("skill_router".to_string()),
            workflow_node_id: Some(task_type.to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            assigned_agent: Some("skill_router".to_string()),
            priority: 0,
            attempt: 0,
            max_attempts: 1,
            last_error: None,
            next_retry_at_ms: None,
            lease_owner: None,
            lease_token: None,
            lease_expires_at_ms: None,
            task_rev: 1,
            created_ts: now,
            updated_ts: now,
        };
        let _ = context_run_engine()
            .commit_task_mutation(
                state,
                run_id,
                task.clone(),
                ContextBlackboardPatchOp::AddTask,
                serde_json::to_value(&task).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
                "context.task.created".to_string(),
                ContextRunStatus::Running,
                None,
                json!({
                    "task_id": task_id,
                    "task_type": task_type,
                    "task_rev": task.task_rev,
                    "source": "skill_router",
                }),
            )
            .await?;
    }

    let current = load_context_run_state(state, run_id)
        .await?
        .tasks
        .into_iter()
        .find(|row| row.id == task_id);
    let next_rev = current
        .as_ref()
        .map(|row| row.task_rev.saturating_add(1))
        .unwrap_or(1);
    let next_task = ContextBlackboardTask {
        status: status.clone(),
        assigned_agent: Some("skill_router".to_string()),
        last_error: None,
        task_rev: next_rev,
        updated_ts: now,
        ..current.unwrap_or(ContextBlackboardTask {
            id: task_id.to_string(),
            task_type: task_type.to_string(),
            payload: task_payload.clone(),
            status: ContextBlackboardTaskStatus::Pending,
            workflow_id: Some("skill_router".to_string()),
            workflow_node_id: Some(task_type.to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            assigned_agent: Some("skill_router".to_string()),
            priority: 0,
            attempt: 0,
            max_attempts: 1,
            last_error: None,
            next_retry_at_ms: None,
            lease_owner: None,
            lease_token: None,
            lease_expires_at_ms: None,
            task_rev: 1,
            created_ts: now,
            updated_ts: now,
        })
    };
    let _ = context_run_engine()
        .commit_task_mutation(
            state,
            run_id,
            next_task,
            ContextBlackboardPatchOp::UpdateTaskState,
            json!({
                "task_id": task_id,
                "status": status,
                "assigned_agent": "skill_router",
                "task_rev": next_rev,
                "error": Value::Null,
            }),
            context_task_status_event_name(&status).to_string(),
            ContextRunStatus::Running,
            None,
            json!({
                "task_id": task_id,
                "status": status,
                "task_rev": next_rev,
                "source": "skill_router",
            }),
        )
        .await?;
    Ok(())
}

pub(super) async fn skills_list() -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let service = skills_service();
    let skills = service
        .list_skills()
        .map_err(|e| skill_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!(skills)))
}

pub(super) async fn skills_catalog() -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let service = skills_service();
    let skills = service
        .list_catalog()
        .map_err(|e| skill_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!(skills)))
}

pub(super) async fn skills_get(
    Path(name): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let service = skills_service();
    let loaded = service
        .load_skill(&name)
        .map_err(|e| skill_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let Some(skill) = loaded else {
        return Err(skill_error(
            StatusCode::NOT_FOUND,
            format!("Skill '{}' not found", name),
        ));
    };
    Ok(Json(json!(skill)))
}

pub(super) async fn skills_import_preview(
    Json(input): Json<SkillsImportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let service = skills_service();
    let file_or_path = input.file_or_path.ok_or_else(|| {
        skill_error(
            StatusCode::BAD_REQUEST,
            "Missing file_or_path for /skills/import/preview",
        )
    })?;
    let preview = service
        .skills_import_preview(
            &file_or_path,
            input.location,
            input.namespace,
            input.conflict_policy.unwrap_or(SkillsConflictPolicy::Skip),
        )
        .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!(preview)))
}

pub(super) async fn skills_import(
    Json(input): Json<SkillsImportRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let service = skills_service();
    if let Some(content) = input.content {
        let skill = service
            .import_skill_from_content(&content, input.location)
            .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
        return Ok(Json(json!(skill)));
    }
    let file_or_path = input.file_or_path.ok_or_else(|| {
        skill_error(
            StatusCode::BAD_REQUEST,
            "Missing content or file_or_path for /skills/import",
        )
    })?;
    let result = service
        .skills_import(
            &file_or_path,
            input.location,
            input.namespace,
            input.conflict_policy.unwrap_or(SkillsConflictPolicy::Skip),
        )
        .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!(result)))
}

pub(super) async fn skills_validate(
    Json(input): Json<SkillsValidateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let service = skills_service();
    let report = service
        .validate_skill_source(input.content.as_deref(), input.file_or_path.as_deref())
        .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!(report)))
}

pub(super) async fn skills_router_match(
    State(state): State<AppState>,
    Json(input): Json<SkillsRouterMatchRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let goal = input.goal.unwrap_or_default();
    if goal.trim().is_empty() {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "Missing non-empty goal for /skills/router/match",
        ));
    }
    let max_matches = input.max_matches.unwrap_or(3).clamp(1, 10);
    let threshold = input.threshold.unwrap_or(0.35).clamp(0.0, 1.0);
    let service = skills_service();
    let result = service
        .route_skill_match(&goal, max_matches, threshold)
        .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
    let payload = json!(result);
    if let Some(run_id) = sanitize_context_id(input.context_run_id.as_deref()) {
        let _ = ensure_skill_router_context_run(&state, &run_id, Some(goal.as_str())).await;
        let digest = Sha256::digest(goal.as_bytes());
        let task_id = format!("skill-router-match-{:x}", digest);
        let task_status = if payload
            .get("skill_name")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .is_some()
        {
            ContextBlackboardTaskStatus::Done
        } else {
            ContextBlackboardTaskStatus::Blocked
        };
        let _ = emit_skill_router_task(
            &state,
            &run_id,
            &task_id[..task_id.len().min(30)],
            "skill_router.match",
            json!({
                "title": "Skill Router Match",
                "goal": goal,
                "result": payload.clone(),
            }),
            task_status,
        )
        .await;
    }
    Ok(Json(payload))
}

pub(super) fn detect_skill_workflow_kind(base_dir: &str) -> Option<String> {
    let workflow_path = PathBuf::from(base_dir).join("workflow.yaml");
    let raw = std::fs::read_to_string(&workflow_path).ok()?;
    let parsed = serde_yaml::from_str::<serde_yaml::Value>(&raw).ok()?;
    parsed
        .get("kind")
        .and_then(|v| v.as_str())
        .map(|v| v.to_string())
}

#[derive(Debug, Deserialize)]
struct SkillWorkflowRecipe {
    kind: String,
    #[serde(default)]
    skill_id: Option<String>,
    #[serde(default)]
    execution_mode: Option<String>,
    #[serde(default)]
    goal_template: Option<String>,
}

fn load_skill_workflow_recipe(base_dir: &str) -> Option<SkillWorkflowRecipe> {
    let workflow_path = PathBuf::from(base_dir).join("workflow.yaml");
    let raw = std::fs::read_to_string(&workflow_path).ok()?;
    serde_yaml::from_str::<SkillWorkflowRecipe>(&raw).ok()
}

fn compile_skill_workflow_plan(
    skill: &SkillContent,
    recipe: &SkillWorkflowRecipe,
    goal: Option<&str>,
    schedule: Option<&Value>,
) -> crate::WorkflowPlan {
    let now = crate::now_ms();
    let normalized_goal = goal
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            recipe
                .goal_template
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
        })
        .unwrap_or_else(|| skill.info.description.clone());
    let schedule = schedule
        .and_then(|value| schedule_from_value(value, crate::RoutineMisfirePolicy::RunOnce))
        .unwrap_or(crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        });
    let execution_mode = recipe
        .execution_mode
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("single");
    let skill_ref = recipe
        .skill_id
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(skill.info.name.as_str());
    let agent_role = match execution_mode {
        "team" => "specialist",
        "swarm" => "researcher",
        _ => "worker",
    };
    let output_contract = match recipe.kind.as_str() {
        "pack_builder_recipe" => Some(crate::AutomationFlowOutputContract {
            kind: "generic_artifact".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        "automation_v2_dag" => None,
        _ => None,
    };
    crate::WorkflowPlan {
        plan_id: format!("skill-plan-{}-{now}", skill.info.name),
        planner_version: "skills_compile_v1".to_string(),
        plan_source: "skills_compile".to_string(),
        original_prompt: normalized_goal.clone(),
        normalized_prompt: normalized_goal.clone(),
        confidence: "high".to_string(),
        title: format!("{} Workflow", skill.info.name.replace('-', " ")),
        description: Some(format!(
            "Compiled from skill `{}` workflow `{}`.",
            skill.info.name, recipe.kind
        )),
        schedule,
        execution_target: "automation_v2".to_string(),
        workspace_root: std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .to_string_lossy()
            .to_string(),
        steps: vec![crate::WorkflowPlanStep {
            step_id: "run_skill".to_string(),
            kind: recipe.kind.clone(),
            objective: format!(
                "Use the `{skill_ref}` skill to complete this goal: {normalized_goal}"
            ),
            depends_on: Vec::new(),
            agent_role: agent_role.to_string(),
            input_refs: Vec::new(),
            output_contract,
            metadata: None,
        }],
        requires_integrations: Vec::new(),
        allowed_mcp_servers: Vec::new(),
        operator_preferences: Some(json!({
            "execution_mode": execution_mode,
            "tool_access_mode": "auto",
            "skill_ref": skill_ref,
            "workflow_kind": recipe.kind,
            "source": "skills_compile",
        })),
        save_options: json!({
            "origin": "skills_compile",
            "workflow_kind": recipe.kind,
        }),
    }
}

pub(super) fn slugify_skill_name(input: &str) -> String {
    let cleaned = input
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == ' ' || c == '-' || c == '_' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>();
    let mut out = cleaned
        .split_whitespace()
        .take(6)
        .collect::<Vec<_>>()
        .join("-");
    if out.is_empty() {
        out = "generated-skill".to_string();
    }
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').to_string()
}

#[derive(Debug, Clone)]
pub(super) struct GeneratedSkillScaffold {
    router: Value,
    artifacts: SkillBundleArtifacts,
}

pub(super) fn generate_skill_scaffold(
    service: &SkillService,
    prompt: &str,
    threshold: f64,
) -> Result<GeneratedSkillScaffold, String> {
    let routed = service.route_skill_match(prompt, 3, threshold)?;
    let suggested_name = routed
        .skill_name
        .clone()
        .unwrap_or_else(|| slugify_skill_name(prompt));
    let skill_md = format!(
        "---\nname: {name}\ndescription: Generated from prompt.\nversion: 0.1.0\n---\n\n# Skill: {title}\n\n## Purpose\n{purpose}\n\n## Inputs\n- user prompt\n\n## Agents\n- worker\n\n## Tools\n- webfetch\n\n## Workflow\n1. Interpret user intent\n2. Execute workflow steps\n3. Return result\n\n## Outputs\n- completed task result\n\n## Schedule compatibility\n- manual\n",
        name = suggested_name,
        title = suggested_name.replace('-', " "),
        purpose = prompt.trim()
    );
    let workflow_yaml = if suggested_name == "dev-agent" {
        "kind: automation_v2_dag\nskill_id: dev-agent\n".to_string()
    } else {
        format!(
            "kind: pack_builder_recipe\nskill_id: {}\nexecution_mode: team\ngoal_template: \"{}\"\n",
            suggested_name,
            prompt.replace('"', "'")
        )
    };
    let automation_example = format!(
        "name: {}\nschedule:\n  type: manual\n  timezone: user_local\ninputs:\n  prompt: \"{}\"\n",
        suggested_name.replace('-', " "),
        prompt.replace('"', "'")
    );
    Ok(GeneratedSkillScaffold {
        router: json!(routed),
        artifacts: SkillBundleArtifacts {
            skill_md,
            workflow_yaml: Some(workflow_yaml),
            automation_example_yaml: Some(automation_example),
        },
    })
}

pub(super) async fn skills_compile(
    State(state): State<AppState>,
    Json(input): Json<SkillsCompileRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let service = skills_service();
    let threshold = input.threshold.unwrap_or(0.35).clamp(0.0, 1.0);
    let max_matches = input.max_matches.unwrap_or(3).clamp(1, 10);
    let goal_for_bb = input.goal.clone();
    let context_run_for_bb = input.context_run_id.clone();

    let resolved_skill = if let Some(name) = input
        .skill_name
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        Some(name.to_string())
    } else if let Some(goal) = input.goal.as_deref() {
        let routed = service
            .route_skill_match(goal, max_matches, threshold)
            .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
        routed.skill_name
    } else {
        None
    };

    let Some(skill_name) = resolved_skill else {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "Missing skill_name and no routeable goal provided",
        ));
    };

    let loaded = service
        .load_skill(&skill_name)
        .map_err(|e| skill_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    let Some(skill) = loaded else {
        return Err(skill_error(
            StatusCode::NOT_FOUND,
            format!("Skill '{}' not found", skill_name),
        ));
    };
    let validation = service
        .validate_skill_source(Some(&skill.content), None)
        .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
    let workflow_kind = detect_skill_workflow_kind(&skill.base_dir)
        .unwrap_or_else(|| "pack_builder_recipe".to_string());
    let automation_preview = load_skill_workflow_recipe(&skill.base_dir).map(|recipe| {
        let mut automation = super::compile_plan_to_automation_v2(
            &compile_skill_workflow_plan(
                &skill,
                &recipe,
                input.goal.as_deref(),
                input.schedule.as_ref(),
            ),
            None,
            "skills_compile",
        );
        if let Some(agent) = automation.agents.first_mut() {
            agent.skills = vec![skill.info.name.clone()];
            if recipe.kind == "pack_builder_recipe" {
                agent.tool_policy.allowlist = vec!["*".to_string()];
            }
        }
        if let Some(metadata) = automation.metadata.as_mut().and_then(Value::as_object_mut) {
            metadata.insert("skill_name".to_string(), json!(skill.info.name));
            metadata.insert("skill_path".to_string(), json!(skill.info.path));
            metadata.insert("skill_workflow_kind".to_string(), json!(recipe.kind));
            metadata.insert(
                "skill_goal_template".to_string(),
                json!(recipe.goal_template),
            );
            metadata.insert(
                "skill_execution_mode".to_string(),
                json!(recipe.execution_mode),
            );
        }
        automation
    });

    let execution_plan = json!({
        "workflow_kind": workflow_kind,
        "goal": input.goal,
        "schedule": input.schedule,
        "default_action": if automation_preview.is_some() || workflow_kind == "automation_v2_dag" {
            "create_automation_v2"
        } else {
            "pack_builder_preview"
        }
    });

    let response = json!({
        "skill_name": skill.info.name,
        "workflow_kind": execution_plan.get("workflow_kind"),
        "validation": validation,
        "automation_preview": automation_preview,
        "execution_plan": execution_plan,
        "status": "compiled"
    });
    if let Some(run_id) = sanitize_context_id(context_run_for_bb.as_deref()) {
        let _ = ensure_skill_router_context_run(&state, &run_id, goal_for_bb.as_deref()).await;
        let task_id = format!("skill-router-compile-{skill_name}");
        let _ = emit_skill_router_task(
            &state,
            &run_id,
            &task_id.replace([' ', '/', ':'], "-"),
            "skill_router.compile",
            json!({
                "title": format!("Compile Skill {skill_name}"),
                "goal": goal_for_bb,
                "result": response.clone(),
            }),
            ContextBlackboardTaskStatus::Done,
        )
        .await;
    }
    Ok(Json(response))
}

pub(super) async fn skills_generate(
    Json(input): Json<SkillsGenerateRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let prompt = input.prompt.unwrap_or_default();
    if prompt.trim().is_empty() {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "Missing prompt for /skills/generate",
        ));
    }
    let service = skills_service();
    let threshold = input.threshold.unwrap_or(0.35).clamp(0.0, 1.0);
    let scaffold = generate_skill_scaffold(&service, &prompt, threshold)
        .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({
        "status": "generated_scaffold",
        "prompt": prompt,
        "router": scaffold.router,
        "artifacts": {
            "SKILL.md": scaffold.artifacts.skill_md,
            "workflow.yaml": scaffold.artifacts.workflow_yaml,
            "automation.example.yaml": scaffold.artifacts.automation_example_yaml
        }
    })))
}

pub(super) async fn skills_generate_install(
    Json(input): Json<SkillsGenerateInstallRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let location = input.location.unwrap_or(SkillLocation::Project);
    let conflict_policy = input.conflict_policy.unwrap_or(SkillsConflictPolicy::Skip);
    let service = skills_service();
    let artifacts = if let Some(raw) = input.artifacts {
        let skill_md = raw.skill_md.unwrap_or_default();
        if skill_md.trim().is_empty() {
            return Err(skill_error(
                StatusCode::BAD_REQUEST,
                "artifacts.SKILL.md is required when artifacts are provided",
            ));
        }
        SkillBundleArtifacts {
            skill_md,
            workflow_yaml: raw.workflow_yaml,
            automation_example_yaml: raw.automation_example_yaml,
        }
    } else {
        let prompt = input.prompt.unwrap_or_default();
        if prompt.trim().is_empty() {
            return Err(skill_error(
                StatusCode::BAD_REQUEST,
                "Missing prompt or artifacts for /skills/generate/install",
            ));
        }
        let threshold = input.threshold.unwrap_or(0.35).clamp(0.0, 1.0);
        generate_skill_scaffold(&service, &prompt, threshold)
            .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?
            .artifacts
    };
    let validation = service
        .validate_skill_source(Some(&artifacts.skill_md), None)
        .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
    if validation.invalid > 0 {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "Generated skill did not pass SKILL.md validation",
        ));
    }
    let installed = service
        .install_skill_bundle(artifacts, location, conflict_policy)
        .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({
        "status": "installed",
        "skill": installed,
        "validation": validation
    })))
}

pub(super) async fn skills_eval_benchmark(
    Json(input): Json<SkillsEvalBenchmarkRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let threshold = input.threshold.unwrap_or(0.35).clamp(0.0, 1.0);
    let cases = input.cases.unwrap_or_default();
    if cases.is_empty() {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "At least one eval case is required",
        ));
    }
    let service = skills_service();
    let mut evaluated = Vec::<Value>::new();
    let mut pass_count = 0usize;
    for (idx, case) in cases.iter().enumerate() {
        let prompt = case.prompt.clone().unwrap_or_default();
        if prompt.trim().is_empty() {
            evaluated.push(json!({
                "index": idx,
                "prompt": prompt,
                "passed": false,
                "error": "empty_prompt"
            }));
            continue;
        }
        let routed = service
            .route_skill_match(&prompt, 1, threshold)
            .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
        let matched_skill = routed.skill_name.clone();
        let expected = case.expected_skill.clone();
        let passed = match (expected.as_deref(), matched_skill.as_deref()) {
            (Some(exp), Some(actual)) => exp == actual,
            (Some(_), None) => false,
            (None, Some(_)) => true,
            (None, None) => routed.decision == "no_match",
        };
        if passed {
            pass_count += 1;
        }
        evaluated.push(json!({
            "index": idx,
            "prompt": prompt,
            "expected_skill": expected,
            "matched_skill": matched_skill,
            "decision": routed.decision,
            "confidence": routed.confidence,
            "passed": passed,
            "reason": routed.reason
        }));
    }
    let total = evaluated.len();
    let accuracy = if total == 0 {
        0.0
    } else {
        pass_count as f64 / total as f64
    };
    Ok(Json(json!({
        "status": "scaffold",
        "total": total,
        "passed": pass_count,
        "failed": total.saturating_sub(pass_count),
        "accuracy": accuracy,
        "threshold": threshold,
        "cases": evaluated,
    })))
}

pub(super) async fn skills_eval_triggers(
    Json(input): Json<SkillsEvalTriggersRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let skill_name = input.skill_name.unwrap_or_default();
    if skill_name.trim().is_empty() {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "Missing skill_name for trigger evaluation",
        ));
    }
    let prompts = input.prompts.unwrap_or_default();
    if prompts.is_empty() {
        return Err(skill_error(
            StatusCode::BAD_REQUEST,
            "At least one prompt is required for trigger evaluation",
        ));
    }
    let threshold = input.threshold.unwrap_or(0.35).clamp(0.0, 1.0);
    let service = skills_service();
    let mut true_positive = 0usize;
    let mut false_negative = 0usize;
    let mut rows = Vec::<Value>::new();
    for (idx, prompt) in prompts.iter().enumerate() {
        let routed = service
            .route_skill_match(prompt, 1, threshold)
            .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
        let matched = routed
            .skill_name
            .as_deref()
            .map(|v| v == skill_name)
            .unwrap_or(false);
        if matched {
            true_positive += 1;
        } else {
            false_negative += 1;
        }
        rows.push(json!({
            "index": idx,
            "prompt": prompt,
            "decision": routed.decision,
            "matched_skill": routed.skill_name,
            "confidence": routed.confidence,
            "reason": routed.reason,
            "is_expected_skill": matched
        }));
    }
    let total = prompts.len();
    let recall = if total == 0 {
        0.0
    } else {
        true_positive as f64 / total as f64
    };
    Ok(Json(json!({
        "status": "scaffold",
        "skill_name": skill_name,
        "threshold": threshold,
        "total": total,
        "true_positive": true_positive,
        "false_negative": false_negative,
        "recall": recall,
        "cases": rows,
    })))
}

pub(super) async fn skills_delete(
    Path(name): Path<String>,
    Query(query): Query<SkillLocationQuery>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let service = skills_service();
    let location = query.location.unwrap_or(SkillLocation::Project);
    let deleted = service
        .delete_skill(&name, location)
        .map_err(|e| skill_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!({ "deleted": deleted })))
}

pub(super) async fn skills_templates_list() -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)>
{
    let service = skills_service();
    let templates = service
        .list_templates()
        .map_err(|e| skill_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!(templates)))
}

pub(super) async fn skills_templates_install(
    Path(id): Path<String>,
    Json(input): Json<SkillsTemplateInstallRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let service = skills_service();
    let installed = service
        .install_template(&id, input.location)
        .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!(installed)))
}

pub(super) async fn skill_list() -> Json<Value> {
    let service = skills_service();
    let skills = service.list_skills().unwrap_or_default();
    Json(json!({
        "skills": skills,
        "deprecation_warning": "GET /skill is deprecated; use GET /skills instead."
    }))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RunMemoryCapabilityPolicy {
    Default,
    CoderWorkflow,
}

pub(super) fn run_memory_subject(subject_hint: Option<&str>) -> String {
    subject_hint
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("default")
        .to_string()
}

pub(super) fn issue_run_memory_capability(
    run_id: &str,
    subject_hint: Option<&str>,
    partition: &tandem_memory::MemoryPartition,
    policy: RunMemoryCapabilityPolicy,
) -> MemoryCapabilityToken {
    let memory = match policy {
        RunMemoryCapabilityPolicy::Default => MemoryCapabilities::default(),
        RunMemoryCapabilityPolicy::CoderWorkflow => MemoryCapabilities {
            read_tiers: vec![
                tandem_memory::GovernedMemoryTier::Session,
                tandem_memory::GovernedMemoryTier::Project,
            ],
            write_tiers: vec![tandem_memory::GovernedMemoryTier::Session],
            promote_targets: vec![tandem_memory::GovernedMemoryTier::Project],
            require_review_for_promote: true,
            allow_auto_use_tiers: vec![tandem_memory::GovernedMemoryTier::Curated],
        },
    };
    MemoryCapabilityToken {
        run_id: run_id.to_string(),
        subject: run_memory_subject(subject_hint),
        org_id: partition.org_id.clone(),
        workspace_id: partition.workspace_id.clone(),
        project_id: partition.project_id.clone(),
        memory,
        expires_at: u64::MAX,
    }
}

pub(super) fn default_memory_capability_for(
    run_id: &str,
    partition: &tandem_memory::MemoryPartition,
) -> MemoryCapabilityToken {
    issue_run_memory_capability(run_id, None, partition, RunMemoryCapabilityPolicy::Default)
}

fn workflow_learning_kind_from_str(value: &str) -> Option<WorkflowLearningCandidateKind> {
    match value.trim().to_ascii_lowercase().as_str() {
        "memory_fact" => Some(WorkflowLearningCandidateKind::MemoryFact),
        "repair_hint" => Some(WorkflowLearningCandidateKind::RepairHint),
        "prompt_patch" => Some(WorkflowLearningCandidateKind::PromptPatch),
        "graph_patch" => Some(WorkflowLearningCandidateKind::GraphPatch),
        _ => None,
    }
}

fn workflow_learning_status_from_str(value: &str) -> Option<WorkflowLearningCandidateStatus> {
    match value.trim().to_ascii_lowercase().as_str() {
        "proposed" => Some(WorkflowLearningCandidateStatus::Proposed),
        "approved" => Some(WorkflowLearningCandidateStatus::Approved),
        "rejected" => Some(WorkflowLearningCandidateStatus::Rejected),
        "applied" => Some(WorkflowLearningCandidateStatus::Applied),
        "superseded" => Some(WorkflowLearningCandidateStatus::Superseded),
        "regressed" => Some(WorkflowLearningCandidateStatus::Regressed),
        _ => None,
    }
}

fn workflow_learning_kind_label(kind: WorkflowLearningCandidateKind) -> &'static str {
    match kind {
        WorkflowLearningCandidateKind::MemoryFact => "memory_fact",
        WorkflowLearningCandidateKind::RepairHint => "repair_hint",
        WorkflowLearningCandidateKind::PromptPatch => "prompt_patch",
        WorkflowLearningCandidateKind::GraphPatch => "graph_patch",
    }
}

fn workflow_learning_candidate_partition(
    tenant_context: &TenantContext,
    candidate: &WorkflowLearningCandidate,
    tier: tandem_memory::GovernedMemoryTier,
) -> tandem_memory::MemoryPartition {
    tandem_memory::MemoryPartition {
        org_id: tenant_context.org_id.clone(),
        workspace_id: tenant_context.workspace_id.clone(),
        project_id: candidate.project_id.clone(),
        tier,
    }
}

fn workflow_learning_candidate_title(summary: &str, fallback: &str) -> String {
    let trimmed = summary.trim();
    if trimmed.is_empty() {
        return fallback.to_string();
    }
    let clipped = trimmed.chars().take(60).collect::<String>();
    if trimmed.chars().count() > 60 {
        format!("{clipped}...")
    } else {
        clipped
    }
}

fn workflow_learning_candidate_memory_content(
    candidate: &WorkflowLearningCandidate,
) -> Option<String> {
    candidate
        .proposed_memory_payload
        .as_ref()
        .and_then(|payload: &Value| {
            payload
                .get("content")
                .and_then(Value::as_str)
                .or_else(|| payload.get("text").and_then(Value::as_str))
        })
        .map(|value: &str| value.trim())
        .filter(|value: &&str| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            let trimmed = candidate.summary.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
}

struct GovernedDistillationWriter {
    state: AppState,
    tenant_context: TenantContext,
    partition: tandem_memory::MemoryPartition,
    capability: MemoryCapabilityToken,
    run_id: String,
    workflow_id: Option<String>,
    artifact_refs: Vec<String>,
    subject: String,
}

impl GovernedDistillationWriter {
    async fn upsert_memory_fact_candidate(
        &self,
        session_id: &str,
        fact: &DistilledFact,
        memory_id: Option<String>,
        fingerprint: &str,
    ) -> MemoryResult<String> {
        let workflow_id = self
            .workflow_id
            .clone()
            .unwrap_or_else(|| format!("session:{}", session_id.trim()));
        let candidate = WorkflowLearningCandidate {
            candidate_id: format!("wflearn-{}", Uuid::new_v4()),
            workflow_id,
            project_id: self.partition.project_id.clone(),
            source_run_id: self.run_id.clone(),
            kind: WorkflowLearningCandidateKind::MemoryFact,
            status: WorkflowLearningCandidateStatus::Proposed,
            confidence: fact.importance_score,
            summary: fact.content.clone(),
            fingerprint: fingerprint.to_string(),
            node_id: None,
            node_kind: None,
            validator_family: None,
            evidence_refs: vec![json!({
                "session_id": session_id,
                "run_id": self.run_id,
                "distillation_id": fact.distillation_id,
                "fact_id": fact.id,
                "fact_category": fact.category,
            })],
            artifact_refs: self.artifact_refs.clone(),
            proposed_memory_payload: Some(json!({
                "content": fact.content,
                "kind": "fact",
                "classification": "internal",
            })),
            proposed_revision_prompt: None,
            source_memory_id: memory_id,
            promoted_memory_id: None,
            needs_plan_bundle: false,
            baseline_before: None,
            latest_observed_metrics: None,
            last_revision_session_id: None,
            run_ids: vec![self.run_id.clone()],
            created_at_ms: crate::now_ms(),
            updated_at_ms: crate::now_ms(),
        };
        self.state
            .upsert_workflow_learning_candidate(candidate)
            .await
            .map(|candidate| candidate.candidate_id)
            .map_err(|error| tandem_memory::types::MemoryError::InvalidConfig(error.to_string()))
    }

    async fn store_fact(
        &self,
        session_id: &str,
        fact: &DistilledFact,
    ) -> MemoryResult<tandem_memory::DistillationMemoryWrite> {
        let content_hash = hash_text(&fact.content);
        let fact_category = fact.category.to_string();
        let fingerprint = hash_text(&format!(
            "{}:{}:{}:{}",
            self.partition.project_id,
            self.workflow_id.as_deref().unwrap_or(session_id),
            fact.category,
            fact.content
        ));
        let db = open_global_memory_db().await.ok_or_else(|| {
            tandem_memory::types::MemoryError::InvalidConfig(
                "global memory db unavailable".to_string(),
            )
        })?;
        let existing = db
            .list_global_memory(
                &self.subject,
                None,
                Some(&self.partition.project_id),
                None,
                200,
                0,
            )
            .await
            .map_err(|error| tandem_memory::types::MemoryError::InvalidConfig(error.to_string()))?
            .into_iter()
            .find(|record| {
                record.content_hash == content_hash
                    && record
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("origin"))
                        .and_then(Value::as_str)
                        == Some("session_distillation")
                    && record
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("fact_category"))
                        .and_then(Value::as_str)
                        == Some(fact_category.as_str())
                    && record
                        .metadata
                        .as_ref()
                        .and_then(|metadata| metadata.get("workflow_id"))
                        .and_then(Value::as_str)
                        == self.workflow_id.as_deref()
            });

        if let Some(existing) = existing {
            let mut next_metadata = existing.metadata.clone().unwrap_or_else(|| json!({}));
            if let Some(object) = next_metadata.as_object_mut() {
                object.insert("fingerprint".to_string(), json!(fingerprint));
                object.insert("artifact_refs".to_string(), json!(self.artifact_refs));
                object.insert("session_id".to_string(), json!(session_id));
                object.insert("workflow_id".to_string(), json!(self.workflow_id));
                object.insert("last_distilled_at_ms".to_string(), json!(crate::now_ms()));
            }
            let _ = db
                .update_global_memory_context(
                    &existing.id,
                    &existing.visibility,
                    existing.demoted,
                    Some(&next_metadata),
                    existing.provenance.as_ref(),
                )
                .await
                .map_err(|error| {
                    tandem_memory::types::MemoryError::InvalidConfig(error.to_string())
                })?;
            let candidate_id = self
                .upsert_memory_fact_candidate(
                    session_id,
                    fact,
                    Some(existing.id.clone()),
                    &fingerprint,
                )
                .await?;
            return Ok(tandem_memory::DistillationMemoryWrite {
                stored: false,
                deduped: true,
                memory_id: Some(existing.id),
                candidate_id: Some(candidate_id),
            });
        }

        let request = MemoryPutRequest {
            run_id: self.run_id.clone(),
            partition: self.partition.clone(),
            kind: tandem_memory::MemoryContentKind::Fact,
            content: fact.content.clone(),
            artifact_refs: self.artifact_refs.clone(),
            classification: tandem_memory::MemoryClassification::Internal,
            metadata: Some(json!({
                "origin": "session_distillation",
                "fact_category": fact.category,
                "session_id": session_id,
                "run_id": self.run_id,
                "workflow_id": self.workflow_id,
                "artifact_refs": self.artifact_refs,
                "fingerprint": fingerprint,
                "distillation_id": fact.distillation_id,
                "fact_id": fact.id,
            })),
        };
        let response = memory_put_impl(
            &self.state,
            &self.tenant_context,
            request,
            Some(self.capability.clone()),
        )
        .await
        .map_err(|status| {
            tandem_memory::types::MemoryError::InvalidConfig(format!(
                "memory_put failed with status {status}"
            ))
        })?;
        let candidate_id = self
            .upsert_memory_fact_candidate(session_id, fact, Some(response.id.clone()), &fingerprint)
            .await?;
        Ok(tandem_memory::DistillationMemoryWrite {
            stored: response.stored,
            deduped: !response.stored,
            memory_id: Some(response.id),
            candidate_id: Some(candidate_id),
        })
    }
}

#[async_trait]
impl tandem_memory::DistillationMemoryWriter for GovernedDistillationWriter {
    async fn store_user_fact(
        &self,
        session_id: &str,
        fact: &DistilledFact,
    ) -> MemoryResult<tandem_memory::DistillationMemoryWrite> {
        self.store_fact(session_id, fact).await
    }

    async fn store_agent_fact(
        &self,
        session_id: &str,
        fact: &DistilledFact,
    ) -> MemoryResult<tandem_memory::DistillationMemoryWrite> {
        self.store_fact(session_id, fact).await
    }
}

fn memory_metadata_with_storage_fields(
    metadata: Option<Value>,
    artifact_refs: &[String],
    classification: tandem_memory::MemoryClassification,
) -> Option<Value> {
    let mut metadata = metadata.unwrap_or_else(|| json!({}));
    if !metadata.is_object() {
        metadata = json!({ "value": metadata });
    }
    if let Some(obj) = metadata.as_object_mut() {
        if !artifact_refs.is_empty() {
            obj.insert("artifact_refs".to_string(), json!(artifact_refs));
        }
        obj.insert("classification".to_string(), json!(classification));
    }
    Some(metadata)
}

fn memory_artifact_refs(metadata: Option<&Value>) -> Vec<String> {
    metadata
        .and_then(|row| row.get("artifact_refs"))
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn memory_put_provenance(
    request: &MemoryPutRequest,
    partition_key: &str,
    artifact_refs: &[String],
    tenant_context: &TenantContext,
) -> Value {
    json!({
        "origin_event_type": "memory.put",
        "origin_run_id": request.run_id,
        "partition_key": partition_key,
        "tenant_context": tenant_context,
        "partition": {
            "org_id": request.partition.org_id,
            "workspace_id": request.partition.workspace_id,
            "project_id": request.partition.project_id,
            "tier": request.partition.tier,
        },
        "artifact_refs": artifact_refs,
    })
}

async fn emit_blocked_memory_promote_guardrail(
    state: &AppState,
    tenant_context: &TenantContext,
    request: &MemoryPromoteRequest,
    actor: String,
    detail: &str,
) -> Result<(), StatusCode> {
    let audit_id = Uuid::new_v4().to_string();
    let partition_key = format!(
        "{}/{}/{}/{}",
        request.partition.org_id,
        request.partition.workspace_id,
        request.partition.project_id,
        request.to_tier
    );
    let linkage = json!({
        "run_id": request.run_id,
        "project_id": request.partition.project_id,
        "origin_event_type": Value::Null,
        "origin_run_id": request.run_id,
        "origin_session_id": Value::Null,
        "origin_message_id": Value::Null,
        "partition_key": partition_key,
        "promote_run_id": Value::Null,
        "approval_id": request.review.approval_id,
        "artifact_refs": [],
    });
    append_memory_audit(
        state,
        tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_promote".to_string(),
            run_id: request.run_id.clone(),
            tenant_context: tenant_context.clone(),
            memory_id: None,
            source_memory_id: Some(request.source_memory_id.clone()),
            to_tier: Some(request.to_tier),
            partition_key: partition_key.clone(),
            actor,
            status: "blocked".to_string(),
            detail: Some(format!("{detail}{}", memory_linkage_detail(&linkage))),
            created_at_ms: crate::now_ms(),
        },
    )
    .await?;
    publish_tenant_event(
        state,
        tenant_context,
        "memory.promote",
        json!({
            "runID": request.run_id,
            "sourceMemoryID": request.source_memory_id,
            "toTier": request.to_tier,
            "partitionKey": partition_key,
            "status": "blocked",
            "kind": Value::Null,
            "classification": Value::Null,
            "artifactRefs": [],
            "visibility": Value::Null,
            "scrubStatus": Value::Null,
            "linkage": linkage,
            "detail": detail,
            "auditID": audit_id,
        }),
    );
    Ok(())
}

async fn emit_blocked_memory_put_guardrail(
    state: &AppState,
    tenant_context: &TenantContext,
    request: &MemoryPutRequest,
    actor: String,
    detail: &str,
) -> Result<(), StatusCode> {
    let audit_id = Uuid::new_v4().to_string();
    let partition_key = request.partition.key();
    let metadata = memory_metadata_with_storage_fields(
        request.metadata.clone(),
        &request.artifact_refs,
        request.classification,
    );
    let provenance = memory_put_provenance(
        request,
        &partition_key,
        &request.artifact_refs,
        tenant_context,
    );
    let linkage = memory_linkage_from_parts(
        &request.run_id,
        Some(&request.partition.project_id),
        metadata.as_ref(),
        Some(&provenance),
    );
    append_memory_audit(
        state,
        tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_put".to_string(),
            run_id: request.run_id.clone(),
            tenant_context: tenant_context.clone(),
            memory_id: None,
            source_memory_id: None,
            to_tier: Some(request.partition.tier),
            partition_key: partition_key.clone(),
            actor,
            status: "blocked".to_string(),
            detail: Some(format!("{detail}{}", memory_linkage_detail(&linkage))),
            created_at_ms: crate::now_ms(),
        },
    )
    .await?;
    publish_tenant_event(
        state,
        tenant_context,
        "memory.put",
        json!({
            "runID": request.run_id,
            "kind": memory_kind_for_request(request.kind.clone()),
            "classification": request.classification,
            "artifactRefs": request.artifact_refs.clone(),
            "visibility": Value::Null,
            "tier": request.partition.tier,
            "partitionKey": partition_key,
            "linkage": linkage,
            "status": "blocked",
            "detail": detail,
            "auditID": audit_id,
        }),
    );
    Ok(())
}

fn validate_memory_capability_guardrail_context(
    run_id: &str,
    partition: &tandem_memory::MemoryPartition,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, (String, &'static str, StatusCode)> {
    let cap = capability.unwrap_or_else(|| default_memory_capability_for(run_id, partition));
    if cap.run_id != run_id
        || cap.org_id != partition.org_id
        || cap.workspace_id != partition.workspace_id
        || cap.project_id != partition.project_id
    {
        return Err((
            cap.subject.clone(),
            "capability context mismatch",
            StatusCode::FORBIDDEN,
        ));
    }
    if cap.expires_at < crate::now_ms() {
        return Err((
            cap.subject.clone(),
            "capability expired",
            StatusCode::UNAUTHORIZED,
        ));
    }
    Ok(cap)
}

async fn validate_memory_put_capability_with_guardrail(
    state: &AppState,
    tenant_context: &TenantContext,
    request: &MemoryPutRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, StatusCode> {
    let cap = match validate_memory_capability_guardrail_context(
        &request.run_id,
        &request.partition,
        capability,
    ) {
        Ok(cap) => cap,
        Err((actor, detail, status)) => {
            emit_blocked_memory_put_guardrail(state, tenant_context, request, actor, detail)
                .await?;
            return Err(status);
        }
    };
    Ok(cap)
}

async fn validate_memory_promote_capability_with_guardrail(
    state: &AppState,
    tenant_context: &TenantContext,
    request: &MemoryPromoteRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, StatusCode> {
    let cap = match validate_memory_capability_guardrail_context(
        &request.run_id,
        &request.partition,
        capability,
    ) {
        Ok(cap) => cap,
        Err((actor, detail, status)) => {
            emit_blocked_memory_promote_guardrail(state, tenant_context, request, actor, detail)
                .await?;
            return Err(status);
        }
    };
    Ok(cap)
}

async fn validate_memory_search_capability_with_guardrail(
    state: &AppState,
    tenant_context: &TenantContext,
    request: &MemorySearchRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, StatusCode> {
    let cap = match validate_memory_capability_guardrail_context(
        &request.run_id,
        &request.partition,
        capability,
    ) {
        Ok(cap) => cap,
        Err((actor, detail, status)) => {
            let requested_scopes = if request.read_scopes.is_empty() {
                default_memory_capability_for(&request.run_id, &request.partition)
                    .memory
                    .read_tiers
            } else {
                request.read_scopes.clone()
            };
            return emit_blocked_memory_search_guardrail(
                status,
                detail,
                actor,
                state,
                tenant_context,
                request,
                &requested_scopes,
                &request.partition.key(),
            )
            .await;
        }
    };
    Ok(cap)
}

async fn emit_blocked_memory_search_guardrail(
    status_code: StatusCode,
    detail: &str,
    actor: String,
    state: &AppState,
    tenant_context: &TenantContext,
    request: &MemorySearchRequest,
    requested_scopes: &[tandem_memory::GovernedMemoryTier],
    partition_key: &str,
) -> Result<MemoryCapabilityToken, StatusCode> {
    let audit_id = Uuid::new_v4().to_string();
    let linkage = json!({
        "run_id": request.run_id,
        "project_id": request.partition.project_id,
        "origin_event_type": "memory.search",
        "origin_run_id": request.run_id,
        "origin_session_id": Value::Null,
        "origin_message_id": Value::Null,
        "partition_key": partition_key,
        "promote_run_id": Value::Null,
        "approval_id": Value::Null,
        "artifact_refs": [],
    });
    let search_detail = format!(
        "query={} result_count=0 result_ids= result_kinds= requested_scopes={} scopes_used= blocked_scopes={} detail={}{}",
        request.query,
        requested_scopes
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>()
            .join(","),
        requested_scopes
            .iter()
            .map(|scope| scope.to_string())
            .collect::<Vec<_>>()
            .join(","),
        detail,
        memory_linkage_detail(&linkage)
    );
    append_memory_audit(
        state,
        tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_search".to_string(),
            run_id: request.run_id.clone(),
            tenant_context: tenant_context.clone(),
            memory_id: None,
            source_memory_id: None,
            to_tier: None,
            partition_key: partition_key.to_string(),
            actor,
            status: "blocked".to_string(),
            detail: Some(search_detail),
            created_at_ms: crate::now_ms(),
        },
    )
    .await?;
    publish_tenant_event(
        state,
        tenant_context,
        "memory.search",
        json!({
            "runID": request.run_id,
            "query": request.query,
            "partitionKey": partition_key,
            "resultCount": 0,
            "resultIDs": [],
            "resultKinds": [],
            "requestedScopes": requested_scopes,
            "scopesUsed": [],
            "blockedScopes": requested_scopes,
            "linkage": linkage,
            "status": "blocked",
            "detail": detail,
            "auditID": audit_id,
        }),
    );
    Err(status_code)
}

async fn emit_missing_memory_demote_audit(
    state: &AppState,
    tenant_context: &TenantContext,
    run_id: &str,
    memory_id: &str,
    detail: &str,
) -> Result<(), StatusCode> {
    let audit_id = Uuid::new_v4().to_string();
    append_memory_audit(
        state,
        tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_demote".to_string(),
            run_id: run_id.to_string(),
            tenant_context: tenant_context.clone(),
            memory_id: Some(memory_id.to_string()),
            source_memory_id: None,
            to_tier: None,
            partition_key: "demoted".to_string(),
            actor: "system".to_string(),
            status: "not_found".to_string(),
            detail: Some(detail.to_string()),
            created_at_ms: crate::now_ms(),
        },
    )
    .await?;
    publish_tenant_event(
        state,
        tenant_context,
        "memory.updated",
        json!({
            "memoryID": memory_id,
            "runID": run_id,
            "action": "demote",
            "kind": Value::Null,
            "classification": Value::Null,
            "artifactRefs": [],
            "visibility": Value::Null,
            "tier": Value::Null,
            "partitionKey": "demoted",
            "demoted": Value::Null,
            "status": "not_found",
            "detail": detail,
            "auditID": audit_id,
        }),
    );
    Ok(())
}

async fn emit_missing_memory_delete_audit(
    state: &AppState,
    tenant_context: &TenantContext,
    memory_id: &str,
    detail: &str,
) -> Result<(), StatusCode> {
    let audit_id = Uuid::new_v4().to_string();
    append_memory_audit(
        state,
        tenant_context,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_delete".to_string(),
            run_id: "unknown".to_string(),
            tenant_context: tenant_context.clone(),
            memory_id: Some(memory_id.to_string()),
            source_memory_id: None,
            to_tier: None,
            partition_key: "global".to_string(),
            actor: "admin".to_string(),
            status: "not_found".to_string(),
            detail: Some(detail.to_string()),
            created_at_ms: crate::now_ms(),
        },
    )
    .await?;
    publish_tenant_event(
        state,
        tenant_context,
        "memory.deleted",
        json!({
            "memoryID": memory_id,
            "runID": Value::Null,
            "kind": Value::Null,
            "classification": Value::Null,
            "artifactRefs": [],
            "visibility": Value::Null,
            "tier": Value::Null,
            "partitionKey": Value::Null,
            "demoted": Value::Null,
            "status": "not_found",
            "detail": detail,
            "auditID": audit_id,
        }),
    );
    Ok(())
}
