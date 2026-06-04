/// AI Evaluation Framework
///
/// This module provides structured evaluation infrastructure for testing AI system quality,
/// regression detection, and compliance auditing.
///
/// The evaluation framework consists of:
/// - **dataset.rs**: Test case definitions in YAML/JSON format
/// - **metrics.rs**: Metric computation and aggregation
/// - **runner.rs**: Eval execution engine (CLI binary in bin/eval_runner.rs)
/// - **regression_detection.rs**: Baseline comparison and alerting (Phase 4)
pub mod bootstrap;
pub mod dataset;
pub mod engine_executor;
pub mod metrics;
pub mod regression_detection;
pub mod runner;
pub mod scripted_provider;
pub mod spec_mapper;

pub use bootstrap::{bootstrap_eval_app_state, EvalBootstrapOptions};
pub use dataset::{ArtifactStatus, EvalDataset, EvalExpectedOutput, EvalTestCase, MetricTolerance};
pub use engine_executor::{
    extract_eval_result, EngineExecutor, DEFAULT_MAX_DURATION_SECS, DEFAULT_POLL_INTERVAL_MS,
};
pub use metrics::{EvalMetrics, EvalRunResult};
pub use regression_detection::{
    detect_regressions, EvalBaseline, RegressionReport, RegressionStatus, RegressionThresholds,
};
pub use runner::{EngineMode, EvalRunner, EvalRunnerConfig};
pub use scripted_provider::{
    ScriptedEvalProvider, ScriptedResponse, SCRIPTED_MODEL_ID, SCRIPTED_PROVIDER_ID,
};
pub use spec_mapper::{
    contract_kind_for_node_type, test_case_to_spec, test_case_to_stub_spec,
    validator_for_node_type, EVAL_AGENT_ID, EVAL_TRIGGER_TYPE,
};
