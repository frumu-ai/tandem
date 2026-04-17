#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plan_package_roundtrips_preview_shape() {
        let package = PlanPackage {
            plan_id: "plan_0f6e8c".to_string(),
            plan_revision: 3,
            lifecycle_state: PlanLifecycleState::Preview,
            owner: PlanOwner {
                owner_id: "user".to_string(),
                scope: "workspace".to_string(),
                audience: "internal".to_string(),
            },
            mission: MissionDefinition {
                goal: "Operationalize a user goal".to_string(),
                summary: Some("Turn one mission into a governed multi-routine plan".to_string()),
                domain: Some("mixed".to_string()),
            },
            success_criteria: SuccessCriteria {
                required_artifacts: vec!["founder_brief.md".to_string()],
                minimum_viable_completion: Some("At least one usable routine graph".to_string()),
                minimum_output: None,
                freshness_window_hours: None,
            },
            budget_policy: Some(BudgetPolicy {
                max_cost_per_run_usd: Some(4.0),
                max_daily_cost_usd: Some(20.0),
                max_weekly_cost_usd: Some(60.0),
                token_ceiling_per_run: Some(40_000),
                cheap_model_preferred_for: vec!["search".to_string()],
                strong_model_reserved_for: vec!["final synthesis".to_string()],
            }),
            budget_enforcement: Some(BudgetEnforcement {
                cost_tracking_unit: Some(CostTrackingUnit {
                    method: Some("token_count × model_rate_per_token".to_string()),
                    recorded_fields: vec!["tokens_in".to_string(), "tokens_out".to_string()],
                    tracking_scope: vec!["step".to_string(), "plan_run".to_string()],
                }),
                soft_warning_threshold: Some(0.8),
                hard_limit_behavior: Some("pause_before_step".to_string()),
                partial_result_preservation: Some(true),
                daily_and_weekly_enforcement: None,
            }),
            approval_policy: Some(ApprovalMatrix {
                public_posts: Some(ApprovalMode::ApprovalRequired),
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
            trigger_policy: Some(TriggerPolicy {
                supported: vec![TriggerKind::Scheduled, TriggerKind::Manual],
            }),
            output_roots: Some(OutputRoots {
                plan: Some("knowledge/workflows/plan/".to_string()),
                history: Some("knowledge/workflows/run-history/".to_string()),
                proof: Some("knowledge/workflows/proof/".to_string()),
                drafts: Some("knowledge/workflows/drafts/".to_string()),
            }),
            precedence_log: vec![PrecedenceLogEntry {
                path: "budget_policy.max_cost_per_run_usd".to_string(),
                compiler_default: Some(json!(2.0)),
                user_override: Some(json!(4.0)),
                approved_plan_state: None,
                resolved_value: Some(json!(4.0)),
                source_tier: PrecedenceSourceTier::UserOverride,
                conflict_detected: true,
                resolution_rule: "approved_plan_state > user_override > compiler_default"
                    .to_string(),
                resolved_at: Some("2026-03-27T09:12:00Z".to_string()),
            }],
            plan_diff: Some(PlanDiff {
                from_revision: 2,
                to_revision: 3,
                changed_fields: vec![PlanDiffChangedField {
                    path: "routine_graph[0].trigger.schedule".to_string(),
                    change_type: PlanDiffChangeType::Update,
                    old_value: Some(json!("0 9 * * *")),
                    new_value: Some(json!("0 10 * * *")),
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
            }),
            manual_trigger_record: Some(ManualTriggerRecord {
                trigger_id: "mt_01HZY".to_string(),
                plan_id: "plan_0f6e8c".to_string(),
                plan_revision: 3,
                routine_id: "founder_brief_daily".to_string(),
                triggered_by: "user_123".to_string(),
                trigger_source: ManualTriggerSource::Calendar,
                dry_run: true,
                approval_policy_snapshot: Some(ApprovalMatrix {
                    internal_reports: Some(ApprovalMode::AutoApproved),
                    public_posts: Some(ApprovalMode::ApprovalRequired),
                    ..ApprovalMatrix::default()
                }),
                connector_binding_snapshot: vec![ConnectorBinding {
                    capability: "gmail".to_string(),
                    binding_type: "oauth_integration".to_string(),
                    binding_id: "gmail-prod".to_string(),
                    allowlist_pattern: Some("gmail.send".to_string()),
                    status: "mapped".to_string(),
                }],
                triggered_at: "2026-03-27T09:15:00Z".to_string(),
                run_id: Some("run_abc123".to_string()),
                outcome: Some("paused_after_validation".to_string()),
                artifacts_produced: vec!["founder_brief_draft.md".to_string()],
                notes: Some("Dry-run from calendar entry".to_string()),
            }),
            validation_state: Some(PlanValidationState {
                required_connectors_mapped: Some(false),
                directories_writable: Some(true),
                schedules_valid: Some(true),
                models_resolved: Some(true),
                dependencies_resolvable: Some(true),
                approvals_complete: Some(true),
                degraded_modes_acknowledged: Some(false),
                data_scopes_valid: Some(true),
                audit_scopes_valid: Some(true),
                mission_context_scopes_valid: Some(true),
                inter_routine_policy_complete: Some(true),
                credential_envelopes_valid: Some(true),
                compartmentalized_activation_ready: Some(true),
                context_objects_valid: Some(true),
                success_criteria_evaluation: None,
            }),
            overlap_policy: Some(OverlapPolicy {
                exact_identity: Some(OverlapIdentity {
                    hash_version: Some(1),
                    canonical_hash: Some("abc123".to_string()),
                    normalized_fields: vec!["goal".to_string(), "outputs".to_string()],
                }),
                semantic_identity: Some(SemanticIdentity {
                    similarity_model: Some("text-embedding-3-large".to_string()),
                    semantic_signature: Some("vec-ref".to_string()),
                    similarity_threshold: Some(0.8),
                }),
                overlap_log: vec![OverlapLogEntry {
                    matched_plan_id: "plan_old".to_string(),
                    matched_plan_revision: 2,
                    match_layer: "semantic".to_string(),
                    similarity_score: Some(0.92),
                    decision: "fork".to_string(),
                    decided_by: "user_confirmed".to_string(),
                    decided_at: "2026-03-27T10:00:00Z".to_string(),
                }],
            }),
            routine_graph: vec![RoutinePackage {
                routine_id: "founder_brief_daily".to_string(),
                semantic_kind: RoutineSemanticKind::Reporting,
                trigger: TriggerDefinition {
                    trigger_type: TriggerKind::Scheduled,
                    schedule: Some("0 10 * * *".to_string()),
                    timezone: Some("UTC".to_string()),
                },
                dependencies: vec![RoutineDependency {
                    dependency_type: "routine".to_string(),
                    routine_id: "market_pain_daily".to_string(),
                    mode: DependencyMode::Hard,
                }],
                dependency_resolution: DependencyResolution {
                    strategy: DependencyResolutionStrategy::TopologicalSequential,
                    partial_failure_mode: PartialFailureMode::PauseDownstreamOnly,
                    reentry_point: ReentryPoint::FailedStep,
                    mid_routine_connector_failure: MidRoutineConnectorFailureMode::SurfaceAndPause,
                },
                connector_resolution: RoutineConnectorResolution {
                    states: vec!["unresolved".to_string(), "bound".to_string()],
                    binding_options: vec!["mcp_server".to_string(), "native_feature".to_string()],
                },
                data_scope: DataScope {
                    readable_paths: vec!["mission.goal".to_string()],
                    writable_paths: vec![
                        "knowledge/workflows/drafts/founder_brief_daily/**".to_string()
                    ],
                    denied_paths: vec!["credentials/**".to_string()],
                    cross_routine_visibility: CrossRoutineVisibility::DeclaredOutputsOnly,
                    mission_context_scope: MissionContextScope::GoalAndDependencies,
                    mission_context_justification: None,
                },
                audit_scope: AuditScope {
                    run_history_visibility: RunHistoryVisibility::PlanOwner,
                    named_audit_roles: Vec::new(),
                    intermediate_artifact_visibility: IntermediateArtifactVisibility::RoutineOnly,
                    final_artifact_visibility: FinalArtifactVisibility::DeclaredConsumers,
                },
                success_criteria: SuccessCriteria {
                    required_artifacts: vec!["founder_brief.md".to_string()],
                    minimum_viable_completion: None,
                    minimum_output: Some("one usable brief draft".to_string()),
                    freshness_window_hours: Some(24),
                },
                steps: vec![StepPackage {
                    step_id: "draft_brief".to_string(),
                    label: "Draft brief".to_string(),
                    kind: "reporting".to_string(),
                    action: "synthesize the daily findings into a founder brief".to_string(),
                    inputs: vec!["clustered_themes".to_string()],
                    outputs: vec!["founder_brief_draft".to_string()],
                    dependencies: vec!["market_pain_daily".to_string()],
                    context_reads: vec![
                        "ctx:founder_brief_daily:mission.goal".to_string(),
                        "ctx:founder_brief_daily:workspace.environment".to_string(),
                    ],
                    context_writes: vec![
                        "ctx:founder_brief_daily:draft_brief:founder_brief_draft.md".to_string(),
                    ],
                    connector_requirements: vec![ConnectorRequirement {
                        capability: "gmail".to_string(),
                        required: true,
                    }],
                    model_policy: StepModelPolicy {
                        primary: Some(StepModelSelection {
                            tier: ModelTier::Mid,
                        }),
                    },
                    approval_policy: ApprovalMode::DraftOnly,
                    success_criteria: SuccessCriteria {
                        minimum_output: Some("one usable brief draft".to_string()),
                        ..SuccessCriteria::default()
                    },
                    failure_policy: StepFailurePolicy {
                        on_model_failure: Some("retry_once_then_pause".to_string()),
                        ..StepFailurePolicy::default()
                    },
                    retry_policy: StepRetryPolicy {
                        max_attempts: Some(2),
                    },
                    artifacts: vec!["founder_brief_draft.md".to_string()],
                    provenance: Some(StepProvenance {
                        plan_id: Some("plan_0f6e8c".to_string()),
                        routine_id: Some("founder_brief_daily".to_string()),
                        step_id: Some("draft_brief".to_string()),
                        cost_provenance: None,
                    }),
                    notes: None,
                }],
            }],
            connector_intents: vec![ConnectorIntent {
                capability: "gmail".to_string(),
                why: "Deliver founder brief".to_string(),
                required: true,
                degraded_mode_allowed: false,
            }],
            connector_bindings: vec![ConnectorBinding {
                capability: "gmail".to_string(),
                binding_type: "oauth_integration".to_string(),
                binding_id: "gmail-prod".to_string(),
                allowlist_pattern: Some("gmail.send".to_string()),
                status: "mapped".to_string(),
            }],
            connector_binding_resolution: None,
            model_routing_resolution: None,
            credential_envelopes: vec![CredentialEnvelope {
                routine_id: "founder_brief_daily".to_string(),
                entitled_connectors: vec![CredentialBindingRef {
                    capability: "gmail".to_string(),
                    binding_id: "gmail-prod".to_string(),
                }],
                denied_connectors: Vec::new(),
                envelope_issued_at: None,
                envelope_expires_at: None,
                issuing_authority: Some("engine".to_string()),
            }],
            context_objects: vec![ContextObject {
                context_object_id: "ctx:founder_brief_daily:draft_brief:founder_brief_draft.md"
                    .to_string(),
                name: "Draft brief handoff".to_string(),
                kind: "step_output_handoff".to_string(),
                scope: ContextObjectScope::Handoff,
                owner_routine_id: "founder_brief_daily".to_string(),
                producer_step_id: Some("draft_brief".to_string()),
                declared_consumers: vec!["founder_brief_daily".to_string()],
                artifact_ref: Some("founder_brief_draft.md".to_string()),
                data_scope_refs: vec![
                    "knowledge/workflows/drafts/founder_brief_daily/**".to_string()
                ],
                freshness_window_hours: Some(24),
                validation_status: ContextValidationStatus::Pending,
                provenance: ContextObjectProvenance {
                    plan_id: "plan_0f6e8c".to_string(),
                    routine_id: "founder_brief_daily".to_string(),
                    step_id: Some("draft_brief".to_string()),
                },
                summary: Some("one usable brief draft".to_string()),
            }],
            metadata: Some(serde_json::json!({
                "source": "preview",
                "schema_version": 1
            })),
        };

        let json = serde_json::to_value(&package).expect("serialize plan package");
        let roundtrip: PlanPackage =
            serde_json::from_value(json).expect("deserialize plan package");

        assert_eq!(roundtrip, package);
        assert_eq!(roundtrip.routine_graph.len(), 1);
        assert_eq!(roundtrip.routine_graph[0].steps.len(), 1);
    }

    #[test]
    fn compile_workflow_plan_preview_package_projects_workflow_plan_json() {
        let plan = crate::contracts::WorkflowPlanJson {
            plan_id: "plan_preview".to_string(),
            planner_version: "v1".to_string(),
            plan_source: "test".to_string(),
            original_prompt: "Research the market and draft a summary.".to_string(),
            normalized_prompt: "research the market and draft a summary".to_string(),
            confidence: "medium".to_string(),
            title: "Research market".to_string(),
            description: Some("Preview plan".to_string()),
            schedule: crate::contracts::default_fallback_schedule_json(),
            execution_target: "automation_v2".to_string(),
            workspace_root: "/repo".to_string(),
            steps: vec![WorkflowPlanStep {
                step_id: "research_sources".to_string(),
                kind: "research".to_string(),
                objective: "Collect source material".to_string(),
                depends_on: Vec::new(),
                agent_role: "worker".to_string(),
                input_refs: vec![json!({"from_step_id":"seed","alias":"seed_input"})],
                output_contract: Some(json!({
                    "kind": "brief",
                    "enforcement": {
                        "required_tools": ["websearch"]
                    }
                })),
                metadata: Some(json!({
                    "builder": {
                        "web_research_expected": true
                    }
                })),
            }],
            requires_integrations: vec!["gmail".to_string()],
            allowed_mcp_servers: vec!["github".to_string()],
            operator_preferences: None,
            save_options: json!({"can_export_pack": true}),
        };

        let package = compile_workflow_plan_preview_package(&plan, Some("user"));

        assert_eq!(package.plan_id, "plan_preview");
        assert_eq!(package.lifecycle_state, PlanLifecycleState::Preview);
        assert_eq!(package.owner.owner_id, "user");
        assert_eq!(package.routine_graph.len(), 1);
        assert_eq!(package.routine_graph[0].steps.len(), 1);
        assert_eq!(
            package.routine_graph[0].steps[0].context_reads,
            vec![
                "ctx:plan_preview_routine:mission.goal".to_string(),
                "ctx:plan_preview_routine:workspace.environment".to_string(),
            ]
        );
        assert_eq!(
            package.routine_graph[0].steps[0].context_writes,
            vec!["ctx:plan_preview_routine:research_sources:research_sources.artifact".to_string()]
        );
        assert_eq!(
            package.routine_graph[0].steps[0].connector_requirements[0].capability,
            "websearch"
        );
        assert_eq!(package.connector_intents[0].capability, "gmail");
        assert_eq!(
            package.connector_binding_resolution.as_ref().map(|report| (
                report.mapped_count,
                report.unresolved_required_count,
                report.entries.len()
            )),
            Some((0, 1, 1))
        );
        assert_eq!(
            package
                .output_roots
                .as_ref()
                .and_then(|roots| roots.plan.as_deref()),
            Some("/repo/knowledge/workflows/plan/")
        );
        assert_eq!(
            package.routine_graph[0].trigger.trigger_type,
            TriggerKind::Manual
        );
        assert!(package.routine_graph[0].trigger.schedule.is_none());
        assert!(package.routine_graph[0].trigger.timezone.is_none());
        assert_eq!(
            package
                .budget_policy
                .as_ref()
                .and_then(|policy| policy.max_cost_per_run_usd),
            Some(4.0)
        );
        assert_eq!(
            package
                .budget_enforcement
                .as_ref()
                .and_then(|enforcement| enforcement.soft_warning_threshold),
            Some(0.8)
        );
        assert_eq!(
            package
                .overlap_policy
                .as_ref()
                .and_then(|policy| policy.exact_identity.as_ref())
                .and_then(|identity| identity.hash_version),
            Some(1)
        );
        assert_eq!(
            package
                .overlap_policy
                .as_ref()
                .and_then(|policy| policy.semantic_identity.as_ref())
                .and_then(|identity| identity.similarity_threshold),
            Some(0.85)
        );
        assert_eq!(package.credential_envelopes.len(), 1);
        assert_eq!(
            package.credential_envelopes[0].routine_id,
            package.routine_graph[0].routine_id
        );
        assert!(package.precedence_log.is_empty());
        assert!(package.plan_diff.is_none());
        assert!(package.manual_trigger_record.is_none());
        assert_eq!(package.credential_envelopes[0].entitled_connectors.len(), 0);
        assert_eq!(package.context_objects.len(), 3);
        assert_eq!(package.context_objects[0].kind, "mission_goal");
        assert_eq!(package.context_objects[1].kind, "workspace_environment");
        assert_eq!(
            package.context_objects[2].producer_step_id.as_deref(),
            Some("research_sources")
        );
        assert_eq!(
            package.routine_graph[0].data_scope.writable_paths[0],
            "/repo/knowledge/workflows/plan/plan_preview_routine/**"
        );
        assert_eq!(
            package
                .validation_state
                .as_ref()
                .and_then(|state| state.credential_envelopes_valid),
            Some(true)
        );
    }

    #[test]
    fn with_manual_trigger_record_captures_plan_snapshots() {
        let plan = compile_workflow_plan_preview_package(
            &crate::contracts::WorkflowPlanJson {
                plan_id: "plan_manual_trigger".to_string(),
                planner_version: "v1".to_string(),
                plan_source: "test".to_string(),
                original_prompt: "Draft a brief".to_string(),
                normalized_prompt: "draft a brief".to_string(),
                confidence: "medium".to_string(),
                title: "Draft a brief".to_string(),
                description: Some("Preview plan".to_string()),
                schedule: crate::contracts::default_fallback_schedule_json(),
                execution_target: "automation_v2".to_string(),
                workspace_root: "/repo".to_string(),
                steps: vec![WorkflowPlanStep {
                    step_id: "draft_brief".to_string(),
                    kind: "analysis".to_string(),
                    objective: "Draft a brief".to_string(),
                    depends_on: Vec::new(),
                    agent_role: "writer".to_string(),
                    input_refs: Vec::new(),
                    output_contract: Some(json!({"kind": "report_markdown"})),
                    metadata: None,
                }],
                requires_integrations: vec!["github".to_string()],
                allowed_mcp_servers: vec!["github".to_string()],
                operator_preferences: None,
                save_options: json!({}),
            },
            Some("control-panel"),
        );
        let updated = with_manual_trigger_record(
            &plan,
            "manual-trigger-run_123",
            "control-panel",
            ManualTriggerSource::Api,
            true,
            "2026-03-28T10:15:00Z",
            Some("run_123"),
            Some("queued"),
            vec!["artifact.md".to_string()],
            Some("Triggered from test"),
        )
        .expect("manual trigger record");

        let record = updated
            .manual_trigger_record
            .as_ref()
            .expect("manual trigger record");
        assert_eq!(record.trigger_id, "manual-trigger-run_123");
        assert_eq!(record.plan_id, updated.plan_id);
        assert_eq!(record.plan_revision, updated.plan_revision);
        assert_eq!(record.routine_id, updated.routine_graph[0].routine_id);
        assert_eq!(record.triggered_by, "control-panel");
        assert_eq!(record.trigger_source, ManualTriggerSource::Api);
        assert!(record.dry_run);
        assert_eq!(record.run_id.as_deref(), Some("run_123"));
        assert_eq!(record.outcome.as_deref(), Some("queued"));
        assert_eq!(record.artifacts_produced, vec!["artifact.md".to_string()]);
        assert_eq!(record.notes.as_deref(), Some("Triggered from test"));
        assert_eq!(
            record.connector_binding_snapshot,
            updated.connector_bindings
        );
        assert_eq!(record.approval_policy_snapshot, updated.approval_policy);
    }

    #[test]
    fn lifecycle_transition_table_matches_spec() {
        assert!(can_transition_plan_lifecycle(
            PlanLifecycleState::Preview,
            PlanLifecycleState::AwaitingApproval
        ));
        assert!(can_transition_plan_lifecycle(
            PlanLifecycleState::Approved,
            PlanLifecycleState::Applied
        ));
        assert!(can_transition_plan_lifecycle(
            PlanLifecycleState::Archived,
            PlanLifecycleState::Draft
        ));
        assert!(!can_transition_plan_lifecycle(
            PlanLifecycleState::Draft,
            PlanLifecycleState::Active
        ));
        assert!(!can_transition_plan_lifecycle(
            PlanLifecycleState::Archived,
            PlanLifecycleState::Applied
        ));
    }
}
