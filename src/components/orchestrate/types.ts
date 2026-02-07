// Orchestrator TypeScript Types
// Mirrors the Rust types in src-tauri/src/orchestrator/types.rs

export interface OrchestratorConfig {
  max_iterations: number;
  max_total_tokens: number;
  max_tokens_per_step: number;
  max_wall_time_secs: number;
  max_subagent_runs: number;
  max_web_sources: number;
  max_task_retries: number;
  require_write_approval: boolean;
  enable_research: boolean;
  allow_dangerous_actions: boolean;
  max_parallel_tasks?: number;
  llm_parallel?: number;
  fs_write_parallel?: number;
  shell_parallel?: number;
  network_parallel?: number;
}

export const DEFAULT_ORCHESTRATOR_CONFIG: OrchestratorConfig = {
  max_iterations: 500,
  max_total_tokens: 400_000,
  max_tokens_per_step: 60_000,
  max_wall_time_secs: 60 * 60,
  max_subagent_runs: 2000,
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
};

export type RunStatus =
  | "idle"
  | "planning"
  | "awaiting_approval"
  | "revision_requested"
  | "executing"
  | "paused"
  | "completed"
  | "failed"
  | "cancelled";

export type TaskState = "pending" | "in_progress" | "blocked" | "done" | "failed";

export interface Task {
  id: string;
  title: string;
  description: string;
  dependencies: string[];
  acceptance_criteria: string[];
  state: TaskState;
  retry_count: number;
  error_message?: string;
  session_id?: string;
  runtime_status?: string;
  runtime_detail?: string;
}

export interface Budget {
  max_iterations: number;
  iterations_used: number;
  max_tokens: number;
  tokens_used: number;
  max_wall_time_secs: number;
  wall_time_secs: number;
  max_subagent_runs: number;
  subagent_runs_used: number;
  exceeded: boolean;
  exceeded_reason?: string;
}

export interface RunSnapshot {
  run_id: string;
  status: RunStatus;
  objective: string;
  task_count: number;
  tasks_completed: number;
  tasks_failed: number;
  budget: Budget;
  current_task_id?: string;
  error_message?: string;
  created_at: string;
  updated_at: string;
}

export interface RunSummary {
  run_id: string;
  session_id: string;
  objective: string;
  status: RunStatus;
  created_at: string;
  updated_at: string;
}

export interface Run {
  run_id: string;
  session_id: string;
  objective: string;
  config: OrchestratorConfig;
  status: RunStatus;
  tasks: Task[];
  budget: Budget;
  started_at: string;
  ended_at?: string;
  error_message?: string;
  revision_feedback?: string;
}

export interface OrchestratorEvent {
  type: string;
  run_id: string;
  timestamp: string;
  [key: string]: unknown;
}
