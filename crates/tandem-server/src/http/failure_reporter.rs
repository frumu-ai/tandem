use crate::capability_resolver::canonicalize_tool_name;
use crate::http::AppState;
use crate::{FailureReporterConfig, FailureReporterSubmission};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize, Default)]
pub(super) struct FailureReporterConfigInput {
    #[serde(default)]
    pub failure_reporter: Option<FailureReporterConfig>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct FailureReporterDraftsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct FailureReporterSubmissionInput {
    #[serde(default)]
    pub report: Option<FailureReporterSubmission>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct FailureReporterDecisionInput {
    #[serde(default)]
    pub reason: Option<String>,
}

pub(super) async fn get_failure_reporter_config(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let config = state.failure_reporter_config().await;
    Json(json!({
        "failure_reporter": config
    }))
}

pub(super) async fn patch_failure_reporter_config(
    State(state): State<AppState>,
    Json(input): Json<FailureReporterConfigInput>,
) -> Response {
    let Some(config) = input.failure_reporter else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "failure_reporter object is required",
                "code": "FAILURE_REPORTER_CONFIG_REQUIRED",
            })),
        )
            .into_response();
    };
    match state.put_failure_reporter_config(config).await {
        Ok(saved) => Json(json!({ "failure_reporter": saved })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid failure reporter config",
                "code": "FAILURE_REPORTER_CONFIG_INVALID",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn get_failure_reporter_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.failure_reporter_status().await;
    Json(json!({
        "status": status
    }))
}

pub(super) async fn recompute_failure_reporter_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.failure_reporter_status().await;
    Json(json!({
        "status": status
    }))
}

pub(super) async fn get_failure_reporter_debug(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.failure_reporter_status().await;
    let selected_server_tools = if let Some(server_name) = status.config.mcp_server.as_deref() {
        state.mcp.server_tools(server_name).await
    } else {
        Vec::new()
    };
    let canonicalized_discovered_tools = selected_server_tools
        .iter()
        .map(|tool| {
            json!({
                "server_name": tool.server_name,
                "tool_name": tool.tool_name,
                "namespaced_name": tool.namespaced_name,
                "canonical_name": canonicalize_tool_name(&tool.namespaced_name),
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "status": status,
        "selected_server_tools": selected_server_tools,
        "canonicalized_discovered_tools": canonicalized_discovered_tools,
    }))
}

pub(super) async fn list_failure_reporter_drafts(
    State(state): State<AppState>,
    Query(query): Query<FailureReporterDraftsQuery>,
) -> Json<serde_json::Value> {
    let drafts = state
        .list_failure_reporter_drafts(query.limit.unwrap_or(50))
        .await;
    Json(json!({
        "drafts": drafts,
        "count": drafts.len(),
    }))
}

pub(super) async fn get_failure_reporter_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let draft = state.get_failure_reporter_draft(&id).await;
    match draft {
        Some(draft) => Json(json!({ "draft": draft })).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Failure reporter draft not found",
                "code": "FAILURE_REPORTER_DRAFT_NOT_FOUND",
            })),
        )
            .into_response(),
    }
}

fn map_failure_reporter_draft_update_error(
    draft_id: String,
    error: anyhow::Error,
) -> (StatusCode, Json<serde_json::Value>) {
    let detail = error.to_string();
    if detail.contains("not found") {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Failure Reporter draft not found",
                "code": "FAILURE_REPORTER_DRAFT_NOT_FOUND",
                "draft_id": draft_id,
            })),
        )
    } else if detail.contains("not waiting for approval") {
        (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Failure Reporter draft is not waiting for approval",
                "code": "FAILURE_REPORTER_DRAFT_NOT_PENDING_APPROVAL",
                "draft_id": draft_id,
                "detail": detail,
            })),
        )
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to update Failure Reporter draft",
                "code": "FAILURE_REPORTER_DRAFT_UPDATE_FAILED",
                "draft_id": draft_id,
                "detail": detail,
            })),
        )
    }
}

pub(super) async fn report_failure_reporter_issue(
    State(state): State<AppState>,
    Json(input): Json<FailureReporterSubmissionInput>,
) -> Response {
    let Some(report) = input.report else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "report object is required",
                "code": "FAILURE_REPORTER_REPORT_REQUIRED",
            })),
        )
            .into_response();
    };
    match state.submit_failure_reporter_draft(report).await {
        Ok(draft) => Json(json!({ "draft": draft })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to create Failure Reporter draft",
                "code": "FAILURE_REPORTER_REPORT_INVALID",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn approve_failure_reporter_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<FailureReporterDecisionInput>,
) -> Response {
    match state
        .update_failure_reporter_draft_status(&id, "draft_ready", input.reason.as_deref())
        .await
    {
        Ok(draft) => Json(json!({ "ok": true, "draft": draft })).into_response(),
        Err(error) => map_failure_reporter_draft_update_error(id, error).into_response(),
    }
}

pub(super) async fn deny_failure_reporter_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<FailureReporterDecisionInput>,
) -> Response {
    match state
        .update_failure_reporter_draft_status(&id, "denied", input.reason.as_deref())
        .await
    {
        Ok(draft) => Json(json!({ "ok": true, "draft": draft })).into_response(),
        Err(error) => map_failure_reporter_draft_update_error(id, error).into_response(),
    }
}
