use anyhow::{anyhow, bail, Result};
use futures::StreamExt;
use reqwest::{header::HeaderMap, header::HeaderValue, Client};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tandem_types::{CreateSessionRequest, ModelSpec};
use tandem_wire::{WireProviderEntry, WireSessionMessage};

const NET_RETRY_ATTEMPTS: usize = 2;
const ENGINE_STARTING_ATTEMPTS: usize = 10;
const ENGINE_STARTING_DELAY_MS: u64 = 450;

enum EngineRetryOutcome {
    Response(reqwest::Response),
    ErrorStatus(reqwest::StatusCode, String),
}

fn is_engine_starting_text(body: &str) -> bool {
    body.contains("ENGINE_STARTING")
        || body.contains("Engine starting")
        || body.contains("Service Unavailable")
}

async fn send_with_engine_retry<F>(mut make_req: F) -> Result<EngineRetryOutcome>
where
    F: FnMut() -> reqwest::RequestBuilder,
{
    let mut net_attempts = 0;
    let mut starting_attempts = 0;
    loop {
        match make_req().send().await {
            Ok(resp) if resp.status().is_success() => {
                return Ok(EngineRetryOutcome::Response(resp))
            }
            Ok(resp) => {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                if (status == reqwest::StatusCode::SERVICE_UNAVAILABLE
                    || is_engine_starting_text(&body))
                    && starting_attempts < ENGINE_STARTING_ATTEMPTS
                {
                    starting_attempts += 1;
                    tokio::time::sleep(Duration::from_millis(ENGINE_STARTING_DELAY_MS)).await;
                    continue;
                }
                return Ok(EngineRetryOutcome::ErrorStatus(status, body));
            }
            Err(err)
                if (err.is_connect() || err.is_timeout()) && net_attempts < NET_RETRY_ATTEMPTS =>
            {
                net_attempts += 1;
                tokio::time::sleep(Duration::from_millis(500 * net_attempts as u64)).await;
            }
            Err(err) => return Err(err.into()),
        }
    }
}

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
pub struct ContextRunDetailResponse {
    pub run: ContextRunState,
    #[serde(default)]
    pub rollback_preview_summary: serde_json::Value,
    #[serde(default)]
    pub rollback_history_summary: serde_json::Value,
    #[serde(default)]
    pub last_rollback_outcome: serde_json::Value,
    #[serde(default)]
    pub rollback_policy: serde_json::Value,
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

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
pub struct ContextRunRollbackHistoryEntry {
    pub seq: u64,
    pub ts_ms: u64,
    pub event_id: String,
    pub outcome: String,
    #[serde(default)]
    pub selected_event_ids: Vec<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub missing_event_ids: Option<Vec<String>>,
    #[serde(default)]
    pub applied_step_count: Option<u64>,
    #[serde(default)]
    pub applied_operation_count: Option<u64>,
    #[serde(default)]
    pub applied_by_action: Option<HashMap<String, u64>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
pub struct ContextRunRollbackPreviewStep {
    pub seq: u64,
    pub event_id: String,
    #[serde(default)]
    pub tool: Option<String>,
    pub executable: bool,
    pub operation_count: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
pub struct ContextRunRollbackPreviewResponse {
    #[serde(default)]
    pub steps: Vec<ContextRunRollbackPreviewStep>,
    #[serde(default)]
    pub step_count: u64,
    #[serde(default)]
    pub executable_step_count: u64,
    #[serde(default)]
    pub advisory_step_count: u64,
    #[serde(default)]
    pub executable: bool,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq, Default)]
pub struct ContextRunRollbackExecuteResponse {
    pub applied: bool,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub selected_event_ids: Vec<String>,
    #[serde(default)]
    pub missing_event_ids: Option<Vec<String>>,
    #[serde(default)]
    pub applied_step_count: Option<u64>,
    #[serde(default)]
    pub applied_operation_count: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Default)]
pub struct ContextRunRollbackHistoryResponse {
    #[serde(default)]
    pub entries: Vec<ContextRunRollbackHistoryEntry>,
    #[serde(default)]
    pub summary: serde_json::Value,
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

