// Ralph Loop Service
// Core orchestration logic for iterative task execution
// Inspired by: https://raw.githubusercontent.com/Th0rgal/open-ralph-wiggum/refs/heads/master/ralph.ts

use crate::error::{Result, TandemError};
use crate::ralph::storage::RalphStorage;
use crate::ralph::types::{
    IterationRecord, RalphConfig, RalphRunStatus, RalphState, RalphStateSnapshot,
};
use crate::sidecar::{SendMessageRequest, SidecarManager, StreamEvent};
use crate::stream_hub::StreamHub;
use regex::Regex;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Notify, RwLock};
use tokio_util::sync::CancellationToken;

/// Manages all active Ralph Loop runs
pub struct RalphLoopManager {
    runs: RwLock<HashMap<String, Arc<RalphRunHandle>>>,
}

impl RalphLoopManager {
    pub fn new() -> Self {
        Self {
            runs: RwLock::new(HashMap::new()),
        }
    }

    /// Start a new Ralph Loop
    pub async fn start(
        &self,
        session_id: String,
        prompt: String,
        config: RalphConfig,
        workspace_path: PathBuf,
        sidecar: Arc<SidecarManager>,
        stream_hub: Arc<StreamHub>,
    ) -> Result<String> {
        let run_id = format!(
            "ralph_{}",
            uuid::Uuid::new_v4().to_string().replace("-", "")[..16].to_string()
        );

        let state = RalphState::new(
            run_id.clone(),
            session_id.clone(),
            prompt.clone(),
            config.clone(),
        );

        let storage = RalphStorage::new(&workspace_path);
        storage.save_state(&state)?;

        let handle = Arc::new(RalphRunHandle::new(
            run_id.clone(),
            session_id,
            prompt,
            config,
            storage,
            sidecar,
            stream_hub,
            workspace_path,
        ));

        {
            let mut runs = self.runs.write().await;
            runs.insert(run_id.clone(), handle.clone());
        }

        // Spawn the loop task
        tokio::spawn(async move {
            if let Err(e) = handle.run_loop().await {
                tracing::error!("Ralph loop {} failed: {}", handle.run_id, e);
                let mut state = handle.state.write().await;
                state.status = RalphRunStatus::Error;
                state.error_message = Some(e.to_string());
                state.active = false;
                state.ended_at = Some(chrono::Utc::now());
                let _ = handle.storage.save_state(&*state);
            }
        });

        Ok(run_id)
    }

    /// Cancel a running loop
    pub async fn cancel(&self, run_id: &str) -> Result<()> {
        let handle = {
            let runs = self.runs.read().await;
            runs.get(run_id).cloned()
        };

        if let Some(handle) = handle {
            handle.cancel().await;
            Ok(())
        } else {
            Err(TandemError::Ralph(format!("Run {} not found", run_id)))
        }
    }

    /// Pause a running loop (stops after current iteration)
    pub async fn pause(&self, run_id: &str) -> Result<()> {
        let handle = {
            let runs = self.runs.read().await;
            runs.get(run_id).cloned()
        };

        if let Some(handle) = handle {
            handle.pause().await;
            Ok(())
        } else {
            Err(TandemError::Ralph(format!("Run {} not found", run_id)))
        }
    }

    /// Resume a paused loop
    pub async fn resume(&self, run_id: &str) -> Result<()> {
        let handle = {
            let runs = self.runs.read().await;
            runs.get(run_id).cloned()
        };

        if let Some(handle) = handle {
            handle.resume().await;
            Ok(())
        } else {
            Err(TandemError::Ralph(format!("Run {} not found", run_id)))
        }
    }

    /// Add context to be injected in next iteration
    pub async fn add_context(&self, run_id: &str, text: String) -> Result<()> {
        let handle = {
            let runs = self.runs.read().await;
            runs.get(run_id).cloned()
        };

        if let Some(handle) = handle {
            handle.add_context(text).await
        } else {
            Err(TandemError::Ralph(format!("Run {} not found", run_id)))
        }
    }

    /// Get current status
    pub async fn status(&self, run_id: &str) -> Result<RalphStateSnapshot> {
        let handle = {
            let runs = self.runs.read().await;
            runs.get(run_id).cloned()
        };

        if let Some(handle) = handle {
            handle.get_snapshot().await
        } else {
            Err(TandemError::Ralph(format!("Run {} not found", run_id)))
        }
    }

    /// Get iteration history
    pub async fn history(&self, run_id: &str, limit: usize) -> Result<Vec<IterationRecord>> {
        let handle = {
            let runs = self.runs.read().await;
            runs.get(run_id).cloned()
        };

        if let Some(handle) = handle {
            handle.get_history(limit).await
        } else {
            Err(TandemError::Ralph(format!("Run {} not found", run_id)))
        }
    }

    /// Clean up completed runs
    pub async fn cleanup_completed(&self) {
        let mut runs = self.runs.write().await;
        runs.retain(|_, handle| {
            let state = handle.state.try_read();
            match state {
                Ok(state) => state.active,
                Err(_) => true, // Keep if we can't acquire lock
            }
        });
    }
}

/// Handle for a single Ralph Loop run
pub struct RalphRunHandle {
    pub run_id: String,
    session_id: String,
    prompt: String,
    config: RalphConfig,
    storage: RalphStorage,
    sidecar: Arc<SidecarManager>,
    stream_hub: Arc<StreamHub>,
    workspace_path: PathBuf,
    pub state: RwLock<RalphState>,
    cancel_token: CancellationToken,
    pause_notify: Notify,
    is_paused: RwLock<bool>,
    last_errors: RwLock<Vec<String>>,
    consecutive_no_changes: RwLock<u32>,
}

impl RalphRunHandle {
    fn new(
        run_id: String,
        session_id: String,
        prompt: String,
        config: RalphConfig,
        storage: RalphStorage,
        sidecar: Arc<SidecarManager>,
        stream_hub: Arc<StreamHub>,
        workspace_path: PathBuf,
    ) -> Self {
        let state = RalphState::new(
            run_id.clone(),
            session_id.clone(),
            prompt.clone(),
            config.clone(),
        );

        Self {
            run_id,
            session_id,
            prompt,
            config,
            storage,
            sidecar,
            stream_hub,
            workspace_path,
            state: RwLock::new(state),
            cancel_token: CancellationToken::new(),
            pause_notify: Notify::new(),
            is_paused: RwLock::new(false),
            last_errors: RwLock::new(Vec::new()),
            consecutive_no_changes: RwLock::new(0),
        }
    }

    /// Main loop execution
    async fn run_loop(&self) -> Result<()> {
        loop {
            // Check for cancellation
            if self.cancel_token.is_cancelled() {
                tracing::info!("Ralph loop {} cancelled", self.run_id);
                break;
            }

            // Check for pause - wait if paused
            {
                let is_paused = *self.is_paused.read().await;
                if is_paused {
                    tracing::info!("Ralph loop {} paused", self.run_id);
                    self.pause_notify.notified().await;
                    tracing::info!("Ralph loop {} resumed", self.run_id);
                    continue;
                }
            }

            // Get current state
            let current_iteration = {
                let state = self.state.read().await;
                state.iteration
            };

            // Check max iterations
            if current_iteration > self.config.max_iterations {
                tracing::warn!("Ralph loop {} reached max iterations", self.run_id);
                let mut state = self.state.write().await;
                state.status = RalphRunStatus::Completed;
                state.active = false;
                state.ended_at = Some(chrono::Utc::now());
                self.storage.save_state(&*state)?;
                break;
            }

            // Run single iteration
            let iteration_result = self.run_iteration(current_iteration).await;

            match iteration_result {
                Ok((should_stop, completion_detected)) => {
                    if should_stop && completion_detected {
                        tracing::info!("Ralph loop {} completed successfully", self.run_id);
                        let mut state = self.state.write().await;
                        state.status = RalphRunStatus::Completed;
                        state.active = false;
                        state.ended_at = Some(chrono::Utc::now());
                        self.storage.save_state(&*state)?;
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("Ralph loop {} iteration failed: {}", self.run_id, e);
                    let mut state = self.state.write().await;
                    state.status = RalphRunStatus::Error;
                    state.error_message = Some(e.to_string());
                    state.active = false;
                    state.ended_at = Some(chrono::Utc::now());
                    self.storage.save_state(&*state)?;
                    return Err(e);
                }
            }

            // Increment iteration counter
            {
                let mut state = self.state.write().await;
                state.iteration += 1;
                self.storage.save_state(&*state)?;
            }

            // Small delay between iterations to prevent overwhelming
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        Ok(())
    }

    /// Run a single iteration
    async fn run_iteration(&self, iteration: u32) -> Result<(bool, bool)> {
        let start_time = Instant::now();
        let iteration_start = chrono::Utc::now();

        // Build the prompt
        let prompt = self.build_prompt(iteration).await;

        // Capture git state before
        let git_before = self.capture_git_state().await;

        // Send message to sidecar
        let request = SendMessageRequest::text(prompt);
        self.sidecar
            .append_message_and_start_run(&self.session_id, request)
            .await?;

        // Subscribe to events and wait for completion
        let (content, tools_used, errors) = self.wait_for_completion().await?;

        // Capture git state after
        let git_after = self.capture_git_state().await;
        let files_modified = self.compute_file_changes(&git_before, &git_after);

        // Check for completion promise
        let completion_detected = self.detect_completion(&content);

        // Detect struggle
        let struggle = self.detect_struggle(&files_modified, &errors).await;

        // Record iteration
        let record = IterationRecord {
            iteration,
            started_at: iteration_start,
            ended_at: chrono::Utc::now(),
            duration_ms: start_time.elapsed().as_millis() as u64,
            completion_detected,
            tools_used,
            files_modified: files_modified.clone(),
            errors: errors.clone(),
            context_injected: self.storage.load_context().ok().flatten(),
        };

        self.storage.append_history(&record)?;
        self.storage.clear_context().ok();

        // Update state
        {
            let mut state = self.state.write().await;
            state.last_iteration_duration_ms = Some(record.duration_ms);
            state.struggle_detected = struggle;
        }

        // Store errors for next iteration
        {
            let mut last_errors = self.last_errors.write().await;
            *last_errors = errors;
        }

        // Determine if we should stop
        let should_stop =
            completion_detected && iteration >= self.config.min_iterations && !struggle;

        Ok((should_stop, completion_detected))
    }

    /// Build the iteration prompt
    async fn build_prompt(&self, iteration: u32) -> String {
        let base_prompt = &self.prompt;
        let context = self.storage.load_context().ok().flatten();
        let last_errors = self.last_errors.read().await.clone();
        let struggle = *self.consecutive_no_changes.read().await >= 3;

        let mut prompt = String::new();

        // Iteration header
        prompt.push_str(&format!("=== ITERATION {} ===\n\n", iteration));

        // Base task
        prompt.push_str("TASK:\n");
        prompt.push_str(base_prompt);
        prompt.push_str("\n\n");

        // Additional context (user injected)
        if let Some(ctx) = context {
            prompt.push_str("ADDITIONAL CONTEXT:\n");
            prompt.push_str(&ctx);
            prompt.push_str("\n\n");
        }

        // Struggle hint
        if struggle {
            prompt.push_str("NOTE: Progress appears to be stalled. Consider:\n");
            prompt.push_str("- Trying a different approach\n");
            prompt.push_str("- Breaking the task into smaller steps\n");
            prompt.push_str("- Checking for errors or issues\n");
            prompt.push_str("\n");
        }

        // Previous errors
        if !last_errors.is_empty() {
            prompt.push_str("ERRORS FROM LAST ITERATION:\n");
            for err in &last_errors {
                prompt.push_str(&format!("- {}\n", err));
            }
            prompt.push_str("\n");
        }

        // Plan mode instruction
        if self.config.plan_mode_guard {
            prompt.push_str("PLAN MODE ACTIVE:\n");
            prompt.push_str(
                "Do not execute changes directly. Stage operations and update the plan markdown. ",
            );
            prompt.push_str("Wait for user approval before executing.\n\n");
        }

        // Task completion instruction
        prompt.push_str("IMPORTANT: As you verify that tasks are done, you MUST use the `todowrite` tool to mark them as completed.\n");
        prompt.push_str("Call `todowrite` with status=\"completed\" for the specific task ID in the TODO list.\n\n");

        // Completion token instruction
        prompt.push_str(&format!(
            "When the task is genuinely complete, output: <promise>{}</promise>\n",
            self.config.completion_promise
        ));
        prompt.push_str("Only output this token when the task is truly finished. Do not lie to exit the loop.\n");

        prompt
    }

    /// Wait for iteration completion via SSE events
    async fn wait_for_completion(&self) -> Result<(String, HashMap<String, u32>, Vec<String>)> {
        let mut stream = self.stream_hub.subscribe();

        let mut content = String::new();
        let mut tools_used: HashMap<String, u32> = HashMap::new();
        let mut errors: Vec<String> = Vec::new();

        loop {
            match stream.recv().await {
                Ok(env) => {
                    let event = env.payload;
                    match &event {
                        StreamEvent::Content {
                            session_id, delta, ..
                        } => {
                            if session_id == &self.session_id {
                                if let Some(text) = delta {
                                    content.push_str(text);
                                }
                            }
                        }
                        StreamEvent::ToolStart {
                            session_id, tool, ..
                        } => {
                            if session_id == &self.session_id {
                                *tools_used.entry(tool.clone()).or_insert(0) += 1;
                            }
                        }
                        StreamEvent::ToolEnd {
                            session_id, error, ..
                        } => {
                            if session_id == &self.session_id {
                                if let Some(err) = error {
                                    errors.push(err.clone());
                                }
                            }
                        }
                        StreamEvent::SessionIdle { session_id } => {
                            if session_id == &self.session_id {
                                break;
                            }
                        }
                        StreamEvent::SessionError { session_id, error } => {
                            if session_id == &self.session_id {
                                errors.push(error.clone());
                                break;
                            }
                        }
                        _ => {}
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                    tracing::warn!("Ralph stream lagged by {} events", skipped);
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }

        Ok((content, tools_used, errors))
    }

    /// Detect completion promise in content
    fn detect_completion(&self, content: &str) -> bool {
        let pattern = format!(
            r"(?i)<promise>\s*{}\s*</promise>",
            regex::escape(&self.config.completion_promise)
        );
        let re = Regex::new(&pattern).unwrap();
        re.is_match(content)
    }

    /// Detect if the loop is struggling
    async fn detect_struggle(&self, files_modified: &[String], errors: &[String]) -> bool {
        let mut consecutive = self.consecutive_no_changes.write().await;

        // Check for no file changes
        if files_modified.is_empty() {
            *consecutive += 1;
        } else {
            *consecutive = 0;
        }

        // Check for repeated errors
        let last_errors = self.last_errors.read().await;
        let has_repeated_errors = !errors.is_empty()
            && !last_errors.is_empty()
            && errors.iter().any(|e| last_errors.contains(e));

        *consecutive >= 3 || has_repeated_errors
    }

    /// Capture current git state
    async fn capture_git_state(&self) -> Vec<String> {
        let output = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(&self.workspace_path)
            .output();

        match output {
            Ok(output) if output.status.success() => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.lines().map(|s| s.to_string()).collect()
            }
            _ => Vec::new(),
        }
    }

    /// Compute file changes between two git states
    fn compute_file_changes(&self, before: &[String], after: &[String]) -> Vec<String> {
        let before_set: std::collections::HashSet<_> = before.iter().cloned().collect();
        let after_set: std::collections::HashSet<_> = after.iter().cloned().collect();

        after_set.difference(&before_set).cloned().collect()
    }

    // Control methods
    pub async fn cancel(&self) {
        self.cancel_token.cancel();
        let mut state = self.state.write().await;
        if state.active {
            state.status = RalphRunStatus::Cancelled;
            state.active = false;
            state.ended_at = Some(chrono::Utc::now());
            let _ = self.storage.save_state(&*state);
        }
    }

    pub async fn pause(&self) {
        let mut is_paused = self.is_paused.write().await;
        *is_paused = true;
        let mut state = self.state.write().await;
        state.status = RalphRunStatus::Paused;
        let _ = self.storage.save_state(&*state);
    }

    pub async fn resume(&self) {
        let mut is_paused = self.is_paused.write().await;
        *is_paused = false;
        self.pause_notify.notify_one();
        let mut state = self.state.write().await;
        state.status = RalphRunStatus::Running;
        let _ = self.storage.save_state(&*state);
    }

    pub async fn add_context(&self, text: String) -> Result<()> {
        self.storage.save_context(&text)
    }

    pub async fn get_snapshot(&self) -> Result<RalphStateSnapshot> {
        let state = self.state.read().await;
        let history = self.storage.load_history(50)?;
        let total_files_modified: usize = history.iter().map(|r| r.files_modified.len()).sum();
        Ok(state.to_snapshot(history.len(), total_files_modified))
    }

    pub async fn get_history(&self, limit: usize) -> Result<Vec<IterationRecord>> {
        self.storage.load_history(limit)
    }
}

impl Default for RalphLoopManager {
    fn default() -> Self {
        Self::new()
    }
}
