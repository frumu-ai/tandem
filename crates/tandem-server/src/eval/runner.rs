/// Evaluation Runner
///
/// This module provides the core evaluation execution logic. The runner loads eval datasets,
/// executes test cases, and produces metrics for analysis.
///
/// Three engine modes (`EngineMode`):
/// - **Simulation** (default): hardcoded deterministic outcomes, no engine involved. Used
///   by the regression-gate CI workflow because it's zero-cost and fast.
/// - **Stub**: real Tandem engine execution backed by `ScriptedEvalProvider` — exercises
///   the full automation/validator/repair path with deterministic responses. Used to
///   capture realistic baselines without real API spend.
/// - **Live**: real engine + real provider (requires API keys). Used for human-run
///   baseline captures and scheduled CI.
///
/// Stub and Live modes require an `AppState` to be wired into the runner via
/// `EvalRunner::with_app_state()`. The CLI does not yet bootstrap an `AppState`; a
/// follow-up phase will lift `test_state()` from `http/tests` so the binary can
/// construct one on demand.
use std::path::Path;
use std::time::Duration;

use crate::app::state::AppState;
use crate::eval::dataset::{ArtifactStatus, EvalDataset, EvalTestCase};
use crate::eval::engine_executor::{EngineExecutor, RemoteEngineExecutor};
use crate::eval::metrics::{EvalMetrics, EvalRunResult};
use crate::failures::AIFailureMode;

/// Which execution path a test case takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineMode {
    /// Hardcoded deterministic simulation — no engine, no AI calls.
    Simulation,
    /// Real engine, `ScriptedEvalProvider` swapped in for deterministic responses.
    Stub,
    /// Real engine, real provider (requires API keys in config).
    Live,
}

impl EngineMode {
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.to_ascii_lowercase().as_str() {
            "simulation" | "sim" => Ok(Self::Simulation),
            "stub" | "scripted" => Ok(Self::Stub),
            "live" | "real" => Ok(Self::Live),
            other => Err(format!(
                "unknown engine mode '{}' — expected one of simulation|stub|live",
                other
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            EngineMode::Simulation => "simulation",
            EngineMode::Stub => "stub",
            EngineMode::Live => "live",
        }
    }
}

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
    /// Engine execution mode. Defaults to `Simulation` for safety.
    pub engine_mode: EngineMode,
    /// Remote engine HTTP endpoint URL (for Stub/Live modes)
    pub engine_url: String,
    /// Remote engine API token (for Stub/Live modes)
    pub engine_token: Option<String>,
    /// Legacy alias for `engine_mode == Simulation`. Kept for backward compatibility
    /// with existing callers and the `--simulation` CLI flag. When `engine_mode` is
    /// set, this is informational only.
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
            engine_mode: EngineMode::Simulation,
            engine_url: "http://127.0.0.1:39731".to_string(),
            engine_token: None,
            simulation_mode: true,
            random_seed: None,
        }
    }
}

/// The evaluation runner
pub struct EvalRunner {
    config: EvalRunnerConfig,
    /// AppState for Stub/Live modes. None means only Simulation can run.
    app_state: Option<AppState>,
}

impl EvalRunner {
    /// Create a new evaluation runner with the given config. AppState is unset, so only
    /// Simulation mode will work — Stub/Live calls will return failed `EvalRunResult`s
    /// with a clear error message.
    pub fn new(config: EvalRunnerConfig) -> Self {
        Self {
            config,
            app_state: None,
        }
    }

    /// Attach an `AppState` so Stub/Live modes can dispatch through `EngineExecutor`.
    pub fn with_app_state(mut self, state: AppState) -> Self {
        self.app_state = Some(state);
        self
    }

    /// Load a dataset from a YAML file
    pub fn load_dataset(&self, path: &Path) -> Result<EvalDataset, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read dataset file: {}", e))?;

        serde_yaml::from_str(&content).map_err(|e| format!("Failed to parse YAML dataset: {}", e))
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

        match self.effective_engine_mode() {
            EngineMode::Simulation => self.run_simulation(test_case, start_time),
            EngineMode::Stub | EngineMode::Live => {
                // Prefer remote engine (via HTTP) if URL is configured
                if !self.config.engine_url.is_empty() {
                    if let Some(token) = &self.config.engine_token {
                        let executor = RemoteEngineExecutor::new(
                            self.config.engine_url.clone(),
                            token.clone(),
                        )
                        .with_max_duration(Duration::from_secs(self.config.max_test_duration_secs));
                        return executor.run_test_case(test_case).await;
                    }
                }

                // Fall back to local AppState if available
                match self.app_state.as_ref() {
                    Some(state) => {
                        let executor = EngineExecutor::new(state.clone())
                            .with_max_duration(Duration::from_secs(
                                self.config.max_test_duration_secs,
                            ))
                            .with_stub_inline_artifacts(
                                self.effective_engine_mode() == EngineMode::Stub,
                            );
                        executor.run_test_case(test_case).await
                    }
                    None => {
                        engine_mode_unavailable(test_case, self.effective_engine_mode(), start_time)
                    }
                }
            }
        }
    }

    /// Reconciles the new `engine_mode` field with the legacy `simulation_mode` bool.
    /// When `simulation_mode` is explicitly true and `engine_mode` is at its Simulation
    /// default, we honor that for backward compatibility. Otherwise `engine_mode` wins.
    fn effective_engine_mode(&self) -> EngineMode {
        if self.config.simulation_mode && self.config.engine_mode == EngineMode::Simulation {
            EngineMode::Simulation
        } else {
            self.config.engine_mode
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
                Some(format!("Test {} failed in simulation", test_case.id)),
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

        std::fs::write(path, json).map_err(|e| format!("Failed to write results file: {}", e))?;

        Ok(())
    }
}

/// Returned for Stub/Live test cases when no `AppState` has been attached to the
/// runner — keeps the same `EvalRunResult` shape so metrics aggregation still works
/// and the error is visible in the JSON output.
fn engine_mode_unavailable(
    test_case: &EvalTestCase,
    mode: EngineMode,
    start_time: std::time::Instant,
) -> EvalRunResult {
    let error = format!(
        "engine mode '{}' requires AppState — call EvalRunner::with_app_state(...) before run_test_case",
        mode.as_str()
    );
    EvalRunResult {
        test_id: test_case.id.clone(),
        description: test_case.description.clone(),
        passed: false,
        artifact_status: ArtifactStatus::Failed,
        repair_iterations: 0,
        tokens_used: 0,
        cost_usd: 0.0,
        duration_ms: start_time.elapsed().as_millis() as u64,
        validators_passed: Vec::new(),
        validators_failed: test_case.expected_output.required_validators.clone(),
        failure_mode: Some(AIFailureMode::FeatureDisabled {
            feature: format!("eval_runner_mode_{}", mode.as_str()),
        }),
        error_message: Some(error),
        tags: test_case.tags.clone(),
    }
}

fn engine_mode_unavailable_with_message(
    test_case: &EvalTestCase,
    mode: EngineMode,
    start_time: std::time::Instant,
    message: String,
) -> EvalRunResult {
    EvalRunResult {
        test_id: test_case.id.clone(),
        description: test_case.description.clone(),
        passed: false,
        artifact_status: ArtifactStatus::Failed,
        repair_iterations: 0,
        tokens_used: 0,
        cost_usd: 0.0,
        duration_ms: start_time.elapsed().as_millis() as u64,
        validators_passed: Vec::new(),
        validators_failed: test_case.expected_output.required_validators.clone(),
        failure_mode: Some(AIFailureMode::FeatureDisabled {
            feature: format!("eval_runner_mode_{}", mode.as_str()),
        }),
        error_message: Some(message),
        tags: test_case.tags.clone(),
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
    fn engine_mode_parses_known_strings() {
        assert_eq!(
            EngineMode::parse("simulation").unwrap(),
            EngineMode::Simulation
        );
        assert_eq!(EngineMode::parse("sim").unwrap(), EngineMode::Simulation);
        assert_eq!(EngineMode::parse("STUB").unwrap(), EngineMode::Stub);
        assert_eq!(EngineMode::parse("scripted").unwrap(), EngineMode::Stub);
        assert_eq!(EngineMode::parse("Live").unwrap(), EngineMode::Live);
        assert_eq!(EngineMode::parse("real").unwrap(), EngineMode::Live);
        assert!(EngineMode::parse("unknown").is_err());
    }

    #[test]
    fn engine_mode_as_str_roundtrips() {
        for mode in [EngineMode::Simulation, EngineMode::Stub, EngineMode::Live] {
            assert_eq!(EngineMode::parse(mode.as_str()).unwrap(), mode);
        }
    }

    #[tokio::test]
    async fn stub_mode_without_app_state_returns_clear_error() {
        let config = EvalRunnerConfig {
            engine_mode: EngineMode::Stub,
            simulation_mode: false,
            ..Default::default()
        };
        let runner = EvalRunner::new(config);

        let mut tc = EvalTestCase::new("test_stub", "needs engine");
        tc.tags = vec!["happy_path".to_string()];
        let result = runner.run_test_case(&tc).await;

        assert!(!result.passed);
        assert!(matches!(
            result.failure_mode,
            Some(AIFailureMode::FeatureDisabled { .. })
        ));
        assert!(result
            .error_message
            .as_ref()
            .map_or(false, |m| m.contains("AppState")));
    }

    #[tokio::test]
    async fn live_mode_without_app_state_returns_clear_error() {
        let config = EvalRunnerConfig {
            engine_mode: EngineMode::Live,
            simulation_mode: false,
            ..Default::default()
        };
        let runner = EvalRunner::new(config);

        let tc = EvalTestCase::new("test_live", "needs engine");
        let result = runner.run_test_case(&tc).await;

        assert!(!result.passed);
        assert!(result
            .error_message
            .as_ref()
            .map_or(false, |m| m.contains("live")));
    }

    #[tokio::test]
    async fn legacy_simulation_mode_bool_still_routes_to_simulation() {
        // Caller sets simulation_mode=true but leaves engine_mode at default — must keep
        // running in simulation, not break with a missing-AppState error.
        let config = EvalRunnerConfig {
            simulation_mode: true,
            engine_mode: EngineMode::Simulation, // default but make it explicit
            ..Default::default()
        };
        let runner = EvalRunner::new(config);
        let mut tc = EvalTestCase::new("legacy", "legacy caller");
        tc.tags = vec!["happy_path".to_string()];
        let result = runner.run_test_case(&tc).await;
        assert!(result.passed);
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
