/// Regression Detection System
///
/// Compares current evaluation metrics against a saved baseline to detect
/// quality regressions before deployment. Integrates with CI/CD via JSON
/// reports and configurable thresholds.
///
/// Used by `.github/workflows/eval-regression-gate.yml` to fail PRs when
/// pass_rate drops or cost increases beyond acceptable thresholds.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::eval::metrics::EvalMetrics;

/// Persisted baseline metrics for a dataset
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalBaseline {
    /// Dataset this baseline applies to
    pub dataset_name: String,
    /// Dataset version this baseline was captured against
    pub dataset_version: String,
    /// Baseline schema version (for forward compatibility)
    pub baseline_version: String,
    /// When this baseline was created (ms timestamp)
    pub created_at_ms: u64,
    /// Git commit SHA when baseline was captured
    pub commit_sha: Option<String>,
    /// Git branch when baseline was captured
    pub branch: Option<String>,

    /// Baseline pass rate (0.0 - 1.0)
    pub pass_rate: f64,
    /// Baseline average repair iterations
    pub avg_repair_iterations: f64,
    /// Baseline average cost per test in USD
    pub avg_cost_per_test: f64,
    /// Baseline total cost in USD
    pub total_cost_usd: f64,
    /// Baseline provider failure rate (0.0 - 1.0)
    pub provider_failure_rate: f64,

    /// Pass rates by validator class
    #[serde(default)]
    pub validator_pass_rates: HashMap<String, f64>,
}

impl EvalBaseline {
    /// Create a baseline from current metrics
    pub fn from_metrics(metrics: &EvalMetrics) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            dataset_name: metrics.dataset_name.clone(),
            dataset_version: metrics.dataset_version.clone(),
            baseline_version: "1.0".to_string(),
            created_at_ms: now,
            commit_sha: std::env::var("GIT_COMMIT_SHA").ok(),
            branch: std::env::var("GIT_BRANCH").ok(),
            pass_rate: metrics.pass_rate,
            avg_repair_iterations: metrics.avg_repair_iterations,
            avg_cost_per_test: metrics.avg_cost_per_test,
            total_cost_usd: metrics.total_cost_usd,
            provider_failure_rate: metrics.provider_failure_rate,
            validator_pass_rates: metrics.validator_pass_rates.clone(),
        }
    }

    /// Load a baseline from a JSON file
    pub fn load_from_file(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("Failed to read baseline file: {}", e))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse baseline JSON: {}", e))
    }

    /// Save the baseline to a JSON file
    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize baseline: {}", e))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create baseline directory: {}", e))?;
        }
        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write baseline file: {}", e))
    }
}

/// Status of an individual regression check
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionStatus {
    /// Metric is within acceptable range (or improved)
    Pass,
    /// Metric is degraded but within warning threshold
    Warning,
    /// Metric exceeded the critical regression threshold (CI should fail)
    Regression,
}

/// A single metric comparison result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionCheck {
    /// Name of the metric being checked
    pub metric_name: String,
    /// Baseline value
    pub expected: f64,
    /// Current run value
    pub actual: f64,
    /// Absolute difference (actual - expected)
    pub delta: f64,
    /// Percentage change (delta / expected * 100)
    pub delta_percent: f64,
    /// Threshold for warning/regression triggered
    pub threshold: f64,
    /// Overall status of this check
    pub status: RegressionStatus,
    /// Human-readable explanation
    pub message: String,
}

/// Thresholds for regression detection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionThresholds {
    /// Pass rate drop that triggers a warning (e.g., 0.02 = 2 percentage points)
    pub pass_rate_drop_warning: f64,
    /// Pass rate drop that triggers a regression (e.g., 0.05 = 5 percentage points)
    pub pass_rate_drop_critical: f64,
    /// Relative cost increase that triggers a warning (e.g., 0.10 = 10%)
    pub cost_increase_warning: f64,
    /// Relative cost increase that triggers a regression (e.g., 0.20 = 20%)
    pub cost_increase_critical: f64,
    /// Relative repair iteration increase warning threshold
    pub repair_iter_increase_warning: f64,
    /// Relative repair iteration increase regression threshold
    pub repair_iter_increase_critical: f64,
    /// Provider failure rate increase warning threshold (absolute)
    pub provider_failure_increase_warning: f64,
    /// Provider failure rate increase regression threshold (absolute)
    pub provider_failure_increase_critical: f64,
}

impl Default for RegressionThresholds {
    fn default() -> Self {
        Self {
            pass_rate_drop_warning: 0.02,
            pass_rate_drop_critical: 0.05,
            cost_increase_warning: 0.10,
            cost_increase_critical: 0.20,
            repair_iter_increase_warning: 0.15,
            repair_iter_increase_critical: 0.30,
            provider_failure_increase_warning: 0.02,
            provider_failure_increase_critical: 0.05,
        }
    }
}

/// Complete regression detection report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegressionReport {
    pub dataset_name: String,
    pub baseline_version: String,
    pub generated_at_ms: u64,
    pub thresholds: RegressionThresholds,
    pub checks: Vec<RegressionCheck>,
    pub has_regressions: bool,
    pub has_warnings: bool,
}

impl RegressionReport {
    /// Returns true if any check is a Regression (CI should fail)
    pub fn should_fail_ci(&self) -> bool {
        self.has_regressions
    }

    /// Save the report to a JSON file
    pub fn save_to_file(&self, path: &Path) -> Result<(), String> {
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize report: {}", e))?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("Failed to create report directory: {}", e))?;
        }
        std::fs::write(path, content)
            .map_err(|e| format!("Failed to write report file: {}", e))
    }

    /// Format a human-readable summary of the report
    pub fn format_summary(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "=== Regression Detection Report: {} ===\n",
            self.dataset_name
        ));
        out.push_str(&format!("Baseline version: {}\n", self.baseline_version));

        let overall = if self.has_regressions {
            "REGRESSION DETECTED"
        } else if self.has_warnings {
            "WARNINGS DETECTED"
        } else {
            "PASS"
        };
        out.push_str(&format!("Overall: {}\n\n", overall));

        out.push_str("Checks:\n");
        for check in &self.checks {
            let icon = match check.status {
                RegressionStatus::Pass => "[OK]",
                RegressionStatus::Warning => "[WARN]",
                RegressionStatus::Regression => "[FAIL]",
            };
            out.push_str(&format!(
                "  {} {}: expected={:.4}, actual={:.4}, delta={:+.4} ({:+.1}%)\n",
                icon,
                check.metric_name,
                check.expected,
                check.actual,
                check.delta,
                check.delta_percent
            ));
            if check.status != RegressionStatus::Pass {
                out.push_str(&format!("       {}\n", check.message));
            }
        }

        if self.has_regressions {
            out.push_str("\nACTION REQUIRED: One or more metrics exceeded the critical threshold.\n");
            out.push_str("Review the changes that caused these regressions before merging.\n");
        }

        out
    }
}

/// Compare current metrics against a baseline to detect regressions
pub fn detect_regressions(
    current: &EvalMetrics,
    baseline: &EvalBaseline,
    thresholds: &RegressionThresholds,
) -> RegressionReport {
    let mut checks = Vec::new();

    checks.push(check_pass_rate(current, baseline, thresholds));
    checks.push(check_cost(current, baseline, thresholds));
    checks.push(check_repair_iterations(current, baseline, thresholds));
    checks.push(check_provider_failure_rate(current, baseline, thresholds));

    let has_regressions = checks.iter().any(|c| c.status == RegressionStatus::Regression);
    let has_warnings = checks.iter().any(|c| c.status == RegressionStatus::Warning);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    RegressionReport {
        dataset_name: current.dataset_name.clone(),
        baseline_version: baseline.baseline_version.clone(),
        generated_at_ms: now,
        thresholds: thresholds.clone(),
        checks,
        has_regressions,
        has_warnings,
    }
}

fn check_pass_rate(
    current: &EvalMetrics,
    baseline: &EvalBaseline,
    thresholds: &RegressionThresholds,
) -> RegressionCheck {
    let delta = current.pass_rate - baseline.pass_rate;
    let delta_percent = if baseline.pass_rate > 0.0 {
        (delta / baseline.pass_rate) * 100.0
    } else {
        0.0
    };

    let drop = -delta;
    let (status, message, threshold) = if drop >= thresholds.pass_rate_drop_critical {
        (
            RegressionStatus::Regression,
            format!(
                "Pass rate dropped {:.1} percentage points (>={:.1} threshold)",
                drop * 100.0,
                thresholds.pass_rate_drop_critical * 100.0
            ),
            thresholds.pass_rate_drop_critical,
        )
    } else if drop >= thresholds.pass_rate_drop_warning {
        (
            RegressionStatus::Warning,
            format!(
                "Pass rate dropped {:.1} percentage points",
                drop * 100.0
            ),
            thresholds.pass_rate_drop_warning,
        )
    } else {
        (
            RegressionStatus::Pass,
            "Pass rate within acceptable range".to_string(),
            thresholds.pass_rate_drop_warning,
        )
    };

    RegressionCheck {
        metric_name: "pass_rate".to_string(),
        expected: baseline.pass_rate,
        actual: current.pass_rate,
        delta,
        delta_percent,
        threshold,
        status,
        message,
    }
}

fn check_cost(
    current: &EvalMetrics,
    baseline: &EvalBaseline,
    thresholds: &RegressionThresholds,
) -> RegressionCheck {
    let delta = current.avg_cost_per_test - baseline.avg_cost_per_test;
    let delta_percent = if baseline.avg_cost_per_test > 0.0 {
        (delta / baseline.avg_cost_per_test) * 100.0
    } else {
        0.0
    };

    let increase_ratio = if baseline.avg_cost_per_test > 0.0 {
        delta / baseline.avg_cost_per_test
    } else {
        0.0
    };

    let (status, message, threshold) = if increase_ratio >= thresholds.cost_increase_critical {
        (
            RegressionStatus::Regression,
            format!(
                "Avg cost per test increased {:.1}% (>={:.1}% threshold)",
                increase_ratio * 100.0,
                thresholds.cost_increase_critical * 100.0
            ),
            thresholds.cost_increase_critical,
        )
    } else if increase_ratio >= thresholds.cost_increase_warning {
        (
            RegressionStatus::Warning,
            format!("Avg cost per test increased {:.1}%", increase_ratio * 100.0),
            thresholds.cost_increase_warning,
        )
    } else {
        (
            RegressionStatus::Pass,
            "Cost within acceptable range".to_string(),
            thresholds.cost_increase_warning,
        )
    };

    RegressionCheck {
        metric_name: "avg_cost_per_test".to_string(),
        expected: baseline.avg_cost_per_test,
        actual: current.avg_cost_per_test,
        delta,
        delta_percent,
        threshold,
        status,
        message,
    }
}

fn check_repair_iterations(
    current: &EvalMetrics,
    baseline: &EvalBaseline,
    thresholds: &RegressionThresholds,
) -> RegressionCheck {
    let delta = current.avg_repair_iterations - baseline.avg_repair_iterations;
    let delta_percent = if baseline.avg_repair_iterations > 0.0 {
        (delta / baseline.avg_repair_iterations) * 100.0
    } else {
        0.0
    };

    let increase_ratio = if baseline.avg_repair_iterations > 0.0 {
        delta / baseline.avg_repair_iterations
    } else {
        0.0
    };

    let (status, message, threshold) =
        if increase_ratio >= thresholds.repair_iter_increase_critical {
            (
                RegressionStatus::Regression,
                format!(
                    "Avg repair iterations increased {:.1}% (>={:.1}% threshold)",
                    increase_ratio * 100.0,
                    thresholds.repair_iter_increase_critical * 100.0
                ),
                thresholds.repair_iter_increase_critical,
            )
        } else if increase_ratio >= thresholds.repair_iter_increase_warning {
            (
                RegressionStatus::Warning,
                format!(
                    "Avg repair iterations increased {:.1}%",
                    increase_ratio * 100.0
                ),
                thresholds.repair_iter_increase_warning,
            )
        } else {
            (
                RegressionStatus::Pass,
                "Repair iterations within acceptable range".to_string(),
                thresholds.repair_iter_increase_warning,
            )
        };

    RegressionCheck {
        metric_name: "avg_repair_iterations".to_string(),
        expected: baseline.avg_repair_iterations,
        actual: current.avg_repair_iterations,
        delta,
        delta_percent,
        threshold,
        status,
        message,
    }
}

fn check_provider_failure_rate(
    current: &EvalMetrics,
    baseline: &EvalBaseline,
    thresholds: &RegressionThresholds,
) -> RegressionCheck {
    let delta = current.provider_failure_rate - baseline.provider_failure_rate;
    let delta_percent = if baseline.provider_failure_rate > 0.0 {
        (delta / baseline.provider_failure_rate) * 100.0
    } else {
        0.0
    };

    let (status, message, threshold) = if delta >= thresholds.provider_failure_increase_critical {
        (
            RegressionStatus::Regression,
            format!(
                "Provider failure rate increased {:.1} percentage points (>={:.1} threshold)",
                delta * 100.0,
                thresholds.provider_failure_increase_critical * 100.0
            ),
            thresholds.provider_failure_increase_critical,
        )
    } else if delta >= thresholds.provider_failure_increase_warning {
        (
            RegressionStatus::Warning,
            format!(
                "Provider failure rate increased {:.1} percentage points",
                delta * 100.0
            ),
            thresholds.provider_failure_increase_warning,
        )
    } else {
        (
            RegressionStatus::Pass,
            "Provider failure rate within acceptable range".to_string(),
            thresholds.provider_failure_increase_warning,
        )
    };

    RegressionCheck {
        metric_name: "provider_failure_rate".to_string(),
        expected: baseline.provider_failure_rate,
        actual: current.provider_failure_rate,
        delta,
        delta_percent,
        threshold,
        status,
        message,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_metrics(pass_rate: f64, avg_cost: f64, avg_repair: f64, provider_fail: f64) -> EvalMetrics {
        let mut m = EvalMetrics::new("test_dataset", "1.0");
        m.pass_rate = pass_rate;
        m.avg_cost_per_test = avg_cost;
        m.avg_repair_iterations = avg_repair;
        m.provider_failure_rate = provider_fail;
        m
    }

    fn make_baseline(pass_rate: f64, avg_cost: f64, avg_repair: f64, provider_fail: f64) -> EvalBaseline {
        EvalBaseline {
            dataset_name: "test_dataset".to_string(),
            dataset_version: "1.0".to_string(),
            baseline_version: "1.0".to_string(),
            created_at_ms: 0,
            commit_sha: None,
            branch: None,
            pass_rate,
            avg_repair_iterations: avg_repair,
            avg_cost_per_test: avg_cost,
            total_cost_usd: avg_cost * 10.0,
            provider_failure_rate: provider_fail,
            validator_pass_rates: HashMap::new(),
        }
    }

    #[test]
    fn test_baseline_from_metrics() {
        let mut m = make_metrics(0.9, 0.05, 1.5, 0.01);
        m.total_cost_usd = 0.5;
        let baseline = EvalBaseline::from_metrics(&m);

        assert_eq!(baseline.dataset_name, "test_dataset");
        assert_eq!(baseline.pass_rate, 0.9);
        assert_eq!(baseline.avg_cost_per_test, 0.05);
        assert_eq!(baseline.avg_repair_iterations, 1.5);
    }

    #[test]
    fn test_no_regression_when_metrics_stable() {
        let current = make_metrics(0.90, 0.05, 1.5, 0.01);
        let baseline = make_baseline(0.90, 0.05, 1.5, 0.01);
        let thresholds = RegressionThresholds::default();

        let report = detect_regressions(&current, &baseline, &thresholds);
        assert!(!report.has_regressions);
        assert!(!report.has_warnings);
        assert!(!report.should_fail_ci());
    }

    #[test]
    fn test_pass_rate_drop_triggers_regression() {
        // Drop from 0.95 to 0.85 (10 pp drop, exceeds 5 pp critical threshold)
        let current = make_metrics(0.85, 0.05, 1.5, 0.01);
        let baseline = make_baseline(0.95, 0.05, 1.5, 0.01);
        let thresholds = RegressionThresholds::default();

        let report = detect_regressions(&current, &baseline, &thresholds);
        assert!(report.has_regressions);
        assert!(report.should_fail_ci());

        let pass_rate_check = report.checks.iter()
            .find(|c| c.metric_name == "pass_rate")
            .expect("pass_rate check should exist");
        assert_eq!(pass_rate_check.status, RegressionStatus::Regression);
    }

    #[test]
    fn test_pass_rate_small_drop_triggers_warning() {
        // Drop from 0.95 to 0.93 (2 pp drop, hits warning at 2 pp)
        let current = make_metrics(0.93, 0.05, 1.5, 0.01);
        let baseline = make_baseline(0.95, 0.05, 1.5, 0.01);
        let thresholds = RegressionThresholds::default();

        let report = detect_regressions(&current, &baseline, &thresholds);
        assert!(!report.has_regressions);
        assert!(report.has_warnings);

        let check = report.checks.iter()
            .find(|c| c.metric_name == "pass_rate")
            .expect("pass_rate check should exist");
        assert_eq!(check.status, RegressionStatus::Warning);
    }

    #[test]
    fn test_cost_increase_triggers_regression() {
        // Cost up 25% (exceeds 20% critical)
        let current = make_metrics(0.90, 0.125, 1.5, 0.01);
        let baseline = make_baseline(0.90, 0.10, 1.5, 0.01);
        let thresholds = RegressionThresholds::default();

        let report = detect_regressions(&current, &baseline, &thresholds);
        assert!(report.has_regressions);

        let check = report.checks.iter()
            .find(|c| c.metric_name == "avg_cost_per_test")
            .expect("cost check should exist");
        assert_eq!(check.status, RegressionStatus::Regression);
    }

    #[test]
    fn test_improvement_not_flagged() {
        // Pass rate IMPROVED from 0.85 to 0.95 - should not be a regression
        let current = make_metrics(0.95, 0.04, 1.2, 0.005);
        let baseline = make_baseline(0.85, 0.05, 1.5, 0.01);
        let thresholds = RegressionThresholds::default();

        let report = detect_regressions(&current, &baseline, &thresholds);
        assert!(!report.has_regressions);
        assert!(!report.has_warnings);
    }

    #[test]
    fn test_repair_iter_increase_triggers_regression() {
        // Repair iter up 50% (exceeds 30% critical)
        let current = make_metrics(0.90, 0.05, 3.0, 0.01);
        let baseline = make_baseline(0.90, 0.05, 2.0, 0.01);
        let thresholds = RegressionThresholds::default();

        let report = detect_regressions(&current, &baseline, &thresholds);
        assert!(report.has_regressions);

        let check = report.checks.iter()
            .find(|c| c.metric_name == "avg_repair_iterations")
            .expect("repair check should exist");
        assert_eq!(check.status, RegressionStatus::Regression);
    }

    #[test]
    fn test_provider_failure_increase_triggers_regression() {
        // Provider failures up 7 pp (exceeds 5 pp critical)
        let current = make_metrics(0.90, 0.05, 1.5, 0.08);
        let baseline = make_baseline(0.90, 0.05, 1.5, 0.01);
        let thresholds = RegressionThresholds::default();

        let report = detect_regressions(&current, &baseline, &thresholds);
        assert!(report.has_regressions);

        let check = report.checks.iter()
            .find(|c| c.metric_name == "provider_failure_rate")
            .expect("provider failure check should exist");
        assert_eq!(check.status, RegressionStatus::Regression);
    }

    #[test]
    fn test_baseline_serialization_roundtrip() {
        let tmp = TempDir::new().expect("create tempdir");
        let path = tmp.path().join("baseline.json");

        let original = make_baseline(0.92, 0.045, 1.3, 0.015);
        original.save_to_file(&path).expect("save");

        let loaded = EvalBaseline::load_from_file(&path).expect("load");
        assert_eq!(loaded.dataset_name, original.dataset_name);
        assert_eq!(loaded.pass_rate, original.pass_rate);
        assert_eq!(loaded.avg_cost_per_test, original.avg_cost_per_test);
    }

    #[test]
    fn test_report_format_includes_status() {
        let current = make_metrics(0.85, 0.05, 1.5, 0.01);
        let baseline = make_baseline(0.95, 0.05, 1.5, 0.01);
        let thresholds = RegressionThresholds::default();

        let report = detect_regressions(&current, &baseline, &thresholds);
        let summary = report.format_summary();

        assert!(summary.contains("REGRESSION DETECTED"));
        assert!(summary.contains("pass_rate"));
        assert!(summary.contains("[FAIL]"));
    }

    #[test]
    fn test_report_serialization() {
        let current = make_metrics(0.90, 0.05, 1.5, 0.01);
        let baseline = make_baseline(0.90, 0.05, 1.5, 0.01);
        let thresholds = RegressionThresholds::default();

        let report = detect_regressions(&current, &baseline, &thresholds);
        let json = serde_json::to_string(&report).expect("serialize");

        let parsed: RegressionReport = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed.dataset_name, report.dataset_name);
        assert_eq!(parsed.checks.len(), report.checks.len());
    }
}
