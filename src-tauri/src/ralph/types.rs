// Ralph Loop Types
// Inspired by: https://raw.githubusercontent.com/Th0rgal/open-ralph-wiggum/refs/heads/master/ralph.ts

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Configuration for a Ralph Loop run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RalphConfig {
    pub min_iterations: u32,
    pub max_iterations: u32,
    pub completion_promise: String,
    pub allow_all_permissions: bool,
    pub plan_mode_guard: bool,
}

impl Default for RalphConfig {
    fn default() -> Self {
        Self {
            min_iterations: 1,
            max_iterations: 50,
            completion_promise: "COMPLETE".to_string(),
            allow_all_permissions: false,
            plan_mode_guard: true,
        }
    }
}

/// Current status of a Ralph Loop
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RalphRunStatus {
    Idle,
    Running,
    Paused,
    Completed,
    Cancelled,
    Error,
}

/// Current state of a Ralph Loop run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RalphState {
    pub run_id: String,
    pub session_id: String,
    pub active: bool,
    pub status: RalphRunStatus,
    pub iteration: u32,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: Option<chrono::DateTime<chrono::Utc>>,
    pub prompt: String,
    pub config: RalphConfig,
    pub last_iteration_duration_ms: Option<u64>,
    pub struggle_detected: bool,
    pub error_message: Option<String>,
}

/// Record of a single iteration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IterationRecord {
    pub iteration: u32,
    pub started_at: chrono::DateTime<chrono::Utc>,
    pub ended_at: chrono::DateTime<chrono::Utc>,
    pub duration_ms: u64,
    pub completion_detected: bool,
    pub tools_used: HashMap<String, u32>,
    pub files_modified: Vec<String>,
    pub errors: Vec<String>,
    pub context_injected: Option<String>,
}

/// Snapshot for UI consumption
#[derive(Debug, Clone, Serialize)]
pub struct RalphStateSnapshot {
    pub run_id: String,
    pub status: RalphRunStatus,
    pub iteration: u32,
    pub total_iterations: usize,
    pub last_duration_ms: Option<u64>,
    pub files_modified_count: usize,
    pub struggle_detected: bool,
}

impl RalphState {
    pub fn new(run_id: String, session_id: String, prompt: String, config: RalphConfig) -> Self {
        Self {
            run_id,
            session_id,
            active: true,
            status: RalphRunStatus::Running,
            iteration: 1,
            started_at: chrono::Utc::now(),
            ended_at: None,
            prompt,
            config,
            last_iteration_duration_ms: None,
            struggle_detected: false,
            error_message: None,
        }
    }

    pub fn to_snapshot(
        &self,
        total_iterations: usize,
        files_modified_count: usize,
    ) -> RalphStateSnapshot {
        RalphStateSnapshot {
            run_id: self.run_id.clone(),
            status: self.status,
            iteration: self.iteration,
            total_iterations,
            last_duration_ms: self.last_iteration_duration_ms,
            files_modified_count,
            struggle_detected: self.struggle_detected,
        }
    }
}
