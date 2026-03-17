use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::Path as FsPath;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
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
    mcp_catalog,
    pack_manager::{PackExportRequest, PackInstallRequest, PackUninstallRequest},
    ActiveRun, AppState, ChannelStatus, DiscordConfigFile, SlackConfigFile, TelegramConfigFile,
};

pub(crate) mod bug_monitor;
mod capabilities;
mod channels_api;
mod coder;
mod config_providers;
pub(crate) mod context_runs;
pub(crate) mod context_types;
mod global;
mod mcp;
mod middleware;
mod mission_builder;
mod missions_teams;
mod pack_builder;
mod packs;
mod permissions_questions;
mod presets;
mod resources;
mod router;
mod routes_bug_monitor;
mod routes_capabilities;
mod routes_coder;
mod routes_config_providers;
mod routes_context;
mod routes_global;
mod routes_mcp;
mod routes_mission_builder;
mod routes_missions_teams;
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
mod routes_workflow_planner;
mod routes_workflows;
pub(crate) mod routines_automations;
mod sessions;
mod setup_understanding;
mod skills_memory;
mod system_api;
mod workflow_planner;
mod workflows;

use capabilities::*;
use context_runs::*;
use context_types::*;
use mcp::*;
use pack_builder::*;
use packs::*;
use permissions_questions::*;
use presets::*;
use resources::*;
use sessions::*;
use setup_understanding::*;
use skills_memory::*;
use system_api::*;

#[cfg(test)]
pub(crate) use context_runs::session_context_run_id;
pub(crate) use context_runs::sync_workflow_run_blackboard;
#[cfg(test)]
pub(crate) use context_runs::workflow_context_run_id;

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

#[derive(Debug, Serialize)]
struct ErrorEnvelope {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    code: Option<String>,
}

pub async fn serve(addr: SocketAddr, state: AppState) -> anyhow::Result<()> {
    let reaper_state = state.clone();
    let session_part_persister_state = state.clone();
    let session_context_run_journaler_state = state.clone();
    let status_indexer_state = state.clone();
    let routine_scheduler_state = state.clone();
    let routine_executor_state = state.clone();
    let usage_aggregator_state = state.clone();
    let automation_v2_scheduler_state = state.clone();
    let automation_v2_executor_state = state.clone();
    let workflow_dispatcher_state = state.clone();
    let agent_team_supervisor_state = state.clone();
    let global_memory_ingestor_state = state.clone();
    let bug_monitor_state = state.clone();
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
    let session_part_persister = tokio::spawn(crate::run_session_part_persister(
        session_part_persister_state,
    ));
    let session_context_run_journaler = tokio::spawn(crate::run_session_context_run_journaler(
        session_context_run_journaler_state,
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
    let workflow_dispatcher =
        tokio::spawn(crate::run_workflow_dispatcher(workflow_dispatcher_state));
    let agent_team_supervisor = tokio::spawn(crate::run_agent_team_supervisor(
        agent_team_supervisor_state,
    ));
    let bug_monitor = tokio::spawn(crate::run_bug_monitor(bug_monitor_state));
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
    session_part_persister.abort();
    session_context_run_journaler.abort();
    status_indexer.abort();
    routine_scheduler.abort();
    routine_executor.abort();
    usage_aggregator.abort();
    automation_v2_scheduler.abort();
    automation_v2_executor.abort();
    workflow_dispatcher.abort();
    agent_team_supervisor.abort();
    bug_monitor.abort();
    global_memory_ingestor.abort();
    hygiene_task.abort();
    if let Some(mut set) = channel_listener_set {
        set.abort_all();
    }
    result?;
    Ok(())
}

fn app_router(state: AppState) -> Router {
    router::build_router(state)
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
    let mut out = input[..max_len].to_string();
    out.push_str("...<truncated>");
    out
}

#[cfg(test)]
mod tests;
