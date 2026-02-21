// Orchestrator Types
// Core type definitions for multi-agent orchestration
// See: docs/orchestration_plan.md

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ============================================================================
// Configuration
// ============================================================================

/// Configuration for an orchestration run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    /// Maximum total planning + execution iterations
    pub max_iterations: u32,
    /// Maximum total tokens (estimated if metering unavailable)
    pub max_total_tokens: u64,
    /// Maximum tokens per sub-agent call
    pub max_tokens_per_step: u64,
    /// Maximum wall time in seconds
    pub max_wall_time_secs: u64,
    /// Maximum sub-agent invocations
    pub max_subagent_runs: u32,
    /// Maximum web sources when research enabled
    pub max_web_sources: u32,
    /// Maximum retries per task before fail-block
    pub max_task_retries: u32,
    /// Require approval before writing files
    pub require_write_approval: bool,
    /// Enable research/web agent
    pub enable_research: bool,
    /// Enable dangerous actions (shell, install, etc.)
    pub allow_dangerous_actions: bool,
    #[serde(default)]
    pub max_parallel_tasks: u32,
    #[serde(default)]
    pub llm_parallel: u32,
    #[serde(default)]
    pub fs_write_parallel: u32,
    #[serde(default)]
    pub shell_parallel: u32,
    #[serde(default)]
    pub network_parallel: u32,
    #[serde(default = "default_strict_planner_json")]
    pub strict_planner_json: bool,
    #[serde(default = "default_strict_validator_json")]
    pub strict_validator_json: bool,
    #[serde(default = "default_allow_prose_fallback")]
    pub allow_prose_fallback: bool,
    #[serde(default = "default_contract_warnings_enabled")]
    pub contract_warnings_enabled: bool,
}

fn strict_contract_flag_default() -> bool {
    match std::env::var("TANDEM_ORCH_STRICT_CONTRACT") {
        Ok(v) => matches!(
            v.trim().to_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        ),
        Err(_) => cfg!(debug_assertions),
    }
}

fn default_strict_planner_json() -> bool {
    strict_contract_flag_default()
}

fn default_strict_validator_json() -> bool {
    strict_contract_flag_default()
}

fn default_allow_prose_fallback() -> bool {
    true
}

fn default_contract_warnings_enabled() -> bool {
    true
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            // Safety cap on overall run "steps". This is intentionally generous because
            // we also cap tokens + subagent calls, and users shouldn't see runs fail
            // simply due to long multi-step workflows.
            max_iterations: 500, // Increased from 200
            max_total_tokens: 400_000,
            max_tokens_per_step: 60_000,
            max_wall_time_secs: 60 * 60, // 60 minutes
            // Each task attempt currently includes multiple model calls (e.g. build + validate).
            // Keep this cap generous to avoid premature pauses while still bounding cost.
            max_subagent_runs: 2000, // Increased from 500
            max_web_sources: 30,
            max_task_retries: 3,
            require_write_approval: true,
            enable_research: false,
            allow_dangerous_actions: false,
            max_parallel_tasks: 4,
            llm_parallel: 3,
            fs_write_parallel: 1,
            shell_parallel: 1,
            network_parallel: 2,
            strict_planner_json: default_strict_planner_json(),
            strict_validator_json: default_strict_validator_json(),
            allow_prose_fallback: default_allow_prose_fallback(),
            contract_warnings_enabled: default_contract_warnings_enabled(),
        }
    }
}

// ============================================================================
// Run State
// ============================================================================

/// Status of an orchestration run
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Run created, awaiting planning
    Idle,
    /// Planner agent is generating task DAG
    Planning,
    /// Plan generated, awaiting user approval
    AwaitingApproval,
    /// User requested revision to the plan
    RevisionRequested,
    /// Executing tasks
    Executing,
    /// Execution paused by user
    Paused,
    /// All tasks completed successfully
    Completed,
    /// Run failed (budget exceeded or unrecoverable error)
    Failed,
    /// Run cancelled by user
    Cancelled,
}

/// Source that initiated an orchestration run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RunSource {
    #[default]
    Orchestrator,
    CommandCenter,
}

/// Represents a complete orchestration run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    /// Unique run identifier
    pub run_id: String,
    /// Session ID for sidecar communication
    pub session_id: String,
    /// UI origin for this run (orchestrator panel vs command center).
    #[serde(default)]
    pub source: RunSource,
    /// Provider for this run's model (sidecar provider ID, e.g. "openrouter", "opencode")
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// Model for this run (provider-specific model ID)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// Optional role-based model/provider overrides.
    #[serde(default)]
    pub agent_model_routing: AgentModelRouting,
    /// User's objective
    pub objective: String,
    /// Run configuration
    pub config: OrchestratorConfig,
    /// Current run status
    pub status: RunStatus,
    /// Task list (DAG)
    pub tasks: Vec<Task>,
    /// Current budget state
    pub budget: Budget,
    /// Run start time
    pub started_at: chrono::DateTime<chrono::Utc>,
    /// Run end time (if completed/failed/cancelled)
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Error message if failed
    pub error_message: Option<String>,
    /// Plan revision feedback (if revision requested)
    pub revision_feedback: Option<String>,
}

impl Run {
    pub fn new(
        run_id: String,
        session_id: String,
        objective: String,
        config: OrchestratorConfig,
    ) -> Self {
        Self {
            run_id,
            session_id,
            source: RunSource::Orchestrator,
            provider: None,
            model: None,
            agent_model_routing: AgentModelRouting::default(),
            objective,
            config: config.clone(),
            status: RunStatus::Idle,
            tasks: Vec::new(),
            budget: Budget::from_config(&config),
            started_at: chrono::Utc::now(),
            ended_at: None,
            error_message: None,
            revision_feedback: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelSelection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentModelRouting {
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub roles: HashMap<String, ModelSelection>,
    // Legacy compatibility fields: read old persisted payloads and map into `roles`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner: Option<ModelSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub builder: Option<ModelSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub validator: Option<ModelSelection>,
}

pub const ROLE_ORCHESTRATOR: &str = "orchestrator";
pub const ROLE_DELEGATOR: &str = "delegator";
pub const ROLE_WORKER: &str = "worker";
pub const ROLE_WATCHER: &str = "watcher";
pub const ROLE_REVIEWER: &str = "reviewer";
pub const ROLE_TESTER: &str = "tester";

pub fn normalize_role_key(raw: &str) -> String {
    let key = raw.trim().to_lowercase();
    match key.as_str() {
        "planner" => ROLE_ORCHESTRATOR.to_string(),
        "builder" => ROLE_WORKER.to_string(),
        "validator" => ROLE_REVIEWER.to_string(),
        "researcher" => ROLE_WATCHER.to_string(),
        _ => key,
    }
}

impl AgentModelRouting {
    pub fn canonicalized(&self) -> Self {
        let mut roles = HashMap::<String, ModelSelection>::new();
        for (key, selection) in &self.roles {
            let normalized = normalize_role_key(key);
            roles.entry(normalized).or_insert_with(|| selection.clone());
        }

        if let Some(sel) = self.planner.clone() {
            roles.entry(ROLE_ORCHESTRATOR.to_string()).or_insert(sel);
        }
        if let Some(sel) = self.builder.clone() {
            roles.entry(ROLE_WORKER.to_string()).or_insert(sel);
        }
        if let Some(sel) = self.validator.clone() {
            roles.entry(ROLE_REVIEWER.to_string()).or_insert(sel);
        }

        Self {
            roles,
            planner: None,
            builder: None,
            validator: None,
        }
    }

    pub fn get_for_role(&self, role: &str) -> Option<&ModelSelection> {
        let normalized = normalize_role_key(role);
        self.roles.get(&normalized)
    }
}

/// Snapshot of run state for UI consumption
#[derive(Debug, Clone, Serialize)]
pub struct RunSnapshot {
    pub run_id: String,
    pub status: RunStatus,
    pub objective: String,
    pub task_count: usize,
    pub tasks_completed: usize,
    pub tasks_failed: usize,
    pub budget: Budget,
    pub current_task_id: Option<String>,
    pub error_message: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

/// Lightweight summary of a run for listing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunSummary {
    pub run_id: String,
    pub session_id: String,
    #[serde(default)]
    pub source: RunSource,
    pub objective: String,
    pub status: RunStatus,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

impl Run {
    pub fn to_snapshot(&self) -> RunSnapshot {
        let tasks_completed = self
            .tasks
            .iter()
            .filter(|t| t.state == TaskState::Done)
            .count();
        let tasks_failed = self
            .tasks
            .iter()
            .filter(|t| t.state == TaskState::Failed)
            .count();
        let current_task_id = self
            .tasks
            .iter()
            .find(|t| t.state == TaskState::InProgress)
            .map(|t| t.id.clone());

        RunSnapshot {
            run_id: self.run_id.clone(),
            status: self.status,
            objective: self.objective.clone(),
            task_count: self.tasks.len(),
            tasks_completed,
            tasks_failed,
            budget: self.budget.clone(),
            current_task_id,
            error_message: self.error_message.clone(),
            created_at: self.started_at,
            updated_at: self.ended_at.unwrap_or_else(chrono::Utc::now),
        }
    }
}

// ============================================================================
// Task State
// ============================================================================

/// State of a task in the DAG
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    /// Waiting for dependencies
    Pending,
    /// Currently being executed
    InProgress,
    /// Blocked (dependency failed or manual block)
    Blocked,
    /// Completed successfully
    Done,
    /// Failed after max retries
    Failed,
}

/// A single task in the orchestration DAG
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    /// Unique task identifier
    pub id: String,
    /// Short task title
    pub title: String,
    /// Detailed task description
    pub description: String,
    /// IDs of tasks that must complete before this one
    pub dependencies: Vec<String>,
    /// Acceptance criteria for validation
    pub acceptance_criteria: Vec<String>,
    /// Assigned role for execution (canonical default: worker)
    #[serde(default = "default_task_role")]
    pub assigned_role: String,
    /// Optional template hint for role-specific execution
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<String>,
    /// Optional gate stage for this task
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate: Option<TaskGate>,
    /// Current task state
    pub state: TaskState,
    /// Number of retry attempts
    pub retry_count: u32,
    /// Artifact outputs from this task
    pub artifacts: Vec<Artifact>,
    /// Validation result (if validated)
    pub validation_result: Option<ValidationResult>,
    /// Error message if failed
    pub error_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
}

impl Task {
    pub fn new(id: String, title: String, description: String) -> Self {
        Self {
            id,
            title,
            description,
            dependencies: Vec::new(),
            acceptance_criteria: Vec::new(),
            assigned_role: default_task_role(),
            template_id: None,
            gate: None,
            state: TaskState::Pending,
            retry_count: 0,
            artifacts: Vec::new(),
            validation_result: None,
            error_message: None,
            session_id: None,
        }
    }
}

fn default_task_role() -> String {
    ROLE_WORKER.to_string()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskGate {
    Review,
    Test,
}

// ============================================================================
// Artifacts
// ============================================================================

/// Type of artifact produced by a sub-agent
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactType {
    /// Code patch/diff
    Patch,
    /// Notes or documentation
    Notes,
    /// Research sources
    Sources,
    /// Research fact cards
    FactCards,
    /// Generic file
    File,
}

/// An artifact produced by a sub-agent
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Artifact {
    /// Artifact type
    pub artifact_type: ArtifactType,
    /// Relative path within run folder
    pub path: String,
    /// Optional content preview
    pub preview: Option<String>,
}

/// Result of validation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationResult {
    /// Whether validation passed
    pub passed: bool,
    /// Feedback from validator
    pub feedback: String,
    /// Suggested fixes if failed
    pub suggested_fixes: Vec<String>,
}

// ============================================================================
// Budget
// ============================================================================

/// Budget tracking state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Budget {
    /// Maximum iterations allowed
    pub max_iterations: u32,
    /// Iterations used
    pub iterations_used: u32,
    /// Maximum tokens allowed
    pub max_tokens: u64,
    /// Tokens used (estimated)
    pub tokens_used: u64,
    /// Maximum wall time in seconds
    pub max_wall_time_secs: u64,
    /// Elapsed wall time in seconds
    pub wall_time_secs: u64,
    /// Maximum sub-agent runs
    pub max_subagent_runs: u32,
    /// Sub-agent runs used
    pub subagent_runs_used: u32,
    /// Budget exceeded flag
    pub exceeded: bool,
    /// Which limit was exceeded
    pub exceeded_reason: Option<String>,
}

impl Budget {
    pub fn from_config(config: &OrchestratorConfig) -> Self {
        Self {
            max_iterations: config.max_iterations,
            iterations_used: 0,
            max_tokens: config.max_total_tokens,
            tokens_used: 0,
            max_wall_time_secs: config.max_wall_time_secs,
            wall_time_secs: 0,
            max_subagent_runs: config.max_subagent_runs,
            subagent_runs_used: 0,
            exceeded: false,
            exceeded_reason: None,
        }
    }

    /// Check if any budget limit is exceeded
    pub fn is_exceeded(&self) -> bool {
        self.exceeded
            || self.iterations_used >= self.max_iterations
            || self.tokens_used >= self.max_tokens
            || self.wall_time_secs >= self.max_wall_time_secs
            || self.subagent_runs_used >= self.max_subagent_runs
    }

    /// Get percentage of budget used (0.0 to 1.0) for the most-used dimension
    pub fn usage_percentage(&self) -> f64 {
        let iter_pct = self.iterations_used as f64 / self.max_iterations.max(1) as f64;
        let token_pct = self.tokens_used as f64 / self.max_tokens.max(1) as f64;
        let time_pct = self.wall_time_secs as f64 / self.max_wall_time_secs.max(1) as f64;
        let agent_pct = self.subagent_runs_used as f64 / self.max_subagent_runs.max(1) as f64;

        iter_pct.max(token_pct).max(time_pct).max(agent_pct)
    }
}

// ============================================================================
// Events
// ============================================================================

/// Event types for append-only log
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OrchestratorEvent {
    RunCreated {
        run_id: String,
        objective: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    PlanningStarted {
        run_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    PlanGenerated {
        run_id: String,
        task_count: usize,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    PlanApproved {
        run_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    RevisionRequested {
        run_id: String,
        feedback: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TaskStarted {
        run_id: String,
        task_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TaskCompleted {
        run_id: String,
        task_id: String,
        passed: bool,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    TaskTrace {
        run_id: String,
        task_id: String,
        session_id: Option<String>,
        stage: String,
        detail: Option<String>,
        thread: Option<String>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    ApprovalRequested {
        run_id: String,
        action: String,
        reason: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    ApprovalGranted {
        run_id: String,
        action: String,
        scope: ApprovalScope,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    BudgetUpdated {
        run_id: String,
        budget: Budget,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    RunPaused {
        run_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    RunResumed {
        run_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    RunCompleted {
        run_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    RunFailed {
        run_id: String,
        reason: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    RunCancelled {
        run_id: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    ContractWarning {
        run_id: String,
        task_id: Option<String>,
        agent: String,
        phase: String,
        reason: String,
        fallback_used: bool,
        snippet: Option<String>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    ContractError {
        run_id: String,
        task_id: Option<String>,
        agent: String,
        phase: String,
        reason: String,
        fallback_used: bool,
        snippet: Option<String>,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
}

// ============================================================================
// Approvals
// ============================================================================

/// Scope of an approval
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalScope {
    /// Approve this single action
    Once,
    /// Approve similar actions for this run
    ForRun,
}

/// Pending approval request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApprovalRequest {
    /// Unique token for this request
    pub token: String,
    /// Run ID
    pub run_id: String,
    /// Task ID (if applicable)
    pub task_id: Option<String>,
    /// Action being requested (e.g., "write_file:/src/lib.rs")
    pub action: String,
    /// Tool name
    pub tool: String,
    /// Reason from agent
    pub reason: String,
    /// Preview of the action (diff, command, etc.)
    pub preview: Option<String>,
    /// Request timestamp
    pub requested_at: chrono::DateTime<chrono::Utc>,
}

// ============================================================================
// Sub-Agent Types
// ============================================================================

/// Sub-agent role
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentRole {
    Orchestrator,
    Delegator,
    Worker,
    Watcher,
    Reviewer,
    Tester,
    Planner,
    Builder,
    Validator,
    Researcher,
}

impl AgentRole {
    pub fn role_key(&self) -> &'static str {
        match self {
            AgentRole::Orchestrator | AgentRole::Planner => ROLE_ORCHESTRATOR,
            AgentRole::Delegator => ROLE_DELEGATOR,
            AgentRole::Worker | AgentRole::Builder => ROLE_WORKER,
            AgentRole::Watcher | AgentRole::Researcher => ROLE_WATCHER,
            AgentRole::Reviewer | AgentRole::Validator => ROLE_REVIEWER,
            AgentRole::Tester => ROLE_TESTER,
        }
    }
}

/// Result from a sub-agent call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResult {
    /// Agent role
    pub role: AgentRole,
    /// Whether the call succeeded
    pub success: bool,
    /// Output content
    pub output: String,
    /// Artifacts produced
    pub artifacts: Vec<Artifact>,
    /// Estimated tokens used
    pub tokens_used: u64,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Error message if failed
    pub error: Option<String>,
}
