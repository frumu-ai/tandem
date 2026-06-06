//! Goal Capability Learning: discovering and composing capabilities to reach goals.
//!
//! GCL analyzes a declarative goal specification and identifies which available
//! capabilities (tools, connectors, sub-automations) must be composed in sequence
//! to achieve that goal. This is distinct from Workflow Learning, which improves
//! *existing* workflows by analyzing execution traces.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A declarative specification of a desired outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalSpec {
    pub goal_id: String,
    pub title: String,
    pub description: String,
    pub input_parameters: Vec<GoalParameter>,
    pub expected_output_format: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub constraints: Vec<String>,
}

/// An input parameter required by a goal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalParameter {
    pub name: String,
    pub data_type: String,
    pub description: String,
    #[serde(default)]
    pub required: bool,
}

/// A discovered capability that may satisfy part of a goal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AvailableCapability {
    pub capability_id: String,
    pub tool_name: String,
    pub input_schema: Value,
    pub output_schema: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

/// A potential composition path to satisfy a goal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CompositionPath {
    /// Ordered sequence of capability_ids to execute.
    pub sequence: Vec<String>,
    /// Confidence that this path will satisfy the goal (0.0 to 1.0).
    pub compatibility_score: f64,
    /// Reasoning about why this composition was chosen.
    pub reasoning: String,
}

/// A gap between a goal's requirements and available capabilities.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CapabilityGap {
    NotFound {
        description: String,
    },
    NotAuthorized {
        capability_id: String,
    },
    RejectedByConstraint {
        capability_id: String,
        reason: String,
    },
}

/// Report from analyzing available capabilities against a goal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityDiscoveryReport {
    pub goal_id: String,
    pub discovered_capabilities: Vec<AvailableCapability>,
    pub composition_candidates: Vec<CompositionPath>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub gaps: Vec<CapabilityGap>,
    pub overall_confidence_score: f64,
    pub reasoning: String,
}

impl CapabilityDiscoveryReport {
    /// Primary recommendation: the highest-confidence composition path, if any.
    pub fn primary_recommendation(&self) -> Option<&CompositionPath> {
        self.composition_candidates.iter().max_by(|a, b| {
            a.compatibility_score
                .partial_cmp(&b.compatibility_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }
}

/// Request to discover capabilities for a goal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalCapabilityLearningRequest {
    pub goal: GoalSpec,
    #[serde(default)]
    pub max_candidates: usize,
}

/// Response from a goal capability learning discovery.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GoalCapabilityLearningResponse {
    pub request_id: String,
    pub report: CapabilityDiscoveryReport,
}

#[cfg(test)]
mod tests;
