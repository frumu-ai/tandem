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

/// A capability the goal needs, expressed independently of any concrete tool.
///
/// Discovery resolves each `CapabilityRequirement` to zero or more
/// [`AvailableCapability`] candidates. Keeping requirements tool-agnostic lets a
/// goal be satisfied by different concrete tools across tenants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityRequirement {
    /// Stable id for this requirement within the goal (e.g. `read_source`).
    pub requirement_id: String,
    /// Human-readable description of what the step must accomplish.
    pub description: String,
    /// Tags a satisfying capability must carry (e.g. `["file_io", "read"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_tags: Vec<String>,
    /// Whether the goal can still be satisfied if this requirement is unmet.
    #[serde(default = "default_true")]
    pub mandatory: bool,
}

fn default_true() -> bool {
    true
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
    /// The tool-agnostic requirements the goal was decomposed into.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub requirements: Vec<CapabilityRequirement>,
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

/// Governance lifecycle of a [`StrategyCandidate`].
///
/// Mirrors the `WorkflowLearningCandidate` status machine deliberately: a
/// strategy candidate is reviewable evidence that flows
/// `Proposed -> Approved -> Applied`, may be `Rejected`, and may be
/// `Superseded` by a newer candidate for the same goal. The shared shape keeps
/// goal-learning review semantics identical to workflow-learning review, even
/// though the two candidate *payloads* are different (see module docs and the
/// GCL design note for the ownership decision).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyCandidateStatus {
    /// Newly produced by discovery; awaiting human/governance review.
    Proposed,
    /// Approved for materialization into a proposal draft.
    Approved,
    /// Reviewer declined; retained as auditable evidence.
    Rejected,
    /// Materialized into a `WorkflowProposalDraft` / Automation V2 preview.
    Applied,
    /// Replaced by a newer candidate for the same goal.
    Superseded,
}

impl StrategyCandidateStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Approved => "approved",
            Self::Rejected => "rejected",
            Self::Applied => "applied",
            Self::Superseded => "superseded",
        }
    }

    /// Whether this status permits a transition into `next`.
    ///
    /// Fails closed: terminal states (`Rejected`, `Superseded`) allow no
    /// further transitions, and `Applied` may only be superseded.
    pub fn can_transition_to(self, next: Self) -> bool {
        use StrategyCandidateStatus::*;
        matches!(
            (self, next),
            (Proposed, Approved)
                | (Proposed, Rejected)
                | (Proposed, Superseded)
                | (Approved, Applied)
                | (Approved, Rejected)
                | (Approved, Superseded)
                | (Applied, Superseded)
        )
    }

    /// Terminal states accept no further transitions.
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Rejected | Self::Superseded)
    }
}

/// A reviewable strategy for reaching a goal: a goal + chosen composition path
/// + its discovery provenance, carried through a governance lifecycle.
///
/// This is intentionally a *separate type* from `WorkflowLearningCandidate`.
/// Workflow-learning candidates repair an existing workflow and are bound to a
/// `workflow_id`/`source_run_id`/`node_id`; a strategy candidate composes a
/// *new* workflow toward a goal and has no such anchors. The two reuse the same
/// status lifecycle ([`StrategyCandidateStatus`]) but not the same payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StrategyCandidate {
    pub candidate_id: String,
    pub goal_id: String,
    /// The discovery decision (`gcl_*`) that produced this candidate.
    pub discovery_decision_id: String,
    /// The composition path this strategy proposes to execute.
    pub composition: CompositionPath,
    pub status: StrategyCandidateStatus,
    /// 0.0-1.0, copied from the composition's compatibility at proposal time.
    pub confidence: f64,
    /// Stable hash for de-duplicating equivalent strategies for a goal.
    pub fingerprint: String,
    /// When applied, the proposal draft this candidate produced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal_draft_id: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Linkage record connecting an applied [`StrategyCandidate`] to the planner /
/// Automation V2 preview surfaces, so goal-learning output reuses the existing
/// proposal-review machinery rather than introducing a parallel one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowProposalDraft {
    pub proposal_draft_id: String,
    /// The strategy candidate this draft materializes.
    pub strategy_candidate_id: String,
    pub goal_id: String,
    /// The planner plan-draft id this proposal feeds into, when materialized
    /// through the workflow planner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner_plan_draft_id: Option<String>,
    /// The Automation V2 preview/spec id produced for review, when compiled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub automation_v2_preview_id: Option<String>,
    /// Capability ids the draft requires (mirrors planner review record).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_capabilities: Vec<String>,
    /// Capability ids that are required but currently blocked/unauthorized.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub blocked_capabilities: Vec<String>,
    pub created_at_ms: u64,
}

/// Audit event types emitted across the goal-planning and strategy/proposal
/// review lifecycle. Centralized as constants so emitters and audit consumers
/// agree on the wire vocabulary.
pub mod audit_events {
    /// A goal was accepted and discovery was run for it.
    pub const GOAL_PLANNED: &str = "goal_capability_learning.goal_planned";
    /// A strategy candidate was proposed by discovery.
    pub const STRATEGY_PROPOSED: &str = "goal_capability_learning.strategy_proposed";
    /// A reviewer approved a strategy candidate.
    pub const STRATEGY_APPROVED: &str = "goal_capability_learning.strategy_approved";
    /// A reviewer rejected a strategy candidate.
    pub const STRATEGY_REJECTED: &str = "goal_capability_learning.strategy_rejected";
    /// An approved strategy was materialized into a proposal draft.
    pub const PROPOSAL_DRAFTED: &str = "goal_capability_learning.proposal_drafted";
    /// A strategy candidate was superseded by a newer one.
    pub const STRATEGY_SUPERSEDED: &str = "goal_capability_learning.strategy_superseded";
}

#[cfg(test)]
mod tests;
