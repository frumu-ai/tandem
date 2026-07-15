// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Workflow Learning v1 production-validation and auto-apply policy (GCL-04 / TAN-44).
//!
//! Workflow Learning generates [`WorkflowLearningCandidate`]s from terminal runs
//! (memory facts, repair hints, prompt/graph patches). Today the only automatic
//! status transition is `Applied → Regressed`; promotion to `Applied` is entirely
//! human (the review endpoint). The thresholds for that regression check were
//! hardcoded inline, and there was no decided policy for *whether* a candidate may
//! be applied without a human.
//!
//! This module makes both decisions explicit and testable:
//!
//! - [`WorkflowLearningPromotionPolicy::evaluate_promotion`] decides whether a
//!   freshly proposed candidate may be auto-applied, requires human review, or is
//!   categorically blocked from auto-apply.
//! - [`WorkflowLearningPromotionPolicy::evaluate_regression`] decides whether an
//!   applied candidate has regressed against its baseline (the before/after gate).
//!
//! The policy **fails closed**: its [`Default`] keeps auto-apply *off* and uses the
//! exact regression thresholds the runtime already used, so routing the existing
//! status machine through it is behavior-preserving until an operator opts in.

use crate::automation_v2::types::{
    WorkflowLearningCandidate, WorkflowLearningCandidateKind, WorkflowLearningMetricsSnapshot,
};

/// Default minimum candidate confidence required before auto-apply is considered.
/// Matches the highest confidence the generator currently assigns (GraphPatch is
/// `0.80`, but GraphPatch is categorically blocked from auto-apply anyway).
pub const DEFAULT_MIN_AUTO_APPLY_CONFIDENCE: f64 = 0.8;
/// Default minimum evidence (recent terminal runs) before auto-apply is considered.
pub const DEFAULT_MIN_BASELINE_SAMPLE_SIZE: usize = 5;
/// Default ceiling on the share of recent runs that show human intervention before
/// auto-apply is vetoed. `0.0` means *any* recent human steering vetoes auto-apply.
pub const DEFAULT_MAX_HUMAN_INTERVENTION_RATE: f64 = 0.0;
/// Default post-apply runs required before a before/after regression verdict.
/// Preserves the previously-inlined `WORKFLOW_LEARNING_POST_APPLY_MIN_SAMPLE_SIZE`.
pub const DEFAULT_POST_APPLY_MIN_SAMPLE_SIZE: usize = 3;

/// Whether a proposed candidate may be auto-applied without human review.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromotionDecision {
    /// Apply automatically; thresholds and gates are all satisfied.
    AutoApply { reason_code: &'static str },
    /// Eligible kind, but auto-apply is disabled or a threshold is unmet — leave
    /// the candidate `Proposed` for a human to review (today's default path).
    RequireHumanReview {
        reason_code: &'static str,
        reason: String,
    },
    /// Categorically ineligible for auto-apply regardless of policy/thresholds
    /// (structural changes that rewrite the workflow graph or need a plan bundle).
    Block {
        reason_code: &'static str,
        reason: String,
    },
}

impl PromotionDecision {
    pub fn is_auto_apply(&self) -> bool {
        matches!(self, Self::AutoApply { .. })
    }

    pub fn reason_code(&self) -> &'static str {
        match self {
            Self::AutoApply { reason_code }
            | Self::RequireHumanReview { reason_code, .. }
            | Self::Block { reason_code, .. } => reason_code,
        }
    }
}

/// The before/after verdict for an applied candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegressionVerdict {
    /// Not enough post-apply runs yet to judge; hold the verdict.
    Insufficient,
    /// Metrics held or improved against baseline.
    Healthy,
    /// A guarded metric fell below baseline beyond the margin.
    Regressed {
        reason_code: &'static str,
        reason: String,
    },
}

impl RegressionVerdict {
    pub fn is_regressed(&self) -> bool {
        matches!(self, Self::Regressed { .. })
    }
}

/// Declarative policy for promoting and validating workflow-learning candidates.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkflowLearningPromotionPolicy {
    /// Master switch. Off by default: promotion stays human-driven (fail closed).
    pub auto_apply_enabled: bool,
    /// Minimum candidate confidence required for auto-apply.
    pub min_confidence: f64,
    /// Minimum recent-run sample size (evidence) required for auto-apply.
    pub min_baseline_sample_size: usize,
    /// Ceiling on the recent human-intervention rate before auto-apply is vetoed.
    pub max_human_intervention_rate: f64,
    /// Post-apply runs required before a regression verdict is rendered.
    pub post_apply_min_sample_size: usize,
    /// Margin applied to before/after metric comparisons (epsilon-safe).
    pub regression_margin: f64,
}

impl Default for WorkflowLearningPromotionPolicy {
    fn default() -> Self {
        Self {
            auto_apply_enabled: false,
            min_confidence: DEFAULT_MIN_AUTO_APPLY_CONFIDENCE,
            min_baseline_sample_size: DEFAULT_MIN_BASELINE_SAMPLE_SIZE,
            max_human_intervention_rate: DEFAULT_MAX_HUMAN_INTERVENTION_RATE,
            post_apply_min_sample_size: DEFAULT_POST_APPLY_MIN_SAMPLE_SIZE,
            regression_margin: f64::EPSILON,
        }
    }
}

fn env_bool(key: &str) -> Option<bool> {
    std::env::var(key)
        .ok()
        .and_then(|v| match v.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        })
}

fn env_f64(key: &str) -> Option<f64> {
    std::env::var(key).ok().and_then(|v| v.trim().parse().ok())
}

fn env_usize(key: &str) -> Option<usize> {
    std::env::var(key).ok().and_then(|v| v.trim().parse().ok())
}

impl WorkflowLearningPromotionPolicy {
    /// Resolve the policy from environment overrides, falling back to the
    /// fail-closed [`Default`]. Operators opt into auto-apply explicitly.
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            auto_apply_enabled: env_bool("TANDEM_WORKFLOW_LEARNING_AUTO_APPLY")
                .unwrap_or(defaults.auto_apply_enabled),
            min_confidence: env_f64("TANDEM_WORKFLOW_LEARNING_MIN_CONFIDENCE")
                .unwrap_or(defaults.min_confidence),
            min_baseline_sample_size: env_usize("TANDEM_WORKFLOW_LEARNING_MIN_BASELINE_SAMPLE")
                .unwrap_or(defaults.min_baseline_sample_size),
            max_human_intervention_rate: env_f64(
                "TANDEM_WORKFLOW_LEARNING_MAX_HUMAN_INTERVENTION_RATE",
            )
            .unwrap_or(defaults.max_human_intervention_rate),
            post_apply_min_sample_size: env_usize("TANDEM_WORKFLOW_LEARNING_POST_APPLY_MIN_SAMPLE")
                .unwrap_or(defaults.post_apply_min_sample_size),
            regression_margin: env_f64("TANDEM_WORKFLOW_LEARNING_REGRESSION_MARGIN")
                .unwrap_or(defaults.regression_margin),
        }
    }

    /// A candidate kind that rewrites workflow structure is never auto-applied:
    /// such changes always require a human, regardless of confidence or thresholds.
    fn kind_is_structural(kind: WorkflowLearningCandidateKind) -> bool {
        matches!(kind, WorkflowLearningCandidateKind::GraphPatch)
    }

    fn human_intervention_rate(metrics: &WorkflowLearningMetricsSnapshot) -> f64 {
        if metrics.sample_size == 0 {
            // No evidence is treated as maximal uncertainty, not zero risk.
            return 1.0;
        }
        metrics.human_intervention_count as f64 / metrics.sample_size as f64
    }

    /// Decide whether `candidate` may be auto-applied, given the workflow's current
    /// `metrics` snapshot (the same snapshot used as the baseline on apply).
    ///
    /// Precedence (fail closed):
    /// 1. structural change / needs plan bundle → `Block` (always human);
    /// 2. auto-apply disabled → `RequireHumanReview`;
    /// 3. confidence below threshold → `RequireHumanReview`;
    /// 4. insufficient evidence → `RequireHumanReview`;
    /// 5. recent human steering above ceiling → `RequireHumanReview`;
    /// 6. otherwise → `AutoApply`.
    pub fn evaluate_promotion(
        &self,
        candidate: &WorkflowLearningCandidate,
        metrics: &WorkflowLearningMetricsSnapshot,
    ) -> PromotionDecision {
        if Self::kind_is_structural(candidate.kind) || candidate.needs_plan_bundle {
            return PromotionDecision::Block {
                reason_code: "structural_change_requires_human",
                reason: "graph/structural patches and plan-bundle changes are never auto-applied"
                    .to_string(),
            };
        }

        if !self.auto_apply_enabled {
            return PromotionDecision::RequireHumanReview {
                reason_code: "auto_apply_disabled",
                reason: "auto-apply is disabled; candidate awaits human review".to_string(),
            };
        }

        if candidate.confidence + f64::EPSILON < self.min_confidence {
            return PromotionDecision::RequireHumanReview {
                reason_code: "insufficient_confidence",
                reason: format!(
                    "confidence {:.3} is below the auto-apply threshold {:.3}",
                    candidate.confidence, self.min_confidence
                ),
            };
        }

        if metrics.sample_size < self.min_baseline_sample_size {
            return PromotionDecision::RequireHumanReview {
                reason_code: "insufficient_evidence",
                reason: format!(
                    "sample size {} is below the minimum {} required for auto-apply",
                    metrics.sample_size, self.min_baseline_sample_size
                ),
            };
        }

        let intervention_rate = Self::human_intervention_rate(metrics);
        if intervention_rate > self.max_human_intervention_rate + f64::EPSILON {
            return PromotionDecision::RequireHumanReview {
                reason_code: "active_human_steering",
                reason: format!(
                    "human-intervention rate {:.3} exceeds the auto-apply ceiling {:.3}",
                    intervention_rate, self.max_human_intervention_rate
                ),
            };
        }

        PromotionDecision::AutoApply {
            reason_code: "thresholds_met",
        }
    }

    /// Render the before/after regression verdict for an applied candidate by
    /// comparing the `latest` metrics against the `baseline` captured at apply.
    ///
    /// `post_apply_sample_size` is the count of terminal runs that completed
    /// *after* the baseline was captured. It is passed in explicitly rather than
    /// derived from the snapshot sample sizes: both snapshots come from a rolling
    /// window (capped at 50 recent runs), so on a mature workflow
    /// `latest.sample_size - baseline.sample_size` is pinned at 0 and a candidate
    /// could never accumulate enough post-apply evidence to be judged.
    pub fn evaluate_regression(
        &self,
        baseline: &WorkflowLearningMetricsSnapshot,
        latest: &WorkflowLearningMetricsSnapshot,
        post_apply_sample_size: usize,
    ) -> RegressionVerdict {
        if post_apply_sample_size < self.post_apply_min_sample_size {
            return RegressionVerdict::Insufficient;
        }

        if latest.completion_rate + self.regression_margin < baseline.completion_rate {
            return RegressionVerdict::Regressed {
                reason_code: "completion_rate_regressed",
                reason: format!(
                    "completion rate fell from {:.3} to {:.3} after apply",
                    baseline.completion_rate, latest.completion_rate
                ),
            };
        }
        if latest.validation_pass_rate + self.regression_margin < baseline.validation_pass_rate {
            return RegressionVerdict::Regressed {
                reason_code: "validation_pass_rate_regressed",
                reason: format!(
                    "validation pass rate fell from {:.3} to {:.3} after apply",
                    baseline.validation_pass_rate, latest.validation_pass_rate
                ),
            };
        }

        RegressionVerdict::Healthy
    }
}

#[cfg(test)]
mod tests;
