/// Evaluation Runner
///
/// This module provides the core evaluation execution logic. The runner loads eval datasets,
/// executes test cases (currently via simulation), and produces metrics for analysis.
///
/// In future iterations, this will integrate with the full Tandem engine to execute real
/// automation runs. For now, it provides the framework structure and simulation mode for
/// CI/CD integration testing.

use std::path::Path;

use crate::eval::dataset::{ArtifactStatus, EvalDataset, EvalTestCase};
use crate::eval::metrics::{EvalMetrics, EvalRunResult};
use crate::failures::AIFailureMode;

/// Configuration for an evaluation run
#[derive(Debug, Clone)]
pub struct EvalRunnerConfig {
    /// Number of parallel workers (default: 1)
    pub num_workers: u32,
    /// Default provider for tests that don't specify (default: "anthropic")
    pub default_provider: String,
    /// Default model
    pub default_model: String,
    /// Maximum test execution time in seconds
    pub max_test_duration_secs: u64,
    /// Whether to use simulation mode (no actual AI calls)
    pub simulation_mode: bool,
    /// Random seed for reproducible simulation results
    pub random_seed: Option<u64>,
}

impl Default for EvalRunnerConfig {
    fn default() -> Self {
        Self {
            num_workers: 1,
            default_provider: "anthropic".to_string(),
            default_model: "claude-haiku-4-5-20251001".to_string(),
            max_test_duration_secs: 300,
            simulation_mode: true, // Default to simulation for safety
            random_seed: None,
        }
    }
}

/// The evaluation runner
pub struct EvalRunner {
    config: EvalRunnerConfig,
}

impl EvalRunner {
    /// Create a new evaluation runner with the given config
    pub fn new(config: EvalRunnerConfig) -> Self {
        Self { config }
    }

    /// Load a dataset from a YAML file
    pub fn load_dataset(&self, path: &Path) -> Result<EvalDataset, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read dataset file: {}", e))?;

        serde_yaml::from_str(&content)
            .map_err(|e| format!("Failed to parse YAML dataset: {}", e))
    }

    /// Execute a complete dataset and return aggregated metrics
    pub async fn run_dataset(&self, dataset: &EvalDataset) -> EvalMetrics {
        let mut metrics = EvalMetrics::new(&dataset.name, &dataset.version);

        // Sort test cases by priority (highest first)
        let test_cases = dataset.sorted_by_priority();

        for test_case in test_cases {
            if !test_case.enabled {
                metrics.add_skipped();
                continue;
            }

            let result = self.run_test_case(test_case).await;
            metrics.add_result(result);
        }

        metrics.finalize();
        metrics
    }

    /// Execute a single test case
    pub async fn run_test_case(&self, test_case: &EvalTestCase) -> EvalRunResult {
        let start_time = std::time::Instant::now();

        if self.config.simulation_mode {
            self.run_simulation(test_case, start_time)
        } else {
            // TODO: Integrate with actual Tandem engine
            // For now, simulation mode is the only implementation
            self.run_simulation(test_case, start_time)
        }
    }

    /// Run a test case in simulation mode (no actual AI calls)
    ///
    /// This deterministically simulates likely outcomes based on test characteristics.
    /// Useful for CI/CD framework validation without incurring AI costs.
    fn run_simulation(
        &self,
        test_case: &EvalTestCase,
        start_time: std::time::Instant,
    ) -> EvalRunResult {
        // Determine simulated outcome based on test characteristics
        let is_critical_path = test_case.tags.contains(&"happy_path".to_string());
        let is_transient_test = test_case.tags.contains(&"transient".to_string());
        let is_disabled_path = test_case.tags.contains(&"degradation".to_string());

        // Simulated pass/fail logic
        let passed = if is_critical_path {
            true // Happy path always passes in simulation
        } else if is_transient_test {
            true // Transient failures recover in simulation
        } else if is_disabled_path {
            test_case.expected_output.unmet_requirements_acceptable
        } else {
            true // Default to pass
        };

        // Simulated metrics based on test config
        let repair_iterations = if passed {
            if is_critical_path {
                1
            } else if is_transient_test {
                2
            } else {
                1
            }
        } else {
            test_case.expected_output.max_repair_iterations.unwrap_or(3)
        };

        let tokens_used = 1500 + (repair_iterations as u64 * 500);
        let cost_usd = (tokens_used as f64) * 0.000003; // Approximate Claude Sonnet pricing

        let validators_passed = if passed {
            test_case.expected_output.required_validators.clone()
        } else {
            Vec::new()
        };

        let validators_failed = if !passed {
            test_case.expected_output.required_validators.clone()
        } else {
            Vec::new()
        };

        let (failure_mode, error_message) = if !passed {
            (
                Some(AIFailureMode::ArtifactValidationFailed {
                    validator_class: "contract".to_string(),
                }),
                Some(format!(
                    "Test {} failed in simulation",
                    test_case.id
                )),
            )
        } else {
            (None, None)
        };

        let duration_ms = start_time.elapsed().as_millis() as u64;

        EvalRunResult {
            test_id: test_case.id.clone(),
            description: test_case.description.clone(),
            passed,
            artifact_status: if passed {
                test_case.expected_output.artifact_status
            } else {
                ArtifactStatus::Failed
            },
            repair_iterations,
            tokens_used,
            cost_usd,
            duration_ms,
            validators_passed,
            validators_failed,
            failure_mode,
            error_message,
            tags: test_case.tags.clone(),
        }
    }

    /// Save evaluation results to a JSON file
    pub fn save_results(&self, metrics: &EvalMetrics, path: &Path) -> Result<(), String> {
        let json = serde_json::to_string_pretty(metrics)
            .map_err(|e| format!("Failed to serialize metrics: {}", e))?;

        std::fs::write(path, json)
            .map_err(|e| format!("Failed to write results file: {}", e))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::eval::dataset::EvalDataset;

    #[test]
    fn test_runner_creation() {
        let config = EvalRunnerConfig::default();
        let _runner = EvalRunner::new(config);
    }

    #[tokio::test]
    async fn test_simulation_mode() {
        let config = EvalRunnerConfig {
            simulation_mode: true,
            ..Default::default()
        };
        let runner = EvalRunner::new(config);

        let mut test_case = EvalTestCase::new("test1", "Test case");
        test_case.tags = vec!["happy_path".to_string()];

        let result = runner.run_test_case(&test_case).await;
        assert!(result.passed);
        assert_eq!(result.test_id, "test1");
    }

    #[tokio::test]
    async fn test_run_dataset() {
        let config = EvalRunnerConfig {
            simulation_mode: true,
            ..Default::default()
        };
        let runner = EvalRunner::new(config);

        let mut tc1 = EvalTestCase::new("test1", "Happy path");
        tc1.tags = vec!["happy_path".to_string()];
        let mut tc2 = EvalTestCase::new("test2", "Transient");
        tc2.tags = vec!["transient".to_string()];

        let dataset = EvalDataset::new("test_dataset", "1.0")
            .add_test_case(tc1)
            .add_test_case(tc2);

        let metrics = runner.run_dataset(&dataset).await;
        assert_eq!(metrics.total_tests, 2);
        assert_eq!(metrics.passed_tests, 2);
        assert!(metrics.pass_rate > 0.99);
    }

    #[tokio::test]
    async fn test_skip_disabled_tests() {
        let config = EvalRunnerConfig {
            simulation_mode: true,
            ..Default::default()
        };
        let runner = EvalRunner::new(config);

        let mut tc1 = EvalTestCase::new("test1", "Enabled");
        tc1.tags = vec!["happy_path".to_string()];
        let mut tc2 = EvalTestCase::new("test2", "Disabled");
        tc2.enabled = false;

        let dataset = EvalDataset::new("test_dataset", "1.0")
            .add_test_case(tc1)
            .add_test_case(tc2);

        let metrics = runner.run_dataset(&dataset).await;
        assert_eq!(metrics.total_tests, 1);
        assert_eq!(metrics.skipped_tests, 1);
    }

    #[test]
    fn test_save_results() {
        let temp_dir = tempfile::tempdir().unwrap();
        let output_path = temp_dir.path().join("results.json");

        let metrics = EvalMetrics::new("test", "1.0");
        let runner = EvalRunner::new(EvalRunnerConfig::default());

        let result = runner.save_results(&metrics, &output_path);
        assert!(result.is_ok());
        assert!(output_path.exists());
    }

    #[test]
    fn test_load_dataset_from_yaml() {
        let yaml = r#"
name: "test_dataset"
version: "1.0"
description: "Test"
tags: ["test"]
test_cases:
  - id: "test1"
    description: "Test 1"
    priority: 1
    automation_spec:
      name: "test"
      nodes: []
    expected_output:
      artifact_status: "completed"
      required_validators: []
"#;

        let temp_dir = tempfile::tempdir().unwrap();
        let yaml_path = temp_dir.path().join("test.yaml");
        std::fs::write(&yaml_path, yaml).unwrap();

        let runner = EvalRunner::new(EvalRunnerConfig::default());
        let dataset = runner.load_dataset(&yaml_path).unwrap();

        assert_eq!(dataset.name, "test_dataset");
        assert_eq!(dataset.test_cases.len(), 1);
    }
}
