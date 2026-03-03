use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::Cursor;
use std::io::Write;
use std::net::SocketAddr;
use std::path::Path as FsPath;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::{path::Component, time::UNIX_EPOCH};

use async_trait::async_trait;
use axum::extract::ws::{Message as WsMessage, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, Request, State};
use axum::http::header::{self, HeaderValue};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::response::Response;
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use base64::Engine;
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
use tandem_orchestrator::{
    AgentInstanceStatus, DefaultMissionReducer, MissionEvent, MissionReducer, MissionSpec,
    NoopMissionReducer, SpawnRequest, SpawnSource, WorkItem, WorkItemStatus,
};
use tandem_skills::{SkillLocation, SkillService, SkillsConflictPolicy};
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
use tandem_wire::{
    WireProviderCatalog, WireProviderEntry, WireProviderModel, WireProviderModelLimit, WireSession,
    WireSessionMessage,
};

use crate::ResourceStoreError;
use crate::{
    agent_teams::{emit_spawn_approved, emit_spawn_denied, emit_spawn_requested},
    capability_resolver::{
        classify_missing_required, providers_for_capability, CapabilityBindingsFile,
        CapabilityBlockingIssue, CapabilityReadinessInput, CapabilityReadinessOutput,
        CapabilityResolveInput,
    },
    evaluate_routine_execution_policy, mcp_catalog,
    pack_builder::PackBuilderTool,
    pack_manager::{PackExportRequest, PackInstallRequest, PackUninstallRequest},
    ActiveRun, AppState, AutomationAgentMcpPolicy, AutomationAgentProfile,
    AutomationAgentToolPolicy, AutomationExecutionPolicy, AutomationFlowSpec, AutomationRunStatus,
    AutomationV2Schedule, AutomationV2Spec, AutomationV2Status, ChannelStatus, DiscordConfigFile,
    RoutineExecutionDecision, RoutineHistoryEvent, RoutineMisfirePolicy, RoutineRunArtifact,
    RoutineRunRecord, RoutineRunStatus, RoutineSchedule, RoutineSpec, RoutineStatus,
    RoutineStoreError, SlackConfigFile, StartupStatus, TelegramConfigFile,
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

#[derive(Debug, Deserialize, Default, Clone, Copy)]
struct RunEventsQuery {
    since_seq: Option<u64>,
    tail: Option<usize>,
}

#[derive(Debug, Deserialize, Default, Clone, Copy)]
struct ContextRunReplayQuery {
    upto_seq: Option<u64>,
    from_checkpoint: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ContextRunStatus {
    Queued,
    Planning,
    Running,
    AwaitingApproval,
    Paused,
    Blocked,
    Failed,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ContextStepStatus {
    Pending,
    Runnable,
    InProgress,
    Blocked,
    Done,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ContextWorkspaceLease {
    workspace_id: String,
    canonical_path: String,
    lease_epoch: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextRunStep {
    step_id: String,
    title: String,
    status: ContextStepStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextRunState {
    run_id: String,
    run_type: String,
    status: ContextRunStatus,
    objective: String,
    workspace: ContextWorkspaceLease,
    #[serde(default)]
    steps: Vec<ContextRunStep>,
    #[serde(default)]
    why_next_step: Option<String>,
    revision: u64,
    created_at_ms: u64,
    updated_at_ms: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct ContextRunCreateInput {
    run_id: Option<String>,
    objective: String,
    run_type: Option<String>,
    workspace: Option<ContextWorkspaceLease>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextRunEventRecord {
    event_id: String,
    run_id: String,
    seq: u64,
    ts_ms: u64,
    #[serde(rename = "type")]
    event_type: String,
    status: ContextRunStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    step_id: Option<String>,
    payload: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ContextRunEventAppendInput {
    #[serde(rename = "type")]
    event_type: String,
    status: ContextRunStatus,
    step_id: Option<String>,
    #[serde(default)]
    payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ContextBlackboardItem {
    id: String,
    ts_ms: u64,
    text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ContextBlackboardArtifact {
    id: String,
    ts_ms: u64,
    path: String,
    artifact_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    step_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    source_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ContextBlackboardSummaries {
    rolling: String,
    latest_context_pack: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ContextBlackboardState {
    #[serde(default)]
    facts: Vec<ContextBlackboardItem>,
    #[serde(default)]
    decisions: Vec<ContextBlackboardItem>,
    #[serde(default)]
    open_questions: Vec<ContextBlackboardItem>,
    #[serde(default)]
    artifacts: Vec<ContextBlackboardArtifact>,
    #[serde(default)]
    summaries: ContextBlackboardSummaries,
    revision: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum ContextBlackboardPatchOp {
    AddFact,
    AddDecision,
    AddOpenQuestion,
    AddArtifact,
    SetRollingSummary,
    SetLatestContextPack,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextBlackboardPatchRecord {
    patch_id: String,
    run_id: String,
    seq: u64,
    ts_ms: u64,
    op: ContextBlackboardPatchOp,
    payload: Value,
}

#[derive(Debug, Clone, Deserialize)]
struct ContextBlackboardPatchInput {
    op: ContextBlackboardPatchOp,
    payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextCheckpointRecord {
    checkpoint_id: String,
    run_id: String,
    seq: u64,
    ts_ms: u64,
    reason: String,
    run_state: ContextRunState,
    blackboard: ContextBlackboardState,
}

#[derive(Debug, Clone, Deserialize)]
struct ContextCheckpointCreateInput {
    reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextReplayDrift {
    mismatch: bool,
    status_mismatch: bool,
    why_next_step_mismatch: bool,
    step_count_mismatch: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct ContextDriverNextInput {
    dry_run: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ContextTodoSyncItemInput {
    id: Option<String>,
    content: String,
    status: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ContextTodoSyncInput {
    todos: Vec<ContextTodoSyncItemInput>,
    source_session_id: Option<String>,
    source_run_id: Option<String>,
    replace: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
struct ContextLeaseValidateInput {
    phase: String,
    current_path: String,
    step_id: Option<String>,
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
struct PackSelectorPath {
    selector: String,
}

#[derive(Debug, Deserialize)]
struct PackDetectInput {
    path: String,
    #[serde(default)]
    attachment_id: Option<String>,
    #[serde(default)]
    connector: Option<String>,
    #[serde(default)]
    channel_id: Option<String>,
    #[serde(default)]
    sender_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PackInstallFromAttachmentInput {
    attachment_id: String,
    path: String,
    #[serde(default)]
    connector: Option<String>,
    #[serde(default)]
    channel_id: Option<String>,
    #[serde(default)]
    sender_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct PackUpdateApplyInput {
    #[serde(default)]
    target_version: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PresetForkInput {
    kind: String,
    source_path: String,
    #[serde(default)]
    target_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct PresetOverrideWriteInput {
    content: String,
}

#[derive(Debug, Deserialize, Default)]
struct PresetOverridesExportInput {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    version: Option<String>,
    #[serde(default)]
    output_path: Option<String>,
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
struct MemoryDemoteInput {
    id: String,
    run_id: String,
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
struct AgentTeamTemplateCreateInput {
    template: tandem_orchestrator::AgentTemplate,
}

#[derive(Debug, Deserialize, Default)]
struct AgentTeamTemplatePatchInput {
    role: Option<tandem_orchestrator::AgentRole>,
    system_prompt: Option<String>,
    skills: Option<Vec<tandem_orchestrator::SkillRef>>,
    default_budget: Option<tandem_orchestrator::BudgetLimit>,
    capabilities: Option<tandem_orchestrator::CapabilitySpec>,
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
    user_id: Option<String>,
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
    state
        .tools
        .register_tool(
            "pack_builder".to_string(),
            Arc::new(PackBuilderTool::new(state.clone())),
        )
        .await;

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

async fn bootstrap_mcp_servers_when_ready(state: AppState) {
    for _ in 0..120 {
        if state.is_ready() {
            bootstrap_mcp_servers(&state).await;
            return;
        }
        let startup = state.startup_snapshot().await;
        if matches!(startup.status, crate::StartupStatus::Failed) {
            tracing::warn!("mcp bootstrap: skipped because runtime startup failed");
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    tracing::warn!("mcp bootstrap: timed out waiting for runtime readiness");
}

async fn bootstrap_mcp_servers(state: &AppState) {
    let mut enabled_servers = state
        .mcp
        .list()
        .await
        .into_iter()
        .filter_map(|(name, server)| if server.enabled { Some(name) } else { None })
        .collect::<Vec<_>>();
    enabled_servers.sort();

    for name in enabled_servers {
        let connected = state.mcp.connect(&name).await;
        if !connected {
            tracing::warn!("mcp bootstrap: failed to connect server '{}'", name);
            continue;
        }
        let count = sync_mcp_tools_for_server(state, &name).await;
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.connected",
            json!({
                "name": name,
                "status": "connected",
                "source": "startup_bootstrap"
            }),
        ));
        state.event_bus.publish(EngineEvent::new(
            "mcp.tools.updated",
            json!({
                "name": name,
                "count": count,
                "source": "startup_bootstrap"
            }),
        ));
        tracing::info!(
            "mcp bootstrap: connected '{}' with {} tools registered",
            name,
            count
        );
    }
}

#[derive(Debug, Deserialize)]
struct ToolExecutionInput {
    tool: String,
    args: Option<Value>,
}

#[derive(Debug, Deserialize, Default)]
struct McpAddInput {
    name: Option<String>,
    transport: Option<String>,
    headers: Option<HashMap<String, String>>,
    enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
struct McpPatchInput {
    enabled: Option<bool>,
}

#[derive(Clone)]
struct McpBridgeTool {
    schema: ToolSchema,
    mcp: tandem_runtime::McpRegistry,
    server_name: String,
    tool_name: String,
}

#[async_trait]
impl Tool for McpBridgeTool {
    fn schema(&self) -> ToolSchema {
        self.schema.clone()
    }

    async fn execute(&self, args: Value) -> anyhow::Result<ToolResult> {
        self.mcp
            .call_tool(&self.server_name, &self.tool_name, args)
            .await
            .map_err(anyhow::Error::msg)
    }
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

async fn packs_list(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let packs = state.pack_manager.list().await.map_err(|err| {
        tracing::warn!("packs list failed: {}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(json!({ "packs": packs })))
}

async fn packs_get(
    State(state): State<AppState>,
    Path(PackSelectorPath { selector }): Path<PackSelectorPath>,
) -> Result<Json<Value>, StatusCode> {
    let inspection = state.pack_manager.inspect(&selector).await.map_err(|err| {
        if err.to_string().contains("not found") {
            StatusCode::NOT_FOUND
        } else {
            tracing::warn!("pack inspect failed: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })?;
    Ok(Json(json!({
        "pack": inspection,
    })))
}

async fn packs_install(
    State(state): State<AppState>,
    Json(input): Json<PackInstallRequest>,
) -> Result<Json<Value>, StatusCode> {
    state.event_bus.publish(EngineEvent::new(
        "pack.install.started",
        json!({
            "source": input.source,
            "path": input.path,
            "url": input.url,
        }),
    ));
    let result = state.pack_manager.install(input).await;
    match result {
        Ok(installed) => {
            state.event_bus.publish(EngineEvent::new(
                "pack.install.succeeded",
                json!({
                    "pack_id": installed.pack_id,
                    "name": installed.name,
                    "version": installed.version,
                }),
            ));
            state.event_bus.publish(EngineEvent::new(
                "registry.updated",
                json!({ "entity": "packs" }),
            ));
            Ok(Json(json!({ "installed": installed })))
        }
        Err(err) => {
            state.event_bus.publish(EngineEvent::new(
                "pack.install.failed",
                json!({
                    "error": err.to_string(),
                    "code": "pack_install_failed",
                }),
            ));
            Err(StatusCode::BAD_REQUEST)
        }
    }
}

async fn packs_install_from_attachment(
    State(state): State<AppState>,
    Json(input): Json<PackInstallFromAttachmentInput>,
) -> Result<Json<Value>, StatusCode> {
    let source = json!({
        "kind": "attachment",
        "attachment_id": input.attachment_id,
        "connector": input.connector,
        "channel_id": input.channel_id,
        "sender_id": input.sender_id,
    });
    packs_install(
        State(state),
        Json(PackInstallRequest {
            path: Some(input.path),
            url: None,
            source,
        }),
    )
    .await
}

async fn packs_uninstall(
    State(state): State<AppState>,
    Json(input): Json<PackUninstallRequest>,
) -> Result<Json<Value>, StatusCode> {
    let removed = state.pack_manager.uninstall(input).await.map_err(|err| {
        if err.to_string().contains("not found") {
            StatusCode::NOT_FOUND
        } else {
            tracing::warn!("pack uninstall failed: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })?;
    state.event_bus.publish(EngineEvent::new(
        "registry.updated",
        json!({ "entity": "packs" }),
    ));
    Ok(Json(json!({ "removed": removed })))
}

async fn packs_export(
    State(state): State<AppState>,
    Json(input): Json<PackExportRequest>,
) -> Result<Json<Value>, StatusCode> {
    let exported = state.pack_manager.export(input).await.map_err(|err| {
        tracing::warn!("pack export failed: {}", err);
        StatusCode::BAD_REQUEST
    })?;
    Ok(Json(json!({ "exported": exported })))
}

async fn packs_updates_get(
    State(state): State<AppState>,
    Path(PackSelectorPath { selector }): Path<PackSelectorPath>,
) -> Result<Json<Value>, StatusCode> {
    let inspection = state.pack_manager.inspect(&selector).await.map_err(|err| {
        if err.to_string().contains("not found") {
            StatusCode::NOT_FOUND
        } else {
            tracing::warn!("pack updates check failed: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })?;
    Ok(Json(json!({
        "pack_id": inspection.installed.pack_id,
        "name": inspection.installed.name,
        "current_version": inspection.installed.version,
        "updates": [],
        "permissions_diff": {
            "added_required_capabilities": [],
            "removed_required_capabilities": [],
            "added_provider_specific_dependencies": [],
            "removed_provider_specific_dependencies": [],
            "routine_scope_changed": false
        },
        "reapproval_required": false
    })))
}

async fn packs_update_post(
    State(state): State<AppState>,
    Path(PackSelectorPath { selector }): Path<PackSelectorPath>,
    Json(input): Json<PackUpdateApplyInput>,
) -> Result<Json<Value>, StatusCode> {
    let inspection = state.pack_manager.inspect(&selector).await.map_err(|err| {
        if err.to_string().contains("not found") {
            StatusCode::NOT_FOUND
        } else {
            tracing::warn!("pack update apply failed: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        }
    })?;
    state.event_bus.publish(EngineEvent::new(
        "pack.update.not_available",
        json!({
            "pack_id": inspection.installed.pack_id,
            "name": inspection.installed.name,
            "current_version": inspection.installed.version,
            "target_version": input.target_version,
            "reason": "updates_not_implemented",
        }),
    ));
    Ok(Json(json!({
        "updated": false,
        "pack_id": inspection.installed.pack_id,
        "name": inspection.installed.name,
        "current_version": inspection.installed.version,
        "target_version": input.target_version,
        "reason": "updates_not_implemented",
        "permissions_diff": {
            "added_required_capabilities": [],
            "removed_required_capabilities": [],
            "added_provider_specific_dependencies": [],
            "removed_provider_specific_dependencies": [],
            "routine_scope_changed": false
        },
        "reapproval_required": false
    })))
}

async fn packs_detect(
    State(state): State<AppState>,
    Json(input): Json<PackDetectInput>,
) -> Result<Json<Value>, StatusCode> {
    let path = PathBuf::from(&input.path);
    let is_pack = state.pack_manager.detect(&path).await.map_err(|err| {
        tracing::warn!("pack detect failed: {}", err);
        StatusCode::BAD_REQUEST
    })?;
    if is_pack {
        state.event_bus.publish(EngineEvent::new(
            "pack.detected",
            json!({
                "path": input.path,
                "attachment_id": input.attachment_id,
                "connector": input.connector,
                "channel_id": input.channel_id,
                "sender_id": input.sender_id,
                "marker": "tandempack.yaml",
            }),
        ));
    }
    Ok(Json(json!({
        "is_pack": is_pack,
        "marker": "tandempack.yaml",
    })))
}

async fn capabilities_bindings_get(
    State(state): State<AppState>,
) -> Result<Json<Value>, StatusCode> {
    let bindings = state
        .capability_resolver
        .list_bindings()
        .await
        .map_err(|err| {
            tracing::warn!("capability bindings get failed: {}", err);
            StatusCode::INTERNAL_SERVER_ERROR
        })?;
    Ok(Json(json!({ "bindings": bindings })))
}

async fn capabilities_bindings_put(
    State(state): State<AppState>,
    Json(file): Json<CapabilityBindingsFile>,
) -> Result<Json<Value>, StatusCode> {
    state
        .capability_resolver
        .set_bindings(file)
        .await
        .map_err(|err| {
            tracing::warn!("capability bindings put failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;
    Ok(Json(json!({ "ok": true })))
}

async fn capabilities_discovery(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let discovered = state
        .capability_resolver
        .discover_from_runtime(state.mcp.list_tools().await, state.tools.list().await)
        .await;
    Ok(Json(json!({ "tools": discovered })))
}

async fn capabilities_resolve(
    State(state): State<AppState>,
    Json(input): Json<CapabilityResolveInput>,
) -> Result<Response, StatusCode> {
    let discovered = state
        .capability_resolver
        .discover_from_runtime(state.mcp.list_tools().await, state.tools.list().await)
        .await;
    let result = state
        .capability_resolver
        .resolve(input.clone(), discovered)
        .await
        .map_err(|err| {
            tracing::warn!("capability resolve failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;
    if !result.missing_required.is_empty() {
        let bindings = state
            .capability_resolver
            .list_bindings()
            .await
            .unwrap_or_else(|_| CapabilityBindingsFile::default());
        let mut suggestions = HashMap::<String, Vec<String>>::new();
        for missing in &result.missing_required {
            let rows = bindings
                .bindings
                .iter()
                .filter(|row| row.capability_id == *missing)
                .map(|row| format!("{}:{}", row.provider, row.tool_name))
                .collect::<Vec<_>>();
            suggestions.insert(missing.clone(), rows);
        }
        let workflow_id = input
            .workflow_id
            .clone()
            .unwrap_or_else(|| "unknown_workflow".to_string());
        let payload = crate::capability_resolver::CapabilityResolver::missing_capability_error(
            &workflow_id,
            &result.missing_required,
            &suggestions,
        );
        return Ok((StatusCode::CONFLICT, Json(payload)).into_response());
    }
    Ok(Json(json!({ "resolution": result })).into_response())
}

async fn capabilities_readiness(
    State(state): State<AppState>,
    Json(input): Json<CapabilityReadinessInput>,
) -> Result<Response, StatusCode> {
    let discovered = state
        .capability_resolver
        .discover_from_runtime(state.mcp.list_tools().await, state.tools.list().await)
        .await;
    let resolve_input = CapabilityResolveInput {
        workflow_id: input.workflow_id.clone(),
        required_capabilities: input.required_capabilities.clone(),
        optional_capabilities: input.optional_capabilities.clone(),
        provider_preference: input.provider_preference.clone(),
        available_tools: input.available_tools.clone(),
    };
    let result = state
        .capability_resolver
        .resolve(resolve_input, discovered)
        .await
        .map_err(|err| {
            tracing::warn!("capability readiness resolve failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;

    let bindings = state
        .capability_resolver
        .list_bindings()
        .await
        .unwrap_or_else(|_| CapabilityBindingsFile::default());
    let (missing_required_capabilities, unbound_capabilities) =
        classify_missing_required(&bindings, &result.missing_required);

    let mcp_servers = state.mcp.list().await;
    let enabled_servers = mcp_servers
        .values()
        .filter(|server| server.enabled)
        .collect::<Vec<_>>();
    let connected_servers = enabled_servers
        .iter()
        .filter(|server| server.connected)
        .map(|server| server.name.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();

    let mut required_providers = unbound_capabilities
        .iter()
        .flat_map(|capability_id| providers_for_capability(&bindings, capability_id))
        .collect::<Vec<_>>();
    required_providers.sort();
    required_providers.dedup();

    let mut missing_servers = Vec::new();
    let mut disconnected_servers = Vec::new();
    for provider in &required_providers {
        match provider.as_str() {
            "custom" => {}
            "mcp" => {
                if enabled_servers.is_empty() {
                    missing_servers.push(provider.clone());
                } else if connected_servers.is_empty() {
                    disconnected_servers.push(provider.clone());
                }
            }
            name => {
                let any_enabled = enabled_servers
                    .iter()
                    .any(|server| server.name.eq_ignore_ascii_case(name));
                if !any_enabled {
                    missing_servers.push(provider.clone());
                    continue;
                }
                let any_connected = connected_servers.contains(name);
                if !any_connected {
                    disconnected_servers.push(provider.clone());
                }
            }
        }
    }
    missing_servers.sort();
    missing_servers.dedup();
    disconnected_servers.sort();
    disconnected_servers.dedup();

    let mut auth_pending_tools = mcp_servers
        .values()
        .filter(|server| server.connected)
        .flat_map(|server| {
            server.pending_auth_by_tool.keys().map(move |tool| {
                format!(
                    "mcp.{}.{}",
                    mcp_namespace_segment(&server.name),
                    mcp_namespace_segment(tool)
                )
            })
        })
        .collect::<Vec<_>>();
    auth_pending_tools.sort();
    auth_pending_tools.dedup();

    let missing_secret_refs = Vec::<String>::new();
    let mut blocking_issues = Vec::<CapabilityBlockingIssue>::new();
    if !missing_required_capabilities.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "missing_required_capabilities".to_string(),
            message: "Some required capabilities do not have any bindings.".to_string(),
            capability_ids: missing_required_capabilities.clone(),
            providers: Vec::new(),
            tools: Vec::new(),
        });
    }
    if !unbound_capabilities.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "unbound_capabilities".to_string(),
            message: "Some required capabilities have bindings, but no available runtime tools."
                .to_string(),
            capability_ids: unbound_capabilities.clone(),
            providers: required_providers.clone(),
            tools: Vec::new(),
        });
    }
    if !missing_servers.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "missing_mcp_servers".to_string(),
            message: "Required provider servers are not configured.".to_string(),
            capability_ids: Vec::new(),
            providers: missing_servers.clone(),
            tools: Vec::new(),
        });
    }
    if !disconnected_servers.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "disconnected_mcp_servers".to_string(),
            message: "Required provider servers are configured but disconnected.".to_string(),
            capability_ids: Vec::new(),
            providers: disconnected_servers.clone(),
            tools: Vec::new(),
        });
    }
    if !auth_pending_tools.is_empty() {
        blocking_issues.push(CapabilityBlockingIssue {
            code: "auth_pending_tools".to_string(),
            message: "At least one MCP tool still requires authorization.".to_string(),
            capability_ids: Vec::new(),
            providers: Vec::new(),
            tools: auth_pending_tools.clone(),
        });
    }

    let mut recommendations = Vec::<String>::new();
    if !missing_required_capabilities.is_empty() {
        recommendations.push(
            "Add capability bindings for each missing required capability in /capabilities/bindings."
                .to_string(),
        );
    }
    if !unbound_capabilities.is_empty() {
        recommendations.push(
            "Connect/refresh MCP servers so required capability bindings match discovered tools."
                .to_string(),
        );
    }
    if !missing_servers.is_empty() {
        recommendations.push("Configure missing MCP servers in /mcp and reconnect.".to_string());
    }
    if !disconnected_servers.is_empty() {
        recommendations.push("Connect and refresh disconnected MCP servers.".to_string());
    }
    if !auth_pending_tools.is_empty() {
        recommendations.push(
            "Complete MCP authorization flow for pending tools, then refresh server tools."
                .to_string(),
        );
    }

    let runnable = blocking_issues.is_empty() || input.allow_unbound;
    let output = CapabilityReadinessOutput {
        workflow_id: input
            .workflow_id
            .clone()
            .unwrap_or_else(|| "unknown_workflow".to_string()),
        runnable,
        resolved: result.resolved,
        missing_required_capabilities,
        unbound_capabilities,
        missing_optional_capabilities: result.missing_optional,
        missing_servers,
        disconnected_servers,
        auth_pending_tools,
        missing_secret_refs,
        considered_bindings: result.considered_bindings,
        recommendations,
        blocking_issues,
    };
    let status = if output.runnable {
        StatusCode::OK
    } else {
        StatusCode::CONFLICT
    };
    state.event_bus.publish(EngineEvent::new(
        "capabilities.readiness.evaluated",
        json!({
            "workflow_id": output.workflow_id,
            "runnable": output.runnable,
            "blocking_issue_count": output.blocking_issues.len(),
        }),
    ));
    Ok((status, Json(json!({ "readiness": output }))).into_response())
}

async fn presets_index(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let index = state.preset_registry.index().await.map_err(|err| {
        tracing::warn!("presets index failed: {}", err);
        StatusCode::INTERNAL_SERVER_ERROR
    })?;
    Ok(Json(json!({ "index": index })))
}

async fn presets_compose_preview(
    Json(input): Json<crate::preset_composer::PromptComposeInput>,
) -> Result<Json<Value>, StatusCode> {
    let out = crate::preset_composer::compose(input);
    Ok(Json(json!({ "composition": out })))
}

async fn presets_fork(
    State(state): State<AppState>,
    Json(input): Json<PresetForkInput>,
) -> Result<Json<Value>, StatusCode> {
    let source_path = std::path::PathBuf::from(&input.source_path);
    let path = state
        .preset_registry
        .fork_to_override(&input.kind, &source_path, input.target_id.as_deref())
        .await
        .map_err(|err| {
            tracing::warn!("preset fork failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;
    state.event_bus.publish(EngineEvent::new(
        "registry.updated",
        json!({ "entity": "presets" }),
    ));
    Ok(Json(json!({
        "forked": true,
        "path": path.to_string_lossy(),
    })))
}

async fn presets_override_put(
    State(state): State<AppState>,
    Path((kind, id)): Path<(String, String)>,
    Json(input): Json<PresetOverrideWriteInput>,
) -> Result<Json<Value>, StatusCode> {
    let path = state
        .preset_registry
        .save_override(&kind, &id, &input.content)
        .await
        .map_err(|err| {
            tracing::warn!("preset override put failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;
    state.event_bus.publish(EngineEvent::new(
        "registry.updated",
        json!({ "entity": "presets" }),
    ));
    Ok(Json(json!({
        "saved": true,
        "path": path.to_string_lossy(),
    })))
}

async fn presets_override_delete(
    State(state): State<AppState>,
    Path((kind, id)): Path<(String, String)>,
) -> Result<Json<Value>, StatusCode> {
    let removed = state
        .preset_registry
        .delete_override(&kind, &id)
        .await
        .map_err(|err| {
            tracing::warn!("preset override delete failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;
    if removed {
        state.event_bus.publish(EngineEvent::new(
            "registry.updated",
            json!({ "entity": "presets" }),
        ));
    }
    Ok(Json(json!({ "removed": removed })))
}

async fn presets_capability_summary(
    Json(input): Json<crate::preset_summary::CapabilitySummaryInput>,
) -> Result<Json<Value>, StatusCode> {
    let summary = crate::preset_summary::summarize(input);
    Ok(Json(json!({ "summary": summary })))
}

async fn presets_export_overrides(
    State(state): State<AppState>,
    Json(input): Json<PresetOverridesExportInput>,
) -> Result<Json<Value>, StatusCode> {
    let name = input.name.as_deref().unwrap_or("preset-overrides");
    let version = input.version.as_deref().unwrap_or("0.1.0");
    let exported = state
        .preset_registry
        .export_overrides(name, version, input.output_path.as_deref())
        .await
        .map_err(|err| {
            tracing::warn!("preset overrides export failed: {}", err);
            StatusCode::BAD_REQUEST
        })?;
    Ok(Json(json!({ "exported": exported })))
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
        .route("/global/storage/files", get(global_storage_files))
        .route("/global/storage/repair", post(global_storage_repair))
        .route(
            "/global/config",
            get(global_config).patch(global_config_patch),
        )
        .route("/global/dispose", post(global_dispose))
        .route("/event", get(events))
        .route("/run/{id}/events", get(run_events))
        .route("/api/run/{id}/events", get(run_events))
        .route(
            "/context/runs",
            post(context_run_create).get(context_run_list),
        )
        .route(
            "/context/runs/{run_id}",
            get(context_run_get).put(context_run_put),
        )
        .route(
            "/context/runs/{run_id}/events",
            get(context_run_events).post(context_run_event_append),
        )
        .route(
            "/context/runs/{run_id}/todos/sync",
            post(context_run_todos_sync),
        )
        .route(
            "/context/runs/{run_id}/events/stream",
            get(context_run_events_stream),
        )
        .route(
            "/context/runs/{run_id}/lease/validate",
            post(context_run_lease_validate),
        )
        .route(
            "/context/runs/{run_id}/blackboard",
            get(context_run_blackboard_get),
        )
        .route(
            "/context/runs/{run_id}/blackboard/patches",
            post(context_run_blackboard_patch),
        )
        .route(
            "/context/runs/{run_id}/checkpoints",
            post(context_run_checkpoint_create),
        )
        .route(
            "/context/runs/{run_id}/checkpoints/latest",
            get(context_run_checkpoint_latest),
        )
        .route("/context/runs/{run_id}/replay", get(context_run_replay))
        .route(
            "/context/runs/{run_id}/driver/next",
            post(context_run_driver_next),
        )
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
        .route(
            "/config/identity",
            get(get_config_identity).patch(patch_config_identity),
        )
        .route("/config/providers", get(config_providers))
        .route("/mcp", get(list_mcp).post(add_mcp))
        .route("/mcp/{name}/connect", post(connect_mcp))
        .route("/mcp/{name}/disconnect", post(disconnect_mcp))
        .route(
            "/mcp/{name}",
            axum::routing::patch(patch_mcp).delete(delete_mcp),
        )
        .route("/mcp/{name}/refresh", post(refresh_mcp))
        .route("/mcp/{name}/auth", post(auth_mcp).delete(delete_auth_mcp))
        .route("/mcp/{name}/auth/callback", post(callback_mcp))
        .route("/mcp/{name}/auth/authenticate", post(authenticate_mcp))
        .route("/mcp/catalog", get(mcp_catalog_index))
        .route("/mcp/catalog/{slug}/toml", get(mcp_catalog_toml))
        .route("/mcp/tools", get(mcp_tools))
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
        .route("/memory/demote", post(memory_demote))
        .route("/memory/search", post(memory_search))
        .route("/memory/audit", get(memory_audit))
        .route("/memory", get(memory_list))
        .route("/memory/{id}", axum::routing::delete(memory_delete))
        .route("/packs", get(packs_list))
        .route("/packs/{selector}", get(packs_get))
        .route("/packs/install", post(packs_install))
        .route(
            "/packs/install_from_attachment",
            post(packs_install_from_attachment),
        )
        .route("/packs/uninstall", post(packs_uninstall))
        .route("/packs/export", post(packs_export))
        .route("/packs/detect", post(packs_detect))
        .route("/packs/{selector}/updates", get(packs_updates_get))
        .route("/packs/{selector}/update", post(packs_update_post))
        .route(
            "/capabilities/bindings",
            get(capabilities_bindings_get).put(capabilities_bindings_put),
        )
        .route("/capabilities/discovery", get(capabilities_discovery))
        .route("/capabilities/resolve", post(capabilities_resolve))
        .route("/capabilities/readiness", post(capabilities_readiness))
        .route("/presets/index", get(presets_index))
        .route("/presets/compose/preview", post(presets_compose_preview))
        .route("/presets/fork", post(presets_fork))
        .route(
            "/presets/overrides/{kind}/{id}",
            put(presets_override_put).delete(presets_override_delete),
        )
        .route(
            "/presets/capability_summary",
            post(presets_capability_summary),
        )
        .route("/presets/export_overrides", post(presets_export_overrides))
        .route("/channels/config", get(channels_config))
        .route("/channels/status", get(channels_status))
        .route("/channels/{name}/verify", post(channels_verify))
        .route(
            "/channels/{name}",
            put(channels_put).delete(channels_delete),
        )
        .route("/admin/reload-config", post(admin_reload_config))
        .route("/mission", get(mission_list).post(mission_create))
        .route("/mission/{id}", get(mission_get))
        .route("/mission/{id}/event", post(mission_apply_event))
        .route(
            "/agent-team/templates",
            get(agent_team_templates).post(agent_team_template_create),
        )
        .route(
            "/agent-team/templates/{id}",
            patch(agent_team_template_patch).delete(agent_team_template_delete),
        )
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
        .route("/routines/runs", get(routines_runs_all))
        .route("/routines/{id}/runs", get(routines_runs))
        .route("/routines/runs/{run_id}", get(routines_run_get))
        .route(
            "/routines/runs/{run_id}/approve",
            post(routines_run_approve),
        )
        .route("/routines/runs/{run_id}/deny", post(routines_run_deny))
        .route("/routines/runs/{run_id}/pause", post(routines_run_pause))
        .route("/routines/runs/{run_id}/resume", post(routines_run_resume))
        .route(
            "/routines/runs/{run_id}/artifacts",
            get(routines_run_artifacts).post(routines_run_artifact_add),
        )
        .route(
            "/automations",
            get(automations_list).post(automations_create),
        )
        .route("/automations/events", get(automations_events))
        .route(
            "/automations/{id}",
            axum::routing::patch(automations_patch).delete(automations_delete),
        )
        .route("/automations/{id}/run_now", post(automations_run_now))
        .route("/automations/{id}/history", get(automations_history))
        .route("/automations/runs", get(automations_runs_all))
        .route("/automations/{id}/runs", get(automations_runs))
        .route("/automations/runs/{run_id}", get(automations_run_get))
        .route(
            "/automations/runs/{run_id}/approve",
            post(automations_run_approve),
        )
        .route(
            "/automations/runs/{run_id}/deny",
            post(automations_run_deny),
        )
        .route(
            "/automations/runs/{run_id}/pause",
            post(automations_run_pause),
        )
        .route(
            "/automations/runs/{run_id}/resume",
            post(automations_run_resume),
        )
        .route(
            "/automations/runs/{run_id}/artifacts",
            get(automations_run_artifacts).post(automations_run_artifact_add),
        )
        .route(
            "/automations/v2",
            get(automations_v2_list).post(automations_v2_create),
        )
        .route("/automations/v2/events", get(automations_v2_events))
        .route(
            "/automations/v2/{id}",
            get(automations_v2_get)
                .patch(automations_v2_patch)
                .delete(automations_v2_delete),
        )
        .route("/automations/v2/{id}/run_now", post(automations_v2_run_now))
        .route("/automations/v2/{id}/pause", post(automations_v2_pause))
        .route("/automations/v2/{id}/resume", post(automations_v2_resume))
        .route("/automations/v2/{id}/runs", get(automations_v2_runs))
        .route("/automations/v2/runs/{run_id}", get(automations_v2_run_get))
        .route(
            "/automations/v2/runs/{run_id}/pause",
            post(automations_v2_run_pause),
        )
        .route(
            "/automations/v2/runs/{run_id}/resume",
            post(automations_v2_run_resume),
        )
        .route(
            "/automations/v2/runs/{run_id}/cancel",
            post(automations_v2_run_cancel),
        )
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
        .get("x-agent-token")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        return Some(token.to_string());
    }
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

fn sanitize_relative_subpath(raw: Option<&str>) -> Result<PathBuf, StatusCode> {
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
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(StatusCode::BAD_REQUEST);
    }
    Ok(candidate)
}

async fn global_storage_files(
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

    for entry in WalkBuilder::new(&base).build().flatten() {
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
    headers: HeaderMap,
    Query(query): Query<ListSessionsQuery>,
) -> Json<Vec<WireSession>> {
    let request_id = request_id_from_headers(&headers);
    let started = Instant::now();
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
    let elapsed_ms = started.elapsed().as_millis();
    tracing::info!(
        "session.list request_id={} scope={:?} matched={} returned={} page={} page_size={} elapsed_ms={}",
        request_id,
        effective_scope,
        total_after_scope,
        items.len(),
        page,
        page_size,
        elapsed_ms
    );
    if elapsed_ms >= 1_000 {
        tracing::warn!(
            "slow request request_id={} route=GET /session elapsed_ms={} scope={:?} archived_filter={}",
            request_id,
            elapsed_ms,
            effective_scope,
            query.archived.is_some()
        );
    }
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
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<WireSession>, StatusCode> {
    let request_id = request_id_from_headers(&headers);
    let started = Instant::now();
    let result = state
        .storage
        .get_session(&id)
        .await
        .map(|session| Json(session.into()))
        .ok_or(StatusCode::NOT_FOUND);
    let elapsed_ms = started.elapsed().as_millis();
    let status = if result.is_ok() { "ok" } else { "not_found" };
    tracing::info!(
        "session.get request_id={} session_id={} status={} elapsed_ms={}",
        request_id,
        id,
        status,
        elapsed_ms
    );
    if elapsed_ms >= 500 {
        tracing::warn!(
            "slow request request_id={} route=GET /session/{{id}} session_id={} elapsed_ms={}",
            request_id,
            id,
            elapsed_ms
        );
    }
    result
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
        client_id,
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
            client_id,
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
        client_id,
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
    client_id: Option<String>,
) {
    tokio::spawn(async move {
        let _ = execute_run(state, session_id, run_id, req, correlation_id, client_id).await;
    });
}

async fn execute_run(
    state: AppState,
    session_id: String,
    run_id: String,
    req: SendMessageRequest,
    correlation_id: Option<String>,
    _client_id: Option<String>,
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

    // Consolidate memory if enabled
    let effective = state.config.get_effective_value().await;
    let parsed: crate::EffectiveAppConfig = serde_json::from_value(effective).unwrap_or_default();
    if parsed.memory_consolidation.enabled {
        let providers = state.providers.clone();
        let consolidation_cfg = parsed.memory_consolidation.clone();
        let session_id_clone = session_id.clone();
        tokio::spawn(async move {
            if let Ok(paths) = tandem_core::resolve_shared_paths() {
                // Open a fresh connection for the background task
                if let Ok(mem) =
                    tandem_memory::manager::MemoryManager::new(&paths.memory_db_path).await
                {
                    if let Err(e) = mem
                        .consolidate_session(
                            &session_id_clone,
                            None,
                            &providers,
                            &consolidation_cfg,
                        )
                        .await
                    {
                        tracing::warn!(
                            "memory consolidation failed for session {session_id_clone}: {e}"
                        );
                    }
                }
            }
        });
    }

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
    let lower = message.to_ascii_lowercase();
    if is_os_mismatch_error(message) {
        return "OS_MISMATCH";
    }
    if lower.contains("provider_server_error")
        || lower.contains("internal server error")
        || lower.contains("provider stream chunk error")
        || lower.contains("json error injected into sse stream")
    {
        return "PROVIDER_SERVER_ERROR";
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
) -> Result<Json<Value>, (StatusCode, Json<ErrorEnvelope>)> {
    let accepted = matches!(
        input.reply.as_str(),
        "once" | "always" | "reject" | "allow" | "deny"
    );
    if !accepted {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorEnvelope {
                error: "reply must be one of once|always|reject|allow|deny".to_string(),
                code: Some("invalid_permission_reply".to_string()),
            }),
        ));
    }
    let ok = state.permissions.reply(&id, &input.reply).await;
    if !ok {
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorEnvelope {
                error: "Permission request not found".to_string(),
                code: Some("permission_request_not_found".to_string()),
            }),
        ));
    }
    Ok(Json(json!({
        "ok": true,
        "requestID": id,
        "reply": input.reply,
        "status": "applied",
        "persistedRule": matches!(input.reply.as_str(), "always" | "allow")
    })))
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
        ("openai", "OpenAI", "gpt-5.2"),
        ("anthropic", "Anthropic", "claude-sonnet-4-6"),
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
fn provider_env_candidates(provider_id: &str) -> Vec<String> {
    let normalized = provider_id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
        .collect::<String>()
        .to_ascii_uppercase();

    let mut out = vec![format!("{}_API_KEY", normalized)];
    match provider_id.to_ascii_lowercase().as_str() {
        "openai" => out.push("OPENAI_API_KEY".to_string()),
        "openrouter" => out.push("OPENROUTER_API_KEY".to_string()),
        "anthropic" => out.push("ANTHROPIC_API_KEY".to_string()),
        "groq" => out.push("GROQ_API_KEY".to_string()),
        "mistral" => out.push("MISTRAL_API_KEY".to_string()),
        "together" => out.push("TOGETHER_API_KEY".to_string()),
        "azure" => out.push("AZURE_OPENAI_API_KEY".to_string()),
        "vertex" => out.push("VERTEX_API_KEY".to_string()),
        "bedrock" => out.push("BEDROCK_API_KEY".to_string()),
        "copilot" => out.push("GITHUB_TOKEN".to_string()),
        "cohere" => out.push("COHERE_API_KEY".to_string()),
        _ => {}
    }
    out.sort();
    out.dedup();
    out
}

fn provider_has_env_secret(provider_id: &str) -> bool {
    provider_env_candidates(provider_id).into_iter().any(|key| {
        std::env::var(&key)
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
    })
}

async fn provider_auth(State(state): State<AppState>) -> Json<Value> {
    let cfg = state.config.get_effective_value().await;
    let providers_cfg = cfg
        .get("providers")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    let persisted = tandem_core::load_provider_auth();
    let persisted_ids = persisted
        .keys()
        .map(|id| id.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let runtime_auth = state.auth.read().await.clone();
    let connected = state
        .providers
        .list()
        .await
        .into_iter()
        .map(|provider| provider.id.to_ascii_lowercase())
        .collect::<std::collections::HashSet<_>>();
    let mut known_ids = std::collections::HashSet::new();

    if let Some(default_id) = cfg.get("default_provider").and_then(Value::as_str) {
        let normalized = default_id.trim().to_ascii_lowercase();
        if !normalized.is_empty() {
            known_ids.insert(normalized);
        }
    }
    known_ids.extend(providers_cfg.keys().map(|id| id.to_ascii_lowercase()));
    known_ids.extend(connected.iter().cloned());
    known_ids.extend(runtime_auth.keys().map(|id| id.to_ascii_lowercase()));
    known_ids.extend(persisted_ids.iter().cloned());

    let mut ids = known_ids.into_iter().collect::<Vec<_>>();
    ids.sort();

    let mut providers = serde_json::Map::new();
    for provider_id in ids {
        let has_runtime_key = runtime_auth
            .get(&provider_id)
            .map(|token| !token.trim().is_empty())
            .unwrap_or(false);
        let has_config_key = providers_cfg
            .get(&provider_id)
            .and_then(Value::as_object)
            .map(|entry| {
                entry
                    .get("api_key")
                    .or_else(|| entry.get("apiKey"))
                    .and_then(Value::as_str)
                    .map(|token| !token.trim().is_empty())
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let has_env_key = provider_has_env_secret(&provider_id);
        let has_persisted_key = persisted_ids.contains(&provider_id);
        let has_key = has_env_key || has_runtime_key || has_config_key || has_persisted_key;
        let source = if has_env_key {
            "env"
        } else if has_persisted_key {
            "persisted"
        } else if has_runtime_key || has_config_key {
            "runtime"
        } else {
            "none"
        };
        let configured = connected.contains(&provider_id);
        providers.insert(
            provider_id,
            json!({
                "has_key": has_key,
                "configured": configured,
                "connected": configured,
                "source": source,
            }),
        );
    }

    Json(json!({ "providers": providers }))
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

fn merge_json(base: &mut Value, overlay: &Value) {
    if overlay.is_null() {
        return;
    }
    match (base, overlay) {
        (Value::Object(base_map), Value::Object(overlay_map)) => {
            for (key, value) in overlay_map {
                if value.is_null() {
                    continue;
                }
                match base_map.get_mut(key) {
                    Some(existing) => merge_json(existing, value),
                    None => {
                        base_map.insert(key.clone(), value.clone());
                    }
                }
            }
        }
        (base_value, overlay_value) => {
            *base_value = overlay_value.clone();
        }
    }
}

fn identity_default_value() -> Value {
    json!({
        "bot": {
            "canonical_name": "Tandem",
            "avatar_url": null,
            "aliases": {
                "desktop": "Tandem",
                "tui": "Tandem TUI",
                "portal": "Tandem Portal",
                "control_panel": "Tandem Control Panel",
                "channels": "Tandem",
                "protocol": "Tandem",
                "cli": "Tandem"
            }
        },
        "personality": {
            "default": {
                "preset": "balanced",
                "custom_instructions": null
            },
            "per_agent": {}
        }
    })
}

fn personality_presets_catalog() -> Value {
    json!([
        {
            "id": "balanced",
            "label": "Balanced",
            "description": "Pragmatic, direct, and neutral tone."
        },
        {
            "id": "concise",
            "label": "Concise",
            "description": "Short, high-signal responses focused on outcomes."
        },
        {
            "id": "friendly",
            "label": "Friendly",
            "description": "Warm, approachable style while staying practical."
        },
        {
            "id": "mentor",
            "label": "Mentor",
            "description": "Guides with context and explicit reasoning."
        },
        {
            "id": "critical",
            "label": "Critical",
            "description": "Skeptical, risk-first framing with clear tradeoffs."
        }
    ])
}

fn normalize_effective_config_with_identity(mut value: Value) -> Value {
    let legacy_bot_name = value
        .get("bot_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string);
    let legacy_persona = value
        .get("persona")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string);

    if !value.is_object() {
        return value;
    }

    let root = value.as_object_mut().expect("checked object");
    if !root.contains_key("identity") || !root.get("identity").is_some_and(Value::is_object) {
        root.insert("identity".to_string(), identity_default_value());
    }
    if let Some(identity) = root.get_mut("identity") {
        let mut normalized = identity_default_value();
        merge_json(&mut normalized, identity);
        *identity = normalized;
    }

    if let Some(legacy_bot_name) = legacy_bot_name {
        let canonical_name = root
            .get("identity")
            .and_then(Value::as_object)
            .and_then(|identity| identity.get("bot"))
            .and_then(Value::as_object)
            .and_then(|bot| bot.get("canonical_name"))
            .and_then(Value::as_str);
        let should_fill = canonical_name
            .map(|value| value.trim().is_empty())
            .unwrap_or(true);
        if should_fill {
            root["identity"]["bot"]["canonical_name"] = Value::String(legacy_bot_name);
        }
    }

    if let Some(legacy_persona) = legacy_persona {
        let has_custom = root
            .get("identity")
            .and_then(|identity| identity.get("personality"))
            .and_then(|personality| personality.get("default"))
            .and_then(|default| default.get("custom_instructions"))
            .and_then(Value::as_str)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        if !has_custom {
            root["identity"]["personality"]["default"]["custom_instructions"] =
                Value::String(legacy_persona);
        }
    }

    let canonical_name = root
        .get("identity")
        .and_then(|identity| identity.get("bot"))
        .and_then(|bot| bot.get("canonical_name"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("Tandem")
        .to_string();
    if let Some(aliases) = root
        .get_mut("identity")
        .and_then(Value::as_object_mut)
        .and_then(|identity| identity.get_mut("bot"))
        .and_then(Value::as_object_mut)
        .and_then(|bot| bot.get_mut("aliases"))
        .and_then(Value::as_object_mut)
    {
        for alias in [
            "desktop",
            "portal",
            "channels",
            "protocol",
            "cli",
            "control_panel",
        ] {
            let needs_fill = aliases
                .get(alias)
                .and_then(Value::as_str)
                .map(|v| v.trim().is_empty())
                .unwrap_or(true);
            if needs_fill {
                aliases.insert(alias.to_string(), Value::String(canonical_name.clone()));
            }
        }
        let tui_needs_fill = aliases
            .get("tui")
            .and_then(Value::as_str)
            .map(|v| v.trim().is_empty())
            .unwrap_or(true);
        if tui_needs_fill {
            aliases.insert(
                "tui".to_string(),
                Value::String(format!("{canonical_name} TUI")),
            );
        }
    }

    let default_preset_empty = root
        .get("identity")
        .and_then(|identity| identity.get("personality"))
        .and_then(|personality| personality.get("default"))
        .and_then(|default| default.get("preset"))
        .and_then(Value::as_str)
        .map(|v| v.trim().is_empty())
        .unwrap_or(true);
    if default_preset_empty {
        root["identity"]["personality"]["default"]["preset"] =
            Value::String("balanced".to_string());
    }

    root.insert("bot_name".to_string(), Value::String(canonical_name));
    let compat_persona = root
        .get("identity")
        .and_then(|identity| identity.get("personality"))
        .and_then(|personality| personality.get("default"))
        .and_then(|default| default.get("custom_instructions"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string);
    root.insert(
        "persona".to_string(),
        compat_persona.map_or(Value::Null, Value::String),
    );

    value
}

fn normalize_layers_with_identity(mut value: Value) -> Value {
    let Some(root) = value.as_object_mut() else {
        return value;
    };
    for layer in ["global", "project", "managed", "env", "runtime", "cli"] {
        if let Some(entry) = root.get_mut(layer) {
            let normalized = normalize_effective_config_with_identity(entry.clone());
            *entry = normalized;
        }
    }
    value
}

fn normalize_config_patch_input(mut input: Value) -> Value {
    let Some(root) = input.as_object_mut() else {
        return input;
    };
    let legacy_bot_name = root
        .get("bot_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string);
    let legacy_persona = root
        .get("persona")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string);

    if let Some(legacy_bot_name) = legacy_bot_name {
        root.entry("identity".to_string())
            .or_insert_with(|| json!({}));
        root["identity"]["bot"]["canonical_name"] = Value::String(legacy_bot_name);
    }
    if let Some(legacy_persona) = legacy_persona {
        root.entry("identity".to_string())
            .or_insert_with(|| json!({}));
        root["identity"]["personality"]["default"]["custom_instructions"] =
            Value::String(legacy_persona);
        if root["identity"]["personality"]["default"]
            .get("preset")
            .and_then(Value::as_str)
            .map(|v| v.trim().is_empty())
            .unwrap_or(true)
        {
            root["identity"]["personality"]["default"]["preset"] =
                Value::String("balanced".to_string());
        }
    }

    root.remove("bot_name");
    root.remove("persona");
    normalize_identity_avatar_patch(root);
    input
}

fn normalize_identity_avatar_patch(root: &mut serde_json::Map<String, Value>) {
    let avatar_slot = root
        .get_mut("identity")
        .and_then(Value::as_object_mut)
        .and_then(|identity| identity.get_mut("bot"))
        .and_then(Value::as_object_mut)
        .and_then(|bot| bot.get_mut("avatar_url"));

    let Some(slot) = avatar_slot else {
        return;
    };
    let Some(raw) = slot.as_str() else {
        return;
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        *slot = Value::Null;
        return;
    }
    if let Some(normalized) = normalize_avatar_data_url(trimmed) {
        *slot = Value::String(normalized);
    }
}

fn normalize_avatar_data_url(input: &str) -> Option<String> {
    if !input.starts_with("data:image/") {
        return Some(input.to_string());
    }

    let (meta, payload) = input.split_once(',')?;
    if !meta.contains(";base64") {
        return None;
    }
    // Safety guard against very large inline payloads.
    if payload.len() > 24 * 1024 * 1024 {
        return None;
    }

    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload.as_bytes())
        .ok()?;
    if bytes.len() > 16 * 1024 * 1024 {
        return None;
    }

    let mut image = image::load_from_memory(&bytes).ok()?;
    if image.width() > 512 || image.height() > 512 {
        image = image.thumbnail(512, 512);
    }

    // Re-encode to PNG for consistent, browser-safe storage.
    let mut out = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Png)
        .ok()?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(out);
    Some(format!("data:image/png;base64,{encoded}"))
}

async fn get_config(State(state): State<AppState>) -> Json<Value> {
    let effective = normalize_effective_config_with_identity(redacted(
        state.config.get_effective_value().await,
    ));
    let layers = normalize_layers_with_identity(redacted(state.config.get_layers_value().await));
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
    let effective = match state
        .config
        .patch_project(normalize_config_patch_input(input))
        .await
    {
        Ok(effective) => effective,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    Json(json!({ "effective": normalize_effective_config_with_identity(redacted(effective)) }))
        .into_response()
}
async fn global_config(State(state): State<AppState>) -> Json<Value> {
    let global =
        normalize_effective_config_with_identity(redacted(state.config.get_global_value().await));
    let effective = normalize_effective_config_with_identity(redacted(
        state.config.get_effective_value().await,
    ));
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
    let effective = match state
        .config
        .patch_global(normalize_config_patch_input(input))
        .await
    {
        Ok(effective) => effective,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    Json(json!({ "effective": normalize_effective_config_with_identity(redacted(effective)) }))
        .into_response()
}

async fn get_config_identity(State(state): State<AppState>) -> Json<Value> {
    let effective = normalize_effective_config_with_identity(redacted(
        state.config.get_effective_value().await,
    ));
    let identity = effective
        .get("identity")
        .cloned()
        .unwrap_or_else(identity_default_value);
    Json(json!({
        "identity": identity,
        "presets": personality_presets_catalog()
    }))
}

async fn patch_config_identity(
    State(state): State<AppState>,
    Json(input): Json<Value>,
) -> Response {
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

    let patch = if input.get("identity").is_some() {
        normalize_config_patch_input(input)
    } else {
        normalize_config_patch_input(json!({ "identity": input }))
    };
    let effective = match state.config.patch_project(patch).await {
        Ok(effective) => effective,
        Err(_) => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    };
    state
        .providers
        .reload(state.config.get().await.into())
        .await;
    Json(json!({
        "identity": normalize_effective_config_with_identity(redacted(effective))
            .get("identity")
            .cloned()
            .unwrap_or_else(identity_default_value),
        "presets": personality_presets_catalog()
    }))
    .into_response()
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
async fn add_mcp(State(state): State<AppState>, Json(input): Json<McpAddInput>) -> Json<Value> {
    let name = input.name.unwrap_or_else(|| "default".to_string());
    let transport = input.transport.unwrap_or_else(|| "stdio".to_string());
    state
        .mcp
        .add_or_update(
            name.clone(),
            transport,
            input.headers.unwrap_or_default(),
            input.enabled.unwrap_or(true),
        )
        .await;
    state.event_bus.publish(EngineEvent::new(
        "mcp.server.updated",
        json!({
            "name": name,
        }),
    ));
    Json(json!({"ok": true}))
}

fn mcp_namespace_segment(raw: &str) -> String {
    let mut out = String::new();
    let mut previous_underscore = false;
    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            previous_underscore = false;
        } else if !previous_underscore {
            out.push('_');
            previous_underscore = true;
        }
    }
    let cleaned = out.trim_matches('_');
    if cleaned.is_empty() {
        "server".to_string()
    } else {
        cleaned.to_string()
    }
}

async fn sync_mcp_tools_for_server(state: &AppState, name: &str) -> usize {
    let prefix = format!("mcp.{}.", mcp_namespace_segment(name));
    state.tools.unregister_by_prefix(&prefix).await;
    let tools = state.mcp.server_tools(name).await;
    for tool in &tools {
        let schema = ToolSchema {
            name: tool.namespaced_name.clone(),
            description: if tool.description.trim().is_empty() {
                format!("MCP tool {} from {}", tool.tool_name, tool.server_name)
            } else {
                tool.description.clone()
            },
            input_schema: tool.input_schema.clone(),
        };
        state
            .tools
            .register_tool(
                schema.name.clone(),
                Arc::new(McpBridgeTool {
                    schema,
                    mcp: state.mcp.clone(),
                    server_name: tool.server_name.clone(),
                    tool_name: tool.tool_name.clone(),
                }),
            )
            .await;
    }
    tools.len()
}

async fn connect_mcp(State(state): State<AppState>, Path(name): Path<String>) -> Json<Value> {
    let ok = state.mcp.connect(&name).await;
    if ok {
        let count = sync_mcp_tools_for_server(&state, &name).await;
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.connected",
            json!({
                "name": name,
                "status": "connected",
            }),
        ));
        state.event_bus.publish(EngineEvent::new(
            "mcp.tools.updated",
            json!({
                "name": name,
                "count": count,
            }),
        ));
    } else {
        let prefix = format!("mcp.{}.", mcp_namespace_segment(&name));
        let removed = state.tools.unregister_by_prefix(&prefix).await;
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.disconnected",
            json!({
                "name": name,
                "removedToolCount": removed,
                "reason": "connect_failed"
            }),
        ));
    }
    Json(json!({"ok": ok}))
}
async fn disconnect_mcp(State(state): State<AppState>, Path(name): Path<String>) -> Json<Value> {
    let ok = state.mcp.disconnect(&name).await;
    if ok {
        let prefix = format!("mcp.{}.", mcp_namespace_segment(&name));
        let removed = state.tools.unregister_by_prefix(&prefix).await;
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.disconnected",
            json!({
                "name": name,
                "removedToolCount": removed,
            }),
        ));
    }
    Json(json!({"ok": ok}))
}

async fn delete_mcp(State(state): State<AppState>, Path(name): Path<String>) -> Json<Value> {
    let prefix = format!("mcp.{}.", mcp_namespace_segment(&name));
    let removed_tool_count = state.tools.unregister_by_prefix(&prefix).await;
    let ok = state.mcp.remove(&name).await;
    if ok {
        state.event_bus.publish(EngineEvent::new(
            "mcp.server.deleted",
            json!({
                "name": name,
                "removedToolCount": removed_tool_count,
            }),
        ));
    }
    Json(json!({ "ok": ok, "removedToolCount": removed_tool_count }))
}

async fn patch_mcp(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(input): Json<McpPatchInput>,
) -> Json<Value> {
    let mut changed = false;
    if let Some(enabled) = input.enabled {
        changed = state.mcp.set_enabled(&name, enabled).await;
        if changed {
            if enabled {
                let _ = state.mcp.connect(&name).await;
                let count = sync_mcp_tools_for_server(&state, &name).await;
                state.event_bus.publish(EngineEvent::new(
                    "mcp.tools.updated",
                    json!({
                        "name": name,
                        "count": count,
                    }),
                ));
            } else {
                let prefix = format!("mcp.{}.", mcp_namespace_segment(&name));
                let _ = state.tools.unregister_by_prefix(&prefix).await;
            }
            state.event_bus.publish(EngineEvent::new(
                "mcp.server.updated",
                json!({
                    "name": name,
                    "enabled": enabled,
                }),
            ));
        }
    }
    Json(json!({"ok": changed}))
}

async fn refresh_mcp(State(state): State<AppState>, Path(name): Path<String>) -> Json<Value> {
    let result = state.mcp.refresh(&name).await;
    match result {
        Ok(tools) => {
            let count = sync_mcp_tools_for_server(&state, &name).await;
            state.event_bus.publish(EngineEvent::new(
                "mcp.tools.updated",
                json!({
                    "name": name,
                    "count": count,
                }),
            ));
            Json(json!({
                "ok": true,
                "count": tools.len(),
            }))
        }
        Err(error) => {
            let prefix = format!("mcp.{}.", mcp_namespace_segment(&name));
            let removed = state.tools.unregister_by_prefix(&prefix).await;
            state.event_bus.publish(EngineEvent::new(
                "mcp.server.disconnected",
                json!({
                    "name": name,
                    "removedToolCount": removed,
                    "reason": "refresh_failed"
                }),
            ));
            Json(json!({
                "ok": false,
                "error": error,
                "removedToolCount": removed
            }))
        }
    }
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
async fn mcp_catalog_index() -> Result<Json<Value>, StatusCode> {
    if let Some(index) = mcp_catalog::index() {
        return Ok(Json(index.clone()));
    }
    Err(StatusCode::SERVICE_UNAVAILABLE)
}
async fn mcp_catalog_toml(Path(slug): Path<String>) -> Result<Response, StatusCode> {
    if let Some(toml) = mcp_catalog::toml_for_slug(&slug) {
        return Ok((
            [(header::CONTENT_TYPE, "application/toml; charset=utf-8")],
            toml,
        )
            .into_response());
    }
    Err(StatusCode::NOT_FOUND)
}
async fn mcp_tools(State(state): State<AppState>) -> Json<Value> {
    Json(json!(state.mcp.list_tools().await))
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
async fn run_command(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<CommandRunInput>,
) -> Result<Json<Value>, StatusCode> {
    let request_id = request_id_from_headers(&headers);
    let started = Instant::now();
    let lookup_started = Instant::now();
    let session = state
        .storage
        .get_session(&id)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let lookup_ms = lookup_started.elapsed().as_millis();
    let workspace_root = session
        .workspace_root
        .as_deref()
        .and_then(tandem_core::normalize_workspace_path)
        .or_else(|| tandem_core::normalize_workspace_path(&session.directory));
    let default_cwd = tandem_core::normalize_workspace_path(&session.directory)
        .or_else(|| workspace_root.clone())
        .unwrap_or_else(|| ".".to_string());

    let command = input.command.ok_or(StatusCode::BAD_REQUEST)?;
    let mut cmd = Command::new(&command);
    cmd.args(input.args);
    let effective_cwd = if let Some(requested_cwd) = input.cwd {
        let normalized = tandem_core::normalize_workspace_path(&requested_cwd)
            .unwrap_or_else(|| requested_cwd.trim().to_string());
        if normalized.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        if let Some(root) = workspace_root.as_ref() {
            let requested_path = PathBuf::from(&normalized);
            let candidate = if requested_path.is_absolute() {
                requested_path
            } else {
                PathBuf::from(root).join(requested_path)
            };
            if !tandem_core::is_within_workspace_root(&candidate, &PathBuf::from(root)) {
                return Err(StatusCode::BAD_REQUEST);
            }
        }
        normalized
    } else {
        default_cwd
    };
    cmd.current_dir(&effective_cwd);

    let command_started = Instant::now();
    let output = cmd
        .output()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let command_ms = command_started.elapsed().as_millis();
    let elapsed_ms = started.elapsed().as_millis();
    tracing::info!(
        "session.command request_id={} session_id={} command={} ok={} elapsed_ms={} lookup_ms={} command_ms={}",
        request_id,
        id,
        command,
        output.status.success(),
        elapsed_ms,
        lookup_ms,
        command_ms
    );
    if elapsed_ms >= 2_000 {
        tracing::warn!(
            "slow request request_id={} route=POST /session/{{id}}/command session_id={} command={} elapsed_ms={} lookup_ms={} command_ms={}",
            request_id,
            id,
            command,
            elapsed_ms,
            lookup_ms,
            command_ms
        );
    }
    Ok(Json(json!({
        "ok": output.status.success(),
        "cwd": effective_cwd,
        "stdout": String::from_utf8_lossy(&output.stdout).to_string(),
        "stderr": String::from_utf8_lossy(&output.stderr).to_string()
    })))
}

fn request_id_from_headers(headers: &HeaderMap) -> String {
    headers
        .get("x-tandem-correlation-id")
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(|v| v.to_string())
        .unwrap_or_else(|| Uuid::new_v4().simple().to_string())
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
    let normalized_id = id.trim().to_ascii_lowercase();
    if normalized_id.is_empty() {
        return Json(json!({"ok": false, "error": "provider id cannot be empty"}));
    }
    let token = input.token.unwrap_or_default().trim().to_string();
    if token.is_empty() {
        return Json(json!({"ok": false, "error": "token cannot be empty"}));
    }

    let backend = match tandem_core::set_provider_auth(&normalized_id, &token) {
        Ok(tandem_core::ProviderAuthBackend::Keychain) => "keychain",
        Ok(tandem_core::ProviderAuthBackend::File) => "file",
        Err(err) => {
            return Json(json!({
                "ok": false,
                "id": normalized_id,
                "error": format!("failed to persist provider auth: {err}")
            }));
        }
    };

    // Keep legacy in-memory auth map for compatibility while runtime config
    // becomes the canonical provider-key source.
    state
        .auth
        .write()
        .await
        .insert(normalized_id.clone(), token.clone());

    let patch = json!({
        "providers": {
            normalized_id.clone(): {
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
    Json(json!({"ok": ok, "id": normalized_id, "backend": backend}))
}
async fn delete_auth(State(state): State<AppState>, Path(id): Path<String>) -> Json<Value> {
    let normalized_id = id.trim().to_ascii_lowercase();
    if normalized_id.is_empty() {
        return Json(json!({"ok": false, "error": "provider id cannot be empty"}));
    }
    let removed = state.auth.write().await.remove(&normalized_id).is_some();
    let persisted_removed = tandem_core::delete_provider_auth(&normalized_id)
        .map(|ok| ok)
        .unwrap_or(false);
    let runtime_removed = state
        .config
        .delete_runtime_provider_key(&normalized_id)
        .await
        .is_ok();
    if runtime_removed {
        state
            .providers
            .reload(state.config.get().await.into())
            .await;
    }
    Json(json!({"ok": removed || runtime_removed || persisted_removed}))
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

fn scrub_content_for_memory(input: &str) -> (String, ScrubReport) {
    let mut scrubbed = input.to_string();
    let mut redactions = 0u32;
    let mut blocked = false;

    let redact_patterns = [
        r"(?i)authorization:\s*bearer\s+[a-z0-9\.\-_]+",
        r"(?i)(api[_-]?key|token|secret)\s*[:=]\s*[a-z0-9\-_]{8,}",
        r"(?i)x-api-key\s*:\s*[a-z0-9\-_]{8,}",
        r"(?i)sk-[a-z0-9]{12,}",
        r"(?i)ghp_[a-z0-9]{12,}",
    ];
    for pattern in redact_patterns {
        if let Ok(re) = Regex::new(pattern) {
            let matches = re.find_iter(&scrubbed).count() as u32;
            if matches > 0 {
                redactions = redactions.saturating_add(matches);
                scrubbed = re.replace_all(&scrubbed, "[REDACTED]").to_string();
            }
        }
    }

    let block_markers = [
        "-----begin private key-----",
        "aws_secret_access_key",
        "-----begin rsa private key-----",
    ];
    let lowered = input.to_lowercase();
    for marker in block_markers {
        if lowered.contains(marker) {
            blocked = true;
            break;
        }
    }

    if blocked {
        (
            String::new(),
            ScrubReport {
                status: ScrubStatus::Blocked,
                redactions,
                block_reason: Some("sensitive secret marker detected".to_string()),
            },
        )
    } else if redactions > 0 {
        (
            scrubbed,
            ScrubReport {
                status: ScrubStatus::Redacted,
                redactions,
                block_reason: None,
            },
        )
    } else {
        (
            scrubbed,
            ScrubReport {
                status: ScrubStatus::Passed,
                redactions: 0,
                block_reason: None,
            },
        )
    }
}

fn hash_text(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

async fn append_memory_audit(
    state: &AppState,
    event: crate::MemoryAuditEvent,
) -> Result<(), StatusCode> {
    let mut audit = state.memory_audit_log.write().await;
    audit.push(event);
    Ok(())
}

#[derive(Debug, Clone)]
struct RunMemoryContext {
    run_id: String,
    user_id: String,
    started_at_ms: u64,
    host_tag: Option<String>,
}

async fn open_global_memory_db() -> Option<MemoryDatabase> {
    let paths = tandem_core::resolve_shared_paths().ok()?;
    MemoryDatabase::new(&paths.memory_db_path).await.ok()
}

fn event_run_id(event: &EngineEvent) -> Option<String> {
    event
        .properties
        .get("runID")
        .or_else(|| event.properties.get("run_id"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

fn event_session_id(event: &EngineEvent) -> Option<String> {
    event
        .properties
        .get("sessionID")
        .or_else(|| event.properties.get("sessionId"))
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
}

fn summarize_value(value: &Value, limit: usize) -> String {
    let text = if value.is_string() {
        value.as_str().unwrap_or_default().to_string()
    } else {
        value.to_string()
    };
    truncate_text(&text, limit)
}

async fn persist_global_memory_record(
    state: &AppState,
    db: &MemoryDatabase,
    mut record: GlobalMemoryRecord,
) {
    state.event_bus.publish(EngineEvent::new(
        "memory.write.attempted",
        json!({
            "runID": record.run_id,
            "sourceType": record.source_type,
            "sessionID": record.session_id,
            "messageID": record.message_id,
        }),
    ));

    let (scrubbed, scrub) = scrub_content_for_memory(&record.content);
    if scrub.status == ScrubStatus::Blocked || scrubbed.trim().is_empty() {
        state.event_bus.publish(EngineEvent::new(
            "memory.write.skipped",
            json!({
                "runID": record.run_id,
                "sourceType": record.source_type,
                "reason": scrub.block_reason.unwrap_or_else(|| "scrub_blocked".to_string()),
                "sessionID": record.session_id,
                "messageID": record.message_id,
            }),
        ));
        return;
    }

    record.content = scrubbed;
    record.redaction_count = scrub.redactions;
    record.redaction_status = match scrub.status {
        ScrubStatus::Passed => "passed".to_string(),
        ScrubStatus::Redacted => "redacted".to_string(),
        ScrubStatus::Blocked => "blocked".to_string(),
    };
    record.content_hash = hash_text(&record.content);

    match db.put_global_memory_record(&record).await {
        Ok(write) => {
            let event_name = if write.deduped {
                "memory.write.skipped"
            } else {
                "memory.write.succeeded"
            };
            state.event_bus.publish(EngineEvent::new(
                event_name,
                json!({
                    "runID": record.run_id,
                    "memoryID": write.id,
                    "sourceType": record.source_type,
                    "deduped": write.deduped,
                    "redactionStatus": record.redaction_status,
                    "redactionCount": record.redaction_count,
                    "sessionID": record.session_id,
                    "messageID": record.message_id,
                }),
            ));
        }
        Err(err) => {
            state.event_bus.publish(EngineEvent::new(
                "memory.write.skipped",
                json!({
                    "runID": record.run_id,
                    "sourceType": record.source_type,
                    "reason": format!("db_error:{err}"),
                    "sessionID": record.session_id,
                    "messageID": record.message_id,
                }),
            ));
        }
    }
}

async fn ingest_run_messages(
    state: &AppState,
    db: &MemoryDatabase,
    session_id: &str,
    ctx: &RunMemoryContext,
) {
    let Some(session) = state.storage.get_session(session_id).await else {
        return;
    };

    for message in session.messages {
        let created_ms = message.created_at.timestamp_millis() as u64;
        if created_ms + 1_000 < ctx.started_at_ms {
            continue;
        }
        for part in message.parts {
            match (message.role.clone(), part) {
                (MessageRole::User, MessagePart::Text { text }) => {
                    let now = crate::now_ms();
                    persist_global_memory_record(
                        state,
                        db,
                        GlobalMemoryRecord {
                            id: Uuid::new_v4().to_string(),
                            user_id: ctx.user_id.clone(),
                            source_type: "user_message".to_string(),
                            content: text,
                            content_hash: String::new(),
                            run_id: ctx.run_id.clone(),
                            session_id: Some(session_id.to_string()),
                            message_id: Some(message.id.clone()),
                            tool_name: None,
                            project_tag: session.project_id.clone(),
                            channel_tag: None,
                            host_tag: ctx.host_tag.clone(),
                            metadata: Some(json!({"role": "user"})),
                            provenance: Some(json!({
                                "origin_event_type": "session.run.finished",
                                "origin_message_id": message.id,
                                "origin_session_id": session_id
                            })),
                            redaction_status: "passed".to_string(),
                            redaction_count: 0,
                            visibility: "private".to_string(),
                            demoted: false,
                            score_boost: 0.0,
                            created_at_ms: now,
                            updated_at_ms: now,
                            expires_at_ms: None,
                        },
                    )
                    .await;
                }
                (MessageRole::Assistant, MessagePart::Text { text }) => {
                    let now = crate::now_ms();
                    persist_global_memory_record(
                        state,
                        db,
                        GlobalMemoryRecord {
                            id: Uuid::new_v4().to_string(),
                            user_id: ctx.user_id.clone(),
                            source_type: "assistant_final".to_string(),
                            content: text,
                            content_hash: String::new(),
                            run_id: ctx.run_id.clone(),
                            session_id: Some(session_id.to_string()),
                            message_id: Some(message.id.clone()),
                            tool_name: None,
                            project_tag: session.project_id.clone(),
                            channel_tag: None,
                            host_tag: ctx.host_tag.clone(),
                            metadata: Some(json!({"role": "assistant"})),
                            provenance: Some(json!({
                                "origin_event_type": "session.run.finished",
                                "origin_message_id": message.id,
                                "origin_session_id": session_id
                            })),
                            redaction_status: "passed".to_string(),
                            redaction_count: 0,
                            visibility: "private".to_string(),
                            demoted: false,
                            score_boost: 0.0,
                            created_at_ms: now,
                            updated_at_ms: now,
                            expires_at_ms: None,
                        },
                    )
                    .await;
                }
                (
                    MessageRole::Assistant | MessageRole::Tool,
                    MessagePart::ToolInvocation {
                        tool,
                        args,
                        result,
                        error,
                    },
                ) => {
                    let now = crate::now_ms();
                    let tool_input = summarize_value(&args, 1200);
                    persist_global_memory_record(
                        state,
                        db,
                        GlobalMemoryRecord {
                            id: Uuid::new_v4().to_string(),
                            user_id: ctx.user_id.clone(),
                            source_type: "tool_input".to_string(),
                            content: format!("tool={} args={}", tool, tool_input),
                            content_hash: String::new(),
                            run_id: ctx.run_id.clone(),
                            session_id: Some(session_id.to_string()),
                            message_id: Some(message.id.clone()),
                            tool_name: Some(tool.clone()),
                            project_tag: session.project_id.clone(),
                            channel_tag: None,
                            host_tag: ctx.host_tag.clone(),
                            metadata: None,
                            provenance: Some(json!({"origin_event_type": "session.run.finished"})),
                            redaction_status: "passed".to_string(),
                            redaction_count: 0,
                            visibility: "private".to_string(),
                            demoted: false,
                            score_boost: 0.0,
                            created_at_ms: now,
                            updated_at_ms: now,
                            expires_at_ms: Some(now + 30 * 24 * 60 * 60 * 1000),
                        },
                    )
                    .await;

                    let tool_output = result
                        .as_ref()
                        .map(|v| summarize_value(v, 1500))
                        .or(error)
                        .unwrap_or_default();
                    if !tool_output.trim().is_empty() {
                        let now = crate::now_ms();
                        persist_global_memory_record(
                            state,
                            db,
                            GlobalMemoryRecord {
                                id: Uuid::new_v4().to_string(),
                                user_id: ctx.user_id.clone(),
                                source_type: "tool_output".to_string(),
                                content: format!("tool={} output={}", tool, tool_output),
                                content_hash: String::new(),
                                run_id: ctx.run_id.clone(),
                                session_id: Some(session_id.to_string()),
                                message_id: Some(message.id.clone()),
                                tool_name: Some(tool),
                                project_tag: session.project_id.clone(),
                                channel_tag: None,
                                host_tag: ctx.host_tag.clone(),
                                metadata: None,
                                provenance: Some(
                                    json!({"origin_event_type": "session.run.finished"}),
                                ),
                                redaction_status: "passed".to_string(),
                                redaction_count: 0,
                                visibility: "private".to_string(),
                                demoted: false,
                                score_boost: 0.0,
                                created_at_ms: now,
                                updated_at_ms: now,
                                expires_at_ms: Some(now + 30 * 24 * 60 * 60 * 1000),
                            },
                        )
                        .await;
                    }
                }
                _ => {}
            }
        }
    }
}

async fn ingest_event_memory_records(
    state: &AppState,
    db: &MemoryDatabase,
    event: &EngineEvent,
    ctx_by_session: &HashMap<String, RunMemoryContext>,
) {
    let session_id = event_session_id(event);
    let session_ctx = session_id
        .as_ref()
        .and_then(|sid| ctx_by_session.get(sid))
        .cloned();
    let run_id = event_run_id(event)
        .or_else(|| session_ctx.as_ref().map(|c| c.run_id.clone()))
        .unwrap_or_else(|| "unknown".to_string());
    let user_id = session_ctx
        .as_ref()
        .map(|c| c.user_id.clone())
        .unwrap_or_else(|| "default".to_string());
    let host_tag = session_ctx.as_ref().and_then(|c| c.host_tag.clone());

    let (source_type, content, ttl_ms): (&str, String, Option<u64>) =
        match event.event_type.as_str() {
            "permission.asked" => (
                "approval_request",
                format!(
                    "permission requested tool={} query={}",
                    event
                        .properties
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown"),
                    event
                        .properties
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                ),
                Some(14 * 24 * 60 * 60 * 1000),
            ),
            "permission.replied" => (
                "approval_decision",
                format!(
                    "permission reply requestID={} reply={}",
                    event
                        .properties
                        .get("requestID")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    event
                        .properties
                        .get("reply")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                ),
                Some(14 * 24 * 60 * 60 * 1000),
            ),
            "mcp.auth.required" | "mcp.auth.pending" => (
                "auth_challenge",
                format!(
                    "mcp auth tool={} server={} status={} message={}",
                    event
                        .properties
                        .get("tool")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    event
                        .properties
                        .get("server")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                    event.event_type,
                    event
                        .properties
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or(""),
                ),
                Some(7 * 24 * 60 * 60 * 1000),
            ),
            "todo.updated" => (
                "plan_todos",
                format!(
                    "todo updated: {}",
                    summarize_value(event.properties.get("todos").unwrap_or(&Value::Null), 1200)
                ),
                Some(60 * 24 * 60 * 60 * 1000),
            ),
            "question.asked" => (
                "question_prompt",
                format!(
                    "question asked: {}",
                    summarize_value(
                        event.properties.get("questions").unwrap_or(&Value::Null),
                        1200
                    )
                ),
                Some(60 * 24 * 60 * 60 * 1000),
            ),
            _ => return,
        };

    let now = crate::now_ms();
    persist_global_memory_record(
        state,
        db,
        GlobalMemoryRecord {
            id: Uuid::new_v4().to_string(),
            user_id,
            source_type: source_type.to_string(),
            content,
            content_hash: String::new(),
            run_id,
            session_id,
            message_id: event
                .properties
                .get("messageID")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            tool_name: event
                .properties
                .get("tool")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            project_tag: None,
            channel_tag: event
                .properties
                .get("channel")
                .and_then(|v| v.as_str())
                .map(ToString::to_string),
            host_tag,
            metadata: None,
            provenance: Some(json!({
                "origin_event_type": event.event_type,
            })),
            redaction_status: "passed".to_string(),
            redaction_count: 0,
            visibility: "private".to_string(),
            demoted: false,
            score_boost: 0.0,
            created_at_ms: now,
            updated_at_ms: now,
            expires_at_ms: ttl_ms.map(|ttl| now + ttl),
        },
    )
    .await;
}

async fn run_global_memory_ingestor(state: AppState) {
    let mut rx = state.event_bus.subscribe();
    let Some(db) = open_global_memory_db().await else {
        tracing::warn!("global memory ingestor disabled: could not open memory database");
        return;
    };

    let mut by_session: HashMap<String, RunMemoryContext> = HashMap::new();
    loop {
        match rx.recv().await {
            Ok(event) => match event.event_type.as_str() {
                "session.run.started" => {
                    let session_id = event_session_id(&event);
                    let run_id = event_run_id(&event);
                    if let (Some(session_id), Some(run_id)) = (session_id, run_id) {
                        let started_at_ms = event
                            .properties
                            .get("startedAtMs")
                            .and_then(|v| v.as_u64())
                            .unwrap_or_else(crate::now_ms);
                        let user_id = event
                            .properties
                            .get("clientID")
                            .and_then(|v| v.as_str())
                            .filter(|v| !v.trim().is_empty())
                            .unwrap_or("default")
                            .to_string();
                        let host_tag = event
                            .properties
                            .get("environment")
                            .and_then(|v| v.get("os"))
                            .and_then(|v| v.as_str())
                            .map(ToString::to_string);
                        by_session.insert(
                            session_id,
                            RunMemoryContext {
                                run_id,
                                user_id,
                                started_at_ms,
                                host_tag,
                            },
                        );
                    }
                }
                "session.run.finished" => {
                    if let Some(session_id) = event_session_id(&event) {
                        if let Some(ctx) = by_session.remove(&session_id) {
                            ingest_run_messages(&state, &db, &session_id, &ctx).await;
                        }
                    }
                }
                "permission.asked" | "permission.replied" | "mcp.auth.required"
                | "mcp.auth.pending" | "todo.updated" | "question.asked" => {
                    ingest_event_memory_records(&state, &db, &event, &by_session).await;
                }
                _ => {}
            },
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
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
    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let source_type = match request.kind {
        tandem_memory::MemoryContentKind::SolutionCapsule => "solution_capsule",
        tandem_memory::MemoryContentKind::Note => "note",
        tandem_memory::MemoryContentKind::Fact => "fact",
    }
    .to_string();
    let user_id = capability.subject.clone();
    let record = GlobalMemoryRecord {
        id: id.clone(),
        user_id,
        source_type,
        content: request.content,
        content_hash: String::new(),
        run_id: request.run_id.clone(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: Some(request.partition.project_id.clone()),
        channel_tag: None,
        host_tag: None,
        metadata: request.metadata.clone(),
        provenance: Some(json!({
            "origin_event_type": "memory.put",
            "partition_key": partition_key,
        })),
        redaction_status: "passed".to_string(),
        redaction_count: 0,
        visibility: "private".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: now,
        updated_at_ms: now,
        expires_at_ms: None,
    };
    persist_global_memory_record(&state, &db, record).await;

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

    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let source = db
        .get_global_memory(&request.source_memory_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some(source) = source else {
        let scrub_report = ScrubReport {
            status: ScrubStatus::Blocked,
            redactions: 0,
            block_reason: Some("source memory missing or previously blocked".to_string()),
        };
        let audit_id = Uuid::new_v4().to_string();
        append_memory_audit(
            &state,
            crate::MemoryAuditEvent {
                audit_id: audit_id.clone(),
                action: "memory_promote".to_string(),
                run_id: request.run_id.clone(),
                memory_id: None,
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
                status: "blocked".to_string(),
                detail: scrub_report.block_reason.clone(),
                created_at_ms: crate::now_ms(),
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
    };
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

    let new_id = source.id.clone();
    db.set_global_memory_visibility(&new_id, "shared", false)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

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

    let limit = request.limit.unwrap_or(8).clamp(1, 100);
    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let hits = db
        .search_global_memory(
            &capability.subject,
            &request.query,
            limit,
            Some(&request.partition.project_id),
            None,
            None,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let results = hits
        .into_iter()
        .map(|hit| {
            json!({
                "id": hit.record.id,
                "tier": request.partition.tier,
                "classification": "internal",
                "kind": "fact",
                "source_type": hit.record.source_type,
                "created_at_ms": hit.record.created_at_ms,
                "content": hit.record.content,
                "score": hit.score,
                "run_id": hit.record.run_id,
            })
        })
        .collect::<Vec<_>>();

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

async fn memory_demote(
    State(state): State<AppState>,
    Json(input): Json<MemoryDemoteInput>,
) -> Result<Json<Value>, StatusCode> {
    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let changed = db
        .set_global_memory_visibility(&input.id, "private", true)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !changed {
        return Err(StatusCode::NOT_FOUND);
    }
    state.event_bus.publish(EngineEvent::new(
        "memory.updated",
        json!({
            "memoryID": input.id,
            "runID": input.run_id,
            "action": "demote",
        }),
    ));
    Ok(Json(json!({"ok": true})))
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
    State(_state): State<AppState>,
    Query(query): Query<MemoryListQuery>,
) -> Json<Value> {
    let q = query.q.unwrap_or_default();
    let limit = query.limit.unwrap_or(100).clamp(1, 1000);
    let offset = query.offset.unwrap_or(0);
    let user_id = query.user_id.unwrap_or_else(|| "default".to_string());
    let page = if let Some(db) = open_global_memory_db().await {
        db.list_global_memory(&user_id, Some(&q), limit as i64, offset as i64)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|row| {
                json!({
                    "id": row.id,
                    "user_id": row.user_id,
                    "run_id": row.run_id,
                    "source_type": row.source_type,
                    "content": row.content,
                    "metadata": row.metadata,
                    "created_at_ms": row.created_at_ms,
                    "updated_at_ms": row.updated_at_ms,
                    "visibility": row.visibility,
                })
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let total = page.len();
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
    let db = open_global_memory_db()
        .await
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    let record = db
        .get_global_memory(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some(record) = record else {
        return Err(StatusCode::NOT_FOUND);
    };
    let deleted = db
        .delete_global_memory(&id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if !deleted {
        return Err(StatusCode::NOT_FOUND);
    }
    let now = crate::now_ms();
    append_memory_audit(
        &state,
        crate::MemoryAuditEvent {
            audit_id: Uuid::new_v4().to_string(),
            action: "memory_delete".to_string(),
            run_id: record.run_id,
            memory_id: Some(id.clone()),
            source_memory_id: None,
            to_tier: None,
            partition_key: record.project_tag.unwrap_or_else(|| "global".to_string()),
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
            "style_profile": telegram
                .and_then(|cfg| cfg.get("style_profile"))
                .and_then(Value::as_str)
                .unwrap_or("default"),
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

async fn channels_verify(
    State(state): State<AppState>,
    Path(name): Path<String>,
    input: Option<Json<Value>>,
) -> Result<Json<Value>, StatusCode> {
    let normalized = name.to_ascii_lowercase();
    let payload = input.map(|Json(v)| v).unwrap_or_else(|| json!({}));

    match normalized.as_str() {
        "discord" => Ok(Json(discord_channel_verify(&state, &payload).await)),
        _ => Err(StatusCode::NOT_FOUND),
    }
}

const DISCORD_FLAG_GATEWAY_PRESENCE: u64 = 1 << 12;
const DISCORD_FLAG_GATEWAY_PRESENCE_LIMITED: u64 = 1 << 13;
const DISCORD_FLAG_GATEWAY_MEMBERS: u64 = 1 << 14;
const DISCORD_FLAG_GATEWAY_MEMBERS_LIMITED: u64 = 1 << 15;
const DISCORD_FLAG_GATEWAY_MESSAGE_CONTENT: u64 = 1 << 18;
const DISCORD_FLAG_GATEWAY_MESSAGE_CONTENT_LIMITED: u64 = 1 << 19;

async fn discord_channel_verify(state: &AppState, payload: &Value) -> Value {
    let provided_token = payload
        .get("bot_token")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    let effective = state.config.get_effective_value().await;
    let saved_token = effective
        .get("channels")
        .and_then(Value::as_object)
        .and_then(|obj| obj.get("discord"))
        .and_then(Value::as_object)
        .and_then(|cfg| cfg.get("bot_token"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    let token = provided_token.or(saved_token).unwrap_or_default();
    let has_token = !token.is_empty();
    let mut hints: Vec<String> = Vec::new();
    if !has_token {
        hints.push("Add your Discord bot token, then click Save or Verify again.".to_string());
        return json!({
            "ok": false,
            "channel": "discord",
            "checks": {
                "has_token": false,
                "token_auth_ok": false,
                "gateway_ok": false,
                "message_content_intent_ok": false
            },
            "status_codes": {
                "users_me": null,
                "gateway_bot": null,
                "application_me": null
            },
            "hints": hints,
            "details": {}
        });
    }

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return json!({
                "ok": false,
                "channel": "discord",
                "checks": {
                    "has_token": true,
                    "token_auth_ok": false,
                    "gateway_ok": false,
                    "message_content_intent_ok": false
                },
                "status_codes": {
                    "users_me": null,
                    "gateway_bot": null,
                    "application_me": null
                },
                "hints": ["Local HTTP client setup failed. Restart Tandem and retry verification."],
                "details": {
                    "error": e.to_string()
                }
            });
        }
    };
    let auth_header = format!("Bot {token}");

    let users_resp = client
        .get("https://discord.com/api/v10/users/@me")
        .header("Authorization", auth_header.clone())
        .send()
        .await;
    let gateway_resp = client
        .get("https://discord.com/api/v10/gateway/bot")
        .header("Authorization", auth_header.clone())
        .send()
        .await;
    let app_resp = client
        .get("https://discord.com/api/v10/applications/@me")
        .header("Authorization", auth_header)
        .send()
        .await;

    let users_status = users_resp.as_ref().ok().map(|r| r.status().as_u16());
    let gateway_status = gateway_resp.as_ref().ok().map(|r| r.status().as_u16());
    let app_status = app_resp.as_ref().ok().map(|r| r.status().as_u16());

    let token_auth_ok = users_status == Some(200);
    let gateway_ok = gateway_status == Some(200);

    let mut bot_username: Option<String> = None;
    let mut bot_id: Option<String> = None;
    if let Ok(resp) = users_resp {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<Value>().await {
                bot_username = v
                    .get("username")
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
                bot_id = v.get("id").and_then(Value::as_str).map(ToString::to_string);
            }
        }
    }

    let mut app_flags: Option<u64> = None;
    if let Ok(resp) = app_resp {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<Value>().await {
                app_flags = v.get("flags").and_then(Value::as_u64);
            }
        }
    }

    let message_content_intent_ok = app_flags.is_some_and(|flags| {
        flags
            & (DISCORD_FLAG_GATEWAY_MESSAGE_CONTENT | DISCORD_FLAG_GATEWAY_MESSAGE_CONTENT_LIMITED)
            != 0
    });
    let presence_intent_enabled = app_flags.is_some_and(|flags| {
        flags & (DISCORD_FLAG_GATEWAY_PRESENCE | DISCORD_FLAG_GATEWAY_PRESENCE_LIMITED) != 0
    });
    let server_members_intent_enabled = app_flags.is_some_and(|flags| {
        flags & (DISCORD_FLAG_GATEWAY_MEMBERS | DISCORD_FLAG_GATEWAY_MEMBERS_LIMITED) != 0
    });

    if !token_auth_ok {
        if users_status == Some(401) {
            hints.push("Discord rejected this token (401). Regenerate bot token in Developer Portal -> Bot and update Tandem.".to_string());
        } else {
            hints.push("Could not authenticate bot token with Discord `/users/@me`.".to_string());
        }
    }
    if !gateway_ok {
        if gateway_status == Some(429) {
            hints.push("Discord gateway verification is rate-limited right now. Wait a few seconds and verify again.".to_string());
        } else {
            hints.push("Discord `/gateway/bot` check failed. Verify outbound network access to discord.com.".to_string());
        }
    }
    if token_auth_ok && gateway_ok && !message_content_intent_ok {
        hints.push("Enable `Message Content Intent` in Discord Developer Portal -> Bot -> Privileged Gateway Intents.".to_string());
    }
    if hints.is_empty() {
        hints.push("Discord checks passed. If replies are still missing, verify channel/thread permissions: View Channel, Send Messages, Read Message History, Send Messages in Threads.".to_string());
    }

    let ok = token_auth_ok && gateway_ok && message_content_intent_ok;
    json!({
        "ok": ok,
        "channel": "discord",
        "checks": {
            "has_token": has_token,
            "token_auth_ok": token_auth_ok,
            "gateway_ok": gateway_ok,
            "message_content_intent_ok": message_content_intent_ok,
            "presence_intent_enabled": presence_intent_enabled,
            "server_members_intent_enabled": server_members_intent_enabled
        },
        "status_codes": {
            "users_me": users_status,
            "gateway_bot": gateway_status,
            "application_me": app_status
        },
        "hints": hints,
        "details": {
            "bot_username": bot_username,
            "bot_id": bot_id,
            "application_flags": app_flags
        }
    })
}

async fn channels_put(
    State(state): State<AppState>,
    Path(name): Path<String>,
    Json(input): Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let normalized = name.to_ascii_lowercase();
    let effective = state.config.get_effective_value().await;
    let existing_channel_cfg = |channel: &str| -> Option<&serde_json::Map<String, Value>> {
        effective
            .get("channels")
            .and_then(Value::as_object)
            .and_then(|obj| obj.get(channel))
            .and_then(Value::as_object)
    };
    let existing_bot_token = |channel: &str| -> Option<String> {
        existing_channel_cfg(channel)
            .and_then(|cfg| cfg.get("bot_token"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let existing_channel_id = |channel: &str| -> Option<String> {
        existing_channel_cfg(channel)
            .and_then(|cfg| cfg.get("channel_id"))
            .and_then(Value::as_str)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

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
            let mut cfg: TelegramConfigFile =
                serde_json::from_value(input).map_err(|_| StatusCode::BAD_REQUEST)?;
            if cfg.bot_token.trim().is_empty() {
                cfg.bot_token = existing_bot_token("telegram").unwrap_or_default();
            }
            if cfg.bot_token.trim().is_empty() {
                return Err(StatusCode::BAD_REQUEST);
            }
            channels_obj.insert("telegram".to_string(), json!(cfg));
        }
        "discord" => {
            let mut cfg: DiscordConfigFile =
                serde_json::from_value(input).map_err(|_| StatusCode::BAD_REQUEST)?;
            if cfg.bot_token.trim().is_empty() {
                cfg.bot_token = existing_bot_token("discord").unwrap_or_default();
            }
            if cfg.bot_token.trim().is_empty() {
                return Err(StatusCode::BAD_REQUEST);
            }
            channels_obj.insert("discord".to_string(), json!(cfg));
        }
        "slack" => {
            let mut cfg: SlackConfigFile =
                serde_json::from_value(input).map_err(|_| StatusCode::BAD_REQUEST)?;
            if cfg.bot_token.trim().is_empty() {
                cfg.bot_token = existing_bot_token("slack").unwrap_or_default();
            }
            if cfg.channel_id.trim().is_empty() {
                cfg.channel_id = existing_channel_id("slack").unwrap_or_default();
            }
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
        emit_spawn_requested(state, &req);
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

async fn agent_team_template_create(
    State(state): State<AppState>,
    Json(input): Json<AgentTeamTemplateCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    if input.template.template_id.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "ok": false,
                "code": "INVALID_TEMPLATE_ID",
                "error": "template_id is required"
            })),
        ));
    }
    let workspace_root = state.workspace_index.snapshot().await.root;
    let template = state
        .agent_teams
        .upsert_template(&workspace_root, input.template)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "ok": false,
                    "code": "TEMPLATE_PERSIST_FAILED",
                    "error": error.to_string(),
                })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "template": template,
    })))
}

async fn agent_team_template_patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AgentTeamTemplatePatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let existing = state
        .agent_teams
        .list_templates()
        .await
        .into_iter()
        .find(|template| template.template_id == id)
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "ok": false,
                    "code": "TEMPLATE_NOT_FOUND",
                    "error": "template not found",
                    "templateID": id,
                })),
            )
        })?;
    let mut updated = existing;
    if let Some(role) = input.role {
        updated.role = role;
    }
    if let Some(system_prompt) = input.system_prompt {
        updated.system_prompt = Some(system_prompt);
    }
    if let Some(skills) = input.skills {
        updated.skills = skills;
    }
    if let Some(default_budget) = input.default_budget {
        updated.default_budget = default_budget;
    }
    if let Some(capabilities) = input.capabilities {
        updated.capabilities = capabilities;
    }

    let workspace_root = state.workspace_index.snapshot().await.root;
    let template = state
        .agent_teams
        .upsert_template(&workspace_root, updated)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "ok": false,
                    "code": "TEMPLATE_PERSIST_FAILED",
                    "error": error.to_string(),
                })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "template": template,
    })))
}

async fn agent_team_template_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let workspace_root = state.workspace_index.snapshot().await.root;
    let deleted = state
        .agent_teams
        .delete_template(&workspace_root, &id)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "ok": false,
                    "code": "TEMPLATE_DELETE_FAILED",
                    "error": error.to_string(),
                })),
            )
        })?;
    if !deleted {
        return Err((
            StatusCode::NOT_FOUND,
            Json(json!({
                "ok": false,
                "code": "TEMPLATE_NOT_FOUND",
                "error": "template not found",
                "templateID": id,
            })),
        ));
    }
    Ok(Json(json!({
        "ok": true,
        "deleted": true,
        "templateID": id,
    })))
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

async fn run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<RunEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(run_events_sse_stream(state, run_id, query))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
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
    }
    blackboard.revision = patch.seq;
    Ok(())
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
        status: ContextRunStatus::Queued,
        objective: input.objective,
        workspace: input.workspace.unwrap_or_default(),
        steps: Vec::new(),
        why_next_step: None,
        revision: 1,
        created_at_ms: now,
        updated_at_ms: now,
    };
    save_context_run_state(&state, &run).await?;
    Ok(Json(json!({ "ok": true, "run": run })))
}

async fn context_run_list(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let root = context_runs_root(&state);
    if !root.exists() {
        return Ok(Json(json!({ "runs": [] })));
    }
    let mut rows = Vec::<ContextRunState>::new();
    let mut dir = tokio::fs::read_dir(root)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    while let Ok(Some(entry)) = dir.next_entry().await {
        if !entry
            .file_type()
            .await
            .map(|kind| kind.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let run_id = entry.file_name().to_string_lossy().to_string();
        if let Ok(run) = load_context_run_state(&state, &run_id).await {
            rows.push(run);
        }
    }
    rows.sort_by(|a, b| b.updated_at_ms.cmp(&a.updated_at_ms));
    Ok(Json(json!({ "runs": rows })))
}

async fn context_run_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let run = load_context_run_state(&state, &run_id).await?;
    Ok(Json(json!({ "run": run })))
}

async fn context_run_put(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(mut run): Json<ContextRunState>,
) -> Result<Json<Value>, StatusCode> {
    if run.run_id != run_id {
        return Err(StatusCode::BAD_REQUEST);
    }
    let now = crate::now_ms();
    run.revision = run.revision.saturating_add(1);
    run.updated_at_ms = now;
    save_context_run_state(&state, &run).await?;
    Ok(Json(json!({ "ok": true, "run": run })))
}

fn context_run_is_terminal(status: &ContextRunStatus) -> bool {
    matches!(
        status,
        ContextRunStatus::Failed | ContextRunStatus::Completed | ContextRunStatus::Cancelled
    )
}

fn context_step_status_from_todo(status: Option<&str>) -> ContextStepStatus {
    let normalized = status
        .map(|v| v.trim().to_ascii_lowercase())
        .unwrap_or_else(|| "pending".to_string());
    match normalized.as_str() {
        "in_progress" | "in-progress" | "working" | "doing" | "active" => {
            ContextStepStatus::InProgress
        }
        "runnable" | "ready" => ContextStepStatus::Runnable,
        "done" | "completed" | "complete" => ContextStepStatus::Done,
        "blocked" => ContextStepStatus::Blocked,
        "failed" | "error" => ContextStepStatus::Failed,
        _ => ContextStepStatus::Pending,
    }
}

fn normalize_context_todo_step_id(raw: Option<&str>, idx: usize) -> String {
    raw.map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| format!("todo-{}", idx.saturating_add(1)))
}

async fn context_run_todos_sync(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextTodoSyncInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut run = load_context_run_state(&state, &run_id).await?;
    let replace = input.replace.unwrap_or(true);

    let mapped_steps = input
        .todos
        .iter()
        .enumerate()
        .map(|(idx, todo)| ContextRunStep {
            step_id: normalize_context_todo_step_id(todo.id.as_deref(), idx),
            title: todo.content.trim().to_string(),
            status: context_step_status_from_todo(todo.status.as_deref()),
        })
        .filter(|step| !step.title.is_empty())
        .collect::<Vec<_>>();

    if replace {
        run.steps = mapped_steps;
    } else {
        for step in mapped_steps {
            if let Some(existing) = run.steps.iter_mut().find(|row| row.step_id == step.step_id) {
                existing.title = step.title;
                existing.status = step.status;
            } else {
                run.steps.push(step);
            }
        }
    }

    if context_run_is_terminal(&run.status) {
        // keep terminal status unchanged
    } else if run
        .steps
        .iter()
        .any(|step| matches!(step.status, ContextStepStatus::InProgress))
    {
        run.status = ContextRunStatus::Running;
    } else if run
        .steps
        .iter()
        .any(|step| matches!(step.status, ContextStepStatus::Runnable))
    {
        run.status = ContextRunStatus::Planning;
    } else if run
        .steps
        .iter()
        .all(|step| matches!(step.status, ContextStepStatus::Done))
        && !run.steps.is_empty()
    {
        run.status = ContextRunStatus::Completed;
    } else if !run.steps.is_empty() {
        run.status = ContextRunStatus::Planning;
    }

    run.why_next_step = run
        .steps
        .iter()
        .find(|step| {
            matches!(
                step.status,
                ContextStepStatus::InProgress
                    | ContextStepStatus::Runnable
                    | ContextStepStatus::Pending
            )
        })
        .map(|step| format!("continue task `{}` from synced todo list", step.step_id));

    run.revision = run.revision.saturating_add(1);
    run.updated_at_ms = crate::now_ms();
    save_context_run_state(&state, &run).await?;

    let _ = context_run_event_append(
        State(state.clone()),
        Path(run_id.clone()),
        Json(ContextRunEventAppendInput {
            event_type: "todo_synced".to_string(),
            status: run.status.clone(),
            step_id: None,
            payload: json!({
                "count": run.steps.len(),
                "replace": replace,
                "source_session_id": input.source_session_id,
                "source_run_id": input.source_run_id,
                "todos": input.todos,
                "why_next_step": run.why_next_step.clone(),
            }),
        }),
    )
    .await;

    Ok(Json(json!({
        "ok": true,
        "run": run
    })))
}

fn context_driver_select_next_step(
    run: &ContextRunState,
) -> (Option<usize>, String, ContextRunStatus) {
    if context_run_is_terminal(&run.status) {
        return (
            None,
            format!(
                "run is terminal (`{}`); no next step can be selected",
                serde_json::to_string(&run.status).unwrap_or_else(|_| "\"terminal\"".to_string())
            ),
            run.status.clone(),
        );
    }
    if let Some(step) = run
        .steps
        .iter()
        .find(|step| matches!(step.status, ContextStepStatus::InProgress))
    {
        return (
            None,
            format!(
                "step `{}` is already in_progress; keep current execution focus",
                step.step_id
            ),
            ContextRunStatus::Running,
        );
    }
    if let Some((idx, step)) = run
        .steps
        .iter()
        .enumerate()
        .find(|(_, step)| matches!(step.status, ContextStepStatus::Runnable))
    {
        return (
            Some(idx),
            format!(
                "selected runnable step `{}` as next execution target",
                step.step_id
            ),
            ContextRunStatus::Running,
        );
    }
    if let Some((idx, step)) = run
        .steps
        .iter()
        .enumerate()
        .find(|(_, step)| matches!(step.status, ContextStepStatus::Pending))
    {
        return (
            Some(idx),
            format!(
                "no runnable step available; promoted pending step `{}` for execution",
                step.step_id
            ),
            ContextRunStatus::Running,
        );
    }
    if !run.steps.is_empty()
        && run
            .steps
            .iter()
            .all(|step| matches!(step.status, ContextStepStatus::Done))
    {
        return (
            None,
            "all steps are done; marking run completed".to_string(),
            ContextRunStatus::Completed,
        );
    }
    if run
        .steps
        .iter()
        .any(|step| matches!(step.status, ContextStepStatus::Failed))
    {
        return (
            None,
            "one or more steps failed and no runnable work remains; run is blocked".to_string(),
            ContextRunStatus::Blocked,
        );
    }
    (
        None,
        "no actionable steps found; run remains blocked".to_string(),
        ContextRunStatus::Blocked,
    )
}

async fn context_run_driver_next(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextDriverNextInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut run = load_context_run_state(&state, &run_id).await?;
    let dry_run = input.dry_run.unwrap_or(false);
    let (selected_idx, why_next_step, target_status) = context_driver_select_next_step(&run);

    let selected_step_id = selected_idx.map(|idx| run.steps[idx].step_id.clone());
    let selected_step_status = selected_idx.map(|idx| {
        if matches!(run.steps[idx].status, ContextStepStatus::Pending) {
            "pending"
        } else {
            "runnable"
        }
    });

    if !dry_run {
        if let Some(idx) = selected_idx {
            if matches!(run.steps[idx].status, ContextStepStatus::Pending) {
                run.steps[idx].status = ContextStepStatus::Runnable;
            }
            run.steps[idx].status = ContextStepStatus::InProgress;
        }
        run.status = target_status.clone();
        run.why_next_step = Some(why_next_step.clone());
        run.revision = run.revision.saturating_add(1);
        run.updated_at_ms = crate::now_ms();
        save_context_run_state(&state, &run).await?;

        let _ = context_run_event_append(
            State(state.clone()),
            Path(run_id.clone()),
            Json(ContextRunEventAppendInput {
                event_type: "meta_next_step_selected".to_string(),
                status: target_status.clone(),
                step_id: selected_step_id.clone(),
                payload: json!({
                    "why_next_step": why_next_step,
                    "selected_step_id": selected_step_id,
                    "selected_step_previous_status": selected_step_status,
                    "driver": "context_driver_v1"
                }),
            }),
        )
        .await;
    }

    Ok(Json(json!({
        "ok": true,
        "dry_run": dry_run,
        "run_id": run_id,
        "selected_step_id": selected_step_id,
        "target_status": target_status,
        "why_next_step": why_next_step,
        "run": if dry_run { serde_json::to_value(&run).unwrap_or_else(|_| json!(null)) } else { serde_json::to_value(&run).unwrap_or_else(|_| json!(null)) }
    })))
}

async fn context_run_event_append(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextRunEventAppendInput>,
) -> Result<Json<Value>, StatusCode> {
    let events_path = context_run_events_path(&state, &run_id);
    let seq = latest_context_run_event_seq(&events_path).saturating_add(1);
    let record = ContextRunEventRecord {
        event_id: format!("evt-{}", Uuid::new_v4()),
        run_id: run_id.clone(),
        seq,
        ts_ms: crate::now_ms(),
        event_type: input.event_type,
        status: input.status.clone(),
        step_id: input.step_id.clone(),
        payload: input.payload,
    };
    append_jsonl_line(
        &events_path,
        &serde_json::to_value(&record).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    )?;

    if let Ok(mut run) = load_context_run_state(&state, &run_id).await {
        run.status = input.status;
        run.revision = run.revision.saturating_add(1);
        run.updated_at_ms = crate::now_ms();
        save_context_run_state(&state, &run).await?;
    }

    Ok(Json(json!({ "ok": true, "event": record })))
}

async fn context_run_events(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<RunEventsQuery>,
) -> Result<Json<Value>, StatusCode> {
    let rows = load_context_run_events_jsonl(
        &context_run_events_path(&state, &run_id),
        query.since_seq,
        query.tail,
    );
    Ok(Json(json!({ "events": rows })))
}

fn context_run_events_sse_stream(
    state: AppState,
    run_id: String,
    query: RunEventsQuery,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let (tx, rx) = tokio::sync::mpsc::channel::<String>(256);
    tokio::spawn(async move {
        let events_path = context_run_events_path(&state, &run_id);
        let ready = serde_json::to_string(&json!({
            "status":"ready",
            "stream":"context_run_events",
            "runID": run_id,
            "timestamp_ms": crate::now_ms(),
            "path": events_path.to_string_lossy().to_string(),
        }))
        .unwrap_or_default();
        if tx.send(ready).await.is_err() {
            return;
        }

        let initial = load_context_run_events_jsonl(&events_path, query.since_seq, query.tail);
        let mut last_seq = query.since_seq.unwrap_or(0);
        for row in initial {
            last_seq = last_seq.max(row.seq);
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
            let updates = load_context_run_events_jsonl(&events_path, Some(last_seq), None);
            for row in updates {
                last_seq = last_seq.max(row.seq);
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

async fn context_run_events_stream(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<RunEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(context_run_events_sse_stream(state, run_id, query))
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}

async fn context_run_lease_validate(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextLeaseValidateInput>,
) -> Result<Json<Value>, StatusCode> {
    let mut run = load_context_run_state(&state, &run_id).await?;
    if run.workspace.canonical_path.trim().is_empty() {
        run.workspace.canonical_path = input.current_path.clone();
        if run.workspace.lease_epoch == 0 {
            run.workspace.lease_epoch = 1;
        }
        run.revision = run.revision.saturating_add(1);
        run.updated_at_ms = crate::now_ms();
        save_context_run_state(&state, &run).await?;
        return Ok(Json(json!({ "ok": true, "mismatch": false })));
    }

    if run.workspace.canonical_path != input.current_path {
        run.status = ContextRunStatus::Paused;
        run.revision = run.revision.saturating_add(1);
        run.updated_at_ms = crate::now_ms();
        save_context_run_state(&state, &run).await?;
        let _ = context_run_event_append(
            State(state.clone()),
            Path(run_id.clone()),
            Json(ContextRunEventAppendInput {
                event_type: "workspace_mismatch".to_string(),
                status: ContextRunStatus::Paused,
                step_id: input.step_id.clone(),
                payload: json!({
                    "phase": input.phase,
                    "expected_path": run.workspace.canonical_path,
                    "actual_path": input.current_path,
                }),
            }),
        )
        .await;
        return Ok(Json(json!({
            "ok": false,
            "mismatch": true,
            "status": "paused"
        })));
    }

    Ok(Json(json!({ "ok": true, "mismatch": false })))
}

async fn context_run_blackboard_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let blackboard = load_context_blackboard(&state, &run_id);
    Ok(Json(json!({ "blackboard": blackboard })))
}

async fn context_run_blackboard_patch(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextBlackboardPatchInput>,
) -> Result<Json<Value>, StatusCode> {
    let patch_path = context_run_blackboard_patches_path(&state, &run_id);
    let seq = {
        let rows = std::fs::read_to_string(&patch_path).unwrap_or_default();
        rows.lines()
            .filter_map(|line| serde_json::from_str::<ContextBlackboardPatchRecord>(line).ok())
            .map(|row| row.seq)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    };
    let patch = ContextBlackboardPatchRecord {
        patch_id: format!("bbp-{}", Uuid::new_v4()),
        run_id: run_id.clone(),
        seq,
        ts_ms: crate::now_ms(),
        op: input.op,
        payload: input.payload,
    };
    append_jsonl_line(
        &patch_path,
        &serde_json::to_value(&patch).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    )?;
    let mut blackboard = load_context_blackboard(&state, &run_id);
    apply_context_blackboard_patch(&mut blackboard, &patch)?;
    save_context_blackboard(&state, &run_id, &blackboard)?;
    Ok(Json(
        json!({ "ok": true, "patch": patch, "blackboard": blackboard }),
    ))
}

async fn context_run_checkpoint_create(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<ContextCheckpointCreateInput>,
) -> Result<Json<Value>, StatusCode> {
    let run_state = load_context_run_state(&state, &run_id).await?;
    let blackboard = load_context_blackboard(&state, &run_id);
    let events_path = context_run_events_path(&state, &run_id);
    let seq = latest_context_run_event_seq(&events_path);
    let checkpoint = ContextCheckpointRecord {
        checkpoint_id: format!("cp-{}", Uuid::new_v4()),
        run_id: run_id.clone(),
        seq,
        ts_ms: crate::now_ms(),
        reason: input.reason,
        run_state,
        blackboard,
    };
    let dir = context_run_checkpoints_dir(&state, &run_id);
    std::fs::create_dir_all(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let path = dir.join(format!("{:020}.json", seq));
    let payload =
        serde_json::to_string_pretty(&checkpoint).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    std::fs::write(path, payload).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "ok": true, "checkpoint": checkpoint })))
}

async fn context_run_checkpoint_latest(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, StatusCode> {
    let dir = context_run_checkpoints_dir(&state, &run_id);
    if !dir.exists() {
        return Ok(Json(json!({ "checkpoint": null })));
    }
    let mut latest_name: Option<String> = None;
    let mut entries = std::fs::read_dir(&dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    while let Some(Ok(entry)) = entries.next() {
        if !entry
            .file_type()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let should_replace = match latest_name.as_ref() {
            Some(current) => name > *current,
            None => true,
        };
        if should_replace {
            latest_name = Some(name);
        }
    }
    let Some(file_name) = latest_name else {
        return Ok(Json(json!({ "checkpoint": null })));
    };
    let raw = std::fs::read_to_string(dir.join(file_name))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let checkpoint = serde_json::from_str::<ContextCheckpointRecord>(&raw)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(json!({ "checkpoint": checkpoint })))
}

fn parse_context_step_status_text(value: &str) -> Option<ContextStepStatus> {
    match value.trim().to_ascii_lowercase().as_str() {
        "pending" => Some(ContextStepStatus::Pending),
        "runnable" => Some(ContextStepStatus::Runnable),
        "in_progress" | "in-progress" | "working" | "active" => Some(ContextStepStatus::InProgress),
        "blocked" => Some(ContextStepStatus::Blocked),
        "done" | "completed" | "complete" => Some(ContextStepStatus::Done),
        "failed" | "error" => Some(ContextStepStatus::Failed),
        _ => None,
    }
}

fn replay_step_status_from_event(event: &ContextRunEventRecord) -> Option<ContextStepStatus> {
    if let Some(status_text) = event.payload.get("step_status").and_then(Value::as_str) {
        if let Some(status) = parse_context_step_status_text(status_text) {
            return Some(status);
        }
    }
    let ty = event.event_type.to_ascii_lowercase();
    if ty.contains("step_started") || ty.contains("step_running") {
        return Some(ContextStepStatus::InProgress);
    }
    if ty.contains("step_blocked") {
        return Some(ContextStepStatus::Blocked);
    }
    if ty.contains("step_failed") {
        return Some(ContextStepStatus::Failed);
    }
    if ty.contains("step_done") || ty.contains("step_completed") {
        return Some(ContextStepStatus::Done);
    }
    None
}

fn latest_context_checkpoint_record(
    state: &AppState,
    run_id: &str,
) -> Option<ContextCheckpointRecord> {
    let dir = context_run_checkpoints_dir(state, run_id);
    if !dir.exists() {
        return None;
    }
    let mut latest_name: Option<String> = None;
    let mut entries = std::fs::read_dir(&dir).ok()?;
    while let Some(Ok(entry)) = entries.next() {
        if !entry
            .file_type()
            .ok()
            .map(|kind| kind.is_file())
            .unwrap_or(false)
        {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let should_replace = match latest_name.as_ref() {
            Some(current) => name > *current,
            None => true,
        };
        if should_replace {
            latest_name = Some(name);
        }
    }
    let file_name = latest_name?;
    let raw = std::fs::read_to_string(dir.join(file_name)).ok()?;
    serde_json::from_str::<ContextCheckpointRecord>(&raw).ok()
}

fn context_run_replay_materialize(
    run_id: &str,
    persisted: &ContextRunState,
    checkpoint: Option<&ContextCheckpointRecord>,
    events: &[ContextRunEventRecord],
) -> ContextRunState {
    let mut replay = if let Some(cp) = checkpoint {
        cp.run_state.clone()
    } else {
        let mut base = persisted.clone();
        base.status = ContextRunStatus::Queued;
        base.why_next_step = None;
        base.revision = 1;
        base.updated_at_ms = base.created_at_ms;
        for step in &mut base.steps {
            step.status = ContextStepStatus::Pending;
        }
        base
    };
    replay.run_id = run_id.to_string();
    let mut step_index: HashMap<String, usize> = replay
        .steps
        .iter()
        .enumerate()
        .map(|(idx, step)| (step.step_id.clone(), idx))
        .collect();

    for event in events {
        replay.status = event.status.clone();
        replay.updated_at_ms = replay.updated_at_ms.max(event.ts_ms);
        if let Some(why) = event.payload.get("why_next_step").and_then(Value::as_str) {
            if !why.trim().is_empty() {
                replay.why_next_step = Some(why.to_string());
            }
        }
        if let Some(items) = event.payload.get("steps").and_then(Value::as_array) {
            for item in items {
                let Some(step_id) = item.get("step_id").and_then(Value::as_str) else {
                    continue;
                };
                let step_title = item
                    .get("title")
                    .and_then(Value::as_str)
                    .unwrap_or(step_id)
                    .to_string();
                let step_status = item
                    .get("status")
                    .and_then(Value::as_str)
                    .and_then(parse_context_step_status_text)
                    .unwrap_or(ContextStepStatus::Pending);
                let index = if let Some(existing) = step_index.get(step_id).copied() {
                    existing
                } else {
                    replay.steps.push(ContextRunStep {
                        step_id: step_id.to_string(),
                        title: step_title.clone(),
                        status: ContextStepStatus::Pending,
                    });
                    let idx = replay.steps.len().saturating_sub(1);
                    step_index.insert(step_id.to_string(), idx);
                    idx
                };
                replay.steps[index].title = step_title;
                replay.steps[index].status = step_status;
            }
        }
        if let Some(step_id) = &event.step_id {
            let index = if let Some(existing) = step_index.get(step_id).copied() {
                existing
            } else {
                replay.steps.push(ContextRunStep {
                    step_id: step_id.clone(),
                    title: event
                        .payload
                        .get("step_title")
                        .and_then(Value::as_str)
                        .unwrap_or(step_id.as_str())
                        .to_string(),
                    status: ContextStepStatus::Pending,
                });
                let idx = replay.steps.len().saturating_sub(1);
                step_index.insert(step_id.clone(), idx);
                idx
            };
            if let Some(step_status) = replay_step_status_from_event(event) {
                replay.steps[index].status = step_status;
            }
        }
    }

    let checkpoint_revision = checkpoint.map(|cp| cp.run_state.revision).unwrap_or(1);
    replay.revision = checkpoint_revision.saturating_add(events.len() as u64);
    replay
}

async fn context_run_replay(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Query(query): Query<ContextRunReplayQuery>,
) -> Result<Json<Value>, StatusCode> {
    let persisted = load_context_run_state(&state, &run_id).await?;
    let from_checkpoint = query.from_checkpoint.unwrap_or(true);
    let checkpoint = if from_checkpoint {
        latest_context_checkpoint_record(&state, &run_id)
    } else {
        None
    };
    let start_seq = checkpoint.as_ref().map(|cp| cp.seq).unwrap_or(0);
    let mut events = load_context_run_events_jsonl(
        &context_run_events_path(&state, &run_id),
        Some(start_seq),
        None,
    );
    if let Some(upto_seq) = query.upto_seq {
        events.retain(|row| row.seq <= upto_seq);
    }
    let replay = context_run_replay_materialize(&run_id, &persisted, checkpoint.as_ref(), &events);
    let status_mismatch = replay.status != persisted.status;
    let why_next_step_mismatch =
        persisted.why_next_step.is_some() && replay.why_next_step != persisted.why_next_step;
    let step_count_mismatch =
        !persisted.steps.is_empty() && replay.steps.len() != persisted.steps.len();
    let drift = ContextReplayDrift {
        mismatch: status_mismatch || why_next_step_mismatch || step_count_mismatch,
        status_mismatch,
        why_next_step_mismatch,
        step_count_mismatch,
    };
    Ok(Json(json!({
        "ok": true,
        "run_id": run_id,
        "from_checkpoint": checkpoint.is_some(),
        "checkpoint_seq": checkpoint.as_ref().map(|cp| cp.seq),
        "events_applied": events.len(),
        "replay": replay,
        "persisted": persisted,
        "drift": drift
    })))
}

fn objective_from_args(args: &Value, routine_id: &str, entrypoint: &str) -> String {
    args.get("prompt")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            format!("Execute automation '{routine_id}' with entrypoint '{entrypoint}'.")
        })
}

fn success_criteria_from_args(args: &Value) -> Vec<String> {
    args.get("success_criteria")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.as_str())
                .map(str::trim)
                .filter(|row| !row.is_empty())
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn mode_from_args(args: &Value) -> String {
    args.get("mode")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .unwrap_or("standalone")
        .to_string()
}

fn normalize_automation_mode(raw: Option<&str>) -> Result<String, String> {
    let value = raw.unwrap_or("standalone").trim();
    if value.is_empty() {
        return Ok("standalone".to_string());
    }
    if value.eq_ignore_ascii_case("standalone") {
        return Ok("standalone".to_string());
    }
    if value.eq_ignore_ascii_case("orchestrated") {
        return Ok("orchestrated".to_string());
    }
    Err("mode must be one of standalone|orchestrated".to_string())
}

fn validate_model_spec_object(value: &Value, path: &str) -> Result<(), String> {
    let obj = value
        .as_object()
        .ok_or_else(|| format!("{path} must be an object"))?;
    let provider_id = obj
        .get("provider_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{path}.provider_id is required"))?;
    let model_id = obj
        .get("model_id")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| format!("{path}.model_id is required"))?;
    if provider_id.is_empty() || model_id.is_empty() {
        return Err(format!(
            "{path}.provider_id and {path}.model_id are required"
        ));
    }
    Ok(())
}

fn validate_model_policy(value: &Value) -> Result<(), String> {
    let obj = value
        .as_object()
        .ok_or_else(|| "model_policy must be an object".to_string())?;
    if let Some(default_model) = obj.get("default_model") {
        validate_model_spec_object(default_model, "model_policy.default_model")?;
    }
    if let Some(role_models) = obj.get("role_models") {
        let role_obj = role_models
            .as_object()
            .ok_or_else(|| "model_policy.role_models must be an object".to_string())?;
        for (role, spec) in role_obj {
            validate_model_spec_object(spec, &format!("model_policy.role_models.{role}"))?;
        }
    }
    Ok(())
}

fn routine_to_automation_wire(routine: RoutineSpec) -> Value {
    json!({
        "automation_id": routine.routine_id,
        "name": routine.name,
        "status": routine.status,
        "schedule": routine.schedule,
        "timezone": routine.timezone,
        "misfire_policy": routine.misfire_policy,
        "mode": mode_from_args(&routine.args),
        "mission": {
            "objective": objective_from_args(&routine.args, &routine.routine_id, &routine.entrypoint),
            "success_criteria": success_criteria_from_args(&routine.args),
            "briefing": routine.args.get("briefing").cloned(),
            "entrypoint_compat": routine.entrypoint,
        },
        "policy": {
            "tool": {
                "run_allowlist": routine.allowed_tools,
                "external_integrations_allowed": routine.external_integrations_allowed,
                "orchestrator_only_tool_calls": routine
                    .args
                    .get("orchestrator_only_tool_calls")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false),
            },
            "approval": {
                "requires_approval": routine.requires_approval
            }
        },
        "model_policy": routine.args.get("model_policy").cloned(),
        "output_targets": routine.output_targets,
        "creator_type": routine.creator_type,
        "creator_id": routine.creator_id,
        "next_fire_at_ms": routine.next_fire_at_ms,
        "last_fired_at_ms": routine.last_fired_at_ms
    })
}

fn routine_run_to_automation_wire(run: RoutineRunRecord) -> Value {
    json!({
        "run_id": run.run_id,
        "automation_id": run.routine_id,
        "trigger_type": run.trigger_type,
        "run_count": run.run_count,
        "status": run.status,
        "created_at_ms": run.created_at_ms,
        "updated_at_ms": run.updated_at_ms,
        "fired_at_ms": run.fired_at_ms,
        "started_at_ms": run.started_at_ms,
        "finished_at_ms": run.finished_at_ms,
        "mode": mode_from_args(&run.args),
        "mission_snapshot": {
            "objective": objective_from_args(&run.args, &run.routine_id, &run.entrypoint),
            "success_criteria": success_criteria_from_args(&run.args),
            "entrypoint_compat": run.entrypoint,
        },
        "policy_snapshot": {
            "tool": {
                "run_allowlist": run.allowed_tools,
            },
            "approval": {
                "requires_approval": run.requires_approval
            }
        },
        "model_policy": run.args.get("model_policy").cloned(),
        "requires_approval": run.requires_approval,
        "approval_reason": run.approval_reason,
        "denial_reason": run.denial_reason,
        "paused_reason": run.paused_reason,
        "detail": run.detail,
        "output_targets": run.output_targets,
        "artifacts": run.artifacts,
        "correlation_id": run.run_id,
    })
}

fn routine_event_to_run_event(event: &EngineEvent) -> Option<EngineEvent> {
    let mut props = event.properties.clone();
    let event_type = match event.event_type.as_str() {
        "routine.run.created" => "run.started",
        "routine.run.started" => "run.step",
        "routine.run.completed" => "run.completed",
        "routine.run.failed" => "run.failed",
        "routine.approval_required" => "approval.required",
        "routine.run.artifact_added" => "run.step",
        "routine.run.model_selected" => "run.step",
        "routine.blocked" => "run.failed",
        _ => return None,
    };
    if let Some(routine_id) = props
        .get("routineID")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
    {
        props
            .as_object_mut()
            .expect("object")
            .insert("automationID".to_string(), Value::String(routine_id));
    }
    if event.event_type == "routine.run.started"
        || event.event_type == "routine.run.artifact_added"
        || event.event_type == "routine.run.model_selected"
    {
        props
            .as_object_mut()
            .expect("object")
            .insert("phase".to_string(), Value::String("do".to_string()));
    }
    Some(EngineEvent::new(event_type, props))
}

fn automation_create_to_routine(input: AutomationCreateInput) -> Result<RoutineSpec, String> {
    if input.mission.objective.trim().is_empty() {
        return Err("mission.objective is required".to_string());
    }
    let mode = normalize_automation_mode(input.mode.as_deref())?;
    let mut args = json!({
        "prompt": input.mission.objective.trim(),
        "success_criteria": input.mission.success_criteria,
        "mode": mode,
    });
    if let Some(briefing) = input.mission.briefing {
        if let Some(obj) = args.as_object_mut() {
            obj.insert("briefing".to_string(), Value::String(briefing));
        }
    }
    if let Some(policy) = input.policy.as_ref() {
        if let Some(value) = policy.tool.orchestrator_only_tool_calls {
            if let Some(obj) = args.as_object_mut() {
                obj.insert(
                    "orchestrator_only_tool_calls".to_string(),
                    Value::Bool(value),
                );
            }
        }
    }
    if let Some(model_policy) = input.model_policy {
        validate_model_policy(&model_policy)?;
        if let Some(obj) = args.as_object_mut() {
            obj.insert("model_policy".to_string(), model_policy);
        }
    }
    let (allowed_tools, external_integrations_allowed, requires_approval) =
        if let Some(policy) = input.policy {
            (
                policy.tool.run_allowlist.unwrap_or_default(),
                policy.tool.external_integrations_allowed.unwrap_or(false),
                policy.approval.requires_approval.unwrap_or(true),
            )
        } else {
            (Vec::new(), false, true)
        };
    Ok(RoutineSpec {
        routine_id: input
            .automation_id
            .unwrap_or_else(|| format!("automation-{}", uuid::Uuid::new_v4().simple())),
        name: input.name,
        status: RoutineStatus::Active,
        schedule: input.schedule,
        timezone: input.timezone.unwrap_or_else(|| "UTC".to_string()),
        misfire_policy: input
            .misfire_policy
            .unwrap_or(RoutineMisfirePolicy::RunOnce),
        entrypoint: input
            .mission
            .entrypoint_compat
            .unwrap_or_else(|| "mission.default".to_string()),
        args: Value::Object(args.as_object().cloned().unwrap_or_default()),
        allowed_tools,
        output_targets: input.output_targets.unwrap_or_default(),
        creator_type: input.creator_type.unwrap_or_else(|| "user".to_string()),
        creator_id: input.creator_id.unwrap_or_else(|| "desktop".to_string()),
        requires_approval,
        external_integrations_allowed,
        next_fire_at_ms: input.next_fire_at_ms,
        last_fired_at_ms: None,
    })
}

async fn automations_create(
    State(state): State<AppState>,
    Json(input): Json<AutomationCreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let routine = automation_create_to_routine(input).map_err(|detail| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "Invalid automation definition",
                "code": "AUTOMATION_INVALID",
                "detail": detail,
            })),
        )
    })?;
    let saved = state
        .put_routine(routine)
        .await
        .map_err(routine_error_response)?;
    state.event_bus.publish(EngineEvent::new(
        "automation.updated",
        json!({
            "automationID": saved.routine_id,
        }),
    ));
    Ok(Json(json!({
        "automation": routine_to_automation_wire(saved)
    })))
}

async fn automations_list(State(state): State<AppState>) -> Json<Value> {
    let rows = state
        .list_routines()
        .await
        .into_iter()
        .map(routine_to_automation_wire)
        .collect::<Vec<_>>();
    Json(json!({
        "automations": rows,
        "count": rows.len(),
    }))
}

async fn automations_patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AutomationPatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let mut routine = state.get_routine(&id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Automation not found",
                "code":"AUTOMATION_NOT_FOUND",
                "automationID": id,
            })),
        )
    })?;
    if let Some(name) = input.name.as_ref() {
        routine.name = name.clone();
    }
    if let Some(status) = input.status.as_ref() {
        routine.status = status.clone();
    }
    if let Some(schedule) = input.schedule.as_ref() {
        routine.schedule = schedule.clone();
    }
    if let Some(timezone) = input.timezone.as_ref() {
        routine.timezone = timezone.clone();
    }
    if let Some(misfire_policy) = input.misfire_policy.as_ref() {
        routine.misfire_policy = misfire_policy.clone();
    }
    if let Some(next_fire_at_ms) = input.next_fire_at_ms {
        routine.next_fire_at_ms = Some(next_fire_at_ms);
    }
    if let Some(output_targets) = input.output_targets.as_ref() {
        routine.output_targets = output_targets.clone();
    }
    if let Some(model_policy) = input.model_policy.as_ref() {
        let mut args = routine.args.as_object().cloned().unwrap_or_default();
        if model_policy
            .as_object()
            .map(|obj| obj.is_empty())
            .unwrap_or(false)
        {
            args.remove("model_policy");
        } else if model_policy.is_object() {
            validate_model_policy(model_policy).map_err(|detail| {
                (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": "Invalid automation patch",
                        "code": "AUTOMATION_INVALID",
                        "detail": detail,
                    })),
                )
            })?;
            args.insert("model_policy".to_string(), model_policy.clone());
        } else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Invalid automation patch",
                    "code": "AUTOMATION_INVALID",
                    "detail": "model_policy must be an object (use {} to clear)",
                })),
            ));
        }
        routine.args = Value::Object(args);
    }
    if let Some(policy) = input.policy.as_ref() {
        if let Some(allowed) = policy.tool.run_allowlist.as_ref() {
            routine.allowed_tools = allowed.clone();
        }
        if let Some(external_allowed) = policy.tool.external_integrations_allowed {
            routine.external_integrations_allowed = external_allowed;
        }
        if let Some(requires_approval) = policy.approval.requires_approval {
            routine.requires_approval = requires_approval;
        }
        if let Some(orchestrator_only) = policy.tool.orchestrator_only_tool_calls {
            let mut args = routine.args.as_object().cloned().unwrap_or_default();
            args.insert(
                "orchestrator_only_tool_calls".to_string(),
                Value::Bool(orchestrator_only),
            );
            routine.args = Value::Object(args);
        }
    }
    if let Some(mode) = input.mode.as_ref() {
        let normalized_mode = normalize_automation_mode(Some(mode.as_str())).map_err(|detail| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "error": "Invalid automation patch",
                    "code": "AUTOMATION_INVALID",
                    "detail": detail,
                })),
            )
        })?;
        let mut args = routine.args.as_object().cloned().unwrap_or_default();
        args.insert("mode".to_string(), Value::String(normalized_mode));
        routine.args = Value::Object(args);
    }
    if let Some(mission) = input.mission.as_ref() {
        let mut args = routine.args.as_object().cloned().unwrap_or_default();
        if let Some(objective) = mission.objective.as_ref() {
            args.insert("prompt".to_string(), Value::String(objective.clone()));
        }
        if let Some(success_criteria) = mission.success_criteria.as_ref() {
            args.insert("success_criteria".to_string(), json!(success_criteria));
        }
        if let Some(briefing) = mission.briefing.as_ref() {
            args.insert("briefing".to_string(), Value::String(briefing.clone()));
        }
        if let Some(entrypoint) = mission.entrypoint_compat.as_ref() {
            routine.entrypoint = entrypoint.clone();
        }
        routine.args = Value::Object(args);
    }
    let updated = state
        .put_routine(routine)
        .await
        .map_err(routine_error_response)?;
    Ok(Json(json!({
        "automation": routine_to_automation_wire(updated)
    })))
}

async fn automations_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let deleted = state
        .delete_routine(&id)
        .await
        .map_err(routine_error_response)?
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "error":"Automation not found",
                    "code":"AUTOMATION_NOT_FOUND",
                    "automationID": id,
                })),
            )
        })?;
    Ok(Json(json!({
        "ok": true,
        "automation": routine_to_automation_wire(deleted)
    })))
}

async fn automations_run_now(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RoutineRunNowInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_now(State(state.clone()), Path(id), Json(input)).await?;
    let payload = response.0;
    let run_id = payload
        .get("runID")
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Run ID missing", "code": "AUTOMATION_RUN_MAPPING_FAILED"})),
            )
        })?;
    let run = state.get_routine_run(run_id).await.ok_or_else(|| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "Run lookup failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"})),
        )
    })?;
    Ok(Json(json!({
        "ok": true,
        "status": payload.get("status").cloned().unwrap_or(Value::String("queued".to_string())),
        "run": routine_run_to_automation_wire(run),
    })))
}

async fn automations_history(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineHistoryQuery>,
) -> Json<Value> {
    let response = routines_history(State(state), Path(id.clone()), Query(query)).await;
    let mut payload = response.0;
    if let Some(object) = payload.as_object_mut() {
        object.insert("automationID".to_string(), Value::String(id));
        object.remove("routineID");
    }
    Json(payload)
}

async fn automations_runs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(25).clamp(1, 200);
    let rows = state
        .list_routine_runs(Some(&id), limit)
        .await
        .into_iter()
        .map(routine_run_to_automation_wire)
        .collect::<Vec<_>>();
    Json(json!({
        "runs": rows,
        "count": rows.len(),
    }))
}

async fn automations_runs_all(
    State(state): State<AppState>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(25).clamp(1, 200);
    let rows = state
        .list_routine_runs(query.routine_id.as_deref(), limit)
        .await
        .into_iter()
        .map(routine_run_to_automation_wire)
        .collect::<Vec<_>>();
    Json(json!({
        "runs": rows,
        "count": rows.len(),
    }))
}

async fn automations_run_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let run = state.get_routine_run(&run_id).await.ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error":"Automation run not found",
                "code":"AUTOMATION_RUN_NOT_FOUND",
                "runID": run_id,
            })),
        )
    })?;
    Ok(Json(json!({
        "run": routine_run_to_automation_wire(run)
    })))
}

async fn automations_run_approve(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_approve(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    Ok(Json(
        json!({ "ok": true, "run": routine_run_to_automation_wire(run) }),
    ))
}

async fn automations_run_deny(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_deny(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    Ok(Json(
        json!({ "ok": true, "run": routine_run_to_automation_wire(run) }),
    ))
}

async fn automations_run_pause(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_pause(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    Ok(Json(
        json!({ "ok": true, "run": routine_run_to_automation_wire(run) }),
    ))
}

async fn automations_run_resume(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_resume(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    Ok(Json(
        json!({ "ok": true, "run": routine_run_to_automation_wire(run) }),
    ))
}

async fn automations_run_artifacts(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_artifacts(State(state), Path(run_id.clone())).await?;
    let mut payload = response.0;
    if let Some(object) = payload.as_object_mut() {
        object.insert("automationRunID".to_string(), Value::String(run_id));
        object.remove("runID");
    }
    Ok(Json(payload))
}

async fn automations_run_artifact_add(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunArtifactInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let response = routines_run_artifact_add(State(state), Path(run_id), Json(input)).await?;
    let run = response
        .0
        .get("run")
        .and_then(|v| serde_json::from_value::<RoutineRunRecord>(v.clone()).ok())
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error": "Run mapping failed", "code": "AUTOMATION_RUN_MAPPING_FAILED"}),
                ),
            )
        })?;
    Ok(Json(
        json!({ "ok": true, "run": routine_run_to_automation_wire(run) }),
    ))
}

fn automations_sse_stream(
    state: AppState,
    automation_id: Option<String>,
    run_id: Option<String>,
) -> impl Stream<Item = Result<Event, std::convert::Infallible>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "automations",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            let mapped = routine_event_to_run_event(&event)?;
            if let Some(automation_id) = automation_id.as_deref() {
                let event_automation_id = mapped
                    .properties
                    .get("automationID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if event_automation_id != automation_id {
                    return None;
                }
            }
            if let Some(run_id) = run_id.as_deref() {
                let event_run_id = mapped
                    .properties
                    .get("runID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if event_run_id != run_id {
                    return None;
                }
            }
            let payload = serde_json::to_string(&mapped).unwrap_or_default();
            Some(Ok(Event::default().data(payload)))
        }
        Err(_) => None,
    });
    ready.chain(live)
}

async fn automations_events(
    State(state): State<AppState>,
    Query(query): Query<AutomationEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    Sse::new(automations_sse_stream(
        state,
        query.automation_id,
        query.run_id,
    ))
    .keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
}

fn normalize_automation_v2_agent(mut agent: AutomationAgentProfile) -> AutomationAgentProfile {
    if agent.display_name.trim().is_empty() {
        agent.display_name = agent.agent_id.clone();
    }
    if agent.tool_policy.allowlist.is_empty() {
        agent.tool_policy = AutomationAgentToolPolicy {
            allowlist: vec!["read".to_string()],
            denylist: Vec::new(),
        };
    }
    if agent.mcp_policy.allowed_servers.is_empty() {
        agent.mcp_policy = AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        };
    }
    agent
}

async fn automations_v2_create(
    State(state): State<AppState>,
    Json(input): Json<AutomationV2CreateInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let now = crate::now_ms();
    let automation = AutomationV2Spec {
        automation_id: input
            .automation_id
            .unwrap_or_else(|| format!("automation-v2-{}", Uuid::new_v4())),
        name: input.name,
        description: input.description,
        status: input.status.unwrap_or(AutomationV2Status::Draft),
        schedule: input.schedule,
        agents: input
            .agents
            .into_iter()
            .map(normalize_automation_v2_agent)
            .collect(),
        flow: input.flow,
        execution: input.execution.unwrap_or(AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
        }),
        output_targets: input.output_targets.unwrap_or_default(),
        created_at_ms: now,
        updated_at_ms: now,
        creator_id: input.creator_id.unwrap_or_else(|| "unknown".to_string()),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error.to_string(),
                "code": "AUTOMATION_V2_CREATE_FAILED",
            })),
        )
    })?;
    Ok(Json(json!({ "automation": stored })))
}

async fn automations_v2_list(State(state): State<AppState>) -> Json<Value> {
    let rows = state.list_automations_v2().await;
    Json(json!({
        "automations": rows,
        "count": rows.len(),
    }))
}

async fn automations_v2_get(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    Ok(Json(json!({ "automation": automation })))
}

async fn automations_v2_patch(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<AutomationV2PatchInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    if let Some(name) = input.name {
        automation.name = name;
    }
    if let Some(description) = input.description {
        automation.description = Some(description);
    }
    if let Some(status) = input.status {
        automation.status = status;
    }
    if let Some(schedule) = input.schedule {
        automation.schedule = schedule;
    }
    if let Some(agents) = input.agents {
        automation.agents = agents
            .into_iter()
            .map(normalize_automation_v2_agent)
            .collect();
    }
    if let Some(flow) = input.flow {
        automation.flow = flow;
    }
    if let Some(execution) = input.execution {
        automation.execution = execution;
    }
    if let Some(output_targets) = input.output_targets {
        automation.output_targets = output_targets;
    }
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": error.to_string(),
                "code": "AUTOMATION_V2_UPDATE_FAILED",
            })),
        )
    })?;
    Ok(Json(json!({ "automation": stored })))
}

async fn automations_v2_delete(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let deleted = state.delete_automation_v2(&id).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": error.to_string(),
                "code": "AUTOMATION_V2_DELETE_FAILED",
            })),
        )
    })?;
    if deleted.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    }
    Ok(Json(
        json!({ "ok": true, "deleted": true, "automationID": id }),
    ))
}

async fn automations_v2_run_now(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": error.to_string(),
                    "code": "AUTOMATION_V2_RUN_CREATE_FAILED",
                })),
            )
        })?;
    Ok(Json(json!({ "ok": true, "run": run })))
}

async fn automations_v2_pause(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    automation.status = AutomationV2Status::Paused;
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_UPDATE_FAILED"})),
        )
    })?;
    let reason = reason_or_default(input.reason, "paused by operator");
    let runs = state.list_automation_v2_runs(Some(&id), 100).await;
    for run in runs {
        if run.status == AutomationRunStatus::Running {
            let _ = state
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Pausing;
                    row.pause_reason = Some(reason.clone());
                })
                .await;
            for session_id in run.active_session_ids {
                let _ = state.cancellations.cancel(&session_id).await;
            }
            let _ = state
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Paused;
                })
                .await;
        }
    }
    Ok(Json(json!({ "ok": true, "automation": stored })))
}

async fn automations_v2_resume(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(mut automation) = state.get_automation_v2(&id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Automation not found", "code":"AUTOMATION_V2_NOT_FOUND", "automationID": id}),
            ),
        ));
    };
    automation.status = AutomationV2Status::Active;
    let stored = state.put_automation_v2(automation).await.map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": error.to_string(), "code":"AUTOMATION_V2_UPDATE_FAILED"})),
        )
    })?;
    Ok(Json(json!({ "ok": true, "automation": stored })))
}

async fn automations_v2_runs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(query): Query<RoutineRunsQuery>,
) -> Json<Value> {
    let limit = query.limit.unwrap_or(50);
    let rows = state.list_automation_v2_runs(Some(&id), limit).await;
    Json(json!({ "automationID": id, "runs": rows, "count": rows.len() }))
}

async fn automations_v2_run_get(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(run) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    Ok(Json(json!({ "run": run })))
}

async fn automations_v2_run_pause(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if !matches!(
        current.status,
        AutomationRunStatus::Running | AutomationRunStatus::Queued
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run is not pausable", "code":"AUTOMATION_V2_RUN_NOT_PAUSABLE", "runID": run_id}),
            ),
        ));
    }
    let reason = reason_or_default(input.reason, "paused by operator");
    let _ = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Pausing;
            run.pause_reason = Some(reason.clone());
        })
        .await;
    let latest = state.get_automation_v2_run(&run_id).await;
    if let Some(run) = latest {
        for session_id in run.active_session_ids {
            let _ = state.cancellations.cancel(&session_id).await;
        }
    }
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Paused;
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    Ok(Json(json!({ "ok": true, "run": updated })))
}

async fn automations_v2_run_resume(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if current.status != AutomationRunStatus::Paused {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run is not paused", "code":"AUTOMATION_V2_RUN_NOT_PAUSED", "runID": run_id}),
            ),
        ));
    }
    let reason = reason_or_default(input.reason, "resumed by operator");
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Queued;
            run.resume_reason = Some(reason.clone());
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    Ok(Json(json!({ "ok": true, "run": updated })))
}

async fn automations_v2_run_cancel(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
    Json(input): Json<RoutineRunDecisionInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(current) = state.get_automation_v2_run(&run_id).await else {
        return Err((
            StatusCode::NOT_FOUND,
            Json(
                json!({"error":"Run not found", "code":"AUTOMATION_V2_RUN_NOT_FOUND", "runID": run_id}),
            ),
        ));
    };
    if matches!(
        current.status,
        AutomationRunStatus::Cancelled
            | AutomationRunStatus::Completed
            | AutomationRunStatus::Failed
    ) {
        return Err((
            StatusCode::CONFLICT,
            Json(
                json!({"error":"Run already terminal", "code":"AUTOMATION_V2_RUN_TERMINAL", "runID": run_id}),
            ),
        ));
    }
    for session_id in current.active_session_ids {
        let _ = state.cancellations.cancel(&session_id).await;
    }
    let reason = reason_or_default(input.reason, "cancelled by operator");
    let updated = state
        .update_automation_v2_run(&run_id, |run| {
            run.status = AutomationRunStatus::Cancelled;
            run.detail = Some(reason.clone());
        })
        .await
        .ok_or_else(|| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({"error":"Run update failed", "code":"AUTOMATION_V2_RUN_UPDATE_FAILED"}),
                ),
            )
        })?;
    Ok(Json(json!({ "ok": true, "run": updated })))
}

async fn automations_v2_events(
    State(state): State<AppState>,
    Query(query): Query<AutomationEventsQuery>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let ready = tokio_stream::once(Ok(Event::default().data(
        serde_json::to_string(&json!({
            "status": "ready",
            "stream": "automations_v2",
            "timestamp_ms": crate::now_ms(),
        }))
        .unwrap_or_default(),
    )));
    let rx = state.event_bus.subscribe();
    let live = BroadcastStream::new(rx).filter_map(move |msg| match msg {
        Ok(event) => {
            if !event.event_type.starts_with("automation.v2.") {
                return None;
            }
            if let Some(automation_id) = query.automation_id.as_deref() {
                let value = event
                    .properties
                    .get("automationID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if value != automation_id {
                    return None;
                }
            }
            if let Some(run_id) = query.run_id.as_deref() {
                let value = event
                    .properties
                    .get("runID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                if value != run_id {
                    return None;
                }
            }
            let payload = serde_json::to_string(&event).unwrap_or_default();
            Some(Ok(Event::default().data(payload)))
        }
        Err(_) => None,
    });
    Sse::new(ready.chain(live)).keep_alive(KeepAlive::new().interval(Duration::from_secs(10)))
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
            "/skills/import/preview":{"post":{"summary":"Preview skill import conflicts/actions"}},
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
            "/packs/{selector}/update":{"post":{"summary":"Apply update for installed pack (stub)"}},
            "/capabilities/bindings":{"get":{"summary":"List capability bindings"},"put":{"summary":"Replace capability bindings file"}},
            "/capabilities/discovery":{"get":{"summary":"Discover available provider tools for capability resolution"}},
            "/capabilities/resolve":{"post":{"summary":"Resolve required capabilities to provider tools based on bindings and preference"}},
            "/capabilities/readiness":{"post":{"summary":"Evaluate capability readiness and return actionable blocking issues"}},
            "/presets/index":{"get":{"summary":"List layered preset registry index (builtins, packs, overrides)"}},
            "/presets/compose/preview":{"post":{"summary":"Deterministically compose preset prompt fragments (core->domain->style->safety)"}},
            "/presets/fork":{"post":{"summary":"Fork builtin/pack preset file into project overrides"}},
            "/presets/overrides/{kind}/{id}":{"put":{"summary":"Save project override preset content"},"delete":{"summary":"Delete project override preset"}},
            "/presets/capability_summary":{"post":{"summary":"Compute agent + automation capability summary (required dominates optional)"}},
            "/presets/export_overrides":{"post":{"summary":"Export project preset overrides as portable tandempack zip"}},
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
            "/routines/{id}/runs":{"get":{"summary":"List routine runs for a routine"}},
            "/routines/runs":{"get":{"summary":"List routine runs across routines"}},
            "/routines/runs/{run_id}":{"get":{"summary":"Get a routine run record"}},
            "/routines/runs/{run_id}/approve":{"post":{"summary":"Approve a pending routine run"}},
            "/routines/runs/{run_id}/deny":{"post":{"summary":"Deny a pending routine run"}},
            "/routines/runs/{run_id}/pause":{"post":{"summary":"Pause a routine run"}},
            "/routines/runs/{run_id}/resume":{"post":{"summary":"Resume a paused routine run"}},
            "/routines/runs/{run_id}/artifacts":{"get":{"summary":"List routine run artifacts"},"post":{"summary":"Attach artifact to routine run"}},
            "/routines/events":{"get":{"summary":"SSE stream for routine lifecycle events"}},
            "/automations":{"get":{"summary":"List automations"},"post":{"summary":"Create automation"}},
            "/automations/{id}":{"patch":{"summary":"Update automation"},"delete":{"summary":"Delete automation"}},
            "/automations/{id}/run_now":{"post":{"summary":"Trigger automation immediately"}},
            "/automations/{id}/history":{"get":{"summary":"List automation history"}},
            "/automations/{id}/runs":{"get":{"summary":"List runs for an automation"}},
            "/automations/runs":{"get":{"summary":"List automation runs"}},
            "/automations/runs/{run_id}":{"get":{"summary":"Get an automation run"}},
            "/automations/runs/{run_id}/approve":{"post":{"summary":"Approve a pending automation run"}},
            "/automations/runs/{run_id}/deny":{"post":{"summary":"Deny a pending automation run"}},
            "/automations/runs/{run_id}/pause":{"post":{"summary":"Pause an automation run"}},
            "/automations/runs/{run_id}/resume":{"post":{"summary":"Resume a paused automation run"}},
            "/automations/runs/{run_id}/artifacts":{"get":{"summary":"List automation run artifacts"},"post":{"summary":"Attach artifact to automation run"}},
            "/automations/events":{"get":{"summary":"SSE stream for automation run events"}},
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
        PluginRegistry, Storage, ToolPolicyContext, ToolPolicyHook,
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
        let tandem_home = root.join("tandem-home");
        std::env::set_var("TANDEM_GLOBAL_CONFIG", &global);
        std::env::set_var("TANDEM_HOME", &tandem_home);
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

    fn write_pack_zip(path: &std::path::Path, manifest: &str) {
        let file = std::fs::File::create(path).expect("create zip");
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("tandempack.yaml", opts)
            .expect("start marker");
        std::io::Write::write_all(&mut zip, manifest.as_bytes()).expect("write marker");
        zip.start_file("README.md", opts).expect("start readme");
        std::io::Write::write_all(&mut zip, b"# pack").expect("write readme");
        zip.finish().expect("finish zip");
    }

    fn write_plain_zip_without_marker(path: &std::path::Path) {
        let file = std::fs::File::create(path).expect("create zip");
        let mut zip = zip::ZipWriter::new(file);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        zip.start_file("README.md", opts).expect("start readme");
        std::io::Write::write_all(&mut zip, b"# not a pack").expect("write readme");
        zip.start_file("agents/a.txt", opts)
            .expect("start agents file");
        std::io::Write::write_all(&mut zip, b"agent body").expect("write agents file");
        zip.finish().expect("finish zip");
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
    async fn permission_reply_route_rejects_invalid_reply() {
        let state = test_state().await;
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/permission/some-request/reply")
            .header("content-type", "application/json")
            .body(Body::from(json!({"reply":"invalid"}).to_string()))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            payload.get("code").and_then(|v| v.as_str()),
            Some("invalid_permission_reply")
        );
    }

    #[test]
    fn load_run_events_jsonl_filters_since_and_tail() {
        let test_root = std::env::temp_dir().join(format!("run-events-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&test_root).expect("mkdir");
        let path = test_root.join("events.jsonl");
        std::fs::write(
            &path,
            [
                serde_json::json!({"seq":1,"type":"run_created"}).to_string(),
                serde_json::json!({"seq":2,"type":"planning_started"}).to_string(),
                serde_json::json!({"seq":3,"type":"task_started"}).to_string(),
            ]
            .join("\n"),
        )
        .expect("write");

        let since = load_run_events_jsonl(&path, Some(1), None);
        assert_eq!(since.len(), 2);
        assert_eq!(since[0].get("seq").and_then(|v| v.as_u64()), Some(2));
        assert_eq!(since[1].get("seq").and_then(|v| v.as_u64()), Some(3));

        let tail = load_run_events_jsonl(&path, None, Some(1));
        assert_eq!(tail.len(), 1);
        assert_eq!(tail[0].get("seq").and_then(|v| v.as_u64()), Some(3));

        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&test_root);
    }

    #[tokio::test]
    async fn permission_reply_route_returns_not_found_for_unknown_request() {
        let state = test_state().await;
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/permission/missing-request/reply")
            .header("content-type", "application/json")
            .body(Body::from(json!({"reply":"always"}).to_string()))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            payload.get("code").and_then(|v| v.as_str()),
            Some("permission_request_not_found")
        );
    }

    #[tokio::test]
    async fn permission_reply_route_applies_and_persists_allow_rule() {
        let state = test_state().await;
        let request = state
            .permissions
            .ask_for_session(Some("s1"), "glob", json!({"pattern":"*"}))
            .await;
        let app = app_router(state.clone());
        let req = Request::builder()
            .method("POST")
            .uri(format!("/permission/{}/reply", request.id))
            .header("content-type", "application/json")
            .body(Body::from(json!({"reply":"always"}).to_string()))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(payload.get("ok").and_then(|v| v.as_bool()), Some(true));
        assert_eq!(
            payload.get("reply").and_then(|v| v.as_str()),
            Some("always")
        );
        assert_eq!(
            payload.get("persistedRule").and_then(|v| v.as_bool()),
            Some(true)
        );
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
    async fn packs_detect_requires_root_marker() {
        let state = test_state().await;
        let mut rx = state.event_bus.subscribe();
        let root = std::env::temp_dir().join(format!("tandem-pack-detect-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let plain_zip = root.join("plain.zip");
        write_pack_zip(
            &plain_zip,
            "name: detect-test\nversion: 1.0.0\ntype: skill\npack_id: detect-test\n",
        );
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/packs/detect")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "path": plain_zip.to_string_lossy()
                })
                .to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(payload.get("is_pack").and_then(|v| v.as_bool()), Some(true));
        let event = next_event_of_type(&mut rx, "pack.detected").await;
        assert_eq!(
            event.properties.get("marker").and_then(|v| v.as_str()),
            Some("tandempack.yaml")
        );
        assert_eq!(
            event.properties.get("path").and_then(|v| v.as_str()),
            Some(plain_zip.to_string_lossy().as_ref())
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn packs_detect_returns_false_without_marker() {
        let state = test_state().await;
        let root = std::env::temp_dir().join(format!("tandem-pack-detect-none-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let plain_zip = root.join("plain.zip");
        write_plain_zip_without_marker(&plain_zip);
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/packs/detect")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "path": plain_zip.to_string_lossy()
                })
                .to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            payload.get("is_pack").and_then(|v| v.as_bool()),
            Some(false)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn packs_install_list_and_uninstall_roundtrip() {
        let state = test_state().await;
        let mut rx = state.event_bus.subscribe();
        let root = std::env::temp_dir().join(format!("tandem-pack-install-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let pack_zip = root.join("pack.zip");
        write_pack_zip(
            &pack_zip,
            "name: roundtrip-pack\nversion: 1.2.3\ntype: workflow\npack_id: roundtrip-pack\n",
        );
        let app = app_router(state.clone());
        let install_req = Request::builder()
            .method("POST")
            .uri("/packs/install")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "path": pack_zip.to_string_lossy(),
                    "source": {"kind":"test"}
                })
                .to_string(),
            ))
            .expect("request");
        let install_resp = app.clone().oneshot(install_req).await.expect("response");
        assert_eq!(install_resp.status(), StatusCode::OK);
        let install_body = to_bytes(install_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let install_payload: Value = serde_json::from_slice(&install_body).expect("json");
        let install_path = install_payload
            .get("installed")
            .and_then(|v| v.get("install_path"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(install_path.ends_with("/roundtrip-pack/1.2.3"));
        let current_pointer = std::path::PathBuf::from(install_path)
            .parent()
            .expect("parent")
            .join("current");
        let current_version = std::fs::read_to_string(&current_pointer).expect("read current");
        assert_eq!(current_version.trim(), "1.2.3");
        let started = next_event_of_type(&mut rx, "pack.install.started").await;
        assert_eq!(
            started.properties.get("path").and_then(|v| v.as_str()),
            Some(pack_zip.to_string_lossy().as_ref())
        );
        let succeeded = next_event_of_type(&mut rx, "pack.install.succeeded").await;
        assert_eq!(
            succeeded.properties.get("pack_id").and_then(|v| v.as_str()),
            Some("roundtrip-pack")
        );
        let registry = next_event_of_type(&mut rx, "registry.updated").await;
        assert_eq!(
            registry.properties.get("entity").and_then(|v| v.as_str()),
            Some("packs")
        );

        let list_req = Request::builder()
            .method("GET")
            .uri("/packs")
            .body(Body::empty())
            .expect("request");
        let list_resp = app.clone().oneshot(list_req).await.expect("response");
        assert_eq!(list_resp.status(), StatusCode::OK);
        let list_body = to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
        let packs = list_payload
            .get("packs")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(packs.iter().any(|p| {
            p.get("pack_id").and_then(|v| v.as_str()) == Some("roundtrip-pack")
                && p.get("version").and_then(|v| v.as_str()) == Some("1.2.3")
        }));

        let uninstall_req = Request::builder()
            .method("POST")
            .uri("/packs/uninstall")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "pack_id": "roundtrip-pack",
                    "version": "1.2.3"
                })
                .to_string(),
            ))
            .expect("request");
        let uninstall_resp = app.clone().oneshot(uninstall_req).await.expect("response");
        assert_eq!(uninstall_resp.status(), StatusCode::OK);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn capabilities_resolve_prefers_arcade_when_requested() {
        let state = test_state().await;
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/capabilities/resolve")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "workflow_id": "wf-pr",
                    "required_capabilities": ["github.create_pull_request"],
                    "provider_preference": ["arcade", "composio"],
                    "available_tools": [
                        {"provider":"composio","tool_name":"mcp.composio.github_create_pull_request","schema":{}},
                        {"provider":"arcade","tool_name":"mcp.arcade.github_create_pull_request","schema":{}}
                    ]
                })
                .to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        let provider = payload
            .get("resolution")
            .and_then(|v| v.get("resolved"))
            .and_then(|v| v.as_array())
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("provider"))
            .and_then(|v| v.as_str());
        assert_eq!(provider, Some("arcade"));
    }

    #[tokio::test]
    async fn packs_updates_endpoints_return_stub_payload() {
        let state = test_state().await;
        let root = std::env::temp_dir().join(format!("tandem-pack-updates-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&root).expect("mkdir");
        let pack_zip = root.join("pack.zip");
        write_pack_zip(
            &pack_zip,
            "name: update-pack\nversion: 1.0.0\ntype: workflow\npack_id: update-pack\n",
        );
        let app = app_router(state.clone());
        let install_req = Request::builder()
            .method("POST")
            .uri("/packs/install")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "path": pack_zip.to_string_lossy(),
                    "source": {"kind":"test"}
                })
                .to_string(),
            ))
            .expect("request");
        let install_resp = app.clone().oneshot(install_req).await.expect("response");
        assert_eq!(install_resp.status(), StatusCode::OK);

        let updates_req = Request::builder()
            .method("GET")
            .uri("/packs/update-pack/updates")
            .body(Body::empty())
            .expect("request");
        let updates_resp = app.clone().oneshot(updates_req).await.expect("response");
        assert_eq!(updates_resp.status(), StatusCode::OK);
        let updates_body = to_bytes(updates_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let updates_payload: Value = serde_json::from_slice(&updates_body).expect("json");
        assert_eq!(
            updates_payload
                .get("current_version")
                .and_then(|v| v.as_str()),
            Some("1.0.0")
        );
        assert_eq!(
            updates_payload
                .get("reapproval_required")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
        assert!(updates_payload
            .get("permissions_diff")
            .and_then(|v| v.as_object())
            .is_some());

        let apply_req = Request::builder()
            .method("POST")
            .uri("/packs/update-pack/update")
            .header("content-type", "application/json")
            .body(Body::from(json!({"target_version":"1.1.0"}).to_string()))
            .expect("request");
        let apply_resp = app.clone().oneshot(apply_req).await.expect("response");
        assert_eq!(apply_resp.status(), StatusCode::OK);
        let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let apply_payload: Value = serde_json::from_slice(&apply_body).expect("json");
        assert_eq!(
            apply_payload.get("reason").and_then(|v| v.as_str()),
            Some("updates_not_implemented")
        );
        assert_eq!(
            apply_payload
                .get("reapproval_required")
                .and_then(|v| v.as_bool()),
            Some(false)
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn capabilities_resolve_returns_missing_capability_error() {
        let state = test_state().await;
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/capabilities/resolve")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "workflow_id": "wf-pr",
                    "required_capabilities": ["github.create_pull_request"],
                    "provider_preference": ["arcade", "composio"],
                    "available_tools": []
                })
                .to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(
            payload.get("code").and_then(|v| v.as_str()),
            Some("missing_capability")
        );
        assert_eq!(
            payload.get("workflow_id").and_then(|v| v.as_str()),
            Some("wf-pr")
        );
    }

    #[tokio::test]
    async fn capabilities_readiness_returns_blocking_issues_when_unbound() {
        let state = test_state().await;
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/capabilities/readiness")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "workflow_id": "wf-readiness",
                    "required_capabilities": ["github.create_pull_request"],
                    "provider_preference": ["composio", "arcade", "mcp"],
                    "available_tools": []
                })
                .to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        let readiness = payload.get("readiness").cloned().unwrap_or(Value::Null);
        assert_eq!(
            readiness.get("runnable").and_then(|v| v.as_bool()),
            Some(false)
        );
        assert!(readiness
            .get("unbound_capabilities")
            .and_then(|v| v.as_array())
            .is_some_and(|rows| !rows.is_empty()));
        assert!(readiness
            .get("blocking_issues")
            .and_then(|v| v.as_array())
            .is_some_and(|rows| !rows.is_empty()));
    }

    #[tokio::test]
    async fn pack_builder_preview_external_goal_prefers_mcp_and_generates_mcp_actions() {
        let state = test_state().await;
        state
            .tools
            .register_tool(
                "pack_builder".to_string(),
                Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
            )
            .await;
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/tool/execute")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "tool": "pack_builder",
                    "args": {
                        "mode": "preview",
                        "goal": "create a pack that checks latest headline news and posts to slack"
                    }
                })
                .to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");

        let metadata = payload.get("metadata").cloned().unwrap_or(Value::Null);
        let mapped = metadata
            .get("mcp_mapping")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert!(
            mapped.iter().any(|row| {
                row.as_str()
                    .is_some_and(|name| name.starts_with("mcp.") && !name.trim().is_empty())
            }),
            "expected at least one MCP tool mapping for external goal"
        );

        let zip_path = metadata
            .get("zip_path")
            .and_then(|v| v.as_str())
            .expect("zip path in preview");
        let file = std::fs::File::open(zip_path).expect("open zip");
        let mut archive = zip::ZipArchive::new(file).expect("zip archive");
        let mut mission = String::new();
        std::io::Read::read_to_string(
            &mut archive.by_name("missions/default.yaml").expect("mission"),
            &mut mission,
        )
        .expect("read mission");
        assert!(
            mission.lines().any(|line| line.contains("action: mcp.")),
            "mission should explicitly invoke discovered MCP tool IDs"
        );
    }

    #[tokio::test]
    async fn pack_builder_apply_requires_explicit_approvals() {
        let state = test_state().await;
        state
            .tools
            .register_tool(
                "pack_builder".to_string(),
                Arc::new(crate::pack_builder::PackBuilderTool::new(state.clone())),
            )
            .await;
        let app = app_router(state.clone());

        let preview_req = Request::builder()
            .method("POST")
            .uri("/tool/execute")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "tool": "pack_builder",
                    "args": {
                        "mode": "preview",
                        "goal": "create a pack for notion and slack sync"
                    }
                })
                .to_string(),
            ))
            .expect("preview request");
        let preview_resp = app
            .clone()
            .oneshot(preview_req)
            .await
            .expect("preview response");
        assert_eq!(preview_resp.status(), StatusCode::OK);
        let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
            .await
            .expect("preview body");
        let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
        let plan_id = preview_payload
            .get("metadata")
            .and_then(|v| v.get("plan_id"))
            .and_then(|v| v.as_str())
            .expect("plan id");

        let apply_req = Request::builder()
            .method("POST")
            .uri("/tool/execute")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "tool": "pack_builder",
                    "args": {
                        "mode": "apply",
                        "plan_id": plan_id
                    }
                })
                .to_string(),
            ))
            .expect("apply request");
        let apply_resp = app.oneshot(apply_req).await.expect("apply response");
        assert_eq!(apply_resp.status(), StatusCode::OK);
        let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
            .await
            .expect("apply body");
        let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
        let metadata = apply_payload
            .get("metadata")
            .cloned()
            .unwrap_or(Value::Null);
        assert_eq!(
            metadata.get("error").and_then(|v| v.as_str()),
            Some("approval_required")
        );
    }

    #[tokio::test]
    async fn presets_index_route_returns_layered_index_shape() {
        let state = test_state().await;
        let app = app_router(state);
        let req = Request::builder()
            .method("GET")
            .uri("/presets/index")
            .body(Body::empty())
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        let index = payload.get("index").cloned().unwrap_or(Value::Null);
        assert!(index
            .get("skill_modules")
            .and_then(|v| v.as_array())
            .is_some());
        assert!(index
            .get("agent_presets")
            .and_then(|v| v.as_array())
            .is_some());
        assert!(index
            .get("automation_presets")
            .and_then(|v| v.as_array())
            .is_some());
        assert!(index
            .get("pack_presets")
            .and_then(|v| v.as_array())
            .is_some());
        assert!(index
            .get("generated_at_ms")
            .and_then(|v| v.as_u64())
            .is_some());
    }

    #[tokio::test]
    async fn presets_compose_preview_is_deterministic() {
        let state = test_state().await;
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/presets/compose/preview")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "base_prompt": "Base",
                    "fragments": [
                        {"id":"zeta","phase":"style","content":"Style Z"},
                        {"id":"alpha","phase":"core","content":"Core A"},
                        {"id":"safe","phase":"safety","content":"Do no harm"}
                    ]
                })
                .to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        let ordered = payload
            .get("composition")
            .and_then(|v| v.get("ordered_fragment_ids"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(ordered.len(), 3);
        assert_eq!(ordered[0].as_str(), Some("alpha"));
        assert_eq!(ordered[1].as_str(), Some("zeta"));
        assert_eq!(ordered[2].as_str(), Some("safe"));
        assert!(payload
            .get("composition")
            .and_then(|v| v.get("composition_hash"))
            .and_then(|v| v.as_str())
            .map(|s| !s.is_empty())
            .unwrap_or(false));
    }

    #[tokio::test]
    async fn presets_override_put_and_delete_roundtrip() {
        let state = test_state().await;
        let app = app_router(state);
        let put_req = Request::builder()
            .method("PUT")
            .uri("/presets/overrides/agent_preset/dev_agent")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "content": "id: dev_agent\nversion: 1.0.0\n"
                })
                .to_string(),
            ))
            .expect("request");
        let put_resp = app.clone().oneshot(put_req).await.expect("response");
        assert_eq!(put_resp.status(), StatusCode::OK);
        let put_body = to_bytes(put_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let put_payload: Value = serde_json::from_slice(&put_body).expect("json");
        assert_eq!(
            put_payload.get("saved").and_then(|v| v.as_bool()),
            Some(true)
        );

        let del_req = Request::builder()
            .method("DELETE")
            .uri("/presets/overrides/agent_preset/dev_agent")
            .body(Body::empty())
            .expect("request");
        let del_resp = app.clone().oneshot(del_req).await.expect("response");
        assert_eq!(del_resp.status(), StatusCode::OK);
        let del_body = to_bytes(del_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let del_payload: Value = serde_json::from_slice(&del_body).expect("json");
        assert_eq!(
            del_payload.get("removed").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn presets_fork_copies_source_into_overrides() {
        let state = test_state().await;
        let app = app_router(state);
        let seed_req = Request::builder()
            .method("PUT")
            .uri("/presets/overrides/skill_module/source_seed")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "content": "id: source_seed\nversion: 1.0.0\n"
                })
                .to_string(),
            ))
            .expect("request");
        let seed_resp = app.clone().oneshot(seed_req).await.expect("response");
        assert_eq!(seed_resp.status(), StatusCode::OK);
        let seed_body = to_bytes(seed_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let seed_payload: Value = serde_json::from_slice(&seed_body).expect("json");
        let source_path = seed_payload
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        assert!(!source_path.is_empty());

        let fork_req = Request::builder()
            .method("POST")
            .uri("/presets/fork")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "kind": "skill_module",
                    "source_path": source_path,
                    "target_id": "forked_skill"
                })
                .to_string(),
            ))
            .expect("request");
        let fork_resp = app.clone().oneshot(fork_req).await.expect("response");
        assert_eq!(fork_resp.status(), StatusCode::OK);
        let fork_body = to_bytes(fork_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let fork_payload: Value = serde_json::from_slice(&fork_body).expect("json");
        assert_eq!(
            fork_payload.get("forked").and_then(|v| v.as_bool()),
            Some(true)
        );
        let forked_path = fork_payload
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert!(forked_path.ends_with("forked_skill.yaml"));
    }

    #[tokio::test]
    async fn presets_capability_summary_merges_agent_and_task_caps() {
        let state = test_state().await;
        let app = app_router(state);
        let req = Request::builder()
            .method("POST")
            .uri("/presets/capability_summary")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "agent": {
                        "required": ["github.create_pull_request"],
                        "optional": ["slack.post_message"]
                    },
                    "tasks": [
                        {"required": ["slack.post_message"], "optional": []},
                        {"required": [], "optional": ["jira.create_issue"]}
                    ]
                })
                .to_string(),
            ))
            .expect("request");
        let resp = app.oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
        let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
        let payload: Value = serde_json::from_slice(&body).expect("json");
        let required = payload
            .get("summary")
            .and_then(|v| v.get("automation"))
            .and_then(|v| v.get("required"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let optional = payload
            .get("summary")
            .and_then(|v| v.get("automation"))
            .and_then(|v| v.get("optional"))
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        assert_eq!(required.len(), 2);
        assert_eq!(optional.len(), 1);
        assert_eq!(
            payload
                .get("summary")
                .and_then(|v| v.get("totals"))
                .and_then(|v| v.get("task_count"))
                .and_then(|v| v.as_u64()),
            Some(2)
        );
    }

    #[tokio::test]
    async fn presets_export_overrides_returns_zip_payload() {
        let state = test_state().await;
        let app = app_router(state);
        let seed_req = Request::builder()
            .method("PUT")
            .uri("/presets/overrides/skill_module/export_seed")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "content": "id: export_seed\nversion: 1.0.0\n"
                })
                .to_string(),
            ))
            .expect("request");
        let seed_resp = app.clone().oneshot(seed_req).await.expect("response");
        assert_eq!(seed_resp.status(), StatusCode::OK);

        let export_req = Request::builder()
            .method("POST")
            .uri("/presets/export_overrides")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "name": "exported-overrides",
                    "version": "1.0.0"
                })
                .to_string(),
            ))
            .expect("request");
        let export_resp = app.clone().oneshot(export_req).await.expect("response");
        assert_eq!(export_resp.status(), StatusCode::OK);
        let export_body = to_bytes(export_resp.into_body(), usize::MAX)
            .await
            .expect("body");
        let export_payload: Value = serde_json::from_slice(&export_body).expect("json");
        assert!(export_payload
            .get("exported")
            .and_then(|v| v.get("path"))
            .and_then(|v| v.as_str())
            .map(|s| s.ends_with(".zip"))
            .unwrap_or(false));
        assert!(
            export_payload
                .get("exported")
                .and_then(|v| v.get("bytes"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0)
                > 0
        );
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
    async fn routines_allowlist_is_persisted_and_copied_to_runs() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/routines")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "routine_id": "routine-tools",
                    "name": "Tool-scoped routine",
                    "schedule": { "interval_seconds": { "seconds": 90 } },
                    "entrypoint": "mission.default",
                    "allowed_tools": ["  mcp.arcade.search  ", "read", "read", ""],
                    "output_targets": ["  s3://reports/daily.json  ", "s3://reports/daily.json", ""]
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
        let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
        assert_eq!(
            create_payload
                .get("routine")
                .and_then(|v| v.get("allowed_tools"))
                .and_then(|v| v.as_array())
                .map(|rows| rows
                    .iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect::<Vec<_>>()),
            Some(vec!["mcp.arcade.search".to_string(), "read".to_string()])
        );
        assert_eq!(
            create_payload
                .get("routine")
                .and_then(|v| v.get("output_targets"))
                .and_then(|v| v.as_array())
                .map(|rows| rows
                    .iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect::<Vec<_>>()),
            Some(vec!["s3://reports/daily.json".to_string()])
        );

        let patch_req = Request::builder()
            .method("PATCH")
            .uri("/routines/routine-tools")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "allowed_tools": ["mcp.arcade.send_email", "bash"],
                    "output_targets": ["https://storage.example/run/output.md"]
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
                .and_then(|v| v.get("allowed_tools"))
                .and_then(|v| v.as_array())
                .map(|rows| rows
                    .iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect::<Vec<_>>()),
            Some(vec![
                "mcp.arcade.send_email".to_string(),
                "bash".to_string()
            ])
        );
        assert_eq!(
            patch_payload
                .get("routine")
                .and_then(|v| v.get("output_targets"))
                .and_then(|v| v.as_array())
                .map(|rows| rows
                    .iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect::<Vec<_>>()),
            Some(vec!["https://storage.example/run/output.md".to_string()])
        );

        let run_now_req = Request::builder()
            .method("POST")
            .uri("/routines/routine-tools/run_now")
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
        let run_id = run_now_payload
            .get("runID")
            .and_then(|v| v.as_str())
            .expect("runID");

        let run_get_req = Request::builder()
            .method("GET")
            .uri(format!("/routines/runs/{run_id}"))
            .body(Body::empty())
            .expect("run get request");
        let run_get_resp = app
            .clone()
            .oneshot(run_get_req)
            .await
            .expect("run get response");
        assert_eq!(run_get_resp.status(), StatusCode::OK);
        let run_get_body = to_bytes(run_get_resp.into_body(), usize::MAX)
            .await
            .expect("run get body");
        let run_get_payload: Value = serde_json::from_slice(&run_get_body).expect("run get json");
        assert_eq!(
            run_get_payload
                .get("run")
                .and_then(|v| v.get("allowed_tools"))
                .and_then(|v| v.as_array())
                .map(|rows| rows
                    .iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect::<Vec<_>>()),
            Some(vec![
                "mcp.arcade.send_email".to_string(),
                "bash".to_string()
            ])
        );
        assert_eq!(
            run_get_payload
                .get("run")
                .and_then(|v| v.get("output_targets"))
                .and_then(|v| v.as_array())
                .map(|rows| rows
                    .iter()
                    .filter_map(|v| v.as_str().map(ToString::to_string))
                    .collect::<Vec<_>>()),
            Some(vec!["https://storage.example/run/output.md".to_string()])
        );
    }

    #[tokio::test]
    async fn routines_runs_all_can_filter_by_routine() {
        let state = test_state().await;
        let app = app_router(state.clone());

        for routine_id in ["routine-run-a", "routine-run-b"] {
            let create_req = Request::builder()
                .method("POST")
                .uri("/routines")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "routine_id": routine_id,
                        "name": format!("Routine {routine_id}"),
                        "schedule": { "interval_seconds": { "seconds": 60 } },
                        "entrypoint": "mission.default",
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
                .uri(format!("/routines/{routine_id}/run_now"))
                .header("content-type", "application/json")
                .body(Body::from(json!({}).to_string()))
                .expect("run_now request");
            let run_now_resp = app
                .clone()
                .oneshot(run_now_req)
                .await
                .expect("run_now response");
            assert_eq!(run_now_resp.status(), StatusCode::OK);
        }

        let all_req = Request::builder()
            .method("GET")
            .uri("/routines/runs?limit=10")
            .body(Body::empty())
            .expect("runs all request");
        let all_resp = app
            .clone()
            .oneshot(all_req)
            .await
            .expect("runs all response");
        assert_eq!(all_resp.status(), StatusCode::OK);
        let all_body = to_bytes(all_resp.into_body(), usize::MAX)
            .await
            .expect("runs all body");
        let all_payload: Value = serde_json::from_slice(&all_body).expect("runs all json");
        assert!(all_payload
            .get("count")
            .and_then(|v| v.as_u64())
            .is_some_and(|count| count >= 2));

        let filtered_req = Request::builder()
            .method("GET")
            .uri("/routines/runs?routine_id=routine-run-b&limit=10")
            .body(Body::empty())
            .expect("runs filtered request");
        let filtered_resp = app
            .clone()
            .oneshot(filtered_req)
            .await
            .expect("runs filtered response");
        assert_eq!(filtered_resp.status(), StatusCode::OK);
        let filtered_body = to_bytes(filtered_resp.into_body(), usize::MAX)
            .await
            .expect("runs filtered body");
        let filtered_payload: Value =
            serde_json::from_slice(&filtered_body).expect("runs filtered json");
        assert!(filtered_payload
            .get("count")
            .and_then(|v| v.as_u64())
            .is_some_and(|count| count >= 1));
        let all_match_routine = filtered_payload
            .get("runs")
            .and_then(|v| v.as_array())
            .map(|rows| {
                rows.iter().all(|row| {
                    row.get("routine_id")
                        .and_then(|v| v.as_str())
                        .is_some_and(|id| id == "routine-run-b")
                })
            })
            .unwrap_or(false);
        assert!(all_match_routine);
    }

    #[tokio::test]
    async fn automations_create_requires_mission_objective() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/automations")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "automation_id": "auto-empty-objective",
                    "name": "Automation without objective",
                    "schedule": { "interval_seconds": { "seconds": 300 } },
                    "mission": {
                        "objective": "   "
                    }
                })
                .to_string(),
            ))
            .expect("automation create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("automation create response");
        assert_eq!(create_resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn automations_create_rejects_invalid_mode() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/automations")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "automation_id": "auto-invalid-mode",
                    "name": "Automation invalid mode",
                    "schedule": { "interval_seconds": { "seconds": 300 } },
                    "mode": "swarm-ish",
                    "mission": {
                        "objective": "Execute a mission with invalid mode."
                    }
                })
                .to_string(),
            ))
            .expect("automation create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("automation create response");
        assert_eq!(create_resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn automations_create_and_run_now_roundtrip() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/automations")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "automation_id": "auto-digest",
                    "name": "Daily Digest Automation",
                    "schedule": { "interval_seconds": { "seconds": 600 } },
                    "mission": {
                        "objective": "Generate a daily digest with clear sources.",
                        "success_criteria": ["Contains source URLs", "Writes one artifact"]
                    },
                    "policy": {
                        "tool": {
                            "run_allowlist": ["read", "websearch", "webfetch", "write"],
                            "external_integrations_allowed": true
                        },
                        "approval": {
                            "requires_approval": true
                        }
                    }
                })
                .to_string(),
            ))
            .expect("automation create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("automation create response");
        assert_eq!(create_resp.status(), StatusCode::OK);

        let run_now_req = Request::builder()
            .method("POST")
            .uri("/automations/auto-digest/run_now")
            .header("content-type", "application/json")
            .body(Body::from(json!({}).to_string()))
            .expect("automation run_now request");
        let run_now_resp = app
            .clone()
            .oneshot(run_now_req)
            .await
            .expect("automation run_now response");
        assert_eq!(run_now_resp.status(), StatusCode::OK);
        let run_now_body = to_bytes(run_now_resp.into_body(), usize::MAX)
            .await
            .expect("automation run_now body");
        let run_now_payload: Value =
            serde_json::from_slice(&run_now_body).expect("automation run_now json");
        assert_eq!(
            run_now_payload
                .get("run")
                .and_then(|v| v.get("automation_id"))
                .and_then(|v| v.as_str()),
            Some("auto-digest")
        );
        assert_eq!(
            run_now_payload
                .get("run")
                .and_then(|v| v.get("mission_snapshot"))
                .and_then(|v| v.get("objective"))
                .and_then(|v| v.as_str()),
            Some("Generate a daily digest with clear sources.")
        );
        let run_id = run_now_payload
            .get("run")
            .and_then(|v| v.get("run_id"))
            .and_then(|v| v.as_str())
            .expect("automation run_id in run_now response")
            .to_string();

        let history_req = Request::builder()
            .method("GET")
            .uri("/automations/auto-digest/history?limit=5")
            .body(Body::empty())
            .expect("automation history request");
        let history_resp = app
            .clone()
            .oneshot(history_req)
            .await
            .expect("automation history response");
        assert_eq!(history_resp.status(), StatusCode::OK);
        let history_body = to_bytes(history_resp.into_body(), usize::MAX)
            .await
            .expect("automation history body");
        let history_payload: Value =
            serde_json::from_slice(&history_body).expect("automation history json");
        assert_eq!(
            history_payload.get("automationID").and_then(|v| v.as_str()),
            Some("auto-digest")
        );

        let add_artifact_req = Request::builder()
            .method("POST")
            .uri(format!("/automations/runs/{run_id}/artifacts"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "uri": "file://reports/daily-digest.md",
                    "kind": "report",
                    "label": "Daily Digest",
                })
                .to_string(),
            ))
            .expect("automation add artifact request");
        let add_artifact_resp = app
            .clone()
            .oneshot(add_artifact_req)
            .await
            .expect("automation add artifact response");
        assert_eq!(add_artifact_resp.status(), StatusCode::OK);

        let list_artifacts_req = Request::builder()
            .method("GET")
            .uri(format!("/automations/runs/{run_id}/artifacts"))
            .body(Body::empty())
            .expect("automation list artifacts request");
        let list_artifacts_resp = app
            .clone()
            .oneshot(list_artifacts_req)
            .await
            .expect("automation list artifacts response");
        assert_eq!(list_artifacts_resp.status(), StatusCode::OK);
        let list_artifacts_body = to_bytes(list_artifacts_resp.into_body(), usize::MAX)
            .await
            .expect("automation list artifacts body");
        let list_artifacts_payload: Value =
            serde_json::from_slice(&list_artifacts_body).expect("automation list artifacts json");
        assert_eq!(
            list_artifacts_payload
                .get("automationRunID")
                .and_then(|v| v.as_str()),
            Some(run_id.as_str())
        );
        assert!(list_artifacts_payload
            .get("count")
            .and_then(|v| v.as_u64())
            .is_some_and(|count| count >= 1));

        let patch_req = Request::builder()
            .method("PATCH")
            .uri("/automations/auto-digest")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "mode": "ORCHESTRATED"
                })
                .to_string(),
            ))
            .expect("automation patch request");
        let patch_resp = app
            .clone()
            .oneshot(patch_req)
            .await
            .expect("automation patch response");
        assert_eq!(patch_resp.status(), StatusCode::OK);
        let patch_body = to_bytes(patch_resp.into_body(), usize::MAX)
            .await
            .expect("automation patch body");
        let patch_payload: Value =
            serde_json::from_slice(&patch_body).expect("automation patch json");
        assert_eq!(
            patch_payload
                .get("automation")
                .and_then(|v| v.get("mode"))
                .and_then(|v| v.as_str()),
            Some("orchestrated")
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
                    "content": concat!("-----BEGIN", " PRIVATE KEY-----"),
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
    async fn memory_demote_hides_item_from_search_results() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let put_req = Request::builder()
            .method("POST")
            .uri("/memory/put")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "run-5",
                    "partition": {
                        "org_id": "org-1",
                        "workspace_id": "ws-1",
                        "project_id": "proj-1",
                        "tier": "session"
                    },
                    "kind": "fact",
                    "content": "demote me from search",
                    "classification": "internal"
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

        let demote_req = Request::builder()
            .method("POST")
            .uri("/memory/demote")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "id": memory_id,
                    "run_id": "run-5"
                })
                .to_string(),
            ))
            .expect("demote request");
        let demote_resp = app
            .clone()
            .oneshot(demote_req)
            .await
            .expect("demote response");
        assert_eq!(demote_resp.status(), StatusCode::OK);

        let search_req = Request::builder()
            .method("POST")
            .uri("/memory/search")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "run-5",
                    "query": "demote me from search",
                    "partition": {
                        "org_id": "org-1",
                        "workspace_id": "ws-1",
                        "project_id": "proj-1",
                        "tier": "session"
                    },
                    "read_scopes": ["session"],
                    "limit": 10
                })
                .to_string(),
            ))
            .expect("search request");
        let search_resp = app
            .clone()
            .oneshot(search_req)
            .await
            .expect("search response");
        assert_eq!(search_resp.status(), StatusCode::OK);
        let search_body = to_bytes(search_resp.into_body(), usize::MAX)
            .await
            .expect("search body");
        let search_payload: Value = serde_json::from_slice(&search_body).expect("search json");
        let count = search_payload
            .get("results")
            .and_then(|v| v.as_array())
            .map(|rows| rows.len())
            .unwrap_or_default();
        assert_eq!(count, 0);
    }

    #[tokio::test]
    async fn admin_and_channel_routes_require_auth_when_api_token_enabled() {
        let state = test_state().await;
        state.set_api_token(Some("tk_test".to_string())).await;
        let app = app_router(state);

        for (method, uri) in [
            ("GET", "/channels/config"),
            ("GET", "/channels/status"),
            ("POST", "/channels/discord/verify"),
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
    async fn channels_verify_discord_without_token_returns_setup_hint() {
        let state = test_state().await;
        let app = app_router(state);

        let req = Request::builder()
            .method("POST")
            .uri("/channels/discord/verify")
            .body(Body::empty())
            .expect("request");
        let resp = app.clone().oneshot(req).await.expect("response");
        assert_eq!(resp.status(), StatusCode::OK);

        let body = to_bytes(resp.into_body(), usize::MAX)
            .await
            .expect("response body");
        let payload: Value = serde_json::from_slice(&body).expect("json body");
        assert_eq!(
            payload.get("ok").and_then(Value::as_bool),
            Some(false),
            "verify should fail without token"
        );
        assert_eq!(
            payload.get("channel").and_then(Value::as_str),
            Some("discord")
        );
        assert!(
            payload
                .get("hints")
                .and_then(Value::as_array)
                .is_some_and(|arr| !arr.is_empty()),
            "verify should include setup hints"
        );
    }

    #[test]
    fn sanitize_relative_subpath_accepts_safe_relative_paths() {
        let parsed = sanitize_relative_subpath(Some("channel_uploads/telegram"))
            .expect("safe relative path");
        assert_eq!(
            parsed.to_string_lossy().replace('\\', "/"),
            "channel_uploads/telegram"
        );
    }

    #[test]
    fn sanitize_relative_subpath_rejects_parent_segments() {
        let err = sanitize_relative_subpath(Some("../secrets")).expect_err("must reject parent");
        assert_eq!(err, StatusCode::BAD_REQUEST);
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

    #[tokio::test]
    async fn routine_tool_policy_hook_denies_disallowed_tool_for_session_scope() {
        let state = test_state().await;
        let session = Session::new(Some("routine-session".to_string()), Some(".".to_string()));
        let session_id = session.id.clone();
        state
            .storage
            .save_session(session)
            .await
            .expect("save session");

        state
            .set_routine_session_policy(
                session_id.clone(),
                "run-routine-hook-1".to_string(),
                "routine-hook-1".to_string(),
                vec!["read".to_string(), "mcp.arcade.search".to_string()],
            )
            .await;

        let hook = crate::agent_teams::ServerToolPolicyHook::new(state.clone());
        let decision = hook
            .evaluate_tool(ToolPolicyContext {
                session_id,
                message_id: "msg-1".to_string(),
                tool: "bash".to_string(),
                args: json!({"command":"echo hi"}),
            })
            .await
            .expect("policy decision");

        assert!(!decision.allowed);
        assert!(decision
            .reason
            .as_deref()
            .unwrap_or_default()
            .contains("not allowed for routine"));
    }

    #[tokio::test]
    async fn context_run_create_append_event_and_get() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/context/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "ctx-run-1",
                    "objective": "test context run"
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

        let event_req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-1/events")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "type": "planning_started",
                    "status": "planning",
                    "payload": {"k":"v"}
                })
                .to_string(),
            ))
            .expect("event request");
        let event_resp = app
            .clone()
            .oneshot(event_req)
            .await
            .expect("event response");
        assert_eq!(event_resp.status(), StatusCode::OK);

        let list_events_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-1/events?since_seq=0")
            .body(Body::empty())
            .expect("list events request");
        let list_events_resp = app
            .clone()
            .oneshot(list_events_req)
            .await
            .expect("list events response");
        assert_eq!(list_events_resp.status(), StatusCode::OK);
        let list_events_body = to_bytes(list_events_resp.into_body(), usize::MAX)
            .await
            .expect("list events body");
        let list_events_payload: Value =
            serde_json::from_slice(&list_events_body).expect("list events json");
        assert_eq!(
            list_events_payload
                .get("events")
                .and_then(|v| v.as_array())
                .map(|rows| rows.len()),
            Some(1)
        );

        let get_run_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-1")
            .body(Body::empty())
            .expect("get run request");
        let get_run_resp = app
            .clone()
            .oneshot(get_run_req)
            .await
            .expect("get run response");
        assert_eq!(get_run_resp.status(), StatusCode::OK);
        let get_run_body = to_bytes(get_run_resp.into_body(), usize::MAX)
            .await
            .expect("get run body");
        let get_run_payload: Value = serde_json::from_slice(&get_run_body).expect("get run json");
        assert_eq!(
            get_run_payload
                .get("run")
                .and_then(|run| run.get("status"))
                .and_then(Value::as_str),
            Some("planning")
        );
    }

    #[tokio::test]
    async fn context_run_lease_mismatch_pauses_run() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/context/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "ctx-run-lease",
                    "objective": "lease mismatch",
                    "workspace": {
                        "workspace_id": "ws-1",
                        "canonical_path": "/expected/path",
                        "lease_epoch": 1
                    }
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

        let validate_req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-lease/lease/validate")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "phase": "pre_dispatch",
                    "current_path": "/other/path",
                    "step_id": "step-1"
                })
                .to_string(),
            ))
            .expect("validate request");
        let validate_resp = app
            .clone()
            .oneshot(validate_req)
            .await
            .expect("validate response");
        assert_eq!(validate_resp.status(), StatusCode::OK);
        let validate_body = to_bytes(validate_resp.into_body(), usize::MAX)
            .await
            .expect("validate body");
        let validate_payload: Value =
            serde_json::from_slice(&validate_body).expect("validate json");
        assert_eq!(
            validate_payload.get("mismatch").and_then(Value::as_bool),
            Some(true)
        );

        let get_run_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-lease")
            .body(Body::empty())
            .expect("get run request");
        let get_run_resp = app
            .clone()
            .oneshot(get_run_req)
            .await
            .expect("get run response");
        let get_run_body = to_bytes(get_run_resp.into_body(), usize::MAX)
            .await
            .expect("get run body");
        let get_run_payload: Value = serde_json::from_slice(&get_run_body).expect("get run json");
        assert_eq!(
            get_run_payload
                .get("run")
                .and_then(|run| run.get("status"))
                .and_then(Value::as_str),
            Some("paused")
        );
    }

    #[tokio::test]
    async fn context_run_replay_matches_persisted_state_without_drift() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/context/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "ctx-run-replay-ok",
                    "objective": "replay no drift"
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

        let event_req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-replay-ok/events")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "type": "step_started",
                    "status": "running",
                    "step_id": "s1",
                    "payload": {
                        "step_title": "Plan",
                        "step_status": "in_progress",
                        "why_next_step": "Need active planning"
                    }
                })
                .to_string(),
            ))
            .expect("event request");
        let event_resp = app
            .clone()
            .oneshot(event_req)
            .await
            .expect("event response");
        assert_eq!(event_resp.status(), StatusCode::OK);

        let replay_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-replay-ok/replay")
            .body(Body::empty())
            .expect("replay request");
        let replay_resp = app
            .clone()
            .oneshot(replay_req)
            .await
            .expect("replay response");
        assert_eq!(replay_resp.status(), StatusCode::OK);
        let replay_body = to_bytes(replay_resp.into_body(), usize::MAX)
            .await
            .expect("replay body");
        let replay_payload: Value = serde_json::from_slice(&replay_body).expect("replay json");
        assert_eq!(
            replay_payload
                .get("drift")
                .and_then(|d| d.get("mismatch"))
                .and_then(Value::as_bool),
            Some(false)
        );
        assert_eq!(
            replay_payload
                .get("replay")
                .and_then(|r| r.get("status"))
                .and_then(Value::as_str),
            Some("running")
        );
    }

    #[tokio::test]
    async fn context_run_replay_detects_status_drift() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/context/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "ctx-run-replay-drift",
                    "objective": "replay drift"
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

        let event_req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-replay-drift/events")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "type": "planning_started",
                    "status": "planning",
                    "payload": {}
                })
                .to_string(),
            ))
            .expect("event request");
        let event_resp = app
            .clone()
            .oneshot(event_req)
            .await
            .expect("event response");
        assert_eq!(event_resp.status(), StatusCode::OK);

        let get_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-replay-drift")
            .body(Body::empty())
            .expect("get request");
        let get_resp = app.clone().oneshot(get_req).await.expect("get response");
        let get_body = to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body");
        let mut get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
        get_payload["run"]["status"] = Value::String("failed".to_string());

        let put_req = Request::builder()
            .method("PUT")
            .uri("/context/runs/ctx-run-replay-drift")
            .header("content-type", "application/json")
            .body(Body::from(
                get_payload
                    .get("run")
                    .cloned()
                    .expect("run payload")
                    .to_string(),
            ))
            .expect("put request");
        let put_resp = app.clone().oneshot(put_req).await.expect("put response");
        assert_eq!(put_resp.status(), StatusCode::OK);

        let replay_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-replay-drift/replay")
            .body(Body::empty())
            .expect("replay request");
        let replay_resp = app
            .clone()
            .oneshot(replay_req)
            .await
            .expect("replay response");
        assert_eq!(replay_resp.status(), StatusCode::OK);
        let replay_body = to_bytes(replay_resp.into_body(), usize::MAX)
            .await
            .expect("replay body");
        let replay_payload: Value = serde_json::from_slice(&replay_body).expect("replay json");
        assert_eq!(
            replay_payload
                .get("drift")
                .and_then(|d| d.get("mismatch"))
                .and_then(Value::as_bool),
            Some(true)
        );
        assert_eq!(
            replay_payload
                .get("drift")
                .and_then(|d| d.get("status_mismatch"))
                .and_then(Value::as_bool),
            Some(true)
        );
    }

    #[tokio::test]
    async fn context_run_driver_next_selects_runnable_step_and_sets_why() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/context/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "ctx-run-driver-1",
                    "objective": "meta manager select"
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

        let get_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-driver-1")
            .body(Body::empty())
            .expect("get request");
        let get_resp = app.clone().oneshot(get_req).await.expect("get response");
        let get_body = to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body");
        let mut get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
        get_payload["run"]["steps"] = json!([
            {"step_id":"s1","title":"Plan","status":"pending"},
            {"step_id":"s2","title":"Execute","status":"runnable"}
        ]);

        let put_req = Request::builder()
            .method("PUT")
            .uri("/context/runs/ctx-run-driver-1")
            .header("content-type", "application/json")
            .body(Body::from(
                get_payload
                    .get("run")
                    .cloned()
                    .expect("run payload")
                    .to_string(),
            ))
            .expect("put request");
        let put_resp = app.clone().oneshot(put_req).await.expect("put response");
        assert_eq!(put_resp.status(), StatusCode::OK);

        let next_req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-driver-1/driver/next")
            .header("content-type", "application/json")
            .body(Body::from(json!({}).to_string()))
            .expect("next request");
        let next_resp = app.clone().oneshot(next_req).await.expect("next response");
        assert_eq!(next_resp.status(), StatusCode::OK);
        let next_body = to_bytes(next_resp.into_body(), usize::MAX)
            .await
            .expect("next body");
        let next_payload: Value = serde_json::from_slice(&next_body).expect("next json");
        assert_eq!(
            next_payload.get("selected_step_id").and_then(Value::as_str),
            Some("s2")
        );
        assert!(next_payload
            .get("why_next_step")
            .and_then(Value::as_str)
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false));

        let run_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-driver-1")
            .body(Body::empty())
            .expect("run request");
        let run_resp = app.clone().oneshot(run_req).await.expect("run response");
        let run_body = to_bytes(run_resp.into_body(), usize::MAX)
            .await
            .expect("run body");
        let run_payload: Value = serde_json::from_slice(&run_body).expect("run json");
        assert_eq!(
            run_payload
                .get("run")
                .and_then(|r| r.get("status"))
                .and_then(Value::as_str),
            Some("running")
        );
        assert_eq!(
            run_payload
                .get("run")
                .and_then(|r| r.get("steps"))
                .and_then(Value::as_array)
                .and_then(|steps| steps.get(1))
                .and_then(|step| step.get("status"))
                .and_then(Value::as_str),
            Some("in_progress")
        );
    }

    #[tokio::test]
    async fn context_run_driver_next_respects_terminal_state() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/context/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "ctx-run-driver-2",
                    "objective": "terminal check"
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

        let get_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-driver-2")
            .body(Body::empty())
            .expect("get request");
        let get_resp = app.clone().oneshot(get_req).await.expect("get response");
        let get_body = to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body");
        let mut get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
        get_payload["run"]["status"] = Value::String("completed".to_string());

        let put_req = Request::builder()
            .method("PUT")
            .uri("/context/runs/ctx-run-driver-2")
            .header("content-type", "application/json")
            .body(Body::from(
                get_payload
                    .get("run")
                    .cloned()
                    .expect("run payload")
                    .to_string(),
            ))
            .expect("put request");
        let put_resp = app.clone().oneshot(put_req).await.expect("put response");
        assert_eq!(put_resp.status(), StatusCode::OK);

        let next_req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-driver-2/driver/next")
            .header("content-type", "application/json")
            .body(Body::from(json!({}).to_string()))
            .expect("next request");
        let next_resp = app.clone().oneshot(next_req).await.expect("next response");
        assert_eq!(next_resp.status(), StatusCode::OK);
        let next_body = to_bytes(next_resp.into_body(), usize::MAX)
            .await
            .expect("next body");
        let next_payload: Value = serde_json::from_slice(&next_body).expect("next json");
        assert_eq!(next_payload.get("selected_step_id"), Some(&Value::Null));
        assert_eq!(
            next_payload.get("target_status").and_then(Value::as_str),
            Some("completed")
        );
    }

    #[tokio::test]
    async fn context_run_driver_next_dry_run_does_not_mutate_state_or_events() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/context/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "ctx-run-driver-dry",
                    "objective": "dry run guardrail"
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

        let get_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-driver-dry")
            .body(Body::empty())
            .expect("get request");
        let get_resp = app.clone().oneshot(get_req).await.expect("get response");
        let get_body = to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body");
        let mut get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
        get_payload["run"]["steps"] = json!([
            {"step_id":"s1","title":"Plan","status":"runnable"}
        ]);
        let before_revision = get_payload["run"]["revision"]
            .as_u64()
            .expect("before revision");

        let put_req = Request::builder()
            .method("PUT")
            .uri("/context/runs/ctx-run-driver-dry")
            .header("content-type", "application/json")
            .body(Body::from(
                get_payload
                    .get("run")
                    .cloned()
                    .expect("run payload")
                    .to_string(),
            ))
            .expect("put request");
        let put_resp = app.clone().oneshot(put_req).await.expect("put response");
        assert_eq!(put_resp.status(), StatusCode::OK);

        let dry_next_req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-driver-dry/driver/next")
            .header("content-type", "application/json")
            .body(Body::from(json!({"dry_run": true}).to_string()))
            .expect("dry next request");
        let dry_next_resp = app
            .clone()
            .oneshot(dry_next_req)
            .await
            .expect("dry next response");
        assert_eq!(dry_next_resp.status(), StatusCode::OK);

        let run_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-driver-dry")
            .body(Body::empty())
            .expect("run request");
        let run_resp = app.clone().oneshot(run_req).await.expect("run response");
        let run_body = to_bytes(run_resp.into_body(), usize::MAX)
            .await
            .expect("run body");
        let run_payload: Value = serde_json::from_slice(&run_body).expect("run json");
        assert_eq!(
            run_payload
                .get("run")
                .and_then(|r| r.get("revision"))
                .and_then(Value::as_u64),
            Some(before_revision.saturating_add(1))
        );
        assert_eq!(
            run_payload
                .get("run")
                .and_then(|r| r.get("steps"))
                .and_then(Value::as_array)
                .and_then(|steps| steps.first())
                .and_then(|step| step.get("status"))
                .and_then(Value::as_str),
            Some("runnable")
        );

        let events_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-driver-dry/events")
            .body(Body::empty())
            .expect("events request");
        let events_resp = app
            .clone()
            .oneshot(events_req)
            .await
            .expect("events response");
        let events_body = to_bytes(events_resp.into_body(), usize::MAX)
            .await
            .expect("events body");
        let events_payload: Value = serde_json::from_slice(&events_body).expect("events json");
        let has_decision_event = events_payload
            .get("events")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter().any(|row| {
                    row.get("type")
                        .and_then(Value::as_str)
                        .map(|ty| ty == "meta_next_step_selected")
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        assert!(!has_decision_event);
    }

    #[tokio::test]
    async fn context_run_driver_next_emits_decision_event_with_why() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/context/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "ctx-run-driver-event",
                    "objective": "emit decision event"
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

        let get_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-driver-event")
            .body(Body::empty())
            .expect("get request");
        let get_resp = app.clone().oneshot(get_req).await.expect("get response");
        let get_body = to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body");
        let mut get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
        get_payload["run"]["steps"] = json!([
            {"step_id":"s1","title":"Plan","status":"runnable"}
        ]);

        let put_req = Request::builder()
            .method("PUT")
            .uri("/context/runs/ctx-run-driver-event")
            .header("content-type", "application/json")
            .body(Body::from(
                get_payload
                    .get("run")
                    .cloned()
                    .expect("run payload")
                    .to_string(),
            ))
            .expect("put request");
        let put_resp = app.clone().oneshot(put_req).await.expect("put response");
        assert_eq!(put_resp.status(), StatusCode::OK);

        let next_req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-driver-event/driver/next")
            .header("content-type", "application/json")
            .body(Body::from(json!({"dry_run": false}).to_string()))
            .expect("next request");
        let next_resp = app.clone().oneshot(next_req).await.expect("next response");
        assert_eq!(next_resp.status(), StatusCode::OK);

        let events_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-driver-event/events")
            .body(Body::empty())
            .expect("events request");
        let events_resp = app
            .clone()
            .oneshot(events_req)
            .await
            .expect("events response");
        let events_body = to_bytes(events_resp.into_body(), usize::MAX)
            .await
            .expect("events body");
        let events_payload: Value = serde_json::from_slice(&events_body).expect("events json");
        let decision_event = events_payload
            .get("events")
            .and_then(Value::as_array)
            .and_then(|rows| {
                rows.iter().find(|row| {
                    row.get("type")
                        .and_then(Value::as_str)
                        .map(|ty| ty == "meta_next_step_selected")
                        .unwrap_or(false)
                })
            })
            .cloned()
            .expect("decision event");
        assert!(decision_event
            .get("payload")
            .and_then(|p| p.get("why_next_step"))
            .and_then(Value::as_str)
            .map(|why| !why.trim().is_empty())
            .unwrap_or(false));
    }

    #[tokio::test]
    async fn context_run_fault_injection_workspace_mismatch_checkpoint_replay() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/context/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "ctx-run-fault-1",
                    "objective": "fault injection path",
                    "workspace": {
                        "workspace_id": "ws-fault",
                        "canonical_path": "/expected/path",
                        "lease_epoch": 1
                    }
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

        let get_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-fault-1")
            .body(Body::empty())
            .expect("get request");
        let get_resp = app.clone().oneshot(get_req).await.expect("get response");
        let get_body = to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body");
        let mut get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
        get_payload["run"]["steps"] = json!([
            {"step_id":"s1","title":"Plan","status":"runnable"}
        ]);

        let put_req = Request::builder()
            .method("PUT")
            .uri("/context/runs/ctx-run-fault-1")
            .header("content-type", "application/json")
            .body(Body::from(
                get_payload
                    .get("run")
                    .cloned()
                    .expect("run payload")
                    .to_string(),
            ))
            .expect("put request");
        let put_resp = app.clone().oneshot(put_req).await.expect("put response");
        assert_eq!(put_resp.status(), StatusCode::OK);

        let next_req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-fault-1/driver/next")
            .header("content-type", "application/json")
            .body(Body::from(json!({"dry_run": false}).to_string()))
            .expect("next request");
        let next_resp = app.clone().oneshot(next_req).await.expect("next response");
        assert_eq!(next_resp.status(), StatusCode::OK);

        let mismatch_req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-fault-1/lease/validate")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "phase": "pre_tool_call",
                    "current_path": "/other/path",
                    "step_id": "s1"
                })
                .to_string(),
            ))
            .expect("mismatch request");
        let mismatch_resp = app
            .clone()
            .oneshot(mismatch_req)
            .await
            .expect("mismatch response");
        assert_eq!(mismatch_resp.status(), StatusCode::OK);
        let mismatch_body = to_bytes(mismatch_resp.into_body(), usize::MAX)
            .await
            .expect("mismatch body");
        let mismatch_payload: Value =
            serde_json::from_slice(&mismatch_body).expect("mismatch json");
        assert_eq!(
            mismatch_payload.get("mismatch").and_then(Value::as_bool),
            Some(true)
        );

        let checkpoint_req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-fault-1/checkpoints")
            .header("content-type", "application/json")
            .body(Body::from(json!({"reason":"fault_injection"}).to_string()))
            .expect("checkpoint request");
        let checkpoint_resp = app
            .clone()
            .oneshot(checkpoint_req)
            .await
            .expect("checkpoint response");
        assert_eq!(checkpoint_resp.status(), StatusCode::OK);

        let events_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-fault-1/events")
            .body(Body::empty())
            .expect("events request");
        let events_resp = app
            .clone()
            .oneshot(events_req)
            .await
            .expect("events response");
        let events_body = to_bytes(events_resp.into_body(), usize::MAX)
            .await
            .expect("events body");
        let events_payload: Value = serde_json::from_slice(&events_body).expect("events json");
        let event_rows = events_payload
            .get("events")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        assert!(event_rows.iter().any(|row| {
            row.get("type")
                .and_then(Value::as_str)
                .map(|ty| ty == "meta_next_step_selected")
                .unwrap_or(false)
        }));
        assert!(event_rows.iter().any(|row| {
            row.get("type")
                .and_then(Value::as_str)
                .map(|ty| ty == "workspace_mismatch")
                .unwrap_or(false)
        }));
        assert!(event_rows.iter().any(|row| {
            row.get("status")
                .and_then(Value::as_str)
                .map(|status| status == "paused")
                .unwrap_or(false)
        }));

        let replay_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-fault-1/replay")
            .body(Body::empty())
            .expect("replay request");
        let replay_resp = app
            .clone()
            .oneshot(replay_req)
            .await
            .expect("replay response");
        assert_eq!(replay_resp.status(), StatusCode::OK);
        let replay_body = to_bytes(replay_resp.into_body(), usize::MAX)
            .await
            .expect("replay body");
        let replay_payload: Value = serde_json::from_slice(&replay_body).expect("replay json");
        assert_eq!(
            replay_payload
                .get("replay")
                .and_then(|r| r.get("status"))
                .and_then(Value::as_str),
            Some("paused")
        );
        assert_eq!(
            replay_payload
                .get("drift")
                .and_then(|d| d.get("mismatch"))
                .and_then(Value::as_bool),
            Some(false)
        );
    }

    #[tokio::test]
    async fn context_run_todos_sync_maps_to_steps_and_emits_event() {
        let state = test_state().await;
        let app = app_router(state.clone());

        let create_req = Request::builder()
            .method("POST")
            .uri("/context/runs")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": "ctx-run-todos-sync",
                    "objective": "sync todos"
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

        let sync_req = Request::builder()
            .method("POST")
            .uri("/context/runs/ctx-run-todos-sync/todos/sync")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "replace": true,
                    "source_session_id": "s-1",
                    "source_run_id": "r-1",
                    "todos": [
                        {"id":"task-1","content":"Plan architecture","status":"in_progress"},
                        {"id":"task-2","content":"Implement endpoint","status":"pending"},
                        {"id":"task-3","content":"Write tests","status":"completed"}
                    ]
                })
                .to_string(),
            ))
            .expect("sync request");
        let sync_resp = app.clone().oneshot(sync_req).await.expect("sync response");
        assert_eq!(sync_resp.status(), StatusCode::OK);

        let run_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-todos-sync")
            .body(Body::empty())
            .expect("run request");
        let run_resp = app.clone().oneshot(run_req).await.expect("run response");
        let run_body = to_bytes(run_resp.into_body(), usize::MAX)
            .await
            .expect("run body");
        let run_payload: Value = serde_json::from_slice(&run_body).expect("run json");
        assert_eq!(
            run_payload
                .get("run")
                .and_then(|r| r.get("status"))
                .and_then(Value::as_str),
            Some("running")
        );
        assert_eq!(
            run_payload
                .get("run")
                .and_then(|r| r.get("steps"))
                .and_then(Value::as_array)
                .map(|rows| rows.len()),
            Some(3)
        );

        let events_req = Request::builder()
            .method("GET")
            .uri("/context/runs/ctx-run-todos-sync/events")
            .body(Body::empty())
            .expect("events request");
        let events_resp = app
            .clone()
            .oneshot(events_req)
            .await
            .expect("events response");
        let events_body = to_bytes(events_resp.into_body(), usize::MAX)
            .await
            .expect("events body");
        let events_payload: Value = serde_json::from_slice(&events_body).expect("events json");
        let has_todo_synced = events_payload
            .get("events")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter().any(|row| {
                    row.get("type")
                        .and_then(Value::as_str)
                        .map(|v| v == "todo_synced")
                        .unwrap_or(false)
                })
            })
            .unwrap_or(false);
        assert!(has_todo_synced);
    }
}
