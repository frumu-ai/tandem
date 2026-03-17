use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use futures::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use tandem_observability::{emit_event, ObservabilityEvent, ProcessKind};
use tandem_providers::{ChatMessage, StreamChunk, TokenUsage};
use tandem_types::{Message, MessagePart, MessageRole, ToolMode};
use tokio_util::sync::CancellationToken;
use tracing::Level;
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
    let build = build_workflow_plan(
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
    let plan = build.plan;
    let planner_diagnostics = build.planner_diagnostics.clone();
    state
        .put_workflow_plan_draft(workflow_plan_draft_from_plan(
            plan.clone(),
            planner_diagnostics.clone(),
        ))
        .await;
    Ok(Json(json!({
        "plan": plan,
        "clarifier": build.clarifier,
        "planner_diagnostics": planner_diagnostics,
        "assistant_message": build.assistant_text.map(|text| json!({
            "role": "assistant",
            "text": text,
        })),
    })))
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
    let build = build_workflow_plan(
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
    let plan = build.plan;
    let mut draft = workflow_plan_draft_from_plan(plan.clone(), build.planner_diagnostics.clone());
    if let Some(text) = build.assistant_text.clone() {
        draft
            .conversation
            .messages
            .push(crate::WorkflowPlanChatMessage {
                role: "assistant".to_string(),
                text,
                created_at_ms: crate::now_ms(),
            });
        draft.conversation.updated_at_ms = crate::now_ms();
    }
    state.put_workflow_plan_draft(draft.clone()).await;
    Ok(Json(json!({
        "plan": draft.current_plan,
        "conversation": draft.conversation,
        "planner_diagnostics": draft.planner_diagnostics,
        "clarifier": build.clarifier,
        "assistant_message": build.assistant_text.map(|text| json!({
            "role": "assistant",
            "text": text,
        })),
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
        "planner_diagnostics": draft.planner_diagnostics,
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
        revise_workflow_plan_with_planner_loop(
            &state,
            &draft.current_plan,
            &draft.conversation,
            message,
        )
        .await;
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
        "planner_diagnostics": draft.planner_diagnostics,
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
        "planner_diagnostics": draft.planner_diagnostics,
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
    if let Some(role_models) = map.get_mut("role_models").and_then(Value::as_object_mut) {
        let invalid_role_keys = role_models
            .iter()
            .filter_map(|(key, value)| {
                let role = value.as_object()?;
                let provider_id = role
                    .get("provider_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let model_id = role
                    .get("model_id")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if provider_id.is_some() && model_id.is_some() {
                    None
                } else {
                    Some(key.clone())
                }
            })
            .collect::<Vec<_>>();
        for key in invalid_role_keys {
            role_models.remove(&key);
        }
        if role_models.is_empty() {
            map.remove("role_models");
        }
    } else {
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

fn workflow_plan_draft_from_plan(
    plan: crate::WorkflowPlan,
    planner_diagnostics: Option<Value>,
) -> crate::WorkflowPlanDraftRecord {
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
        planner_diagnostics,
    }
}

struct WorkflowPlanBuildOutput {
    plan: crate::WorkflowPlan,
    assistant_text: Option<String>,
    clarifier: Value,
    planner_diagnostics: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct PlannerClarifier {
    #[serde(default)]
    field: Option<String>,
    question: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PlannerBuildAction {
    Build,
    Clarify,
}

#[derive(Debug, Deserialize)]
struct PlannerBuildPayload {
    action: PlannerBuildAction,
    #[serde(default)]
    assistant_text: Option<String>,
    #[serde(default)]
    clarifier: Option<PlannerClarifier>,
    #[serde(default)]
    plan: Option<Value>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PlannerRevisionAction {
    Revise,
    Clarify,
    Keep,
}

#[derive(Debug, Deserialize)]
struct PlannerRevisionPayload {
    action: PlannerRevisionAction,
    #[serde(default)]
    assistant_text: Option<String>,
    #[serde(default)]
    change_summary: Vec<String>,
    #[serde(default)]
    clarifier: Option<PlannerClarifier>,
    #[serde(default)]
    plan: Option<Value>,
}

#[derive(Debug, Clone)]
struct PlannerInvocationFailure {
    reason: String,
    detail: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct PlannerCapabilitySummary {
    built_in_capabilities: Vec<String>,
    mcp_servers: Vec<PlannerMcpServerCapabilitySummary>,
}

#[derive(Debug, Clone, serde::Serialize)]
struct PlannerMcpServerCapabilitySummary {
    server: String,
    tool_count: usize,
    capabilities: Vec<String>,
    sample_tools: Vec<String>,
}

enum PlannerPlanMode {
    Create,
    Revise,
}

struct PlannerPlanNormalizationContext<'a> {
    mode: PlannerPlanMode,
    plan_id: &'a str,
    planner_version: &'a str,
    plan_source: &'a str,
    original_prompt: &'a str,
    normalized_prompt: &'a str,
    resolved_workspace_root: &'a str,
    explicit_schedule: Option<&'a crate::AutomationV2Schedule>,
    request_allowed_mcp_servers: &'a [String],
    request_operator_preferences: Option<&'a Value>,
}

fn planner_diagnostics(reason: impl Into<String>, detail: Option<String>) -> Option<Value> {
    let reason = reason.into();
    if reason.trim().is_empty() {
        return None;
    }
    Some(json!({
        "fallback_reason": reason,
        "detail": detail.filter(|value| !value.trim().is_empty()),
    }))
}

async fn build_workflow_plan(
    state: &AppState,
    prompt: &str,
    explicit_schedule: Option<&Value>,
    plan_source: &str,
    allowed_mcp_servers: Vec<String>,
    workspace_root: Option<&str>,
    operator_preferences: Option<Value>,
) -> Result<WorkflowPlanBuildOutput, String> {
    let normalized_prompt = normalize_prompt(prompt);
    let explicit_schedule = explicit_schedule.and_then(schedule_from_value);
    let fallback_schedule = explicit_schedule.clone().unwrap_or_else(manual_schedule);
    let title = plan_title(prompt, &fallback_schedule.schedule_type);
    let workspace_root = resolve_workspace_root(state, workspace_root).await?;
    let allowed_mcp_servers = normalize_string_list(allowed_mcp_servers);
    let operator_preferences = normalize_operator_preferences(operator_preferences);
    let plan_id = format!("wfplan-{}", Uuid::new_v4());
    let planner_version = "v1".to_string();
    let normalization_ctx = PlannerPlanNormalizationContext {
        mode: PlannerPlanMode::Create,
        plan_id: &plan_id,
        planner_version: &planner_version,
        plan_source,
        original_prompt: prompt.trim(),
        normalized_prompt: &normalized_prompt,
        resolved_workspace_root: &workspace_root,
        explicit_schedule: explicit_schedule.as_ref(),
        request_allowed_mcp_servers: &allowed_mcp_servers,
        request_operator_preferences: operator_preferences.as_ref(),
    };

    if let Some(model) = planner_model_spec(operator_preferences.as_ref()) {
        if planner_model_provider_is_configured(state, &model).await {
            match try_llm_build_workflow_plan(
                state,
                &model,
                prompt,
                &normalized_prompt,
                explicit_schedule.as_ref(),
                plan_source,
                &workspace_root,
                &allowed_mcp_servers,
                operator_preferences.as_ref(),
            )
            .await
            {
                Ok(payload) => match payload.action {
                    PlannerBuildAction::Build => {
                        if let Some(candidate) = payload.plan.and_then(decode_planner_plan_value) {
                            match normalize_and_validate_planner_plan(candidate, &normalization_ctx)
                            {
                                Ok(plan) => {
                                    return Ok(WorkflowPlanBuildOutput {
                                        plan,
                                        assistant_text: payload.assistant_text,
                                        clarifier: Value::Null,
                                        planner_diagnostics: None,
                                    });
                                }
                                Err(error) => {
                                    let detail = truncate_text(&error, 500);
                                    tracing::warn!(
                                        plan_id = %plan_id,
                                        plan_source = %plan_source,
                                        "workflow planner llm output rejected by validation: {detail}"
                                    );
                                    return Ok(WorkflowPlanBuildOutput {
                                        plan: build_minimal_fallback_plan(
                                            &plan_id,
                                            &planner_version,
                                            plan_source,
                                            prompt,
                                            &normalized_prompt,
                                            title.clone(),
                                            workspace_root.clone(),
                                            fallback_schedule.clone(),
                                            allowed_mcp_servers.clone(),
                                            operator_preferences.clone(),
                                            Some("Planner fallback draft. The planner returned a workflow that Tandem could not validate.".to_string()),
                                        ),
                                        assistant_text: payload.assistant_text.or(Some(
                                            "The planner returned a workflow Tandem could not validate. Tandem used a minimal fallback plan instead.".to_string(),
                                        )),
                                        clarifier: Value::Null,
                                        planner_diagnostics: planner_diagnostics(
                                            "validation_rejected",
                                            Some(detail),
                                        ),
                                    });
                                }
                            }
                        }
                        return Ok(WorkflowPlanBuildOutput {
                            plan: build_minimal_fallback_plan(
                                &plan_id,
                                &planner_version,
                                plan_source,
                                prompt,
                                &normalized_prompt,
                                title.clone(),
                                workspace_root.clone(),
                                fallback_schedule.clone(),
                                allowed_mcp_servers.clone(),
                                operator_preferences.clone(),
                                Some("Planner fallback draft. The planner returned an invalid JSON plan.".to_string()),
                            ),
                            assistant_text: payload.assistant_text.or(Some(
                                "The planner returned a response Tandem could not parse into a valid workflow plan."
                                    .to_string(),
                            )),
                            clarifier: Value::Null,
                            planner_diagnostics: planner_diagnostics("invalid_json", None),
                        });
                    }
                    PlannerBuildAction::Clarify => {
                        let question = payload
                            .clarifier
                            .as_ref()
                            .map(|row| row.question.trim())
                            .filter(|value| !value.is_empty())
                            .unwrap_or("The request is ambiguous. Clarify the workflow goal or constraints.");
                        let field = payload
                            .clarifier
                            .as_ref()
                            .and_then(|row| row.field.as_deref())
                            .filter(|value| !value.trim().is_empty())
                            .unwrap_or("general");
                        return Ok(WorkflowPlanBuildOutput {
                            plan: build_minimal_fallback_plan(
                                &plan_id,
                                &planner_version,
                                plan_source,
                                prompt,
                                &normalized_prompt,
                                title.clone(),
                                workspace_root.clone(),
                                fallback_schedule.clone(),
                                allowed_mcp_servers.clone(),
                                operator_preferences.clone(),
                                Some("Planner fallback draft. Clarification is needed before Tandem can generate a richer workflow.".to_string()),
                            ),
                            assistant_text: Some(
                                payload
                                    .assistant_text
                                    .unwrap_or_else(|| question.to_string()),
                            ),
                            clarifier: json!({
                                "field": field,
                                "question": question,
                            }),
                            planner_diagnostics: planner_diagnostics(
                                "clarification_needed",
                                None,
                            ),
                        });
                    }
                },
                Err(failure) => {
                    return Ok(WorkflowPlanBuildOutput {
                        plan: build_minimal_fallback_plan(
                            &plan_id,
                            &planner_version,
                            plan_source,
                            prompt,
                            &normalized_prompt,
                            title.clone(),
                            workspace_root.clone(),
                            fallback_schedule.clone(),
                            allowed_mcp_servers.clone(),
                            operator_preferences.clone(),
                            Some("Planner fallback draft. Tandem could not complete a provider-safe planning call for this model.".to_string()),
                        ),
                        assistant_text: Some(
                            failure
                                .detail
                                .clone()
                                .unwrap_or_else(|| "The planner could not complete a valid provider call. Tandem used a minimal fallback workflow instead.".to_string()),
                        ),
                        clarifier: Value::Null,
                        planner_diagnostics: planner_diagnostics(
                            failure.reason,
                            failure.detail,
                        ),
                    });
                }
            }
        } else {
            let question = planner_llm_provider_unconfigured_hint(&model.provider_id);
            return Ok(WorkflowPlanBuildOutput {
                plan: build_minimal_fallback_plan(
                    &plan_id,
                    &planner_version,
                    plan_source,
                    prompt,
                    &normalized_prompt,
                    title.clone(),
                    workspace_root.clone(),
                    fallback_schedule.clone(),
                    allowed_mcp_servers.clone(),
                    operator_preferences.clone(),
                    Some("Planner fallback draft. Configure the planner provider for richer workflow generation.".to_string()),
                ),
                assistant_text: Some(question.clone()),
                clarifier: json!({
                    "field": "general",
                    "question": question,
                }),
                planner_diagnostics: planner_diagnostics("provider_unconfigured", None),
            });
        }
    }

    Ok(WorkflowPlanBuildOutput {
        plan: build_minimal_fallback_plan(
            &plan_id,
            &planner_version,
            plan_source,
            prompt,
            &normalized_prompt,
            title,
            workspace_root,
            fallback_schedule,
            allowed_mcp_servers,
            operator_preferences,
            Some(
                "Planner fallback draft. Configure a planner model for richer workflow planning."
                    .to_string(),
            ),
        ),
        assistant_text: None,
        clarifier: Value::Null,
        planner_diagnostics: planner_diagnostics("no_planner_model", None),
    })
}

async fn revise_workflow_plan_with_planner_loop(
    state: &AppState,
    current_plan: &crate::WorkflowPlan,
    conversation: &crate::WorkflowPlanConversation,
    message: &str,
) -> (crate::WorkflowPlan, String, Vec<String>, Value) {
    let Some(model) = planner_model_spec(current_plan.operator_preferences.as_ref()) else {
        let question = planner_llm_unavailable_hint();
        return (
            current_plan.clone(),
            format!("I kept the current plan. Clarification needed: {question}"),
            Vec::new(),
            json!({
                "field": "general",
                "question": question,
            }),
        );
    };
    if !planner_model_provider_is_configured(state, &model).await {
        let question = planner_llm_provider_unconfigured_hint(&model.provider_id);
        return (
            current_plan.clone(),
            format!("I kept the current plan. Clarification needed: {question}"),
            Vec::new(),
            json!({
                "field": "general",
                "question": question,
            }),
        );
    }
    let normalization_ctx = PlannerPlanNormalizationContext {
        mode: PlannerPlanMode::Revise,
        plan_id: &current_plan.plan_id,
        planner_version: &current_plan.planner_version,
        plan_source: &current_plan.plan_source,
        original_prompt: &current_plan.original_prompt,
        normalized_prompt: &current_plan.normalized_prompt,
        resolved_workspace_root: &current_plan.workspace_root,
        explicit_schedule: None,
        request_allowed_mcp_servers: &current_plan.allowed_mcp_servers,
        request_operator_preferences: current_plan.operator_preferences.as_ref(),
    };
    match try_llm_revise_workflow_plan(state, &model, current_plan, conversation, message).await {
        Ok(payload) => parse_llm_revision_payload(current_plan, payload, &normalization_ctx)
            .unwrap_or_else(|| {
                let question = planner_llm_invalid_response_hint();
                (
                    current_plan.clone(),
                    format!("I kept the current plan. Clarification needed: {question}"),
                    Vec::new(),
                    json!({
                        "field": "general",
                        "question": question,
                    }),
                )
            }),
        Err(failure) => {
            let question = failure
                .detail
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| planner_llm_invalid_response_hint().to_string());
            (
                current_plan.clone(),
                format!("I kept the current plan. Clarification needed: {question}"),
                Vec::new(),
                json!({
                    "field": "general",
                    "question": question,
                }),
            )
        }
    }
}

fn planner_llm_unavailable_hint() -> &'static str {
    "This workflow needs planner model settings before Tandem can revise it. Configure a planner model and try again."
}

fn planner_llm_provider_unconfigured_hint(provider_id: &str) -> String {
    format!(
        "The configured planner model uses provider `{provider_id}`, but that provider is not configured on this engine. Configure the provider first and try again."
    )
}

fn planner_llm_invalid_response_hint() -> &'static str {
    "The planner could not produce a valid workflow revision. Keep the current plan or try a more specific request."
}

fn plan_save_options() -> Value {
    json!({
        "can_export_pack": true,
        "can_save_skill": true,
    })
}

fn build_minimal_fallback_plan(
    plan_id: &str,
    planner_version: &str,
    plan_source: &str,
    prompt: &str,
    normalized_prompt: &str,
    title: String,
    workspace_root: String,
    schedule: crate::AutomationV2Schedule,
    allowed_mcp_servers: Vec<String>,
    operator_preferences: Option<Value>,
    description: Option<String>,
) -> crate::WorkflowPlan {
    crate::WorkflowPlan {
        plan_id: plan_id.to_string(),
        planner_version: planner_version.to_string(),
        plan_source: plan_source.to_string(),
        original_prompt: prompt.trim().to_string(),
        normalized_prompt: normalized_prompt.to_string(),
        confidence: "low".to_string(),
        title,
        description,
        schedule,
        execution_target: "automation_v2".to_string(),
        workspace_root,
        steps: vec![plan_step_with_dep(
            "execute_goal",
            "execute",
            "Execute the requested automation goal directly.",
            "worker",
            &[],
            Vec::new(),
            Some("structured_json"),
        )],
        requires_integrations: Vec::new(),
        allowed_mcp_servers,
        operator_preferences,
        save_options: plan_save_options(),
    }
}

fn normalize_and_validate_planner_plan(
    mut candidate: crate::WorkflowPlan,
    ctx: &PlannerPlanNormalizationContext<'_>,
) -> Result<crate::WorkflowPlan, String> {
    candidate.plan_id = ctx.plan_id.to_string();
    candidate.planner_version = ctx.planner_version.to_string();
    candidate.plan_source = ctx.plan_source.to_string();
    candidate.original_prompt = ctx.original_prompt.to_string();
    candidate.normalized_prompt = ctx.normalized_prompt.to_string();
    candidate.execution_target = "automation_v2".to_string();
    candidate.requires_integrations = normalize_string_list(candidate.requires_integrations);
    candidate.description = candidate.description.and_then(|value| {
        let trimmed = value.trim();
        (!trimmed.is_empty()).then_some(trimmed.to_string())
    });
    candidate.confidence = match candidate.confidence.trim().to_ascii_lowercase().as_str() {
        "low" | "medium" | "high" => candidate.confidence.trim().to_ascii_lowercase(),
        _ => "medium".to_string(),
    };
    candidate.title = {
        let trimmed = candidate.title.trim();
        if trimmed.is_empty() {
            plan_title(ctx.original_prompt, &candidate.schedule.schedule_type)
        } else {
            crate::truncate_text(trimmed, 120)
        }
    };
    candidate.save_options = if candidate.save_options.is_object() {
        candidate.save_options
    } else {
        plan_save_options()
    };

    match ctx.mode {
        PlannerPlanMode::Create => {
            candidate.workspace_root = ctx.resolved_workspace_root.to_string();
            candidate.allowed_mcp_servers = ctx.request_allowed_mcp_servers.to_vec();
            candidate.operator_preferences = merge_create_operator_preferences(
                ctx.request_operator_preferences,
                candidate.operator_preferences.take(),
            );
            if let Some(explicit_schedule) = ctx.explicit_schedule {
                candidate.schedule = explicit_schedule.clone();
            }
        }
        PlannerPlanMode::Revise => {
            candidate.workspace_root =
                crate::normalize_absolute_workspace_root(&candidate.workspace_root)?;
            candidate.allowed_mcp_servers = normalize_string_list(candidate.allowed_mcp_servers);
            candidate.operator_preferences =
                normalize_operator_preferences(candidate.operator_preferences.take());
        }
    }

    validate_workflow_plan(&candidate)?;
    Ok(candidate)
}

fn decode_planner_plan_value(value: Value) -> Option<crate::WorkflowPlan> {
    serde_json::from_value::<crate::WorkflowPlan>(value.clone())
        .ok()
        .or_else(|| decode_planner_plan_value_relaxed(value))
}

fn decode_planner_plan_value_relaxed(mut value: Value) -> Option<crate::WorkflowPlan> {
    let plan = value.as_object_mut()?;
    plan.entry("plan_id".to_string())
        .or_insert_with(|| Value::String(String::new()));
    plan.entry("planner_version".to_string())
        .or_insert_with(|| Value::String(String::new()));
    plan.entry("plan_source".to_string())
        .or_insert_with(|| Value::String(String::new()));
    plan.entry("original_prompt".to_string())
        .or_insert_with(|| Value::String(String::new()));
    plan.entry("normalized_prompt".to_string())
        .or_insert_with(|| Value::String(String::new()));
    plan.entry("confidence".to_string())
        .or_insert_with(|| Value::String("medium".to_string()));
    plan.entry("title".to_string())
        .or_insert_with(|| Value::String(String::new()));
    plan.entry("save_options".to_string())
        .or_insert_with(|| json!({}));
    plan.entry("requires_integrations".to_string())
        .or_insert_with(|| json!([]));
    plan.entry("allowed_mcp_servers".to_string())
        .or_insert_with(|| json!([]));
    let steps = plan.get_mut("steps")?.as_array_mut()?;
    for step in steps.iter_mut() {
        let Some(step_obj) = step.as_object_mut() else {
            continue;
        };
        if !step_obj.contains_key("step_id") {
            if let Some(id) = step_obj.get("id").cloned() {
                step_obj.insert("step_id".to_string(), id);
            }
        }
        if !step_obj.contains_key("kind") {
            if let Some(kind) = step_obj.get("type").cloned() {
                step_obj.insert("kind".to_string(), kind);
            }
        }
        if !step_obj.contains_key("objective") {
            let objective = step_obj
                .get("objective")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    step_obj
                        .get("config")
                        .and_then(|row| row.get("objective"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .or_else(|| {
                    step_obj
                        .get("label")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            if let Some(objective) = objective {
                step_obj.insert("objective".to_string(), Value::String(objective));
            }
        }
        if !step_obj.contains_key("agent_role") {
            let agent_role = step_obj
                .get("agent_role")
                .and_then(Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    step_obj
                        .get("config")
                        .and_then(|row| row.get("agent_role"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "worker".to_string());
            step_obj.insert("agent_role".to_string(), Value::String(agent_role));
        }
        if let Some(input_refs) = step_obj.get_mut("input_refs").and_then(Value::as_array_mut) {
            for input_ref in input_refs.iter_mut() {
                match input_ref {
                    Value::String(from_step_id) => {
                        *input_ref = json!({
                            "from_step_id": from_step_id,
                            "alias": from_step_id,
                        });
                    }
                    Value::Object(map) => {
                        if !map.contains_key("from_step_id") {
                            if let Some(value) = map
                                .get("from")
                                .cloned()
                                .or_else(|| map.get("step_id").cloned())
                                .or_else(|| map.get("id").cloned())
                            {
                                map.insert("from_step_id".to_string(), value);
                            }
                        }
                        if !map.contains_key("alias") {
                            if let Some(from_step_id) = map
                                .get("from_step_id")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                            {
                                map.insert("alias".to_string(), Value::String(from_step_id));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        if !step_obj.contains_key("output_contract") {
            let inferred_kind = step_obj
                .get("config")
                .and_then(|row| row.get("format"))
                .and_then(Value::as_str)
                .and_then(|format| match format.trim().to_ascii_lowercase().as_str() {
                    "markdown" | "md" => Some("report_markdown".to_string()),
                    "json" => Some("structured_json".to_string()),
                    "text" | "summary" => Some("text_summary".to_string()),
                    _ => None,
                });
            if let Some(kind) = inferred_kind {
                step_obj.insert(
                    "output_contract".to_string(),
                    json!({
                        "kind": kind,
                        "validator": output_validator_kind_for_kind(&kind),
                    }),
                );
            }
        }
    }
    serde_json::from_value::<crate::WorkflowPlan>(value).ok()
}

fn output_validator_kind_for_kind(kind: &str) -> crate::AutomationOutputValidatorKind {
    match kind.trim().to_ascii_lowercase().as_str() {
        "brief" => crate::AutomationOutputValidatorKind::ResearchBrief,
        "review" | "review_summary" | "approval_gate" => {
            crate::AutomationOutputValidatorKind::ReviewDecision
        }
        "structured_json" => crate::AutomationOutputValidatorKind::StructuredJson,
        _ => crate::AutomationOutputValidatorKind::GenericArtifact,
    }
}

fn merge_create_operator_preferences(
    explicit: Option<&Value>,
    candidate: Option<Value>,
) -> Option<Value> {
    let candidate = normalize_operator_preferences(candidate);
    let explicit = normalize_operator_preferences(explicit.cloned());
    match (candidate, explicit) {
        (None, None) => None,
        (Some(candidate), None) => Some(candidate),
        (None, Some(explicit)) => Some(explicit),
        (Some(candidate), Some(explicit)) => {
            let mut merged = candidate.as_object().cloned().unwrap_or_default();
            for (key, value) in explicit.as_object().cloned().unwrap_or_default() {
                merged.insert(key, value);
            }
            normalize_operator_preferences(Some(Value::Object(merged)))
        }
    }
}

pub(crate) fn validate_workflow_plan(plan: &crate::WorkflowPlan) -> Result<(), String> {
    if plan.execution_target.trim() != "automation_v2" {
        return Err("execution_target must be automation_v2".to_string());
    }
    crate::normalize_absolute_workspace_root(&plan.workspace_root)?;
    let allowed_step_ids = allowed_workflow_step_ids();
    let step_ids = plan
        .steps
        .iter()
        .map(|step| step.step_id.as_str())
        .collect::<std::collections::HashSet<_>>();
    if step_ids.is_empty() {
        return Err("workflow plan must include at least one step".to_string());
    }
    for step in &plan.steps {
        if !allowed_step_ids.contains(step.step_id.as_str()) {
            return Err(format!("unsupported workflow step id `{}`", step.step_id));
        }
        for dep in &step.depends_on {
            if !step_ids.contains(dep.as_str()) {
                return Err(format!(
                    "workflow step `{}` depends on unknown step `{}`",
                    step.step_id, dep
                ));
            }
        }
        for input in &step.input_refs {
            if !step_ids.contains(input.from_step_id.as_str()) {
                return Err(format!(
                    "workflow step `{}` references unknown input step `{}`",
                    step.step_id, input.from_step_id
                ));
            }
        }
    }
    Ok(())
}

const ALLOWED_WORKFLOW_STEP_IDS: &[&str] = &[
    "collect_inputs",
    "research_sources",
    "extract_pain_points",
    "cluster_topics",
    "analyze_findings",
    "generate_report",
    "compare_results",
    "compare_with_features",
    "notify_user",
    "execute_goal",
];

fn allowed_workflow_step_ids() -> std::collections::HashSet<&'static str> {
    ALLOWED_WORKFLOW_STEP_IDS.iter().copied().collect()
}

async fn try_llm_build_workflow_plan(
    state: &AppState,
    model: &tandem_types::ModelSpec,
    prompt: &str,
    normalized_prompt: &str,
    explicit_schedule: Option<&crate::AutomationV2Schedule>,
    plan_source: &str,
    workspace_root: &str,
    allowed_mcp_servers: &[String],
    operator_preferences: Option<&Value>,
) -> Result<PlannerBuildPayload, PlannerInvocationFailure> {
    let capability_summary = build_planner_capability_summary(state, allowed_mcp_servers).await;
    let payload = invoke_planner_llm(
        state,
        "Workflow Planner Create",
        workspace_root,
        model.clone(),
        build_llm_workflow_creation_prompt(
            prompt,
            normalized_prompt,
            explicit_schedule,
            plan_source,
            workspace_root,
            allowed_mcp_servers,
            operator_preferences,
            &capability_summary,
        ),
        format!("workflow-plan-build:{plan_source}"),
        planner_build_timeout_ms(),
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
    )
    .await?;
    serde_json::from_value(payload).map_err(|error| PlannerInvocationFailure {
        reason: "invalid_json".to_string(),
        detail: Some(truncate_text(&error.to_string(), 500)),
    })
}

async fn try_llm_revise_workflow_plan(
    state: &AppState,
    model: &tandem_types::ModelSpec,
    current_plan: &crate::WorkflowPlan,
    conversation: &crate::WorkflowPlanConversation,
    message: &str,
) -> Result<PlannerRevisionPayload, PlannerInvocationFailure> {
    let capability_summary =
        build_planner_capability_summary(state, &current_plan.allowed_mcp_servers).await;
    let payload = invoke_planner_llm(
        state,
        "Workflow Planner Revision",
        &current_plan.workspace_root,
        model.clone(),
        build_llm_workflow_revision_prompt(
            current_plan,
            conversation,
            message,
            &capability_summary,
        ),
        format!("workflow-plan-revision:{}", current_plan.plan_id),
        planner_revision_timeout_ms(),
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
    )
    .await?;
    serde_json::from_value(payload).map_err(|error| PlannerInvocationFailure {
        reason: "invalid_json".to_string(),
        detail: Some(truncate_text(&error.to_string(), 500)),
    })
}

async fn invoke_planner_llm(
    state: &AppState,
    session_title: &str,
    workspace_root: &str,
    model: tandem_types::ModelSpec,
    prompt: String,
    _run_key: String,
    timeout_ms: u64,
    override_env: &str,
) -> Result<Value, PlannerInvocationFailure> {
    if let Some(payload) = planner_test_override_payload(override_env, true) {
        return Ok(payload);
    }
    let workspace_root = resolve_workspace_root(state, Some(workspace_root))
        .await
        .map_err(|error| PlannerInvocationFailure {
            reason: "invalid_workspace_root".to_string(),
            detail: Some(error),
        })?;
    let mut session = Session::new(
        Some(session_title.to_string()),
        Some(workspace_root.clone()),
    );
    let session_id = session.id.clone();
    session.workspace_root = Some(workspace_root.clone());
    state
        .storage
        .save_session(session)
        .await
        .map_err(|error| PlannerInvocationFailure {
            reason: "storage_error".to_string(),
            detail: Some(truncate_text(&error.to_string(), 500)),
        })?;
    state
        .storage
        .append_message(
            &session_id,
            Message::new(
                MessageRole::User,
                vec![MessagePart::Text {
                    text: prompt.clone(),
                }],
            ),
        )
        .await
        .map_err(|error| PlannerInvocationFailure {
            reason: "storage_error".to_string(),
            detail: Some(truncate_text(&error.to_string(), 500)),
        })?;

    let cancel = CancellationToken::new();
    emit_event(
        Level::INFO,
        ProcessKind::Engine,
        ObservabilityEvent {
            event: "provider.call.start",
            component: "workflow.planner",
            correlation_id: None,
            session_id: Some(&session_id),
            run_id: None,
            message_id: None,
            provider_id: Some(model.provider_id.as_str()),
            model_id: Some(model.model_id.as_str()),
            status: Some("dispatch"),
            error_code: None,
            detail: Some("planner provider dispatch"),
        },
    );

    let planner_future = async {
        let messages = vec![ChatMessage {
            role: "user".to_string(),
            content: prompt,
            attachments: Vec::new(),
        }];
        let stream = state
            .providers
            .stream_for_provider(
                Some(model.provider_id.as_str()),
                Some(model.model_id.as_str()),
                messages,
                ToolMode::None,
                None,
                cancel.clone(),
            )
            .await
            .map_err(|error| PlannerInvocationFailure {
                reason: classify_planner_provider_failure_reason(&error.to_string()).to_string(),
                detail: Some(truncate_text(&error.to_string(), 500)),
            })?;
        tokio::pin!(stream);
        let mut output = String::new();
        let mut saw_first_delta = false;
        let mut usage: Option<TokenUsage> = None;
        while let Some(chunk) = stream.next().await {
            match chunk {
                Ok(StreamChunk::TextDelta(delta)) => {
                    if !saw_first_delta && !delta.trim().is_empty() {
                        saw_first_delta = true;
                        emit_event(
                            Level::INFO,
                            ProcessKind::Engine,
                            ObservabilityEvent {
                                event: "provider.call.first_byte",
                                component: "workflow.planner",
                                correlation_id: None,
                                session_id: Some(&session_id),
                                run_id: None,
                                message_id: None,
                                provider_id: Some(model.provider_id.as_str()),
                                model_id: Some(model.model_id.as_str()),
                                status: Some("streaming"),
                                error_code: None,
                                detail: Some("first text delta"),
                            },
                        );
                    }
                    output.push_str(&delta);
                }
                Ok(StreamChunk::ReasoningDelta(delta)) => {
                    output.push_str(&delta);
                }
                Ok(StreamChunk::Done {
                    finish_reason: _,
                    usage: provider_usage,
                }) => {
                    usage = provider_usage;
                    break;
                }
                Ok(StreamChunk::ToolCallStart { .. })
                | Ok(StreamChunk::ToolCallDelta { .. })
                | Ok(StreamChunk::ToolCallEnd { .. }) => {}
                Err(error) => {
                    return Err(PlannerInvocationFailure {
                        reason: classify_planner_provider_failure_reason(&error.to_string())
                            .to_string(),
                        detail: Some(truncate_text(&error.to_string(), 500)),
                    });
                }
            }
        }
        Ok::<(String, Option<TokenUsage>), PlannerInvocationFailure>((output, usage))
    };

    let output =
        match tokio::time::timeout(std::time::Duration::from_millis(timeout_ms), planner_future)
            .await
        {
            Ok(Ok((output, usage))) => {
                let finish_detail = usage
                    .as_ref()
                    .map(|value| {
                        format!(
                            "planner stream complete (prompt={}, completion={})",
                            value.prompt_tokens, value.completion_tokens
                        )
                    })
                    .unwrap_or_else(|| "planner stream complete".to_string());
                emit_event(
                    Level::INFO,
                    ProcessKind::Engine,
                    ObservabilityEvent {
                        event: "provider.call.finish",
                        component: "workflow.planner",
                        correlation_id: None,
                        session_id: Some(&session_id),
                        run_id: None,
                        message_id: None,
                        provider_id: Some(model.provider_id.as_str()),
                        model_id: Some(model.model_id.as_str()),
                        status: Some("completed"),
                        error_code: None,
                        detail: Some(&finish_detail),
                    },
                );
                output
            }
            Ok(Err(error)) => {
                emit_event(
                    Level::ERROR,
                    ProcessKind::Engine,
                    ObservabilityEvent {
                        event: "provider.call.error",
                        component: "workflow.planner",
                        correlation_id: None,
                        session_id: Some(&session_id),
                        run_id: None,
                        message_id: None,
                        provider_id: Some(model.provider_id.as_str()),
                        model_id: Some(model.model_id.as_str()),
                        status: Some("failed"),
                        error_code: Some(error.reason.as_str()),
                        detail: error.detail.as_deref(),
                    },
                );
                return Err(error);
            }
            Err(_) => {
                cancel.cancel();
                emit_event(
                    Level::WARN,
                    ProcessKind::Engine,
                    ObservabilityEvent {
                        event: "provider.call.error",
                        component: "workflow.planner",
                        correlation_id: None,
                        session_id: Some(&session_id),
                        run_id: None,
                        message_id: None,
                        provider_id: Some(model.provider_id.as_str()),
                        model_id: Some(model.model_id.as_str()),
                        status: Some("failed"),
                        error_code: Some("timeout"),
                        detail: Some("workflow planner llm call timed out before completion"),
                    },
                );
                return Err(PlannerInvocationFailure {
                    reason: "timeout".to_string(),
                    detail: Some("Workflow planner timed out before completion.".to_string()),
                });
            }
        };

    if output.trim().is_empty() {
        return Err(PlannerInvocationFailure {
            reason: "empty_output".to_string(),
            detail: Some("Workflow planner completed without assistant text.".to_string()),
        });
    }
    state
        .storage
        .append_message(
            &session_id,
            Message::new(
                MessageRole::Assistant,
                vec![MessagePart::Text {
                    text: output.clone(),
                }],
            ),
        )
        .await
        .map_err(|error| PlannerInvocationFailure {
            reason: "storage_error".to_string(),
            detail: Some(truncate_text(&error.to_string(), 500)),
        })?;
    extract_json_value_from_text(&output).ok_or_else(|| PlannerInvocationFailure {
        reason: "invalid_json".to_string(),
        detail: Some("Workflow planner returned text without valid JSON.".to_string()),
    })
}

fn classify_planner_provider_failure_reason(error: &str) -> &'static str {
    let lower = error.to_ascii_lowercase();
    if lower.contains("array too long") || lower.contains("maximum length 128") {
        "tool_schema_too_large"
    } else if lower.contains("invalid function name")
        || lower.contains("function_declarations")
        || lower.contains("tools[0]")
    {
        "provider_tool_schema_invalid"
    } else {
        "provider_request_failed"
    }
}

async fn build_planner_capability_summary(
    state: &AppState,
    allowed_mcp_servers: &[String],
) -> Value {
    let mut servers = Vec::new();
    for server in allowed_mcp_servers {
        let tools = state.mcp.server_tools(server).await;
        let mut capabilities = std::collections::BTreeSet::new();
        let mut sample_tools = Vec::new();
        for tool in tools.iter().take(8) {
            let tool_name = tool.namespaced_name.trim().to_string();
            if !tool_name.is_empty() {
                sample_tools.push(tool_name.clone());
            }
            let lower = tool.tool_name.to_ascii_lowercase();
            if lower.contains("gmail_send_email") || lower.contains("send_email") {
                capabilities.insert("email_send".to_string());
            }
            if lower.contains("gmail_send_draft") || lower.contains("send_draft") {
                capabilities.insert("email_draft".to_string());
            }
            if lower.contains("reddit") {
                capabilities.insert("reddit_research".to_string());
            }
            if lower.contains("search") {
                capabilities.insert("search".to_string());
            }
            if lower.contains("docs") || lower.contains("document") {
                capabilities.insert("docs".to_string());
            }
            if lower.contains("slack") {
                capabilities.insert("slack_delivery".to_string());
            }
        }
        servers.push(PlannerMcpServerCapabilitySummary {
            server: server.to_string(),
            tool_count: tools.len(),
            capabilities: capabilities.into_iter().collect(),
            sample_tools,
        });
    }
    json!(PlannerCapabilitySummary {
        built_in_capabilities: vec![
            "web_research".to_string(),
            "web_fetch".to_string(),
            "workspace_read".to_string(),
        ],
        mcp_servers: servers,
    })
}

fn parse_llm_revision_payload(
    current_plan: &crate::WorkflowPlan,
    payload: PlannerRevisionPayload,
    ctx: &PlannerPlanNormalizationContext<'_>,
) -> Option<(crate::WorkflowPlan, String, Vec<String>, Value)> {
    match payload.action {
        PlannerRevisionAction::Clarify => {
            let clarifier = payload.clarifier?;
            let question = clarifier.question.trim();
            if question.is_empty() {
                return None;
            }
            let assistant_text = payload
                .assistant_text
                .unwrap_or_else(|| question.to_string());
            Some((
                current_plan.clone(),
                assistant_text,
                Vec::new(),
                json!({
                    "field": clarifier.field.unwrap_or_else(|| "general".to_string()),
                    "question": question,
                }),
            ))
        }
        PlannerRevisionAction::Keep => Some((
            current_plan.clone(),
            payload
                .assistant_text
                .unwrap_or_else(|| "I kept the current workflow plan.".to_string()),
            Vec::new(),
            Value::Null,
        )),
        PlannerRevisionAction::Revise => {
            let candidate = decode_planner_plan_value(payload.plan?)?;
            let revised_plan = normalize_and_validate_planner_plan(candidate, ctx).ok()?;
            if workflow_steps_equal(&revised_plan.steps, &current_plan.steps)
                && revised_plan.title == current_plan.title
                && revised_plan.description == current_plan.description
                && workflow_schedule_equal(&revised_plan.schedule, &current_plan.schedule)
                && revised_plan.workspace_root == current_plan.workspace_root
                && revised_plan.allowed_mcp_servers == current_plan.allowed_mcp_servers
                && revised_plan.operator_preferences == current_plan.operator_preferences
            {
                return Some((
                    current_plan.clone(),
                    payload
                        .assistant_text
                        .unwrap_or_else(|| "I kept the current workflow plan.".to_string()),
                    Vec::new(),
                    Value::Null,
                ));
            }
            let change_summary = if payload.change_summary.is_empty() {
                vec!["updated workflow plan".to_string()]
            } else {
                payload.change_summary
            };
            let assistant_text = payload
                .assistant_text
                .unwrap_or_else(|| format!("Updated the plan: {}.", change_summary.join(", ")));
            Some((revised_plan, assistant_text, change_summary, Value::Null))
        }
    }
}

fn planner_test_override_payload(primary_env: &str, include_legacy: bool) -> Option<Value> {
    let raw = std::env::var(primary_env).ok().or_else(|| {
        include_legacy
            .then(|| std::env::var("TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE").ok())
            .flatten()
    })?;
    if raw.trim().is_empty() {
        return None;
    }
    extract_json_value_from_text(&raw)
}

fn planner_build_timeout_ms() -> u64 {
    std::env::var("TANDEM_WORKFLOW_PLANNER_BUILD_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(250, 60_000))
        .unwrap_or(30_000)
}

fn planner_revision_timeout_ms() -> u64 {
    std::env::var("TANDEM_WORKFLOW_PLANNER_REVISION_TIMEOUT_MS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(|value| value.clamp(250, 60_000))
        .unwrap_or(20_000)
}

pub(crate) fn planner_model_spec(
    operator_preferences: Option<&Value>,
) -> Option<tandem_types::ModelSpec> {
    let prefs = operator_preferences?;
    if let Some(planner_model) = prefs
        .get("role_models")
        .and_then(|row| row.get("planner"))
        .and_then(Value::as_object)
    {
        let provider_id = planner_model
            .get("provider_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let model_id = planner_model
            .get("model_id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if let (Some(provider_id), Some(model_id)) = (provider_id, model_id) {
            return Some(tandem_types::ModelSpec {
                provider_id: provider_id.to_string(),
                model_id: model_id.to_string(),
            });
        }
    }
    let provider_id = prefs.get("model_provider").and_then(Value::as_str)?.trim();
    let model_id = prefs.get("model_id").and_then(Value::as_str)?.trim();
    if provider_id.is_empty() || model_id.is_empty() {
        return None;
    }
    Some(tandem_types::ModelSpec {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    })
}

async fn planner_model_provider_is_configured(
    state: &AppState,
    model: &tandem_types::ModelSpec,
) -> bool {
    state
        .providers
        .list()
        .await
        .into_iter()
        .any(|provider| provider.id == model.provider_id)
}

fn build_llm_workflow_creation_prompt(
    prompt: &str,
    normalized_prompt: &str,
    explicit_schedule: Option<&crate::AutomationV2Schedule>,
    plan_source: &str,
    workspace_root: &str,
    allowed_mcp_servers: &[String],
    operator_preferences: Option<&Value>,
    capability_summary: &Value,
) -> String {
    format!(
        concat!(
            "You are creating a Tandem automation workflow plan.\n",
            "Planner intelligence lives in the model. Return JSON only.\n",
            "Allowed step ids: {}.\n",
            "Plan invariants:\n",
            "- execution_target must be automation_v2\n",
            "- workspace_root must be a non-empty absolute path\n",
            "- do not invent unsupported step ids\n",
            "- keep the graph minimal but sufficient\n",
            "- steps must form a valid DAG\n",
            "- input_refs and depends_on must reference existing steps\n",
            "WorkflowPlan.step schema:\n",
            "- each step must use fields: step_id, kind, objective, agent_role, depends_on, input_refs, output_contract\n",
            "- do not use alternate keys like id, type, label, or config as the primary step schema\n",
            "- input_refs must be objects shaped like {{\"from_step_id\":\"...\",\"alias\":\"...\"}}\n",
            "- output_contract must be either null or {{\"kind\":\"structured_json|report_markdown|text_summary|urls|citations|brief|review|review_summary|approval_gate\",\"validator\":\"structured_json|generic_artifact|research_brief|review_decision\"}}\n",
            "Schedule schema:\n",
            "- manual: {{\"type\":\"manual\",\"timezone\":\"UTC\",\"misfire_policy\":{{\"type\":\"run_once\"}}}}\n",
            "- cron: {{\"type\":\"cron\",\"cron_expression\":\"...\",\"timezone\":\"UTC\",\"misfire_policy\":{{\"type\":\"run_once\"}}}}\n",
            "- interval: {{\"type\":\"interval\",\"interval_seconds\":3600,\"timezone\":\"UTC\",\"misfire_policy\":{{\"type\":\"run_once\"}}}}\n",
            "Operator preference schema you may set:\n",
            "- execution_mode: single | team | swarm\n",
            "- max_parallel_agents: 1..16\n",
            "- model_provider + model_id\n",
            "- role_models.planner.provider_id + role_models.planner.model_id\n",
            "Explicit inputs that must be preserved exactly if provided:\n",
            "- workspace_root: {}\n",
            "- plan_source: {}\n",
            "- explicit_schedule: {}\n",
            "- allowed_mcp_servers: {}\n",
            "- operator_preferences: {}\n",
            "Planner capability summary (use this instead of inventing tools):\n{}\n",
            "Delivery rule:\n",
            "- plan email delivery only when the capability summary shows email_send or email_draft\n",
            "- default email delivery to inline body content\n",
            "- only plan an attachment when a workflow step is expected to produce a concrete attachment artifact such as an upload result or valid s3key\n",
            "Return one of:\n",
            "{{\"action\":\"build\",\"assistant_text\":\"...\",\"plan\":{{...full WorkflowPlan...}}}}\n",
            "{{\"action\":\"clarify\",\"assistant_text\":\"...\",\"clarifier\":{{\"field\":\"general\",\"question\":\"...\"}}}}\n",
            "Original prompt:\n{}\n\n",
            "Normalized prompt:\n{}\n"
        ),
        ALLOWED_WORKFLOW_STEP_IDS.join(", "),
        workspace_root,
        plan_source,
        serde_json::to_string_pretty(&explicit_schedule).unwrap_or_else(|_| "null".to_string()),
        serde_json::to_string_pretty(&allowed_mcp_servers).unwrap_or_else(|_| "[]".to_string()),
        serde_json::to_string_pretty(&operator_preferences).unwrap_or_else(|_| "null".to_string()),
        serde_json::to_string_pretty(capability_summary).unwrap_or_else(|_| "{}".to_string()),
        prompt.trim(),
        normalized_prompt,
    )
}

fn build_llm_workflow_revision_prompt(
    current_plan: &crate::WorkflowPlan,
    conversation: &crate::WorkflowPlanConversation,
    message: &str,
    capability_summary: &Value,
) -> String {
    let transcript = conversation
        .messages
        .iter()
        .rev()
        .take(8)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|entry| format!("{}: {}", entry.role, entry.text.trim()))
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        concat!(
            "You are revising a Tandem automation workflow plan.\n",
            "Planner intelligence lives in the model. Return JSON only.\n",
            "Allowed step ids: {}.\n",
            "Plan invariants:\n",
            "- execution_target must remain automation_v2\n",
            "- workspace_root must remain a non-empty absolute path\n",
            "- do not invent unsupported step ids\n",
            "- steps must form a valid DAG\n",
            "- input_refs and depends_on must reference existing steps\n",
            "- keep the workflow graph minimal but sufficient\n",
            "WorkflowPlan.step schema:\n",
            "- each step must use fields: step_id, kind, objective, agent_role, depends_on, input_refs, output_contract\n",
            "- do not use alternate keys like id, type, label, or config as the primary step schema\n",
            "- input_refs must be objects shaped like {{\"from_step_id\":\"...\",\"alias\":\"...\"}}\n",
            "- output_contract must be either null or {{\"kind\":\"structured_json|report_markdown|text_summary|urls|citations|brief|review|review_summary|approval_gate\",\"validator\":\"structured_json|generic_artifact|research_brief|review_decision\"}}\n",
            "You may revise title, description, schedule, workspace_root, allowed_mcp_servers, operator_preferences, steps, dependencies, input_refs, and output_contracts.\n",
            "Schedule schema:\n",
            "- manual | cron | interval using the same shape already present on WorkflowPlan.schedule\n",
            "Operator preference schema you may set:\n",
            "- execution_mode: single | team | swarm\n",
            "- max_parallel_agents: 1..16\n",
            "- model_provider + model_id\n",
            "- role_models.planner.provider_id + role_models.planner.model_id\n",
            "Planner capability summary (use this instead of inventing tools):\n{}\n",
            "Delivery rule:\n",
            "- keep email delivery inline by default\n",
            "- only preserve or add attachment behavior when the workflow has a concrete attachment artifact with a valid s3key/upload result\n",
            "Return one of:\n",
            "{{\"action\":\"revise\",\"assistant_text\":\"...\",\"change_summary\":[\"...\"],\"plan\":{{...full WorkflowPlan...}}}}\n",
            "{{\"action\":\"clarify\",\"assistant_text\":\"...\",\"clarifier\":{{\"field\":\"general\",\"question\":\"...\"}}}}\n",
            "{{\"action\":\"keep\",\"assistant_text\":\"...\"}}\n\n",
            "Original prompt:\n{}\n\n",
            "Current plan JSON:\n{}\n\n",
            "Recent planning conversation:\n{}\n\n",
            "User revision request:\n{}\n"
        ),
        ALLOWED_WORKFLOW_STEP_IDS.join(", "),
        serde_json::to_string_pretty(capability_summary).unwrap_or_else(|_| "{}".to_string()),
        current_plan.original_prompt.trim(),
        serde_json::to_string_pretty(current_plan).unwrap_or_else(|_| "{}".to_string()),
        if transcript.trim().is_empty() {
            "(none yet)".to_string()
        } else {
            transcript
        },
        message.trim(),
    )
}

pub(crate) fn extract_json_value_from_text(text: &str) -> Option<Value> {
    serde_json::from_str(text.trim())
        .ok()
        .or_else(|| {
            let fenced = text.split("```").find_map(|chunk| {
                let trimmed = chunk.trim();
                if trimmed.starts_with('{') {
                    Some(trimmed)
                } else if let Some(rest) = trimmed.strip_prefix("json") {
                    let rest = rest.trim();
                    rest.starts_with('{').then_some(rest)
                } else {
                    None
                }
            })?;
            serde_json::from_str(fenced).ok()
        })
        .or_else(|| {
            let start = text.find('{')?;
            let end = text.rfind('}')?;
            (start < end)
                .then(|| serde_json::from_str::<Value>(&text[start..=end]).ok())
                .flatten()
        })
}

fn normalize_prompt(prompt: &str) -> String {
    prompt
        .trim()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn manual_schedule() -> crate::AutomationV2Schedule {
    crate::AutomationV2Schedule {
        schedule_type: crate::AutomationV2ScheduleType::Manual,
        cron_expression: None,
        interval_seconds: None,
        timezone: "UTC".to_string(),
        misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
    }
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

fn workflow_steps_equal(
    left: &[crate::WorkflowPlanStep],
    right: &[crate::WorkflowPlanStep],
) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
}

fn workflow_schedule_equal(
    left: &crate::AutomationV2Schedule,
    right: &crate::AutomationV2Schedule,
) -> bool {
    serde_json::to_value(left).ok() == serde_json::to_value(right).ok()
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
            validator: Some(output_validator_kind_for_kind(kind)),
            schema: None,
            summary_guidance: None,
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
    let tool_allowlist = compile_workflow_agent_tool_allowlist(
        &plan.allowed_mcp_servers,
        plan.operator_preferences.as_ref(),
    );
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
                allowlist: tool_allowlist.clone(),
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
                retry_policy: Some(json!({
                    "max_attempts": 3
                })),
                timeout_ms: None,
                stage_kind: None,
                gate: None,
                metadata: None,
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
            max_total_tokens: None,
            max_total_cost_usd: None,
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

fn compile_workflow_agent_tool_allowlist(
    allowed_mcp_servers: &[String],
    operator_preferences: Option<&Value>,
) -> Vec<String> {
    let custom_allowlist = operator_preferences
        .and_then(|prefs| prefs.get("tool_allowlist"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let tool_access_mode = operator_preferences
        .and_then(|prefs| prefs.get("tool_access_mode"))
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("all");
    let mut allowlist = if tool_access_mode == "custom" {
        custom_allowlist
    } else {
        vec!["*".to_string()]
    };
    for server in allowed_mcp_servers {
        let namespace = normalize_mcp_server_namespace(server);
        allowlist.push(format!("mcp.{namespace}.*"));
    }
    crate::normalize_allowed_tools(allowlist)
}

fn normalize_mcp_server_namespace(raw: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    let cleaned = out.trim_matches('_');
    if cleaned.is_empty() {
        "server".to_string()
    } else {
        cleaned.to_string()
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
