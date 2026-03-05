use anyhow::{anyhow, bail, Result};
use futures::StreamExt;
use reqwest::{header::HeaderMap, header::HeaderValue, Client};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tandem_types::{CreateSessionRequest, ModelSpec};
use tandem_wire::{WireProviderEntry, WireSessionMessage};

#[derive(Clone)]
pub struct EngineClient {
    base_url: String,
    client: Client,
    api_key: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EngineStatus {
    pub healthy: bool,
    pub version: String,
    pub mode: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct BrowserBlockingIssue {
    pub code: String,
    pub message: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct BrowserBinaryStatus {
    pub found: bool,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub channel: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct BrowserStatusResponse {
    pub enabled: bool,
    pub runnable: bool,
    #[serde(default)]
    pub headless_default: bool,
    #[serde(default)]
    pub sidecar: BrowserBinaryStatus,
    #[serde(default)]
    pub browser: BrowserBinaryStatus,
    #[serde(default)]
    pub blocking_issues: Vec<BrowserBlockingIssue>,
    #[serde(default)]
    pub recommendations: Vec<String>,
    #[serde(default)]
    pub install_hints: Vec<String>,
    #[serde(default)]
    pub last_checked_at_ms: Option<u64>,
    #[serde(default)]
    pub last_error: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct SessionTime {
    pub created: Option<u64>,
    pub updated: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct Session {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub directory: Option<String>,
    #[serde(rename = "workspaceRoot", default)]
    pub workspace_root: Option<String>,
    #[serde(default)]
    pub time: Option<SessionTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionScope {
    Workspace,
    Global,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ProviderCatalog {
    pub all: Vec<WireProviderEntry>,
    pub connected: Vec<String>,
    pub default: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct ConfigProvidersResponse {
    pub providers: HashMap<String, ProviderConfigEntry>,
    pub default: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct ProviderConfigEntry {
    pub api_key: Option<String>,
    pub url: Option<String>,
    pub default_model: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct EngineLease {
    pub lease_id: String,
    pub client_id: String,
    pub client_type: String,
    pub acquired_at_ms: u64,
    pub last_renewed_at_ms: u64,
    pub ttl_ms: u64,
    pub lease_count: usize,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct SendMessageRequest {
    #[serde(default)]
    pub parts: Vec<MessagePartInput>,
    pub model: Option<ModelSpec>,
    pub agent: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct PermissionRequest {
    pub id: String,
    #[serde(rename = "sessionID", default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    #[serde(rename = "argsSource", default)]
    pub args_source: Option<String>,
    #[serde(rename = "argsIntegrity", default)]
    pub args_integrity: Option<String>,
    #[serde(default)]
    pub query: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct PermissionSnapshot {
    #[serde(default)]
    pub requests: Vec<PermissionRequest>,
    #[serde(default)]
    pub rules: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct QuestionChoice {
    pub label: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct QuestionInfo {
    #[serde(default)]
    pub header: String,
    pub question: String,
    #[serde(default)]
    pub options: Vec<QuestionChoice>,
    #[serde(default)]
    pub multiple: Option<bool>,
    #[serde(default)]
    pub custom: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct QuestionToolRef {
    #[serde(rename = "callID", default)]
    pub call_id: Option<String>,
    #[serde(rename = "messageID", default)]
    pub message_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct QuestionRequest {
    pub id: String,
    #[serde(rename = "sessionID", default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub questions: Vec<QuestionInfo>,
    #[serde(default)]
    pub tool: Option<QuestionToolRef>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamRequestEvent {
    PermissionAsked(PermissionRequest),
    PermissionReplied { request_id: String, reply: String },
    QuestionAsked(QuestionRequest),
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamToolDelta {
    pub tool_call_id: String,
    pub tool_name: String,
    pub args_delta: String,
    pub args_preview: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamAgentTeamEvent {
    pub event_type: String,
    pub team_name: Option<String>,
    pub recipient: Option<String>,
    pub message_type: Option<String>,
    pub request_id: Option<String>,
    pub message_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PromptRunResult {
    pub messages: Vec<WireSessionMessage>,
    pub streamed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StreamEventEnvelope {
    pub event_type: String,
    pub session_id: Option<String>,
    pub run_id: Option<String>,
    pub agent_id: Option<String>,
    pub channel: Option<String>,
    pub payload: serde_json::Value,
}

#[derive(Debug, Deserialize)]
struct PromptConflictResponse {
    code: Option<String>,
    #[serde(rename = "activeRun")]
    active_run: Option<ActiveRunRef>,
}

#[derive(Debug, Deserialize)]
struct ActiveRunRef {
    #[serde(rename = "runID")]
    run_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessagePartInput {
    Text {
        text: String,
    },
    File {
        mime: String,
        filename: Option<String>,
        url: String,
    },
}

#[derive(Debug, Deserialize, Serialize, Clone, Default)]
pub struct UpdateSessionRequest {
    pub title: Option<String>,
    pub model: Option<ModelSpec>,
    pub provider: Option<String>,
    pub mode: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutineSchedule {
    IntervalSeconds { seconds: u64 },
    Cron { expression: String },
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RoutineMisfirePolicy {
    Skip,
    RunOnce,
    CatchUp { max_runs: u32 },
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutineStatus {
    Active,
    Paused,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct RoutineSpec {
    pub routine_id: String,
    pub name: String,
    pub status: RoutineStatus,
    pub schedule: RoutineSchedule,
    pub timezone: String,
    pub misfire_policy: RoutineMisfirePolicy,
    pub entrypoint: String,
    #[serde(default)]
    pub args: serde_json::Value,
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub output_targets: Vec<String>,
    pub creator_type: String,
    pub creator_id: String,
    pub requires_approval: bool,
    pub external_integrations_allowed: bool,
    #[serde(default)]
    pub next_fire_at_ms: Option<u64>,
    #[serde(default)]
    pub last_fired_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct RoutineHistoryEvent {
    pub routine_id: String,
    pub trigger_type: String,
    pub run_count: u32,
    pub fired_at_ms: u64,
    pub status: String,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct RoutineCreateRequest {
    #[serde(default)]
    pub routine_id: Option<String>,
    pub name: String,
    pub schedule: RoutineSchedule,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub misfire_policy: Option<RoutineMisfirePolicy>,
    pub entrypoint: String,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub output_targets: Option<Vec<String>>,
    #[serde(default)]
    pub creator_type: Option<String>,
    #[serde(default)]
    pub creator_id: Option<String>,
    #[serde(default)]
    pub requires_approval: Option<bool>,
    #[serde(default)]
    pub external_integrations_allowed: Option<bool>,
    #[serde(default)]
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct RoutinePatchRequest {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub status: Option<RoutineStatus>,
    #[serde(default)]
    pub schedule: Option<RoutineSchedule>,
    #[serde(default)]
    pub timezone: Option<String>,
    #[serde(default)]
    pub misfire_policy: Option<RoutineMisfirePolicy>,
    #[serde(default)]
    pub entrypoint: Option<String>,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,
    #[serde(default)]
    pub output_targets: Option<Vec<String>>,
    #[serde(default)]
    pub requires_approval: Option<bool>,
    #[serde(default)]
    pub external_integrations_allowed: Option<bool>,
    #[serde(default)]
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct RoutineRunNowRequest {
    #[serde(default)]
    pub run_count: Option<u32>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct RoutineRunNowResponse {
    pub ok: bool,
    pub status: String,
    #[serde(rename = "routineID")]
    pub routine_id: String,
    #[serde(rename = "runCount")]
    pub run_count: u32,
    #[serde(rename = "firedAtMs", default)]
    pub fired_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct RoutineListResponse {
    routines: Vec<RoutineSpec>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct RoutineRecordResponse {
    routine: RoutineSpec,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct RoutineDeleteResponse {
    deleted: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct RoutineHistoryResponse {
    events: Vec<RoutineHistoryEvent>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct PackInstallRecord {
    pub pack_id: String,
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub pack_type: Option<String>,
    #[serde(default)]
    pub install_path: Option<String>,
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub installed_at_ms: Option<u64>,
    #[serde(default)]
    pub routines_enabled: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct PacksListResponse {
    #[serde(default)]
    packs: Vec<PackInstallRecord>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct PackRecordEnvelope {
    pack: PackRecordPayload,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct PackRecordPayload {
    installed: PackInstallRecord,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct PackInstallResponse {
    installed: PackInstallRecord,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct PackUninstallResponse {
    removed: PackInstallRecord,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct PackExportInfo {
    pub path: String,
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct PackExportResponse {
    exported: PackExportInfo,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct PackDetectionResponse {
    pub is_pack: bool,
    pub marker: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct PackUpdatesResponse {
    #[serde(default)]
    pub pack_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub current_version: Option<String>,
    #[serde(default)]
    pub updates: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct PackUpdateResult {
    pub updated: bool,
    #[serde(default)]
    pub pack_id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub current_version: Option<String>,
    #[serde(default)]
    pub target_version: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct PresetRecord {
    pub id: String,
    pub version: String,
    pub kind: String,
    pub layer: String,
    #[serde(default)]
    pub pack: Option<String>,
    pub path: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct PresetIndex {
    #[serde(default)]
    pub skill_modules: Vec<PresetRecord>,
    #[serde(default)]
    pub agent_presets: Vec<PresetRecord>,
    #[serde(default)]
    pub automation_presets: Vec<PresetRecord>,
    #[serde(default)]
    pub pack_presets: Vec<PresetRecord>,
    #[serde(default)]
    pub generated_at_ms: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct PresetsIndexResponse {
    index: PresetIndex,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct CapabilityBinding {
    pub capability_id: String,
    pub provider: String,
    pub tool_name: String,
    #[serde(default)]
    pub request_transform: Option<serde_json::Value>,
    #[serde(default)]
    pub response_transform: Option<serde_json::Value>,
    #[serde(default)]
    pub metadata: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct CapabilityBindingsFile {
    pub schema_version: String,
    #[serde(default)]
    pub generated_at: Option<String>,
    #[serde(default)]
    pub bindings: Vec<CapabilityBinding>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct CapabilityBindingsEnvelope {
    bindings: CapabilityBindingsFile,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct CapabilityDiscoveredTool {
    pub provider: String,
    pub tool_name: String,
    #[serde(default)]
    pub schema: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct CapabilityDiscoveryResponse {
    #[serde(default)]
    pub tools: Vec<CapabilityDiscoveredTool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct CapabilityResolutionResponse {
    pub resolution: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct CapabilityResolveRequest {
    #[serde(default)]
    pub workflow_id: Option<String>,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub optional_capabilities: Vec<String>,
    #[serde(default)]
    pub provider_preference: Vec<String>,
    #[serde(default)]
    pub available_tools: Vec<CapabilityDiscoveredTool>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextRunStatus {
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

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ContextStepStatus {
    Pending,
    Runnable,
    InProgress,
    Blocked,
    Done,
    Failed,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
pub struct ContextWorkspaceLease {
    pub workspace_id: String,
    pub canonical_path: String,
    pub lease_epoch: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct ContextRunStep {
    pub step_id: String,
    pub title: String,
    pub status: ContextStepStatus,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct ContextRunState {
    pub run_id: String,
    pub run_type: String,
    pub status: ContextRunStatus,
    pub objective: String,
    pub workspace: ContextWorkspaceLease,
    #[serde(default)]
    pub steps: Vec<ContextRunStep>,
    #[serde(default)]
    pub why_next_step: Option<String>,
    pub revision: u64,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ContextRunEventRecord {
    pub event_id: String,
    pub run_id: String,
    pub seq: u64,
    pub ts_ms: u64,
    #[serde(rename = "type")]
    pub event_type: String,
    pub status: ContextRunStatus,
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(default)]
    pub payload: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
pub struct ContextBlackboardItem {
    pub id: String,
    pub ts_ms: u64,
    pub text: String,
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(default)]
    pub source_event_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
pub struct ContextBlackboardArtifact {
    pub id: String,
    pub ts_ms: u64,
    pub path: String,
    pub artifact_type: String,
    #[serde(default)]
    pub step_id: Option<String>,
    #[serde(default)]
    pub source_event_id: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
pub struct ContextBlackboardSummaries {
    pub rolling: String,
    pub latest_context_pack: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
pub struct ContextBlackboardState {
    #[serde(default)]
    pub facts: Vec<ContextBlackboardItem>,
    #[serde(default)]
    pub decisions: Vec<ContextBlackboardItem>,
    #[serde(default)]
    pub open_questions: Vec<ContextBlackboardItem>,
    #[serde(default)]
    pub artifacts: Vec<ContextBlackboardArtifact>,
    #[serde(default)]
    pub summaries: ContextBlackboardSummaries,
    pub revision: u64,
}

#[derive(Debug, Deserialize)]
struct ContextRunRecordResponse {
    run: ContextRunState,
}

#[derive(Debug, Deserialize)]
struct ContextRunListResponse {
    runs: Vec<ContextRunState>,
}

#[derive(Debug, Deserialize)]
struct ContextRunEventsResponse {
    events: Vec<ContextRunEventRecord>,
}

#[derive(Debug, Deserialize)]
struct ContextRunEventRecordResponse {
    event: ContextRunEventRecord,
}

#[derive(Debug, Deserialize)]
struct ContextBlackboardResponse {
    blackboard: ContextBlackboardState,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
pub struct ContextReplayDrift {
    pub mismatch: bool,
    pub status_mismatch: bool,
    pub why_next_step_mismatch: bool,
    pub step_count_mismatch: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ContextRunReplayResponse {
    pub ok: bool,
    pub run_id: String,
    #[serde(default)]
    pub from_checkpoint: bool,
    #[serde(default)]
    pub checkpoint_seq: Option<u64>,
    #[serde(default)]
    pub events_applied: usize,
    pub replay: ContextRunState,
    pub persisted: ContextRunState,
    pub drift: ContextReplayDrift,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct ContextDriverNextResponse {
    pub ok: bool,
    #[serde(default)]
    pub dry_run: bool,
    pub run_id: String,
    #[serde(default)]
    pub selected_step_id: Option<String>,
    pub target_status: ContextRunStatus,
    pub why_next_step: String,
    pub run: ContextRunState,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
pub struct ContextTodoSyncItem {
    #[serde(default)]
    pub id: Option<String>,
    pub content: String,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Draft,
    Running,
    Paused,
    Succeeded,
    Failed,
    Canceled,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionWorkItemStatus {
    Todo,
    InProgress,
    Blocked,
    Review,
    Test,
    Rework,
    Done,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct MissionBudget {
    #[serde(default)]
    pub max_steps: Option<u32>,
    #[serde(default)]
    pub max_tool_calls: Option<u32>,
    #[serde(default)]
    pub max_duration_ms: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct MissionCapabilities {
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub allowed_agents: Vec<String>,
    #[serde(default)]
    pub allowed_memory_tiers: Vec<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MissionSpec {
    pub mission_id: String,
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub success_criteria: Vec<String>,
    #[serde(default)]
    pub entrypoint: Option<String>,
    #[serde(default)]
    pub budgets: MissionBudget,
    #[serde(default)]
    pub capabilities: MissionCapabilities,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MissionWorkItem {
    pub work_item_id: String,
    pub title: String,
    #[serde(default)]
    pub detail: Option<String>,
    pub status: MissionWorkItemStatus,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub assigned_agent: Option<String>,
    #[serde(default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub artifact_refs: Vec<String>,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MissionState {
    pub mission_id: String,
    pub status: MissionStatus,
    pub spec: MissionSpec,
    #[serde(default)]
    pub work_items: Vec<MissionWorkItem>,
    pub revision: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MissionCreateWorkItem {
    #[serde(default)]
    pub work_item_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub detail: Option<String>,
    #[serde(default)]
    pub assigned_agent: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MissionCreateRequest {
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub work_items: Vec<MissionCreateWorkItem>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct MissionApplyEventResult {
    pub mission: MissionState,
    #[serde(default)]
    pub commands: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct MissionListResponse {
    missions: Vec<MissionState>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct MissionRecordResponse {
    mission: MissionState,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct AgentTeamMissionSummary {
    #[serde(rename = "missionID")]
    pub mission_id: String,
    #[serde(rename = "instanceCount")]
    pub instance_count: u64,
    #[serde(rename = "runningCount")]
    pub running_count: u64,
    #[serde(rename = "completedCount")]
    pub completed_count: u64,
    #[serde(rename = "failedCount")]
    pub failed_count: u64,
    #[serde(rename = "cancelledCount")]
    pub cancelled_count: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct AgentTeamInstance {
    #[serde(rename = "instanceID")]
    pub instance_id: String,
    #[serde(rename = "missionID")]
    pub mission_id: String,
    #[serde(rename = "parentInstanceID", default)]
    pub parent_instance_id: Option<String>,
    pub role: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub status: String,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct AgentTeamSpawnApproval {
    #[serde(rename = "approvalID")]
    pub approval_id: String,
    #[serde(rename = "createdAtMs")]
    pub created_at_ms: u64,
    #[serde(default)]
    pub request: serde_json::Value,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct AgentTeamToolApproval {
    #[serde(rename = "approvalID")]
    pub approval_id: String,
    #[serde(rename = "sessionID", default)]
    pub session_id: Option<String>,
    #[serde(rename = "toolCallID", default)]
    pub tool_call_id: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct AgentTeamMissionsResponse {
    #[serde(default)]
    missions: Vec<AgentTeamMissionSummary>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
struct AgentTeamInstancesResponse {
    #[serde(default)]
    instances: Vec<AgentTeamInstance>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct AgentTeamApprovalsResponse {
    #[serde(rename = "spawnApprovals", default)]
    pub spawn_approvals: Vec<AgentTeamSpawnApproval>,
    #[serde(rename = "toolApprovals", default)]
    pub tool_approvals: Vec<AgentTeamToolApproval>,
}

impl EngineClient {
    pub fn new(base_url: String) -> Self {
        Self::new_with_token(base_url, None)
    }

    pub fn new_with_token(base_url: String, api_token: Option<String>) -> Self {
        let mut headers = HeaderMap::new();
        if let Some(token) = api_token
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
        {
            if let Ok(value) = HeaderValue::from_str(token) {
                headers.insert("x-tandem-token", value);
            }
        }
        let client = Client::builder()
            .default_headers(headers)
            .build()
            .unwrap_or_else(|_| Client::new());
        Self {
            base_url,
            client,
            api_key: None,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn check_health(&self) -> Result<bool> {
        let url = format!("{}/global/health", self.base_url);
        let resp = self.client.get(&url).send().await?;
        Ok(resp.status().is_success())
    }

    pub async fn get_engine_status(&self) -> Result<EngineStatus> {
        let url = format!("{}/global/health", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let status = resp.json::<EngineStatus>().await?;
        Ok(status)
    }

    pub async fn get_browser_status(&self) -> Result<BrowserStatusResponse> {
        let url = format!("{}/browser/status", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let status = resp.json::<BrowserStatusResponse>().await?;
        Ok(status)
    }

    pub async fn acquire_lease(
        &self,
        client_id: &str,
        client_type: &str,
        ttl_ms: Option<u64>,
    ) -> Result<EngineLease> {
        let url = format!("{}/global/lease/acquire", self.base_url);
        let payload = serde_json::json!({
            "client_id": client_id,
            "client_type": client_type,
            "ttl_ms": ttl_ms.unwrap_or(60_000),
        });
        let resp = self.client.post(&url).json(&payload).send().await?;
        let lease = resp.json::<EngineLease>().await?;
        Ok(lease)
    }

    pub async fn renew_lease(&self, lease_id: &str) -> Result<bool> {
        let url = format!("{}/global/lease/renew", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "lease_id": lease_id }))
            .send()
            .await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn release_lease(&self, lease_id: &str) -> Result<bool> {
        let url = format!("{}/global/lease/release", self.base_url);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "lease_id": lease_id }))
            .send()
            .await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        let workspace = std::env::current_dir()
            .ok()
            .and_then(|p| normalize_workspace_path(&p));
        self.list_sessions_scoped(SessionScope::Workspace, workspace)
            .await
    }

    pub async fn list_sessions_scoped(
        &self,
        scope: SessionScope,
        workspace: Option<String>,
    ) -> Result<Vec<Session>> {
        let url = format!("{}/api/session", self.base_url);
        let scope_value = match scope {
            SessionScope::Workspace => "workspace",
            SessionScope::Global => "global",
        };
        let mut req = self.client.get(&url).query(&[("scope", scope_value)]);
        if matches!(scope, SessionScope::Workspace) {
            if let Some(workspace) = workspace {
                req = req.query(&[("workspace", workspace)]);
            }
        }
        let resp = req.send().await?;
        let sessions = resp.json::<Vec<Session>>().await?;
        Ok(sessions)
    }

    pub async fn create_session(&self, title: Option<String>) -> Result<Session> {
        let url = format!("{}/api/session", self.base_url);
        let req = CreateSessionRequest {
            parent_id: None,
            title,
            directory: std::env::current_dir()
                .ok()
                .and_then(|p| normalize_workspace_path(&p)),
            workspace_root: std::env::current_dir()
                .ok()
                .and_then(|p| normalize_workspace_path(&p)),
            model: None,
            provider: None,
            permission: Some(default_tui_permission_rules()),
        };

        let resp = self.client.post(&url).json(&req).send().await?;
        let session = resp.json::<Session>().await?;
        Ok(session)
    }

    pub async fn get_session(&self, session_id: &str) -> Result<Session> {
        let url = format!("{}/session/{}", self.base_url, session_id);
        let resp = self.client.get(&url).send().await?;
        let session = resp.json::<Session>().await?;
        Ok(session)
    }

    pub async fn get_session_messages(&self, session_id: &str) -> Result<Vec<WireSessionMessage>> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        let resp = self.client.get(&url).send().await?;
        let messages = resp.json::<Vec<WireSessionMessage>>().await?;
        Ok(messages)
    }

    pub async fn update_session(
        &self,
        session_id: &str,
        req: UpdateSessionRequest,
    ) -> Result<Session> {
        let url = format!("{}/session/{}", self.base_url, session_id);
        let resp = self.client.patch(&url).json(&req).send().await?;
        let session = resp.json::<Session>().await?;
        Ok(session)
    }

    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        let url = format!("{}/session/{}", self.base_url, session_id);
        self.client.delete(&url).send().await?;
        Ok(())
    }

    pub async fn list_providers(&self) -> Result<ProviderCatalog> {
        let url = format!("{}/provider", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let catalog = resp.json::<ProviderCatalog>().await?;
        Ok(catalog)
    }

    pub async fn config_providers(&self) -> Result<ConfigProvidersResponse> {
        let url = format!("{}/config/providers", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let config = resp.json::<ConfigProvidersResponse>().await?;
        Ok(config)
    }

    pub async fn set_auth(&self, provider_id: &str, api_key: &str) -> Result<()> {
        let url = format!("{}/auth/{}", self.base_url, provider_id);
        self.client
            .put(&url)
            .json(&serde_json::json!({ "apiKey": api_key }))
            .send()
            .await?;
        Ok(())
    }

    pub async fn delete_auth(&self, provider_id: &str) -> Result<()> {
        let url = format!("{}/auth/{}", self.base_url, provider_id);
        self.client.delete(&url).send().await?;
        Ok(())
    }

    pub async fn list_permissions(&self) -> Result<PermissionSnapshot> {
        let url = format!("{}/permission", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let snapshot = resp.json::<PermissionSnapshot>().await?;
        Ok(snapshot)
    }

    pub async fn reply_permission(&self, id: &str, reply: &str) -> Result<bool> {
        let url = format!("{}/permission/{}/reply", self.base_url, id);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "reply": reply }))
            .send()
            .await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn list_questions(&self) -> Result<Vec<QuestionRequest>> {
        let url = format!("{}/question", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let snapshot = resp.json::<Vec<QuestionRequest>>().await?;
        Ok(snapshot)
    }

    pub async fn reply_question(&self, id: &str, answers: Vec<Vec<String>>) -> Result<bool> {
        let url = format!("{}/question/{}/reply", self.base_url, id);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "answers": answers }))
            .send()
            .await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn reject_question(&self, id: &str) -> Result<bool> {
        let url = format!("{}/question/{}/reject", self.base_url, id);
        let resp = self.client.post(&url).send().await?;
        let body = resp.json::<serde_json::Value>().await?;
        Ok(body.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn send_prompt(
        &self,
        session_id: &str,
        message: &str,
        agent: Option<&str>,
        model: Option<ModelSpec>,
    ) -> Result<Vec<WireSessionMessage>> {
        let result = self
            .send_prompt_with_stream(session_id, message, agent, model, |_| {})
            .await?;
        Ok(result.messages)
    }

    pub async fn send_prompt_with_stream<F>(
        &self,
        session_id: &str,
        message: &str,
        agent: Option<&str>,
        model: Option<ModelSpec>,
        mut on_delta: F,
    ) -> Result<PromptRunResult>
    where
        F: FnMut(String),
    {
        self.send_prompt_with_stream_events(session_id, message, agent, None, model, |event| {
            if let Some(delta) = extract_delta_text(&event.payload) {
                if !delta.is_empty() {
                    on_delta(delta);
                }
            }
        })
        .await
    }

    pub async fn send_prompt_with_stream_events<F>(
        &self,
        session_id: &str,
        message: &str,
        agent: Option<&str>,
        agent_id: Option<&str>,
        model: Option<ModelSpec>,
        mut on_event: F,
    ) -> Result<PromptRunResult>
    where
        F: FnMut(StreamEventEnvelope),
    {
        let append_url = format!(
            "{}/session/{}/message?mode=append",
            self.base_url, session_id
        );
        let prompt_url = format!("{}/session/{}/prompt_sync", self.base_url, session_id);
        let req = SendMessageRequest {
            parts: vec![MessagePartInput::Text {
                text: message.to_string(),
            }],
            model,
            agent: agent.map(String::from),
        };
        let append_resp = self.client.post(&append_url).json(&req).send().await?;
        if !append_resp.status().is_success() {
            let status = append_resp.status();
            let body = append_resp.text().await?;
            bail!("append failed {}: {}", status, body);
        }
        let mut prompt_req = self
            .client
            .post(&prompt_url)
            .header("Accept", "text/event-stream");
        if let Some(agent_id) = agent_id {
            prompt_req = prompt_req.header("x-tandem-agent-id", agent_id);
        }
        let resp = prompt_req.json(&req).send().await?;
        if resp.status() == reqwest::StatusCode::CONFLICT {
            let body = resp.text().await?;
            let run_id = serde_json::from_str::<PromptConflictResponse>(&body)
                .ok()
                .and_then(|payload| {
                    if payload.code.as_deref() == Some("SESSION_RUN_CONFLICT") {
                        payload.active_run.and_then(|run| run.run_id)
                    } else {
                        None
                    }
                });
            if let Some(run_id) = run_id {
                bail!(
                    "409 Conflict: session has active run `{}`. Queue follow-up or cancel first.",
                    run_id
                );
            }
            bail!("409 Conflict: {}", body);
        }
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await?;
            bail!("{}: {}", status, body);
        }
        let content_type = resp
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        if content_type.starts_with("text/event-stream") {
            let mut stream = resp.bytes_stream();
            let mut streamed = false;
            let mut buffer = String::new();
            while let Some(chunk) =
                tokio::time::timeout(Duration::from_secs(90), stream.next()).await?
            {
                let chunk = chunk?;
                let text = String::from_utf8_lossy(&chunk);
                buffer.push_str(&text);
                while let Some(payload) = parse_sse_payload(&mut buffer) {
                    if let Some(event) = parse_stream_event_envelope(payload) {
                        if extract_delta_text(&event.payload)
                            .map(|d| !d.trim().is_empty())
                            .unwrap_or(false)
                        {
                            streamed = true;
                        }
                        on_event(event);
                    }
                }
            }
            let final_url = format!("{}/session/{}/message", self.base_url, session_id);
            let final_resp = self.client.get(&final_url).send().await?;
            let final_status = final_resp.status();
            let final_body = final_resp.text().await?;
            if !final_status.is_success() {
                bail!("{}: {}", final_status, final_body);
            }
            let messages: Vec<WireSessionMessage> = serde_json::from_str(&final_body)
                .map_err(|err| anyhow!("Invalid response body: {} | body: {}", err, final_body))?;
            return Ok(PromptRunResult { messages, streamed });
        }
        let body = resp.text().await?;
        let messages: Vec<WireSessionMessage> = serde_json::from_str(&body)
            .map_err(|err| anyhow!("Invalid response body: {} | body: {}", err, body))?;
        Ok(PromptRunResult {
            messages,
            streamed: false,
        })
    }

    pub async fn abort_session(&self, session_id: &str) -> Result<()> {
        let url = format!("{}/session/{}/cancel", self.base_url, session_id);
        self.client.post(&url).send().await?;
        Ok(())
    }

    pub async fn cancel_run_by_id(&self, session_id: &str, run_id: &str) -> Result<bool> {
        let url = format!(
            "{}/session/{}/run/{}/cancel",
            self.base_url, session_id, run_id
        );
        let resp = self.client.post(&url).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload
            .get("cancelled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false))
    }

    pub async fn get_config(&self) -> Result<serde_json::Value> {
        let url = format!("{}/config", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let config = resp.json::<serde_json::Value>().await?;
        Ok(config)
    }

    pub async fn patch_config(&self, patch: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/config", self.base_url);
        let resp = self.client.patch(&url).json(&patch).send().await?;
        let config = resp.json::<serde_json::Value>().await?;
        Ok(config)
    }

    pub async fn attach_session_to_workspace(
        &self,
        session_id: &str,
        target_workspace: &str,
        reason_tag: &str,
    ) -> Result<Session> {
        let url = format!("{}/api/session/{}/attach", self.base_url, session_id);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "target_workspace": target_workspace,
                "reason_tag": reason_tag
            }))
            .send()
            .await?;
        let session = resp.json::<Session>().await?;
        Ok(session)
    }

    pub async fn routines_list(&self) -> Result<Vec<RoutineSpec>> {
        let url = format!("{}/routines", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<RoutineListResponse>().await?;
        Ok(payload.routines)
    }

    pub async fn routines_create(&self, request: RoutineCreateRequest) -> Result<RoutineSpec> {
        let url = format!("{}/routines", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<RoutineRecordResponse>().await?;
        Ok(payload.routine)
    }

    pub async fn routines_patch(
        &self,
        routine_id: &str,
        request: RoutinePatchRequest,
    ) -> Result<RoutineSpec> {
        let url = format!("{}/routines/{}", self.base_url, routine_id);
        let resp = self.client.patch(&url).json(&request).send().await?;
        let payload = resp.json::<RoutineRecordResponse>().await?;
        Ok(payload.routine)
    }

    pub async fn routines_delete(&self, routine_id: &str) -> Result<bool> {
        let url = format!("{}/routines/{}", self.base_url, routine_id);
        let resp = self.client.delete(&url).send().await?;
        let payload = resp.json::<RoutineDeleteResponse>().await?;
        Ok(payload.deleted)
    }

    pub async fn routines_run_now(
        &self,
        routine_id: &str,
        request: RoutineRunNowRequest,
    ) -> Result<RoutineRunNowResponse> {
        let url = format!("{}/routines/{}/run_now", self.base_url, routine_id);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<RoutineRunNowResponse>().await?;
        Ok(payload)
    }

    pub async fn routines_history(
        &self,
        routine_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<RoutineHistoryEvent>> {
        let url = format!("{}/routines/{}/history", self.base_url, routine_id);
        let mut req = self.client.get(&url);
        if let Some(limit) = limit {
            req = req.query(&[("limit", limit)]);
        }
        let resp = req.send().await?;
        let payload = resp.json::<RoutineHistoryResponse>().await?;
        Ok(payload.events)
    }

    pub async fn packs_list(&self) -> Result<Vec<PackInstallRecord>> {
        let url = format!("{}/packs", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<PacksListResponse>().await?;
        Ok(payload.packs)
    }

    pub async fn packs_get(&self, selector: &str) -> Result<PackInstallRecord> {
        let url = format!("{}/packs/{}", self.base_url, selector);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<PackRecordEnvelope>().await?;
        Ok(payload.pack.installed)
    }

    pub async fn packs_install(&self, request: serde_json::Value) -> Result<PackInstallRecord> {
        let url = format!("{}/packs/install", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<PackInstallResponse>().await?;
        Ok(payload.installed)
    }

    pub async fn packs_uninstall(&self, request: serde_json::Value) -> Result<PackInstallRecord> {
        let url = format!("{}/packs/uninstall", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<PackUninstallResponse>().await?;
        Ok(payload.removed)
    }

    pub async fn packs_export(&self, request: serde_json::Value) -> Result<PackExportInfo> {
        let url = format!("{}/packs/export", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<PackExportResponse>().await?;
        Ok(payload.exported)
    }

    pub async fn packs_detect(&self, request: serde_json::Value) -> Result<PackDetectionResponse> {
        let url = format!("{}/packs/detect", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<PackDetectionResponse>().await?;
        Ok(payload)
    }

    pub async fn packs_updates(&self, selector: &str) -> Result<PackUpdatesResponse> {
        let url = format!("{}/packs/{}/updates", self.base_url, selector);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<PackUpdatesResponse>().await?;
        Ok(payload)
    }

    pub async fn packs_update(
        &self,
        selector: &str,
        request: serde_json::Value,
    ) -> Result<PackUpdateResult> {
        let url = format!("{}/packs/{}/update", self.base_url, selector);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<PackUpdateResult>().await?;
        Ok(payload)
    }

    pub async fn presets_index(&self) -> Result<PresetIndex> {
        let url = format!("{}/presets/index", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<PresetsIndexResponse>().await?;
        Ok(payload.index)
    }

    pub async fn presets_compose_preview(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/presets/compose/preview", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload)
    }

    pub async fn presets_capability_summary(
        &self,
        request: serde_json::Value,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/presets/capability_summary", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload)
    }

    pub async fn presets_fork(&self, request: serde_json::Value) -> Result<serde_json::Value> {
        let url = format!("{}/presets/fork", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload)
    }

    pub async fn presets_override_put(
        &self,
        kind: &str,
        id: &str,
        content: &str,
    ) -> Result<serde_json::Value> {
        let url = format!("{}/presets/overrides/{}/{}", self.base_url, kind, id);
        let body = serde_json::json!({ "content": content });
        let resp = self.client.put(&url).json(&body).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload)
    }

    pub async fn capabilities_bindings_get(&self) -> Result<CapabilityBindingsFile> {
        let url = format!("{}/capabilities/bindings", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<CapabilityBindingsEnvelope>().await?;
        Ok(payload.bindings)
    }

    pub async fn capabilities_bindings_put(&self, request: CapabilityBindingsFile) -> Result<bool> {
        let url = format!("{}/capabilities/bindings", self.base_url);
        let resp = self.client.put(&url).json(&request).send().await?;
        let payload = resp.json::<serde_json::Value>().await?;
        Ok(payload.get("ok").and_then(|v| v.as_bool()).unwrap_or(false))
    }

    pub async fn capabilities_discovery(&self) -> Result<CapabilityDiscoveryResponse> {
        let url = format!("{}/capabilities/discovery", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<CapabilityDiscoveryResponse>().await?;
        Ok(payload)
    }

    pub async fn capabilities_resolve(
        &self,
        request: CapabilityResolveRequest,
    ) -> Result<CapabilityResolutionResponse> {
        let url = format!("{}/capabilities/resolve", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<CapabilityResolutionResponse>().await?;
        Ok(payload)
    }

    pub async fn context_runs_list(&self) -> Result<Vec<ContextRunState>> {
        let url = format!("{}/context/runs", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<ContextRunListResponse>().await?;
        Ok(payload.runs)
    }

    pub async fn context_run_create(
        &self,
        run_id: Option<String>,
        objective: String,
        run_type: Option<String>,
        workspace: Option<ContextWorkspaceLease>,
    ) -> Result<ContextRunState> {
        let url = format!("{}/context/runs", self.base_url);
        let body = serde_json::json!({
            "run_id": run_id,
            "objective": objective,
            "run_type": run_type,
            "workspace": workspace,
        });
        let resp = self.client.post(&url).json(&body).send().await?;
        let payload = resp.json::<ContextRunRecordResponse>().await?;
        Ok(payload.run)
    }

    pub async fn context_run_get(&self, run_id: &str) -> Result<ContextRunState> {
        let url = format!("{}/context/runs/{}", self.base_url, run_id);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<ContextRunRecordResponse>().await?;
        Ok(payload.run)
    }

    pub async fn context_run_put(&self, run: &ContextRunState) -> Result<ContextRunState> {
        let url = format!("{}/context/runs/{}", self.base_url, run.run_id);
        let resp = self.client.put(&url).json(run).send().await?;
        let payload = resp.json::<ContextRunRecordResponse>().await?;
        Ok(payload.run)
    }

    pub async fn context_run_events(
        &self,
        run_id: &str,
        since_seq: Option<u64>,
        tail: Option<usize>,
    ) -> Result<Vec<ContextRunEventRecord>> {
        let url = format!("{}/context/runs/{}/events", self.base_url, run_id);
        let mut req = self.client.get(&url);
        if let Some(since_seq) = since_seq {
            req = req.query(&[("since_seq", since_seq)]);
        }
        if let Some(tail) = tail {
            req = req.query(&[("tail", tail)]);
        }
        let resp = req.send().await?;
        let payload = resp.json::<ContextRunEventsResponse>().await?;
        Ok(payload.events)
    }

    pub async fn context_run_append_event(
        &self,
        run_id: &str,
        event_type: &str,
        status: ContextRunStatus,
        step_id: Option<String>,
        payload: serde_json::Value,
    ) -> Result<ContextRunEventRecord> {
        let url = format!("{}/context/runs/{}/events", self.base_url, run_id);
        let body = serde_json::json!({
            "type": event_type,
            "status": status,
            "step_id": step_id,
            "payload": payload,
        });
        let resp = self.client.post(&url).json(&body).send().await?;
        let parsed = resp.json::<ContextRunEventRecordResponse>().await?;
        Ok(parsed.event)
    }

    pub async fn context_run_blackboard(&self, run_id: &str) -> Result<ContextBlackboardState> {
        let url = format!("{}/context/runs/{}/blackboard", self.base_url, run_id);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<ContextBlackboardResponse>().await?;
        Ok(payload.blackboard)
    }

    pub async fn context_run_replay(
        &self,
        run_id: &str,
        upto_seq: Option<u64>,
        from_checkpoint: Option<bool>,
    ) -> Result<ContextRunReplayResponse> {
        let url = format!("{}/context/runs/{}/replay", self.base_url, run_id);
        let mut req = self.client.get(&url);
        if let Some(upto_seq) = upto_seq {
            req = req.query(&[("upto_seq", upto_seq)]);
        }
        if let Some(from_checkpoint) = from_checkpoint {
            req = req.query(&[("from_checkpoint", from_checkpoint)]);
        }
        let resp = req.send().await?;
        let payload = resp.json::<ContextRunReplayResponse>().await?;
        Ok(payload)
    }

    pub async fn context_run_driver_next(
        &self,
        run_id: &str,
        dry_run: bool,
    ) -> Result<ContextDriverNextResponse> {
        let url = format!("{}/context/runs/{}/driver/next", self.base_url, run_id);
        let body = serde_json::json!({ "dry_run": dry_run });
        let resp = self.client.post(&url).json(&body).send().await?;
        let payload = resp.json::<ContextDriverNextResponse>().await?;
        Ok(payload)
    }

    pub async fn context_run_sync_todos(
        &self,
        run_id: &str,
        todos: Vec<ContextTodoSyncItem>,
        replace: bool,
        source_session_id: Option<String>,
        source_run_id: Option<String>,
    ) -> Result<ContextRunState> {
        let url = format!("{}/context/runs/{}/todos/sync", self.base_url, run_id);
        let body = serde_json::json!({
            "replace": replace,
            "source_session_id": source_session_id,
            "source_run_id": source_run_id,
            "todos": todos,
        });
        let resp = self.client.post(&url).json(&body).send().await?;
        let payload = resp.json::<ContextRunRecordResponse>().await?;
        Ok(payload.run)
    }

    pub async fn mission_list(&self) -> Result<Vec<MissionState>> {
        let url = format!("{}/mission", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<MissionListResponse>().await?;
        Ok(payload.missions)
    }

    pub async fn mission_create(&self, request: MissionCreateRequest) -> Result<MissionState> {
        let url = format!("{}/mission", self.base_url);
        let resp = self.client.post(&url).json(&request).send().await?;
        let payload = resp.json::<MissionRecordResponse>().await?;
        Ok(payload.mission)
    }

    pub async fn mission_get(&self, mission_id: &str) -> Result<MissionState> {
        let url = format!("{}/mission/{}", self.base_url, mission_id);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<MissionRecordResponse>().await?;
        Ok(payload.mission)
    }

    pub async fn mission_apply_event(
        &self,
        mission_id: &str,
        event: serde_json::Value,
    ) -> Result<MissionApplyEventResult> {
        let url = format!("{}/mission/{}/event", self.base_url, mission_id);
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "event": event }))
            .send()
            .await?;
        let payload = resp.json::<MissionApplyEventResult>().await?;
        Ok(payload)
    }

    pub async fn agent_team_missions(&self) -> Result<Vec<AgentTeamMissionSummary>> {
        let url = format!("{}/agent-team/missions", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<AgentTeamMissionsResponse>().await?;
        Ok(payload.missions)
    }

    pub async fn agent_team_instances(
        &self,
        mission_id: Option<&str>,
    ) -> Result<Vec<AgentTeamInstance>> {
        let url = format!("{}/agent-team/instances", self.base_url);
        let req = if let Some(mission_id) = mission_id {
            self.client.get(&url).query(&[("missionID", mission_id)])
        } else {
            self.client.get(&url)
        };
        let resp = req.send().await?;
        let payload = resp.json::<AgentTeamInstancesResponse>().await?;
        Ok(payload.instances)
    }

    pub async fn agent_team_approvals(&self) -> Result<AgentTeamApprovalsResponse> {
        let url = format!("{}/agent-team/approvals", self.base_url);
        let resp = self.client.get(&url).send().await?;
        let payload = resp.json::<AgentTeamApprovalsResponse>().await?;
        Ok(payload)
    }

    pub async fn agent_team_approve_spawn(&self, approval_id: &str, reason: &str) -> Result<bool> {
        let url = format!(
            "{}/agent-team/approvals/spawn/{}/approve",
            self.base_url, approval_id
        );
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "reason": reason }))
            .send()
            .await?;
        Ok(resp.status().is_success())
    }

    pub async fn agent_team_deny_spawn(&self, approval_id: &str, reason: &str) -> Result<bool> {
        let url = format!(
            "{}/agent-team/approvals/spawn/{}/deny",
            self.base_url, approval_id
        );
        let resp = self
            .client
            .post(&url)
            .json(&serde_json::json!({ "reason": reason }))
            .send()
            .await?;
        Ok(resp.status().is_success())
    }
}

fn normalize_workspace_path(path: &PathBuf) -> Option<String> {
    let absolute = if path.is_absolute() {
        path.clone()
    } else {
        std::env::current_dir().ok()?.join(path)
    };
    let normalized = if absolute.exists() {
        absolute.canonicalize().ok()?
    } else {
        absolute
    };
    Some(normalized.to_string_lossy().to_string())
}

fn default_tui_permission_rules() -> Vec<serde_json::Value> {
    tandem_core::default_tui_permission_rules()
        .into_iter()
        .map(|rule| {
            serde_json::json!({
                "permission": rule.permission,
                "pattern": rule.pattern,
                "action": rule.action
            })
        })
        .collect()
}

fn parse_sse_payload(buffer: &mut String) -> Option<serde_json::Value> {
    let (end_idx, delim_len) = if let Some(i) = buffer.find("\r\n\r\n") {
        (i, 4)
    } else if let Some(i) = buffer.find("\n\n") {
        (i, 2)
    } else {
        return None;
    };

    let event_str = buffer[..end_idx].to_string();
    *buffer = buffer[end_idx + delim_len..].to_string();

    let mut data_lines: Vec<String> = Vec::new();
    for raw_line in event_str.lines() {
        let line = raw_line.trim_end_matches('\r');
        if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.trim_start().to_string());
        }
    }
    if data_lines.is_empty() {
        return None;
    }
    let data = data_lines.join("\n");
    if data == "[DONE]" {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(&data).ok()
}

fn parse_stream_event_envelope(payload: serde_json::Value) -> Option<StreamEventEnvelope> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?.to_string();
    let props = payload
        .get("properties")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    Some(StreamEventEnvelope {
        event_type,
        session_id: props
            .get("sessionID")
            .or_else(|| props.get("sessionId"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        run_id: props
            .get("runID")
            .or_else(|| props.get("run_id"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        agent_id: props
            .get("agentID")
            .or_else(|| props.get("agent"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        channel: props
            .get("channel")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
        payload,
    })
}

pub fn extract_delta_text(payload: &serde_json::Value) -> Option<String> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    if event_type != "message.part.updated" {
        return None;
    }
    let props = payload.get("properties")?;
    if let Some(delta) = props.get("delta") {
        let extracted = match delta {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Object(map) => map
                .get("text")
                .or_else(|| map.get("delta").and_then(|d| d.get("text")))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string()),
            serde_json::Value::Array(items) => {
                let text = items
                    .iter()
                    .filter_map(|item| match item {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Object(map) => map
                            .get("text")
                            .or_else(|| map.get("delta").and_then(|d| d.get("text")))
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("");
                if text.is_empty() {
                    None
                } else {
                    Some(text)
                }
            }
            _ => None,
        };
        if extracted.is_some() {
            return extracted;
        }
    }
    // Some runtime snapshots only include the final text payload without explicit delta.
    let from_part_text = props
        .get("part")
        .and_then(|p| p.get("text"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.trim().is_empty());
    if from_part_text.is_some() {
        return from_part_text;
    }

    // Some providers emit content arrays with typed text chunks.
    props
        .get("part")
        .and_then(|p| p.get("content"))
        .and_then(|c| c.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Object(map) => map
                        .get("text")
                        .or_else(|| map.get("value"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("")
        })
        .filter(|s| !s.trim().is_empty())
}

pub fn extract_stream_activity(payload: &serde_json::Value) -> Option<String> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    let props = payload.get("properties")?;

    match event_type {
        "permission.asked" => {
            let tool = props.get("tool").and_then(|v| v.as_str()).unwrap_or("tool");
            let request_id = props
                .get("requestID")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Some(format!(
                "Waiting for permission: `{}` (request `{}`)",
                tool, request_id
            ))
        }
        "permission.replied" => {
            let request_id = props
                .get("requestID")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let reply = props
                .get("reply")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Some(format!(
                "Permission `{}` replied with `{}`.",
                request_id, reply
            ))
        }
        "question.asked" => Some("Agent is waiting for your input.".to_string()),
        "message.part.updated" => {
            let Some(part) = props.get("part") else {
                return None;
            };
            let part_type = part.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if part_type != "tool" {
                return None;
            }
            let tool = part
                .get("tool")
                .or_else(|| part.get("name"))
                .and_then(|v| v.as_str())
                .unwrap_or("tool");
            let status = part
                .get("state")
                .and_then(|s| s.get("status"))
                .or_else(|| part.get("status"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            match status {
                "running" => Some(format!("Running tool `{}`...", tool)),
                "pending" => Some(format!("Tool `{}` pending...", tool)),
                "completed" | "done" => Some(format!("Tool `{}` completed.", tool)),
                "failed" | "error" => Some(format!("Tool `{}` failed.", tool)),
                "cancelled" | "canceled" => Some(format!("Tool `{}` cancelled.", tool)),
                _ => Some(format!("Tool `{}` update.", tool)),
            }
        }
        _ => None,
    }
}

pub fn extract_stream_tool_delta(payload: &serde_json::Value) -> Option<StreamToolDelta> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    if event_type != "message.part.updated" {
        return None;
    }
    let props = payload.get("properties")?;
    let tool_delta = props.get("toolCallDelta")?;
    let tool_call_id = tool_delta.get("id").and_then(|v| v.as_str())?.to_string();
    let tool_name = tool_delta
        .get("tool")
        .or_else(|| tool_delta.get("name"))
        .and_then(|v| v.as_str())
        .unwrap_or("tool")
        .to_string();
    let args_delta = tool_delta
        .get("argsDelta")
        .or_else(|| tool_delta.get("delta"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    let args_preview = tool_delta
        .get("parsedArgsPreview")
        .or_else(|| tool_delta.get("argsPreview"))
        .map(|v| {
            if let Some(s) = v.as_str() {
                s.to_string()
            } else {
                v.to_string()
            }
        })
        .unwrap_or_else(|| args_delta.clone());
    Some(StreamToolDelta {
        tool_call_id,
        tool_name,
        args_delta,
        args_preview,
    })
}

pub fn extract_stream_request(payload: &serde_json::Value) -> Option<StreamRequestEvent> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    let props = payload.get("properties")?.clone();

    match event_type {
        "permission.asked" => {
            let request = serde_json::from_value::<PermissionRequest>(serde_json::json!({
                "id": props
                    .get("requestID")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default(),
                "sessionID": props.get("sessionID").cloned().unwrap_or(serde_json::Value::Null),
                "status": "pending",
                "tool": props.get("tool").cloned().unwrap_or(serde_json::Value::Null),
                "args": props.get("args").cloned().unwrap_or(serde_json::Value::Null),
                "argsSource": props
                    .get("argsSource")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                "argsIntegrity": props
                    .get("argsIntegrity")
                    .cloned()
                    .unwrap_or(serde_json::Value::Null),
                "query": props.get("query").cloned().unwrap_or(serde_json::Value::Null),
            }))
            .ok()?;
            if request.id.trim().is_empty() {
                return None;
            }
            Some(StreamRequestEvent::PermissionAsked(request))
        }
        "permission.replied" => Some(StreamRequestEvent::PermissionReplied {
            request_id: props
                .get("requestID")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
            reply: props
                .get("reply")
                .and_then(|v| v.as_str())
                .unwrap_or_default()
                .to_string(),
        }),
        "question.asked" => {
            let mut questions_value = props
                .get("questions")
                .cloned()
                .unwrap_or_else(|| serde_json::json!([]));
            let has_questions = questions_value
                .as_array()
                .map(|arr| !arr.is_empty())
                .unwrap_or(false);
            if !has_questions {
                let fallback_question = props
                    .get("question")
                    .and_then(|v| v.as_str())
                    .or_else(|| props.get("prompt").and_then(|v| v.as_str()))
                    .or_else(|| props.get("query").and_then(|v| v.as_str()))
                    .map(str::trim)
                    .filter(|s| !s.is_empty());
                if let Some(question) = fallback_question {
                    let options = props
                        .get("choices")
                        .or_else(|| props.get("options"))
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|entry| {
                            if let Some(label) = entry.as_str() {
                                serde_json::json!({ "label": label, "description": "" })
                            } else if entry.is_object() {
                                entry
                            } else {
                                serde_json::json!({ "label": entry.to_string(), "description": "" })
                            }
                        })
                        .collect::<Vec<_>>();
                    questions_value = serde_json::json!([{
                        "header": "Question",
                        "question": question,
                        "options": options,
                        "multiple": false,
                        "custom": true
                    }]);
                }
            }
            let request = serde_json::from_value::<QuestionRequest>(serde_json::json!({
                "id": props.get("id").cloned().unwrap_or(serde_json::Value::Null),
                "sessionID": props.get("sessionID").cloned().unwrap_or(serde_json::Value::Null),
                "questions": questions_value,
                "tool": props.get("tool").cloned().unwrap_or(serde_json::Value::Null),
            }))
            .ok()?;
            if request.id.trim().is_empty() {
                return None;
            }
            Some(StreamRequestEvent::QuestionAsked(request))
        }
        _ => None,
    }
}

pub fn extract_stream_agent_team_event(
    payload: &serde_json::Value,
) -> Option<StreamAgentTeamEvent> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    if !event_type.starts_with("agent_team.") {
        return None;
    }
    let properties = payload.get("properties")?;
    Some(StreamAgentTeamEvent {
        event_type: event_type.to_string(),
        team_name: properties
            .get("teamName")
            .or_else(|| properties.get("team_name"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        recipient: properties
            .get("recipient")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        message_type: properties
            .get("messageType")
            .or_else(|| properties.get("message_type"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        request_id: properties
            .get("requestId")
            .or_else(|| properties.get("request_id"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
        message_id: properties
            .get("messageID")
            .or_else(|| properties.get("message_id"))
            .and_then(|v| v.as_str())
            .map(str::to_string),
    })
}

pub fn extract_stream_todo_update(
    payload: &serde_json::Value,
) -> Option<(String, Vec<serde_json::Value>)> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    if event_type != "todo.updated" {
        return None;
    }
    let props = payload.get("properties")?;
    let session_id = props
        .get("sessionID")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())?;
    let todos = props
        .get("todos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    Some((session_id, todos))
}

pub fn extract_stream_error(payload: &serde_json::Value) -> Option<String> {
    let event_type = payload.get("type").and_then(|v| v.as_str())?;
    let props = payload.get("properties")?;

    if event_type == "session.error" {
        if let Some(message) = props
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|v| v.as_str())
        {
            let code = props
                .get("error")
                .and_then(|e| e.get("code"))
                .and_then(|v| v.as_str())
                .unwrap_or("ENGINE_ERROR");
            return Some(format!("{}: {}", code, message));
        }
        return Some("Engine reported an error.".to_string());
    }

    if event_type == "session.run.finished" {
        let status = props.get("status").and_then(|v| v.as_str()).unwrap_or("");
        if status != "completed" {
            let reason = props
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("run did not complete");
            return Some(format!("Run {}: {}", status, reason));
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn spawn_single_response_server(
        expected_path: &'static str,
        response_status: &'static str,
        response_body: &'static str,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = [0u8; 4096];
            let n = socket.read(&mut buf).await.expect("read");
            let req = String::from_utf8_lossy(&buf[..n]);
            let first_line = req.lines().next().unwrap_or("");
            assert!(
                first_line.contains(expected_path),
                "expected path {}, got {}",
                expected_path,
                first_line
            );
            let response = format!(
                "HTTP/1.1 {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response_status,
                response_body.len(),
                response_body
            );
            socket
                .write_all(response.as_bytes())
                .await
                .expect("write_all");
        });
        format!("http://{}", addr)
    }

    #[tokio::test]
    async fn cancel_run_by_id_posts_expected_endpoint() {
        let base = spawn_single_response_server(
            "/session/s1/run/run_42/cancel",
            "200 OK",
            r#"{"ok":true,"cancelled":true}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let cancelled = client
            .cancel_run_by_id("s1", "run_42")
            .await
            .expect("cancel");
        assert!(cancelled);
    }

    #[tokio::test]
    async fn cancel_run_by_id_returns_false_for_non_active_run() {
        let base = spawn_single_response_server(
            "/session/s1/run/run_missing/cancel",
            "200 OK",
            r#"{"ok":true,"cancelled":false}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let cancelled = client
            .cancel_run_by_id("s1", "run_missing")
            .await
            .expect("cancel");
        assert!(!cancelled);
    }

    #[tokio::test]
    async fn mission_list_reads_engine_missions_endpoint() {
        let base = spawn_single_response_server(
            "/mission",
            "200 OK",
            r#"{"missions":[{"mission_id":"m1","status":"draft","spec":{"mission_id":"m1","title":"Demo","goal":"Test","success_criteria":[],"entrypoint":null,"budgets":{},"capabilities":{},"metadata":null},"work_items":[],"revision":0,"updated_at_ms":1}]}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let missions = client.mission_list().await.expect("mission_list");
        assert_eq!(missions.len(), 1);
        assert_eq!(missions[0].mission_id, "m1");
    }

    #[tokio::test]
    async fn mission_get_reads_engine_mission_endpoint() {
        let base = spawn_single_response_server(
            "/mission/m1",
            "200 OK",
            r#"{"mission":{"mission_id":"m1","status":"draft","spec":{"mission_id":"m1","title":"Demo","goal":"Test","success_criteria":[],"entrypoint":null,"budgets":{},"capabilities":{},"metadata":null},"work_items":[],"revision":0,"updated_at_ms":1}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let mission = client.mission_get("m1").await.expect("mission_get");
        assert_eq!(mission.mission_id, "m1");
        assert_eq!(mission.spec.title, "Demo");
    }

    #[tokio::test]
    async fn mission_create_posts_to_engine_mission_endpoint() {
        let base = spawn_single_response_server(
            "/mission",
            "200 OK",
            r#"{"mission":{"mission_id":"m2","status":"draft","spec":{"mission_id":"m2","title":"Create","goal":"Test","success_criteria":[],"entrypoint":null,"budgets":{},"capabilities":{},"metadata":null},"work_items":[],"revision":0,"updated_at_ms":1}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let mission = client
            .mission_create(MissionCreateRequest {
                title: "Create".to_string(),
                goal: "Test".to_string(),
                work_items: vec![],
            })
            .await
            .expect("mission_create");
        assert_eq!(mission.mission_id, "m2");
    }

    #[tokio::test]
    async fn mission_apply_event_posts_event_payload() {
        let base = spawn_single_response_server(
            "/mission/m1/event",
            "200 OK",
            r#"{"mission":{"mission_id":"m1","status":"running","spec":{"mission_id":"m1","title":"Demo","goal":"Test","success_criteria":[],"entrypoint":null,"budgets":{},"capabilities":{},"metadata":null},"work_items":[],"revision":1,"updated_at_ms":2},"commands":[{"type":"emit_notice"}]}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let result = client
            .mission_apply_event(
                "m1",
                serde_json::json!({
                    "type": "mission_started",
                    "mission_id": "m1"
                }),
            )
            .await
            .expect("mission_apply_event");
        assert_eq!(result.mission.revision, 1);
        assert_eq!(result.commands.len(), 1);
    }

    #[tokio::test]
    async fn context_runs_list_reads_engine_context_runs_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs",
            "200 OK",
            r#"{"runs":[{"run_id":"ctx-1","run_type":"interactive","status":"running","objective":"Ship context-driving","workspace":{"workspace_id":"ws1","canonical_path":"/tmp/ws","lease_epoch":1},"steps":[{"step_id":"s1","title":"Plan","status":"in_progress"}],"why_next_step":"Need plan before execution","revision":3,"created_at_ms":1,"updated_at_ms":2}]}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let runs = client.context_runs_list().await.expect("context_runs_list");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].run_id, "ctx-1");
        assert_eq!(runs[0].status, ContextRunStatus::Running);
    }

    #[tokio::test]
    async fn context_run_get_reads_engine_context_run_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-2",
            "200 OK",
            r#"{"run":{"run_id":"ctx-2","run_type":"cron","status":"paused","objective":"Nightly pipeline","workspace":{"workspace_id":"ws2","canonical_path":"/tmp/cron","lease_epoch":2},"steps":[],"why_next_step":null,"revision":7,"created_at_ms":3,"updated_at_ms":4}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let run = client
            .context_run_get("ctx-2")
            .await
            .expect("context_run_get");
        assert_eq!(run.run_id, "ctx-2");
        assert_eq!(run.run_type, "cron");
        assert_eq!(run.status, ContextRunStatus::Paused);
    }

    #[tokio::test]
    async fn context_run_events_reads_engine_context_run_events_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-3/events",
            "200 OK",
            r#"{"events":[{"event_id":"evt-1","run_id":"ctx-3","seq":12,"ts_ms":1000,"type":"step_started","status":"running","step_id":"s-plan","payload":{"why_next_step":"execute plan"}}]}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let events = client
            .context_run_events("ctx-3", Some(10), Some(5))
            .await
            .expect("context_run_events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].seq, 12);
        assert_eq!(events[0].event_type, "step_started");
        assert_eq!(events[0].status, ContextRunStatus::Running);
        assert_eq!(events[0].step_id.as_deref(), Some("s-plan"));
    }

    #[tokio::test]
    async fn context_run_append_event_posts_to_engine_context_run_events_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-4/events",
            "200 OK",
            r#"{"event":{"event_id":"evt-2","run_id":"ctx-4","seq":3,"ts_ms":2000,"type":"run_paused","status":"paused","step_id":null,"payload":{"source":"tui"}}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let event = client
            .context_run_append_event(
                "ctx-4",
                "run_paused",
                ContextRunStatus::Paused,
                None,
                serde_json::json!({ "source": "tui" }),
            )
            .await
            .expect("context_run_append_event");
        assert_eq!(event.run_id, "ctx-4");
        assert_eq!(event.seq, 3);
        assert_eq!(event.status, ContextRunStatus::Paused);
    }

    #[tokio::test]
    async fn context_run_blackboard_reads_engine_context_run_blackboard_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-5/blackboard",
            "200 OK",
            r#"{"blackboard":{"facts":[{"id":"f1","ts_ms":1,"text":"fact","step_id":null,"source_event_id":null}],"decisions":[],"open_questions":[],"artifacts":[],"summaries":{"rolling":"summary","latest_context_pack":"pack"},"revision":9}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let blackboard = client
            .context_run_blackboard("ctx-5")
            .await
            .expect("context_run_blackboard");
        assert_eq!(blackboard.revision, 9);
        assert_eq!(blackboard.facts.len(), 1);
        assert_eq!(blackboard.summaries.rolling, "summary");
    }

    #[tokio::test]
    async fn context_run_replay_reads_engine_context_run_replay_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-6/replay",
            "200 OK",
            r#"{"ok":true,"run_id":"ctx-6","from_checkpoint":true,"checkpoint_seq":9,"events_applied":2,"replay":{"run_id":"ctx-6","run_type":"interactive","status":"running","objective":"obj","workspace":{"workspace_id":"w","canonical_path":"/tmp","lease_epoch":1},"steps":[],"why_next_step":"next","revision":3,"created_at_ms":1,"updated_at_ms":2},"persisted":{"run_id":"ctx-6","run_type":"interactive","status":"running","objective":"obj","workspace":{"workspace_id":"w","canonical_path":"/tmp","lease_epoch":1},"steps":[],"why_next_step":"next","revision":3,"created_at_ms":1,"updated_at_ms":2},"drift":{"mismatch":false,"status_mismatch":false,"why_next_step_mismatch":false,"step_count_mismatch":false}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let replay = client
            .context_run_replay("ctx-6", Some(10), Some(true))
            .await
            .expect("context_run_replay");
        assert_eq!(replay.run_id, "ctx-6");
        assert!(!replay.drift.mismatch);
        assert_eq!(replay.events_applied, 2);
    }

    #[tokio::test]
    async fn context_run_driver_next_posts_engine_context_run_driver_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-7/driver/next",
            "200 OK",
            r#"{"ok":true,"dry_run":false,"run_id":"ctx-7","selected_step_id":"s2","target_status":"running","why_next_step":"selected runnable step","run":{"run_id":"ctx-7","run_type":"interactive","status":"running","objective":"obj","workspace":{"workspace_id":"w","canonical_path":"/tmp","lease_epoch":1},"steps":[{"step_id":"s2","title":"Exec","status":"in_progress"}],"why_next_step":"selected runnable step","revision":4,"created_at_ms":1,"updated_at_ms":2}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let next = client
            .context_run_driver_next("ctx-7", false)
            .await
            .expect("context_run_driver_next");
        assert_eq!(next.run_id, "ctx-7");
        assert_eq!(next.selected_step_id.as_deref(), Some("s2"));
        assert_eq!(next.target_status, ContextRunStatus::Running);
    }

    #[tokio::test]
    async fn context_run_sync_todos_posts_engine_context_todos_sync_endpoint() {
        let base = spawn_single_response_server(
            "/context/runs/ctx-8/todos/sync",
            "200 OK",
            r#"{"run":{"run_id":"ctx-8","run_type":"interactive","status":"planning","objective":"obj","workspace":{"workspace_id":"w","canonical_path":"/tmp","lease_epoch":1},"steps":[{"step_id":"task-1","title":"Plan","status":"in_progress"}],"why_next_step":"continue task `task-1` from synced todo list","revision":5,"created_at_ms":1,"updated_at_ms":2}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let run = client
            .context_run_sync_todos(
                "ctx-8",
                vec![ContextTodoSyncItem {
                    id: Some("task-1".to_string()),
                    content: "Plan".to_string(),
                    status: Some("in_progress".to_string()),
                }],
                true,
                Some("s-1".to_string()),
                Some("r-1".to_string()),
            )
            .await
            .expect("context_run_sync_todos");
        assert_eq!(run.run_id, "ctx-8");
        assert_eq!(run.steps.len(), 1);
        assert_eq!(run.steps[0].step_id, "task-1");
    }

    #[tokio::test]
    async fn packs_list_reads_engine_packs_endpoint() {
        let base = spawn_single_response_server(
            "/packs",
            "200 OK",
            r#"{"packs":[{"pack_id":"p1","name":"pack-one","version":"1.0.0"}]}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let packs = client.packs_list().await.expect("packs_list");
        assert_eq!(packs.len(), 1);
        assert_eq!(packs[0].pack_id, "p1");
        assert_eq!(packs[0].name, "pack-one");
    }

    #[tokio::test]
    async fn capabilities_bindings_get_reads_engine_endpoint() {
        let base = spawn_single_response_server(
            "/capabilities/bindings",
            "200 OK",
            r#"{"bindings":{"schema_version":"v1","bindings":[{"capability_id":"github.create_pull_request","provider":"composio","tool_name":"mcp.composio.github_create_pull_request","metadata":{}}]}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let bindings = client
            .capabilities_bindings_get()
            .await
            .expect("capabilities_bindings_get");
        assert_eq!(bindings.schema_version, "v1");
        assert_eq!(bindings.bindings.len(), 1);
        assert_eq!(
            bindings.bindings[0].capability_id,
            "github.create_pull_request"
        );
    }

    #[tokio::test]
    async fn capabilities_resolve_posts_engine_endpoint() {
        let base = spawn_single_response_server(
            "/capabilities/resolve",
            "200 OK",
            r#"{"resolution":{"resolved":[{"capability_id":"github.create_pull_request","provider":"arcade","tool_name":"mcp.arcade.github_create_pull_request"}],"missing_required":[]}}"#,
        )
        .await;
        let client = EngineClient::new(base);
        let resolved = client
            .capabilities_resolve(CapabilityResolveRequest {
                workflow_id: Some("wf-pr".to_string()),
                required_capabilities: vec!["github.create_pull_request".to_string()],
                optional_capabilities: vec![],
                provider_preference: vec!["arcade".to_string(), "composio".to_string()],
                available_tools: vec![],
            })
            .await
            .expect("capabilities_resolve");
        let provider = resolved
            .resolution
            .get("resolved")
            .and_then(|v| v.as_array())
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("provider"))
            .and_then(|v| v.as_str());
        assert_eq!(provider, Some("arcade"));
    }

    #[test]
    fn parse_stream_event_envelope_extracts_core_fields() {
        let payload = serde_json::json!({
            "type": "message.part.updated",
            "properties": {
                "sessionID": "s1",
                "runID": "r1",
                "agentID": "A2",
                "channel": "assistant",
                "delta": "hello"
            }
        });
        let envelope = parse_stream_event_envelope(payload.clone()).expect("envelope");
        assert_eq!(envelope.event_type, "message.part.updated");
        assert_eq!(envelope.session_id.as_deref(), Some("s1"));
        assert_eq!(envelope.run_id.as_deref(), Some("r1"));
        assert_eq!(envelope.agent_id.as_deref(), Some("A2"));
        assert_eq!(envelope.channel.as_deref(), Some("assistant"));
        assert_eq!(envelope.payload, payload);
    }

    #[test]
    fn parse_sse_payload_reads_data_block() {
        let mut buffer =
            "event: message\ndata: {\"type\":\"message.part.updated\",\"properties\":{\"delta\":\"x\"}}\n\n"
                .to_string();
        let parsed = parse_sse_payload(&mut buffer).expect("payload");
        assert_eq!(
            parsed.get("type").and_then(|v| v.as_str()),
            Some("message.part.updated")
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn parse_stream_event_envelope_handles_mission_events_contract_shape() {
        let payload = serde_json::json!({
            "type": "mission.created",
            "properties": {
                "missionID": "m-123",
                "workItemCount": 2
            }
        });
        let envelope = parse_stream_event_envelope(payload.clone()).expect("envelope");
        assert_eq!(envelope.event_type, "mission.created");
        assert_eq!(envelope.session_id, None);
        assert_eq!(envelope.run_id, None);
        assert_eq!(envelope.agent_id, None);
        assert_eq!(envelope.channel, None);
        assert_eq!(
            envelope
                .payload
                .get("properties")
                .and_then(|p| p.get("missionID"))
                .and_then(|v| v.as_str()),
            Some("m-123")
        );
        assert_eq!(
            envelope
                .payload
                .get("properties")
                .and_then(|p| p.get("workItemCount"))
                .and_then(|v| v.as_u64()),
            Some(2)
        );
    }

    #[test]
    fn parse_stream_event_envelope_handles_routine_policy_events_contract_shape() {
        let payloads = vec![
            serde_json::json!({
                "type": "routine.fired",
                "properties": {
                    "routineID": "r-1",
                    "runCount": 1,
                    "triggerType": "manual",
                    "firedAtMs": 123
                }
            }),
            serde_json::json!({
                "type": "routine.approval_required",
                "properties": {
                    "routineID": "r-2",
                    "runCount": 1,
                    "triggerType": "manual",
                    "reason": "manual approval required before external side effects (manual)"
                }
            }),
            serde_json::json!({
                "type": "routine.blocked",
                "properties": {
                    "routineID": "r-3",
                    "runCount": 1,
                    "triggerType": "manual",
                    "reason": "external integrations are disabled by policy"
                }
            }),
        ];

        for payload in payloads {
            let envelope = parse_stream_event_envelope(payload.clone()).expect("envelope");
            assert!(envelope.event_type.starts_with("routine."));
            assert_eq!(envelope.session_id, None);
            assert_eq!(envelope.run_id, None);
            assert_eq!(
                envelope
                    .payload
                    .get("properties")
                    .and_then(|p| p.get("routineID"))
                    .and_then(|v| v.as_str())
                    .map(|s| !s.is_empty()),
                Some(true)
            );
            assert_eq!(
                envelope
                    .payload
                    .get("properties")
                    .and_then(|p| p.get("runCount"))
                    .and_then(|v| v.as_u64()),
                Some(1)
            );
        }
    }

    #[test]
    fn extract_stream_error_reads_session_error() {
        let payload = serde_json::json!({
            "type": "session.error",
            "properties": {
                "error": { "code": "PROVIDER_AUTH", "message": "missing API key" }
            }
        });
        let msg = extract_stream_error(&payload).expect("error");
        assert!(msg.contains("PROVIDER_AUTH"));
        assert!(msg.contains("missing API key"));
    }

    #[test]
    fn extract_stream_tool_delta_reads_tool_call_delta_payload() {
        let payload = serde_json::json!({
            "type": "message.part.updated",
            "properties": {
                "toolCallDelta": {
                    "id": "call_1",
                    "tool": "write",
                    "argsDelta": "{\"path\":\"src/main.rs\"",
                    "parsedArgsPreview": { "path": "src/main.rs" }
                }
            }
        });
        let delta = extract_stream_tool_delta(&payload).expect("tool delta");
        assert_eq!(delta.tool_call_id, "call_1");
        assert_eq!(delta.tool_name, "write");
        assert!(delta.args_delta.contains("path"));
        assert!(delta.args_preview.contains("src/main.rs"));
    }

    #[test]
    fn extract_stream_tool_delta_ignores_non_tool_payloads() {
        let payload = serde_json::json!({
            "type": "message.part.updated",
            "properties": {
                "part": { "type": "text", "text": "hello" }
            }
        });
        assert!(extract_stream_tool_delta(&payload).is_none());
    }

    #[test]
    fn extract_stream_agent_team_event_reads_mailbox_properties() {
        let payload = serde_json::json!({
            "type": "agent_team.mailbox.enqueued",
            "properties": {
                "teamName": "alpha",
                "recipient": "A2",
                "messageType": "task_prompt",
                "messageID": "m-1"
            }
        });
        let event = extract_stream_agent_team_event(&payload).expect("agent-team event");
        assert_eq!(event.event_type, "agent_team.mailbox.enqueued");
        assert_eq!(event.team_name.as_deref(), Some("alpha"));
        assert_eq!(event.recipient.as_deref(), Some("A2"));
        assert_eq!(event.message_type.as_deref(), Some("task_prompt"));
        assert_eq!(event.message_id.as_deref(), Some("m-1"));
    }
}
