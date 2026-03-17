use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use futures::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

use crate::{execute_workflow, simulate_workflow_event};
use tandem_types::EngineEvent;
use tandem_workflows::WorkflowStepSpec;

use super::AppState;

fn manual_schedule() -> crate::AutomationV2Schedule {
    crate::AutomationV2Schedule {
        schedule_type: crate::AutomationV2ScheduleType::Manual,
        cron_expression: None,
        interval_seconds: None,
        timezone: "UTC".to_string(),
        misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
    }
}

fn workflow_step_objective(step: &WorkflowStepSpec) -> String {
    match &step.with {
        Some(with) if !with.is_null() => format!(
            "Execute workflow action `{}` with payload {}.",
            step.action, with
        ),
        _ => format!("Execute workflow action `{}`.", step.action),
    }
}

fn compile_workflow_spec_to_automation_preview(
    workflow: &crate::WorkflowSpec,
) -> crate::AutomationV2Spec {
    let plan = crate::WorkflowPlan {
        plan_id: format!("workflow-preview-{}", workflow.workflow_id),
        planner_version: "workflow_registry_v1".to_string(),
        plan_source: "workflow_registry".to_string(),
        original_prompt: workflow
            .description
            .clone()
            .unwrap_or_else(|| workflow.name.clone()),
        normalized_prompt: workflow
            .description
            .clone()
            .unwrap_or_else(|| workflow.name.clone()),
        confidence: "high".to_string(),
        title: workflow.name.clone(),
        description: workflow.description.clone(),
        schedule: manual_schedule(),
        execution_target: "automation_v2".to_string(),
        workspace_root: std::env::current_dir()
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
            .to_string_lossy()
            .to_string(),
        steps: workflow
            .steps
            .iter()
            .map(|step| crate::WorkflowPlanStep {
                step_id: step.step_id.clone(),
                kind: "workflow_action".to_string(),
                objective: workflow_step_objective(step),
                depends_on: Vec::new(),
                agent_role: "operator".to_string(),
                input_refs: Vec::new(),
                output_contract: Some(crate::AutomationFlowOutputContract {
                    kind: "generic_artifact".to_string(),
                    validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                    schema: None,
                    summary_guidance: None,
                }),
            })
            .collect(),
        requires_integrations: Vec::new(),
        allowed_mcp_servers: Vec::new(),
        operator_preferences: Some(json!({
            "source": "workflow_registry",
            "tool_access_mode": "auto",
        })),
        save_options: json!({
            "origin": "workflow_registry",
        }),
    };
    let mut automation =
        super::workflow_planner::compile_plan_to_automation_v2(&plan, "workflow_registry");
    if let Some(metadata) = automation.metadata.as_mut().and_then(Value::as_object_mut) {
        metadata.insert("workflow_id".to_string(), json!(workflow.workflow_id));
        metadata.insert("workflow_name".to_string(), json!(workflow.name));
        metadata.insert("workflow_source".to_string(), json!(workflow.source));
        metadata.insert("workflow_enabled".to_string(), json!(workflow.enabled));
    }
    automation
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowRunsQuery {
    pub workflow_id: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowEventsQuery {
    pub workflow_id: Option<String>,
    pub run_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowRunPath {
    pub id: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowHookPath {
    pub id: String,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct WorkflowValidateInput {
    #[serde(default)]
    pub reload: Option<bool>,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowHookPatchInput {
    pub enabled: bool,
}

#[derive(Debug, Deserialize)]
pub(super) struct WorkflowSimulateInput {
    pub event_type: String,
    #[serde(default)]
    pub properties: Value,
}

pub(super) async fn workflows_list(State(state): State<AppState>) -> Json<Value> {
    let workflows = state.list_workflows().await;
    let automation_previews = workflows
        .iter()
        .map(|workflow| {
            (
                workflow.workflow_id.clone(),
                serde_json::to_value(compile_workflow_spec_to_automation_preview(workflow))
                    .unwrap_or(Value::Null),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    Json(json!({
        "workflows": workflows,
        "automation_previews": automation_previews,
        "count": automation_previews.len(),
    }))
}

pub(super) async fn workflows_get(
    State(state): State<AppState>,
    Path(WorkflowRunPath { id }): Path<WorkflowRunPath>,
) -> Result<Json<Value>, StatusCode> {
    let workflow = state.get_workflow(&id).await.ok_or(StatusCode::NOT_FOUND)?;
    let hooks = state.list_workflow_hooks(Some(&id)).await;
    let automation_preview = compile_workflow_spec_to_automation_preview(&workflow);
    Ok(Json(json!({
        "workflow": workflow,
        "hooks": hooks,
        "automation_preview": automation_preview
    })))
}

pub(super) async fn workflows_validate(
    State(state): State<AppState>,
    Json(input): Json<WorkflowValidateInput>,
) -> Result<Json<Value>, StatusCode> {
    let messages = if input.reload.unwrap_or(true) {
        state
            .reload_workflows()
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?
    } else {
        Vec::new()
    };
    Ok(Json(json!({
        "messages": messages,
        "registry": state.workflow_registry().await,
    })))
}

pub(super) async fn workflow_hooks_list(
    State(state): State<AppState>,
    Query(query): Query<WorkflowRunsQuery>,
) -> Json<Value> {
    let hooks = state
        .list_workflow_hooks(query.workflow_id.as_deref())
        .await;
    Json(json!({ "hooks": hooks, "count": hooks.len() }))
}

pub(super) async fn workflow_hooks_patch(
    State(state): State<AppState>,
    Path(WorkflowHookPath { id }): Path<WorkflowHookPath>,
    Json(input): Json<WorkflowHookPatchInput>,
) -> Result<Json<Value>, StatusCode> {
    let hook = state
        .set_workflow_hook_enabled(&id, input.enabled)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!({ "hook": hook })))
}

pub(super) async fn workflows_simulate(
    State(state): State<AppState>,
    Json(input): Json<WorkflowSimulateInput>,
) -> Json<Value> {
    let event = EngineEvent::new(input.event_type, input.properties);
    let result = simulate_workflow_event(&state, &event).await;
    Json(json!({ "simulation": result }))
}

pub(super) async fn workflows_run(
    State(state): State<AppState>,
    Path(WorkflowRunPath { id }): Path<WorkflowRunPath>,
) -> Result<Json<Value>, StatusCode> {
    let workflow = state.get_workflow(&id).await.ok_or(StatusCode::NOT_FOUND)?;
    let run = execute_workflow(
        &state,
        &workflow,
        Some("manual".to_string()),
        None,
        None,
        false,
    )
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "run": run })))
}

pub(super) async fn workflow_runs_list(
    State(state): State<AppState>,
    Query(query): Query<WorkflowRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50);
    let runs = state
        .list_workflow_runs(query.workflow_id.as_deref(), limit)
        .await;
    Json(json!({ "runs": runs, "count": runs.len() }))
}

pub(super) async fn workflow_runs_get(
    State(state): State<AppState>,
    Path(WorkflowRunPath { id }): Path<WorkflowRunPath>,
) -> Result<Json<Value>, StatusCode> {
    let run = state
        .get_workflow_run(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!({ "run": run })))
}

pub(super) fn workflow_events_stream(
    state: AppState,
    workflow_id: Option<String>,
    run_id: Option<String>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "workflows",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            if !event.event_type.starts_with("workflow.") {
                return None;
            }
            if let Some(expected) = workflow_id.as_deref() {
                let actual = event
                    .properties
                    .get("workflowID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if actual != expected {
                    return None;
                }
            }
            if let Some(expected) = run_id.as_deref() {
                let actual = event
                    .properties
                    .get("runID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if actual != expected {
                    return None;
                }
            }
            Some(Ok(
                Event::default().data(serde_json::to_string(&event).unwrap_or_default())
            ))
        }
        Err(_) => None,
    });
    ready.chain(live)
}

pub(super) async fn workflow_events(
    State(state): State<AppState>,
    Query(query): Query<WorkflowEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(workflow_events_stream(
        state,
        query.workflow_id,
        query.run_id,
    ))
    .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}
