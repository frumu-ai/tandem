// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use crate::app::state::automation::receipts::{
    append_automation_attempt_receipt, automation_attempt_receipt_path_for_state_dir,
    AutomationAttemptReceiptDraft, AutomationAttemptReceiptRecord,
};
use crate::capability_resolver;

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
async fn automation_v2_definitions_are_persisted_as_per_workflow_shards() {
    let root = std::env::temp_dir().join(format!(
        "tandem-automation-v2-shards-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("state root");
    let active_path = root.join("automations_v2.json");
    let mut state = test_state_with_path(root.join("shared.json"));
    state.automations_v2_path = active_path.clone();
    state.automation_governance_path = root.join("automation_governance.json");

    let automation = AutomationSpecBuilder::new("automation-v2-sharded-test")
        .name("Large workflow")
        .metadata(json!({
            "planner_trace": "x".repeat(16_384),
            "approved_plan": {
                "notes": "persisted as a shard instead of growing the hot aggregate file"
            }
        }))
        .build();
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("persist automation");

    assert!(
        !active_path.exists(),
        "legacy aggregate file should not be rewritten after shard persistence"
    );
    let shard_path = root
        .join("automations-v2")
        .join("automation-v2-sharded-test.json");
    assert!(shard_path.exists(), "automation shard should exist");
    assert!(
        root.join("automations-v2").join("index.json").exists(),
        "definition index should exist"
    );

    let mut reloaded = test_state_with_path(root.join("shared-reloaded.json"));
    reloaded.automations_v2_path = active_path.clone();
    reloaded.automation_governance_path = root.join("automation_governance-reloaded.json");
    reloaded
        .load_automations_v2()
        .await
        .expect("load sharded automations");
    let loaded = reloaded
        .get_automation_v2("automation-v2-sharded-test")
        .await
        .expect("loaded automation");
    assert_eq!(loaded.name, automation.name);
    assert_eq!(loaded.metadata, automation.metadata);
}

#[tokio::test]
async fn automation_v2_legacy_aggregate_is_migrated_to_shards() {
    let root = std::env::temp_dir().join(format!(
        "tandem-automation-v2-legacy-migrate-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("state root");
    let active_path = root.join("automations_v2.json");
    let automation = AutomationSpecBuilder::new("automation-v2-legacy-test")
        .metadata(json!({ "legacy": true }))
        .build();
    let aggregate =
        std::collections::HashMap::from([(automation.automation_id.clone(), automation.clone())]);
    std::fs::write(
        &active_path,
        serde_json::to_string_pretty(&aggregate).expect("serialize aggregate"),
    )
    .expect("write legacy aggregate");

    let mut state = test_state_with_path(root.join("shared.json"));
    state.automations_v2_path = active_path.clone();
    state.automation_governance_path = root.join("automation_governance.json");
    state
        .load_automations_v2()
        .await
        .expect("migrate legacy aggregate");

    assert!(
        !active_path.exists(),
        "legacy aggregate should be archived out of the hot path"
    );
    assert!(
        root.join("automations_v2.legacy-aggregate.json").exists(),
        "legacy aggregate backup should be preserved"
    );
    assert!(
        root.join("automations-v2")
            .join("automation-v2-legacy-test.json")
            .exists(),
        "migrated automation shard should exist"
    );
    assert!(state
        .get_automation_v2("automation-v2-legacy-test")
        .await
        .is_some());
}

#[tokio::test]
async fn automation_v2_run_store_legacy_fixture_loads_and_rewrites_versioned_envelope() {
    let root = std::env::temp_dir().join(format!(
        "tandem-automation-v2-run-schema-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("state root");
    let active_path = root.join("automation_v2_runs.json");
    std::fs::write(
        &active_path,
        include_str!("../fixtures/automation_v2_runs_v0_map.json"),
    )
    .expect("legacy runs fixture");

    let mut state = test_state_with_path(root.join("shared.json"));
    state.automation_v2_runs_path = active_path.clone();
    state
        .load_automation_v2_runs()
        .await
        .expect("load legacy run fixture");

    let runs = state.automation_v2_runs.read().await;
    let run = runs
        .get("run-fixture-awaiting")
        .expect("awaiting approval run");
    assert_eq!(run.status, AutomationRunStatus::AwaitingApproval);
    assert_eq!(
        run.checkpoint
            .awaiting_gate
            .as_ref()
            .map(|gate| gate.node_id.as_str()),
        Some("approval_gate")
    );
    assert!(run
        .checkpoint
        .pending_nodes
        .iter()
        .any(|node_id| node_id == "send_email"));
    drop(runs);

    let raw = std::fs::read_to_string(&active_path).expect("versioned run store");
    let persisted: Value = serde_json::from_str(&raw).expect("versioned runs json");
    assert_eq!(
        persisted.get("schema_version").and_then(Value::as_u64),
        Some(AUTOMATION_V2_RUNS_SCHEMA_VERSION as u64)
    );
    assert!(persisted
        .get("runs")
        .and_then(|runs| runs.get("run-fixture-awaiting"))
        .is_some());
}

#[test]
fn automation_v2_run_store_current_fixture_loads_without_upgrade() {
    let (runs, upgraded) = parse_automation_v2_runs_file(include_str!(
        "../fixtures/automation_v2_runs_v1_envelope.json"
    ))
    .expect("versioned run fixture");
    assert!(!upgraded);
    assert!(runs.contains_key("run-fixture-versioned"));
}

#[tokio::test]
async fn automation_v2_run_store_rejects_future_schema_without_overwrite() {
    let root = std::env::temp_dir().join(format!(
        "tandem-automation-v2-run-schema-future-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("state root");
    let active_path = root.join("automation_v2_runs.json");
    let future = r#"{"schema_version":999,"runs":{}}"#;
    std::fs::write(&active_path, future).expect("future runs");

    let mut state = test_state_with_path(root.join("shared.json"));
    state.automation_v2_runs_path = active_path.clone();
    let error = state
        .load_automation_v2_runs()
        .await
        .expect_err("future run store should fail to load");
    assert!(
        error.to_string().contains("newer than supported"),
        "unexpected error: {error}"
    );
    assert_eq!(
        std::fs::read_to_string(active_path).expect("read future runs"),
        future
    );
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
    assert_eq!(appended.version, 2); // schema version bumped for hash-chain support
    assert_eq!(appended.run_id, run_id);
    assert_eq!(appended.node_id, node_id);
    assert_eq!(appended.attempt, 2);
    assert_eq!(appended.session_id, "sess-2");
    assert_eq!(appended.seq, 8);
    assert_eq!(appended.event_type, "completed");
    assert_eq!(appended.payload, serde_json::json!({"ok": true}));
    // Hash-chain: first v2 record after a v1 seed has no prev_hash (unhashed
    // v1 records cannot be chained from), and always has a non-empty record_hash.
    assert!(appended.prev_hash.is_none());
    assert!(!appended.record_hash.is_empty());

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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        wait: None,
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        wait: None,
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        wait: None,
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        wait: None,
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
                required_tool_calls: Vec::new(),
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        wait: None,
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
fn runnable_filter_serializes_nodes_targeting_same_mcp_tool() {
    let reddit_a = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "search_agents_keep".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Use mcp.reddit_gmail.reddit_search_across_subreddits to search agents keep."
            .to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: None,
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        wait: None,
        metadata: None,
    };
    let reddit_b = AutomationFlowNode {
        node_id: "search_agent_failing".to_string(),
        objective: "Use mcp.reddit_gmail.reddit_search_across_subreddits to search agent failing."
            .to_string(),
        ..reddit_a.clone()
    };
    let notion = AutomationFlowNode {
        node_id: "resolve_notion_targets".to_string(),
        objective: "Use mcp.notion.notion_search to resolve the target page.".to_string(),
        ..reddit_a.clone()
    };

    let filtered = automation_filter_runnable_by_write_scope_conflicts(
        vec![reddit_a.clone(), reddit_b, notion.clone()],
        3,
    );

    let ids = filtered
        .iter()
        .map(|node| node.node_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["search_agents_keep", "resolve_notion_targets"]);
}

#[test]
fn mcp_server_matching_handles_natural_hyphenated_server_mentions() {
    assert!(automation_text_mentions_mcp_server(
        "Use the connected Reddit MCP to gather community signals.",
        "reddit-gmail"
    ));
    assert!(automation_text_mentions_mcp_server(
        "Use the connected Tandem MCP docs as reference.",
        "tandem-mcp"
    ));
    assert!(!automation_text_mentions_mcp_server(
        "Use the connected Reddit MCP to gather community signals.",
        "notion"
    ));
}

#[test]
fn connector_backed_reddit_objective_requests_reddit_mcp_wildcard() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "gather_reddit_signals".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Use the connected Reddit MCP to search across relevant subreddits.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: None,
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        wait: None,
        metadata: None,
    };

    let requested = automation_requested_server_scoped_mcp_tools(
        &node,
        &[
            "notion".to_string(),
            "reddit-gmail".to_string(),
            "tandem-mcp".to_string(),
        ],
    );

    assert!(requested.contains(&"mcp.reddit_gmail.*".to_string()));
    assert!(!requested.contains(&"mcp.notion.*".to_string()));
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
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
    assert_eq!(requirements.repair_budget, Some(5));
    assert_eq!(
        requirements.repair_exhaustion_behavior,
        Some(tandem_types::PrewriteRepairExhaustionBehavior::FailClosed)
    );
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
                required_tool_calls: Vec::new(),
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        wait: None,
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
    assert_eq!(requirements.repair_budget, Some(5));
    assert_eq!(
        requirements.repair_exhaustion_behavior,
        Some(tandem_types::PrewriteRepairExhaustionBehavior::FailClosed)
    );
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
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
    assert_eq!(requirements.repair_budget, Some(5));
    assert_eq!(
        requirements.repair_exhaustion_behavior,
        Some(tandem_types::PrewriteRepairExhaustionBehavior::FailClosed)
    );
}

#[test]
fn legacy_quality_mode_keeps_waive_and_write_repair_behavior() {
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
        metadata: Some(json!({
            "quality_mode": "legacy",
            "builder": {
                "output_path": "notes.md",
                "required_tools": ["read"]
            }
        })),
    };

    let requirements =
        automation_node_prewrite_requirements(&node, &["read".to_string(), "write".to_string()])
            .expect("prewrite requirements");

    assert_eq!(requirements.repair_budget, Some(5));
    assert_eq!(
        requirements.repair_exhaustion_behavior,
        Some(tandem_types::PrewriteRepairExhaustionBehavior::WaiveAndWrite)
    );
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
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
            profile: None,
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
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
            allowed_connections: Vec::new(),
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
            profile: None,
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
                required_tool_calls: Vec::new(),
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        wait: None,
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
            allowed_connections: Vec::new(),
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
            profile: None,
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
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
            allowed_connections: Vec::new(),
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
            profile: None,
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
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
            allowed_connections: Vec::new(),
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

#[tokio::test]
async fn create_automation_v2_run_defaults_effective_profile_to_guided() {
    use crate::automation_v2::execution_profile::ExecutionProfile;
    let root = std::env::temp_dir().join(format!(
        "tandem-execution-profile-default-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("state root");
    let mut state = test_state_with_path(root.join("shared.json"));
    state.automations_v2_path = root.join("automations_v2.json");
    state.automation_v2_runs_path = root.join("automation_v2_runs.json");
    state.automation_governance_path = root.join("automation_governance.json");
    let automation = AutomationSpecBuilder::new("automation-profile-default")
        .name("Profile default")
        .build();
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("persist automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    assert_eq!(run.effective_execution_profile, ExecutionProfile::Guided);
    assert!(run.requested_execution_profile.is_none());
}

#[tokio::test]
async fn create_automation_v2_run_with_profile_uses_run_override() {
    use crate::automation_v2::execution_profile::ExecutionProfile;
    let root = std::env::temp_dir().join(format!(
        "tandem-execution-profile-override-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("state root");
    let mut state = test_state_with_path(root.join("shared.json"));
    state.automations_v2_path = root.join("automations_v2.json");
    state.automation_v2_runs_path = root.join("automation_v2_runs.json");
    state.automation_governance_path = root.join("automation_governance.json");
    let automation = AutomationSpecBuilder::new("automation-profile-override")
        .name("Profile override")
        .execution_profile(ExecutionProfile::Strict)
        .build();
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("persist automation");
    let run = state
        .create_automation_v2_run_with_profile(&automation, "manual", Some(ExecutionProfile::Yolo))
        .await
        .expect("create run with profile");
    assert_eq!(run.effective_execution_profile, ExecutionProfile::Yolo);
    assert_eq!(
        run.requested_execution_profile,
        Some(ExecutionProfile::Yolo)
    );
}

#[tokio::test]
async fn create_automation_v2_run_inherits_workflow_profile_when_no_override() {
    use crate::automation_v2::execution_profile::ExecutionProfile;
    let root = std::env::temp_dir().join(format!(
        "tandem-execution-profile-inherit-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("state root");
    let mut state = test_state_with_path(root.join("shared.json"));
    state.automations_v2_path = root.join("automations_v2.json");
    state.automation_v2_runs_path = root.join("automation_v2_runs.json");
    state.automation_governance_path = root.join("automation_governance.json");
    let automation = AutomationSpecBuilder::new("automation-profile-inherit")
        .name("Profile inherit")
        .execution_profile(ExecutionProfile::Guided)
        .build();
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("persist automation");
    let run = state
        .create_automation_v2_run_with_profile(&automation, "manual", None)
        .await
        .expect("create run");
    // Resolver precedence: requested override (None) -> spec.execution.profile (Guided).
    assert_eq!(run.effective_execution_profile, ExecutionProfile::Guided);
    assert!(run.requested_execution_profile.is_none());
}

#[tokio::test]
async fn create_automation_v2_run_with_profile_stamps_snapshot_with_effective() {
    use crate::automation_v2::execution_profile::ExecutionProfile;
    let root = std::env::temp_dir().join(format!(
        "tandem-execution-profile-snapshot-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("state root");
    let mut state = test_state_with_path(root.join("shared.json"));
    state.automations_v2_path = root.join("automations_v2.json");
    state.automation_v2_runs_path = root.join("automation_v2_runs.json");
    state.automation_governance_path = root.join("automation_governance.json");
    let automation = AutomationSpecBuilder::new("automation-profile-snapshot")
        .name("Profile snapshot")
        .execution_profile(ExecutionProfile::Strict)
        .build();
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("persist automation");
    // Run-level override of Lenient (`yolo` wire value) on a Strict-saved automation. The snapshot
    // stored on the run must carry profile=Yolo so downstream validation
    // logic that reads automation.execution.profile (e.g. the repair-budget
    // multiplier) honors the override.
    let run = state
        .create_automation_v2_run_with_profile(&automation, "manual", Some(ExecutionProfile::Yolo))
        .await
        .expect("create run");
    let snapshot = run
        .automation_snapshot
        .as_ref()
        .expect("snapshot set on run");
    assert_eq!(
        snapshot.execution.profile,
        Some(ExecutionProfile::Yolo),
        "snapshot must carry the run-level effective profile"
    );
    // Original spec object is not mutated.
    assert_eq!(automation.execution.profile, Some(ExecutionProfile::Strict));
}

#[tokio::test]
async fn create_automation_v2_dry_run_with_profile_persists_override() {
    use crate::automation_v2::execution_profile::ExecutionProfile;
    let root = std::env::temp_dir().join(format!(
        "tandem-execution-profile-dry-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&root).expect("state root");
    let mut state = test_state_with_path(root.join("shared.json"));
    state.automations_v2_path = root.join("automations_v2.json");
    state.automation_v2_runs_path = root.join("automation_v2_runs.json");
    state.automation_governance_path = root.join("automation_governance.json");
    let automation = AutomationSpecBuilder::new("automation-profile-dry")
        .name("Profile dry")
        .build();
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("persist automation");
    let run = state
        .create_automation_v2_dry_run_with_profile(
            &automation,
            "manual",
            Some(ExecutionProfile::Guided),
        )
        .await
        .expect("create dry run");
    assert_eq!(run.effective_execution_profile, ExecutionProfile::Guided);
    assert_eq!(
        run.requested_execution_profile,
        Some(ExecutionProfile::Guided)
    );
    assert_eq!(run.status, crate::AutomationRunStatus::Completed);
}
