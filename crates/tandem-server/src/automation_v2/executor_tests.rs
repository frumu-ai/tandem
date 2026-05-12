use super::*;

fn test_automation() -> crate::automation_v2::types::AutomationV2Spec {
    crate::automation_v2::types::AutomationV2Spec {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        automation_id: "automation-test".to_string(),
        name: "test".to_string(),
        description: None,
        status: crate::automation_v2::types::AutomationV2Status::Active,
        schedule: crate::automation_v2::types::AutomationV2Schedule {
            schedule_type: crate::automation_v2::types::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::Skip,
        },
        agents: Vec::new(),
        flow: crate::automation_v2::types::AutomationFlowSpec {
            nodes: vec![crate::automation_v2::types::AutomationFlowNode {
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                node_id: "research-brief".to_string(),
                agent_id: "research".to_string(),
                objective: "Research".to_string(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: None,
                retry_policy: None,
                timeout_ms: None,
                max_tool_calls: None,
                stage_kind: None,
                gate: None,
                metadata: None,
            }],
        },
        execution: crate::automation_v2::types::AutomationExecutionPolicy {
            profile: None,
            max_parallel_agents: None,
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "tests".to_string(),
        workspace_root: None,
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

fn test_run_with_output(output: Value) -> crate::automation_v2::types::AutomationV2RunRecord {
    crate::automation_v2::types::AutomationV2RunRecord {
        run_id: "run-test".to_string(),
        automation_id: "automation-test".to_string(),
        tenant_context: tandem_types::TenantContext::local_implicit(),
        trigger_type: "manual".to_string(),
        status: crate::automation_v2::types::AutomationRunStatus::Running,
        created_at_ms: 0,
        updated_at_ms: 0,
        started_at_ms: Some(0),
        finished_at_ms: None,
        active_session_ids: Vec::new(),
        latest_session_id: None,
        active_instance_ids: Vec::new(),
        checkpoint: crate::automation_v2::types::AutomationRunCheckpoint {
            completed_nodes: Vec::new(),
            pending_nodes: Vec::new(),
            node_outputs: std::collections::HashMap::from([("research-brief".to_string(), output)]),
            node_attempts: std::collections::HashMap::new(),
            node_attempt_verdicts: std::collections::HashMap::new(),
            blocked_nodes: Vec::new(),
            awaiting_gate: None,
            gate_history: Vec::new(),
            lifecycle_history: Vec::new(),
            last_failure: None,
        },
        runtime_context: None,
        automation_snapshot: None,
        pause_reason: None,
        resume_reason: None,
        detail: None,
        stop_kind: None,
        stop_reason: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
        scheduler: None,
        trigger_reason: None,
        consumed_handoff_id: None,
        learning_summary: None,
        effective_execution_profile:
            crate::automation_v2::execution_profile::ExecutionProfile::Strict,
        requested_execution_profile: None,
    }
}

fn test_node(
    node_id: &str,
    depends_on: Vec<&str>,
) -> crate::automation_v2::types::AutomationFlowNode {
    crate::automation_v2::types::AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
        agent_id: format!("agent-{node_id}"),
        objective: node_id.to_string(),
        depends_on: depends_on.into_iter().map(str::to_string).collect(),
        input_refs: Vec::new(),
        output_contract: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: None,
    }
}

#[test]
fn approval_rejection_rollback_prefers_derived_review_dependency() {
    let mut automation = test_automation();
    automation.flow.nodes = vec![
        test_node("gather_reddit_signals", vec![]),
        test_node("gather_tandem_reference", vec![]),
        test_node("gather_web_sources", vec![]),
        test_node("inspect_notion_row", vec![]),
        test_node(
            "synthesize_report",
            vec![
                "gather_reddit_signals",
                "gather_tandem_reference",
                "gather_web_sources",
                "inspect_notion_row",
            ],
        ),
        test_node(
            "validate_report",
            vec![
                "synthesize_report",
                "gather_reddit_signals",
                "gather_tandem_reference",
                "gather_web_sources",
                "inspect_notion_row",
            ],
        ),
        test_node(
            "update_notion_row",
            vec!["validate_report", "synthesize_report"],
        ),
        test_node("verify_notion_update", vec!["update_notion_row"]),
    ];
    let mut run = test_run_with_output(json!({"status": "blocked", "approved": false}));
    run.checkpoint.completed_nodes = vec![
        "gather_reddit_signals".to_string(),
        "gather_tandem_reference".to_string(),
        "gather_web_sources".to_string(),
        "inspect_notion_row".to_string(),
        "synthesize_report".to_string(),
    ];

    let roots = approval_rejection_rollback_roots(&automation, "validate_report", &run.checkpoint);

    assert_eq!(roots, vec!["synthesize_report".to_string()]);
    let reset_roots = roots.into_iter().collect::<std::collections::HashSet<_>>();
    let mut nodes_to_reset =
        crate::app::state::collect_automation_descendants(&automation, &reset_roots)
            .into_iter()
            .collect::<Vec<_>>();
    nodes_to_reset.sort();
    assert_eq!(
        nodes_to_reset,
        vec![
            "synthesize_report".to_string(),
            "update_notion_row".to_string(),
            "validate_report".to_string(),
            "verify_notion_update".to_string(),
        ]
    );
}

#[test]
fn approval_rejection_rollback_stops_when_derived_dependency_is_exhausted() {
    let mut automation = test_automation();
    automation.flow.nodes = vec![
        test_node("gather_reddit_signals", vec![]),
        test_node("gather_tandem_reference", vec![]),
        test_node("gather_web_sources", vec![]),
        test_node("inspect_notion_row", vec![]),
        test_node(
            "synthesize_report",
            vec![
                "gather_reddit_signals",
                "gather_tandem_reference",
                "gather_web_sources",
                "inspect_notion_row",
            ],
        ),
        test_node(
            "validate_report",
            vec![
                "synthesize_report",
                "gather_reddit_signals",
                "gather_tandem_reference",
                "gather_web_sources",
                "inspect_notion_row",
            ],
        ),
    ];
    let mut run = test_run_with_output(json!({"status": "blocked", "approved": false}));
    run.checkpoint.completed_nodes = vec![
        "gather_reddit_signals".to_string(),
        "gather_tandem_reference".to_string(),
        "gather_web_sources".to_string(),
        "inspect_notion_row".to_string(),
        "synthesize_report".to_string(),
    ];
    run.checkpoint
        .node_attempts
        .insert("synthesize_report".to_string(), 3);
    run.checkpoint
        .node_attempts
        .insert("gather_reddit_signals".to_string(), 1);
    run.checkpoint
        .node_attempts
        .insert("gather_tandem_reference".to_string(), 1);
    run.checkpoint
        .node_attempts
        .insert("gather_web_sources".to_string(), 1);
    run.checkpoint
        .node_attempts
        .insert("inspect_notion_row".to_string(), 1);

    let roots = approval_rejection_rollback_roots(&automation, "validate_report", &run.checkpoint);

    assert!(
        roots.is_empty(),
        "review rollback must not replay raw source nodes when the derived synthesis is exhausted"
    );
}

#[test]
fn yolo_review_relaxation_turns_rejection_into_advisory_completion() {
    let mut output = json!({
        "status": "blocked",
        "approved": false,
        "blocked_reason": "claims are not supported cleanly"
    });

    let relaxed = relax_yolo_review_output(
        &mut output,
        crate::automation_v2::execution_profile::ExecutionProfile::Yolo,
        Some(crate::AutomationOutputValidatorKind::ReviewDecision),
    );

    assert!(relaxed);
    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(output.get("approved").and_then(Value::as_bool), Some(true));
    assert_eq!(
        output.get("original_approved").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        output
            .pointer("/artifact_validation/effective_outcome")
            .and_then(Value::as_str),
        Some("experimental")
    );
    assert_eq!(
        output
            .pointer("/artifact_validation/warning_count")
            .and_then(Value::as_u64),
        Some(1)
    );
}

#[test]
fn yolo_review_relaxation_does_not_affect_strict_review_rejections() {
    let mut output = json!({
        "status": "blocked",
        "approved": false,
        "blocked_reason": "claims are not supported cleanly"
    });

    let relaxed = relax_yolo_review_output(
        &mut output,
        crate::automation_v2::execution_profile::ExecutionProfile::Strict,
        Some(crate::AutomationOutputValidatorKind::ReviewDecision),
    );

    assert!(!relaxed);
    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(output.get("approved").and_then(Value::as_bool), Some(false));
}

#[test]
fn yolo_relaxes_tool_resolution_failure_into_experimental_completion() {
    let node = &test_automation().flow.nodes[0];
    let mut output = build_node_execution_error_output_with_category(
        node,
        "required automation capabilities were not offered after MCP/tool sync: web_research",
        false,
        "tool_resolution_failed",
    );

    let relaxed = relax_yolo_non_safety_blocker_output(
        &mut output,
        crate::automation_v2::execution_profile::ExecutionProfile::Yolo,
    );

    assert!(relaxed);
    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        output
            .pointer("/artifact_validation/effective_outcome")
            .and_then(Value::as_str),
        Some("experimental")
    );
    assert_eq!(
        output
            .pointer("/artifact_validation/relaxed_validator_classes/0/class")
            .and_then(Value::as_str),
        Some("validator_kind_specific_soft_check")
    );
    assert!(output.get("content").is_some());
}

#[test]
fn yolo_does_not_relax_missing_required_source_reads() {
    let mut output = json!({
        "status": "blocked",
        "blocked_reason": "research completed without reading the exact required source files",
        "failure_kind": "artifact_rejected",
        "artifact_validation": {
            "blocking_classification": "artifact_contract_unmet",
            "unmet_requirements": ["required_source_paths_not_read"],
            "validation_basis": {
                "missing_required_source_read_paths": ["RESUME.md"]
            }
        }
    });

    let relaxed = relax_yolo_non_safety_blocker_output(
        &mut output,
        crate::automation_v2::execution_profile::ExecutionProfile::Yolo,
    );

    assert!(!relaxed);
    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        output
            .pointer("/artifact_validation/unmet_requirements/0")
            .and_then(Value::as_str),
        Some("required_source_paths_not_read")
    );
}

#[test]
fn yolo_does_not_relax_safety_blockers() {
    let mut output = json!({
        "status": "blocked",
        "blocker_category": "tenant_policy_denied",
        "blocked_reason": "tenant policy denied this tool"
    });

    let relaxed = relax_yolo_non_safety_blocker_output(
        &mut output,
        crate::automation_v2::execution_profile::ExecutionProfile::Yolo,
    );

    assert!(!relaxed);
    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("blocked")
    );
}

#[test]
fn promote_materialized_output_completes_missing_output_repairs() {
    let node = crate::automation_v2::types::AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-brief".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Research".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(crate::automation_v2::types::AutomationFlowOutputContract {
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
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/runs/run-test/artifacts/research-brief.json"
            }
        })),
    };
    let mut output = json!({
        "status": "needs_repair",
        "blocked_reason": "required output `.tandem/runs/run-test/artifacts/research-brief.json` was not created in the current attempt",
        "failure_kind": "artifact_rejected",
        "validator_summary": {
            "outcome": "needs_repair",
            "reason": "required output `.tandem/runs/run-test/artifacts/research-brief.json` was not created in the current attempt",
            "unmet_requirements": ["current_attempt_output_missing"]
        },
        "artifact_validation": {
            "rejected_artifact_reason": "required output `.tandem/runs/run-test/artifacts/research-brief.json` was not created in the current attempt",
            "unmet_requirements": ["current_attempt_output_missing"],
            "validation_basis": {
                "current_attempt_output_materialized": false,
                "verified_output_materialized": false
            }
        },
        "attempt_evidence": {
            "artifact": {
                "status": "missing",
                "path": ".tandem/runs/run-test/artifacts/research-brief.json"
            }
        }
    });

    promote_materialized_output(
        &mut output,
        &node,
        ".tandem/runs/run-test/artifacts/research-brief.json",
        "{\"status\":\"completed\"}",
        None,
    );

    assert_eq!(node_output_status(&output), "completed");
    assert_eq!(
        output
            .pointer("/artifact_validation/accepted_candidate_source")
            .and_then(Value::as_str),
        Some("verified_output")
    );
    assert_eq!(
        output
            .pointer("/artifact_validation/validation_basis/current_attempt_output_materialized")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        output
            .pointer("/attempt_evidence/artifact/status")
            .and_then(Value::as_str),
        Some("written")
    );
}

#[test]
fn promote_materialized_output_marks_session_salvage_recovery_source() {
    let node = &test_automation().flow.nodes[0];
    let mut output = json!({
        "status": "completed",
        "artifact_validation": {
            "validation_basis": {}
        },
        "attempt_evidence": {
            "artifact": {
                "status": "missing"
            }
        }
    });

    promote_materialized_output(
        &mut output,
        node,
        ".tandem/runs/run-test/artifacts/research-brief.json",
        "{\"status\":\"completed\"}",
        Some("session_text_salvage"),
    );

    assert_eq!(
        output
            .pointer("/artifact_validation/artifact_recovered_from_session")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        output
            .pointer("/attempt_evidence/artifact/recovery_source")
            .and_then(Value::as_str),
        Some("session_text_salvage")
    );
    assert_eq!(
        output
            .pointer("/artifact_validation/accepted_candidate_source")
            .and_then(Value::as_str),
        Some("session_write_recovery")
    );
}

#[test]
fn derive_terminal_run_state_marks_blocked_outputs_as_blocked() {
    let automation = test_automation();
    let run = test_run_with_output(json!({
        "status": "blocked",
        "failure_kind": "research_citations_missing",
    }));
    assert_eq!(
        derive_terminal_run_state(&automation, &run, false),
        DerivedTerminalRunState::Blocked {
            blocked_nodes: vec!["research-brief".to_string()],
            detail: "automation run blocked by upstream node outcome".to_string(),
        }
    );
}

#[test]
fn derive_terminal_run_state_marks_verify_failed_outputs_as_failed() {
    let automation = test_automation();
    let run = test_run_with_output(json!({
        "status": "verify_failed",
        "failure_kind": "verification_failed",
    }));
    assert_eq!(
        derive_terminal_run_state(&automation, &run, false),
        DerivedTerminalRunState::Failed {
            failed_nodes: vec!["research-brief".to_string()],
            blocked_nodes: Vec::new(),
            detail: "automation run failed from node outcomes: research-brief".to_string(),
        }
    );
}

#[test]
fn derive_terminal_run_state_does_not_block_repairable_research_outputs() {
    let automation = test_automation();
    let run = test_run_with_output(json!({
        "status": "needs_repair",
        "failure_kind": "research_missing_reads",
        "artifact_validation": {
            "repair_exhausted": false
        }
    }));
    assert_eq!(
        derive_terminal_run_state(&automation, &run, false),
        DerivedTerminalRunState::Completed
    );
}

#[test]
fn derive_terminal_run_state_fails_pending_repairable_nodes_at_attempt_cap() {
    let automation = test_automation();
    let mut run = test_run_with_output(json!({
        "status": "needs_repair",
        "artifact_validation": {
            "repair_exhausted": false
        }
    }));
    run.checkpoint.pending_nodes = vec!["research-brief".to_string()];
    run.checkpoint
        .node_attempts
        .insert("research-brief".to_string(), 3);

    assert_eq!(
        derive_terminal_run_state(&automation, &run, false),
        DerivedTerminalRunState::Failed {
            failed_nodes: vec!["research-brief".to_string()],
            blocked_nodes: Vec::new(),
            detail: "automation run failed from node outcomes: research-brief".to_string(),
        }
    );
}

#[test]
fn derive_terminal_run_state_fails_pending_nodes_that_exhausted_attempts() {
    let automation = test_automation();
    let mut run = test_run_with_output(json!({
        "status": "completed",
    }));
    run.checkpoint.node_outputs.clear();
    run.checkpoint.pending_nodes = vec!["research-brief".to_string()];
    run.checkpoint
        .node_attempts
        .insert("research-brief".to_string(), 3);

    assert_eq!(
        derive_terminal_run_state(&automation, &run, true),
        DerivedTerminalRunState::Failed {
            failed_nodes: vec!["research-brief".to_string()],
            blocked_nodes: Vec::new(),
            detail: "automation run failed from node outcomes: research-brief".to_string(),
        }
    );
}

#[test]
fn repairable_workspace_file_failure_requeues_even_when_run_artifact_passed() {
    let mut run = test_run_with_output(json!({
        "status": "needs_repair",
        "failure_kind": "artifact_rejected",
        "validator_summary": {
            "outcome": "passed",
            "unmet_requirements": []
        },
        "artifact_validation": {
            "validation_outcome": "needs_repair",
            "unmet_requirements": ["required_workspace_files_missing"],
            "required_next_tool_actions": ["Write `tandem-review.md` before updating the run artifact."],
            "repair_exhausted": false
        }
    }));
    run.checkpoint.pending_nodes.clear();
    assert!(crate::app::state::automation_node_has_passing_artifact(
        "research-brief",
        &run.checkpoint
    ));

    reconcile_pending_nodes_after_node_output(
        &mut run.checkpoint,
        "research-brief",
        true,
        false,
        &std::collections::HashSet::new(),
    );

    assert_eq!(
        run.checkpoint.pending_nodes,
        vec!["research-brief".to_string()]
    );
}

#[test]
fn terminal_workspace_file_repair_failure_does_not_requeue() {
    let mut run = test_run_with_output(json!({
        "status": "needs_repair",
        "validator_summary": {
            "outcome": "passed",
            "unmet_requirements": []
        },
        "artifact_validation": {
            "unmet_requirements": ["required_workspace_files_missing"],
            "repair_exhausted": true
        }
    }));
    run.checkpoint.pending_nodes = vec!["research-brief".to_string()];

    reconcile_pending_nodes_after_node_output(
        &mut run.checkpoint,
        "research-brief",
        true,
        true,
        &std::collections::HashSet::new(),
    );

    assert!(run.checkpoint.pending_nodes.is_empty());
}

#[test]
fn needs_repair_output_with_passing_validator_is_not_settled_completed() {
    let run = test_run_with_output(json!({
        "status": "needs_repair",
        "blocked_reason": "Required workspace files were not written in the current attempt.",
        "validator_summary": {
            "outcome": "passed",
            "unmet_requirements": []
        },
        "artifact_validation": {
            "validation_outcome": "passed",
            "unmet_requirements": [],
            "repair_exhausted": false
        }
    }));

    assert!(crate::app::state::automation_node_has_passing_artifact(
        "research-brief",
        &run.checkpoint
    ));
    assert!(
        !run_node_is_settled_completed(&run, "research-brief"),
        "a stale needs_repair output must not suppress later repair attempts"
    );
}

#[test]
fn repair_expected_contract_uses_normalized_upstream_synthesis_enforcement() {
    let node = crate::automation_v2::types::AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "synthesize_report".to_string(),
        agent_id: "research_synthesizer".to_string(),
        objective: "Synthesize the Tandem MCP reference notes, Reddit MCP findings, and current web research into a final report body for the existing Notion row.".to_string(),
        depends_on: vec![
            "gather_tandem_reference".to_string(),
            "gather_reddit_signals".to_string(),
            "gather_web_sources".to_string(),
        ],
        input_refs: vec![
            crate::AutomationFlowInputRef {
                from_step_id: "gather_tandem_reference".to_string(),
                alias: "tandem_reference_notes".to_string(),
            },
            crate::AutomationFlowInputRef {
                from_step_id: "gather_web_sources".to_string(),
                alias: "web_source_notes".to_string(),
            },
        ],
        output_contract: Some(crate::automation_v2::types::AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("external_research".to_string()),
                required_tools: vec!["websearch".to_string()],
                required_tool_calls: Vec::new(),
                required_evidence: vec!["external_sources".to_string()],
                required_sections: vec!["citations".to_string()],
                prewrite_gates: vec!["successful_web_research".to_string()],
                retry_on_missing: vec![
                    "external_sources".to_string(),
                    "citations".to_string(),
                    "successful_web_research".to_string(),
                ],
                terminal_on: vec![
                    "tool_unavailable".to_string(),
                    "repair_budget_exhausted".to_string(),
                ],
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
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
                "output_path": ".tandem/artifacts/synthesize-report.json"
            }
        })),
    };

    let expected = normalized_output_contract_value(&node).expect("normalized contract");
    let enforcement = expected
        .get("enforcement")
        .expect("normalized enforcement")
        .clone();

    assert_eq!(enforcement["validation_profile"], "research_synthesis");
    assert!(!enforcement["required_tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .any(|value| value.as_str() == Some("websearch")));
    assert!(!enforcement["required_evidence"]
        .as_array()
        .expect("evidence array")
        .iter()
        .any(|value| value.as_str() == Some("external_sources")));
    assert!(!enforcement["prewrite_gates"]
        .as_array()
        .expect("gates array")
        .iter()
        .any(|value| value.as_str() == Some("successful_web_research")));
    assert_eq!(enforcement["session_text_recovery"], "allow");
}

#[test]
fn workflow_failure_evidence_extracts_missing_workspace_files_and_actions() {
    let output = json!({
        "artifact_validation": {
            "must_write_file_statuses": [{
                "path": "tandem-review.md",
                "materialized_by_current_attempt": false,
                "touched_by_current_attempt": false
            }],
            "required_next_tool_actions": [
                "Write `tandem-review.md` before updating the run artifact."
            ]
        }
    });

    assert_eq!(
        output_missing_workspace_paths(Some(&output)),
        vec!["tandem-review.md".to_string()]
    );
    assert_eq!(
        output_required_next_tool_actions(Some(&output)),
        vec!["Write `tandem-review.md` before updating the run artifact.".to_string()]
    );
}

#[test]
fn transient_execution_error_output_requests_retry_without_handoff_requirements() {
    let node = &test_automation().flow.nodes[0];
    let output = build_node_execution_error_output(
        node,
        "provider stream connect timeout after 90000 ms",
        false,
    );
    assert_eq!(node_output_status(&output), "needs_repair");
    assert_eq!(node_output_failure_kind(&output), "execution_failed");
    assert_eq!(
        output.get("blocker_category").and_then(Value::as_str),
        Some("provider_connect_timeout")
    );
    assert_eq!(
        output.get("blocked_reason").and_then(Value::as_str),
        Some("provider stream connect timeout after 90000 ms")
    );
    assert!(output
        .pointer("/validator_summary/unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items.is_empty()));
    assert!(output
        .pointer("/artifact_validation/required_next_tool_actions")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|value| value.as_str().is_some_and(
            |text| text.contains("Do not classify this attempt as a missing handoff")
        ))));
}

#[test]
fn generic_provider_error_is_classified_and_normalized() {
    let node = &test_automation().flow.nodes[0];
    let output = build_node_execution_error_output(node, "Provider returned error", false);
    assert_eq!(node_output_status(&output), "needs_repair");
    assert_eq!(
        output.get("blocker_category").and_then(Value::as_str),
        Some("provider_server_error")
    );
    assert_eq!(
        output.get("blocked_reason").and_then(Value::as_str),
        Some("provider returned error before any node response was recorded")
    );
}

#[test]
fn transient_provider_retry_backoff_escalates_between_attempts() {
    assert_eq!(
        transient_provider_retry_backoff_ms("Provider returned error", 1),
        Some(2_000)
    );
    assert_eq!(
        transient_provider_retry_backoff_ms("Provider returned error", 2),
        Some(5_000)
    );
    assert_eq!(
        transient_provider_retry_backoff_ms("provider stream connect timeout after 90000 ms", 3),
        Some(8_000)
    );
    assert_eq!(
        transient_provider_retry_backoff_ms("authentication failed", 1),
        None
    );
}

#[test]
fn tool_resolution_execution_error_output_uses_dedicated_blocker_category() {
    let node = &test_automation().flow.nodes[0];
    let output = build_node_execution_error_output_with_category(
        node,
        "required automation capabilities were not offered after MCP/tool sync: email_delivery",
        false,
        "tool_resolution_failed",
    );
    assert_eq!(node_output_status(&output), "needs_repair");
    assert_eq!(
        output.get("blocker_category").and_then(Value::as_str),
        Some("tool_resolution_failed")
    );
    assert!(output
        .pointer("/artifact_validation/required_next_tool_actions")
        .and_then(Value::as_array)
        .is_some_and(|items| items.iter().any(|value| value
            .as_str()
            .is_some_and(|text| text.contains("collapsed tool set")))));
}

#[test]
fn terminal_execution_error_output_marks_node_failed() {
    let node = &test_automation().flow.nodes[0];
    let output = build_node_execution_error_output(
        node,
        "provider stream connect timeout after 90000 ms",
        true,
    );
    assert_eq!(node_output_status(&output), "failed");
    assert_eq!(node_output_failure_kind(&output), "run_failed");
    assert!(output
        .pointer("/artifact_validation/required_next_tool_actions")
        .and_then(Value::as_array)
        .is_some_and(|items| items.is_empty()));
}
