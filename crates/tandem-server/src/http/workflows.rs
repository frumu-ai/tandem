use axum::{
    extract::{Extension, Path, Query, State},
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
use tandem_types::{EngineEvent, TenantContext};

use super::AppState;

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
                serde_json::to_value(
                    crate::workflows::compile_workflow_spec_to_automation_preview(workflow),
                )
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
    let automation_preview =
        crate::workflows::compile_workflow_spec_to_automation_preview(&workflow);
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
    Extension(tenant_context): Extension<tandem_types::TenantContext>,
    Path(WorkflowRunPath { id }): Path<WorkflowRunPath>,
) -> Result<Json<Value>, StatusCode> {
    let workflow = state.get_workflow(&id).await.ok_or(StatusCode::NOT_FOUND)?;
    let run = execute_workflow(
        &state,
        &workflow,
        tenant_context,
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
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<WorkflowRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50);
    let mut runs = state
        .list_workflow_runs(query.workflow_id.as_deref(), limit)
        .await;
    runs.retain(|run| super::tenant_matches(&tenant_context, &run.tenant_context));
    Json(json!({ "runs": runs, "count": runs.len() }))
}

pub(super) async fn workflow_runs_get(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(WorkflowRunPath { id }): Path<WorkflowRunPath>,
) -> Result<Json<Value>, StatusCode> {
    let run = state
        .get_workflow_run(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    super::ensure_same_tenant(&tenant_context, &run.tenant_context)?;
    Ok(Json(json!({ "run": run })))
}

fn workflow_event_tenant_context(event: &EngineEvent) -> TenantContext {
    event
        .properties
        .get("tenantContext")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_else(TenantContext::local_implicit)
}

pub(super) fn workflow_events_stream(
    state: AppState,
    tenant_context: TenantContext,
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
            if !super::tenant_matches(&tenant_context, &workflow_event_tenant_context(&event)) {
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
    Extension(tenant_context): Extension<TenantContext>,
    Query(query): Query<WorkflowEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(workflow_events_stream(
        state,
        tenant_context,
        query.workflow_id,
        query.run_id,
    ))
    .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}
