// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::plan_package::{OverlapLogEntry, PlanPackage};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverlapDecision {
    Reuse,
    Merge,
    Fork,
    New,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverlapMatchLayer {
    Canonical,
    Semantic,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OverlapComparison {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_plan_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_plan_revision: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub match_layer: Option<OverlapMatchLayer>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similarity_score: Option<f64>,
    pub decision: OverlapDecision,
    pub requires_user_confirmation: bool,
    pub reason: String,
}

pub fn analyze_plan_overlap(
    candidate: &PlanPackage,
    prior_plans: &[PlanPackage],
) -> OverlapComparison {
    let candidate_hash = canonical_hash(candidate);
    if let Some(candidate_hash) = candidate_hash {
        if let Some(matched) = prior_plans
            .iter()
            .find(|prior| canonical_hash(prior).as_deref() == Some(candidate_hash.as_str()))
        {
            let exact_reuse = same_trigger(candidate, matched)
                && same_connector_capabilities(candidate, matched)
                && candidate.approval_policy == matched.approval_policy
                && candidate.owner.scope == matched.owner.scope;
            return OverlapComparison {
                matched_plan_id: Some(matched.plan_id.clone()),
                matched_plan_revision: Some(matched.plan_revision),
                match_layer: Some(OverlapMatchLayer::Canonical),
                similarity_score: None,
                decision: if exact_reuse {
                    OverlapDecision::Reuse
                } else {
                    OverlapDecision::Fork
                },
                requires_user_confirmation: false,
                reason: if exact_reuse {
                    "Canonical overlap hash matches and runtime shape is compatible.".to_string()
                } else {
                    "Canonical overlap hash matches, but trigger/policy shape differs enough to fork.".to_string()
                },
            };
        }
    }

    let threshold = candidate
        .overlap_policy
        .as_ref()
        .and_then(|policy| policy.semantic_identity.as_ref())
        .and_then(|identity| identity.similarity_threshold)
        .unwrap_or(0.85);

    let best = prior_plans
        .iter()
        .map(|prior| {
            (
                prior,
                goal_similarity_score(&candidate.mission.goal, &prior.mission.goal),
            )
        })
        .max_by(|(_, left), (_, right)| {
            left.partial_cmp(right).unwrap_or(std::cmp::Ordering::Equal)
        });

    if let Some((matched, score)) = best {
        if score >= threshold {
            let decision =
                if same_trigger(candidate, matched) && routine_id_overlap(candidate, matched) {
                    OverlapDecision::Merge
                } else {
                    OverlapDecision::Fork
                };
            return OverlapComparison {
                matched_plan_id: Some(matched.plan_id.clone()),
                matched_plan_revision: Some(matched.plan_revision),
                match_layer: Some(OverlapMatchLayer::Semantic),
                similarity_score: Some(score),
                decision,
                requires_user_confirmation: true,
                reason: "Goal similarity exceeds the configured threshold and should be surfaced for confirmation.".to_string(),
            };
        }
    }

    OverlapComparison {
        matched_plan_id: None,
        matched_plan_revision: None,
        match_layer: None,
        similarity_score: None,
        decision: OverlapDecision::New,
        requires_user_confirmation: false,
        reason: "No canonical or near-match overlap was detected.".to_string(),
    }
}

pub fn overlap_log_entry_from_analysis(
    analysis: &OverlapComparison,
    decided_by: &str,
    decided_at: &str,
) -> Option<OverlapLogEntry> {
    let matched_plan_id = analysis.matched_plan_id.clone()?;
    let matched_plan_revision = analysis.matched_plan_revision?;
    let match_layer = match analysis.match_layer.as_ref()? {
        OverlapMatchLayer::Canonical => "canonical".to_string(),
        OverlapMatchLayer::Semantic => "semantic".to_string(),
    };
    let decision = match analysis.decision {
        OverlapDecision::Reuse => "reuse".to_string(),
        OverlapDecision::Merge => "merge".to_string(),
        OverlapDecision::Fork => "fork".to_string(),
        OverlapDecision::New => "new".to_string(),
    };
    Some(OverlapLogEntry {
        matched_plan_id,
        matched_plan_revision,
        match_layer,
        similarity_score: analysis.similarity_score,
        decision,
        decided_by: decided_by.to_string(),
        decided_at: decided_at.to_string(),
    })
}

fn canonical_hash(plan: &PlanPackage) -> Option<String> {
    plan.overlap_policy
        .as_ref()
        .and_then(|policy| policy.exact_identity.as_ref())
        .and_then(|identity| identity.canonical_hash.clone())
}

fn same_trigger(left: &PlanPackage, right: &PlanPackage) -> bool {
    let left_triggers = left
        .routine_graph
        .iter()
        .map(|routine| {
            (
                &routine.routine_id,
                &routine.trigger.trigger_type,
                routine.trigger.schedule.as_deref(),
                routine.trigger.timezone.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    let right_triggers = right
        .routine_graph
        .iter()
        .map(|routine| {
            (
                &routine.routine_id,
                &routine.trigger.trigger_type,
                routine.trigger.schedule.as_deref(),
                routine.trigger.timezone.as_deref(),
            )
        })
        .collect::<Vec<_>>();
    left_triggers == right_triggers
}

fn same_connector_capabilities(left: &PlanPackage, right: &PlanPackage) -> bool {
    connector_capabilities(left) == connector_capabilities(right)
}

fn connector_capabilities(plan: &PlanPackage) -> BTreeSet<String> {
    plan.connector_intents
        .iter()
        .map(|intent| intent.capability.clone())
        .chain(
            plan.connector_bindings
                .iter()
                .map(|binding| binding.capability.clone()),
        )
        .collect()
}

fn routine_id_overlap(left: &PlanPackage, right: &PlanPackage) -> bool {
    let left_ids = left
        .routine_graph
        .iter()
        .map(|routine| routine.routine_id.as_str())
        .collect::<BTreeSet<_>>();
    right
        .routine_graph
        .iter()
        .any(|routine| left_ids.contains(routine.routine_id.as_str()))
}

fn goal_similarity_score(left: &str, right: &str) -> f64 {
    let left_tokens = normalized_goal_tokens(left);
    let right_tokens = normalized_goal_tokens(right);
    if left_tokens.is_empty() || right_tokens.is_empty() {
        return 0.0;
    }
    let intersection = left_tokens.intersection(&right_tokens).count() as f64;
    let union = left_tokens.union(&right_tokens).count() as f64;
    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn normalized_goal_tokens(goal: &str) -> BTreeSet<String> {
    goal.split(|ch: char| !ch.is_ascii_alphanumeric())
        .map(|token| token.trim().to_ascii_lowercase())
        .filter(|token| !token.is_empty())
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::plan_package::{
        ApprovalMatrix, ApprovalMode, AuditScope, CommunicationModel, CrossRoutineVisibility,
        DataScope, DependencyResolution, DependencyResolutionStrategy, FinalArtifactVisibility,
        InterRoutinePolicy, IntermediateArtifactVisibility, MidRoutineConnectorFailureMode,
        MissionContextScope, MissionDefinition, OverlapIdentity, OverlapPolicy, PeerVisibility,
        PlanLifecycleState, PlanOwner, ReentryPoint, RoutineConnectorResolution, RoutinePackage,
        RoutineSemanticKind, RunHistoryVisibility, SemanticIdentity, StepPackage, SuccessCriteria,
        TriggerDefinition, TriggerKind,
    };

    use super::*;

    fn sample_plan(plan_id: &str, goal: &str, canonical_hash: &str) -> PlanPackage {
        PlanPackage {
            plan_id: plan_id.to_string(),
            plan_revision: 1,
            lifecycle_state: PlanLifecycleState::Preview,
            owner: PlanOwner {
                owner_id: "workflow_planner".to_string(),
                scope: "workspace".to_string(),
                audience: "internal".to_string(),
            },
            mission: MissionDefinition {
                goal: goal.to_string(),
                summary: Some(goal.to_string()),
                domain: Some("workflow".to_string()),
            },
            success_criteria: SuccessCriteria::default(),
            budget_policy: None,
            budget_enforcement: None,
            approval_policy: Some(ApprovalMatrix {
                internal_reports: Some(ApprovalMode::AutoApproved),
                ..ApprovalMatrix::default()
            }),
            inter_routine_policy: Some(InterRoutinePolicy {
                communication_model: CommunicationModel::ArtifactOnly,
                shared_memory_access: false,
                shared_memory_justification: None,
                peer_visibility: PeerVisibility::DeclaredOutputsOnly,
                artifact_handoff_validation: true,
            }),
            trigger_policy: None,
            output_roots: None,
            precedence_log: Vec::new(),
            plan_diff: None,
            manual_trigger_record: None,
            validation_state: None,
            overlap_policy: Some(OverlapPolicy {
                exact_identity: Some(OverlapIdentity {
                    hash_version: Some(1),
                    canonical_hash: Some(canonical_hash.to_string()),
                    normalized_fields: vec!["goal".to_string()],
                }),
                semantic_identity: Some(SemanticIdentity {
                    similarity_model: Some("text-embedding-3-large".to_string()),
                    semantic_signature: None,
                    similarity_threshold: Some(0.5),
                }),
                overlap_log: Vec::new(),
            }),
            routine_graph: vec![RoutinePackage {
                routine_id: "routine_daily".to_string(),
                semantic_kind: RoutineSemanticKind::Mixed,
                trigger: TriggerDefinition {
                    trigger_type: TriggerKind::Manual,
                    schedule: None,
                    timezone: None,
                },
                dependencies: Vec::new(),
                dependency_resolution: DependencyResolution {
                    strategy: DependencyResolutionStrategy::TopologicalSequential,
                    partial_failure_mode:
                        crate::plan_package::PartialFailureMode::PauseDownstreamOnly,
                    reentry_point: ReentryPoint::FailedStep,
                    mid_routine_connector_failure: MidRoutineConnectorFailureMode::SurfaceAndPause,
                },
                connector_resolution: RoutineConnectorResolution::default(),
                data_scope: DataScope {
                    readable_paths: vec!["mission.goal".to_string()],
                    writable_paths: vec!["knowledge/workflows/drafts/**".to_string()],
                    denied_paths: vec!["credentials/**".to_string()],
                    cross_routine_visibility: CrossRoutineVisibility::None,
                    mission_context_scope: MissionContextScope::GoalAndOwnRoutine,
                    mission_context_justification: None,
                },
                audit_scope: AuditScope {
                    run_history_visibility: RunHistoryVisibility::PlanOwner,
                    named_audit_roles: Vec::new(),
                    intermediate_artifact_visibility: IntermediateArtifactVisibility::RoutineOnly,
                    final_artifact_visibility: FinalArtifactVisibility::DeclaredConsumers,
                },
                success_criteria: SuccessCriteria::default(),
                steps: vec![StepPackage {
                    step_id: "generate_report".to_string(),
                    label: "Generate report".to_string(),
                    kind: "report".to_string(),
                    action: "Generate report".to_string(),
                    inputs: Vec::new(),
                    outputs: vec!["generate_report:report_markdown".to_string()],
                    dependencies: Vec::new(),
                    context_reads: Vec::new(),
                    context_writes: Vec::new(),
                    connector_requirements: Vec::new(),
                    model_policy: Default::default(),
                    approval_policy: ApprovalMode::InternalOnly,
                    success_criteria: SuccessCriteria::default(),
                    failure_policy: Default::default(),
                    retry_policy: Default::default(),
                    artifacts: vec!["generate_report.artifact".to_string()],
                    provenance: None,
                    notes: Some(json!({"source":"test"}).to_string()),
                }],
            }],
            connector_intents: Vec::new(),
            connector_bindings: Vec::new(),
            connector_binding_resolution: None,
            model_routing_resolution: None,
            credential_envelopes: Vec::new(),
            context_objects: Vec::new(),
            metadata: None,
        }
    }

    #[test]
    fn exact_canonical_match_prefers_reuse() {
        let candidate = sample_plan("candidate", "Prepare a daily market report", "same-hash");
        let prior = sample_plan("prior", "Prepare a daily market report", "same-hash");

        let analysis = analyze_plan_overlap(&candidate, &[prior]);

        assert_eq!(analysis.match_layer, Some(OverlapMatchLayer::Canonical));
        assert_eq!(analysis.decision, OverlapDecision::Reuse);
        assert!(!analysis.requires_user_confirmation);
    }

    #[test]
    fn canonical_match_forks_on_policy_mismatch() {
        let mut candidate = sample_plan("candidate", "Prepare a daily market report", "same-hash");
        let prior = sample_plan("prior", "Prepare a daily market report", "same-hash");

        candidate.approval_policy = Some(ApprovalMatrix {
            internal_reports: Some(ApprovalMode::AutoApproved),
            public_posts: Some(ApprovalMode::ApprovalRequired),
            ..ApprovalMatrix::default()
        });

        let analysis = analyze_plan_overlap(&candidate, &[prior]);

        assert_eq!(analysis.match_layer, Some(OverlapMatchLayer::Canonical));
        assert_eq!(analysis.decision, OverlapDecision::Fork);
        assert!(!analysis.requires_user_confirmation);
    }

    #[test]
    fn near_match_requests_confirmation() {
        let candidate = sample_plan(
            "candidate",
            "Investigate a production bug and prepare a summary",
            "hash-a",
        );
        let prior = sample_plan(
            "prior",
            "Investigate production bug and prepare summary",
            "hash-b",
        );

        let analysis = analyze_plan_overlap(&candidate, &[prior]);

        assert_eq!(analysis.match_layer, Some(OverlapMatchLayer::Semantic));
        assert!(analysis.similarity_score.unwrap_or(0.0) >= 0.5);
        assert!(analysis.requires_user_confirmation);
        assert!(matches!(
            analysis.decision,
            OverlapDecision::Merge | OverlapDecision::Fork
        ));
    }

    #[test]
    fn non_match_is_new() {
        let candidate = sample_plan("candidate", "Launch a new reporting workflow", "hash-a");
        let prior = sample_plan("prior", "Rotate on-call support queue", "hash-b");

        let analysis = analyze_plan_overlap(&candidate, &[prior]);

        assert_eq!(analysis.match_layer, None);
        assert_eq!(analysis.decision, OverlapDecision::New);
        assert!(!analysis.requires_user_confirmation);
    }

    #[test]
    fn empty_prior_list_is_new() {
        let candidate = sample_plan("candidate", "Prepare a daily market report", "hash-a");

        let analysis = analyze_plan_overlap(&candidate, &[]);

        assert_eq!(analysis.decision, OverlapDecision::New);
        assert_eq!(analysis.matched_plan_id, None);
    }

    #[test]
    fn semantic_match_with_shared_trigger_and_routine_merges() {
        // Same routine ids + identical manual triggers: the near-match should
        // be folded into the existing plan rather than forked.
        let candidate = sample_plan(
            "candidate",
            "Prepare the daily market report for the desk",
            "hash-a",
        );
        let prior = sample_plan("prior", "Prepare the daily market report", "hash-b");

        let analysis = analyze_plan_overlap(&candidate, &[prior]);

        assert_eq!(analysis.match_layer, Some(OverlapMatchLayer::Semantic));
        assert_eq!(analysis.decision, OverlapDecision::Merge);
        assert!(analysis.requires_user_confirmation);
        assert_eq!(analysis.matched_plan_id.as_deref(), Some("prior"));
    }

    #[test]
    fn semantic_match_with_different_trigger_forks() {
        let candidate = sample_plan(
            "candidate",
            "Prepare the daily market report for the desk",
            "hash-a",
        );
        let mut prior = sample_plan("prior", "Prepare the daily market report", "hash-b");
        prior.routine_graph[0].trigger = TriggerDefinition {
            trigger_type: TriggerKind::Scheduled,
            schedule: Some("0 9 * * *".to_string()),
            timezone: Some("UTC".to_string()),
        };

        let analysis = analyze_plan_overlap(&candidate, &[prior]);

        assert_eq!(analysis.match_layer, Some(OverlapMatchLayer::Semantic));
        assert_eq!(analysis.decision, OverlapDecision::Fork);
        assert!(analysis.requires_user_confirmation);
    }

    #[test]
    fn similarity_below_configured_threshold_is_new() {
        // The goals share roughly half their tokens; raising the candidate's
        // threshold above that similarity must suppress the semantic match.
        let mut candidate = sample_plan(
            "candidate",
            "Prepare a daily market report for trading",
            "hash-a",
        );
        if let Some(policy) = candidate.overlap_policy.as_mut() {
            if let Some(identity) = policy.semantic_identity.as_mut() {
                identity.similarity_threshold = Some(0.95);
            }
        }
        let prior = sample_plan("prior", "Prepare a weekly compliance report", "hash-b");

        let analysis = analyze_plan_overlap(&candidate, &[prior]);

        assert_eq!(analysis.decision, OverlapDecision::New);
        assert_eq!(analysis.match_layer, None);
    }

    #[test]
    fn plans_without_overlap_policy_still_get_semantic_detection() {
        // No canonical hash on either side: detection falls through to the
        // semantic layer with the default 0.85 threshold.
        let mut candidate = sample_plan("candidate", "Prepare a daily market report", "ignored");
        candidate.overlap_policy = None;
        let mut prior = sample_plan("prior", "Prepare a daily market report", "ignored");
        prior.overlap_policy = None;

        let analysis = analyze_plan_overlap(&candidate, &[prior]);

        assert_eq!(analysis.match_layer, Some(OverlapMatchLayer::Semantic));
        assert!(analysis.similarity_score.unwrap_or(0.0) >= 0.85);
        assert!(analysis.requires_user_confirmation);
    }

    #[test]
    fn canonical_hash_does_not_match_priors_without_hashes() {
        let candidate = sample_plan("candidate", "Prepare a daily market report", "same-hash");
        let mut prior = sample_plan("prior", "Prepare a daily market report", "same-hash");
        prior.overlap_policy = None;

        let analysis = analyze_plan_overlap(&candidate, &[prior]);

        // The identical goal still matches — but through the semantic layer,
        // never by pretending the hashless prior had a canonical hash.
        assert_eq!(analysis.match_layer, Some(OverlapMatchLayer::Semantic));
    }

    #[test]
    fn best_scoring_prior_wins_among_multiple_candidates() {
        let candidate = sample_plan(
            "candidate",
            "Summarize weekly sales numbers into a report",
            "hash-a",
        );
        let distant = sample_plan("distant", "Rotate the on-call support queue", "hash-b");
        let close = sample_plan(
            "close",
            "Summarize weekly sales numbers into a short report",
            "hash-c",
        );

        let analysis = analyze_plan_overlap(&candidate, &[distant, close]);

        assert_eq!(analysis.matched_plan_id.as_deref(), Some("close"));
        assert_eq!(analysis.match_layer, Some(OverlapMatchLayer::Semantic));
    }

    #[test]
    fn overlap_log_entry_maps_layers_and_decisions_to_wire_strings() {
        let analysis = OverlapComparison {
            matched_plan_id: Some("prior".to_string()),
            matched_plan_revision: Some(3),
            match_layer: Some(OverlapMatchLayer::Semantic),
            similarity_score: Some(0.91),
            decision: OverlapDecision::Merge,
            requires_user_confirmation: true,
            reason: "test".to_string(),
        };

        let entry = overlap_log_entry_from_analysis(
            &analysis,
            "operator@example.com",
            "2026-06-11T00:00:00Z",
        )
        .expect("matched analysis produces a log entry");

        assert_eq!(entry.matched_plan_id, "prior");
        assert_eq!(entry.matched_plan_revision, 3);
        assert_eq!(entry.match_layer, "semantic");
        assert_eq!(entry.decision, "merge");
        assert_eq!(entry.similarity_score, Some(0.91));
        assert_eq!(entry.decided_by, "operator@example.com");
        assert_eq!(entry.decided_at, "2026-06-11T00:00:00Z");
    }

    #[test]
    fn overlap_log_entry_is_none_when_nothing_matched() {
        let analysis = OverlapComparison {
            matched_plan_id: None,
            matched_plan_revision: None,
            match_layer: None,
            similarity_score: None,
            decision: OverlapDecision::New,
            requires_user_confirmation: false,
            reason: "no match".to_string(),
        };

        assert!(overlap_log_entry_from_analysis(&analysis, "operator", "now").is_none());
    }
}
