// Tandem Sidecar Manager
// Handles spawning, lifecycle, and communication with the OpenCode sidecar process

use crate::error::{Result, TandemError};
use futures::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
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
            operation_timeout: Duration::from_secs(120),
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
}

/// Session response from OpenCode
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(default)]
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
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

/// Send message request
#[derive(Debug, Serialize)]
pub struct SendMessageRequest {
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// Streaming event from OpenCode
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum StreamEvent {
    /// Text content chunk
    Content { content: String },
    /// Tool call started
    ToolStart {
        id: String,
        tool: String,
        args: serde_json::Value,
    },
    /// Tool call completed
    ToolEnd {
        id: String,
        result: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
    },
    /// Message completed
    Done {
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
    },
    /// Error occurred
    Error { message: String },
    /// Thinking/reasoning content
    Thinking { content: String },
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
    /// Environment variables to pass to OpenCode
    env_vars: RwLock<HashMap<String, String>>,
}

impl SidecarManager {
    pub fn new(config: SidecarConfig) -> Self {
        let http_client = Client::builder()
            .timeout(config.operation_timeout)
            .build()
            .expect("Failed to create HTTP client");

        Self {
            circuit_breaker: Mutex::new(CircuitBreaker::new(config.clone())),
            config: RwLock::new(config),
            state: RwLock::new(SidecarState::Stopped),
            process: Mutex::new(None),
            port: RwLock::new(None),
            http_client,
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
                tracing::info!("Sidecar already running");
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

        // Build the command
        let mut cmd = Command::new(sidecar_path);

        // OpenCode uses 'serve' subcommand for server mode
        cmd.args([
            "serve",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ]);

        // Set working directory if workspace is configured
        if let Some(ref workspace) = config.workspace_path {
            cmd.current_dir(workspace);
            cmd.env("OPENCODE_DIR", workspace);
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
        let mut process_guard = self.process.lock().await;
        if let Some(mut child) = process_guard.take() {
            let _ = child.kill();
            let _ = child.wait();
        }

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
        let timeout = Duration::from_secs(30);

        tracing::debug!("Waiting for sidecar to be ready on port {}", port);

        while start.elapsed() < timeout {
            match self.health_check(port).await {
                Ok(_) => {
                    tracing::debug!("Sidecar is ready");
                    return Ok(());
                }
                Err(e) => {
                    tracing::trace!("Health check failed: {}, retrying...", e);
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }

        Err(TandemError::Sidecar(
            "Sidecar failed to start within timeout".to_string(),
        ))
    }

    /// Health check for the sidecar
    async fn health_check(&self, port: u16) -> Result<()> {
        let url = format!("http://127.0.0.1:{}/health", port);

        let response = self
            .http_client
            .get(&url)
            .timeout(Duration::from_secs(2))
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Health check request failed: {}", e)))?;

        if response.status().is_success() {
            Ok(())
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
    pub async fn create_session(&self, request: CreateSessionRequest) -> Result<Session> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/sessions", self.base_url().await?);

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
    pub async fn get_session(&self, session_id: &str) -> Result<Session> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/sessions/{}", self.base_url().await?, session_id);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to get session: {}", e)))?;

        self.handle_response(response).await
    }

    /// List all sessions
    pub async fn list_sessions(&self) -> Result<Vec<Session>> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/sessions", self.base_url().await?);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list sessions: {}", e)))?;

        self.handle_response(response).await
    }

    /// Delete a session
    pub async fn delete_session(&self, session_id: &str) -> Result<()> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/sessions/{}", self.base_url().await?, session_id);

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

    // ========================================================================
    // Message Handling
    // ========================================================================

    /// Send a message to a session (non-streaming)
    pub async fn send_message(
        &self,
        session_id: &str,
        request: SendMessageRequest,
    ) -> Result<Message> {
        self.check_circuit_breaker().await?;

        let url = format!(
            "{}/sessions/{}/messages",
            self.base_url().await?,
            session_id
        );

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to send message: {}", e)))?;

        self.handle_response(response).await
    }

    /// Send a message and get streaming response
    /// Returns a stream of events that should be forwarded to the frontend
    pub async fn send_message_streaming(
        &self,
        session_id: &str,
        request: SendMessageRequest,
    ) -> Result<impl futures::Stream<Item = Result<StreamEvent>>> {
        self.check_circuit_breaker().await?;

        let url = format!(
            "{}/sessions/{}/messages/stream",
            self.base_url().await?,
            session_id
        );

        let response = self
            .http_client
            .post(&url)
            .json(&request)
            .header("Accept", "text/event-stream")
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to send message: {}", e)))?;

        if !response.status().is_success() {
            self.record_failure().await;
            return Err(TandemError::Sidecar(format!(
                "Streaming request failed: {}",
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
                        buffer.push_str(&text);

                        // Parse SSE events from buffer
                        while let Some(event) = parse_sse_event(&mut buffer) {
                            yield Ok(event);
                        }
                    }
                    Err(e) => {
                        yield Err(TandemError::Sidecar(format!("Stream error: {}", e)));
                        break;
                    }
                }
            }
        })
    }

    /// Cancel ongoing generation in a session
    pub async fn cancel_generation(&self, session_id: &str) -> Result<()> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/sessions/{}/cancel", self.base_url().await?, session_id);

        let response = self
            .http_client
            .post(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to cancel: {}", e)))?;

        if response.status().is_success() {
            self.record_success().await;
            Ok(())
        } else {
            self.record_failure().await;
            Err(TandemError::Sidecar(format!(
                "Failed to cancel: {}",
                response.status()
            )))
        }
    }

    // ========================================================================
    // Model & Provider Info
    // ========================================================================

    /// List available models
    pub async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/models", self.base_url().await?);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list models: {}", e)))?;

        self.handle_response(response).await
    }

    /// List available providers
    pub async fn list_providers(&self) -> Result<Vec<ProviderInfo>> {
        self.check_circuit_breaker().await?;

        let url = format!("{}/providers", self.base_url().await?);

        let response = self
            .http_client
            .get(&url)
            .send()
            .await
            .map_err(|e| TandemError::Sidecar(format!("Failed to list providers: {}", e)))?;

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
        if response.status().is_success() {
            self.record_success().await;
            response
                .json()
                .await
                .map_err(|e| TandemError::Sidecar(format!("Failed to parse response: {}", e)))
        } else {
            self.record_failure().await;
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
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
    // SSE format: "data: {json}\n\n"
    if let Some(end_idx) = buffer.find("\n\n") {
        let event_str = buffer[..end_idx].to_string();
        *buffer = buffer[end_idx + 2..].to_string();

        // Parse the event
        for line in event_str.lines() {
            if let Some(data) = line.strip_prefix("data: ") {
                if data == "[DONE]" {
                    return Some(StreamEvent::Done { message_id: None });
                }

                match serde_json::from_str::<StreamEvent>(data) {
                    Ok(event) => return Some(event),
                    Err(e) => {
                        tracing::warn!("Failed to parse SSE event: {} - data: {}", e, data);
                        // Try to parse as raw content
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(data) {
                            if let Some(content) = value.get("content").and_then(|c| c.as_str()) {
                                return Some(StreamEvent::Content {
                                    content: content.to_string(),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    None
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
        let mut buffer = String::from("data: {\"type\":\"content\",\"content\":\"Hello\"}\n\n");
        let event = parse_sse_event(&mut buffer);
        assert!(matches!(event, Some(StreamEvent::Content { content }) if content == "Hello"));
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_parse_sse_done() {
        let mut buffer = String::from("data: [DONE]\n\n");
        let event = parse_sse_event(&mut buffer);
        assert!(matches!(event, Some(StreamEvent::Done { .. })));
    }
}
