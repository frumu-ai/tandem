use serde::Deserialize;
use serde_json::Value;
use std::time::{Duration, UNIX_EPOCH};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct BrowserSmokeTestInput {
    #[serde(default)]
    url: Option<String>,
}

pub(super) async fn global_health(State(state): State<AppState>) -> impl IntoResponse {
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
    let browser = state.browser_health_summary().await;
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
        "environment": environment,
        "browser": browser
    }))
}

pub(super) async fn browser_status(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!(state.browser_status().await))
}

pub(super) async fn browser_install(State(state): State<AppState>) -> impl IntoResponse {
    match state.install_browser_sidecar().await {
        Ok(result) => (StatusCode::OK, Json(json!(result))).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "ok": false,
                "code": "browser_install_failed",
                "error": err.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn browser_smoke_test(
    State(state): State<AppState>,
    payload: Option<Json<BrowserSmokeTestInput>>,
) -> impl IntoResponse {
    let input = payload
        .map(|Json(value)| value)
        .unwrap_or(BrowserSmokeTestInput { url: None });
    match state.browser_smoke_test(input.url).await {
        Ok(result) => (StatusCode::OK, Json(json!(result))).into_response(),
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "ok": false,
                "code": "browser_smoke_test_failed",
                "error": err.to_string(),
            })),
        )
            .into_response(),
    }
}

pub(super) async fn global_lease_acquire(
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

pub(super) async fn global_lease_renew(
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

pub(super) async fn global_lease_release(
    State(state): State<AppState>,
    Json(input): Json<EngineLeaseReleaseInput>,
) -> Json<Value> {
    let now = crate::now_ms();
    let mut leases = state.engine_leases.write().await;
    leases.retain(|_, l| !l.is_expired(now));
    let removed = leases.remove(&input.lease_id).is_some();
    Json(json!({ "ok": removed, "lease_count": leases.len() }))
}

pub(super) async fn global_storage_repair(
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

fn resolve_storage_list_root() -> PathBuf {
    if let Ok(root) = std::env::var("TANDEM_HOME") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(root) = std::env::var("TANDEM_STATE_DIR") {
        let trimmed = root.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    if let Ok(paths) = tandem_core::resolve_shared_paths() {
        return paths.canonical_root;
    }
    dirs::home_dir()
        .map(|home| home.join(".tandem"))
        .unwrap_or_else(|| PathBuf::from(".tandem"))
}

pub(crate) fn sanitize_relative_subpath(raw: Option<&str>) -> Result<PathBuf, StatusCode> {
    let Some(raw) = raw else {
        return Ok(PathBuf::new());
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(PathBuf::new());
    }
    let candidate = PathBuf::from(trimmed);
    if candidate.is_absolute() {
        return Err(StatusCode::BAD_REQUEST);
    }
    if candidate.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(candidate)
}

pub(super) async fn global_storage_files(
    Query(query): Query<StorageFilesQuery>,
) -> Result<Json<Value>, StatusCode> {
    let root = resolve_storage_list_root();
    let rel = sanitize_relative_subpath(query.path.as_deref())?;
    let base = if rel.as_os_str().is_empty() {
        root.clone()
    } else {
        root.join(&rel)
    };

    if !base.exists() {
        return Err(StatusCode::NOT_FOUND);
    }
    if !base.is_dir() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let limit = query.limit.unwrap_or(500).clamp(1, 5_000);
    let mut files = Vec::new();

    for entry in ignore::WalkBuilder::new(&base).build().flatten() {
        if !entry.file_type().map(|f| f.is_file()).unwrap_or(false) {
            continue;
        }
        let abs = entry.path().to_path_buf();
        let rel_to_root = abs
            .strip_prefix(&root)
            .unwrap_or(&abs)
            .to_string_lossy()
            .replace('\\', "/");
        let rel_to_base = abs
            .strip_prefix(&base)
            .unwrap_or(&abs)
            .to_string_lossy()
            .replace('\\', "/");
        let meta = std::fs::metadata(&abs).ok();
        let size_bytes = meta.as_ref().map(|m| m.len()).unwrap_or(0);
        let modified_at_ms = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64);
        files.push(json!({
            "path": rel_to_root,
            "relative_to_base": rel_to_base,
            "size_bytes": size_bytes,
            "modified_at_ms": modified_at_ms,
        }));
        if files.len() >= limit {
            break;
        }
    }

    Ok(Json(json!({
        "root": root.to_string_lossy(),
        "base": base.to_string_lossy(),
        "count": files.len(),
        "limit": limit,
        "files": files,
    })))
}

fn sse_stream(
    state: AppState,
    filter: EventFilterQuery,
) -> impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>
{
    let rx = state.event_bus.subscribe();
    let initial = tokio_stream::once(Ok(axum::response::sse::Event::default().data(
        serde_json::to_string(&EngineEvent::new("server.connected", json!({}))).unwrap_or_default(),
    )));
    let ready = tokio_stream::once(Ok(axum::response::sse::Event::default().data(
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
            Some(Ok(axum::response::sse::Event::default().data(payload)))
        }
        Err(_) => None,
    });
    initial.chain(ready).chain(live)
}

pub(super) async fn events(
    State(state): State<AppState>,
    Query(filter): Query<EventFilterQuery>,
) -> axum::response::Sse<
    impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    axum::response::Sse::new(sse_stream(state, filter))
        .keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(10)))
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

pub(super) async fn global_dispose(State(state): State<AppState>) -> Json<Value> {
    let cancelled = state.cancellations.cancel_all().await;
    let closed_browser_sessions = state.close_all_browser_sessions().await;
    Json(json!({
        "ok": true,
        "cancelledSessions": cancelled,
        "closedBrowserSessions": closed_browser_sessions,
    }))
}

pub(super) async fn tool_ids(State(state): State<AppState>) -> Json<Value> {
    let ids = state
        .tools
        .list()
        .await
        .into_iter()
        .map(|t| t.name)
        .collect::<Vec<_>>();
    Json(json!(ids))
}

pub(super) async fn tool_list_for_model(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.tools.list().await))
}

#[derive(Debug, Deserialize)]
pub(super) struct ToolExecutionInput {
    pub tool: String,
    pub args: Option<Value>,
}

pub(super) async fn execute_tool(
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

pub(super) async fn create_worktree(
    Json(input): Json<WorktreeInput>,
) -> Result<Json<Value>, StatusCode> {
    let path = input.path.unwrap_or_else(|| "worktree-temp".to_string());
    let branch = input
        .branch
        .unwrap_or_else(|| format!("wt-{}", chrono::Utc::now().timestamp()));
    let base = input.base.unwrap_or_else(|| "HEAD".to_string());
    let output = std::process::Command::new("git")
        .args(["worktree", "add", "-b", &branch, &path, &base])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "ok": output.status.success(),
        "path": path,
        "branch": branch,
        "stderr": String::from_utf8_lossy(&output.stderr).to_string()
    })))
}

pub(super) async fn list_worktrees() -> Json<Value> {
    let output = std::process::Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .output()
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

pub(super) async fn delete_worktree(
    Json(input): Json<WorktreeInput>,
) -> Result<Json<Value>, StatusCode> {
    let Some(path) = input.path else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let output = std::process::Command::new("git")
        .args(["worktree", "remove", "--force", &path])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "ok": output.status.success(),
        "path": path,
        "stderr": String::from_utf8_lossy(&output.stderr).to_string()
    })))
}

pub(super) async fn reset_worktree(
    Json(input): Json<WorktreeInput>,
) -> Result<Json<Value>, StatusCode> {
    let Some(path) = input.path else {
        return Err(StatusCode::BAD_REQUEST);
    };
    let target = input.base.unwrap_or_else(|| "HEAD".to_string());
    let output = std::process::Command::new("git")
        .args(["-C", &path, "reset", "--hard", &target])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({
        "ok": output.status.success(),
        "path": path,
        "target": target,
        "stderr": String::from_utf8_lossy(&output.stderr).to_string()
    })))
}

pub(super) async fn agent_list(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.agents.list().await))
}

pub(super) async fn openapi_doc() -> Json<Value> {
    Json(json!({
        "openapi":"3.1.0",
        "info":{"title":"tandem-engine","version":"0.1.0"},
        "paths":{
            "/global/health":{"get":{"summary":"Health check"}},
            "/global/storage/files":{"get":{"summary":"List files under the engine storage directory"}},
            "/global/storage/repair":{"post":{"summary":"Force legacy storage repair scan"}},
            "/session":{"get":{"summary":"List sessions"},"post":{"summary":"Create session"}},
            "/session/{id}/message":{"post":{"summary":"Append message"}},
            "/session/{id}/prompt_async":{"post":{"summary":"Start async prompt run"}},
            "/session/{id}/prompt_sync":{"post":{"summary":"Start sync prompt run"}},
            "/session/{id}/run":{"get":{"summary":"Get active run"}},
            "/session/{id}/cancel":{"post":{"summary":"Cancel active run"}},
            "/session/{id}/run/{run_id}/cancel":{"post":{"summary":"Cancel run by id"}},
            "/event":{"get":{"summary":"SSE event stream"}},
            "/run/{id}/events":{"get":{"summary":"SSE stream for sequenced run events"}},
            "/context/runs":{"get":{"summary":"List context runs"},"post":{"summary":"Create context run"}},
            "/context/runs/events/stream":{"get":{"summary":"Multiplex SSE stream for context run events and blackboard patches"}},
            "/context/runs/{run_id}":{"get":{"summary":"Get context run state"},"put":{"summary":"Update context run state"}},
            "/context/runs/{run_id}/events":{"get":{"summary":"List context run events"},"post":{"summary":"Append context run event"}},
            "/context/runs/{run_id}/todos/sync":{"post":{"summary":"Sync todo list into context run steps"}},
            "/context/runs/{run_id}/events/stream":{"get":{"summary":"SSE stream for context run events"}},
            "/context/runs/{run_id}/lease/validate":{"post":{"summary":"Validate workspace lease and auto-pause on mismatch"}},
            "/context/runs/{run_id}/blackboard":{"get":{"summary":"Get materialized context blackboard"}},
            "/context/runs/{run_id}/blackboard/patches":{"post":{"summary":"Append context blackboard patch"}},
            "/context/runs/{run_id}/checkpoints":{"post":{"summary":"Create context run checkpoint"}},
            "/context/runs/{run_id}/checkpoints/latest":{"get":{"summary":"Get latest context run checkpoint"}},
            "/context/runs/{run_id}/replay":{"get":{"summary":"Replay context run from events/checkpoint and report drift"}},
            "/context/runs/{run_id}/driver/next":{"post":{"summary":"Select next context step using engine meta-manager state rules"}},
            "/provider":{"get":{"summary":"List providers"}},
            "/session/{id}/fork":{"post":{"summary":"Fork a session"}},
            "/worktree":{"get":{"summary":"List worktrees"},"post":{"summary":"Create worktree"},"delete":{"summary":"Delete worktree"}},
            "/mcp/catalog":{"get":{"summary":"List embedded MCP remote-pack catalog"}},
            "/mcp/catalog/{slug}/toml":{"get":{"summary":"Get embedded MCP remote-pack TOML by slug"}},
            "/mcp/resources":{"get":{"summary":"List MCP resources"}},
            "/tool":{"get":{"summary":"List tools"}},
            "/skills":{"get":{"summary":"List installed skills"},"post":{"summary":"Import skill from content or file/zip"}},
            "/skills/{name}":{"get":{"summary":"Load skill content"},"delete":{"summary":"Delete skill by name and location"}},
            "/skills/catalog":{"get":{"summary":"List enriched skill catalog records"}},
            "/skills/import/preview":{"post":{"summary":"Preview skill import conflicts/actions"}},
            "/skills/validate":{"post":{"summary":"Validate skill content/path and required sections"}},
            "/skills/router/match":{"post":{"summary":"Match goal text to best skill"}},
            "/skills/compile":{"post":{"summary":"Compile selected/routed skill into execution summary"}},
            "/skills/generate":{"post":{"summary":"Generate scaffold skill artifacts from prompt"}},
            "/skills/generate/install":{"post":{"summary":"Install generated/custom skill bundle artifacts"}},
            "/skills/evals/benchmark":{"post":{"summary":"Run benchmark scaffold for skill routing quality"}},
            "/skills/evals/triggers":{"post":{"summary":"Run trigger recall scaffold for a target skill"}},
            "/skills/templates":{"get":{"summary":"List installable skill templates"}},
            "/skills/templates/{id}/install":{"post":{"summary":"Install a skill template"}},
            "/memory/put":{"post":{"summary":"Store global memory content"}},
            "/memory/promote":{"post":{"summary":"Promote memory across visibility tiers with scrub/audit"}},
            "/memory/demote":{"post":{"summary":"Demote memory back to private visibility"}},
            "/memory/search":{"post":{"summary":"Search global memory with capability gating"}},
            "/memory/audit":{"get":{"summary":"List memory audit events"}},
            "/memory":{"get":{"summary":"List memory records"}},
            "/memory/{id}":{"delete":{"summary":"Delete memory record"}},
            "/packs":{"get":{"summary":"List installed packs"}},
            "/packs/{selector}":{"get":{"summary":"Inspect installed pack by pack_id or name"}},
            "/packs/install":{"post":{"summary":"Install tandem pack from local path or URL"}},
            "/packs/install_from_attachment":{"post":{"summary":"Install tandem pack from downloaded attachment path"}},
            "/packs/uninstall":{"post":{"summary":"Uninstall tandem pack"}},
            "/packs/export":{"post":{"summary":"Export installed tandem pack as zip"}},
            "/packs/detect":{"post":{"summary":"Detect tandem pack marker in zip and emit pack.detected"}},
            "/packs/{selector}/updates":{"get":{"summary":"Check updates for installed pack (stub)"}},
            "/packs/{selector}/update":{"post":{"summary":"Apply updates for installed pack (stub)"}}
        }
    }))
}

pub(super) async fn instance_dispose() -> Json<Value> {
    Json(json!({"ok": true}))
}

pub(super) async fn run_events(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> axum::response::Sse<
    impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    let rx = state.event_bus.subscribe();
    let initial = tokio_stream::once(Ok(axum::response::sse::Event::default().data(
        serde_json::to_string(&EngineEvent::new(
            "run.stream.connected",
            json!({ "runID": id }),
        ))
        .unwrap_or_default(),
    )));
    let live = tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            let event_run = event
                .properties
                .get("runID")
                .or_else(|| event.properties.get("run_id"))
                .and_then(|v| v.as_str());
            if event_run == Some(&id) {
                let payload = serde_json::to_string(&event).unwrap_or_default();
                Some(Ok(axum::response::sse::Event::default().data(payload)))
            } else {
                None
            }
        }
        Err(_) => None,
    });
    axum::response::Sse::new(initial.chain(live)).keep_alive(
        axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(10)),
    )
}

pub(super) async fn list_projects(State(state): State<AppState>) -> Json<Value> {
    let sessions = state
        .storage
        .list_sessions_scoped(tandem_core::SessionListScope::Global)
        .await;
    let mut directories = sessions
        .iter()
        .map(|s| s.directory.clone())
        .collect::<Vec<_>>();
    directories.sort();
    directories.dedup();
    Json(json!(directories))
}

pub(super) async fn push_log(
    State(state): State<AppState>,
    Json(input): Json<LogInput>,
) -> Json<Value> {
    let entry = json!({
        "ts": chrono::Utc::now(),
        "level": input.level.unwrap_or_else(|| "info".to_string()),
        "message": input.message.unwrap_or_default(),
        "context": input.context
    });
    state.logs.write().await.push(entry);
    Json(json!({"ok": true}))
}
