use super::context_runs::context_run_engine;
use super::*;

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

    let execution_plan = json!({
        "workflow_kind": workflow_kind,
        "goal": input.goal,
        "schedule": input.schedule,
        "default_action": if workflow_kind == "automation_v2_dag" {
            "create_automation_v2"
        } else {
            "pack_builder_preview"
        }
    });

    let response = json!({
        "skill_name": skill.info.name,
        "workflow_kind": execution_plan.get("workflow_kind"),
        "validation": validation,
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
) -> Value {
    json!({
        "origin_event_type": "memory.put",
        "origin_run_id": request.run_id,
        "partition_key": partition_key,
        "partition": {
            "org_id": request.partition.org_id,
            "workspace_id": request.partition.workspace_id,
            "project_id": request.partition.project_id,
            "tier": request.partition.tier,
        },
        "artifact_refs": artifact_refs,
    })
}

fn memory_promote_metadata(
    metadata: Option<&Value>,
    request: &MemoryPromoteRequest,
    promoted_at_ms: u64,
) -> Option<Value> {
    let mut obj = metadata
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    obj.insert(
        "promotion".to_string(),
        json!({
            "promoted_at_ms": promoted_at_ms,
            "promote_run_id": request.run_id,
            "source_memory_id": request.source_memory_id,
            "from_tier": request.from_tier,
            "to_tier": request.to_tier,
            "reason": request.reason,
            "review": {
                "required": request.review.required,
                "reviewer_id": request.review.reviewer_id,
                "approval_id": request.review.approval_id,
            },
        }),
    );
    Some(Value::Object(obj))
}

fn memory_promote_provenance(
    provenance: Option<&Value>,
    request: &MemoryPromoteRequest,
    partition_key: &str,
    promoted_at_ms: u64,
) -> Value {
    let mut obj = provenance
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    obj.insert(
        "promotion".to_string(),
        json!({
            "promoted_at_ms": promoted_at_ms,
            "promote_run_id": request.run_id,
            "source_memory_id": request.source_memory_id,
            "partition_key": partition_key,
            "to_tier": request.to_tier,
            "reviewer_id": request.review.reviewer_id,
            "approval_id": request.review.approval_id,
        }),
    );
    Value::Object(obj)
}

fn memory_linkage(record: &GlobalMemoryRecord) -> Value {
    let artifact_refs = memory_artifact_refs(record.metadata.as_ref());
    let provenance = record.provenance.as_ref();
    json!({
        "run_id": record.run_id,
        "project_id": record.project_tag,
        "origin_event_type": provenance
            .and_then(|row| row.get("origin_event_type"))
            .and_then(Value::as_str),
        "origin_run_id": provenance
            .and_then(|row| row.get("origin_run_id"))
            .and_then(Value::as_str)
            .or(Some(record.run_id.as_str())),
        "origin_session_id": provenance
            .and_then(|row| row.get("origin_session_id"))
            .and_then(Value::as_str),
        "origin_message_id": provenance
            .and_then(|row| row.get("origin_message_id"))
            .and_then(Value::as_str),
        "partition_key": provenance
            .and_then(|row| row.get("partition_key"))
            .and_then(Value::as_str),
        "promote_run_id": provenance
            .and_then(|row| row.get("promotion"))
            .and_then(|row| row.get("promote_run_id"))
            .and_then(Value::as_str),
        "approval_id": provenance
            .and_then(|row| row.get("promotion"))
            .and_then(|row| row.get("approval_id"))
            .and_then(Value::as_str),
        "artifact_refs": artifact_refs,
    })
}

fn memory_kind_label(source_type: &str) -> &str {
    match source_type {
        "solution_capsule" => "solution_capsule",
        "note" => "note",
        "fact" => "fact",
        other => other,
    }
}

fn memory_kind_for_request(kind: tandem_memory::MemoryContentKind) -> &'static str {
    match kind {
        tandem_memory::MemoryContentKind::SolutionCapsule => "solution_capsule",
        tandem_memory::MemoryContentKind::Note => "note",
        tandem_memory::MemoryContentKind::Fact => "fact",
    }
}

fn memory_tier_for_visibility(visibility: &str) -> tandem_memory::GovernedMemoryTier {
    if visibility.eq_ignore_ascii_case("shared") {
        tandem_memory::GovernedMemoryTier::Project
    } else {
        tandem_memory::GovernedMemoryTier::Session
    }
}

fn memory_classification_label(metadata: Option<&Value>) -> &str {
    metadata
        .and_then(|row| row.get("classification"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("internal")
}

pub(super) fn validate_memory_capability(
    run_id: &str,
    partition: &tandem_memory::MemoryPartition,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryCapabilityToken, StatusCode> {
    let cap = capability.unwrap_or_else(|| default_memory_capability_for(run_id, partition));
    if cap.run_id != run_id
        || cap.org_id != partition.org_id
        || cap.workspace_id != partition.workspace_id
        || cap.project_id != partition.project_id
    {
        return Err(StatusCode::FORBIDDEN);
    }
    if cap.expires_at < crate::now_ms() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(cap)
}

pub(super) fn scrub_content(input: &str) -> ScrubReport {
    let mut redactions = 0u32;
    let mut blocked = false;
    let lower = input.to_lowercase();
    let redact_markers = [
        "api_key",
        "secret=",
        "authorization: bearer",
        "x-api-key",
        "token=",
    ];
    for marker in redact_markers {
        if lower.contains(marker) {
            redactions = redactions.saturating_add(1);
        }
    }
    let block_markers = [
        "-----begin private key-----",
        "aws_secret_access_key",
        "sk-ant-",
        "ghp_",
    ];
    for marker in block_markers {
        if lower.contains(marker) {
            blocked = true;
            break;
        }
    }
    if blocked {
        ScrubReport {
            status: ScrubStatus::Blocked,
            redactions,
            block_reason: Some("sensitive secret marker detected".to_string()),
        }
    } else if redactions > 0 {
        ScrubReport {
            status: ScrubStatus::Redacted,
            redactions,
            block_reason: None,
        }
    } else {
        ScrubReport {
            status: ScrubStatus::Passed,
            redactions: 0,
            block_reason: None,
        }
    }
}

pub(super) fn scrub_content_for_memory(input: &str) -> (String, ScrubReport) {
    let mut scrubbed = input.to_string();
    let mut redactions = 0u32;
    let mut blocked = false;
    let redact_patterns = [
        r"(?i)authorization:\s*bearer\s+[a-z0-9\.\-_]+",
        r"(?i)(api[_-]?key|token|secret)\s*[:=]\s*[a-z0-9\-_]{8,}",
        r"(?i)x-api-key\s*:\s*[a-z0-9\-_]{8,}",
        r"(?i)sk-[a-z0-9]{12,}",
        r"(?i)ghp_[a-z0-9]{12,}",
    ];
    for pattern in redact_patterns {
        if let Ok(re) = Regex::new(pattern) {
            let matches = re.find_iter(&scrubbed).count() as u32;
            if matches > 0 {
                redactions = redactions.saturating_add(matches);
                scrubbed = re.replace_all(&scrubbed, "[REDACTED]").to_string();
            }
        }
    }
    let block_markers = [
        "-----begin private key-----",
        "aws_secret_access_key",
        "-----begin rsa private key-----",
    ];
    let lowered = input.to_lowercase();
    for marker in block_markers {
        if lowered.contains(marker) {
            blocked = true;
            break;
        }
    }
    if blocked {
        (
            String::new(),
            ScrubReport {
                status: ScrubStatus::Blocked,
                redactions,
                block_reason: Some("sensitive secret marker detected".to_string()),
            },
        )
    } else if redactions > 0 {
        (
            scrubbed,
            ScrubReport {
                status: ScrubStatus::Redacted,
                redactions,
                block_reason: None,
            },
        )
    } else {
        (
            scrubbed,
            ScrubReport {
                status: ScrubStatus::Passed,
                redactions: 0,
                block_reason: None,
            },
        )
    }
}

pub(super) fn hash_text(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

pub(super) async fn append_memory_audit(
    state: &AppState,
    event: crate::MemoryAuditEvent,
) -> Result<(), StatusCode> {
    let mut audit = state.memory_audit_log.write().await;
    audit.push(event);
    Ok(())
}

#[derive(Debug, Clone)]
pub(super) struct RunMemoryContext {
    run_id: String,
    user_id: String,
    started_at_ms: u64,
    host_tag: Option<String>,
}

pub(super) async fn open_global_memory_db() -> Option<MemoryDatabase> {
    let paths = tandem_core::resolve_shared_paths().ok()?;
    if let Some(parent) = paths.memory_db_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }
    MemoryDatabase::new(&paths.memory_db_path).await.ok()
}

pub(super) fn event_run_id(event: &EngineEvent) -> Option<String> {
    event
        .properties
        .get("runID")
        .or_else(|| event.properties.get("run_id"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

pub(super) fn event_session_id(event: &EngineEvent) -> Option<String> {
    event
        .properties
        .get("sessionID")
        .or_else(|| event.properties.get("sessionId"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

pub(super) fn summarize_value(value: &Value, limit: usize) -> String {
    let text = if value.is_string() {
        value.as_str().unwrap_or_default().to_string()
    } else {
        value.to_string()
    };
    truncate_text(&text, limit)
}

pub(super) async fn persist_global_memory_record(
    state: &AppState,
    db: &MemoryDatabase,
    mut record: GlobalMemoryRecord,
) {
    state.event_bus.publish(EngineEvent::new(
        "memory.write.attempted",
        json!({
            "runID": record.run_id,
            "sourceType": record.source_type,
            "sessionID": record.session_id,
            "messageID": record.message_id,
        }),
    ));
    let (scrubbed, scrub) = scrub_content_for_memory(&record.content);
    if scrub.status == ScrubStatus::Blocked || scrubbed.trim().is_empty() {
        state.event_bus.publish(EngineEvent::new(
            "memory.write.skipped",
            json!({
                "runID": record.run_id,
                "sourceType": record.source_type,
                "reason": scrub.block_reason.unwrap_or_else(|| "scrub_blocked".to_string()),
                "sessionID": record.session_id,
                "messageID": record.message_id,
            }),
        ));
        return;
    }
    record.content = scrubbed;
    record.redaction_count = scrub.redactions;
    record.redaction_status = match scrub.status {
        ScrubStatus::Passed => "passed".to_string(),
        ScrubStatus::Redacted => "redacted".to_string(),
        ScrubStatus::Blocked => "blocked".to_string(),
    };
    record.content_hash = hash_text(&record.content);
    match db.put_global_memory_record(&record).await {
        Ok(write) => {
            let event_name = if write.deduped {
                "memory.write.skipped"
            } else {
                "memory.write.succeeded"
            };
            state.event_bus.publish(EngineEvent::new(
                event_name,
                json!({
                    "runID": record.run_id,
                    "memoryID": write.id,
                    "sourceType": record.source_type,
                    "deduped": write.deduped,
                    "redactionStatus": record.redaction_status,
                    "redactionCount": record.redaction_count,
                    "sessionID": record.session_id,
                    "messageID": record.message_id,
                }),
            ));
        }
        Err(err) => {
            state.event_bus.publish(EngineEvent::new(
                "memory.write.skipped",
                json!({
                    "runID": record.run_id,
                    "sourceType": record.source_type,
                    "reason": format!("db_error:{err}"),
                    "sessionID": record.session_id,
                    "messageID": record.message_id,
                }),
            ));
        }
    }
}

pub(super) async fn ingest_run_messages(
    state: &AppState,
    db: &MemoryDatabase,
    session_id: &str,
    ctx: &RunMemoryContext,
) {
    let Some(session) = state.storage.get_session(session_id).await else {
        return;
    };
    for message in session.messages {
        let created_ms = message.created_at.timestamp_millis() as u64;
        if created_ms + 1_000 < ctx.started_at_ms {
            continue;
        }
        for part in message.parts {
            match (message.role.clone(), part) {
                (MessageRole::User, MessagePart::Text { text }) => {
                    let now = crate::now_ms();
                    persist_global_memory_record(
                        state,
                        db,
                        GlobalMemoryRecord {
                            id: Uuid::new_v4().to_string(),
                            user_id: ctx.user_id.clone(),
                            source_type: "user_message".to_string(),
                            content: text,
                            content_hash: String::new(),
                            run_id: ctx.run_id.clone(),
                            session_id: Some(session_id.to_string()),
                            message_id: Some(message.id.clone()),
                            tool_name: None,
                            project_tag: session.project_id.clone(),
                            channel_tag: None,
                            host_tag: ctx.host_tag.clone(),
                            metadata: Some(json!({"role": "user"})),
                            provenance: Some(json!({"origin_event_type": "session.run.finished", "origin_message_id": message.id, "origin_session_id": session_id})),
                            redaction_status: "passed".to_string(),
                            redaction_count: 0,
                            visibility: "private".to_string(),
                            demoted: false,
                            score_boost: 0.0,
                            created_at_ms: now,
                            updated_at_ms: now,
                            expires_at_ms: None,
                        },
                    )
                    .await;
                }
                (MessageRole::Assistant, MessagePart::Text { text }) => {
                    let now = crate::now_ms();
                    persist_global_memory_record(
                        state,
                        db,
                        GlobalMemoryRecord {
                            id: Uuid::new_v4().to_string(),
                            user_id: ctx.user_id.clone(),
                            source_type: "assistant_final".to_string(),
                            content: text,
                            content_hash: String::new(),
                            run_id: ctx.run_id.clone(),
                            session_id: Some(session_id.to_string()),
                            message_id: Some(message.id.clone()),
                            tool_name: None,
                            project_tag: session.project_id.clone(),
                            channel_tag: None,
                            host_tag: ctx.host_tag.clone(),
                            metadata: Some(json!({"role": "assistant"})),
                            provenance: Some(json!({"origin_event_type": "session.run.finished", "origin_message_id": message.id, "origin_session_id": session_id})),
                            redaction_status: "passed".to_string(),
                            redaction_count: 0,
                            visibility: "private".to_string(),
                            demoted: false,
                            score_boost: 0.0,
                            created_at_ms: now,
                            updated_at_ms: now,
                            expires_at_ms: None,
                        },
                    )
                    .await;
                }
                (
                    MessageRole::Assistant | MessageRole::Tool,
                    MessagePart::ToolInvocation {
                        tool,
                        args,
                        result,
                        error,
                    },
                ) => {
                    let now = crate::now_ms();
                    let tool_input = summarize_value(&args, 1200);
                    persist_global_memory_record(
                        state,
                        db,
                        GlobalMemoryRecord {
                            id: Uuid::new_v4().to_string(),
                            user_id: ctx.user_id.clone(),
                            source_type: "tool_input".to_string(),
                            content: format!("tool={} args={}", tool, tool_input),
                            content_hash: String::new(),
                            run_id: ctx.run_id.clone(),
                            session_id: Some(session_id.to_string()),
                            message_id: Some(message.id.clone()),
                            tool_name: Some(tool.clone()),
                            project_tag: session.project_id.clone(),
                            channel_tag: None,
                            host_tag: ctx.host_tag.clone(),
                            metadata: None,
                            provenance: Some(json!({"origin_event_type": "session.run.finished"})),
                            redaction_status: "passed".to_string(),
                            redaction_count: 0,
                            visibility: "private".to_string(),
                            demoted: false,
                            score_boost: 0.0,
                            created_at_ms: now,
                            updated_at_ms: now,
                            expires_at_ms: Some(now + 30 * 24 * 60 * 60 * 1000),
                        },
                    )
                    .await;
                    let tool_output = result
                        .as_ref()
                        .map(|v| summarize_value(v, 1500))
                        .or(error)
                        .unwrap_or_default();
                    if !tool_output.trim().is_empty() {
                        let now = crate::now_ms();
                        persist_global_memory_record(
                            state,
                            db,
                            GlobalMemoryRecord {
                                id: Uuid::new_v4().to_string(),
                                user_id: ctx.user_id.clone(),
                                source_type: "tool_output".to_string(),
                                content: format!("tool={} output={}", tool, tool_output),
                                content_hash: String::new(),
                                run_id: ctx.run_id.clone(),
                                session_id: Some(session_id.to_string()),
                                message_id: Some(message.id.clone()),
                                tool_name: Some(tool),
                                project_tag: session.project_id.clone(),
                                channel_tag: None,
                                host_tag: ctx.host_tag.clone(),
                                metadata: None,
                                provenance: Some(
                                    json!({"origin_event_type": "session.run.finished"}),
                                ),
                                redaction_status: "passed".to_string(),
                                redaction_count: 0,
                                visibility: "private".to_string(),
                                demoted: false,
                                score_boost: 0.0,
                                created_at_ms: now,
                                updated_at_ms: now,
                                expires_at_ms: Some(now + 30 * 24 * 60 * 60 * 1000),
                            },
                        )
                        .await;
                    }
                }
                _ => {}
            }
        }
    }
}

pub(super) async fn ingest_event_memory_records(
    state: &AppState,
    db: &MemoryDatabase,
    event: &EngineEvent,
    ctx_by_session: &HashMap<String, RunMemoryContext>,
) {
    let session_id = event_session_id(event);
    let session_ctx = session_id
        .as_ref()
        .and_then(|sid| ctx_by_session.get(sid))
        .cloned();
    let run_id = event_run_id(event)
        .or_else(|| session_ctx.as_ref().map(|c| c.run_id.clone()))
        .unwrap_or_else(|| "unknown".to_string());
    let user_id = session_ctx
        .as_ref()
        .map(|c| c.user_id.clone())
        .unwrap_or_else(|| "default".to_string());
    let host_tag = session_ctx.as_ref().and_then(|c| c.host_tag.clone());
    let (source_type, content, ttl_ms): (&str, String, Option<u64>) =
        match event.event_type.as_str() {
            "permission.asked" => (
                "approval_request",
                format!(
                    "permission requested tool={} query={}",
                    event
                        .properties
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    event
                        .properties
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                ),
                Some(14 * 24 * 60 * 60 * 1000),
            ),
            "permission.replied" => (
                "approval_decision",
                format!(
                    "permission reply requestID={} reply={}",
                    event
                        .properties
                        .get("requestID")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    event
                        .properties
                        .get("reply")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                ),
                Some(14 * 24 * 60 * 60 * 1000),
            ),
            "mcp.auth.required" | "mcp.auth.pending" => (
                "auth_challenge",
                format!(
                    "mcp auth tool={} server={} status={} message={}",
                    event
                        .properties
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    event
                        .properties
                        .get("server")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    event.event_type,
                    event
                        .properties
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                ),
                Some(7 * 24 * 60 * 60 * 1000),
            ),
            "todo.updated" => (
                "plan_todos",
                format!(
                    "todo updated: {}",
                    summarize_value(event.properties.get("todos").unwrap_or(&Value::Null), 1200)
                ),
                Some(60 * 24 * 60 * 60 * 1000),
            ),
            "question.asked" => (
                "question_prompt",
                format!(
                    "question asked: {}",
                    summarize_value(
                        event.properties.get("questions").unwrap_or(&Value::Null),
                        1200
                    )
                ),
                Some(60 * 24 * 60 * 60 * 1000),
            ),
            _ => return,
        };
    let now = crate::now_ms();
    persist_global_memory_record(
        state,
        db,
        GlobalMemoryRecord {
            id: Uuid::new_v4().to_string(),
            user_id,
            source_type: source_type.to_string(),
            content,
            content_hash: String::new(),
            run_id,
            session_id,
            message_id: event
                .properties
                .get("messageID")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            tool_name: event
                .properties
                .get("tool")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            project_tag: None,
            channel_tag: event
                .properties
                .get("channel")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            host_tag,
            metadata: None,
            provenance: Some(json!({
                "origin_event_type": event.event_type,
            })),
            redaction_status: "passed".to_string(),
            redaction_count: 0,
            visibility: "private".to_string(),
            demoted: false,
            score_boost: 0.0,
            created_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: ttl_ms.map(|ttl| now + ttl),
        },
    )
    .await;
}

pub(super) async fn run_global_memory_ingestor(state: AppState) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        tracing::warn!("global memory ingestor: skipped because runtime did not become ready");
        return;
    }
    let mut rx = state.event_bus.subscribe();
    let Some(db) = open_global_memory_db().await else {
        tracing::warn!("global memory ingestor disabled: could not open memory database");
        return;
    };
    let mut by_session: HashMap<String, RunMemoryContext> = HashMap::new();
    loop {
        match rx.recv().await {
            Ok(event) => match event.event_type.as_str() {
                "session.run.started" => {
                    let session_id = event_session_id(&event);
                    let run_id = event_run_id(&event);
                    if let (Some(session_id), Some(run_id)) = (session_id, run_id) {
                        let started_at_ms = event
                            .properties
                            .get("startedAtMs")
                            .and_then(|v| v.as_u64())
                            .unwrap_or_else(crate::now_ms);
                        let user_id = event
                            .properties
                            .get("clientID")
                            .and_then(|v| v.as_str())
                            .filter(|v| !v.trim().is_empty())
                            .unwrap_or("default")
                            .to_string();
                        let host_tag = event
                            .properties
                            .get("environment")
                            .and_then(|v| v.get("os"))
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string);
                        by_session.insert(
                            session_id,
                            RunMemoryContext {
                                run_id,
                                user_id,
                                started_at_ms,
                                host_tag,
                            },
                        );
                    }
                }
                "session.run.finished" => {
                    if let Some(session_id) = event_session_id(&event) {
                        if let Some(ctx) = by_session.remove(&session_id) {
                            ingest_run_messages(&state, &db, &session_id, &ctx).await;
                        }
                    }
                }
                "permission.asked" | "permission.replied" | "mcp.auth.required"
                | "mcp.auth.pending" | "todo.updated" | "question.asked" => {
                    ingest_event_memory_records(&state, &db, &event, &by_session).await;
                }
                _ => {}
            },
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

pub(super) async fn memory_put(
    State(state): State<AppState>,
    Json(input): Json<MemoryPutInput>,
) -> Result<Json<MemoryPutResponse>, StatusCode> {
    let response = memory_put_impl(&state, input.request, input.capability).await?;
    Ok(Json(response))
}

pub(super) async fn memory_put_impl(
    state: &AppState,
    request: MemoryPutRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryPutResponse, StatusCode> {
    let capability = validate_memory_capability(&request.run_id, &request.partition, capability)?;
    if !capability
        .memory
        .write_tiers
        .contains(&request.partition.tier)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    let id = Uuid::new_v4().to_string();
    let partition_key = request.partition.key();
    let kind = memory_kind_for_request(request.kind.clone());
    let now = crate::now_ms();
    let audit_id = Uuid::new_v4().to_string();
    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let artifact_refs = request.artifact_refs.clone();
    let source_type = match request.kind {
        tandem_memory::MemoryContentKind::SolutionCapsule => "solution_capsule",
        tandem_memory::MemoryContentKind::Note => "note",
        tandem_memory::MemoryContentKind::Fact => "fact",
    }
    .to_string();
    let user_id = capability.subject.clone();
    let record = GlobalMemoryRecord {
        id: id.clone(),
        user_id,
        source_type,
        content: request.content.clone(),
        content_hash: String::new(),
        run_id: request.run_id.clone(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: Some(request.partition.project_id.clone()),
        channel_tag: None,
        host_tag: None,
        metadata: memory_metadata_with_storage_fields(
            request.metadata.clone(),
            &artifact_refs,
            request.classification,
        ),
        provenance: Some(memory_put_provenance(
            &request,
            &partition_key,
            &artifact_refs,
        )),
        redaction_status: "passed".to_string(),
        redaction_count: 0,
        visibility: "private".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: now,
        updated_at_ms: now,
        expires_at_ms: None,
    };
    persist_global_memory_record(&state, &db, record).await;
    append_memory_audit(
        &state,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_put".to_string(),
            run_id: request.run_id.clone(),
            memory_id: Some(id.clone()),
            source_memory_id: None,
            to_tier: Some(request.partition.tier),
            partition_key: partition_key.clone(),
            actor: capability.subject,
            status: "ok".to_string(),
            detail: None,
            created_at_ms: now,
        },
    )
    .await?;
    state.event_bus.publish(EngineEvent::new(
        "memory.put",
        json!({
            "runID": request.run_id,
            "memoryID": id,
            "kind": kind,
            "classification": request.classification,
            "artifactRefs": artifact_refs,
            "visibility": "private",
            "tier": request.partition.tier,
            "partitionKey": partition_key,
            "auditID": audit_id,
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "memory.updated",
        json!({
            "memoryID": id,
            "runID": request.run_id,
            "action": "put",
            "kind": kind,
            "classification": request.classification,
            "artifactRefs": artifact_refs,
            "visibility": "private",
            "tier": request.partition.tier,
            "partitionKey": partition_key,
            "auditID": audit_id,
        }),
    ));
    Ok(MemoryPutResponse {
        id,
        stored: true,
        tier: request.partition.tier,
        partition_key,
        audit_id,
    })
}

pub(super) async fn memory_promote(
    State(state): State<AppState>,
    Json(input): Json<MemoryPromoteInput>,
) -> Result<Json<MemoryPromoteResponse>, StatusCode> {
    let response = memory_promote_impl(&state, input.request, input.capability).await?;
    Ok(Json(response))
}

pub(super) async fn memory_promote_impl(
    state: &AppState,
    request: MemoryPromoteRequest,
    capability: Option<MemoryCapabilityToken>,
) -> Result<MemoryPromoteResponse, StatusCode> {
    let source_memory_id = request.source_memory_id.clone();
    let capability = validate_memory_capability(&request.run_id, &request.partition, capability)?;
    if !capability.memory.promote_targets.contains(&request.to_tier) {
        return Err(StatusCode::FORBIDDEN);
    }
    if capability.memory.require_review_for_promote
        && (request.review.approval_id.is_none() || request.review.reviewer_id.is_none())
    {
        return Err(StatusCode::FORBIDDEN);
    }
    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let source = db
        .get_global_memory(&request.source_memory_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some(source) = source else {
        let scrub_report = ScrubReport {
            status: ScrubStatus::Blocked,
            redactions: 0,
            block_reason: Some("source memory missing or previously blocked".to_string()),
        };
        let audit_id = Uuid::new_v4().to_string();
        let partition_key = format!(
            "{}/{}/{}/{}",
            request.partition.org_id,
            request.partition.workspace_id,
            request.partition.project_id,
            request.to_tier
        );
        append_memory_audit(
            &state,
            crate::MemoryAuditEvent {
                audit_id: audit_id.clone(),
                action: "memory_promote".to_string(),
                run_id: request.run_id.clone(),
                memory_id: None,
                source_memory_id: Some(source_memory_id.clone()),
                to_tier: Some(request.to_tier),
                partition_key: partition_key.clone(),
                actor: capability.subject,
                status: "blocked".to_string(),
                detail: scrub_report.block_reason.clone(),
                created_at_ms: crate::now_ms(),
            },
        )
        .await?;
        state.event_bus.publish(EngineEvent::new(
            "memory.promote",
            json!({
                "runID": request.run_id,
                "sourceMemoryID": source_memory_id,
                "toTier": request.to_tier,
                "partitionKey": partition_key,
                "status": "blocked",
                "kind": Value::Null,
                "classification": Value::Null,
                "artifactRefs": [],
                "visibility": Value::Null,
                "scrubStatus": scrub_report.status,
                "detail": scrub_report.block_reason.clone(),
                "auditID": audit_id,
            }),
        ));
        return Ok(MemoryPromoteResponse {
            promoted: false,
            new_memory_id: None,
            to_tier: request.to_tier,
            scrub_report,
            audit_id,
        });
    };
    let scrub_report = scrub_content(&source.content);
    let audit_id = Uuid::new_v4().to_string();
    let now = crate::now_ms();
    let partition_key = format!(
        "{}/{}/{}/{}",
        request.partition.org_id,
        request.partition.workspace_id,
        request.partition.project_id,
        request.to_tier
    );
    if scrub_report.status == ScrubStatus::Blocked {
        append_memory_audit(
            &state,
            crate::MemoryAuditEvent {
                audit_id: audit_id.clone(),
                action: "memory_promote".to_string(),
                run_id: request.run_id.clone(),
                memory_id: None,
                source_memory_id: Some(source_memory_id.clone()),
                to_tier: Some(request.to_tier),
                partition_key: partition_key.clone(),
                actor: capability.subject,
                status: "blocked".to_string(),
                detail: scrub_report.block_reason.clone(),
                created_at_ms: now,
            },
        )
        .await?;
        state.event_bus.publish(EngineEvent::new(
            "memory.promote",
            json!({
                "runID": request.run_id,
                "sourceMemoryID": source_memory_id,
                "toTier": request.to_tier,
                "partitionKey": partition_key,
                "status": "blocked",
                "kind": memory_kind_label(&source.source_type),
                "classification": memory_classification_label(source.metadata.as_ref()),
                "artifactRefs": memory_artifact_refs(source.metadata.as_ref()),
                "visibility": source.visibility,
                "scrubStatus": scrub_report.status,
                "detail": scrub_report.block_reason.clone(),
                "auditID": audit_id,
            }),
        ));
        return Ok(MemoryPromoteResponse {
            promoted: false,
            new_memory_id: None,
            to_tier: request.to_tier,
            scrub_report,
            audit_id,
        });
    }
    let new_id = source.id.clone();
    let next_metadata = memory_promote_metadata(source.metadata.as_ref(), &request, now);
    let next_provenance =
        memory_promote_provenance(source.provenance.as_ref(), &request, &partition_key, now);
    let classification = memory_classification_label(next_metadata.as_ref());
    let artifact_refs = memory_artifact_refs(next_metadata.as_ref());
    let kind = memory_kind_label(&source.source_type);
    db.update_global_memory_context(
        &new_id,
        "shared",
        false,
        next_metadata.as_ref(),
        Some(&next_provenance),
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    append_memory_audit(
        &state,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_promote".to_string(),
            run_id: request.run_id.clone(),
            memory_id: Some(new_id.clone()),
            source_memory_id: Some(source_memory_id.clone()),
            to_tier: Some(request.to_tier),
            partition_key: format!(
                "{}/{}/{}/{}",
                request.partition.org_id,
                request.partition.workspace_id,
                request.partition.project_id,
                request.to_tier
            ),
            actor: capability.subject,
            status: "ok".to_string(),
            detail: None,
            created_at_ms: now,
        },
    )
    .await?;
    state.event_bus.publish(EngineEvent::new(
        "memory.promote",
        json!({
            "runID": request.run_id,
            "sourceMemoryID": source_memory_id,
            "memoryID": new_id,
            "kind": kind,
            "classification": classification,
            "artifactRefs": artifact_refs,
            "visibility": "shared",
            "toTier": request.to_tier,
            "partitionKey": partition_key,
            "approvalID": request.review.approval_id,
            "auditID": audit_id,
            "scrubStatus": scrub_report.status,
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "memory.updated",
        json!({
            "memoryID": new_id,
            "runID": request.run_id,
            "action": "promote",
            "kind": kind,
            "classification": classification,
            "artifactRefs": artifact_refs,
            "visibility": "shared",
            "tier": request.to_tier,
            "partitionKey": partition_key,
            "sourceMemoryID": source_memory_id,
            "approvalID": request.review.approval_id,
            "auditID": audit_id,
        }),
    ));
    Ok(MemoryPromoteResponse {
        promoted: true,
        new_memory_id: Some(new_id),
        to_tier: request.to_tier,
        scrub_report,
        audit_id,
    })
}

pub(super) async fn memory_search(
    State(state): State<AppState>,
    Json(input): Json<MemorySearchInput>,
) -> Result<Json<MemorySearchResponse>, StatusCode> {
    let request = input.request;
    let capability =
        validate_memory_capability(&request.run_id, &request.partition, input.capability)?;
    let requested_scopes = if request.read_scopes.is_empty() {
        capability.memory.read_tiers.clone()
    } else {
        request.read_scopes.clone()
    };
    let mut scopes_used = Vec::new();
    let mut blocked_scopes = Vec::new();
    for scope in &requested_scopes {
        if capability.memory.read_tiers.contains(scope) {
            scopes_used.push(scope.clone());
        } else {
            blocked_scopes.push(scope.clone());
        }
    }
    let allow_private_results = scopes_used
        .iter()
        .any(|scope| matches!(scope, tandem_memory::GovernedMemoryTier::Session));
    let limit = request.limit.unwrap_or(8).clamp(1, 100);
    let hits = if scopes_used.is_empty() {
        Vec::new()
    } else {
        let db = open_global_memory_db()
            .await
            .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
        db.search_global_memory(
            &capability.subject,
            &request.query,
            limit,
            Some(&request.partition.project_id),
            None,
            None,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .filter(|hit| allow_private_results || hit.record.visibility.eq_ignore_ascii_case("shared"))
        .collect::<Vec<_>>()
    };
    let results = hits
        .into_iter()
        .map(|hit| {
            json!({
                "id": hit.record.id,
                "tier": memory_tier_for_visibility(&hit.record.visibility),
                "classification": memory_classification_label(hit.record.metadata.as_ref()),
                "kind": memory_kind_label(&hit.record.source_type),
                "source_type": hit.record.source_type,
                "created_at_ms": hit.record.created_at_ms,
                "content": hit.record.content,
                "score": hit.score,
                "run_id": hit.record.run_id,
                "visibility": hit.record.visibility,
                "artifact_refs": memory_artifact_refs(hit.record.metadata.as_ref()),
                "linkage": memory_linkage(&hit.record),
                "metadata": hit.record.metadata,
                "provenance": hit.record.provenance,
            })
        })
        .collect::<Vec<_>>();
    let result_ids = results
        .iter()
        .filter_map(|row| row.get("id").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let result_kinds = results
        .iter()
        .filter_map(|row| row.get("kind").and_then(Value::as_str))
        .map(ToString::to_string)
        .collect::<Vec<_>>();
    let audit_id = Uuid::new_v4().to_string();
    let now = crate::now_ms();
    let search_status = if scopes_used.is_empty() && !blocked_scopes.is_empty() {
        "blocked"
    } else {
        "ok"
    };
    append_memory_audit(
        &state,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_search".to_string(),
            run_id: request.run_id.clone(),
            memory_id: None,
            source_memory_id: None,
            to_tier: None,
            partition_key: request.partition.key(),
            actor: capability.subject,
            status: search_status.to_string(),
            detail: (!blocked_scopes.is_empty()).then(|| {
                format!(
                    "requested_scopes={} blocked_scopes={}",
                    requested_scopes
                        .iter()
                        .map(|scope| scope.to_string())
                        .collect::<Vec<_>>()
                        .join(","),
                    blocked_scopes
                        .iter()
                        .map(|scope| scope.to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                )
            }),
            created_at_ms: now,
        },
    )
    .await?;
    state.event_bus.publish(EngineEvent::new(
        "memory.search",
        json!({
            "runID": request.run_id,
            "query": request.query,
            "partitionKey": request.partition.key(),
            "resultCount": results.len(),
            "resultIDs": result_ids,
            "resultKinds": result_kinds,
            "requestedScopes": requested_scopes,
            "scopesUsed": scopes_used.clone(),
            "blockedScopes": blocked_scopes.clone(),
            "status": search_status,
            "auditID": audit_id,
        }),
    ));
    Ok(Json(MemorySearchResponse {
        results,
        scopes_used,
        blocked_scopes,
        audit_id,
    }))
}

pub(super) async fn memory_demote(
    State(state): State<AppState>,
    Json(input): Json<MemoryDemoteInput>,
) -> Result<Json<Value>, StatusCode> {
    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let record = db
        .get_global_memory(&input.id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some(record) = record else {
        return Err(StatusCode::NOT_FOUND);
    };
    let changed = db
        .set_global_memory_visibility(&input.id, "private", true)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !changed {
        return Err(StatusCode::NOT_FOUND);
    }
    let partition_key = memory_linkage(&record)
        .get("partition_key")
        .and_then(Value::as_str)
        .unwrap_or("demoted")
        .to_string();
    let audit_id = Uuid::new_v4().to_string();
    append_memory_audit(
        &state,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_demote".to_string(),
            run_id: input.run_id.clone(),
            memory_id: Some(input.id.clone()),
            source_memory_id: None,
            to_tier: None,
            partition_key: partition_key.clone(),
            actor: "system".to_string(),
            status: "ok".to_string(),
            detail: None,
            created_at_ms: crate::now_ms(),
        },
    )
    .await?;
    state.event_bus.publish(EngineEvent::new(
        "memory.updated",
        json!({
            "memoryID": input.id,
            "runID": input.run_id,
            "action": "demote",
            "kind": memory_kind_label(&record.source_type),
            "classification": memory_classification_label(record.metadata.as_ref()),
            "artifactRefs": memory_artifact_refs(record.metadata.as_ref()),
            "visibility": "private",
            "tier": tandem_memory::GovernedMemoryTier::Session,
            "partitionKey": partition_key,
            "demoted": true,
            "auditID": audit_id,
        }),
    ));
    Ok(Json(json!({
        "ok": true,
        "audit_id": audit_id,
    })))
}

pub(super) async fn memory_audit(
    State(state): State<AppState>,
    Query(query): Query<MemoryAuditQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let mut entries = state.memory_audit_log.read().await.clone();
    if let Some(run_id) = query.run_id {
        entries.retain(|event| event.run_id == run_id);
    }
    entries.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
    entries.truncate(limit);
    Json(json!({
        "events": entries,
        "count": entries.len(),
    }))
}

pub(super) async fn memory_list(
    State(_state): State<AppState>,
    Query(query): Query<MemoryListQuery>,
) -> Json<Value> {
    let q = query.q.unwrap_or_default();
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let offset = query.offset.unwrap_or(0);
    let user_id = query.user_id.unwrap_or_else(|| "default".to_string());
    let page = if let Some(db) = open_global_memory_db().await {
        db.list_global_memory(&user_id, Some(&q), limit as i64, offset as i64)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|row| {
                json!({
                    "id": row.id,
                    "user_id": row.user_id,
                    "run_id": row.run_id,
                    "tier": memory_tier_for_visibility(&row.visibility),
                    "classification": memory_classification_label(row.metadata.as_ref()),
                    "kind": memory_kind_label(&row.source_type),
                    "source_type": row.source_type,
                    "content": row.content,
                    "artifact_refs": memory_artifact_refs(row.metadata.as_ref()),
                    "linkage": memory_linkage(&row),
                    "metadata": row.metadata,
                    "provenance": row.provenance,
                    "created_at_ms": row.created_at_ms,
                    "updated_at_ms": row.updated_at_ms,
                    "visibility": row.visibility,
                    "demoted": row.demoted,
                })
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let total = page.len();
    Json(json!({
        "items": page,
        "count": total,
        "offset": offset,
        "limit": limit,
    }))
}

pub(super) async fn memory_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let record = db
        .get_global_memory(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some(record) = record else {
        return Err(StatusCode::NOT_FOUND);
    };
    let deleted = db
        .delete_global_memory(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !deleted {
        return Err(StatusCode::NOT_FOUND);
    }
    let now = crate::now_ms();
    let audit_id = Uuid::new_v4().to_string();
    let run_id = record.run_id.clone();
    append_memory_audit(
        &state,
        crate::MemoryAuditEvent {
            audit_id: audit_id.clone(),
            action: "memory_delete".to_string(),
            run_id: run_id.clone(),
            memory_id: Some(id.clone()),
            source_memory_id: None,
            to_tier: None,
            partition_key: record
                .project_tag
                .clone()
                .unwrap_or_else(|| "global".to_string()),
            actor: "admin".to_string(),
            status: "ok".to_string(),
            detail: None,
            created_at_ms: now,
        },
    )
    .await?;
    state.event_bus.publish(EngineEvent::new(
        "memory.deleted",
        json!({
            "memoryID": id,
            "runID": run_id,
            "kind": memory_kind_label(&record.source_type),
            "classification": memory_classification_label(record.metadata.as_ref()),
            "artifactRefs": memory_artifact_refs(record.metadata.as_ref()),
            "visibility": record.visibility,
            "tier": memory_tier_for_visibility(&record.visibility),
            "partitionKey": memory_linkage(&record)
                .get("partition_key")
                .and_then(Value::as_str),
            "demoted": record.demoted,
            "auditID": audit_id,
        }),
    ));
    Ok(Json(json!({
        "ok": true,
        "audit_id": audit_id,
    })))
}
