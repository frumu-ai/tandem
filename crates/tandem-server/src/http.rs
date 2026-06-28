use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path as FsPath;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use axum::extract::Extension;
use axum::extract::{Path, Query, State};
use axum::http::header::{self, HeaderValue};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::{Json, Router};
use futures::Stream;
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
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use uuid::Uuid;

use tandem_tools::Tool;
use tandem_types::{
    ApprovalListFilter, ApprovalRequest, CreateSessionRequest, EngineEvent, Message, MessagePart,
    MessagePartInput, MessageRole, RequestPrincipal, SendMessageRequest, Session, TenantContext,
    TodoItem, ToolResult, ToolSchema,
};
pub(crate) use tandem_wire::{ErrorCode, ErrorEnvelope};
use tandem_wire::{WireSession, WireSessionMessage};

use crate::ResourceStoreError;
use crate::{
    capability_resolver::{
        classify_missing_required, providers_for_capability, CapabilityBindingsFile,
        CapabilityBlockingIssue, CapabilityReadinessInput, CapabilityReadinessOutput,
        CapabilityResolveInput,
    },
    mcp_catalog,
    pack_manager::{PackExportRequest, PackInstallRequest, PackUninstallRequest},
    ActiveRun, AppState, DiscordConfigFile, SlackConfigFile, TelegramConfigFile,
};

pub(crate) mod approvals;
mod audit_stream;
mod automation_projection_runtime;
pub(crate) mod bug_monitor;
mod capabilities;
pub(crate) mod channel_automation_drafts;
mod channel_enrollment;
mod channel_interaction_audit;
mod channels_api;
mod coder;
pub(crate) mod config_providers;
pub(crate) mod context_packs;
mod context_run_ledger;
mod context_run_mutation_checkpoints;
pub(crate) mod context_runs;
pub(crate) mod context_types;
pub(crate) mod cross_tenant_grants;
mod discord_interactions;
mod external_actions;
mod global;
mod goal_capability_learning;
pub(crate) mod governance;
mod marketplace;
pub(crate) mod mcp;
mod mcp_connection_inventory;
pub(crate) mod mcp_discovery;
pub(crate) mod mcp_inventory;
pub(crate) mod mcp_run_as;
mod middleware;
mod mission_builder;
mod mission_builder_host;
mod mission_builder_runtime;
mod missions_teams;
mod observability_metrics;
mod optimizations;
mod pack_builder;
mod packs;
mod permissions_questions;
mod presets;
mod resources;
mod router;
mod routes_approvals;
mod routes_automation_webhooks;
mod routes_bug_monitor;
mod routes_capabilities;
mod routes_channel_automation_drafts;
mod routes_coder;
mod routes_config_providers;
mod routes_context;
mod routes_external_actions;
mod routes_global;
mod routes_goal_capability_learning;
mod routes_governance;
mod routes_marketplace;
mod routes_mcp;
mod routes_mission_builder;
mod routes_missions_teams;
mod routes_optimizations;
mod routes_pack_builder;
mod routes_packs;
mod routes_permissions_questions;
mod routes_presets;
mod routes_resources;
mod routes_routines_automations;
mod routes_sessions;
mod routes_setup_understanding;
mod routes_skills_memory;
mod routes_system_api;
mod routes_task_intake;
mod routes_workflow_planner;
mod routes_workflows;
pub(crate) mod routines_automations;
mod runtime_events;
mod session_kb_grounding;
mod session_source;
mod sessions;
mod setup_understanding;
mod skills_memory;
mod slack_interactions;
mod system_api;
mod task_intake;
mod telegram_interactions;
pub(crate) mod workflow_planner;
mod workflow_planner_connector_writers;
mod workflow_planner_host;
mod workflow_planner_policy;
pub(crate) mod workflow_planner_runtime;
mod workflow_planner_transport;
mod workflows;

pub(crate) use skills_memory::{memory_promote_impl, memory_put_impl};

use capabilities::*;
use context_runs::*;
use context_types::*;
use marketplace::*;
use mcp::*;
use mcp_connection_inventory::*;
use pack_builder::*;
use packs::*;
use permissions_questions::*;
use presets::*;
use resources::*;
use session_source::*;
use sessions::*;
use setup_understanding::*;
use skills_memory::*;
use system_api::*;

#[cfg(test)]
pub(crate) use context_runs::session_context_run_id;
pub(crate) use context_runs::sync_workflow_run_blackboard;
#[cfg(test)]
pub(crate) use context_runs::workflow_context_run_id;
pub(crate) use workflow_planner_runtime::compile_plan_to_automation_v2;

#[derive(Debug, Deserialize)]
struct ListSessionsQuery {
    q: Option<String>,
    page: Option<usize>,
    page_size: Option<usize>,
    archived: Option<bool>,
    scope: Option<SessionScope>,
    workspace: Option<String>,
    source: Option<String>,
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
    permission: Option<Vec<serde_json::Value>>,
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
    repo_root: Option<String>,
    path: Option<String>,
    branch: Option<String>,
    base: Option<String>,
    task_id: Option<String>,
    owner_run_id: Option<String>,
    lease_id: Option<String>,
    managed: Option<bool>,
    cleanup_branch: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct WorktreeListQuery {
    repo_root: Option<String>,
    managed_only: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct WorktreeCleanupInput {
    repo_root: Option<String>,
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    remove_orphan_dirs: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct LogInput {
    level: Option<String>,
    message: Option<String>,
    context: Option<Value>,
}

pub type ServerRouter = Router<AppState>;
pub type RouteRegistrar = fn(ServerRouter) -> ServerRouter;
type HttpError = (StatusCode, Json<ErrorEnvelope>);

fn http_error(status: StatusCode, error: impl Into<String>, code: ErrorCode) -> HttpError {
    (status, Json(ErrorEnvelope::new(error.into(), code)))
}

fn session_not_found_error() -> HttpError {
    http_error(
        StatusCode::NOT_FOUND,
        "Session not found",
        ErrorCode::SessionNotFound,
    )
}

fn persistence_error(error: impl Into<String>) -> HttpError {
    http_error(
        StatusCode::INTERNAL_SERVER_ERROR,
        error,
        ErrorCode::PersistenceFailed,
    )
}

pub async fn serve(addr: SocketAddr, state: AppState) -> anyhow::Result<()> {
    serve_with_route_extensions(addr, state, &[]).await
}

/// In hosted/enterprise auth modes the data layers must fail closed on
/// local-implicit tenant context instead of trusting ingress middleware alone
/// (TAN-196). Sets the process-wide defaults that MemoryDatabase and
/// McpRegistry instances inherit at construction; the registry instance held
/// by RuntimeState is flipped in `bootstrap_mcp_servers_when_ready` once the
/// runtime is initialized.
fn apply_strict_tenant_enforcement_defaults(
) -> anyhow::Result<crate::memory::policy_status::MemoryContextPolicyStatus> {
    let mode = crate::config::env::resolve_runtime_auth_mode();
    let strict = crate::memory::policy_status::enterprise_memory_policy_required(
        mode,
        crate::config::env::context_assertion_verifier_configured(),
        crate::config::env::hosted_control_plane_configured(),
    );
    if strict {
        tandem_memory::db::set_strict_tenant_enforcement_default(true);
        tandem_runtime::mcp::set_strict_tenant_enforcement_default(true);
        tandem_tools::set_strict_tenant_enforcement_default(true);
    }
    let status = crate::memory::policy_status::current_memory_context_policy_status();
    status.ensure_startup_ready()?;
    tracing::info!(
        auth_mode = %status.runtime_auth_mode.as_str(),
        effective_memory_auth_mode = %status.effective_memory_auth_mode.as_str(),
        memory_policy_mode = ?status.memory_policy_mode,
        strict_required = status.strict_required,
        context_assertion_verifier_configured = status.context_assertion_verifier_configured,
        hosted_control_plane_configured = status.hosted_control_plane_configured,
        cross_tenant_grant_signing_key_configured = status.cross_tenant_grant_signing_key_configured,
        strict_memory = status.strict_memory_enforcement,
        strict_mcp = status.strict_mcp_enforcement,
        strict_tools = status.strict_tool_enforcement,
        "memory/context tenant policy resolved"
    );
    Ok(status)
}

pub async fn serve_with_route_extensions(
    addr: SocketAddr,
    state: AppState,
    route_extensions: &[RouteRegistrar],
) -> anyhow::Result<()> {
    apply_strict_tenant_enforcement_defaults()?;
    let reaper_state = state.clone();
    let session_part_persister_state = state.clone();
    let session_context_run_journaler_state = state.clone();
    let runtime_event_log_persister_state = state.clone();
    let status_indexer_state = state.clone();
    let routine_scheduler_state = state.clone();
    let routine_executor_state = state.clone();
    let usage_aggregator_state = state.clone();
    let automation_v2_scheduler_state = state.clone();
    let automation_v2_executor_state = state.clone();
    let optimization_scheduler_state = state.clone();
    let workflow_dispatcher_state = state.clone();
    let agent_team_supervisor_state = state.clone();
    let global_memory_ingestor_state = state.clone();
    let bug_monitor_state = state.clone();
    let bug_monitor_log_watcher_state = state.clone();
    let bug_monitor_recovery_sweep_state = state.clone();
    let governance_health_state = state.clone();
    let approval_outbound_state = state.clone();
    let mcp_bootstrap_state = state.clone();
    tokio::spawn(async move {
        bootstrap_mcp_servers_when_ready(mcp_bootstrap_state).await;
    });
    let app = build_router_with_extensions(state.clone(), route_extensions);
    let reaper = tokio::spawn(async move {
        if !reaper_state.wait_until_ready_or_failed(120, 250).await {
            let startup = reaper_state.startup_snapshot().await;
            tracing::warn!(
                component = "run_reaper",
                startup_status = ?startup.status,
                startup_phase = %startup.phase,
                attempt_id = %startup.attempt_id,
                "run reaper exiting before runtime access because startup did not become ready"
            );
            return;
        }
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
    let session_part_persister = tokio::spawn(crate::run_session_part_persister(
        session_part_persister_state,
    ));
    let session_context_run_journaler = tokio::spawn(crate::run_session_context_run_journaler(
        session_context_run_journaler_state,
    ));
    let runtime_event_log_persister = tokio::spawn(crate::run_runtime_event_log_persister(
        runtime_event_log_persister_state,
    ));
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
    let optimization_scheduler = tokio::spawn(crate::run_optimization_scheduler(
        optimization_scheduler_state,
    ));
    let workflow_dispatcher =
        tokio::spawn(crate::run_workflow_dispatcher(workflow_dispatcher_state));
    let agent_team_supervisor = tokio::spawn(crate::run_agent_team_supervisor(
        agent_team_supervisor_state,
    ));
    let bug_monitor = tokio::spawn(crate::run_bug_monitor(bug_monitor_state));
    let bug_monitor_log_watcher = tokio::spawn(
        crate::bug_monitor::log_watcher::run_bug_monitor_log_watcher(bug_monitor_log_watcher_state),
    );
    let bug_monitor_recovery_sweep = tokio::spawn(crate::run_bug_monitor_recovery_sweep(
        bug_monitor_recovery_sweep_state,
    ));
    let global_memory_ingestor =
        tokio::spawn(run_global_memory_ingestor(global_memory_ingestor_state));
    let approval_outbound_cancel = Arc::new(AtomicBool::new(false));
    let approval_outbound_cancel_for_task = approval_outbound_cancel.clone();
    let mut approval_outbound = tokio::spawn(run_approval_outbound(
        approval_outbound_state,
        approval_outbound_cancel_for_task,
    ));
    let shutdown_state = state.clone();
    let shutdown_timeout_secs = crate::config::env::resolve_scheduler_shutdown_timeout_secs();
    let approval_outbound_cancel_for_shutdown = approval_outbound_cancel.clone();

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

    // --- Automation v2 runs archiver (runs at startup, then every 24h) ---
    // Moves terminal (completed/failed/blocked/cancelled) runs older than
    // TANDEM_AUTOMATION_V2_RUNS_RETENTION_DAYS (default 7) from the hot runs
    // file to a sidecar archive file. The hot file is rewritten on every run
    // status change, so keeping it small is critical for persistence
    // throughput. Without this, the file grows unbounded and state writes
    // slow to the point that in-memory state lags on-disk state by minutes.
    let archiver_state = state.clone();
    let _automation_v2_archiver = tokio::spawn(async move {
        // Wait for startup to reach Ready so runtime-backed state is safe.
        loop {
            if archiver_state.is_automation_scheduler_stopping() {
                return;
            }
            let startup = archiver_state.startup_snapshot().await;
            if matches!(startup.status, crate::app::startup::StartupStatus::Ready) {
                break;
            }
            if matches!(startup.status, crate::app::startup::StartupStatus::Failed) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        loop {
            let retention_days: u64 = std::env::var("TANDEM_AUTOMATION_V2_RUNS_RETENTION_DAYS")
                .ok()
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(7);
            if retention_days > 0 {
                match archiver_state
                    .archive_stale_automation_v2_runs(retention_days)
                    .await
                {
                    Ok(n) if n > 0 => {
                        tracing::info!(
                            archived = n,
                            retention_days,
                            "automation v2 archiver: pruned stale terminal runs"
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %e, "automation v2 archiver: archive failed");
                    }
                }
            }
            tokio::time::sleep(Duration::from_secs(24 * 60 * 60)).await;
        }
    });

    let automation_governance_health_checker = tokio::spawn(async move {
        loop {
            if governance_health_state.is_automation_scheduler_stopping() {
                return;
            }
            let startup = governance_health_state.startup_snapshot().await;
            if matches!(startup.status, crate::app::startup::StartupStatus::Ready) {
                break;
            }
            if matches!(startup.status, crate::app::startup::StartupStatus::Failed) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(250)).await;
        }
        loop {
            let interval_ms = governance_health_state
                .automation_governance
                .read()
                .await
                .limits
                .health_check_interval_ms
                .max(60 * 1000);
            match governance_health_state
                .run_automation_governance_health_check()
                .await
            {
                Ok(count) if count > 0 => {
                    tracing::info!(
                        finding_count = count,
                        "automation governance health check recorded findings"
                    );
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(error = %error, "automation governance health check failed");
                }
            }
            tokio::time::sleep(Duration::from_millis(interval_ms)).await;
        }
    });

    // Channel listeners are started during runtime initialization
    // (`initialize_runtime()` in `engine/src/main.rs`) so `serve()` only owns
    // the HTTP server lifecycle.
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            if tokio::signal::ctrl_c().await.is_err() {
                futures::future::pending::<()>().await;
            }
            approval_outbound_cancel_for_shutdown.store(true, Ordering::Relaxed);
            shutdown_state.set_automation_scheduler_stopping(true);
            tokio::time::sleep(Duration::from_secs(shutdown_timeout_secs)).await;
            let failed = shutdown_state
                .fail_running_automation_runs_for_shutdown()
                .await;
            if failed > 0 {
                tracing::warn!(
                    failed_runs = failed,
                    "automation runs marked failed during scheduler shutdown"
                );
            }
        })
        .await;
    reaper.abort();
    session_part_persister.abort();
    session_context_run_journaler.abort();
    runtime_event_log_persister.abort();
    status_indexer.abort();
    routine_scheduler.abort();
    routine_executor.abort();
    usage_aggregator.abort();
    automation_v2_scheduler.abort();
    automation_v2_executor.abort();
    optimization_scheduler.abort();
    workflow_dispatcher.abort();
    agent_team_supervisor.abort();
    bug_monitor.abort();
    bug_monitor_log_watcher.abort();
    bug_monitor_recovery_sweep.abort();
    global_memory_ingestor.abort();
    approval_outbound_cancel.store(true, Ordering::Relaxed);
    let approval_outbound_shutdown_timeout =
        crate::app::approval_outbound::DEFAULT_POLL_INTERVAL + Duration::from_secs(1);
    if tokio::time::timeout(approval_outbound_shutdown_timeout, &mut approval_outbound)
        .await
        .is_err()
    {
        approval_outbound.abort();
    }
    hygiene_task.abort();
    automation_governance_health_checker.abort();
    result?;
    Ok(())
}

struct AppStatePendingApprovalsSource {
    state: AppState,
}

#[async_trait]
impl crate::app::approval_outbound::PendingApprovalsSource for AppStatePendingApprovalsSource {
    async fn list_pending(&self, filter: &ApprovalListFilter) -> Vec<ApprovalRequest> {
        approvals::list_pending_approvals(&self.state, filter).await
    }
}

async fn run_approval_outbound(state: AppState, cancel: Arc<AtomicBool>) {
    if !state.wait_until_ready_or_failed(120, 250).await {
        let startup = state.startup_snapshot().await;
        tracing::warn!(
            component = "approval_outbound",
            startup_status = ?startup.status,
            startup_phase = %startup.phase,
            attempt_id = %startup.attempt_id,
            "approval outbound fan-out exiting before runtime access because startup did not become ready"
        );
        return;
    }

    let effective = state.config.get_effective_value().await;
    let channels = effective
        .get("channels")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();

    let message_map = Arc::new(
        crate::app::state::approval_message_map::ApprovalMessageMap::load_or_default(
            crate::config::paths::resolve_approval_message_map_path(),
        )
        .await,
    );

    let mut notifiers: Vec<Arc<dyn crate::app::approval_outbound::ApprovalNotifier>> = Vec::new();
    if let Some(slack_value) = channels.get("slack") {
        match serde_json::from_value::<SlackConfigFile>(slack_value.clone()) {
            Ok(cfg) if !cfg.bot_token.trim().is_empty() && !cfg.channel_id.trim().is_empty() => {
                let slack_config = tandem_channels::config::SlackConfig {
                    bot_token: cfg.bot_token,
                    channel_id: cfg.channel_id,
                    allowed_users: crate::config::channels::normalize_allowed_users_or_wildcard(
                        cfg.allowed_users,
                    ),
                    mention_only: cfg.mention_only,
                    security_profile: cfg.security_profile,
                };
                notifiers.push(Arc::new(
                    crate::app::notifiers::slack::from_config_with_message_map(
                        slack_config,
                        message_map.clone(),
                    ),
                ));
            }
            Ok(_) => {
                tracing::warn!(
                    target: "tandem_server::approval_outbound",
                    "Slack approval notifier disabled because bot_token or channel_id is empty"
                );
            }
            Err(error) => {
                tracing::warn!(
                    target: "tandem_server::approval_outbound",
                    %error,
                    "Slack approval notifier disabled because config could not be parsed"
                );
            }
        }
    }
    if let Some(discord_value) = channels.get("discord") {
        match serde_json::from_value::<DiscordConfigFile>(discord_value.clone()) {
            Ok(cfg)
                if !cfg.bot_token.trim().is_empty()
                    && cfg
                        .approval_channel_id
                        .as_deref()
                        .map(|value| !value.trim().is_empty())
                        .unwrap_or(false) =>
            {
                let recipient = cfg.approval_channel_id.unwrap_or_default();
                let discord_config = tandem_channels::config::DiscordConfig {
                    bot_token: cfg.bot_token,
                    guild_id: cfg.guild_id,
                    allowed_users: crate::config::channels::normalize_allowed_users_or_wildcard(
                        cfg.allowed_users,
                    ),
                    mention_only: cfg.mention_only,
                    security_profile: cfg.security_profile,
                };
                notifiers.push(Arc::new(
                    crate::app::notifiers::discord::from_config_with_message_map(
                        discord_config,
                        recipient,
                        message_map.clone(),
                    ),
                ));
            }
            Ok(_) => {
                tracing::warn!(
                    target: "tandem_server::approval_outbound",
                    "Discord approval notifier disabled because bot_token or approval_channel_id is empty"
                );
            }
            Err(error) => {
                tracing::warn!(
                    target: "tandem_server::approval_outbound",
                    %error,
                    "Discord approval notifier disabled because config could not be parsed"
                );
            }
        }
    }
    if let Some(telegram_value) = channels.get("telegram") {
        match serde_json::from_value::<TelegramConfigFile>(telegram_value.clone()) {
            Ok(cfg)
                if !cfg.bot_token.trim().is_empty()
                    && cfg
                        .approval_chat_id
                        .as_deref()
                        .map(|value| !value.trim().is_empty())
                        .unwrap_or(false) =>
            {
                let recipient = cfg.approval_chat_id.unwrap_or_default();
                let telegram_config = tandem_channels::config::TelegramConfig {
                    bot_token: cfg.bot_token,
                    allowed_users: crate::config::channels::normalize_allowed_users_or_wildcard(
                        cfg.allowed_users,
                    ),
                    mention_only: cfg.mention_only,
                    style_profile: cfg.style_profile,
                    security_profile: cfg.security_profile,
                };
                notifiers.push(Arc::new(
                    crate::app::notifiers::telegram::from_config_with_message_map(
                        telegram_config,
                        recipient,
                        message_map.clone(),
                    ),
                ));
            }
            Ok(_) => {
                tracing::warn!(
                    target: "tandem_server::approval_outbound",
                    "Telegram approval notifier disabled because bot_token or approval_chat_id is empty"
                );
            }
            Err(error) => {
                tracing::warn!(
                    target: "tandem_server::approval_outbound",
                    %error,
                    "Telegram approval notifier disabled because config could not be parsed"
                );
            }
        }
    }

    if notifiers.is_empty() {
        tracing::info!(
            target: "tandem_server::approval_outbound",
            "approval outbound fan-out has no configured channel notifiers"
        );
        return;
    }

    let source = Arc::new(AppStatePendingApprovalsSource {
        state: state.clone(),
    });
    crate::app::approval_outbound::run_polling_loop(
        source,
        Arc::new(notifiers),
        ApprovalListFilter::default(),
        crate::app::approval_outbound::DEFAULT_POLL_INTERVAL,
        cancel,
    )
    .await;
}

pub fn build_router_with_extensions(
    state: AppState,
    route_extensions: &[RouteRegistrar],
) -> Router {
    router::build_router(state, route_extensions)
}

fn app_router(state: AppState) -> Router {
    build_router_with_extensions(state, &[])
}

fn tenant_matches(a: &TenantContext, b: &TenantContext) -> bool {
    a.org_id == b.org_id && a.workspace_id == b.workspace_id && a.deployment_id == b.deployment_id
}

fn ensure_same_tenant(
    request_tenant: &TenantContext,
    resource_tenant: &TenantContext,
) -> Result<(), StatusCode> {
    if tenant_matches(request_tenant, resource_tenant) {
        Ok(())
    } else {
        Err(StatusCode::NOT_FOUND)
    }
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

pub(super) fn truncate_for_stream(input: &str, max_len: usize) -> String {
    if input.len() <= max_len {
        return input.to_string();
    }
    let mut end = 0usize;
    for (idx, ch) in input.char_indices() {
        let next = idx + ch.len_utf8();
        if next > max_len {
            break;
        }
        end = next;
    }
    let mut out = input[..end].to_string();
    out.push_str("...<truncated>");
    out
}

#[cfg(test)]
mod tests;
