/// AI Evaluation Framework
///
/// This module provides structured evaluation infrastructure for testing AI system quality,
/// regression detection, and compliance auditing.
///
/// The evaluation framework consists of:
/// - **dataset.rs**: Test case definitions in YAML/JSON format
/// - **runner.rs**: CLI tool for bulk evaluation execution (Phase 3)
/// - **metrics.rs**: Metric computation and aggregation (Phase 3)
/// - **regression_detection.rs**: Baseline comparison and alerting (Phase 4)

pub mod dataset;

pub use dataset::{EvalDataset, EvalExpectedOutput, EvalTestCase};
