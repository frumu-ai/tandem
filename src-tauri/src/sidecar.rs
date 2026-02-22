// Tandem Sidecar Manager
// Handles spawning, lifecycle, and communication with the tandem-engine sidecar process

use crate::error::{Result, TandemError};
use crate::logs::LogRingBuffer;
use futures::StreamExt;
use reqwest::{header::HeaderMap, header::HeaderValue, Client};
use serde::de::Error as _;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tandem_core::{
    engine_api_token_file_path, load_or_create_engine_api_token, resolve_shared_paths,
    DEFAULT_ENGINE_PORT,
};
use tandem_observability::{emit_event, ObservabilityEvent, ProcessKind};
use tandem_skills::{SkillContent, SkillInfo, SkillLocation, SkillTemplateInfo};
use tokio::sync::{Mutex, RwLock};

#[cfg(windows)]
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
#[cfg(windows)]
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
    SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};
#[cfg(windows)]
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE};

#[cfg(windows)]
// Store as integer so the manager stays Send + Sync (Tauri requires this for shared state).
struct WindowsJobHandle(isize);

#[cfg(windows)]
impl WindowsJobHandle {
    fn as_handle(&self) -> HANDLE {
        self.0 as HANDLE
    }
}

#[cfg(windows)]
impl Drop for WindowsJobHandle {
    fn drop(&mut self) {
        unsafe {
            if self.0 != 0 {
                // With JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, closing the job handle terminates
                // all assigned processes. This prevents orphaned sidecars during dev reloads.
                let _ = CloseHandle(self.as_handle());
            }
        }
    }
}

#[cfg(windows)]
fn windows_create_kill_on_close_job() -> std::io::Result<WindowsJobHandle> {
    unsafe {
        let job = CreateJobObjectW(std::ptr::null_mut(), std::ptr::null());
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

        let ok = SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &mut info as *mut _ as *mut _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        );
        if ok == 0 {
            let e = std::io::Error::last_os_error();
            let _ = CloseHandle(job);
            return Err(e);
        }

        Ok(WindowsJobHandle(job as isize))
    }
}

#[cfg(windows)]
fn windows_try_assign_pid_to_job(job: HANDLE, pid: u32) -> std::io::Result<()> {
    unsafe {
        // We reopen the process by PID to get a handle we can pass to AssignProcessToJobObject.
        // This avoids relying on std's internal Child handle representation.
        let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
        if process.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let ok = AssignProcessToJobObject(job, process);
        let assign_err = if ok == 0 {
            Some(std::io::Error::last_os_error())
        } else {
            None
        };
        let _ = CloseHandle(process);
        if let Some(e) = assign_err {
            return Err(e);
        }
        Ok(())
    }
}

/// Sidecar process state
#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SidecarState {
    Stopped,
    Starting,
    Running,
    Stopping,
    Failed,
}

/// Circuit breaker state for resilience
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    Closed,   // Normal operation
    Open,     // Blocking requests (cooldown)
    HalfOpen, // Testing recovery
}

#[derive(Debug, Clone, Serialize)]
pub struct SidecarCircuitSnapshot {
    pub state: String,
    pub failure_count: u32,
    pub last_failure_age_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SidecarRuntimeSnapshot {
    pub state: SidecarState,
    pub shared_mode: bool,
    pub owns_process: bool,
    pub port: Option<u16>,
    pub pid: Option<u32>,
    pub binary_path: Option<String>,
    pub circuit: SidecarCircuitSnapshot,
}

#[derive(Debug, Clone, Deserialize)]
struct SidecarHealthResponse {
    healthy: bool,
    #[serde(default = "default_health_ready")]
    ready: bool,
    #[serde(default)]
    phase: String,
    #[serde(default)]
    startup_attempt_id: String,
    #[serde(default)]
    startup_elapsed_ms: u64,
    #[serde(default)]
    last_error: Option<String>,
    #[serde(default)]
    build_id: Option<String>,
    #[serde(default)]
    binary_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SidecarStartupHealth {
    pub healthy: bool,
    pub ready: bool,
    pub phase: String,
    pub startup_attempt_id: String,
    pub startup_elapsed_ms: u64,
    pub last_error: Option<String>,
    pub build_id: Option<String>,
    pub binary_path: Option<String>,
}

fn default_health_ready() -> bool {
    true
}

fn default_sidecar_port() -> u16 {
    std::env::var("TANDEM_ENGINE_PORT")
        .ok()
        .and_then(|raw| raw.trim().parse::<u16>().ok())
        .unwrap_or(DEFAULT_ENGINE_PORT)
}

fn build_http_client(timeout: Duration, api_token: &str) -> Client {
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(api_token) {
        headers.insert("x-tandem-token", value);
    }
    Client::builder()
        .default_headers(headers)
        .timeout(timeout)
        .build()
        .expect("Failed to create HTTP client")
}

fn build_stream_client(api_token: &str) -> Client {
    let mut headers = HeaderMap::new();
    if let Ok(value) = HeaderValue::from_str(api_token) {
        headers.insert("x-tandem-token", value);
    }
    Client::builder()
        .default_headers(headers)
        .http1_only()
        .tcp_keepalive(Duration::from_secs(60))
        .build()
        .expect("Failed to create stream client")
}

/// Configuration for the sidecar manager
#[derive(Debug, Clone)]
pub struct SidecarConfig {
    /// Port for the sidecar to listen on (0 = auto-assign)
    pub port: u16,
    /// Maximum number of consecutive failures before circuit opens
    pub max_failures: u32,
    /// Cooldown duration when circuit is open
    pub cooldown_duration: Duration,
    /// Timeout for sidecar operations
    pub operation_timeout: Duration,
    /// Heartbeat interval
    /// Health check interval (currently unused, reserved for future)
    #[allow(dead_code)]
    pub heartbeat_interval: Duration,
    /// Workspace path for OpenCode
    pub workspace_path: Option<PathBuf>,
    /// Shared mode allows Desktop and TUI/CLI to attach to a single local engine instance.
    pub shared_mode: bool,
}

impl Default for SidecarConfig {
    fn default() -> Self {
        Self {
            port: default_sidecar_port(),
            max_failures: 3,
            cooldown_duration: Duration::from_secs(30),
            operation_timeout: Duration::from_secs(300),
            heartbeat_interval: Duration::from_secs(5),
            workspace_path: None,
            shared_mode: true,
        }
    }
}

/// Circuit breaker for handling sidecar failures
pub struct CircuitBreaker {
    state: CircuitState,
    failure_count: u32,
    last_failure: Option<Instant>,
    config: SidecarConfig,
}

impl CircuitBreaker {
    pub fn new(config: SidecarConfig) -> Self {
        Self {
            state: CircuitState::Closed,
            failure_count: 0,
            last_failure: None,
            config,
        }
    }

    pub fn record_success(&mut self) {
        self.failure_count = 0;
        self.state = CircuitState::Closed;
    }

    pub fn record_failure(&mut self) {
        self.failure_count += 1;
        self.last_failure = Some(Instant::now());

        if self.failure_count >= self.config.max_failures {
            tracing::warn!(
                "Circuit breaker opened after {} failures",
                self.failure_count
            );
            self.state = CircuitState::Open;
        }
    }

    pub fn can_execute(&mut self) -> bool {
        match self.state {
            CircuitState::Closed => true,
            CircuitState::HalfOpen => true,
            CircuitState::Open => {
                if let Some(last_failure) = self.last_failure {
                    if last_failure.elapsed() >= self.config.cooldown_duration {
                        tracing::info!("Circuit breaker entering half-open state");
                        self.state = CircuitState::HalfOpen;
                        return true;
                    }
                }
                false
            }
        }
    }
}

// ============================================================================
// OpenCode API Types
// ============================================================================

/// Session creation request
#[derive(Debug, Serialize)]
pub struct CreateSessionRequest {
    /// Optional parent session ID. When set, OpenCode treats this session as a child and it
    /// will not be returned when listing only root sessions.
    #[serde(rename = "parentID", skip_serializing_if = "Option::is_none")]
    pub parent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<Vec<PermissionRule>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(rename = "workspace_root", skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct PermissionRule {
    pub permission: String,
    pub pattern: String,
    pub action: String,
}

/// Session time information from OpenCode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTime {
    pub created: u64,
    pub updated: u64,
}

/// Session response from OpenCode
/// Matches the actual API response format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slug: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(rename = "projectID", skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
    #[serde(rename = "workspaceRoot", skip_serializing_if = "Option::is_none")]
    pub workspace_root: Option<String>,
    #[serde(
        rename = "originWorkspaceRoot",
        skip_serializing_if = "Option::is_none"
    )]
    pub origin_workspace_root: Option<String>,
    #[serde(
        rename = "attachedFromWorkspace",
        skip_serializing_if = "Option::is_none"
    )]
    pub attached_from_workspace: Option<String>,
    #[serde(
        rename = "attachedToWorkspace",
        skip_serializing_if = "Option::is_none"
    )]
    pub attached_to_workspace: Option<String>,
    #[serde(rename = "attachTimestampMs", skip_serializing_if = "Option::is_none")]
    pub attach_timestamp_ms: Option<u64>,
    #[serde(rename = "attachReason", skip_serializing_if = "Option::is_none")]
    pub attach_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<SessionTime>,
    // Legacy fields for compatibility
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_model_string_or_object"
    )]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default)]
    pub messages: Vec<serde_json::Value>,
}

fn deserialize_model_string_or_object<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let Some(value) = value else {
        return Ok(None);
    };

    match value {
        serde_json::Value::String(s) => Ok(Some(s)),
        serde_json::Value::Object(map) => {
            if let Some(model) = map
                .get("modelID")
                .and_then(|v| v.as_str())
                .or_else(|| map.get("model_id").and_then(|v| v.as_str()))
            {
                return Ok(Some(model.to_string()));
            }
            Err(D::Error::custom(
                "model object missing modelID/model_id string field",
            ))
        }
        other => Err(D::Error::custom(format!(
            "invalid type for model field: {other}"
        ))),
    }
}

/// Message in a session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: String, // "user", "assistant", "system"
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
}

/// Tool call made by the assistant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub tool: String,
    pub args: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>, // "pending", "running", "completed", "failed"
}

/// Text part for message input
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextPartInput {
    #[serde(rename = "type")]
    pub part_type: String, // Always "text"
    pub text: String,
}

/// File part for message input (images, documents)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FilePartInput {
    #[serde(rename = "type")]
    pub part_type: String, // Always "file"
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    pub url: String, // data URL or file path
}

/// Message part enum for sending
#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum MessagePartInput {
    Text(TextPartInput),
    File(FilePartInput),
}

/// Model specification for prompt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSpec {
    #[serde(rename = "providerID", alias = "provider_id")]
    pub provider_id: String,
    #[serde(rename = "modelID", alias = "model_id")]
    pub model_id: String,
}

/// Send message request (prompt_async format)
#[derive(Debug, Serialize)]
pub struct SendMessageRequest {
    pub parts: Vec<MessagePartInput>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelSpec>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
}

impl SendMessageRequest {
    /// Create a simple text message request
    pub fn text(content: String) -> Self {
        Self {
            parts: vec![MessagePartInput::Text(TextPartInput {
                part_type: "text".to_string(),
                text: content,
            })],
            model: None,
            agent: None,
        }
    }

    /// Create a text message request with a specific model (reserved for future use)
    #[allow(dead_code)]
    pub fn text_with_model(content: String, provider_id: String, model_id: String) -> Self {
        Self {
            parts: vec![MessagePartInput::Text(TextPartInput {
                part_type: "text".to_string(),
                text: content,
            })],
            model: Some(ModelSpec {
                provider_id,
                model_id,
            }),
            agent: None,
        }
    }

    /// Create a message with text and file attachments
    pub fn with_attachments(content: String, attachments: Vec<FilePartInput>) -> Self {
        let mut parts: Vec<MessagePartInput> = attachments
            .into_iter()
            .map(MessagePartInput::File)
            .collect();

        if !content.is_empty() {
            parts.push(MessagePartInput::Text(TextPartInput {
                part_type: "text".to_string(),
                text: content,
            }));
        }

        Self {
            parts,
            model: None,
            agent: None,
        }
    }

    /// Set the agent for this request (reserved for future use)
    #[allow(dead_code)]
    pub fn with_agent(mut self, agent: String) -> Self {
        self.agent = Some(agent);
        self
    }
}

/// Project from OpenCode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub worktree: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vcs: Option<String>,
    #[serde(default)]
    pub sandboxes: Vec<serde_json::Value>,
    #[serde(default)]
    pub time: ProjectTime,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectTime {
    #[serde(default)]
    pub created: u64,
    #[serde(default)]
    pub updated: u64,
}

/// Full message with parts from OpenCode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    pub info: MessageInfo,
    pub parts: Vec<serde_json::Value>, // Parts can have various types
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageInfo {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub role: String,
    pub time: MessageTime,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<MessageSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deleted: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reverted: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageTime {
    pub created: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub completed: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageSummary {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default)]
    pub diffs: Vec<serde_json::Value>,
}

/// OpenCode event properties wrapper (reserved for future use)
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventProperties<T> {
    #[serde(flatten)]
    pub properties: T,
}

/// Message part from OpenCode (reserved for future use)
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePart {
    pub id: Option<String>,
    #[serde(rename = "sessionID")]
    pub session_id: Option<String>,
    #[serde(rename = "messageID")]
    pub message_id: Option<String>,
    #[serde(rename = "type")]
    pub part_type: Option<String>,
    pub text: Option<String>,
    // Tool-related fields
    pub tool: Option<String>,
    pub args: Option<serde_json::Value>,
    pub state: Option<String>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

/// Message part updated event properties (reserved for future use)
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagePartUpdatedProps {
    pub part: MessagePart,
    pub delta: Option<String>,
}

/// Session status event properties (reserved for future use)
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStatusProps {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub status: String,
}

/// Session idle event properties (reserved for future use)
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionIdleProps {
    #[serde(rename = "sessionID")]
    pub session_id: String,
}

/// Session error event properties (reserved for future use)
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionErrorProps {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub error: String,
    #[serde(default, rename = "errorCode")]
    pub error_code: Option<String>,
}

/// Permission asked event properties (reserved for future use)
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionAskedProps {
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "requestID")]
    pub request_id: String,
    pub tool: Option<String>,
    pub args: Option<serde_json::Value>,
    #[serde(rename = "argsSource")]
    pub args_source: Option<String>,
    #[serde(rename = "argsIntegrity")]
    pub args_integrity: Option<String>,
    pub query: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct PermissionSnapshot {
    #[serde(default)]
    pub requests: Vec<PermissionRequest>,
    #[serde(default)]
    pub rules: Vec<serde_json::Value>,
}

/// Raw OpenCode event from SSE stream
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenCodeEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub properties: serde_json::Value,
}

/// Simplified streaming event for frontend consumption
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Text content chunk (delta or full)
    Content {
        session_id: String,
        message_id: String,
        content: String,
        delta: Option<String>,
    },
    /// Tool call started or updated
    ToolStart {
        session_id: String,
        message_id: String,
        part_id: String,
        tool: String,
        args: serde_json::Value,
    },
    /// Tool call completed
    ToolEnd {
        session_id: String,
        message_id: String,
        part_id: String,
        tool: String,
        result: Option<serde_json::Value>,
        error: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_code: Option<String>,
    },
    /// Session status changed
    SessionStatus { session_id: String, status: String },
    /// Session run started
    RunStarted {
        session_id: String,
        run_id: String,
        started_at_ms: u64,
        client_id: Option<String>,
    },
    /// Session run finished
    RunFinished {
        session_id: String,
        run_id: String,
        finished_at_ms: u64,
        status: String,
        error: Option<String>,
    },
    /// Session run conflict
    RunConflict {
        session_id: String,
        run_id: String,
        retry_after_ms: u64,
        attach_event_stream: String,
    },
    /// Session is idle (generation complete)
    SessionIdle { session_id: String },
    /// Session error
    SessionError {
        session_id: String,
        error: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error_code: Option<String>,
    },
    /// Permission requested
    PermissionAsked {
        session_id: String,
        request_id: String,
        tool: Option<String>,
        args: Option<serde_json::Value>,
        args_source: Option<String>,
        args_integrity: Option<String>,
        query: Option<String>,
    },
    /// File edited (from file.edited event)
    FileEdited {
        session_id: String,
        file_path: String,
    },
    /// Raw event (for debugging or unhandled types)
    Raw {
        event_type: String,
        data: serde_json::Value,
    },
    /// Todo list updated
    TodoUpdated {
        session_id: String,
        todos: Vec<TodoItem>,
    },
    /// Question asked by LLM
    QuestionAsked {
        session_id: String,
        request_id: String,
        questions: Vec<QuestionInfo>,
        tool_call_id: Option<String>,
        tool_message_id: Option<String>,
    },
    /// Local memory retrieval telemetry for prompt-context injection.
    MemoryRetrieval {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        used: bool,
        chunks_total: usize,
        session_chunks: usize,
        history_chunks: usize,
        project_fact_chunks: usize,
        latency_ms: u128,
        query_hash: String,
        score_min: Option<f64>,
        score_max: Option<f64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        embedding_status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        embedding_reason: Option<String>,
    },
    /// Local memory storage telemetry for chat-turn ingestion.
    MemoryStorage {
        session_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        role: String,
        session_chunks_stored: usize,
        project_chunks_stored: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
}

/// A single multiple-choice option for a question prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionChoice {
    pub label: String,
    pub description: String,
}

/// A single question in a question request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionInfo {
    /// Very short label (max ~12 chars in OpenCode schema).
    pub header: String,
    /// The full question text.
    pub question: String,
    /// Multiple-choice options.
    pub options: Vec<QuestionChoice>,
    /// Allow selecting multiple options.
    pub multiple: Option<bool>,
    /// Allow typing a custom answer.
    pub custom: Option<bool>,
}

/// Frontend-friendly question request (snake_case fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionRequest {
    pub session_id: String,
    pub request_id: String,
    pub questions: Vec<QuestionInfo>,
    pub tool_call_id: Option<String>,
    pub tool_message_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenCodeQuestionToolRef {
    #[serde(rename = "callID")]
    pub call_id: String,
    #[serde(rename = "messageID")]
    pub message_id: String,
}

/// OpenCode wire format for question requests.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OpenCodeQuestionRequest {
    pub id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    pub questions: Vec<QuestionInfo>,
    pub tool: Option<OpenCodeQuestionToolRef>,
}

/// Model info from OpenCode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context_length: Option<u32>,
}

/// Provider info from OpenCode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub models: Vec<String>,
    #[serde(default)]
    pub configured: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelRuntimeStatus {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub last_error: Option<String>,
    #[serde(default)]
    pub active_sessions: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsStatusResponse {
    #[serde(default)]
    pub telegram: ChannelRuntimeStatus,
    #[serde(default)]
    pub discord: ChannelRuntimeStatus,
    #[serde(default)]
    pub slack: ChannelRuntimeStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramChannelConfigView {
    #[serde(default)]
    pub has_token: bool,
    #[serde(default)]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub mention_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DiscordChannelConfigView {
    #[serde(default)]
    pub has_token: bool,
    #[serde(default)]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub mention_only: bool,
    #[serde(default)]
    pub guild_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SlackChannelConfigView {
    #[serde(default)]
    pub has_token: bool,
    #[serde(default)]
    pub allowed_users: Vec<String>,
    #[serde(default)]
    pub channel_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ChannelsConfigResponse {
    #[serde(default)]
    pub telegram: TelegramChannelConfigView,
    #[serde(default)]
    pub discord: DiscordChannelConfigView,
    #[serde(default)]
    pub slack: SlackChannelConfigView,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderCatalogResponse {
    #[serde(default)]
    all: Vec<ProviderCatalogEntry>,
    #[serde(default)]
    connected: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderCatalogEntry {
    id: String,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    models: HashMap<String, ProviderCatalogModel>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderCatalogModel {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    limit: Option<ProviderCatalogLimit>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderCatalogLimit {
    #[serde(default)]
    context: Option<u32>,
}

/// Todo item from OpenCode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    pub status: String, // "pending" | "in_progress" | "completed" | "cancelled"
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutineSchedule {
    IntervalSeconds { seconds: u64 },
    Cron { expression: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "type")]
pub enum RoutineMisfirePolicy {
    Skip,
    RunOnce,
    CatchUp { max_runs: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RoutineStatus {
    Active,
    Paused,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub creator_type: String,
    pub creator_id: String,
    pub requires_approval: bool,
    pub external_integrations_allowed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_fired_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineHistoryEvent {
    pub routine_id: String,
    pub trigger_type: String,
    pub run_count: u32,
    pub fired_at_ms: u64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineCreateRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routine_id: Option<String>,
    pub name: String,
    pub schedule: RoutineSchedule,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub misfire_policy: Option<RoutineMisfirePolicy>,
    pub entrypoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creator_type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creator_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_approval: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_integrations_allowed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoutinePatchRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<RoutineStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule: Option<RoutineSchedule>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub misfire_policy: Option<RoutineMisfirePolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entrypoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires_approval: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_integrations_allowed: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_fire_at_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoutineRunNowRequest {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run_count: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutineRunNowResponse {
    pub ok: bool,
    pub status: String,
    #[serde(rename = "routineID")]
    pub routine_id: String,
    #[serde(rename = "runCount")]
    pub run_count: u32,
    #[serde(rename = "firedAtMs", default, skip_serializing_if = "Option::is_none")]
    pub fired_at_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct RoutineListResponse {
    routines: Vec<RoutineSpec>,
}

#[derive(Debug, Deserialize)]
struct RoutineRecordResponse {
    routine: RoutineSpec,
}

#[derive(Debug, Deserialize)]
struct RoutineDeleteResponse {
    deleted: bool,
}

#[derive(Debug, Deserialize)]
struct RoutineHistoryResponse {
    events: Vec<RoutineHistoryEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionStatus {
    Draft,
    Running,
    Paused,
    Succeeded,
    Failed,
    Canceled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkItemStatus {
    Todo,
    InProgress,
    Blocked,
    Review,
    Test,
    Rework,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MissionBudget {
    #[serde(default)]
    pub max_steps: Option<u32>,
    #[serde(default)]
    pub max_tool_calls: Option<u32>,
    #[serde(default)]
    pub max_duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct MissionCapabilities {
    #[serde(default)]
    pub allowed_tools: Vec<String>,
    #[serde(default)]
    pub allowed_agents: Vec<String>,
    #[serde(default)]
    pub allowed_memory_tiers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionWorkItem {
    pub work_item_id: String,
    pub title: String,
    #[serde(default)]
    pub detail: Option<String>,
    pub status: WorkItemStatus,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionState {
    pub mission_id: String,
    pub status: MissionStatus,
    pub spec: MissionSpec,
    #[serde(default)]
    pub work_items: Vec<MissionWorkItem>,
    pub revision: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionCreateWorkItem {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_item_id: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assigned_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionCreateRequest {
    pub title: String,
    pub goal: String,
    #[serde(default)]
    pub work_items: Vec<MissionCreateWorkItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MissionApplyEventResult {
    pub mission: MissionState,
    #[serde(default)]
    pub commands: Vec<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct MissionListResponse {
    missions: Vec<MissionState>,
}

#[derive(Debug, Deserialize)]
struct MissionRecordResponse {
    mission: MissionState,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentTeamBudgetLimit {
    #[serde(default)]
    pub max_tokens: Option<u64>,
    #[serde(default)]
    pub max_steps: Option<u32>,
    #[serde(default)]
    pub max_tool_calls: Option<u32>,
    #[serde(default)]
    pub max_duration_ms: Option<u64>,
    #[serde(default)]
    pub max_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTeamTemplate {
    #[serde(rename = "templateID")]
    pub template_id: String,
    pub role: String,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub skills: Vec<serde_json::Value>,
    #[serde(default)]
    pub default_budget: AgentTeamBudgetLimit,
    #[serde(default)]
    pub capabilities: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTeamInstance {
    #[serde(rename = "instanceID")]
    pub instance_id: String,
    #[serde(rename = "missionID")]
    pub mission_id: String,
    #[serde(rename = "parentInstanceID", default)]
    pub parent_instance_id: Option<String>,
    pub role: String,
    #[serde(rename = "templateID")]
    pub template_id: String,
    #[serde(rename = "sessionID")]
    pub session_id: String,
    #[serde(rename = "runID", default)]
    pub run_id: Option<String>,
    pub status: String,
    pub budget: AgentTeamBudgetLimit,
    #[serde(rename = "skillHash")]
    pub skill_hash: String,
    #[serde(default)]
    pub capabilities: serde_json::Value,
    #[serde(default)]
    pub metadata: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTeamMissionSummary {
    #[serde(rename = "missionID")]
    pub mission_id: String,
    #[serde(rename = "instanceCount")]
    pub instance_count: usize,
    #[serde(rename = "runningCount")]
    pub running_count: usize,
    #[serde(rename = "completedCount")]
    pub completed_count: usize,
    #[serde(rename = "failedCount")]
    pub failed_count: usize,
    #[serde(rename = "cancelledCount")]
    pub cancelled_count: usize,
    #[serde(rename = "queuedCount")]
    pub queued_count: usize,
    #[serde(rename = "tokenUsedTotal")]
    pub token_used_total: u64,
    #[serde(rename = "toolCallsUsedTotal")]
    pub tool_calls_used_total: u64,
    #[serde(rename = "stepsUsedTotal")]
    pub steps_used_total: u64,
    #[serde(rename = "costUsedUsdTotal")]
    pub cost_used_usd_total: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTeamSpawnApproval {
    #[serde(rename = "approvalID")]
    pub approval_id: String,
    #[serde(rename = "createdAtMs")]
    pub created_at_ms: u64,
    pub request: serde_json::Value,
    #[serde(rename = "decisionCode", default)]
    pub decision_code: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentTeamApprovals {
    #[serde(rename = "spawnApprovals", default)]
    pub spawn_approvals: Vec<AgentTeamSpawnApproval>,
    #[serde(rename = "toolApprovals", default)]
    pub tool_approvals: Vec<AgentTeamToolApproval>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTeamToolApproval {
    #[serde(rename = "approvalID")]
    pub approval_id: String,
    #[serde(rename = "sessionID", default)]
    pub session_id: Option<String>,
    #[serde(rename = "toolCallID")]
    pub tool_call_id: String,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentTeamInstancesQuery {
    #[serde(rename = "missionID", default)]
    pub mission_id: Option<String>,
    #[serde(rename = "parentInstanceID", default)]
    pub parent_instance_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTeamSpawnRequest {
    #[serde(rename = "missionID", default)]
    pub mission_id: Option<String>,
    #[serde(rename = "parentInstanceID", default)]
    pub parent_instance_id: Option<String>,
    #[serde(rename = "templateID", default)]
    pub template_id: Option<String>,
    pub role: String,
    #[serde(default)]
    pub source: Option<String>,
    pub justification: String,
    #[serde(default)]
    pub budget_override: Option<AgentTeamBudgetLimit>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTeamSpawnResult {
    pub ok: bool,
    #[serde(rename = "missionID", default)]
    pub mission_id: Option<String>,
    #[serde(rename = "instanceID", default)]
    pub instance_id: Option<String>,
    #[serde(rename = "sessionID", default)]
    pub session_id: Option<String>,
    #[serde(rename = "runID", default)]
    pub run_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(rename = "skillHash", default)]
    pub skill_hash: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(rename = "requiresUserApproval", default)]
    pub requires_user_approval: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentTeamCancelRequest {
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTeamDecisionResult {
    pub ok: bool,
    #[serde(rename = "approvalID", default)]
    pub approval_id: Option<String>,
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(rename = "instanceID", default)]
    pub instance_id: Option<String>,
    #[serde(rename = "sessionID", default)]
    pub session_id: Option<String>,
    #[serde(rename = "missionID", default)]
    pub mission_id: Option<String>,
    #[serde(default)]
    pub status: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub code: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[derive(Debug, Deserialize)]
struct AgentTeamTemplatesResponse {
    templates: Vec<AgentTeamTemplate>,
}

#[derive(Debug, Deserialize)]
struct AgentTeamInstancesResponse {
    instances: Vec<AgentTeamInstance>,
}

#[derive(Debug, Deserialize)]
struct AgentTeamMissionSummariesResponse {
    missions: Vec<AgentTeamMissionSummary>,
}

#[derive(Debug, Deserialize)]
struct AgentTeamApprovalsResponse {
    #[serde(rename = "spawnApprovals", default)]
    spawn_approvals: Vec<AgentTeamSpawnApproval>,
    #[serde(rename = "toolApprovals", default)]
    tool_approvals: Vec<AgentTeamToolApproval>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveRunStatusResponse {
    #[serde(rename = "runID")]
    pub run_id: String,
    #[serde(rename = "startedAtMs")]
    pub started_at_ms: u64,
    #[serde(rename = "lastActivityAtMs")]
    pub last_activity_at_ms: u64,
    #[serde(rename = "clientID")]
    pub client_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct ActiveRunEnvelope {
    active: Option<ActiveRunStatusResponse>,
}

#[derive(Debug, Clone, Serialize)]
struct SkillsImportRequest {
    location: SkillLocation,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_or_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    namespace: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    conflict_policy: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct SkillsDeleteQuery<'a> {
    location: &'a str,
}

#[derive(Debug, Clone, Serialize)]
struct SkillsTemplateInstallRequest {
    location: SkillLocation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsImportPreviewItem {
    pub source: String,
    pub valid: bool,
    pub name: Option<String>,
    pub description: Option<String>,
    pub conflict: bool,
    pub action: String,
    pub target_path: Option<String>,
    pub error: Option<String>,
    pub version: Option<String>,
    pub author: Option<String>,
    pub tags: Vec<String>,
    pub requires: Vec<String>,
    pub compatibility: Option<String>,
    pub triggers: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsImportPreview {
    pub items: Vec<SkillsImportPreviewItem>,
    pub total: usize,
    pub valid: usize,
    pub invalid: usize,
    pub conflicts: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillsImportResult {
    pub imported: Vec<SkillInfo>,
    pub skipped: Vec<String>,
    pub errors: Vec<String>,
}

// ============================================================================
// Sidecar Manager
// ============================================================================

/// Main sidecar manager
pub struct SidecarManager {
    config: RwLock<SidecarConfig>,
    state: RwLock<SidecarState>,
    /// Serializes start/stop lifecycle transitions to prevent duplicate spawns.
    lifecycle_lock: Mutex<()>,
    process: Mutex<Option<Child>>,
    owns_process: RwLock<bool>,
    #[cfg(windows)]
    windows_job: Mutex<Option<WindowsJobHandle>>,
    circuit_breaker: Mutex<CircuitBreaker>,
    port: RwLock<Option<u16>>,
    binary_path: RwLock<Option<String>>,
    http_client: Client,
    /// HTTP client without global timeout for long-lived streams
    stream_client: Client,
    /// Environment variables to pass to OpenCode
    env_vars: RwLock<HashMap<String, String>>,
    /// Always-drained stdout/stderr lines from the sidecar (bounded ring buffer).
    log_buffer: Arc<LogRingBuffer>,
    api_token: String,
    api_token_backend: String,
}

impl SidecarManager {
    pub fn new(config: SidecarConfig) -> Self {
        let token_material = load_or_create_engine_api_token();
        let api_token = token_material.token;
        let http_client = build_http_client(config.operation_timeout, &api_token);
        let stream_client = build_stream_client(&api_token);

        Self {
            circuit_breaker: Mutex::new(CircuitBreaker::new(config.clone())),
            config: RwLock::new(config),
            state: RwLock::new(SidecarState::Stopped),
            lifecycle_lock: Mutex::new(()),
            process: Mutex::new(None),
            owns_process: RwLock::new(false),
            #[cfg(windows)]
            windows_job: Mutex::new(None),
            port: RwLock::new(None),
            binary_path: RwLock::new(None),
            http_client,
            stream_client,
            env_vars: RwLock::new(HashMap::new()),
            log_buffer: Arc::new(LogRingBuffer::new(2000)),
            api_token,
            api_token_backend: token_material.backend,
        }
    }

    pub fn sidecar_logs_snapshot(&self, last_n: usize) -> (Vec<(u64, String)>, u64) {
        let lines = self
            .log_buffer
            .snapshot(last_n)
            .into_iter()
            .map(|l| (l.seq, l.text))
            .collect::<Vec<_>>();
        (lines, self.log_buffer.dropped_total())
    }

    pub fn api_token(&self) -> String {
        self.api_token.clone()
    }

    pub fn api_token_path(&self) -> PathBuf {
        engine_api_token_file_path()
    }

    pub fn api_token_backend(&self) -> String {
        self.api_token_backend.clone()
    }

    pub fn sidecar_logs_since(&self, seq: u64) -> (Vec<(u64, String)>, u64) {
        let lines = self
            .log_buffer
            .since(seq)
            .into_iter()
            .map(|l| (l.seq, l.text))
            .collect::<Vec<_>>();
        (lines, self.log_buffer.dropped_total())
    }

    /// Get the current sidecar state
    pub async fn state(&self) -> SidecarState {
        *self.state.read().await
    }

    pub async fn shared_mode(&self) -> bool {
        self.config.read().await.shared_mode
    }

    /// Get the port the sidecar is listening on
    pub async fn port(&self) -> Option<u16> {
        *self.port.read().await
    }

    pub async fn runtime_snapshot(&self) -> SidecarRuntimeSnapshot {
        let state = *self.state.read().await;
        let shared_mode = self.config.read().await.shared_mode;
        let owns_process = *self.owns_process.read().await;
        let port = *self.port.read().await;
        let binary_path = self.binary_path.read().await.clone();
        let pid = self.process.lock().await.as_ref().map(|p| p.id());
        let cb = self.circuit_breaker.lock().await;
        let circuit = SidecarCircuitSnapshot {
            state: match cb.state {
                CircuitState::Closed => "closed".to_string(),
                CircuitState::Open => "open".to_string(),
                CircuitState::HalfOpen => "half_open".to_string(),
            },
            failure_count: cb.failure_count,
            last_failure_age_ms: cb
                .last_failure
                .map(|inst| inst.elapsed().as_millis() as u64),
        };
        SidecarRuntimeSnapshot {
            state,
            shared_mode,
            owns_process,
            port,
            pid,
            binary_path,
            circuit,
        }
    }

    pub async fn startup_health(&self) -> Result<SidecarStartupHealth> {
        let port = self
            .port()
            .await
            .ok_or_else(|| TandemError::Sidecar("Sidecar port not assigned".to_string()))?;
        let health = self.health_check(port).await?;
        Ok(SidecarStartupHealth {
            healthy: health.healthy,
            ready: health.ready,
            phase: health.phase,
            startup_attempt_id: health.startup_attempt_id,
            startup_elapsed_ms: health.startup_elapsed_ms,
            last_error: health.last_error,
            build_id: health.build_id,
            binary_path: health.binary_path,
        })
    }

    /// Set environment variables for OpenCode
    pub async fn set_env(&self, key: &str, value: &str) {
        let mut env_vars = self.env_vars.write().await;
        env_vars.insert(key.to_string(), value.to_string());
    }

    /// Remove an environment variable for OpenCode
    pub async fn remove_env(&self, key: &str) {
        let mut env_vars = self.env_vars.write().await;
        env_vars.remove(key);
    }

    /// Set the workspace path
    pub async fn set_workspace(&self, path: PathBuf) {
        let mut config = self.config.write().await;
        config.workspace_path = Some(path);
    }

    /// Get the base URL for the sidecar API
    async fn base_url(&self) -> Result<String> {
        let port = self
            .port()
            .await
            .ok_or_else(|| TandemError::Sidecar("Sidecar not running".to_string()))?;
        Ok(format!("http://127.0.0.1:{}", port))
    }

    /// Start the sidecar process
    pub async fn start(&self, sidecar_path: &str) -> Result<()> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;

        {
            let state = self.state.read().await;
            if *state == SidecarState::Running {
                tracing::debug!("Sidecar already running");
                return Ok(());
            }
        }

        {
            let mut state = self.state.write().await;
            *state = SidecarState::Starting;
        }

        tracing::info!("Starting tandem-engine sidecar from: {}", sidecar_path);
        {
            let mut path_guard = self.binary_path.write().await;
            *path_guard = Some(sidecar_path.to_string());
        }

        // Find an available/configured port.
        let port = self.find_available_port().await?;

        // Get config and env vars
        let config = self.config.read().await;
        let env_vars = self.env_vars.read().await;

        // In shared mode, prefer attaching to an already-running engine.
        if config.shared_mode {
            if let Ok(health) = self.health_check(port).await {
                if health.ready {
                    {
                        let mut port_guard = self.port.write().await;
                        *port_guard = Some(port);
                    }
                    {
                        let mut owns_guard = self.owns_process.write().await;
                        *owns_guard = false;
                    }
                    {
                        let mut state = self.state.write().await;
                        *state = SidecarState::Running;
                    }
                    tracing::info!(
                        "Attached to existing tandem-engine sidecar on port {}",
                        port
                    );
                    return Ok(());
                }
                tracing::info!(
                    "Existing tandem-engine on port {} is healthy but not ready yet (phase={} elapsed_ms={})",
                    port,
                    health.phase,
                    health.startup_elapsed_ms
                );
            }
        }

        tracing::debug!(
            "Sidecar env set: OPENROUTER_API_KEY={} OPENCODE_ZEN_API_KEY={} ANTHROPIC_API_KEY={} OPENAI_API_KEY={}",
            env_vars.contains_key("OPENROUTER_API_KEY"),
            env_vars.contains_key("OPENCODE_ZEN_API_KEY"),
            env_vars.contains_key("ANTHROPIC_API_KEY"),
            env_vars.contains_key("OPENAI_API_KEY")
        );
        let has_openrouter_key = env_vars
            .get("OPENROUTER_API_KEY")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);
        let has_openai_key = env_vars
            .get("OPENAI_API_KEY")
            .map(|v| !v.trim().is_empty())
            .unwrap_or(false);

        // Build the command
        let mut cmd = Command::new(sidecar_path);

        // Hide console window on Windows
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        // tandem-engine 'serve' subcommand starts a headless server
        // Use --hostname and --port flags
        cmd.args([
            "serve",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ]);
        if let Ok(paths) = resolve_shared_paths() {
            let state_dir = paths.engine_state_dir.to_string_lossy().to_string();
            cmd.args(["--state-dir", &state_dir]);
        }

        // Set working directory if workspace is configured
        if let Some(ref workspace) = config.workspace_path {
            cmd.current_dir(workspace);
            cmd.env("OPENCODE_DIR", workspace);
        }

        // Ensure sidecar config exists and is updated with dynamic Ollama models.
        //
        // IMPORTANT: Do not overwrite the entire config file; preserve unknown fields so
        // MCP/plugin settings (and any user settings) survive sidecar restarts.
        match crate::tandem_config::global_config_path() {
            Ok(config_path) => {
                // Make sure the sidecar loads the file we're managing, even if its
                // defaults change across versions/platforms.
                cmd.env("OPENCODE_CONFIG", &config_path);

                // Discover local Ollama models dynamically
                let mut models_map = serde_json::Map::new();
                if let Ok(output) = Command::new("ollama").arg("list").output() {
                    if output.status.success() {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        for line in stdout.lines().skip(1) {
                            let parts: Vec<&str> = line.split_whitespace().collect();
                            if !parts.is_empty() {
                                let name = parts[0];
                                let mut model_info = serde_json::Map::new();
                                model_info.insert(
                                    "name".to_string(),
                                    serde_json::Value::String(name.to_string()),
                                );
                                models_map.insert(
                                    name.to_string(),
                                    serde_json::Value::Object(model_info),
                                );
                            }
                        }
                    }
                }

                let models = serde_json::Value::Object(models_map);

                if let Err(e) = crate::tandem_config::update_config_at(&config_path, |cfg| {
                    // Keep sidecar defaults aligned with actual key availability to avoid
                    // silent fallback to OpenAI defaults when only OpenRouter auth is present.
                    if has_openrouter_key && !has_openai_key {
                        if let Some(root) = cfg.as_object_mut() {
                            let should_set_default = match root
                                .get("default_provider")
                                .and_then(|v| v.as_str())
                                .map(|s| s.trim().to_lowercase())
                            {
                                Some(current) => current.is_empty() || current == "openai",
                                None => true,
                            };
                            if should_set_default {
                                root.insert(
                                    "default_provider".to_string(),
                                    serde_json::Value::String("openrouter".to_string()),
                                );
                            }

                            let providers =
                                root.entry("providers".to_string()).or_insert_with(|| {
                                    serde_json::Value::Object(serde_json::Map::new())
                                });
                            if let Some(providers_obj) = providers.as_object_mut() {
                                let openrouter = providers_obj
                                    .entry("openrouter".to_string())
                                    .or_insert_with(|| {
                                        serde_json::Value::Object(serde_json::Map::new())
                                    });
                                if let Some(openrouter_obj) = openrouter.as_object_mut() {
                                    openrouter_obj.entry("url".to_string()).or_insert_with(|| {
                                        serde_json::Value::String(
                                            "https://openrouter.ai/api/v1".to_string(),
                                        )
                                    });
                                }
                            }
                        }
                    }
                    crate::tandem_config::set_provider_ollama_models(cfg, models);
                    Ok(())
                }) {
                    tracing::warn!("Failed to update sidecar config {:?}: {}", config_path, e);
                } else {
                    tracing::info!(
                        "Updated sidecar config with Ollama models at: {:?}",
                        config_path
                    );
                }
            }
            Err(e) => {
                tracing::warn!("Could not determine sidecar config path: {}", e);
            }
        }

        // Pass environment variables (including API keys)
        for (key, value) in env_vars.iter() {
            cmd.env(key, value);
        }
        cmd.env("TANDEM_API_TOKEN", &self.api_token);

        // OPTIMIZATION: Set Bun/JSC memory limits to avoid excessive idle usage
        // (Addresses feedback about 500MB+ idle usage)
        if !env_vars.contains_key("BUN_JSC_forceRAMSize") {
            // Limit to ~256MB to force aggressive GC
            cmd.env("BUN_JSC_forceRAMSize", "268435456");
        }
        if !env_vars.contains_key("BUN_GARBAGE_COLLECTOR_LEVEL") {
            // Hint for more aggressive GC if supported
            cmd.env("BUN_GARBAGE_COLLECTOR_LEVEL", "1");
        }

        // Configure stdio
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Spawn the process
        let mut child = cmd
            .spawn()
            .map_err(|e| TandemError::Sidecar(format!("Failed to spawn sidecar: {}", e)))?;

        // IMPORTANT: Always drain stdout/stderr when we pipe them. If we don't, the sidecar can
        // deadlock when its stdio buffers fill up. We keep a bounded in-memory ring buffer so this
        // remains low overhead but always safe.
        {
            use std::io::{BufRead, BufReader};
            let log_buf = self.log_buffer.clone();

            if let Some(stdout) = child.stdout.take() {
                let log_buf = log_buf.clone();
                std::thread::spawn(move || {
                    let reader = BufReader::new(stdout);
                    for line in reader.lines().flatten() {
                        log_buf.push(format!("STDOUT {line}"));
                    }
                });
            }

            if let Some(stderr) = child.stderr.take() {
                std::thread::spawn(move || {
                    let reader = BufReader::new(stderr);
                    for line in reader.lines().flatten() {
                        log_buf.push(format!("STDERR {line}"));
                    }
                });
            }
        }

        // On Windows, put the sidecar into a Job Object configured to kill child processes when
        // the job handle is closed. This prevents orphaned sidecars during `tauri dev` rebuilds
        // where the app process may be terminated abruptly without running shutdown hooks.
        #[cfg(windows)]
        {
            if !config.shared_mode {
                let pid = child.id();
                let mut job_guard = self.windows_job.lock().await;
                if job_guard.is_none() {
                    match windows_create_kill_on_close_job() {
                        Ok(job) => *job_guard = Some(job),
                        Err(e) => {
                            tracing::warn!(
                                "Failed to create Windows job object for sidecar: {}",
                                e
                            );
                        }
                    }
                }
                if let Some(job) = job_guard.as_ref() {
                    if let Err(e) = windows_try_assign_pid_to_job(job.as_handle(), pid) {
                        // If Tandem itself is running inside a non-breakaway job, Windows may reject
                        // assigning the child to another job. In that case we fall back to best-effort
                        // shutdown hooks and manual cleanup.
                        tracing::warn!(
                            "Failed to assign sidecar PID {} to job object (may orphan on dev reload): {}",
                            pid,
                            e
                        );
                    }
                }
            }
        }

        // Store the process and port
        {
            let mut process_guard = self.process.lock().await;
            *process_guard = Some(child);
        }
        {
            let mut owns_guard = self.owns_process.write().await;
            *owns_guard = true;
        }
        {
            let mut port_guard = self.port.write().await;
            *port_guard = Some(port);
        }

        // Wait for sidecar to be ready
        match self.wait_for_ready(port).await {
            Ok(_) => {
                let mut state = self.state.write().await;
                *state = SidecarState::Running;
                tracing::info!("tandem-engine sidecar started on port {}", port);
                Ok(())
            }
            Err(e) => {
                // Clean up on failure without re-entering lifecycle lock.
                let child = {
                    let mut process_guard = self.process.lock().await;
                    process_guard.take()
                };
                if let Some(mut child) = child {
                    #[cfg(windows)]
                    {
                        use std::process::Command as StdCommand;
                        let pid = child.id();
                        let mut cmd = StdCommand::new("taskkill");
                        cmd.args(["/F", "/T", "/PID", &pid.to_string()]);
                        let _ = cmd.output();
                    }
                    #[cfg(not(windows))]
                    {
                        let _ = child.kill();
                    }
                    let _ = child.wait();
                }
                {
                    let mut port_guard = self.port.write().await;
                    *port_guard = None;
                }
                {
                    let mut owns_guard = self.owns_process.write().await;
                    *owns_guard = false;
                }
                #[cfg(windows)]
                {
                    let mut job_guard = self.windows_job.lock().await;
                    *job_guard = None;
                }
                let mut state = self.state.write().await;
                *state = SidecarState::Failed;
                Err(e)
            }
        }
    }

    /// Stop the sidecar process
    pub async fn stop(&self) -> Result<()> {
        let _lifecycle_guard = self.lifecycle_lock.lock().await;

        {
            let state = self.state.read().await;
            if *state == SidecarState::Stopped {
                return Ok(());
            }
        }

        {
            let mut state = self.state.write().await;
            *state = SidecarState::Stopping;
        }

        tracing::info!("Stopping tandem-engine sidecar");

        let shared_mode = self.config.read().await.shared_mode;
        let owns_process = *self.owns_process.read().await;

        // Shared mode: detach only when this client does not own the process.
        // If we own it, continue to the normal kill path below so we don't leave
        // orphaned/stuck sidecars behind.
        if shared_mode && !owns_process {
            {
                let mut process_guard = self.process.lock().await;
                let _ = process_guard.take();
            }
            {
                let mut port_guard = self.port.write().await;
                *port_guard = None;
            }
            {
                let mut owns_guard = self.owns_process.write().await;
                *owns_guard = false;
            }
            {
                let mut state = self.state.write().await;
                *state = SidecarState::Stopped;
            }
            tracing::info!("Detached from shared tandem-engine sidecar");
            return Ok(());
        }

        // If this manager didn't spawn the process, detach only.
        if !owns_process {
            {
                let mut process_guard = self.process.lock().await;
                let _ = process_guard.take();
            }
            {
                let mut port_guard = self.port.write().await;
                *port_guard = None;
            }
            {
                let mut state = self.state.write().await;
                *state = SidecarState::Stopped;
            }
            tracing::info!("Detached from external tandem-engine sidecar");
            return Ok(());
        }

        // Kill the process
        let child = {
            let mut process_guard = self.process.lock().await;
            process_guard.take()
        };

        if let Some(mut child) = child {
            #[cfg(windows)]
            {
                // On Windows, try graceful termination first, then force kill
                use std::process::Command as StdCommand;
                let pid = child.id();
                tracing::info!("Killing tandem-engine process with PID {}", pid);

                // Try taskkill /T to terminate child processes too
                let mut cmd = StdCommand::new("taskkill");
                cmd.args(["/F", "/T", "/PID", &pid.to_string()]);

                // Hide console window on Windows
                #[cfg(target_os = "windows")]
                {
                    use std::os::windows::process::CommandExt;
                    const CREATE_NO_WINDOW: u32 = 0x08000000;
                    cmd.creation_flags(CREATE_NO_WINDOW);
                }

                let result = cmd.output();

                match result {
                    Ok(output) => {
                        tracing::info!(
                            "taskkill result: {}",
                            String::from_utf8_lossy(&output.stdout)
                        );
                        if !output.status.success() {
                            tracing::warn!(
                                "taskkill stderr: {}",
                                String::from_utf8_lossy(&output.stderr)
                            );
                        }
                    }
                    Err(e) => tracing::error!("Failed to run taskkill: {}", e),
                }

                // Wait a moment for the process to fully terminate
                tokio::time::sleep(Duration::from_millis(500)).await;
            }

            #[cfg(not(windows))]
            {
                let _ = child.kill();
            }

            // Wait for the process to exit
            let _ = child.wait();
        }

        // Drop the job handle last (kills any straggling descendants).
        #[cfg(windows)]
        {
            let mut job_guard = self.windows_job.lock().await;
            *job_guard = None;
        }

        // Give extra time for Windows to release file handles
        #[cfg(windows)]
        tokio::time::sleep(Duration::from_millis(500)).await;

        // Clear the port
        {
            let mut port_guard = self.port.write().await;
            *port_guard = None;
        }

        {
            let mut state = self.state.write().await;
            *state = SidecarState::Stopped;
        }
        {
            let mut owns_guard = self.owns_process.write().await;
            *owns_guard = false;
        }

        tracing::info!("tandem-engine sidecar stopped");
        Ok(())
    }

    /// Restart the sidecar
    pub async fn restart(&self, sidecar_path: &str) -> Result<()> {
        self.stop().await?;
        tokio::time::sleep(Duration::from_millis(500)).await;
        self.start(sidecar_path).await
    }

    /// Find an available port
    async fn find_available_port(&self) -> Result<u16> {
        let preferred_port = self.config.read().await.port;
        if preferred_port != 0 {
            // Prefer configured port, but gracefully fall back if it is unavailable.
            if std::net::TcpListener::bind(("127.0.0.1", preferred_port)).is_ok() {
                return Ok(preferred_port);
            }
            tracing::warn!(
                "Configured sidecar port {} is unavailable; falling back to an ephemeral port",
                preferred_port
            );
        }

        // Find a random available port
        let listener = std::net::TcpListener::bind("127.0.0.1:0")
            .map_err(|e| TandemError::Sidecar(format!("Failed to find available port: {}", e)))?;

        let port = listener
            .local_addr()
            .map_err(|e| TandemError::Sidecar(format!("Failed to get port: {}", e)))?
            .port();

        drop(listener);
        Ok(port)
    }

    /// Wait for the sidecar to be ready
    async fn wait_for_ready(&self, port: u16) -> Result<()> {
        let start = Instant::now();
        let timeout = self.startup_wait_timeout();

        tracing::debug!("Waiting for sidecar to be ready on port {}", port);
        emit_event(
            tracing::Level::INFO,
            ProcessKind::Desktop,
            ObservabilityEvent {
                event: "sidecar.wait.start",
                component: "sidecar",
                correlation_id: None,
                session_id: None,
                run_id: None,
                message_id: None,
                provider_id: None,
                model_id: None,
                status: Some("start"),
                error_code: None,
                detail: Some(&format!("port={} timeout_ms={}", port, timeout.as_millis())),
            },
        );

        let mut last_error = String::new();
        let mut last_health: Option<SidecarHealthResponse> = None;
        let mut last_progress_emit = Instant::now() - Duration::from_secs(5);
        while start.elapsed() < timeout {
            // If the child exited before becoming healthy, fail fast with useful logs.
            {
                let mut process_guard = self.process.lock().await;
                if let Some(child) = process_guard.as_mut() {
                    match child.try_wait() {
                        Ok(Some(status)) => {
                            let tail = self
                                .log_buffer
                                .snapshot(40)
                                .into_iter()
                                .map(|l| l.text)
                                .collect::<Vec<_>>()
                                .join("\n");
                            let detail = if tail.trim().is_empty() {
                                format!("sidecar process exited early with status {}", status)
                            } else {
                                format!(
                                    "sidecar process exited early with status {}\nrecent logs:\n{}",
                                    status, tail
                                )
                            };
                            return Err(TandemError::Sidecar(detail));
                        }
                        Ok(None) => {}
                        Err(e) => {
                            tracing::warn!("Failed to query sidecar process status: {}", e);
                        }
                    }
                }
            }

            match self.health_check(port).await {
                Ok(health) if health.ready => {
                    tracing::info!("Sidecar is ready after {:?}", start.elapsed());
                    emit_event(
                        tracing::Level::INFO,
                        ProcessKind::Desktop,
                        ObservabilityEvent {
                            event: "sidecar.wait.ready",
                            component: "sidecar",
                            correlation_id: None,
                            session_id: None,
                            run_id: None,
                            message_id: None,
                            provider_id: None,
                            model_id: None,
                            status: Some("ok"),
                            error_code: None,
                            detail: Some(&format!(
                                "port={} elapsed_ms={}",
                                port,
                                start.elapsed().as_millis()
                            )),
                        },
                    );
                    return Ok(());
                }
                Ok(health) => {
                    last_error = format!(
                        "Engine starting: phase={} attempt_id={} elapsed_ms={}{}",
                        health.phase,
                        health.startup_attempt_id,
                        health.startup_elapsed_ms,
                        health
                            .last_error
                            .as_ref()
                            .map(|e| format!(" last_error={}", e))
                            .unwrap_or_default()
                    );
                    last_health = Some(health.clone());
                    if last_progress_emit.elapsed() >= Duration::from_secs(3) {
                        emit_event(
                            tracing::Level::INFO,
                            ProcessKind::Desktop,
                            ObservabilityEvent {
                                event: "sidecar.wait.progress",
                                component: "sidecar",
                                correlation_id: None,
                                session_id: None,
                                run_id: None,
                                message_id: None,
                                provider_id: None,
                                model_id: None,
                                status: Some("starting"),
                                error_code: None,
                                detail: Some(&format!(
                                    "port={} phase={} attempt_id={} elapsed_ms={}",
                                    port,
                                    health.phase,
                                    health.startup_attempt_id,
                                    health.startup_elapsed_ms
                                )),
                            },
                        );
                        last_progress_emit = Instant::now();
                    }
                }
                Err(e) => {
                    last_error = e.to_string();
                    tracing::trace!("Health check failed: {}, retrying...", e);
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        if let Some(health) = &last_health {
            last_error = format!(
                "{}; final_health={{ready:{},phase:{},attempt_id:{},elapsed_ms:{},last_error:{}}}",
                last_error,
                health.ready,
                health.phase,
                health.startup_attempt_id,
                health.startup_elapsed_ms,
                health.last_error.clone().unwrap_or_default()
            );
        }
        emit_event(
            tracing::Level::WARN,
            ProcessKind::Desktop,
            ObservabilityEvent {
                event: "sidecar.wait.timeout",
                component: "sidecar",
                correlation_id: None,
                session_id: None,
                run_id: None,
                message_id: None,
                provider_id: None,
                model_id: None,
                status: Some("timeout"),
                error_code: Some("ENGINE_START_TIMEOUT"),
                detail: Some(&format!(
                    "port={} timeout_ms={} last_error={}",
                    port,
                    timeout.as_millis(),
                    last_error
                )),
            },
        );
        tracing::error!("Sidecar failed to start. Last error: {}", last_error);
        Err(TandemError::Sidecar(format!(
            "Sidecar failed to start within {}s timeout. Last error: {}",
            timeout.as_secs(),
            last_error
        )))
    }

    /// Health check for the sidecar
    /// tandem-engine exposes /global/health endpoint that returns JSON
    async fn health_check(&self, port: u16) -> Result<SidecarHealthResponse> {
        let url = format!("http://127.0.0.1:{}/global/health", port);

        let response = self
            .http_client
            .get(&url)
            // Keep connect/read timeout short so startup retries remain responsive even when
            // localhost connect attempts linger in SYN_SENT on Windows.
            .timeout(Duration::from_secs(5))
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Health check request failed: {}", e)))?;

        if response.status().is_success() {
            let body = response
                .json::<SidecarHealthResponse>()
                .await
                .map_err(|e| {
                    TandemError::Sidecar(format!("Health check returned invalid JSON: {}", e))
                })?;

            if body.healthy {
                tracing::debug!(
                    "tandem-engine health check passed: ready={} phase={} attempt_id={} elapsed_ms={}",
                    body.ready,
                    body.phase,
                    body.startup_attempt_id,
                    body.startup_elapsed_ms
                );
                Ok(body)
            } else {
                Err(TandemError::Sidecar(format!(
                    "Health check returned unhealthy: ready={} phase={} attempt_id={} elapsed_ms={}",
                    body.ready,
                    body.phase,
                    body.startup_attempt_id,
                    body.startup_elapsed_ms
                )))
            }
        } else {
            Err(TandemError::Sidecar(format!(
                "Health check returned status: {}",
                response.status()
            )))
        }
    }

    fn startup_wait_timeout(&self) -> Duration {
        let normal = Duration::from_secs(120);
        let extended = Duration::from_secs(240);
        let force_repair = std::env::var("TANDEM_FORCE_LEGACY_REPAIR")
            .map(|v| {
                let lower = v.trim().to_ascii_lowercase();
                matches!(lower.as_str(), "1" | "true" | "yes" | "on")
            })
            .unwrap_or(false);
        if force_repair {
            return extended;
        }

        if let Ok(paths) = resolve_shared_paths() {
            let marker = paths
                .engine_state_dir
                .join("storage")
                .join("legacy_import_marker.json");
            if !marker.exists() {
                return extended;
            }
        }
        normal
    }

    // ========================================================================
    // Session Management
    // ========================================================================

    /// Create a new chat session
    /// OpenCode API: POST /session
    pub async fn create_session(&self, request: CreateSessionRequest) -> Result<Session> {
        self.check_circuit_breaker().await?;

        let base = self.base_url().await?;
        let url = format!("{}/session", base);
        let fallback_url = format!("{}/api/session", base);
        tracing::debug!("Creating session at: {}", url);

        const ENGINE_STARTUP_RETRIES: usize = 10;
        const ENGINE_STARTUP_RETRY_MS: u64 = 450;
        let is_engine_starting = |text: &str| {
            text.contains("ENGINE_STARTING")
                || text.contains("Engine starting")
                || text.contains("Service Unavailable")
        };

        let mut last_error = String::new();
        for attempt in 0..=ENGINE_STARTUP_RETRIES {
            let response = self
                .http_client
                .post(&url)
                .json(&request)
                .send()
                .await
                .map_err(|e| TandemError::Sidecar(format!("Failed to create session: {}", e)))?;

            if response.status().is_success() {
                self.record_success().await;
                let body = response.text().await.map_err(|e| {
                    TandemError::Sidecar(format!("Failed to read response body: {}", e))
                })?;
                let session: Session = serde_json::from_str(&body).map_err(|e| {
                    TandemError::Sidecar(format!(
                        "Failed to parse response: {}. Body: {}",
                        e,
                        &body[..body.len().min(200)]
                    ))
                })?;
                return Ok(session);
            }

            tracing::warn!(
                "create_session failed on primary URL {}, retrying via {}",
                url,
                fallback_url
            );
            let fallback = self
                .http_client
                .post(&fallback_url)
                .json(&request)
                .send()
                .await
                .map_err(|e| TandemError::Sidecar(format!("Failed to create session: {}", e)))?;

            if fallback.status().is_success() {
                self.record_success().await;
                let body = fallback.text().await.map_err(|e| {
                    TandemError::Sidecar(format!("Failed to read response body: {}", e))
                })?;
                let session: Session = serde_json::from_str(&body).map_err(|e| {
                    TandemError::Sidecar(format!(
                        "Failed to parse response: {}. Body: {}",
                        e,
                        &body[..body.len().min(200)]
                    ))
                })?;
                return Ok(session);
            }

            let status = fallback.status();
            let body = fallback.text().await.unwrap_or_default();
            last_error = format!("Failed to create session: {} {}", status, body);

            if is_engine_starting(&body) && attempt < ENGINE_STARTUP_RETRIES {
                tracing::warn!(
                    "Engine still starting during create_session (attempt {}/{}). Retrying in {}ms.",
                    attempt + 1,
                    ENGINE_STARTUP_RETRIES + 1,
                    ENGINE_STARTUP_RETRY_MS
                );
                tokio::time::sleep(Duration::from_millis(ENGINE_STARTUP_RETRY_MS)).await;
                continue;
            }

            if !is_engine_starting(&body) {
                self.record_failure().await;
            }
            return Err(TandemError::Sidecar(last_error));
        }

        Err(TandemError::Sidecar(if last_error.is_empty() {
            "Failed to create session: engine did not become ready in time".to_string()
        } else {
            last_error
        }))
    }

    /// Get a session by ID
    /// OpenCode API: GET /session/{id}
    pub async fn get_session(&self, session_id: &str) -> Result<Session> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/session/{}", self.base_url().await?, session_id);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to get session: {}", e)))?;

        let session: Session = self.handle_response(response).await?;
        Ok(normalize_session_for_workspace(
            session,
            self.workspace_directory().await.as_deref(),
        ))
    }

    /// List all sessions
    /// OpenCode API: GET /session
    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        self.check_circuit_breaker().await?;

        let base_url = self.base_url().await?;
        let workspace_directory = self.workspace_directory().await;
        let url = format!("{}/session", base_url);
        let mut req = self.http_client.get(&url).query(&[("scope", "workspace")]);
        if let Some(directory) = workspace_directory.as_deref() {
            req = req.query(&[("workspace", directory)]);
        }

        let response = match req.send().await {
            Ok(resp) => resp,
            Err(primary_err) => {
                // Compatibility fallback for older sidecar builds that do not support scope/workspace.
                let mut compat_req = self.http_client.get(&url).query(&[("roots", "true")]);
                if let Some(directory) = workspace_directory.as_deref() {
                    compat_req = compat_req.query(&[("directory", directory)]);
                }
                compat_req.send().await.map_err(|compat_err| {
                    TandemError::Sidecar(format!(
                        "Failed to list sessions (primary: {}; compat: {})",
                        primary_err, compat_err
                    ))
                })?
            }
        };

        let raw: serde_json::Value = self.handle_response(response).await?;
        parse_sessions_response(raw)
            .map(|sessions| {
                normalize_sessions_for_workspace(sessions, workspace_directory.as_deref())
            })
            .ok_or_else(|| {
                TandemError::Sidecar("Failed to parse sessions response shape".to_string())
            })
    }

    /// Delete a session
    /// OpenCode API: DELETE /session/{id}
    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/session/{}", self.base_url().await?, session_id);

        let response = self
            .http_client
            .delete(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to delete session: {}", e)))?;

        if response.status().is_success() {
            self.record_success().await;
            Ok(())
        } else {
            self.record_failure().await;
            Err(TandemError::Sidecar(format!(
                "Failed to delete session: {}",
                response.status()
            )))
        }
    }

    /// List all projects
    /// OpenCode API: GET /project
    pub async fn list_projects(&self) -> Result<Vec<Project>> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/project", self.base_url().await?);

        let mut req = self.http_client.get(&url);
        if let Some(directory) = self.workspace_directory().await {
            req = req.query(&[("directory", directory)]);
        }
        let response = req
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list projects: {}", e)))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to read projects body: {}", e)))?;

        if !status.is_success() {
            self.record_failure().await;
            return Err(TandemError::Sidecar(format!(
                "Failed to list projects ({}): {}",
                status, body
            )));
        }

        self.record_success().await;

        let value: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
            TandemError::Sidecar(format!(
                "Failed to parse projects payload: {}. Body: {}",
                e,
                &body[..body.len().min(200)]
            ))
        })?;

        let mut projects = Vec::new();
        if let Some(items) = value.as_array() {
            for item in items {
                if let Some(path) = item.as_str() {
                    projects.push(Project {
                        id: path.to_string(),
                        worktree: path.to_string(),
                        vcs: None,
                        sandboxes: Vec::new(),
                        time: ProjectTime {
                            created: 0,
                            updated: 0,
                        },
                    });
                    continue;
                }
                if let Ok(project) = serde_json::from_value::<Project>(item.clone()) {
                    projects.push(project);
                    continue;
                }
                if let Some(obj) = item.as_object() {
                    let worktree = obj
                        .get("worktree")
                        .and_then(|v| v.as_str())
                        .or_else(|| obj.get("directory").and_then(|v| v.as_str()))
                        .unwrap_or(".")
                        .to_string();
                    let id = obj
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&worktree)
                        .to_string();
                    projects.push(Project {
                        id,
                        worktree,
                        vcs: obj.get("vcs").and_then(|v| v.as_str()).map(str::to_string),
                        sandboxes: obj
                            .get("sandboxes")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default(),
                        time: ProjectTime {
                            created: obj
                                .get("time")
                                .and_then(|t| t.get("created"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0),
                            updated: obj
                                .get("time")
                                .and_then(|t| t.get("updated"))
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0),
                        },
                    });
                }
            }
        }

        Ok(projects)
    }

    /// Get messages for a session
    /// OpenCode API: GET /session/{id}/message
    pub async fn get_session_messages(&self, session_id: &str) -> Result<Vec<SessionMessage>> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/session/{}/message", self.base_url().await?, session_id);

        let response =
            self.http_client.get(&url).send().await.map_err(|e| {
                TandemError::Sidecar(format!("Failed to get session messages: {}", e))
            })?;

        self.handle_response(response).await
    }

    /// Get todos for a session
    /// OpenCode API: GET /session/{id}/todo
    pub async fn get_session_todos(&self, session_id: &str) -> Result<Vec<TodoItem>> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/session/{}/todo", self.base_url().await?, session_id);
        tracing::debug!("Fetching todos from: {}", url);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to get session todos: {}", e)))?;

        tracing::debug!("Todos API response status: {}", response.status());

        let todos: Vec<TodoItem> = self.handle_response(response).await?;
        tracing::debug!("Fetched {} todos for session {}", todos.len(), session_id);

        Ok(todos)
    }

    // ========================================================================
    // Message Handling
    // ========================================================================

    /// Send a message to a session (async, non-blocking)
    /// OpenCode API: POST /session/{id}/prompt_async
    /// Returns 204 No Content - actual response comes via /event SSE stream
    pub async fn append_message_and_start_run(
        &self,
        session_id: &str,
        request: SendMessageRequest,
    ) -> Result<()> {
        self.append_message_and_start_run_with_context(session_id, request, None)
            .await
    }

    pub async fn append_message_and_start_run_with_context(
        &self,
        session_id: &str,
        request: SendMessageRequest,
        correlation_id: Option<&str>,
    ) -> Result<()> {
        self.check_circuit_breaker().await?;

        let base = self.base_url().await?;
        let append_url = format!("{}/session/{}/message?mode=append", base, session_id);
        let append_fallback_url =
            format!("{}/api/session/{}/message?mode=append", base, session_id);
        let url = format!("{}/session/{}/prompt_async?return=run", base, session_id);
        let fallback_url = format!(
            "{}/api/session/{}/prompt_async?return=run",
            base, session_id
        );

        if let Some(model) = &request.model {
            tracing::debug!(
                "Sending prompt to sidecar (session {}): provider={} model={}",
                session_id,
                model.provider_id,
                model.model_id
            );
        } else {
            tracing::debug!(
                "Sending prompt to sidecar (session {}) without explicit model spec",
                session_id
            );
        }

        tracing::debug!("Sending prompt to: {} with {:?}", url, request);

        const ENGINE_STARTUP_RETRIES: usize = 10;
        const ENGINE_STARTUP_RETRY_MS: u64 = 450;

        let is_engine_starting = |text: &str| {
            text.contains("ENGINE_STARTING")
                || text.contains("Engine starting")
                || text.contains("Service Unavailable")
        };

        // Phase 1: append message. Retry transient startup failures before surfacing error.
        let mut append_ok = false;
        let mut last_append_error = String::new();
        for attempt in 0..=ENGINE_STARTUP_RETRIES {
            let mut append_builder = self.http_client.post(&append_url);
            if let Some(cid) = correlation_id {
                append_builder = append_builder
                    .header("x-tandem-correlation-id", cid)
                    .header("x-tandem-session-id", session_id);
            }
            let append_response =
                append_builder.json(&request).send().await.map_err(|e| {
                    TandemError::Sidecar(format!("Failed to append message: {}", e))
                })?;

            if append_response.status().is_success() {
                append_ok = true;
                break;
            }

            tracing::warn!(
                "append message failed on primary URL {}, retrying via {}",
                append_url,
                append_fallback_url
            );
            let mut append_fallback_builder = self.http_client.post(&append_fallback_url);
            if let Some(cid) = correlation_id {
                append_fallback_builder = append_fallback_builder
                    .header("x-tandem-correlation-id", cid)
                    .header("x-tandem-session-id", session_id);
            }
            let append_fallback_response = append_fallback_builder
                .json(&request)
                .send()
                .await
                .map_err(|e| TandemError::Sidecar(format!("Failed to append message: {}", e)))?;

            if append_fallback_response.status().is_success() {
                append_ok = true;
                break;
            }

            let status = append_fallback_response.status();
            let body = append_fallback_response.text().await.unwrap_or_default();
            last_append_error = format!("Failed to append message: {} {}", status, body);

            if is_engine_starting(&body) && attempt < ENGINE_STARTUP_RETRIES {
                tracing::warn!(
                    "Engine still starting during append (attempt {}/{}). Retrying in {}ms.",
                    attempt + 1,
                    ENGINE_STARTUP_RETRIES + 1,
                    ENGINE_STARTUP_RETRY_MS
                );
                tokio::time::sleep(Duration::from_millis(ENGINE_STARTUP_RETRY_MS)).await;
                continue;
            }

            if !is_engine_starting(&body) {
                self.record_failure().await;
            }
            return Err(TandemError::Sidecar(last_append_error));
        }

        if !append_ok {
            return Err(TandemError::Sidecar(if last_append_error.is_empty() {
                "Failed to append message: engine did not become ready in time".to_string()
            } else {
                last_append_error
            }));
        }

        // Phase 2: start run. Retry ENGINE_STARTING responses; do not append again.
        let mut last_send_error: Option<String> = None;
        for attempt in 0..=ENGINE_STARTUP_RETRIES {
            let mut request_builder = self.http_client.post(&url);
            if let Some(cid) = correlation_id {
                request_builder = request_builder
                    .header("x-tandem-correlation-id", cid)
                    .header("x-tandem-session-id", session_id);
            }
            let response = request_builder
                .json(&request)
                .send()
                .await
                .map_err(|e| TandemError::Sidecar(format!("Failed to send message: {}", e)))?;

            let outcome = match Self::handle_prompt_async_response(response).await {
                Ok(()) => Ok(()),
                Err(err) if err.starts_with("html_response") => {
                    tracing::warn!(
                        "prompt_async returned HTML from {}, retrying via {}",
                        url,
                        fallback_url
                    );
                    let mut fallback_builder = self.http_client.post(&fallback_url);
                    if let Some(cid) = correlation_id {
                        fallback_builder = fallback_builder
                            .header("x-tandem-correlation-id", cid)
                            .header("x-tandem-session-id", session_id);
                    }
                    let response = fallback_builder.json(&request).send().await.map_err(|e| {
                        TandemError::Sidecar(format!("Failed to send message: {}", e))
                    })?;
                    Self::handle_prompt_async_response(response).await
                }
                Err(err) => Err(err),
            };

            match outcome {
                Ok(()) => {
                    self.record_success().await;
                    return Ok(());
                }
                Err(err) => {
                    if is_engine_starting(&err) && attempt < ENGINE_STARTUP_RETRIES {
                        tracing::warn!(
                            "Engine still starting during prompt_async (attempt {}/{}). Retrying in {}ms.",
                            attempt + 1,
                            ENGINE_STARTUP_RETRIES + 1,
                            ENGINE_STARTUP_RETRY_MS
                        );
                        tokio::time::sleep(Duration::from_millis(ENGINE_STARTUP_RETRY_MS)).await;
                        last_send_error = Some(err);
                        continue;
                    }
                    if !is_engine_starting(&err) {
                        self.record_failure().await;
                    }
                    return Err(TandemError::Sidecar(format!(
                        "Failed to send message: {}",
                        err
                    )));
                }
            }
        }

        let final_err =
            last_send_error.unwrap_or_else(|| "engine did not become ready in time".to_string());
        Err(TandemError::Sidecar(format!(
            "Failed to send message: {}",
            final_err
        )))
    }

    async fn handle_prompt_async_response(
        response: reqwest::Response,
    ) -> std::result::Result<(), String> {
        let status = response.status();
        let headers = response.headers().clone();
        let content_type = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let body = response.text().await.unwrap_or_default();
        parse_prompt_async_response(status.as_u16(), &headers, &content_type, &body)
    }

    pub async fn get_active_run(
        &self,
        session_id: &str,
    ) -> Result<Option<ActiveRunStatusResponse>> {
        self.check_circuit_breaker().await?;

        let base = self.base_url().await?;
        let url = format!("{}/session/{}/run", base, session_id);
        let fallback_url = format!("{}/api/session/{}/run", base, session_id);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to get active run: {}", e)))?;

        match self.handle_response::<ActiveRunEnvelope>(response).await {
            Ok(payload) => Ok(payload.active),
            Err(err) => {
                tracing::warn!(
                    "Failed to read active run from {}, retrying {}: {}",
                    url,
                    fallback_url,
                    err
                );
                let fallback = self
                    .http_client
                    .get(&fallback_url)
                    .send()
                    .await
                    .map_err(|e| {
                        TandemError::Sidecar(format!("Failed to get active run: {}", e))
                    })?;
                let payload: ActiveRunEnvelope = self.handle_response(fallback).await?;
                Ok(payload.active)
            }
        }
    }

    pub async fn recover_active_run_attach_stream(
        &self,
        session_id: &str,
    ) -> Result<Option<String>> {
        let active = self.get_active_run(session_id).await?;
        Ok(active.map(|run| {
            format!(
                "/event?sessionID={}&runID={}",
                session_id,
                run.run_id.as_str()
            )
        }))
    }

    /// Revert a message (undo)
    /// OpenCode API: POST /session/{id}/revert
    /// Reverts the specified message and any file changes it made
    pub async fn revert_message(&self, session_id: &str, message_id: &str) -> Result<()> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/session/{}/revert", self.base_url().await?, session_id);
        tracing::info!("Reverting message {} in session {}", message_id, session_id);

        let body = serde_json::json!({
            "messageID": message_id
        });

        let response = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to revert message: {}", e)))?;

        if response.status().is_success() {
            self.record_success().await;
            tracing::info!("Successfully reverted message {}", message_id);
            Ok(())
        } else {
            self.record_failure().await;
            let body = response.text().await.unwrap_or_default();
            Err(TandemError::Sidecar(format!(
                "Failed to revert message: {}",
                body
            )))
        }
    }

    /// Unrevert messages (redo)
    /// OpenCode API: POST /session/{id}/unrevert
    /// Restores previously reverted messages
    pub async fn unrevert_message(&self, session_id: &str) -> Result<()> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/session/{}/unrevert", self.base_url().await?, session_id);
        tracing::info!("Unreverting messages in session {}", session_id);

        let response = self
            .http_client
            .post(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to unrevert message: {}", e)))?;

        if response.status().is_success() {
            self.record_success().await;
            tracing::info!("Successfully unreverted messages");
            Ok(())
        } else {
            self.record_failure().await;
            let body = response.text().await.unwrap_or_default();
            Err(TandemError::Sidecar(format!(
                "Failed to unrevert message: {}",
                body
            )))
        }
    }

    /// Subscribe to the event stream
    /// OpenCode API: GET /event (SSE)
    /// Returns a stream of events for all sessions
    pub async fn subscribe_events(
        &self,
    ) -> Result<impl futures::Stream<Item = Result<StreamEvent>>> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/event", self.base_url().await?);
        tracing::debug!("Subscribing to events at: {}", url);

        let response = self
            .stream_client
            .get(&url)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to subscribe to events: {}", e)))?;

        if !response.status().is_success() {
            // During sidecar restart/startup, /event can briefly return 503.
            // Treat this as transient so stream retries do not trip the global circuit breaker.
            let sidecar_state = self.state().await;
            let is_transient_startup_503 = response.status()
                == reqwest::StatusCode::SERVICE_UNAVAILABLE
                && matches!(
                    sidecar_state,
                    SidecarState::Starting | SidecarState::Stopping
                );
            if !is_transient_startup_503 {
                self.record_failure().await;
            }
            return Err(TandemError::Sidecar(format!(
                "Event subscription failed: {}",
                response.status()
            )));
        }

        self.record_success().await;

        // Convert the byte stream to SSE events
        let stream = response.bytes_stream();

        Ok(async_stream::stream! {
            let mut buffer = String::new();

            futures::pin_mut!(stream);

            while let Some(chunk_result) = stream.next().await {
                match chunk_result {
                    Ok(chunk) => {
                        let text = String::from_utf8_lossy(&chunk);
                        // Log raw SSE data for debugging
                        if !text.is_empty() && text.trim() != "" {
                            tracing::debug!("SSE raw chunk: {}", text.replace('\n', "\\n").chars().take(500).collect::<String>());
                        }
                        buffer.push_str(&text);

                        // Parse SSE events from buffer
                        while let Some(event) = parse_sse_event(&mut buffer) {
                            yield Ok(event);
                        }
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        // "error decoding response body" is a common reqwest error when connection is closed
                        // during a chunked transfer. On Linux this often happens during idle timeouts.
                        if err_msg.contains("error decoding response body") {
                            tracing::warn!("SSE stream closed by server (likely timeout): {}", err_msg);
                        } else {
                            tracing::error!("SSE stream error: {}", e);
                        }
                        yield Err(TandemError::Sidecar(format!("Stream error: {}", e)));
                        break;
                    }
                }
            }
            tracing::debug!("SSE stream ended");
        })
    }

    /// Cancel ongoing generation in a session
    pub async fn cancel_generation(&self, session_id: &str) -> Result<()> {
        self.check_circuit_breaker().await?;

        // 1. Send cancel request to the sidecar via HTTP API
        let base = self.base_url().await?;
        let url = format!("{}/session/{}/cancel", base, session_id);
        let fallback_url = format!("{}/api/session/{}/cancel", base, session_id);

        tracing::info!("Cancelling session: {}", session_id);

        // Try primary URL
        let mut response = self
            .http_client
            .post(&url)
            .timeout(Duration::from_secs(5)) // Short timeout for cancel
            .send()
            .await;

        // If primary fails or returns 404, try fallback
        if response.is_err()
            || response
                .as_ref()
                .is_ok_and(|r| r.status() == reqwest::StatusCode::NOT_FOUND)
        {
            tracing::warn!(
                "Cancel failed on primary URL {}, trying fallback {}",
                url,
                fallback_url
            );
            let fallback_response = self
                .http_client
                .post(&fallback_url)
                .timeout(Duration::from_secs(5))
                .send()
                .await;

            // Only use fallback if it didn't error (or if primary was an error)
            if fallback_response.is_ok() {
                response = fallback_response;
            }
        }

        match response {
            Ok(resp) => {
                if resp.status().is_success() {
                    tracing::info!("Cancel request successful for session {}", session_id);
                    self.record_success().await;
                    Ok(())
                } else {
                    let status = resp.status();
                    let body = resp.text().await.unwrap_or_default();
                    tracing::warn!("Cancel API request failed: {} - {}", status, body);
                    // Even if API fails, we consider the attempt "success" for circuit breaker
                    // because we don't want to open the circuit on a cancel
                    self.record_success().await;
                    Ok(())
                }
            }
            Err(e) => {
                tracing::error!("Failed to send cancel request: {}", e);
                // Don't fail the operation, just log it. The frontend will stop listening anyway.
                self.record_success().await;
                Ok(())
            }
        }
    }

    pub async fn cancel_run_by_id(&self, session_id: &str, run_id: &str) -> Result<bool> {
        self.check_circuit_breaker().await?;

        let base = self.base_url().await?;
        let url = format!("{}/session/{}/run/{}/cancel", base, session_id, run_id);
        let fallback_url = format!("{}/api/session/{}/run/{}/cancel", base, session_id, run_id);
        let response = self
            .http_client
            .post(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to cancel run by id: {}", e)))?;

        match self.handle_response::<serde_json::Value>(response).await {
            Ok(payload) => Ok(payload
                .get("cancelled")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)),
            Err(err) => {
                tracing::warn!(
                    "Cancel run by id failed on {}, retrying {}: {}",
                    url,
                    fallback_url,
                    err
                );
                let fallback = self
                    .http_client
                    .post(&fallback_url)
                    .send()
                    .await
                    .map_err(|e| {
                        TandemError::Sidecar(format!("Failed to cancel run by id: {}", e))
                    })?;
                let payload: serde_json::Value = self.handle_response(fallback).await?;
                Ok(payload
                    .get("cancelled")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false))
            }
        }
    }

    // ========================================================================
    // Routines
    // ========================================================================

    pub async fn routines_create(&self, request: RoutineCreateRequest) -> Result<RoutineSpec> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/routines", self.base_url().await?);
        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to create routine: {}", e)))?;
        let payload: RoutineRecordResponse = self.handle_response(response).await?;
        Ok(payload.routine)
    }

    pub async fn routines_list(&self) -> Result<Vec<RoutineSpec>> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/routines", self.base_url().await?);
        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list routines: {}", e)))?;
        let payload: RoutineListResponse = self.handle_response(response).await?;
        Ok(payload.routines)
    }

    pub async fn routines_patch(
        &self,
        routine_id: &str,
        request: RoutinePatchRequest,
    ) -> Result<RoutineSpec> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/routines/{}", self.base_url().await?, routine_id);
        let response = self
            .http_client
            .patch(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to patch routine: {}", e)))?;
        let payload: RoutineRecordResponse = self.handle_response(response).await?;
        Ok(payload.routine)
    }

    pub async fn routines_delete(&self, routine_id: &str) -> Result<bool> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/routines/{}", self.base_url().await?, routine_id);
        let response = self
            .http_client
            .delete(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to delete routine: {}", e)))?;
        let payload: RoutineDeleteResponse = self.handle_response(response).await?;
        Ok(payload.deleted)
    }

    pub async fn routines_run_now(
        &self,
        routine_id: &str,
        request: RoutineRunNowRequest,
    ) -> Result<RoutineRunNowResponse> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/routines/{}/run_now", self.base_url().await?, routine_id);
        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to trigger routine: {}", e)))?;
        self.handle_response(response).await
    }

    pub async fn routines_history(
        &self,
        routine_id: &str,
        limit: Option<usize>,
    ) -> Result<Vec<RoutineHistoryEvent>> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/routines/{}/history", self.base_url().await?, routine_id);
        let mut request = self.http_client.get(&url);
        if let Some(limit) = limit {
            request = request.query(&[("limit", limit)]);
        }
        let response = request
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to load routine history: {}", e)))?;
        let payload: RoutineHistoryResponse = self.handle_response(response).await?;
        Ok(payload.events)
    }

    // ========================================================================
    // Missions
    // ========================================================================

    pub async fn mission_create(&self, request: MissionCreateRequest) -> Result<MissionState> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/mission", self.base_url().await?);
        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to create mission: {}", e)))?;
        let payload: MissionRecordResponse = self.handle_response(response).await?;
        Ok(payload.mission)
    }

    pub async fn mission_list(&self) -> Result<Vec<MissionState>> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/mission", self.base_url().await?);
        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list missions: {}", e)))?;
        let payload: MissionListResponse = self.handle_response(response).await?;
        Ok(payload.missions)
    }

    pub async fn mission_get(&self, mission_id: &str) -> Result<MissionState> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/mission/{}", self.base_url().await?, mission_id);
        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to get mission: {}", e)))?;
        let payload: MissionRecordResponse = self.handle_response(response).await?;
        Ok(payload.mission)
    }

    pub async fn mission_apply_event(
        &self,
        mission_id: &str,
        event: serde_json::Value,
    ) -> Result<MissionApplyEventResult> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/mission/{}/event", self.base_url().await?, mission_id);
        let response = self
            .http_client
            .post(&url)
            .json(&serde_json::json!({ "event": event }))
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to apply mission event: {}", e)))?;
        self.handle_response(response).await
    }

    // ========================================================================
    // Agent Teams
    // ========================================================================

    pub async fn agent_team_list_templates(&self) -> Result<Vec<AgentTeamTemplate>> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/agent-team/templates", self.base_url().await?);
        let response = self.http_client.get(&url).send().await.map_err(|e| {
            TandemError::Sidecar(format!("Failed to list agent-team templates: {}", e))
        })?;
        let payload: AgentTeamTemplatesResponse = self.handle_response(response).await?;
        Ok(payload.templates)
    }

    pub async fn agent_team_list_instances(
        &self,
        query: AgentTeamInstancesQuery,
    ) -> Result<Vec<AgentTeamInstance>> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/agent-team/instances", self.base_url().await?);
        let mut request = self.http_client.get(&url);
        let mut pairs: Vec<(&str, String)> = Vec::new();
        if let Some(v) = query.mission_id {
            pairs.push(("missionID", v));
        }
        if let Some(v) = query.parent_instance_id {
            pairs.push(("parentInstanceID", v));
        }
        if let Some(v) = query.status {
            pairs.push(("status", v));
        }
        if !pairs.is_empty() {
            request = request.query(&pairs);
        }
        let response = request.send().await.map_err(|e| {
            TandemError::Sidecar(format!("Failed to list agent-team instances: {}", e))
        })?;
        let payload: AgentTeamInstancesResponse = self.handle_response(response).await?;
        Ok(payload.instances)
    }

    pub async fn agent_team_list_missions(&self) -> Result<Vec<AgentTeamMissionSummary>> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/agent-team/missions", self.base_url().await?);
        let response = self.http_client.get(&url).send().await.map_err(|e| {
            TandemError::Sidecar(format!("Failed to list agent-team missions: {}", e))
        })?;
        let payload: AgentTeamMissionSummariesResponse = self.handle_response(response).await?;
        Ok(payload.missions)
    }

    pub async fn agent_team_list_approvals(&self) -> Result<AgentTeamApprovals> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/agent-team/approvals", self.base_url().await?);
        let response = self.http_client.get(&url).send().await.map_err(|e| {
            TandemError::Sidecar(format!("Failed to list agent-team approvals: {}", e))
        })?;
        let payload: AgentTeamApprovalsResponse = self.handle_response(response).await?;
        Ok(AgentTeamApprovals {
            spawn_approvals: payload.spawn_approvals,
            tool_approvals: payload.tool_approvals,
        })
    }

    pub async fn agent_team_spawn(
        &self,
        request: AgentTeamSpawnRequest,
    ) -> Result<AgentTeamSpawnResult> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/agent-team/spawn", self.base_url().await?);
        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                TandemError::Sidecar(format!("Failed to spawn agent-team instance: {}", e))
            })?;
        self.handle_response(response).await
    }

    pub async fn agent_team_cancel_instance(
        &self,
        instance_id: &str,
        request: AgentTeamCancelRequest,
    ) -> Result<AgentTeamDecisionResult> {
        self.check_circuit_breaker().await?;
        let url = format!(
            "{}/agent-team/instance/{}/cancel",
            self.base_url().await?,
            instance_id
        );
        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                TandemError::Sidecar(format!("Failed to cancel agent-team instance: {}", e))
            })?;
        self.handle_response(response).await
    }

    pub async fn agent_team_cancel_mission(
        &self,
        mission_id: &str,
        request: AgentTeamCancelRequest,
    ) -> Result<AgentTeamDecisionResult> {
        self.check_circuit_breaker().await?;
        let url = format!(
            "{}/agent-team/mission/{}/cancel",
            self.base_url().await?,
            mission_id
        );
        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                TandemError::Sidecar(format!("Failed to cancel agent-team mission: {}", e))
            })?;
        self.handle_response(response).await
    }

    pub async fn agent_team_approve_spawn(
        &self,
        approval_id: &str,
        request: AgentTeamCancelRequest,
    ) -> Result<AgentTeamDecisionResult> {
        self.check_circuit_breaker().await?;
        let url = format!(
            "{}/agent-team/approvals/spawn/{}/approve",
            self.base_url().await?,
            approval_id
        );
        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| {
                TandemError::Sidecar(format!("Failed to approve agent-team spawn: {}", e))
            })?;
        self.handle_response(response).await
    }

    pub async fn agent_team_deny_spawn(
        &self,
        approval_id: &str,
        request: AgentTeamCancelRequest,
    ) -> Result<AgentTeamDecisionResult> {
        self.check_circuit_breaker().await?;
        let url = format!(
            "{}/agent-team/approvals/spawn/{}/deny",
            self.base_url().await?,
            approval_id
        );
        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to deny agent-team spawn: {}", e)))?;
        self.handle_response(response).await
    }

    // ========================================================================
    // Model & Provider Info
    // ========================================================================

    /// List available models
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        self.check_circuit_breaker().await?;

        let base = self.base_url().await?;
        if let Ok(catalog) = self.fetch_provider_catalog(&base).await {
            return Ok(Self::models_from_provider_catalog(&catalog));
        }

        let url = format!("{}/models", base);
        let fallback_url = format!("{}/api/models", base);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list models: {}", e)))?;

        match self.handle_response(response).await {
            Ok(models) => Ok(models),
            Err(e) => {
                tracing::warn!(
                    "Failed to parse models from {}, retrying with {}: {}",
                    url,
                    fallback_url,
                    e
                );
                let response = self
                    .http_client
                    .get(&fallback_url)
                    .send()
                    .await
                    .map_err(|err| {
                        TandemError::Sidecar(format!("Failed to list models: {}", err))
                    })?;
                self.handle_response(response).await
            }
        }
    }

    /// List available providers
    pub async fn list_providers(&self) -> Result<Vec<ProviderInfo>> {
        self.check_circuit_breaker().await?;

        let base = self.base_url().await?;
        if let Ok(catalog) = self.fetch_provider_catalog(&base).await {
            return Ok(Self::providers_from_provider_catalog(catalog));
        }

        let url = format!("{}/providers", base);
        let fallback_url = format!("{}/api/providers", base);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list providers: {}", e)))?;

        match self.handle_response(response).await {
            Ok(providers) => Ok(providers),
            Err(e) => {
                tracing::warn!(
                    "Failed to parse providers from {}, retrying with {}: {}",
                    url,
                    fallback_url,
                    e
                );
                let response = self
                    .http_client
                    .get(&fallback_url)
                    .send()
                    .await
                    .map_err(|err| {
                        TandemError::Sidecar(format!("Failed to list providers: {}", err))
                    })?;
                self.handle_response(response).await
            }
        }
    }

    pub async fn channels_status(&self) -> Result<ChannelsStatusResponse> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/channels/status", self.base_url().await?);
        let response =
            self.http_client.get(&url).send().await.map_err(|e| {
                TandemError::Sidecar(format!("Failed to fetch channels status: {}", e))
            })?;
        self.handle_response(response).await
    }

    pub async fn channels_config(&self) -> Result<ChannelsConfigResponse> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/channels/config", self.base_url().await?);
        let response =
            self.http_client.get(&url).send().await.map_err(|e| {
                TandemError::Sidecar(format!("Failed to fetch channels config: {}", e))
            })?;
        self.handle_response(response).await
    }

    pub async fn channels_put(&self, name: &str, body: serde_json::Value) -> Result<()> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/channels/{}", self.base_url().await?, name);
        let response = self
            .http_client
            .put(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                TandemError::Sidecar(format!("Failed to update channel '{}': {}", name, e))
            })?;
        let _: serde_json::Value = self.handle_response(response).await?;
        Ok(())
    }

    pub async fn channels_delete(&self, name: &str) -> Result<()> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/channels/{}", self.base_url().await?, name);
        let response = self.http_client.delete(&url).send().await.map_err(|e| {
            TandemError::Sidecar(format!("Failed to disable channel '{}': {}", name, e))
        })?;
        let _: serde_json::Value = self.handle_response(response).await?;
        Ok(())
    }

    /// Set runtime-only auth token for a provider on the engine sidecar.
    pub async fn set_provider_auth(&self, provider_id: &str, api_key: &str) -> Result<()> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/auth/{}", self.base_url().await?, provider_id);
        let response = self
            .http_client
            .put(&url)
            .json(&serde_json::json!({ "apiKey": api_key }))
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to set provider auth: {}", e)))?;
        let _: serde_json::Value = self.handle_response(response).await?;
        Ok(())
    }

    // ========================================================================
    // Skills
    // ========================================================================

    pub async fn list_skills(&self) -> Result<Vec<SkillInfo>> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/skills", self.base_url().await?);
        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list skills: {}", e)))?;
        self.handle_response(response).await
    }

    pub async fn get_skill(&self, name: &str) -> Result<SkillContent> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/skills/{}", self.base_url().await?, name);
        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to load skill: {}", e)))?;
        self.handle_response(response).await
    }

    pub async fn import_skill_content(
        &self,
        content: String,
        location: SkillLocation,
    ) -> Result<SkillInfo> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/skills/import", self.base_url().await?);
        let body = SkillsImportRequest {
            location,
            content: Some(content),
            file_or_path: None,
            namespace: None,
            conflict_policy: None,
        };
        let response = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to import skill: {}", e)))?;
        self.handle_response(response).await
    }

    pub async fn skills_import_preview(
        &self,
        file_or_path: String,
        location: SkillLocation,
        namespace: Option<String>,
        conflict_policy: String,
    ) -> Result<SkillsImportPreview> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/skills/import/preview", self.base_url().await?);
        let body = SkillsImportRequest {
            location,
            content: None,
            file_or_path: Some(file_or_path),
            namespace,
            conflict_policy: Some(conflict_policy),
        };
        let response = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to preview skill import: {}", e)))?;
        self.handle_response(response).await
    }

    pub async fn skills_import(
        &self,
        file_or_path: String,
        location: SkillLocation,
        namespace: Option<String>,
        conflict_policy: String,
    ) -> Result<SkillsImportResult> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/skills/import", self.base_url().await?);
        let body = SkillsImportRequest {
            location,
            content: None,
            file_or_path: Some(file_or_path),
            namespace,
            conflict_policy: Some(conflict_policy),
        };
        let response = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to import skills: {}", e)))?;
        self.handle_response(response).await
    }

    pub async fn delete_skill(&self, name: String, location: SkillLocation) -> Result<()> {
        self.check_circuit_breaker().await?;
        let base = self.base_url().await?;
        let location_text = match location {
            SkillLocation::Project => "project",
            SkillLocation::Global => "global",
        };
        let url = format!("{}/skills/{}", base, name);
        let response = self
            .http_client
            .delete(&url)
            .query(&[SkillsDeleteQuery {
                location: location_text,
            }])
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to delete skill: {}", e)))?;
        let _: serde_json::Value = self.handle_response(response).await?;
        Ok(())
    }

    pub async fn list_skill_templates(&self) -> Result<Vec<SkillTemplateInfo>> {
        self.check_circuit_breaker().await?;
        let url = format!("{}/skills/templates", self.base_url().await?);
        let response =
            self.http_client.get(&url).send().await.map_err(|e| {
                TandemError::Sidecar(format!("Failed to list skill templates: {}", e))
            })?;
        self.handle_response(response).await
    }

    pub async fn install_skill_template(
        &self,
        template_id: String,
        location: SkillLocation,
    ) -> Result<SkillInfo> {
        self.check_circuit_breaker().await?;
        let url = format!(
            "{}/skills/templates/{}/install",
            self.base_url().await?,
            template_id
        );
        let body = SkillsTemplateInstallRequest { location };
        let response = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to install template: {}", e)))?;
        self.handle_response(response).await
    }

    // ========================================================================
    // Tool Approval
    // ========================================================================

    /// Approve a pending tool execution
    pub async fn approve_tool(&self, session_id: &str, tool_call_id: &str) -> Result<()> {
        self.check_circuit_breaker().await?;

        let url = format!(
            "{}/sessions/{}/tools/{}/approve",
            self.base_url().await?,
            session_id,
            tool_call_id
        );

        let response = self
            .http_client
            .post(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to approve tool: {}", e)))?;

        if response.status().is_success() {
            self.record_success().await;
            Ok(())
        } else {
            self.record_failure().await;
            Err(TandemError::Sidecar(format!(
                "Failed to approve tool: {}",
                response.status()
            )))
        }
    }

    /// Deny a pending tool execution
    pub async fn deny_tool(&self, session_id: &str, tool_call_id: &str) -> Result<()> {
        self.check_circuit_breaker().await?;

        let url = format!(
            "{}/sessions/{}/tools/{}/deny",
            self.base_url().await?,
            session_id,
            tool_call_id
        );

        let response = self
            .http_client
            .post(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to deny tool: {}", e)))?;

        if response.status().is_success() {
            self.record_success().await;
            Ok(())
        } else {
            self.record_failure().await;
            Err(TandemError::Sidecar(format!(
                "Failed to deny tool: {}",
                response.status()
            )))
        }
    }

    /// Answer a question from the LLM
    pub async fn answer_question(
        &self,
        session_id: &str,
        question_id: &str,
        answer: String,
    ) -> Result<()> {
        self.check_circuit_breaker().await?;

        let url = format!(
            "{}/sessions/{}/questions/{}/answer",
            self.base_url().await?,
            session_id,
            question_id
        );

        let body = serde_json::json!({ "answer": answer });

        let response = self
            .http_client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to answer question: {}", e)))?;

        if response.status().is_success() {
            self.record_success().await;
            Ok(())
        } else {
            self.record_failure().await;
            Err(TandemError::Sidecar(format!(
                "Failed to answer question: {}",
                response.status()
            )))
        }
    }

    // ========================================================================
    // Question Requests
    // ========================================================================

    /// List all pending question requests.
    ///
    /// OpenCode API: GET /question
    pub async fn list_questions(&self) -> Result<Vec<QuestionRequest>> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/question", self.base_url().await?);
        let mut req = self.http_client.get(&url);
        if let Some(directory) = self.workspace_directory().await {
            req = req.query(&[("directory", directory)]);
        }

        let response = req
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list questions: {}", e)))?;

        let requests: Vec<OpenCodeQuestionRequest> = self.handle_response(response).await?;
        Ok(requests
            .into_iter()
            .map(|r| QuestionRequest {
                session_id: r.session_id,
                request_id: r.id,
                questions: r.questions,
                tool_call_id: r.tool.as_ref().map(|t| t.call_id.clone()),
                tool_message_id: r.tool.as_ref().map(|t| t.message_id.clone()),
            })
            .collect())
    }

    /// Reply to a question request.
    ///
    /// OpenCode API: POST /question/{requestID}/reply
    pub async fn reply_question(&self, request_id: &str, answers: Vec<Vec<String>>) -> Result<()> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/question/{}/reply", self.base_url().await?, request_id);

        let mut req = self.http_client.post(&url);
        if let Some(directory) = self.workspace_directory().await {
            req = req.query(&[("directory", directory)]);
        }

        let response = req
            .json(&serde_json::json!({ "answers": answers }))
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to reply to question: {}", e)))?;

        let _ok: bool = self.handle_response(response).await?;
        Ok(())
    }

    /// Reject a question request.
    ///
    /// OpenCode API: POST /question/{requestID}/reject
    pub async fn reject_question(&self, request_id: &str) -> Result<()> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/question/{}/reject", self.base_url().await?, request_id);

        let mut req = self.http_client.post(&url);
        if let Some(directory) = self.workspace_directory().await {
            req = req.query(&[("directory", directory)]);
        }

        let response = req
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to reject question: {}", e)))?;

        let _ok: bool = self.handle_response(response).await?;
        Ok(())
    }

    // ========================================================================
    // Helpers
    // ========================================================================

    async fn workspace_directory(&self) -> Option<String> {
        let config = self.config.read().await;
        config
            .workspace_path
            .as_ref()
            .map(|p| p.to_string_lossy().to_string())
    }

    async fn check_circuit_breaker(&self) -> Result<()> {
        let mut cb = self.circuit_breaker.lock().await;
        if !cb.can_execute() {
            return Err(TandemError::Sidecar("Circuit breaker is open".to_string()));
        }
        Ok(())
    }

    async fn fetch_provider_catalog(&self, base: &str) -> Result<ProviderCatalogResponse> {
        let url = format!("{}/provider", base);
        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list providers: {}", e)))?;
        let raw: serde_json::Value = self.handle_response(response).await.map_err(|e| {
            tracing::warn!("Failed to parse providers from {}: {}", url, e);
            e
        })?;
        parse_provider_catalog_response(raw).ok_or_else(|| {
            TandemError::Sidecar(format!(
                "Failed to parse provider catalog shape from {}",
                url
            ))
        })
    }

    fn providers_from_provider_catalog(catalog: ProviderCatalogResponse) -> Vec<ProviderInfo> {
        let connected: HashSet<String> = catalog.connected.into_iter().collect();
        catalog
            .all
            .into_iter()
            .map(|provider| {
                let models = provider.models.keys().cloned().collect::<Vec<_>>();
                ProviderInfo {
                    id: provider.id.clone(),
                    name: provider.name.unwrap_or_else(|| provider.id.clone()),
                    models,
                    configured: connected.contains(&provider.id),
                }
            })
            .collect()
    }

    fn models_from_provider_catalog(catalog: &ProviderCatalogResponse) -> Vec<ModelInfo> {
        let mut models = Vec::new();
        for provider in &catalog.all {
            for (model_id, model) in &provider.models {
                let name = model.name.clone().unwrap_or_else(|| model_id.clone());
                let context_length = model.limit.as_ref().and_then(|limit| limit.context);
                models.push(ModelInfo {
                    id: model_id.clone(),
                    name,
                    provider: Some(provider.id.clone()),
                    context_length,
                });
            }
        }
        models
    }

    async fn record_success(&self) {
        let mut cb = self.circuit_breaker.lock().await;
        cb.record_success();
    }

    async fn record_failure(&self) {
        let mut cb = self.circuit_breaker.lock().await;
        cb.record_failure();
    }

    async fn handle_response<T: serde::de::DeserializeOwned>(
        &self,
        response: reqwest::Response,
    ) -> Result<T> {
        let status = response.status();
        let url = response.url().to_string();

        if status.is_success() {
            self.record_success().await;

            // Get the response body as text first for debugging
            let body = response.text().await.map_err(|e| {
                TandemError::Sidecar(format!("Failed to read response body: {}", e))
            })?;

            tracing::debug!("Response from {}: {}", url, &body[..body.len().min(500)]);

            // Parse the JSON
            serde_json::from_str(&body).map_err(|e| {
                tracing::error!("Failed to parse response from {}: {}", url, e);
                tracing::error!("Response body: {}", &body[..body.len().min(1000)]);
                TandemError::Sidecar(format!(
                    "Failed to parse response: {}. Body: {}",
                    e,
                    &body[..body.len().min(200)]
                ))
            })
        } else {
            let body = response.text().await.unwrap_or_default();
            if !body.contains("ENGINE_STARTING") {
                self.record_failure().await;
            } else {
                tracing::debug!(
                    "Ignoring circuit breaker failure for ENGINE_STARTING: {}",
                    url
                );
            }
            tracing::error!("Request to {} failed ({}): {}", url, status, body);
            Err(TandemError::Sidecar(format!(
                "Request failed ({}): {}",
                status, body
            )))
        }
    }
}

impl Drop for SidecarManager {
    fn drop(&mut self) {
        // Ensure sidecar is stopped when manager is dropped
        // Note: This is blocking, but Drop can't be async
        if let Ok(mut process_guard) = self.process.try_lock() {
            if let Some(mut child) = process_guard.take() {
                tracing::info!("Killing tandem-engine sidecar on drop");
                let _ = child.kill();
            }
        }
    }
}

// ============================================================================
// SSE Parsing
// ============================================================================

/// Parse a single SSE event from the buffer
fn parse_sse_event(buffer: &mut String) -> Option<StreamEvent> {
    // SSE format:
    //   data: {json}\n\n
    // or
    //   event: type\ndata:{json}\n\n
    //
    // Notes:
    // - The `data:` prefix may or may not include a space after the colon.
    // - An event may contain multiple `data:` lines; they must be concatenated with '\n'.
    // - Some servers use \r\n line endings.

    // Find event delimiter (\n\n or \r\n\r\n)
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
        // Generic done signal
        return Some(StreamEvent::SessionIdle {
            session_id: "unknown".to_string(),
        });
    }

    match serde_json::from_str::<OpenCodeEvent>(&data) {
        Ok(event) => convert_opencode_event(event),
        Err(e) => {
            tracing::debug!("Failed to parse as OpenCodeEvent: {} - data: {}", e, data);
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&data) {
                return Some(StreamEvent::Raw {
                    event_type: "unknown".to_string(),
                    data: value,
                });
            }
            None
        }
    }
}

/// Convert OpenCode event to our StreamEvent format
fn convert_opencode_event(event: OpenCodeEvent) -> Option<StreamEvent> {
    let props = &event.properties;

    let event_type = event.event_type.trim();

    // OpenCode emits periodic keep-alive noise. Treat all `server.*` events as ignorable.
    // This avoids log spam and prevents "unhandled event" warnings from drowning real issues.
    if event_type.starts_with("server.") {
        return None;
    }

    match event_type {
        // Ignore noisy server-level events. Treating them as "unhandled" spams logs and can keep
        // UI streaming loops alive forever even when the session is stuck.
        // Diffs can be very large and are emitted frequently; the app doesn't currently render them.
        "session.diff" => None,
        // We currently only need `question.asked` to render the UI. This is an ack.
        "question.replied" => None,
        "message.part.updated" => {
            // Extract part info
            let part = props.get("part")?;

            // Debug log the full event
            tracing::debug!("message.part.updated event: {:?}", props);

            // IMPORTANT: Only process events with a delta - this indicates streaming content
            // from the assistant. Events without delta are typically user message confirmations.
            let delta = props
                .get("delta")
                .and_then(|d| match d {
                    // Historical format: delta is a string
                    serde_json::Value::String(s) => Some(s.clone()),
                    // Newer format: delta can be an object, commonly with a `text` field
                    serde_json::Value::Object(map) => map
                        .get("text")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string()),
                    // Some implementations send an array of chunks
                    serde_json::Value::Array(items) => {
                        let mut out = String::new();
                        for item in items {
                            match item {
                                serde_json::Value::String(s) => out.push_str(s),
                                serde_json::Value::Object(map) => {
                                    if let Some(s) = map
                                        .get("text")
                                        .and_then(|t| t.as_str())
                                        .map(|s| s.to_string())
                                    {
                                        out.push_str(&s);
                                    }
                                }
                                _ => {}
                            }
                        }
                        if out.is_empty() {
                            None
                        } else {
                            Some(out)
                        }
                    }
                    _ => None,
                })
                // Some event versions nest delta under the part payload
                .or_else(|| {
                    part.get("delta").and_then(|d| match d {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Object(map) => map
                            .get("text")
                            .and_then(|t| t.as_str())
                            .map(|s| s.to_string()),
                        serde_json::Value::Array(items) => {
                            let mut out = String::new();
                            for item in items {
                                match item {
                                    serde_json::Value::String(s) => out.push_str(s),
                                    serde_json::Value::Object(map) => {
                                        if let Some(s) = map
                                            .get("text")
                                            .and_then(|t| t.as_str())
                                            .map(|s| s.to_string())
                                        {
                                            out.push_str(&s);
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            if out.is_empty() {
                                None
                            } else {
                                Some(out)
                            }
                        }
                        _ => None,
                    })
                });

            let session_id = part.get("sessionID").and_then(|s| s.as_str())?.to_string();
            let message_id = part.get("messageID").and_then(|s| s.as_str())?.to_string();
            let part_id = part
                .get("id")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let part_type = part.get("type").and_then(|s| s.as_str()).unwrap_or("text");

            match part_type {
                "text" | "reasoning" => {
                    // Only emit content events if there's a delta (streaming from assistant)
                    // This prevents echoing user messages which come without delta
                    if delta.is_none() {
                        // Some OpenCode builds omit `delta` for assistant streaming updates.
                        // In that case, only allow this event through if the role is explicitly assistant.
                        let role = props
                            .get("message")
                            .and_then(|m| m.get("role").or_else(|| m.get("info")?.get("role")))
                            .and_then(|r| r.as_str())
                            .unwrap_or("");
                        if role != "assistant" {
                            // Tandem engine final text snapshots may omit role metadata.
                            // If this is a non-empty text snapshot without delta and without
                            // message role envelope, pass it through so final answer is visible.
                            let has_text = part
                                .get("text")
                                .and_then(|s| s.as_str())
                                .map(|s| !s.trim().is_empty())
                                .unwrap_or(false);
                            let has_message_envelope = props.get("message").is_some();
                            if has_text && !has_message_envelope {
                                tracing::debug!(
                                    "Emitting text part without delta based on text snapshot fallback"
                                );
                            } else {
                                tracing::debug!(
                                    "Skipping text/reasoning part without delta (likely user message)"
                                );
                                return None;
                            }
                        } else {
                            tracing::debug!("Emitting text part without delta for assistant");
                        }
                    }

                    let text = part
                        .get("text")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();

                    // Filter out [REDACTED] markers that leak from reasoning output
                    if text.trim() == "[REDACTED]" || text.is_empty() {
                        return None;
                    }

                    Some(StreamEvent::Content {
                        session_id,
                        message_id,
                        content: text,
                        delta,
                    })
                }
                // Ignore reasoning parts to avoid showing "[REDACTED]" in chat
                //"reasoning" => None,
                "tool-invocation" | "tool" => {
                    let tool = part
                        .get("tool")
                        .and_then(|s| s.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    let state_value = part.get("state");
                    let explicit_state = state_value
                        .and_then(|s| s.get("status"))
                        .and_then(|s| s.as_str())
                        .or_else(|| part.get("state").and_then(|s| s.as_str()));
                    let raw_args = state_value
                        .and_then(|s| s.get("input"))
                        .cloned()
                        .or_else(|| part.get("args").cloned())
                        .unwrap_or(serde_json::Value::Null);
                    let args = match normalize_tool_args(&tool, &raw_args) {
                        Ok(value) => value,
                        Err(reason) => {
                            // Be permissive on stream payload parsing: partial/odd args can occur
                            // mid-stream and should not hard-fail the entire session.
                            tracing::warn!(
                                "tool args normalization fallback for tool={} part_id={} reason={}",
                                tool,
                                part_id,
                                reason
                            );
                            raw_args.clone()
                        }
                    };
                    let has_output = state_value.and_then(|s| s.get("output")).is_some()
                        || part.get("result").is_some()
                        || part.get("output").is_some();
                    let has_error = state_value.and_then(|s| s.get("error")).is_some()
                        || part.get("error").is_some();
                    let state = explicit_state.unwrap_or_else(|| {
                        if has_error {
                            "failed"
                        } else if has_output {
                            "completed"
                        } else {
                            "pending"
                        }
                    });

                    match state {
                        // OpenCode has used multiple spellings across builds.
                        "pending" | "running" | "in_progress" => Some(StreamEvent::ToolStart {
                            session_id,
                            message_id,
                            part_id,
                            tool,
                            args,
                        }),
                        // Treat any terminal-ish status as a tool end, otherwise the UI can get stuck
                        // showing a "pending tool" forever (e.g. when a tool is cancelled/denied).
                        "completed" | "failed" | "error" | "cancelled" | "canceled" | "denied"
                        | "rejected" | "aborted" | "skipped" | "timeout" | "timed_out" => {
                            let result = state_value
                                .and_then(|s| s.get("output"))
                                .cloned()
                                .or_else(|| part.get("result").cloned());
                            let error = state_value
                                .and_then(|s| s.get("error"))
                                .and_then(|e| e.as_str())
                                .map(|s| s.to_string())
                                .or_else(|| {
                                    part.get("error")
                                        .and_then(|e| e.as_str())
                                        .map(|s| s.to_string())
                                });
                            let error = error.or_else(|| {
                                // Some terminal states don't include a structured error payload.
                                if state != "completed" {
                                    Some(state.to_string())
                                } else {
                                    None
                                }
                            });
                            let error_code =
                                derive_tool_error_code(state_value, part, error.as_deref(), state);
                            Some(StreamEvent::ToolEnd {
                                session_id,
                                message_id,
                                part_id,
                                tool,
                                result,
                                error,
                                error_code,
                            })
                        }
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        "message.updated" => {
            // Full message update - use role to avoid echoing user input
            let message = props.get("message").unwrap_or(props);
            let info = message
                .get("info")
                .or_else(|| message.get("message"))
                .unwrap_or(message);

            let role = info.get("role").and_then(|r| r.as_str()).unwrap_or("");
            tracing::debug!("message.updated event - role: {}, info: {:?}", role, info);

            if role != "assistant" {
                tracing::debug!("Skipping message.updated for non-assistant role: {}", role);
                return None;
            }

            let session_id = info
                .get("sessionID")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();
            let message_id = info
                .get("id")
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();

            if let Some(error_value) = info.get("error") {
                if let Some(error) = extract_error_message(error_value) {
                    return Some(StreamEvent::SessionError {
                        session_id,
                        error_code: extract_error_code(error_value)
                            .or_else(|| classify_error_code(&error)),
                        error,
                    });
                }
            }

            let parts = message
                .get("parts")
                .or_else(|| props.get("parts"))
                .and_then(|p| p.as_array());

            if let Some(parts) = parts {
                let mut content = String::new();
                for part in parts {
                    if part.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(text) = part.get("text").and_then(|t| t.as_str()) {
                            content.push_str(text);
                        }
                    }
                }
                if !content.is_empty() {
                    return Some(StreamEvent::Content {
                        session_id,
                        message_id,
                        content,
                        delta: None,
                    });
                }
            }

            None
        }
        "session.updated" => {
            let session = props.get("session").unwrap_or(props);
            let status = session
                .get("status")
                .or_else(|| props.get("status"))
                .and_then(|s| s.as_str())
                .unwrap_or("");
            let session_id = session
                .get("id")
                .or_else(|| session.get("sessionID"))
                .and_then(|s| s.as_str())
                .unwrap_or("")
                .to_string();

            if matches!(status, "idle" | "complete" | "completed") {
                return Some(StreamEvent::SessionIdle { session_id });
            }
            None
        }
        "session.status" => {
            let session_id = props.get("sessionID").and_then(|s| s.as_str())?.to_string();
            let status = props.get("status").and_then(|s| s.as_str())?.to_string();
            Some(StreamEvent::SessionStatus { session_id, status })
        }
        "session.run.started" => {
            let session_id = props.get("sessionID").and_then(|s| s.as_str())?.to_string();
            let run_id = props.get("runID").and_then(|s| s.as_str())?.to_string();
            let started_at_ms = props
                .get("startedAtMs")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let client_id = props
                .get("clientID")
                .and_then(|v| v.as_str())
                .map(|v| v.to_string());
            Some(StreamEvent::RunStarted {
                session_id,
                run_id,
                started_at_ms,
                client_id,
            })
        }
        "session.run.finished" => {
            let session_id = props.get("sessionID").and_then(|s| s.as_str())?.to_string();
            let run_id = props.get("runID").and_then(|s| s.as_str())?.to_string();
            let finished_at_ms = props
                .get("finishedAtMs")
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let status = props
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("completed")
                .to_string();
            let error = props.get("error").and_then(extract_error_message);
            Some(StreamEvent::RunFinished {
                session_id,
                run_id,
                finished_at_ms,
                status,
                error,
            })
        }
        "session.run.conflict" => {
            let session_id = props.get("sessionID").and_then(|s| s.as_str())?.to_string();
            let run_id = props.get("runID").and_then(|s| s.as_str())?.to_string();
            let retry_after_ms = props
                .get("retryAfterMs")
                .and_then(|v| v.as_u64())
                .unwrap_or(500);
            let attach_event_stream = props
                .get("attachEventStream")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            Some(StreamEvent::RunConflict {
                session_id,
                run_id,
                retry_after_ms,
                attach_event_stream,
            })
        }
        "session.idle" => {
            let session_id = props.get("sessionID").and_then(|s| s.as_str())?.to_string();
            Some(StreamEvent::SessionIdle { session_id })
        }
        "session.error" => {
            let session_id = props.get("sessionID").and_then(|s| s.as_str())?.to_string();
            let error_value = props.get("error").unwrap_or(&serde_json::Value::Null);
            let error =
                extract_error_message(error_value).unwrap_or_else(|| error_value.to_string());
            Some(StreamEvent::SessionError {
                session_id,
                error_code: extract_error_code(props)
                    .or_else(|| extract_error_code(error_value))
                    .or_else(|| classify_error_code(&error)),
                error,
            })
        }
        "file.edited" => {
            let file_path = props.get("file").and_then(|s| s.as_str())?.to_string();
            Some(StreamEvent::FileEdited {
                session_id: props.get("sessionID").and_then(|s| s.as_str())?.to_string(),
                file_path,
            })
        }
        "permission.asked" => {
            let session_id = props.get("sessionID").and_then(|s| s.as_str())?.to_string();
            let request_id = props.get("requestID").and_then(|s| s.as_str())?.to_string();
            let tool = props
                .get("tool")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());
            let args = props.get("args").cloned();
            let args_source = props
                .get("argsSource")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());
            let args_integrity = props
                .get("argsIntegrity")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());
            let query = props
                .get("query")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());
            Some(StreamEvent::PermissionAsked {
                session_id,
                request_id,
                tool,
                args,
                args_source,
                args_integrity,
                query,
            })
        }
        "question.asked" => {
            // OpenCode question requests can contain multiple questions.
            // Schema reference: `QuestionRequest` -> `questions: QuestionInfo[]`
            let session_id = props.get("sessionID").and_then(|s| s.as_str())?.to_string();
            let request_id = props.get("id").and_then(|s| s.as_str())?.to_string();
            let (tool_call_id, tool_message_id) = props
                .get("tool")
                .and_then(|t| t.as_object())
                .map(|tool| {
                    (
                        tool.get("callID")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        tool.get("messageID")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                    )
                })
                .unwrap_or((None, None));

            let questions = props
                .get("questions")
                .and_then(|q| q.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|q| {
                            let question = q.get("question").and_then(|s| s.as_str())?.to_string();
                            let header = q
                                .get("header")
                                .and_then(|s| s.as_str())
                                .unwrap_or("")
                                .to_string();
                            let multiple = q.get("multiple").and_then(|b| b.as_bool());
                            let custom = q.get("custom").and_then(|b| b.as_bool());
                            let options = q
                                .get("options")
                                .and_then(|o| o.as_array())
                                .map(|opts| {
                                    opts.iter()
                                        .filter_map(|opt| {
                                            Some(QuestionChoice {
                                                label: opt.get("label")?.as_str()?.to_string(),
                                                description: opt
                                                    .get("description")
                                                    .and_then(|d| d.as_str())
                                                    .unwrap_or("")
                                                    .to_string(),
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default();

                            Some(QuestionInfo {
                                header,
                                question,
                                options,
                                multiple,
                                custom,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            Some(StreamEvent::QuestionAsked {
                session_id,
                request_id,
                questions,
                tool_call_id,
                tool_message_id,
            })
        }
        "todo.updated" => {
            let session_id = props.get("sessionID").and_then(|s| s.as_str())?.to_string();

            // Try to parse todos array, but don't fail if it's malformed
            let todos = if let Some(todos_array) = props.get("todos").and_then(|t| t.as_array()) {
                let parsed_todos: Vec<TodoItem> = todos_array
                    .iter()
                    .filter_map(|todo| {
                        Some(TodoItem {
                            id: todo.get("id")?.as_str()?.to_string(),
                            content: todo.get("content")?.as_str()?.to_string(),
                            status: todo.get("status")?.as_str()?.to_string(),
                        })
                    })
                    .collect();

                tracing::debug!(
                    "Parsed {} todos from todo.updated event for session {}",
                    parsed_todos.len(),
                    session_id
                );
                parsed_todos
            } else {
                tracing::debug!(
                    "todo.updated event missing or malformed todos array for session {}",
                    session_id
                );
                Vec::new()
            };

            Some(StreamEvent::TodoUpdated { session_id, todos })
        }
        _ => {
            // Return as raw event for other types
            tracing::debug!(
                "Unhandled event type: {} - data: {:?}",
                event_type,
                event.properties
            );
            Some(StreamEvent::Raw {
                event_type: event_type.to_string(),
                data: event.properties,
            })
        }
    }
}

fn parse_prompt_async_response(
    status: u16,
    headers: &reqwest::header::HeaderMap,
    content_type: &str,
    body: &str,
) -> std::result::Result<(), String> {
    if status == 204 {
        let run_id = headers
            .get("x-tandem-run-id")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        if !run_id.is_empty() {
            tracing::debug!("prompt_async accepted with header run_id={}", run_id);
        }
        return Ok(());
    }

    if status == 202 {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            let run_id = json.get("runID").and_then(|v| v.as_str()).unwrap_or("");
            let attach = json
                .get("attachEventStream")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            tracing::debug!(
                "prompt_async accepted with body run_id={} attach_event_stream={}",
                run_id,
                attach
            );
            return Ok(());
        }
        return Err(format!("202 response had invalid JSON body: {}", body));
    }

    if status == 409 {
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(body) {
            let retry_after_ms = json
                .get("retryAfterMs")
                .and_then(|v| v.as_u64())
                .unwrap_or(500);
            let attach = json
                .get("attachEventStream")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let active_run = json
                .get("activeRun")
                .and_then(|v| v.get("runID"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            return Err(format!(
                "run_conflict retryAfterMs={} activeRun={} attachEventStream={}",
                retry_after_ms, active_run, attach
            ));
        }
        return Err(format!("run_conflict body={}", body));
    }

    if (200..300).contains(&status) {
        let is_html = content_type.contains("text/html")
            || body.trim_start().starts_with("<!doctype html")
            || body.trim_start().starts_with("<html");
        if is_html {
            return Err(format!(
                "html_response status={} content_type={}",
                status, content_type
            ));
        }
        return Err(format!(
            "unexpected_success status={} content_type={} body={}",
            status, content_type, body
        ));
    }

    Err(format!("status={} body={}", status, body))
}

fn extract_error_message(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(message) => Some(message.clone()),
        serde_json::Value::Object(map) => {
            // Prioritize deeper, more specific error messages from providers

            // Try data.error.message (common in wrapped provider errors)
            if let Some(message) = map
                .get("data")
                .and_then(|data| data.get("error"))
                .and_then(|err| err.get("message"))
                .and_then(|m| m.as_str())
            {
                return Some(message.to_string());
            }

            // Try error.message
            if let Some(message) = map
                .get("error")
                .and_then(|err| err.get("message"))
                .and_then(|m| m.as_str())
            {
                return Some(message.to_string());
            }

            // Try data.message
            if let Some(message) = map
                .get("data")
                .and_then(|data| data.get("message"))
                .and_then(|m| m.as_str())
            {
                return Some(message.to_string());
            }

            // Try the top-level message, but if it's generic like "Provider returned error",
            // keep looking for something better or combine it.
            let top_message = map.get("message").and_then(|m| m.as_str());
            if let Some(msg) = top_message {
                if msg != "Provider returned error" && msg != "Error" {
                    return Some(msg.to_string());
                }
            }

            // If we found a generic message but also have a raw error string elsewhere
            if let Some(msg) = top_message {
                return Some(msg.to_string());
            }

            Some(value.to_string())
        }
        serde_json::Value::Null => None,
        _ => Some(value.to_string()),
    }
}

fn classify_error_code(message: &str) -> Option<String> {
    let msg = message.to_ascii_lowercase();
    if msg.contains("invalid_tool_args")
        || msg.contains("invalid tool args")
        || msg.contains("missing required 'path'")
        || msg.contains("missing required 'content'")
        || msg.contains("tool args must be a json object")
        || msg.contains("write_content_missing")
        || msg.contains("write requires `content`")
        || msg.contains("write requires non-empty `content`")
    {
        return Some("INVALID_TOOL_ARGS".to_string());
    }
    if msg.contains("workspace_not_found") || msg.contains("workspace root not found") {
        return Some("WORKSPACE_NOT_FOUND".to_string());
    }
    if msg.contains("exceeded timeout")
        || msg.contains("timed out")
        || msg.contains("stream_idle_timeout")
    {
        return Some("TOOL_TIMEOUT".to_string());
    }
    if msg.contains("(os error 3)") || msg.contains("system cannot find the path specified") {
        return Some("PATH_NOT_FOUND".to_string());
    }
    None
}

fn normalize_tool_args(
    tool: &str,
    args: &serde_json::Value,
) -> std::result::Result<serde_json::Value, String> {
    let needs_path_validation =
        matches!(tool.trim().to_ascii_lowercase().as_str(), "read" | "write");
    if !needs_path_validation {
        return Ok(args.clone());
    }

    let parsed = match args {
        serde_json::Value::Object(_) => args.clone(),
        serde_json::Value::String(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return Ok(serde_json::Value::Null);
            }
            // Plain-string path fallback.
            if !(trimmed.starts_with('{') || trimmed.starts_with('[') || trimmed.starts_with('"')) {
                let mut obj = serde_json::Map::new();
                obj.insert(
                    "path".to_string(),
                    serde_json::Value::String(trimmed.to_string()),
                );
                return Ok(serde_json::Value::Object(obj));
            }
            serde_json::from_str::<serde_json::Value>(trimmed)
                .map_err(|_| "tool args string was not valid JSON".to_string())?
        }
        // Running updates can legitimately omit args; don't fail-fast here.
        serde_json::Value::Null => return Ok(serde_json::Value::Null),
        _ => return Err("tool args must be a JSON object or string".to_string()),
    };

    let mut obj = match parsed {
        serde_json::Value::Object(map) => map,
        // Non-object JSON payloads are treated as pass-through to avoid false parser failures.
        other => return Ok(other),
    };

    if obj
        .get("path")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .unwrap_or("")
        .is_empty()
    {
        for key in [
            "file_path",
            "filePath",
            "filepath",
            "filename",
            "file",
            "target",
        ] {
            if let Some(alias) = obj
                .get(key)
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
            {
                obj.insert(
                    "path".to_string(),
                    serde_json::Value::String(alias.to_string()),
                );
                break;
            }
        }
    }

    // Do not reject here for missing path: stream payloads are frequently partial in-flight.
    Ok(serde_json::Value::Object(obj))
}

fn extract_error_code(value: &serde_json::Value) -> Option<String> {
    if let Some(code) = value.get("errorCode").and_then(|v| v.as_str()) {
        return Some(code.to_string());
    }
    if let Some(code) = value.get("code").and_then(|v| v.as_str()) {
        return Some(code.to_string());
    }
    if let Some(code) = value
        .get("error")
        .and_then(|v| v.get("code"))
        .and_then(|v| v.as_str())
    {
        return Some(code.to_string());
    }
    if let Some(code) = value
        .get("data")
        .and_then(|v| v.get("error"))
        .and_then(|v| v.get("code"))
        .and_then(|v| v.as_str())
    {
        return Some(code.to_string());
    }
    None
}

fn derive_tool_error_code(
    state_value: Option<&serde_json::Value>,
    part: &serde_json::Value,
    error: Option<&str>,
    state: &str,
) -> Option<String> {
    if let Some(code) = state_value.and_then(extract_error_code) {
        return Some(code);
    }
    if let Some(code) = extract_error_code(part) {
        return Some(code);
    }
    if matches!(state, "timeout" | "timed_out") {
        return Some("TOOL_TIMEOUT".to_string());
    }
    error.and_then(classify_error_code)
}

fn tagged_error(error_code: &str, message: &str) -> String {
    format!("[{}] {}", error_code, message)
}

fn parse_provider_catalog_response(raw: serde_json::Value) -> Option<ProviderCatalogResponse> {
    fn parse_model_map(
        raw_models: Option<&serde_json::Value>,
    ) -> HashMap<String, ProviderCatalogModel> {
        let mut models = HashMap::new();
        let Some(raw_models) = raw_models else {
            return models;
        };

        if let Some(map) = raw_models.as_object() {
            for (id, value) in map {
                let name = value
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let context = value
                    .get("limit")
                    .and_then(|v| v.get("context"))
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32)
                    .or_else(|| {
                        value
                            .get("context")
                            .and_then(|v| v.as_u64())
                            .map(|v| v as u32)
                    })
                    .or_else(|| {
                        value
                            .get("context_length")
                            .and_then(|v| v.as_u64())
                            .map(|v| v as u32)
                    });
                models.insert(
                    id.to_string(),
                    ProviderCatalogModel {
                        name,
                        limit: context.map(|context| ProviderCatalogLimit {
                            context: Some(context),
                        }),
                    },
                );
            }
            return models;
        }

        if let Some(items) = raw_models.as_array() {
            for item in items {
                if let Some(id) = item.as_str() {
                    models.insert(
                        id.to_string(),
                        ProviderCatalogModel {
                            name: Some(id.to_string()),
                            limit: None,
                        },
                    );
                    continue;
                }
                if let Some(obj) = item.as_object() {
                    let id = obj
                        .get("id")
                        .and_then(|v| v.as_str())
                        .or_else(|| obj.get("model").and_then(|v| v.as_str()));
                    if let Some(id) = id {
                        let name = obj
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                            .or_else(|| Some(id.to_string()));
                        let context = obj
                            .get("context_length")
                            .and_then(|v| v.as_u64())
                            .map(|v| v as u32);
                        models.insert(
                            id.to_string(),
                            ProviderCatalogModel {
                                name,
                                limit: context.map(|context| ProviderCatalogLimit {
                                    context: Some(context),
                                }),
                            },
                        );
                    }
                }
            }
        }
        models
    }

    fn parse_provider_entry(value: &serde_json::Value) -> Option<(ProviderCatalogEntry, bool)> {
        let obj = value.as_object()?;
        let id = obj.get("id").and_then(|v| v.as_str())?.to_string();
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let configured = obj
            .get("configured")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let models = parse_model_map(obj.get("models"));
        Some((ProviderCatalogEntry { id, name, models }, configured))
    }

    if let Some(obj) = raw.as_object() {
        let connected = obj
            .get("connected")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let source_entries = obj
            .get("all")
            .and_then(|v| v.as_array())
            .or_else(|| obj.get("providers").and_then(|v| v.as_array()));
        if let Some(entries) = source_entries {
            let mut all = Vec::new();
            let mut connected_out = connected;
            for entry in entries {
                if let Some((parsed, configured)) = parse_provider_entry(entry) {
                    if configured && !connected_out.contains(&parsed.id) {
                        connected_out.push(parsed.id.clone());
                    }
                    all.push(parsed);
                }
            }
            return Some(ProviderCatalogResponse {
                all,
                connected: connected_out,
            });
        }
    }

    if let Some(arr) = raw.as_array() {
        let mut all = Vec::new();
        let mut connected = Vec::new();
        for entry in arr {
            if let Some((parsed, configured)) = parse_provider_entry(entry) {
                if configured {
                    connected.push(parsed.id.clone());
                }
                all.push(parsed);
            }
        }
        return Some(ProviderCatalogResponse { all, connected });
    }

    None
}

fn parse_sessions_response(raw: serde_json::Value) -> Option<Vec<Session>> {
    fn decode_array(items: &[serde_json::Value]) -> Vec<Session> {
        let mut sessions = Vec::new();
        for item in items {
            match serde_json::from_value::<Session>(item.clone()) {
                Ok(session) => sessions.push(session),
                Err(err) => {
                    tracing::debug!("Skipping malformed session entry: {}", err);
                }
            }
        }
        sessions
    }

    if let Some(items) = raw.as_array() {
        return Some(decode_array(items));
    }

    if let Some(obj) = raw.as_object() {
        if let Some(items) = obj.get("items").and_then(|v| v.as_array()) {
            return Some(decode_array(items));
        }
        if let Some(items) = obj.get("sessions").and_then(|v| v.as_array()) {
            return Some(decode_array(items));
        }
        if let Some(items) = obj.get("data").and_then(|v| v.as_array()) {
            return Some(decode_array(items));
        }
    }

    None
}

fn normalize_session_for_workspace(
    mut session: Session,
    workspace_directory: Option<&str>,
) -> Session {
    if session
        .workspace_root
        .as_deref()
        .map(|s| s.trim().is_empty())
        .unwrap_or(true)
    {
        session.workspace_root = session.directory.clone();
    }

    let has_real_directory = session
        .directory
        .as_deref()
        .map(|d| {
            let trimmed = d.trim();
            !trimmed.is_empty() && trimmed != "." && trimmed != "./" && trimmed != ".\\"
        })
        .unwrap_or(false);

    if !has_real_directory {
        if let Some(workspace) = workspace_directory {
            if !workspace.trim().is_empty() {
                session.directory = Some(workspace.to_string());
                if session.workspace_root.is_none() {
                    session.workspace_root = Some(workspace.to_string());
                }
            }
        }
    }

    session
}

fn normalize_sessions_for_workspace(
    sessions: Vec<Session>,
    workspace_directory: Option<&str>,
) -> Vec<Session> {
    sessions
        .into_iter()
        .map(|session| normalize_session_for_workspace(session, workspace_directory))
        .collect()
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
        let addr = listener.local_addr().expect("local_addr");
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept");
            let mut buf = [0u8; 4096];
            let n = socket.read(&mut buf).await.expect("read");
            let req = String::from_utf8_lossy(&buf[..n]);
            let first_line = req.lines().next().unwrap_or("");
            assert!(
                first_line.contains(expected_path),
                "expected path {}, got request line {}",
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

    #[test]
    fn test_circuit_breaker() {
        let config = SidecarConfig::default();
        let mut cb = CircuitBreaker::new(config);

        assert!(cb.can_execute());

        // Record failures
        cb.record_failure();
        cb.record_failure();
        assert!(cb.can_execute()); // Still closed

        cb.record_failure();
        assert!(!cb.can_execute()); // Now open

        // Success resets
        cb.state = CircuitState::HalfOpen;
        cb.record_success();
        assert!(cb.can_execute());
    }

    #[test]
    fn test_parse_sse_event() {
        // Test OpenCode format event
        let mut buffer = String::from(
            "data: {\"type\":\"message.part.updated\",\"properties\":{\"part\":{\"sessionID\":\"ses_123\",\"messageID\":\"msg_456\",\"type\":\"text\",\"text\":\"Hello\"},\"delta\":\"Hello\"}}\n\n"
        );
        let event = parse_sse_event(&mut buffer);
        assert!(
            matches!(event, Some(StreamEvent::Content { session_id, content, .. }) if session_id == "ses_123" && content == "Hello")
        );
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_parse_sse_done() {
        let mut buffer = String::from("data: [DONE]\n\n");
        let event = parse_sse_event(&mut buffer);
        assert!(matches!(event, Some(StreamEvent::SessionIdle { .. })));
    }

    #[test]
    fn test_parse_sse_session_idle() {
        let mut buffer = String::from(
            "data: {\"type\":\"session.idle\",\"properties\":{\"sessionID\":\"ses_123\"}}\n\n",
        );
        let event = parse_sse_event(&mut buffer);
        assert!(
            matches!(event, Some(StreamEvent::SessionIdle { session_id }) if session_id == "ses_123")
        );
    }

    #[test]
    fn test_parse_sse_question_asked_multi() {
        let mut buffer = String::from(
            "data: {\"type\":\"question.asked\",\"properties\":{\"id\":\"que_123\",\"sessionID\":\"ses_123\",\"questions\":[{\"header\":\"Topic\",\"question\":\"What is the topic?\",\"multiple\":true,\"custom\":true,\"options\":[{\"label\":\"A\",\"description\":\"Option A\"},{\"label\":\"B\",\"description\":\"Option B\"}]},{\"header\":\"Tone\",\"question\":\"Pick a tone\",\"multiple\":false,\"custom\":false,\"options\":[{\"label\":\"Informative\",\"description\":\"Straightforward\"}]}]}}\n\n",
        );

        let event = parse_sse_event(&mut buffer);
        match event {
            Some(StreamEvent::QuestionAsked {
                session_id,
                request_id,
                questions,
                tool_call_id,
                tool_message_id,
            }) => {
                assert_eq!(session_id, "ses_123");
                assert_eq!(request_id, "que_123");
                assert_eq!(tool_call_id, None);
                assert_eq!(tool_message_id, None);
                assert_eq!(questions.len(), 2);
                assert_eq!(questions[0].header, "Topic");
                assert_eq!(questions[0].multiple, Some(true));
                assert_eq!(questions[0].custom, Some(true));
                assert_eq!(questions[0].options.len(), 2);
                assert_eq!(questions[0].options[0].label, "A");
                assert_eq!(questions[0].options[0].description, "Option A");
                assert_eq!(questions[1].custom, Some(false));
            }
            other => panic!("Unexpected event: {:?}", other),
        }
    }

    #[test]
    fn test_parse_sse_tool_lifecycle_with_structured_state() {
        let mut start_buffer = String::from(
            "data: {\"type\":\"message.part.updated\",\"properties\":{\"part\":{\"id\":\"call_1\",\"sessionID\":\"ses_123\",\"messageID\":\"msg_456\",\"type\":\"tool\",\"tool\":\"todo_write\",\"state\":{\"status\":\"running\",\"input\":{\"todos\":[{\"content\":\"task\"}]}}}}}\n\n",
        );
        let start = parse_sse_event(&mut start_buffer);
        match start {
            Some(StreamEvent::ToolStart {
                session_id,
                message_id,
                part_id,
                tool,
                args,
            }) => {
                assert_eq!(session_id, "ses_123");
                assert_eq!(message_id, "msg_456");
                assert_eq!(part_id, "call_1");
                assert_eq!(tool, "todo_write");
                assert_eq!(
                    args.get("todos")
                        .and_then(|t| t.as_array())
                        .map(|v| v.len()),
                    Some(1)
                );
            }
            other => panic!("Unexpected start event: {:?}", other),
        }

        let mut end_buffer = String::from(
            "data: {\"type\":\"message.part.updated\",\"properties\":{\"part\":{\"id\":\"call_1\",\"sessionID\":\"ses_123\",\"messageID\":\"msg_456\",\"type\":\"tool\",\"tool\":\"todo_write\",\"state\":{\"status\":\"completed\",\"output\":{\"todos\":[{\"id\":\"t1\",\"content\":\"task\",\"status\":\"open\"}]}}}}}\n\n",
        );
        let end = parse_sse_event(&mut end_buffer);
        match end {
            Some(StreamEvent::ToolEnd {
                session_id,
                message_id,
                part_id,
                tool,
                result,
                error,
                ..
            }) => {
                assert_eq!(session_id, "ses_123");
                assert_eq!(message_id, "msg_456");
                assert_eq!(part_id, "call_1");
                assert_eq!(tool, "todo_write");
                assert!(error.is_none());
                let result = result.unwrap_or_default();
                assert_eq!(
                    result
                        .get("todos")
                        .and_then(|t| t.as_array())
                        .map(|v| v.len()),
                    Some(1)
                );
            }
            other => panic!("Unexpected end event: {:?}", other),
        }
    }

    #[test]
    fn test_parse_sse_todo_updated_tolerates_invalid_entries() {
        let mut buffer = String::from(
            "data: {\"type\":\"todo.updated\",\"properties\":{\"sessionID\":\"ses_123\",\"todos\":[{\"id\":\"1\",\"content\":\"good\",\"status\":\"open\"},{\"content\":\"missing id\"}]}}\n\n",
        );
        let event = parse_sse_event(&mut buffer);
        match event {
            Some(StreamEvent::TodoUpdated { session_id, todos }) => {
                assert_eq!(session_id, "ses_123");
                assert_eq!(todos.len(), 1);
                assert_eq!(todos[0].id, "1");
                assert_eq!(todos[0].content, "good");
                assert_eq!(todos[0].status, "open");
            }
            other => panic!("Unexpected event: {:?}", other),
        }
    }

    #[test]
    fn test_parse_sse_mission_event_surfaces_raw_contract_payload() {
        let mut buffer = String::from(
            "data: {\"type\":\"mission.created\",\"properties\":{\"missionID\":\"m-123\",\"workItemCount\":2}}\n\n",
        );
        let event = parse_sse_event(&mut buffer);
        match event {
            Some(StreamEvent::Raw { event_type, data }) => {
                assert_eq!(event_type, "mission.created");
                assert_eq!(
                    data.get("missionID").and_then(|v| v.as_str()),
                    Some("m-123")
                );
                assert_eq!(data.get("workItemCount").and_then(|v| v.as_u64()), Some(2));
            }
            other => panic!("Unexpected event: {:?}", other),
        }
    }

    #[test]
    fn test_parse_sse_routine_events_surface_raw_contract_payload() {
        let cases = [
            (
                "routine.fired",
                serde_json::json!({
                    "routineID":"r-1",
                    "runCount":1,
                    "triggerType":"manual",
                    "firedAtMs":123
                }),
            ),
            (
                "routine.approval_required",
                serde_json::json!({
                    "routineID":"r-2",
                    "runCount":1,
                    "triggerType":"manual",
                    "reason":"manual approval required before external side effects (manual)"
                }),
            ),
            (
                "routine.blocked",
                serde_json::json!({
                    "routineID":"r-3",
                    "runCount":1,
                    "triggerType":"manual",
                    "reason":"external integrations are disabled by policy"
                }),
            ),
        ];

        for (event_type, properties) in cases {
            let payload = serde_json::json!({
                "type": event_type,
                "properties": properties
            })
            .to_string();
            let mut buffer = format!("data: {payload}\n\n");
            let event = parse_sse_event(&mut buffer);
            match event {
                Some(StreamEvent::Raw {
                    event_type: parsed_type,
                    data,
                }) => {
                    assert_eq!(parsed_type, event_type);
                    assert_eq!(data.get("runCount").and_then(|v| v.as_u64()), Some(1));
                    assert_eq!(
                        data.get("routineID").and_then(|v| v.as_str()),
                        Some(
                            properties
                                .get("routineID")
                                .and_then(|v| v.as_str())
                                .expect("routineID")
                        )
                    );
                }
                other => panic!("Unexpected event: {:?}", other),
            }
        }
    }

    #[test]
    fn test_parse_provider_catalog_legacy_array_shape() {
        let raw = serde_json::json!([
            {
                "id": "openrouter",
                "name": "OpenRouter",
                "models": ["openai/gpt-4o-mini", "z-ai/glm-5"],
                "configured": true
            },
            {
                "id": "ollama",
                "name": "Ollama",
                "models": [{"id":"llama3.1:8b","name":"llama3.1:8b","context_length":8192}],
                "configured": false
            }
        ]);
        let parsed = parse_provider_catalog_response(raw).expect("catalog");
        assert_eq!(parsed.all.len(), 2);
        assert_eq!(parsed.connected, vec!["openrouter".to_string()]);
        let openrouter = parsed
            .all
            .iter()
            .find(|p| p.id == "openrouter")
            .expect("openrouter");
        assert_eq!(openrouter.models.len(), 2);
    }

    #[test]
    fn test_parse_provider_catalog_all_shape() {
        let raw = serde_json::json!({
            "all": [
                {
                    "id": "openai",
                    "name": "OpenAI",
                    "models": {
                        "gpt-4o-mini": {"name":"gpt-4o-mini","limit":{"context":128000}}
                    }
                }
            ],
            "connected": ["openai"]
        });
        let parsed = parse_provider_catalog_response(raw).expect("catalog");
        assert_eq!(parsed.all.len(), 1);
        assert_eq!(parsed.connected, vec!["openai".to_string()]);
        let openai = &parsed.all[0];
        assert!(openai.models.contains_key("gpt-4o-mini"));
    }

    #[test]
    fn test_parse_sessions_response_object_items_shape() {
        let raw = serde_json::json!({
            "items": [
                {"id":"ses_1","title":"Chat 1","directory":".","time":{"created":1,"updated":1}},
                {"id":"ses_2","title":"Chat 2","directory":".","time":{"created":2,"updated":2}}
            ]
        });
        let sessions = parse_sessions_response(raw).expect("sessions");
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "ses_1");
    }

    #[test]
    fn test_parse_sessions_response_skips_malformed_entries() {
        let raw = serde_json::json!([
            {"id":"ses_ok","title":"OK","directory":".","time":{"created":1,"updated":1}},
            {"title":"bad missing id"},
            {"id":"ses_ok2","title":"OK 2","directory":".","time":{"created":2,"updated":2}}
        ]);
        let sessions = parse_sessions_response(raw).expect("sessions");
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].id, "ses_ok");
        assert_eq!(sessions[1].id, "ses_ok2");
    }

    #[test]
    fn test_memory_retrieval_event_serialization_tag() {
        let event = StreamEvent::MemoryRetrieval {
            session_id: "ses_123".to_string(),
            status: Some("retrieved_used".to_string()),
            used: true,
            chunks_total: 5,
            session_chunks: 1,
            history_chunks: 2,
            project_fact_chunks: 2,
            latency_ms: 33,
            query_hash: "abc123def456".to_string(),
            score_min: Some(0.1),
            score_max: Some(0.8),
            embedding_status: Some("ok".to_string()),
            embedding_reason: None,
        };

        let value = serde_json::to_value(event).unwrap();
        assert_eq!(
            value.get("type").and_then(|v| v.as_str()),
            Some("memory_retrieval")
        );
        assert_eq!(
            value.get("session_id").and_then(|v| v.as_str()),
            Some("ses_123")
        );
        assert_eq!(value.get("chunks_total").and_then(|v| v.as_u64()), Some(5));
    }

    #[test]
    fn test_parse_prompt_async_response_409_includes_retry_and_attach() {
        let headers = reqwest::header::HeaderMap::new();
        let content_type = "application/json";
        let body = r#"{
            "code":"SESSION_RUN_CONFLICT",
            "activeRun":{"runID":"run_123"},
            "retryAfterMs":500,
            "attachEventStream":"/event?sessionID=s1&runID=run_123"
        }"#;
        let err = parse_prompt_async_response(409, &headers, content_type, body)
            .expect_err("should be conflict");
        assert!(err.contains("run_conflict"));
        assert!(err.contains("retryAfterMs=500"));
        assert!(err.contains("activeRun=run_123"));
        assert!(err.contains("attachEventStream=/event?sessionID=s1&runID=run_123"));
    }

    #[test]
    fn test_parse_prompt_async_response_202_parses_run_payload() {
        let headers = reqwest::header::HeaderMap::new();
        let content_type = "application/json";
        let body = r#"{
            "runID":"run_abc",
            "attachEventStream":"/event?sessionID=s1&runID=run_abc"
        }"#;
        let result = parse_prompt_async_response(202, &headers, content_type, body);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn recover_active_run_attach_stream_uses_get_run_endpoint() {
        let base = spawn_single_response_server(
            "/session/s1/run",
            "200 OK",
            r#"{"active":{"runID":"run_42","startedAtMs":1,"lastActivityAtMs":2,"clientID":null}}"#,
        )
        .await;
        let manager = SidecarManager::new(SidecarConfig::default());
        let port = base
            .split(':')
            .next_back()
            .and_then(|s| s.parse::<u16>().ok())
            .expect("port");
        {
            let mut guard = manager.port.write().await;
            *guard = Some(port);
        }
        let attach = manager
            .recover_active_run_attach_stream("s1")
            .await
            .expect("recover");
        assert_eq!(attach, Some("/event?sessionID=s1&runID=run_42".to_string()));
    }

    #[tokio::test]
    async fn cancel_run_by_id_posts_expected_endpoint() {
        let base = spawn_single_response_server(
            "/session/s1/run/run_42/cancel",
            "200 OK",
            r#"{"ok":true,"cancelled":true}"#,
        )
        .await;
        let manager = SidecarManager::new(SidecarConfig::default());
        let port = base
            .split(':')
            .next_back()
            .and_then(|s| s.parse::<u16>().ok())
            .expect("port");
        {
            let mut guard = manager.port.write().await;
            *guard = Some(port);
        }
        let cancelled = manager
            .cancel_run_by_id("s1", "run_42")
            .await
            .expect("cancel");
        assert!(cancelled);
    }

    #[tokio::test]
    async fn cancel_run_by_id_handles_non_active_run() {
        let base = spawn_single_response_server(
            "/session/s1/run/run_missing/cancel",
            "200 OK",
            r#"{"ok":true,"cancelled":false}"#,
        )
        .await;
        let manager = SidecarManager::new(SidecarConfig::default());
        let port = base
            .split(':')
            .next_back()
            .and_then(|s| s.parse::<u16>().ok())
            .expect("port");
        {
            let mut guard = manager.port.write().await;
            *guard = Some(port);
        }
        let cancelled = manager
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
        let manager = SidecarManager::new(SidecarConfig::default());
        let port = base
            .split(':')
            .next_back()
            .and_then(|s| s.parse::<u16>().ok())
            .expect("port");
        {
            let mut guard = manager.port.write().await;
            *guard = Some(port);
        }
        let missions = manager.mission_list().await.expect("mission_list");
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
        let manager = SidecarManager::new(SidecarConfig::default());
        let port = base
            .split(':')
            .next_back()
            .and_then(|s| s.parse::<u16>().ok())
            .expect("port");
        {
            let mut guard = manager.port.write().await;
            *guard = Some(port);
        }
        let mission = manager.mission_get("m1").await.expect("mission_get");
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
        let manager = SidecarManager::new(SidecarConfig::default());
        let port = base
            .split(':')
            .next_back()
            .and_then(|s| s.parse::<u16>().ok())
            .expect("port");
        {
            let mut guard = manager.port.write().await;
            *guard = Some(port);
        }
        let mission = manager
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
        let manager = SidecarManager::new(SidecarConfig::default());
        let port = base
            .split(':')
            .next_back()
            .and_then(|s| s.parse::<u16>().ok())
            .expect("port");
        {
            let mut guard = manager.port.write().await;
            *guard = Some(port);
        }
        let result = manager
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
}
