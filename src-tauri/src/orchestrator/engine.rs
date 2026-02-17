// Orchestrator Engine
// Main orchestration loop: plan -> dispatch -> collect -> validate -> update
// See: docs/orchestration_plan.md

use crate::error::{Result, TandemError};
use crate::orchestrator::{
    agents::{AgentPrompts, PlannerConstraints},
    budget::{BudgetCheckResult, BudgetTracker},
    policy::PolicyEngine,
    scheduler::TaskScheduler,
    store::OrchestratorStore,
    types::*,
};
use crate::sidecar::SidecarManager;
use crate::stream_hub::StreamHub;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

fn is_sidecar_session_not_found(err: &TandemError) -> bool {
    matches!(err, TandemError::Sidecar(msg) if msg.contains("404 Not Found"))
}

fn extract_text_from_message_part(part: &serde_json::Value) -> Option<String> {
    let obj = part.as_object()?;
    let part_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or_default();

    if let Some(text) = obj.get("text").and_then(|v| v.as_str()) {
        if !text.trim().is_empty() {
            return Some(text.to_string());
        }
    }

    if let Some(content) = obj.get("content").and_then(|v| v.as_str()) {
        if !content.trim().is_empty() {
            return Some(content.to_string());
        }
    }

    if matches!(part_type, "text" | "reasoning" | "output_text") {
        if let Some(delta) = obj.get("delta").and_then(|v| v.as_str()) {
            if !delta.trim().is_empty() {
                return Some(delta.to_string());
            }
        }
    }

    None
}

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
    /// Shared stream hub receiver source
    stream_hub: Arc<StreamHub>,
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
    contract_metrics: Arc<RwLock<HashMap<String, u64>>>,
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
    const RELAXED_MAX_ITERATIONS: u32 = 1_000_000;
    const RELAXED_MAX_TOTAL_TOKENS: u64 = 100_000_000;
    const RELAXED_MAX_WALL_TIME_SECS: u64 = 7 * 24 * 60 * 60; // 7 days
    const RELAXED_MAX_SUBAGENT_RUNS: u32 = 100_000;

    fn is_rate_limit_error(error: &str) -> bool {
        let e = error.to_lowercase();
        e.contains("rate limit")
            || e.contains("ratelimit")
            || e.contains("too many requests")
            || e.contains("http 429")
            || e.contains("429")
    }

    fn is_provider_quota_error(error: &str) -> bool {
        let e = error.to_lowercase();
        e.contains("key limit exceeded")
            || e.contains("monthly limit")
            || e.contains("quota exceeded")
            || e.contains("insufficient_quota")
            || e.contains("out of credits")
            || e.contains("billing")
            || e.contains("payment required")
    }
    /// Create a new orchestrator engine
    pub fn new(
        run: Run,
        policy: PolicyEngine,
        store: OrchestratorStore,
        sidecar: Arc<SidecarManager>,
        stream_hub: Arc<StreamHub>,
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
            stream_hub,
            workspace_path,
            cancel_token: Arc::new(StdMutex::new(CancellationToken::new())),
            pause_signal,
            event_tx,
            task_semaphore: Arc::new(Semaphore::new(max_parallel_tasks)),
            llm_semaphore: Arc::new(Semaphore::new(llm_parallel)),
            task_sessions: Arc::new(RwLock::new(HashMap::new())),
            contract_metrics: Arc::new(RwLock::new(HashMap::new())),
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
        if let Err(e) = self.run_planning_phase().await {
            // Ensure the run transitions to a terminal state instead of leaving the UI stuck
            // in "planning" forever.
            let reason = e.to_string();
            let _ = self.handle_failure(&reason).await;
            return Err(e);
        }

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
            if run.tasks.is_empty() {
                return Err(TandemError::InvalidOperation(
                    "Cannot execute: plan has no tasks (generate a plan first)".to_string(),
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

        let contract_config = {
            let run = self.run.read().await;
            run.config.clone()
        };

        // Parse tasks from response with strict JSON-first enforcement.
        let parsed_tasks = if contract_config.strict_planner_json {
            match AgentPrompts::parse_task_list_strict(&response) {
                Ok(tasks) => {
                    self.bump_contract_metric("planner_strict_parse_success")
                        .await;
                    tasks
                }
                Err(strict_err) => {
                    self.bump_contract_metric("planner_parse_failed").await;
                    let snippet = Self::sample_snippet(&response);
                    if contract_config.contract_warnings_enabled {
                        self.emit_contract_warning(
                            None,
                            "planner",
                            "planning",
                            &strict_err,
                            false,
                            snippet.clone(),
                        )
                        .await;
                    }
                    if contract_config.allow_prose_fallback {
                        if let Some(tasks) = AgentPrompts::parse_task_list_fallback(&response) {
                            self.bump_contract_metric("planner_fallback_used").await;
                            if contract_config.contract_warnings_enabled {
                                self.emit_contract_warning(
                                    None,
                                    "planner",
                                    "planning",
                                    "planner strict parse failed; prose fallback used",
                                    true,
                                    snippet,
                                )
                                .await;
                            }
                            tasks
                        } else {
                            if contract_config.contract_warnings_enabled {
                                self.emit_contract_error(
                                    None,
                                    "planner",
                                    "planning",
                                    "PLANNER_CONTRACT_PARSE_FAILED",
                                    false,
                                    Some(strict_err.clone()),
                                )
                                .await;
                            }
                            return Err(TandemError::ParseError(format!(
                                "PLANNER_CONTRACT_PARSE_FAILED: {}",
                                strict_err
                            )));
                        }
                    } else {
                        if contract_config.contract_warnings_enabled {
                            self.emit_contract_error(
                                None,
                                "planner",
                                "planning",
                                "PLANNER_CONTRACT_PARSE_FAILED",
                                false,
                                Some(strict_err.clone()),
                            )
                            .await;
                        }
                        return Err(TandemError::ParseError(format!(
                            "PLANNER_CONTRACT_PARSE_FAILED: {}",
                            strict_err
                        )));
                    }
                }
            }
        } else {
            AgentPrompts::parse_task_list(&response).ok_or_else(|| {
                TandemError::ParseError("Failed to parse task list from planner output".to_string())
            })?
        };

        // Convert to Task objects
        let tasks: Vec<Task> = parsed_tasks.into_iter().map(Task::from).collect();
        if tasks.is_empty() {
            return Err(TandemError::ParseError(
                "Planner produced an empty task list".to_string(),
            ));
        }

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
        {
            let run = self.run.read().await;
            if run.tasks.is_empty() {
                let reason = "Cannot execute: run has no tasks (generate a plan first)";
                let _ = self.handle_failure(reason).await;
                return Err(TandemError::InvalidOperation(reason.to_string()));
            }
        }

        let mut join_set: JoinSet<Result<()>> = JoinSet::new();
        // Throttle repetitive budget warnings (e.g. wall_time at 80% every loop tick).
        // We log when the warning bucket advances (5% steps) or after cooldown.
        let mut last_budget_warning: HashMap<String, (u8, Instant)> = HashMap::new();
        const WARNING_BUCKET_STEP_PERCENT: u8 = 5;
        const WARNING_LOG_COOLDOWN_SECS: u64 = 30;
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
                    let percent = (percentage * 100.0).floor().clamp(0.0, 100.0) as u8;
                    let bucket =
                        (percent / WARNING_BUCKET_STEP_PERCENT) * WARNING_BUCKET_STEP_PERCENT;
                    let now = Instant::now();

                    let should_log = match last_budget_warning.get(&dimension) {
                        Some((prev_bucket, prev_at)) => {
                            bucket > *prev_bucket
                                || now.duration_since(*prev_at).as_secs()
                                    >= WARNING_LOG_COOLDOWN_SECS
                        }
                        None => true,
                    };

                    if should_log {
                        tracing::warn!("Budget warning: {} at {}%", dimension, percent);
                        last_budget_warning.insert(dimension, (bucket, now));
                    }
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

            // A task may have raised a pause request (e.g. provider quota/rate-limit)
            // while we were draining join results. Honor it immediately so we don't
            // schedule more work in the same loop tick.
            if *self.pause_signal.read().await {
                join_set.abort_all();
                self.stop_active_generations().await;
                self.handle_pause().await?;
                return Ok(());
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

                let (has_any_failed, failed_reason) = {
                    let run = self.run.read().await;
                    let has_failed = TaskScheduler::any_failed(&run.tasks);
                    let reason = run
                        .tasks
                        .iter()
                        .find(|t| t.state == TaskState::Failed)
                        .map(|t| {
                            if let Some(msg) = t.error_message.as_deref() {
                                format!("Task {} failed: {}", t.id, msg)
                            } else {
                                format!("Task {} failed", t.id)
                            }
                        });
                    (has_failed, reason)
                };

                // If nothing is running/schedulable and at least one task is failed,
                // transition the run to terminal failed state instead of idling forever.
                if has_any_failed {
                    let reason = failed_reason.unwrap_or_else(|| {
                        "One or more tasks failed (max retries exceeded)".to_string()
                    });
                    self.handle_failure(&reason).await?;
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

        // If anything inside this task execution errors out, we MUST not leave the task
        // stuck in `in_progress` (otherwise the orphan-recovery logic will keep re-queuing
        // it forever and budgets will "explode").
        let execution_result: Result<(String, bool, Option<ValidationResult>)> = async {
            let mut session_id = self.get_or_create_task_session_id(&task).await?;
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
                .call_agent_for_task_with_recovery(&task, &mut session_id, &prompt)
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
                .call_agent_for_task_with_recovery(&task, &mut session_id, &validator_prompt)
                .await?;

            // Record tokens
            {
                let mut tracker = self.budget_tracker.write().await;
                tracker.record_tokens(None, Some(validator_response.len()));
            }

            let contract_config = {
                let run = self.run.read().await;
                run.config.clone()
            };

            // Parse validation result with strict JSON-first enforcement.
            let validation = if contract_config.strict_validator_json {
                match AgentPrompts::parse_validation_result_strict(&validator_response) {
                    Ok(parsed) => {
                        self.bump_contract_metric("validator_strict_parse_success")
                            .await;
                        Some(parsed)
                    }
                    Err(strict_err) => {
                        self.bump_contract_metric("validator_parse_failed").await;
                        let snippet = Self::sample_snippet(&validator_response);
                        if contract_config.contract_warnings_enabled {
                            self.emit_contract_warning(
                                Some(task_id.clone()),
                                "validator",
                                "task_validation",
                                &strict_err,
                                false,
                                snippet.clone(),
                            )
                            .await;
                        }
                        if contract_config.allow_prose_fallback {
                            let fallback =
                                AgentPrompts::parse_validation_result_fallback(&validator_response);
                            if fallback.is_some() {
                                self.bump_contract_metric("validator_fallback_used").await;
                                if contract_config.contract_warnings_enabled {
                                    self.emit_contract_warning(
                                        Some(task_id.clone()),
                                        "validator",
                                        "task_validation",
                                        "validator strict parse failed; prose fallback used",
                                        true,
                                        snippet,
                                    )
                                    .await;
                                }
                            }
                            fallback
                        } else {
                            if contract_config.contract_warnings_enabled {
                                self.emit_contract_error(
                                    Some(task_id.clone()),
                                    "validator",
                                    "task_validation",
                                    "VALIDATOR_CONTRACT_PARSE_FAILED",
                                    false,
                                    Some(strict_err.clone()),
                                )
                                .await;
                            }
                            self.emit_task_trace(
                                &task_id,
                                Some(&session_id),
                                "VALIDATOR_CONTRACT_PARSE_FAILED",
                                Some(strict_err.clone()),
                            );
                            return Err(TandemError::ParseError(format!(
                                "VALIDATOR_CONTRACT_PARSE_FAILED: {}",
                                strict_err
                            )));
                        }
                    }
                }
            } else {
                AgentPrompts::parse_validation_result(&validator_response)
            };
            let passed = validation.as_ref().map(|v| v.passed).unwrap_or(false);

            Ok((session_id, passed, validation))
        }
        .await;

        match execution_result {
            Ok((session_id, passed, validation)) => {
                // Update task state
                {
                    let mut run = self.run.write().await;
                    // Extract config value before mutable borrow of tasks
                    let max_retries = run.config.max_task_retries;

                    if let Some(t) = run.tasks.iter_mut().find(|t| t.id == task_id) {
                        t.validation_result = validation;

                        if passed {
                            t.state = TaskState::Done;
                            t.error_message = None;
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
                    task_id: task_id.clone(),
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
            Err(e) => {
                self.mark_task_error(&task_id, &e.to_string()).await?;
                Err(e)
            }
        }
    }

    async fn mark_task_error(&self, task_id: &str, error: &str) -> Result<()> {
        let rate_limited = Self::is_rate_limit_error(error);
        let quota_exceeded = Self::is_provider_quota_error(error);
        let should_pause = rate_limited || quota_exceeded;

        let session_id = {
            let mut run = self.run.write().await;
            let max_retries = run.config.max_task_retries;

            let mut session_id = None;

            if let Some(t) = run.tasks.iter_mut().find(|t| t.id == task_id) {
                session_id = t.session_id.clone();
                t.error_message = Some(error.to_string());

                if should_pause {
                    // Treat provider capacity/quota failures as a "pause and switch model" event,
                    // not a normal task failure that burns retries.
                    t.state = TaskState::Pending;
                    if quota_exceeded {
                        t.error_message = Some(
                            "Provider quota/credits exceeded. Switch model/provider and retry."
                                .to_string(),
                        );
                    } else if rate_limited {
                        t.error_message = Some(
                            "Provider rate-limited. Switch model/provider and retry.".to_string(),
                        );
                    }
                    run.error_message = Some(if quota_exceeded {
                        "Paused: provider quota/credits exceeded. Switch model/provider and retry."
                            .to_string()
                    } else {
                        "Paused: provider rate-limited. Switch model/provider and retry."
                            .to_string()
                    });
                } else {
                    t.retry_count += 1;
                    if t.retry_count >= max_retries {
                        t.state = TaskState::Failed;
                    } else {
                        t.state = TaskState::Pending;
                    }
                }
            }

            TaskScheduler::update_blocked_tasks(&mut run.tasks);
            session_id
        };

        self.save_state().await?;

        if should_pause {
            let mut pause = self.pause_signal.write().await;
            *pause = true;
        }

        self.emit_event(OrchestratorEvent::TaskCompleted {
            run_id: self.get_run_id().await,
            task_id: task_id.to_string(),
            passed: false,
            timestamp: chrono::Utc::now(),
        });

        self.emit_task_trace(
            task_id,
            session_id.as_deref(),
            "EXEC_FINISHED",
            Some(format!("error: {}", error)),
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

        let base_session = match self.sidecar.get_session(&run_session_id).await {
            Ok(session) => session,
            Err(e) if is_sidecar_session_not_found(&e) => {
                tracing::warn!(
                    "Base orchestrator session {} is missing (404). Recreating base session.",
                    run_session_id
                );
                let new_session_id = self.recreate_base_run_session().await?;
                self.sidecar.get_session(&new_session_id).await?
            }
            Err(e) => return Err(e),
        };

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

        let provider = base_session.provider.clone();
        let model = match (base_session.model.clone(), provider.clone()) {
            (Some(model_id), Some(provider_id)) => Some(json!({
                "provider_id": provider_id,
                "model_id": model_id
            })),
            _ => None,
        };

        let request = CreateSessionRequest {
            parent_id: Some(base_session.id.clone()),
            title: Some(format!(
                "Orchestrator Task {}: {}",
                task.id,
                &task.title[..task.title.len().min(50)]
            )),
            model,
            provider,
            permission,
            directory: Some(self.workspace_path.to_string_lossy().to_string()),
            workspace_root: Some(self.workspace_path.to_string_lossy().to_string()),
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

    async fn call_agent_for_task_with_recovery(
        &self,
        task: &Task,
        session_id: &mut String,
        prompt: &str,
    ) -> Result<String> {
        match self
            .call_agent(Some(&task.id), session_id.as_str(), prompt)
            .await
        {
            Ok(response) => Ok(response),
            Err(e) if is_sidecar_session_not_found(&e) => {
                tracing::warn!(
                    "Task {} session {} missing (404). Recreating task session and retrying once.",
                    task.id,
                    session_id
                );
                self.invalidate_task_session(&task.id).await;
                *session_id = self.get_or_create_task_session_id(task).await?;
                self.call_agent(Some(&task.id), session_id.as_str(), prompt)
                    .await
            }
            Err(e) => Err(e),
        }
    }

    async fn invalidate_task_session(&self, task_id: &str) {
        {
            let mut sessions = self.task_sessions.write().await;
            sessions.remove(task_id);
        }
        {
            let mut run = self.run.write().await;
            if let Some(task) = run.tasks.iter_mut().find(|t| t.id == task_id) {
                task.session_id = None;
            }
        }
    }

    async fn recreate_base_run_session(&self) -> Result<String> {
        use crate::sidecar::{CreateSessionRequest, PermissionRule};

        let (objective, run_model, run_provider) = {
            let run = self.run.read().await;
            (
                run.objective.clone(),
                run.model.clone(),
                run.provider.clone(),
            )
        };

        let model = match (run_provider.clone(), run_model.clone()) {
            (Some(provider_id), Some(model_id))
                if !provider_id.trim().is_empty() && !model_id.trim().is_empty() =>
            {
                Some(json!({
                    "provider_id": provider_id,
                    "model_id": model_id
                }))
            }
            _ => None,
        };

        let request = CreateSessionRequest {
            parent_id: None,
            title: Some(format!(
                "Orchestrator Run: {}",
                &objective[..objective.len().min(50)]
            )),
            model,
            provider: run_provider,
            permission: Some(vec![
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
            ]),
            directory: Some(self.workspace_path.to_string_lossy().to_string()),
            workspace_root: Some(self.workspace_path.to_string_lossy().to_string()),
        };

        let session = self.sidecar.create_session(request).await?;
        let new_session_id = session.id;

        {
            let mut run = self.run.write().await;
            run.session_id = new_session_id.clone();
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
        self.save_state().await?;

        tracing::info!(
            "Recreated missing base orchestrator session with new id {}",
            new_session_id
        );
        Ok(new_session_id)
    }

    /// Call an agent via the sidecar
    async fn call_agent(
        &self,
        task_id: Option<&str>,
        session_id: &str,
        prompt: &str,
    ) -> Result<String> {
        use crate::sidecar::{ModelSpec, SendMessageRequest, StreamEvent};

        let _llm_permit = self
            .llm_semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| TandemError::Orchestrator("Failed to acquire LLM permit".to_string()))?;

        tracing::info!("Agent call with prompt length: {}", prompt.len());

        // Prefer the model/provider persisted on the run. (Some OpenCode builds don't populate
        // legacy `session.model/provider` in GET /session responses.)
        let (run_model, run_provider) = self.get_run_model_provider().await;
        let model_spec = match (run_provider.clone(), run_model.clone()) {
            (Some(provider_id), Some(model_id))
                if !provider_id.trim().is_empty() && !model_id.trim().is_empty() =>
            {
                tracing::info!(
                    "Orchestrator agent call using run model: provider={} model={}",
                    provider_id,
                    model_id
                );
                Some(ModelSpec {
                    provider_id,
                    model_id,
                })
            }
            _ => match self.sidecar.get_session(session_id).await {
                Ok(session) => match (session.provider.clone(), session.model.clone()) {
                    (Some(provider_id), Some(model_id))
                        if !provider_id.trim().is_empty() && !model_id.trim().is_empty() =>
                    {
                        tracing::info!(
                            "Orchestrator agent call using session model: provider={} model={}",
                            provider_id,
                            model_id
                        );
                        Some(ModelSpec {
                            provider_id,
                            model_id,
                        })
                    }
                    _ => {
                        tracing::warn!(
                            "Orchestrator session {} has no provider/model set; sending without explicit model spec",
                            session_id
                        );
                        None
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        "Failed to fetch orchestrator session {} for model spec: {}",
                        session_id,
                        e
                    );
                    None
                }
            },
        };

        let mut stream = self.stream_hub.subscribe();
        let mut active_session_id = session_id.to_string();

        // Then send message to sidecar
        let mut request = SendMessageRequest::text(prompt.to_string());
        request.model = model_spec.clone();
        if let Err(e) = self
            .sidecar
            .append_message_and_start_run(&active_session_id, request)
            .await
        {
            if task_id.is_none() && is_sidecar_session_not_found(&e) {
                tracing::warn!(
                    "Base orchestrator session {} missing (404) during agent call. Recreating and retrying once.",
                    active_session_id
                );
                active_session_id = self.recreate_base_run_session().await?;
                let mut retry_request = SendMessageRequest::text(prompt.to_string());
                retry_request.model = model_spec;
                self.sidecar
                    .append_message_and_start_run(&active_session_id, retry_request)
                    .await?;
            } else {
                return Err(e);
            }
        }

        let mut content = String::new();
        let mut errors: Vec<String> = Vec::new();
        let mut first_tool_part_id: Option<String> = None;
        let mut first_tool_finished = false;
        let mut stalled_windows: usize = 0;
        const MAX_STALLED_WINDOWS_BEFORE_FAIL: usize = 4;

        // Add a hard timeout to prevent hanging forever (even if the sidecar only sends heartbeats).
        // Planning can legitimately take a while (large repos, slower models, cold starts).
        // Keep this reasonably high so we don't fail healthy runs, but still fail-fast for true hangs.
        let timeout = tokio::time::Duration::from_secs(300);
        let consume = async {
            let per_event_timeout = tokio::time::Duration::from_secs(60);
            loop {
                let next = tokio::time::timeout(per_event_timeout, stream.recv()).await;
                let event = match next {
                    Ok(Ok(env)) => {
                        stalled_windows = 0;
                        env.payload
                    }
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped))) => {
                        tracing::warn!("Orchestrator stream lagged by {} events", skipped);
                        continue;
                    }
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                    Err(_) => {
                        stalled_windows = stalled_windows.saturating_add(1);
                        tracing::warn!(
                            "No SSE events for {}s (window {}/{}), probing active run for session {}",
                            stalled_windows * 60,
                            stalled_windows,
                            MAX_STALLED_WINDOWS_BEFORE_FAIL,
                            active_session_id
                        );

                        match self.sidecar.get_active_run(&active_session_id).await {
                            Ok(Some(active)) => {
                                if let Some(task_id) = task_id {
                                    self.emit_task_trace(
                                        task_id,
                                        Some(&active_session_id),
                                        "STREAM_STALLED",
                                        Some(format!(
                                            "run={} last_activity_ms={}",
                                            active.run_id, active.last_activity_at_ms
                                        )),
                                    );
                                }

                                if stalled_windows >= MAX_STALLED_WINDOWS_BEFORE_FAIL {
                                    errors.push(format!(
                                        "No SSE events received for {}s while run {} remained active",
                                        stalled_windows * 60,
                                        active.run_id
                                    ));
                                    break;
                                }

                                continue;
                            }
                            Ok(None) => {
                                tracing::info!(
                                    "No active run found for {}; treating stall as terminal boundary",
                                    active_session_id
                                );
                                if content.trim().is_empty() {
                                    if let Some(recovered) = self
                                        .recover_agent_response_from_history(&active_session_id)
                                        .await
                                    {
                                        content = recovered;
                                    }
                                }
                                break;
                            }
                            Err(probe_err) => {
                                tracing::warn!(
                                    "Failed to probe active run for {} during stall recovery: {}",
                                    active_session_id,
                                    probe_err
                                );
                                if stalled_windows >= MAX_STALLED_WINDOWS_BEFORE_FAIL {
                                    errors.push(format!(
                                        "No SSE events received for {}s and run probe failed: {}",
                                        stalled_windows * 60,
                                        probe_err
                                    ));
                                    break;
                                }
                                continue;
                            }
                        }
                    }
                };
                match &event {
                    StreamEvent::Content {
                        session_id: sid,
                        delta,
                        content: full_content,
                        ..
                    } => {
                        if sid == &active_session_id {
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
                        if sid == &active_session_id {
                            tracing::info!(
                                "Session {} is idle, response complete",
                                active_session_id
                            );
                            break;
                        }
                    }
                    StreamEvent::ToolStart {
                        session_id: sid,
                        part_id,
                        tool,
                        ..
                    } => {
                        if sid == &active_session_id && first_tool_part_id.is_none() {
                            first_tool_part_id = Some(part_id.clone());
                            if let Some(task_id) = task_id {
                                self.emit_task_trace(
                                    task_id,
                                    Some(&active_session_id),
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
                        if sid == &active_session_id
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
                                    Some(&active_session_id),
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
                        if sid == &active_session_id {
                            tracing::error!("Session {} error: {}", active_session_id, error);
                            errors.push(error.clone());
                            break;
                        }
                    }
                    StreamEvent::Raw { event_type, data } => {
                        tracing::debug!("Raw event for orchestrator: {} - {:?}", event_type, data);
                    }
                    _ => {}
                }
            }
        };

        if tokio::time::timeout(timeout, consume).await.is_err() {
            tracing::warn!("Agent call timed out after {:?}", timeout);
            errors.push(format!("Timed out after {:?}", timeout));
        }

        if content.trim().is_empty() && errors.is_empty() {
            if let Some(recovered) = self
                .recover_agent_response_from_history(&active_session_id)
                .await
            {
                content = recovered;
            }
        }

        if !errors.is_empty() {
            return Err(TandemError::Sidecar(errors.join(", ")));
        }

        tracing::info!("Agent response received, length: {}", content.len());
        Ok(content)
    }

    async fn recover_agent_response_from_history(&self, session_id: &str) -> Option<String> {
        let messages = self.sidecar.get_session_messages(session_id).await.ok()?;
        let latest_assistant = messages.iter().rev().find(|message| {
            message.info.role == "assistant"
                && !message.info.deleted.unwrap_or(false)
                && !message.info.reverted.unwrap_or(false)
        })?;

        let text = latest_assistant
            .parts
            .iter()
            .filter_map(extract_text_from_message_part)
            .collect::<Vec<_>>()
            .join("");

        if text.trim().is_empty() {
            None
        } else {
            Some(text)
        }
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
            // Budget limits are a safety rail, not a "hard crash" moment.
            // Pause the run so users can resume (and we can upgrade defaults on resume).
            run.status = RunStatus::Paused;
            run.ended_at = None;
            run.error_message = Some(format!(
                "Paused: budget limit reached ({}). {}",
                dimension, reason
            ));
            self.reset_in_progress_tasks_to_pending(&mut run);
        }

        self.save_state().await?;

        self.emit_event(OrchestratorEvent::RunPaused {
            run_id: self.get_run_id().await,
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

    pub async fn get_run_model_provider(&self) -> (Option<String>, Option<String>) {
        let run = self.run.read().await;
        (run.model.clone(), run.provider.clone())
    }

    pub async fn set_run_model_provider(&self, model: Option<String>, provider: Option<String>) {
        let mut run = self.run.write().await;
        run.model = model;
        run.provider = provider;
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

    fn sample_snippet(input: &str) -> Option<String> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            return None;
        }
        let mut snippet = trimmed
            .chars()
            .take(400)
            .collect::<String>()
            .replace('\n', " ")
            .replace('\r', " ");
        if trimmed.len() > 400 {
            snippet.push_str(" ...");
        }
        Some(snippet)
    }

    async fn bump_contract_metric(&self, key: &str) {
        let mut metrics = self.contract_metrics.write().await;
        let value = metrics.entry(key.to_string()).or_insert(0);
        *value += 1;
        tracing::info!("orchestrator.contract_metric {}={}", key, value);
    }

    async fn emit_contract_warning(
        &self,
        task_id: Option<String>,
        agent: &str,
        phase: &str,
        reason: &str,
        fallback_used: bool,
        snippet: Option<String>,
    ) {
        tracing::warn!(
            "orchestrator.contract_warning agent={} phase={} fallback_used={} reason={} snippet={}",
            agent,
            phase,
            fallback_used,
            reason,
            snippet.as_deref().unwrap_or("")
        );
        self.emit_event(OrchestratorEvent::ContractWarning {
            run_id: self.run_id.clone(),
            task_id,
            agent: agent.to_string(),
            phase: phase.to_string(),
            reason: reason.to_string(),
            fallback_used,
            snippet,
            timestamp: chrono::Utc::now(),
        });
    }

    async fn emit_contract_error(
        &self,
        task_id: Option<String>,
        agent: &str,
        phase: &str,
        reason: &str,
        fallback_used: bool,
        snippet: Option<String>,
    ) {
        tracing::error!(
            "orchestrator.contract_error agent={} phase={} fallback_used={} reason={} snippet={}",
            agent,
            phase,
            fallback_used,
            reason,
            snippet.as_deref().unwrap_or("")
        );
        self.emit_event(OrchestratorEvent::ContractError {
            run_id: self.run_id.clone(),
            task_id,
            agent: agent.to_string(),
            phase: phase.to_string(),
            reason: reason.to_string(),
            fallback_used,
            snippet,
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
            if run.status != RunStatus::Paused
                && run.status != RunStatus::Cancelled
                && run.status != RunStatus::Failed
            {
                return Err(TandemError::InvalidOperation(
                    "Run must be paused, failed, or cancelled to change resume model".to_string(),
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

        // If there is no plan (or an older version allowed an empty plan), treat "restart" as
        // "replan" so we don't immediately flip to "completed" with nothing executed.
        let needs_plan = { self.run.read().await.tasks.is_empty() };
        if needs_plan {
            self.run_planning_phase().await?;
            return Ok(());
        }

        let upgraded_limits = {
            let mut run = self.run.write().await;
            let upgraded = Self::upgrade_legacy_limits(&mut run);
            let was_completed = run.status == RunStatus::Completed;
            run.status = RunStatus::Executing;
            run.error_message = None;
            run.ended_at = None;

            // Reset tasks so a retry can run end-to-end.
            // If the run already completed, "restart" means rerun the full plan.
            let rerun_all = was_completed;
            for task in run.tasks.iter_mut() {
                if rerun_all
                    || task.state == TaskState::Failed
                    || task.state == TaskState::InProgress
                    || task.state == TaskState::Blocked
                {
                    task.state = TaskState::Pending;
                    task.retry_count = 0;
                    task.error_message = None;
                    // Force a fresh task session so we don't reuse stale context.
                    task.session_id = None;
                }
            }
            upgraded
        };
        if upgraded_limits {
            self.update_budget_limits().await;
        }

        {
            let mut sessions = self.task_sessions.write().await;
            sessions.clear();
        }

        self.budget_tracker.write().await.set_active(true);
        // Persist status update before starting loop so UI immediately reflects change
        self.save_state().await?;
        self.run_execution_loop().await
    }

    fn upgrade_legacy_limits(run: &mut Run) -> bool {
        let mut changed = false;

        if run.config.max_iterations == 10
            || run.config.max_iterations == 30
            || run.config.max_iterations == 200
        {
            run.config.max_iterations = 500;
            changed = true;
        }
        if run.config.max_subagent_runs == 20
            || run.config.max_subagent_runs == 50
            || run.config.max_subagent_runs == 500
        {
            run.config.max_subagent_runs = 2000;
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

    /// Extend run budget limits in-place so users can continue long-running workflows.
    pub async fn extend_budget_limits(
        &self,
        add_iterations: u32,
        add_tokens: u64,
        add_wall_time_secs: u64,
        add_subagent_runs: u32,
        clear_caps: bool,
    ) -> Result<RunSnapshot> {
        {
            let mut run = self.run.write().await;

            if clear_caps {
                run.config.max_iterations =
                    run.config.max_iterations.max(Self::RELAXED_MAX_ITERATIONS);
                run.config.max_total_tokens = run
                    .config
                    .max_total_tokens
                    .max(Self::RELAXED_MAX_TOTAL_TOKENS);
                run.config.max_wall_time_secs = run
                    .config
                    .max_wall_time_secs
                    .max(Self::RELAXED_MAX_WALL_TIME_SECS);
                run.config.max_subagent_runs = run
                    .config
                    .max_subagent_runs
                    .max(Self::RELAXED_MAX_SUBAGENT_RUNS);
            } else {
                run.config.max_iterations =
                    run.config.max_iterations.saturating_add(add_iterations);
                run.config.max_total_tokens =
                    run.config.max_total_tokens.saturating_add(add_tokens);
                run.config.max_wall_time_secs = run
                    .config
                    .max_wall_time_secs
                    .saturating_add(add_wall_time_secs);
                run.config.max_subagent_runs = run
                    .config
                    .max_subagent_runs
                    .saturating_add(add_subagent_runs);
            }

            run.budget.max_iterations = run.config.max_iterations;
            run.budget.max_tokens = run.config.max_total_tokens;
            run.budget.max_wall_time_secs = run.config.max_wall_time_secs;
            run.budget.max_subagent_runs = run.config.max_subagent_runs;

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

                if run.status == RunStatus::Failed {
                    run.status = RunStatus::Paused;
                    run.ended_at = None;
                    for task in run.tasks.iter_mut() {
                        if task.state == TaskState::InProgress {
                            task.state = TaskState::Pending;
                        }
                    }
                }
            }
        }

        self.update_budget_limits().await;
        self.save_state().await?;
        Ok(self.get_snapshot().await)
    }

    #[cfg(test)]
    pub async fn set_task_state(&self, task_id: &str, state: TaskState) {
        let mut run = self.run.write().await;
        if let Some(t) = run.tasks.iter_mut().find(|t| t.id == task_id) {
            t.state = state;
        }
    }
}
