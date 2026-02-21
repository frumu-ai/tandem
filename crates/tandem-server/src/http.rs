use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::header::{self, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::{get, post, put};
use axum::{Json, Router};
use futures::Stream;
use ignore::WalkBuilder;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tandem_memory::{
    MemoryCapabilities, MemoryCapabilityToken, MemoryPromoteRequest, MemoryPromoteResponse,
    MemoryPutRequest, MemoryPutResponse, MemorySearchRequest, MemorySearchResponse, ScrubReport,
    ScrubStatus,
};
use tandem_orchestrator::{
    AgentInstanceStatus, DefaultMissionReducer, MissionEvent, MissionReducer, MissionSpec,
    NoopMissionReducer, SpawnRequest, SpawnSource, WorkItem, WorkItemStatus,
};
use tandem_skills::{SkillLocation, SkillService, SkillsConflictPolicy};
use tokio::process::Command;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::{Any, CorsLayer};
use uuid::Uuid;

use tandem_channels::start_channel_listeners;
use tandem_types::{
    CreateSessionRequest, EngineEvent, Message, MessagePart, MessagePartInput, MessageRole,
    SendMessageRequest, Session, TodoItem,
};
use tandem_wire::{
    WireProviderCatalog, WireProviderEntry, WireProviderModel, WireProviderModelLimit, WireSession,
    WireSessionMessage,
};

use crate::ResourceStoreError;
use crate::{
    agent_teams::{emit_spawn_approved, emit_spawn_denied, emit_spawn_requested},
    evaluate_routine_execution_policy, ActiveRun, AppState, ChannelStatus, DiscordConfigFile,
    RoutineExecutionDecision, RoutineHistoryEvent, RoutineMisfirePolicy, RoutineSchedule,
    RoutineSpec, RoutineStatus, RoutineStoreError, SlackConfigFile, StartupStatus,
    TelegramConfigFile,
};

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum SessionScope {
    Workspace,
    Global,
}

#[derive(Debug, Deserialize)]
struct PermissionReplyInput {
    reply: String,
}

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

#[derive(Debug, Serialize)]
struct AgentTeamToolApprovalOutput {
    #[serde(rename = "approvalID")]
    approval_id: String,
    #[serde(rename = "sessionID", skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(rename = "toolCallID")]
    tool_call_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    args: Option<Value>,
    status: String,
}

#[derive(Debug, Deserialize)]
struct FindTextQuery {
    pattern: String,
    path: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct FindFileQuery {
    q: String,
    path: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct FileListQuery {
    path: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct FileContentQuery {
    path: String,
}

#[derive(Debug, Deserialize, Default)]
struct PtyUpdateInput {
    input: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct LspQuery {
    action: Option<String>,
    path: Option<String>,
    symbol: Option<String>,
    q: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct WorktreeInput {
    path: Option<String>,
    branch: Option<String>,
    base: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct CommandRunInput {
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ShellRunInput {
    command: Option<String>,
    cwd: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct AuthInput {
    #[serde(alias = "apiKey", alias = "api_key")]
    token: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ApiTokenInput {
    #[serde(alias = "apiToken", alias = "api_token")]
    token: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct LogInput {
    level: Option<String>,
    message: Option<String>,
    context: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
struct PathInfoQuery {
    refresh: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct QuestionReplyInput {
    #[serde(default)]
    _answers: Vec<Vec<String>>,
}

#[derive(Debug, Deserialize, Default)]
struct QuestionAnswerInput {
    answer: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SkillLocationQuery {
    location: Option<SkillLocation>,
}

#[derive(Debug, Deserialize)]
struct SkillsImportRequest {
    content: Option<String>,
    file_or_path: Option<String>,
    location: SkillLocation,
    namespace: Option<String>,
    conflict_policy: Option<SkillsConflictPolicy>,
}

#[derive(Debug, Deserialize)]
struct SkillsTemplateInstallRequest {
    location: SkillLocation,
}

#[derive(Debug, Deserialize)]
struct MemoryPutInput {
    #[serde(flatten)]
    request: MemoryPutRequest,
    capability: Option<MemoryCapabilityToken>,
}

#[derive(Debug, Deserialize)]
struct MemoryPromoteInput {
    #[serde(flatten)]
    request: MemoryPromoteRequest,
    capability: Option<MemoryCapabilityToken>,
}

#[derive(Debug, Deserialize)]
struct MemorySearchInput {
    #[serde(flatten)]
    request: MemorySearchRequest,
    capability: Option<MemoryCapabilityToken>,
}

#[derive(Debug, Deserialize, Default)]
struct MemoryAuditQuery {
    run_id: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct MissionCreateInput {
    title: String,
    goal: String,
    #[serde(default)]
    work_items: Vec<MissionCreateWorkItem>,
}

#[derive(Debug, Deserialize)]
struct MissionCreateWorkItem {
    #[serde(default)]
    work_item_id: Option<String>,
    title: String,
    #[serde(default)]
    detail: Option<String>,
    #[serde(default)]
    assigned_agent: Option<String>,
}

#[derive(Debug, Deserialize)]
struct MissionEventInput {
    event: MissionEvent,
}

#[derive(Debug, Deserialize)]
struct AgentTeamSpawnInput {
    #[serde(rename = "missionID")]
    mission_id: Option<String>,
    #[serde(rename = "parentInstanceID")]
    parent_instance_id: Option<String>,
    #[serde(rename = "templateID")]
    template_id: Option<String>,
    role: tandem_orchestrator::AgentRole,
    source: Option<SpawnSource>,
    justification: String,
    #[serde(default)]
    budget_override: Option<tandem_orchestrator::BudgetLimit>,
}

#[derive(Debug, Deserialize, Default)]
struct AgentTeamInstancesQuery {
    #[serde(rename = "missionID")]
    mission_id: Option<String>,
    #[serde(rename = "parentInstanceID")]
    parent_instance_id: Option<String>,
    status: Option<AgentInstanceStatus>,
}

#[derive(Debug, Deserialize, Default)]
struct AgentTeamCancelInput {
    reason: Option<String>,
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
    creator_type: Option<String>,
    creator_id: Option<String>,
    requires_approval: Option<bool>,
    external_integrations_allowed: Option<bool>,
    next_fire_at_ms: Option<u64>,
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
struct RoutineEventsQuery {
    routine_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ResourceListQuery {
    prefix: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize, Default)]
struct ResourceEventsQuery {
    prefix: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct MemoryListQuery {
    q: Option<String>,
    limit: Option<usize>,
    offset: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct ResourceWriteInput {
    value: Value,
    if_match_rev: Option<u64>,
    updated_by: Option<String>,
    ttl_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ResourceDeleteInput {
    if_match_rev: Option<u64>,
    updated_by: Option<String>,
}

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}

#[derive(Debug, Serialize)]
struct LegacyProviderInfo {
    id: String,
    name: String,
    models: Vec<String>,
    configured: bool,
}

pub async fn serve(addr: SocketAddr, state: AppState) -> anyhow::Result<()> {
    let reaper_state = state.clone();
    let status_indexer_state = state.clone();
    let routine_scheduler_state = state.clone();
    let agent_team_supervisor_state = state.clone();
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
    let agent_team_supervisor = tokio::spawn(crate::run_agent_team_supervisor(
        agent_team_supervisor_state,
    ));

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
    agent_team_supervisor.abort();
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

fn app_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut router = Router::new()
        .route("/global/health", get(global_health))
        .route("/global/event", get(events))
        .route("/global/lease/acquire", post(global_lease_acquire))
        .route("/global/lease/renew", post(global_lease_renew))
        .route("/global/lease/release", post(global_lease_release))
        .route("/global/storage/repair", post(global_storage_repair))
        .route(
            "/global/config",
            get(global_config).patch(global_config_patch),
        )
        .route("/global/dispose", post(global_dispose))
        .route("/event", get(events))
        .route("/project", get(list_projects))
        .route("/session", post(create_session).get(list_sessions))
        .route("/api/session", post(create_session).get(list_sessions))
        .route("/session/status", get(session_status))
        .route(
            "/session/{id}",
            get(get_session)
                .delete(delete_session)
                .patch(update_session),
        )
        .route("/session/{id}/attach", post(attach_session))
        .route(
            "/session/{id}/workspace/override",
            post(grant_workspace_override),
        )
        .route(
            "/api/session/{id}",
            get(get_session)
                .delete(delete_session)
                .patch(update_session),
        )
        .route("/api/session/{id}/attach", post(attach_session))
        .route(
            "/api/session/{id}/workspace/override",
            post(grant_workspace_override),
        )
        .route(
            "/session/{id}/message",
            get(session_messages).post(post_session_message_append),
        )
        .route(
            "/api/session/{id}/message",
            get(session_messages).post(post_session_message_append),
        )
        .route("/session/{id}/todo", get(session_todos))
        .route("/api/session/{id}/todo", get(session_todos))
        .route("/session/{id}/prompt_async", post(prompt_async))
        .route("/api/session/{id}/prompt_async", post(prompt_async))
        .route("/session/{id}/prompt_sync", post(prompt_sync))
        .route("/api/session/{id}/prompt_sync", post(prompt_sync))
        .route("/session/{id}/run", get(get_active_run))
        .route("/api/session/{id}/run", get(get_active_run))
        .route("/session/{id}/abort", post(abort_session))
        .route("/session/{id}/cancel", post(abort_session))
        .route("/api/session/{id}/cancel", post(abort_session))
        .route("/session/{id}/run/{run_id}/cancel", post(cancel_run_by_id))
        .route(
            "/api/session/{id}/run/{run_id}/cancel",
            post(cancel_run_by_id),
        )
        .route("/session/{id}/fork", post(fork_session))
        .route("/session/{id}/revert", post(revert_session))
        .route("/session/{id}/unrevert", post(unrevert_session))
        .route(
            "/session/{id}/share",
            post(share_session).delete(unshare_session),
        )
        .route("/session/{id}/summarize", post(summarize_session))
        .route("/session/{id}/diff", get(session_diff))
        .route("/session/{id}/children", get(session_children))
        .route("/session/{id}/init", post(init_session))
        .route("/permission", get(list_permissions))
        .route("/permission/{id}/reply", post(reply_permission))
        .route(
            "/sessions/{session_id}/tools/{tool_call_id}/approve",
            post(approve_tool_by_call),
        )
        .route(
            "/sessions/{session_id}/tools/{tool_call_id}/deny",
            post(deny_tool_by_call),
        )
        .route("/question", get(list_questions))
        .route("/question/{id}/reply", post(reply_question))
        .route("/question/{id}/reject", post(reject_question))
        .route(
            "/sessions/{session_id}/questions/{question_id}/answer",
            post(answer_question),
        )
        .route("/provider", get(list_providers))
        .route("/providers", get(list_providers_legacy))
        .route("/api/providers", get(list_providers_legacy))
        .route("/provider/auth", get(provider_auth))
        .route(
            "/provider/{id}/oauth/authorize",
            post(provider_oauth_authorize),
        )
        .route(
            "/provider/{id}/oauth/callback",
            post(provider_oauth_callback),
        )
        .route("/config", get(get_config).patch(patch_config))
        .route("/config/providers", get(config_providers))
        .route("/mcp", get(list_mcp).post(add_mcp))
        .route("/mcp/{name}/connect", post(connect_mcp))
        .route("/mcp/{name}/disconnect", post(disconnect_mcp))
        .route("/mcp/{name}/auth", post(auth_mcp).delete(delete_auth_mcp))
        .route("/mcp/{name}/auth/callback", post(callback_mcp))
        .route("/mcp/{name}/auth/authenticate", post(authenticate_mcp))
        .route("/mcp/resources", get(mcp_resources))
        .route("/tool/ids", get(tool_ids))
        .route("/tool", get(tool_list_for_model))
        .route("/tool/execute", post(execute_tool))
        .route(
            "/worktree",
            get(list_worktrees)
                .post(create_worktree)
                .delete(delete_worktree),
        )
        .route("/worktree/reset", post(reset_worktree))
        .route("/find", get(find_text))
        .route("/find/file", get(find_file))
        .route("/find/symbol", get(find_symbol))
        .route("/file", get(file_list))
        .route("/file/content", get(file_content))
        .route("/file/status", get(file_status))
        .route("/vcs", get(vcs))
        .route("/pty", get(pty_list).post(pty_create))
        .route("/pty/{id}", get(pty_get).put(pty_update).delete(pty_delete))
        .route("/pty/{id}/ws", get(pty_ws))
        .route("/lsp", get(lsp_status))
        .route("/formatter", get(formatter_status))
        .route("/command", get(command_list))
        .route("/session/{id}/command", post(run_command))
        .route("/session/{id}/shell", post(run_shell))
        .route("/auth/{id}", put(set_auth).delete(delete_auth))
        .route("/auth/token", put(set_api_token).delete(clear_api_token))
        .route("/auth/token/generate", post(generate_api_token))
        .route("/path", get(path_info))
        .route("/agent", get(agent_list))
        .route("/skills", get(skills_list).post(skills_import))
        .route("/skills/import", post(skills_import))
        .route("/skills/import/preview", post(skills_import_preview))
        .route("/skills/templates", get(skills_templates_list))
        .route(
            "/skills/templates/{id}/install",
            post(skills_templates_install),
        )
        .route("/skills/{name}", get(skills_get).delete(skills_delete))
        .route("/memory/put", post(memory_put))
        .route("/memory/promote", post(memory_promote))
        .route("/memory/search", post(memory_search))
        .route("/memory/audit", get(memory_audit))
        .route("/memory", get(memory_list))
        .route("/memory/{id}", axum::routing::delete(memory_delete))
        .route("/channels/config", get(channels_config))
        .route("/channels/status", get(channels_status))
        .route(
            "/channels/{name}",
            put(channels_put).delete(channels_delete),
        )
        .route("/admin/reload-config", post(admin_reload_config))
        .route("/mission", get(mission_list).post(mission_create))
        .route("/mission/{id}", get(mission_get))
        .route("/mission/{id}/event", post(mission_apply_event))
        .route("/agent-team/templates", get(agent_team_templates))
        .route("/agent-team/instances", get(agent_team_instances))
        .route("/agent-team/missions", get(agent_team_missions))
        .route("/agent-team/approvals", get(agent_team_approvals))
        .route(
            "/agent-team/approvals/spawn/{id}/approve",
            post(agent_team_approve_spawn),
        )
        .route(
            "/agent-team/approvals/spawn/{id}/deny",
            post(agent_team_deny_spawn),
        )
        .route("/agent-team/spawn", post(agent_team_spawn))
        .route(
            "/agent-team/instance/{id}/cancel",
            post(agent_team_cancel_instance),
        )
        .route(
            "/agent-team/mission/{id}/cancel",
            post(agent_team_cancel_mission),
        )
        .route("/routines", get(routines_list).post(routines_create))
        .route("/routines/events", get(routines_events))
        .route(
            "/routines/{id}",
            axum::routing::patch(routines_patch).delete(routines_delete),
        )
        .route("/routines/{id}/run_now", post(routines_run_now))
        .route("/routines/{id}/history", get(routines_history))
        .route("/resource", get(resource_list))
        .route("/resource/events", get(resource_events))
        .route(
            "/resource/{*key}",
            get(resource_get)
                .put(resource_put)
                .patch(resource_patch)
                .delete(resource_delete),
        )
        .route("/skill", get(skill_list))
        .route("/instance/dispose", post(instance_dispose))
        .route("/log", post(push_log))
        .route("/doc", get(openapi_doc));

    if state.web_ui_enabled() {
        router = router.merge(crate::webui::web_ui_router(&state.web_ui_prefix()));
    }

    router
        .layer(cors)
        .layer(middleware::from_fn_with_state(state.clone(), startup_gate))
        .layer(middleware::from_fn_with_state(state.clone(), auth_gate))
        .with_state(state)
}

async fn auth_gate(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if request.method() == Method::OPTIONS {
        return next.run(request).await;
    }
    let path = request.uri().path();
    if state.web_ui_enabled() && request.uri().path().starts_with(&state.web_ui_prefix()) {
        return next.run(request).await;
    }

    if path == "/global/health" {
        return next.run(request).await;
    }

    let required = state.api_token().await;
    let Some(expected) = required else {
        return next.run(request).await;
    };

    // Allow initial token bootstrap endpoints only when token auth is currently disabled.
    // Once a token is configured, these endpoints also require auth.
    let provided = extract_request_token(request.headers());
    if provided.as_deref() == Some(expected.as_str()) {
        return next.run(request).await;
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(ErrorEnvelope {
            error: "Unauthorized: missing or invalid API token".to_string(),
            code: Some("AUTH_REQUIRED".to_string()),
        }),
    )
        .into_response()
}

fn extract_request_token(headers: &HeaderMap) -> Option<String> {
    if let Some(token) = headers
        .get("x-tandem-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(token.to_string());
    }

    let auth = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())?;
    let trimmed = auth.trim();
    let bearer = trimmed
        .strip_prefix("Bearer ")
        .or_else(|| trimmed.strip_prefix("bearer "))?;
    let token = bearer.trim();
    if token.is_empty() {
        None
    } else {
        Some(token.to_string())
    }
}

async fn startup_gate(State(state): State<AppState>, request: Request, next: Next) -> Response {
    if request.method() == Method::OPTIONS {
        return next.run(request).await;
    }
    if request.uri().path() == "/global/health" {
        return next.run(request).await;
    }
    if state.is_ready() {
        return next.run(request).await;
    }

    let snapshot = state.startup_snapshot().await;
    let status_text = match snapshot.status {
        StartupStatus::Starting => "starting",
        StartupStatus::Ready => "ready",
        StartupStatus::Failed => "failed",
    };
    let code = match snapshot.status {
        StartupStatus::Failed => "ENGINE_STARTUP_FAILED",
        _ => "ENGINE_STARTING",
    };
    let error = format!(
        "Engine {}: phase={} attempt_id={} elapsed_ms={}{}",
        status_text,
        snapshot.phase,
        snapshot.attempt_id,
        snapshot.elapsed_ms,
        snapshot
            .last_error
            .as_ref()
            .map(|e| format!(" error={}", e))
            .unwrap_or_default()
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(ErrorEnvelope {
            error,
            code: Some(code.to_string()),
        }),
    )
        .into_response()
}

async fn global_health(State(state): State<AppState>) -> impl IntoResponse {
    let now = crate::now_ms();
    let lease_count = {
        let mut leases = state.engine_leases.write().await;
        leases.retain(|_, lease| !lease.is_expired(now));
        leases.len()
    };
    let startup = state.startup_snapshot().await;
    let build_id = crate::build_id();
    let binary_path = crate::binary_path_for_health();
    let environment = state.host_runtime_context();
    Json(json!({
        "healthy": true,
        "ready": state.is_ready(),
        "apiTokenRequired": state.api_token().await.is_some(),
        "phase": startup.phase,
        "startup_attempt_id": startup.attempt_id,
        "startup_elapsed_ms": startup.elapsed_ms,
        "last_error": startup.last_error,
        "version": env!("CARGO_PKG_VERSION"),
        "build_id": build_id,
        "binary_path": binary_path,
        "mode": state.mode_label(),
        "leaseCount": lease_count,
        "environment": environment
    }))
}

async fn global_lease_acquire(
    State(state): State<AppState>,
    Json(input): Json<EngineLeaseAcquireInput>,
) -> Json<Value> {
    let now = crate::now_ms();
    let lease_id = Uuid::new_v4().to_string();
    let lease = crate::EngineLease {
        lease_id: lease_id.clone(),
        client_id: input
            .client_id
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "unknown".to_string()),
        client_type: input
            .client_type
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_else(|| "unknown".to_string()),
        acquired_at_ms: now,
        last_renewed_at_ms: now,
        ttl_ms: input.ttl_ms.unwrap_or(60_000).clamp(5_000, 10 * 60_000),
    };
    let mut leases = state.engine_leases.write().await;
    leases.retain(|_, l| !l.is_expired(now));
    leases.insert(lease_id.clone(), lease.clone());
    Json(json!({
        "lease_id": lease_id,
        "client_id": lease.client_id,
        "client_type": lease.client_type,
        "acquired_at_ms": lease.acquired_at_ms,
        "last_renewed_at_ms": lease.last_renewed_at_ms,
        "ttl_ms": lease.ttl_ms,
        "lease_count": leases.len()
    }))
}

async fn global_lease_renew(
    State(state): State<AppState>,
    Json(input): Json<EngineLeaseRenewInput>,
) -> Json<Value> {
    let now = crate::now_ms();
    let mut leases = state.engine_leases.write().await;
    leases.retain(|_, l| !l.is_expired(now));
    let renewed = if let Some(lease) = leases.get_mut(&input.lease_id) {
        lease.last_renewed_at_ms = now;
        true
    } else {
        false
    };
    Json(json!({ "ok": renewed, "lease_count": leases.len() }))
}

async fn global_lease_release(
    State(state): State<AppState>,
    Json(input): Json<EngineLeaseReleaseInput>,
) -> Json<Value> {
    let now = crate::now_ms();
    let mut leases = state.engine_leases.write().await;
    leases.retain(|_, l| !l.is_expired(now));
    let removed = leases.remove(&input.lease_id).is_some();
    Json(json!({ "ok": removed, "lease_count": leases.len() }))
}

async fn global_storage_repair(
    State(state): State<AppState>,
    Json(input): Json<StorageRepairInput>,
) -> Result<Json<Value>, StatusCode> {
    let force = input.force.unwrap_or(false);
    let report = state
        .storage
        .run_legacy_storage_repair_scan(force)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "status": report.status,
        "marker_updated": report.marker_updated,
        "sessions_merged": report.sessions_merged,
        "messages_recovered": report.messages_recovered,
        "parts_recovered": report.parts_recovered,
        "legacy_counts": report.legacy_counts,
        "imported_counts": report.imported_counts,
    })))
}

fn sse_stream(
    state: AppState,
    filter: EventFilterQuery,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let rx = state.event_bus.subscribe();
    let initial = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&EngineEvent::new("server.connected", json!({}))).unwrap_or_default(),
    )));
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&EngineEvent::new(
            "engine.lifecycle.ready",
            json!({
                "status": "ready",
                "transport": "sse",
                "timestamp_ms": crate::now_ms(),
            }),
        ))
        .unwrap_or_default(),
    )));
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            if !event_matches_filter(&event, &filter) {
                return None;
            }
            let normalized = if let Some(run_id) = filter.run_id.as_deref() {
                let session_hint = filter
                    .session_id
                    .as_deref()
                    .or_else(|| {
                        event
                            .properties
                            .get("sessionID")
                            .or_else(|| event.properties.get("sessionId"))
                            .and_then(|v| v.as_str())
                    })
                    .unwrap_or_default()
                    .to_string();
                normalize_run_event(event, &session_hint, run_id)
            } else {
                event
            };
            let payload = serde_json::to_string(&normalized).unwrap_or_default();
            let payload = truncate_for_stream(&payload, 16_000);
            Some(Ok(Event::default().data(payload)))
        }
        Err(_) => None,
    });
    initial.chain(ready).chain(live)
}

async fn events(
    State(state): State<AppState>,
    Query(filter): Query<EventFilterQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(sse_stream(state, filter))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}

fn event_matches_filter(event: &EngineEvent, filter: &EventFilterQuery) -> bool {
    if filter.session_id.is_none() && filter.run_id.is_none() {
        return true;
    }
    let event_session = event
        .properties
        .get("sessionID")
        .or_else(|| event.properties.get("sessionId"))
        .or_else(|| event.properties.get("id"))
        .and_then(|v| v.as_str());
    if let Some(session_id) = filter.session_id.as_deref() {
        if event_session != Some(session_id) {
            return false;
        }
    }
    if let Some(run_id) = filter.run_id.as_deref() {
        let event_run = event
            .properties
            .get("runID")
            .or_else(|| event.properties.get("run_id"))
            .and_then(|v| v.as_str());
        if let Some(value) = event_run {
            return value == run_id;
        }
        return filter.session_id.is_some() && event_session.is_some();
    }
    true
}

async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<CreateSessionRequest>,
) -> Result<Json<WireSession>, StatusCode> {
    let requested_permission_rules = req.permission.clone();
    let mut session = Session::new(req.title, req.directory);
    let workspace_from_runtime = {
        let snapshot = state.workspace_index.snapshot().await;
        tandem_core::normalize_workspace_path(&snapshot.root)
    };
    let workspace = req
        .workspace_root
        .as_deref()
        .and_then(tandem_core::normalize_workspace_path)
        .or_else(|| tandem_core::normalize_workspace_path(&session.directory))
        .or(workspace_from_runtime);
    if let Some(workspace) = workspace {
        session.workspace_root = Some(workspace.clone());
        if session.directory.trim() == "." || session.directory.trim().is_empty() {
            session.directory = workspace;
        }
    }
    session.environment = Some(state.host_runtime_context());
    session.model = req.model;
    session.provider = req.provider;
    state
        .storage
        .save_session(session.clone())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    apply_session_permission_rules(&state, requested_permission_rules).await;
    state.event_bus.publish(EngineEvent::new(
        "session.created",
        json!({"sessionID": session.id}),
    ));
    Ok(Json(session.into()))
}

async fn apply_session_permission_rules(state: &AppState, rules: Option<Vec<serde_json::Value>>) {
    let Some(rules) = rules else {
        return;
    };
    for raw in rules {
        let Some((permission, pattern, action)) = parse_permission_rule_input(&raw) else {
            continue;
        };
        let _ = state
            .permissions
            .add_rule(permission, pattern, action)
            .await;
    }
}

fn parse_permission_rule_input(
    raw: &serde_json::Value,
) -> Option<(String, String, tandem_core::PermissionAction)> {
    let obj = raw.as_object()?;
    let permission = obj.get("permission")?.as_str()?.trim().to_string();
    if permission.is_empty() {
        return None;
    }
    let pattern = obj
        .get("pattern")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or(permission.as_str())
        .to_string();
    let action = obj.get("action").and_then(|v| v.as_str())?;
    let action = match action.trim().to_ascii_lowercase().as_str() {
        "allow" | "always" => tandem_core::PermissionAction::Allow,
        "ask" | "once" => tandem_core::PermissionAction::Ask,
        "deny" | "reject" => tandem_core::PermissionAction::Deny,
        _ => return None,
    };
    Some((permission, pattern, action))
}

async fn list_sessions(
    State(state): State<AppState>,
    Query(query): Query<ListSessionsQuery>,
) -> Json<Vec<WireSession>> {
    let workspace_from_query = query
        .workspace
        .as_deref()
        .and_then(tandem_core::normalize_workspace_path);
    let workspace_from_runtime = {
        let snapshot = state.workspace_index.snapshot().await;
        tandem_core::normalize_workspace_path(&snapshot.root)
    };
    let effective_scope = query.scope.unwrap_or_else(|| {
        if workspace_from_query.is_some() || workspace_from_runtime.is_some() {
            SessionScope::Workspace
        } else {
            SessionScope::Global
        }
    });
    let mut sessions = match effective_scope {
        SessionScope::Global => {
            state
                .storage
                .list_sessions_scoped(tandem_core::SessionListScope::Global)
                .await
        }
        SessionScope::Workspace => {
            let workspace = workspace_from_query.or(workspace_from_runtime);
            match workspace {
                Some(workspace_root) => {
                    state
                        .storage
                        .list_sessions_scoped(tandem_core::SessionListScope::Workspace {
                            workspace_root,
                        })
                        .await
                }
                None => Vec::new(),
            }
        }
    };
    let total_after_scope = sessions.len();
    sessions.sort_by(|a, b| b.time.updated.cmp(&a.time.updated));

    if let Some(archived) = query.archived {
        let mut filtered = Vec::new();
        for session in sessions {
            let status = state.storage.session_status(&session.id).await;
            let is_archived = status
                .as_ref()
                .and_then(|v| v.get("archived"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if is_archived == archived {
                filtered.push(session);
            }
        }
        sessions = filtered;
    }
    if let Some(q) = query.q.as_ref() {
        let q_lower = q.to_lowercase();
        sessions.retain(|session| {
            session.title.to_lowercase().contains(&q_lower)
                || session.directory.to_lowercase().contains(&q_lower)
        });
    }

    let page_size = query.page_size.unwrap_or(20).max(1);
    let page = query.page.unwrap_or(1).max(1);
    let start = (page - 1) * page_size;
    let items = sessions
        .into_iter()
        .skip(start)
        .take(page_size)
        .map(Into::into)
        .collect::<Vec<WireSession>>();
    tracing::debug!(
        "session.list scope={:?} matched={} page={} page_size={}",
        effective_scope,
        total_after_scope,
        page,
        page_size
    );
    Json(items)
}

async fn attach_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AttachSessionInput>,
) -> Result<Json<WireSession>, StatusCode> {
    let reason = input
        .reason_tag
        .unwrap_or_else(|| "manual_attach".to_string());
    let session = state
        .storage
        .attach_session_to_workspace(&id, &input.target_workspace, &reason)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    state.event_bus.publish(EngineEvent::new(
        "session.attached",
        json!({
            "sessionID": session.id,
            "workspaceRoot": session.workspace_root,
            "attachedFromWorkspace": session.attached_from_workspace,
            "attachedToWorkspace": session.attached_to_workspace,
            "attachReason": session.attach_reason
        }),
    ));
    Ok(Json(session.into()))
}

async fn grant_workspace_override(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<WorkspaceOverrideInput>,
) -> Result<Json<Value>, StatusCode> {
    if state.storage.get_session(&id).await.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    let ttl = input.ttl_seconds.unwrap_or(900).clamp(30, 86_400);
    let expires_at = state
        .engine_loop
        .grant_workspace_override_for_session(&id, ttl)
        .await;
    state.event_bus.publish(EngineEvent::new(
        "session.workspace_override.granted",
        json!({
            "sessionID": id,
            "ttlSeconds": ttl,
            "expiresAtMs": expires_at
        }),
    ));
    Ok(Json(json!({
        "ok": true,
        "ttlSeconds": ttl,
        "expiresAtMs": expires_at
    })))
}

async fn get_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WireSession>, StatusCode> {
    state
        .storage
        .get_session(&id)
        .await
        .map(|session| Json(session.into()))
        .ok_or(StatusCode::NOT_FOUND)
}

async fn delete_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let deleted = state
        .storage
        .delete_session(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"deleted": deleted})))
}

async fn session_messages(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let messages = session
        .messages
        .iter()
        .map(|msg| WireSessionMessage::from_message(msg, &id))
        .collect::<Vec<_>>();
    Ok(Json(json!(messages)))
}

async fn prompt_async(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<PromptAsyncQuery>,
    headers: HeaderMap,
    Json(req): Json<SendMessageRequest>,
) -> Result<Response, StatusCode> {
    if state.storage.get_session(&id).await.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    let session_id = id.clone();
    let correlation_id = headers
        .get("x-tandem-correlation-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let client_id = headers
        .get("x-tandem-client-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let run_id = Uuid::new_v4().to_string();

    let active_run = match state
        .run_registry
        .acquire(
            &session_id,
            run_id.clone(),
            client_id.clone(),
            req.agent.clone(),
            req.agent.clone(),
        )
        .await
    {
        Ok(run) => run,
        Err(active) => {
            let payload = conflict_payload(&session_id, &active);
            state.event_bus.publish(EngineEvent::new(
                "session.run.conflict",
                json!({
                    "sessionID": session_id,
                    "runID": active.run_id,
                    "retryAfterMs": 500,
                    "attachEventStream": attach_event_stream_path(&id, &active.run_id),
                }),
            ));
            return Ok((StatusCode::CONFLICT, Json(payload)).into_response());
        }
    };

    tracing::info!(
        target: "tandem.obs",
        event = "server.prompt_async.start",
        component = "http.prompt_async",
        session_id = %session_id,
        correlation_id = %correlation_id.as_deref().unwrap_or(""),
        "prompt_async request accepted"
    );
    state.event_bus.publish(EngineEvent::new(
        "session.run.started",
        json!({
            "sessionID": session_id,
            "runID": active_run.run_id,
            "startedAtMs": active_run.started_at_ms,
            "clientID": active_run.client_id,
            "agentID": active_run.agent_id,
            "agentProfile": active_run.agent_profile,
            "environment": state.host_runtime_context(),
        }),
    ));

    spawn_run_task(
        state.clone(),
        id.clone(),
        run_id.clone(),
        req,
        correlation_id,
    );

    if query.r#return.as_deref() == Some("run") {
        let mut response = (
            StatusCode::ACCEPTED,
            Json(json!({
                "runID": run_id,
                "attachEventStream": attach_event_stream_path(&id, &run_id),
            })),
        )
            .into_response();
        if let Ok(value) = HeaderValue::from_str(&run_id) {
            response.headers_mut().insert("x-tandem-run-id", value);
        }
        return Ok(response);
    }

    let mut response = StatusCode::NO_CONTENT.into_response();
    if let Ok(value) = HeaderValue::from_str(&run_id) {
        response.headers_mut().insert("x-tandem-run-id", value);
    }
    Ok(response)
}

async fn prompt_sync(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(req): Json<SendMessageRequest>,
) -> Result<Response, StatusCode> {
    if state.storage.get_session(&id).await.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    let accept_sse = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("text/event-stream"))
        .unwrap_or(false);
    let correlation_id = headers
        .get("x-tandem-correlation-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let client_id = headers
        .get("x-tandem-client-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());
    let agent_id = headers
        .get("x-tandem-agent-id")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .or_else(|| req.agent.clone());
    let agent_profile = req.agent.clone();
    let run_id = Uuid::new_v4().to_string();
    let active_run = match state
        .run_registry
        .acquire(
            &id,
            run_id.clone(),
            client_id.clone(),
            agent_id.clone(),
            agent_profile.clone(),
        )
        .await
    {
        Ok(run) => run,
        Err(active) => {
            let payload = conflict_payload(&id, &active);
            state.event_bus.publish(EngineEvent::new(
                "session.run.conflict",
                json!({
                    "sessionID": id,
                    "runID": active.run_id,
                    "retryAfterMs": 500,
                    "attachEventStream": attach_event_stream_path(&id, &active.run_id),
                }),
            ));
            return Ok((StatusCode::CONFLICT, Json(payload)).into_response());
        }
    };
    state.event_bus.publish(EngineEvent::new(
        "session.run.started",
        json!({
            "sessionID": id,
            "runID": active_run.run_id,
            "startedAtMs": active_run.started_at_ms,
            "clientID": active_run.client_id,
            "agentID": active_run.agent_id,
            "agentProfile": active_run.agent_profile,
            "environment": state.host_runtime_context(),
        }),
    ));

    if accept_sse {
        spawn_run_task(
            state.clone(),
            id.clone(),
            run_id.clone(),
            req,
            correlation_id,
        );
        let stream = sse_run_stream(
            state.clone(),
            id.clone(),
            run_id.clone(),
            agent_id.clone(),
            agent_profile.clone(),
        );
        return Ok(Sse::new(stream)
            .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
            .into_response());
    }

    let _ = execute_run(
        state.clone(),
        id.clone(),
        run_id.clone(),
        req,
        correlation_id,
    )
    .await;
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let messages = session
        .messages
        .iter()
        .map(|msg| WireSessionMessage::from_message(msg, &id))
        .collect::<Vec<_>>();
    Ok(Json(json!(messages)).into_response())
}

fn spawn_run_task(
    state: AppState,
    session_id: String,
    run_id: String,
    req: SendMessageRequest,
    correlation_id: Option<String>,
) {
    tokio::spawn(async move {
        let _ = execute_run(state, session_id, run_id, req, correlation_id).await;
    });
}

async fn execute_run(
    state: AppState,
    session_id: String,
    run_id: String,
    req: SendMessageRequest,
    correlation_id: Option<String>,
) -> anyhow::Result<()> {
    let mut run_fut = Box::pin(state.engine_loop.run_prompt_async_with_context(
        session_id.clone(),
        req,
        correlation_id.clone(),
    ));
    let mut timeout = Box::pin(tokio::time::sleep(Duration::from_secs(60 * 10)));
    let mut ticker = tokio::time::interval(Duration::from_secs(2));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    let (status, error_msg): (&str, Option<String>) = loop {
        tokio::select! {
            _ = ticker.tick() => {
                state.run_registry.touch(&session_id, &run_id).await;
            }
            _ = &mut timeout => {
                let _ = state.cancellations.cancel(&session_id).await;
                state.event_bus.publish(EngineEvent::new(
                    "session.error",
                    json!({
                        "sessionID": session_id,
                        "error": {
                            "code": "ENGINE_TIMEOUT",
                            "message": "prompt_async timed out",
                        }
                    }),
                ));
                state.event_bus.publish(EngineEvent::new(
                    "session.status",
                    json!({"sessionID": session_id, "status":"error"}),
                ));
                state.event_bus.publish(EngineEvent::new(
                    "session.updated",
                    json!({"sessionID": session_id, "status":"error"}),
                ));
                break ("timeout", Some("prompt_async timed out".to_string()));
            }
            result = &mut run_fut => {
                match result {
                    Ok(()) => break ("completed", None),
                    Err(err) => {
                        let error_message = err.to_string();
                        let error_code = dispatch_error_code(&error_message);
                        state.event_bus.publish(EngineEvent::new(
                            "session.error",
                            json!({
                                "sessionID": session_id,
                                "error": {
                                    "code": error_code,
                                    "message": truncate_text(&error_message, 500),
                                }
                            }),
                        ));
                        state.event_bus.publish(EngineEvent::new(
                            "session.status",
                            json!({"sessionID": session_id, "status":"error"}),
                        ));
                        state.event_bus.publish(EngineEvent::new(
                            "session.updated",
                            json!({"sessionID": session_id, "status":"error"}),
                        ));
                        let _ = state.cancellations.cancel(&session_id).await;
                        break ("error", Some(truncate_text(&error_message, 500)));
                    }
                }
            }
        }
    };

    let _ = state
        .run_registry
        .finish_if_match(&session_id, &run_id)
        .await;
    state.event_bus.publish(EngineEvent::new(
        "session.run.finished",
        json!({
            "sessionID": session_id,
            "runID": run_id,
            "finishedAtMs": crate::now_ms(),
            "status": status,
            "error": error_msg,
        }),
    ));
    Ok(())
}

fn sse_run_stream(
    state: AppState,
    session_id: String,
    run_id: String,
    agent_id: Option<String>,
    agent_profile: Option<String>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let rx = state.event_bus.subscribe();
    let started = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&EngineEvent::new(
            "session.run.started",
            json!({
                "sessionID": session_id,
                "runID": run_id,
                "startedAtMs": crate::now_ms(),
                "agentID": agent_id,
                "agentProfile": agent_profile,
                "channel": "system",
                "environment": state.host_runtime_context(),
            }),
        ))
        .unwrap_or_default(),
    )));
    let filter_session_id = session_id.clone();
    let filter_run_id = run_id.clone();
    let end_run_id = run_id.clone();
    let map_session_id = session_id.clone();
    let map_run_id = run_id.clone();

    // Ignore unrelated events from the shared bus and only terminate when this run finishes.
    let run_events = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) if event_matches_run(&event, &filter_session_id, &filter_run_id) => Some(event),
        _ => None,
    });
    let live = run_events.take_while(move |event| {
        let is_finished = event.event_type == "session.run.finished"
            && event
                .properties
                .get("runID")
                .and_then(|v| v.as_str())
                .map(|v| v == end_run_id.as_str())
                .unwrap_or(false);
        !is_finished
    });
    let mapped = live.map(move |event| {
        let normalized = normalize_run_event(event, &map_session_id, &map_run_id);
        let payload = serde_json::to_string(&normalized).unwrap_or_default();
        Ok(Event::default().data(payload))
    });
    started.chain(mapped)
}

fn conflict_payload(session_id: &str, active: &ActiveRun) -> Value {
    json!({
        "code": "SESSION_RUN_CONFLICT",
        "sessionID": session_id,
        "activeRun": {
            "runID": active.run_id,
            "startedAtMs": active.started_at_ms,
            "lastActivityAtMs": active.last_activity_at_ms,
            "clientID": active.client_id,
            "agentID": active.agent_id,
            "agentProfile": active.agent_profile,
        },
        "retryAfterMs": 500,
        "attachEventStream": attach_event_stream_path(session_id, &active.run_id),
    })
}

fn attach_event_stream_path(session_id: &str, run_id: &str) -> String {
    format!("/event?sessionID={session_id}&runID={run_id}")
}

fn event_matches_run(event: &EngineEvent, session_id: &str, run_id: &str) -> bool {
    let event_session = event
        .properties
        .get("sessionID")
        .or_else(|| event.properties.get("sessionId"))
        .or_else(|| event.properties.get("id"))
        .and_then(|v| v.as_str());
    if event_session != Some(session_id) {
        return false;
    }
    let event_run = event
        .properties
        .get("runID")
        .or_else(|| event.properties.get("run_id"))
        .and_then(|v| v.as_str());
    match event_run {
        Some(value) => value == run_id,
        None => true,
    }
}

fn normalize_run_event(mut event: EngineEvent, session_id: &str, run_id: &str) -> EngineEvent {
    if !event.properties.is_object() {
        event.properties = json!({});
    }
    if let Some(props) = event.properties.as_object_mut() {
        if !props.contains_key("sessionID") {
            props.insert("sessionID".to_string(), json!(session_id));
        }
        if !props.contains_key("runID") {
            props.insert("runID".to_string(), json!(run_id));
        }
        if !props.contains_key("agentID") {
            if let Some(agent) = props.get("agent").and_then(|v| v.as_str()) {
                props.insert("agentID".to_string(), json!(agent));
            }
        }
        if !props.contains_key("channel") {
            let channel = infer_event_channel(&event.event_type, props);
            props.insert("channel".to_string(), json!(channel));
        }
    }
    event
}

fn infer_event_channel(event_type: &str, props: &serde_json::Map<String, Value>) -> &'static str {
    if event_type.starts_with("session.") {
        return "system";
    }
    if event_type.starts_with("todo.") || event_type.starts_with("question.") {
        return "system";
    }
    if event_type == "message.part.updated" {
        if let Some(part_type) = props
            .get("part")
            .and_then(|v| v.get("type"))
            .and_then(|v| v.as_str())
        {
            if part_type == "tool-invocation" || part_type == "tool-result" {
                return "tool";
            }
        }
        return "assistant";
    }
    "log"
}

fn dispatch_error_code(message: &str) -> &'static str {
    if is_os_mismatch_error(message) {
        return "OS_MISMATCH";
    }
    if message.contains("invalid_function_parameters")
        || message.contains("array schema missing items")
    {
        "TOOL_SCHEMA_INVALID"
    } else {
        "ENGINE_DISPATCH_FAILED"
    }
}

fn is_os_mismatch_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("os error 3")
        || lower.contains("system cannot find the path specified")
        || lower.contains("cannot find path")
        || lower.contains("is not recognized as an internal or external command")
        || lower.contains("no such file or directory")
        || lower.contains("command not found")
}

fn truncate_text(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut out = input[..max_len].to_string();
    out.push_str("...<truncated>");
    out
}

async fn append_message_only(
    state: &AppState,
    session_id: &str,
    req: SendMessageRequest,
) -> Result<WireSessionMessage, String> {
    if state.storage.get_session(session_id).await.is_none() {
        return Err("session not found".to_string());
    }
    let text = req
        .parts
        .iter()
        .map(|p| match p {
            MessagePartInput::Text { text } => text.clone(),
            MessagePartInput::File {
                mime,
                filename,
                url,
            } => format!(
                "[file mime={} name={} url={}]",
                mime,
                filename.clone().unwrap_or_else(|| "unknown".to_string()),
                url
            ),
        })
        .collect::<Vec<_>>()
        .join("\n");
    let msg = Message::new(
        MessageRole::User,
        vec![MessagePart::Text { text: text.clone() }],
    );
    let wire = WireSessionMessage::from_message(&msg, session_id);
    state
        .storage
        .append_message(session_id, msg)
        .await
        .map_err(|e| format!("{e:#}"))?;

    // Auto-update title for new sessions (or existing ones stuck as "New session")
    if let Some(mut session) = state.storage.get_session(session_id).await {
        if tandem_core::title_needs_repair(&session.title) {
            // Prefer the earliest user-authored text in history.
            let first_user_text = session.messages.iter().find_map(|message| {
                if !matches!(message.role, MessageRole::User) {
                    return None;
                }
                message.parts.iter().find_map(|part| match part {
                    MessagePart::Text { text } if !text.trim().is_empty() => Some(text.clone()),
                    _ => None,
                })
            });

            // Fallback to the current appended text when history probing fails.
            let title_source = first_user_text.unwrap_or_else(|| text.clone());
            if let Some(new_title) =
                tandem_core::derive_session_title_from_prompt(&title_source, 60)
            {
                session.title = new_title;
                session.time.updated = chrono::Utc::now();
                // Ignore errors here as it's a nice-to-have update
                let _ = state.storage.save_session(session).await;
            }
        }
    }

    Ok(wire)
}

async fn session_todos(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if state.storage.get_session(&id).await.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    let todos = state
        .storage
        .get_todos(&id)
        .await
        .into_iter()
        .filter_map(|v| serde_json::from_value::<TodoItem>(v).ok())
        .collect::<Vec<_>>();
    Ok(Json(json!(todos)))
}
async fn list_projects(State(state): State<AppState>) -> Json<Value> {
    let sessions = state.storage.list_sessions().await;
    let mut directories = sessions
        .iter()
        .map(|s| s.directory.clone())
        .collect::<Vec<_>>();
    directories.sort();
    directories.dedup();
    Json(json!(directories))
}
async fn session_status(State(state): State<AppState>) -> Json<Value> {
    let sessions = state.storage.list_sessions().await;
    let mut map = serde_json::Map::new();
    for s in sessions {
        let mut status = json!({"type":"idle"});
        if let Some(meta) = state.storage.session_status(&s.id).await {
            status["meta"] = meta;
        }
        map.insert(s.id, status);
    }
    Json(Value::Object(map))
}
async fn update_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateSessionInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    if let Some(title) = input.title {
        session.title = title;
    }
    session.time.updated = chrono::Utc::now();
    state
        .storage
        .save_session(session.clone())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if let Some(archived) = input.archived {
        state
            .storage
            .set_archived(&id, archived)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(Json(json!(session)))
}
async fn post_session_message_append(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(req): Json<SendMessageRequest>,
) -> Result<Response, (StatusCode, String)> {
    let wire = append_message_only(&state, &id, req)
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err))?;
    Ok(Json(wire).into_response())
}

async fn get_active_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    if state.storage.get_session(&id).await.is_none() {
        return Err(StatusCode::NOT_FOUND);
    }
    let active = state.run_registry.get(&id).await;
    match active {
        Some(run) => Ok(Json(json!({ "active": run }))),
        None => Ok(Json(json!({ "active": Value::Null }))),
    }
}

async fn abort_session(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    let cancelled = state.cancellations.cancel(&id).await;
    let cancelled_run = state.run_registry.finish_active(&id).await;
    if let Some(run) = cancelled_run.as_ref() {
        state.event_bus.publish(EngineEvent::new(
            "session.run.finished",
            json!({
                "sessionID": id,
                "runID": run.run_id,
                "finishedAtMs": crate::now_ms(),
                "status": "cancelled",
            }),
        ));
    }
    Json(json!({
        "ok": true,
        "cancelled": cancelled || cancelled_run.is_some()
    }))
}

async fn cancel_run_by_id(
    State(state): State<AppState>,
    Path((id, run_id)): Path<(String, String)>,
) -> Json<Value> {
    let active = state.run_registry.get(&id).await;
    if let Some(active_run) = active {
        if active_run.run_id == run_id {
            let _cancelled = state.cancellations.cancel(&id).await;
            let _ = state.run_registry.finish_if_match(&id, &run_id).await;
            state.event_bus.publish(EngineEvent::new(
                "session.run.finished",
                json!({
                    "sessionID": id,
                    "runID": run_id,
                    "finishedAtMs": crate::now_ms(),
                    "status": "cancelled",
                }),
            ));
            return Json(json!({"ok": true, "cancelled": true}));
        }
    }
    Json(json!({"ok": true, "cancelled": false}))
}
async fn fork_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let child = state
        .storage
        .fork_session(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!({"ok": true, "session": child})))
}
async fn revert_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let ok = state
        .storage
        .revert_session(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": ok})))
}
async fn unrevert_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let ok = state
        .storage
        .unrevert_session(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": ok})))
}
async fn share_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let share_id = state
        .storage
        .set_shared(&id, true)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": share_id.is_some(), "shareID": share_id})))
}
async fn unshare_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let _ = state
        .storage
        .set_shared(&id, false)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": true})))
}
async fn summarize_session(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let total_messages = session.messages.len();
    let mut text_parts = Vec::new();
    for message in session.messages.iter().rev().take(4) {
        for part in &message.parts {
            if let MessagePart::Text { text } = part {
                text_parts.push(text.clone());
            }
        }
    }
    text_parts.reverse();
    let excerpt = text_parts.join(" ");
    let clipped = excerpt.chars().take(280).collect::<String>();
    let summary = if clipped.is_empty() {
        format!("Session with {total_messages} messages and no text parts.")
    } else {
        format!("Session with {total_messages} messages. Recent: {clipped}")
    };
    state
        .storage
        .set_summary(&id, summary.clone())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": true, "summary": summary})))
}
async fn session_diff(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let diff = state.storage.session_diff(&id).await;
    Ok(Json(json!(diff.unwrap_or_else(|| json!({})))))
}
async fn session_children(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    Json(json!(state.storage.children(&id).await))
}
async fn init_session() -> Json<Value> {
    Json(json!({"ok": true}))
}

async fn list_permissions(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "requests": state.permissions.list().await,
        "rules": state.permissions.list_rules().await
    }))
}

async fn reply_permission(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<PermissionReplyInput>,
) -> Json<Value> {
    let accepted = matches!(
        input.reply.as_str(),
        "once" | "always" | "reject" | "allow" | "deny"
    );
    if !accepted {
        return Json(json!({
            "ok": false,
            "error":"reply must be one of once|always|reject|allow|deny",
            "code":"invalid_permission_reply"
        }));
    }
    let ok = state.permissions.reply(&id, &input.reply).await;
    Json(json!({"ok": ok}))
}

async fn approve_tool_by_call(
    State(state): State<AppState>,
    Path((_session_id, tool_call_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let ok = state.permissions.reply(&tool_call_id, "allow").await;
    if !ok {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorEnvelope {
                error: "Permission request not found".to_string(),
                code: Some("permission_request_not_found".to_string()),
            }),
        ));
    }
    Ok(Json(json!({"ok": true})))
}

async fn deny_tool_by_call(
    State(state): State<AppState>,
    Path((_session_id, tool_call_id)): Path<(String, String)>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let ok = state.permissions.reply(&tool_call_id, "deny").await;
    if !ok {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorEnvelope {
                error: "Permission request not found".to_string(),
                code: Some("permission_request_not_found".to_string()),
            }),
        ));
    }
    Ok(Json(json!({"ok": true})))
}

async fn list_questions(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.storage.list_question_requests().await))
}
async fn reply_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(_input): Json<QuestionReplyInput>,
) -> Result<Json<Value>, StatusCode> {
    let ok = state
        .storage
        .reply_question(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if ok {
        state.event_bus.publish(EngineEvent::new(
            "question.replied",
            json!({"id": id, "ok": true}),
        ));
    }
    Ok(Json(json!({"ok": ok})))
}
async fn reject_question(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let ok = state
        .storage
        .reject_question(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if ok {
        state.event_bus.publish(EngineEvent::new(
            "question.replied",
            json!({"id": id, "ok": false}),
        ));
    }
    Ok(Json(json!({"ok": ok})))
}

async fn answer_question(
    State(state): State<AppState>,
    Path((_session_id, question_id)): Path<(String, String)>,
    Json(input): Json<QuestionAnswerInput>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let ok = state
        .storage
        .reply_question(&question_id)
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorEnvelope {
                    error: "Failed to answer question".to_string(),
                    code: Some("question_answer_failed".to_string()),
                }),
            )
        })?;
    if !ok {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorEnvelope {
                error: "Question request not found".to_string(),
                code: Some("question_not_found".to_string()),
            }),
        ));
    }
    if ok {
        state.event_bus.publish(EngineEvent::new(
            "question.replied",
            json!({"id": question_id, "ok": true, "answer": input.answer}),
        ));
    }
    Ok(Json(json!({"ok": true})))
}
async fn list_providers(State(state): State<AppState>) -> Json<Value> {
    let cfg = state.config.get().await;
    let default = cfg.default_provider.unwrap_or_else(|| "local".to_string());
    let connected = state
        .providers
        .list()
        .await
        .into_iter()
        .map(|p| p.id)
        .collect::<Vec<_>>();
    let all = state.providers.list().await;
    let mut wire = WireProviderCatalog::from_providers(all, connected);
    let effective_cfg = state.config.get_effective_value().await;

    merge_known_provider_defaults(&mut wire);
    merge_provider_models_from_config(&mut wire, &effective_cfg);
    if let Some(openrouter_models) = fetch_openrouter_models(&effective_cfg).await {
        merge_provider_model_map(
            &mut wire,
            "openrouter",
            Some("OpenRouter"),
            openrouter_models,
        );
    }

    Json(json!({
        "all": wire.all,
        "connected": wire.connected,
        "default": default
    }))
}

fn merge_known_provider_defaults(wire: &mut WireProviderCatalog) {
    let known = [
        ("openrouter", "OpenRouter", "openai/gpt-4o-mini"),
        ("openai", "OpenAI", "gpt-4o-mini"),
        ("anthropic", "Anthropic", "claude-3-5-sonnet-latest"),
        ("ollama", "Ollama", "llama3.1:8b"),
        ("groq", "Groq", "llama-3.1-8b-instant"),
        ("mistral", "Mistral", "mistral-small-latest"),
        (
            "together",
            "Together",
            "meta-llama/Llama-3.1-8B-Instruct-Turbo",
        ),
        ("cohere", "Cohere", "command-r-plus"),
        ("azure", "Azure OpenAI-Compatible", "gpt-4o-mini"),
        (
            "bedrock",
            "Bedrock-Compatible",
            "anthropic.claude-3-5-sonnet-20240620-v1:0",
        ),
        ("vertex", "Vertex-Compatible", "gemini-1.5-flash"),
        ("copilot", "GitHub Copilot-Compatible", "gpt-4o-mini"),
    ];

    for (provider_id, provider_name, default_model) in known {
        let mut models = HashMap::new();
        models.insert(
            default_model.to_string(),
            WireProviderModel {
                name: Some(default_model.to_string()),
                limit: None,
            },
        );
        merge_provider_model_map(wire, provider_id, Some(provider_name), models);
    }
}

fn ensure_provider_entry<'a>(
    wire: &'a mut WireProviderCatalog,
    provider_id: &str,
    provider_name: Option<&str>,
) -> &'a mut WireProviderEntry {
    if let Some(idx) = wire.all.iter().position(|entry| entry.id == provider_id) {
        return &mut wire.all[idx];
    }

    wire.all.push(WireProviderEntry {
        id: provider_id.to_string(),
        name: provider_name.map(|s| s.to_string()),
        models: HashMap::new(),
    });
    wire.all.last_mut().expect("provider entry just inserted")
}

fn merge_provider_model_map(
    wire: &mut WireProviderCatalog,
    provider_id: &str,
    provider_name: Option<&str>,
    models: HashMap<String, WireProviderModel>,
) {
    let entry = ensure_provider_entry(wire, provider_id, provider_name);
    for (model_id, model) in models {
        entry.models.insert(model_id, model);
    }
}

fn merge_provider_models_from_config(wire: &mut WireProviderCatalog, cfg: &Value) {
    let Some(provider_root) = cfg.get("provider").and_then(|v| v.as_object()) else {
        return;
    };

    for (provider_id, provider_value) in provider_root {
        let provider_name = provider_value
            .get("name")
            .and_then(|v| v.as_str())
            .or(Some(provider_id.as_str()));

        let mut model_map: HashMap<String, WireProviderModel> = HashMap::new();
        if let Some(models_obj) = provider_value.get("models").and_then(|v| v.as_object()) {
            for (model_id, model_value) in models_obj {
                let display_name = model_value
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| Some(model_id.to_string()));
                let context = model_value
                    .get("limit")
                    .and_then(|v| v.get("context"))
                    .and_then(|v| v.as_u64())
                    .or_else(|| model_value.get("context_length").and_then(|v| v.as_u64()))
                    .map(|v| v as u32);

                model_map.insert(
                    model_id.to_string(),
                    WireProviderModel {
                        name: display_name,
                        limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
                    },
                );
            }
        }

        if !model_map.is_empty() {
            merge_provider_model_map(wire, provider_id, provider_name, model_map);
        }
    }
}

async fn fetch_openrouter_models(cfg: &Value) -> Option<HashMap<String, WireProviderModel>> {
    let api_key = cfg
        .get("provider")
        .and_then(|v| v.get("openrouter"))
        .and_then(|v| v.get("api_key"))
        .and_then(|v| v.as_str())
        .filter(|k| !k.trim().is_empty() && *k != "x")
        .map(|k| k.to_string())
        .or_else(|| {
            cfg.get("providers")
                .and_then(|v| v.get("openrouter"))
                .and_then(|v| v.get("api_key"))
                .and_then(|v| v.as_str())
                .filter(|k| !k.trim().is_empty() && *k != "x")
                .map(|k| k.to_string())
        })
        .or_else(|| std::env::var("OPENCODE_OPENROUTER_API_KEY").ok())
        .filter(|k| !k.trim().is_empty())
        .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
        .filter(|k| !k.trim().is_empty());

    let client = reqwest::Client::new();
    let mut req = client
        .get("https://openrouter.ai/api/v1/models")
        .timeout(Duration::from_secs(20));
    if let Some(api_key) = api_key {
        req = req.bearer_auth(api_key);
    }
    let resp = match req.send().await {
        Ok(resp) => resp,
        Err(err) => {
            tracing::debug!("Failed to fetch OpenRouter models: {}", err);
            return None;
        }
    };

    if !resp.status().is_success() {
        tracing::debug!("OpenRouter models request returned {}", resp.status());
        return None;
    }

    let body: Value = match resp.json().await {
        Ok(v) => v,
        Err(err) => {
            tracing::debug!("Failed to decode OpenRouter models: {}", err);
            return None;
        }
    };

    let data = body.get("data").and_then(|v| v.as_array())?;

    let mut out = HashMap::new();
    for item in data {
        let Some(model_id) = item.get("id").and_then(|v| v.as_str()) else {
            continue;
        };
        let name = item
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .or_else(|| Some(model_id.to_string()));
        let context = item
            .get("context_length")
            .and_then(|v| v.as_u64())
            .or_else(|| {
                item.get("top_provider")
                    .and_then(|v| v.get("context_length"))
                    .and_then(|v| v.as_u64())
            })
            .map(|v| v as u32);

        out.insert(
            model_id.to_string(),
            WireProviderModel {
                name,
                limit: context.map(|ctx| WireProviderModelLimit { context: Some(ctx) }),
            },
        );
    }

    if out.is_empty() {
        None
    } else {
        Some(out)
    }
}
async fn list_providers_legacy(State(state): State<AppState>) -> Json<Vec<LegacyProviderInfo>> {
    let connected_ids = state
        .providers
        .list()
        .await
        .into_iter()
        .map(|p| p.id)
        .collect::<std::collections::HashSet<_>>();
    let providers = state
        .providers
        .list()
        .await
        .into_iter()
        .map(|p| LegacyProviderInfo {
            id: p.id.clone(),
            name: p.name,
            models: p.models.into_iter().map(|m| m.id).collect(),
            configured: connected_ids.contains(&p.id),
        })
        .collect::<Vec<_>>();
    Json(providers)
}
async fn provider_auth() -> Json<Value> {
    Json(json!({}))
}
async fn provider_oauth_authorize() -> Json<Value> {
    Json(json!({"authorizationUrl": null}))
}
async fn provider_oauth_callback() -> Json<Value> {
    Json(json!({"ok": true}))
}

fn redact_secret_fields(value: &mut Value) {
    match value {
        Value::Object(map) => {
            for (key, field) in map.iter_mut() {
                if key.eq_ignore_ascii_case("api_key")
                    || key.eq_ignore_ascii_case("apikey")
                    || key.eq_ignore_ascii_case("bot_token")
                    || key.eq_ignore_ascii_case("botToken")
                {
                    *field = Value::String("[REDACTED]".to_string());
                } else {
                    redact_secret_fields(field);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                redact_secret_fields(item);
            }
        }
        _ => {}
    }
}

fn redacted(mut value: Value) -> Value {
    redact_secret_fields(&mut value);
    value
}

fn contains_secret_config_fields(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().any(|(key, field)| {
            key.eq_ignore_ascii_case("api_key")
                || key.eq_ignore_ascii_case("apikey")
                || key.eq_ignore_ascii_case("bot_token")
                || key.eq_ignore_ascii_case("botToken")
                || contains_secret_config_fields(field)
        }),
        Value::Array(items) => items.iter().any(contains_secret_config_fields),
        _ => false,
    }
}

async fn get_config(State(state): State<AppState>) -> Json<Value> {
    let effective = redacted(state.config.get_effective_value().await);
    let layers = redacted(state.config.get_layers_value().await);
    Json(json!({
        "effective": effective,
        "layers": layers
    }))
}
async fn patch_config(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    if contains_secret_config_fields(&input) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
            "error": "Secret provider keys are not accepted in config patches.",
            "code": "CONFIG_SECRET_REJECTED",
            "hint": "Use PUT /auth/{provider} or environment variables."
            })),
        )
            .into_response();
    }
    let effective = match state.config.patch_project(input).await {
        Ok(effective) => effective,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    Json(json!({ "effective": redacted(effective) })).into_response()
}
async fn global_config(State(state): State<AppState>) -> Json<Value> {
    let global = redacted(state.config.get_global_value().await);
    let effective = redacted(state.config.get_effective_value().await);
    Json(json!({
        "global": global,
        "effective": effective
    }))
}
async fn global_config_patch(State(state): State<AppState>, Json(input): Json<Value>) -> Response {
    if contains_secret_config_fields(&input) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
            "error": "Secret provider keys are not accepted in global config patches.",
            "code": "CONFIG_SECRET_REJECTED",
            "hint": "Use PUT /auth/{provider} or environment variables."
            })),
        )
            .into_response();
    }
    let effective = match state.config.patch_global(input).await {
        Ok(effective) => effective,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    Json(json!({ "effective": redacted(effective) })).into_response()
}
async fn config_providers(State(state): State<AppState>) -> Json<Value> {
    let cfg = state.config.get_effective_value().await;
    let providers = redacted(cfg.get("providers").cloned().unwrap_or_else(|| json!({})));
    let default_provider = cfg.get("default_provider").cloned().unwrap_or(Value::Null);
    Json(json!({
        "providers": providers,
        "default": default_provider
    }))
}
async fn global_dispose(State(state): State<AppState>) -> Json<Value> {
    let cancelled = state.cancellations.cancel_all().await;
    Json(json!({"ok": true, "cancelledSessions": cancelled}))
}

async fn list_mcp(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.mcp.list().await))
}
async fn add_mcp(
    State(state): State<AppState>,
    Json(input): Json<HashMap<String, String>>,
) -> Json<Value> {
    let name = input
        .get("name")
        .cloned()
        .unwrap_or_else(|| "default".to_string());
    let transport = input
        .get("transport")
        .cloned()
        .unwrap_or_else(|| "stdio".to_string());
    state.mcp.add(name, transport).await;
    Json(json!({"ok": true}))
}
async fn connect_mcp(State(state): State<AppState>, Path(name): Path<String>) -> Json<Value> {
    Json(json!({"ok": state.mcp.connect(&name).await}))
}
async fn disconnect_mcp(State(state): State<AppState>, Path(name): Path<String>) -> Json<Value> {
    Json(json!({"ok": state.mcp.disconnect(&name).await}))
}
async fn auth_mcp(Path(name): Path<String>) -> Json<Value> {
    Json(json!({"authorizationUrl": format!("https://example.invalid/mcp/{name}/authorize")}))
}
async fn callback_mcp(Path(name): Path<String>) -> Json<Value> {
    Json(json!({"ok": true, "name": name}))
}
async fn authenticate_mcp(Path(name): Path<String>) -> Json<Value> {
    Json(json!({"ok": true, "name": name, "authenticated": true}))
}
async fn delete_auth_mcp(Path(name): Path<String>) -> Json<Value> {
    Json(json!({"ok": true, "name": name}))
}
async fn mcp_resources(State(state): State<AppState>) -> Json<Value> {
    let resources = state
        .mcp
        .list()
        .await
        .into_values()
        .filter(|server| server.connected)
        .map(|server| {
            json!({
                "server": server.name,
                "resources": [
                    {"uri": format!("mcp://{}/tools", server.name), "name":"tools"},
                    {"uri": format!("mcp://{}/prompts", server.name), "name":"prompts"}
                ]
            })
        })
        .collect::<Vec<_>>();
    Json(json!(resources))
}

async fn tool_ids(State(state): State<AppState>) -> Json<Value> {
    let ids = state
        .tools
        .list()
        .await
        .into_iter()
        .map(|t| t.name)
        .collect::<Vec<_>>();
    Json(json!(ids))
}
async fn tool_list_for_model(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.tools.list().await))
}
async fn create_worktree(Json(input): Json<WorktreeInput>) -> Result<Json<Value>, StatusCode> {
    let path = input.path.unwrap_or_else(|| "worktree-temp".to_string());
    let branch = input
        .branch
        .unwrap_or_else(|| format!("wt-{}", chrono::Utc::now().timestamp()));
    let base = input.base.unwrap_or_else(|| "HEAD".to_string());
    let output = Command::new("git")
        .args(["worktree", "add", "-b", &branch, &path, &base])
        .output()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "ok": output.status.success(),
        "path": path,
        "branch": branch,
        "stderr": String::from_utf8_lossy(&output.stderr).to_string()
    })))
}
async fn list_worktrees() -> Json<Value> {
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
        .await
        .ok();
    let raw = output
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let mut worktrees = Vec::new();
    let mut current = serde_json::Map::new();
    for line in raw.lines() {
        if line.is_empty() {
            if !current.is_empty() {
                worktrees.push(Value::Object(current.clone()));
                current.clear();
            }
            continue;
        }
        let mut parts = line.splitn(2, ' ');
        let key = parts.next().unwrap_or_default();
        let value = parts.next().unwrap_or_default();
        current.insert(key.to_string(), Value::String(value.to_string()));
    }
    if !current.is_empty() {
        worktrees.push(Value::Object(current));
    }
    Json(json!(worktrees))
}
async fn delete_worktree(Json(input): Json<WorktreeInput>) -> Result<Json<Value>, StatusCode> {
    let Some(path) = input.path else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let output = Command::new("git")
        .args(["worktree", "remove", "--force", &path])
        .output()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "ok": output.status.success(),
        "path": path,
        "stderr": String::from_utf8_lossy(&output.stderr).to_string()
    })))
}
async fn reset_worktree(Json(input): Json<WorktreeInput>) -> Result<Json<Value>, StatusCode> {
    let Some(path) = input.path else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let target = input.base.unwrap_or_else(|| "HEAD".to_string());
    let output = Command::new("git")
        .args(["-C", &path, "reset", "--hard", &target])
        .output()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "ok": output.status.success(),
        "path": path,
        "target": target,
        "stderr": String::from_utf8_lossy(&output.stderr).to_string()
    })))
}
async fn find_text(Query(query): Query<FindTextQuery>) -> Result<Json<Value>, StatusCode> {
    let root = query.path.unwrap_or_else(|| ".".to_string());
    let regex = Regex::new(&query.pattern).map_err(|_| StatusCode::BAD_REQUEST)?;
    let mut matches = Vec::new();
    let limit = query.limit.unwrap_or(100).max(1);

    for entry in WalkBuilder::new(root).build().flatten() {
        if !entry.file_type().map(|f| f.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        if let Ok(content) = std::fs::read_to_string(path) {
            for (index, line) in content.lines().enumerate() {
                if regex.is_match(line) {
                    matches.push(json!({
                        "path": path.display().to_string(),
                        "line": index + 1,
                        "text": line
                    }));
                    if matches.len() >= limit {
                        return Ok(Json(json!(matches)));
                    }
                }
            }
        }
    }
    Ok(Json(json!(matches)))
}
async fn find_file(Query(query): Query<FindFileQuery>) -> Json<Value> {
    let root = query.path.unwrap_or_else(|| ".".to_string());
    let needle = query.q.to_lowercase();
    let mut files = Vec::new();
    let limit = query.limit.unwrap_or(100).max(1);
    for entry in WalkBuilder::new(root).build().flatten() {
        if !entry.file_type().map(|f| f.is_file()).unwrap_or(false) {
            continue;
        }
        let path = entry.path();
        let name = path.file_name().and_then(|v| v.to_str()).unwrap_or("");
        if name.to_lowercase().contains(&needle) {
            files.push(path.display().to_string());
            if files.len() >= limit {
                break;
            }
        }
    }
    Json(json!(files))
}
async fn find_symbol(Query(query): Query<FindTextQuery>) -> Result<Json<Value>, StatusCode> {
    find_text(Query(query)).await
}
async fn file_list(Query(query): Query<FileListQuery>) -> Json<Value> {
    let root = query.path.unwrap_or_else(|| ".".to_string());
    let mut files = Vec::new();
    let limit = query.limit.unwrap_or(200).max(1);
    for entry in WalkBuilder::new(root).build().flatten() {
        if !entry.file_type().map(|f| f.is_file()).unwrap_or(false) {
            continue;
        }
        files.push(entry.path().display().to_string());
        if files.len() >= limit {
            break;
        }
    }
    Json(json!(files))
}
async fn file_content(Query(query): Query<FileContentQuery>) -> Result<Json<Value>, StatusCode> {
    let path = PathBuf::from(query.path);
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(Json(json!({"content": content})))
}
async fn file_status() -> Json<Value> {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .output()
        .await
        .ok();
    let files = output
        .as_ref()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default()
        .lines()
        .filter_map(|line| {
            if line.len() < 4 {
                return None;
            }
            let status = line[0..2].trim().to_string();
            let path = line[3..].to_string();
            Some(json!({"status": status, "path": path}))
        })
        .collect::<Vec<_>>();
    Json(json!(files))
}
async fn vcs() -> Json<Value> {
    let branch = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".to_string());
    let numstat_raw = Command::new("git")
        .args(["diff", "--numstat"])
        .output()
        .await
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
        .unwrap_or_default();
    let numstat = numstat_raw
        .lines()
        .filter_map(|line| {
            let parts = line.split('\t').collect::<Vec<_>>();
            if parts.len() < 3 {
                return None;
            }
            Some(json!({
                "added": parts[0],
                "removed": parts[1],
                "path": parts[2]
            }))
        })
        .collect::<Vec<_>>();
    Json(json!({"branch": branch, "numstat": numstat}))
}
async fn pty_list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.pty.list().await))
}
async fn pty_create(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let id = state
        .pty
        .create()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": true, "id": id})))
}
async fn pty_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let snapshot = state.pty.snapshot(&id).await.ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(json!(snapshot)))
}
async fn pty_update(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<PtyUpdateInput>,
) -> Result<Json<Value>, StatusCode> {
    if let Some(data) = input.input.as_ref() {
        let ok = state
            .pty
            .write(&id, data)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        return Ok(Json(json!({"ok": ok})));
    }
    Ok(Json(json!({"ok": false, "error":"missing input"})))
}
async fn pty_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let ok = state
        .pty
        .kill(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": ok})))
}
async fn pty_ws(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| pty_ws_stream(socket, state, id))
}

async fn pty_ws_stream(mut socket: WebSocket, state: AppState, id: String) {
    let mut offset = 0usize;
    loop {
        let Some((chunk, next_offset, running)) = state.pty.read_since(&id, offset).await else {
            let _ = socket
                .send(WsMessage::Text("{\"closed\":true}".into()))
                .await;
            break;
        };
        if !chunk.is_empty() {
            let payload =
                json!({"id": id, "chunk": truncate_for_stream(&chunk, 4096), "running": running})
                    .to_string();
            if socket.send(WsMessage::Text(payload.into())).await.is_err() {
                break;
            }
        }
        offset = next_offset;
        if !running {
            let _ = socket
                .send(WsMessage::Text("{\"closed\":true}".into()))
                .await;
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}
async fn lsp_status(
    State(state): State<AppState>,
    Query(query): Query<LspQuery>,
) -> Result<Json<Value>, StatusCode> {
    let action = query.action.as_deref().unwrap_or("status");
    match action {
        "status" => Ok(Json(json!({"ok": true, "backend": "heuristic-lsp"}))),
        "diagnostics" => {
            let path = query.path.ok_or(StatusCode::BAD_REQUEST)?;
            Ok(Json(json!(state.lsp.diagnostics(&path))))
        }
        "definition" => {
            let symbol = query.symbol.ok_or(StatusCode::BAD_REQUEST)?;
            Ok(Json(json!(state.lsp.goto_definition(&symbol))))
        }
        "references" => {
            let symbol = query.symbol.ok_or(StatusCode::BAD_REQUEST)?;
            Ok(Json(json!(state.lsp.references(&symbol))))
        }
        "hover" => {
            let symbol = query.symbol.ok_or(StatusCode::BAD_REQUEST)?;
            Ok(Json(json!({"text": state.lsp.hover(&symbol)})))
        }
        "symbols" => Ok(Json(json!(state.lsp.symbols(query.q.as_deref())))),
        "call_hierarchy" => {
            let symbol = query.symbol.ok_or(StatusCode::BAD_REQUEST)?;
            Ok(Json(state.lsp.call_hierarchy(&symbol)))
        }
        _ => Err(StatusCode::BAD_REQUEST),
    }
}
async fn formatter_status() -> Json<Value> {
    Json(json!([]))
}
async fn command_list() -> Json<Value> {
    Json(json!([
        {"id":"git-status","command":"git","args":["status","--short"]},
        {"id":"git-branch","command":"git","args":["branch","--show-current"]},
        {"id":"cargo-check","command":"cargo","args":["check","-p","tandem-engine"]}
    ]))
}
async fn run_command(Json(input): Json<CommandRunInput>) -> Result<Json<Value>, StatusCode> {
    let command = input.command.ok_or(StatusCode::BAD_REQUEST)?;
    let mut cmd = Command::new(&command);
    cmd.args(input.args);
    if let Some(cwd) = input.cwd {
        cmd.current_dir(cwd);
    }
    let output = cmd
        .output()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "ok": output.status.success(),
        "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
        "stderr": String::from_utf8_lossy(&output.stderr).to_string()
    })))
}
async fn run_shell(Json(input): Json<ShellRunInput>) -> Result<Json<Value>, StatusCode> {
    let command = input.command.ok_or(StatusCode::BAD_REQUEST)?;
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoProfile", "-Command", &command]);
    if let Some(cwd) = input.cwd {
        cmd.current_dir(cwd);
    }
    let output = cmd
        .output()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "ok": output.status.success(),
        "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
        "stderr": String::from_utf8_lossy(&output.stderr).to_string()
    })))
}
async fn set_auth(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AuthInput>,
) -> Json<Value> {
    let token = input.token.unwrap_or_default().trim().to_string();
    if token.is_empty() {
        return Json(json!({"ok": false, "error": "token cannot be empty"}));
    }

    // Keep legacy in-memory auth map for compatibility while runtime config
    // becomes the canonical provider-key source.
    state.auth.write().await.insert(id.clone(), token.clone());

    let patch = json!({
        "providers": {
            id.clone(): {
                "api_key": token
            }
        }
    });
    let ok = state.config.patch_runtime(patch).await.is_ok();
    if ok {
        state
            .providers
            .reload(state.config.get().await.into())
            .await;
    }
    Json(json!({"ok": ok, "id": id}))
}
async fn delete_auth(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    let removed = state.auth.write().await.remove(&id).is_some();
    let runtime_removed = state.config.delete_runtime_provider_key(&id).await.is_ok();
    if runtime_removed {
        state
            .providers
            .reload(state.config.get().await.into())
            .await;
    }
    Json(json!({"ok": removed || runtime_removed}))
}

async fn set_api_token(
    State(state): State<AppState>,
    Json(input): Json<ApiTokenInput>,
) -> Json<Value> {
    let token = input.token.unwrap_or_default().trim().to_string();
    if token.is_empty() {
        return Json(json!({
            "ok": false,
            "error": "token cannot be empty"
        }));
    }
    state.set_api_token(Some(token)).await;
    Json(json!({"ok": true}))
}

async fn clear_api_token(State(state): State<AppState>) -> Json<Value> {
    state.set_api_token(None).await;
    Json(json!({"ok": true}))
}

async fn generate_api_token(State(state): State<AppState>) -> Json<Value> {
    let token = format!("tk_{}", Uuid::new_v4().simple());
    state.set_api_token(Some(token.clone())).await;
    Json(json!({
        "ok": true,
        "token": token
    }))
}
async fn path_info(
    State(state): State<AppState>,
    Query(query): Query<PathInfoQuery>,
) -> Json<Value> {
    let refresh = query.refresh.unwrap_or(false);
    let snapshot = if refresh {
        state.workspace_index.refresh().await
    } else {
        state.workspace_index.snapshot().await
    };
    Json(json!({
        "workspace": snapshot,
        "inProcessMode": state.in_process_mode.load(std::sync::atomic::Ordering::Relaxed)
    }))
}
async fn agent_list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.agents.list().await))
}

fn skills_service() -> SkillService {
    SkillService::for_workspace(std::env::current_dir().ok())
}

fn skill_error(
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

async fn skills_list() -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let service = skills_service();
    let skills = service
        .list_skills()
        .map_err(|e| skill_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!(skills)))
}

async fn skills_get(
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

async fn skills_import_preview(
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

async fn skills_import(
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

async fn skills_delete(
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

async fn skills_templates_list() -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let service = skills_service();
    let templates = service
        .list_templates()
        .map_err(|e| skill_error(StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(json!(templates)))
}

async fn skills_templates_install(
    Path(id): Path<String>,
    Json(input): Json<SkillsTemplateInstallRequest>,
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let service = skills_service();
    let installed = service
        .install_template(&id, input.location)
        .map_err(|e| skill_error(StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!(installed)))
}

async fn skill_list() -> Json<Value> {
    let service = skills_service();
    let skills = service.list_skills().unwrap_or_default();
    Json(json!({
        "skills": skills,
        "deprecation_warning": "GET /skill is deprecated; use GET /skills instead."
    }))
}

fn default_memory_capability_for(
    run_id: &str,
    partition: &tandem_memory::MemoryPartition,
) -> MemoryCapabilityToken {
    MemoryCapabilityToken {
        run_id: run_id.to_string(),
        subject: "default".to_string(),
        org_id: partition.org_id.clone(),
        workspace_id: partition.workspace_id.clone(),
        project_id: partition.project_id.clone(),
        memory: MemoryCapabilities::default(),
        expires_at: u64::MAX,
    }
}

fn validate_memory_capability(
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

fn scrub_content(input: &str) -> ScrubReport {
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

async fn append_memory_audit(
    state: &AppState,
    event: crate::MemoryAuditEvent,
) -> Result<(), StatusCode> {
    let mut audit = state.memory_audit_log.write().await;
    audit.push(event);
    Ok(())
}

async fn memory_put(
    State(state): State<AppState>,
    Json(input): Json<MemoryPutInput>,
) -> Result<Json<MemoryPutResponse>, StatusCode> {
    let request = input.request;
    let capability =
        validate_memory_capability(&request.run_id, &request.partition, input.capability)?;

    if !capability
        .memory
        .write_tiers
        .contains(&request.partition.tier)
    {
        return Err(StatusCode::FORBIDDEN);
    }

    let id = Uuid::new_v4().to_string();
    let partition_key = request.partition.key();
    let now = crate::now_ms();
    let audit_id = Uuid::new_v4().to_string();

    let record = crate::GovernedMemoryRecord {
        id: id.clone(),
        run_id: request.run_id.clone(),
        partition: request.partition.clone(),
        kind: request.kind,
        content: request.content,
        artifact_refs: request.artifact_refs,
        classification: request.classification,
        metadata: request.metadata,
        source_memory_id: None,
        created_at_ms: now,
    };

    {
        let mut records = state.memory_records.write().await;
        records.insert(id.clone(), record);
    }

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
            "tier": request.partition.tier,
            "partitionKey": partition_key,
            "auditID": audit_id,
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "memory.updated",
        json!({
            "memoryID": id,
            "action": "put",
        }),
    ));

    Ok(Json(MemoryPutResponse {
        id,
        stored: true,
        tier: request.partition.tier,
        partition_key,
        audit_id,
    }))
}

async fn memory_promote(
    State(state): State<AppState>,
    Json(input): Json<MemoryPromoteInput>,
) -> Result<Json<MemoryPromoteResponse>, StatusCode> {
    let request = input.request;
    let source_memory_id = request.source_memory_id.clone();
    let capability =
        validate_memory_capability(&request.run_id, &request.partition, input.capability)?;

    if !capability.memory.promote_targets.contains(&request.to_tier) {
        return Err(StatusCode::FORBIDDEN);
    }
    if capability.memory.require_review_for_promote
        && (request.review.approval_id.is_none() || request.review.reviewer_id.is_none())
    {
        return Err(StatusCode::FORBIDDEN);
    }

    let source = {
        let records = state.memory_records.read().await;
        records.get(&request.source_memory_id).cloned()
    }
    .ok_or(StatusCode::NOT_FOUND)?;

    if source.partition.org_id != request.partition.org_id
        || source.partition.workspace_id != request.partition.workspace_id
        || source.partition.project_id != request.partition.project_id
        || source.partition.tier != request.from_tier
    {
        return Err(StatusCode::FORBIDDEN);
    }

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
                partition_key,
                actor: capability.subject,
                status: "blocked".to_string(),
                detail: scrub_report.block_reason.clone(),
                created_at_ms: now,
            },
        )
        .await?;

        return Ok(Json(MemoryPromoteResponse {
            promoted: false,
            new_memory_id: None,
            to_tier: request.to_tier,
            scrub_report,
            audit_id,
        }));
    }

    let new_id = Uuid::new_v4().to_string();
    let promoted_record = crate::GovernedMemoryRecord {
        id: new_id.clone(),
        run_id: request.run_id.clone(),
        partition: tandem_memory::MemoryPartition {
            org_id: request.partition.org_id.clone(),
            workspace_id: request.partition.workspace_id.clone(),
            project_id: request.partition.project_id.clone(),
            tier: request.to_tier,
        },
        kind: source.kind,
        content: source.content,
        artifact_refs: source.artifact_refs,
        classification: source.classification,
        metadata: source.metadata,
        source_memory_id: Some(source.id),
        created_at_ms: now,
    };

    {
        let mut records = state.memory_records.write().await;
        records.insert(new_id.clone(), promoted_record);
    }

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
            "toTier": request.to_tier,
            "auditID": audit_id,
            "scrubStatus": scrub_report.status,
        }),
    ));
    state.event_bus.publish(EngineEvent::new(
        "memory.updated",
        json!({
            "memoryID": new_id,
            "action": "promote",
        }),
    ));

    Ok(Json(MemoryPromoteResponse {
        promoted: true,
        new_memory_id: Some(new_id),
        to_tier: request.to_tier,
        scrub_report,
        audit_id,
    }))
}

async fn memory_search(
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
    for scope in requested_scopes {
        if capability.memory.read_tiers.contains(&scope) {
            scopes_used.push(scope);
        } else {
            blocked_scopes.push(scope);
        }
    }

    let limit = request.limit.unwrap_or(8).clamp(1, 100) as usize;
    let query_lower = request.query.to_lowercase();

    let mut results = Vec::new();
    {
        let records = state.memory_records.read().await;
        for record in records.values() {
            if record.partition.org_id != request.partition.org_id
                || record.partition.workspace_id != request.partition.workspace_id
                || record.partition.project_id != request.partition.project_id
            {
                continue;
            }
            if !scopes_used.contains(&record.partition.tier) {
                continue;
            }
            if !query_lower.is_empty() && !record.content.to_lowercase().contains(&query_lower) {
                continue;
            }
            results.push(json!({
                "id": record.id,
                "tier": record.partition.tier,
                "classification": record.classification,
                "kind": record.kind,
                "source_memory_id": record.source_memory_id,
                "created_at_ms": record.created_at_ms,
                "content": record.content,
                "artifact_refs": record.artifact_refs,
            }));
            if results.len() >= limit {
                break;
            }
        }
    }

    let audit_id = Uuid::new_v4().to_string();
    let now = crate::now_ms();
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
            status: "ok".to_string(),
            detail: None,
            created_at_ms: now,
        },
    )
    .await?;

    state.event_bus.publish(EngineEvent::new(
        "memory.search",
        json!({
            "runID": request.run_id,
            "partitionKey": request.partition.key(),
            "resultCount": results.len(),
            "blockedScopes": blocked_scopes,
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

async fn memory_audit(
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

async fn memory_list(
    State(state): State<AppState>,
    Query(query): Query<MemoryListQuery>,
) -> Json<Value> {
    let q = query.q.unwrap_or_default().to_lowercase();
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let offset = query.offset.unwrap_or(0);
    let mut items = state
        .memory_records
        .read()
        .await
        .values()
        .cloned()
        .collect::<Vec<_>>();
    items.sort_by(|a, b| b.created_at_ms.cmp(&a.created_at_ms));
    if !q.is_empty() {
        items.retain(|row| {
            row.id.to_lowercase().contains(&q)
                || row.run_id.to_lowercase().contains(&q)
                || row.content.to_lowercase().contains(&q)
                || row.partition.key().to_lowercase().contains(&q)
        });
    }
    let total = items.len();
    let page = items
        .into_iter()
        .skip(offset)
        .take(limit)
        .map(|row| {
            json!({
                "id": row.id,
                "run_id": row.run_id,
                "partition": row.partition,
                "kind": row.kind,
                "content": row.content,
                "artifact_refs": row.artifact_refs,
                "classification": row.classification,
                "metadata": row.metadata,
                "source_memory_id": row.source_memory_id,
                "created_at_ms": row.created_at_ms,
            })
        })
        .collect::<Vec<_>>();
    Json(json!({
        "items": page,
        "count": total,
        "offset": offset,
        "limit": limit,
    }))
}

async fn memory_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let deleted = state.memory_records.write().await.remove(&id);
    let Some(record) = deleted else {
        return Err(StatusCode::NOT_FOUND);
    };
    let now = crate::now_ms();
    append_memory_audit(
        &state,
        crate::MemoryAuditEvent {
            audit_id: Uuid::new_v4().to_string(),
            action: "memory_delete".to_string(),
            run_id: record.run_id,
            memory_id: Some(id.clone()),
            source_memory_id: record.source_memory_id,
            to_tier: Some(record.partition.tier),
            partition_key: record.partition.key(),
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
        }),
    ));
    Ok(Json(json!({"ok": true})))
}

fn parse_allowed_users(value: Option<&Value>) -> Vec<String> {
    let mut users = value
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if users.is_empty() {
        users.push("*".to_string());
    }
    users
}

async fn channels_config(State(state): State<AppState>) -> Json<Value> {
    let effective = state.config.get_effective_value().await;
    let channels = effective.get("channels").and_then(Value::as_object);

    let telegram = channels
        .and_then(|obj| obj.get("telegram"))
        .and_then(Value::as_object);
    let discord = channels
        .and_then(|obj| obj.get("discord"))
        .and_then(Value::as_object);
    let slack = channels
        .and_then(|obj| obj.get("slack"))
        .and_then(Value::as_object);

    let telegram_has_token = telegram
        .and_then(|cfg| cfg.get("bot_token"))
        .and_then(Value::as_str)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let discord_has_token = discord
        .and_then(|cfg| cfg.get("bot_token"))
        .and_then(Value::as_str)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let slack_has_token = slack
        .and_then(|cfg| cfg.get("bot_token"))
        .and_then(Value::as_str)
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    Json(json!({
        "telegram": {
            "has_token": telegram_has_token,
            "allowed_users": parse_allowed_users(telegram.and_then(|cfg| cfg.get("allowed_users"))),
            "mention_only": telegram
                .and_then(|cfg| cfg.get("mention_only"))
                .and_then(Value::as_bool)
                .unwrap_or(false),
        },
        "discord": {
            "has_token": discord_has_token,
            "allowed_users": parse_allowed_users(discord.and_then(|cfg| cfg.get("allowed_users"))),
            "mention_only": discord
                .and_then(|cfg| cfg.get("mention_only"))
                .and_then(Value::as_bool)
                .unwrap_or(true),
            "guild_id": discord
                .and_then(|cfg| cfg.get("guild_id"))
                .and_then(Value::as_str),
        },
        "slack": {
            "has_token": slack_has_token,
            "allowed_users": parse_allowed_users(slack.and_then(|cfg| cfg.get("allowed_users"))),
            "channel_id": slack
                .and_then(|cfg| cfg.get("channel_id"))
                .and_then(Value::as_str),
        }
    }))
}

async fn channels_status(State(state): State<AppState>) -> Json<Value> {
    let status = state.channel_statuses().await;
    Json(json!({
        "telegram": status.get("telegram").cloned().unwrap_or_else(|| ChannelStatus {
            enabled: false,
            connected: false,
            last_error: None,
            active_sessions: 0,
            meta: json!({}),
        }),
        "discord": status.get("discord").cloned().unwrap_or_else(|| ChannelStatus {
            enabled: false,
            connected: false,
            last_error: None,
            active_sessions: 0,
            meta: json!({}),
        }),
        "slack": status.get("slack").cloned().unwrap_or_else(|| ChannelStatus {
            enabled: false,
            connected: false,
            last_error: None,
            active_sessions: 0,
            meta: json!({}),
        }),
    }))
}

async fn channels_put(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let normalized = name.to_ascii_lowercase();
    let mut project = state.config.get_project_value().await;
    let Some(root) = project.as_object_mut() else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let channels = root
        .entry("channels".to_string())
        .or_insert_with(|| json!({}));
    let Some(channels_obj) = channels.as_object_mut() else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    match normalized.as_str() {
        "telegram" => {
            let cfg: TelegramConfigFile =
                serde_json::from_value(input).map_err(|_| StatusCode::BAD_REQUEST)?;
            if cfg.bot_token.trim().is_empty() {
                return Err(StatusCode::BAD_REQUEST);
            }
            channels_obj.insert("telegram".to_string(), json!(cfg));
        }
        "discord" => {
            let cfg: DiscordConfigFile =
                serde_json::from_value(input).map_err(|_| StatusCode::BAD_REQUEST)?;
            if cfg.bot_token.trim().is_empty() {
                return Err(StatusCode::BAD_REQUEST);
            }
            channels_obj.insert("discord".to_string(), json!(cfg));
        }
        "slack" => {
            let cfg: SlackConfigFile =
                serde_json::from_value(input).map_err(|_| StatusCode::BAD_REQUEST)?;
            if cfg.bot_token.trim().is_empty() || cfg.channel_id.trim().is_empty() {
                return Err(StatusCode::BAD_REQUEST);
            }
            channels_obj.insert("slack".to_string(), json!(cfg));
        }
        _ => return Err(StatusCode::NOT_FOUND),
    }
    state
        .config
        .replace_project_value(project)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    state
        .restart_channel_listeners()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": true})))
}

async fn channels_delete(
    State(state): State<AppState>,
    Path(name): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let mut project = state.config.get_project_value().await;
    let Some(root) = project.as_object_mut() else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    let channels = root
        .entry("channels".to_string())
        .or_insert_with(|| json!({}));
    let Some(channels_obj) = channels.as_object_mut() else {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    };
    channels_obj.remove(&name.to_ascii_lowercase());
    state
        .config
        .replace_project_value(project)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    state
        .restart_channel_listeners()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": true})))
}

async fn admin_reload_config(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    state
        .restart_channel_listeners()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({"ok": true})))
}

fn mission_event_id(event: &MissionEvent) -> &str {
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

async fn mission_create(
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

async fn mission_list(State(state): State<AppState>) -> Json<Value> {
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

async fn mission_get(
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

async fn mission_apply_event(
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
        emit_spawn_requested(&state, &req);
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

async fn agent_team_templates(State(state): State<AppState>) -> Json<Value> {
    let templates = state.agent_teams.list_templates().await;
    Json(json!({
        "templates": templates,
        "count": templates.len(),
    }))
}

async fn agent_team_instances(
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

async fn agent_team_missions(State(state): State<AppState>) -> Json<Value> {
    let missions = state.agent_teams.list_mission_summaries().await;
    Json(json!({
        "missions": missions,
        "count": missions.len(),
    }))
}

async fn agent_team_approvals(State(state): State<AppState>) -> Json<Value> {
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

async fn agent_team_spawn(
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
    })))
}

async fn agent_team_approve_spawn(
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
    })))
}

async fn agent_team_deny_spawn(
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

async fn agent_team_cancel_instance(
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

async fn agent_team_cancel_mission(
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
                    "runCount": run_count,
                    "triggerType": trigger_type,
                    "firedAtMs": now,
                }),
            ));
            Ok(Json(json!({
                "ok": true,
                "status": "queued",
                "routineID": id,
                "runCount": run_count,
                "firedAtMs": now,
            })))
        }
        RoutineExecutionDecision::RequiresApproval { reason } => {
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
                    "runCount": run_count,
                    "triggerType": trigger_type,
                    "reason": reason,
                }),
            ));
            Ok(Json(json!({
                "ok": true,
                "status": "pending_approval",
                "routineID": id,
                "runCount": run_count,
            })))
        }
        RoutineExecutionDecision::Blocked { reason } => {
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
                    "runCount": run_count,
                    "triggerType": trigger_type,
                    "reason": reason,
                }),
            ));
            Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Routine blocked by policy",
                    "code": "ROUTINE_POLICY_BLOCKED",
                    "routineID": id,
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

fn resource_error_response(error: ResourceStoreError) -> (StatusCode, Json<Value>) {
    match error {
        ResourceStoreError::InvalidKey { key } => (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid resource key namespace",
                "code": "INVALID_RESOURCE_KEY",
                "key": key,
            })),
        ),
        ResourceStoreError::RevisionConflict(conflict) => (
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Resource revision conflict",
                "code": "RESOURCE_REVISION_CONFLICT",
                "key": conflict.key,
                "expected_rev": conflict.expected_rev,
                "current_rev": conflict.current_rev,
            })),
        ),
        ResourceStoreError::PersistFailed { message } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Resource persistence failed",
                "code": "RESOURCE_PERSIST_FAILED",
                "detail": message,
            })),
        ),
    }
}

fn normalize_resource_key(raw: String) -> String {
    raw.trim_start_matches('/').trim().to_string()
}

async fn resource_list(
    State(state): State<AppState>,
    Query(query): Query<ResourceListQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(100).clamp(1, 500);
    let rows = state
        .list_shared_resources(query.prefix.as_deref(), limit)
        .await;
    Json(json!({
        "resources": rows,
        "count": rows.len(),
    }))
}

async fn resource_get(
    State(state): State<AppState>,
    Path(key): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let key = normalize_resource_key(key);
    let resource = state.get_shared_resource(&key).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Resource not found",
                "code": "RESOURCE_NOT_FOUND",
                "key": key,
            })),
        )
    })?;

    Ok(Json(json!({
        "resource": resource,
    })))
}

async fn resource_put(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(input): Json<ResourceWriteInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let key = normalize_resource_key(key);
    let updated_by = input.updated_by.unwrap_or_else(|| "system".to_string());
    let record = state
        .put_shared_resource(
            key.clone(),
            input.value,
            input.if_match_rev,
            updated_by.clone(),
            input.ttl_ms,
        )
        .await
        .map_err(resource_error_response)?;

    state.event_bus.publish(EngineEvent::new(
        "resource.updated",
        json!({
            "key": record.key,
            "rev": record.rev,
            "updatedBy": updated_by,
            "updatedAtMs": record.updated_at_ms,
        }),
    ));

    Ok(Json(json!({
        "resource": record
    })))
}

async fn resource_patch(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(input): Json<ResourceWriteInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let key = normalize_resource_key(key);
    let existing = state.get_shared_resource(&key).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Resource not found",
                "code": "RESOURCE_NOT_FOUND",
                "key": key,
            })),
        )
    })?;

    let merged_value = if existing.value.is_object() && input.value.is_object() {
        let mut map = existing.value.as_object().cloned().unwrap_or_default();
        for (k, v) in input.value.as_object().cloned().unwrap_or_default() {
            map.insert(k, v);
        }
        Value::Object(map)
    } else {
        input.value
    };

    let updated_by = input.updated_by.unwrap_or_else(|| "system".to_string());
    let record = state
        .put_shared_resource(
            key.clone(),
            merged_value,
            input.if_match_rev,
            updated_by.clone(),
            input.ttl_ms.or(existing.ttl_ms),
        )
        .await
        .map_err(resource_error_response)?;

    state.event_bus.publish(EngineEvent::new(
        "resource.updated",
        json!({
            "key": record.key,
            "rev": record.rev,
            "updatedBy": updated_by,
            "updatedAtMs": record.updated_at_ms,
        }),
    ));

    Ok(Json(json!({
        "resource": record
    })))
}

async fn resource_delete(
    State(state): State<AppState>,
    Path(key): Path<String>,
    Json(input): Json<ResourceDeleteInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let key = normalize_resource_key(key);
    let updated_by = input.updated_by.unwrap_or_else(|| "system".to_string());
    let deleted = state
        .delete_shared_resource(&key, input.if_match_rev)
        .await
        .map_err(resource_error_response)?;

    if let Some(record) = deleted {
        state.event_bus.publish(EngineEvent::new(
            "resource.deleted",
            json!({
                "key": record.key,
                "rev": record.rev,
                "updatedBy": updated_by,
                "updatedAtMs": crate::now_ms(),
            }),
        ));
        Ok(Json(json!({
            "deleted": true,
            "key": key,
        })))
    } else {
        Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "Resource not found",
                "code": "RESOURCE_NOT_FOUND",
                "key": key,
            })),
        ))
    }
}

fn resource_sse_stream(
    state: AppState,
    prefix: Option<String>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "resource",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            if event.event_type != "resource.updated" && event.event_type != "resource.deleted" {
                return None;
            }
            if let Some(prefix) = prefix.as_deref() {
                let key = event
                    .properties
                    .get("key")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if !key.starts_with(prefix) {
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

async fn resource_events(
    State(state): State<AppState>,
    Query(query): Query<ResourceEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(resource_sse_stream(state, query.prefix))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}

async fn instance_dispose() -> Json<Value> {
    Json(json!({"ok": true}))
}
async fn push_log(State(state): State<AppState>, Json(input): Json<LogInput>) -> Json<Value> {
    let entry = json!({
        "ts": chrono::Utc::now(),
        "level": input.level.unwrap_or_else(|| "info".to_string()),
        "message": input.message.unwrap_or_default(),
        "context": input.context
    });
    state.logs.write().await.push(entry);
    Json(json!({"ok": true}))
}
async fn openapi_doc() -> Json<Value> {
    Json(json!({
        "openapi":"3.1.0",
        "info":{"title":"tandem-engine","version":"0.1.0"},
        "paths":{
            "/global/health":{"get":{"summary":"Health check"}},
            "/global/storage/repair":{"post":{"summary":"Force legacy storage repair scan"}},
            "/session":{"get":{"summary":"List sessions"},"post":{"summary":"Create session"}},
            "/session/{id}/message":{"post":{"summary":"Append message"}},
            "/session/{id}/prompt_async":{"post":{"summary":"Start async prompt run"}},
            "/session/{id}/prompt_sync":{"post":{"summary":"Start sync prompt run"}},
            "/session/{id}/run":{"get":{"summary":"Get active run"}},
            "/session/{id}/cancel":{"post":{"summary":"Cancel active run"}},
            "/session/{id}/run/{run_id}/cancel":{"post":{"summary":"Cancel run by id"}},
            "/event":{"get":{"summary":"SSE event stream"}},
            "/provider":{"get":{"summary":"List providers"}},
            "/session/{id}/fork":{"post":{"summary":"Fork a session"}},
            "/worktree":{"get":{"summary":"List worktrees"},"post":{"summary":"Create worktree"},"delete":{"summary":"Delete worktree"}},
            "/mcp/resources":{"get":{"summary":"List MCP resources"}},
            "/tool":{"get":{"summary":"List tools"}},
            "/skills":{"get":{"summary":"List installed skills"},"post":{"summary":"Import skill from content or file/zip"}},
            "/skills/{name}":{"get":{"summary":"Load skill content"},"delete":{"summary":"Delete skill by name and location"}},
            "/skills/import/preview":{"post":{"summary":"Preview skill import conflicts/actions"}},
            "/skills/templates":{"get":{"summary":"List installable skill templates"}},
            "/skills/templates/{id}/install":{"post":{"summary":"Install a skill template"}},
            "/memory/put":{"post":{"summary":"Store scoped memory content"}},
            "/memory/promote":{"post":{"summary":"Promote memory across tiers with scrub/audit"}},
            "/memory/search":{"post":{"summary":"Search scoped memory with capability gating"}},
            "/memory/audit":{"get":{"summary":"List memory audit events"}},
            "/mission":{"get":{"summary":"List missions"},"post":{"summary":"Create mission"}},
            "/mission/{id}":{"get":{"summary":"Get mission"}},
            "/mission/{id}/event":{"post":{"summary":"Apply mission event through reducer"}},
            "/agent-team/templates":{"get":{"summary":"List agent team templates"}},
            "/agent-team/instances":{"get":{"summary":"List agent team instances"}},
            "/agent-team/missions":{"get":{"summary":"List agent team mission summaries"}},
            "/agent-team/approvals":{"get":{"summary":"List pending approvals for agent-team actions"}},
            "/agent-team/approvals/spawn/{id}/approve":{"post":{"summary":"Approve a pending spawn approval"}},
            "/agent-team/approvals/spawn/{id}/deny":{"post":{"summary":"Deny a pending spawn approval"}},
            "/agent-team/spawn":{"post":{"summary":"Spawn an agent team instance with server policy gating"}},
            "/agent-team/instance/{id}/cancel":{"post":{"summary":"Cancel an agent team instance"}},
            "/agent-team/mission/{id}/cancel":{"post":{"summary":"Cancel all instances for a mission"}},
            "/routines":{"get":{"summary":"List routines"},"post":{"summary":"Create routine"}},
            "/routines/{id}":{"patch":{"summary":"Update routine"},"delete":{"summary":"Delete routine"}},
            "/routines/{id}/run_now":{"post":{"summary":"Trigger routine immediately"}},
            "/routines/{id}/history":{"get":{"summary":"List routine history"}},
            "/routines/events":{"get":{"summary":"SSE stream for routine lifecycle events"}},
            "/resource":{"get":{"summary":"List shared resources by prefix"}},
            "/resource/{key}":{"get":{"summary":"Get shared resource"},"put":{"summary":"Put shared resource with optional revision guard"},"patch":{"summary":"Patch shared resource with optional revision guard"},"delete":{"summary":"Delete shared resource with optional revision guard"}},
            "/resource/events":{"get":{"summary":"SSE stream for shared resource events"}},
            "/command":{"get":{"summary":"List executable commands"}},
            "/session/{id}/command":{"post":{"summary":"Run explicit command"}},
            "/session/{id}/shell":{"post":{"summary":"Run shell command"}},
            "/lsp":{"get":{"summary":"LSP diagnostics/navigation"}},
            "/pty/{id}/ws":{"get":{"summary":"PTY websocket stream"}}
        }
    }))
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
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::body::{to_bytes, Body};
    use axum::http::Request;
    use std::time::Duration;
    use tandem_core::{
        AgentRegistry, CancellationRegistry, ConfigStore, EngineLoop, EventBus, PermissionManager,
        PluginRegistry, Storage,
    };
    use tandem_providers::ProviderRegistry;
    use tandem_runtime::{LspManager, McpRegistry, PtyManager, WorkspaceIndex};
    use tandem_tools::ToolRegistry;
    use tokio::sync::broadcast;
    use tower::ServiceExt;
    use uuid::Uuid;

    async fn test_state() -> AppState {
        let root = std::env::temp_dir().join(format!("tandem-http-test-{}", Uuid::new_v4()));
        let global = root.join("global-config.json");
        std::env::set_var("TANDEM_GLOBAL_CONFIG", &global);
        let storage = Arc::new(Storage::new(root.join("storage")).await.expect("storage"));
        let config = ConfigStore::new(root.join("config.json"), None)
            .await
            .expect("config");
        let event_bus = EventBus::new();
        let providers = ProviderRegistry::new(config.get().await.into());
        let plugins = PluginRegistry::new(".").await.expect("plugins");
        let agents = AgentRegistry::new(".").await.expect("agents");
        let tools = ToolRegistry::new();
        let permissions = PermissionManager::new(event_bus.clone());
        let mcp = McpRegistry::new_with_state_file(root.join("mcp.json"));
        let pty = PtyManager::new();
        let lsp = LspManager::new(".");
        let auth = Arc::new(tokio::sync::RwLock::new(HashMap::new()));
        let logs = Arc::new(tokio::sync::RwLock::new(Vec::new()));
        let workspace_index = WorkspaceIndex::new(".").await;
        let cancellations = CancellationRegistry::new();
        let host_runtime_context = crate::detect_host_runtime_context();
        let engine_loop = EngineLoop::new(
            storage.clone(),
            event_bus.clone(),
            providers.clone(),
            plugins.clone(),
            agents.clone(),
            permissions.clone(),
            tools.clone(),
            cancellations.clone(),
            host_runtime_context.clone(),
        );
        let mut state = AppState::new_starting(Uuid::new_v4().to_string(), false);
        state.shared_resources_path = root.join("shared_resources.json");
        state
            .mark_ready(crate::RuntimeState {
                storage,
                config,
                event_bus,
                providers,
                plugins,
                agents,
                tools,
                permissions,
                mcp,
                pty,
                lsp,
                auth,
                logs,
                workspace_index,
                cancellations,
                engine_loop,
                host_runtime_context,
            })
            .await
            .expect("runtime ready");
        state
    }

    async fn next_event_of_type(
        rx: &mut broadcast::Receiver<EngineEvent>,
        expected_type: &str,
    ) -> EngineEvent {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.expect("event");
                if event.event_type == expected_type {
                    return event;
                }
            }
        })
        .await
        .expect("event timeout")
    }

    #[tokio::test]
    async fn approve_tool_by_call_route_replies_permission() {
        let state = test_state().await;
        let request = state
            .permissions
            .ask_for_session(Some("s1"), "bash", json!({"command":"echo hi"}))
            .await;
        let app = app_router(state.clone());
        let req = Request::builder()
            .method("POST")
            .uri(format!("/sessions/s1/tools/{}/approve", request.id))
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(payload.get("ok").and_then(|v| v.as_bool()), Some(true));
    }

    #[tokio::test]
    async fn session_todo_route_returns_normalized_items() {
        let state = test_state().await;
        let session = Session::new(Some("test".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        state.storage.save_session(session).await.expect("save");
        state
            .storage
            .set_todos(
                &session_id,
                vec![
                    json!({"content":"one"}),
                    json!({"text":"two","status":"in_progress"}),
                ],
            )
            .await
            .expect("set todos");

        let app = app_router(state);
        let req = Request::builder()
            .method("GET")
            .uri(format!("/session/{session_id}/todo"))
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        let todos = payload.as_array().expect("todos array");
        assert_eq!(todos.len(), 2);
        for todo in todos {
            assert!(todo.get("id").and_then(|v| v.as_str()).is_some());
            assert!(todo.get("content").and_then(|v| v.as_str()).is_some());
            assert!(todo.get("status").and_then(|v| v.as_str()).is_some());
        }
    }

    #[tokio::test]
    async fn answer_question_alias_route_returns_ok() {
        let state = test_state().await;
        let session = Session::new(Some("q".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        state.storage.save_session(session).await.expect("save");
        let question = state
            .storage
            .add_question_request(
                &session_id,
                "m1",
                vec![json!({"header":"h","question":"q","options":[]})],
            )
            .await
            .expect("question");

        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri(format!(
                "/sessions/{}/questions/{}/answer",
                session_id, question.id
            ))
            .header("content-type", "application/json")
            .body(Body::from(r#"{"answer":"ok"}"#))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(payload.get("ok").and_then(|v| v.as_bool()), Some(true));
    }

    #[tokio::test]
    async fn api_session_alias_lists_sessions() {
        let state = test_state().await;
        let session = Session::new(Some("alias".to_string()), Some(".".to_string()));
        state.storage.save_session(session).await.expect("save");
        let app = app_router(state);
        let req = Request::builder()
            .method("GET")
            .uri("/api/session")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert!(payload.as_array().map(|v| !v.is_empty()).unwrap_or(false));
    }

    #[tokio::test]
    async fn create_session_accepts_camel_case_model_spec() {
        let state = test_state().await;
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/session")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "title": "camel-model",
                    "model": {
                        "providerID": "openrouter",
                        "modelID": "openai/gpt-4o-mini"
                    }
                })
                .to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        let model = payload.get("model").cloned().unwrap_or_else(|| json!({}));
        assert_eq!(
            model.get("providerID").and_then(|v| v.as_str()),
            Some("openrouter")
        );
        assert_eq!(
            model.get("modelID").and_then(|v| v.as_str()),
            Some("openai/gpt-4o-mini")
        );
        assert!(payload.get("environment").is_some());
    }

    #[tokio::test]
    async fn global_health_route_returns_healthy_shape() {
        let state = test_state().await;
        let app = app_router(state);
        let req = Request::builder()
            .method("GET")
            .uri("/global/health")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(payload.get("healthy").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(payload.get("ready").and_then(|v| v.as_bool()), Some(true));
        assert!(payload.get("phase").is_some());
        assert!(payload.get("startup_attempt_id").is_some());
        assert!(payload.get("startup_elapsed_ms").is_some());
        assert!(payload.get("version").and_then(|v| v.as_str()).is_some());
        assert!(payload.get("mode").and_then(|v| v.as_str()).is_some());
        assert!(payload.get("environment").is_some());
    }

    #[tokio::test]
    async fn non_health_routes_are_blocked_until_runtime_ready() {
        let state = AppState::new_starting(Uuid::new_v4().to_string(), false);
        let app = app_router(state);
        let req = Request::builder()
            .method("GET")
            .uri("/provider")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            payload.get("code").and_then(|v| v.as_str()),
            Some("ENGINE_STARTING")
        );
    }

    #[tokio::test]
    async fn provider_route_returns_catalog_shape() {
        let state = test_state().await;
        let app = app_router(state);
        let req = Request::builder()
            .method("GET")
            .uri("/provider")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        let all = payload
            .get("all")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(!all.is_empty());
        let first = all.first().cloned().unwrap_or_else(|| json!({}));
        assert!(first.get("id").and_then(|v| v.as_str()).is_some());
    }

    #[test]
    fn merge_known_provider_defaults_does_not_mark_all_connected() {
        let mut wire = WireProviderCatalog {
            all: Vec::new(),
            connected: Vec::new(),
        };
        merge_known_provider_defaults(&mut wire);

        assert!(wire.all.iter().any(|p| p.id == "openrouter"));
        assert!(wire.connected.is_empty());
    }

    #[tokio::test]
    async fn post_session_message_returns_wire_message() {
        let state = test_state().await;
        let session = Session::new(Some("post-msg".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        state.storage.save_session(session).await.expect("save");
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/message"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"parts":[{"type":"text","text":"hello from test"}]}).to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert!(payload.get("info").is_some());
        assert!(payload.get("parts").is_some());
    }

    #[tokio::test]
    async fn session_listing_honors_workspace_scope_query() {
        let state = test_state().await;
        let ws_a = std::env::temp_dir()
            .join(format!("tandem-http-ws-a-{}", Uuid::new_v4()))
            .to_string_lossy()
            .to_string();
        let ws_b = std::env::temp_dir()
            .join(format!("tandem-http-ws-b-{}", Uuid::new_v4()))
            .to_string_lossy()
            .to_string();

        let mut session_a = Session::new(Some("A".to_string()), Some(ws_a.clone()));
        session_a.workspace_root = Some(ws_a.clone());
        state.storage.save_session(session_a).await.expect("save A");

        let mut session_b = Session::new(Some("B".to_string()), Some(ws_b.clone()));
        session_b.workspace_root = Some(ws_b.clone());
        state.storage.save_session(session_b).await.expect("save B");

        let app = app_router(state);
        let encoded_ws_a = ws_a.replace('\\', "%5C").replace(':', "%3A");
        let scoped_req = Request::builder()
            .method("GET")
            .uri(format!(
                "/session?scope=workspace&workspace={}",
                encoded_ws_a
            ))
            .body(Body::empty())
            .expect("request");
        let scoped_resp = app.clone().oneshot(scoped_req).await.expect("response");
        assert_eq!(scoped_resp.status(), StatusCode::OK);
        let scoped_body = to_bytes(scoped_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let scoped_payload: Value = serde_json::from_slice(&scoped_body).expect("json");
        assert_eq!(scoped_payload.as_array().map(|v| v.len()), Some(1));

        let global_req = Request::builder()
            .method("GET")
            .uri("/session?scope=global")
            .body(Body::empty())
            .expect("request");
        let global_resp = app.oneshot(global_req).await.expect("response");
        assert_eq!(global_resp.status(), StatusCode::OK);
        let global_body = to_bytes(global_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let global_payload: Value = serde_json::from_slice(&global_body).expect("json");
        assert_eq!(global_payload.as_array().map(|v| v.len()), Some(2));
    }

    #[tokio::test]
    async fn attach_session_route_updates_workspace_metadata() {
        let state = test_state().await;
        let ws_a = std::env::temp_dir()
            .join(format!("tandem-http-attach-a-{}", Uuid::new_v4()))
            .to_string_lossy()
            .to_string();
        let ws_b = std::env::temp_dir()
            .join(format!("tandem-http-attach-b-{}", Uuid::new_v4()))
            .to_string_lossy()
            .to_string();
        let mut session = Session::new(Some("attach".to_string()), Some(ws_a.clone()));
        session.workspace_root = Some(ws_a);
        let session_id = session.id.clone();
        state.storage.save_session(session).await.expect("save");

        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/attach"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"target_workspace": ws_b, "reason_tag": "manual_attach"}).to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            payload.get("attachReason").and_then(|v| v.as_str()),
            Some("manual_attach")
        );
        assert!(payload
            .get("workspaceRoot")
            .and_then(|v| v.as_str())
            .is_some());
    }

    #[tokio::test]
    async fn message_part_updated_event_contains_required_wire_fields() {
        let state = test_state().await;
        let session = Session::new(Some("sse-shape".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        state.storage.save_session(session).await.expect("save");
        let mut rx = state.event_bus.subscribe();
        let app = app_router(state);

        let req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/prompt_async"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"parts":[{"type":"text","text":"hello streaming"}]}).to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.expect("event");
                if event.event_type == "message.part.updated" {
                    return event;
                }
            }
        })
        .await
        .expect("message.part.updated timeout");

        let part = event
            .properties
            .get("part")
            .cloned()
            .unwrap_or_else(|| json!({}));
        assert!(part.get("id").and_then(|v| v.as_str()).is_some());
        assert_eq!(
            part.get("sessionID").and_then(|v| v.as_str()),
            Some(session_id.as_str())
        );
        assert!(part.get("messageID").and_then(|v| v.as_str()).is_some());
        assert!(part.get("type").and_then(|v| v.as_str()).is_some());
    }

    #[test]
    fn normalize_run_event_adds_required_fields() {
        let event = EngineEvent::new(
            "message.part.updated",
            json!({
                "part": { "type": "text" },
                "delta": "hello"
            }),
        );
        let normalized = normalize_run_event(event, "s-1", "r-1");
        assert_eq!(
            normalized
                .properties
                .get("sessionID")
                .and_then(|v| v.as_str()),
            Some("s-1")
        );
        assert_eq!(
            normalized.properties.get("runID").and_then(|v| v.as_str()),
            Some("r-1")
        );
        assert_eq!(
            normalized
                .properties
                .get("channel")
                .and_then(|v| v.as_str()),
            Some("assistant")
        );
    }

    #[test]
    fn infer_event_channel_routes_tool_message_parts() {
        let channel = infer_event_channel(
            "message.part.updated",
            &serde_json::from_value::<serde_json::Map<String, Value>>(json!({
                "part": { "type": "tool-result" }
            }))
            .expect("map"),
        );
        assert_eq!(channel, "tool");
    }

    #[tokio::test]
    async fn prompt_async_permission_approve_executes_tool_and_emits_todo_update() {
        let state = test_state().await;
        let session = Session::new(Some("perm".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        state.storage.save_session(session).await.expect("save");
        let mut rx = state.event_bus.subscribe();
        let app = app_router(state.clone());

        let prompt_body = json!({
            "parts": [
                {
                    "type": "text",
                    "text": "/tool todo_write {\"todos\":[{\"content\":\"write tests\"}]}"
                }
            ]
        });
        let req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/prompt_async"))
            .header("content-type", "application/json")
            .body(Body::from(prompt_body.to_string()))
            .expect("request");
        let resp = app.clone().oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let request_id = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.expect("event");
                if event.event_type == "permission.asked" {
                    let id = event
                        .properties
                        .get("requestID")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !id.is_empty() {
                        return id;
                    }
                }
            }
        })
        .await
        .expect("permission asked timeout");

        let approve_req = Request::builder()
            .method("POST")
            .uri(format!(
                "/sessions/{}/tools/{}/approve",
                session_id, request_id
            ))
            .body(Body::empty())
            .expect("approve request");
        let approve_resp = app.clone().oneshot(approve_req).await.expect("approve");
        assert_eq!(approve_resp.status(), StatusCode::OK);

        let todo_event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.expect("event");
                if event.event_type == "todo.updated" {
                    return event;
                }
            }
        })
        .await
        .expect("todo.updated timeout");

        assert_eq!(
            todo_event
                .properties
                .get("sessionID")
                .and_then(|v| v.as_str()),
            Some(session_id.as_str())
        );
        let todos = todo_event
            .properties
            .get("todos")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(todos.len(), 1);
        assert_eq!(
            todos[0].get("content").and_then(|v| v.as_str()),
            Some("write tests")
        );
    }

    #[tokio::test]
    async fn approve_route_returns_error_envelope_for_unknown_request() {
        let state = test_state().await;
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/sessions/s1/tools/missing/approve")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            payload.get("code").and_then(|v| v.as_str()),
            Some("permission_request_not_found")
        );
        assert!(payload.get("error").and_then(|v| v.as_str()).is_some());
    }

    #[tokio::test]
    async fn prompt_async_return_run_returns_202_with_run_id_and_attach_stream() {
        let state = test_state().await;
        let session = Session::new(Some("return-run".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        state.storage.save_session(session).await.expect("save");
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/prompt_async?return=run"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"parts":[{"type":"text","text":"hello return=run"}]}).to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::ACCEPTED);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        let run_id = payload.get("runID").and_then(|v| v.as_str()).unwrap_or("");
        let attach = payload
            .get("attachEventStream")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(!run_id.is_empty());
        assert_eq!(
            attach,
            format!("/event?sessionID={session_id}&runID={run_id}")
        );
    }

    #[tokio::test]
    async fn get_session_run_returns_active_metadata_while_run_is_in_flight() {
        let state = test_state().await;
        let session = Session::new(Some("active-run".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        state.storage.save_session(session).await.expect("save");
        let app = app_router(state.clone());

        let first_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/prompt_async?return=run"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "parts": [
                        {"type":"text","text":"/tool todo_write {\"todos\":[{\"content\":\"hold run\"}]}"}
                    ]
                })
                .to_string(),
            ))
            .expect("request");
        let first_resp = app.clone().oneshot(first_req).await.expect("response");
        assert_eq!(first_resp.status(), StatusCode::ACCEPTED);
        let first_body = to_bytes(first_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let first_payload: Value = serde_json::from_slice(&first_body).expect("json");
        let run_id = first_payload
            .get("runID")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        assert!(!run_id.is_empty());

        let run_req = Request::builder()
            .method("GET")
            .uri(format!("/session/{session_id}/run"))
            .body(Body::empty())
            .expect("request");
        let run_resp = app.oneshot(run_req).await.expect("response");
        assert_eq!(run_resp.status(), StatusCode::OK);
        let run_body = to_bytes(run_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let run_payload: Value = serde_json::from_slice(&run_body).expect("json");
        let active = run_payload.get("active").cloned().unwrap_or(Value::Null);
        assert_eq!(
            active.get("runID").and_then(|v| v.as_str()),
            Some(run_id.as_str())
        );

        let cancel_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/cancel"))
            .body(Body::empty())
            .expect("cancel request");
        let cancel_resp = app_router(state)
            .oneshot(cancel_req)
            .await
            .expect("cancel response");
        assert_eq!(cancel_resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn concurrent_prompt_async_returns_conflict_with_nested_active_run() {
        let state = test_state().await;
        let session = Session::new(Some("conflict".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        state.storage.save_session(session).await.expect("save");
        let app = app_router(state.clone());

        let first_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/prompt_async?return=run"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "parts": [
                        {"type":"text","text":"/tool todo_write {\"todos\":[{\"content\":\"block\"}]}"}
                    ]
                })
                .to_string(),
            ))
            .expect("request");
        let first_resp = app.clone().oneshot(first_req).await.expect("response");
        assert_eq!(first_resp.status(), StatusCode::ACCEPTED);
        let first_body = to_bytes(first_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let first_payload: Value = serde_json::from_slice(&first_body).expect("json");
        let active_run_id = first_payload
            .get("runID")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        assert!(!active_run_id.is_empty());

        let second_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/prompt_async"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"parts":[{"type":"text","text":"second prompt"}]}).to_string(),
            ))
            .expect("request");
        let second_resp = app.clone().oneshot(second_req).await.expect("response");
        assert_eq!(second_resp.status(), StatusCode::CONFLICT);
        let second_body = to_bytes(second_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let second_payload: Value = serde_json::from_slice(&second_body).expect("json");
        assert_eq!(
            second_payload.get("code").and_then(|v| v.as_str()),
            Some("SESSION_RUN_CONFLICT")
        );
        assert_eq!(
            second_payload
                .get("activeRun")
                .and_then(|v| v.get("runID"))
                .and_then(|v| v.as_str()),
            Some(active_run_id.as_str())
        );
        assert!(second_payload
            .get("activeRun")
            .and_then(|v| v.get("startedAtMs"))
            .and_then(|v| v.as_i64())
            .is_some());
        assert!(second_payload
            .get("activeRun")
            .and_then(|v| v.get("lastActivityAtMs"))
            .and_then(|v| v.as_i64())
            .is_some());
        assert!(second_payload
            .get("retryAfterMs")
            .and_then(|v| v.as_u64())
            .is_some());
        assert_eq!(
            second_payload
                .get("attachEventStream")
                .and_then(|v| v.as_str()),
            Some(format!("/event?sessionID={session_id}&runID={active_run_id}").as_str())
        );

        let cancel_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/cancel"))
            .body(Body::empty())
            .expect("cancel request");
        let cancel_resp = app_router(state)
            .oneshot(cancel_req)
            .await
            .expect("cancel response");
        assert_eq!(cancel_resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn append_message_succeeds_while_run_is_active() {
        let state = test_state().await;
        let session = Session::new(Some("append-active".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        state.storage.save_session(session).await.expect("save");
        let app = app_router(state.clone());

        let first_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/prompt_async?return=run"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "parts": [
                        {"type":"text","text":"/tool todo_write {\"todos\":[{\"content\":\"block append\"}]}"}
                    ]
                })
                .to_string(),
            ))
            .expect("request");
        let first_resp = app.clone().oneshot(first_req).await.expect("response");
        assert_eq!(first_resp.status(), StatusCode::ACCEPTED);

        let append_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/message?mode=append"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"parts":[{"type":"text","text":"appended while active"}]}).to_string(),
            ))
            .expect("append request");
        let append_resp = app.clone().oneshot(append_req).await.expect("response");
        assert_eq!(append_resp.status(), StatusCode::OK);
        let _ = to_bytes(append_resp.into_body(), usize::MAX)
            .await
            .expect("body");

        let list_req = Request::builder()
            .method("GET")
            .uri(format!("/session/{session_id}/message"))
            .body(Body::empty())
            .expect("list request");
        let list_resp = app.clone().oneshot(list_req).await.expect("response");
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
        let list = list_payload.as_array().cloned().unwrap_or_default();
        assert!(!list.is_empty());
        let has_appended_text = list.iter().any(|message| {
            message
                .get("parts")
                .and_then(|v| v.as_array())
                .map(|parts| {
                    parts.iter().any(|part| {
                        part.get("text").and_then(|v| v.as_str()) == Some("appended while active")
                    })
                })
                .unwrap_or(false)
        });
        assert!(has_appended_text);

        let cancel_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/cancel"))
            .body(Body::empty())
            .expect("cancel request");
        let cancel_resp = app_router(state)
            .oneshot(cancel_req)
            .await
            .expect("cancel response");
        assert_eq!(cancel_resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn skills_endpoints_return_expected_shapes() {
        let state = test_state().await;
        let app = app_router(state);

        let list_req = Request::builder()
            .method("GET")
            .uri("/skills")
            .body(Body::empty())
            .expect("request");
        let list_resp = app.clone().oneshot(list_req).await.expect("response");
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
        assert!(list_payload.is_array());

        let legacy_req = Request::builder()
            .method("GET")
            .uri("/skill")
            .body(Body::empty())
            .expect("request");
        let legacy_resp = app.clone().oneshot(legacy_req).await.expect("response");
        assert_eq!(legacy_resp.status(), StatusCode::OK);
        let legacy_body = to_bytes(legacy_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let legacy_payload: Value = serde_json::from_slice(&legacy_body).expect("json");
        assert!(legacy_payload.get("skills").is_some());
        assert!(legacy_payload.get("deprecation_warning").is_some());
    }

    #[tokio::test]
    async fn auto_rename_session_on_first_message() {
        let state = test_state().await;
        let app = app_router(state.clone());

        // 1. Create session
        let create_req = Request::builder()
            .method("POST")
            .uri("/session")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "title": null }).to_string()))
            .expect("create request");
        let create_resp = app.clone().oneshot(create_req).await.expect("response");
        assert_eq!(create_resp.status(), StatusCode::OK);
        let body = to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let session: Value = serde_json::from_slice(&body).expect("json");
        let session_id = session
            .get("id")
            .and_then(|v| v.as_str())
            .expect("session id")
            .to_string();
        let title = session
            .get("title")
            .and_then(|v| v.as_str())
            .expect("title");
        assert_eq!(title, "New session");

        // 2. Append first message
        let append_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/message"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "parts": [{"type": "text", "text": "Hello world this is a test message"}]
                })
                .to_string(),
            ))
            .expect("append request");
        let append_resp = app.clone().oneshot(append_req).await.expect("response");
        assert_eq!(append_resp.status(), StatusCode::OK);

        // 3. Verify title changed
        let get_req = Request::builder()
            .method("GET")
            .uri(format!("/session/{session_id}"))
            .body(Body::empty())
            .expect("get request");
        let get_resp = app.clone().oneshot(get_req).await.expect("response");
        assert_eq!(get_resp.status(), StatusCode::OK);
        let body = to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let session: Value = serde_json::from_slice(&body).expect("json");
        let title = session
            .get("title")
            .and_then(|v| v.as_str())
            .expect("title");
        assert_eq!(title, "Hello world this is a test message");

        // 4. Append second message
        let append_req_2 = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/message"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "parts": [{"type": "text", "text": "Another message"}]
                })
                .to_string(),
            ))
            .expect("append request");
        let append_resp_2 = app.clone().oneshot(append_req_2).await.expect("response");
        assert_eq!(append_resp_2.status(), StatusCode::OK);

        // 5. Verify title did NOT change
        let get_req_2 = Request::builder()
            .method("GET")
            .uri(format!("/session/{session_id}"))
            .body(Body::empty())
            .expect("get request");
        let get_resp_2 = app.clone().oneshot(get_req_2).await.expect("response");

        let body = to_bytes(get_resp_2.into_body(), usize::MAX)
            .await
            .expect("body");
        let session: Value = serde_json::from_slice(&body).expect("json");
        let title = session
            .get("title")
            .and_then(|v| v.as_str())
            .expect("title");
        // Title should remain as the first message
        assert_eq!(title, "Hello world this is a test message");
    }

    #[tokio::test]
    async fn auto_rename_ignores_memory_context_wrappers() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/session")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "title": null }).to_string()))
            .expect("create request");
        let create_resp = app.clone().oneshot(create_req).await.expect("response");
        assert_eq!(create_resp.status(), StatusCode::OK);
        let body = to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let session: Value = serde_json::from_slice(&body).expect("json");
        let session_id = session
            .get("id")
            .and_then(|v| v.as_str())
            .expect("session id")
            .to_string();

        let wrapped = "<memory_context>\n<current_session>\n- fact\n</current_session>\n</memory_context>\n\n[Mode instructions]\nUse tools.\n\n[User request]\nShip the fix quickly";
        let append_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/message"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "parts": [{"type":"text","text": wrapped}]
                })
                .to_string(),
            ))
            .expect("append request");
        let append_resp = app.clone().oneshot(append_req).await.expect("response");
        assert_eq!(append_resp.status(), StatusCode::OK);

        let get_req = Request::builder()
            .method("GET")
            .uri(format!("/session/{session_id}"))
            .body(Body::empty())
            .expect("get request");
        let get_resp = app.clone().oneshot(get_req).await.expect("response");
        assert_eq!(get_resp.status(), StatusCode::OK);
        let body = to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let session: Value = serde_json::from_slice(&body).expect("json");
        let title = session
            .get("title")
            .and_then(|v| v.as_str())
            .expect("title");
        assert_eq!(title, "Ship the fix quickly");
    }

    #[tokio::test]
    async fn resource_put_patch_get_and_list_roundtrip() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let put_req = Request::builder()
            .method("PUT")
            .uri("/resource/project/demo/board")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "value": {"status":"todo","count":1},
                    "updated_by": "agent-1"
                })
                .to_string(),
            ))
            .expect("put request");
        let put_resp = app.clone().oneshot(put_req).await.expect("put response");
        assert_eq!(put_resp.status(), StatusCode::OK);

        let patch_req = Request::builder()
            .method("PATCH")
            .uri("/resource/project/demo/board")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "value": {"count":2},
                    "if_match_rev": 1,
                    "updated_by": "agent-2"
                })
                .to_string(),
            ))
            .expect("patch request");
        let patch_resp = app
            .clone()
            .oneshot(patch_req)
            .await
            .expect("patch response");
        assert_eq!(patch_resp.status(), StatusCode::OK);

        let get_req = Request::builder()
            .method("GET")
            .uri("/resource/project/demo/board")
            .body(Body::empty())
            .expect("get request");
        let get_resp = app.clone().oneshot(get_req).await.expect("get response");
        assert_eq!(get_resp.status(), StatusCode::OK);
        let get_body = to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body");
        let payload: Value = serde_json::from_slice(&get_body).expect("json");
        assert_eq!(
            payload
                .get("resource")
                .and_then(|r| r.get("rev"))
                .and_then(|v| v.as_u64()),
            Some(2)
        );
        assert_eq!(
            payload
                .get("resource")
                .and_then(|r| r.get("value"))
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str()),
            Some("todo")
        );
        assert_eq!(
            payload
                .get("resource")
                .and_then(|r| r.get("value"))
                .and_then(|v| v.get("count"))
                .and_then(|v| v.as_i64()),
            Some(2)
        );

        let list_req = Request::builder()
            .method("GET")
            .uri("/resource?prefix=project/demo")
            .body(Body::empty())
            .expect("list request");
        let list_resp = app.clone().oneshot(list_req).await.expect("list response");
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("list body");
        let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
        assert_eq!(list_payload.get("count").and_then(|v| v.as_u64()), Some(1));
    }

    #[tokio::test]
    async fn resource_put_conflict_returns_409() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let first_req = Request::builder()
            .method("PUT")
            .uri("/resource/mission/demo/card-1")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "value": {"title":"Card 1"},
                    "updated_by": "agent-1"
                })
                .to_string(),
            ))
            .expect("first request");
        let first_resp = app
            .clone()
            .oneshot(first_req)
            .await
            .expect("first response");
        assert_eq!(first_resp.status(), StatusCode::OK);

        let conflict_req = Request::builder()
            .method("PUT")
            .uri("/resource/mission/demo/card-1")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "value": {"title":"Card 1 updated"},
                    "if_match_rev": 99,
                    "updated_by": "agent-2"
                })
                .to_string(),
            ))
            .expect("conflict request");
        let conflict_resp = app
            .clone()
            .oneshot(conflict_req)
            .await
            .expect("conflict response");
        assert_eq!(conflict_resp.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn resource_updated_event_contract_snapshot() {
        let state = test_state().await;
        let mut rx = state.event_bus.subscribe();
        let app = app_router(state.clone());

        let put_req = Request::builder()
            .method("PUT")
            .uri("/resource/project/demo/board")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "value": {"status":"todo"},
                    "updated_by": "agent-1"
                })
                .to_string(),
            ))
            .expect("put request");
        let put_resp = app.clone().oneshot(put_req).await.expect("put response");
        assert_eq!(put_resp.status(), StatusCode::OK);

        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.expect("event");
                if event.event_type == "resource.updated" {
                    return event;
                }
            }
        })
        .await
        .expect("resource.updated timeout");

        let mut properties = event
            .properties
            .as_object()
            .cloned()
            .expect("resource.updated properties object");
        let updated_at_ms = properties
            .remove("updatedAtMs")
            .and_then(|v| v.as_u64())
            .expect("updatedAtMs");
        assert!(updated_at_ms > 0);

        let snapshot = json!({
            "type": event.event_type,
            "properties": properties,
        });
        let expected = json!({
            "type": "resource.updated",
            "properties": {
                "key": "project/demo/board",
                "rev": 1,
                "updatedBy": "agent-1"
            }
        });
        assert_eq!(snapshot, expected);
    }

    #[tokio::test]
    async fn resource_deleted_event_contract_snapshot() {
        let state = test_state().await;
        let mut rx = state.event_bus.subscribe();
        let app = app_router(state.clone());

        let put_req = Request::builder()
            .method("PUT")
            .uri("/resource/project/demo/board")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "value": {"status":"todo"},
                    "updated_by": "agent-1"
                })
                .to_string(),
            ))
            .expect("put request");
        let put_resp = app.clone().oneshot(put_req).await.expect("put response");
        assert_eq!(put_resp.status(), StatusCode::OK);

        let delete_req = Request::builder()
            .method("DELETE")
            .uri("/resource/project/demo/board")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "if_match_rev": 1,
                    "updated_by": "reviewer-1"
                })
                .to_string(),
            ))
            .expect("delete request");
        let delete_resp = app
            .clone()
            .oneshot(delete_req)
            .await
            .expect("delete response");
        assert_eq!(delete_resp.status(), StatusCode::OK);

        let event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.expect("event");
                if event.event_type == "resource.deleted" {
                    return event;
                }
            }
        })
        .await
        .expect("resource.deleted timeout");

        let mut properties = event
            .properties
            .as_object()
            .cloned()
            .expect("resource.deleted properties object");
        let updated_at_ms = properties
            .remove("updatedAtMs")
            .and_then(|v| v.as_u64())
            .expect("updatedAtMs");
        assert!(updated_at_ms > 0);

        let snapshot = json!({
            "type": event.event_type,
            "properties": properties,
        });
        let expected = json!({
            "type": "resource.deleted",
            "properties": {
                "key": "project/demo/board",
                "rev": 1,
                "updatedBy": "reviewer-1"
            }
        });
        assert_eq!(snapshot, expected);
    }

    #[tokio::test]
    async fn mission_create_and_get_roundtrip() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/mission")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "title": "Ship control center",
                    "goal": "Build mission scaffolding",
                    "work_items": [
                        {"work_item_id":"w-1","title":"Implement API"}
                    ]
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);
        let create_body = to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let create_payload: Value = serde_json::from_slice(&create_body).expect("json");
        let mission_id = create_payload
            .get("mission")
            .and_then(|v| v.get("mission_id"))
            .and_then(|v| v.as_str())
            .expect("mission id")
            .to_string();

        let get_req = Request::builder()
            .method("GET")
            .uri(format!("/mission/{mission_id}"))
            .body(Body::empty())
            .expect("get request");
        let get_resp = app.clone().oneshot(get_req).await.expect("get response");
        assert_eq!(get_resp.status(), StatusCode::OK);
        let get_body = to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let get_payload: Value = serde_json::from_slice(&get_body).expect("json");
        assert_eq!(
            get_payload
                .get("mission")
                .and_then(|v| v.get("work_items"))
                .and_then(|v| v.as_array())
                .map(|v| v.len()),
            Some(1)
        );
    }

    #[tokio::test]
    async fn agent_team_spawn_denied_when_policy_missing() {
        let state = test_state().await;
        let app = app_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/agent-team/spawn")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "role": "worker",
                    "source": "ui_action",
                    "justification": "need parallel implementation"
                })
                .to_string(),
            ))
            .expect("spawn request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            payload.get("code").and_then(|v| v.as_str()),
            Some("spawn_policy_missing")
        );
    }

    #[tokio::test]
    async fn agent_team_spawn_approved_with_policy_and_template() {
        let state = test_state().await;
        let workspace_root = state.workspace_index.snapshot().await.root;
        state
            .agent_teams
            .set_for_test(
                Some(workspace_root),
                Some(tandem_orchestrator::SpawnPolicy {
                    enabled: true,
                    require_justification: true,
                    max_agents: Some(20),
                    max_concurrent: Some(10),
                    child_budget_percent_of_parent_remaining: Some(50),
                    spawn_edges: {
                        let mut map = std::collections::HashMap::new();
                        map.insert(
                            tandem_orchestrator::AgentRole::Orchestrator,
                            tandem_orchestrator::RoleSpawnRule {
                                behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                                can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                            },
                        );
                        map
                    },
                    required_skills: std::collections::HashMap::new(),
                    role_defaults: std::collections::HashMap::new(),
                    mission_total_budget: None,
                    cost_per_1k_tokens_usd: None,
                    skill_sources: Default::default(),
                }),
                vec![tandem_orchestrator::AgentTemplate {
                    template_id: "worker-default".to_string(),
                    role: tandem_orchestrator::AgentRole::Worker,
                    system_prompt: Some("You are a worker".to_string()),
                    skills: vec![],
                    default_budget: tandem_orchestrator::BudgetLimit::default(),
                    capabilities: tandem_orchestrator::CapabilitySpec::default(),
                }],
            )
            .await;
        let app = app_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/agent-team/spawn")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "missionID": "m1",
                    "role": "worker",
                    "templateID": "worker-default",
                    "source": "ui_action",
                    "justification": "implement split test coverage"
                })
                .to_string(),
            ))
            .expect("spawn request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(payload.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert!(payload.get("instanceID").and_then(|v| v.as_str()).is_some());
        let skill_hash = payload
            .get("skillHash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(skill_hash.starts_with("sha256:"));
    }

    #[tokio::test]
    async fn agent_team_spawn_agent_tool_uses_same_policy_gate() {
        let state = test_state().await;
        let workspace_root = state.workspace_index.snapshot().await.root;
        state
            .agent_teams
            .set_for_test(
                Some(workspace_root),
                Some(tandem_orchestrator::SpawnPolicy {
                    enabled: true,
                    require_justification: true,
                    max_agents: Some(20),
                    max_concurrent: Some(10),
                    child_budget_percent_of_parent_remaining: Some(50),
                    spawn_edges: {
                        let mut map = std::collections::HashMap::new();
                        map.insert(
                            tandem_orchestrator::AgentRole::Orchestrator,
                            tandem_orchestrator::RoleSpawnRule {
                                behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                                can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                            },
                        );
                        map
                    },
                    required_skills: std::collections::HashMap::new(),
                    role_defaults: std::collections::HashMap::new(),
                    mission_total_budget: None,
                    cost_per_1k_tokens_usd: None,
                    skill_sources: Default::default(),
                }),
                vec![tandem_orchestrator::AgentTemplate {
                    template_id: "worker-default".to_string(),
                    role: tandem_orchestrator::AgentRole::Worker,
                    system_prompt: Some("You are a worker".to_string()),
                    skills: vec![],
                    default_budget: tandem_orchestrator::BudgetLimit::default(),
                    capabilities: tandem_orchestrator::CapabilitySpec::default(),
                }],
            )
            .await;
        let session = Session::new(Some("spawn tool".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        state
            .storage
            .save_session(session)
            .await
            .expect("save session");
        let mut rx = state.event_bus.subscribe();
        let app = app_router(state.clone());

        let prompt_body = json!({
            "parts": [
                {
                    "type": "text",
                    "text": "/tool spawn_agent {\"missionID\":\"m2\",\"role\":\"worker\",\"templateID\":\"worker-default\",\"source\":\"tool_call\",\"justification\":\"parallelize task\"}"
                }
            ]
        });
        let req = Request::builder()
            .method("POST")
            .uri(format!("/session/{session_id}/prompt_async"))
            .header("content-type", "application/json")
            .body(Body::from(prompt_body.to_string()))
            .expect("request");
        let resp = app.clone().oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        let request_id = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.expect("event");
                if event.event_type == "permission.asked" {
                    let id = event
                        .properties
                        .get("requestID")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    if !id.is_empty() {
                        return id;
                    }
                }
            }
        })
        .await
        .expect("permission asked timeout");

        let approve_req = Request::builder()
            .method("POST")
            .uri(format!(
                "/sessions/{}/tools/{}/approve",
                session_id, request_id
            ))
            .body(Body::empty())
            .expect("approve request");
        let approve_resp = app.clone().oneshot(approve_req).await.expect("approve");
        assert_eq!(approve_resp.status(), StatusCode::OK);

        let spawn_event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.expect("event");
                if event.event_type == "agent_team.spawn.approved" {
                    return event;
                }
            }
        })
        .await
        .expect("spawn event timeout");
        assert_eq!(
            spawn_event
                .properties
                .get("sessionID")
                .and_then(|v| v.as_str()),
            Some(session_id.as_str())
        );
        assert_eq!(
            spawn_event
                .properties
                .get("source")
                .and_then(|v| v.as_str()),
            Some("tool_call")
        );
    }

    #[tokio::test]
    async fn agent_team_cancel_instance_endpoint_updates_status() {
        let state = test_state().await;
        let workspace_root = state.workspace_index.snapshot().await.root;
        state
            .agent_teams
            .set_for_test(
                Some(workspace_root),
                Some(tandem_orchestrator::SpawnPolicy {
                    enabled: true,
                    require_justification: true,
                    max_agents: Some(20),
                    max_concurrent: Some(10),
                    child_budget_percent_of_parent_remaining: Some(50),
                    spawn_edges: {
                        let mut map = std::collections::HashMap::new();
                        map.insert(
                            tandem_orchestrator::AgentRole::Orchestrator,
                            tandem_orchestrator::RoleSpawnRule {
                                behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                                can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                            },
                        );
                        map
                    },
                    required_skills: std::collections::HashMap::new(),
                    role_defaults: std::collections::HashMap::new(),
                    mission_total_budget: None,
                    cost_per_1k_tokens_usd: None,
                    skill_sources: Default::default(),
                }),
                vec![tandem_orchestrator::AgentTemplate {
                    template_id: "worker-default".to_string(),
                    role: tandem_orchestrator::AgentRole::Worker,
                    system_prompt: Some("You are a worker".to_string()),
                    skills: vec![],
                    default_budget: tandem_orchestrator::BudgetLimit::default(),
                    capabilities: tandem_orchestrator::CapabilitySpec::default(),
                }],
            )
            .await;
        let app = app_router(state.clone());

        let spawn_req = Request::builder()
            .method("POST")
            .uri("/agent-team/spawn")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "missionID": "m3",
                    "role": "worker",
                    "templateID": "worker-default",
                    "source": "ui_action",
                    "justification": "work chunk"
                })
                .to_string(),
            ))
            .expect("spawn request");
        let spawn_resp = app
            .clone()
            .oneshot(spawn_req)
            .await
            .expect("spawn response");
        assert_eq!(spawn_resp.status(), StatusCode::OK);
        let spawn_body = to_bytes(spawn_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let spawn_payload: Value = serde_json::from_slice(&spawn_body).expect("json");
        let instance_id = spawn_payload
            .get("instanceID")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        assert!(!instance_id.is_empty());

        let cancel_req = Request::builder()
            .method("POST")
            .uri(format!("/agent-team/instance/{instance_id}/cancel"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "reason": "manual stop"
                })
                .to_string(),
            ))
            .expect("cancel request");
        let cancel_resp = app.oneshot(cancel_req).await.expect("cancel response");
        assert_eq!(cancel_resp.status(), StatusCode::OK);
        let cancel_body = to_bytes(cancel_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let cancel_payload: Value = serde_json::from_slice(&cancel_body).expect("json");
        assert_eq!(
            cancel_payload.get("status").and_then(|v| v.as_str()),
            Some("cancelled")
        );
    }

    #[tokio::test]
    async fn agent_team_capability_policy_denies_network_tool_by_default() {
        let state = test_state().await;
        let workspace_root = state.workspace_index.snapshot().await.root;
        state
            .agent_teams
            .set_for_test(
                Some(workspace_root),
                Some(tandem_orchestrator::SpawnPolicy {
                    enabled: true,
                    require_justification: true,
                    max_agents: Some(20),
                    max_concurrent: Some(10),
                    child_budget_percent_of_parent_remaining: Some(50),
                    spawn_edges: {
                        let mut map = std::collections::HashMap::new();
                        map.insert(
                            tandem_orchestrator::AgentRole::Orchestrator,
                            tandem_orchestrator::RoleSpawnRule {
                                behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                                can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                            },
                        );
                        map
                    },
                    required_skills: std::collections::HashMap::new(),
                    role_defaults: std::collections::HashMap::new(),
                    mission_total_budget: None,
                    cost_per_1k_tokens_usd: None,
                    skill_sources: Default::default(),
                }),
                vec![tandem_orchestrator::AgentTemplate {
                    template_id: "worker-default".to_string(),
                    role: tandem_orchestrator::AgentRole::Worker,
                    system_prompt: Some("You are a worker".to_string()),
                    skills: vec![],
                    default_budget: tandem_orchestrator::BudgetLimit::default(),
                    capabilities: tandem_orchestrator::CapabilitySpec::default(),
                }],
            )
            .await;
        let mut rx = state.event_bus.subscribe();
        let app = app_router(state.clone());

        let spawn_req = Request::builder()
            .method("POST")
            .uri("/agent-team/spawn")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "missionID": "m4",
                    "role": "worker",
                    "templateID": "worker-default",
                    "source": "ui_action",
                    "justification": "run safe task"
                })
                .to_string(),
            ))
            .expect("spawn request");
        let spawn_resp = app
            .clone()
            .oneshot(spawn_req)
            .await
            .expect("spawn response");
        assert_eq!(spawn_resp.status(), StatusCode::OK);
        let spawn_body = to_bytes(spawn_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let spawn_payload: Value = serde_json::from_slice(&spawn_body).expect("json");
        let child_session_id = spawn_payload
            .get("sessionID")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        assert!(!child_session_id.is_empty());

        let prompt_req = Request::builder()
            .method("POST")
            .uri(format!("/session/{child_session_id}/prompt_async"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "parts": [
                        {
                            "type": "text",
                            "text": "/tool websearch {\"query\":\"rust async\"}"
                        }
                    ]
                })
                .to_string(),
            ))
            .expect("prompt request");
        let prompt_resp = app
            .clone()
            .oneshot(prompt_req)
            .await
            .expect("prompt response");
        assert_eq!(prompt_resp.status(), StatusCode::NO_CONTENT);

        let denied_event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.expect("event");
                if event.event_type == "agent_team.capability.denied" {
                    return event;
                }
            }
        })
        .await
        .expect("capability denied timeout");
        assert_eq!(
            denied_event
                .properties
                .get("sessionID")
                .and_then(|v| v.as_str()),
            Some(child_session_id.as_str())
        );
        assert_eq!(
            denied_event.properties.get("tool").and_then(|v| v.as_str()),
            Some("websearch")
        );
    }

    #[tokio::test]
    async fn mission_created_event_contract_snapshot() {
        let state = test_state().await;
        let mut rx = state.event_bus.subscribe();
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/mission")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "title": "Event contract",
                    "goal": "Capture mission.created shape",
                    "work_items": [{"work_item_id":"w-1","title":"Task"}]
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);
        let create_body = to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let create_payload: Value = serde_json::from_slice(&create_body).expect("json");
        let mission_id = create_payload
            .get("mission")
            .and_then(|v| v.get("mission_id"))
            .and_then(|v| v.as_str())
            .expect("mission_id");

        let event = next_event_of_type(&mut rx, "mission.created").await;
        let snapshot = json!({
            "type": event.event_type,
            "properties": event.properties,
        });
        let expected = json!({
            "type": "mission.created",
            "properties": {
                "missionID": mission_id,
                "workItemCount": 1
            }
        });
        assert_eq!(snapshot, expected);
    }

    #[tokio::test]
    async fn mission_updated_event_contract_snapshot() {
        let state = test_state().await;
        let mut rx = state.event_bus.subscribe();
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/mission")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "title": "Mission update contract",
                    "goal": "Capture mission.updated shape",
                    "work_items": [{"work_item_id":"w-1","title":"Task"}]
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);
        let create_body = to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let create_payload: Value = serde_json::from_slice(&create_body).expect("json");
        let mission_id = create_payload
            .get("mission")
            .and_then(|v| v.get("mission_id"))
            .and_then(|v| v.as_str())
            .expect("mission_id")
            .to_string();

        let apply_req = Request::builder()
            .method("POST")
            .uri(format!("/mission/{mission_id}/event"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "event": {
                        "type": "mission_started",
                        "mission_id": mission_id
                    }
                })
                .to_string(),
            ))
            .expect("apply request");
        let apply_resp = app
            .clone()
            .oneshot(apply_req)
            .await
            .expect("apply response");
        assert_eq!(apply_resp.status(), StatusCode::OK);

        let event = next_event_of_type(&mut rx, "mission.updated").await;
        let snapshot = json!({
            "type": event.event_type,
            "properties": event.properties,
        });
        let expected = json!({
            "type": "mission.updated",
            "properties": {
                "missionID": mission_id,
                "revision": 2,
                "status": "running",
                "commandCount": 0
            }
        });
        assert_eq!(snapshot, expected);
    }

    #[tokio::test]
    async fn mission_apply_event_moves_item_to_rework_on_reviewer_denial() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/mission")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "title": "Gate flow",
                    "goal": "Validate reducer flow",
                    "work_items": [{"work_item_id":"w-1","title":"Patch logic"}]
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        let create_body = to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body");
        let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
        let mission_id = create_payload
            .get("mission")
            .and_then(|v| v.get("mission_id"))
            .and_then(|v| v.as_str())
            .expect("mission id")
            .to_string();

        let run_finished_req = Request::builder()
            .method("POST")
            .uri(format!("/mission/{mission_id}/event"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "event": {
                        "type": "run_finished",
                        "mission_id": mission_id,
                        "work_item_id": "w-1",
                        "run_id": "run-1",
                        "status": "success"
                    }
                })
                .to_string(),
            ))
            .expect("run finished request");
        let run_finished_resp = app
            .clone()
            .oneshot(run_finished_req)
            .await
            .expect("run finished response");
        assert_eq!(run_finished_resp.status(), StatusCode::OK);

        let deny_req = Request::builder()
            .method("POST")
            .uri(format!("/mission/{mission_id}/event"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "event": {
                        "type": "approval_denied",
                        "mission_id": mission_id,
                        "work_item_id": "w-1",
                        "approval_id": "review-1",
                        "reason": "needs revision"
                    }
                })
                .to_string(),
            ))
            .expect("deny request");
        let deny_resp = app.clone().oneshot(deny_req).await.expect("deny response");
        assert_eq!(deny_resp.status(), StatusCode::OK);
        let deny_body = to_bytes(deny_resp.into_body(), usize::MAX)
            .await
            .expect("deny body");
        let deny_payload: Value = serde_json::from_slice(&deny_body).expect("deny json");
        assert_eq!(
            deny_payload
                .get("mission")
                .and_then(|v| v.get("work_items"))
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str()),
            Some("rework")
        );
    }

    #[tokio::test]
    async fn mission_started_triggers_orchestrator_runtime_spawn_for_assigned_agent() {
        let state = test_state().await;
        let workspace_root = state.workspace_index.snapshot().await.root;
        state
            .agent_teams
            .set_for_test(
                Some(workspace_root),
                Some(tandem_orchestrator::SpawnPolicy {
                    enabled: true,
                    require_justification: true,
                    max_agents: Some(20),
                    max_concurrent: Some(10),
                    child_budget_percent_of_parent_remaining: Some(50),
                    spawn_edges: {
                        let mut map = std::collections::HashMap::new();
                        map.insert(
                            tandem_orchestrator::AgentRole::Orchestrator,
                            tandem_orchestrator::RoleSpawnRule {
                                behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                                can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                            },
                        );
                        map
                    },
                    required_skills: std::collections::HashMap::new(),
                    role_defaults: std::collections::HashMap::new(),
                    mission_total_budget: None,
                    cost_per_1k_tokens_usd: None,
                    skill_sources: Default::default(),
                }),
                vec![tandem_orchestrator::AgentTemplate {
                    template_id: "worker-default".to_string(),
                    role: tandem_orchestrator::AgentRole::Worker,
                    system_prompt: Some("You are a worker".to_string()),
                    skills: vec![],
                    default_budget: tandem_orchestrator::BudgetLimit::default(),
                    capabilities: tandem_orchestrator::CapabilitySpec::default(),
                }],
            )
            .await;
        let mut rx = state.event_bus.subscribe();
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/mission")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "title": "Mission with assigned worker",
                    "goal": "exercise orchestrator runtime spawn",
                    "work_items": [{
                        "work_item_id":"w-assign-1",
                        "title":"Ship patch",
                        "assigned_agent":"worker"
                    }]
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);
        let create_body = to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body");
        let create_payload: Value = serde_json::from_slice(&create_body).expect("json");
        let mission_id = create_payload
            .get("mission")
            .and_then(|v| v.get("mission_id"))
            .and_then(|v| v.as_str())
            .expect("mission id")
            .to_string();

        let start_req = Request::builder()
            .method("POST")
            .uri(format!("/mission/{mission_id}/event"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "event": {
                        "type": "mission_started",
                        "mission_id": mission_id
                    }
                })
                .to_string(),
            ))
            .expect("start request");
        let start_resp = app
            .clone()
            .oneshot(start_req)
            .await
            .expect("start response");
        assert_eq!(start_resp.status(), StatusCode::OK);
        let start_body = to_bytes(start_resp.into_body(), usize::MAX)
            .await
            .expect("start body");
        let start_payload: Value = serde_json::from_slice(&start_body).expect("json");
        assert_eq!(
            start_payload
                .get("orchestratorSpawns")
                .and_then(|v| v.as_array())
                .map(|rows| !rows.is_empty()),
            Some(true)
        );
        assert_eq!(
            start_payload
                .get("orchestratorSpawns")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("ok"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );

        let spawn_event = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.expect("event");
                if event.event_type == "agent_team.spawn.approved" {
                    return event;
                }
            }
        })
        .await
        .expect("spawn event timeout");
        assert_eq!(
            spawn_event
                .properties
                .get("source")
                .and_then(|v| v.as_str()),
            Some("orchestrator_runtime")
        );
    }

    #[tokio::test]
    async fn agent_team_provider_usage_event_updates_token_usage() {
        let state = test_state().await;
        let workspace_root = state.workspace_index.snapshot().await.root;
        state
            .agent_teams
            .set_for_test(
                Some(workspace_root),
                Some(tandem_orchestrator::SpawnPolicy {
                    enabled: true,
                    require_justification: true,
                    max_agents: Some(20),
                    max_concurrent: Some(10),
                    child_budget_percent_of_parent_remaining: Some(50),
                    spawn_edges: {
                        let mut map = std::collections::HashMap::new();
                        map.insert(
                            tandem_orchestrator::AgentRole::Orchestrator,
                            tandem_orchestrator::RoleSpawnRule {
                                behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                                can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                            },
                        );
                        map
                    },
                    required_skills: std::collections::HashMap::new(),
                    role_defaults: std::collections::HashMap::new(),
                    mission_total_budget: None,
                    cost_per_1k_tokens_usd: None,
                    skill_sources: Default::default(),
                }),
                vec![tandem_orchestrator::AgentTemplate {
                    template_id: "worker-default".to_string(),
                    role: tandem_orchestrator::AgentRole::Worker,
                    system_prompt: Some("You are a worker".to_string()),
                    skills: vec![],
                    default_budget: tandem_orchestrator::BudgetLimit {
                        max_tokens: Some(10_000),
                        max_steps: None,
                        max_tool_calls: None,
                        max_duration_ms: None,
                        max_cost_usd: None,
                    },
                    capabilities: tandem_orchestrator::CapabilitySpec::default(),
                }],
            )
            .await;
        let app = app_router(state.clone());

        let spawn_req = Request::builder()
            .method("POST")
            .uri("/agent-team/spawn")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "missionID": "m5",
                    "role": "worker",
                    "templateID": "worker-default",
                    "source": "ui_action",
                    "justification": "usage update test"
                })
                .to_string(),
            ))
            .expect("spawn request");
        let spawn_resp = app
            .clone()
            .oneshot(spawn_req)
            .await
            .expect("spawn response");
        assert_eq!(spawn_resp.status(), StatusCode::OK);
        let spawn_body = to_bytes(spawn_resp.into_body(), usize::MAX)
            .await
            .expect("spawn body");
        let spawn_payload: Value = serde_json::from_slice(&spawn_body).expect("json");
        let session_id = spawn_payload
            .get("sessionID")
            .and_then(|v| v.as_str())
            .expect("session id")
            .to_string();

        let usage_event = EngineEvent::new(
            "provider.usage",
            json!({
                "sessionID": session_id,
                "messageID": "msg-1",
                "promptTokens": 12,
                "completionTokens": 34,
                "totalTokens": 46
            }),
        );
        state
            .agent_teams
            .handle_engine_event(&state, &usage_event)
            .await;

        let list_req = Request::builder()
            .method("GET")
            .uri("/agent-team/instances?missionID=m5")
            .body(Body::empty())
            .expect("list request");
        let list_resp = app.oneshot(list_req).await.expect("list response");
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("list body");
        let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
        assert_eq!(
            list_payload
                .get("instances")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("metadata"))
                .and_then(|v| v.get("budgetUsage"))
                .and_then(|v| v.get("tokensUsed"))
                .and_then(|v| v.as_u64()),
            Some(46)
        );
    }

    #[tokio::test]
    async fn agent_team_request_only_spawn_surfaces_in_approvals_endpoint() {
        let state = test_state().await;
        let workspace_root = state.workspace_index.snapshot().await.root;
        state
            .agent_teams
            .set_for_test(
                Some(workspace_root),
                Some(tandem_orchestrator::SpawnPolicy {
                    enabled: true,
                    require_justification: true,
                    max_agents: Some(20),
                    max_concurrent: Some(10),
                    child_budget_percent_of_parent_remaining: Some(50),
                    spawn_edges: {
                        let mut map = std::collections::HashMap::new();
                        map.insert(
                            tandem_orchestrator::AgentRole::Orchestrator,
                            tandem_orchestrator::RoleSpawnRule {
                                behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                                can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                            },
                        );
                        map.insert(
                            tandem_orchestrator::AgentRole::Worker,
                            tandem_orchestrator::RoleSpawnRule {
                                behavior: Some(tandem_orchestrator::SpawnBehavior::RequestOnly),
                                can_spawn: vec![tandem_orchestrator::AgentRole::Tester],
                            },
                        );
                        map
                    },
                    required_skills: std::collections::HashMap::new(),
                    role_defaults: std::collections::HashMap::new(),
                    mission_total_budget: None,
                    cost_per_1k_tokens_usd: None,
                    skill_sources: Default::default(),
                }),
                vec![
                    tandem_orchestrator::AgentTemplate {
                        template_id: "worker-default".to_string(),
                        role: tandem_orchestrator::AgentRole::Worker,
                        system_prompt: None,
                        skills: vec![],
                        default_budget: tandem_orchestrator::BudgetLimit::default(),
                        capabilities: tandem_orchestrator::CapabilitySpec::default(),
                    },
                    tandem_orchestrator::AgentTemplate {
                        template_id: "tester-default".to_string(),
                        role: tandem_orchestrator::AgentRole::Tester,
                        system_prompt: None,
                        skills: vec![],
                        default_budget: tandem_orchestrator::BudgetLimit::default(),
                        capabilities: tandem_orchestrator::CapabilitySpec::default(),
                    },
                ],
            )
            .await;
        let app = app_router(state.clone());

        let spawn_worker_req = Request::builder()
            .method("POST")
            .uri("/agent-team/spawn")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "missionID": "m-approval",
                    "role": "worker",
                    "templateID": "worker-default",
                    "source": "ui_action",
                    "justification": "primary worker"
                })
                .to_string(),
            ))
            .expect("spawn worker");
        let spawn_worker_resp = app
            .clone()
            .oneshot(spawn_worker_req)
            .await
            .expect("spawn worker response");
        assert_eq!(spawn_worker_resp.status(), StatusCode::OK);
        let worker_body = to_bytes(spawn_worker_resp.into_body(), usize::MAX)
            .await
            .expect("worker body");
        let worker_payload: Value = serde_json::from_slice(&worker_body).expect("worker json");
        let worker_instance_id = worker_payload
            .get("instanceID")
            .and_then(|v| v.as_str())
            .expect("worker instance id")
            .to_string();

        let spawn_tester_req = Request::builder()
            .method("POST")
            .uri("/agent-team/spawn")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "missionID": "m-approval",
                    "parentInstanceID": worker_instance_id,
                    "role": "tester",
                    "templateID": "tester-default",
                    "source": "ui_action",
                    "justification": "needs approval edge"
                })
                .to_string(),
            ))
            .expect("spawn tester");
        let spawn_tester_resp = app
            .clone()
            .oneshot(spawn_tester_req)
            .await
            .expect("spawn tester response");
        assert_eq!(spawn_tester_resp.status(), StatusCode::FORBIDDEN);
        let tester_body = to_bytes(spawn_tester_resp.into_body(), usize::MAX)
            .await
            .expect("tester body");
        let tester_payload: Value = serde_json::from_slice(&tester_body).expect("tester json");
        assert_eq!(
            tester_payload
                .get("requiresUserApproval")
                .and_then(|v| v.as_bool()),
            Some(true)
        );

        let approvals_req = Request::builder()
            .method("GET")
            .uri("/agent-team/approvals")
            .body(Body::empty())
            .expect("approvals request");
        let approvals_resp = app
            .oneshot(approvals_req)
            .await
            .expect("approvals response");
        assert_eq!(approvals_resp.status(), StatusCode::OK);
        let approvals_body = to_bytes(approvals_resp.into_body(), usize::MAX)
            .await
            .expect("approvals body");
        let approvals_payload: Value =
            serde_json::from_slice(&approvals_body).expect("approvals json");
        assert_eq!(
            approvals_payload
                .get("spawnApprovals")
                .and_then(|v| v.as_array())
                .map(|v| !v.is_empty()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn mission_total_budget_exhaustion_blocks_followup_spawn() {
        let state = test_state().await;
        let workspace_root = state.workspace_index.snapshot().await.root;
        state
            .agent_teams
            .set_for_test(
                Some(workspace_root),
                Some(tandem_orchestrator::SpawnPolicy {
                    enabled: true,
                    require_justification: true,
                    max_agents: Some(20),
                    max_concurrent: Some(10),
                    child_budget_percent_of_parent_remaining: Some(50),
                    mission_total_budget: Some(tandem_orchestrator::BudgetLimit {
                        max_tokens: Some(40),
                        max_steps: None,
                        max_tool_calls: None,
                        max_duration_ms: None,
                        max_cost_usd: None,
                    }),
                    cost_per_1k_tokens_usd: None,
                    spawn_edges: {
                        let mut map = std::collections::HashMap::new();
                        map.insert(
                            tandem_orchestrator::AgentRole::Orchestrator,
                            tandem_orchestrator::RoleSpawnRule {
                                behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                                can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                            },
                        );
                        map
                    },
                    required_skills: std::collections::HashMap::new(),
                    role_defaults: std::collections::HashMap::new(),
                    skill_sources: Default::default(),
                }),
                vec![tandem_orchestrator::AgentTemplate {
                    template_id: "worker-default".to_string(),
                    role: tandem_orchestrator::AgentRole::Worker,
                    system_prompt: None,
                    skills: vec![],
                    default_budget: tandem_orchestrator::BudgetLimit::default(),
                    capabilities: tandem_orchestrator::CapabilitySpec::default(),
                }],
            )
            .await;
        let app = app_router(state.clone());

        let spawn_1_req = Request::builder()
            .method("POST")
            .uri("/agent-team/spawn")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "missionID": "m-budget",
                    "role": "worker",
                    "templateID": "worker-default",
                    "source": "ui_action",
                    "justification": "initial worker"
                })
                .to_string(),
            ))
            .expect("spawn 1");
        let spawn_1_resp = app
            .clone()
            .oneshot(spawn_1_req)
            .await
            .expect("spawn 1 response");
        assert_eq!(spawn_1_resp.status(), StatusCode::OK);
        let spawn_1_body = to_bytes(spawn_1_resp.into_body(), usize::MAX)
            .await
            .expect("spawn 1 body");
        let spawn_1_payload: Value = serde_json::from_slice(&spawn_1_body).expect("spawn 1 json");
        let session_id = spawn_1_payload
            .get("sessionID")
            .and_then(|v| v.as_str())
            .expect("session id")
            .to_string();

        state
            .agent_teams
            .handle_engine_event(
                &state,
                &EngineEvent::new(
                    "provider.usage",
                    json!({
                        "sessionID": session_id,
                        "totalTokens": 50
                    }),
                ),
            )
            .await;

        let spawn_2_req = Request::builder()
            .method("POST")
            .uri("/agent-team/spawn")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "missionID": "m-budget",
                    "role": "worker",
                    "templateID": "worker-default",
                    "source": "ui_action",
                    "justification": "follow-up worker"
                })
                .to_string(),
            ))
            .expect("spawn 2");
        let spawn_2_resp = app
            .clone()
            .oneshot(spawn_2_req)
            .await
            .expect("spawn 2 response");
        assert_eq!(spawn_2_resp.status(), StatusCode::FORBIDDEN);
        let spawn_2_body = to_bytes(spawn_2_resp.into_body(), usize::MAX)
            .await
            .expect("spawn 2 body");
        let spawn_2_payload: Value = serde_json::from_slice(&spawn_2_body).expect("spawn 2 json");
        assert_eq!(
            spawn_2_payload.get("code").and_then(|v| v.as_str()),
            Some("spawn_mission_budget_exceeded")
        );
    }

    #[tokio::test]
    async fn agent_team_missions_endpoint_returns_rollup_counts() {
        let state = test_state().await;
        let workspace_root = state.workspace_index.snapshot().await.root;
        state
            .agent_teams
            .set_for_test(
                Some(workspace_root),
                Some(tandem_orchestrator::SpawnPolicy {
                    enabled: true,
                    require_justification: true,
                    max_agents: Some(20),
                    max_concurrent: Some(10),
                    child_budget_percent_of_parent_remaining: Some(50),
                    spawn_edges: {
                        let mut map = std::collections::HashMap::new();
                        map.insert(
                            tandem_orchestrator::AgentRole::Orchestrator,
                            tandem_orchestrator::RoleSpawnRule {
                                behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                                can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                            },
                        );
                        map
                    },
                    required_skills: std::collections::HashMap::new(),
                    role_defaults: std::collections::HashMap::new(),
                    mission_total_budget: None,
                    cost_per_1k_tokens_usd: None,
                    skill_sources: Default::default(),
                }),
                vec![tandem_orchestrator::AgentTemplate {
                    template_id: "worker-default".to_string(),
                    role: tandem_orchestrator::AgentRole::Worker,
                    system_prompt: Some("You are a worker".to_string()),
                    skills: vec![],
                    default_budget: tandem_orchestrator::BudgetLimit::default(),
                    capabilities: tandem_orchestrator::CapabilitySpec::default(),
                }],
            )
            .await;
        let app = app_router(state.clone());

        for mission_id in ["m6", "m6", "m7"] {
            let spawn_req = Request::builder()
                .method("POST")
                .uri("/agent-team/spawn")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "missionID": mission_id,
                        "role": "worker",
                        "templateID": "worker-default",
                        "source": "ui_action",
                        "justification": "rollup"
                    })
                    .to_string(),
                ))
                .expect("spawn request");
            let spawn_resp = app
                .clone()
                .oneshot(spawn_req)
                .await
                .expect("spawn response");
            assert_eq!(spawn_resp.status(), StatusCode::OK);
        }

        let req = Request::builder()
            .method("GET")
            .uri("/agent-team/missions")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(payload.get("count").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(
            payload
                .get("missions")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("missionID"))
                .and_then(|v| v.as_str()),
            Some("m6")
        );
        assert_eq!(
            payload
                .get("missions")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("instanceCount"))
                .and_then(|v| v.as_u64()),
            Some(2)
        );
    }

    #[tokio::test]
    async fn mission_canceled_triggers_orchestrator_runtime_instance_cancellation() {
        let state = test_state().await;
        let workspace_root = state.workspace_index.snapshot().await.root;
        state
            .agent_teams
            .set_for_test(
                Some(workspace_root),
                Some(tandem_orchestrator::SpawnPolicy {
                    enabled: true,
                    require_justification: true,
                    max_agents: Some(20),
                    max_concurrent: Some(10),
                    child_budget_percent_of_parent_remaining: Some(50),
                    spawn_edges: {
                        let mut map = std::collections::HashMap::new();
                        map.insert(
                            tandem_orchestrator::AgentRole::Orchestrator,
                            tandem_orchestrator::RoleSpawnRule {
                                behavior: Some(tandem_orchestrator::SpawnBehavior::Allow),
                                can_spawn: vec![tandem_orchestrator::AgentRole::Worker],
                            },
                        );
                        map
                    },
                    required_skills: std::collections::HashMap::new(),
                    role_defaults: std::collections::HashMap::new(),
                    mission_total_budget: None,
                    cost_per_1k_tokens_usd: None,
                    skill_sources: Default::default(),
                }),
                vec![tandem_orchestrator::AgentTemplate {
                    template_id: "worker-default".to_string(),
                    role: tandem_orchestrator::AgentRole::Worker,
                    system_prompt: Some("You are a worker".to_string()),
                    skills: vec![],
                    default_budget: tandem_orchestrator::BudgetLimit::default(),
                    capabilities: tandem_orchestrator::CapabilitySpec::default(),
                }],
            )
            .await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/mission")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "title": "Cancel mission bridge",
                    "goal": "validate cancellation propagation",
                    "work_items": [{
                        "work_item_id":"w-cancel-1",
                        "title":"Do work",
                        "assigned_agent":"worker"
                    }]
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);
        let create_body = to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body");
        let create_payload: Value = serde_json::from_slice(&create_body).expect("json");
        let mission_id = create_payload
            .get("mission")
            .and_then(|v| v.get("mission_id"))
            .and_then(|v| v.as_str())
            .expect("mission id")
            .to_string();

        let start_req = Request::builder()
            .method("POST")
            .uri(format!("/mission/{mission_id}/event"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "event": {
                        "type": "mission_started",
                        "mission_id": mission_id
                    }
                })
                .to_string(),
            ))
            .expect("start request");
        let start_resp = app
            .clone()
            .oneshot(start_req)
            .await
            .expect("start response");
        assert_eq!(start_resp.status(), StatusCode::OK);

        let cancel_req = Request::builder()
            .method("POST")
            .uri(format!("/mission/{mission_id}/event"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "event": {
                        "type": "mission_canceled",
                        "mission_id": mission_id,
                        "reason": "user stop"
                    }
                })
                .to_string(),
            ))
            .expect("cancel request");
        let cancel_resp = app
            .clone()
            .oneshot(cancel_req)
            .await
            .expect("cancel response");
        assert_eq!(cancel_resp.status(), StatusCode::OK);
        let cancel_body = to_bytes(cancel_resp.into_body(), usize::MAX)
            .await
            .expect("cancel body");
        let cancel_payload: Value = serde_json::from_slice(&cancel_body).expect("json");
        assert_eq!(
            cancel_payload
                .get("orchestratorCancellations")
                .and_then(|v| v.get("triggered"))
                .and_then(|v| v.as_bool()),
            Some(true)
        );
        assert_eq!(
            cancel_payload
                .get("orchestratorCancellations")
                .and_then(|v| v.get("cancelledInstances"))
                .and_then(|v| v.as_u64()),
            Some(1)
        );

        let instances_req = Request::builder()
            .method("GET")
            .uri(format!("/agent-team/instances?missionID={mission_id}"))
            .body(Body::empty())
            .expect("instances request");
        let instances_resp = app
            .oneshot(instances_req)
            .await
            .expect("instances response");
        assert_eq!(instances_resp.status(), StatusCode::OK);
        let instances_body = to_bytes(instances_resp.into_body(), usize::MAX)
            .await
            .expect("instances body");
        let instances_payload: Value = serde_json::from_slice(&instances_body).expect("json");
        assert_eq!(
            instances_payload
                .get("instances")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str()),
            Some("cancelled")
        );
    }

    #[tokio::test]
    async fn routines_create_run_now_and_history_roundtrip() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/routines")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "routine_id": "routine-1",
                    "name": "Daily digest",
                    "schedule": { "interval_seconds": { "seconds": 60 } },
                    "entrypoint": "mission.default",
                    "creator_type": "user",
                    "creator_id": "u-1"
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);

        let run_now_req = Request::builder()
            .method("POST")
            .uri("/routines/routine-1/run_now")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_count": 2,
                    "reason": "manual smoke check"
                })
                .to_string(),
            ))
            .expect("run_now request");
        let run_now_resp = app
            .clone()
            .oneshot(run_now_req)
            .await
            .expect("run_now response");
        assert_eq!(run_now_resp.status(), StatusCode::OK);

        let history_req = Request::builder()
            .method("GET")
            .uri("/routines/routine-1/history?limit=10")
            .body(Body::empty())
            .expect("history request");
        let history_resp = app
            .clone()
            .oneshot(history_req)
            .await
            .expect("history response");
        assert_eq!(history_resp.status(), StatusCode::OK);
        let history_body = to_bytes(history_resp.into_body(), usize::MAX)
            .await
            .expect("history body");
        let history_payload: Value = serde_json::from_slice(&history_body).expect("history json");
        assert_eq!(
            history_payload.get("count").and_then(|v| v.as_u64()),
            Some(1)
        );
        assert_eq!(
            history_payload
                .get("events")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("run_count"))
                .and_then(|v| v.as_u64()),
            Some(2)
        );
    }

    #[tokio::test]
    async fn routines_patch_can_pause_routine() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/routines")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "routine_id": "routine-2",
                    "name": "Research routine",
                    "schedule": { "interval_seconds": { "seconds": 120 } },
                    "entrypoint": "mission.default"
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);

        let patch_req = Request::builder()
            .method("PATCH")
            .uri("/routines/routine-2")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "status": "paused"
                })
                .to_string(),
            ))
            .expect("patch request");
        let patch_resp = app
            .clone()
            .oneshot(patch_req)
            .await
            .expect("patch response");
        assert_eq!(patch_resp.status(), StatusCode::OK);
        let patch_body = to_bytes(patch_resp.into_body(), usize::MAX)
            .await
            .expect("patch body");
        let patch_payload: Value = serde_json::from_slice(&patch_body).expect("patch json");
        assert_eq!(
            patch_payload
                .get("routine")
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str()),
            Some("paused")
        );
    }

    #[tokio::test]
    async fn routines_run_now_blocks_external_side_effects_by_default() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/routines")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "routine_id": "routine-ext-blocked",
                    "name": "External email sender",
                    "schedule": { "interval_seconds": { "seconds": 300 } },
                    "entrypoint": "connector.email.reply",
                    "requires_approval": true,
                    "external_integrations_allowed": false
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);

        let run_now_req = Request::builder()
            .method("POST")
            .uri("/routines/routine-ext-blocked/run_now")
            .header("content-type", "application/json")
            .body(Body::from(json!({}).to_string()))
            .expect("run_now request");
        let run_now_resp = app
            .clone()
            .oneshot(run_now_req)
            .await
            .expect("run_now response");
        assert_eq!(run_now_resp.status(), StatusCode::FORBIDDEN);

        let history_req = Request::builder()
            .method("GET")
            .uri("/routines/routine-ext-blocked/history?limit=5")
            .body(Body::empty())
            .expect("history request");
        let history_resp = app
            .clone()
            .oneshot(history_req)
            .await
            .expect("history response");
        assert_eq!(history_resp.status(), StatusCode::OK);
        let history_body = to_bytes(history_resp.into_body(), usize::MAX)
            .await
            .expect("history body");
        let history_payload: Value = serde_json::from_slice(&history_body).expect("history json");
        assert_eq!(
            history_payload
                .get("events")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str()),
            Some("blocked_policy")
        );
    }

    #[tokio::test]
    async fn routines_run_now_requires_approval_for_external_side_effects_when_enabled() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/routines")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "routine_id": "routine-ext-approval",
                    "name": "External draft workflow",
                    "schedule": { "interval_seconds": { "seconds": 300 } },
                    "entrypoint": "connector.email.reply",
                    "requires_approval": true,
                    "external_integrations_allowed": true
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);

        let run_now_req = Request::builder()
            .method("POST")
            .uri("/routines/routine-ext-approval/run_now")
            .header("content-type", "application/json")
            .body(Body::from(json!({}).to_string()))
            .expect("run_now request");
        let run_now_resp = app
            .clone()
            .oneshot(run_now_req)
            .await
            .expect("run_now response");
        assert_eq!(run_now_resp.status(), StatusCode::OK);
        let run_now_body = to_bytes(run_now_resp.into_body(), usize::MAX)
            .await
            .expect("run_now body");
        let run_now_payload: Value = serde_json::from_slice(&run_now_body).expect("run_now json");
        assert_eq!(
            run_now_payload.get("status").and_then(|v| v.as_str()),
            Some("pending_approval")
        );

        let history_req = Request::builder()
            .method("GET")
            .uri("/routines/routine-ext-approval/history?limit=5")
            .body(Body::empty())
            .expect("history request");
        let history_resp = app
            .clone()
            .oneshot(history_req)
            .await
            .expect("history response");
        assert_eq!(history_resp.status(), StatusCode::OK);
        let history_body = to_bytes(history_resp.into_body(), usize::MAX)
            .await
            .expect("history body");
        let history_payload: Value = serde_json::from_slice(&history_body).expect("history json");
        assert_eq!(
            history_payload
                .get("events")
                .and_then(|v| v.get(0))
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str()),
            Some("pending_approval")
        );
    }

    #[tokio::test]
    async fn routine_fired_event_contract_snapshot() {
        let state = test_state().await;
        let mut rx = state.event_bus.subscribe();
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/routines")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "routine_id": "routine-fired-contract",
                    "name": "Routine fired contract",
                    "schedule": { "interval_seconds": { "seconds": 300 } },
                    "entrypoint": "mission.default"
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);

        let run_now_req = Request::builder()
            .method("POST")
            .uri("/routines/routine-fired-contract/run_now")
            .header("content-type", "application/json")
            .body(Body::from(json!({ "run_count": 2 }).to_string()))
            .expect("run now request");
        let run_now_resp = app
            .clone()
            .oneshot(run_now_req)
            .await
            .expect("run now response");
        assert_eq!(run_now_resp.status(), StatusCode::OK);

        let event = next_event_of_type(&mut rx, "routine.fired").await;
        let mut properties = event
            .properties
            .as_object()
            .cloned()
            .expect("properties object");
        let fired_at_ms = properties
            .remove("firedAtMs")
            .and_then(|v| v.as_u64())
            .expect("firedAtMs");
        assert!(fired_at_ms > 0);

        let snapshot = json!({
            "type": event.event_type,
            "properties": properties,
        });
        let expected = json!({
            "type": "routine.fired",
            "properties": {
                "routineID": "routine-fired-contract",
                "runCount": 2,
                "triggerType": "manual"
            }
        });
        assert_eq!(snapshot, expected);
    }

    #[tokio::test]
    async fn routine_approval_required_event_contract_snapshot() {
        let state = test_state().await;
        let mut rx = state.event_bus.subscribe();
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/routines")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "routine_id": "routine-approval-contract",
                    "name": "Routine approval contract",
                    "schedule": { "interval_seconds": { "seconds": 300 } },
                    "entrypoint": "connector.email.reply",
                    "requires_approval": true,
                    "external_integrations_allowed": true
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);

        let run_now_req = Request::builder()
            .method("POST")
            .uri("/routines/routine-approval-contract/run_now")
            .header("content-type", "application/json")
            .body(Body::from(json!({}).to_string()))
            .expect("run now request");
        let run_now_resp = app
            .clone()
            .oneshot(run_now_req)
            .await
            .expect("run now response");
        assert_eq!(run_now_resp.status(), StatusCode::OK);

        let event = next_event_of_type(&mut rx, "routine.approval_required").await;
        let snapshot = json!({
            "type": event.event_type,
            "properties": event.properties,
        });
        let expected = json!({
            "type": "routine.approval_required",
            "properties": {
                "routineID": "routine-approval-contract",
                "runCount": 1,
                "triggerType": "manual",
                "reason": "manual approval required before external side effects (manual)"
            }
        });
        assert_eq!(snapshot, expected);
    }

    #[tokio::test]
    async fn routine_blocked_event_contract_snapshot() {
        let state = test_state().await;
        let mut rx = state.event_bus.subscribe();
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/routines")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "routine_id": "routine-blocked-contract",
                    "name": "Routine blocked contract",
                    "schedule": { "interval_seconds": { "seconds": 300 } },
                    "entrypoint": "connector.email.reply",
                    "requires_approval": true,
                    "external_integrations_allowed": false
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);

        let run_now_req = Request::builder()
            .method("POST")
            .uri("/routines/routine-blocked-contract/run_now")
            .header("content-type", "application/json")
            .body(Body::from(json!({}).to_string()))
            .expect("run now request");
        let run_now_resp = app
            .clone()
            .oneshot(run_now_req)
            .await
            .expect("run now response");
        assert_eq!(run_now_resp.status(), StatusCode::FORBIDDEN);

        let event = next_event_of_type(&mut rx, "routine.blocked").await;
        let snapshot = json!({
            "type": event.event_type,
            "properties": event.properties,
        });
        let expected = json!({
            "type": "routine.blocked",
            "properties": {
                "routineID": "routine-blocked-contract",
                "runCount": 1,
                "triggerType": "manual",
                "reason": "external integrations are disabled by policy"
            }
        });
        assert_eq!(snapshot, expected);
    }

    #[tokio::test]
    async fn memory_put_enforces_default_write_scope() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let req = Request::builder()
            .method("POST")
            .uri("/memory/put")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "run-1",
                    "partition": {
                        "org_id": "org-1",
                        "workspace_id": "ws-1",
                        "project_id": "proj-1",
                        "tier": "project"
                    },
                    "kind": "note",
                    "content": "should fail without write scope",
                    "classification": "internal"
                })
                .to_string(),
            ))
            .expect("request");

        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn memory_put_then_search_in_session_scope() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let put_req = Request::builder()
            .method("POST")
            .uri("/memory/put")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "run-2",
                    "partition": {
                        "org_id": "org-1",
                        "workspace_id": "ws-1",
                        "project_id": "proj-1",
                        "tier": "session"
                    },
                    "kind": "solution_capsule",
                    "content": "retry budget extension pattern",
                    "classification": "internal",
                    "artifact_refs": ["artifact://run-2/task-1/patch.diff"]
                })
                .to_string(),
            ))
            .expect("put request");
        let put_resp = app.clone().oneshot(put_req).await.expect("response");
        assert_eq!(put_resp.status(), StatusCode::OK);

        let search_req = Request::builder()
            .method("POST")
            .uri("/memory/search")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "run-2",
                    "query": "budget extension",
                    "read_scopes": ["session"],
                    "partition": {
                        "org_id": "org-1",
                        "workspace_id": "ws-1",
                        "project_id": "proj-1",
                        "tier": "session"
                    },
                    "limit": 5
                })
                .to_string(),
            ))
            .expect("search request");
        let search_resp = app.oneshot(search_req).await.expect("response");
        assert_eq!(search_resp.status(), StatusCode::OK);
        let body = to_bytes(search_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        let result_count = payload
            .get("results")
            .and_then(|v| v.as_array())
            .map(|v| v.len())
            .unwrap_or(0);
        assert!(result_count >= 1);
    }

    #[tokio::test]
    async fn memory_promote_blocks_sensitive_content_and_emits_audit() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let capability = json!({
            "run_id": "run-3",
            "subject": "reviewer-user",
            "org_id": "org-1",
            "workspace_id": "ws-1",
            "project_id": "proj-1",
            "memory": {
                "read_tiers": ["session", "project"],
                "write_tiers": ["session"],
                "promote_targets": ["project"],
                "require_review_for_promote": true,
                "allow_auto_use_tiers": ["curated"]
            },
            "expires_at": 9999999999999u64
        });

        let put_req = Request::builder()
            .method("POST")
            .uri("/memory/put")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "run-3",
                    "partition": {
                        "org_id": "org-1",
                        "workspace_id": "ws-1",
                        "project_id": "proj-1",
                        "tier": "session"
                    },
                    "kind": "solution_capsule",
                    "content": "-----BEGIN PRIVATE KEY-----",
                    "classification": "restricted",
                    "capability": capability
                })
                .to_string(),
            ))
            .expect("put request");
        let put_resp = app.clone().oneshot(put_req).await.expect("put response");
        assert_eq!(put_resp.status(), StatusCode::OK);
        let put_body = to_bytes(put_resp.into_body(), usize::MAX)
            .await
            .expect("put body");
        let put_payload: Value = serde_json::from_slice(&put_body).expect("put json");
        let memory_id = put_payload
            .get("id")
            .and_then(|v| v.as_str())
            .expect("memory id")
            .to_string();

        let promote_req = Request::builder()
            .method("POST")
            .uri("/memory/promote")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "run-3",
                    "source_memory_id": memory_id,
                    "from_tier": "session",
                    "to_tier": "project",
                    "partition": {
                        "org_id": "org-1",
                        "workspace_id": "ws-1",
                        "project_id": "proj-1",
                        "tier": "session"
                    },
                    "reason": "promote test",
                    "review": {
                        "required": true,
                        "reviewer_id": "user-1",
                        "approval_id": "appr-1"
                    },
                    "capability": capability
                })
                .to_string(),
            ))
            .expect("promote request");
        let promote_resp = app
            .clone()
            .oneshot(promote_req)
            .await
            .expect("promote response");
        assert_eq!(promote_resp.status(), StatusCode::OK);
        let promote_body = to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body");
        let promote_payload: Value = serde_json::from_slice(&promote_body).expect("promote json");
        assert_eq!(
            promote_payload.get("promoted").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert_eq!(
            promote_payload
                .get("scrub_report")
                .and_then(|v| v.get("status"))
                .and_then(|v| v.as_str()),
            Some("blocked")
        );

        let audit_req = Request::builder()
            .method("GET")
            .uri("/memory/audit?run_id=run-3")
            .body(Body::empty())
            .expect("audit request");
        let audit_resp = app
            .clone()
            .oneshot(audit_req)
            .await
            .expect("audit response");
        assert_eq!(audit_resp.status(), StatusCode::OK);
        let audit_body = to_bytes(audit_resp.into_body(), usize::MAX)
            .await
            .expect("audit body");
        let audit_payload: Value = serde_json::from_slice(&audit_body).expect("audit json");
        let blocked_promote_exists = audit_payload
            .get("events")
            .and_then(|v| v.as_array())
            .map(|events| {
                events.iter().any(|event| {
                    event.get("action").and_then(|v| v.as_str()) == Some("memory_promote")
                        && event.get("status").and_then(|v| v.as_str()) == Some("blocked")
                })
            })
            .unwrap_or(false);
        assert!(blocked_promote_exists);
    }

    #[tokio::test]
    async fn memory_list_and_delete_admin_routes_work() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let put_req = Request::builder()
            .method("POST")
            .uri("/memory/put")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "run-4",
                    "partition": {
                        "org_id": "org-1",
                        "workspace_id": "ws-1",
                        "project_id": "proj-1",
                        "tier": "session"
                    },
                    "kind": "fact",
                    "content": "admin memory test",
                    "artifact_refs": [],
                    "classification": "internal",
                    "metadata": null
                })
                .to_string(),
            ))
            .expect("memory put request");
        let put_resp = app
            .clone()
            .oneshot(put_req)
            .await
            .expect("memory put response");
        assert_eq!(put_resp.status(), StatusCode::OK);
        let put_body = to_bytes(put_resp.into_body(), usize::MAX)
            .await
            .expect("memory put body");
        let put_payload: Value = serde_json::from_slice(&put_body).expect("memory put json");
        let memory_id = put_payload
            .get("id")
            .and_then(|v| v.as_str())
            .expect("memory id")
            .to_string();

        let list_req = Request::builder()
            .method("GET")
            .uri("/memory?limit=20")
            .body(Body::empty())
            .expect("memory list request");
        let list_resp = app
            .clone()
            .oneshot(list_req)
            .await
            .expect("memory list response");
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("memory list body");
        let list_payload: Value = serde_json::from_slice(&list_body).expect("memory list json");
        let contains = list_payload
            .get("items")
            .and_then(|v| v.as_array())
            .map(|rows| {
                rows.iter()
                    .any(|row| row.get("id").and_then(|v| v.as_str()) == Some(memory_id.as_str()))
            })
            .unwrap_or(false);
        assert!(contains);

        let del_req = Request::builder()
            .method("DELETE")
            .uri(format!("/memory/{memory_id}"))
            .body(Body::empty())
            .expect("memory delete request");
        let del_resp = app
            .clone()
            .oneshot(del_req)
            .await
            .expect("memory delete response");
        assert_eq!(del_resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn admin_and_channel_routes_require_auth_when_api_token_enabled() {
        let state = test_state().await;
        state.set_api_token(Some("tk_test".to_string())).await;
        let app = app_router(state);

        for (method, uri) in [
            ("GET", "/channels/config"),
            ("GET", "/channels/status"),
            ("POST", "/admin/reload-config"),
            ("GET", "/memory"),
        ] {
            let req = Request::builder()
                .method(method)
                .uri(uri)
                .body(Body::empty())
                .expect("request");
            let resp = app.clone().oneshot(req).await.expect("response");
            assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
        }
    }

    #[tokio::test]
    async fn channels_config_returns_non_secret_shape() {
        let state = test_state().await;
        let _ = state
            .config
            .patch_project(json!({
                "channels": {
                    "telegram": {
                        "bot_token": "tg-secret",
                        "allowed_users": ["@alice", "@bob"],
                        "mention_only": true
                    },
                    "discord": {
                        "bot_token": "dc-secret",
                        "allowed_users": ["*"],
                        "mention_only": false,
                        "guild_id": "1234"
                    },
                    "slack": {
                        "bot_token": "sl-secret",
                        "channel_id": "C123",
                        "allowed_users": ["U1"]
                    }
                }
            }))
            .await
            .expect("patch project");
        let app = app_router(state);

        let req = Request::builder()
            .method("GET")
            .uri("/channels/config")
            .body(Body::empty())
            .expect("request");
        let resp = app.clone().oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload
                .get("telegram")
                .and_then(|v| v.get("has_token"))
                .and_then(Value::as_bool),
            Some(true)
        );
        assert!(payload
            .get("telegram")
            .and_then(Value::as_object)
            .is_some_and(|obj| !obj.contains_key("bot_token")));
        assert!(payload
            .get("discord")
            .and_then(Value::as_object)
            .is_some_and(|obj| !obj.contains_key("bot_token")));
        assert!(payload
            .get("slack")
            .and_then(Value::as_object)
            .is_some_and(|obj| !obj.contains_key("bot_token")));
    }

    #[tokio::test]
    async fn get_config_redacts_channel_bot_token() {
        let state = test_state().await;
        let _ = state
            .config
            .patch_project(json!({
                "channels": {
                    "telegram": {
                        "bot_token": "tg-secret",
                        "allowed_users": ["*"],
                        "mention_only": false
                    }
                }
            }))
            .await
            .expect("patch project");
        let app = app_router(state);

        let req = Request::builder()
            .method("GET")
            .uri("/config")
            .body(Body::empty())
            .expect("request");
        let resp = app.clone().oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload
                .get("effective")
                .and_then(|v| v.get("channels"))
                .and_then(|v| v.get("telegram"))
                .and_then(|v| v.get("bot_token"))
                .and_then(Value::as_str),
            Some("[REDACTED]")
        );
    }
}
