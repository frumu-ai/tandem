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
        max_tool_calls: None,
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
        max_tool_calls: None,
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
fn execution_policy_reports_workflow_class() {
    let research = AutomationFlowNode {
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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md"
            }
        })),
    };
    let code = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "code".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Code".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: None,
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
                "task_kind": "code_change",
                "output_path": "handoff.md"
            }
        })),
    };

    assert_eq!(
        automation_node_execution_policy(&research, ".")
            .get("workflow_class")
            .and_then(Value::as_str),
        Some("research")
    );
    assert_eq!(
        automation_node_execution_policy(&code, ".")
            .get("workflow_class")
            .and_then(Value::as_str),
        Some("code")
    );
}

#[test]
fn workflow_state_events_capture_typed_stability_transitions() {
    let mut run = AutomationV2RunRecord {
        run_id: "run-1".to_string(),
        automation_id: "automation-1".to_string(),
        tenant_context: tandem_types::TenantContext::local_implicit(),
        trigger_type: "manual".to_string(),
        status: AutomationRunStatus::Running,
        created_at_ms: 0,
        updated_at_ms: 0,
        started_at_ms: Some(0),
        finished_at_ms: None,
        latest_session_id: None,
        active_session_ids: Vec::new(),
        active_instance_ids: Vec::new(),
        checkpoint: AutomationRunCheckpoint {
            completed_nodes: Vec::new(),
            pending_nodes: Vec::new(),
            node_outputs: std::collections::HashMap::new(),
            node_attempts: std::collections::HashMap::new(),
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
    };
    let output = json!({
        "status": "blocked",
        "workflow_class": "research",
        "phase": "research_validation",
        "failure_kind": "research_missing_reads",
        "blocked_reason": "research completed without concrete file reads",
        "artifact_validation": {
            "accepted_candidate_source": "session_write_recovery",
            "artifact_candidates": [
                {
                    "source": "session_write",
                    "length": 1200,
                    "substantive": true,
                    "placeholder_like": false,
                    "accepted": false
                }
            ],
            "repair_attempted": true,
            "repair_succeeded": false,
            "unmet_requirements": ["no_concrete_reads"],
            "blocking_classification": "tool_available_but_not_used",
            "required_next_tool_actions": [
                "Use `read` on concrete workspace files before finalizing the brief."
            ],
            "verification": {
                "verification_expected": false,
                "verification_ran": false,
                "verification_failed": false
            }
        }
    });

    record_automation_workflow_state_events(
        &mut run,
        "research-brief",
        &output,
        2,
        Some("session-1"),
        "blocked brief",
        "brief",
    );

    let events = run
        .checkpoint
        .lifecycle_history
        .iter()
        .map(|event| event.event.as_str())
        .collect::<Vec<_>>();
    assert!(events.contains(&"workflow_state_changed"));
    assert!(events.contains(&"artifact_candidate_written"));
    assert!(events.contains(&"artifact_accepted"));
    assert!(events.contains(&"repair_started"));
    assert!(events.contains(&"repair_exhausted"));
    assert!(events.contains(&"research_coverage_failed"));

    let state_event = run
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|event| event.event == "workflow_state_changed")
        .expect("workflow state event");
    assert_eq!(
        state_event
            .metadata
            .as_ref()
            .and_then(|value| value.get("workflow_class"))
            .and_then(Value::as_str),
        Some("research")
    );
    assert_eq!(
        state_event
            .metadata
            .as_ref()
            .and_then(|value| value.get("failure_kind"))
            .and_then(Value::as_str),
        Some("research_missing_reads")
    );
    assert_eq!(
        state_event
            .metadata
            .as_ref()
            .and_then(|value| value.get("blocking_classification"))
            .and_then(Value::as_str),
        Some("tool_available_but_not_used")
    );
    assert_eq!(
        state_event
            .metadata
            .as_ref()
            .and_then(|value| value.get("required_next_tool_actions"))
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some("Use `read` on concrete workspace files before finalizing the brief.")
    );
}

#[test]
fn code_workflow_verification_failure_sets_verify_failed_status() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "implement".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Implement feature".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: None,
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
                "task_kind": "code_change",
                "verification_command": "cargo test"
            }
        })),
    };
    let tool_telemetry = json!({
        "requested_tools": ["glob", "read", "edit", "apply_patch", "write", "bash"],
        "executed_tools": ["read", "apply_patch", "bash"],
        "verification_expected": true,
        "verification_ran": true,
        "verification_failed": true,
        "latest_verification_failure": "verification command failed with exit code 101: cargo test"
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Done\n\n{\"status\":\"completed\"}",
            None,
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "verify_failed");
    assert_eq!(
        reason.as_deref(),
        Some("verification command failed with exit code 101: cargo test")
    );
    assert_eq!(approved, None);
}

#[test]
fn code_workflow_without_verification_run_is_blocked() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "implement".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Implement feature".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: None,
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
                "task_kind": "code_change",
                "verification_command": "cargo test"
            }
        })),
    };
    let tool_telemetry = json!({
        "requested_tools": ["glob", "read", "edit", "apply_patch", "write", "bash"],
        "executed_tools": ["read", "apply_patch"],
        "verification_expected": true,
        "verification_ran": false,
        "verification_failed": false
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Done\n\n{\"status\":\"completed\"}",
            None,
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "needs_repair");
    assert_eq!(
        reason.as_deref(),
        Some("coding task completed without running the declared verification command")
    );
    assert_eq!(approved, None);
}

#[test]
fn collect_automation_external_action_receipts_records_bound_publisher_tools() {
    let automation = AutomationV2Spec {
        automation_id: "auto-publish-test".to_string(),
        name: "Publish Test".to_string(),
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
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "publish".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Publish final update".to_string(),
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
                "role": "publisher"
            }
        })),
    };
    let mut session = Session::new(Some("publisher".to_string()), Some(".".to_string()));
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "workflow_test.slack".to_string(),
                args: json!({
                    "channel": "engineering",
                    "text": "Ship it"
                }),
                result: Some(json!({
                    "output": "posted",
                    "metadata": {
                        "channel": "engineering"
                    }
                })),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "workflow_test.internal".to_string(),
                args: json!({
                    "value": 1
                }),
                result: Some(json!({"output": "ignored"})),
                error: None,
            },
        ],
    ));
    let mut bindings = capability_resolver::CapabilityBindingsFile::default();
    bindings
        .bindings
        .push(capability_resolver::CapabilityBinding {
            capability_id: "slack.post_message".to_string(),
            provider: "custom".to_string(),
            tool_name: "workflow_test.slack".to_string(),
            tool_name_aliases: Vec::new(),
            request_transform: None,
            response_transform: None,
            metadata: json!({}),
        });

    let receipts = collect_automation_external_action_receipts(
        &bindings,
        "run-1",
        &automation,
        &node,
        1,
        "session-1",
        &session,
    );

    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].source_kind.as_deref(), Some("automation_v2"));
    assert_eq!(
        receipts[0].capability_id.as_deref(),
        Some("slack.post_message")
    );
    assert_eq!(
        receipts[0].context_run_id.as_deref(),
        Some("automation-v2-run-1")
    );
    assert_eq!(receipts[0].target.as_deref(), Some("engineering"));
}

#[test]
fn collect_automation_external_action_receipts_ignores_non_outbound_nodes() {
    let automation = AutomationV2Spec {
        automation_id: "auto-draft-test".to_string(),
        name: "Draft Test".to_string(),
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
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "draft".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Draft final update".to_string(),
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
                "role": "writer"
            }
        })),
    };
    let mut session = Session::new(Some("writer".to_string()), Some(".".to_string()));
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "workflow_test.slack".to_string(),
            args: json!({
                "channel": "engineering",
                "text": "Ship it"
            }),
            result: Some(json!({"output": "posted"})),
            error: None,
        }],
    ));
    let mut bindings = capability_resolver::CapabilityBindingsFile::default();
    bindings
        .bindings
        .push(capability_resolver::CapabilityBinding {
            capability_id: "slack.post_message".to_string(),
            provider: "custom".to_string(),
            tool_name: "workflow_test.slack".to_string(),
            tool_name_aliases: Vec::new(),
            request_transform: None,
            response_transform: None,
            metadata: json!({}),
        });

    let receipts = collect_automation_external_action_receipts(
        &bindings,
        "run-1",
        &automation,
        &node,
        1,
        "session-1",
        &session,
    );

    assert!(receipts.is_empty());
}

#[test]
fn collect_automation_external_action_receipts_stabilize_identity_across_retries() {
    let automation = AutomationV2Spec {
        automation_id: "auto-publish-attempt-test".to_string(),
        name: "Publish Attempt Test".to_string(),
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
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "publish".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Publish final update".to_string(),
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
                "role": "publisher"
            }
        })),
    };
    let mut session = Session::new(Some("publisher".to_string()), Some(".".to_string()));
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "workflow_test.slack".to_string(),
            args: json!({
                "channel": "engineering",
                "text": "Ship it"
            }),
            result: Some(json!({"output": "posted"})),
            error: None,
        }],
    ));
    let mut bindings = capability_resolver::CapabilityBindingsFile::default();
    bindings
        .bindings
        .push(capability_resolver::CapabilityBinding {
            capability_id: "slack.post_message".to_string(),
            provider: "custom".to_string(),
            tool_name: "workflow_test.slack".to_string(),
            tool_name_aliases: Vec::new(),
            request_transform: None,
            response_transform: None,
            metadata: json!({}),
        });

    let first_attempt = collect_automation_external_action_receipts(
        &bindings,
        "run-1",
        &automation,
        &node,
        1,
        "session-1",
        &session,
    );
    let second_attempt = collect_automation_external_action_receipts(
        &bindings,
        "run-1",
        &automation,
        &node,
        2,
        "session-1",
        &session,
    );

    assert_eq!(first_attempt.len(), 1);
    assert_eq!(second_attempt.len(), 1);
    assert_eq!(first_attempt[0].action_id, second_attempt[0].action_id);
    assert_eq!(
        first_attempt[0].idempotency_key,
        second_attempt[0].idempotency_key
    );
    assert_ne!(first_attempt[0].source_id, second_attempt[0].source_id);
}

#[test]
fn code_workflow_with_full_verification_plan_reports_done() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "implement".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Implement feature".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: None,
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
                "task_kind": "code_change",
                "verification_command": "cargo check\ncargo test\ncargo clippy --all-targets"
            }
        })),
    };
    let mut session = Session::new(Some("verification pass".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "bash".to_string(),
                args: json!({"command":"cargo check"}),
                result: Some(json!({"metadata":{"exit_code":0}})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "bash".to_string(),
                args: json!({"command":"cargo test"}),
                result: Some(json!({"metadata":{"exit_code":0}})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "bash".to_string(),
                args: json!({"command":"cargo clippy --all-targets"}),
                result: Some(json!({"metadata":{"exit_code":0}})),
                error: None,
            },
        ],
    ));

    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &[
            "glob".to_string(),
            "read".to_string(),
            "edit".to_string(),
            "apply_patch".to_string(),
            "write".to_string(),
            "bash".to_string(),
        ],
    );

    assert_eq!(
        tool_telemetry
            .get("verification_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        tool_telemetry
            .get("verification_total")
            .and_then(Value::as_u64),
        Some(3)
    );
    assert_eq!(
        tool_telemetry
            .get("verification_completed")
            .and_then(Value::as_u64),
        Some(3)
    );

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Done\n\n{\"status\":\"completed\"}",
            None,
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "done");
    assert_eq!(reason, None);
    assert_eq!(approved, None);
}

#[test]
fn code_workflow_with_partial_verification_is_blocked() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "implement".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Implement feature".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: None,
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
                "task_kind": "code_change",
                "verification_command": "cargo check\ncargo test\ncargo clippy --all-targets"
            }
        })),
    };
    let mut session = Session::new(Some("verification partial".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "bash".to_string(),
                args: json!({"command":"cargo check"}),
                result: Some(json!({"metadata":{"exit_code":0}})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "bash".to_string(),
                args: json!({"command":"cargo test"}),
                result: Some(json!({"metadata":{"exit_code":0}})),
                error: None,
            },
        ],
    ));

    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &[
            "glob".to_string(),
            "read".to_string(),
            "edit".to_string(),
            "apply_patch".to_string(),
            "write".to_string(),
            "bash".to_string(),
        ],
    );

    assert_eq!(
        tool_telemetry
            .get("verification_outcome")
            .and_then(Value::as_str),
        Some("partial")
    );

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Done\n\n{\"status\":\"completed\"}",
            None,
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "blocked");
    assert_eq!(
        reason.as_deref(),
        Some("coding task completed with only 2 of 3 declared verification commands run")
    );
    assert_eq!(approved, None);
}

#[test]
fn email_delivery_nodes_block_without_email_tool_execution() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "notify_user".to_string(),
        agent_id: "agent-committer".to_string(),
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

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "The report is ready.\n\n{\"status\":\"completed\",\"approved\":true}",
            None,
            &json!({
                "requested_tools": ["*"],
                "executed_tools": ["read"],
                "tool_call_counts": {"read": 1},
                "workspace_inspection_used": true,
                "email_delivery_attempted": false,
                "email_delivery_succeeded": false,
                "latest_email_delivery_failure": null
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
fn email_delivery_nodes_request_repair_when_email_tools_were_offered_but_unused() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "notify_user".to_string(),
        agent_id: "agent-committer".to_string(),
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

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "The report is ready.\n\n{\"status\":\"completed\",\"approved\":true}",
            None,
            &json!({
                "requested_tools": ["read", "email_draft"],
                "executed_tools": ["read"],
                "tool_call_counts": {"read": 1},
                "workspace_inspection_used": true,
                "email_delivery_attempted": false,
                "email_delivery_succeeded": false,
                "latest_email_delivery_failure": null,
                "capability_resolution": {
                    "email_tool_diagnostics": {
                        "available_tools": ["email_draft"],
                        "offered_tools": ["email_draft"],
                        "available_send_tools": [],
                        "offered_send_tools": [],
                        "available_draft_tools": ["email_draft"],
                        "offered_draft_tools": ["email_draft"]
                    }
                }
            }),
            None,
        );

    assert_eq!(status, "needs_repair");
    assert_eq!(
        reason.as_deref(),
        Some(
            "email delivery to `recipient@example.com` was requested but no email draft/send tool executed"
        )
    );
    assert_eq!(approved, Some(true));
}

