use super::*;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use tandem_types::{MessageRole, PrewriteCoverageMode, Session};

use crate::app::state::automation::collect_automation_attempt_receipt_events;
use crate::app::state::automation::node_output::{
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
mod prompting;
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
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
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
    assert!(prompt.contains("Full Synthesis Requirement:"));
    assert!(prompt.contains("Deterministic Delivery Body:"));
    assert!(
        prompt.contains("Source artifact: `.tandem/runs/run-email/artifacts/generate-report.html`")
    );
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

#[tokio::test]
async fn automation_run_requeue_increments_attempt_counter() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-requeue-attempts-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("workspace");

    let automation = AutomationV2Spec {
        automation_id: "automation-inline-requeue-attempts".to_string(),
        name: "Requeue Attempt Increments".to_string(),
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
    };

    let state = ready_test_state().await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();

    crate::automation_v2::executor::run_automation_v2_run(state.clone(), run).await;
    let first = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("first run");
    assert_eq!(
        first.checkpoint.node_attempts.get("collect_inputs"),
        Some(&1)
    );

    state
        .update_automation_v2_run(&run_id, |row| {
            row.status = AutomationRunStatus::Queued;
            row.detail = Some("requeue collect_inputs".to_string());
            row.resume_reason = Some("requeue collect_inputs".to_string());
            row.stop_kind = None;
            row.stop_reason = None;
            row.pause_reason = None;
            row.checkpoint.awaiting_gate = None;
            row.checkpoint.node_outputs.remove("collect_inputs");
            row.checkpoint
                .completed_nodes
                .retain(|node_id| node_id != "collect_inputs");
            row.checkpoint
                .blocked_nodes
                .retain(|node_id| node_id != "collect_inputs");
            if !row
                .checkpoint
                .pending_nodes
                .iter()
                .any(|node_id| node_id == "collect_inputs")
            {
                row.checkpoint
                    .pending_nodes
                    .push("collect_inputs".to_string());
            }
            if row
                .checkpoint
                .last_failure
                .as_ref()
                .is_some_and(|failure| failure.node_id == "collect_inputs")
            {
                row.checkpoint.last_failure = None;
            }
        })
        .await
        .expect("requeue run");

    let rerun = state.get_automation_v2_run(&run_id).await.expect("rerun");
    crate::automation_v2::executor::run_automation_v2_run(state.clone(), rerun).await;
    let second = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("second run");
    assert_eq!(
        second.checkpoint.node_attempts.get("collect_inputs"),
        Some(&2)
    );
    assert!(second
        .checkpoint
        .node_outputs
        .contains_key("collect_inputs"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn automation_run_requires_stored_runtime_context_partition_at_startup() {
    let automation = AutomationV2Spec {
        automation_id: "auto-runtime-context-test".to_string(),
        name: "Runtime Context Test".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some(".".to_string()),
        metadata: Some(json!({
            "context_materialization": {
                "routines": [
                    {
                        "routine_id": "collect_inputs",
                        "visible_context_objects": [],
                        "step_context_bindings": []
                    }
                ]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };
    let state = ready_test_state().await;
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("store automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.runtime_context = None;
        })
        .await
        .expect("clear runtime context");
    let stored_before_clear = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("stored run before clear");
    assert!(state
        .automation_v2_runtime_context(&stored_before_clear)
        .is_some());
    let stored_run = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("stored run");
    assert!(state.automation_v2_runtime_context(&stored_run).is_some());

    crate::automation_v2::executor::run_automation_v2_run(state.clone(), stored_run).await;

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Failed);
    assert_eq!(
        persisted.detail.as_deref(),
        Some("runtime context partition missing for automation run")
    );
}

#[tokio::test]
async fn automation_run_rejects_invalid_activation_validation_snapshot() {
    let automation = AutomationV2Spec {
        automation_id: "auto-activation-validation-test".to_string(),
        name: "Activation Validation Test".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some(".".to_string()),
        metadata: Some(json!({
            "context_materialization": {
                "routines": [
                    {
                        "routine_id": "collect_inputs",
                        "visible_context_objects": [],
                        "step_context_bindings": []
                    }
                ]
            },
            "plan_package_validation": {
                "ready_for_apply": false,
                "ready_for_activation": false,
                "blocker_count": 1,
                "warning_count": 0,
                "validation_state": {},
                "issues": [
                    {
                        "code": "cross_routine_scope_overlap",
                        "severity": "error",
                        "path": "routines[0]",
                        "message": "scope leak",
                        "blocking": true
                    }
                ]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };
    let state = ready_test_state().await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();

    crate::automation_v2::executor::run_automation_v2_run(state.clone(), run).await;

    let persisted = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Failed);
    assert_eq!(
        persisted.detail.as_deref(),
        Some("plan package not ready for activation: scope leak (cross_routine_scope_overlap)")
    );
}

#[tokio::test]
async fn automation_v2_approved_plan_materialization_is_recovered_from_snapshot() {
    let automation = AutomationV2Spec {
        automation_id: "auto-approved-plan-test".to_string(),
        name: "Approved Plan Test".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some(".".to_string()),
        metadata: Some(json!({
            "plan_package_bundle": {
                "scope_snapshot": {
                    "plan_id": "plan-approved-1",
                    "plan_revision": 4,
                    "context_objects": [
                        {
                            "context_object_id": "ctx:plan:goal",
                            "name": "Plan goal",
                            "kind": "mission_goal",
                            "scope": "mission",
                            "owner_routine_id": "routine_a",
                            "declared_consumers": ["routine_a"],
                            "data_scope_refs": ["mission.goal"],
                            "validation_status": "pending",
                            "provenance": {
                                "plan_id": "plan-approved-1",
                                "routine_id": "routine_a"
                            },
                            "summary": "Plan goal"
                        }
                    ],
                    "credential_envelopes": []
                }
            },
            "approved_plan_materialization": {
                "plan_id": "plan-approved-1",
                "plan_revision": 4,
                "lifecycle_state": "approved",
                "routine_count": 1,
                "step_count": 1,
                "context_object_count": 1,
                "routines": [
                    {
                        "routine_id": "routine_a",
                        "step_ids": ["step_a"],
                        "visible_context_object_ids": ["ctx:plan:goal"],
                        "step_context_bindings": [
                            {
                                "step_id": "step_a",
                                "context_reads": ["ctx:plan:goal"],
                                "context_writes": []
                            }
                        ]
                    }
                ]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };

    let state = ready_test_state().await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let runtime_context = state
        .automation_v2_runtime_context(&run)
        .expect("runtime context from approved plan");
    let snapshot = state
        .automation_v2_approved_plan_materialization(&run)
        .expect("approved plan materialization");
    assert_eq!(snapshot.plan_id, "plan-approved-1");
    assert_eq!(snapshot.plan_revision, 4);
    assert_eq!(snapshot.routine_count, 1);
    assert_eq!(snapshot.step_count, 1);
    assert_eq!(runtime_context.routines.len(), 1);
    assert_eq!(
        runtime_context.routines[0].visible_context_objects[0].context_object_id,
        "ctx:plan:goal"
    );
    assert_eq!(
        runtime_context.routines[0].step_context_bindings[0].step_id,
        "step_a"
    );
    assert_eq!(
        automation
            .approved_plan_materialization()
            .as_ref()
            .map(|materialization| materialization.plan_id.as_str()),
        Some("plan-approved-1")
    );
}

#[test]
fn first_attempt_structured_json_prompt_without_output_path_requires_handoff_even_without_enforcement(
) {
    let automation = AutomationV2Spec {
        automation_id: "automation-structured-defaults".to_string(),
        name: "Structured Handoff Defaults".to_string(),
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
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "prompt": "Enumerate the workspace and identify source files."
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
        "run-structured-defaults",
        &node,
        1,
        &agent,
        &[],
        &["glob".to_string(), "read".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Structured Handoff Expectation"));
    assert!(prompt.contains("`status` set to `completed`"));
    assert!(prompt.contains(
            "Do not claim success unless the required structured handoff was actually returned in the final response."
        ));
}

#[test]
fn report_generation_objective_does_not_imply_email_delivery_execution() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Draft the report in simple HTML suitable for email body delivery.".to_string(),
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
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/generate-report.html"
            }
        })),
    };

    assert!(!crate::app::state::automation::automation_node_requires_email_delivery(&node));
}

#[test]
fn execute_goal_objective_with_gmail_draft_or_send_requires_email_delivery() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "operator".to_string(),
        objective: "Create a Gmail draft or send the final HTML summary email to recipient@example.com if mail tools are available.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "approval_gate".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ReviewDecision),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: None,
    };

    assert!(crate::app::state::automation::automation_node_requires_email_delivery(&node));
}

#[test]
fn email_delivery_status_uses_recipient_from_objective_when_metadata_missing() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "operator".to_string(),
        objective: "Create a Gmail draft or send the final HTML summary email to recipient@example.com if mail tools are available.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "approval_gate".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ReviewDecision),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: None,
    };

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "A Gmail draft has been created.\n\n{\"status\":\"completed\",\"approved\":true}",
            None,
            &json!({
                "requested_tools": ["glob", "read", "mcp_list"],
                "executed_tools": ["read", "glob", "mcp_list"],
                "tool_call_counts": {"read": 1, "glob": 1, "mcp_list": 1},
                "workspace_inspection_used": true,
                "email_delivery_attempted": false,
                "email_delivery_succeeded": false,
                "latest_email_delivery_failure": null,
                "capability_resolution": {
                    "email_tool_diagnostics": {
                        "available_tools": ["mcp.composio_1.gmail_send_email", "mcp.composio_1.gmail_create_email_draft"],
                        "offered_tools": ["mcp.composio_1.gmail_send_email", "mcp.composio_1.gmail_create_email_draft"],
                        "available_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "offered_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "available_draft_tools": ["mcp.composio_1.gmail_create_email_draft"],
                        "offered_draft_tools": ["mcp.composio_1.gmail_create_email_draft"]
                    }
                }
            }),
            None,
        );

    assert_eq!(status, "blocked");
    assert_eq!(
        reason.as_deref(),
        Some(
            "email delivery to `recipient@example.com` was requested but no email draft/send tool executed"
        )
    );
    assert_eq!(approved, Some(true));
}

#[test]
fn research_workflow_failure_kind_detects_missing_citations() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let artifact_validation = json!({
        "semantic_block_reason": "research completed without citation-backed claims",
        "unmet_requirements": ["citations_missing", "web_sources_reviewed_missing"],
        "verification": {
            "verification_failed": false
        }
    });

    assert_eq!(
        detect_automation_node_failure_kind(
            &node,
            "blocked",
            None,
            Some("research completed without citation-backed claims"),
            Some(&artifact_validation),
        )
        .as_deref(),
        Some("research_citations_missing")
    );
    assert_eq!(
        detect_automation_node_phase(&node, "blocked", Some(&artifact_validation)),
        "research_validation"
    );
}

#[test]
fn research_workflow_defaults_to_warning_without_strict_source_coverage() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true,
                "allow_preexisting_output_reuse": true
            }
        })),
    };
    let artifact_validation = json!({
        "unmet_requirements": ["no_concrete_reads", "citations_missing", "missing_successful_web_research"],
        "verification": {
            "verification_failed": false
        }
    });

    assert_eq!(
        detect_automation_node_failure_kind(
            &node,
            "completed",
            None,
            None,
            Some(&artifact_validation)
        ),
        None
    );
    assert_eq!(
        detect_automation_node_phase(&node, "completed", Some(&artifact_validation)),
        "completed"
    );
}

#[test]
fn validator_summary_reports_repair_attempt_state() {
    let artifact_validation = json!({
        "semantic_block_reason": "research completed without citation-backed claims",
        "unmet_requirements": ["citations_missing"],
        "repair_attempted": true,
        "repair_attempt": 2,
        "repair_attempts_remaining": 0,
        "repair_succeeded": false,
        "repair_exhausted": true,
    });
    let summary = build_automation_validator_summary(
        crate::AutomationOutputValidatorKind::ResearchBrief,
        "blocked",
        Some("research completed without citation-backed claims"),
        Some(&artifact_validation),
    );
    assert!(summary.repair_attempted);
    assert_eq!(summary.repair_attempt, 2);
    assert_eq!(summary.repair_attempts_remaining, 0);
    assert!(!summary.repair_succeeded);
    assert!(summary.repair_exhausted);
}

#[test]
fn artifact_validation_uses_structured_repair_exhaustion_state_from_session_text() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-repair-state-test-{}", now_ms()));
    std::fs::create_dir_all(workspace_root.join("inputs")).expect("create workspace");
    std::fs::write(workspace_root.join("inputs/questions.md"), "Question")
        .expect("seed input file");

    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let mut session = Session::new(Some("research repair exhausted".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path":"marketing-brief.md",
                "content":"# Marketing Brief\n\n## Findings\nBlocked draft without citations.\n"
            }),
            result: Some(json!({"output":"written"})),
            error: None,
        }],
    ));
    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &[
            "glob".to_string(),
            "read".to_string(),
            "websearch".to_string(),
            "write".to_string(),
        ],
    );
    let session_text = r#"TOOL_MODE_REQUIRED_NOT_SATISFIED: PREWRITE_REQUIREMENTS_EXHAUSTED

{"status":"blocked","reason":"prewrite requirements exhausted before final artifact validation","failureCode":"PREWRITE_REQUIREMENTS_EXHAUSTED","repairAttempt":2,"repairAttemptsRemaining":0,"repairExhausted":true,"unmetRequirements":["concrete_read_required","successful_web_research_required"]}"#;
    let (_accepted_output, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        session_text,
        &tool_telemetry,
        None,
        Some((
            "marketing-brief.md".to_string(),
            "# Marketing Brief\n\n## Findings\nBlocked draft without citations.\n".to_string(),
        )),
        &std::collections::BTreeSet::new(),
    );
    assert!(rejected.is_some());
    assert_eq!(
        metadata.get("repair_attempt").and_then(Value::as_u64),
        Some(2)
    );
    assert_eq!(
        metadata
            .get("repair_attempts_remaining")
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        metadata.get("repair_exhausted").and_then(Value::as_bool),
        Some(true)
    );
    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn research_artifact_validation_requires_citations_and_web_sources_reviewed() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-research-citation-test-{}", now_ms()));
    std::fs::create_dir_all(workspace_root.join("inputs")).expect("create workspace");
    std::fs::write(workspace_root.join("inputs/questions.md"), "Question")
        .expect("seed input file");

    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let mut session = Session::new(Some("research citations".to_string()), None);
    session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![
                MessagePart::ToolInvocation {
                    tool: "read".to_string(),
                    args: json!({"path":"inputs/questions.md"}),
                    result: Some(json!({"output":"Question"})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "websearch".to_string(),
                    args: json!({"query":"market trends"}),
                    result: Some(json!({"output":"Search results found"})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "write".to_string(),
                    args: json!({
                        "path":"marketing-brief.md",
                        "content":"# Marketing Brief\n\n## Files reviewed\n- inputs/questions.md\n\n## Files not reviewed\n- inputs/references.md: not available in this run.\n\n## Findings\nClaims are summarized here without explicit citations.\n"
                    }),
                    result: Some(json!({"output":"written"})),
                    error: None,
                },
            ],
        ));

    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &[
            "read".to_string(),
            "write".to_string(),
            "websearch".to_string(),
        ],
    );
    let (_, artifact_validation, rejected) = validate_automation_artifact_output(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            "",
            &tool_telemetry,
            None,
            Some((
                "marketing-brief.md".to_string(),
                "# Marketing Brief\n\n## Files reviewed\n- inputs/questions.md\n\n## Findings\nClaims are summarized here without explicit citations.\n".to_string(),
            )),
            &std::collections::BTreeSet::new(),
        );

    assert_eq!(
        rejected.as_deref(),
        Some("research completed without citation-backed claims")
    );
    assert_eq!(
        artifact_validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        vec![
            json!("citations_missing"),
            json!("web_sources_reviewed_missing")
        ]
    );
    assert_eq!(
        artifact_validation
            .get("artifact_candidates")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|value| value.get("citation_count"))
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        artifact_validation
            .get("citation_count")
            .and_then(Value::as_u64),
        Some(0)
    );
    assert_eq!(
        artifact_validation
            .get("web_sources_reviewed_present")
            .and_then(Value::as_bool),
        Some(false)
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn research_citations_validation_accepts_external_research_without_files_reviewed_section() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-research-sources-test-{}", now_ms()));
    std::fs::create_dir_all(workspace_root.join("inputs")).expect("create workspace");
    std::fs::write(workspace_root.join("inputs/questions.md"), "Question")
        .expect("seed input file");

    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research_sources".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Research current web sources".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "citations".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Return a citation handoff.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/research-sources.json",
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let mut session = Session::new(Some("research sources".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"path":"inputs/questions.md"}),
                result: Some(json!({"output":"Question"})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "websearch".to_string(),
                args: json!({"query":"autonomous AI agentic workflows 2024 2025"}),
                result: Some(json!({"output":"Search results found"})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path":".tandem/artifacts/research-sources.json",
                    "content":"# Research Sources\n\n## Summary\nCurrent external research was gathered successfully.\n\n## Citations\n1. AI Agents in 2025: Expectations vs. Reality | IBM. Source note: https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality\n2. Agentic AI, explained | MIT Sloan. Source note: https://mitsloan.mit.edu/ideas-made-to-matter/agentic-ai-explained\n\n## Web sources reviewed\n- https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality\n- https://mitsloan.mit.edu/ideas-made-to-matter/agentic-ai-explained\n"
                }),
                result: Some(json!({"output":"written"})),
                error: None,
            },
        ],
    ));

    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &[
            "read".to_string(),
            "write".to_string(),
            "websearch".to_string(),
        ],
    );
    let (_accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "",
        &tool_telemetry,
        None,
        Some((
            ".tandem/artifacts/research-sources.json".to_string(),
            "# Research Sources\n\n## Summary\nCurrent external research was gathered successfully.\n\n## Citations\n1. AI Agents in 2025: Expectations vs. Reality | IBM. Source note: https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality\n2. Agentic AI, explained | MIT Sloan. Source note: https://mitsloan.mit.edu/ideas-made-to-matter/agentic-ai-explained\n\n## Web sources reviewed\n- https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality\n- https://mitsloan.mit.edu/ideas-made-to-matter/agentic-ai-explained\n".to_string(),
        )),
        &std::collections::BTreeSet::new(),
    );

    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert!(!artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("files_reviewed_missing"))));
    assert!(!artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("files_reviewed_not_backed_by_read"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn marketing_template_automation_migrates_to_split_research_flow() {
    let mut automation = AutomationV2Spec {
        automation_id: "automation-v2-test".to_string(),
        name: "Marketing Content Pipeline".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![
            AutomationAgentProfile {
                agent_id: "research".to_string(),
                template_id: None,
                display_name: "Research".to_string(),
                avatar_url: None,
                model_policy: None,
                skills: vec!["analysis".to_string()],
                tool_policy: AutomationAgentToolPolicy {
                    allowlist: vec![
                        "read".to_string(),
                        "write".to_string(),
                        "websearch".to_string(),
                        "glob".to_string(),
                    ],
                    denylist: Vec::new(),
                },
                mcp_policy: AutomationAgentMcpPolicy {
                    allowed_servers: Vec::new(),
                    allowed_tools: None,
                },
                approval_policy: None,
            },
            AutomationAgentProfile {
                agent_id: "copywriter".to_string(),
                template_id: None,
                display_name: "Copywriter".to_string(),
                avatar_url: None,
                model_policy: None,
                skills: Vec::new(),
                tool_policy: AutomationAgentToolPolicy {
                    allowlist: vec!["read".to_string(), "write".to_string()],
                    denylist: Vec::new(),
                },
                mcp_policy: AutomationAgentMcpPolicy {
                    allowed_servers: Vec::new(),
                    allowed_tools: None,
                },
                approval_policy: None,
            },
        ],
        flow: AutomationFlowSpec {
            nodes: vec![
                AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "research-brief".to_string(),
                    agent_id: "research".to_string(),
                    objective: "Legacy research".to_string(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: Some(AutomationFlowOutputContract {
                        kind: "brief".to_string(),
                        validator: None,
                        enforcement: None,
                        schema: None,
                        summary_guidance: Some("Write `marketing-brief.md`.".to_string()),
                    }),
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: None,
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Research Brief",
                            "role": "watcher",
                            "output_path": "marketing-brief.md",
                            "prompt": "Legacy one-shot research prompt"
                        },
                        "studio": {
                            "output_path": "marketing-brief.md"
                        }
                    })),
                },
                AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "draft-copy".to_string(),
                    agent_id: "copywriter".to_string(),
                    objective: "Draft copy".to_string(),
                    depends_on: vec!["research-brief".to_string()],
                    input_refs: vec![AutomationFlowInputRef {
                        from_step_id: "research-brief".to_string(),
                        alias: "marketing_brief".to_string(),
                    }],
                    output_contract: Some(AutomationFlowOutputContract {
                        kind: "draft".to_string(),
                        validator: None,
                        enforcement: None,
                        schema: None,
                        summary_guidance: None,
                    }),
                    retry_policy: None,
                    timeout_ms: None,
                    stage_kind: None,
                    gate: None,
                    metadata: None,
                },
            ],
        },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp/workspace".to_string()),
        metadata: Some(json!({
            "studio": {
                "template_id": "marketing-content-pipeline",
                "version": 1,
                "agent_drafts": [{"agentId":"research"}],
                "node_drafts": [{"nodeId":"research-brief"}]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
    };

    assert!(migrate_bundled_studio_research_split_automation(
        &mut automation
    ));
    assert!(automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == "research-discover-sources"));
    assert!(automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == "research-local-sources"));
    assert!(automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == "research-external-research"));
    let discover_node = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "research-discover-sources")
        .expect("discover node present");
    let discover_enforcement = discover_node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.enforcement.as_ref())
        .expect("discover enforcement");
    assert!(discover_enforcement
        .required_tools
        .iter()
        .any(|tool| tool == "read"));
    assert!(discover_enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "workspace_inspection"));
    assert!(discover_enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "concrete_reads"));
    let final_node = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "research-brief")
        .expect("final node preserved");
    assert_eq!(
        automation_node_research_stage(final_node).as_deref(),
        Some("research_finalize")
    );
    assert_eq!(final_node.depends_on.len(), 3);
    assert!(automation
        .agents
        .iter()
        .any(|agent| agent.agent_id == "research-discover"));
    assert!(automation
        .agents
        .iter()
        .any(|agent| agent.agent_id == "research-local-sources"));
    assert!(automation
        .agents
        .iter()
        .any(|agent| agent.agent_id == "research-external"));
    let studio = automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("studio"))
        .and_then(Value::as_object)
        .expect("studio metadata");
    assert_eq!(studio.get("version").and_then(Value::as_u64), Some(2));
    assert_eq!(
        studio
            .get("workflow_structure_version")
            .and_then(Value::as_u64),
        Some(2)
    );
    assert!(!studio.contains_key("agent_drafts"));
    assert!(!studio.contains_key("node_drafts"));
}

#[test]
fn research_finalize_validation_accepts_upstream_read_evidence() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-research-finalize-test-{}", now_ms()));
    std::fs::create_dir_all(workspace_root.join("inputs")).expect("create workspace");
    std::fs::write(workspace_root.join("inputs/questions.md"), "Question")
        .expect("seed input file");

    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-brief".to_string(),
        agent_id: "research".to_string(),
        objective: "Write marketing brief".to_string(),
        depends_on: vec![
            "research-discover-sources".to_string(),
            "research-local-sources".to_string(),
            "research-external-research".to_string(),
        ],
        input_refs: vec![
            AutomationFlowInputRef {
                from_step_id: "research-discover-sources".to_string(),
                alias: "source_inventory".to_string(),
            },
            AutomationFlowInputRef {
                from_step_id: "research-local-sources".to_string(),
                alias: "local_source_notes".to_string(),
            },
            AutomationFlowInputRef {
                from_step_id: "research-external-research".to_string(),
                alias: "external_research".to_string(),
            },
        ],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("research_synthesis".to_string()),
                required_tools: Vec::new(),
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: vec![
                    "files_reviewed".to_string(),
                    "files_not_reviewed".to_string(),
                    "citations".to_string(),
                ],
                prewrite_gates: Vec::new(),
                retry_on_missing: vec![
                    "local_source_reads".to_string(),
                    "files_reviewed".to_string(),
                    "files_not_reviewed".to_string(),
                    "citations".to_string(),
                ],
                terminal_on: Vec::new(),
                repair_budget: Some(5),
                session_text_recovery: None,
            }),
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "research_stage": "research_finalize",
                "title": "Research Brief",
                "role": "watcher"
            }
        })),
    };

    let mut session = Session::new(Some("research finalize".to_string()), None);
    session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path":"marketing-brief.md",
                    "content":"# Marketing Brief\n\n## Files reviewed\n- inputs/questions.md\n\n## Files not reviewed\n- inputs/extra.md: not needed for this test.\n\n## Proof Points With Citations\n1. Supported claim. Source note: https://example.com/reference\n"
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
    let upstream_evidence = AutomationUpstreamEvidence {
        read_paths: vec!["inputs/questions.md".to_string()],
        discovered_relevant_paths: vec!["inputs/questions.md".to_string()],
        web_research_attempted: false,
        web_research_succeeded: false,
        citation_count: 0,
        citations: vec![],
    };
    let (accepted_output, artifact_validation, rejected) =
            validate_automation_artifact_output_with_upstream(
                &node,
                &session,
                workspace_root.to_str().expect("workspace root"),
                None,
                "",
                &tool_telemetry,
                None,
                Some((
                    "marketing-brief.md".to_string(),
                    "# Marketing Brief\n\n## Files reviewed\n- inputs/questions.md\n\n## Files not reviewed\n- inputs/extra.md: not needed for this test.\n\n## Proof Points With Citations\n1. Supported claim. Source note: https://example.com/reference\n".to_string(),
                )),
                &std::collections::BTreeSet::new(),
                Some(&upstream_evidence),
            );

    assert!(accepted_output.is_some(), "{artifact_validation:?}");
    assert!(
        rejected.is_none(),
        "rejected={rejected:?} metadata={artifact_validation:?}"
    );
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        artifact_validation
            .get("upstream_evidence_applied")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        artifact_validation
            .get("read_paths")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        vec![json!("inputs/questions.md")]
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validator_summary_tracks_verification_and_repair_state() {
    let artifact_validation = json!({
        "accepted_candidate_source": "session_write",
        "repair_attempted": true,
        "repair_succeeded": true,
        "validation_basis": {
            "authority": "filesystem_and_receipts",
            "current_attempt_output_materialized": true
        },
        "verification": {
            "verification_outcome": "passed"
        }
    });

    let summary = build_automation_validator_summary(
        crate::AutomationOutputValidatorKind::CodePatch,
        "done",
        None,
        Some(&artifact_validation),
    );

    assert_eq!(
        summary.kind,
        crate::AutomationOutputValidatorKind::CodePatch
    );
    assert_eq!(summary.outcome, "passed");
    assert_eq!(
        summary.accepted_candidate_source.as_deref(),
        Some("session_write")
    );
    assert_eq!(summary.verification_outcome.as_deref(), Some("passed"));
    assert_eq!(
        summary
            .validation_basis
            .as_ref()
            .and_then(|value| value.get("authority"))
            .and_then(Value::as_str),
        Some("filesystem_and_receipts")
    );
    assert!(summary.repair_attempted);
    assert!(summary.repair_succeeded);
}

#[test]
fn generic_artifact_validation_blocks_weak_report_markdown() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-editorial-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("workspace dir");
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "draft-report".to_string(),
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
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "report.md"
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
    let (_, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "",
        &tool_telemetry,
        None,
        Some(("report.md".to_string(), "# Draft\n\nTODO\n".to_string())),
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(
        rejected.as_deref(),
        Some("editorial artifact is missing expected markdown structure")
    );
    assert_eq!(
        artifact_validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        vec![
            json!("editorial_substance_missing"),
            json!("markdown_structure_missing")
        ]
    );
    assert_eq!(
        artifact_validation
            .get("heading_count")
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        artifact_validation
            .get("paragraph_count")
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        detect_automation_node_failure_kind(
            &node,
            "blocked",
            None,
            None,
            Some(&artifact_validation),
        ),
        Some("editorial_quality_failed".to_string())
    );
    assert_eq!(
        detect_automation_node_phase(&node, "blocked", Some(&artifact_validation)),
        "editorial_validation"
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn publish_node_blocks_when_upstream_editorial_validation_failed() {
    let publish = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "publish".to_string(),
        agent_id: "publisher".to_string(),
        objective: "Publish final output".to_string(),
        depends_on: vec!["draft".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "draft".to_string(),
            alias: "draft".to_string(),
        }],
        output_contract: None,
        retry_policy: None,
        timeout_ms: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "role": "publisher"
            }
        })),
    };
    let mut run = test_phase_run(vec!["publish"], vec!["draft"]);
    run.checkpoint.node_outputs.insert(
        "draft".to_string(),
        json!({
            "node_id": "draft",
            "failure_kind": "editorial_quality_failed",
            "phase": "editorial_validation",
            "validator_summary": {
                "unmet_requirements": ["editorial_substance_missing", "markdown_structure_missing"]
            }
        }),
    );

    let reason = automation_publish_editorial_block_reason(&run, &publish).expect("publish block");
    assert!(reason.contains("draft"));
    assert!(reason.contains("editorial"));
}

#[test]
fn report_markdown_blocks_when_rich_upstream_evidence_is_reduced_to_generic_summary() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-upstream-synthesis-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Create the final report".to_string(),
        depends_on: vec!["analyze_findings".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "analyze_findings".to_string(),
            alias: "analysis".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "report.md"
            }
        })),
    };
    let session = Session::new(
        Some("thin-final-summary".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let mut session = session;
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "report.md",
                "content": "# Strategic Summary\n\nTandem is an engineering agent for local execution.\n\n## Positioning\n\nIt connects human intent to repo changes.\n"
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));
    let thin_report = "# Strategic Summary\n\nTandem is an engineering agent for local execution.\n\n## Positioning\n\nIt connects human intent to repo changes.\n".to_string();
    let upstream_evidence = AutomationUpstreamEvidence {
        read_paths: vec![
            "README.md".to_string(),
            "docs/product-capabilities.md".to_string(),
        ],
        discovered_relevant_paths: vec![
            "README.md".to_string(),
            "docs/product-capabilities.md".to_string(),
        ],
        web_research_attempted: true,
        web_research_succeeded: true,
        citation_count: 3,
        citations: vec![
            "https://example.com/source-1".to_string(),
            "https://example.com/source-2".to_string(),
            "https://example.com/source-3".to_string(),
        ],
    };

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            None,
            "Completed the report.",
            &json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            None,
            Some(("report.md".to_string(), thin_report)),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_some());
    assert_eq!(
        rejected.as_deref(),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn report_markdown_accepts_structured_synthesis_without_inline_citations_when_upstream_is_rich() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-upstream-synthesis-pass-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "analyze_findings".to_string(),
        agent_id: "analyst".to_string(),
        objective: "Synthesize findings into a strategy report".to_string(),
        depends_on: vec!["collect_inputs".to_string(), "research_sources".to_string()],
        input_refs: vec![
            AutomationFlowInputRef {
                from_step_id: "collect_inputs".to_string(),
                alias: "local_grounding".to_string(),
            },
            AutomationFlowInputRef {
                from_step_id: "research_sources".to_string(),
                alias: "external_research".to_string(),
            },
        ],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "analyze-findings.md"
            }
        })),
    };
    let mut session = Session::new(
        Some("structured-synthesis".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let report = "# Strategy Analysis Report\n\n## 1. Executive Summary\nThis analysis synthesizes Tandem's core internal product definitions and external research to refine positioning and strategy. Tandem is positioned as a high-autonomy, agentic engineering engine that solves cognitive load and cross-functional orchestration issues, positioning itself firmly against generic code assistants.\n\n## 2. Product Positioning\n*   **Core Identity:** Tandem by Frumu AI\n*   **Market Category:** Agentic Software Development / IDE-Integrated Engineering Tool\n*   **Key Positioning:** Empowering engineers with high-autonomy, context-accurate AI collaboration embedded directly within their development workflow.\n\n## 3. Target Users & Use-Case Wedges\n*   **Primary Users:** Professional engineers and development teams struggling with high cognitive load.\n*   **Use-Case Wedge:** Utilizing workspace-aware code analysis and automated task execution to bridge the gap between documentation and implementation.\n\n## 4. Investor Narrative & Competitive Outlook\n*   **Competitive Standing:** Tandem differentiates itself by being a full-context engineering engine rather than a simple chatbot.\n*   **Narrative Hook:** Stop context-switching and let Tandem handle tooling and documentation synthesis overhead.\n\n## 5. Risks & Proof Gaps\n*   **Market Risk:** Strong competition from well-capitalized code-assistant vendors.\n*   **Proof Gaps:** Need stronger empirical time-saved and throughput metrics.\n\n## 6. Execution Summary\nThe immediate priority is to prove the agentic value proposition with high-utility automation flows such as multi-file updates and refactors.\n\n---\n*Source Verification: Based on `.tandem/artifacts/collect-inputs.json` and `.tandem/artifacts/research-sources.json`.*\n".to_string();
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "analyze-findings.md",
                "content": report
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));
    let upstream_evidence = AutomationUpstreamEvidence {
        read_paths: vec![
            ".tandem/artifacts/collect-inputs.json".to_string(),
            ".tandem/artifacts/research-sources.json".to_string(),
        ],
        discovered_relevant_paths: vec![
            ".tandem/artifacts/collect-inputs.json".to_string(),
            "README.md".to_string(),
        ],
        web_research_attempted: true,
        web_research_succeeded: true,
        citation_count: 3,
        citations: vec![
            "https://example.com/1".to_string(),
            "https://example.com/2".to_string(),
            "https://example.com/3".to_string(),
        ],
    };

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            None,
            "Completed the report.",
            &json!({
                "requested_tools": ["read", "write"],
                "executed_tools": ["read", "write"],
                "tool_call_counts": {
                    "read": 2,
                    "write": 1
                }
            }),
            None,
            Some(("analyze-findings.md".to_string(), report)),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_some());
    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        None
    );
    assert!(!artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn report_markdown_legacy_metadata_is_forced_to_strict_without_emergency_rollback() {
    with_legacy_quality_rollback_enabled(false, || {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-report-forced-strict-quality-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace_root).expect("create workspace");
        let snapshot = automation_workspace_root_file_snapshot(
            workspace_root.to_str().expect("workspace root"),
        );
        let node = AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "generate_report".to_string(),
            agent_id: "writer".to_string(),
            objective: "Create the final report".to_string(),
            depends_on: vec!["analyze_findings".to_string()],
            input_refs: vec![AutomationFlowInputRef {
                from_step_id: "analyze_findings".to_string(),
                alias: "analysis".to_string(),
            }],
            output_contract: Some(AutomationFlowOutputContract {
                kind: "report_markdown".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                enforcement: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "quality_mode": "legacy",
                "builder": {
                    "output_path": "generate-report.md"
                }
            })),
        };
        let mut session = Session::new(
            Some("legacy-quality-mode".to_string()),
            Some(workspace_root.to_str().expect("workspace root").to_string()),
        );
        let generic_report = "# Summary\n\nPlaceholder update.\n".to_string();
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": "generate-report.md",
                    "content": generic_report
                }),
                result: Some(json!("ok")),
                error: None,
            }],
        ));
        let upstream_evidence = AutomationUpstreamEvidence {
            read_paths: vec![
                ".tandem/artifacts/collect-inputs.json".to_string(),
                ".tandem/artifacts/research-sources.json".to_string(),
            ],
            discovered_relevant_paths: vec![
                ".tandem/artifacts/collect-inputs.json".to_string(),
                ".tandem/artifacts/research-sources.json".to_string(),
            ],
            web_research_attempted: true,
            web_research_succeeded: true,
            citation_count: 3,
            citations: vec![
                "https://example.com/legacy-1".to_string(),
                "https://example.com/legacy-2".to_string(),
                "https://example.com/legacy-3".to_string(),
            ],
        };

        let (_accepted_output, artifact_validation, rejected) =
            validate_automation_artifact_output_with_upstream(
                &node,
                &session,
                workspace_root.to_str().expect("workspace root"),
                None,
                "Completed the report.",
                &json!({
                    "requested_tools": ["read", "write"],
                    "executed_tools": ["read", "write"],
                    "tool_call_counts": {
                        "read": 2,
                        "write": 1
                    }
                }),
                None,
                Some(("generate-report.md".to_string(), generic_report)),
                &snapshot,
                Some(&upstream_evidence),
            );

        assert!(rejected.is_some());
        assert!(artifact_validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .is_some_and(|items| items
                .iter()
                .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));
        assert_eq!(
            artifact_validation
                .get("validation_basis")
                .and_then(|value| value.get("quality_mode"))
                .and_then(Value::as_str),
            Some("strict_research_v1")
        );
        assert_eq!(
            artifact_validation
                .get("validation_basis")
                .and_then(|value| value.get("requested_quality_mode"))
                .and_then(Value::as_str),
            Some("legacy")
        );
        assert_eq!(
            artifact_validation
                .get("validation_basis")
                .and_then(|value| value.get("legacy_quality_rollback_enabled"))
                .and_then(Value::as_bool),
            Some(false)
        );

        let _ = std::fs::remove_dir_all(&workspace_root);
    });
}

#[test]
fn report_markdown_legacy_quality_mode_allows_generic_synthesis_with_emergency_rollback() {
    with_legacy_quality_rollback_enabled(true, || {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-report-legacy-quality-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace_root).expect("create workspace");
        let snapshot = automation_workspace_root_file_snapshot(
            workspace_root.to_str().expect("workspace root"),
        );
        let node = AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "generate_report".to_string(),
            agent_id: "writer".to_string(),
            objective: "Create the final report".to_string(),
            depends_on: vec!["analyze_findings".to_string()],
            input_refs: vec![AutomationFlowInputRef {
                from_step_id: "analyze_findings".to_string(),
                alias: "analysis".to_string(),
            }],
            output_contract: Some(AutomationFlowOutputContract {
                kind: "report_markdown".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                enforcement: None,
                schema: None,
                summary_guidance: None,
            }),
            retry_policy: None,
            timeout_ms: None,
            stage_kind: None,
            gate: None,
            metadata: Some(json!({
                "quality_mode": "legacy",
                "builder": {
                    "output_path": "generate-report.md"
                }
            })),
        };
        let mut session = Session::new(
            Some("legacy-quality-mode".to_string()),
            Some(workspace_root.to_str().expect("workspace root").to_string()),
        );
        let generic_report = "# Summary\n\nPlaceholder update.\n".to_string();
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": "generate-report.md",
                    "content": generic_report
                }),
                result: Some(json!("ok")),
                error: None,
            }],
        ));
        let upstream_evidence = AutomationUpstreamEvidence {
            read_paths: vec![
                ".tandem/artifacts/collect-inputs.json".to_string(),
                ".tandem/artifacts/research-sources.json".to_string(),
            ],
            discovered_relevant_paths: vec![
                ".tandem/artifacts/collect-inputs.json".to_string(),
                ".tandem/artifacts/research-sources.json".to_string(),
            ],
            web_research_attempted: true,
            web_research_succeeded: true,
            citation_count: 3,
            citations: vec![
                "https://example.com/legacy-1".to_string(),
                "https://example.com/legacy-2".to_string(),
                "https://example.com/legacy-3".to_string(),
            ],
        };

        let (accepted_output, artifact_validation, rejected) =
            validate_automation_artifact_output_with_upstream(
                &node,
                &session,
                workspace_root.to_str().expect("workspace root"),
                None,
                "Completed the report.",
                &json!({
                    "requested_tools": ["read", "write"],
                    "executed_tools": ["read", "write"],
                    "tool_call_counts": {
                        "read": 2,
                        "write": 1
                    }
                }),
                None,
                Some(("generate-report.md".to_string(), generic_report)),
                &snapshot,
                Some(&upstream_evidence),
            );

        assert!(accepted_output.is_some());
        assert!(rejected.is_none());
        assert!(artifact_validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .is_none_or(|items| !items
                .iter()
                .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));
        assert_eq!(
            artifact_validation
                .get("validation_basis")
                .and_then(|value| value.get("quality_mode"))
                .and_then(Value::as_str),
            Some("legacy")
        );
        assert_eq!(
            artifact_validation
                .get("validation_basis")
                .and_then(|value| value.get("requested_quality_mode"))
                .and_then(Value::as_str),
            Some("legacy")
        );
        assert_eq!(
            artifact_validation
                .get("validation_basis")
                .and_then(|value| value.get("legacy_quality_rollback_enabled"))
                .and_then(Value::as_bool),
            Some(true)
        );

        let _ = std::fs::remove_dir_all(&workspace_root);
    });
}

#[test]
fn report_markdown_rejects_generic_synthesis_without_evidence_anchors_when_upstream_is_rich() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-anchor-synthesis-block-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Create the final report".to_string(),
        depends_on: vec!["analyze_findings".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "analyze_findings".to_string(),
            alias: "analysis".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "generate-report.md"
            }
        })),
    };
    let mut session = Session::new(
        Some("anchor-block-report".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let generic_report = "# Strategic Summary\n\n## Executive Summary\nThis report synthesizes the available upstream evidence into a concise outlook.\n\n## Key Findings\n* Growth vectors were identified across the workflow.\n* Strategic positioning remains promising.\n\n## Critical Risks\n* Competitive pressure remains a factor.\n\n## Recommendations\n* Continue iterating on the workflow.\n\n## Evidence/Sources\n* Internal documentation and external research informed this summary.\n\n## Next Steps\n* Refine the messaging and validate the next cycle.\n"
        .to_string();
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "generate-report.md",
                "content": generic_report
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));
    let upstream_evidence = AutomationUpstreamEvidence {
        read_paths: vec![
            ".tandem/artifacts/collect-inputs.json".to_string(),
            ".tandem/artifacts/research-sources.json".to_string(),
            ".tandem/artifacts/analyze-findings.md".to_string(),
        ],
        discovered_relevant_paths: vec![
            ".tandem/artifacts/collect-inputs.json".to_string(),
            ".tandem/artifacts/research-sources.json".to_string(),
            ".tandem/artifacts/analyze-findings.md".to_string(),
        ],
        web_research_attempted: true,
        web_research_succeeded: true,
        citation_count: 3,
        citations: vec![
            "https://example.com/1".to_string(),
            "https://example.com/2".to_string(),
            "https://example.com/3".to_string(),
        ],
    };

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            None,
            "Completed the report.",
            &json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            None,
            Some(("generate-report.md".to_string(), generic_report)),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_some());
    assert_eq!(
        rejected.as_deref(),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn report_markdown_accepts_rich_html_synthesis_when_upstream_is_rich() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-html-synthesis-pass-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Create the final report".to_string(),
        depends_on: vec!["analyze_findings".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "analyze_findings".to_string(),
            alias: "analysis".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "generate-report.md"
            }
        })),
    };
    let mut session = Session::new(
        Some("html-report".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let html_report = r#"
<html>
  <body>
    <h1>Frumu AI Tandem: Strategic Summary</h1>
    <p>We synthesized the local Tandem docs and the external research into one report.</p>
    <h3>Core Value Proposition</h3>
    <p>Tandem is an engine-backed workflow system for local execution and agentic operations.</p>
    <ul>
      <li>Local workspace reads and patch-based code execution.</li>
      <li>Current web research for externally grounded synthesis.</li>
      <li>Explicit delivery gating for email and other side effects.</li>
    </ul>
    <h3>Strategic Outlook</h3>
    <p>The positioning emphasizes deterministic execution, provenance, and operator control.</p>
    <p>Sources reviewed: <a href=\".tandem/runs/run-123/artifacts/analyze-findings.md\">analysis</a> and <a href=\".tandem/runs/run-123/artifacts/research-sources.json\">research</a>.</p>
  </body>
</html>
"#
    .trim()
    .to_string();
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "generate-report.md",
                "content": html_report
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));
    let upstream_evidence = AutomationUpstreamEvidence {
        read_paths: vec![
            ".tandem/artifacts/collect-inputs.json".to_string(),
            ".tandem/artifacts/research-sources.json".to_string(),
            ".tandem/artifacts/analyze-findings.md".to_string(),
        ],
        discovered_relevant_paths: vec![
            ".tandem/artifacts/collect-inputs.json".to_string(),
            ".tandem/artifacts/research-sources.json".to_string(),
            ".tandem/artifacts/analyze-findings.md".to_string(),
        ],
        web_research_attempted: true,
        web_research_succeeded: true,
        citation_count: 3,
        citations: vec![
            "https://example.com/1".to_string(),
            "https://example.com/2".to_string(),
            "https://example.com/3".to_string(),
        ],
    };

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            None,
            "Completed the report.",
            &json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            None,
            Some(("generate-report.md".to_string(), html_report.clone())),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_some());
    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        None
    );
    assert!(!artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn report_markdown_rejects_generic_html_synthesis_without_evidence_anchors_when_upstream_is_rich() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-html-anchor-synthesis-block-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Create the final report".to_string(),
        depends_on: vec!["analyze_findings".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "analyze_findings".to_string(),
            alias: "analysis".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "generate-report.md"
            }
        })),
    };
    let mut session = Session::new(
        Some("html-anchor-block-report".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let generic_html_report = r#"
<html>
  <body>
    <h1>Investor Summary: Strategic Analysis Report</h1>
    <p>We synthesized refined market data and findings from our research cycles into key growth vectors and strategic positioning for the target project.</p>
    <h3>Key Findings</h3>
    <ul>
      <li>Market growth vectors are present.</li>
      <li>Strategic positioning is available.</li>
    </ul>
    <h3>Critical Risks &amp; Considerations</h3>
    <p>Competitive pressure and entry barriers remain relevant.</p>
    <p>Operational mitigation follows the updated strategy.</p>
  </body>
</html>
"#
    .trim()
    .to_string();
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "generate-report.md",
                "content": generic_html_report
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));
    let upstream_evidence = AutomationUpstreamEvidence {
        read_paths: vec![
            ".tandem/artifacts/collect-inputs.json".to_string(),
            ".tandem/artifacts/research-sources.json".to_string(),
            ".tandem/artifacts/analyze-findings.md".to_string(),
        ],
        discovered_relevant_paths: vec![
            ".tandem/artifacts/collect-inputs.json".to_string(),
            ".tandem/artifacts/research-sources.json".to_string(),
            ".tandem/artifacts/analyze-findings.md".to_string(),
        ],
        web_research_attempted: true,
        web_research_succeeded: true,
        citation_count: 3,
        citations: vec![
            "https://example.com/1".to_string(),
            "https://example.com/2".to_string(),
            "https://example.com/3".to_string(),
        ],
    };

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            None,
            "Completed the report.",
            &json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            None,
            Some(("generate-report.md".to_string(), generic_html_report)),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_some());
    assert_eq!(
        rejected.as_deref(),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn research_brief_full_pipeline_overrides_llm_blocked_to_needs_repair_without_source_coverage_flag()
{
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-research-full-pipeline-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let brief_text =
        "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n"
            .to_string();
    std::fs::write(workspace_root.join("marketing-brief.md"), &brief_text).expect("seed brief");
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-brief".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Write marketing brief".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true
            }
        })),
    };
    let mut session = Session::new(
        Some("research-full-pipeline".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "glob".to_string(),
                args: json!({"pattern":"docs/**/*.md"}),
                result: Some(json!({"output": ""})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"marketing-brief.md","content":brief_text}),
                result: Some(json!({"ok": true})),
                error: None,
            },
        ],
    ));
    let requested_tools = vec![
        "glob".to_string(),
        "read".to_string(),
        "websearch".to_string(),
        "write".to_string(),
    ];
    let session_text =
        "The brief is blocked.\n\n{\"status\":\"blocked\",\"reason\":\"tools unavailable\"}";
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    let (accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        session_text,
        &tool_telemetry,
        None,
        Some(("marketing-brief.md".to_string(), brief_text.clone())),
        &std::collections::BTreeSet::new(),
    );
    assert_eq!(
        rejected.as_deref(),
        Some("research completed without concrete file reads or required source coverage")
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("research completed without concrete file reads or required source coverage")
    );

    let output: Value = wrap_automation_node_output(
        &node,
        &session,
        &requested_tools,
        "sess-research-full-pipeline",
        Some("run-research-full-pipeline"),
        session_text,
        accepted_output,
        Some(artifact_validation),
    );

    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("needs_repair")
    );
    assert_eq!(
        output.get("blocked_reason").and_then(Value::as_str),
        Some("research completed without concrete file reads or required source coverage")
    );
    assert!(!automation_output_is_blocked(&output));
    assert!(automation_output_needs_repair(&output));
    assert!(!automation_output_repair_exhausted(&output));

    let _ = std::fs::remove_dir_all(workspace_root);
}
