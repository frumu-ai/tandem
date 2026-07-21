// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serde::Deserialize;
use serde_json::Value;
use std::path::{Path as StdPath, PathBuf};
use std::time::{Duration, UNIX_EPOCH};
use tokio_stream::{wrappers::BroadcastStream, StreamExt};

use super::*;
use crate::action_authorization::{
    authorize_host_effect, AuthorizedHostEffect, CanonicalHostResource, HostAction,
    HostAuthorizationError, HostEffectRequest,
};

#[derive(Debug, Deserialize)]
pub(super) struct BrowserSmokeTestInput {
    #[serde(default)]
    url: Option<String>,
}

fn event_tenant_context(event: &EngineEvent) -> TenantContext {
    event
        .properties
        .get("tenantContext")
        .and_then(|value| serde_json::from_value(value.clone()).ok())
        .unwrap_or_else(TenantContext::local_implicit)
}
fn host_authorization_status(error: HostAuthorizationError) -> StatusCode {
    match error {
        HostAuthorizationError::AuditPersistenceFailed => StatusCode::INTERNAL_SERVER_ERROR,
        HostAuthorizationError::InvalidEffectArguments => StatusCode::BAD_REQUEST,
        _ => StatusCode::FORBIDDEN,
    }
}

async fn authorize_global_host_effect(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    locality: super::host_authority::RequestLocality,
    action: HostAction,
    resource: CanonicalHostResource,
    arguments: Value,
) -> Result<(AuthorizedHostEffect, HostEffectRequest), StatusCode> {
    let effect = HostEffectRequest::new(action, resource, arguments);
    let grant = authorize_host_effect(
        state,
        tenant,
        verified,
        locality.is_direct_loopback(),
        &effect,
    )
    .await
    .map_err(host_authorization_status)?;
    Ok((grant, effect))
}

pub(super) async fn global_health(State(state): State<AppState>) -> impl IntoResponse {
    let _ = prune_expired_leases(&state).await;
    let ready = state.is_ready();
    Json(json!({
        "healthy": ready,
        "ready": ready,
    }))
}
pub(super) async fn global_workspace(State(state): State<AppState>) -> Json<Value> {
    // The central auth gate protects this route. Do not require a direct peer here:
    // the shipped control panel reaches the engine through its authenticated proxy.
    let workspace_root = state.workspace_index.snapshot().await.root;
    Json(json!({ "workspace_root": workspace_root }))
}

pub(super) async fn global_diagnostics(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
) -> Result<Json<Value>, StatusCode> {
    super::host_authority::require_diagnostics_admin(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
    )?;
    let lease_count = prune_expired_leases(&state).await;
    let startup = state.startup_snapshot().await;
    let build = crate::build_provenance();
    let environment = state.host_runtime_context();
    let browser_summary = serde_json::to_value(state.browser_health_summary().await)
        .unwrap_or_else(|_| json!({ "enabled": false }));

    let browser = json!({
        "enabled": browser_summary.get("enabled").and_then(Value::as_bool).unwrap_or(false),
        "runnable": browser_summary.get("runnable").and_then(Value::as_bool).unwrap_or(false),
        "tools_registered": browser_summary
            .get("tools_registered")
            .and_then(Value::as_bool)
            .unwrap_or(false),
    });
    let memory_context_policy =
        crate::memory::policy_status::current_memory_context_policy_status();
    let memory_storage = match state.memory_store().await {
        Ok(store) => match store
            .backend_health(tandem_memory::MemoryBackendHealthRequest { repair: false })
            .await
        {
            Ok(health) => json!({
                "healthy": health.status == tandem_memory::MemoryBackendHealthStatus::Healthy,
                "backend": match health.backend {
                    tandem_memory::MemoryBackendKind::Sqlite => "sqlite",
                    tandem_memory::MemoryBackendKind::Postgres => "postgres",
                    tandem_memory::MemoryBackendKind::Other(_) => "other",
                },
                "checks": health.checks.into_iter().map(|check| json!({
                    "name": check.name,
                    "healthy": check.healthy,
                })).collect::<Vec<_>>(),
            }),
            Err(_) => json!({ "healthy": false, "status": "unavailable" }),
        },
        Err(_) => json!({ "healthy": false, "status": "unavailable" }),
    };
    let healthy = memory_storage
        .get("healthy")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Ok(Json(json!({
        "healthy": healthy,
        "ready": state.is_ready() && healthy,
        "apiTokenRequired": state.api_token().await.is_some(),
        "phase": startup.phase,
        "startup_attempt_id": startup.attempt_id,
        "startup_elapsed_ms": startup.elapsed_ms,
        "startup_error": startup.last_error.is_some(),
        "version": build.version,
        "build_id": build.build_id,
        "git_sha": build.git_sha,
        "mode": state.mode_label(),
        "leaseCount": lease_count,
        "environment": environment,
        "memory_context_policy": memory_context_policy,
        "memory_storage": memory_storage,
        "browser": browser
    })))
}

pub(super) async fn browser_status(State(state): State<AppState>) -> impl IntoResponse {
    Json(json!(state.browser_status().await))
}

pub(super) async fn browser_install(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
) -> impl IntoResponse {
    let deployment_id = tenant
        .deployment_id
        .as_deref()
        .unwrap_or("local-deployment");
    let (grant, effect) = match authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::BrowserInstall,
        CanonicalHostResource::new("deployment", deployment_id, tenant.clone()),
        json!({"operation": "install_browser_sidecar"}),
    )
    .await
    {
        Ok(authorized) => authorized,
        Err(status) => return status.into_response(),
    };
    if let Err(error) = grant.revalidate(&state, &effect) {
        return host_authorization_status(error).into_response();
    }
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
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    payload: Option<Json<BrowserSmokeTestInput>>,
) -> impl IntoResponse {
    let input = payload
        .map(|Json(value)| value)
        .unwrap_or(BrowserSmokeTestInput { url: None });
    let deployment_id = tenant
        .deployment_id
        .as_deref()
        .unwrap_or("local-deployment");
    let (grant, effect) = match authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::BrowserSmokeTest,
        CanonicalHostResource::new("deployment", deployment_id, tenant.clone()),
        json!({
            "operation": "browser_smoke_test",
            "url": input.url,
        }),
    )
    .await
    {
        Ok(authorized) => authorized,
        Err(status) => return status.into_response(),
    };
    if let Err(error) = grant.revalidate(&state, &effect) {
        return host_authorization_status(error).into_response();
    }
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
    Extension(tenant): Extension<TenantContext>,
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
        tenant_context: tenant,
    };
    let mut leases = state.engine_leases.write().await;
    let expired = leases
        .iter()
        .filter(|(_, lease)| lease.is_expired(now))
        .map(|(lease_id, _)| lease_id.clone())
        .collect::<Vec<_>>();
    leases.retain(|_, l| !l.is_expired(now));
    leases.insert(lease_id.clone(), lease.clone());
    drop(leases);
    for expired_lease_id in expired {
        cleanup_managed_worktrees_for_lease(&state, &expired_lease_id).await;
    }
    let lease_count = state.engine_leases.read().await.len();
    Json(json!({
        "lease_id": lease_id,
        "client_id": lease.client_id,
        "client_type": lease.client_type,
        "acquired_at_ms": lease.acquired_at_ms,
        "last_renewed_at_ms": lease.last_renewed_at_ms,
        "ttl_ms": lease.ttl_ms,
        "lease_count": lease_count
    }))
}

pub(super) async fn global_lease_renew(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Json(input): Json<EngineLeaseRenewInput>,
) -> Json<Value> {
    prune_expired_leases(&state).await;
    let now = crate::now_ms();
    let mut leases = state.engine_leases.write().await;
    let renewed = if let Some(lease) = leases.get_mut(&input.lease_id) {
        if lease.tenant_context != tenant {
            false
        } else {
            lease.last_renewed_at_ms = now;
            true
        }
    } else {
        false
    };
    Json(json!({ "ok": renewed, "lease_count": leases.len() }))
}

pub(super) async fn global_lease_release(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Json(input): Json<EngineLeaseReleaseInput>,
) -> Json<Value> {
    prune_expired_leases(&state).await;
    let removed = {
        let mut leases = state.engine_leases.write().await;
        if leases
            .get(&input.lease_id)
            .is_some_and(|lease| lease.tenant_context == tenant)
        {
            leases.remove(&input.lease_id);
            true
        } else {
            false
        }
    };
    let cleanup = if removed {
        cleanup_managed_worktrees_for_lease(&state, &input.lease_id).await
    } else {
        LeaseWorktreeCleanupResult::default()
    };
    let released_worktree_count = cleanup.cleaned_paths.len();
    let released_worktree_failure_count = cleanup.failures.len();
    let expose_host_details = tenant.is_local_implicit();
    Json(json!({
        "ok": removed,
        "lease_count": state.engine_leases.read().await.len(),
        "released_worktree_count": released_worktree_count,
        "released_worktree_failure_count": released_worktree_failure_count,
        "released_worktrees": expose_host_details.then_some(cleanup.cleaned_paths),
        "released_worktree_failures": expose_host_details.then_some(cleanup.failures),
    }))
}

pub(super) async fn global_storage_repair(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Json(input): Json<StorageRepairInput>,
) -> Result<Json<Value>, StatusCode> {
    let force = input.force.unwrap_or(false);
    let deployment_id = tenant
        .deployment_id
        .as_deref()
        .unwrap_or("local-deployment");
    let (grant, effect) = authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::StorageRepair,
        CanonicalHostResource::new("deployment", deployment_id, tenant.clone()),
        json!({"force": force}),
    )
    .await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
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

pub(super) fn event_visible_to_tenant(event: &EngineEvent, request_tenant: &TenantContext) -> bool {
    tenant_matches(request_tenant, &event_tenant_context(event))
}

fn sse_stream(
    state: AppState,
    request_tenant: TenantContext,
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
            if !event_visible_to_tenant(&event, &request_tenant) {
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
                let tenant_context = event_tenant_context(&event);
                normalize_run_event(event, &session_hint, run_id, &tenant_context)
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
    Extension(tenant_context): Extension<TenantContext>,
    Query(filter): Query<EventFilterQuery>,
) -> axum::response::Sse<
    impl tokio_stream::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>>,
> {
    axum::response::Sse::new(sse_stream(state, tenant_context, filter))
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

pub(super) async fn global_dispose(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
) -> Result<Json<Value>, StatusCode> {
    let deployment_id = tenant
        .deployment_id
        .as_deref()
        .unwrap_or("local-deployment");
    let (grant, effect) = authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::GlobalDispose,
        CanonicalHostResource::new("deployment", deployment_id, tenant.clone()),
        json!({"operation": "cancel_all_sessions_and_close_all_browser_sessions"}),
    )
    .await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let cancelled = state.cancellations.cancel_all().await;
    let closed_browser_sessions = state.close_all_browser_sessions().await;
    Ok(Json(json!({
        "ok": true,
        "cancelledSessions": cancelled,
        "closedBrowserSessions": closed_browser_sessions,
    })))
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
    #[serde(default, alias = "scopeAllowlist")]
    pub scope_allowlist: Vec<String>,
}

pub(super) async fn execute_tool(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    verified_tenant_context: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Json(input): Json<ToolExecutionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut args = input.args.unwrap_or_else(|| json!({}));
    let verified_tenant_context = verified_tenant_context.map(|Extension(context)| context);
    if let Some(verified_tenant_context) = verified_tenant_context.as_ref() {
        if let Some(obj) = args.as_object_mut() {
            obj.insert(
                "__verified_tenant_context".to_string(),
                serde_json::to_value(verified_tenant_context).unwrap_or(Value::Null),
            );
        }
    }
    let mut dispatch_context = state.untrusted_tool_dispatch_context(
        tandem_tools::ToolDispatchSource::new("http_global_tool")
            .request(Uuid::new_v4().to_string()),
        tenant_context,
        crate::config::channels::normalize_allowed_tools(input.scope_allowlist),
    );
    if let Some(verified_tenant_context) = verified_tenant_context {
        dispatch_context = dispatch_context.with_verified_tenant_context(verified_tenant_context);
    }
    let result = state
        .tool_dispatcher
        .dispatch(&input.tool, args, dispatch_context)
        .await
        .map_err(|e| {
            if let Some(blocked) = e.downcast_ref::<tandem_tools::ToolDispatchBlocked>() {
                let status = if blocked.decision.outcome
                    == tandem_tools::ToolDispatchPolicyOutcome::ApprovalRequired
                {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::FORBIDDEN
                };
                return (
                    status,
                    Json(json!({
                        "code": if status == StatusCode::CONFLICT {
                            "TOOL_APPROVAL_REQUIRED"
                        } else {
                            "TOOL_DISPATCH_DENIED"
                        },
                        "outcome": blocked.decision.outcome,
                        "reason": blocked.decision.reason,
                        "policy_decision_id": blocked.decision.policy_decision_id,
                        "approval_requirement": blocked.decision.approval_requirement,
                    })),
                );
            }
            tracing::error!("Tool execution failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "code": "TOOL_DISPATCH_FAILED",
                    "error": e.to_string(),
                })),
            )
        })?;
    Ok(Json(json!({
        "output": result.output,
        "metadata": result.metadata
    })))
}

pub(super) async fn create_worktree(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Json(input): Json<WorktreeInput>,
) -> Result<Json<Value>, StatusCode> {
    if input.managed == Some(false) {
        return Err(StatusCode::FORBIDDEN);
    }
    let (resource, repo_candidate) = resolve_worktree_resource_candidate(
        &state,
        &tenant,
        verified.as_deref(),
        input.repository_id.as_deref(),
        input.repo_root.as_deref(),
    )
    .await?;
    if verified.is_some()
        && input
            .lease_id
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
    {
        return Err(StatusCode::CONFLICT);
    }
    let managed = true;
    let base = input.base.clone().unwrap_or_else(|| "HEAD".to_string());
    if base.trim_start().starts_with('-') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let slug = crate::runtime::worktrees::managed_worktree_slug(
        input.task_id.as_deref(),
        input.owner_run_id.as_deref(),
        input.lease_id.as_deref(),
        input.branch.as_deref(),
    );
    let default_path = PathBuf::from(&repo_candidate)
        .join(".tandem")
        .join("worktrees")
        .join(&slug);
    let path = resolve_worktree_path(&repo_candidate, input.path.as_deref(), &default_path)?;
    if !is_within_managed_worktree_root(&repo_candidate, &path) {
        return Err(StatusCode::CONFLICT);
    }
    let branch = input
        .branch
        .clone()
        .unwrap_or_else(|| format!("tandem/{slug}"));
    if branch.trim_start().starts_with('-') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let cleanup_branch = input.cleanup_branch.unwrap_or(true);
    let lease =
        validate_managed_worktree_lease(&state, true, input.lease_id.as_deref(), &tenant).await?;
    let path_string = path.to_string_lossy().to_string();
    let (grant, effect) = authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::WorktreeCreate,
        resource,
        json!({
            "repository_candidate": repo_candidate,
            "path": path_string,
            "branch": branch,
            "base": base,
            "lease_id": &input.lease_id,
            "cleanup_branch": cleanup_branch,
        }),
    )
    .await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let repo_root = verify_git_repo_root(&repo_candidate)?;
    let key = crate::runtime::worktrees::managed_worktree_key(
        &repo_root,
        input.task_id.as_deref(),
        input.owner_run_id.as_deref(),
        input.lease_id.as_deref(),
        &path_string,
        &branch,
    );
    let worktree_id = key.clone();
    let expose_host_paths = verified.is_none();
    if let Some(existing) = state.managed_worktrees.read().await.get(&key).cloned() {
        super::sessions_actor_scope::ensure_same_session_actor(&tenant, &existing.tenant_context)?;
        if existing.repository_id != input.repository_id {
            return Err(StatusCode::NOT_FOUND);
        }
        grant
            .revalidate(&state, &effect)
            .map_err(host_authorization_status)?;
        if worktree_is_registered(&repo_root, &existing.path)? {
            return Ok(Json(json!({
                "ok": true,
                "worktree_id": existing.key,
                "repository_id": existing.repository_id,
                "repo_root": expose_host_paths.then_some(existing.repo_root),
                "path": expose_host_paths.then_some(existing.path),
                "branch": existing.branch,
                "base": existing.base,
                "managed": existing.managed,
                "task_id": existing.task_id,
                "owner_run_id": existing.owner_run_id,
                "lease_id": existing.lease_id,
                "lease_client_id": lease.as_ref().map(|row| row.client_id.clone()),
                "lease_client_type": lease.as_ref().map(|row| row.client_type.clone()),
                "cleanup_branch": existing.cleanup_branch,
                "reused": true,
            })));
        }
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    if path.exists() && !worktree_is_registered(&repo_root, &path_string)? {
        return Ok(Json(json!({
            "ok": false,
            "worktree_id": worktree_id,
            "repository_id": input.repository_id,
            "repo_root": expose_host_paths.then_some(repo_root),
            "path": expose_host_paths.then_some(path_string),
            "branch": branch,
            "base": base,
            "managed": managed,
            "error": "target path already exists but is not a registered worktree",
            "code": "WORKTREE_PATH_CONFLICT",
        })));
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    if worktree_is_registered(&repo_root, &path_string)? {
        let now = crate::now_ms();
        state.managed_worktrees.write().await.insert(
            key.clone(),
            crate::ManagedWorktreeRecord {
                key: crate::runtime::worktrees::managed_worktree_key(
                    &repo_root,
                    input.task_id.as_deref(),
                    input.owner_run_id.as_deref(),
                    input.lease_id.as_deref(),
                    &path_string,
                    &branch,
                ),
                repo_root: repo_root.clone(),
                repository_id: input.repository_id.clone(),
                tenant_context: tenant.clone(),
                path: path_string.clone(),
                branch: branch.clone(),
                base: base.clone(),
                managed,
                task_id: input.task_id,
                owner_run_id: input.owner_run_id,
                lease_id: input.lease_id,
                cleanup_branch,
                created_at_ms: now,
                updated_at_ms: now,
            },
        );
        return Ok(Json(json!({
            "ok": true,
            "worktree_id": worktree_id,
            "repository_id": input.repository_id,
            "repo_root": expose_host_paths.then_some(repo_root),
            "path": expose_host_paths.then_some(path_string),
            "branch": branch,
            "base": base,
            "managed": managed,
            "cleanup_branch": cleanup_branch,
            "lease_client_id": lease.as_ref().map(|row| row.client_id.clone()),
            "lease_client_type": lease.as_ref().map(|row| row.client_type.clone()),
            "reused": true,
        })));
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let output = std::process::Command::new("git")
        .args([
            "-C",
            &repo_root,
            "worktree",
            "add",
            "-b",
            &branch,
            &path_string,
            &base,
        ])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let ok = output.status.success();
    if ok {
        let now = crate::now_ms();
        state.managed_worktrees.write().await.insert(
            key.clone(),
            crate::ManagedWorktreeRecord {
                key: crate::runtime::worktrees::managed_worktree_key(
                    &repo_root,
                    input.task_id.as_deref(),
                    input.owner_run_id.as_deref(),
                    input.lease_id.as_deref(),
                    &path_string,
                    &branch,
                ),
                repo_root: repo_root.clone(),
                repository_id: input.repository_id.clone(),
                tenant_context: tenant.clone(),
                path: path_string.clone(),
                branch: branch.clone(),
                base: base.clone(),
                managed,
                task_id: input.task_id,
                owner_run_id: input.owner_run_id,
                lease_id: input.lease_id,
                cleanup_branch,
                created_at_ms: now,
                updated_at_ms: now,
            },
        );
    }
    Ok(Json(json!({
        "ok": ok,
        "worktree_id": worktree_id,
        "repository_id": input.repository_id,
        "repo_root": expose_host_paths.then_some(repo_root),
        "path": expose_host_paths.then_some(path_string),
        "branch": branch,
        "base": base,
        "managed": managed,
        "cleanup_branch": cleanup_branch,
        "lease_client_id": lease.as_ref().map(|row| row.client_id.clone()),
        "lease_client_type": lease.as_ref().map(|row| row.client_type.clone()),
        "reused": false,
        "stderr": expose_host_paths.then(|| String::from_utf8_lossy(&output.stderr).to_string())
    })))
}

pub(super) async fn list_worktrees(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Query(query): Query<WorktreeListQuery>,
) -> Result<Json<Value>, StatusCode> {
    let (resource, repo_candidate) = resolve_worktree_resource_candidate(
        &state,
        &tenant,
        verified.as_deref(),
        query.repository_id.as_deref(),
        query.repo_root.as_deref(),
    )
    .await?;
    let (grant, effect) = authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::WorktreeList,
        resource,
        json!({
            "repository_candidate": repo_candidate,
            "managed_only": true,
        }),
    )
    .await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let repo_root = verify_git_repo_root(&repo_candidate)?;
    let records = state
        .managed_worktrees
        .read()
        .await
        .values()
        .filter(|record| {
            record.managed
                && record.repo_root == repo_root
                && record.repository_id == query.repository_id
                && record.tenant_context.org_id == tenant.org_id
                && record.tenant_context.workspace_id == tenant.workspace_id
                && record.tenant_context.deployment_id == tenant.deployment_id
                && record.tenant_context.actor_id == tenant.actor_id
        })
        .cloned()
        .collect::<Vec<_>>();
    let expose_host_paths = verified.is_none();
    let mut worktrees = Vec::with_capacity(records.len());
    for record in records {
        grant
            .revalidate(&state, &effect)
            .map_err(host_authorization_status)?;
        let registered = worktree_is_registered(&repo_root, &record.path)?;
        worktrees.push(json!({
            "worktree_id": record.key,
            "repository_id": record.repository_id,
            "path": expose_host_paths.then_some(record.path),
            "repo_root": expose_host_paths.then_some(record.repo_root),
            "branch": record.branch,
            "base": record.base,
            "managed": true,
            "task_id": record.task_id,
            "owner_run_id": record.owner_run_id,
            "lease_id": record.lease_id,
            "cleanup_branch": record.cleanup_branch,
            "registered": registered,
        }));
    }
    Ok(Json(json!(worktrees)))
}

pub(super) async fn delete_worktree(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Json(input): Json<WorktreeInput>,
) -> Result<Json<Value>, StatusCode> {
    if verified.is_some()
        && input
            .worktree_id
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (repository_resource, repo_candidate) = resolve_worktree_resource_candidate(
        &state,
        &tenant,
        verified.as_deref(),
        input.repository_id.as_deref(),
        input.repo_root.as_deref(),
    )
    .await?;
    let record = state
        .managed_worktrees
        .read()
        .await
        .values()
        .find(|record| {
            input
                .worktree_id
                .as_deref()
                .is_some_and(|worktree_id| record.key == worktree_id)
                || (verified.is_none()
                    && input
                        .path
                        .as_deref()
                        .is_some_and(|path| record.path == path))
        })
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)?;
    if !record.managed
        || record.repo_root != repo_candidate
        || record.repository_id != input.repository_id
        || record.tenant_context.org_id != tenant.org_id
        || record.tenant_context.workspace_id != tenant.workspace_id
        || record.tenant_context.deployment_id != tenant.deployment_id
        || record.tenant_context.actor_id != tenant.actor_id
    {
        return Err(StatusCode::NOT_FOUND);
    }
    validate_worktree_mutation_authority(&state, Some(&record), input.lease_id.as_deref()).await?;
    let resource = CanonicalHostResource::new(
        "managed_worktree",
        record.key.clone(),
        record.tenant_context.clone(),
    );
    let (grant, effect) = authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::WorktreeDelete,
        resource,
        json!({
            "repository_id": repository_resource.id,
            "repo_root": record.repo_root,
            "path": record.path,
            "branch": record.branch,
            "lease_id": &input.lease_id,
            "cleanup_branch": record.cleanup_branch,
        }),
    )
    .await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let repo_root = verify_git_repo_root(&repo_candidate)?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let dirty = std::process::Command::new("git")
        .args(["-C", &record.path, "status", "--porcelain"])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !dirty.status.success() || !dirty.stdout.is_empty() {
        return Err(StatusCode::CONFLICT);
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let output = std::process::Command::new("git")
        .args([
            "-C",
            &repo_root,
            "worktree",
            "remove",
            "--force",
            &record.path,
        ])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let mut branch_deleted = false;
    if output.status.success() && record.cleanup_branch {
        grant
            .revalidate(&state, &effect)
            .map_err(host_authorization_status)?;
        let branch_out = std::process::Command::new("git")
            .args(["-C", &repo_root, "branch", "-D", &record.branch])
            .output()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        branch_deleted = branch_out.status.success();
    }
    if output.status.success() {
        state.managed_worktrees.write().await.remove(&record.key);
    }
    let expose_host_paths = verified.is_none();
    Ok(Json(json!({
        "ok": output.status.success(),
        "worktree_id": record.key,
        "repository_id": record.repository_id,
        "repo_root": expose_host_paths.then_some(repo_root),
        "path": expose_host_paths.then_some(record.path),
        "branch": record.branch,
        "cleanup_branch": record.cleanup_branch,
        "branch_deleted": branch_deleted,
        "stderr": String::from_utf8_lossy(&output.stderr).to_string()
    })))
}

pub(super) async fn reset_worktree(
    State(state): State<AppState>,
    Extension(tenant): Extension<TenantContext>,
    Extension(locality): Extension<super::host_authority::RequestLocality>,
    verified: Option<Extension<tandem_types::VerifiedTenantContext>>,
    Json(input): Json<WorktreeInput>,
) -> Result<Json<Value>, StatusCode> {
    if verified.is_some()
        && input
            .worktree_id
            .as_deref()
            .map(str::trim)
            .is_none_or(str::is_empty)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (repository_resource, repo_candidate) = resolve_worktree_resource_candidate(
        &state,
        &tenant,
        verified.as_deref(),
        input.repository_id.as_deref(),
        input.repo_root.as_deref(),
    )
    .await?;
    let record = state
        .managed_worktrees
        .read()
        .await
        .values()
        .find(|record| {
            input
                .worktree_id
                .as_deref()
                .is_some_and(|worktree_id| record.key == worktree_id)
                || (verified.is_none()
                    && input
                        .path
                        .as_deref()
                        .is_some_and(|path| record.path == path))
        })
        .cloned()
        .ok_or(StatusCode::NOT_FOUND)?;
    if !record.managed
        || record.repo_root != repo_candidate
        || record.repository_id != input.repository_id
        || record.tenant_context.org_id != tenant.org_id
        || record.tenant_context.workspace_id != tenant.workspace_id
        || record.tenant_context.deployment_id != tenant.deployment_id
        || record.tenant_context.actor_id != tenant.actor_id
    {
        return Err(StatusCode::NOT_FOUND);
    }
    validate_worktree_mutation_authority(&state, Some(&record), input.lease_id.as_deref()).await?;
    let target = input.base.clone().unwrap_or_else(|| "HEAD".to_string());
    if target.trim_start().starts_with('-') {
        return Err(StatusCode::BAD_REQUEST);
    }
    let backup_ref = format!("refs/tandem/backups/{}", Uuid::new_v4());
    let resource = CanonicalHostResource::new(
        "managed_worktree",
        record.key.clone(),
        record.tenant_context.clone(),
    );
    let (grant, effect) = authorize_global_host_effect(
        &state,
        &tenant,
        verified.as_deref(),
        locality,
        HostAction::WorktreeReset,
        resource,
        json!({
            "repository_id": repository_resource.id,
            "repo_root": record.repo_root,
            "path": record.path,
            "target": target,
            "backup_ref": backup_ref,
            "lease_id": &input.lease_id,
        }),
    )
    .await?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let repo_root = verify_git_repo_root(&repo_candidate)?;
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    if !worktree_is_registered(&repo_root, &record.path)? {
        return Err(StatusCode::NOT_FOUND);
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let dirty = std::process::Command::new("git")
        .args(["-C", &record.path, "status", "--porcelain"])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !dirty.status.success() || !dirty.stdout.is_empty() {
        return Err(StatusCode::CONFLICT);
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let backup = std::process::Command::new("git")
        .args(["-C", &record.path, "update-ref", &backup_ref, "HEAD"])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !backup.status.success() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }
    grant
        .revalidate(&state, &effect)
        .map_err(host_authorization_status)?;
    let output = std::process::Command::new("git")
        .args(["-C", &record.path, "reset", "--hard", &target])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let expose_host_paths = verified.is_none();
    Ok(Json(json!({
        "ok": output.status.success(),
        "worktree_id": record.key,
        "repository_id": record.repository_id,
        "repo_root": expose_host_paths.then_some(repo_root),
        "path": expose_host_paths.then_some(record.path),
        "target": target,
        "backup_ref": backup_ref,
        "stderr": String::from_utf8_lossy(&output.stderr).to_string()
    })))
}

#[derive(Debug, Clone)]
struct RegisteredWorktreeEntry {
    path: String,
    branch: Option<String>,
}

fn parse_registered_worktree_entries(
    repo_root: &str,
) -> Result<Vec<RegisteredWorktreeEntry>, StatusCode> {
    let output = std::process::Command::new("git")
        .args(["-C", repo_root, "worktree", "list", "--porcelain"])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !output.status.success() {
        return Err(StatusCode::INTERNAL_SERVER_ERROR);
    }

    let mut entries = Vec::new();
    let mut current_path: Option<String> = None;
    let mut current_branch: Option<String> = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if line.is_empty() {
            if let Some(path) = current_path.take() {
                entries.push(RegisteredWorktreeEntry {
                    path,
                    branch: current_branch.take(),
                });
            }
            continue;
        }
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(path.trim().to_string());
            continue;
        }
        if let Some(branch) = line.strip_prefix("branch ") {
            current_branch = branch
                .trim()
                .strip_prefix("refs/heads/")
                .map(ToString::to_string)
                .or_else(|| Some(branch.trim().to_string()));
        }
    }
    if let Some(path) = current_path.take() {
        entries.push(RegisteredWorktreeEntry {
            path,
            branch: current_branch.take(),
        });
    }
    Ok(entries)
}

pub(super) async fn cleanup_worktrees(
    State(state): State<AppState>,
    payload: Option<Json<WorktreeCleanupInput>>,
) -> Result<Json<Value>, StatusCode> {
    let input = payload
        .map(|Json(value)| value)
        .unwrap_or_else(WorktreeCleanupInput::default);
    let repo_root = resolve_worktree_repo_root(&state, input.repo_root.as_deref()).await?;
    let dry_run = input.dry_run.unwrap_or(false);
    let remove_orphan_dirs = input.remove_orphan_dirs.unwrap_or(true);
    let managed_root = crate::runtime::worktrees::managed_worktree_root(&repo_root);
    let managed_root_string = managed_root.to_string_lossy().to_string();
    let records = state.managed_worktrees.read().await.clone();
    let git_managed_worktrees = parse_registered_worktree_entries(&repo_root)?
        .into_iter()
        .filter(|entry| StdPath::new(&entry.path).starts_with(&managed_root))
        .collect::<Vec<_>>();

    let active_paths = records
        .values()
        .filter(|row| row.repo_root == repo_root)
        .map(|row| row.path.clone())
        .collect::<std::collections::HashSet<_>>();
    let tracked_paths = records
        .values()
        .filter(|row| row.repo_root == repo_root)
        .map(|row| row.path.clone())
        .collect::<Vec<_>>();

    let mut stale = Vec::new();
    let mut active = Vec::new();
    for entry in &git_managed_worktrees {
        if active_paths.contains(&entry.path) {
            active.push(entry.path.clone());
        } else {
            stale.push(entry.clone());
        }
    }

    let mut cleaned = Vec::new();
    let mut failures = Vec::new();
    if !dry_run {
        for entry in &stale {
            let remove_output = std::process::Command::new("git")
                .args([
                    "-C",
                    &repo_root,
                    "worktree",
                    "remove",
                    "--force",
                    &entry.path,
                ])
                .output();
            match remove_output {
                Ok(result) if result.status.success() => {
                    state
                        .managed_worktrees
                        .write()
                        .await
                        .retain(|_, row| row.repo_root != repo_root || row.path != entry.path);
                    let mut branch_deleted = None;
                    let mut branch_delete_error = None;
                    if let Some(branch) = entry.branch.as_deref() {
                        match std::process::Command::new("git")
                            .args(["-C", &repo_root, "branch", "-D", branch])
                            .output()
                        {
                            Ok(branch_output) if branch_output.status.success() => {
                                branch_deleted = Some(true);
                            }
                            Ok(branch_output) => {
                                branch_deleted = Some(false);
                                branch_delete_error = Some(
                                    String::from_utf8_lossy(&branch_output.stderr).to_string(),
                                );
                            }
                            Err(err) => {
                                branch_deleted = Some(false);
                                branch_delete_error = Some(err.to_string());
                            }
                        }
                    }
                    cleaned.push(json!({
                        "path": entry.path,
                        "branch": entry.branch,
                        "branch_deleted": branch_deleted,
                        "branch_delete_error": branch_delete_error,
                        "via": "git_worktree_remove",
                    }));
                }
                Ok(result) => {
                    failures.push(json!({
                        "path": entry.path,
                        "branch": entry.branch,
                        "code": "WORKTREE_REMOVE_FAILED",
                        "stderr": String::from_utf8_lossy(&result.stderr).to_string(),
                    }));
                }
                Err(err) => {
                    failures.push(json!({
                        "path": entry.path,
                        "branch": entry.branch,
                        "code": "WORKTREE_REMOVE_FAILED",
                        "error": err.to_string(),
                    }));
                }
            }
        }
    }

    let mut orphan_dirs = Vec::new();
    if managed_root.exists() {
        let entries =
            std::fs::read_dir(&managed_root).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        let registered_paths = if dry_run {
            git_managed_worktrees
                .iter()
                .map(|entry| entry.path.clone())
                .collect::<std::collections::HashSet<_>>()
        } else {
            parse_registered_worktree_entries(&repo_root)?
                .into_iter()
                .map(|entry| entry.path)
                .filter(|path| StdPath::new(path).starts_with(&managed_root))
                .collect::<std::collections::HashSet<_>>()
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let path_string = path.to_string_lossy().to_string();
            if registered_paths.contains(&path_string) {
                continue;
            }
            if active_paths.contains(&path_string) {
                continue;
            }
            orphan_dirs.push(path_string);
        }
    }

    let mut orphan_removed = Vec::new();
    if !dry_run && remove_orphan_dirs {
        for path in &orphan_dirs {
            match std::fs::remove_dir_all(path) {
                Ok(_) => {
                    orphan_removed.push(json!({
                        "path": path,
                        "via": "filesystem_remove_dir_all",
                    }));
                }
                Err(err) => {
                    failures.push(json!({
                        "path": path,
                        "code": "WORKTREE_ORPHAN_DIR_REMOVE_FAILED",
                        "error": err.to_string(),
                    }));
                }
            }
        }
    }

    Ok(Json(json!({
        "ok": failures.is_empty(),
        "dry_run": dry_run,
        "repo_root": repo_root,
        "managed_root": managed_root_string,
        "tracked_paths": tracked_paths,
        "active_paths": active,
        "stale_paths": stale.iter().map(|entry| json!({
            "path": entry.path,
            "branch": entry.branch,
        })).collect::<Vec<_>>(),
        "cleaned_worktrees": cleaned,
        "orphan_dirs": orphan_dirs,
        "orphan_dirs_removed": orphan_removed,
        "failures": failures,
    })))
}

async fn resolve_worktree_resource_candidate(
    state: &AppState,
    tenant: &TenantContext,
    verified: Option<&tandem_types::VerifiedTenantContext>,
    repository_id: Option<&str>,
    repo_root: Option<&str>,
) -> Result<(CanonicalHostResource, String), StatusCode> {
    if let Some(repository_id) = repository_id
        .map(str::trim)
        .filter(|repository_id| !repository_id.is_empty())
    {
        let session = state
            .storage
            .get_session(repository_id)
            .await
            .ok_or(StatusCode::NOT_FOUND)?;
        super::sessions_actor_scope::ensure_same_session_actor(tenant, &session.tenant_context)?;
        let workspace = session
            .workspace_root
            .as_deref()
            .unwrap_or(session.directory.as_str());
        let canonical = tokio::fs::canonicalize(workspace)
            .await
            .map_err(|_| StatusCode::NOT_FOUND)?;
        let candidate = crate::normalize_absolute_workspace_root(&canonical.to_string_lossy())
            .map_err(|_| StatusCode::BAD_REQUEST)?;
        return Ok((
            CanonicalHostResource::new("repository", repository_id, session.tenant_context),
            candidate,
        ));
    }
    if verified.is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let requested = if let Some(repo_root) = repo_root {
        PathBuf::from(repo_root)
    } else {
        let root = state.workspace_index.snapshot().await.root;
        let root = PathBuf::from(root);
        if root.is_absolute() {
            root
        } else {
            std::env::current_dir()
                .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
                .join(root)
        }
    };
    let canonical = tokio::fs::canonicalize(requested)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;
    let candidate = crate::normalize_absolute_workspace_root(&canonical.to_string_lossy())
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    Ok((
        CanonicalHostResource::new("local_repository", "local-repository", tenant.clone()),
        candidate,
    ))
}

fn verify_git_repo_root(candidate: &str) -> Result<String, StatusCode> {
    let output = std::process::Command::new("git")
        .args(["-C", candidate, "rev-parse", "--show-toplevel"])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !output.status.success() {
        return Err(StatusCode::CONFLICT);
    }
    let resolved =
        crate::normalize_absolute_workspace_root(String::from_utf8_lossy(&output.stdout).trim())
            .map_err(|_| StatusCode::CONFLICT)?;
    if resolved != candidate {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(resolved)
}
async fn resolve_worktree_repo_root(
    state: &AppState,
    repo_root: Option<&str>,
) -> Result<String, StatusCode> {
    let requested = if let Some(repo_root) = repo_root {
        crate::normalize_absolute_workspace_root(repo_root).map_err(|_| StatusCode::BAD_REQUEST)?
    } else {
        let root = state.workspace_index.snapshot().await.root;
        if StdPath::new(&root).is_absolute() {
            crate::normalize_absolute_workspace_root(&root).map_err(|_| StatusCode::BAD_REQUEST)?
        } else {
            let cwd = std::env::current_dir().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
            let joined = cwd.join(root);
            crate::normalize_absolute_workspace_root(&joined.to_string_lossy())
                .map_err(|_| StatusCode::BAD_REQUEST)?
        }
    };
    let output = std::process::Command::new("git")
        .args(["-C", &requested, "rev-parse", "--show-toplevel"])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !output.status.success() {
        return Err(StatusCode::CONFLICT);
    }
    let resolved = String::from_utf8_lossy(&output.stdout).trim().to_string();
    crate::normalize_absolute_workspace_root(&resolved).map_err(|_| StatusCode::CONFLICT)
}

async fn validate_managed_worktree_lease(
    state: &AppState,
    managed: bool,
    lease_id: Option<&str>,
    tenant: &TenantContext,
) -> Result<Option<crate::EngineLease>, StatusCode> {
    if !managed {
        return Ok(None);
    }
    let Some(lease_id) = lease_id.filter(|value| !value.trim().is_empty()) else {
        return Err(StatusCode::CONFLICT);
    };
    let now = crate::now_ms();
    let mut leases = state.engine_leases.write().await;
    leases.retain(|_, lease| !lease.is_expired(now));
    let lease = leases.get(lease_id).cloned().ok_or(StatusCode::CONFLICT)?;
    if lease.tenant_context != *tenant {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(Some(lease))
}

async fn prune_expired_leases(state: &AppState) -> usize {
    let now = crate::now_ms();
    let expired = {
        let mut leases = state.engine_leases.write().await;
        let expired = leases
            .iter()
            .filter(|(_, lease)| lease.is_expired(now))
            .map(|(lease_id, _)| lease_id.clone())
            .collect::<Vec<_>>();
        leases.retain(|_, lease| !lease.is_expired(now));
        expired
    };
    for lease_id in expired {
        cleanup_managed_worktrees_for_lease(state, &lease_id).await;
    }
    state.engine_leases.read().await.len()
}

async fn validate_worktree_mutation_authority(
    state: &AppState,
    record: Option<&crate::ManagedWorktreeRecord>,
    lease_id: Option<&str>,
) -> Result<(), StatusCode> {
    let record = record.ok_or(StatusCode::NOT_FOUND)?;
    if !record.managed {
        return Err(StatusCode::FORBIDDEN);
    }
    let record_lease_id = record
        .lease_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .ok_or(StatusCode::CONFLICT)?;
    let request_lease_id = lease_id
        .filter(|value| !value.trim().is_empty())
        .ok_or(StatusCode::CONFLICT)?;
    if request_lease_id != record_lease_id {
        return Err(StatusCode::CONFLICT);
    }
    validate_managed_worktree_lease(state, true, Some(request_lease_id), &record.tenant_context)
        .await
        .map(|_| ())
}

#[derive(Default)]
struct LeaseWorktreeCleanupResult {
    cleaned_paths: Vec<String>,
    failures: Vec<Value>,
}

async fn cleanup_managed_worktrees_for_lease(
    state: &AppState,
    lease_id: &str,
) -> LeaseWorktreeCleanupResult {
    let records = state
        .managed_worktrees
        .read()
        .await
        .values()
        .filter(|row| row.lease_id.as_deref() == Some(lease_id))
        .cloned()
        .collect::<Vec<_>>();
    let mut result = LeaseWorktreeCleanupResult::default();
    for record in records {
        let effect = HostEffectRequest::new(
            HostAction::WorktreeCleanup,
            CanonicalHostResource::new(
                "managed_worktree",
                record.key.clone(),
                record.tenant_context.clone(),
            ),
            json!({
                "repository_id": &record.repository_id,
                "repo_root": &record.repo_root,
                "path": &record.path,
                "branch": &record.branch,
                "lease_id": lease_id,
                "cleanup_branch": record.cleanup_branch,
                "reason": "lease_released_or_expired",
            }),
        );
        let grant = match crate::action_authorization::authorize_internal_host_effect(
            state,
            "http.global.cleanup_managed_worktrees_for_lease",
            &effect,
        )
        .await
        {
            Ok(grant) => grant,
            Err(error) => {
                result.failures.push(json!({
                    "worktree_id": record.key,
                    "code": error.code(),
                }));
                continue;
            }
        };
        if let Err(error) = grant.revalidate(state, &effect) {
            result.failures.push(json!({
                "worktree_id": record.key,
                "code": error.code(),
            }));
            continue;
        }
        let output = match std::process::Command::new("git")
            .args([
                "-C",
                &record.repo_root,
                "worktree",
                "remove",
                "--force",
                &record.path,
            ])
            .output()
        {
            Ok(output) => output,
            Err(_) => {
                result.failures.push(json!({
                    "path": record.path,
                    "branch": record.branch,
                    "repo_root": record.repo_root,
                    "code": "WORKTREE_REMOVE_FAILED",
                }));
                continue;
            }
        };
        if !output.status.success() {
            result.failures.push(json!({
                "path": record.path,
                "branch": record.branch,
                "repo_root": record.repo_root,
                "code": "WORKTREE_REMOVE_FAILED",
                "stderr": String::from_utf8_lossy(&output.stderr).to_string(),
            }));
            continue;
        }
        if record.cleanup_branch {
            if let Err(error) = grant.revalidate(state, &effect) {
                result.failures.push(json!({
                    "worktree_id": record.key,
                    "code": error.code(),
                }));
                continue;
            }
            match std::process::Command::new("git")
                .args(["-C", &record.repo_root, "branch", "-D", &record.branch])
                .output()
            {
                Ok(branch_output) if branch_output.status.success() => {}
                Ok(branch_output) => {
                    result.failures.push(json!({
                        "path": record.path,
                        "branch": record.branch,
                        "repo_root": record.repo_root,
                        "code": "WORKTREE_BRANCH_DELETE_FAILED",
                        "stderr": String::from_utf8_lossy(&branch_output.stderr).to_string(),
                    }));
                }
                Err(_) => {
                    result.failures.push(json!({
                        "path": record.path,
                        "branch": record.branch,
                        "repo_root": record.repo_root,
                        "code": "WORKTREE_BRANCH_DELETE_FAILED",
                    }));
                }
            }
        }
        state
            .managed_worktrees
            .write()
            .await
            .retain(|_, row| !(row.repo_root == record.repo_root && row.path == record.path));
        result.cleaned_paths.push(record.path);
    }
    result
}

fn resolve_worktree_path(
    repo_root: &str,
    raw: Option<&str>,
    default_path: &StdPath,
) -> Result<PathBuf, StatusCode> {
    let Some(raw) = raw.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(default_path.to_path_buf());
    };
    let candidate = PathBuf::from(raw);
    if candidate.is_absolute()
        || candidate.components().any(|component| {
            matches!(
                component,
                std::path::Component::ParentDir
                    | std::path::Component::RootDir
                    | std::path::Component::Prefix(_)
            )
        })
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = PathBuf::from(repo_root).join(candidate);
    if !is_within_managed_worktree_root(repo_root, &path) {
        return Err(StatusCode::FORBIDDEN);
    }
    Ok(path)
}

fn is_within_managed_worktree_root(repo_root: &str, path: &StdPath) -> bool {
    let managed_root = PathBuf::from(repo_root).join(".tandem").join("worktrees");
    path.starts_with(managed_root)
}

fn worktree_is_registered(repo_root: &str, path: &str) -> Result<bool, StatusCode> {
    let output = std::process::Command::new("git")
        .args(["-C", repo_root, "worktree", "list", "--porcelain"])
        .output()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !output.status.success() {
        return Ok(false);
    }
    let needle = PathBuf::from(path);
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if let Some(value) = line.strip_prefix("worktree ") {
            if PathBuf::from(value) == needle {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

fn annotate_managed_worktree(
    record: &mut serde_json::Map<String, Value>,
    repo_root: &str,
    managed_records: &std::collections::HashMap<String, crate::ManagedWorktreeRecord>,
) {
    let path = record
        .get("worktree")
        .and_then(Value::as_str)
        .or_else(|| record.get("path").and_then(Value::as_str));
    let Some(path) = path else {
        return;
    };
    if let Some(managed) = managed_records
        .values()
        .find(|row| row.repo_root == repo_root && row.path == path)
    {
        record.insert("path".to_string(), Value::String(managed.path.clone()));
        record.insert("branch".to_string(), Value::String(managed.branch.clone()));
        record.insert("base".to_string(), Value::String(managed.base.clone()));
        record.insert("managed".to_string(), Value::Bool(managed.managed));
        record.insert(
            "repo_root".to_string(),
            Value::String(managed.repo_root.clone()),
        );
        record.insert(
            "cleanup_branch".to_string(),
            Value::Bool(managed.cleanup_branch),
        );
        record.insert(
            "task_id".to_string(),
            managed
                .task_id
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        record.insert(
            "owner_run_id".to_string(),
            managed
                .owner_run_id
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        record.insert(
            "lease_id".to_string(),
            managed
                .lease_id
                .clone()
                .map(Value::String)
                .unwrap_or(Value::Null),
        );
        record.insert("registered".to_string(), Value::Bool(true));
    }
}

async fn find_managed_worktree_by_path(
    state: &AppState,
    repo_root: &str,
    path: &str,
) -> Option<crate::ManagedWorktreeRecord> {
    state
        .managed_worktrees
        .read()
        .await
        .values()
        .find(|row| row.repo_root == repo_root && row.path == path)
        .cloned()
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
            "/mcp/catalog":{"get":{"summary":"List embedded MCP remote-pack catalog with connection overlay"}},
            "/mcp/request-capability":{"post":{"summary":"Request human approval for an MCP capability gap"}},
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
            "/workflow-plans/preview":{"post":{"summary":"Preview an engine-owned workflow plan from a raw prompt"}},
            "/workflow-plans/apply":{"post":{"summary":"Compile and persist a previewed workflow plan as automation v2"}},
            "/workflow-plans/chat/start":{"post":{"summary":"Start a workflow plan drafting conversation"}},
            "/workflow-plans/chat/message":{"post":{"summary":"Revise a workflow plan draft with a planning chat message"}},
            "/workflow-plans/chat/reset":{"post":{"summary":"Reset a workflow plan draft back to its initial preview"}},
            "/workflow-plans/{plan_id}":{"get":{"summary":"Fetch a workflow plan draft and planning conversation"}},
            "/optimizations":{"post":{"summary":"Create an optimization campaign for a saved workflow snapshot"}},
            "/optimizations/{id}":{"get":{"summary":"Fetch optimization campaign state"}},
            "/optimizations/{id}/actions":{"post":{"summary":"Control optimization campaign lifecycle or promotion approval"}},
            "/optimizations/{id}/experiments/{experiment_id}":{"get":{"summary":"Fetch optimization experiment detail"}},
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
            "/packs/{selector}/files/{*path}":{"get":{"summary":"Fetch a file from an installed pack"}},
            "/packs/install":{"post":{"summary":"Install tandem pack from local path or URL"}},
            "/packs/install_from_attachment":{"post":{"summary":"Install tandem pack from downloaded attachment path"}},
            "/packs/uninstall":{"post":{"summary":"Uninstall tandem pack"}},
            "/packs/export":{"post":{"summary":"Export installed tandem pack as zip"}},
            "/packs/detect":{"post":{"summary":"Detect tandem pack marker in zip and emit pack.detected"}},
            "/packs/{selector}/updates":{"get":{"summary":"Check updates for installed pack (stub)"}},
            "/packs/{selector}/update":{"post":{"summary":"Apply updates for installed pack (stub)"}},
            "/marketplace/catalog":{"get":{"summary":"Load marketplace pack catalog"}},
            "/marketplace/packs/{pack_id}/files/{*path}":{"get":{"summary":"Fetch a file from a marketplace pack zip"}}
        }
    }))
}

pub(super) async fn instance_dispose() -> Json<Value> {
    Json(json!({"ok": true}))
}

pub(super) async fn run_events(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
    Path(id): Path<String>,
) -> Response {
    if let Some(session_id) = state.run_registry.session_for_run(&id).await {
        let Some(session) = state.storage.get_session(&session_id).await else {
            return StatusCode::NOT_FOUND.into_response();
        };
        if ensure_same_tenant(&tenant_context, &session.tenant_context).is_err() {
            return StatusCode::NOT_FOUND.into_response();
        }
    } else if let Ok(run) = super::context_runs::load_context_run_state(&state, &id).await {
        if ensure_same_tenant(&tenant_context, &run.tenant_context).is_err() {
            return StatusCode::NOT_FOUND.into_response();
        }
    } else {
        return StatusCode::NOT_FOUND.into_response();
    }

    let rx = state.event_bus.subscribe();
    let stream_tenant = tenant_context.clone();
    let stream_run_id = id.clone();
    let initial = tokio_stream::once(Ok::<_, std::convert::Infallible>(
        axum::response::sse::Event::default().data(
            serde_json::to_string(&EngineEvent::new(
                "run.stream.connected",
                json!({ "runID": id }),
            ))
            .unwrap_or_default(),
        ),
    ));
    let live = tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            let event_run = event
                .properties
                .get("runID")
                .or_else(|| event.properties.get("run_id"))
                .and_then(|v| v.as_str());
            if event_run == Some(stream_run_id.as_str())
                && event_visible_to_tenant(&event, &stream_tenant)
            {
                let payload = serde_json::to_string(&event).unwrap_or_default();
                Some(Ok(axum::response::sse::Event::default().data(payload)))
            } else {
                None
            }
        }
        Err(_) => None,
    });
    axum::response::Sse::new(initial.chain(live))
        .keep_alive(
            axum::response::sse::KeepAlive::new().interval(std::time::Duration::from_secs(10)),
        )
        .into_response()
}

pub(super) async fn list_projects(
    State(state): State<AppState>,
    Extension(tenant_context): Extension<TenantContext>,
) -> Json<Value> {
    let sessions = state
        .storage
        .list_sessions_scoped(tandem_core::SessionListScope::Global)
        .await;
    let mut directories = sessions
        .iter()
        .filter(|s| tenant_matches(&tenant_context, &s.tenant_context))
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
