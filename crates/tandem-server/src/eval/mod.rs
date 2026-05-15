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

pub mod dataset;
pub mod metrics;
pub mod runner;
pub mod regression_detection;

pub use dataset::{ArtifactStatus, EvalDataset, EvalExpectedOutput, EvalTestCase, MetricTolerance};
pub use metrics::{EvalMetrics, EvalRunResult};
pub use runner::{EvalRunner, EvalRunnerConfig};
pub use regression_detection::{
    detect_regressions, EvalBaseline, RegressionReport, RegressionStatus, RegressionThresholds,
};
