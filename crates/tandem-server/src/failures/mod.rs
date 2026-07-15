// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

/// AI Failure Mode Taxonomy
///
/// This module provides structured categorization of AI system failures across Tandem,
/// enabling systematic failure analysis, regression detection, and compliance auditing.
use serde::{Deserialize, Serialize};
use std::fmt;

/// Categorizes the severity and type of AI failures
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCategoryKind {
    /// Cannot recover, must fail the entire automation
    Critical,
    /// High-priority failure, likely requires human intervention
    High,
    /// Medium-priority, may recover with retries
    Medium,
    /// Low-priority, informational for monitoring
    Low,
}

/// Structured enumeration of AI failure modes
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "failure_mode", content = "details")]
pub enum AIFailureMode {
    // ===== Output Validation Failures =====
    /// Artifact failed one or more validators
    ArtifactValidationFailed { validator_class: String },
    /// Contract violation (expected/observed mismatch)
    ContractViolation { expected: String, observed: String },
    /// Required citations missing from artifact
    CitationMissing {
        required_count: u32,
        found_count: u32,
    },
    /// Web sources or references missing
    WebSourcesMissing,
    /// File path validation failed
    PathTraversalDetected { attempted_path: String },
    /// Output structure invalid (JSON schema, markdown, etc.)
    InvalidOutputStructure { expected_format: String },

    // ===== Provider Failures =====
    /// Provider request timed out
    ProviderTimeout { timeout_ms: u64, provider: String },
    /// Rate limit exceeded
    ProviderRateLimit {
        provider: String,
        retry_after_secs: Option<u64>,
    },
    /// Model not found or unavailable
    ProviderModelNotFound { model: String, provider: String },
    /// Provider returned error
    ProviderError {
        provider: String,
        error_code: String,
    },
    /// Provider stream decode failure
    ProviderStreamFailure { provider: String, error: String },
    /// Fallback provider exhausted
    FallbackProvidersExhausted,

    // ===== Repair & Retry Exhaustion =====
    /// Repair attempts exceeded budget
    RepairBudgetExhausted { max_iterations: u32, attempts: u32 },
    /// Infinite repair loop detected
    RepairLoopInfinite { cycle_count: u32 },
    /// Repair state machine invalid
    RepairStateInvalid { state: String },
    /// Node repair required but unavailable
    RepairUnavailable { reason: String },

    // ===== Data Access Failures =====
    /// Source file/directory access denied
    SourceAccessDenied { source: String, reason: String },
    /// Source not found
    SourceNotFound { source: String },
    /// Data corruption detected
    DataCorruption { source: String, error: String },
    /// Workspace not initialized
    WorkspaceNotFound { workspace_id: String },

    // ===== Timeout Failures =====
    /// Node execution exceeded timeout
    NodeTimeout { timeout_ms: u64, actual_ms: u64 },
    /// Session execution exceeded timeout
    SessionTimeout { timeout_ms: u64, actual_ms: u64 },
    /// Provider call exceeded timeout
    ProviderCallTimeout { timeout_ms: u64 },

    // ===== Authorization & Security Failures =====
    /// User not authorized for operation
    AuthorizationFailed { user_id: String, resource: String },
    /// API key invalid or expired
    InvalidApiKey { provider: String },
    /// Permission denied for resource
    PermissionDenied { resource: String },

    // ===== Resource Exhaustion =====
    /// Token budget exceeded
    TokenBudgetExhausted { limit: u64, used: u64 },
    /// Cost budget exceeded
    CostBudgetExhausted { limit_usd: f64, used_usd: f64 },
    /// Memory limit exceeded
    MemoryExhausted { limit_mb: u64 },
    /// Disk space exhausted
    DiskSpaceExhausted { required_mb: u64, available_mb: u64 },

    // ===== Configuration Failures =====
    /// Configuration invalid or incomplete
    ConfigurationError { field: String, reason: String },
    /// Required dependency missing
    DependencyMissing { dependency: String },
    /// Feature not enabled
    FeatureDisabled { feature: String },

    // ===== Unknown/Other Failures =====
    /// Uncategorized failure
    Unknown { error_message: String },
}

/// Detailed failure context for analysis and debugging
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FailureContext {
    /// Unique automation ID
    pub automation_id: String,
    /// Node ID where failure occurred
    pub node_id: String,
    /// Run ID being executed
    pub run_id: String,
    /// Failure mode classification
    pub failure_mode: AIFailureMode,
    /// Severity/category
    pub category: FailureCategoryKind,
    /// Timestamp in milliseconds
    pub timestamp_ms: u64,
    /// Original error text
    pub error_text: String,
    /// Whether recovery was attempted
    pub recovery_attempted: bool,
    /// Number of repair attempts made
    pub repair_attempts: u32,
    /// Provider that failed (if applicable)
    pub provider: Option<String>,
    /// Tags for filtering/analysis
    pub tags: Vec<String>,
    /// Human-readable description
    pub description: String,
}

impl fmt::Display for AIFailureMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl fmt::Display for FailureCategoryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FailureCategoryKind::Critical => write!(f, "Critical"),
            FailureCategoryKind::High => write!(f, "High"),
            FailureCategoryKind::Medium => write!(f, "Medium"),
            FailureCategoryKind::Low => write!(f, "Low"),
        }
    }
}

/// Classify error text into a structured failure mode
///
/// Attempts to parse error messages and categorize them into known failure modes.
/// Falls back to `AIFailureMode::Unknown` if no pattern matches.
pub fn classify_error_text(error_text: &str, node_context: Option<&str>) -> AIFailureMode {
    let lower = error_text.to_lowercase();

    // Provider failures
    if lower.contains("timeout") || lower.contains("timed out") {
        return AIFailureMode::ProviderTimeout {
            timeout_ms: 30000,
            provider: "unknown".to_string(),
        };
    }
    if lower.contains("rate limit") || lower.contains("rate_limit") {
        return AIFailureMode::ProviderRateLimit {
            provider: "unknown".to_string(),
            retry_after_secs: None,
        };
    }
    if lower.contains("model not found") || lower.contains("model_not_found") {
        return AIFailureMode::ProviderModelNotFound {
            model: "unknown".to_string(),
            provider: "unknown".to_string(),
        };
    }

    // Validation failures
    if lower.contains("validation failed") || lower.contains("validator") {
        return AIFailureMode::ArtifactValidationFailed {
            validator_class: "unknown".to_string(),
        };
    }
    if lower.contains("citation") && lower.contains("missing") {
        return AIFailureMode::CitationMissing {
            required_count: 1,
            found_count: 0,
        };
    }
    if lower.contains("web source") && lower.contains("missing") {
        return AIFailureMode::WebSourcesMissing;
    }

    // Repair failures
    if lower.contains("repair") && lower.contains("budget") {
        return AIFailureMode::RepairBudgetExhausted {
            max_iterations: 3,
            attempts: 3,
        };
    }
    if lower.contains("repair") && lower.contains("loop") {
        return AIFailureMode::RepairLoopInfinite { cycle_count: 5 };
    }

    // Access failures
    if lower.contains("permission") && lower.contains("denied") {
        return AIFailureMode::SourceAccessDenied {
            source: "unknown".to_string(),
            reason: error_text.to_string(),
        };
    }
    if lower.contains("not found") && lower.contains("source") {
        return AIFailureMode::SourceNotFound {
            source: "unknown".to_string(),
        };
    }

    // Timeout failures
    if lower.contains("node") && lower.contains("timeout") {
        return AIFailureMode::NodeTimeout {
            timeout_ms: 600000,
            actual_ms: 600001,
        };
    }

    // Configuration failures
    if lower.contains("configuration") || lower.contains("config") {
        return AIFailureMode::ConfigurationError {
            field: "unknown".to_string(),
            reason: error_text.to_string(),
        };
    }

    // Default to unknown
    AIFailureMode::Unknown {
        error_message: error_text.to_string(),
    }
}

/// Determine whether a failure should trigger a retry
///
/// Some failures are transient and can be retried, others are deterministic and will fail again.
pub fn should_retry(failure: &AIFailureMode) -> bool {
    matches!(
        failure,
        AIFailureMode::ProviderTimeout { .. }
            | AIFailureMode::ProviderRateLimit { .. }
            | AIFailureMode::ProviderStreamFailure { .. }
            | AIFailureMode::ProviderCallTimeout { .. }
            | AIFailureMode::SessionTimeout { .. }
    )
}

/// Determine severity category for a failure mode
pub fn categorize_failure(failure: &AIFailureMode) -> FailureCategoryKind {
    match failure {
        // Critical failures
        AIFailureMode::AuthorizationFailed { .. }
        | AIFailureMode::PermissionDenied { .. }
        | AIFailureMode::ContractViolation { .. }
        | AIFailureMode::RepairLoopInfinite { .. }
        | AIFailureMode::TokenBudgetExhausted { .. }
        | AIFailureMode::SourceNotFound { .. } => FailureCategoryKind::Critical,

        // High severity
        AIFailureMode::ArtifactValidationFailed { .. }
        | AIFailureMode::RepairBudgetExhausted { .. }
        | AIFailureMode::PathTraversalDetected { .. }
        | AIFailureMode::NodeTimeout { .. }
        | AIFailureMode::CostBudgetExhausted { .. }
        | AIFailureMode::ProviderError { .. }
        | AIFailureMode::FallbackProvidersExhausted => FailureCategoryKind::High,

        // Medium severity
        AIFailureMode::ProviderTimeout { .. }
        | AIFailureMode::ProviderRateLimit { .. }
        | AIFailureMode::ProviderStreamFailure { .. }
        | AIFailureMode::CitationMissing { .. }
        | AIFailureMode::WebSourcesMissing
        | AIFailureMode::DataCorruption { .. }
        | AIFailureMode::SourceAccessDenied { .. }
        | AIFailureMode::SessionTimeout { .. }
        | AIFailureMode::ConfigurationError { .. } => FailureCategoryKind::Medium,

        // Low severity
        AIFailureMode::ProviderModelNotFound { .. }
        | AIFailureMode::InvalidApiKey { .. }
        | AIFailureMode::MemoryExhausted { .. }
        | AIFailureMode::DiskSpaceExhausted { .. }
        | AIFailureMode::DependencyMissing { .. }
        | AIFailureMode::FeatureDisabled { .. }
        | AIFailureMode::InvalidOutputStructure { .. }
        | AIFailureMode::RepairStateInvalid { .. }
        | AIFailureMode::RepairUnavailable { .. }
        | AIFailureMode::WorkspaceNotFound { .. }
        | AIFailureMode::ProviderCallTimeout { .. }
        | AIFailureMode::Unknown { .. } => FailureCategoryKind::Low,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_provider_timeout() {
        let failure = classify_error_text("Request timed out after 30s", None);
        assert!(matches!(failure, AIFailureMode::ProviderTimeout { .. }));
    }

    #[test]
    fn test_classify_rate_limit() {
        let failure = classify_error_text("Rate limit exceeded", None);
        assert!(matches!(failure, AIFailureMode::ProviderRateLimit { .. }));
    }

    #[test]
    fn test_classify_validation_failure() {
        let failure = classify_error_text("Artifact validation failed: missing_section", None);
        assert!(matches!(
            failure,
            AIFailureMode::ArtifactValidationFailed { .. }
        ));
    }

    #[test]
    fn test_classify_citation_missing() {
        let failure = classify_error_text("Citations missing from research output", None);
        assert!(matches!(failure, AIFailureMode::CitationMissing { .. }));
    }

    #[test]
    fn test_classify_unknown() {
        let failure = classify_error_text("Some random error that we don't understand", None);
        assert!(matches!(failure, AIFailureMode::Unknown { .. }));
    }

    #[test]
    fn test_should_retry_transient() {
        let timeout = AIFailureMode::ProviderTimeout {
            timeout_ms: 30000,
            provider: "openai".to_string(),
        };
        assert!(should_retry(&timeout));
    }

    #[test]
    fn test_should_not_retry_deterministic() {
        let validation = AIFailureMode::ArtifactValidationFailed {
            validator_class: "citation".to_string(),
        };
        assert!(!should_retry(&validation));
    }

    #[test]
    fn test_categorize_critical() {
        let auth_failure = AIFailureMode::AuthorizationFailed {
            user_id: "user1".to_string(),
            resource: "automation1".to_string(),
        };
        assert_eq!(
            categorize_failure(&auth_failure),
            FailureCategoryKind::Critical
        );
    }

    #[test]
    fn test_categorize_high() {
        let validation = AIFailureMode::ArtifactValidationFailed {
            validator_class: "contract".to_string(),
        };
        assert_eq!(categorize_failure(&validation), FailureCategoryKind::High);
    }

    #[test]
    fn test_categorize_medium() {
        let timeout = AIFailureMode::ProviderTimeout {
            timeout_ms: 30000,
            provider: "openai".to_string(),
        };
        assert_eq!(categorize_failure(&timeout), FailureCategoryKind::Medium);
    }

    #[test]
    fn test_failure_context_serialization() {
        let context = FailureContext {
            automation_id: "auto1".to_string(),
            node_id: "node1".to_string(),
            run_id: "run1".to_string(),
            failure_mode: AIFailureMode::ProviderTimeout {
                timeout_ms: 30000,
                provider: "openai".to_string(),
            },
            category: FailureCategoryKind::Medium,
            timestamp_ms: 1234567890,
            error_text: "Request timed out".to_string(),
            recovery_attempted: true,
            repair_attempts: 2,
            provider: Some("openai".to_string()),
            tags: vec!["transient".to_string(), "retriable".to_string()],
            description: "Provider timeout during execution".to_string(),
        };

        let json = serde_json::to_string(&context).unwrap();
        let parsed: FailureContext = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.automation_id, "auto1");
        assert_eq!(parsed.repair_attempts, 2);
    }

    #[test]
    fn test_failure_mode_display() {
        let failure = AIFailureMode::CitationMissing {
            required_count: 3,
            found_count: 1,
        };
        let display_str = failure.to_string();
        assert!(display_str.contains("CitationMissing"));
    }
}
