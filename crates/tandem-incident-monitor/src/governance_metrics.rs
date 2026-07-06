// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Governance maturity metric thresholds and drift configuration (TAN-488).
//!
//! The metric *values* are computed server-side from audit events, incidents,
//! receipts, and policy decisions (they need live state); these types carry the
//! operator-tunable thresholds and drift sensitivity, with production-safe
//! defaults. A metric whose rate falls below its threshold — or a drift delta
//! that exceeds `drift_rate_delta_max` — is flagged as a breach and surfaced as
//! a dry-run posture finding.

use serde::{Deserialize, Serialize};

fn default_governance_confidence_min() -> f64 {
    0.9
}
fn default_authority_boundary_compliance_min() -> f64 {
    0.95
}
fn default_escalation_utilization_min() -> f64 {
    0.9
}
fn default_route_readiness_min() -> f64 {
    0.9
}
fn default_receipt_completeness_min() -> f64 {
    0.95
}
fn default_drift_rate_delta_max() -> f64 {
    0.25
}

/// Operator-tunable minimum rates (0.0–1.0) for the governance maturity metrics,
/// plus the maximum tolerated drift delta between the current and baseline
/// window before a change is flagged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IncidentMonitorGovernanceThresholds {
    /// Minimum share of high-risk decisions that must carry a complete audit
    /// trail (governance confidence score).
    #[serde(default = "default_governance_confidence_min")]
    pub governance_confidence_min: f64,
    /// Minimum share of publish attempts that must stay within configured
    /// authority boundaries.
    #[serde(default = "default_authority_boundary_compliance_min")]
    pub authority_boundary_compliance_min: f64,
    /// Minimum share of escalation-eligible cases that must reach human review.
    #[serde(default = "default_escalation_utilization_min")]
    pub escalation_utilization_min: f64,
    /// Minimum share of enabled sources/destinations that must be ready.
    #[serde(default = "default_route_readiness_min")]
    pub route_readiness_min: f64,
    /// Minimum share of publish receipts that must be complete (non-failed with
    /// an external reference).
    #[serde(default = "default_receipt_completeness_min")]
    pub receipt_completeness_min: f64,
    /// Maximum absolute change in a rate between the baseline and current window
    /// before behavioral drift is flagged.
    #[serde(default = "default_drift_rate_delta_max")]
    pub drift_rate_delta_max: f64,
}

impl Default for IncidentMonitorGovernanceThresholds {
    fn default() -> Self {
        Self {
            governance_confidence_min: default_governance_confidence_min(),
            authority_boundary_compliance_min: default_authority_boundary_compliance_min(),
            escalation_utilization_min: default_escalation_utilization_min(),
            route_readiness_min: default_route_readiness_min(),
            receipt_completeness_min: default_receipt_completeness_min(),
            drift_rate_delta_max: default_drift_rate_delta_max(),
        }
    }
}

impl IncidentMonitorGovernanceThresholds {
    /// The minimum-rate threshold for a metric id, if one is configured.
    pub fn min_for(&self, metric_id: &str) -> Option<f64> {
        match metric_id {
            "governance_confidence" => Some(self.governance_confidence_min),
            "authority_boundary_compliance" => Some(self.authority_boundary_compliance_min),
            "escalation_pathway_utilization" => Some(self.escalation_utilization_min),
            "route_readiness_compliance" => Some(self.route_readiness_min),
            "receipt_completeness" => Some(self.receipt_completeness_min),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_within_unit_range_and_looked_up_by_id() {
        let thresholds = IncidentMonitorGovernanceThresholds::default();
        for id in [
            "governance_confidence",
            "authority_boundary_compliance",
            "escalation_pathway_utilization",
            "route_readiness_compliance",
            "receipt_completeness",
        ] {
            let min = thresholds.min_for(id).expect("threshold for known metric");
            assert!(
                (0.0..=1.0).contains(&min),
                "{id} threshold out of range: {min}"
            );
        }
        assert!(thresholds.min_for("unknown_metric").is_none());
        assert!(thresholds.drift_rate_delta_max > 0.0);
    }
}
