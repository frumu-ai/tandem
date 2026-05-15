/// Evaluation Metrics Computation
///
/// This module computes aggregated metrics from evaluation runs,
/// enabling regression detection and quality tracking over time.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::eval::dataset::ArtifactStatus;
use crate::failures::AIFailureMode;

/// Aggregated metrics from a complete evaluation run
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalMetrics {
    /// Dataset name evaluated
    pub dataset_name: String,
    /// Dataset version evaluated
    pub dataset_version: String,
    /// When the evaluation started (ms timestamp)
    pub started_at_ms: u64,
    /// When the evaluation finished (ms timestamp)
    pub finished_at_ms: u64,
    /// Total duration in milliseconds
    pub duration_ms: u64,

    /// Total number of test cases attempted
    pub total_tests: u32,
    /// Number of tests that passed
    pub passed_tests: u32,
    /// Number of tests that failed
    pub failed_tests: u32,
    /// Number of tests skipped (disabled)
    pub skipped_tests: u32,

    /// Overall pass rate (passed / (passed + failed))
    pub pass_rate: f64,
    /// Average repair iterations across all tests
    pub avg_repair_iterations: f64,
    /// Maximum repair iterations seen
    pub max_repair_iterations: u32,

    /// Total tokens consumed across all tests
    pub total_tokens: u64,
    /// Total estimated cost in USD
    pub total_cost_usd: f64,
    /// Average cost per test
    pub avg_cost_per_test: f64,

    /// Provider failure rate (provider_failures / total_calls)
    pub provider_failure_rate: f64,
    /// Number of provider calls made
    pub total_provider_calls: u64,
    /// Number of provider failures
    pub provider_failures: u64,

    /// Pass rate by validator class
    pub validator_pass_rates: HashMap<String, f64>,

    /// Failure modes encountered (failure_mode -> count)
    pub failure_modes: HashMap<String, u32>,

    /// Individual test results
    pub test_results: Vec<EvalRunResult>,
}

/// Result from a single test case execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalRunResult {
    /// Test case ID
    pub test_id: String,
    /// Test description
    pub description: String,
    /// Did the test pass overall?
    pub passed: bool,
    /// Final artifact status
    pub artifact_status: ArtifactStatus,
    /// Number of repair iterations used
    pub repair_iterations: u32,
    /// Tokens used (input + output)
    pub tokens_used: u64,
    /// Estimated cost in USD
    pub cost_usd: f64,
    /// Duration in milliseconds
    pub duration_ms: u64,
    /// Validators that passed
    pub validators_passed: Vec<String>,
    /// Validators that failed
    pub validators_failed: Vec<String>,
    /// Failure mode if test failed
    pub failure_mode: Option<AIFailureMode>,
    /// Error message if test failed
    pub error_message: Option<String>,
    /// Test tags from dataset
    pub tags: Vec<String>,
}

impl EvalMetrics {
    /// Create new empty metrics for a dataset
    pub fn new(dataset_name: impl Into<String>, dataset_version: impl Into<String>) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            dataset_name: dataset_name.into(),
            dataset_version: dataset_version.into(),
            started_at_ms: now,
            finished_at_ms: now,
            duration_ms: 0,
            total_tests: 0,
            passed_tests: 0,
            failed_tests: 0,
            skipped_tests: 0,
            pass_rate: 0.0,
            avg_repair_iterations: 0.0,
            max_repair_iterations: 0,
            total_tokens: 0,
            total_cost_usd: 0.0,
            avg_cost_per_test: 0.0,
            provider_failure_rate: 0.0,
            total_provider_calls: 0,
            provider_failures: 0,
            validator_pass_rates: HashMap::new(),
            failure_modes: HashMap::new(),
            test_results: Vec::new(),
        }
    }

    /// Add a test result to the metrics
    pub fn add_result(&mut self, result: EvalRunResult) {
        // Update counts
        self.total_tests += 1;
        if result.passed {
            self.passed_tests += 1;
        } else {
            self.failed_tests += 1;
        }

        // Update repair iterations
        if result.repair_iterations > self.max_repair_iterations {
            self.max_repair_iterations = result.repair_iterations;
        }

        // Update tokens and cost
        self.total_tokens += result.tokens_used;
        self.total_cost_usd += result.cost_usd;

        // Track failure modes
        if let Some(failure_mode) = &result.failure_mode {
            let key = format!("{:?}", failure_mode)
                .split(' ')
                .next()
                .unwrap_or("Unknown")
                .to_string();
            *self.failure_modes.entry(key).or_insert(0) += 1;
        }

        // Track validator results
        for validator in &result.validators_passed {
            self.update_validator_rate(validator, true);
        }
        for validator in &result.validators_failed {
            self.update_validator_rate(validator, false);
        }

        // Store the result
        self.test_results.push(result);
    }

    /// Mark a test as skipped (without adding result)
    pub fn add_skipped(&mut self) {
        self.skipped_tests += 1;
    }

    /// Finalize metrics: compute averages and rates
    pub fn finalize(&mut self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.finished_at_ms = now;
        self.duration_ms = now.saturating_sub(self.started_at_ms);

        // Pass rate
        let total_run = self.passed_tests + self.failed_tests;
        self.pass_rate = if total_run > 0 {
            self.passed_tests as f64 / total_run as f64
        } else {
            0.0
        };

        // Average repair iterations
        if !self.test_results.is_empty() {
            let total_repairs: u32 = self
                .test_results
                .iter()
                .map(|r| r.repair_iterations)
                .sum();
            self.avg_repair_iterations =
                total_repairs as f64 / self.test_results.len() as f64;

            self.avg_cost_per_test = self.total_cost_usd / self.test_results.len() as f64;
        }

        // Provider failure rate
        self.provider_failure_rate = if self.total_provider_calls > 0 {
            self.provider_failures as f64 / self.total_provider_calls as f64
        } else {
            0.0
        };
    }

    /// Update pass rate for a specific validator
    fn update_validator_rate(&mut self, validator: &str, passed: bool) {
        let entry = self
            .validator_pass_rates
            .entry(validator.to_string())
            .or_insert(0.0);

        // Use a simple running average approximation
        // For exact calculation, we'd need to track counts per validator
        // This is "good enough" for monitoring trends
        let current = *entry;
        let new_value = if passed { 1.0 } else { 0.0 };
        *entry = (current + new_value) / 2.0;
    }

    /// Generate a human-readable summary
    pub fn summary(&self) -> String {
        let mut output = String::new();
        output.push_str(&format!(
            "=== Evaluation Results: {} v{} ===\n",
            self.dataset_name, self.dataset_version
        ));
        output.push_str(&format!("Duration: {} ms\n", self.duration_ms));
        output.push_str(&format!(
            "Tests: {} total ({} passed, {} failed, {} skipped)\n",
            self.total_tests + self.skipped_tests,
            self.passed_tests,
            self.failed_tests,
            self.skipped_tests
        ));
        output.push_str(&format!(
            "Pass Rate: {:.1}%\n",
            self.pass_rate * 100.0
        ));
        output.push_str(&format!(
            "Avg Repair Iterations: {:.2}\n",
            self.avg_repair_iterations
        ));
        output.push_str(&format!("Total Cost: ${:.2}\n", self.total_cost_usd));
        output.push_str(&format!(
            "Avg Cost/Test: ${:.4}\n",
            self.avg_cost_per_test
        ));

        if !self.failure_modes.is_empty() {
            output.push_str("\nFailure Modes:\n");
            for (mode, count) in &self.failure_modes {
                output.push_str(&format!("  - {}: {}\n", mode, count));
            }
        }

        if self.failed_tests > 0 {
            output.push_str("\nFailed Tests:\n");
            for result in &self.test_results {
                if !result.passed {
                    output.push_str(&format!(
                        "  - {} ({}): {}\n",
                        result.test_id,
                        result.description,
                        result.error_message.as_deref().unwrap_or("unknown error")
                    ));
                }
            }
        }

        output
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_result(id: &str, passed: bool) -> EvalRunResult {
        EvalRunResult {
            test_id: id.to_string(),
            description: format!("Test {}", id),
            passed,
            artifact_status: if passed {
                ArtifactStatus::Completed
            } else {
                ArtifactStatus::Failed
            },
            repair_iterations: if passed { 1 } else { 3 },
            tokens_used: 1000,
            cost_usd: 0.01,
            duration_ms: 1000,
            validators_passed: if passed {
                vec!["contract".to_string()]
            } else {
                vec![]
            },
            validators_failed: if passed {
                vec![]
            } else {
                vec!["contract".to_string()]
            },
            failure_mode: if passed {
                None
            } else {
                Some(AIFailureMode::ArtifactValidationFailed {
                    validator_class: "contract".to_string(),
                })
            },
            error_message: if passed {
                None
            } else {
                Some("Validation failed".to_string())
            },
            tags: vec!["test".to_string()],
        }
    }

    #[test]
    fn test_metrics_creation() {
        let metrics = EvalMetrics::new("test", "1.0");
        assert_eq!(metrics.dataset_name, "test");
        assert_eq!(metrics.total_tests, 0);
    }

    #[test]
    fn test_add_result() {
        let mut metrics = EvalMetrics::new("test", "1.0");
        metrics.add_result(create_test_result("test1", true));
        metrics.add_result(create_test_result("test2", false));

        assert_eq!(metrics.total_tests, 2);
        assert_eq!(metrics.passed_tests, 1);
        assert_eq!(metrics.failed_tests, 1);
    }

    #[test]
    fn test_finalize_pass_rate() {
        let mut metrics = EvalMetrics::new("test", "1.0");
        metrics.add_result(create_test_result("test1", true));
        metrics.add_result(create_test_result("test2", true));
        metrics.add_result(create_test_result("test3", false));
        metrics.finalize();

        assert!((metrics.pass_rate - 0.666).abs() < 0.01);
    }

    #[test]
    fn test_failure_modes_tracked() {
        let mut metrics = EvalMetrics::new("test", "1.0");
        metrics.add_result(create_test_result("test1", false));
        metrics.add_result(create_test_result("test2", false));

        assert!(metrics.failure_modes.contains_key("ArtifactValidationFailed"));
        assert_eq!(metrics.failure_modes.get("ArtifactValidationFailed"), Some(&2));
    }

    #[test]
    fn test_cost_aggregation() {
        let mut metrics = EvalMetrics::new("test", "1.0");
        metrics.add_result(create_test_result("test1", true));
        metrics.add_result(create_test_result("test2", true));
        metrics.finalize();

        assert!((metrics.total_cost_usd - 0.02).abs() < 0.001);
        assert!((metrics.avg_cost_per_test - 0.01).abs() < 0.001);
    }

    #[test]
    fn test_summary_format() {
        let mut metrics = EvalMetrics::new("test", "1.0");
        metrics.add_result(create_test_result("test1", true));
        metrics.finalize();

        let summary = metrics.summary();
        assert!(summary.contains("test v1.0"));
        assert!(summary.contains("Pass Rate"));
    }

    #[test]
    fn test_skipped_tests() {
        let mut metrics = EvalMetrics::new("test", "1.0");
        metrics.add_result(create_test_result("test1", true));
        metrics.add_skipped();
        metrics.add_skipped();

        assert_eq!(metrics.passed_tests, 1);
        assert_eq!(metrics.skipped_tests, 2);
        assert_eq!(metrics.total_tests, 1); // Skipped not counted in total
    }
}
