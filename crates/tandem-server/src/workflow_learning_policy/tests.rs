use super::*;
use crate::automation_v2::types::{
    WorkflowLearningCandidate, WorkflowLearningCandidateKind, WorkflowLearningCandidateStatus,
};

fn candidate(kind: WorkflowLearningCandidateKind, confidence: f64) -> WorkflowLearningCandidate {
    WorkflowLearningCandidate {
        candidate_id: "wflearn-test".to_string(),
        workflow_id: "wf".to_string(),
        project_id: "proj".to_string(),
        source_run_id: "run".to_string(),
        kind,
        status: WorkflowLearningCandidateStatus::Proposed,
        confidence,
        summary: "test".to_string(),
        fingerprint: "fp".to_string(),
        node_id: None,
        node_kind: None,
        validator_family: None,
        evidence_refs: vec![],
        artifact_refs: vec![],
        proposed_memory_payload: None,
        proposed_revision_prompt: None,
        source_memory_id: None,
        promoted_memory_id: None,
        needs_plan_bundle: false,
        baseline_before: None,
        latest_observed_metrics: None,
        last_revision_session_id: None,
        run_ids: vec![],
        created_at_ms: 0,
        updated_at_ms: 0,
    }
}

fn metrics(
    sample_size: usize,
    completion_rate: f64,
    validation_pass_rate: f64,
    human_intervention_count: u64,
) -> WorkflowLearningMetricsSnapshot {
    WorkflowLearningMetricsSnapshot {
        sample_size,
        completion_rate,
        validation_pass_rate,
        human_intervention_count,
        ..Default::default()
    }
}

/// A policy with auto-apply enabled and permissive (but non-trivial) thresholds,
/// for exercising the AutoApply path.
fn auto_apply_policy() -> WorkflowLearningPromotionPolicy {
    WorkflowLearningPromotionPolicy {
        auto_apply_enabled: true,
        min_confidence: 0.8,
        min_baseline_sample_size: 5,
        max_human_intervention_rate: 0.0,
        ..WorkflowLearningPromotionPolicy::default()
    }
}

#[test]
fn default_policy_is_fail_closed_no_auto_apply() {
    let policy = WorkflowLearningPromotionPolicy::default();
    assert!(!policy.auto_apply_enabled);
    // Even a perfect candidate with ample evidence is held for human review.
    let decision = policy.evaluate_promotion(
        &candidate(WorkflowLearningCandidateKind::MemoryFact, 1.0),
        &metrics(100, 1.0, 1.0, 0),
    );
    assert!(!decision.is_auto_apply());
    assert_eq!(decision.reason_code(), "auto_apply_disabled");
}

#[test]
fn structural_patches_are_blocked_even_when_enabled() {
    let policy = auto_apply_policy();
    let decision = policy.evaluate_promotion(
        &candidate(WorkflowLearningCandidateKind::GraphPatch, 1.0),
        &metrics(100, 1.0, 1.0, 0),
    );
    assert!(matches!(decision, PromotionDecision::Block { .. }));
    assert_eq!(decision.reason_code(), "structural_change_requires_human");
}

#[test]
fn needs_plan_bundle_is_blocked_even_when_enabled() {
    let policy = auto_apply_policy();
    let mut c = candidate(WorkflowLearningCandidateKind::PromptPatch, 1.0);
    c.needs_plan_bundle = true;
    let decision = policy.evaluate_promotion(&c, &metrics(100, 1.0, 1.0, 0));
    assert!(matches!(decision, PromotionDecision::Block { .. }));
}

#[test]
fn low_confidence_requires_human() {
    let policy = auto_apply_policy();
    let decision = policy.evaluate_promotion(
        &candidate(WorkflowLearningCandidateKind::MemoryFact, 0.65),
        &metrics(100, 1.0, 1.0, 0),
    );
    assert_eq!(decision.reason_code(), "insufficient_confidence");
}

#[test]
fn insufficient_evidence_requires_human() {
    let policy = auto_apply_policy();
    let decision = policy.evaluate_promotion(
        &candidate(WorkflowLearningCandidateKind::MemoryFact, 0.95),
        &metrics(2, 1.0, 1.0, 0),
    );
    assert_eq!(decision.reason_code(), "insufficient_evidence");
}

#[test]
fn recent_human_steering_vetoes_auto_apply() {
    let policy = auto_apply_policy();
    // 1 of 10 runs needed human intervention -> rate 0.1 > ceiling 0.0.
    let decision = policy.evaluate_promotion(
        &candidate(WorkflowLearningCandidateKind::MemoryFact, 0.95),
        &metrics(10, 1.0, 1.0, 1),
    );
    assert_eq!(decision.reason_code(), "active_human_steering");
}

#[test]
fn auto_apply_when_all_gates_met() {
    let policy = auto_apply_policy();
    let decision = policy.evaluate_promotion(
        &candidate(WorkflowLearningCandidateKind::MemoryFact, 0.95),
        &metrics(20, 1.0, 1.0, 0),
    );
    assert!(decision.is_auto_apply());
    assert_eq!(decision.reason_code(), "thresholds_met");
}

#[test]
fn regression_insufficient_until_min_post_apply_sample() {
    let policy = WorkflowLearningPromotionPolicy::default();
    let baseline = metrics(10, 0.9, 0.9, 0);
    // Only 2 post-apply runs (< 3) -> no verdict yet, even with a sharp drop.
    let latest = metrics(12, 0.1, 0.1, 0);
    assert_eq!(
        policy.evaluate_regression(&baseline, &latest, 2),
        RegressionVerdict::Insufficient
    );
}

#[test]
fn regression_verdict_uses_post_apply_count_not_capped_sample_delta() {
    // Mature workflow: baseline and latest are both pinned at the rolling-window
    // cap (50), so a naive `latest - baseline` would be 0 forever. With an
    // explicit post-apply count above the minimum, a real drop is still caught.
    let policy = WorkflowLearningPromotionPolicy::default();
    let baseline = metrics(50, 0.9, 0.9, 0);
    let latest = metrics(50, 0.5, 0.9, 0);
    assert!(policy
        .evaluate_regression(&baseline, &latest, 8)
        .is_regressed());
}

#[test]
fn regression_detected_on_completion_rate_drop() {
    let policy = WorkflowLearningPromotionPolicy::default();
    let baseline = metrics(10, 0.9, 0.9, 0);
    let latest = metrics(15, 0.5, 0.9, 0);
    let verdict = policy.evaluate_regression(&baseline, &latest, 5);
    assert!(verdict.is_regressed());
    assert_eq!(
        match verdict {
            RegressionVerdict::Regressed { reason_code, .. } => reason_code,
            _ => "unexpected",
        },
        "completion_rate_regressed"
    );
}

#[test]
fn regression_detected_on_validation_pass_rate_drop() {
    let policy = WorkflowLearningPromotionPolicy::default();
    let baseline = metrics(10, 0.9, 0.9, 0);
    let latest = metrics(15, 0.9, 0.4, 0);
    let verdict = policy.evaluate_regression(&baseline, &latest, 5);
    assert!(verdict.is_regressed());
}

#[test]
fn healthy_when_metrics_hold_or_improve() {
    let policy = WorkflowLearningPromotionPolicy::default();
    let baseline = metrics(10, 0.9, 0.9, 0);
    let latest = metrics(15, 0.95, 0.92, 0);
    assert_eq!(
        policy.evaluate_regression(&baseline, &latest, 5),
        RegressionVerdict::Healthy
    );
}

#[test]
fn default_regression_matches_legacy_epsilon_behavior() {
    // The previous inline check used `metrics + f64::EPSILON < baseline`. An equal
    // rate must NOT count as a regression; a strictly lower one must.
    let policy = WorkflowLearningPromotionPolicy::default();
    let baseline = metrics(10, 0.9, 0.9, 0);
    let equal = metrics(15, 0.9, 0.9, 0);
    assert_eq!(
        policy.evaluate_regression(&baseline, &equal, 5),
        RegressionVerdict::Healthy
    );
}
