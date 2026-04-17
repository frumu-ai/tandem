#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan_package::{
        ApprovalMatrix, ApprovalMode, AuditScope, BudgetEnforcement, CommunicationModel,
        ContextObject, ContextObjectProvenance, ContextObjectScope, ContextValidationStatus,
        CredentialBindingRef, CredentialEnvelope, CrossRoutineVisibility, DataScope,
        DependencyResolution, DependencyResolutionStrategy, FinalArtifactVisibility,
        InterRoutinePolicy, IntermediateArtifactVisibility, ManualTriggerRecord,
        ManualTriggerSource, MidRoutineConnectorFailureMode, MissionContextScope,
        MissionDefinition, PartialFailureMode, PeerVisibility, PlanDiff, PlanDiffChangeType,
        PlanDiffChangedField, PlanDiffSummary, PlanLifecycleState, PlanOwner, PrecedenceLogEntry,
        PrecedenceSourceTier, ReentryPoint, RoutinePackage, RoutineSemanticKind,
        RunHistoryVisibility, StepPackage, SuccessCriteria, SuccessCriteriaEvaluationStatus,
        SuccessCriteriaSubjectKind, TriggerDefinition,
    };

    fn sample_plan() -> PlanPackage {
        PlanPackage {
            plan_id: "plan_123".to_string(),
            plan_revision: 1,
            lifecycle_state: PlanLifecycleState::Preview,
            owner: PlanOwner {
                owner_id: "workflow_planner".to_string(),
                scope: "workspace".to_string(),
                audience: "internal".to_string(),
            },
            mission: MissionDefinition {
                goal: "Test plan".to_string(),
                summary: None,
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
            overlap_policy: None,
            routine_graph: vec![RoutinePackage {
                routine_id: "routine_a".to_string(),
                semantic_kind: RoutineSemanticKind::Mixed,
                trigger: TriggerDefinition {
                    trigger_type: TriggerKind::Manual,
                    schedule: None,
                    timezone: None,
                },
                dependencies: Vec::new(),
                dependency_resolution: DependencyResolution {
                    strategy: DependencyResolutionStrategy::TopologicalSequential,
                    partial_failure_mode: PartialFailureMode::PauseDownstreamOnly,
                    reentry_point: ReentryPoint::FailedStep,
                    mid_routine_connector_failure: MidRoutineConnectorFailureMode::SurfaceAndPause,
                },
                connector_resolution: Default::default(),
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
                    step_id: "step_a".to_string(),
                    label: "Step A".to_string(),
                    kind: "analysis".to_string(),
                    action: "Do work".to_string(),
                    inputs: Vec::new(),
                    outputs: Vec::new(),
                    dependencies: Vec::new(),
                    context_reads: Vec::new(),
                    context_writes: vec!["ctx:routine_a:step_a:artifact.md".to_string()],
                    connector_requirements: Vec::new(),
                    model_policy: Default::default(),
                    approval_policy: ApprovalMode::InternalOnly,
                    success_criteria: SuccessCriteria::default(),
                    failure_policy: Default::default(),
                    retry_policy: Default::default(),
                    artifacts: vec!["artifact.md".to_string()],
                    provenance: None,
                    notes: None,
                }],
            }],
            connector_intents: Vec::new(),
            connector_bindings: Vec::new(),
            connector_binding_resolution: None,
            model_routing_resolution: None,
            credential_envelopes: vec![CredentialEnvelope {
                routine_id: "routine_a".to_string(),
                entitled_connectors: Vec::new(),
                denied_connectors: Vec::new(),
                envelope_issued_at: None,
                envelope_expires_at: None,
                issuing_authority: Some("engine".to_string()),
            }],
            context_objects: vec![ContextObject {
                context_object_id: "ctx:routine_a:step_a:artifact.md".to_string(),
                name: "Step A handoff".to_string(),
                kind: "step_output_handoff".to_string(),
                scope: ContextObjectScope::Handoff,
                owner_routine_id: "routine_a".to_string(),
                producer_step_id: Some("step_a".to_string()),
                declared_consumers: vec!["routine_a".to_string()],
                artifact_ref: Some("artifact.md".to_string()),
                data_scope_refs: vec!["knowledge/workflows/drafts/**".to_string()],
                freshness_window_hours: None,
                validation_status: ContextValidationStatus::Pending,
                provenance: ContextObjectProvenance {
                    plan_id: "plan_123".to_string(),
                    routine_id: "routine_a".to_string(),
                    step_id: Some("step_a".to_string()),
                },
                summary: None,
            }],
            metadata: None,
        }
    }

    #[test]
    fn derives_success_criteria_evaluation_report() {
        let mut plan = sample_plan();
        plan.success_criteria = SuccessCriteria {
            minimum_viable_completion: Some("Define plan-level completion".to_string()),
            ..SuccessCriteria::default()
        };
        plan.routine_graph[0].success_criteria = SuccessCriteria::default();
        plan.routine_graph[0].steps[0].success_criteria = SuccessCriteria {
            required_artifacts: vec!["artifact.md".to_string()],
            ..SuccessCriteria::default()
        };

        let report = validate_plan_package(&plan);
        let evaluation = report
            .success_criteria_evaluation
            .expect("success criteria evaluation");

        assert_eq!(evaluation.total_subjects, 3);
        assert_eq!(evaluation.defined_count, 2);
        assert_eq!(evaluation.missing_count, 1);
        assert!(evaluation.entries.iter().any(|entry| {
            entry.subject == SuccessCriteriaSubjectKind::Plan
                && entry.status == SuccessCriteriaEvaluationStatus::Defined
        }));
        assert!(evaluation.entries.iter().any(|entry| {
            entry.subject == SuccessCriteriaSubjectKind::Routine
                && entry.status == SuccessCriteriaEvaluationStatus::Missing
        }));
        assert!(evaluation.entries.iter().any(|entry| {
            entry.subject == SuccessCriteriaSubjectKind::Step
                && entry.status == SuccessCriteriaEvaluationStatus::Defined
        }));
    }

    #[test]
    fn flags_unresolved_required_connectors() {
        let mut plan = sample_plan();
        plan.connector_intents
            .push(crate::plan_package::ConnectorIntent {
                capability: "github".to_string(),
                why: "Needed".to_string(),
                required: true,
                degraded_mode_allowed: false,
            });

        let report = validate_plan_package(&plan);

        assert_eq!(report.blocker_count, 1);
        assert!(!report.ready_for_apply);
        assert_eq!(report.issues[0].code, "required_connector_unresolved");
    }

    #[test]
    fn flags_missing_step_dependency() {
        let mut plan = sample_plan();
        plan.routine_graph[0].steps[0]
            .dependencies
            .push("missing_step".to_string());

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "missing_step_dependency"));
        assert!(report.blocker_count >= 1);
        assert_eq!(report.validation_state.dependencies_resolvable, Some(false));
    }

    #[test]
    fn flags_duplicate_connector_bindings() {
        let mut plan = sample_plan();
        plan.connector_bindings
            .push(crate::plan_package::ConnectorBinding {
                capability: "github".to_string(),
                binding_type: "mcp_server".to_string(),
                binding_id: "binding_1".to_string(),
                allowlist_pattern: None,
                status: "mapped".to_string(),
            });
        plan.connector_bindings
            .push(crate::plan_package::ConnectorBinding {
                capability: "github".to_string(),
                binding_type: "mcp_server".to_string(),
                binding_id: "binding_2".to_string(),
                allowlist_pattern: None,
                status: "mapped".to_string(),
            });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "duplicate_connector_binding"));
        assert!(report.blocker_count >= 1);
    }

    #[test]
    fn flags_mapped_connector_binding_missing_metadata() {
        let mut plan = sample_plan();
        plan.connector_bindings
            .push(crate::plan_package::ConnectorBinding {
                capability: "github".to_string(),
                binding_type: String::new(),
                binding_id: String::new(),
                allowlist_pattern: None,
                status: "mapped".to_string(),
            });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "mapped_connector_binding_missing_metadata"));
        assert!(report.blocker_count >= 1);
    }

    #[test]
    fn flags_missing_approval_policy() {
        let mut plan = sample_plan();
        plan.approval_policy = None;

        let report = validate_plan_package(&plan);

        assert_eq!(report.blocker_count, 1);
        assert_eq!(report.issues[0].code, "approval_policy_missing");
        assert_eq!(report.validation_state.approvals_complete, Some(false));
    }

    #[test]
    fn flags_invalid_budget_hard_limit_behavior() {
        let mut plan = sample_plan();
        plan.budget_enforcement = Some(BudgetEnforcement {
            cost_tracking_unit: None,
            soft_warning_threshold: None,
            hard_limit_behavior: Some("freeze".to_string()),
            partial_result_preservation: None,
            daily_and_weekly_enforcement: None,
        });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "invalid_budget_hard_limit_behavior"));
        assert!(!report.ready_for_apply);
    }

    #[test]
    fn flags_artifact_handoff_validation_disabled() {
        let mut plan = sample_plan();
        plan.inter_routine_policy
            .as_mut()
            .expect("sample plan must include inter_routine_policy")
            .artifact_handoff_validation = false;

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "artifact_handoff_validation_required"));
        assert_eq!(
            report.validation_state.compartmentalized_activation_ready,
            Some(false)
        );
    }

    #[test]
    fn flags_precedence_log_source_value_mismatch() {
        let mut plan = sample_plan();
        plan.precedence_log.push(PrecedenceLogEntry {
            path: "budget_policy.max_cost_per_run_usd".to_string(),
            compiler_default: None,
            user_override: None,
            approved_plan_state: None,
            resolved_value: Some(serde_json::json!(4.0)),
            source_tier: PrecedenceSourceTier::UserOverride,
            conflict_detected: true,
            resolution_rule: "approved_plan_state > user_override > compiler_default".to_string(),
            resolved_at: Some("2026-03-27T09:12:00Z".to_string()),
        });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "precedence_log_source_value_missing"));
    }

    #[test]
    fn flags_plan_diff_revision_mismatch() {
        let mut plan = sample_plan();
        plan.plan_revision = 4;
        plan.plan_diff = Some(PlanDiff {
            from_revision: 3,
            to_revision: 3,
            changed_fields: vec![PlanDiffChangedField {
                path: "routine_graph[0].trigger.schedule".to_string(),
                change_type: PlanDiffChangeType::Update,
                old_value: Some(serde_json::json!("0 9 * * *")),
                new_value: Some(serde_json::json!("0 10 * * *")),
                requires_revalidation: true,
                requires_reapproval: false,
                breaking: false,
            }],
            summary: PlanDiffSummary {
                changed_count: 1,
                breaking_count: 0,
                revalidation_required: true,
                reapproval_required: false,
            },
        });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "plan_diff_revision_order_invalid"));
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "plan_diff_revision_mismatch"));
    }

    #[test]
    fn flags_plan_diff_summary_reapproval_mismatch() {
        let mut plan = sample_plan();
        plan.plan_revision = 4;
        plan.plan_diff = Some(PlanDiff {
            from_revision: 3,
            to_revision: 4,
            changed_fields: vec![PlanDiffChangedField {
                path: "approval_policy.connector_mutations".to_string(),
                change_type: PlanDiffChangeType::Update,
                old_value: Some(serde_json::json!("internal_only")),
                new_value: Some(serde_json::json!("approval_required")),
                requires_revalidation: true,
                requires_reapproval: true,
                breaking: true,
            }],
            summary: PlanDiffSummary {
                changed_count: 1,
                breaking_count: 1,
                revalidation_required: true,
                reapproval_required: false,
            },
        });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "plan_diff_summary_reapproval_mismatch"));
        assert!(report.blocker_count >= 1);
    }

    #[test]
    fn flags_manual_trigger_record_for_unknown_routine() {
        let mut plan = sample_plan();
        plan.manual_trigger_record = Some(ManualTriggerRecord {
            trigger_id: "mt_01".to_string(),
            plan_id: "plan_123".to_string(),
            plan_revision: 1,
            routine_id: "missing_routine".to_string(),
            triggered_by: "user_123".to_string(),
            trigger_source: ManualTriggerSource::Calendar,
            dry_run: true,
            approval_policy_snapshot: Some(ApprovalMatrix {
                internal_reports: Some(ApprovalMode::AutoApproved),
                ..ApprovalMatrix::default()
            }),
            connector_binding_snapshot: Vec::new(),
            triggered_at: "2026-03-27T09:15:00Z".to_string(),
            run_id: Some("run_abc123".to_string()),
            outcome: Some("paused_after_validation".to_string()),
            artifacts_produced: vec!["artifact.md".to_string()],
            notes: Some("Dry-run from calendar entry".to_string()),
        });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "manual_trigger_record_invalid_routine"));
    }

    #[test]
    fn flags_manual_trigger_record_revision_mismatch() {
        let mut plan = sample_plan();
        plan.manual_trigger_record = Some(ManualTriggerRecord {
            trigger_id: "mt_02".to_string(),
            plan_id: "plan_123".to_string(),
            plan_revision: 2,
            routine_id: "routine_a".to_string(),
            triggered_by: "user_123".to_string(),
            trigger_source: ManualTriggerSource::Calendar,
            dry_run: false,
            approval_policy_snapshot: Some(ApprovalMatrix {
                internal_reports: Some(ApprovalMode::AutoApproved),
                ..ApprovalMatrix::default()
            }),
            connector_binding_snapshot: Vec::new(),
            triggered_at: "2026-03-27T09:20:00Z".to_string(),
            run_id: Some("run_def456".to_string()),
            outcome: Some("queued".to_string()),
            artifacts_produced: Vec::new(),
            notes: Some("Triggered from calendar entry".to_string()),
        });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "manual_trigger_record_plan_revision_mismatch"));
        assert!(report.blocker_count >= 1);
    }

    #[test]
    fn flags_empty_writable_scope() {
        let mut plan = sample_plan();
        plan.routine_graph[0].data_scope.writable_paths.clear();

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "empty_writable_scope"));
        assert!(report.blocker_count >= 1);
        assert_eq!(report.validation_state.data_scopes_valid, Some(false));
    }

    #[test]
    fn flags_full_plan_scope_without_justification() {
        let mut plan = sample_plan();
        plan.routine_graph[0].data_scope.mission_context_scope =
            crate::plan_package::MissionContextScope::FullPlan;
        plan.routine_graph[0]
            .data_scope
            .mission_context_justification = None;

        let report = validate_plan_package(&plan);

        assert_eq!(report.blocker_count, 1);
        assert_eq!(
            report.issues[0].code,
            "full_plan_scope_requires_justification"
        );
        assert_eq!(
            report.validation_state.mission_context_scopes_valid,
            Some(false)
        );
    }

    #[test]
    fn flags_named_roles_visibility_without_roles() {
        let mut plan = sample_plan();
        plan.routine_graph[0].audit_scope.run_history_visibility =
            crate::plan_package::RunHistoryVisibility::NamedRoles;
        plan.routine_graph[0].audit_scope.named_audit_roles.clear();

        let report = validate_plan_package(&plan);

        assert_eq!(report.blocker_count, 1);
        assert_eq!(report.issues[0].code, "named_audit_roles_missing");
        assert_eq!(report.validation_state.audit_scopes_valid, Some(false));
    }

    #[test]
    fn flags_missing_inter_routine_policy() {
        let mut plan = sample_plan();
        plan.inter_routine_policy = None;

        let report = validate_plan_package(&plan);

        assert_eq!(report.blocker_count, 1);
        assert_eq!(report.issues[0].code, "inter_routine_policy_missing");
        assert_eq!(
            report.validation_state.inter_routine_policy_complete,
            Some(false)
        );
    }

    #[test]
    fn flags_shared_memory_without_justification() {
        let mut plan = sample_plan();
        let policy = plan.inter_routine_policy.as_mut().expect("policy");
        policy.shared_memory_access = true;
        policy.shared_memory_justification = None;

        let report = validate_plan_package(&plan);

        assert_eq!(report.blocker_count, 1);
        assert_eq!(
            report.issues[0].code,
            "shared_memory_requires_justification"
        );
        assert_eq!(
            report.validation_state.inter_routine_policy_complete,
            Some(false)
        );
    }

    #[test]
    fn flags_cross_routine_scope_overlap() {
        let mut plan = sample_plan();
        plan.routine_graph.push(RoutinePackage {
            routine_id: "routine_b".to_string(),
            semantic_kind: RoutineSemanticKind::Mixed,
            trigger: TriggerDefinition {
                trigger_type: TriggerKind::Manual,
                schedule: None,
                timezone: None,
            },
            dependencies: Vec::new(),
            dependency_resolution: DependencyResolution {
                strategy: DependencyResolutionStrategy::TopologicalSequential,
                partial_failure_mode: PartialFailureMode::PauseDownstreamOnly,
                reentry_point: ReentryPoint::FailedStep,
                mid_routine_connector_failure: MidRoutineConnectorFailureMode::SurfaceAndPause,
            },
            connector_resolution: Default::default(),
            data_scope: DataScope {
                readable_paths: vec!["knowledge/workflows/drafts/**".to_string()],
                writable_paths: vec!["knowledge/workflows/proof/**".to_string()],
                denied_paths: vec!["credentials/**".to_string()],
                cross_routine_visibility: CrossRoutineVisibility::None,
                mission_context_scope: MissionContextScope::GoalAndOwnRoutine,
                mission_context_justification: None,
            },
            audit_scope: AuditScope {
                run_history_visibility: RunHistoryVisibility::PlanOwner,
                named_audit_roles: Vec::new(),
                intermediate_artifact_visibility: IntermediateArtifactVisibility::RoutineOnly,
                final_artifact_visibility: FinalArtifactVisibility::PlanOwner,
            },
            success_criteria: SuccessCriteria::default(),
            steps: vec![StepPackage {
                step_id: "step_b".to_string(),
                label: "Step B".to_string(),
                kind: "analysis".to_string(),
                action: "Do more work".to_string(),
                inputs: Vec::new(),
                outputs: Vec::new(),
                dependencies: Vec::new(),
                context_reads: Vec::new(),
                context_writes: Vec::new(),
                connector_requirements: Vec::new(),
                model_policy: Default::default(),
                approval_policy: ApprovalMode::InternalOnly,
                success_criteria: SuccessCriteria::default(),
                failure_policy: Default::default(),
                retry_policy: Default::default(),
                artifacts: Vec::new(),
                provenance: None,
                notes: None,
            }],
        });
        plan.credential_envelopes.push(CredentialEnvelope {
            routine_id: "routine_b".to_string(),
            entitled_connectors: Vec::new(),
            denied_connectors: Vec::new(),
            envelope_issued_at: None,
            envelope_expires_at: None,
            issuing_authority: Some("engine".to_string()),
        });

        let report = validate_plan_package(&plan);

        assert!(!report.ready_for_activation);
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "cross_routine_scope_overlap"));
        assert_eq!(
            report.validation_state.compartmentalized_activation_ready,
            Some(false)
        );
    }

    #[test]
    fn flags_denied_path_overlapping_output_root() {
        let mut plan = sample_plan();
        plan.output_roots = Some(crate::plan_package::OutputRoots {
            plan: Some("knowledge/workflows/plan/".to_string()),
            history: Some("knowledge/workflows/run-history/".to_string()),
            proof: Some("knowledge/workflows/proof/".to_string()),
            drafts: Some("knowledge/workflows/drafts/".to_string()),
        });
        plan.routine_graph[0].data_scope.denied_paths =
            vec!["knowledge/workflows/drafts/**".to_string()];

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "denied_path_overlaps_output_root"));
        assert_eq!(
            report.validation_state.compartmentalized_activation_ready,
            Some(false)
        );
    }

    #[test]
    fn flags_writable_path_outside_output_roots() {
        let mut plan = sample_plan();
        plan.output_roots = Some(crate::plan_package::OutputRoots {
            plan: Some("knowledge/workflows/plan/".to_string()),
            history: Some("knowledge/workflows/run-history/".to_string()),
            proof: Some("knowledge/workflows/proof/".to_string()),
            drafts: Some("knowledge/workflows/drafts/".to_string()),
        });
        plan.routine_graph[0].data_scope.writable_paths = vec!["/tmp/**".to_string()];

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "writable_path_outside_output_roots"));
        assert_eq!(report.validation_state.data_scopes_valid, Some(false));
    }

    #[test]
    fn allows_writable_path_within_output_root_subtree() {
        let mut plan = sample_plan();
        plan.output_roots = Some(crate::plan_package::OutputRoots {
            plan: Some("knowledge/workflows/plan/".to_string()),
            history: Some("knowledge/workflows/run-history/".to_string()),
            proof: Some("knowledge/workflows/proof/".to_string()),
            drafts: Some("knowledge/workflows/drafts/".to_string()),
        });
        plan.routine_graph[0].data_scope.writable_paths =
            vec!["knowledge/workflows/plan/routine_a/**".to_string()];

        let report = validate_plan_package(&plan);

        assert!(!report
            .issues
            .iter()
            .any(|issue| issue.code == "writable_path_outside_output_roots"));
        assert_eq!(report.validation_state.data_scopes_valid, Some(true));
    }

    #[test]
    fn flags_missing_credential_envelope() {
        let mut plan = sample_plan();
        plan.credential_envelopes.clear();

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "credential_envelope_missing"));
        assert_eq!(
            report.validation_state.credential_envelopes_valid,
            Some(false)
        );
    }

    #[test]
    fn flags_shared_credential_envelope_entry() {
        let mut plan = sample_plan();
        plan.connector_bindings
            .push(crate::plan_package::ConnectorBinding {
                capability: "github".to_string(),
                binding_type: "mcp_server".to_string(),
                binding_id: "binding_shared".to_string(),
                allowlist_pattern: None,
                status: "mapped".to_string(),
            });
        plan.routine_graph.push(RoutinePackage {
            routine_id: "routine_b".to_string(),
            semantic_kind: RoutineSemanticKind::Mixed,
            trigger: TriggerDefinition {
                trigger_type: TriggerKind::Manual,
                schedule: None,
                timezone: None,
            },
            dependencies: Vec::new(),
            dependency_resolution: DependencyResolution {
                strategy: DependencyResolutionStrategy::TopologicalSequential,
                partial_failure_mode: PartialFailureMode::PauseDownstreamOnly,
                reentry_point: ReentryPoint::FailedStep,
                mid_routine_connector_failure: MidRoutineConnectorFailureMode::SurfaceAndPause,
            },
            connector_resolution: Default::default(),
            data_scope: DataScope {
                readable_paths: vec!["mission.goal".to_string()],
                writable_paths: vec!["knowledge/workflows/proof/**".to_string()],
                denied_paths: vec!["credentials/**".to_string()],
                cross_routine_visibility: CrossRoutineVisibility::None,
                mission_context_scope: MissionContextScope::GoalAndOwnRoutine,
                mission_context_justification: None,
            },
            audit_scope: AuditScope {
                run_history_visibility: RunHistoryVisibility::PlanOwner,
                named_audit_roles: Vec::new(),
                intermediate_artifact_visibility: IntermediateArtifactVisibility::RoutineOnly,
                final_artifact_visibility: FinalArtifactVisibility::PlanOwner,
            },
            success_criteria: SuccessCriteria::default(),
            steps: vec![StepPackage {
                step_id: "step_b".to_string(),
                label: "Step B".to_string(),
                kind: "analysis".to_string(),
                action: "Do more work".to_string(),
                inputs: Vec::new(),
                outputs: Vec::new(),
                dependencies: Vec::new(),
                context_reads: Vec::new(),
                context_writes: Vec::new(),
                connector_requirements: vec![crate::plan_package::ConnectorRequirement {
                    capability: "github".to_string(),
                    required: true,
                }],
                model_policy: Default::default(),
                approval_policy: ApprovalMode::InternalOnly,
                success_criteria: SuccessCriteria::default(),
                failure_policy: Default::default(),
                retry_policy: Default::default(),
                artifacts: Vec::new(),
                provenance: None,
                notes: None,
            }],
        });
        plan.credential_envelopes = vec![
            CredentialEnvelope {
                routine_id: "routine_a".to_string(),
                entitled_connectors: vec![CredentialBindingRef {
                    capability: "github".to_string(),
                    binding_id: "binding_shared".to_string(),
                }],
                denied_connectors: Vec::new(),
                envelope_issued_at: None,
                envelope_expires_at: None,
                issuing_authority: Some("engine".to_string()),
            },
            CredentialEnvelope {
                routine_id: "routine_b".to_string(),
                entitled_connectors: vec![CredentialBindingRef {
                    capability: "github".to_string(),
                    binding_id: "binding_shared".to_string(),
                }],
                denied_connectors: Vec::new(),
                envelope_issued_at: None,
                envelope_expires_at: None,
                issuing_authority: Some("engine".to_string()),
            },
        ];

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "shared_credential_envelope_entry"));
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "credential_leakage_attempt"));
        assert_eq!(
            report.validation_state.credential_envelopes_valid,
            Some(false)
        );
    }

    #[test]
    fn flags_credential_envelope_entitlement_mismatch() {
        let mut plan = sample_plan();
        plan.connector_bindings
            .push(crate::plan_package::ConnectorBinding {
                capability: "github".to_string(),
                binding_type: "mcp_server".to_string(),
                binding_id: "binding_github".to_string(),
                allowlist_pattern: None,
                status: "mapped".to_string(),
            });
        plan.routine_graph[0].steps[0].connector_requirements.push(
            crate::plan_package::ConnectorRequirement {
                capability: "github".to_string(),
                required: true,
            },
        );
        plan.credential_envelopes[0].entitled_connectors.clear();

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "credential_envelope_entitlements_mismatch"));
        assert_eq!(
            report.validation_state.credential_envelopes_valid,
            Some(false)
        );
    }

    #[test]
    fn flags_unknown_credential_envelope_binding() {
        let mut plan = sample_plan();
        plan.credential_envelopes[0].entitled_connectors = vec![CredentialBindingRef {
            capability: "github".to_string(),
            binding_id: "missing_binding".to_string(),
        }];

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "credential_envelope_unknown_binding"));
        assert_eq!(
            report.validation_state.credential_envelopes_valid,
            Some(false)
        );
    }

    #[test]
    fn flags_context_object_invalid_routine_reference() {
        let mut plan = sample_plan();
        plan.context_objects[0].owner_routine_id = "missing_routine".to_string();

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "context_object_invalid_routine_reference"));
        assert_eq!(report.validation_state.context_objects_valid, Some(false));
    }

    #[test]
    fn flags_context_object_scope_leak() {
        let mut plan = sample_plan();
        plan.context_objects[0].declared_consumers =
            vec!["routine_a".to_string(), "routine_b".to_string()];
        plan.routine_graph.push(RoutinePackage {
            routine_id: "routine_b".to_string(),
            semantic_kind: RoutineSemanticKind::Mixed,
            trigger: TriggerDefinition {
                trigger_type: TriggerKind::Manual,
                schedule: None,
                timezone: None,
            },
            dependencies: Vec::new(),
            dependency_resolution: DependencyResolution {
                strategy: DependencyResolutionStrategy::TopologicalSequential,
                partial_failure_mode: PartialFailureMode::PauseDownstreamOnly,
                reentry_point: ReentryPoint::FailedStep,
                mid_routine_connector_failure: MidRoutineConnectorFailureMode::SurfaceAndPause,
            },
            connector_resolution: Default::default(),
            data_scope: DataScope {
                readable_paths: vec!["mission.goal".to_string()],
                writable_paths: vec!["knowledge/workflows/proof/**".to_string()],
                denied_paths: vec!["credentials/**".to_string()],
                cross_routine_visibility: CrossRoutineVisibility::None,
                mission_context_scope: MissionContextScope::GoalAndOwnRoutine,
                mission_context_justification: None,
            },
            audit_scope: AuditScope {
                run_history_visibility: RunHistoryVisibility::PlanOwner,
                named_audit_roles: Vec::new(),
                intermediate_artifact_visibility: IntermediateArtifactVisibility::RoutineOnly,
                final_artifact_visibility: FinalArtifactVisibility::PlanOwner,
            },
            success_criteria: SuccessCriteria::default(),
            steps: Vec::new(),
        });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "context_object_scope_leak"));
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "context_scope_escalation_attempt"));
        assert_eq!(report.validation_state.context_objects_valid, Some(false));
    }

    #[test]
    fn flags_context_object_invalid_data_scope_ref() {
        let mut plan = sample_plan();
        plan.context_objects[0].data_scope_refs = vec!["/tmp/**".to_string()];

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "context_object_invalid_data_scope_ref"));
        assert_eq!(report.validation_state.context_objects_valid, Some(false));
    }

    #[test]
    fn flags_context_object_invalid_freshness() {
        let mut plan = sample_plan();
        plan.context_objects[0].freshness_window_hours = Some(0);

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "context_object_invalid_freshness"));
        assert_eq!(report.validation_state.context_objects_valid, Some(false));
    }

    #[test]
    fn flags_context_object_invalid_validation_shape() {
        let mut plan = sample_plan();
        plan.context_objects[0].validation_status = ContextValidationStatus::Valid;

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "context_object_invalid_validation_shape"));
        assert_eq!(report.validation_state.context_objects_valid, Some(false));
    }

    #[test]
    fn flags_context_object_invalid_provenance() {
        let mut plan = sample_plan();
        plan.context_objects[0].provenance.plan_id = "wrong_plan".to_string();

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "context_object_invalid_provenance"));
        assert_eq!(report.validation_state.context_objects_valid, Some(false));
    }

    #[test]
    fn flags_context_object_invalid_seed_shape() {
        let mut plan = sample_plan();
        plan.context_objects[0].kind = "mission_goal".to_string();
        plan.context_objects[0].scope = ContextObjectScope::Handoff;
        plan.context_objects[0].producer_step_id = None;
        plan.context_objects[0].artifact_ref = None;
        plan.context_objects[0].data_scope_refs = vec!["knowledge/workflows/drafts/**".to_string()];
        plan.context_objects[0].provenance.step_id = None;

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "context_object_invalid_seed_shape"));
        assert_eq!(report.validation_state.context_objects_valid, Some(false));
    }

    #[test]
    fn flags_missing_context_object_ref() {
        let mut plan = sample_plan();
        plan.routine_graph[0].steps[0].context_reads = vec!["ctx:routine_a:missing".to_string()];

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "missing_context_object_ref"));
        assert_eq!(report.validation_state.context_objects_valid, Some(false));
    }

    #[test]
    fn flags_context_read_consumer_violation() {
        let mut plan = sample_plan();
        plan.routine_graph.push(RoutinePackage {
            routine_id: "routine_b".to_string(),
            semantic_kind: RoutineSemanticKind::Mixed,
            trigger: TriggerDefinition {
                trigger_type: TriggerKind::Manual,
                schedule: None,
                timezone: None,
            },
            dependencies: Vec::new(),
            dependency_resolution: DependencyResolution {
                strategy: DependencyResolutionStrategy::TopologicalSequential,
                partial_failure_mode: PartialFailureMode::PauseDownstreamOnly,
                reentry_point: ReentryPoint::FailedStep,
                mid_routine_connector_failure: MidRoutineConnectorFailureMode::SurfaceAndPause,
            },
            connector_resolution: Default::default(),
            data_scope: DataScope {
                readable_paths: vec!["mission.goal".to_string()],
                writable_paths: vec!["knowledge/workflows/proof/**".to_string()],
                denied_paths: vec!["credentials/**".to_string()],
                cross_routine_visibility: CrossRoutineVisibility::None,
                mission_context_scope: MissionContextScope::GoalAndOwnRoutine,
                mission_context_justification: None,
            },
            audit_scope: AuditScope {
                run_history_visibility: RunHistoryVisibility::PlanOwner,
                named_audit_roles: Vec::new(),
                intermediate_artifact_visibility: IntermediateArtifactVisibility::RoutineOnly,
                final_artifact_visibility: FinalArtifactVisibility::PlanOwner,
            },
            success_criteria: SuccessCriteria::default(),
            steps: vec![StepPackage {
                step_id: "step_b".to_string(),
                label: "Step B".to_string(),
                kind: "analysis".to_string(),
                action: "Do more work".to_string(),
                inputs: Vec::new(),
                outputs: Vec::new(),
                dependencies: Vec::new(),
                context_reads: vec!["ctx:routine_a:step_a:artifact.md".to_string()],
                context_writes: Vec::new(),
                connector_requirements: Vec::new(),
                model_policy: Default::default(),
                approval_policy: ApprovalMode::InternalOnly,
                success_criteria: SuccessCriteria::default(),
                failure_policy: Default::default(),
                retry_policy: Default::default(),
                artifacts: Vec::new(),
                provenance: None,
                notes: None,
            }],
        });
        plan.credential_envelopes.push(CredentialEnvelope {
            routine_id: "routine_b".to_string(),
            entitled_connectors: Vec::new(),
            denied_connectors: Vec::new(),
            envelope_issued_at: None,
            envelope_expires_at: None,
            issuing_authority: Some("engine".to_string()),
        });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "context_read_consumer_violation"));
        assert_eq!(report.validation_state.context_objects_valid, Some(false));
    }

    #[test]
    fn flags_cross_routine_prompt_injection_attempt() {
        let mut plan = sample_plan();
        plan.routine_graph.push(RoutinePackage {
            routine_id: "routine_b".to_string(),
            semantic_kind: RoutineSemanticKind::Mixed,
            trigger: TriggerDefinition {
                trigger_type: TriggerKind::Manual,
                schedule: None,
                timezone: None,
            },
            dependencies: Vec::new(),
            dependency_resolution: DependencyResolution {
                strategy: DependencyResolutionStrategy::TopologicalSequential,
                partial_failure_mode: PartialFailureMode::PauseDownstreamOnly,
                reentry_point: ReentryPoint::FailedStep,
                mid_routine_connector_failure: MidRoutineConnectorFailureMode::SurfaceAndPause,
            },
            connector_resolution: Default::default(),
            data_scope: DataScope {
                readable_paths: vec!["mission.goal".to_string()],
                writable_paths: vec!["knowledge/workflows/proof/**".to_string()],
                denied_paths: vec!["credentials/**".to_string()],
                cross_routine_visibility: CrossRoutineVisibility::None,
                mission_context_scope: MissionContextScope::GoalAndOwnRoutine,
                mission_context_justification: None,
            },
            audit_scope: AuditScope {
                run_history_visibility: RunHistoryVisibility::PlanOwner,
                named_audit_roles: Vec::new(),
                intermediate_artifact_visibility: IntermediateArtifactVisibility::RoutineOnly,
                final_artifact_visibility: FinalArtifactVisibility::PlanOwner,
            },
            success_criteria: SuccessCriteria::default(),
            steps: vec![StepPackage {
                step_id: "step_b".to_string(),
                label: "Step B".to_string(),
                kind: "analysis".to_string(),
                action: "Do more work".to_string(),
                inputs: Vec::new(),
                outputs: Vec::new(),
                dependencies: Vec::new(),
                context_reads: vec!["ctx:routine_a:step_a:artifact.md".to_string()],
                context_writes: Vec::new(),
                connector_requirements: Vec::new(),
                model_policy: Default::default(),
                approval_policy: ApprovalMode::InternalOnly,
                success_criteria: SuccessCriteria::default(),
                failure_policy: Default::default(),
                retry_policy: Default::default(),
                artifacts: Vec::new(),
                provenance: None,
                notes: None,
            }],
        });
        plan.credential_envelopes.push(CredentialEnvelope {
            routine_id: "routine_b".to_string(),
            entitled_connectors: Vec::new(),
            denied_connectors: Vec::new(),
            envelope_issued_at: None,
            envelope_expires_at: None,
            issuing_authority: Some("engine".to_string()),
        });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "cross_routine_prompt_injection_attempt"));
        assert_eq!(report.validation_state.context_objects_valid, Some(false));
    }

    #[test]
    fn flags_context_write_producer_mismatch() {
        let mut plan = sample_plan();
        plan.routine_graph[0].steps[0].context_writes = vec!["ctx:routine_a:missing".to_string()];
        plan.context_objects.push(ContextObject {
            context_object_id: "ctx:routine_a:missing".to_string(),
            name: "Seed".to_string(),
            kind: "workspace_environment".to_string(),
            scope: ContextObjectScope::Plan,
            owner_routine_id: "routine_a".to_string(),
            producer_step_id: None,
            declared_consumers: vec!["routine_a".to_string()],
            artifact_ref: None,
            data_scope_refs: vec!["mission.goal".to_string()],
            freshness_window_hours: None,
            validation_status: ContextValidationStatus::Pending,
            provenance: ContextObjectProvenance {
                plan_id: "plan_123".to_string(),
                routine_id: "routine_a".to_string(),
                step_id: None,
            },
            summary: None,
        });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "context_write_producer_mismatch"));
        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "direct_peer_invocation_attempt"));
        assert_eq!(report.validation_state.context_objects_valid, Some(false));
    }

    #[test]
    fn flags_cyclic_step_dependencies() {
        let mut plan = sample_plan();
        plan.routine_graph[0].steps[0].dependencies = vec!["step_b".to_string()];
        plan.routine_graph[0].steps.push(StepPackage {
            step_id: "step_b".to_string(),
            label: "Step B".to_string(),
            kind: "analysis".to_string(),
            action: "Do more work".to_string(),
            inputs: Vec::new(),
            outputs: Vec::new(),
            dependencies: vec!["step_a".to_string()],
            context_reads: Vec::new(),
            context_writes: Vec::new(),
            connector_requirements: Vec::new(),
            model_policy: Default::default(),
            approval_policy: ApprovalMode::InternalOnly,
            success_criteria: SuccessCriteria::default(),
            failure_policy: Default::default(),
            retry_policy: Default::default(),
            artifacts: Vec::new(),
            provenance: None,
            notes: None,
        });

        let report = validate_plan_package(&plan);

        assert!(report
            .issues
            .iter()
            .any(|issue| issue.code == "cyclic_step_dependencies"));
        assert_eq!(report.validation_state.dependencies_resolvable, Some(false));
    }
}
