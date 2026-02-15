use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::header::{self, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
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
use tandem_skills::{SkillLocation, SkillService, SkillsConflictPolicy};
use tokio::process::Command;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use tandem_types::{
    CreateSessionRequest, EngineEvent, Message, MessagePart, MessagePartInput, MessageRole,
    SendMessageRequest, Session, TodoItem,
};
use tandem_wire::{
    WireProviderCatalog, WireProviderEntry, WireProviderModel, WireProviderModelLimit, WireSession,
    WireSessionMessage,
};

use crate::{ActiveRun, AppState, StartupStatus};

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

    let listener = tokio::net::TcpListener::bind(addr).await?;
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            if tokio::signal::ctrl_c().await.is_err() {
                futures::future::pending::<()>().await;
            }
        })
        .await;
    reaper.abort();
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
    Router::new()
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
        .route("/skill", get(skill_list))
        .route("/instance/dispose", post(instance_dispose))
        .route("/log", post(push_log))
        .route("/doc", get(openapi_doc))
        .layer(middleware::from_fn_with_state(state.clone(), startup_gate))
        .with_state(state)
}

async fn startup_gate(State(state): State<AppState>, request: Request, next: Next) -> Response {
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
    Json(json!({
        "healthy": true,
        "ready": state.is_ready(),
        "phase": startup.phase,
        "startup_attempt_id": startup.attempt_id,
        "startup_elapsed_ms": startup.elapsed_ms,
        "last_error": startup.last_error,
        "version": env!("CARGO_PKG_VERSION"),
        "build_id": build_id,
        "binary_path": binary_path,
        "mode": state.mode_label(),
        "leaseCount": lease_count
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
    if message.contains("invalid_function_parameters")
        || message.contains("array schema missing items")
    {
        "TOOL_SCHEMA_INVALID"
    } else {
        "ENGINE_DISPATCH_FAILED"
    }
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
            let cancelled = state.cancellations.cancel(&id).await;
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
            return Json(json!({"ok": true, "cancelled": cancelled || true}));
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
    if !wire.connected.iter().any(|id| id == provider_id) {
        wire.connected.push(provider_id.to_string());
    }

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

    let Some(data) = body.get("data").and_then(|v| v.as_array()) else {
        return None;
    };

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
async fn get_config(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "effective": state.config.get_effective_value().await,
        "layers": state.config.get_layers_value().await
    }))
}
async fn patch_config(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let effective = state
        .config
        .patch_project(input)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    Ok(Json(json!({ "effective": effective })))
}
async fn global_config(State(state): State<AppState>) -> Json<Value> {
    Json(json!({
        "global": state.config.get_global_value().await,
        "effective": state.config.get_effective_value().await
    }))
}
async fn global_config_patch(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let effective = state
        .config
        .patch_global(input)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    Ok(Json(json!({ "effective": effective })))
}
async fn config_providers(State(state): State<AppState>) -> Json<Value> {
    let cfg = state.config.get_effective_value().await;
    let providers = cfg.get("providers").cloned().unwrap_or_else(|| json!({}));
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
    let token = input.token.unwrap_or_default();
    state.auth.write().await.insert(id.clone(), token);
    Json(json!({"ok": true, "id": id}))
}
async fn delete_auth(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    let removed = state.auth.write().await.remove(&id).is_some();
    Json(json!({"ok": removed}))
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
        let engine_loop = EngineLoop::new(
            storage.clone(),
            event_bus.clone(),
            providers.clone(),
            plugins.clone(),
            agents.clone(),
            permissions.clone(),
            tools.clone(),
            cancellations.clone(),
        );
        let state = AppState::new_starting(Uuid::new_v4().to_string(), false);
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
            })
            .await
            .expect("runtime ready");
        state
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
}
