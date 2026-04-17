    build_automation_attempt_evidence, build_automation_validator_summary,
    detect_automation_blocker_category, detect_automation_node_failure_kind,
    detect_automation_node_phase, detect_automation_node_status, wrap_automation_node_output,
};
use crate::app::state::automation::receipts::{
    append_automation_attempt_receipt, automation_attempt_receipt_path_for_state_dir,
    AutomationAttemptReceiptDraft, AutomationAttemptReceiptRecord,
};
use crate::capability_resolver;

mod brief_coverage;
mod brief_outcomes;
mod brief_validation;
mod integration;
mod prompting;
mod replay_suite;
mod runtime_paths;
mod structured_handoff;
mod telemetry;
mod tool_discovery;
mod validation;
mod validation_recovery;
mod workflow_policy;

fn with_legacy_quality_rollback_enabled<T>(enabled: bool, f: impl FnOnce() -> T) -> T {
    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    let _guard = ENV_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .expect("env lock");
    let key = "TANDEM_AUTOMATION_QUALITY_LEGACY_ROLLBACK";
    let previous = std::env::var(key).ok();
    std::env::set_var(key, if enabled { "true" } else { "false" });
    let result = f();
    if let Some(previous) = previous {
        std::env::set_var(key, previous);
    } else {
        std::env::remove_var(key);
    }
    result
}

fn routine_dependency_plan_package() -> tandem_plan_compiler::api::PlanPackage {
    use tandem_plan_compiler::api::{
        ApprovalMode, AuditScope, CommunicationModel, CrossRoutineVisibility, DataScope,
        DependencyMode, DependencyResolution, DependencyResolutionStrategy,
        FinalArtifactVisibility, InterRoutinePolicy, IntermediateArtifactVisibility,
        MidRoutineConnectorFailureMode, MissionContextScope, MissionDefinition, PartialFailureMode,
        PeerVisibility, PlanLifecycleState, PlanOwner, PlanPackage, ReentryPoint,
        RoutineConnectorResolution, RoutineDependency, RoutinePackage, RoutineSemanticKind,
        RunHistoryVisibility, StepFailurePolicy, StepModelPolicy, StepPackage, StepRetryPolicy,
        SuccessCriteria, TriggerDefinition, TriggerKind,
    };

    let step_a = StepPackage {
        step_id: "step_a".to_string(),
        label: "Step A".to_string(),
        kind: "execute".to_string(),
        action: "run_a".to_string(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        dependencies: Vec::new(),
        context_reads: Vec::new(),
        context_writes: Vec::new(),
        connector_requirements: Vec::new(),
        model_policy: StepModelPolicy::default(),
        approval_policy: ApprovalMode::InternalOnly,
        success_criteria: SuccessCriteria::default(),
        failure_policy: StepFailurePolicy::default(),
        retry_policy: StepRetryPolicy::default(),
        artifacts: Vec::new(),
        provenance: None,
        notes: None,
    };
    let step_b = StepPackage {
        step_id: "step_b".to_string(),
        label: "Step B".to_string(),
        kind: "execute".to_string(),
        action: "run_b".to_string(),
        inputs: Vec::new(),
        outputs: Vec::new(),
        dependencies: Vec::new(),
        context_reads: Vec::new(),
        context_writes: Vec::new(),
        connector_requirements: Vec::new(),
        model_policy: StepModelPolicy::default(),
        approval_policy: ApprovalMode::InternalOnly,
        success_criteria: SuccessCriteria::default(),
        failure_policy: StepFailurePolicy::default(),
        retry_policy: StepRetryPolicy::default(),
        artifacts: Vec::new(),
        provenance: None,
        notes: None,
    };

    PlanPackage {
        plan_id: "plan-dependency-test".to_string(),
        plan_revision: 1,
        lifecycle_state: PlanLifecycleState::Preview,
        owner: PlanOwner {
            owner_id: "owner".to_string(),
            scope: "workspace".to_string(),
            audience: "internal".to_string(),
        },
        mission: MissionDefinition {
            goal: "Dependency test".to_string(),
            summary: None,
            domain: None,
        },
        success_criteria: SuccessCriteria::default(),
        budget_policy: None,
        budget_enforcement: None,
        approval_policy: None,
        inter_routine_policy: Some(InterRoutinePolicy {
            communication_model: CommunicationModel::ArtifactOnly,
            shared_memory_access: false,
            shared_memory_justification: None,
            peer_visibility: PeerVisibility::None,
            artifact_handoff_validation: true,
        }),
        trigger_policy: None,
        output_roots: None,
        precedence_log: Vec::new(),
        plan_diff: None,
        manual_trigger_record: None,
        validation_state: None,
        overlap_policy: None,
        routine_graph: vec![
            RoutinePackage {
                routine_id: "routine_a".to_string(),
                semantic_kind: RoutineSemanticKind::Execution,
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
                connector_resolution: RoutineConnectorResolution::default(),
                data_scope: DataScope {
                    readable_paths: Vec::new(),
                    writable_paths: Vec::new(),
                    denied_paths: Vec::new(),
                    cross_routine_visibility: CrossRoutineVisibility::None,
                    mission_context_scope: MissionContextScope::GoalOnly,
                    mission_context_justification: None,
                },
                audit_scope: AuditScope {
                    run_history_visibility: RunHistoryVisibility::PlanOwner,
                    named_audit_roles: Vec::new(),
                    intermediate_artifact_visibility: IntermediateArtifactVisibility::RoutineOnly,
                    final_artifact_visibility: FinalArtifactVisibility::PlanOwner,
                },
                success_criteria: SuccessCriteria::default(),
                steps: vec![step_a],
            },
            RoutinePackage {
                routine_id: "routine_b".to_string(),
                semantic_kind: RoutineSemanticKind::Execution,
                trigger: TriggerDefinition {
                    trigger_type: TriggerKind::Manual,
                    schedule: None,
                    timezone: None,
                },
                dependencies: vec![RoutineDependency {
                    dependency_type: "routine".to_string(),
                    routine_id: "routine_a".to_string(),
                    mode: DependencyMode::Hard,
                }],
                dependency_resolution: DependencyResolution {
                    strategy: DependencyResolutionStrategy::TopologicalSequential,
                    partial_failure_mode: PartialFailureMode::PauseDownstreamOnly,
                    reentry_point: ReentryPoint::FailedStep,
                    mid_routine_connector_failure: MidRoutineConnectorFailureMode::SurfaceAndPause,
                },
                connector_resolution: RoutineConnectorResolution::default(),
                data_scope: DataScope {
                    readable_paths: Vec::new(),
                    writable_paths: Vec::new(),
                    denied_paths: Vec::new(),
                    cross_routine_visibility: CrossRoutineVisibility::None,
                    mission_context_scope: MissionContextScope::GoalOnly,
                    mission_context_justification: None,
                },
                audit_scope: AuditScope {
                    run_history_visibility: RunHistoryVisibility::PlanOwner,
                    named_audit_roles: Vec::new(),
                    intermediate_artifact_visibility: IntermediateArtifactVisibility::RoutineOnly,
                    final_artifact_visibility: FinalArtifactVisibility::PlanOwner,
                },
                success_criteria: SuccessCriteria::default(),
                steps: vec![step_b],
            },
        ],
        connector_intents: Vec::new(),
        connector_bindings: Vec::new(),
        connector_binding_resolution: None,
        model_routing_resolution: None,
        credential_envelopes: Vec::new(),
        context_objects: Vec::new(),
        metadata: None,
    }
}

#[tokio::test]
async fn automation_attempt_receipt_append_uses_jsonl_path_and_skips_malformed_lines() {
    let state_dir =
        std::env::temp_dir().join(format!("tandem-receipt-ledger-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&state_dir).expect("state dir");

    let run_id = "run-ledger-test";
    let node_id = "node-alpha";
    let expected_path = automation_attempt_receipt_path_for_state_dir(&state_dir, run_id, node_id);

    if let Some(parent) = expected_path.parent() {
        std::fs::create_dir_all(parent).expect("receipt parent");
    }

    std::fs::write(
        &expected_path,
        concat!(
            "not-json\n",
            "{\"version\":1,\"run_id\":\"run-ledger-test\",\"node_id\":\"node-alpha\",\"attempt\":1,\"session_id\":\"sess-1\",\"seq\":7,\"ts_ms\":10,\"event_type\":\"started\",\"payload\":{\"step\":\"seed\"}}\n"
        ),
    )
    .expect("seed receipts");

    let summary = append_automation_attempt_receipt(
        &state_dir,
        AutomationAttemptReceiptDraft {
            run_id: run_id.to_string(),
            node_id: node_id.to_string(),
            attempt: 2,
            session_id: "sess-2".to_string(),
            event_type: "completed".to_string(),
            payload: serde_json::json!({"ok": true}),
        },
    )
    .await
    .expect("append receipt");

    assert_eq!(summary.path, expected_path);
    assert_eq!(summary.seq, 8);
    assert_eq!(summary.record_count, 2);

    let raw = std::fs::read_to_string(&expected_path).expect("receipt text");
    let mut lines = raw.lines();
    assert_eq!(lines.next(), Some("not-json"));
    let last_line = lines.last().expect("appended line");
    let appended: AutomationAttemptReceiptRecord =
        serde_json::from_str(last_line).expect("parse appended receipt");
    assert_eq!(appended.version, 1);
    assert_eq!(appended.run_id, run_id);
    assert_eq!(appended.node_id, node_id);
    assert_eq!(appended.attempt, 2);
    assert_eq!(appended.session_id, "sess-2");
    assert_eq!(appended.seq, 8);
    assert_eq!(appended.event_type, "completed");
    assert_eq!(appended.payload, serde_json::json!({"ok": true}));

    let _ = std::fs::remove_dir_all(&state_dir);
}

#[tokio::test]
async fn automation_attempt_receipt_collects_tool_and_artifact_events() {
    let automation = test_phase_automation(
        serde_json::json!([{ "phase_id": "phase_1", "title": "Phase 1", "execution_mode": "soft" }]),
        vec![test_automation_node("draft", Vec::new(), "phase_1", 1)],
    );
    let node = automation.flow.nodes[0].clone();
    let mut session = Session::new(Some("receipt test".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "read".to_string(),
            args: serde_json::json!({"path":"README.md"}),
            result: Some(serde_json::json!({"ok": true})),
            error: None,
        }],
    ));
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "bash".to_string(),
            args: serde_json::json!({"cmd":"false"}),
            result: None,
            error: Some("exit 1".to_string()),
        }],
    ));

    let events = collect_automation_attempt_receipt_events(
        &automation,
        "run-1",
        &node,
        2,
        &session.id,
        &session,
        Some(&("out.md".to_string(), "artifact".to_string())),
        None,
        Some("out.md"),
        Some(&serde_json::json!({"status":"succeeded"})),
    );

    let event_types = events
        .iter()
        .map(|event| event.event_type.as_str())
        .collect::<Vec<_>>();

    assert_eq!(
        event_types,
        vec![
            "tool_invoked",
            "tool_succeeded",
            "tool_invoked",
            "tool_failed",
            "artifact_write_success",
        ]
    );
}

#[test]
fn report_markdown_completed_status_does_not_trigger_blocked_handoff_cleanup() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-completed-status-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("workspace dir");
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Draft the final report".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "report.md",
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let session = Session::new(
        Some("editorial".to_string()),
        Some(workspace_root.to_string_lossy().to_string()),
    );
    let tool_telemetry = json!({
        "requested_tools": ["write"],
        "executed_tools": ["write"],
        "tool_call_counts": {
            "write": 1
        }
    });
    let report_text = "# Report\n\nPipeline status: blocked by missing resume grounding artifacts.\n\nThis artifact cannot be finalized until required source reads and web research are available.\n";
    let (accepted_output, artifact_validation, _) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        Some(("report.md".to_string(), report_text.to_string())),
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(
        accepted_output.as_ref().map(|(path, _)| path.as_str()),
        Some("report.md")
    );
    assert_eq!(
        artifact_validation
            .get("accepted_artifact_path")
            .and_then(Value::as_str),
        Some("report.md")
    );
    assert!(artifact_validation
        .get("semantic_block_reason")
        .and_then(Value::as_str)
        .is_some());

    let _ = std::fs::remove_dir_all(&workspace_root);
}
#[test]
fn automation_blocked_nodes_respects_barrier_open_phase() {
    let automation = test_phase_automation(
        json!([
            { "phase_id": "phase_1", "title": "Phase 1", "execution_mode": "barrier" },
            { "phase_id": "phase_2", "title": "Phase 2", "execution_mode": "soft" }
        ]),
        vec![
            test_automation_node("draft", Vec::new(), "phase_1", 1),
            test_automation_node("publish", Vec::new(), "phase_2", 100),
        ],
    );
    let run = test_phase_run(vec!["draft", "publish"], Vec::new());

    assert_eq!(
        automation_blocked_nodes(&automation, &run),
        vec!["publish".to_string()]
    );
}

#[test]
fn automation_soft_phase_prefers_current_open_phase_before_priority() {
    let automation = test_phase_automation(
        json!([
            { "phase_id": "phase_1", "title": "Phase 1", "execution_mode": "soft" },
            { "phase_id": "phase_2", "title": "Phase 2", "execution_mode": "soft" }
        ]),
        vec![
            test_automation_node("draft", Vec::new(), "phase_1", 1),
            test_automation_node("publish", Vec::new(), "phase_2", 100),
        ],
    );
    let run = test_phase_run(vec!["draft", "publish"], Vec::new());
    let phase_rank = automation_phase_rank_map(&automation);
    let current_open_phase_rank =
        automation_current_open_phase(&automation, &run).map(|(_, rank, _)| rank);
    let draft = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "draft")
        .expect("draft node");
    let publish = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "publish")
        .expect("publish node");

    assert!(automation_blocked_nodes(&automation, &run).is_empty());
    assert!(
        automation_node_sort_key(draft, &phase_rank, current_open_phase_rank)
            < automation_node_sort_key(publish, &phase_rank, current_open_phase_rank)
    );
}

#[test]
fn automation_soft_phase_limits_runnable_frontier_to_current_open_phase() {
    let automation = test_phase_automation(
        json!([
            { "phase_id": "phase_1", "title": "Phase 1", "execution_mode": "soft" },
            { "phase_id": "phase_2", "title": "Phase 2", "execution_mode": "soft" }
        ]),
        vec![
            test_automation_node("draft", Vec::new(), "phase_1", 1),
            test_automation_node("publish", Vec::new(), "phase_2", 100),
        ],
    );
    let run = test_phase_run(vec!["draft", "publish"], Vec::new());

    let filtered =
        automation_filter_runnable_by_open_phase(&automation, &run, automation.flow.nodes.clone());

    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].node_id, "draft");
}

#[test]
fn automation_routine_dependency_filter_blocks_downstream_routine_until_upstream_completes() {
    let mut automation = test_phase_automation(
        json!([
            { "phase_id": "phase_1", "title": "Phase 1", "execution_mode": "soft" }
        ]),
        vec![
            test_automation_node("step_a", Vec::new(), "phase_1", 1),
            test_automation_node("step_b", Vec::new(), "phase_1", 2),
        ],
    );
    let plan_package_value =
        serde_json::to_value(routine_dependency_plan_package()).expect("plan package json");
    let parsed_plan_package: tandem_plan_compiler::api::PlanPackage =
        serde_json::from_value(plan_package_value.clone()).expect("plan package parse");
    assert_eq!(parsed_plan_package.routine_graph.len(), 2);
    assert_eq!(
        parsed_plan_package.routine_graph[0].steps[0].step_id,
        "step_a"
    );
    assert_eq!(
        parsed_plan_package.routine_graph[1].steps[0].step_id,
        "step_b"
    );
    assert_eq!(
        parsed_plan_package.routine_graph[1].dependencies[0].routine_id,
        "routine_a"
    );
    automation.metadata = Some(json!({
        "plan_package": plan_package_value
    }));

    let incomplete_run = test_phase_run(vec!["step_a", "step_b"], Vec::new());
    let filtered = automation_filter_runnable_by_routine_dependencies(
        &automation,
        &incomplete_run,
        automation.flow.nodes.clone(),
    );
    assert_eq!(
        filtered
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<Vec<_>>(),
        vec!["step_a"]
    );
    assert_eq!(
        automation_blocked_nodes(&automation, &incomplete_run),
        vec!["step_b".to_string()]
    );

    let complete_upstream_run = test_phase_run(vec!["step_b"], vec!["step_a"]);
    let filtered = automation_filter_runnable_by_routine_dependencies(
        &automation,
        &complete_upstream_run,
        vec![automation
            .flow
            .nodes
            .iter()
            .find(|node| node.node_id == "step_b")
            .cloned()
            .expect("step_b node")],
    );
    assert_eq!(
        filtered
            .iter()
            .map(|node| node.node_id.as_str())
            .collect::<Vec<_>>(),
        vec!["step_b"]
    );
    assert!(automation_blocked_nodes(&automation, &complete_upstream_run).is_empty());
}

#[test]
fn runnable_write_scope_filter_skips_overlapping_code_nodes() {
    let first = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "first".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "First".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "task_kind": "code_change",
                "write_scope": "src"
            }
        })),
    };
    let overlapping = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "overlap".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Overlap".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "task_kind": "code_change",
                "write_scope": "src/lib"
            }
        })),
    };
    let disjoint = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "disjoint".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Disjoint".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "task_kind": "code_change",
                "write_scope": "docs"
            }
        })),
    };

    let filtered = automation_filter_runnable_by_write_scope_conflicts(
        vec![first.clone(), overlapping, disjoint.clone()],
        3,
    );

    let ids = filtered
        .iter()
        .map(|node| node.node_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["first", "disjoint"]);
}

#[test]
fn runnable_write_scope_filter_allows_non_code_nodes_to_run_in_parallel() {
    let code = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "code".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Code".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "task_kind": "code_change",
                "write_scope": "src"
            }
        })),
    };
    let brief = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "brief".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Brief".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("research_synthesis".to_string()),
                required_tools: Vec::new(),
                required_evidence: vec![
                    "local_source_reads".to_string(),
                    "external_sources".to_string(),
                ],
                required_sections: vec![
                    "citations".to_string(),
                    "web_sources_reviewed".to_string(),
                ],
                prewrite_gates: Vec::new(),
                retry_on_missing: Vec::new(),
                terminal_on: Vec::new(),
                repair_budget: None,
                session_text_recovery: None,
            }),
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md"
            }
        })),
    };

    let filtered =
        automation_filter_runnable_by_write_scope_conflicts(vec![code.clone(), brief.clone()], 2);

    let ids = filtered
        .iter()
        .map(|node| node.node_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["code", "brief"]);
}

#[test]
fn generic_required_tools_prewrite_requirements_enable_repair() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "artifact".to_string(),
        agent_id: "writer".to_string(),
        objective: "Write notes.md".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "artifact".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "notes.md",
                "web_research_expected": true,
                "required_tools": ["read", "websearch"]
            }
        })),
    };

    let requirements = automation_node_prewrite_requirements(
        &node,
        &[
            "read".to_string(),
            "websearch".to_string(),
            "write".to_string(),
        ],
    )
    .expect("prewrite requirements");

    assert!(requirements.concrete_read_required);
    assert!(requirements.successful_web_research_required);
    assert!(requirements.repair_on_unmet_requirements);
    assert_eq!(requirements.coverage_mode, PrewriteCoverageMode::None);
}

#[test]
fn research_finalize_prewrite_requirements_skip_same_node_reads_and_websearch() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-brief".to_string(),
        agent_id: "research".to_string(),
        objective: "Write marketing brief".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("research_synthesis".to_string()),
                required_tools: Vec::new(),
                required_evidence: vec![
                    "local_source_reads".to_string(),
                    "external_sources".to_string(),
                ],
                required_sections: vec![
                    "files_reviewed".to_string(),
                    "files_not_reviewed".to_string(),
                    "citations".to_string(),
                    "web_sources_reviewed".to_string(),
                ],
                prewrite_gates: Vec::new(),
                retry_on_missing: Vec::new(),
                terminal_on: Vec::new(),
                repair_budget: Some(5),
                session_text_recovery: None,
            }),
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "research_stage": "research_finalize"
            }
        })),
    };

    let requirements = automation_node_prewrite_requirements(
        &node,
        &[
            "read".to_string(),
            "write".to_string(),
            "websearch".to_string(),
        ],
    )
    .expect("prewrite requirements");

    assert!(!requirements.workspace_inspection_required);
    assert!(!requirements.web_research_required);
    assert!(!requirements.concrete_read_required);
    assert!(!requirements.successful_web_research_required);
}

#[test]
fn explicit_input_files_skip_workspace_inspection_but_still_require_concrete_reads() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "draft_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Write final report".to_string(),
        depends_on: vec!["collect_inputs".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "collect_inputs".to_string(),
            alias: "inputs".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "artifact".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "reports/final.md",
                "input_files": ["inputs/brief.md"],
                "required_tools": ["read"]
            }
        })),
    };

    let requirements =
        automation_node_prewrite_requirements(&node, &["read".to_string(), "write".to_string()])
            .expect("prewrite requirements");

    assert!(!requirements.workspace_inspection_required);
    assert!(requirements.concrete_read_required);
    assert!(requirements.repair_on_unmet_requirements);
}

#[test]
fn generic_required_tools_validation_needs_repair_when_read_unused() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-required-tools-test-{}", now_ms()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "artifact".to_string(),
        agent_id: "writer".to_string(),
        objective: "Write notes.md".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "artifact".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "notes.md",
                "required_tools": ["read"]
            }
        })),
    };
    let mut session = Session::new(Some("required tools".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path":"notes.md",
                "content":"# Notes\n\nA short summary written without reading sources.\n"
            }),
            result: Some(json!({"output":"written"})),
            error: None,
        }],
    ));

    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &["read".to_string(), "write".to_string()],
    );
    let (_, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "",
        &tool_telemetry,
        None,
        Some((
            "notes.md".to_string(),
            "# Notes\n\nA short summary written without reading sources.\n".to_string(),
        )),
        &std::collections::BTreeSet::new(),
    );

    assert!(rejected.is_some());
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("research completed without concrete file reads or required source coverage")
    );
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("needs_repair")
    );
    assert_eq!(
        artifact_validation
            .get("blocking_classification")
            .and_then(Value::as_str),
        Some("tool_available_but_not_used")
    );
    assert_eq!(
        artifact_validation
            .get("required_next_tool_actions")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some("Use `read` on concrete workspace files before finalizing the brief.")
    );

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Done — `notes.md` was written.",
            Some(&(
                "notes.md".to_string(),
                "# Notes\n\nA short summary written without reading sources.\n".to_string(),
            )),
            &tool_telemetry,
            Some(&artifact_validation),
        );
    assert_eq!(status, "needs_repair");
    assert_eq!(
        detect_automation_node_failure_kind(
            &node,
            &status,
            approved,
            reason.as_deref(),
            Some(&artifact_validation),
        )
        .as_deref(),
        Some("required_tool_unused_read")
    );

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn generic_required_tools_nodes_default_to_five_attempts() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "artifact".to_string(),
        agent_id: "writer".to_string(),
        objective: "Write notes.md".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "artifact".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "notes.md",
                "required_tools": ["read"]
            }
        })),
    };

    assert_eq!(automation_node_max_attempts(&node), 5);
}

#[test]
fn first_attempt_required_tools_prompt_requires_completed_status() {
    let automation = AutomationV2Spec {
        automation_id: "automation-2".to_string(),
        name: "Generic Artifact Automation".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "artifact".to_string(),
        agent_id: "writer".to_string(),
        objective: "Write notes.md".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "artifact".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "notes.md",
                "required_tools": ["read"]
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "writer".to_string(),
        template_id: None,
        display_name: "Writer".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["read".to_string(), "write".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-2",
        &node,
        1,
        &agent,
        &[],
        &["read".to_string(), "write".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("`status` set to `completed`"));
    assert!(prompt.contains("required workflow tools remain available"));
    assert!(!prompt.contains("at least `status` (`completed` or `blocked`)"));
}

#[test]
fn first_attempt_required_tools_prompt_without_output_path_requires_handoff() {
    let automation = AutomationV2Spec {
        automation_id: "automation-structured".to_string(),
        name: "Structured Handoff Automation".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "discover".to_string(),
        agent_id: "research-discover".to_string(),
        objective: "Enumerate sources".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("local_research".to_string()),
                required_tools: vec!["read".to_string()],
                required_evidence: Vec::new(),
                required_sections: Vec::new(),
                prewrite_gates: vec![
                    "workspace_inspection".to_string(),
                    "concrete_reads".to_string(),
                ],
                retry_on_missing: vec![
                    "workspace_inspection".to_string(),
                    "concrete_reads".to_string(),
                ],
                terminal_on: Vec::new(),
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
            schema: None,
            summary_guidance: Some("Return a structured handoff.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "research_stage": "research_discover",
                "required_tools": ["read"]
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "research-discover".to_string(),
        template_id: None,
        display_name: "Research Discover".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["glob".to_string(), "read".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-structured",
        &node,
        1,
        &agent,
        &[],
        &["glob".to_string(), "read".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("`status` set to `completed`"));
    assert!(prompt.contains("required workflow tools remain available"));
    assert!(prompt.contains(
            "Do not claim success unless the required structured handoff was actually returned in the final response."
        ));
    assert!(!prompt.contains("write tool actually created the output file"));
}

#[test]
fn prompt_includes_inline_metadata_inputs_and_temp_file_warning() {
    let automation = AutomationV2Spec {
        automation_id: "automation-inline-inputs".to_string(),
        name: "Inline Inputs Automation".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "collect_inputs".to_string(),
        agent_id: "planner".to_string(),
        objective: "Capture the report topic, delivery target, and formatting constraints."
            .to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "inputs": {
                "topic": "autonomous AI agentic workflows",
                "delivery_email": "recipient@example.com",
                "email_format": "simple html"
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "planner".to_string(),
        template_id: None,
        display_name: "Planner".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["read".to_string(), "write".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-inline",
        &node,
        1,
        &agent,
        &[],
        &["read".to_string(), "write".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Node Inputs:"));
    assert!(prompt.contains("autonomous AI agentic workflows"));
    assert!(prompt.contains("Do not search `/tmp`"));
}

#[test]
fn collect_inputs_prompt_requires_reading_before_writing() {
    let automation = AutomationV2Spec {
        automation_id: "automation-collect-inputs".to_string(),
        name: "Collect Inputs".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "collect_inputs".to_string(),
        agent_id: "planner".to_string(),
        objective: "Inspect the workspace and ground the project identity before web research."
            .to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "collect-inputs.json"
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "planner".to_string(),
        template_id: None,
        display_name: "Planner".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["glob".to_string(), "read".to_string(), "write".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-collect-inputs",
        &node,
        1,
        &agent,
        &[],
        &["glob".to_string(), "read".to_string(), "write".to_string()],
        None,
        None,
        None,
    );

    let expected_output_path =
        crate::app::state::automation::automation_node_required_output_path_for_run(
            &node,
            Some("run-collect-inputs"),
        )
        .expect("scoped output path");
    assert!(prompt.contains("Required Run Artifact:"));
    assert!(prompt.contains("use `write` immediately to create the full file contents"));
    assert!(prompt.contains("Do not let an empty `glob` end the run"));
    assert!(prompt.contains(&expected_output_path));
}

#[test]
fn prompt_includes_email_delivery_metadata_for_notify_user() {
    let automation = AutomationV2Spec {
        automation_id: "automation-email-delivery".to_string(),
        name: "Email Delivery Automation".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "notify_user".to_string(),
        agent_id: "committer".to_string(),
        objective: "Send the finalized report to the requested email address in the email body using simple HTML.".to_string(),
        depends_on: vec!["generate_report".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "generate_report".to_string(),
            alias: "report_body".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "approval_gate".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ReviewDecision),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "delivery": {
                "method": "email",
                "to": "recipient@example.com",
                "content_type": "text/html",
                "inline_body_only": true,
                "attachments": false
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "committer".to_string(),
        template_id: None,
        display_name: "Committer".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["*".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: vec!["composio-1".to_string()],
            allowed_tools: None,
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-email",
        &node,
        1,
        &agent,
        &[json!({
            "alias": "report_body",
            "from_step_id": "generate_report",
            "output": {
                "content": {
                    "path": ".tandem/artifacts/generate-report.html",
                    "text": "<h1>Tandem Strategic Analysis</h1><p>Rich upstream report body.</p>"
                }
            }
        })],
        &["*".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Delivery target:"));
    assert!(prompt.contains("`recipient@example.com`"));
    assert!(prompt.contains("Inline body only: `true`"));
    assert!(prompt.contains("Upstream synthesis rules:"));
    assert!(
        prompt.contains("use the compiled upstream report/body as the email body source of truth")
    );
    assert!(prompt.contains("For email delivery, use the compiled upstream report/body as the email body source of truth."));
    assert!(prompt.contains("Deterministic Delivery Body:"));
    assert!(prompt.contains("Source artifact:"));
    assert!(prompt.contains("generate-report.html"));
    assert!(prompt.contains("<h1>Tandem Strategic Analysis</h1>"));
    assert!(prompt.contains(
        "Do not mark the node completed unless you actually execute an email draft or send tool."
    ));
}

#[test]
fn prompt_compacts_upstream_outputs_for_downstream_nodes() {
    let automation = AutomationV2Spec {
        automation_id: "automation-compact-upstream".to_string(),
        name: "Compact Upstream Automation".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "analyze_findings".to_string(),
        agent_id: "analyst".to_string(),
        objective: "Synthesize the clustered themes into a concise analysis and recommendations."
            .to_string(),
        depends_on: vec!["cluster_topics".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "cluster_topics".to_string(),
            alias: "topic_clusters".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: None,
    };
    let agent = AutomationAgentProfile {
        agent_id: "analyst".to_string(),
        template_id: None,
        display_name: "Analyst".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["*".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };
    let upstream_inputs = vec![json!({
        "alias": "topic_clusters",
        "from_step_id": "cluster_topics",
        "output": {
            "status": "completed",
            "phase": "completed",
            "summary": "Clustered pain points into report themes.",
            "contract_kind": "structured_json",
            "artifact_validation": {
                "accepted_artifact_path": ".tandem/artifacts/cluster-topics.json",
                "artifact_candidates": [{"source": "verified_output", "score": 999}],
                "validation_outcome": "passed",
                "warning_count": 0
            },
            "validator_summary": {
                "kind": "structured_json",
                "outcome": "passed",
                "warning_count": 0
            },
            "tool_telemetry": {
                "executed_tools": ["read", "write"],
                "tool_call_counts": {"read": 1, "write": 1}
            },
            "content": {
                "path": ".tandem/artifacts/cluster-topics.json",
                "raw_assistant_text": "very verbose narrative",
                "text": "{\"themes\":[{\"id\":\"T1\",\"summary\":\"alpha\"}],\"cross_cutting_observation\":\"beta\"}"
            }
        }
    })];

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-compact",
        &node,
        1,
        &agent,
        &upstream_inputs,
        &["*".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("\"themes\""));
    assert!(prompt.contains("\"cross_cutting_observation\""));
    assert!(!prompt.contains("artifact_candidates"));
    assert!(!prompt.contains("raw_assistant_text"));
    assert!(!prompt.contains("tool_call_counts"));
}

#[tokio::test]
async fn execute_collect_inputs_node_uses_deterministic_shortcut() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-collect-inputs-exec-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("workspace");

    let automation = AutomationV2Spec {
        automation_id: "automation-inline-collect-inputs".to_string(),
        name: "Collect Inputs Shortcut".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![AutomationAgentProfile {
            agent_id: "agent_planner".to_string(),
            template_id: None,
            display_name: "Planner".to_string(),
            avatar_url: None,
            model_policy: Some(json!({
                "default_model": "openrouter/not-a-real-model"
            })),
            skills: Vec::new(),
            tool_policy: AutomationAgentToolPolicy {
                allowlist: vec!["*".to_string()],
                denylist: Vec::new(),
            },
            mcp_policy: AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
            },
            approval_policy: None,
        }],
        flow: AutomationFlowSpec {
            nodes: vec![AutomationFlowNode {
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                node_id: "collect_inputs".to_string(),
                agent_id: "agent_planner".to_string(),
                objective: "Capture the report topic, delivery target, and formatting constraints."
                    .to_string(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: Some(AutomationFlowOutputContract {
                    kind: "brief".to_string(),
                    validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                    enforcement: None,
                    schema: None,
                    summary_guidance: None,
                }),
                retry_policy: None,
                timeout_ms: None,
                max_tool_calls: None,
                stage_kind: None,
                gate: None,
                metadata: Some(json!({
                    "inputs": {
                        "topic": "autonomous AI agentic workflows",
                        "delivery_email": "recipient@example.com",
                        "email_format": "simple html",
                        "attachments_allowed": false
                    }
                })),
            }],
        },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        creator_id: "test".to_string(),
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: Some(json!({
            "context_materialization": {
                "routines": [
                    {
                        "routine_id": "collect_inputs",
                        "visible_context_objects": [],
                        "step_context_bindings": [
                            {
                                "step_id": "collect_inputs",
                                "context_reads": ["ctx:collect_inputs:mission.goal"],
                                "context_writes": []
                            }
                        ]
                    }
                ]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };

    let state = ready_test_state().await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    assert_eq!(
        run.runtime_context
            .as_ref()
            .map(|context| context.routines.len()),
        Some(1)
    );
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.runtime_context = None;
        })
        .await
        .expect("clear runtime context");
    let claimed = state
        .claim_specific_automation_v2_run(&run.run_id)
        .await
        .expect("claim run");
    assert_eq!(
        claimed
            .runtime_context
            .as_ref()
            .map(|context| context.routines.len()),
        Some(1)
    );
    let node = automation.flow.nodes.first().expect("collect_inputs node");
    let agent = automation.agents.first().expect("planner agent");

    let output = execute_automation_v2_node(&state, &claimed.run_id, &automation, node, agent)
        .await
        .expect("execute collect_inputs");

    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        output
            .get("artifact_validation")
            .and_then(|value| value.get("deterministic_artifact"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        output
            .get("artifact_validation")
            .and_then(|value| value.get("deterministic_source"))
            .and_then(Value::as_str),
        Some("node_metadata_inputs")
    );

    let artifact_path = workspace_root
        .join(".tandem/runs")
        .join(&claimed.run_id)
        .join("artifacts")
        .join("collect-inputs.json");
    assert!(artifact_path.exists());
    let artifact_text = std::fs::read_to_string(&artifact_path).expect("artifact text");
    assert!(artifact_text.contains("autonomous AI agentic workflows"));

    let session_id = output
        .get("content")
        .and_then(|value| value.get("session_id"))
        .and_then(Value::as_str)
        .expect("session id");
    let session = state
        .storage
        .get_session(session_id)
        .await
        .expect("deterministic session");
    assert!(session.messages.iter().all(|message| {
        message
            .parts
            .iter()
            .all(|part| !matches!(part, tandem_types::MessagePart::ToolInvocation { .. }))
    }));

    let _ = std::fs::remove_dir_all(&workspace_root);
}
