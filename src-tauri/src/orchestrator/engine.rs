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
use ignore::WalkBuilder;
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;
use tokio::fs;
use tokio::sync::{mpsc, RwLock, Semaphore};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

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

fn is_nested_agent_tool(tool_name: &str) -> bool {
    let normalized = tool_name
        .split(':')
        .next_back()
        .unwrap_or(tool_name)
        .trim()
        .to_lowercase();
    matches!(normalized.as_str(), "task" | "spawn_agent")
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileFingerprint {
    size: u64,
    modified_ms: u128,
    content_hash: Option<u64>,
}

#[derive(Clone, Debug, Default)]
struct WorkspaceChangeSummary {
    has_changes: bool,
    created: Vec<String>,
    updated: Vec<String>,
    deleted: Vec<String>,
    diff_text: String,
}

#[derive(Clone, Debug, Default)]
struct AgentCallOutcome {
    content: String,
    used_tools: bool,
    used_write_tools: bool,
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
    event_seq: Arc<AtomicU64>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RetryFailureClass {
    RateLimit,
    QuotaExceeded,
    AuthFailed,
    Timeout,
    WorkspaceNotFound,
    InvalidToolArgs,
    PathNotFound,
    Unknown,
}

#[derive(Clone, Debug)]
struct FailurePolicy {
    class: RetryFailureClass,
    should_pause: bool,
    task_message: Option<String>,
    run_message: Option<String>,
}

impl OrchestratorEngine {
    const ROLLING_SUMMARY_MAX_CHARS: usize = 900;

    fn classify_failure(error: &str) -> RetryFailureClass {
        if Self::is_provider_quota_error(error) {
            return RetryFailureClass::QuotaExceeded;
        }
        if Self::is_rate_limit_error(error) {
            return RetryFailureClass::RateLimit;
        }
        if Self::is_auth_error(error) {
            return RetryFailureClass::AuthFailed;
        }
        if Self::is_transient_timeout_error(error) {
            return RetryFailureClass::Timeout;
        }
        if Self::is_workspace_not_found_error(error) {
            return RetryFailureClass::WorkspaceNotFound;
        }
        if Self::is_invalid_tool_args_error(error) {
            return RetryFailureClass::InvalidToolArgs;
        }
        if Self::is_path_not_found_error(error) {
            return RetryFailureClass::PathNotFound;
        }
        RetryFailureClass::Unknown
    }

    fn failure_policy(error: &str) -> FailurePolicy {
        let class = Self::classify_failure(error);
        match class {
            RetryFailureClass::QuotaExceeded => FailurePolicy {
                class,
                should_pause: true,
                task_message: Some(
                    "Provider quota/credits exceeded. Switch model/provider and retry.".to_string(),
                ),
                run_message: Some(
                    "Paused: provider quota/credits exceeded. Switch model/provider and retry."
                        .to_string(),
                ),
            },
            RetryFailureClass::RateLimit => FailurePolicy {
                class,
                should_pause: true,
                task_message: Some("Provider rate-limited. Switch model/provider and retry.".to_string()),
                run_message: Some(
                    "Paused: provider rate-limited. Switch model/provider and retry.".to_string(),
                ),
            },
            RetryFailureClass::AuthFailed => FailurePolicy {
                class,
                should_pause: true,
                task_message: Some(
                    "Provider authentication failed (401/403). Check API key/provider selection and retry."
                        .to_string(),
                ),
                run_message: Some(
                    "Paused: provider authentication failed (401/403). Check API key/provider selection and continue."
                        .to_string(),
                ),
            },
            RetryFailureClass::Timeout => FailurePolicy {
                class,
                should_pause: true,
                task_message: Some(
                    "Tool timeout detected. Run paused so you can resume and continue."
                        .to_string(),
                ),
                run_message: Some(
                    "Paused: transient tool timeout detected. Resume run to continue.".to_string(),
                ),
            },
            RetryFailureClass::WorkspaceNotFound => FailurePolicy {
                class,
                should_pause: true,
                task_message: Some(
                    "Workspace root not found. Verify workspace path and continue.".to_string(),
                ),
                run_message: Some(
                    "Paused: workspace root not found. Verify workspace path and continue."
                        .to_string(),
                ),
            },
            RetryFailureClass::InvalidToolArgs => FailurePolicy {
                class,
                should_pause: true,
                task_message: Some(
                    "Invalid tool arguments detected (missing/invalid file path). Run paused so you can resume after guardrails."
                        .to_string(),
                ),
                run_message: Some(
                    "Paused: invalid tool args (missing file path). Resume to retry with strict path guardrails."
                        .to_string(),
                ),
            },
            RetryFailureClass::PathNotFound => FailurePolicy {
                class,
                should_pause: true,
                task_message: Some(
                    "Path not found detected (os error 3). Verify workspace/file paths and continue."
                        .to_string(),
                ),
                run_message: Some(
                    "Paused: path not found (os error 3). Verify workspace/file paths and continue."
                        .to_string(),
                ),
            },
            RetryFailureClass::Unknown => FailurePolicy {
                class,
                should_pause: false,
                task_message: None,
                run_message: None,
            },
        }
    }

    fn failure_class_label(class: RetryFailureClass) -> &'static str {
        match class {
            RetryFailureClass::RateLimit => "rate_limit",
            RetryFailureClass::QuotaExceeded => "quota_exceeded",
            RetryFailureClass::AuthFailed => "auth_failed",
            RetryFailureClass::Timeout => "timeout",
            RetryFailureClass::WorkspaceNotFound => "workspace_not_found",
            RetryFailureClass::InvalidToolArgs => "invalid_tool_args",
            RetryFailureClass::PathNotFound => "path_not_found",
            RetryFailureClass::Unknown => "unknown",
        }
    }

    fn pinned_constraints() -> Vec<String> {
        vec![
            "Engine state is source-of-truth".to_string(),
            "Only operate within leased workspace".to_string(),
            "Respect configured tool/policy constraints".to_string(),
        ]
    }

    fn compact_rolling_summary(
        existing_summary: &str,
        why_next_step: &str,
        active_step_id: &str,
        recent_events: &[RunEventRecord],
        pinned_constraints: &[String],
    ) -> String {
        let pinned_line = format!("Pinned: {}", pinned_constraints.join(" | "));
        let recent_digest = recent_events
            .iter()
            .rev()
            .take(6)
            .map(|event| format!("{}:{}:{:?}", event.seq, event.event_type, event.status))
            .collect::<Vec<_>>()
            .join(", ");
        let mut compact = format!(
            "{}\nActive step: {}\nWhy next: {}\nRecent: {}\nPrior: {}",
            pinned_line,
            active_step_id,
            why_next_step,
            if recent_digest.is_empty() {
                "none".to_string()
            } else {
                recent_digest
            },
            existing_summary.trim()
        );
        if compact.len() > Self::ROLLING_SUMMARY_MAX_CHARS {
            compact = compact
                .chars()
                .take(Self::ROLLING_SUMMARY_MAX_CHARS)
                .collect();
        }
        compact
    }

    fn existing_dir_string(path: &Path) -> Option<String> {
        if path.is_dir() {
            Some(path.to_string_lossy().to_string())
        } else {
            None
        }
    }

    fn existing_dir_string_from_opt(value: Option<&str>) -> Option<String> {
        let raw = value?.trim();
        if raw.is_empty() {
            return None;
        }
        let path = PathBuf::from(raw);
        Self::existing_dir_string(&path)
    }

    fn role_tool_allowlist(role: AgentRole) -> Vec<&'static str> {
        match role {
            AgentRole::Orchestrator | AgentRole::Planner | AgentRole::Delegator => vec![
                "ls",
                "list",
                "glob",
                "search",
                "grep",
                "codesearch",
                "read",
                "todowrite",
                "todo_write",
                "update_todo_list",
                "websearch",
                "webfetch",
                "webfetch_html",
                "task",
            ],
            AgentRole::Worker | AgentRole::Builder => vec![
                "ls",
                "list",
                "glob",
                "search",
                "grep",
                "codesearch",
                "read",
                "write",
                "edit",
                "apply_patch",
                "todowrite",
                "todo_write",
                "update_todo_list",
                "websearch",
                "webfetch",
                "webfetch_html",
            ],
            AgentRole::Watcher | AgentRole::Researcher => vec![
                "ls",
                "list",
                "glob",
                "search",
                "grep",
                "codesearch",
                "read",
                "websearch",
                "webfetch",
                "webfetch_html",
            ],
            AgentRole::Reviewer | AgentRole::Validator | AgentRole::Tester => vec![
                "ls",
                "list",
                "glob",
                "search",
                "grep",
                "codesearch",
                "read",
                "websearch",
                "webfetch",
                "webfetch_html",
            ],
        }
    }

    fn known_orchestrator_tools() -> &'static [&'static str] {
        &[
            "ls",
            "list",
            "glob",
            "search",
            "grep",
            "codesearch",
            "read",
            "write",
            "edit",
            "apply_patch",
            "todowrite",
            "todo_write",
            "update_todo_list",
            "websearch",
            "webfetch",
            "webfetch_html",
            "bash",
            "task",
            "spawn_agent",
            "batch",
        ]
    }

    fn orchestrator_permission_rules(role: AgentRole) -> Vec<crate::sidecar::PermissionRule> {
        let allowlist = Self::role_tool_allowlist(role);
        let allow_set = allowlist
            .iter()
            .map(|tool| tool.to_string())
            .collect::<HashSet<_>>();

        let mut rules = tandem_core::build_mode_permission_rules(Some(
            &allowlist
                .iter()
                .map(|tool| tool.to_string())
                .collect::<Vec<_>>(),
        ))
        .into_iter()
        .map(|rule| crate::sidecar::PermissionRule {
            permission: rule.permission,
            pattern: rule.pattern,
            action: rule.action,
        })
        .collect::<Vec<_>>();

        for permission in [
            "write",
            "edit",
            "apply_patch",
            "todowrite",
            "todo_write",
            "update_todo_list",
            "websearch",
            "webfetch",
            "webfetch_html",
            "task",
        ] {
            if allow_set.contains(permission) {
                rules.push(crate::sidecar::PermissionRule {
                    permission: permission.to_string(),
                    pattern: "*".to_string(),
                    action: "allow".to_string(),
                });
            }
        }

        for permission in Self::known_orchestrator_tools() {
            if allow_set.contains(*permission) {
                continue;
            }
            rules.push(crate::sidecar::PermissionRule {
                permission: (*permission).to_string(),
                pattern: "*".to_string(),
                action: "deny".to_string(),
            });
        }

        rules
    }

    const RELAXED_MAX_ITERATIONS: u32 = 1_000_000;
    const RELAXED_MAX_TOTAL_TOKENS: u64 = 100_000_000;
    const RELAXED_MAX_WALL_TIME_SECS: u64 = 7 * 24 * 60 * 60; // 7 days
    const RELAXED_MAX_SUBAGENT_RUNS: u32 = 100_000;

    fn extract_error_code(error: &str) -> Option<String> {
        let trimmed = error.trim();
        if let Some(rest) = trimmed.strip_prefix('[') {
            let end = rest.find(']')?;
            let code = &rest[..end];
            if !code.trim().is_empty() {
                return Some(code.trim().to_string());
            }
        }
        None
    }

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

    fn is_auth_error(error: &str) -> bool {
        let e = error.to_lowercase();
        (e.contains("401") || e.contains("unauthorized") || e.contains("forbidden"))
            && (e.contains("provider") || e.contains("auth") || e.contains("api key"))
            || e.contains("no cookie auth credentials found")
    }

    fn is_transient_timeout_error(error: &str) -> bool {
        let e = error.to_lowercase();
        e.contains("exceeded timeout")
            || e.contains("timed out")
            || e.contains("timeout")
            || e.contains("stream_idle_timeout")
    }

    fn is_workspace_not_found_error(error: &str) -> bool {
        let e = error.to_lowercase();
        e.contains("[workspace_not_found]")
            || e.contains("workspace root not found")
            || e.contains("working directory does not exist")
    }

    fn is_path_not_found_error(error: &str) -> bool {
        let e = error.to_lowercase();
        e.contains("(os error 3)")
            || e.contains("the system cannot find the path specified")
            || e.contains("system cannot find the path specified")
            || e.contains("os error 3")
    }

    fn is_invalid_tool_args_error(error: &str) -> bool {
        let e = error.to_lowercase();
        e.contains("[invalid_tool_args]")
            || e.contains("invalid tool args")
            || e.contains("tool args must be a json object")
            || e.contains("missing required 'path'")
            || e.contains("missing required 'content'")
            || e.contains("file_path_missing")
            || e.contains("write_content_missing")
            || e.contains("write requires `content`")
            || e.contains("write requires non-empty `content`")
    }

    fn should_clear_error_on_resume(error: &str) -> bool {
        Self::is_transient_timeout_error(error)
            || Self::is_rate_limit_error(error)
            || Self::is_provider_quota_error(error)
            || Self::is_auth_error(error)
            || Self::is_workspace_not_found_error(error)
            || Self::is_invalid_tool_args_error(error)
            || Self::is_path_not_found_error(error)
            || error
                .to_lowercase()
                .contains("run paused so you can resume and continue")
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
        let latest_seq = store.latest_run_event_seq(&run_id).unwrap_or(0);
        let mut budget_tracker = BudgetTracker::from_budget(run.budget.clone());
        budget_tracker.set_active(matches!(
            run.status,
            RunStatus::Planning | RunStatus::Running
        ));
        let max_parallel_tasks = run.config.max_parallel_tasks.max(1) as usize;
        let llm_parallel = run.config.llm_parallel.max(1) as usize;
        let pause_signal = Arc::new(RwLock::new(matches!(run.status, RunStatus::Paused)));

        let restored_task_sessions = run
            .tasks
            .iter()
            .filter_map(|task| {
                task.session_id
                    .as_ref()
                    .filter(|sid| !sid.trim().is_empty())
                    .map(|sid| (task.id.clone(), sid.clone()))
            })
            .collect::<HashMap<_, _>>();

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
            event_seq: Arc::new(AtomicU64::new(latest_seq)),
            task_semaphore: Arc::new(Semaphore::new(max_parallel_tasks)),
            llm_semaphore: Arc::new(Semaphore::new(llm_parallel)),
            task_sessions: Arc::new(RwLock::new(restored_task_sessions)),
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
            let reason = e.to_string();
            // Planning can fail before task-level retries kick in. For transient conditions,
            // pause instead of failing so users can continue from Command Center.
            if Self::is_transient_timeout_error(&reason)
                || Self::is_rate_limit_error(&reason)
                || Self::is_provider_quota_error(&reason)
                || Self::is_auth_error(&reason)
            {
                let _ = self.handle_transient_start_failure(&reason).await;
            } else {
                // Ensure the run transitions to a terminal state instead of leaving the UI stuck
                // in "planning" forever.
                let _ = self.handle_failure(&reason).await;
            }
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
                && run.status != RunStatus::Running
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
            if run.status != RunStatus::Running {
                run.status = RunStatus::Running;
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

        let session_id = {
            let run = self.run.read().await;
            run.session_id.clone()
        };

        let analysis_prompt =
            AgentPrompts::build_planner_analysis_prompt(&objective, &workspace_summary);
        let analysis_response = self
            .call_agent(None, &session_id, &analysis_prompt, AgentRole::Orchestrator)
            .await?;
        {
            let mut tracker = self.budget_tracker.write().await;
            tracker.record_tokens(
                None,
                Some(
                    analysis_prompt
                        .len()
                        .saturating_add(analysis_response.len()),
                ),
            );
        }

        let prompt = AgentPrompts::build_planner_prompt(
            &objective,
            &workspace_summary,
            &constraints,
            Some(analysis_response.as_str()),
        );

        // Send message and wait for response
        let response = self
            .call_agent(None, &session_id, &prompt, AgentRole::Orchestrator)
            .await?;

        // Record tokens (estimate from prompt + response length).
        // This avoids showing 0 tokens in planning when providers emit sparse/empty content
        // but still consumed input tokens for the planner prompt.
        {
            let mut tracker = self.budget_tracker.write().await;
            tracker.record_tokens(None, Some(prompt.len().saturating_add(response.len())));
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
                self.validate_workspace_lease("pre_dispatch", Some(&task_id))
                    .await?;
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

                let (task, why_next_step) = {
                    let mut run = self.run.write().await;
                    if let Some(idx) = run.tasks.iter().position(|t| t.id == task_id) {
                        if run.tasks[idx].state != TaskState::Pending
                            && run.tasks[idx].state != TaskState::Runnable
                        {
                            continue;
                        }
                        let why_next_step = format!(
                            "Dependencies satisfied; task `{}` selected by scheduler.",
                            task_id
                        );
                        run.why_next_step = Some(why_next_step.clone());
                        run.tasks[idx].state = TaskState::Runnable;
                        run.tasks[idx].state = TaskState::InProgress;
                        (run.tasks[idx].clone(), why_next_step)
                    } else {
                        continue;
                    }
                };
                if let Ok(pack) = self.build_context_pack(&task, &why_next_step).await {
                    self.emit_event(OrchestratorEvent::ContextPackBuilt {
                        run_id: self.run_id.clone(),
                        task_id: task_id.clone(),
                        why_next_step: why_next_step.clone(),
                        event_count: pack.recent_events.len(),
                        timestamp: chrono::Utc::now(),
                    });
                }

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
            let execution_role = self.resolve_task_execution_role(&task).await;
            self.validate_workspace_lease("pre_tool_call", Some(&task_id))
                .await?;
            let mut session_id = self
                .get_or_create_task_session_id(&task, execution_role)
                .await?;
            self.emit_task_trace(&task_id, Some(&session_id), "EXEC_STARTED", None);

            self.emit_event(OrchestratorEvent::TaskStarted {
                run_id: self.get_run_id().await,
                task_id: task_id.clone(),
                timestamp: chrono::Utc::now(),
            });
            let workspace_before = self.capture_workspace_snapshot().await?;
            let hinted_targets = Self::extract_target_file_hints(&task);
            let hinted_before = self.capture_target_file_snapshot(&hinted_targets).await?;

            // Build context for execution role
            let file_context = self.get_task_file_context(&task).await?;

            if let Some(template_id) = task.template_id.as_deref() {
                self.emit_task_trace(
                    &task_id,
                    Some(&session_id),
                    "TEMPLATE_HINT",
                    Some(format!(
                        "template '{}' is advisory; falling back to role prompt templates",
                        template_id
                    )),
                );
            }
            let why_next_step = {
                let run = self.run.read().await;
                run.why_next_step
                    .clone()
                    .unwrap_or_else(|| format!("Executing task `{}`.", task.id))
            };
            let context_pack_summary = self
                .build_context_pack(&task, &why_next_step)
                .await
                .ok()
                .map(|pack| {
                    let constraints = if pack.pinned_constraints.is_empty() {
                        "none".to_string()
                    } else {
                        pack.pinned_constraints.join("; ")
                    };
                    format!(
                        "Goal: {}\nActive step: {}\nWhy now: {}\nPinned constraints: {}\nRecent summary: {}",
                        pack.goal,
                        pack.active_step_id,
                        why_next_step,
                        constraints,
                        pack.rolling_summary
                    )
                });
            let previous_attempt_context = task
                .validation_result
                .as_ref()
                .map(|v| v.feedback.trim().to_string())
                .filter(|v| !v.is_empty())
                .or_else(|| {
                    task.error_message
                        .as_ref()
                        .map(|msg| msg.trim().to_string())
                        .filter(|msg| !msg.is_empty())
                });
            let prompt = AgentPrompts::build_builder_prompt(
                &task,
                &file_context,
                context_pack_summary.as_deref(),
                previous_attempt_context.as_deref(),
            );

            // Record budget
            {
                let mut tracker = self.budget_tracker.write().await;
                tracker.record_iteration();
            }

            let mut builder_call = self
                .call_agent_for_task_with_recovery(&task, &mut session_id, &prompt, execution_role)
                .await?;
            let mut builder_response = builder_call.content.clone();
            let mut builder_used_tools = builder_call.used_tools;
            let mut builder_used_write_tools = builder_call.used_write_tools;

            // Record tokens
            {
                let mut tracker = self.budget_tracker.write().await;
                tracker.record_tokens(
                    None,
                    Some(prompt.len().saturating_add(builder_response.len())),
                );
            }

            // Get per-task workspace changes for validation.
            let mut workspace_changes = self
                .get_recent_changes_since(&workspace_before)
                .await?;
            let hinted_changes = self
                .summarize_target_file_changes(&hinted_before, &hinted_targets)
                .await?;
            if !hinted_changes.is_empty() {
                workspace_changes.has_changes = true;
                if !workspace_changes.diff_text.is_empty() {
                    workspace_changes.diff_text.push('\n');
                }
                workspace_changes.diff_text.push_str("Hint-target file changes:");
                for line in hinted_changes.iter().take(20) {
                    workspace_changes.diff_text.push('\n');
                    workspace_changes.diff_text.push_str(line);
                }
            }
            if self.task_requires_workspace_changes(&task) && !workspace_changes.has_changes {
                // One targeted recovery pass: force explicit file-tool usage with concrete paths.
                self.emit_task_trace(
                    &task.id,
                    Some(session_id.as_str()),
                    "NO_CHANGES_RECOVERY_RETRY",
                    Some("retry=1/1 reason=no_workspace_changes_captured".to_string()),
                );
                let target_hint = if hinted_targets.is_empty() {
                    "the expected artifact file".to_string()
                } else {
                    hinted_targets.join(", ")
                };
                let recovery_prompt = format!(
                    "{}\n\n[Critical Recovery]\n\
Your previous attempt did not persist any workspace file changes.\n\
You MUST perform concrete file edits now.\n\
- Use `write`, `edit`, or `apply_patch` with explicit JSON args.\n\
- For file tools, include a non-empty `path` string.\n\
- Prefer these target files when relevant: {}\n\
- Do not finish until at least one target file has actually changed on disk.",
                    AgentPrompts::build_builder_prompt(
                        &task,
                        &file_context,
                        context_pack_summary.as_deref(),
                        Some(builder_response.as_str())
                    ),
                    target_hint
                );
                builder_call = self
                    .call_agent_for_task_with_recovery(
                        &task,
                        &mut session_id,
                        &recovery_prompt,
                        execution_role,
                    )
                    .await?;
                builder_response = builder_call.content.clone();
                builder_used_tools = builder_used_tools || builder_call.used_tools;
                builder_used_write_tools =
                    builder_used_write_tools || builder_call.used_write_tools;
                {
                    let mut tracker = self.budget_tracker.write().await;
                    tracker.record_tokens(
                        None,
                        Some(recovery_prompt.len().saturating_add(builder_response.len())),
                    );
                }

                workspace_changes = self
                    .get_recent_changes_since(&workspace_before)
                    .await?;
                let hinted_changes = self
                    .summarize_target_file_changes(&hinted_before, &hinted_targets)
                    .await?;
                if !hinted_changes.is_empty() {
                    workspace_changes.has_changes = true;
                    if !workspace_changes.diff_text.is_empty() {
                        workspace_changes.diff_text.push('\n');
                    }
                    workspace_changes.diff_text.push_str("Hint-target file changes:");
                    for line in hinted_changes.iter().take(20) {
                        workspace_changes.diff_text.push('\n');
                        workspace_changes.diff_text.push_str(line);
                    }
                }

                if !workspace_changes.has_changes {
                    if !builder_used_tools {
                        self.emit_task_trace(
                            &task.id,
                            Some(session_id.as_str()),
                            "NO_TOOL_CALLS_DETECTED",
                            Some("builder did not invoke tools after recovery prompt".to_string()),
                        );
                        return Err(TandemError::Orchestrator(
                            "Task requires file changes, but builder invoked no tools. Use read/glob to inspect inputs and write/edit/apply_patch to create or modify files before finishing.".to_string(),
                        ));
                    }
                    if !builder_used_write_tools {
                        self.emit_task_trace(
                            &task.id,
                            Some(session_id.as_str()),
                            "NO_WRITE_TOOL_CALLS_DETECTED",
                            Some("builder invoked only read-only tools after recovery prompt".to_string()),
                        );
                        return Err(TandemError::Orchestrator(
                            "Task requires file changes, but builder only invoked read-only tools. Use write/edit/apply_patch with a concrete path to produce the required artifact.".to_string(),
                        ));
                    }
                    return Err(TandemError::Orchestrator(format!(
                        "Task requires file changes but none were captured. Use write/edit/apply_patch to update {} before completing.",
                        target_hint
                    )));
                }
            }
            let changes_diff = workspace_changes.diff_text;

            // Build validator prompt
            let validator_prompt = AgentPrompts::build_validator_prompt(
                &task,
                &changes_diff,
                Some(builder_response.as_str()),
            );

            // Call validator
            let validator_response = self
                .call_agent_for_task_with_recovery(
                    &task,
                    &mut session_id,
                    &validator_prompt,
                    self.resolve_task_validation_role(&task, execution_role),
                )
                .await?
                .content;

            // Record tokens
            {
                let mut tracker = self.budget_tracker.write().await;
                tracker.record_tokens(
                    None,
                    Some(validator_prompt.len().saturating_add(validator_response.len())),
                );
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
                                let validator_feedback = t
                                    .validation_result
                                    .as_ref()
                                    .map(|v| v.feedback.trim())
                                    .filter(|v| !v.is_empty())
                                    .map(|v| v.to_string());
                                t.error_message = Some(match validator_feedback {
                                    Some(feedback) => {
                                        format!(
                                            "Max retries exceeded. Last validator feedback: {}",
                                            feedback
                                        )
                                    }
                                    None => "Max retries exceeded".to_string(),
                                });
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
        let policy = Self::failure_policy(error);
        let error_code = Self::extract_error_code(error);
        let should_pause = policy.should_pause;

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
                    if let Some(msg) = policy.task_message.clone() {
                        t.error_message = Some(msg);
                    }
                    run.error_message = policy.run_message.clone();
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
            "RETRY_CLASSIFIED",
            Some(format!(
                "class={} action={}{}",
                Self::failure_class_label(policy.class),
                if should_pause {
                    "pause"
                } else {
                    "retry_or_fail"
                },
                error_code
                    .as_deref()
                    .map(|code| format!(" code={}", code))
                    .unwrap_or_default()
            )),
        );

        self.emit_task_trace(
            task_id,
            session_id.as_deref(),
            "EXEC_FINISHED",
            Some(format!(
                "error: {}{}",
                error,
                error_code
                    .as_deref()
                    .map(|code| format!(" code={}", code))
                    .unwrap_or_default()
            )),
        );

        Ok(())
    }

    async fn get_or_create_task_session_id(&self, task: &Task, role: AgentRole) -> Result<String> {
        use crate::sidecar::CreateSessionRequest;

        if !self.workspace_path.is_dir() {
            return Err(TandemError::Sidecar(format!(
                "[WORKSPACE_NOT_FOUND] workspace root not found: {}",
                self.workspace_path.display()
            )));
        }

        if let Some(existing) = self.task_sessions.read().await.get(&task.id).cloned() {
            return Ok(existing);
        }

        if let Some(existing) = task
            .session_id
            .as_ref()
            .map(|sid| sid.trim().to_string())
            .filter(|sid| !sid.is_empty())
        {
            self.task_sessions
                .write()
                .await
                .insert(task.id.clone(), existing.clone());
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

        let permission = Some(Self::orchestrator_permission_rules(role));

        let provider = base_session.provider.clone();
        let model = match (base_session.model.clone(), provider.clone()) {
            (Some(model_id), Some(provider_id)) => Some(json!({
                "providerID": provider_id,
                "modelID": model_id
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
            // Pin task sessions to the known engine workspace to avoid drift across projects.
            directory: Some(self.workspace_path.to_string_lossy().to_string()),
            workspace_root: Self::existing_dir_string_from_opt(
                base_session.workspace_root.as_deref(),
            )
            .or_else(|| Some(self.workspace_path.to_string_lossy().to_string())),
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
        role: AgentRole,
    ) -> Result<AgentCallOutcome> {
        let max_timeout_retries = {
            let run = self.run.read().await;
            run.config.max_timeout_retries_per_task_attempt
        };
        let mut timeout_retries_used: u32 = 0;
        let mut session_recreated_after_404 = false;
        let mut invalid_tool_args_retry_used = false;
        let mut path_not_found_retry_used = false;
        let mut effective_prompt = prompt.to_string();

        loop {
            match self
                .call_agent_with_outcome(
                    Some(&task.id),
                    session_id.as_str(),
                    &effective_prompt,
                    role,
                )
                .await
            {
                Ok(response) => return Ok(response),
                Err(e) if is_sidecar_session_not_found(&e) && !session_recreated_after_404 => {
                    session_recreated_after_404 = true;
                    tracing::warn!(
                        "Task {} session {} missing (404). Recreating task session and retrying once.",
                        task.id,
                        session_id
                    );
                    self.invalidate_task_session(&task.id).await;
                    *session_id = self.get_or_create_task_session_id(task, role).await?;
                }
                Err(e)
                    if Self::is_invalid_tool_args_error(&e.to_string())
                        && !invalid_tool_args_retry_used =>
                {
                    invalid_tool_args_retry_used = true;
                    tracing::warn!(
                        "Task {} role {:?} emitted invalid tool args on session {}. Retrying once with strict tool-arg reminder.",
                        task.id,
                        role,
                        session_id
                    );
                    self.emit_task_trace(
                        &task.id,
                        Some(session_id.as_str()),
                        "AGENT_INVALID_TOOL_ARGS_RETRY",
                        Some("retry=1/1 reason=missing_read_path".to_string()),
                    );
                    effective_prompt = format!(
                        "{prompt}\n\n[Tool Argument Guardrail]\n\
When you call file tools, arguments MUST be a JSON object.\n\
- `read` requires: {{\"path\":\"relative/or/absolute/path\"}}\n\
- `write` requires: {{\"path\":\"relative/or/absolute/path\", \"content\":\"...\"}}\n\
Never call `read` or `write` with empty args (`{{}}`) or missing required fields.\n\
For `write`, always include a non-empty `content` string.\n\
If unsure, run `glob` first to discover files, then call `read` with a concrete path."
                    );
                    self.invalidate_task_session(&task.id).await;
                    *session_id = self.get_or_create_task_session_id(task, role).await?;
                }
                Err(e)
                    if Self::is_path_not_found_error(&e.to_string())
                        && !path_not_found_retry_used =>
                {
                    path_not_found_retry_used = true;
                    tracing::warn!(
                        "Task {} role {:?} hit path-not-found on session {}. Retrying once with strict path guardrail.",
                        task.id,
                        role,
                        session_id
                    );
                    self.emit_task_trace(
                        &task.id,
                        Some(session_id.as_str()),
                        "AGENT_PATH_NOT_FOUND_RETRY",
                        Some("retry=1/1 reason=os_error_3".to_string()),
                    );
                    effective_prompt = format!(
                        "{prompt}\n\n[Path Guardrail]\n\
Your previous attempt hit a path-not-found error (Windows os error 3).\n\
When calling `read`/`write`/`edit`, ALWAYS include a non-empty `path` string.\n\
- Never call file tools with empty args (`{{}}`) or missing `path`.\n\
- For `write`, include a non-empty `content` string.\n\
- Use `glob` first to discover real files/dirs in this workspace.\n\
- Prefer workspace-relative paths that already exist.\n\
- If creating a new file, ensure the parent directory exists first."
                    );
                    self.invalidate_task_session(&task.id).await;
                    *session_id = self.get_or_create_task_session_id(task, role).await?;
                }
                Err(e)
                    if Self::is_transient_timeout_error(&e.to_string())
                        && timeout_retries_used < max_timeout_retries =>
                {
                    timeout_retries_used += 1;
                    tracing::warn!(
                        "Task {} role {:?} timed out on session {} (retry {}/{}). Recreating task session and retrying.",
                        task.id,
                        role,
                        session_id,
                        timeout_retries_used,
                        max_timeout_retries
                    );
                    self.emit_task_trace(
                        &task.id,
                        Some(session_id.as_str()),
                        "AGENT_TIMEOUT_RETRY",
                        Some(format!(
                            "retry={}/{} role={:?}",
                            timeout_retries_used, max_timeout_retries, role
                        )),
                    );
                    self.invalidate_task_session(&task.id).await;
                    *session_id = self.get_or_create_task_session_id(task, role).await?;
                }
                Err(e) => return Err(e),
            }
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
        use crate::sidecar::CreateSessionRequest;

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
                    "providerID": provider_id,
                    "modelID": model_id
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
            permission: Some(Self::orchestrator_permission_rules(AgentRole::Orchestrator)),
            directory: Self::existing_dir_string(&self.workspace_path),
            workspace_root: Self::existing_dir_string(&self.workspace_path),
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
        role: AgentRole,
    ) -> Result<String> {
        self.call_agent_with_outcome(task_id, session_id, prompt, role)
            .await
            .map(|o| o.content)
    }

    async fn call_agent_with_outcome(
        &self,
        task_id: Option<&str>,
        session_id: &str,
        prompt: &str,
        role: AgentRole,
    ) -> Result<AgentCallOutcome> {
        use crate::sidecar::{ModelSpec, SendMessageRequest, StreamEvent};
        self.validate_workspace_lease("pre_tool_call", task_id)
            .await?;

        let _llm_permit = self
            .llm_semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| TandemError::Orchestrator("Failed to acquire LLM permit".to_string()))?;

        tracing::info!("Agent call with prompt length: {}", prompt.len());

        // Prefer the model/provider persisted on the run. (Some OpenCode builds don't populate
        // legacy `session.model/provider` in GET /session responses.)
        let (run_model, run_provider) = self.get_model_provider_for_role(role).await;
        let model_spec = match (run_provider.clone(), run_model.clone()) {
            (Some(provider_id), Some(model_id))
                if !provider_id.trim().is_empty() && !model_id.trim().is_empty() =>
            {
                tracing::info!(
                    "Orchestrator agent call using role model: role={:?} provider={} model={}",
                    role,
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
        let agent_call_timeout_secs = {
            let run = self.run.read().await;
            run.config.max_agent_call_secs.max(60)
        };

        // Then send message to sidecar
        let mut request = SendMessageRequest::text(prompt.to_string());
        request.model = model_spec.clone();
        {
            let mut tracker = self.budget_tracker.write().await;
            tracker.record_subagent_run();
        }
        if let Err(e) = self
            .sidecar
            .append_message_and_start_run(&active_session_id, request)
            .await
        {
            if task_id.is_none() && is_sidecar_session_not_found(&e) {
                tracing::warn!(
                    "Base orchestrator session {} missing (404) during agent call for role {:?}. Recreating and retrying once.",
                    active_session_id
                    ,
                    role
                );
                active_session_id = self.recreate_base_run_session().await?;
                let mut retry_request = SendMessageRequest::text(prompt.to_string());
                retry_request.model = model_spec;
                {
                    let mut tracker = self.budget_tracker.write().await;
                    tracker.record_subagent_run();
                }
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
        let mut used_write_tools = false;
        let mut first_tool_finished = false;
        let mut stalled_windows: usize = 0;
        const MAX_STALLED_WINDOWS_BEFORE_FAIL: usize = 4;
        let mut last_relevant_event_at = Instant::now();

        // Add a hard timeout to prevent hanging forever (even if the sidecar only sends heartbeats).
        // Planning can legitimately take a while (large repos, slower models, cold starts).
        // Keep this reasonably high so we don't fail healthy runs, but still fail-fast for true hangs.
        let timeout = tokio::time::Duration::from_secs(agent_call_timeout_secs);
        let consume = async {
            let per_event_timeout = tokio::time::Duration::from_secs(60);
            loop {
                let next = tokio::time::timeout(per_event_timeout, stream.recv()).await;
                let event = match next {
                    Ok(Ok(env)) => Some(env.payload),
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped))) => {
                        tracing::warn!("Orchestrator stream lagged by {} events", skipped);
                        continue;
                    }
                    Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                    Err(_) => {
                        // Still run the per-session stall detection below even when no event arrived.
                        None
                    }
                };
                if let Some(event) = event.as_ref() {
                    match event {
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
                                    tracing::debug!(
                                        "Got full content: {} chars",
                                        full_content.len()
                                    );
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
                            let normalized_tool = tool
                                .split(':')
                                .next_back()
                                .unwrap_or(tool.as_str())
                                .trim()
                                .to_ascii_lowercase();
                            if sid == &active_session_id
                                && matches!(
                                    normalized_tool.as_str(),
                                    "write" | "edit" | "apply_patch"
                                )
                            {
                                used_write_tools = true;
                            }
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
                            if sid == &active_session_id && is_nested_agent_tool(tool) {
                                let mut tracker = self.budget_tracker.write().await;
                                tracker.record_subagent_run();
                                if let Some(task_id) = task_id {
                                    self.emit_task_trace(
                                        task_id,
                                        Some(&active_session_id),
                                        "AGENT_CALL_NESTED",
                                        Some(format!("via_tool={}", tool)),
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
                            error_code,
                        } => {
                            if sid == &active_session_id {
                                tracing::error!("Session {} error: {}", active_session_id, error);
                                if let Some(code) = error_code {
                                    if error.starts_with('[') {
                                        errors.push(error.clone());
                                    } else {
                                        errors.push(format!("[{}] {}", code, error));
                                    }
                                } else {
                                    errors.push(error.clone());
                                }
                                break;
                            }
                        }
                        StreamEvent::Raw { event_type, data } => {
                            tracing::debug!(
                                "Raw event for orchestrator: {} - {:?}",
                                event_type,
                                data
                            );
                        }
                        _ => {}
                    }
                };

                let is_relevant_event = match event.as_ref() {
                    Some(StreamEvent::Content {
                        session_id: sid, ..
                    })
                    | Some(StreamEvent::SessionIdle { session_id: sid })
                    | Some(StreamEvent::ToolStart {
                        session_id: sid, ..
                    })
                    | Some(StreamEvent::ToolEnd {
                        session_id: sid, ..
                    })
                    | Some(StreamEvent::SessionError {
                        session_id: sid, ..
                    }) => sid == &active_session_id,
                    _ => false,
                };

                if is_relevant_event {
                    stalled_windows = 0;
                    last_relevant_event_at = Instant::now();
                    continue;
                }

                let elapsed = last_relevant_event_at.elapsed().as_secs();
                let next_window_secs = (stalled_windows as u64 + 1) * 60;
                if elapsed < next_window_secs {
                    continue;
                }

                stalled_windows = stalled_windows.saturating_add(1);
                tracing::warn!(
                    "No relevant SSE events for session {} for {}s (window {}/{}), probing active run",
                    active_session_id,
                    elapsed,
                    stalled_windows,
                    MAX_STALLED_WINDOWS_BEFORE_FAIL
                );

                match self.sidecar.get_active_run(&active_session_id).await {
                    Ok(Some(active)) => {
                        if let Some(task_id) = task_id {
                            self.emit_task_trace(
                                task_id,
                                Some(&active_session_id),
                                "STREAM_STALLED",
                                Some(format!(
                                    "run={} last_activity_ms={} stalled_for_secs={}",
                                    active.run_id, active.last_activity_at_ms, elapsed
                                )),
                            );
                        }

                        if stalled_windows >= MAX_STALLED_WINDOWS_BEFORE_FAIL {
                            errors.push(format!(
                                "No relevant SSE events for {}s while run {} remained active (session {})",
                                elapsed, active.run_id, active_session_id
                            ));
                            break;
                        }
                    }
                    Ok(None) => {
                        tracing::info!(
                            "No active run found for {}; treating session stall as terminal boundary",
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
                                "No relevant SSE events for {}s and run probe failed for session {}: {}",
                                elapsed, active_session_id, probe_err
                            ));
                            break;
                        }
                    }
                }
            }
        };

        if tokio::time::timeout(timeout, consume).await.is_err() {
            tracing::warn!(
                "Agent call timed out after {:?} (task={:?} role={:?} session={})",
                timeout,
                task_id,
                role,
                active_session_id
            );
            errors.push(format!(
                "Timed out after {:?} (task={:?} role={:?} session={})",
                timeout, task_id, role, active_session_id
            ));
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
        Ok(AgentCallOutcome {
            content,
            used_tools: first_tool_part_id.is_some(),
            used_write_tools,
        })
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
        let mut top_level_entries: Vec<String> = Vec::new();
        let mut non_meta_entries: Vec<String> = Vec::new();

        let mut dir = match fs::read_dir(&self.workspace_path).await {
            Ok(dir) => dir,
            Err(err) => {
                return Ok(format!(
                    "Workspace: {}\nWorkspace scan unavailable: {}",
                    self.workspace_path.display(),
                    err
                ));
            }
        };

        while let Ok(Some(entry)) = dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".tandem" {
                continue;
            }
            top_level_entries.push(name.clone());
            if name != ".git" {
                non_meta_entries.push(name);
            }
        }

        top_level_entries.sort();
        non_meta_entries.sort();

        let preview = if top_level_entries.is_empty() {
            "(none)".to_string()
        } else {
            top_level_entries
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        };

        let sparse_note = if non_meta_entries.is_empty() {
            "\nWorkspace appears empty or metadata-only. Prefer research-first planning and create starter project artifacts rather than repeated local shell discovery."
        } else {
            ""
        };

        Ok(format!(
            "Workspace: {}\nTop-level entries (excluding .tandem): {}\nNon-metadata entry count: {}{}",
            self.workspace_path.display(),
            preview,
            non_meta_entries.len(),
            sparse_note
        ))
    }

    /// Get file context relevant to a task
    async fn get_task_file_context(&self, _task: &Task) -> Result<String> {
        let mut dir = match fs::read_dir(&self.workspace_path).await {
            Ok(dir) => dir,
            Err(err) => {
                return Ok(format!("File context unavailable: {}", err));
            }
        };

        let mut entries: Vec<String> = Vec::new();
        while let Ok(Some(entry)) = dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == ".tandem" {
                continue;
            }
            entries.push(name);
        }
        entries.sort();

        let non_meta: Vec<String> = entries
            .iter()
            .filter(|name| name.as_str() != ".git")
            .cloned()
            .collect();

        if non_meta.is_empty() {
            return Ok("Workspace appears metadata-only (.git and/or empty after excluding .tandem). Prefer web research and generating initial scaffold files; avoid repeated shell discovery loops.".to_string());
        }

        Ok(format!(
            "Top-level workspace entries: {}",
            non_meta.into_iter().take(25).collect::<Vec<_>>().join(", ")
        ))
    }

    async fn build_context_pack(&self, task: &Task, why_next_step: &str) -> Result<ContextPack> {
        let run = self.run.read().await;
        let recent_events = self
            .store
            .load_run_events(&run.run_id, None, Some(20))
            .unwrap_or_default();
        let blackboard = self.store.load_blackboard(&run.run_id).unwrap_or_default();
        let pinned_constraints = Self::pinned_constraints();
        let rolling_summary = Self::compact_rolling_summary(
            &blackboard.summaries.rolling,
            why_next_step,
            &task.id,
            &recent_events,
            &pinned_constraints,
        );
        Ok(ContextPack {
            goal: run.objective.clone(),
            pinned_constraints,
            active_step_id: task.id.clone(),
            recent_events,
            rolling_summary,
        })
    }

    async fn validate_workspace_lease(&self, phase: &str, task_id: Option<&str>) -> Result<()> {
        let current = std::fs::canonicalize(&self.workspace_path)
            .unwrap_or_else(|_| self.workspace_path.clone())
            .to_string_lossy()
            .to_string();
        let mut run = self.run.write().await;
        if run.workspace_lease.canonical_path.is_empty() {
            run.workspace_lease.canonical_path = current.clone();
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            run.workspace_lease.canonical_path.hash(&mut hasher);
            run.workspace_lease.workspace_id = format!("ws-{:x}", hasher.finish());
            if run.workspace_lease.lease_epoch == 0 {
                run.workspace_lease.lease_epoch = 1;
            }
            return Ok(());
        }
        if run.workspace_lease.canonical_path != current {
            let run_id = run.run_id.clone();
            let expected = run.workspace_lease.canonical_path.clone();
            run.status = RunStatus::Paused;
            run.error_message = Some(format!(
                "Workspace lease mismatch detected in {}. Expected {}, got {}.",
                phase, expected, current
            ));
            drop(run);
            self.emit_event(OrchestratorEvent::WorkspaceMismatch {
                run_id,
                task_id: task_id.map(|value| value.to_string()),
                expected_path: expected,
                actual_path: current,
                phase: phase.to_string(),
                timestamp: chrono::Utc::now(),
            });
            *self.pause_signal.write().await = true;
            return Err(TandemError::Orchestrator(
                "workspace lease mismatch; run paused".to_string(),
            ));
        }
        Ok(())
    }

    fn should_track_workspace_path(rel_path: &str) -> bool {
        let normalized = rel_path.replace('\\', "/");
        !normalized.is_empty()
            && !normalized.starts_with(".git/")
            && normalized != ".git"
            && !normalized.starts_with(".tandem/")
            && normalized != ".tandem"
            && !normalized.starts_with("node_modules/")
            && !normalized.starts_with("target/")
    }

    fn hash_file_contents(path: &Path) -> Option<u64> {
        let bytes = std::fs::read(path).ok()?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        Some(hasher.finish())
    }

    fn fingerprint_path(path: &Path) -> Option<FileFingerprint> {
        if !path.is_file() {
            return None;
        }
        let meta = std::fs::metadata(path).ok()?;
        let modified_ms = meta
            .modified()
            .ok()
            .and_then(|ts| {
                ts.duration_since(std::time::UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_millis())
            })
            .unwrap_or(0);
        let size = meta.len();
        let content_hash = if size <= 1_048_576 {
            Self::hash_file_contents(path)
        } else {
            None
        };
        Some(FileFingerprint {
            size,
            modified_ms,
            content_hash,
        })
    }

    async fn capture_workspace_snapshot(&self) -> Result<HashMap<String, FileFingerprint>> {
        let root = self.workspace_path.clone();
        tokio::task::spawn_blocking(move || {
            let mut snapshot: HashMap<String, FileFingerprint> = HashMap::new();
            let mut walker = WalkBuilder::new(&root);
            walker.hidden(false);
            walker.git_ignore(true);
            walker.git_global(true);
            walker.git_exclude(true);
            for entry in walker.build().flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let rel = match path.strip_prefix(&root) {
                    Ok(p) => p.to_string_lossy().replace('\\', "/"),
                    Err(_) => continue,
                };
                if !Self::should_track_workspace_path(&rel) {
                    continue;
                }
                let Some(fingerprint) = Self::fingerprint_path(path) else {
                    continue;
                };
                snapshot.insert(rel, fingerprint);
            }
            Ok::<_, TandemError>(snapshot)
        })
        .await
        .map_err(|e| TandemError::Orchestrator(format!("workspace snapshot task failed: {}", e)))?
    }

    async fn capture_target_file_snapshot(
        &self,
        targets: &[String],
    ) -> Result<HashMap<String, Option<FileFingerprint>>> {
        if targets.is_empty() {
            return Ok(HashMap::new());
        }
        let root = self.workspace_path.clone();
        let targets = targets.to_vec();
        tokio::task::spawn_blocking(move || {
            let mut out = HashMap::new();
            for target in targets {
                let raw = target.trim();
                if raw.is_empty() {
                    continue;
                }
                let path = {
                    let candidate = PathBuf::from(raw);
                    if candidate.is_absolute() {
                        candidate
                    } else {
                        root.join(candidate)
                    }
                };
                out.insert(target, Self::fingerprint_path(&path));
            }
            Ok::<_, TandemError>(out)
        })
        .await
        .map_err(|e| TandemError::Orchestrator(format!("target snapshot task failed: {}", e)))?
    }

    async fn summarize_target_file_changes(
        &self,
        before: &HashMap<String, Option<FileFingerprint>>,
        targets: &[String],
    ) -> Result<Vec<String>> {
        if targets.is_empty() {
            return Ok(Vec::new());
        }
        let after = self.capture_target_file_snapshot(targets).await?;
        let mut changed = Vec::new();
        for target in targets {
            let old_fp = before.get(target).cloned().flatten();
            let new_fp = after.get(target).cloned().flatten();
            if old_fp != new_fp {
                let symbol = match (old_fp.is_some(), new_fp.is_some()) {
                    (false, true) => "+",
                    (true, false) => "-",
                    _ => "~",
                };
                changed.push(format!("{} {}", symbol, target));
            }
        }
        Ok(changed)
    }

    fn summarize_workspace_changes(
        before: &HashMap<String, FileFingerprint>,
        after: &HashMap<String, FileFingerprint>,
    ) -> WorkspaceChangeSummary {
        let mut created = Vec::new();
        let mut updated = Vec::new();
        let mut deleted = Vec::new();

        for (path, after_meta) in after {
            match before.get(path) {
                None => created.push(path.clone()),
                Some(before_meta) => {
                    let changed = before_meta.size != after_meta.size
                        || before_meta.modified_ms != after_meta.modified_ms
                        || before_meta.content_hash != after_meta.content_hash;
                    if changed {
                        updated.push(path.clone());
                    }
                }
            }
        }
        for path in before.keys() {
            if !after.contains_key(path) {
                deleted.push(path.clone());
            }
        }

        created.sort();
        updated.sort();
        deleted.sort();

        let mut lines = Vec::new();
        lines.push(format!(
            "Workspace changes: {} created, {} updated, {} deleted",
            created.len(),
            updated.len(),
            deleted.len()
        ));
        for path in created.iter().take(20) {
            lines.push(format!("+ {}", path));
        }
        for path in updated.iter().take(40) {
            lines.push(format!("~ {}", path));
        }
        for path in deleted.iter().take(20) {
            lines.push(format!("- {}", path));
        }

        WorkspaceChangeSummary {
            has_changes: !(created.is_empty() && updated.is_empty() && deleted.is_empty()),
            created,
            updated,
            deleted,
            diff_text: lines.join("\n"),
        }
    }

    async fn get_recent_changes_since(
        &self,
        before: &HashMap<String, FileFingerprint>,
    ) -> Result<WorkspaceChangeSummary> {
        let after = self.capture_workspace_snapshot().await?;
        Ok(Self::summarize_workspace_changes(before, &after))
    }

    fn task_requires_workspace_changes(&self, task: &Task) -> bool {
        let mut text = format!("{} {}", task.title, task.description);
        if !task.acceptance_criteria.is_empty() {
            text.push(' ');
            text.push_str(&task.acceptance_criteria.join(" "));
        }
        let lowered = text.to_lowercase();
        let explicit_no_write_markers = [
            "no code changes",
            "read-only",
            "read only",
            "analysis only",
            "do not modify",
            "without modifying",
            "without changes",
        ];
        if explicit_no_write_markers
            .iter()
            .any(|kw| lowered.contains(kw))
        {
            return false;
        }

        let explicit_write_tool_markers = ["`write`", "`edit`", "`apply_patch`", "apply_patch"];
        if explicit_write_tool_markers
            .iter()
            .any(|kw| lowered.contains(kw))
        {
            return true;
        }

        let write_verbs = [
            "write",
            "create",
            "update",
            "modify",
            "edit",
            "save",
            "implement",
            "add",
            "patch",
            "refactor",
            "rewrite",
            "generate",
            "draft",
            "produce",
        ];
        let artifact_terms = [
            "file",
            "files",
            "document",
            "documents",
            "doc",
            "docs",
            "artifact",
            "artifacts",
            "markdown",
            "readme",
            "spec",
            "specification",
            ".md",
            ".ts",
            ".tsx",
            ".rs",
            ".json",
            ".yaml",
            ".yml",
            ".py",
            ".js",
        ];
        let read_only_intents = [
            "review",
            "read",
            "analyze",
            "analyse",
            "audit",
            "inspect",
            "summarize",
            "summarise",
            "brainstorm",
            "research",
            "discover",
            "map",
            "plan",
        ];

        let has_write_verb = write_verbs.iter().any(|kw| lowered.contains(kw));
        let has_artifact_term = artifact_terms.iter().any(|kw| lowered.contains(kw));
        if has_write_verb && has_artifact_term {
            return true;
        }

        let has_read_only_intent = read_only_intents.iter().any(|kw| lowered.contains(kw));
        if has_read_only_intent && !has_write_verb {
            return false;
        }

        false
    }

    fn extract_target_file_hints(task: &Task) -> Vec<String> {
        let mut tokens = Vec::new();
        tokens.extend(task.title.split_whitespace().map(|s| s.to_string()));
        tokens.extend(task.description.split_whitespace().map(|s| s.to_string()));
        for criterion in &task.acceptance_criteria {
            tokens.extend(criterion.split_whitespace().map(|s| s.to_string()));
        }
        let mut hints = HashSet::new();
        for raw in tokens {
            let token = raw
                .trim_matches(|c: char| {
                    c == '`'
                        || c == '"'
                        || c == '\''
                        || c == ','
                        || c == '.'
                        || c == ';'
                        || c == ':'
                        || c == '('
                        || c == ')'
                })
                .to_string();
            if token.contains('/')
                || token.contains('\\')
                || token.contains(".md")
                || token.contains(".ts")
                || token.contains(".tsx")
                || token.contains(".rs")
                || token.contains(".json")
                || token.contains(".yaml")
                || token.contains(".yml")
                || token.contains(".py")
                || token.contains(".js")
            {
                hints.insert(token);
            }
        }
        let mut out: Vec<String> = hints.into_iter().collect();
        out.sort();
        out.truncate(5);
        out
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
            run.status = RunStatus::Running;
            run.ended_at = None;
            if run
                .error_message
                .as_deref()
                .is_some_and(Self::should_clear_error_on_resume)
            {
                run.error_message = None;
            }
            // Preserve per-task error context on resume so the next attempt prompt can carry
            // forward concrete failure rationale instead of restarting from zero context.
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

    /// Re-queue a failed task so it can be attempted again without restarting the entire run.
    pub async fn retry_failed_task(&self, task_id: &str) -> Result<()> {
        {
            let mut run = self.run.write().await;
            let task = run
                .tasks
                .iter_mut()
                .find(|t| t.id == task_id)
                .ok_or_else(|| TandemError::NotFound(format!("Task not found: {}", task_id)))?;

            if task.state != TaskState::Failed {
                return Err(TandemError::InvalidOperation(format!(
                    "Task {} is not failed (current state: {:?})",
                    task_id, task.state
                )));
            }

            task.state = TaskState::Pending;
            task.retry_count = 0;
            task.error_message = None;
            task.validation_result = None;

            TaskScheduler::update_blocked_tasks(&mut run.tasks);

            if matches!(run.status, RunStatus::Failed | RunStatus::Cancelled) {
                run.status = RunStatus::Paused;
                run.ended_at = None;
            }

            if run
                .error_message
                .as_deref()
                .is_some_and(|msg| msg.contains(task_id) || msg.contains("Deadlock detected"))
            {
                run.error_message = None;
            } else if run
                .error_message
                .as_deref()
                .is_some_and(Self::should_clear_error_on_resume)
            {
                run.error_message = None;
            }
        }

        {
            let task_session_id = {
                let run = self.run.read().await;
                run.tasks
                    .iter()
                    .find(|task| task.id == task_id)
                    .and_then(|task| task.session_id.clone())
            };
            if let Some(session_id) = task_session_id {
                self.task_sessions
                    .write()
                    .await
                    .insert(task_id.to_string(), session_id);
            }
        }

        self.save_state().await?;

        self.emit_task_trace(task_id, None, "TASK_REQUEUED", None);

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
            run.status = RunStatus::Blocked;
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

    async fn handle_transient_start_failure(&self, reason: &str) -> Result<()> {
        {
            let run = self.run.read().await;
            if run.status == RunStatus::Cancelled {
                return Ok(());
            }
        }
        self.budget_tracker.write().await.set_active(false);
        {
            let mut run = self.run.write().await;
            run.status = RunStatus::Paused;
            run.ended_at = None;
            run.error_message = Some(if Self::is_provider_quota_error(reason) {
                "Paused: provider quota/credits exceeded during planning. Switch model/provider and continue."
                    .to_string()
            } else if Self::is_auth_error(reason) {
                "Paused: provider authentication failed during planning (401/403). Check API key/provider selection and continue."
                    .to_string()
            } else if Self::is_rate_limit_error(reason) {
                "Paused: provider rate-limited during planning. Retry or switch model/provider and continue."
                    .to_string()
            } else {
                "Paused: transient timeout during planning. Continue run to retry planning."
                    .to_string()
            });
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

    async fn resolve_task_execution_role(&self, task: &Task) -> AgentRole {
        match task.assigned_role.as_str() {
            crate::orchestrator::types::ROLE_ORCHESTRATOR => AgentRole::Orchestrator,
            crate::orchestrator::types::ROLE_DELEGATOR => AgentRole::Delegator,
            crate::orchestrator::types::ROLE_WORKER => AgentRole::Worker,
            crate::orchestrator::types::ROLE_WATCHER => AgentRole::Watcher,
            crate::orchestrator::types::ROLE_REVIEWER => AgentRole::Reviewer,
            crate::orchestrator::types::ROLE_TESTER => AgentRole::Tester,
            _ => {
                self.emit_contract_warning(
                    Some(task.id.clone()),
                    "planner",
                    "task_role",
                    "unknown assigned_role; fallback to worker",
                    true,
                    Some(task.assigned_role.clone()),
                )
                .await;
                AgentRole::Worker
            }
        }
    }

    fn resolve_task_validation_role(&self, task: &Task, execution_role: AgentRole) -> AgentRole {
        if matches!(task.gate, Some(crate::orchestrator::types::TaskGate::Test)) {
            return AgentRole::Tester;
        }
        if matches!(
            task.gate,
            Some(crate::orchestrator::types::TaskGate::Review)
        ) {
            return AgentRole::Reviewer;
        }
        if execution_role == AgentRole::Reviewer || execution_role == AgentRole::Tester {
            return execution_role;
        }
        AgentRole::Reviewer
    }

    pub async fn get_run_model_provider(&self) -> (Option<String>, Option<String>) {
        let run = self.run.read().await;
        (run.model.clone(), run.provider.clone())
    }

    pub async fn get_model_provider_for_role(
        &self,
        role: AgentRole,
    ) -> (Option<String>, Option<String>) {
        let run = self.run.read().await;
        let role_selection = run.agent_model_routing.get_for_role(role.role_key());

        if let Some(selection) = role_selection {
            let model = selection
                .model
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            let provider = selection
                .provider
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            if let (Some(model), Some(provider)) = (model, provider) {
                return (Some(model.to_string()), Some(provider.to_string()));
            }
        }

        (run.model.clone(), run.provider.clone())
    }

    pub async fn get_run_model_routing(&self) -> AgentModelRouting {
        let run = self.run.read().await;
        run.agent_model_routing.canonicalized()
    }

    pub async fn set_run_model_provider(&self, model: Option<String>, provider: Option<String>) {
        let mut run = self.run.write().await;
        run.model = model;
        run.provider = provider;
    }

    pub async fn set_run_model_routing(&self, routing: AgentModelRouting) -> Result<()> {
        {
            let mut run = self.run.write().await;
            run.agent_model_routing = routing.canonicalized();
        }
        self.save_state().await
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
        drop(run);
        self.persist_checkpoint("heartbeat", None).await?;

        Ok(())
    }

    async fn persist_checkpoint(&self, reason: &str, seq_override: Option<u64>) -> Result<()> {
        let run = self.run.read().await.clone();
        let budget = self.budget_tracker.write().await.snapshot();
        let task_sessions = self.task_sessions.read().await.clone();
        let seq = match seq_override {
            Some(value) => value,
            None => self.store.latest_run_event_seq(&run.run_id).unwrap_or(0),
        };
        let checkpoint_id = format!("cp-{}-{}", run.run_id, seq);
        let checkpoint = CheckpointSnapshot {
            checkpoint_id: checkpoint_id.clone(),
            run_id: run.run_id.clone(),
            seq,
            ts_ms: crate::logs::now_ms(),
            reason: reason.to_string(),
            run: run.clone(),
            budget,
            task_sessions,
        };
        self.store.save_checkpoint(&run.run_id, &checkpoint)?;
        {
            let mut run_write = self.run.write().await;
            run_write.current_checkpoint_id = Some(checkpoint_id);
            self.store.save_run(&run_write)?;
        }
        Ok(())
    }

    fn checkpoint_reason_for_event(event: &OrchestratorEvent) -> Option<&'static str> {
        match event {
            OrchestratorEvent::TaskStarted { .. } => Some("step_started"),
            OrchestratorEvent::TaskCompleted { .. } => Some("step_completed"),
            OrchestratorEvent::ApprovalRequested { .. } => Some("approval_requested"),
            OrchestratorEvent::ApprovalGranted { .. } => Some("approval_granted"),
            OrchestratorEvent::RunPaused { .. } => Some("run_paused"),
            OrchestratorEvent::RunResumed { .. } => Some("run_resumed"),
            OrchestratorEvent::RunFailed { .. } => Some("run_failed"),
            OrchestratorEvent::RunCompleted { .. } => Some("run_completed"),
            OrchestratorEvent::RevisionRequested { .. } => Some("revision_requested"),
            _ => None,
        }
    }

    fn emit_event(&self, event: OrchestratorEvent) {
        let run_id = self.run_id.clone();
        let store = self.store.clone();
        let event_for_log = event.clone();
        let status = self
            .run
            .try_read()
            .map(|run| run.status)
            .unwrap_or(RunStatus::Queued);
        let seq = self.event_seq.fetch_add(1, Ordering::SeqCst) + 1;

        let payload = serde_json::to_value(&event_for_log).unwrap_or_else(|_| json!({}));
        let record = RunEventRecord {
            event_id: format!("evt-{}", Uuid::new_v4()),
            run_id: run_id.clone(),
            seq,
            ts_ms: crate::logs::now_ms(),
            event_type: Self::event_type_name(&event_for_log).to_string(),
            status,
            step_id: event_for_log.step_id(),
            payload,
        };
        let blackboard_patches = self.blackboard_patches_for_event(&record, &event_for_log);
        let checkpoint_reason =
            Self::checkpoint_reason_for_event(&event_for_log).map(str::to_string);
        let engine = self.clone();

        tokio::task::spawn_blocking(move || {
            if let Err(e) = store.append_run_event(&run_id, &record) {
                tracing::error!("Failed to append sequenced orchestrator event: {}", e);
            }
            if let Err(e) = store.append_event(&run_id, &event_for_log) {
                tracing::error!("Failed to append orchestrator event: {}", e);
            }
            for patch in blackboard_patches {
                if let Err(e) = store.append_blackboard_patch(&run_id, &patch) {
                    tracing::error!("Failed to append blackboard patch: {}", e);
                }
            }
        });
        if let Some(reason) = checkpoint_reason {
            tauri::async_runtime::spawn(async move {
                if let Err(e) = engine.persist_checkpoint(&reason, Some(seq)).await {
                    tracing::error!("Failed to persist orchestrator checkpoint: {}", e);
                }
            });
        }

        if let Err(e) = self.event_tx.send(event) {
            tracing::error!("Failed to emit orchestrator event: {}", e);
        }
    }

    fn blackboard_patches_for_event(
        &self,
        record: &RunEventRecord,
        event: &OrchestratorEvent,
    ) -> Vec<BlackboardPatchRecord> {
        let mut patches: Vec<BlackboardPatchRecord> = Vec::new();
        let mut patch_seq = record.seq.saturating_mul(100);
        let mut next_patch_seq = || {
            patch_seq = patch_seq.saturating_add(1);
            patch_seq
        };
        let mk_item = |text: String, step_id: Option<String>| BlackboardItem {
            id: format!("bbi-{}", Uuid::new_v4()),
            ts_ms: record.ts_ms,
            text,
            step_id,
            source_event_id: Some(record.event_id.clone()),
        };

        match event {
            OrchestratorEvent::ContextPackBuilt {
                why_next_step,
                task_id,
                ..
            } => {
                let prior_summary = self
                    .store
                    .load_blackboard(&record.run_id)
                    .map(|bb| bb.summaries.rolling)
                    .unwrap_or_default();
                let recent_events = self
                    .store
                    .load_run_events(&record.run_id, None, Some(20))
                    .unwrap_or_default();
                let compact_summary = Self::compact_rolling_summary(
                    &prior_summary,
                    why_next_step,
                    task_id,
                    &recent_events,
                    &Self::pinned_constraints(),
                );
                patches.push(BlackboardPatchRecord {
                    patch_id: format!("bbp-{}", Uuid::new_v4()),
                    run_id: record.run_id.clone(),
                    seq: next_patch_seq(),
                    ts_ms: record.ts_ms,
                    op: BlackboardPatchOp::AddDecision,
                    payload: serde_json::to_value(mk_item(
                        why_next_step.clone(),
                        Some(task_id.clone()),
                    ))
                    .unwrap_or(json!({})),
                });
                patches.push(BlackboardPatchRecord {
                    patch_id: format!("bbp-{}", Uuid::new_v4()),
                    run_id: record.run_id.clone(),
                    seq: next_patch_seq(),
                    ts_ms: record.ts_ms,
                    op: BlackboardPatchOp::SetLatestContextPack,
                    payload: json!(why_next_step),
                });
                patches.push(BlackboardPatchRecord {
                    patch_id: format!("bbp-{}", Uuid::new_v4()),
                    run_id: record.run_id.clone(),
                    seq: next_patch_seq(),
                    ts_ms: record.ts_ms,
                    op: BlackboardPatchOp::SetRollingSummary,
                    payload: json!(compact_summary),
                });
            }
            OrchestratorEvent::TaskCompleted {
                task_id, passed, ..
            } => {
                let op = if *passed {
                    BlackboardPatchOp::AddFact
                } else {
                    BlackboardPatchOp::AddOpenQuestion
                };
                let text = if *passed {
                    format!("Task `{}` completed and passed validation.", task_id)
                } else {
                    format!("Task `{}` completed but did not pass validation.", task_id)
                };
                patches.push(BlackboardPatchRecord {
                    patch_id: format!("bbp-{}", Uuid::new_v4()),
                    run_id: record.run_id.clone(),
                    seq: next_patch_seq(),
                    ts_ms: record.ts_ms,
                    op,
                    payload: serde_json::to_value(mk_item(text, Some(task_id.clone())))
                        .unwrap_or(json!({})),
                });

                if let Ok(run) = self.run.try_read() {
                    if let Some(task) = run.tasks.iter().find(|task| task.id == *task_id) {
                        for artifact in &task.artifacts {
                            patches.push(BlackboardPatchRecord {
                                patch_id: format!("bbp-{}", Uuid::new_v4()),
                                run_id: record.run_id.clone(),
                                seq: next_patch_seq(),
                                ts_ms: record.ts_ms,
                                op: BlackboardPatchOp::AddArtifact,
                                payload: serde_json::to_value(BlackboardArtifactRef {
                                    id: format!("bba-{}", Uuid::new_v4()),
                                    ts_ms: record.ts_ms,
                                    path: artifact.path.clone(),
                                    artifact_type: artifact.artifact_type.clone(),
                                    step_id: Some(task_id.clone()),
                                    source_event_id: Some(record.event_id.clone()),
                                })
                                .unwrap_or(json!({})),
                            });
                        }
                    }
                }
            }
            OrchestratorEvent::RevisionRequested { feedback, .. } => {
                patches.push(BlackboardPatchRecord {
                    patch_id: format!("bbp-{}", Uuid::new_v4()),
                    run_id: record.run_id.clone(),
                    seq: next_patch_seq(),
                    ts_ms: record.ts_ms,
                    op: BlackboardPatchOp::AddOpenQuestion,
                    payload: serde_json::to_value(mk_item(
                        format!("Plan revision requested: {}", feedback),
                        None,
                    ))
                    .unwrap_or(json!({})),
                });
            }
            OrchestratorEvent::WorkspaceMismatch {
                expected_path,
                actual_path,
                phase,
                task_id,
                ..
            } => {
                patches.push(BlackboardPatchRecord {
                    patch_id: format!("bbp-{}", Uuid::new_v4()),
                    run_id: record.run_id.clone(),
                    seq: next_patch_seq(),
                    ts_ms: record.ts_ms,
                    op: BlackboardPatchOp::AddOpenQuestion,
                    payload: serde_json::to_value(mk_item(
                        format!(
                            "Workspace lease mismatch in {}: expected `{}`, got `{}`.",
                            phase, expected_path, actual_path
                        ),
                        task_id.clone(),
                    ))
                    .unwrap_or(json!({})),
                });
            }
            OrchestratorEvent::RunFailed { reason, .. } => {
                patches.push(BlackboardPatchRecord {
                    patch_id: format!("bbp-{}", Uuid::new_v4()),
                    run_id: record.run_id.clone(),
                    seq: next_patch_seq(),
                    ts_ms: record.ts_ms,
                    op: BlackboardPatchOp::AddOpenQuestion,
                    payload: serde_json::to_value(mk_item(format!("Run failed: {}", reason), None))
                        .unwrap_or(json!({})),
                });
                patches.push(BlackboardPatchRecord {
                    patch_id: format!("bbp-{}", Uuid::new_v4()),
                    run_id: record.run_id.clone(),
                    seq: next_patch_seq(),
                    ts_ms: record.ts_ms,
                    op: BlackboardPatchOp::SetRollingSummary,
                    payload: json!(format!("Run failed: {}", reason)),
                });
            }
            OrchestratorEvent::RunCompleted { .. } => {
                patches.push(BlackboardPatchRecord {
                    patch_id: format!("bbp-{}", Uuid::new_v4()),
                    run_id: record.run_id.clone(),
                    seq: next_patch_seq(),
                    ts_ms: record.ts_ms,
                    op: BlackboardPatchOp::SetRollingSummary,
                    payload: json!("Run completed successfully."),
                });
            }
            OrchestratorEvent::PlanGenerated { task_count, .. } => {
                patches.push(BlackboardPatchRecord {
                    patch_id: format!("bbp-{}", Uuid::new_v4()),
                    run_id: record.run_id.clone(),
                    seq: next_patch_seq(),
                    ts_ms: record.ts_ms,
                    op: BlackboardPatchOp::SetRollingSummary,
                    payload: json!(format!("Plan generated with {} tasks.", task_count)),
                });
            }
            _ => {}
        }

        patches
    }

    fn event_type_name(event: &OrchestratorEvent) -> &'static str {
        match event {
            OrchestratorEvent::RunCreated { .. } => "run_created",
            OrchestratorEvent::PlanningStarted { .. } => "planning_started",
            OrchestratorEvent::PlanGenerated { .. } => "plan_generated",
            OrchestratorEvent::PlanApproved { .. } => "plan_approved",
            OrchestratorEvent::RevisionRequested { .. } => "revision_requested",
            OrchestratorEvent::TaskStarted { .. } => "task_started",
            OrchestratorEvent::TaskCompleted { .. } => "task_completed",
            OrchestratorEvent::TaskTrace { .. } => "task_trace",
            OrchestratorEvent::ApprovalRequested { .. } => "approval_requested",
            OrchestratorEvent::ApprovalGranted { .. } => "approval_granted",
            OrchestratorEvent::BudgetUpdated { .. } => "budget_updated",
            OrchestratorEvent::RunPaused { .. } => "run_paused",
            OrchestratorEvent::RunResumed { .. } => "run_resumed",
            OrchestratorEvent::RunCompleted { .. } => "run_completed",
            OrchestratorEvent::RunFailed { .. } => "run_failed",
            OrchestratorEvent::RunCancelled { .. } => "run_cancelled",
            OrchestratorEvent::ContractWarning { .. } => "contract_warning",
            OrchestratorEvent::ContractError { .. } => "contract_error",
            OrchestratorEvent::WorkspaceMismatch { .. } => "workspace_mismatch",
            OrchestratorEvent::ContextPackBuilt { .. } => "context_pack_built",
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

    pub fn workspace_path_string(&self) -> String {
        self.workspace_path.to_string_lossy().to_string()
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
            run.status = RunStatus::Running;
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
        if matches!(run.source, RunSource::CommandCenter)
            && run.config.max_wall_time_secs <= 60 * 60
        {
            run.config.max_wall_time_secs = 48 * 60 * 60;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_rolling_summary_preserves_pinned_constraints() {
        let pinned = OrchestratorEngine::pinned_constraints();
        let events = vec![RunEventRecord {
            event_id: "evt-1".to_string(),
            run_id: "run-1".to_string(),
            seq: 1,
            ts_ms: 1_000,
            event_type: "task_started".to_string(),
            status: RunStatus::Running,
            step_id: Some("task-1".to_string()),
            payload: serde_json::json!({}),
        }];
        let summary = OrchestratorEngine::compact_rolling_summary(
            &"x".repeat(4_000),
            "Dependencies satisfied",
            "task-1",
            &events,
            &pinned,
        );
        assert!(summary.starts_with("Pinned: "));
        assert!(summary.contains("Engine state is source-of-truth"));
        assert!(summary.contains("Only operate within leased workspace"));
        assert!(summary.len() <= OrchestratorEngine::ROLLING_SUMMARY_MAX_CHARS);
    }
}
