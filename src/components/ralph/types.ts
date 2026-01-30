// Ralph Loop TypeScript Types
// Mirrors the Rust types in src-tauri/src/ralph/types.rs

export interface RalphConfig {
  min_iterations: number;
  max_iterations: number;
  completion_promise: string;
  allow_all_permissions: boolean;
  plan_mode_guard: boolean;
}

export type RalphRunStatus = "idle" | "running" | "paused" | "completed" | "cancelled" | "error";

export interface RalphState {
  run_id: string;
  session_id: string;
  active: boolean;
  status: RalphRunStatus;
  iteration: number;
  started_at: string;
  ended_at?: string;
  prompt: string;
  config: RalphConfig;
  last_iteration_duration_ms?: number;
  struggle_detected: boolean;
  error_message?: string;
}

export interface IterationRecord {
  iteration: number;
  started_at: string;
  ended_at: string;
  duration_ms: number;
  completion_detected: boolean;
  tools_used: Record<string, number>;
  files_modified: string[];
  errors: string[];
  context_injected?: string;
}

export interface RalphStateSnapshot {
  run_id: string;
  status: RalphRunStatus;
  iteration: number;
  total_iterations: number;
  last_duration_ms?: number;
  files_modified_count: number;
  struggle_detected: boolean;
}
