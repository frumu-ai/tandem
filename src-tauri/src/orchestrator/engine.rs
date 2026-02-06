// Orchestrator Engine
// Main orchestration loop: plan -> dispatch -> collect -> validate -> update
// See: docs/orchestration_plan.md

use crate::error::{Result, TandemError};
use crate::orchestrator::{
    agents::{AgentPrompts, ParsedTask, PlannerConstraints},
    budget::{BudgetCheckResult, BudgetTracker},
    policy::{PolicyDecision, PolicyEngine},
    scheduler::TaskScheduler,
    store::OrchestratorStore,
    types::*,
};
use crate::sidecar::SidecarManager;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

// ============================================================================
// Orchestrator Engine
// ============================================================================

/// Main orchestration engine
#[derive(Clone)]
pub struct OrchestratorEngine {
    run_id: String,
    /// Run state
    run: Arc<RwLock<Run>>,
    /// Budget tracker
    budget_tracker: Arc<RwLock<BudgetTracker>>,
    /// Policy engine
    policy: Arc<PolicyEngine>,
    /// Persistence store
    store: Arc<OrchestratorStore>,
    /// Sidecar manager for sub-agent calls
    sidecar: Arc<SidecarManager>,
    /// Workspace path
    workspace_path: PathBuf,
    /// Cancellation token
    cancel_token: Arc<StdMutex<CancellationToken>>,
    /// Pause signal
    pause_signal: Arc<RwLock<bool>>,
    /// Event sender for UI updates
    event_tx: mpsc::UnboundedSender<OrchestratorEvent>,
    task_semaphore: Arc<Semaphore>,
    llm_semaphore: Arc<Semaphore>,
    task_sessions: Arc<RwLock<HashMap<String, String>>>,
    #[cfg(test)]
    test_task_executor: Option<
        Arc<
            dyn Fn(OrchestratorEngine, Task) -> futures::future::BoxFuture<'static, Result<()>>
                + Send
                + Sync,
        >,
    >,
}

impl OrchestratorEngine {
    /// Create a new orchestrator engine
    pub fn new(
        run: Run,
        policy: PolicyEngine,
        store: OrchestratorStore,
        sidecar: Arc<SidecarManager>,
        workspace_path: PathBuf,
        event_tx: mpsc::UnboundedSender<OrchestratorEvent>,
    ) -> Self {
        let run_id = run.run_id.clone();
        let mut budget_tracker = BudgetTracker::from_budget(run.budget.clone());
        budget_tracker.set_active(matches!(
            run.status,
            RunStatus::Planning | RunStatus::Executing
        ));
        let max_parallel_tasks = run.config.max_parallel_tasks.max(1) as usize;
        let llm_parallel = run.config.llm_parallel.max(1) as usize;
        let pause_signal = Arc::new(RwLock::new(matches!(run.status, RunStatus::Paused)));

        Self {
            run_id,
            run: Arc::new(RwLock::new(run)),
            budget_tracker: Arc::new(RwLock::new(budget_tracker)),
            policy: Arc::new(policy),
            store: Arc::new(store),
            sidecar,
            workspace_path,
            cancel_token: Arc::new(StdMutex::new(CancellationToken::new())),
            pause_signal,
            event_tx,
            task_semaphore: Arc::new(Semaphore::new(max_parallel_tasks)),
            llm_semaphore: Arc::new(Semaphore::new(llm_parallel)),
            task_sessions: Arc::new(RwLock::new(HashMap::new())),
            #[cfg(test)]
            test_task_executor: None,
        }
    }

    #[cfg(test)]
    pub fn with_test_task_executor<F, Fut>(mut self, f: F) -> Self
    where
        F: Fn(OrchestratorEngine, Task) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        self.test_task_executor = Some(Arc::new(move |engine, task| Box::pin(f(engine, task))));
        self
    }

    /// Start the orchestration run
    pub async fn start(&self) -> Result<()> {
        // Phase 1: Planning
        self.run_planning_phase().await?;

        // Wait for approval (handled externally via approve() call)
        // The run status will be AwaitingApproval

        Ok(())
    }

    /// Resume execution after approval
    pub async fn execute(&self) -> Result<()> {
        let upgraded_limits = {
            let mut run = self.run.write().await;
            Self::upgrade_legacy_limits(&mut run)
        };
        if upgraded_limits {
            self.update_budget_limits().await;
        }

        {
            let run = self.run.read().await;
            if run.status != RunStatus::AwaitingApproval
                && run.status != RunStatus::Paused
                && run.status != RunStatus::Executing
            {
                return Err(TandemError::InvalidOperation(
                    "Run is not awaiting approval, paused, or executing".to_string(),
                ));
            }
        }

        // Update status to Executing if not already
        {
            let mut run = self.run.write().await;
            if run.status != RunStatus::Executing {
                run.status = RunStatus::Executing;
            }
        }
        self.budget_tracker.write().await.set_active(true);

        // Save state immediately so UI reflects the change
        self.save_state().await?;

        // Run the execution loop
        self.run_execution_loop().await
    }

    /// Run the planning phase
    async fn run_planning_phase(&self) -> Result<()> {
        // Update status
        {
            let mut run = self.run.write().await;
            run.status = RunStatus::Planning;
        }
        self.budget_tracker.write().await.set_active(true);

        if self.cancel_if_requested().await? {
            return Ok(());
        }

        // Emit event
        self.emit_event(OrchestratorEvent::PlanningStarted {
            run_id: self.get_run_id().await,
            timestamp: chrono::Utc::now(),
        });

        // Build workspace summary (simplified for now)
        let workspace_summary = self.build_workspace_summary().await?;

        if self.cancel_if_requested().await? {
            return Ok(());
        }

        // Get constraints
        let config = {
            let run = self.run.read().await;
            run.config.clone()
        };

        let constraints = PlannerConstraints {
            max_tasks: 12,
            research_enabled: config.enable_research,
        };

        // Build planner prompt
        let objective = {
            let run = self.run.read().await;
            run.objective.clone()
        };

        let prompt =
            AgentPrompts::build_planner_prompt(&objective, &workspace_summary, &constraints);

        // Call planner via sidecar
        let session_id = {
            let run = self.run.read().await;
            run.session_id.clone()
        };

        // Record budget
        {
            let mut tracker = self.budget_tracker.write().await;
            tracker.record_subagent_run();
        }

        // Send message and wait for response
        let response = self.call_agent(None, &session_id, &prompt).await?;

        // Record tokens (estimate from response length)
        {
            let mut tracker = self.budget_tracker.write().await;
            tracker.record_tokens(None, Some(response.len()));
        }

        if self.cancel_if_requested().await? {
            return Ok(());
        }

        // Parse tasks from response
        let parsed_tasks = AgentPrompts::parse_task_list(&response).ok_or_else(|| {
            TandemError::ParseError("Failed to parse task list from planner output".to_string())
        })?;

        // Convert to Task objects
        let tasks: Vec<Task> = parsed_tasks.into_iter().map(Task::from).collect();

        // Validate task graph
        TaskScheduler::validate(&tasks).map_err(|e| TandemError::ValidationError(e.to_string()))?;

        // Update run with tasks
        {
            let mut run = self.run.write().await;
            run.tasks = tasks;
            run.status = RunStatus::AwaitingApproval;
        }
        self.budget_tracker.write().await.set_active(false);

        {
            let run = self.run.read().await;
            for task in &run.tasks {
                self.emit_task_trace(&task.id, task.session_id.as_deref(), "TASK_CREATED", None);
            }
        }

        // Save state
        self.save_state().await?;

        // Emit event
        let task_count = {
            let run = self.run.read().await;
            run.tasks.len()
        };

        self.emit_event(OrchestratorEvent::PlanGenerated {
            run_id: self.get_run_id().await,
            task_count,
            timestamp: chrono::Utc::now(),
        });

        Ok(())
    }

    /// Run the main execution loop
    async fn run_execution_loop(&self) -> Result<()> {
        let mut join_set: JoinSet<Result<()>> = JoinSet::new();
        loop {
            {
                let mut tracker = self.budget_tracker.write().await;
                tracker.set_active(!join_set.is_empty());
            }

            // Check for cancellation
            if self.is_cancelled() {
                join_set.abort_all();
                self.stop_active_generations().await;
                self.handle_cancellation().await?;
                return Ok(());
            }

            // Check for pause
            if *self.pause_signal.read().await {
                join_set.abort_all();
                self.stop_active_generations().await;
                self.handle_pause().await?;
                return Ok(());
            }

            // Check budget
            let budget_result = {
                let mut tracker = self.budget_tracker.write().await;
                tracker.check()
            };

            match budget_result {
                BudgetCheckResult::Exceeded { dimension, reason } => {
                    join_set.abort_all();
                    self.stop_active_generations().await;
                    self.handle_budget_exceeded(&dimension, &reason).await?;
                    return Ok(());
                }
                BudgetCheckResult::Warning {
                    dimension,
                    percentage,
                } => {
                    // Log warning but continue
                    tracing::warn!(
                        "Budget warning: {} at {:.0}%",
                        dimension,
                        percentage * 100.0
                    );
                }
                BudgetCheckResult::Ok => {}
            }

            while let Some(task_result) = join_set.try_join_next() {
                match task_result {
                    Ok(inner) => {
                        if let Err(e) = inner {
                            tracing::error!("Task returned error: {}", e);
                        }
                    }
                    Err(e) => tracing::error!("Task join returned error: {}", e),
                }
            }

            let runnable_task_ids = {
                let run = self.run.read().await;
                TaskScheduler::get_all_runnable(&run.tasks)
                    .into_iter()
                    .map(|t| t.id.clone())
                    .collect::<Vec<_>>()
            };

            let mut scheduled_any = false;

            for task_id in runnable_task_ids {
                self.emit_task_trace(
                    &task_id,
                    None,
                    "PERMIT_REQUESTED",
                    Some("task_semaphore".to_string()),
                );
                let task_permit = match self.task_semaphore.clone().try_acquire_owned() {
                    Ok(permit) => permit,
                    Err(_) => break,
                };
                self.emit_task_trace(
                    &task_id,
                    None,
                    "PERMIT_ACQUIRED",
                    Some("task_semaphore".to_string()),
                );

                let task = {
                    let mut run = self.run.write().await;
                    if let Some(idx) = run.tasks.iter().position(|t| t.id == task_id) {
                        if run.tasks[idx].state != TaskState::Pending {
                            continue;
                        }
                        run.tasks[idx].state = TaskState::InProgress;
                        run.tasks[idx].clone()
                    } else {
                        continue;
                    }
                };

                let engine = self.clone();
                join_set.spawn(async move {
                    let _permit = task_permit;
                    engine.execute_task(task).await
                });

                self.emit_task_trace(&task_id, None, "SCHEDULED", None);
                scheduled_any = true;
            }

            if self.is_cancelled() {
                join_set.abort_all();
                self.stop_active_generations().await;
                self.handle_cancellation().await?;
                return Ok(());
            }

            let all_done = {
                let run = self.run.read().await;
                TaskScheduler::all_completed(&run.tasks)
            };

            if all_done && join_set.is_empty() {
                self.handle_completion().await?;
                return Ok(());
            }

            if join_set.is_empty() && !scheduled_any {
                let has_deadlock = {
                    let run = self.run.read().await;
                    TaskScheduler::has_deadlock(&run.tasks)
                };

                if has_deadlock {
                    self.handle_failure("Deadlock detected - tasks blocked by failed dependencies")
                        .await?;
                    return Ok(());
                }

                let has_any_failed = {
                    let run = self.run.read().await;
                    TaskScheduler::any_failed(&run.tasks)
                };

                // If nothing is running/schedulable and at least one task is failed,
                // transition the run to terminal failed state instead of idling forever.
                if has_any_failed {
                    self.handle_failure("One or more tasks failed (max retries exceeded)")
                        .await?;
                    return Ok(());
                }

                // If not deadlocked but no tasks running and none scheduled, it means:
                // 1. We are waiting for dependencies (but they are not failed) - this shouldn't happen if join_set is empty, unless tasks are stuck in non-Done state without failing.
                // 2. OR we just have nothing to do and are waiting for something else?
                // Actually, if join_set is empty, it means no tasks are InProgress.
                // If !scheduled_any, it means no tasks are Pending && Runnable.
                // If !all_done, it means some tasks are Pending but NOT Runnable (waiting on deps).
                // If those deps are not InProgress (join_set empty) and not Done (otherwise they'd be runnable),
                // then they must be Failed (caught by deadlock check) or something is wrong with state.

                // Wait a bit before polling again to avoid busy loop if we are just waiting for something unexpected
                // But typically this branch implies we are stuck.

                // One edge case: maybe tasks are 'InProgress' but somehow not in join_set?
                // This happens if we restarted the app. The tasks in JSON are 'InProgress', but the re-hydrated engine has an empty join_set!
                // We need to recover these tasks.

                let orphaned_tasks = {
                    let mut run = self.run.write().await;
                    let orphaned: Vec<Task> = run
                        .tasks
                        .iter()
                        .filter(|t| t.state == TaskState::InProgress)
                        .cloned()
                        .collect();

                    // Reset them to Pending so they get picked up by the scheduler
                    for task in &orphaned {
                        if let Some(t) = run.tasks.iter_mut().find(|t_mut| t_mut.id == task.id) {
                            t.state = TaskState::Pending;
                        }
                    }
                    orphaned
                };

                if !orphaned_tasks.is_empty() {
                    tracing::warn!(
                        "Recovered {} orphaned InProgress tasks",
                        orphaned_tasks.len()
                    );
                    // Loop again immediately to schedule them
                    continue;
                }

                // If we really are stuck
                tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
            }

            if !join_set.is_empty() && !scheduled_any {
                tokio::select! {
                    maybe_task = join_set.join_next() => {
                        if let Some(task_result) = maybe_task {
                            match task_result {
                                Ok(inner) => {
                                    if let Err(e) = inner {
                                        tracing::error!("Task returned error: {}", e);
                                    }
                                }
                                Err(e) => tracing::error!("Task join returned error: {}", e),
                            }
                        }
                    }
                    _ = tokio::time::sleep(tokio::time::Duration::from_millis(100)) => {
                        // Re-check cancel/pause signals at top of loop.
                    }
                }
            }
        }
    }

    /// Execute a single task
    async fn execute_task(&self, task: Task) -> Result<()> {
        #[cfg(test)]
        if let Some(exec) = self.test_task_executor.as_ref() {
            return exec(self.clone(), task).await;
        }

        let task_id = task.id.clone();

        let session_id = self.get_or_create_task_session_id(&task).await?;
        self.emit_task_trace(&task_id, Some(&session_id), "EXEC_STARTED", None);

        self.emit_event(OrchestratorEvent::TaskStarted {
            run_id: self.get_run_id().await,
            task_id: task_id.clone(),
            timestamp: chrono::Utc::now(),
        });

        // Build context for builder
        let file_context = self.get_task_file_context(&task).await?;

        // Build builder prompt
        let prompt = AgentPrompts::build_builder_prompt(&task, &file_context, None);

        // Record budget
        {
            let mut tracker = self.budget_tracker.write().await;
            tracker.record_subagent_run();
            tracker.record_iteration();
        }

        let builder_response = self
            .call_agent(Some(&task_id), &session_id, &prompt)
            .await?;

        // Record tokens
        {
            let mut tracker = self.budget_tracker.write().await;
            tracker.record_tokens(None, Some(builder_response.len()));
        }

        // Get changes for validation
        let changes_diff = self.get_recent_changes().await?;

        // Build validator prompt
        let validator_prompt = AgentPrompts::build_validator_prompt(&task, &changes_diff, None);

        // Call validator
        {
            let mut tracker = self.budget_tracker.write().await;
            tracker.record_subagent_run();
        }

        let validator_response = self
            .call_agent(Some(&task_id), &session_id, &validator_prompt)
            .await?;

        // Record tokens
        {
            let mut tracker = self.budget_tracker.write().await;
            tracker.record_tokens(None, Some(validator_response.len()));
        }

        // Parse validation result
        let validation = AgentPrompts::parse_validation_result(&validator_response);

        let passed = validation.as_ref().map(|v| v.passed).unwrap_or(false);

        // Update task state
        {
            let mut run = self.run.write().await;
            // Extract config value before mutable borrow of tasks
            let max_retries = run.config.max_task_retries;

            if let Some(t) = run.tasks.iter_mut().find(|t| t.id == task_id) {
                t.validation_result = validation;

                if passed {
                    t.state = TaskState::Done;
                } else {
                    t.retry_count += 1;

                    if t.retry_count >= max_retries {
                        t.state = TaskState::Failed;
                        t.error_message = Some("Max retries exceeded".to_string());
                    } else {
                        // Reset to pending for retry
                        t.state = TaskState::Pending;
                    }
                }
            }

            // Update blocked tasks
            TaskScheduler::update_blocked_tasks(&mut run.tasks);
        }

        // Save state
        self.save_state().await?;

        self.emit_event(OrchestratorEvent::TaskCompleted {
            run_id: self.get_run_id().await,
            task_id,
            passed,
            timestamp: chrono::Utc::now(),
        });

        self.emit_task_trace(
            &task.id,
            task.session_id.as_deref().or(Some(session_id.as_str())),
            "EXEC_FINISHED",
            Some(if passed {
                "passed".to_string()
            } else {
                "failed".to_string()
            }),
        );

        Ok(())
    }

    async fn get_or_create_task_session_id(&self, task: &Task) -> Result<String> {
        use crate::sidecar::{CreateSessionRequest, PermissionRule};

        if let Some(existing) = self.task_sessions.read().await.get(&task.id).cloned() {
            return Ok(existing);
        }

        let (run_session_id, config) = {
            let run = self.run.read().await;
            (run.session_id.clone(), run.config.clone())
        };

        let base_session = self.sidecar.get_session(&run_session_id).await?;

        let permission = Some(vec![
            PermissionRule {
                permission: "ls".to_string(),
                pattern: "*".to_string(),
                action: "allow".to_string(),
            },
            PermissionRule {
                permission: "read".to_string(),
                pattern: "*".to_string(),
                action: "allow".to_string(),
            },
            PermissionRule {
                permission: "todowrite".to_string(),
                pattern: "*".to_string(),
                action: "allow".to_string(),
            },
            PermissionRule {
                permission: "websearch".to_string(),
                pattern: "*".to_string(),
                action: "allow".to_string(),
            },
            PermissionRule {
                permission: "webfetch".to_string(),
                pattern: "*".to_string(),
                action: "allow".to_string(),
            },
            PermissionRule {
                permission: "glob".to_string(),
                pattern: "*".to_string(),
                action: "allow".to_string(),
            },
            PermissionRule {
                permission: "grep".to_string(),
                pattern: "*".to_string(),
                action: "allow".to_string(),
            },
            PermissionRule {
                permission: "search".to_string(),
                pattern: "*".to_string(),
                action: "allow".to_string(),
            },
        ]);

        let request = CreateSessionRequest {
            title: Some(format!(
                "Orchestrator Task {}: {}",
                task.id,
                &task.title[..task.title.len().min(50)]
            )),
            model: base_session.model.clone(),
            provider: base_session.provider.clone(),
            permission,
        };

        let session = self.sidecar.create_session(request).await?;
        let session_id = session.id;

        {
            let mut sessions = self.task_sessions.write().await;
            sessions.insert(task.id.clone(), session_id.clone());
        }

        if config.require_write_approval {
            let mut run = self.run.write().await;
            if let Some(t) = run.tasks.iter_mut().find(|t| t.id == task.id) {
                t.session_id = Some(session_id.clone());
            }
        } else {
            let mut run = self.run.write().await;
            if let Some(t) = run.tasks.iter_mut().find(|t| t.id == task.id) {
                t.session_id = Some(session_id.clone());
            }
        }

        Ok(session_id)
    }

    /// Call an agent via the sidecar
    async fn call_agent(
        &self,
        task_id: Option<&str>,
        session_id: &str,
        prompt: &str,
    ) -> Result<String> {
        use crate::sidecar::{SendMessageRequest, StreamEvent};
        use futures::StreamExt;

        let _llm_permit = self
            .llm_semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| TandemError::Orchestrator("Failed to acquire LLM permit".to_string()))?;

        tracing::info!("Agent call with prompt length: {}", prompt.len());

        // Subscribe to events FIRST to avoid race condition
        let stream = self.sidecar.subscribe_events().await?;
        futures::pin_mut!(stream);

        // Then send message to sidecar
        let request = SendMessageRequest::text(prompt.to_string());
        self.sidecar.send_message(session_id, request).await?;

        let mut content = String::new();
        let mut errors: Vec<String> = Vec::new();
        let mut first_tool_part_id: Option<String> = None;
        let mut first_tool_finished = false;

        // Add a timeout to prevent hanging forever
        let timeout = tokio::time::Duration::from_secs(120);
        let start = std::time::Instant::now();

        while let Some(result) = stream.next().await {
            // Check timeout
            if start.elapsed() > timeout {
                tracing::warn!("Agent call timed out after {:?}", timeout);
                break;
            }

            match result {
                Ok(event) => match &event {
                    StreamEvent::Content {
                        session_id: sid,
                        delta,
                        content: full_content,
                        ..
                    } => {
                        if sid == session_id {
                            // Prefer delta if available, otherwise use full content
                            if let Some(text) = delta {
                                content.push_str(text);
                                tracing::debug!("Got content delta: {} chars", text.len());
                            } else if !full_content.is_empty() && content.is_empty() {
                                content = full_content.clone();
                                tracing::debug!("Got full content: {} chars", full_content.len());
                            }
                        }
                    }
                    StreamEvent::SessionIdle { session_id: sid } => {
                        if sid == session_id {
                            tracing::info!("Session {} is idle, response complete", session_id);
                            break;
                        }
                    }
                    StreamEvent::ToolStart {
                        session_id: sid,
                        part_id,
                        tool,
                        ..
                    } => {
                        if sid == session_id && first_tool_part_id.is_none() {
                            first_tool_part_id = Some(part_id.clone());
                            if let Some(task_id) = task_id {
                                self.emit_task_trace(
                                    task_id,
                                    Some(session_id),
                                    "FIRST_TOOL_CALL",
                                    Some(tool.clone()),
                                );
                            }
                        }
                    }
                    StreamEvent::ToolEnd {
                        session_id: sid,
                        part_id,
                        tool,
                        error,
                        ..
                    } => {
                        if sid == session_id
                            && !first_tool_finished
                            && first_tool_part_id.as_deref() == Some(part_id)
                        {
                            first_tool_finished = true;
                            if let Some(task_id) = task_id {
                                let detail = match error.as_ref() {
                                    Some(e) => Some(format!("{}:{}", tool, e)),
                                    None => Some(tool.clone()),
                                };
                                self.emit_task_trace(
                                    task_id,
                                    Some(session_id),
                                    "TOOL_CALL_FINISHED",
                                    detail,
                                );
                            }
                        }
                    }
                    StreamEvent::SessionError {
                        session_id: sid,
                        error,
                    } => {
                        if sid == session_id {
                            tracing::error!("Session {} error: {}", session_id, error);
                            errors.push(error.clone());
                            break;
                        }
                    }
                    StreamEvent::Raw { event_type, data } => {
                        tracing::debug!("Raw event for orchestrator: {} - {:?}", event_type, data);
                    }
                    _ => {}
                },
                Err(e) => {
                    tracing::warn!("Stream error in orchestrator: {}", e);
                    errors.push(e.to_string());
                    break;
                }
            }
        }

        if !errors.is_empty() {
            return Err(TandemError::Sidecar(errors.join(", ")));
        }

        tracing::info!("Agent response received, length: {}", content.len());
        Ok(content)
    }

    /// Build a summary of the workspace
    async fn build_workspace_summary(&self) -> Result<String> {
        // TODO: Implement actual workspace analysis
        Ok(format!(
            "Workspace: {}\nFiles: (summary pending)",
            self.workspace_path.display()
        ))
    }

    /// Get file context relevant to a task
    async fn get_task_file_context(&self, _task: &Task) -> Result<String> {
        // TODO: Implement context selection based on task description
        Ok("File context pending implementation".to_string())
    }

    /// Get recent changes (git diff)
    async fn get_recent_changes(&self) -> Result<String> {
        // TODO: Implement git diff capture
        Ok("No changes captured".to_string())
    }

    // ========================================================================
    // Control Methods
    // ========================================================================

    /// Cancel the run
    pub fn cancel(&self) {
        if let Ok(token) = self.cancel_token.lock() {
            token.cancel();
        } else {
            tracing::error!("Failed to acquire cancel token lock");
        }
    }

    /// Cancel and persist terminal state when no active execution loop is running.
    pub async fn cancel_and_finalize(&self) -> Result<()> {
        self.cancel();
        self.stop_active_generations().await;

        let status = { self.run.read().await.status };
        if matches!(
            status,
            RunStatus::Completed | RunStatus::Failed | RunStatus::Cancelled
        ) {
            return Ok(());
        }

        self.ensure_cancelled_state().await?;

        Ok(())
    }

    /// Pause the run
    pub async fn pause(&self) {
        let mut pause = self.pause_signal.write().await;
        *pause = true;
    }

    /// Force a paused state and persist it immediately.
    /// Used during run rehydration when no execution loop is active.
    pub async fn force_pause_persisted(&self) -> Result<()> {
        {
            let mut pause = self.pause_signal.write().await;
            *pause = true;
        }
        self.budget_tracker.write().await.set_active(false);

        {
            let mut run = self.run.write().await;
            run.status = RunStatus::Paused;
            self.reset_in_progress_tasks_to_pending(&mut run);
        }

        self.save_state().await?;

        self.emit_event(OrchestratorEvent::RunPaused {
            run_id: self.get_run_id().await,
            timestamp: chrono::Utc::now(),
        });

        Ok(())
    }

    /// Resume the run
    pub async fn resume(&self) -> Result<()> {
        {
            let mut run = self.run.write().await;
            if run.status != RunStatus::Paused {
                return Err(TandemError::InvalidOperation(
                    "Run is not paused".to_string(),
                ));
            }
            run.status = RunStatus::Executing;
            run.ended_at = None;
        }

        {
            let mut pause = self.pause_signal.write().await;
            *pause = false;
        }
        self.budget_tracker.write().await.set_active(true);
        self.save_state().await?;

        self.emit_event(OrchestratorEvent::RunResumed {
            run_id: self.get_run_id().await,
            timestamp: chrono::Utc::now(),
        });

        Ok(())
    }

    /// Approve the plan and start execution
    pub async fn approve(&self) -> Result<()> {
        {
            let run = self.run.read().await;
            if run.status != RunStatus::AwaitingApproval {
                return Err(TandemError::InvalidOperation(
                    "Run is not awaiting approval".to_string(),
                ));
            }
        }

        self.emit_event(OrchestratorEvent::PlanApproved {
            run_id: self.get_run_id().await,
            timestamp: chrono::Utc::now(),
        });

        self.execute().await
    }

    /// Request revision of the plan
    pub async fn request_revision(&self, feedback: String) -> Result<()> {
        {
            let mut run = self.run.write().await;
            if run.status != RunStatus::AwaitingApproval {
                return Err(TandemError::InvalidOperation(
                    "Run is not awaiting approval".to_string(),
                ));
            }
            run.status = RunStatus::RevisionRequested;
            run.revision_feedback = Some(feedback.clone());
        }

        self.emit_event(OrchestratorEvent::RevisionRequested {
            run_id: self.get_run_id().await,
            feedback,
            timestamp: chrono::Utc::now(),
        });

        // Re-run planning with feedback
        self.run_planning_phase().await
    }

    // ========================================================================
    // State Handlers
    // ========================================================================

    async fn handle_cancellation(&self) -> Result<()> {
        {
            let run = self.run.read().await;
            if run.status == RunStatus::Cancelled {
                return Ok(());
            }
        }

        self.budget_tracker.write().await.set_active(false);
        {
            let mut run = self.run.write().await;
            run.status = RunStatus::Cancelled;
            run.ended_at = Some(chrono::Utc::now());
            self.reset_in_progress_tasks_to_pending(&mut run);
        }

        self.save_state().await?;

        self.emit_event(OrchestratorEvent::RunCancelled {
            run_id: self.get_run_id().await,
            timestamp: chrono::Utc::now(),
        });

        Ok(())
    }

    async fn ensure_cancelled_state(&self) -> Result<()> {
        let already_cancelled = { self.run.read().await.status == RunStatus::Cancelled };
        if already_cancelled {
            return Ok(());
        }
        self.handle_cancellation().await
    }

    async fn cancel_if_requested(&self) -> Result<bool> {
        if self.is_cancelled() {
            self.ensure_cancelled_state().await?;
            return Ok(true);
        }
        Ok(false)
    }

    fn is_cancelled(&self) -> bool {
        self.cancel_token
            .lock()
            .map(|token| token.is_cancelled())
            .unwrap_or(true)
    }

    fn reset_cancel_token(&self) {
        if let Ok(mut token) = self.cancel_token.lock() {
            *token = CancellationToken::new();
        } else {
            tracing::error!("Failed to reset cancel token");
        }
    }

    fn reset_in_progress_tasks_to_pending(&self, run: &mut Run) {
        for task in run.tasks.iter_mut() {
            if task.state == TaskState::InProgress {
                task.state = TaskState::Pending;
            }
        }
    }

    async fn stop_active_generations(&self) {
        let mut session_ids: HashSet<String> = HashSet::new();
        let run_session_id = { self.run.read().await.session_id.clone() };
        session_ids.insert(run_session_id);

        {
            let task_sessions = self.task_sessions.read().await;
            for sid in task_sessions.values() {
                session_ids.insert(sid.clone());
            }
        }

        for session_id in session_ids {
            if let Err(e) = self.sidecar.cancel_generation(&session_id).await {
                tracing::debug!(
                    "Failed to cancel generation for session {}: {}",
                    session_id,
                    e
                );
            }
        }
    }

    async fn handle_pause(&self) -> Result<()> {
        self.budget_tracker.write().await.set_active(false);
        {
            let mut run = self.run.write().await;
            run.status = RunStatus::Paused;
            self.reset_in_progress_tasks_to_pending(&mut run);
        }

        self.save_state().await?;

        self.emit_event(OrchestratorEvent::RunPaused {
            run_id: self.get_run_id().await,
            timestamp: chrono::Utc::now(),
        });

        Ok(())
    }

    async fn handle_budget_exceeded(&self, dimension: &str, reason: &str) -> Result<()> {
        {
            let run = self.run.read().await;
            if run.status == RunStatus::Cancelled {
                return Ok(());
            }
        }
        self.budget_tracker.write().await.set_active(false);
        {
            let mut run = self.run.write().await;
            run.status = RunStatus::Failed;
            run.ended_at = Some(chrono::Utc::now());
            run.error_message = Some(format!("Budget exceeded: {} - {}", dimension, reason));
            self.reset_in_progress_tasks_to_pending(&mut run);
        }

        self.save_state().await?;

        self.emit_event(OrchestratorEvent::RunFailed {
            run_id: self.get_run_id().await,
            reason: format!("Budget exceeded: {}", reason),
            timestamp: chrono::Utc::now(),
        });

        Ok(())
    }

    async fn handle_completion(&self) -> Result<()> {
        {
            let run = self.run.read().await;
            if run.status == RunStatus::Cancelled {
                return Ok(());
            }
        }
        self.budget_tracker.write().await.set_active(false);
        {
            let mut run = self.run.write().await;
            run.status = RunStatus::Completed;
            run.ended_at = Some(chrono::Utc::now());
        }

        self.save_state().await?;

        self.emit_event(OrchestratorEvent::RunCompleted {
            run_id: self.get_run_id().await,
            timestamp: chrono::Utc::now(),
        });

        Ok(())
    }

    async fn handle_failure(&self, reason: &str) -> Result<()> {
        {
            let run = self.run.read().await;
            if run.status == RunStatus::Cancelled {
                return Ok(());
            }
        }
        self.budget_tracker.write().await.set_active(false);
        {
            let mut run = self.run.write().await;
            run.status = RunStatus::Failed;
            run.ended_at = Some(chrono::Utc::now());
            run.error_message = Some(reason.to_string());
            self.reset_in_progress_tasks_to_pending(&mut run);
        }

        self.save_state().await?;

        self.emit_event(OrchestratorEvent::RunFailed {
            run_id: self.get_run_id().await,
            reason: reason.to_string(),
            timestamp: chrono::Utc::now(),
        });

        Ok(())
    }

    // ========================================================================
    // Helpers
    // ========================================================================

    async fn get_run_id(&self) -> String {
        self.run_id.clone()
    }

    async fn save_state(&self) -> Result<()> {
        // Sync budget to run before saving
        let budget = self.budget_tracker.write().await.snapshot();
        {
            let mut run = self.run.write().await;
            run.budget = budget.clone();
        }

        let run = self.run.read().await;
        self.store.save_run(&run)?;
        self.store.save_budget(&run.run_id, &budget)?;

        Ok(())
    }

    fn emit_event(&self, event: OrchestratorEvent) {
        let run_id = self.run_id.clone();
        let store = self.store.clone();
        let event_for_log = event.clone();

        tokio::task::spawn_blocking(move || {
            if let Err(e) = store.append_event(&run_id, &event_for_log) {
                tracing::error!("Failed to append orchestrator event: {}", e);
            }
        });

        if let Err(e) = self.event_tx.send(event) {
            tracing::error!("Failed to emit orchestrator event: {}", e);
        }
    }

    fn emit_task_trace(
        &self,
        task_id: &str,
        session_id: Option<&str>,
        stage: &str,
        detail: Option<String>,
    ) {
        let thread = std::thread::current()
            .name()
            .map(|s| s.to_string())
            .or_else(|| Some(format!("{:?}", std::thread::current().id())));

        self.emit_event(OrchestratorEvent::TaskTrace {
            run_id: self.run_id.clone(),
            task_id: task_id.to_string(),
            session_id: session_id.map(|s| s.to_string()),
            stage: stage.to_string(),
            detail,
            thread,
            timestamp: chrono::Utc::now(),
        });
    }

    /// Get current run snapshot
    pub async fn get_snapshot(&self) -> RunSnapshot {
        let mut snapshot = self.run.read().await.to_snapshot();
        // Overlay current budget from tracker
        snapshot.budget = self.budget_tracker.write().await.snapshot();
        snapshot
    }

    /// Get current budget snapshot
    pub async fn get_budget(&self) -> Budget {
        self.budget_tracker.write().await.snapshot()
    }

    /// Get task list
    pub async fn get_tasks(&self) -> Vec<Task> {
        self.run.read().await.tasks.clone()
    }

    pub async fn get_config(&self) -> OrchestratorConfig {
        self.run.read().await.config.clone()
    }

    pub async fn get_base_session_id(&self) -> String {
        self.run.read().await.session_id.clone()
    }

    pub async fn set_base_session_for_resume(&self, new_session_id: String) -> Result<()> {
        {
            let mut run = self.run.write().await;
            if run.status != RunStatus::Paused && run.status != RunStatus::Cancelled {
                return Err(TandemError::InvalidOperation(
                    "Run must be paused or cancelled to change resume model".to_string(),
                ));
            }

            run.session_id = new_session_id;

            // Force non-terminal tasks to create fresh task sessions from the new base model.
            for task in run.tasks.iter_mut() {
                if task.state != TaskState::Done {
                    task.session_id = None;
                }
            }
        }

        {
            let mut sessions = self.task_sessions.write().await;
            sessions.clear();
        }

        self.save_state().await
    }

    /// Force restart execution regardless of prior terminal status
    pub async fn restart(&self) -> Result<()> {
        self.reset_cancel_token();
        {
            let mut pause = self.pause_signal.write().await;
            *pause = false;
        }

        let upgraded_limits = {
            let mut run = self.run.write().await;
            let upgraded = Self::upgrade_legacy_limits(&mut run);
            run.status = RunStatus::Executing;
            run.error_message = None;
            run.ended_at = None;

            // Reset non-terminal tasks so a retry can run end-to-end.
            for task in run.tasks.iter_mut() {
                if task.state == TaskState::Failed
                    || task.state == TaskState::InProgress
                    || task.state == TaskState::Blocked
                {
                    task.state = TaskState::Pending;
                    task.retry_count = 0;
                    task.error_message = None;
                }
            }
            upgraded
        };
        if upgraded_limits {
            self.update_budget_limits().await;
        }

        self.budget_tracker.write().await.set_active(true);
        // Persist status update before starting loop so UI immediately reflects change
        self.save_state().await?;
        self.run_execution_loop().await
    }

    fn upgrade_legacy_limits(run: &mut Run) -> bool {
        let mut changed = false;

        if run.config.max_iterations == 10 || run.config.max_iterations == 30 {
            run.config.max_iterations = 200;
            changed = true;
        }
        if run.config.max_subagent_runs == 20 || run.config.max_subagent_runs == 50 {
            run.config.max_subagent_runs = 500;
            changed = true;
        }
        if run.config.max_wall_time_secs == 20 * 60 {
            run.config.max_wall_time_secs = 60 * 60;
            changed = true;
        }

        if !changed {
            return false;
        }

        run.budget.max_iterations = run.config.max_iterations;
        run.budget.max_subagent_runs = run.config.max_subagent_runs;
        run.budget.max_wall_time_secs = run.config.max_wall_time_secs;

        let still_exceeded = run.budget.iterations_used >= run.budget.max_iterations
            || run.budget.tokens_used >= run.budget.max_tokens
            || run.budget.wall_time_secs >= run.budget.max_wall_time_secs
            || run.budget.subagent_runs_used >= run.budget.max_subagent_runs;

        if run.budget.exceeded && !still_exceeded {
            run.budget.exceeded = false;
            run.budget.exceeded_reason = None;
            if run
                .error_message
                .as_deref()
                .is_some_and(|msg| msg.contains("Budget exceeded:"))
            {
                run.error_message = None;
            }
        }

        true
    }

    /// Update budget limits in the tracker to match the run config
    pub async fn update_budget_limits(&self) {
        let config = self.run.read().await.config.clone();
        let mut tracker = self.budget_tracker.write().await;
        tracker.update_limits(&config);
    }

    #[cfg(test)]
    pub async fn set_task_state(&self, task_id: &str, state: TaskState) {
        let mut run = self.run.write().await;
        if let Some(t) = run.tasks.iter_mut().find(|t| t.id == task_id) {
            t.state = state;
        }
    }
}
