/// Evaluation Dataset Definitions
///
/// This module defines the YAML/JSON structure for evaluation test cases,
/// allowing evaluation datasets to be version-controlled and tracked in the codebase.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A complete evaluation dataset containing multiple test cases
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalDataset {
    /// Dataset name (e.g., "critical_path", "provider_failures")
    pub name: String,
    /// Version for tracking dataset evolution
    pub version: String,
    /// Human-readable description of what this dataset tests
    pub description: String,
    /// List of test cases in this dataset
    pub test_cases: Vec<EvalTestCase>,
    /// Tags for filtering/organization (e.g., "critical", "regression", "provider")
    #[serde(default)]
    pub tags: Vec<String>,
    /// When this dataset was created/last updated
    #[serde(default)]
    pub created_at: String,
}

/// A single evaluation test case
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalTestCase {
    /// Unique test case identifier (e.g., "critical_001")
    pub id: String,
    /// Short description of what this test exercises
    pub description: String,
    /// Priority for execution order (1=highest, 3=lowest)
    #[serde(default = "default_priority")]
    pub priority: u32,
    /// Complete automation specification (YAML will be embedded as string)
    pub automation_spec: AutomationSpecTest,
    /// Expected validator output for this test
    pub expected_output: EvalExpectedOutput,
    /// Whether this test is currently enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Tags for filtering (e.g., "happy_path", "validation", "web_research")
    #[serde(default)]
    pub tags: Vec<String>,
    /// Acceptable tolerance for metrics (e.g., repair iterations may vary by ±1)
    #[serde(default)]
    pub metric_tolerance: MetricTolerance,
}

/// Minimal automation spec for test definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutomationSpecTest {
    /// Automation name for test identification
    pub name: String,
    /// Task/flow nodes in this automation
    pub nodes: Vec<TestNode>,
    /// Validators applied to outputs
    #[serde(default)]
    pub validators: Vec<String>,
    /// Optional configuration overrides
    #[serde(default)]
    pub config: HashMap<String, serde_json::Value>,
}

/// Node definition for test automation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestNode {
    /// Node identifier
    pub id: String,
    /// Node type (e.g., "research", "generation", "validation")
    pub node_type: String,
    /// Brief node objective/description
    pub objective: String,
    /// Output contract (what format/structure is expected)
    #[serde(default)]
    pub output_contract: String,
}

/// Expected output characteristics for a test case
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalExpectedOutput {
    /// Artifact should complete (vs blocked/failed)
    pub artifact_status: ArtifactStatus,
    /// Validators that MUST pass (e.g., ["contract", "citations"])
    #[serde(default)]
    pub required_validators: Vec<String>,
    /// Validators that MAY pass (e.g., ["tone_appropriateness"])
    #[serde(default)]
    pub optional_validators: Vec<String>,
    /// Unmet requirements are acceptable (for relaxed profiles)
    #[serde(default)]
    pub unmet_requirements_acceptable: bool,
    /// Maximum repair iterations before failure (None = unlimited)
    pub max_repair_iterations: Option<u32>,
    /// Expected output format (e.g., "json", "markdown", "text")
    #[serde(default)]
    pub output_format: String,
    /// Minimum quality indicators that should be present
    #[serde(default)]
    pub quality_indicators: Vec<String>,
}

/// Artifact completion status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    /// Successfully completed
    Completed,
    /// Completed with warnings/unmet requirements
    CompletedWithWarnings,
    /// Blocked by validation/requirements
    Blocked,
    /// Failed with error
    Failed,
}

/// Tolerance/variance allowed in metric comparisons
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MetricTolerance {
    /// Allowed variance in repair iterations (e.g., 1 means ±1 iteration is acceptable)
    #[serde(default)]
    pub repair_iterations_delta: Option<u32>,
    /// Allowed variance in token usage (percentage, e.g., 0.1 means 10% variance)
    #[serde(default)]
    pub token_usage_tolerance_percent: Option<f64>,
    /// Allowed variance in cost (USD)
    #[serde(default)]
    pub cost_tolerance_usd: Option<f64>,
    /// Test is flaky - allow occasional failures (up to this many failures out of 10 runs)
    #[serde(default)]
    pub acceptable_failure_rate: Option<f32>,
}

// Defaults for serde
fn default_priority() -> u32 {
    2
}

fn default_true() -> bool {
    true
}

impl EvalDataset {
    /// Create a new evaluation dataset
    pub fn new(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: String::new(),
            test_cases: Vec::new(),
            tags: Vec::new(),
            created_at: chrono::Utc::now().to_rfc3339(),
        }
    }

    /// Add a test case to this dataset
    pub fn add_test_case(mut self, test_case: EvalTestCase) -> Self {
        self.test_cases.push(test_case);
        self
    }

    /// Get test cases in priority order (highest first)
    pub fn sorted_by_priority(&self) -> Vec<&EvalTestCase> {
        let mut cases: Vec<_> = self.test_cases.iter().collect();
        cases.sort_by_key(|t| std::cmp::Reverse(t.priority));
        cases
    }

    /// Filter test cases by tag
    pub fn filter_by_tag(&self, tag: &str) -> Vec<&EvalTestCase> {
        self.test_cases
            .iter()
            .filter(|tc| tc.tags.contains(&tag.to_string()) && tc.enabled)
            .collect()
    }

    /// Get enabled test cases only
    pub fn enabled_test_cases(&self) -> Vec<&EvalTestCase> {
        self.test_cases.iter().filter(|tc| tc.enabled).collect()
    }
}

impl EvalTestCase {
    /// Create a new test case
    pub fn new(id: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            description: description.into(),
            priority: 2,
            automation_spec: AutomationSpecTest {
                name: "test_automation".to_string(),
                nodes: Vec::new(),
                validators: Vec::new(),
                config: HashMap::new(),
            },
            expected_output: EvalExpectedOutput {
                artifact_status: ArtifactStatus::Completed,
                required_validators: Vec::new(),
                optional_validators: Vec::new(),
                unmet_requirements_acceptable: false,
                max_repair_iterations: Some(3),
                output_format: "json".to_string(),
                quality_indicators: Vec::new(),
            },
            enabled: true,
            tags: Vec::new(),
            metric_tolerance: MetricTolerance::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_dataset() {
        let dataset = EvalDataset::new("test", "1.0");
        assert_eq!(dataset.name, "test");
        assert_eq!(dataset.version, "1.0");
        assert!(dataset.test_cases.is_empty());
    }

    #[test]
    fn test_add_test_case() {
        let dataset = EvalDataset::new("test", "1.0")
            .add_test_case(EvalTestCase::new("test1", "Test 1"));

        assert_eq!(dataset.test_cases.len(), 1);
        assert_eq!(dataset.test_cases[0].id, "test1");
    }

    #[test]
    fn test_sorted_by_priority() {
        let mut tc1 = EvalTestCase::new("test1", "Test 1");
        tc1.priority = 1;
        let mut tc2 = EvalTestCase::new("test2", "Test 2");
        tc2.priority = 3;
        let mut tc3 = EvalTestCase::new("test3", "Test 3");
        tc3.priority = 2;

        let dataset = EvalDataset::new("test", "1.0")
            .add_test_case(tc1)
            .add_test_case(tc2)
            .add_test_case(tc3);

        let sorted = dataset.sorted_by_priority();
        assert_eq!(sorted[0].id, "test1"); // priority 1
        assert_eq!(sorted[1].id, "test3"); // priority 2
        assert_eq!(sorted[2].id, "test2"); // priority 3
    }

    #[test]
    fn test_filter_by_tag() {
        let mut tc1 = EvalTestCase::new("test1", "Test 1");
        tc1.tags = vec!["critical".to_string()];
        let mut tc2 = EvalTestCase::new("test2", "Test 2");
        tc2.tags = vec!["regression".to_string()];

        let dataset = EvalDataset::new("test", "1.0")
            .add_test_case(tc1)
            .add_test_case(tc2);

        let critical = dataset.filter_by_tag("critical");
        assert_eq!(critical.len(), 1);
        assert_eq!(critical[0].id, "test1");
    }

    #[test]
    fn test_enabled_test_cases() {
        let mut tc1 = EvalTestCase::new("test1", "Test 1");
        tc1.enabled = true;
        let mut tc2 = EvalTestCase::new("test2", "Test 2");
        tc2.enabled = false;

        let dataset = EvalDataset::new("test", "1.0")
            .add_test_case(tc1)
            .add_test_case(tc2);

        let enabled = dataset.enabled_test_cases();
        assert_eq!(enabled.len(), 1);
        assert_eq!(enabled[0].id, "test1");
    }

    #[test]
    fn test_serde_roundtrip() {
        let dataset = EvalDataset::new("test_dataset", "1.0");
        let json = serde_json::to_string(&dataset).unwrap();
        let deserialized: EvalDataset = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "test_dataset");
        assert_eq!(deserialized.version, "1.0");
    }

    #[test]
    fn test_test_case_defaults() {
        let tc = EvalTestCase::new("test1", "Test 1");
        assert_eq!(tc.priority, 2);
        assert!(tc.enabled);
        assert_eq!(tc.expected_output.artifact_status, ArtifactStatus::Completed);
    }

    #[test]
    fn test_artifact_status_serialization() {
        let json = serde_json::to_string(&ArtifactStatus::Completed).unwrap();
        assert_eq!(json, "\"completed\"");

        let status: ArtifactStatus = serde_json::from_str("\"completed\"").unwrap();
        assert_eq!(status, ArtifactStatus::Completed);
    }
}
