use crate::capability_resolver::canonicalize_tool_name;
use crate::http::AppState;
use crate::{bug_monitor_github, BugMonitorConfig, BugMonitorDraftRecord, BugMonitorSubmission};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Deserialize;
use serde_json::json;
use uuid::Uuid;

use super::context_runs::{
    context_run_tasks_create, ensure_context_run_dir, load_context_run_state,
    save_context_run_state,
};
use super::context_types::{
    ContextBlackboardArtifact, ContextBlackboardTaskStatus, ContextRunCreateInput, ContextRunState,
    ContextRunStatus, ContextTaskCreateBatchInput, ContextTaskCreateInput, ContextWorkspaceLease,
};

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorConfigInput {
    #[serde(default)]
    pub bug_monitor: Option<BugMonitorConfig>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorDraftsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorIncidentsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorPostsQuery {
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorSubmissionInput {
    #[serde(default)]
    pub report: Option<BugMonitorSubmission>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct BugMonitorDecisionInput {
    #[serde(default)]
    pub reason: Option<String>,
}

async fn write_bug_monitor_artifact(
    state: &AppState,
    linked_context_run_id: &str,
    artifact_id: &str,
    artifact_type: &str,
    relative_path: &str,
    payload: &serde_json::Value,
) -> Result<(), StatusCode> {
    let path =
        super::context_runs::context_run_dir(state, linked_context_run_id).join(relative_path);
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let raw =
        serde_json::to_string_pretty(payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(&path, raw)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let artifact = ContextBlackboardArtifact {
        id: artifact_id.to_string(),
        ts_ms: crate::now_ms(),
        path: path.to_string_lossy().to_string(),
        artifact_type: artifact_type.to_string(),
        step_id: None,
        source_event_id: None,
    };
    super::context_runs::context_run_engine()
        .commit_blackboard_patch(
            state,
            linked_context_run_id,
            super::context_types::ContextBlackboardPatchOp::AddArtifact,
            serde_json::to_value(&artifact).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        )
        .await?;
    Ok(())
}

pub(super) async fn get_bug_monitor_config(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let config = state.bug_monitor_config().await;
    Json(json!({
        "bug_monitor": config
    }))
}

pub(super) async fn patch_bug_monitor_config(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorConfigInput>,
) -> Response {
    let Some(config) = input.bug_monitor else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "bug_monitor object is required",
                "code": "BUG_MONITOR_CONFIG_REQUIRED",
            })),
        )
            .into_response();
    };
    match state.put_bug_monitor_config(config).await {
        Ok(saved) => Json(json!({ "bug_monitor": saved })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid bug monitor config",
                "code": "BUG_MONITOR_CONFIG_INVALID",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn get_bug_monitor_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.bug_monitor_status().await;
    Json(json!({
        "status": status
    }))
}

pub(super) async fn recompute_bug_monitor_status(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.bug_monitor_status().await;
    Json(json!({
        "status": status
    }))
}

pub(super) async fn get_bug_monitor_debug(
    State(state): State<AppState>,
) -> Json<serde_json::Value> {
    let status = state.bug_monitor_status().await;
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

pub(super) async fn list_bug_monitor_incidents(
    State(state): State<AppState>,
    Query(query): Query<BugMonitorIncidentsQuery>,
) -> Json<serde_json::Value> {
    let incidents = state
        .list_bug_monitor_incidents(query.limit.unwrap_or(50))
        .await;
    Json(json!({
        "incidents": incidents,
        "count": incidents.len(),
    }))
}

pub(super) async fn get_bug_monitor_incident(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match state.get_bug_monitor_incident(&id).await {
        Some(incident) => Json(json!({ "incident": incident })).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor incident not found",
                "code": "BUG_MONITOR_INCIDENT_NOT_FOUND",
                "incident_id": id,
            })),
        )
            .into_response(),
    }
}

pub(super) async fn list_bug_monitor_drafts(
    State(state): State<AppState>,
    Query(query): Query<BugMonitorDraftsQuery>,
) -> Json<serde_json::Value> {
    let drafts = state
        .list_bug_monitor_drafts(query.limit.unwrap_or(50))
        .await;
    Json(json!({
        "drafts": drafts,
        "count": drafts.len(),
    }))
}

pub(super) async fn list_bug_monitor_posts(
    State(state): State<AppState>,
    Query(query): Query<BugMonitorPostsQuery>,
) -> Json<serde_json::Value> {
    let posts = state
        .list_bug_monitor_posts(query.limit.unwrap_or(50))
        .await;
    Json(json!({
        "posts": posts,
        "count": posts.len(),
    }))
}

pub(super) async fn pause_bug_monitor(State(state): State<AppState>) -> Response {
    let mut config = state.bug_monitor_config().await;
    config.paused = true;
    match state.put_bug_monitor_config(config).await {
        Ok(saved) => Json(json!({ "ok": true, "bug_monitor": saved })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to pause Bug Monitor",
                "code": "BUG_MONITOR_PAUSE_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn resume_bug_monitor(State(state): State<AppState>) -> Response {
    let mut config = state.bug_monitor_config().await;
    config.paused = false;
    match state.put_bug_monitor_config(config).await {
        Ok(saved) => Json(json!({ "ok": true, "bug_monitor": saved })).into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to resume Bug Monitor",
                "code": "BUG_MONITOR_RESUME_FAILED",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn replay_bug_monitor_incident(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let Some(incident) = state.get_bug_monitor_incident(&id).await else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor incident not found",
                "code": "BUG_MONITOR_INCIDENT_NOT_FOUND",
                "incident_id": id,
            })),
        )
            .into_response();
    };
    let Some(draft_id) = incident.draft_id.as_deref() else {
        return (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Bug monitor incident has no associated draft",
                "code": "BUG_MONITOR_INCIDENT_NO_DRAFT",
                "incident_id": id,
            })),
        )
            .into_response();
    };
    match ensure_bug_monitor_triage_run(state, draft_id, true).await {
        Ok((draft, run, deduped)) => Json(json!({
            "ok": true,
            "incident": incident,
            "draft": draft,
            "run": run,
            "deduped": deduped,
        }))
        .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to replay Bug Monitor incident",
                "code": "BUG_MONITOR_INCIDENT_REPLAY_FAILED",
                "incident_id": id,
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn get_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let draft = state.get_bug_monitor_draft(&id).await;
    match draft {
        Some(draft) => Json(json!({ "draft": draft })).into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug monitor draft not found",
                "code": "BUG_MONITOR_DRAFT_NOT_FOUND",
            })),
        )
            .into_response(),
    }
}

fn map_bug_monitor_draft_update_error(
    draft_id: String,
    error: anyhow::Error,
) -> (StatusCode, Json<serde_json::Value>) {
    let detail = error.to_string();
    if detail.contains("not found") {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Bug Monitor draft not found",
                "code": "BUG_MONITOR_DRAFT_NOT_FOUND",
                "draft_id": draft_id,
            })),
        )
    } else if detail.contains("not waiting for approval") {
        (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Bug Monitor draft is not waiting for approval",
                "code": "BUG_MONITOR_DRAFT_NOT_PENDING_APPROVAL",
                "draft_id": draft_id,
                "detail": detail,
            })),
        )
    } else {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to update Bug Monitor draft",
                "code": "BUG_MONITOR_DRAFT_UPDATE_FAILED",
                "draft_id": draft_id,
                "detail": detail,
            })),
        )
    }
}

pub(super) async fn report_bug_monitor_issue(
    State(state): State<AppState>,
    Json(input): Json<BugMonitorSubmissionInput>,
) -> Response {
    let Some(report) = input.report else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "report object is required",
                "code": "BUG_MONITOR_REPORT_REQUIRED",
            })),
        )
            .into_response();
    };
    let report_excerpt = report.excerpt.clone();
    match state.submit_bug_monitor_draft(report).await {
        Ok(draft) => {
            let duplicate_matches = super::coder::query_failure_pattern_matches(
                &state,
                &draft.repo,
                &draft.fingerprint,
                draft.title.as_deref(),
                draft.detail.as_deref(),
                &report_excerpt,
                3,
            )
            .await
            .unwrap_or_default();
            Json(json!({
                "draft": draft,
                "duplicate_matches": duplicate_matches,
            }))
            .into_response()
        }
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to create Bug Monitor draft",
                "code": "BUG_MONITOR_REPORT_INVALID",
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn approve_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<BugMonitorDecisionInput>,
) -> Response {
    match state
        .update_bug_monitor_draft_status(&id, "draft_ready", input.reason.as_deref())
        .await
    {
        Ok(draft) => match bug_monitor_github::publish_draft(
            &state,
            &draft.draft_id,
            None,
            bug_monitor_github::PublishMode::Auto,
        )
        .await
        {
            Ok(outcome) => Json(json!({
                "ok": true,
                "draft": outcome.draft,
                "action": outcome.action,
                "post": outcome.post,
            }))
            .into_response(),
            Err(error) => (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Draft approved but GitHub publish failed",
                    "code": "BUG_MONITOR_DRAFT_PUBLISH_FAILED",
                    "draft_id": draft.draft_id,
                    "detail": error.to_string(),
                })),
            )
                .into_response(),
        },
        Err(error) => map_bug_monitor_draft_update_error(id, error).into_response(),
    }
}

pub(super) async fn deny_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<BugMonitorDecisionInput>,
) -> Response {
    match state
        .update_bug_monitor_draft_status(&id, "denied", input.reason.as_deref())
        .await
    {
        Ok(draft) => Json(json!({ "ok": true, "draft": draft })).into_response(),
        Err(error) => map_bug_monitor_draft_update_error(id, error).into_response(),
    }
}

pub(super) async fn create_bug_monitor_triage_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match ensure_bug_monitor_triage_run(state.clone(), &id, false).await {
        Ok((draft, run_id, deduped)) => {
            let run = load_context_run_state(
                &state,
                draft.triage_run_id.as_deref().unwrap_or(run_id.as_str()),
            )
            .await
            .ok();
            Json(json!({
                "ok": true,
                "draft": draft,
                "run": run,
                "deduped": deduped,
            }))
            .into_response()
        }
        Err(error) => {
            let detail = error.to_string();
            let status = if detail.contains("not found") {
                StatusCode::NOT_FOUND
            } else if detail.contains("approved") || detail.contains("Denied") {
                StatusCode::CONFLICT
            } else {
                StatusCode::BAD_REQUEST
            };
            (
                status,
                Json(json!({
                    "error": "Failed to create Bug Monitor triage run",
                    "code": "BUG_MONITOR_TRIAGE_RUN_CREATE_FAILED",
                    "draft_id": id,
                    "detail": detail,
                })),
            )
                .into_response()
        }
    }
}

pub(super) async fn publish_bug_monitor_draft(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match bug_monitor_github::publish_draft(
        &state,
        &id,
        None,
        bug_monitor_github::PublishMode::ManualPublish,
    )
    .await
    {
        Ok(outcome) => Json(json!({
            "ok": true,
            "draft": outcome.draft,
            "action": outcome.action,
            "post": outcome.post,
        }))
        .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to publish Bug Monitor draft to GitHub",
                "code": "BUG_MONITOR_DRAFT_PUBLISH_FAILED",
                "draft_id": id,
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn recheck_bug_monitor_draft_match(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    match bug_monitor_github::publish_draft(
        &state,
        &id,
        None,
        bug_monitor_github::PublishMode::RecheckOnly,
    )
    .await
    {
        Ok(outcome) => Json(json!({
            "ok": true,
            "draft": outcome.draft,
            "action": outcome.action,
            "post": outcome.post,
        }))
        .into_response(),
        Err(error) => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Failed to recheck Bug Monitor draft against GitHub",
                "code": "BUG_MONITOR_DRAFT_RECHECK_FAILED",
                "draft_id": id,
                "detail": error.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(crate) async fn ensure_bug_monitor_triage_run(
    state: AppState,
    id: &str,
    bypass_approval_gate: bool,
) -> anyhow::Result<(BugMonitorDraftRecord, String, bool)> {
    let config = state.bug_monitor_config().await;
    let draft = state
        .get_bug_monitor_draft(id)
        .await
        .ok_or_else(|| anyhow::anyhow!("Bug Monitor draft not found"))?;

    if draft.status.eq_ignore_ascii_case("denied") {
        anyhow::bail!("Denied Bug Monitor drafts cannot create triage runs");
    }
    if !bypass_approval_gate
        && config.require_approval_for_new_issues
        && draft.status.eq_ignore_ascii_case("approval_required")
    {
        anyhow::bail!("Bug Monitor draft must be approved before triage run creation");
    }

    if let Some(existing_run_id) = draft.triage_run_id.clone() {
        match load_context_run_state(&state, &existing_run_id).await {
            Ok(_) => return Ok((draft, existing_run_id, true)),
            Err(_) => {}
        }
    }

    let run_id = format!("failure-triage-{}", Uuid::new_v4().simple());
    let objective = format!(
        "Triage bug monitor draft {} for {}: {}",
        draft.draft_id,
        draft.repo,
        draft
            .title
            .clone()
            .unwrap_or_else(|| "Untitled failure".to_string())
    );
    let workspace = config
        .workspace_root
        .as_ref()
        .map(|root| ContextWorkspaceLease {
            workspace_id: root.clone(),
            canonical_path: root.clone(),
            lease_epoch: crate::now_ms(),
        });
    let model_provider = config
        .model_policy
        .as_ref()
        .and_then(|policy| policy.get("default_model"))
        .and_then(|row| row.get("provider_id"))
        .and_then(|row| row.as_str())
        .map(|row| row.trim().to_string())
        .filter(|row| !row.is_empty());
    let model_id = config
        .model_policy
        .as_ref()
        .and_then(|policy| policy.get("default_model"))
        .and_then(|row| row.get("model_id"))
        .and_then(|row| row.as_str())
        .map(|row| row.trim().to_string())
        .filter(|row| !row.is_empty());
    let mcp_servers = config
        .mcp_server
        .as_ref()
        .map(|row| vec![row.clone()])
        .filter(|row| !row.is_empty());

    let duplicate_matches = super::coder::query_failure_pattern_matches(
        &state,
        &draft.repo,
        &draft.fingerprint,
        draft.title.as_deref(),
        draft.detail.as_deref(),
        &[],
        3,
    )
    .await
    .map_err(|status| {
        anyhow::anyhow!("Failed to query duplicate failure patterns: HTTP {status}")
    })?;

    let create_input = ContextRunCreateInput {
        run_id: Some(run_id.clone()),
        objective,
        run_type: Some("bug_monitor_triage".to_string()),
        workspace,
        source_client: Some("bug_monitor".to_string()),
        model_provider,
        model_id,
        mcp_servers,
    };
    let created_run =
        match super::context_runs::context_run_create(State(state.clone()), Json(create_input))
            .await
        {
            Ok(Json(payload)) => match serde_json::from_value::<ContextRunState>(
                payload.get("run").cloned().unwrap_or_default(),
            ) {
                Ok(run) => run,
                Err(_) => anyhow::bail!("Failed to deserialize triage context run"),
            },
            Err(status) => anyhow::bail!("Failed to create triage context run: HTTP {status}"),
        };

    let inspect_task_id = format!("triage-inspect-{}", Uuid::new_v4().simple());
    let validate_task_id = format!("triage-validate-{}", Uuid::new_v4().simple());
    let tasks_input = ContextTaskCreateBatchInput {
        tasks: vec![
            ContextTaskCreateInput {
                command_id: Some(format!("failure-triage:{run_id}:inspect")),
                id: Some(inspect_task_id.clone()),
                task_type: "inspection".to_string(),
                payload: json!({
                    "task_kind": "inspection",
                    "title": "Inspect failure report and affected area",
                    "draft_id": draft.draft_id,
                    "repo": draft.repo,
                    "summary": draft.title,
                    "detail": draft.detail,
                    "duplicate_matches": duplicate_matches,
                }),
                status: Some(ContextBlackboardTaskStatus::Runnable),
                workflow_id: Some("bug_monitor_triage".to_string()),
                workflow_node_id: Some("inspect_failure_report".to_string()),
                parent_task_id: None,
                depends_on_task_ids: Vec::new(),
                decision_ids: Vec::new(),
                artifact_ids: Vec::new(),
                priority: Some(10),
                max_attempts: Some(2),
            },
            ContextTaskCreateInput {
                command_id: Some(format!("failure-triage:{run_id}:validate")),
                id: Some(validate_task_id.clone()),
                task_type: "validation".to_string(),
                payload: json!({
                    "task_kind": "validation",
                    "title": "Reproduce or validate failure scope",
                    "draft_id": draft.draft_id,
                    "repo": draft.repo,
                    "depends_on": inspect_task_id,
                }),
                status: Some(ContextBlackboardTaskStatus::Pending),
                workflow_id: Some("bug_monitor_triage".to_string()),
                workflow_node_id: Some("validate_failure_scope".to_string()),
                parent_task_id: None,
                depends_on_task_ids: vec![inspect_task_id.clone()],
                decision_ids: Vec::new(),
                artifact_ids: Vec::new(),
                priority: Some(5),
                max_attempts: Some(2),
            },
        ],
    };
    let tasks_response = context_run_tasks_create(
        State(state.clone()),
        Path(run_id.clone()),
        Json(tasks_input),
    )
    .await;
    if tasks_response.is_err() {
        anyhow::bail!("Failed to seed triage tasks");
    }

    if !duplicate_matches.is_empty() {
        write_bug_monitor_artifact(
            &state,
            &run_id,
            "failure-duplicate-matches",
            "failure_duplicate_matches",
            "artifacts/failure_duplicate_matches.json",
            &json!({
                "draft_id": draft.draft_id,
                "repo": draft.repo,
                "fingerprint": draft.fingerprint,
                "matches": duplicate_matches,
                "created_at_ms": crate::now_ms(),
            }),
        )
        .await
        .map_err(|status| {
            anyhow::anyhow!("Failed to write duplicate matches artifact: HTTP {status}")
        })?;
    }

    let mut updated_draft = draft.clone();
    updated_draft.triage_run_id = Some(run_id.clone());
    updated_draft.status = "triage_queued".to_string();
    {
        let mut drafts = state.bug_monitor_drafts.write().await;
        drafts.insert(updated_draft.draft_id.clone(), updated_draft.clone());
    }
    state.persist_bug_monitor_drafts().await?;

    let mut run = match load_context_run_state(&state, &run_id).await {
        Ok(row) => row,
        Err(_) => created_run,
    };
    run.status = ContextRunStatus::Planning;
    run.why_next_step =
        Some("Inspect the failure report, then validate the failure scope.".to_string());
    ensure_context_run_dir(&state, &run_id)
        .await
        .map_err(|status| {
            anyhow::anyhow!("Failed to finalize triage run workspace: HTTP {status}")
        })?;
    save_context_run_state(&state, &run)
        .await
        .map_err(|status| anyhow::anyhow!("Failed to finalize triage run state: HTTP {status}"))?;
    state.event_bus.publish(tandem_types::EngineEvent::new(
        "bug_monitor.triage_run.created",
        json!({
            "draft_id": updated_draft.draft_id,
            "run_id": run.run_id,
            "repo": updated_draft.repo,
        }),
    ));

    Ok((updated_draft, run.run_id, false))
}
