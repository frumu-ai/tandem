use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBenchmarkSuite {
    pub suite_id: String,
    pub scenarios: Vec<WorkflowBenchmarkScenario>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBenchmarkScenario {
    pub scenario_id: String,
    pub baseline: WorkflowBenchmarkObservation,
    pub graph_guided: WorkflowBenchmarkObservation,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBenchmarkObservation {
    pub completed_runs: u64,
    pub latency_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub tool_calls: u64,
    pub wrong_tool_calls: u64,
    pub policy_checks: u64,
    pub policy_failures: u64,
    pub preflight_checks: u64,
    pub preflight_failures: u64,
    pub rerun_steps_considered: u64,
    pub rerun_steps_reused: u64,
    pub sequential_latency_ms: u64,
    pub scheduled_latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBenchmarkReport {
    pub suite_id: String,
    pub scenario_count: usize,
    pub totals: WorkflowBenchmarkComparison,
    pub scenarios: Vec<WorkflowBenchmarkScenarioReport>,
    pub regressions: Vec<WorkflowBenchmarkRegression>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBenchmarkScenarioReport {
    pub scenario_id: String,
    pub comparison: WorkflowBenchmarkComparison,
    pub regressions: Vec<WorkflowBenchmarkRegression>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBenchmarkComparison {
    pub baseline: WorkflowBenchmarkObservation,
    pub graph_guided: WorkflowBenchmarkObservation,
    pub token_savings: i64,
    pub token_savings_rate_bps: i64,
    pub runtime_latency_savings_ms: i64,
    pub runtime_latency_savings_rate_bps: i64,
    pub baseline_wrong_tool_call_rate_bps: u32,
    pub graph_guided_wrong_tool_call_rate_bps: u32,
    pub wrong_tool_call_rate_delta_bps: i64,
    pub baseline_policy_failure_rate_bps: u32,
    pub graph_guided_policy_failure_rate_bps: u32,
    pub policy_failure_rate_delta_bps: i64,
    pub baseline_preflight_success_rate_bps: u32,
    pub graph_guided_preflight_success_rate_bps: u32,
    pub preflight_success_rate_delta_bps: i64,
    pub baseline_rerun_reuse_rate_bps: u32,
    pub graph_guided_rerun_reuse_rate_bps: u32,
    pub rerun_reuse_rate_delta_bps: i64,
    pub graph_guided_parallel_latency_savings_ms: i64,
    pub graph_guided_parallel_latency_savings_rate_bps: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBenchmarkRegression {
    pub scenario_id: Option<String>,
    pub metric: String,
    pub baseline_value: i64,
    pub graph_guided_value: i64,
    pub threshold_bps: u32,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowBenchmarkThresholds {
    pub max_token_regression_bps: u32,
    pub max_latency_regression_bps: u32,
    pub max_wrong_tool_rate_regression_bps: u32,
    pub max_policy_failure_rate_regression_bps: u32,
    pub max_preflight_success_drop_bps: u32,
    pub max_rerun_reuse_drop_bps: u32,
}

impl WorkflowBenchmarkObservation {
    pub fn total_tokens(&self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }
}
