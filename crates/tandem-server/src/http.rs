use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path as FsPath;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{path::Component, time::UNIX_EPOCH};

use async_trait::async_trait;
use axum::extract::{Path, Query, State};
use axum::http::header::{self, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
use axum::middleware as axum_middleware;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::{delete, get, patch, post, put};
use axum::{Json, Router};
use futures::Stream;
use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tandem_memory::types::GlobalMemoryRecord;
use tandem_memory::{
    db::MemoryDatabase, MemoryCapabilities, MemoryCapabilityToken, MemoryPromoteRequest,
    MemoryPromoteResponse, MemoryPutRequest, MemoryPutResponse, MemorySearchRequest,
    MemorySearchResponse, ScrubReport, ScrubStatus,
};
use tandem_skills::{SkillBundleArtifacts, SkillLocation, SkillService, SkillsConflictPolicy};
use tokio::process::Command;
use tokio_stream::wrappers::{BroadcastStream, ReceiverStream};
use tokio_stream::StreamExt;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use tandem_channels::start_channel_listeners;
use tandem_tools::Tool;
use tandem_types::{
    CreateSessionRequest, EngineEvent, Message, MessagePart, MessagePartInput, MessageRole,
    SendMessageRequest, Session, TodoItem, ToolResult, ToolSchema,
};
use tandem_wire::{WireSession, WireSessionMessage};

use crate::ResourceStoreError;
use crate::{
    capability_resolver::{
        classify_missing_required, providers_for_capability, CapabilityBindingsFile,
        CapabilityBlockingIssue, CapabilityReadinessInput, CapabilityReadinessOutput,
        CapabilityResolveInput,
    },
    evaluate_routine_execution_policy, mcp_catalog,
    pack_manager::{PackExportRequest, PackInstallRequest, PackUninstallRequest},
    ActiveRun, AppState, AutomationAgentMcpPolicy, AutomationAgentProfile,
    AutomationAgentToolPolicy, AutomationExecutionPolicy, AutomationFlowSpec, AutomationRunStatus,
    AutomationV2RunRecord, AutomationV2Schedule, AutomationV2Spec, AutomationV2Status,
    ChannelStatus, DiscordConfigFile, RoutineExecutionDecision, RoutineHistoryEvent,
    RoutineMisfirePolicy, RoutineRunArtifact, RoutineRunRecord, RoutineRunStatus, RoutineSchedule,
    RoutineSpec, RoutineStatus, RoutineStoreError, SlackConfigFile, TelegramConfigFile,
};

mod capabilities;
mod channels_api;
mod config_providers;
mod context_runs;
mod context_types;
mod global;
mod mcp;
mod middleware;
mod missions_teams;
mod pack_builder;
mod packs;
mod permissions_questions;
mod presets;
mod resources;
mod router;
mod routes_capabilities;
mod routes_config_providers;
mod routes_context;
mod routes_global;
mod routes_mcp;
mod routes_missions_teams;
mod routes_pack_builder;
mod routes_packs;
mod routes_permissions_questions;
mod routes_presets;
mod routes_resources;
mod routes_routines_automations;
mod routes_sessions;
mod routes_skills_memory;
mod routes_system_api;
mod routines_automations;
mod sessions;
mod skills_memory;
mod system_api;

use capabilities::*;
use channels_api::*;
use config_providers::*;
use context_runs::*;
use context_types::*;
use global::*;
use mcp::*;
use pack_builder::*;
use packs::*;
use permissions_questions::*;
use presets::*;
use resources::*;
use routines_automations::*;
use sessions::*;
use skills_memory::*;
use system_api::*;

#[derive(Debug, Deserialize)]
struct ListSessionsQuery {
    q: Option<String>,
    page: Option<usize>,
    page_size: Option<usize>,
    archived: Option<bool>,
    scope: Option<SessionScope>,
    workspace: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct EventFilterQuery {
    #[serde(rename = "sessionID")]
    session_id: Option<String>,
    #[serde(rename = "runID")]
    run_id: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone, Copy)]
struct RunEventsQuery {
    since_seq: Option<u64>,
    tail: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct PromptAsyncQuery {
    r#return: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EngineLeaseAcquireInput {
    client_id: Option<String>,
    client_type: Option<String>,
    ttl_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct EngineLeaseRenewInput {
    lease_id: String,
}

#[derive(Debug, Deserialize)]
struct EngineLeaseReleaseInput {
    lease_id: String,
}

#[derive(Debug, Deserialize, Default)]
struct StorageRepairInput {
    force: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct StorageFilesQuery {
    path: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct UpdateSessionInput {
    title: Option<String>,
    archived: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct AttachSessionInput {
    target_workspace: String,
    reason_tag: Option<String>,
}

#[derive(Debug, Deserialize)]
struct WorkspaceOverrideInput {
    ttl_seconds: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct WorktreeInput {
    path: Option<String>,
    branch: Option<String>,
    base: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct LogInput {
    level: Option<String>,
    message: Option<String>,
    context: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct RoutineCreateInput {
    routine_id: Option<String>,
    name: String,
    schedule: RoutineSchedule,
    timezone: Option<String>,
    misfire_policy: Option<RoutineMisfirePolicy>,
    entrypoint: String,
    args: Option<Value>,
    allowed_tools: Option<Vec<String>>,
    output_targets: Option<Vec<String>>,
    creator_type: Option<String>,
    creator_id: Option<String>,
    requires_approval: Option<bool>,
    external_integrations_allowed: Option<bool>,
    next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AutomationMissionInput {
    objective: String,
    #[serde(default)]
    success_criteria: Vec<String>,
    #[serde(default)]
    briefing: Option<String>,
    #[serde(default)]
    entrypoint_compat: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AutomationToolPolicyInput {
    #[serde(default)]
    run_allowlist: Option<Vec<String>>,
    #[serde(default)]
    external_integrations_allowed: Option<bool>,
    #[serde(default)]
    orchestrator_only_tool_calls: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct AutomationApprovalPolicyInput {
    #[serde(default)]
    requires_approval: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct AutomationPolicyInput {
    #[serde(default)]
    tool: AutomationToolPolicyInput,
    #[serde(default)]
    approval: AutomationApprovalPolicyInput,
}

#[derive(Debug, Deserialize)]
struct AutomationCreateInput {
    automation_id: Option<String>,
    name: String,
    schedule: RoutineSchedule,
    timezone: Option<String>,
    misfire_policy: Option<RoutineMisfirePolicy>,
    mission: AutomationMissionInput,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    policy: Option<AutomationPolicyInput>,
    #[serde(default)]
    output_targets: Option<Vec<String>>,
    #[serde(default)]
    model_policy: Option<Value>,
    creator_type: Option<String>,
    creator_id: Option<String>,
    next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct AutomationMissionPatchInput {
    #[serde(default)]
    objective: Option<String>,
    #[serde(default)]
    success_criteria: Option<Vec<String>>,
    #[serde(default)]
    briefing: Option<String>,
    #[serde(default)]
    entrypoint_compat: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AutomationPatchInput {
    name: Option<String>,
    status: Option<RoutineStatus>,
    schedule: Option<RoutineSchedule>,
    timezone: Option<String>,
    misfire_policy: Option<RoutineMisfirePolicy>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    mission: Option<AutomationMissionPatchInput>,
    #[serde(default)]
    policy: Option<AutomationPolicyInput>,
    #[serde(default)]
    output_targets: Option<Vec<String>>,
    #[serde(default)]
    model_policy: Option<Value>,
    next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AutomationV2CreateInput {
    automation_id: Option<String>,
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<AutomationV2Status>,
    schedule: AutomationV2Schedule,
    #[serde(default)]
    agents: Vec<AutomationAgentProfile>,
    flow: AutomationFlowSpec,
    #[serde(default)]
    execution: Option<AutomationExecutionPolicy>,
    #[serde(default)]
    output_targets: Option<Vec<String>>,
    #[serde(default)]
    creator_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AutomationV2PatchInput {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<AutomationV2Status>,
    #[serde(default)]
    schedule: Option<AutomationV2Schedule>,
    #[serde(default)]
    agents: Option<Vec<AutomationAgentProfile>>,
    #[serde(default)]
    flow: Option<AutomationFlowSpec>,
    #[serde(default)]
    execution: Option<AutomationExecutionPolicy>,
    #[serde(default)]
    output_targets: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct RoutinePatchInput {
    name: Option<String>,
    status: Option<RoutineStatus>,
    schedule: Option<RoutineSchedule>,
    timezone: Option<String>,
    misfire_policy: Option<RoutineMisfirePolicy>,
    entrypoint: Option<String>,
    args: Option<Value>,
    allowed_tools: Option<Vec<String>>,
    output_targets: Option<Vec<String>>,
    requires_approval: Option<bool>,
    external_integrations_allowed: Option<bool>,
    next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Default)]
struct RoutineRunNowInput {
    run_count: Option<u32>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct RoutineHistoryQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct RoutineRunsQuery {
    routine_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct RoutineRunDecisionInput {
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RoutineRunArtifactInput {
    uri: String,
    kind: String,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
struct RoutineEventsQuery {
    routine_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AutomationEventsQuery {
    automation_id: Option<String>,
    run_id: Option<String>,
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}

pub async fn serve(addr: SocketAddr, state: AppState) -> anyhow::Result<()> {
    let reaper_state = state.clone();
    let status_indexer_state = state.clone();
    let routine_scheduler_state = state.clone();
    let routine_executor_state = state.clone();
    let usage_aggregator_state = state.clone();
    let automation_v2_scheduler_state = state.clone();
    let automation_v2_executor_state = state.clone();
    let agent_team_supervisor_state = state.clone();
    let global_memory_ingestor_state = state.clone();
    let mcp_bootstrap_state = state.clone();
    tokio::spawn(async move {
        bootstrap_mcp_servers_when_ready(mcp_bootstrap_state).await;
    });
    let app = app_router(state);
    let reaper = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(5)).await;
            let stale = reaper_state
                .run_registry
                .reap_stale(reaper_state.run_stale_ms)
                .await;
            for (session_id, run) in stale {
                let _ = reaper_state.cancellations.cancel(&session_id).await;
                let _ = reaper_state
                    .close_browser_sessions_for_owner(&session_id)
                    .await;
                reaper_state.event_bus.publish(EngineEvent::new(
                    "session.run.finished",
                    json!({
                        "sessionID": session_id,
                        "runID": run.run_id,
                        "finishedAtMs": crate::now_ms(),
                        "status": "timeout",
                    }),
                ));
            }
        }
    });
    let status_indexer = tokio::spawn(crate::run_status_indexer(status_indexer_state));
    let routine_scheduler = tokio::spawn(crate::run_routine_scheduler(routine_scheduler_state));
    let routine_executor = tokio::spawn(crate::run_routine_executor(routine_executor_state));
    let usage_aggregator = tokio::spawn(crate::run_usage_aggregator(usage_aggregator_state));
    let automation_v2_scheduler = tokio::spawn(crate::run_automation_v2_scheduler(
        automation_v2_scheduler_state,
    ));
    let automation_v2_executor = tokio::spawn(crate::run_automation_v2_executor(
        automation_v2_executor_state,
    ));
    let agent_team_supervisor = tokio::spawn(crate::run_agent_team_supervisor(
        agent_team_supervisor_state,
    ));
    let global_memory_ingestor =
        tokio::spawn(run_global_memory_ingestor(global_memory_ingestor_state));

    // --- Memory hygiene background task (runs every 12 hours) ---
    // Opens a fresh connection to memory.sqlite each cycle â€” safe because WAL
    // mode allows concurrent readers alongside the main engine connection.
    let hygiene_task = tokio::spawn(async move {
        // Initial delay so startup is not impacted.
        tokio::time::sleep(Duration::from_secs(60)).await;
        loop {
            let retention_days: u32 = std::env::var("TANDEM_MEMORY_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30);
            if retention_days > 0 {
                match tandem_core::resolve_shared_paths() {
                    Ok(paths) => {
                        match tandem_memory::db::MemoryDatabase::new(&paths.memory_db_path).await {
                            Ok(db) => {
                                if let Err(e) = db.run_hygiene(retention_days).await {
                                    tracing::warn!("memory hygiene failed: {}", e);
                                }
                            }
                            Err(e) => tracing::warn!("memory hygiene: could not open DB: {}", e),
                        }
                    }
                    Err(e) => tracing::warn!("memory hygiene: could not resolve paths: {}", e),
                }
            }
            tokio::time::sleep(Duration::from_secs(12 * 60 * 60)).await;
        }
    });

    // --- Channel listeners (optional) ---
    // Reads TANDEM_TELEGRAM_BOT_TOKEN, TANDEM_DISCORD_BOT_TOKEN, TANDEM_SLACK_BOT_TOKEN etc.
    // If no channels are configured the server starts normally without them.
    let channel_listener_set = match tandem_channels::config::ChannelsConfig::from_env() {
        Ok(config) => {
            tracing::info!("tandem-channels: starting configured channel listeners");
            let set = start_channel_listeners(config).await;
            Some(set)
        }
        Err(e) => {
            tracing::info!("tandem-channels: no channels configured ({})", e);
            None
        }
    };

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            if tokio::signal::ctrl_c().await.is_err() {
                futures::future::pending::<()>().await;
            }
        })
        .await;
    reaper.abort();
    status_indexer.abort();
    routine_scheduler.abort();
    routine_executor.abort();
    usage_aggregator.abort();
    automation_v2_scheduler.abort();
    automation_v2_executor.abort();
    agent_team_supervisor.abort();
    global_memory_ingestor.abort();
    hygiene_task.abort();
    if let Some(mut set) = channel_listener_set {
        set.abort_all();
    }
    result?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct ToolExecutionInput {
    tool: String,
    args: Option<Value>,
}

async fn execute_tool(
    State(state): State<AppState>,
    Json(input): Json<ToolExecutionInput>,
) -> Result<Json<Value>, StatusCode> {
    let args = input.args.unwrap_or_else(|| json!({}));
    let result = state.tools.execute(&input.tool, args).await.map_err(|e| {
        tracing::error!("Tool execution failed: {}", e);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(json!({
        "output": result.output,
        "metadata": result.metadata
    })))
}

async fn run_pack_builder_tool(state: &AppState, args: Value) -> Result<Value, StatusCode> {
    let result = state
        .tools
        .execute("pack_builder", args)
        .await
        .map_err(|e| {
            tracing::warn!("pack_builder tool execution failed: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    let mut metadata = result.metadata;
    if let Some(obj) = metadata.as_object_mut() {
        obj.entry("output".to_string())
            .or_insert_with(|| Value::String(result.output));
    }
    Ok(metadata)
}

fn pack_builder_task_status_from_payload(payload: &Value) -> ContextBlackboardTaskStatus {
    let status = payload
        .get("status")
        .and_then(Value::as_str)
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_default();
    let error = payload
        .get("error")
        .and_then(Value::as_str)
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_default();
    if status == "cancelled" {
        return ContextBlackboardTaskStatus::Failed;
    }
    if status.contains("blocked")
        || error.contains("approval_required")
        || error.contains("missing")
        || error.contains("auth")
    {
        return ContextBlackboardTaskStatus::Blocked;
    }
    if status == "applied" || status == "apply_succeeded" {
        return ContextBlackboardTaskStatus::Done;
    }
    if status.contains("apply") || status.contains("running") {
        return ContextBlackboardTaskStatus::InProgress;
    }
    if status.contains("preview_pending") {
        return ContextBlackboardTaskStatus::Runnable;
    }
    ContextBlackboardTaskStatus::Pending
}

fn sanitize_context_id(raw: Option<&str>) -> Option<String> {
    raw.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
}

fn pack_builder_task_id_for(payload: &Value, mode: &str, session_id: Option<&str>) -> String {
    if let Some(plan_id) = payload
        .get("plan_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return format!("pack-builder-plan-{plan_id}");
    }
    if let Some(session) = sanitize_context_id(session_id) {
        return format!("pack-builder-session-{}", session.replace([':', '/'], "_"));
    }
    format!("pack-builder-{mode}-{}", Uuid::new_v4())
}

async fn ensure_pack_builder_context_run(
    state: &AppState,
    run_id: &str,
    objective: Option<&str>,
) -> Result<(), StatusCode> {
    if load_context_run_state(state, run_id).await.is_ok() {
        return Ok(());
    }
    let now = crate::now_ms();
    let run = ContextRunState {
        run_id: run_id.to_string(),
        run_type: "pack_builder".to_string(),
        source_client: Some("pack_builder_api".to_string()),
        model_provider: None,
        model_id: None,
        mcp_servers: Vec::new(),
        status: ContextRunStatus::Running,
        objective: objective
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| "Pack Builder coordination".to_string()),
        workspace: ContextWorkspaceLease::default(),
        steps: Vec::new(),
        why_next_step: Some("Track pack builder workflow via blackboard tasks".to_string()),
        revision: 1,
        created_at_ms: now,
        started_at_ms: Some(now),
        ended_at_ms: None,
        last_error: None,
        updated_at_ms: now,
    };
    save_context_run_state(state, &run).await
}

async fn pack_builder_emit_blackboard_task(
    state: &AppState,
    run_id: &str,
    mode: &str,
    session_id: Option<&str>,
    payload: &Value,
) -> Result<(), StatusCode> {
    let lock = context_run_lock_for(run_id).await;
    let _guard = lock.lock().await;

    let task_id = pack_builder_task_id_for(payload, mode, session_id);
    let now = crate::now_ms();
    let status = pack_builder_task_status_from_payload(payload);
    let blackboard = load_context_blackboard(state, run_id);
    let existing = blackboard
        .tasks
        .iter()
        .find(|row| row.id == task_id)
        .cloned();

    if existing.is_none() {
        let task = ContextBlackboardTask {
            id: task_id.clone(),
            task_type: format!("pack_builder.{mode}"),
            payload: json!({
                "title": format!("Pack Builder {mode}"),
                "mode": mode,
                "plan_id": payload.get("plan_id").cloned().unwrap_or(Value::Null),
                "status": payload.get("status").cloned().unwrap_or(Value::Null),
            }),
            status: ContextBlackboardTaskStatus::Pending,
            workflow_id: Some("pack_builder".to_string()),
            workflow_node_id: Some(mode.to_string()),
            parent_task_id: None,
            depends_on_task_ids: Vec::new(),
            decision_ids: Vec::new(),
            artifact_ids: Vec::new(),
            assigned_agent: Some("pack_builder".to_string()),
            priority: 0,
            attempt: 0,
            max_attempts: 3,
            last_error: None,
            next_retry_at_ms: None,
            lease_owner: None,
            lease_token: None,
            lease_expires_at_ms: None,
            task_rev: 1,
            created_ts: now,
            updated_ts: now,
        };
        let (patch, _) = append_context_blackboard_patch(
            state,
            run_id,
            ContextBlackboardPatchOp::AddTask,
            serde_json::to_value(&task).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        )?;
        let _ = context_run_event_append(
            State(state.clone()),
            Path(run_id.to_string()),
            Json(ContextRunEventAppendInput {
                event_type: "context.task.created".to_string(),
                status: ContextRunStatus::Running,
                step_id: Some(task_id.clone()),
                payload: json!({
                    "task_id": task_id,
                    "task_type": task.task_type,
                    "patch_seq": patch.seq,
                    "task_rev": task.task_rev,
                    "source": "pack_builder",
                }),
            }),
        )
        .await;
    }

    let current = load_context_blackboard(state, run_id)
        .tasks
        .into_iter()
        .find(|row| row.id == task_id);
    let next_rev = current
        .as_ref()
        .map(|row| row.task_rev.saturating_add(1))
        .unwrap_or(1);
    let error_text = payload
        .get("error")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    let update_payload = json!({
        "task_id": task_id,
        "status": status,
        "assigned_agent": "pack_builder",
        "task_rev": next_rev,
        "error": error_text,
    });
    let (patch, _) = append_context_blackboard_patch(
        state,
        run_id,
        ContextBlackboardPatchOp::UpdateTaskState,
        update_payload,
    )?;
    let _ = context_run_event_append(
        State(state.clone()),
        Path(run_id.to_string()),
        Json(ContextRunEventAppendInput {
            event_type: context_task_status_event_name(&status).to_string(),
            status: ContextRunStatus::Running,
            step_id: Some(task_id.clone()),
            payload: json!({
                "task_id": task_id,
                "status": status,
                "patch_seq": patch.seq,
                "task_rev": next_rev,
                "source": "pack_builder",
                "mode": mode,
                "plan_id": payload.get("plan_id").cloned().unwrap_or(Value::Null),
            }),
        }),
    )
    .await;
    Ok(())
}

fn app_router(state: AppState) -> Router {
    router::build_router(state)
}

fn routine_error_response(error: RoutineStoreError) -> (StatusCode, Json<Value>) {
    match error {
        RoutineStoreError::InvalidRoutineId { routine_id } => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid routine id",
                "code": "INVALID_ROUTINE_ID",
                "routineID": routine_id,
            })),
        ),
        RoutineStoreError::InvalidSchedule { detail } => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid routine schedule",
                "code": "INVALID_ROUTINE_SCHEDULE",
                "detail": detail,
            })),
        ),
        RoutineStoreError::PersistFailed { message } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Routine persistence failed",
                "code": "ROUTINE_PERSIST_FAILED",
                "detail": message,
            })),
        ),
    }
}

async fn routines_create(
    State(state): State<AppState>,
    Json(input): Json<RoutineCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let routine = RoutineSpec {
        routine_id: input
            .routine_id
            .unwrap_or_else(|| Uuid::new_v4().to_string()),
        name: input.name,
        status: RoutineStatus::Active,
        schedule: input.schedule,
        timezone: input.timezone.unwrap_or_else(|| "UTC".to_string()),
        misfire_policy: input
            .misfire_policy
            .unwrap_or(RoutineMisfirePolicy::RunOnce),
        entrypoint: input.entrypoint,
        args: input.args.unwrap_or_else(|| json!({})),
        allowed_tools: input.allowed_tools.unwrap_or_default(),
        output_targets: input.output_targets.unwrap_or_default(),
        creator_type: input.creator_type.unwrap_or_else(|| "user".to_string()),
        creator_id: input.creator_id.unwrap_or_else(|| "unknown".to_string()),
        requires_approval: input.requires_approval.unwrap_or(true),
        external_integrations_allowed: input.external_integrations_allowed.unwrap_or(false),
        next_fire_at_ms: input.next_fire_at_ms,
        last_fired_at_ms: None,
    };
    let stored = state
        .put_routine(routine)
        .await
        .map_err(routine_error_response)?;
    state.event_bus.publish(EngineEvent::new(
        "routine.created",
        json!({
            "routineID": stored.routine_id,
            "name": stored.name,
            "entrypoint": stored.entrypoint,
        }),
    ));
    Ok(Json(json!({
        "routine": stored,
    })))
}

async fn routines_list(State(state): State<AppState>) -> Json<Value> {
    let routines = state.list_routines().await;
    Json(json!({
        "routines": routines,
        "count": routines.len(),
    }))
}

async fn routines_patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RoutinePatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut routine = state.get_routine(&id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine not found",
                "code": "ROUTINE_NOT_FOUND",
                "routineID": id,
            })),
        )
    })?;
    if let Some(name) = input.name {
        routine.name = name;
    }
    if let Some(status) = input.status {
        routine.status = status;
    }
    if let Some(schedule) = input.schedule {
        routine.schedule = schedule;
    }
    if let Some(timezone) = input.timezone {
        routine.timezone = timezone;
    }
    if let Some(misfire_policy) = input.misfire_policy {
        routine.misfire_policy = misfire_policy;
    }
    if let Some(entrypoint) = input.entrypoint {
        routine.entrypoint = entrypoint;
    }
    if let Some(args) = input.args {
        routine.args = args;
    }
    if let Some(allowed_tools) = input.allowed_tools {
        routine.allowed_tools = allowed_tools;
    }
    if let Some(output_targets) = input.output_targets {
        routine.output_targets = output_targets;
    }
    if let Some(requires_approval) = input.requires_approval {
        routine.requires_approval = requires_approval;
    }
    if let Some(external_integrations_allowed) = input.external_integrations_allowed {
        routine.external_integrations_allowed = external_integrations_allowed;
    }
    if let Some(next_fire_at_ms) = input.next_fire_at_ms {
        routine.next_fire_at_ms = Some(next_fire_at_ms);
    }

    let stored = state
        .put_routine(routine)
        .await
        .map_err(routine_error_response)?;
    state.event_bus.publish(EngineEvent::new(
        "routine.updated",
        json!({
            "routineID": stored.routine_id,
            "status": stored.status,
            "nextFireAtMs": stored.next_fire_at_ms,
        }),
    ));
    Ok(Json(json!({
        "routine": stored,
    })))
}

async fn routines_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let deleted = state
        .delete_routine(&id)
        .await
        .map_err(routine_error_response)?;
    if let Some(routine) = deleted {
        state.event_bus.publish(EngineEvent::new(
            "routine.deleted",
            json!({
                "routineID": routine.routine_id,
            }),
        ));
        Ok(Json(json!({
            "deleted": true,
            "routineID": id,
        })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine not found",
                "code": "ROUTINE_NOT_FOUND",
                "routineID": id,
            })),
        ))
    }
}

async fn routines_run_now(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RoutineRunNowInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let routine = state.get_routine(&id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine not found",
                "code": "ROUTINE_NOT_FOUND",
                "routineID": id,
            })),
        )
    })?;
    let run_count = input.run_count.unwrap_or(1).clamp(1, 20);
    let now = crate::now_ms();
    let trigger_type = "manual";
    match evaluate_routine_execution_policy(&routine, trigger_type) {
        RoutineExecutionDecision::Allowed => {
            let _ = state.mark_routine_fired(&routine.routine_id, now).await;
            let run = state
                .create_routine_run(
                    &routine,
                    trigger_type,
                    run_count,
                    RoutineRunStatus::Queued,
                    input.reason.clone(),
                )
                .await;
            state
                .append_routine_history(RoutineHistoryEvent {
                    routine_id: routine.routine_id.clone(),
                    trigger_type: trigger_type.to_string(),
                    run_count,
                    fired_at_ms: now,
                    status: "queued".to_string(),
                    detail: input.reason,
                })
                .await;
            state.event_bus.publish(EngineEvent::new(
                "routine.fired",
                json!({
                    "routineID": routine.routine_id,
                    "runID": run.run_id,
                    "runCount": run_count,
                    "triggerType": trigger_type,
                    "firedAtMs": now,
                }),
            ));
            state.event_bus.publish(EngineEvent::new(
                "routine.run.created",
                json!({
                    "run": run,
                }),
            ));
            Ok(Json(json!({
                "ok": true,
                "status": "queued",
                "routineID": id,
                "runID": run.run_id,
                "runCount": run_count,
                "firedAtMs": now,
            })))
        }
        RoutineExecutionDecision::RequiresApproval { reason } => {
            let run = state
                .create_routine_run(
                    &routine,
                    trigger_type,
                    run_count,
                    RoutineRunStatus::PendingApproval,
                    Some(reason.clone()),
                )
                .await;
            state
                .append_routine_history(RoutineHistoryEvent {
                    routine_id: routine.routine_id.clone(),
                    trigger_type: trigger_type.to_string(),
                    run_count,
                    fired_at_ms: now,
                    status: "pending_approval".to_string(),
                    detail: Some(reason.clone()),
                })
                .await;
            state.event_bus.publish(EngineEvent::new(
                "routine.approval_required",
                json!({
                    "routineID": routine.routine_id,
                    "runID": run.run_id,
                    "runCount": run_count,
                    "triggerType": trigger_type,
                    "reason": reason,
                }),
            ));
            state.event_bus.publish(EngineEvent::new(
                "routine.run.created",
                json!({
                    "run": run,
                }),
            ));
            Ok(Json(json!({
                "ok": true,
                "status": "pending_approval",
                "routineID": id,
                "runID": run.run_id,
                "runCount": run_count,
            })))
        }
        RoutineExecutionDecision::Blocked { reason } => {
            let run = state
                .create_routine_run(
                    &routine,
                    trigger_type,
                    run_count,
                    RoutineRunStatus::BlockedPolicy,
                    Some(reason.clone()),
                )
                .await;
            state
                .append_routine_history(RoutineHistoryEvent {
                    routine_id: routine.routine_id.clone(),
                    trigger_type: trigger_type.to_string(),
                    run_count,
                    fired_at_ms: now,
                    status: "blocked_policy".to_string(),
                    detail: Some(reason.clone()),
                })
                .await;
            state.event_bus.publish(EngineEvent::new(
                "routine.blocked",
                json!({
                    "routineID": routine.routine_id,
                    "runID": run.run_id,
                    "runCount": run_count,
                    "triggerType": trigger_type,
                    "reason": reason,
                }),
            ));
            state.event_bus.publish(EngineEvent::new(
                "routine.run.created",
                json!({
                    "run": run,
                }),
            ));
            Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Routine blocked by policy",
                    "code": "ROUTINE_POLICY_BLOCKED",
                    "routineID": id,
                    "runID": run.run_id,
                    "reason": reason,
                })),
            ))
        }
    }
}

async fn routines_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineHistoryQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let events = state.list_routine_history(&id, limit).await;
    Json(json!({
        "routineID": id,
        "events": events,
        "count": events.len(),
    }))
}

async fn routines_runs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50).clamp(1, 500);
    let runs = state.list_routine_runs(Some(&id), limit).await;
    Json(json!({
        "routineID": id,
        "runs": runs,
        "count": runs.len(),
    }))
}

async fn routines_runs_all(
    State(state): State<AppState>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let runs = state
        .list_routine_runs(query.routine_id.as_deref(), limit)
        .await;
    Json(json!({
        "runs": runs,
        "count": runs.len(),
    }))
}

async fn routines_run_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(run) = state.get_routine_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    Ok(Json(json!({ "run": run })))
}

fn reason_or_default(input: Option<String>, fallback: &str) -> String {
    input
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| fallback.to_string())
}

async fn routines_run_approve(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_routine_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    if current.status != RoutineRunStatus::PendingApproval {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Routine run is not waiting for approval",
                "code": "ROUTINE_RUN_NOT_PENDING_APPROVAL",
                "runID": run_id,
            })),
        ));
    }
    let reason = reason_or_default(input.reason, "approved by operator");
    let updated = state
        .update_routine_run_status(&run_id, RoutineRunStatus::Queued, Some(reason.clone()))
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Failed to update routine run",
                    "code":"ROUTINE_RUN_UPDATE_FAILED",
                    "runID": run_id,
                })),
            )
        })?;
    state.event_bus.publish(EngineEvent::new(
        "routine.run.approved",
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "reason": reason,
        }),
    ));
    Ok(Json(json!({ "ok": true, "run": updated })))
}

async fn routines_run_deny(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_routine_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    if current.status != RoutineRunStatus::PendingApproval {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Routine run is not waiting for approval",
                "code": "ROUTINE_RUN_NOT_PENDING_APPROVAL",
                "runID": run_id,
            })),
        ));
    }
    let reason = reason_or_default(input.reason, "denied by operator");
    let updated = state
        .update_routine_run_status(&run_id, RoutineRunStatus::Denied, Some(reason.clone()))
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Failed to update routine run",
                    "code":"ROUTINE_RUN_UPDATE_FAILED",
                    "runID": run_id,
                })),
            )
        })?;
    state.event_bus.publish(EngineEvent::new(
        "routine.run.denied",
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "reason": reason,
        }),
    ));
    Ok(Json(json!({ "ok": true, "run": updated })))
}

async fn routines_run_pause(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_routine_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    if !matches!(
        current.status,
        RoutineRunStatus::Queued | RoutineRunStatus::Running
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Routine run is not pausable",
                "code": "ROUTINE_RUN_NOT_PAUSABLE",
                "runID": run_id,
            })),
        ));
    }
    let reason = reason_or_default(input.reason, "paused by operator");
    let mut cancelled_sessions = Vec::new();
    if current.status == RoutineRunStatus::Running {
        for session_id in &current.active_session_ids {
            if state.cancellations.cancel(session_id).await {
                let _ = state.close_browser_sessions_for_owner(session_id).await;
                cancelled_sessions.push(session_id.clone());
            }
        }
    }
    let updated = state
        .update_routine_run_status(&run_id, RoutineRunStatus::Paused, Some(reason.clone()))
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Failed to update routine run",
                    "code":"ROUTINE_RUN_UPDATE_FAILED",
                    "runID": run_id,
                })),
            )
        })?;
    state.event_bus.publish(EngineEvent::new(
        "routine.run.paused",
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "reason": reason,
            "cancelledSessionIDs": cancelled_sessions,
        }),
    ));
    Ok(Json(json!({
        "ok": true,
        "run": updated,
        "cancelledSessionIDs": cancelled_sessions,
    })))
}

async fn routines_run_resume(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_routine_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    if current.status != RoutineRunStatus::Paused {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Routine run is not paused",
                "code": "ROUTINE_RUN_NOT_PAUSED",
                "runID": run_id,
            })),
        ));
    }
    let reason = reason_or_default(input.reason, "resumed by operator");
    let updated = state
        .update_routine_run_status(&run_id, RoutineRunStatus::Queued, Some(reason.clone()))
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error":"Failed to update routine run",
                    "code":"ROUTINE_RUN_UPDATE_FAILED",
                    "runID": run_id,
                })),
            )
        })?;
    state.event_bus.publish(EngineEvent::new(
        "routine.run.resumed",
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "reason": reason,
        }),
    ));
    Ok(Json(json!({ "ok": true, "run": updated })))
}

async fn routines_run_artifacts(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(run) = state.get_routine_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Routine run not found",
                "code": "ROUTINE_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        ));
    };
    Ok(Json(json!({
        "runID": run_id,
        "artifacts": run.artifacts,
        "count": run.artifacts.len(),
    })))
}

async fn routines_run_artifact_add(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunArtifactInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if input.uri.trim().is_empty() || input.kind.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error":"Artifact requires uri and kind",
                "code":"ROUTINE_ARTIFACT_INVALID",
            })),
        ));
    }
    let artifact = RoutineRunArtifact {
        artifact_id: format!("artifact-{}", Uuid::new_v4()),
        uri: input.uri.trim().to_string(),
        kind: input.kind.trim().to_string(),
        label: input
            .label
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty()),
        created_at_ms: crate::now_ms(),
        metadata: input.metadata,
    };
    let updated = state
        .append_routine_run_artifact(&run_id, artifact.clone())
        .await
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error":"Routine run not found",
                    "code":"ROUTINE_RUN_NOT_FOUND",
                    "runID": run_id,
                })),
            )
        })?;
    state.event_bus.publish(EngineEvent::new(
        "routine.run.artifact_added",
        json!({
            "runID": run_id,
            "routineID": updated.routine_id,
            "artifact": artifact,
        }),
    ));
    Ok(Json(json!({ "ok": true, "run": updated })))
}

fn routines_sse_stream(
    state: AppState,
    routine_id: Option<String>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "routines",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            if !event.event_type.starts_with("routine.") {
                return None;
            }
            if let Some(routine_id) = routine_id.as_deref() {
                let event_routine_id = event
                    .properties
                    .get("routineID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if event_routine_id != routine_id {
                    return None;
                }
            }
            let payload = serde_json::to_string(&event).unwrap_or_default();
            Some(Ok(Event::default().data(payload)))
        }
        Err(_) => None,
    });
    ready.chain(live)
}

async fn routines_events(
    State(state): State<AppState>,
    Query(query): Query<RoutineEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(routines_sse_stream(state, query.routine_id))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}

fn load_run_events_jsonl(path: &FsPath, since_seq: Option<u64>, tail: Option<usize>) -> Vec<Value> {
    let content = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let mut rows: Vec<Value> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| {
            if let Some(since) = since_seq {
                return row.get("seq").and_then(|value| value.as_u64()).unwrap_or(0) > since;
            }
            true
        })
        .collect();
    rows.sort_by_key(|row| row.get("seq").and_then(|value| value.as_u64()).unwrap_or(0));
    if let Some(tail_count) = tail {
        if rows.len() > tail_count {
            rows = rows.split_off(rows.len().saturating_sub(tail_count));
        }
    }
    rows
}

fn run_events_sse_stream(
    state: AppState,
    run_id: String,
    query: RunEventsQuery,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(256);
    tokio::spawn(async move {
        let snapshot = state.workspace_index.snapshot().await;
        let workspace_root = PathBuf::from(snapshot.root);
        let events_path = workspace_root
            .join(".tandem")
            .join("orchestrator")
            .join(&run_id)
            .join("events.jsonl");

        let ready = serde_json::to_string(&json!({
            "status":"ready",
            "stream":"run_events",
            "runID": run_id,
            "timestamp_ms": crate::now_ms(),
            "path": events_path.to_string_lossy().to_string(),
        }))
        .unwrap_or_default();
        if tx.send(ready).await.is_err() {
            return;
        }

        let initial = load_run_events_jsonl(&events_path, query.since_seq, query.tail);
        let mut last_seq = query.since_seq.unwrap_or(0);
        for row in initial {
            let seq = row
                .get("seq")
                .and_then(|value| value.as_u64())
                .unwrap_or(last_seq);
            last_seq = last_seq.max(seq);
            let payload = serde_json::to_string(&json!({
                "type":"run.event",
                "properties": row
            }))
            .unwrap_or_default();
            if tx.send(payload).await.is_err() {
                return;
            }
        }

        let mut interval = tokio::time::interval(Duration::from_millis(1000));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let updates = load_run_events_jsonl(&events_path, Some(last_seq), None);
            for row in updates {
                let seq = row
                    .get("seq")
                    .and_then(|value| value.as_u64())
                    .unwrap_or(last_seq);
                last_seq = last_seq.max(seq);
                let payload = serde_json::to_string(&json!({
                    "type":"run.event",
                    "properties": row
                }))
                .unwrap_or_default();
                if tx.send(payload).await.is_err() {
                    return;
                }
            }
        }
    });

    ReceiverStream::new(rx).map(|payload| Ok(Event::default().data(payload)))
}

fn context_runs_root(state: &AppState) -> PathBuf {
    state
        .shared_resources_path
        .parent()
        .map(|parent| parent.join("context_runs"))
        .unwrap_or_else(|| PathBuf::from(".tandem").join("context_runs"))
}

fn context_run_dir(state: &AppState, run_id: &str) -> PathBuf {
    context_runs_root(state).join(run_id)
}

fn context_run_state_path(state: &AppState, run_id: &str) -> PathBuf {
    context_run_dir(state, run_id).join("run_state.json")
}

fn context_run_events_path(state: &AppState, run_id: &str) -> PathBuf {
    context_run_dir(state, run_id).join("events.jsonl")
}

fn context_run_blackboard_path(state: &AppState, run_id: &str) -> PathBuf {
    context_run_dir(state, run_id).join("blackboard.json")
}

fn context_run_blackboard_patches_path(state: &AppState, run_id: &str) -> PathBuf {
    context_run_dir(state, run_id).join("blackboard_patches.jsonl")
}

fn context_run_checkpoints_dir(state: &AppState, run_id: &str) -> PathBuf {
    context_run_dir(state, run_id).join("checkpoints")
}

async fn ensure_context_run_dir(state: &AppState, run_id: &str) -> Result<(), StatusCode> {
    let run_dir = context_run_dir(state, run_id);
    tokio::fs::create_dir_all(&run_dir)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

async fn load_context_run_state(
    state: &AppState,
    run_id: &str,
) -> Result<ContextRunState, StatusCode> {
    let path = context_run_state_path(state, run_id);
    let raw = tokio::fs::read_to_string(path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    serde_json::from_str::<ContextRunState>(&raw).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn save_context_run_state(state: &AppState, run: &ContextRunState) -> Result<(), StatusCode> {
    ensure_context_run_dir(state, &run.run_id).await?;
    let path = context_run_state_path(state, &run.run_id);
    let payload =
        serde_json::to_string_pretty(run).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::fs::write(path, payload)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn load_context_run_events_jsonl(
    path: &FsPath,
    since_seq: Option<u64>,
    tail: Option<usize>,
) -> Vec<ContextRunEventRecord> {
    let content = match std::fs::read_to_string(path) {
        Ok(value) => value,
        Err(_) => return Vec::new(),
    };
    let mut rows: Vec<ContextRunEventRecord> = content
        .lines()
        .filter_map(|line| serde_json::from_str::<ContextRunEventRecord>(line).ok())
        .filter(|row| {
            if let Some(since) = since_seq {
                return row.seq > since;
            }
            true
        })
        .collect();
    rows.sort_by_key(|row| row.seq);
    if let Some(tail_count) = tail {
        if rows.len() > tail_count {
            rows = rows.split_off(rows.len().saturating_sub(tail_count));
        }
    }
    rows
}

fn latest_context_run_event_seq(path: &FsPath) -> u64 {
    load_context_run_events_jsonl(path, None, None)
        .last()
        .map(|row| row.seq)
        .unwrap_or(0)
}

fn append_jsonl_line(path: &FsPath, value: &Value) -> Result<(), StatusCode> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let line = serde_json::to_string(value).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    writeln!(file, "{}", line).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn load_context_blackboard(state: &AppState, run_id: &str) -> ContextBlackboardState {
    let path = context_run_blackboard_path(state, run_id);
    match std::fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str::<ContextBlackboardState>(&raw).unwrap_or_default(),
        Err(_) => ContextBlackboardState::default(),
    }
}

fn save_context_blackboard(
    state: &AppState,
    run_id: &str,
    blackboard: &ContextBlackboardState,
) -> Result<(), StatusCode> {
    let path = context_run_blackboard_path(state, run_id);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    let payload =
        serde_json::to_string_pretty(blackboard).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(path, payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

fn apply_context_blackboard_patch(
    blackboard: &mut ContextBlackboardState,
    patch: &ContextBlackboardPatchRecord,
) -> Result<(), StatusCode> {
    let mut task_idx: HashMap<String, usize> = HashMap::new();
    for (idx, task) in blackboard.tasks.iter().enumerate() {
        task_idx.insert(task.id.clone(), idx);
    }
    match patch.op {
        ContextBlackboardPatchOp::AddFact => {
            let row = serde_json::from_value::<ContextBlackboardItem>(patch.payload.clone())
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            blackboard.facts.push(row);
        }
        ContextBlackboardPatchOp::AddDecision => {
            let row = serde_json::from_value::<ContextBlackboardItem>(patch.payload.clone())
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            blackboard.decisions.push(row);
        }
        ContextBlackboardPatchOp::AddOpenQuestion => {
            let row = serde_json::from_value::<ContextBlackboardItem>(patch.payload.clone())
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            blackboard.open_questions.push(row);
        }
        ContextBlackboardPatchOp::AddArtifact => {
            let row = serde_json::from_value::<ContextBlackboardArtifact>(patch.payload.clone())
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            blackboard.artifacts.push(row);
        }
        ContextBlackboardPatchOp::SetRollingSummary => {
            let value = patch
                .payload
                .as_str()
                .ok_or(StatusCode::BAD_REQUEST)?
                .to_string();
            blackboard.summaries.rolling = value;
        }
        ContextBlackboardPatchOp::SetLatestContextPack => {
            let value = patch
                .payload
                .as_str()
                .ok_or(StatusCode::BAD_REQUEST)?
                .to_string();
            blackboard.summaries.latest_context_pack = value;
        }
        ContextBlackboardPatchOp::AddTask => {
            let task = serde_json::from_value::<ContextBlackboardTask>(patch.payload.clone())
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            if let Some(idx) = task_idx.get(&task.id).copied() {
                blackboard.tasks[idx] = task;
            } else {
                blackboard.tasks.push(task);
            }
        }
        ContextBlackboardPatchOp::UpdateTaskLease => {
            let task_id = patch
                .payload
                .get("task_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or(StatusCode::BAD_REQUEST)?
                .to_string();
            let Some(idx) = task_idx.get(&task_id).copied() else {
                return Err(StatusCode::NOT_FOUND);
            };
            let task = blackboard.tasks.get_mut(idx).ok_or(StatusCode::NOT_FOUND)?;
            task.lease_owner = patch
                .payload
                .get("lease_owner")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            task.lease_token = patch
                .payload
                .get("lease_token")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            task.lease_expires_at_ms = patch
                .payload
                .get("lease_expires_at_ms")
                .and_then(Value::as_u64);
            if let Some(agent_id) = patch
                .payload
                .get("assigned_agent")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                task.assigned_agent = Some(agent_id.to_string());
            }
            if let Some(next_rev) = patch.payload.get("task_rev").and_then(Value::as_u64) {
                task.task_rev = next_rev;
            } else {
                task.task_rev = task.task_rev.saturating_add(1);
            }
            task.updated_ts = patch.ts_ms;
        }
        ContextBlackboardPatchOp::UpdateTaskState => {
            let task_id = patch
                .payload
                .get("task_id")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .ok_or(StatusCode::BAD_REQUEST)?
                .to_string();
            let Some(idx) = task_idx.get(&task_id).copied() else {
                return Err(StatusCode::NOT_FOUND);
            };
            let task = blackboard.tasks.get_mut(idx).ok_or(StatusCode::NOT_FOUND)?;
            let status = patch
                .payload
                .get("status")
                .cloned()
                .ok_or(StatusCode::BAD_REQUEST)
                .and_then(|row| {
                    serde_json::from_value::<ContextBlackboardTaskStatus>(row)
                        .map_err(|_| StatusCode::BAD_REQUEST)
                })?;
            task.status = status;
            if let Some(lease_owner) = patch
                .payload
                .get("lease_owner")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                task.lease_owner = Some(lease_owner.to_string());
            }
            if patch.payload.get("lease_owner").is_some()
                && patch
                    .payload
                    .get("lease_owner")
                    .and_then(Value::as_str)
                    .is_none()
            {
                task.lease_owner = None;
            }
            if let Some(lease_token) = patch
                .payload
                .get("lease_token")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                task.lease_token = Some(lease_token.to_string());
            }
            if patch.payload.get("lease_token").is_some()
                && patch
                    .payload
                    .get("lease_token")
                    .and_then(Value::as_str)
                    .is_none()
            {
                task.lease_token = None;
            }
            task.lease_expires_at_ms = patch
                .payload
                .get("lease_expires_at_ms")
                .and_then(Value::as_u64);
            if let Some(agent_id) = patch
                .payload
                .get("assigned_agent")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                task.assigned_agent = Some(agent_id.to_string());
            }
            if let Some(err_text) = patch
                .payload
                .get("error")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                task.last_error = Some(err_text.to_string());
            }
            if let Some(next_retry_at_ms) = patch
                .payload
                .get("next_retry_at_ms")
                .and_then(Value::as_u64)
            {
                task.next_retry_at_ms = Some(next_retry_at_ms);
            }
            if let Some(next_attempt) = patch.payload.get("attempt").and_then(Value::as_u64) {
                task.attempt = next_attempt as u32;
            }
            if let Some(next_rev) = patch.payload.get("task_rev").and_then(Value::as_u64) {
                task.task_rev = next_rev;
            } else {
                task.task_rev = task.task_rev.saturating_add(1);
            }
            task.updated_ts = patch.ts_ms;
        }
    }
    blackboard.revision = patch.seq;
    Ok(())
}

fn load_context_blackboard_patches(
    state: &AppState,
    run_id: &str,
    since_seq: Option<u64>,
    tail: Option<usize>,
) -> Vec<ContextBlackboardPatchRecord> {
    let path = context_run_blackboard_patches_path(state, run_id);
    let rows = std::fs::read_to_string(path).unwrap_or_default();
    let mut parsed = rows
        .lines()
        .filter_map(|line| serde_json::from_str::<ContextBlackboardPatchRecord>(line).ok())
        .filter(|row| {
            if let Some(min_seq) = since_seq {
                return row.seq > min_seq;
            }
            true
        })
        .collect::<Vec<_>>();
    parsed.sort_by_key(|row| row.seq);
    if let Some(tail_count) = tail {
        if parsed.len() > tail_count {
            parsed = parsed.split_off(parsed.len().saturating_sub(tail_count));
        }
    }
    parsed
}

fn next_context_blackboard_patch_seq(state: &AppState, run_id: &str) -> u64 {
    load_context_blackboard_patches(state, run_id, None, Some(1))
        .last()
        .map(|row| row.seq)
        .unwrap_or(0)
        .saturating_add(1)
}

fn append_context_blackboard_patch(
    state: &AppState,
    run_id: &str,
    op: ContextBlackboardPatchOp,
    payload: Value,
) -> Result<(ContextBlackboardPatchRecord, ContextBlackboardState), StatusCode> {
    let patch = ContextBlackboardPatchRecord {
        patch_id: format!("bbp-{}", Uuid::new_v4()),
        run_id: run_id.to_string(),
        seq: next_context_blackboard_patch_seq(state, run_id),
        ts_ms: crate::now_ms(),
        op,
        payload,
    };
    append_jsonl_line(
        &context_run_blackboard_patches_path(state, run_id),
        &serde_json::to_value(&patch).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    )?;
    let mut blackboard = load_context_blackboard(state, run_id);
    apply_context_blackboard_patch(&mut blackboard, &patch)?;
    save_context_blackboard(state, run_id, &blackboard)?;
    Ok((patch, blackboard))
}

fn context_task_status_event_name(status: &ContextBlackboardTaskStatus) -> &'static str {
    match status {
        ContextBlackboardTaskStatus::InProgress => "context.task.started",
        ContextBlackboardTaskStatus::Done => "context.task.completed",
        ContextBlackboardTaskStatus::Failed => "context.task.failed",
        ContextBlackboardTaskStatus::Blocked => "context.task.blocked",
        ContextBlackboardTaskStatus::Runnable | ContextBlackboardTaskStatus::Pending => {
            "context.task.requeued"
        }
    }
}

fn context_run_lock_map(
) -> &'static std::sync::OnceLock<tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>
{
    static LOCKS: std::sync::OnceLock<
        tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    > = std::sync::OnceLock::new();
    &LOCKS
}

async fn context_run_lock_for(run_id: &str) -> Arc<tokio::sync::Mutex<()>> {
    let map = context_run_lock_map().get_or_init(|| tokio::sync::Mutex::new(HashMap::new()));
    let mut guard = map.lock().await;
    guard
        .entry(run_id.to_string())
        .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
        .clone()
}

async fn context_run_create(
    State(state): State<AppState>,
    Json(input): Json<ContextRunCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let run_id = input
        .run_id
        .unwrap_or_else(|| format!("run-{}", Uuid::new_v4()));
    ensure_context_run_dir(&state, &run_id).await?;
    let run_path = context_run_state_path(&state, &run_id);
    if run_path.exists() {
        return Ok(Json(json!({
            "error": "run already exists",
            "code": "CONTEXT_RUN_EXISTS",
            "run_id": run_id
        })));
    }
    let now = crate::now_ms();
    let run = ContextRunState {
        run_id: run_id.clone(),
        run_type: input.run_type.unwrap_or_else(|| "interactive".to_string()),
        source_client: input
            .source_client
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        model_provider: input
            .model_provider
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        model_id: input
            .model_id
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty()),
        mcp_servers: input
            .mcp_servers
            .unwrap_or_default()
            .into_iter()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .collect::<Vec<_>>(),
        status: ContextRunStatus::Queued,
        objective: input.objective,
        workspace: input.workspace.unwrap_or_default(),
        steps: Vec::new(),
        why_next_step: None,
        revision: 1,
        created_at_ms: now,
        started_at_ms: None,
        ended_at_ms: None,
        last_error: None,
        updated_at_ms: now,
    };
    save_context_run_state(&state, &run).await?;
    Ok(Json(json!({ "ok": true, "run": run })))
}

fn truncate_for_stream(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut out = input[..max_len].to_string();
    out.push_str("...<truncated>");
    out
}

#[cfg(test)]
mod tests;
