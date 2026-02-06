// Tandem Sidecar Manager
// Handles spawning, lifecycle, and communication with the OpenCode sidecar process

use crate::error::{Result, TandemError};
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, RwLock};

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
}

impl Default for SidecarConfig {
    fn default() -> Self {
        Self {
            port: 0, // Auto-assign
            max_failures: 3,
            cooldown_duration: Duration::from_secs(30),
            operation_timeout: Duration::from_secs(300),
            heartbeat_interval: Duration::from_secs(5),
            workspace_path: None,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission: Option<Vec<PermissionRule>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time: Option<SessionTime>,
    // Legacy fields for compatibility
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default)]
    pub messages: Vec<Message>,
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
    #[serde(rename = "providerID")]
    pub provider_id: String,
    #[serde(rename = "modelID")]
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
    pub time: ProjectTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectTime {
    pub created: u64,
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
    },
    /// Session status changed
    SessionStatus { session_id: String, status: String },
    /// Session is idle (generation complete)
    SessionIdle { session_id: String },
    /// Session error
    SessionError { session_id: String, error: String },
    /// Permission requested
    PermissionAsked {
        session_id: String,
        request_id: String,
        tool: Option<String>,
        args: Option<serde_json::Value>,
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
        question_id: String,
        header: Option<String>,
        question: String,
        options: Vec<QuestionOption>,
    },
}

/// Question option from OpenCode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuestionOption {
    pub id: String,
    pub label: String,
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

// ============================================================================
// Sidecar Manager
// ============================================================================

/// Main sidecar manager
pub struct SidecarManager {
    config: RwLock<SidecarConfig>,
    state: RwLock<SidecarState>,
    process: Mutex<Option<Child>>,
    circuit_breaker: Mutex<CircuitBreaker>,
    port: RwLock<Option<u16>>,
    http_client: Client,
    /// HTTP client without global timeout for long-lived streams
    stream_client: Client,
    /// Environment variables to pass to OpenCode
    env_vars: RwLock<HashMap<String, String>>,
}

impl SidecarManager {
    pub fn new(config: SidecarConfig) -> Self {
        let http_client = Client::builder()
            .timeout(config.operation_timeout)
            .build()
            .expect("Failed to create HTTP client");

        let stream_client = Client::builder()
            .http1_only() // SSE works best with HTTP/1.1 on many platforms
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .expect("Failed to create stream client");

        Self {
            circuit_breaker: Mutex::new(CircuitBreaker::new(config.clone())),
            config: RwLock::new(config),
            state: RwLock::new(SidecarState::Stopped),
            process: Mutex::new(None),
            port: RwLock::new(None),
            http_client,
            stream_client,
            env_vars: RwLock::new(HashMap::new()),
        }
    }

    /// Get the current sidecar state
    pub async fn state(&self) -> SidecarState {
        *self.state.read().await
    }

    /// Get the port the sidecar is listening on
    pub async fn port(&self) -> Option<u16> {
        *self.port.read().await
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

        tracing::info!("Starting OpenCode sidecar from: {}", sidecar_path);

        // Find an available port
        let port = self.find_available_port().await?;

        // Get config and env vars
        let config = self.config.read().await;
        let env_vars = self.env_vars.read().await;

        tracing::debug!(
            "Sidecar env set: OPENROUTER_API_KEY={} OPENCODE_ZEN_API_KEY={} ANTHROPIC_API_KEY={} OPENAI_API_KEY={}",
            env_vars.contains_key("OPENROUTER_API_KEY"),
            env_vars.contains_key("OPENCODE_ZEN_API_KEY"),
            env_vars.contains_key("ANTHROPIC_API_KEY"),
            env_vars.contains_key("OPENAI_API_KEY")
        );

        // Build the command
        let mut cmd = Command::new(sidecar_path);

        // Hide console window on Windows
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        // OpenCode 'serve' subcommand starts a headless server
        // Use --hostname and --port flags
        cmd.args([
            "serve",
            "--hostname",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ]);

        // Set working directory if workspace is configured
        if let Some(ref workspace) = config.workspace_path {
            cmd.current_dir(workspace);
            cmd.env("OPENCODE_DIR", workspace);
        }

        // Ensure OpenCode config exists in user's config directory
        // This enables local Ollama provider support for all users
        if let Some(config_dir) = dirs::config_dir() {
            let opencode_config_dir = config_dir.join("opencode");
            let config_path = opencode_config_dir.join("config.json");

            // We update the config every time it starts to ensure new Ollama models are picked up
            // Create the directory if needed
            if let Err(e) = std::fs::create_dir_all(&opencode_config_dir) {
                tracing::warn!("Failed to create OpenCode config directory: {}", e);
            } else {
                // Discover local Ollama models dynamically
                let mut ollama_models_json = String::from("{}");
                if let Ok(output) = Command::new("ollama").arg("list").output() {
                    if output.status.success() {
                        let stdout = String::from_utf8_lossy(&output.stdout);
                        let mut models_map = serde_json::Map::new();
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
                        if let Ok(json) =
                            serde_json::to_string(&serde_json::Value::Object(models_map))
                        {
                            ollama_models_json = json;
                        }
                    }
                }

                // Write the dynamic config
                let ollama_config = format!(
                    r#"{{
  "$schema": "https://opencode.ai/config.json",
  "provider": {{
    "ollama": {{
      "npm": "@ai-sdk/openai-compatible",
      "name": "Ollama (Local)",
      "options": {{
        "baseURL": "http://localhost:11434/v1"
      }},
      "models": {}
    }}
  }}
}}"#,
                    ollama_models_json
                );

                if let Err(e) = std::fs::write(&config_path, ollama_config) {
                    tracing::warn!("Failed to write OpenCode config: {}", e);
                } else {
                    tracing::info!(
                        "Updated dynamic OpenCode config with Ollama models at: {:?}",
                        config_path
                    );
                }
            }
        }

        // Pass environment variables (including API keys)
        for (key, value) in env_vars.iter() {
            cmd.env(key, value);
        }

        // Configure stdio
        cmd.stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        // Spawn the process
        let child = cmd
            .spawn()
            .map_err(|e| TandemError::Sidecar(format!("Failed to spawn sidecar: {}", e)))?;

        // Store the process and port
        {
            let mut process_guard = self.process.lock().await;
            *process_guard = Some(child);
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
                tracing::info!("OpenCode sidecar started on port {}", port);
                Ok(())
            }
            Err(e) => {
                // Clean up on failure
                self.stop().await?;
                let mut state = self.state.write().await;
                *state = SidecarState::Failed;
                Err(e)
            }
        }
    }

    /// Stop the sidecar process
    pub async fn stop(&self) -> Result<()> {
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

        tracing::info!("Stopping OpenCode sidecar");

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
                tracing::info!("Killing OpenCode process with PID {}", pid);

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

        tracing::info!("OpenCode sidecar stopped");
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
        let config = self.config.read().await;
        if config.port != 0 {
            return Ok(config.port);
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
        let timeout = Duration::from_secs(60); // Increased timeout for slower systems

        tracing::debug!("Waiting for sidecar to be ready on port {}", port);

        let mut last_error = String::new();
        while start.elapsed() < timeout {
            match self.health_check(port).await {
                Ok(_) => {
                    tracing::info!("Sidecar is ready after {:?}", start.elapsed());
                    return Ok(());
                }
                Err(e) => {
                    last_error = e.to_string();
                    tracing::trace!("Health check failed: {}, retrying...", e);
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        tracing::error!("Sidecar failed to start. Last error: {}", last_error);
        Err(TandemError::Sidecar(format!(
            "Sidecar failed to start within 60s timeout. Last error: {}",
            last_error
        )))
    }

    /// Health check for the sidecar
    /// OpenCode exposes /global/health endpoint that returns JSON
    async fn health_check(&self, port: u16) -> Result<()> {
        let url = format!("http://127.0.0.1:{}/global/health", port);

        let response = self
            .http_client
            .get(&url)
            .timeout(Duration::from_secs(30)) // Longer timeout for first request (plugin installation)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Health check request failed: {}", e)))?;

        if response.status().is_success() {
            // Verify it returns valid JSON with healthy: true
            let body: serde_json::Value = response.json().await.map_err(|e| {
                TandemError::Sidecar(format!("Health check returned invalid JSON: {}", e))
            })?;

            if body.get("healthy").and_then(|v| v.as_bool()) == Some(true) {
                tracing::debug!("OpenCode health check passed: {:?}", body);
                Ok(())
            } else {
                Err(TandemError::Sidecar(format!(
                    "Health check returned unhealthy: {:?}",
                    body
                )))
            }
        } else {
            Err(TandemError::Sidecar(format!(
                "Health check returned status: {}",
                response.status()
            )))
        }
    }

    // ========================================================================
    // Session Management
    // ========================================================================

    /// Create a new chat session
    /// OpenCode API: POST /session
    pub async fn create_session(&self, request: CreateSessionRequest) -> Result<Session> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/session", self.base_url().await?);
        tracing::debug!("Creating session at: {}", url);

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to create session: {}", e)))?;

        self.handle_response(response).await
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

        self.handle_response(response).await
    }

    /// List all sessions
    /// OpenCode API: GET /session
    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/session", self.base_url().await?);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list sessions: {}", e)))?;

        self.handle_response(response).await
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

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list projects: {}", e)))?;

        self.handle_response(response).await
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
    pub async fn send_message(&self, session_id: &str, request: SendMessageRequest) -> Result<()> {
        self.check_circuit_breaker().await?;

        let base = self.base_url().await?;
        let url = format!("{}/session/{}/prompt_async", base, session_id);
        let fallback_url = format!("{}/api/session/{}/prompt_async", base, session_id);

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

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to send message: {}", e)))?;

        let status = response.status();
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        // prompt_async should return 204 No Content on success
        if status.as_u16() == 204 {
            self.record_success().await;
            return Ok(());
        }

        if status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let is_html = content_type.contains("text/html")
                || body.trim_start().starts_with("<!doctype html")
                || body.trim_start().starts_with("<html");

            if is_html {
                tracing::warn!(
                    "prompt_async returned HTML from {} (status {}), retrying via {}",
                    url,
                    status,
                    fallback_url
                );

                let response = self
                    .http_client
                    .post(&fallback_url)
                    .json(&request)
                    .send()
                    .await
                    .map_err(|e| TandemError::Sidecar(format!("Failed to send message: {}", e)))?;

                let status = response.status();
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("")
                    .to_string();

                if status.as_u16() == 204 {
                    self.record_success().await;
                    return Ok(());
                }

                let body = response.text().await.unwrap_or_default();
                tracing::warn!(
                    "Sidecar prompt_async failed (fallback): status={} content_type={} body={}",
                    status,
                    content_type,
                    body
                );
                self.record_failure().await;
                return Err(TandemError::Sidecar(format!(
                    "Failed to send message: {}",
                    body
                )));
            }

            tracing::warn!(
                "Sidecar prompt_async unexpected success status: status={} content_type={} body={}",
                status,
                content_type,
                body
            );
            self.record_failure().await;
            return Err(TandemError::Sidecar(format!(
                "Failed to send message: {}",
                body
            )));
        }

        self.record_failure().await;
        let body = response.text().await.unwrap_or_default();
        tracing::warn!(
            "Sidecar prompt_async failed: status={} body={}",
            status,
            body
        );
        Err(TandemError::Sidecar(format!(
            "Failed to send message: {}",
            body
        )))
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
            self.record_failure().await;
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
    // Helpers
    // ========================================================================

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

        self.handle_response(response).await.map_err(|e| {
            tracing::warn!("Failed to parse providers from {}: {}", url, e);
            e
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
            self.record_failure().await;
            let body = response.text().await.unwrap_or_default();
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
                tracing::info!("Killing OpenCode sidecar on drop");
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

    match event.event_type.as_str() {
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
                            tracing::debug!(
                                "Skipping text/reasoning part without delta (likely user message)"
                            );
                            return None;
                        }

                        // If it's an assistant message with text but no delta, we should probably emit it
                        // to ensure we don't miss content.
                        tracing::debug!("Emitting text part without delta for assistant");
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
                    let state = state_value
                        .and_then(|s| s.get("status"))
                        .and_then(|s| s.as_str())
                        .unwrap_or_else(|| {
                            part.get("state")
                                .and_then(|s| s.as_str())
                                .unwrap_or("pending")
                        });
                    let args = state_value
                        .and_then(|s| s.get("input"))
                        .cloned()
                        .or_else(|| part.get("args").cloned())
                        .unwrap_or(serde_json::Value::Null);

                    match state {
                        "pending" | "running" => Some(StreamEvent::ToolStart {
                            session_id,
                            message_id,
                            part_id,
                            tool,
                            args,
                        }),
                        "completed" | "failed" | "error" => {
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
                            Some(StreamEvent::ToolEnd {
                                session_id,
                                message_id,
                                part_id,
                                tool,
                                result,
                                error,
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
                    return Some(StreamEvent::SessionError { session_id, error });
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
        "session.idle" => {
            let session_id = props.get("sessionID").and_then(|s| s.as_str())?.to_string();
            Some(StreamEvent::SessionIdle { session_id })
        }
        "session.error" => {
            let session_id = props.get("sessionID").and_then(|s| s.as_str())?.to_string();
            let error_value = props.get("error").unwrap_or(&serde_json::Value::Null);
            let error =
                extract_error_message(error_value).unwrap_or_else(|| error_value.to_string());
            Some(StreamEvent::SessionError { session_id, error })
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
            Some(StreamEvent::PermissionAsked {
                session_id,
                request_id,
                tool,
                args,
            })
        }
        "question.asked" => {
            let session_id = props.get("sessionID").and_then(|s| s.as_str())?.to_string();
            let question_id = props
                .get("questionID")
                .and_then(|s| s.as_str())?
                .to_string();
            let header = props
                .get("header")
                .and_then(|s| s.as_str())
                .map(|s| s.to_string());
            let question = props.get("question").and_then(|s| s.as_str())?.to_string();

            let options = props
                .get("options")
                .and_then(|o| o.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|opt| {
                            Some(QuestionOption {
                                id: opt.get("id")?.as_str()?.to_string(),
                                label: opt.get("label")?.as_str()?.to_string(),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();

            Some(StreamEvent::QuestionAsked {
                session_id,
                question_id,
                header,
                question,
                options,
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
            tracing::warn!(
                "Unhandled event type: {} - data: {:?}",
                event.event_type,
                event.properties
            );
            Some(StreamEvent::Raw {
                event_type: event.event_type,
                data: event.properties,
            })
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
