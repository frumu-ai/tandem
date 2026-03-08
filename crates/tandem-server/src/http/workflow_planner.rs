use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowPlanPreviewRequest {
    pub prompt: String,
    #[serde(default)]
    pub schedule: Option<Value>,
    #[serde(default)]
    pub plan_source: Option<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub operator_preferences: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowPlanApplyRequest {
    #[serde(default)]
    pub plan_id: Option<String>,
    #[serde(default)]
    pub plan: Option<crate::WorkflowPlan>,
    #[serde(default)]
    pub creator_id: Option<String>,
    #[serde(default)]
    pub pack_builder_export: Option<WorkflowPlanPackBuilderExportRequest>,
}

#[derive(Debug, Deserialize, Clone, Default)]
pub(super) struct WorkflowPlanPackBuilderExportRequest {
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub thread_key: Option<String>,
    #[serde(default)]
    pub auto_apply: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowPlanChatStartRequest {
    pub prompt: String,
    #[serde(default)]
    pub schedule: Option<Value>,
    #[serde(default)]
    pub plan_source: Option<String>,
    #[serde(default)]
    pub allowed_mcp_servers: Vec<String>,
    #[serde(default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub operator_preferences: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowPlanChatMessageRequest {
    pub plan_id: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowPlanChatResetRequest {
    pub plan_id: String,
}

pub(super) async fn workflow_plan_preview(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanPreviewRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let prompt = input.prompt.trim();
    if prompt.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "prompt is required",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    if let Some(workspace_root) = input.workspace_root.as_deref() {
        crate::normalize_absolute_workspace_root(workspace_root).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    }
    let plan = build_workflow_plan(
        &state,
        prompt,
        input.schedule.as_ref(),
        input.plan_source.as_deref().unwrap_or("unknown"),
        input.allowed_mcp_servers,
        input.workspace_root.as_deref(),
        input.operator_preferences,
    )
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    state
        .put_workflow_plan_draft(workflow_plan_draft_from_plan(plan.clone()))
        .await;
    Ok(Json(json!({ "plan": plan })))
}

pub(super) async fn workflow_plan_chat_start(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanChatStartRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let prompt = input.prompt.trim();
    if prompt.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "prompt is required",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    if let Some(workspace_root) = input.workspace_root.as_deref() {
        crate::normalize_absolute_workspace_root(workspace_root).map_err(|error| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": error,
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            )
        })?;
    }
    let plan = build_workflow_plan(
        &state,
        prompt,
        input.schedule.as_ref(),
        input.plan_source.as_deref().unwrap_or("unknown"),
        input.allowed_mcp_servers,
        input.workspace_root.as_deref(),
        input.operator_preferences,
    )
    .await
    .map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;
    let draft = workflow_plan_draft_from_plan(plan.clone());
    state.put_workflow_plan_draft(draft.clone()).await;
    Ok(Json(json!({
        "plan": draft.current_plan,
        "conversation": draft.conversation,
    })))
}

pub(super) async fn workflow_plan_get(
    State(state): State<AppState>,
    axum::extract::Path(plan_id): axum::extract::Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(draft) = state.get_workflow_plan_draft(&plan_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "workflow plan not found",
                "code": "WORKFLOW_PLAN_NOT_FOUND",
                "plan_id": plan_id,
            })),
        ));
    };
    Ok(Json(json!({
        "plan": draft.current_plan,
        "conversation": draft.conversation,
    })))
}

pub(super) async fn workflow_plan_chat_message(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanChatMessageRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let plan_id = input.plan_id.trim();
    let message = input.message.trim();
    if plan_id.is_empty() || message.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "plan_id and message are required",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    let Some(mut draft) = state.get_workflow_plan_draft(plan_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "workflow plan not found",
                "code": "WORKFLOW_PLAN_NOT_FOUND",
                "plan_id": plan_id,
            })),
        ));
    };
    let user_message = crate::WorkflowPlanChatMessage {
        role: "user".to_string(),
        text: message.to_string(),
        created_at_ms: crate::now_ms(),
    };
    draft.conversation.updated_at_ms = user_message.created_at_ms;
    draft.conversation.messages.push(user_message);
    let (revised_plan, assistant_text, change_summary, clarifier) =
        revise_workflow_plan_from_message(&draft.current_plan, message);
    draft.current_plan = revised_plan.clone();
    draft
        .conversation
        .messages
        .push(crate::WorkflowPlanChatMessage {
            role: "assistant".to_string(),
            text: assistant_text.clone(),
            created_at_ms: crate::now_ms(),
        });
    draft.conversation.updated_at_ms = crate::now_ms();
    state.put_workflow_plan_draft(draft.clone()).await;
    Ok(Json(json!({
        "plan": draft.current_plan,
        "conversation": draft.conversation,
        "assistant_message": {
            "role": "assistant",
            "text": assistant_text,
        },
        "change_summary": change_summary,
        "clarifier": clarifier,
    })))
}

pub(super) async fn workflow_plan_chat_reset(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanChatResetRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let plan_id = input.plan_id.trim();
    if plan_id.is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "plan_id is required",
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        ));
    }
    let Some(mut draft) = state.get_workflow_plan_draft(plan_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "workflow plan not found",
                "code": "WORKFLOW_PLAN_NOT_FOUND",
                "plan_id": plan_id,
            })),
        ));
    };
    draft.current_plan = draft.initial_plan.clone();
    draft
        .conversation
        .messages
        .push(crate::WorkflowPlanChatMessage {
            role: "system".to_string(),
            text: "Plan reset to the initial preview.".to_string(),
            created_at_ms: crate::now_ms(),
        });
    draft.conversation.updated_at_ms = crate::now_ms();
    state.put_workflow_plan_draft(draft.clone()).await;
    Ok(Json(json!({
        "plan": draft.current_plan,
        "conversation": draft.conversation,
    })))
}

pub(super) async fn workflow_plan_apply(
    State(state): State<AppState>,
    Json(input): Json<WorkflowPlanApplyRequest>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let plan = match (
        input.plan,
        input
            .plan_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty()),
    ) {
        (Some(plan), _) => plan,
        (None, Some(plan_id)) => state.get_workflow_plan(plan_id).await.ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error": "workflow plan not found",
                    "code": "WORKFLOW_PLAN_NOT_FOUND",
                    "plan_id": plan_id,
                })),
            )
        })?,
        (None, None) => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "plan or plan_id is required",
                    "code": "WORKFLOW_PLAN_INVALID",
                })),
            ));
        }
    };
    validate_workflow_plan(&plan).map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error,
                "code": "WORKFLOW_PLAN_INVALID",
            })),
        )
    })?;

    let automation = compile_plan_to_automation_v2(
        &plan,
        input.creator_id.as_deref().unwrap_or("workflow_planner"),
    );
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error.to_string(),
                "code": "WORKFLOW_PLAN_APPLY_FAILED",
            })),
        )
    })?;
    let pack_builder_export = match input.pack_builder_export {
        Some(export) if export.enabled.unwrap_or(true) => {
            Some(export_workflow_plan_to_pack_builder(&state, &plan, &export).await)
        }
        _ => None,
    };
    Ok(Json(json!({
        "ok": true,
        "plan": plan,
        "automation": stored,
        "pack_builder_export": pack_builder_export,
    })))
}

async fn export_workflow_plan_to_pack_builder(
    state: &AppState,
    plan: &crate::WorkflowPlan,
    export: &WorkflowPlanPackBuilderExportRequest,
) -> Value {
    let goal = plan
        .original_prompt
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let goal = if goal.is_empty() {
        plan.title.clone()
    } else {
        goal
    };
    let args = json!({
        "mode": "preview",
        "goal": goal,
        "__session_id": export.session_id,
        "thread_key": export.thread_key,
        "auto_apply": export.auto_apply.unwrap_or(false),
        "schedule": pack_builder_schedule_from_plan(&plan.schedule),
    });
    match super::pack_builder::run_pack_builder_tool(state, args).await {
        Ok(payload) => payload,
        Err(code) => json!({
            "status": "export_failed",
            "error": "pack_builder_export_failed",
            "http_status": code.as_u16(),
        }),
    }
}

fn pack_builder_schedule_from_plan(schedule: &crate::AutomationV2Schedule) -> Value {
    match schedule.schedule_type {
        crate::AutomationV2ScheduleType::Cron => schedule
            .cron_expression
            .as_ref()
            .map(|expression| {
                json!({
                    "cron": expression,
                    "timezone": schedule.timezone,
                })
            })
            .unwrap_or(Value::Null),
        crate::AutomationV2ScheduleType::Interval => json!({
            "interval_seconds": schedule.interval_seconds.unwrap_or(86_400),
            "timezone": schedule.timezone,
        }),
        crate::AutomationV2ScheduleType::Manual => Value::Null,
    }
}

async fn build_workflow_plan(
    state: &AppState,
    prompt: &str,
    explicit_schedule: Option<&Value>,
    plan_source: &str,
    allowed_mcp_servers: Vec<String>,
    workspace_root: Option<&str>,
    operator_preferences: Option<Value>,
) -> Result<crate::WorkflowPlan, String> {
    let normalized_prompt = normalize_prompt(prompt);
    let schedule = normalize_schedule(explicit_schedule, prompt);
    let (confidence, steps, description) = choose_plan_shape(&normalized_prompt);
    let title = plan_title(prompt, &schedule.schedule_type);
    let workspace_root = resolve_workspace_root(state, workspace_root).await?;
    Ok(crate::WorkflowPlan {
        plan_id: format!("wfplan-{}", Uuid::new_v4()),
        planner_version: "v1".to_string(),
        plan_source: plan_source.to_string(),
        original_prompt: prompt.trim().to_string(),
        normalized_prompt,
        confidence: confidence.to_string(),
        title,
        description: Some(description),
        schedule,
        execution_target: "automation_v2".to_string(),
        workspace_root,
        steps,
        requires_integrations: Vec::new(),
        allowed_mcp_servers: normalize_string_list(allowed_mcp_servers),
        operator_preferences: normalize_operator_preferences(operator_preferences),
        save_options: json!({
            "can_export_pack": true,
            "can_save_skill": true,
        }),
    })
}

fn normalize_operator_preferences(raw: Option<Value>) -> Option<Value> {
    let Some(mut prefs) = raw else {
        return None;
    };
    let Some(map) = prefs.as_object_mut() else {
        return None;
    };
    if let Some(mode) = map
        .get("execution_mode")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        map.insert(
            "execution_mode".to_string(),
            Value::String(mode.to_string()),
        );
    } else {
        map.remove("execution_mode");
    }
    if let Some(max_parallel) = map.get("max_parallel_agents").and_then(Value::as_u64) {
        map.insert(
            "max_parallel_agents".to_string(),
            Value::Number(serde_json::Number::from(max_parallel.clamp(1, 16))),
        );
    } else {
        map.remove("max_parallel_agents");
    }
    if let Some(provider_id) = map
        .get("model_provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        map.insert(
            "model_provider".to_string(),
            Value::String(provider_id.to_string()),
        );
    } else {
        map.remove("model_provider");
    }
    if let Some(model_id) = map
        .get("model_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        map.insert("model_id".to_string(), Value::String(model_id.to_string()));
    } else {
        map.remove("model_id");
    }
    if !map.get("role_models").is_some_and(Value::is_object) {
        map.remove("role_models");
    }
    if map.is_empty() {
        None
    } else {
        Some(prefs)
    }
}

async fn resolve_workspace_root(
    state: &AppState,
    requested: Option<&str>,
) -> Result<String, String> {
    let requested = requested.map(str::trim).filter(|value| !value.is_empty());
    if let Some(workspace_root) = requested {
        return crate::normalize_absolute_workspace_root(workspace_root);
    }
    let root = state.workspace_index.snapshot().await.root;
    match crate::normalize_absolute_workspace_root(&root) {
        Ok(normalized) => Ok(normalized),
        Err(error) => {
            #[cfg(unix)]
            {
                if root.starts_with('\\') {
                    let unix_like = root.replace('\\', "/");
                    return crate::normalize_absolute_workspace_root(&unix_like);
                }
            }
            let cwd = std::env::current_dir().map_err(|_| error.clone())?;
            crate::normalize_absolute_workspace_root(cwd.to_string_lossy().as_ref())
        }
    }
}

fn normalize_string_list(raw: Vec<String>) -> Vec<String> {
    let mut values = raw
        .into_iter()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    values
}

fn workflow_plan_draft_from_plan(plan: crate::WorkflowPlan) -> crate::WorkflowPlanDraftRecord {
    let now = crate::now_ms();
    crate::WorkflowPlanDraftRecord {
        initial_plan: plan.clone(),
        current_plan: plan.clone(),
        conversation: crate::WorkflowPlanConversation {
            conversation_id: format!("wfchat-{}", Uuid::new_v4()),
            plan_id: plan.plan_id.clone(),
            created_at_ms: now,
            updated_at_ms: now,
            messages: Vec::new(),
        },
    }
}

fn revise_workflow_plan_from_message(
    current_plan: &crate::WorkflowPlan,
    message: &str,
) -> (crate::WorkflowPlan, String, Vec<String>, Value) {
    let text = message.trim().to_ascii_lowercase();
    let mut revised = current_plan.clone();
    let mut changes = Vec::new();
    let mut clarifier = Value::Null;

    if let Some(schedule) = schedule_from_revision_message(message) {
        if revised.schedule.schedule_type != schedule.schedule_type
            || revised.schedule.cron_expression != schedule.cron_expression
            || revised.schedule.interval_seconds != schedule.interval_seconds
        {
            revised.schedule = schedule;
            changes.push("updated schedule".to_string());
        }
    }

    if let Some(path) = extract_workspace_root(message) {
        match crate::normalize_absolute_workspace_root(&path) {
            Ok(normalized) => {
                if revised.workspace_root != normalized {
                    revised.workspace_root = normalized;
                    changes.push("updated workspace root".to_string());
                }
            }
            Err(error) => {
                clarifier = json!({
                    "field": "workspace_root",
                    "question": error,
                });
            }
        }
    }

    if let Some(updated_title) = extract_title_from_revision_message(message) {
        if revised.title != updated_title {
            revised.title = updated_title;
            changes.push("updated title".to_string());
        }
    }

    let wants_add_analysis = text.contains("add analysis")
        || text.contains("add an analysis step")
        || text.contains("analyze findings")
        || text.contains("analyze before reporting")
        || text.contains("analyze before report")
        || text.contains("split analysis");
    if wants_add_analysis && ensure_analysis_step(&mut revised) {
        changes.push("added analysis step".to_string());
    }

    let wants_remove_analysis = text.contains("remove analysis")
        || text.contains("skip analysis")
        || text.contains("without analysis")
        || text.contains("no analysis")
        || text.contains("report directly");
    if wants_remove_analysis && remove_analysis_step(&mut revised) {
        changes.push("removed analysis step".to_string());
    }

    if let Some(updated_preferences) =
        revise_operator_preferences(revised.operator_preferences.clone(), &text)
    {
        if revised.operator_preferences.as_ref() != Some(&updated_preferences) {
            let execution_mode_changed = revised
                .operator_preferences
                .as_ref()
                .and_then(|prefs| prefs.get("execution_mode"))
                != updated_preferences.get("execution_mode");
            let max_parallel_changed = revised
                .operator_preferences
                .as_ref()
                .and_then(|prefs| prefs.get("max_parallel_agents"))
                != updated_preferences.get("max_parallel_agents");
            let model_provider_changed = revised
                .operator_preferences
                .as_ref()
                .and_then(|prefs| prefs.get("model_provider"))
                != updated_preferences.get("model_provider");
            let model_id_changed = revised
                .operator_preferences
                .as_ref()
                .and_then(|prefs| prefs.get("model_id"))
                != updated_preferences.get("model_id");
            revised.operator_preferences = Some(updated_preferences);
            if execution_mode_changed {
                changes.push("updated execution mode".to_string());
            }
            if max_parallel_changed {
                changes.push("updated max parallel agents".to_string());
            }
            if model_provider_changed || model_id_changed {
                changes.push("updated model override".to_string());
            }
        }
    }

    let clear_mcp_servers = text.contains("remove all mcp")
        || text.contains("clear mcp")
        || text.contains("no mcp")
        || text.contains("without mcp")
        || text.contains("disable mcp");
    if clear_mcp_servers
        || text.contains("github")
        || text.contains("slack")
        || text.contains("notion")
    {
        let github_only = text.contains("github only");
        let slack_only = text.contains("slack only");
        let notion_only = text.contains("notion only");
        let mut servers = if github_only {
            vec!["github".to_string()]
        } else if slack_only {
            vec!["slack".to_string()]
        } else if notion_only {
            vec!["notion".to_string()]
        } else {
            revised.allowed_mcp_servers.clone()
        };
        if clear_mcp_servers {
            servers.clear();
        }
        if text.contains("use github") || github_only || text.contains("github and") {
            servers.push("github".to_string());
        }
        if text.contains("use slack") || slack_only || text.contains("slack and") {
            servers.push("slack".to_string());
        }
        if text.contains("use notion") || notion_only || text.contains("notion and") {
            servers.push("notion".to_string());
        }
        if text.contains("remove github") {
            servers.retain(|server| server != "github");
        }
        if text.contains("remove slack") {
            servers.retain(|server| server != "slack");
        }
        if text.contains("remove notion") {
            servers.retain(|server| server != "notion");
        }
        let normalized = normalize_string_list(servers);
        if normalized != revised.allowed_mcp_servers {
            revised.allowed_mcp_servers = normalized;
            changes.push("updated allowed MCP servers".to_string());
        }
    }

    let wants_no_notify = text.contains("don't notify")
        || text.contains("do not notify")
        || text.contains("remove notify")
        || text.contains("remove notification");
    if wants_no_notify {
        if remove_notify_step(&mut revised) {
            changes.push("removed notification step".to_string());
        }
    }

    let wants_add_notify = text.contains("notify me")
        || text.contains("send notification")
        || text.contains("add notification")
        || text.contains("add notify")
        || text.contains("post notification")
        || text.contains("send alert")
        || text.contains("notify user");
    if wants_add_notify && ensure_notify_step(&mut revised) {
        changes.push("added notification step".to_string());
    }

    if changes.is_empty() {
        if clarifier.is_object() {
            let assistant = clarifier
                .get("question")
                .and_then(Value::as_str)
                .map(|question| {
                    format!("I kept the current plan. Clarification needed: {question}")
                })
                .unwrap_or_else(|| {
                    "I kept the current plan because the revision request needs clarification."
                        .to_string()
                });
            return (revised, assistant, Vec::new(), clarifier);
        }
        let clarifier = json!({
            "field": "general",
            "question": supported_planner_revision_hint(),
        });
        let assistant = format!(
            "I kept the current plan. Clarification needed: {}",
            supported_planner_revision_hint()
        );
        return (revised, assistant, Vec::new(), clarifier);
    }

    let assistant = format!("Updated the plan: {}.", changes.join(", "));
    (revised, assistant, changes, clarifier)
}

fn supported_planner_revision_hint() -> &'static str {
    "Supported edits in this slice: title, schedule, workspace root, MCP servers, execution mode, model overrides, adding or removing analysis, and adding or removing notifications."
}

fn ensure_analysis_step(plan: &mut crate::WorkflowPlan) -> bool {
    if plan
        .steps
        .iter()
        .any(|step| step.step_id == "analyze_findings")
    {
        return false;
    }
    let Some(report_index) = plan
        .steps
        .iter()
        .position(|step| step.step_id == "generate_report")
    else {
        return false;
    };
    let report_input = plan.steps[report_index]
        .input_refs
        .first()
        .map(|row| row.from_step_id.clone())
        .or_else(|| plan.steps[report_index].depends_on.first().cloned())
        .unwrap_or_else(|| "research_sources".to_string());
    let analysis_input_alias = match report_input.as_str() {
        "collect_inputs" => "collected_inputs",
        "compare_results" => "comparison_findings",
        _ => "source_findings",
    };
    let analysis_step = plan_step_with_dep(
        "analyze_findings",
        "analyze",
        "Analyze the collected findings and identify the important takeaways.",
        "analyst",
        &[report_input.as_str()],
        vec![input_ref(&report_input, analysis_input_alias)],
        Some("structured_json"),
    );
    plan.steps.insert(report_index, analysis_step);
    if let Some(report_step) = plan
        .steps
        .iter_mut()
        .find(|step| step.step_id == "generate_report")
    {
        report_step.depends_on = vec!["analyze_findings".to_string()];
        report_step.input_refs = vec![input_ref("analyze_findings", "analysis")];
    }
    true
}

fn remove_analysis_step(plan: &mut crate::WorkflowPlan) -> bool {
    let Some(analysis_index) = plan
        .steps
        .iter()
        .position(|step| step.step_id == "analyze_findings")
    else {
        return false;
    };
    let analysis_step = plan.steps.remove(analysis_index);
    let fallback_dep = analysis_step
        .depends_on
        .first()
        .cloned()
        .or_else(|| {
            analysis_step
                .input_refs
                .first()
                .map(|row| row.from_step_id.clone())
        })
        .unwrap_or_else(|| "research_sources".to_string());
    let fallback_alias = match fallback_dep.as_str() {
        "collect_inputs" => "report_inputs",
        "compare_results" => "comparison_findings",
        _ => "source_findings",
    };
    if let Some(report_step) = plan
        .steps
        .iter_mut()
        .find(|step| step.step_id == "generate_report")
    {
        if report_step.depends_on == vec!["analyze_findings".to_string()] {
            report_step.depends_on = vec![fallback_dep.clone()];
            report_step.input_refs = vec![input_ref(&fallback_dep, fallback_alias)];
        } else {
            report_step
                .depends_on
                .retain(|dep| dep != "analyze_findings");
            report_step
                .input_refs
                .retain(|input| input.from_step_id != "analyze_findings");
        }
    }
    for step in &mut plan.steps {
        step.depends_on.retain(|dep| dep != "analyze_findings");
        step.input_refs
            .retain(|input| input.from_step_id != "analyze_findings");
    }
    true
}

fn remove_notify_step(plan: &mut crate::WorkflowPlan) -> bool {
    let original_len = plan.steps.len();
    plan.steps.retain(|step| step.step_id != "notify_user");
    for step in &mut plan.steps {
        step.depends_on.retain(|dep| dep != "notify_user");
        step.input_refs
            .retain(|input| input.from_step_id != "notify_user");
    }
    plan.steps.len() != original_len
}

fn ensure_notify_step(plan: &mut crate::WorkflowPlan) -> bool {
    if plan.steps.iter().any(|step| step.step_id == "notify_user") {
        return false;
    }
    let Some(last_step) = plan.steps.last() else {
        return false;
    };
    let (source_step_id, source_alias) = match last_step.step_id.as_str() {
        "generate_report" => ("generate_report", "report"),
        "compare_results" => ("compare_results", "comparison_findings"),
        "analyze_findings" => ("analyze_findings", "analysis"),
        "collect_inputs" => ("collect_inputs", "notification_inputs"),
        "research_sources" => ("research_sources", "notification_inputs"),
        "execute_goal" => ("execute_goal", "execution_output"),
        _ => (last_step.step_id.as_str(), "notification_inputs"),
    };
    plan.steps.push(plan_step_with_dep(
        "notify_user",
        "notify",
        "Prepare the final notification using the upstream workflow output.",
        "writer",
        &[source_step_id],
        vec![input_ref(source_step_id, source_alias)],
        Some("text_summary"),
    ));
    true
}

fn extract_title_from_revision_message(message: &str) -> Option<String> {
    let lowered = message.to_ascii_lowercase();
    let markers = [
        "rename this plan to",
        "rename this automation to",
        "rename this to",
        "rename plan to",
        "rename automation to",
        "rename to",
        "call this plan",
        "call this automation",
        "call this",
        "name this plan",
        "name this automation",
        "name this",
        "title this plan",
        "title this automation",
        "title this",
    ];
    for marker in markers {
        let Some(index) = lowered.find(marker) else {
            continue;
        };
        let mut remainder = message[index + marker.len()..].trim();
        if let Some(stripped) = remainder.strip_prefix("as ") {
            remainder = stripped.trim();
        }
        let remainder_lower = remainder.to_ascii_lowercase();
        let mut end = remainder.len();
        for separator in [" and ", ",", ".", ";", "\n"] {
            if let Some(position) = remainder_lower.find(separator) {
                end = end.min(position);
            }
        }
        let candidate = remainder[..end]
            .trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim();
        if candidate.is_empty() {
            continue;
        }
        return Some(crate::truncate_text(candidate, 120));
    }
    None
}

fn revise_operator_preferences(current: Option<Value>, text: &str) -> Option<Value> {
    let mut prefs = normalize_operator_preferences(current).unwrap_or_else(|| json!({}));
    let map = prefs.as_object_mut()?;
    let mut touched = false;

    if text.contains("single agent") || text.contains("single mode") || text.contains("use single")
    {
        map.insert(
            "execution_mode".to_string(),
            Value::String("single".to_string()),
        );
        map.insert(
            "max_parallel_agents".to_string(),
            Value::Number(serde_json::Number::from(1)),
        );
        touched = true;
    } else if text.contains("agent team") || text.contains("team mode") || text.contains("use team")
    {
        map.insert(
            "execution_mode".to_string(),
            Value::String("team".to_string()),
        );
        map.insert(
            "max_parallel_agents".to_string(),
            Value::Number(serde_json::Number::from(1)),
        );
        touched = true;
    } else if text.contains("swarm mode") || text.contains("use swarm") || text.contains("swarm") {
        let max_parallel = extract_max_parallel_agents(text).unwrap_or(4);
        map.insert(
            "execution_mode".to_string(),
            Value::String("swarm".to_string()),
        );
        map.insert(
            "max_parallel_agents".to_string(),
            Value::Number(serde_json::Number::from(max_parallel)),
        );
        touched = true;
    } else if let Some(max_parallel) = extract_max_parallel_agents(text) {
        map.insert(
            "max_parallel_agents".to_string(),
            Value::Number(serde_json::Number::from(max_parallel)),
        );
        touched = true;
    }

    let provider = extract_model_provider(text);
    let model_id = extract_model_id(text);
    let clear_model_override = text.contains("use default model")
        || text.contains("use the default model")
        || text.contains("use workspace default model")
        || text.contains("use the workspace default model")
        || text.contains("clear model override")
        || text.contains("clear model overrides")
        || text.contains("remove model override")
        || text.contains("remove model overrides");
    if clear_model_override {
        map.remove("model_provider");
        map.remove("model_id");
        map.remove("role_models");
        touched = true;
    }
    if let Some(provider) = provider {
        map.insert("model_provider".to_string(), Value::String(provider));
        touched = true;
    }
    if let Some(model_id) = model_id {
        map.insert("model_id".to_string(), Value::String(model_id));
        touched = true;
    }

    touched.then_some(prefs)
}

fn extract_max_parallel_agents(text: &str) -> Option<u64> {
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    for window in tokens.windows(2) {
        let Some(number) = window[0].parse::<u64>().ok() else {
            continue;
        };
        let noun = window[1]
            .trim_matches(|ch: char| !ch.is_ascii_alphabetic())
            .to_ascii_lowercase();
        if matches!(noun.as_str(), "agent" | "agents" | "workers") {
            return Some(number.clamp(1, 16));
        }
    }
    None
}

fn extract_model_provider(text: &str) -> Option<String> {
    [
        "openai",
        "anthropic",
        "openrouter",
        "google",
        "groq",
        "xai",
        "azure",
    ]
    .iter()
    .find(|provider| text.contains(**provider))
    .map(|provider| (*provider).to_string())
}

fn extract_model_id(text: &str) -> Option<String> {
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    for window in tokens.windows(2) {
        if window[0] == "model" {
            let candidate = sanitize_model_token(window[1]);
            if is_model_token(&candidate) {
                return Some(candidate);
            }
        }
    }
    tokens
        .into_iter()
        .map(sanitize_model_token)
        .find(|token| is_model_token(token))
}

fn sanitize_model_token(token: &str) -> String {
    token
        .trim_matches(|ch: char| {
            matches!(
                ch,
                '"' | '\'' | ',' | '.' | ':' | ';' | '(' | ')' | '[' | ']'
            )
        })
        .to_string()
}

fn is_model_token(token: &str) -> bool {
    !token.is_empty()
        && [
            "gpt", "claude", "gemini", "llama", "qwen", "sonnet", "opus", "haiku", "o1", "o3", "o4",
        ]
        .iter()
        .any(|needle| token.contains(needle))
}

fn extract_workspace_root(message: &str) -> Option<String> {
    message
        .split_whitespace()
        .find(|token| token.starts_with('/') || (token.contains('/') && !token.contains("://")))
        .map(|token| {
            token
                .trim_matches(|ch: char| matches!(ch, '"' | '\'' | ',' | '.'))
                .to_string()
        })
        .filter(|token| !token.is_empty())
}

fn validate_workflow_plan(plan: &crate::WorkflowPlan) -> Result<(), String> {
    crate::normalize_absolute_workspace_root(&plan.workspace_root)?;
    Ok(())
}

fn normalize_prompt(prompt: &str) -> String {
    prompt
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn normalize_schedule(explicit: Option<&Value>, prompt: &str) -> crate::AutomationV2Schedule {
    if let Some(schedule) = explicit.and_then(schedule_from_value) {
        return schedule;
    }
    schedule_from_prompt(prompt).unwrap_or(crate::AutomationV2Schedule {
        schedule_type: crate::AutomationV2ScheduleType::Manual,
        cron_expression: None,
        interval_seconds: None,
        timezone: "UTC".to_string(),
        misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
    })
}

fn schedule_from_value(value: &Value) -> Option<crate::AutomationV2Schedule> {
    let timezone = value
        .get("timezone")
        .and_then(Value::as_str)
        .unwrap_or("UTC")
        .to_string();
    let misfire_policy = crate::RoutineMisfirePolicy::RunOnce;
    if let Some(schedule_type) = value.get("type").and_then(Value::as_str) {
        match schedule_type.trim().to_ascii_lowercase().as_str() {
            "cron" => {
                let expr = value
                    .get("cron_expression")
                    .or_else(|| value.get("cronExpression"))
                    .and_then(Value::as_str)?
                    .trim()
                    .to_string();
                if expr.is_empty() {
                    return None;
                }
                return Some(crate::AutomationV2Schedule {
                    schedule_type: crate::AutomationV2ScheduleType::Cron,
                    cron_expression: Some(expr),
                    interval_seconds: None,
                    timezone,
                    misfire_policy,
                });
            }
            "interval" => {
                let seconds = value
                    .get("interval_seconds")
                    .or_else(|| value.get("intervalSeconds"))
                    .and_then(Value::as_u64)?;
                return Some(crate::AutomationV2Schedule {
                    schedule_type: crate::AutomationV2ScheduleType::Interval,
                    cron_expression: None,
                    interval_seconds: Some(seconds),
                    timezone,
                    misfire_policy,
                });
            }
            "manual" => {
                return Some(crate::AutomationV2Schedule {
                    schedule_type: crate::AutomationV2ScheduleType::Manual,
                    cron_expression: None,
                    interval_seconds: None,
                    timezone,
                    misfire_policy,
                });
            }
            _ => {}
        }
    }
    if let Some(expr) = value
        .get("cron")
        .and_then(|cron| {
            cron.get("expression")
                .or_else(|| cron.get("cron_expression"))
        })
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        return Some(crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Cron,
            cron_expression: Some(expr.to_string()),
            interval_seconds: None,
            timezone,
            misfire_policy,
        });
    }
    let seconds = value.get("interval_seconds").and_then(|row| {
        row.get("seconds")
            .and_then(Value::as_u64)
            .or_else(|| row.as_u64())
    });
    seconds.map(|seconds| crate::AutomationV2Schedule {
        schedule_type: crate::AutomationV2ScheduleType::Interval,
        cron_expression: None,
        interval_seconds: Some(seconds),
        timezone,
        misfire_policy,
    })
}

fn schedule_from_prompt(prompt: &str) -> Option<crate::AutomationV2Schedule> {
    let text = prompt.to_ascii_lowercase();
    let cron_expression = if text.contains("every morning") {
        Some("0 9 * * *")
    } else if text.contains("every evening") {
        Some("0 18 * * *")
    } else if text.contains("every hour") || text.contains("hourly") {
        None
    } else if text.contains("every day") || text.contains("daily") {
        Some("0 9 * * *")
    } else if text.contains("weekly") || text.contains("every monday") {
        Some("0 9 * * 1")
    } else {
        None
    };
    if text.contains("every hour") || text.contains("hourly") {
        return Some(crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Interval,
            cron_expression: None,
            interval_seconds: Some(3600),
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        });
    }
    cron_expression.map(|expr| crate::AutomationV2Schedule {
        schedule_type: crate::AutomationV2ScheduleType::Cron,
        cron_expression: Some(expr.to_string()),
        interval_seconds: None,
        timezone: "UTC".to_string(),
        misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
    })
}

fn schedule_from_revision_message(prompt: &str) -> Option<crate::AutomationV2Schedule> {
    let text = prompt.to_ascii_lowercase();
    if text.contains("manual only")
        || text.contains("run manually")
        || text.contains("manual schedule")
        || text.contains("on demand")
        || text.contains("do not schedule")
        || text.contains("don't schedule")
    {
        return Some(crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        });
    }
    schedule_from_prompt(prompt)
}

fn choose_plan_shape(
    normalized_prompt: &str,
) -> (&'static str, Vec<crate::WorkflowPlanStep>, String) {
    if contains_any(
        normalized_prompt,
        &["compare", "versus", "vs ", "difference"],
    ) {
        return (
            "high",
            vec![
                plan_step(
                    "collect_inputs",
                    "collect",
                    "Gather the inputs needed for comparison.",
                    "researcher",
                ),
                plan_step_with_dep(
                    "compare_results",
                    "compare",
                    "Compare the gathered inputs and identify the important differences.",
                    "analyst",
                    &["collect_inputs"],
                    vec![input_ref("collect_inputs", "comparison_inputs")],
                    Some("structured_json"),
                ),
                plan_step_with_dep(
                    "generate_report",
                    "report",
                    "Generate the final report from the comparison findings.",
                    "writer",
                    &["compare_results"],
                    vec![input_ref("compare_results", "comparison_findings")],
                    Some("report_markdown"),
                ),
            ],
            "Collect inputs, compare them, and produce a report.".to_string(),
        );
    }
    if contains_any(
        normalized_prompt,
        &[
            "research",
            "monitor",
            "watch",
            "digest",
            "summarize",
            "report",
        ],
    ) {
        return (
            "high",
            vec![
                plan_step_with_dep(
                    "research_sources",
                    "research",
                    "Collect current source material relevant to the prompt.",
                    "researcher",
                    &[],
                    Vec::new(),
                    Some("structured_json"),
                ),
                plan_step_with_dep(
                    "analyze_findings",
                    "analyze",
                    "Analyze the collected findings and identify the important takeaways.",
                    "analyst",
                    &["research_sources"],
                    vec![input_ref("research_sources", "source_findings")],
                    Some("structured_json"),
                ),
                plan_step_with_dep(
                    "generate_report",
                    "report",
                    "Generate a concise markdown report from the analyzed findings.",
                    "writer",
                    &["analyze_findings"],
                    vec![input_ref("analyze_findings", "analysis")],
                    Some("report_markdown"),
                ),
            ],
            "Research, analyze, and produce a report.".to_string(),
        );
    }
    if contains_any(normalized_prompt, &["notify", "alert", "post", "send"]) {
        return (
            "medium",
            vec![
                plan_step_with_dep(
                    "collect_inputs",
                    "collect",
                    "Collect the inputs needed before sending a notification.",
                    "researcher",
                    &[],
                    Vec::new(),
                    Some("structured_json"),
                ),
                plan_step_with_dep(
                    "notify_user",
                    "notify",
                    "Prepare the final notification using the collected inputs.",
                    "writer",
                    &["collect_inputs"],
                    vec![input_ref("collect_inputs", "notification_inputs")],
                    Some("text_summary"),
                ),
            ],
            "Collect the needed inputs and prepare a notification.".to_string(),
        );
    }
    if normalized_prompt.split_whitespace().count() >= 5 {
        return (
            "medium",
            vec![plan_step_with_dep(
                "execute_goal",
                "execute",
                "Execute the requested goal as a single automation step.",
                "worker",
                &[],
                Vec::new(),
                Some("structured_json"),
            )],
            "Execute the goal in a single step because the prompt is broad.".to_string(),
        );
    }
    (
        "low",
        vec![plan_step_with_dep(
            "execute_goal",
            "execute",
            "Execute the requested goal as a single automation step.",
            "worker",
            &[],
            Vec::new(),
            Some("structured_json"),
        )],
        "Use a single-step automation because the prompt is ambiguous.".to_string(),
    )
}

fn contains_any(input: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| input.contains(needle))
}

fn input_ref(from_step_id: &str, alias: &str) -> crate::AutomationFlowInputRef {
    crate::AutomationFlowInputRef {
        from_step_id: from_step_id.to_string(),
        alias: alias.to_string(),
    }
}

fn plan_step(
    step_id: &str,
    kind: &str,
    objective: &str,
    agent_role: &str,
) -> crate::WorkflowPlanStep {
    plan_step_with_dep(
        step_id,
        kind,
        objective,
        agent_role,
        &[],
        Vec::new(),
        Some("structured_json"),
    )
}

fn plan_step_with_dep(
    step_id: &str,
    kind: &str,
    objective: &str,
    agent_role: &str,
    depends_on: &[&str],
    input_refs: Vec<crate::AutomationFlowInputRef>,
    output_contract: Option<&str>,
) -> crate::WorkflowPlanStep {
    crate::WorkflowPlanStep {
        step_id: step_id.to_string(),
        kind: kind.to_string(),
        objective: objective.to_string(),
        depends_on: depends_on
            .iter()
            .map(|value| (*value).to_string())
            .collect(),
        agent_role: agent_role.to_string(),
        input_refs,
        output_contract: output_contract.map(|kind| crate::AutomationFlowOutputContract {
            kind: kind.to_string(),
        }),
    }
}

fn plan_title(prompt: &str, schedule_type: &crate::AutomationV2ScheduleType) -> String {
    let trimmed = prompt
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if trimmed.is_empty() {
        return "Automation Plan".to_string();
    }
    let base = if trimmed.len() > 64 {
        format!("{}...", &trimmed[..61])
    } else {
        trimmed
    };
    match schedule_type {
        crate::AutomationV2ScheduleType::Manual => base,
        _ => format!("Scheduled {}", base),
    }
}

fn compile_plan_to_automation_v2(
    plan: &crate::WorkflowPlan,
    creator_id: &str,
) -> crate::AutomationV2Spec {
    let now = crate::now_ms();
    let max_parallel_agents = plan
        .operator_preferences
        .as_ref()
        .and_then(|prefs| prefs.get("max_parallel_agents"))
        .and_then(Value::as_u64)
        .map(|value| value.clamp(1, 16) as u32)
        .or_else(|| {
            plan.operator_preferences
                .as_ref()
                .and_then(|prefs| prefs.get("execution_mode"))
                .and_then(Value::as_str)
                .map(str::trim)
                .and_then(|mode| match mode {
                    "swarm" => Some(4),
                    _ => Some(1),
                })
        })
        .or(Some(1));
    let model_policy = compile_operator_model_policy(plan.operator_preferences.as_ref());
    let agent_roles = plan
        .steps
        .iter()
        .map(|step| step.agent_role.clone())
        .collect::<std::collections::BTreeSet<_>>();
    let agents = agent_roles
        .into_iter()
        .map(|agent_role| crate::AutomationAgentProfile {
            agent_id: agent_id_for_role(&agent_role),
            template_id: None,
            display_name: display_name_for_role(&agent_role),
            avatar_url: None,
            model_policy: model_policy.clone(),
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec!["read".to_string()],
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: plan.allowed_mcp_servers.clone(),
                allowed_tools: None,
            },
            approval_policy: None,
        })
        .collect::<Vec<_>>();
    let flow = crate::AutomationFlowSpec {
        nodes: plan
            .steps
            .iter()
            .map(|step| crate::AutomationFlowNode {
                node_id: step.step_id.clone(),
                agent_id: agent_id_for_role(&step.agent_role),
                objective: step.objective.clone(),
                depends_on: step.depends_on.clone(),
                input_refs: step.input_refs.clone(),
                output_contract: step.output_contract.clone(),
                retry_policy: None,
                timeout_ms: None,
            })
            .collect(),
    };
    crate::AutomationV2Spec {
        automation_id: format!("automation-v2-{}", Uuid::new_v4()),
        name: plan.title.clone(),
        description: plan.description.clone(),
        status: crate::AutomationV2Status::Active,
        schedule: plan.schedule.clone(),
        workspace_root: Some(plan.workspace_root.clone()),
        agents,
        flow,
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents,
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
        },
        output_targets: Vec::new(),
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: creator_id.to_string(),
        metadata: Some(json!({
            "original_prompt": plan.original_prompt.clone(),
            "normalized_prompt": plan.normalized_prompt.clone(),
            "planner_version": plan.planner_version.clone(),
            "plan_source": plan.plan_source.clone(),
            "plan_id": plan.plan_id.clone(),
            "confidence": plan.confidence.clone(),
            "allowed_mcp_servers": plan.allowed_mcp_servers.clone(),
            "workspace_root": plan.workspace_root.clone(),
            "operator_preferences": plan.operator_preferences.clone(),
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    }
}

fn compile_operator_model_policy(operator_preferences: Option<&Value>) -> Option<Value> {
    let prefs = operator_preferences?;
    let provider_id = prefs
        .get("model_provider")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let model_id = prefs
        .get("model_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let role_models = prefs
        .get("role_models")
        .cloned()
        .filter(|value| value.is_object());
    let mut payload = serde_json::Map::new();
    if let (Some(provider_id), Some(model_id)) = (provider_id, model_id) {
        payload.insert(
            "default_model".to_string(),
            json!({
                "provider_id": provider_id,
                "model_id": model_id,
            }),
        );
    }
    if let Some(role_models) = role_models {
        payload.insert("role_models".to_string(), role_models);
    }
    if payload.is_empty() {
        None
    } else {
        Some(Value::Object(payload))
    }
}

fn agent_id_for_role(role: &str) -> String {
    format!("agent_{}", role.trim().replace([' ', '-'], "_"))
}

fn display_name_for_role(role: &str) -> String {
    role.split(['_', '-', ' '])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
