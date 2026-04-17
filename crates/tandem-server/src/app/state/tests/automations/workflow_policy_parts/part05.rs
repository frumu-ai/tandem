#[test]
fn missing_required_output_requests_repair_before_attempt_budget_is_exhausted() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Generate the final report".to_string(),
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
                "output_path": "outputs/generate-report.md"
            }
        })),
    };
    let artifact_validation = json!({
        "rejected_artifact_reason": "required output `outputs/generate-report.md` was not created in the current attempt",
        "semantic_block_reason": "required output was not created in the current attempt",
        "unmet_requirements": ["current_attempt_output_missing"],
        "repair_exhausted": false,
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "TOOL_MODE_REQUIRED_NOT_SATISFIED: WRITE_REQUIRED_NOT_SATISFIED: tool_mode=required but the model ended without executing a productive tool call.",
            None,
            &json!({
                "requested_tools": ["write"],
                "executed_tools": ["glob"]
            }),
            Some(&artifact_validation),
        );

    assert_eq!(status, "needs_repair");
    assert_eq!(
        reason.as_deref(),
        Some("required output `outputs/generate-report.md` was not created in the current attempt")
    );
    assert_eq!(approved, None);
}

#[test]
fn materialized_current_attempt_output_does_not_report_missing_output_requirement() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-current-attempt-output-materialized-{}",
        now_ms()
    ));
    std::fs::create_dir_all(workspace_root.join("outputs")).expect("create workspace");

    let node = AutomationNodeBuilder::new("generate_report")
        .agent_id("writer")
        .objective("Generate the final report")
        .output_contract(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        })
        .metadata(json!({
            "builder": {
                "output_path": "outputs/generate-report.md"
            }
        }))
        .build();
    let artifact_text = "# Final Report\n\nCurrent-attempt artifact created successfully.\n";
    let mut session = Session::new(
        Some("generate report".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "outputs/generate-report.md",
                "content": artifact_text,
            }),
            result: Some(json!({"output":"written"})),
            error: None,
        }],
    ));

    let (accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "Done\n\n{\"status\":\"completed\"}",
        &json!({
            "requested_tools": ["write"],
            "executed_tools": ["write"],
            "tool_call_counts": {"write": 1}
        }),
        None,
        Some((
            "outputs/generate-report.md".to_string(),
            artifact_text.to_string(),
        )),
        &std::collections::BTreeSet::new(),
    );

    assert!(rejected.is_none(), "{artifact_validation:?}");
    assert_eq!(
        accepted_output.as_ref().map(|(path, _)| path.as_str()),
        Some("outputs/generate-report.md")
    );
    assert_eq!(
        artifact_validation
            .get("validation_basis")
            .and_then(Value::as_object)
            .and_then(|value| value.get("current_attempt_output_materialized"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(!artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("current_attempt_output_missing"))));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn required_tool_mode_write_unsatisfied_requests_repair_without_artifact_validation() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Generate the final report".to_string(),
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
                "output_path": "outputs/generate-report.md"
            }
        })),
    };

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "TOOL_MODE_REQUIRED_NOT_SATISFIED: WRITE_REQUIRED_NOT_SATISFIED: tool_mode=required but the model ended without executing a productive tool call.",
            None,
            &json!({
                "requested_tools": ["glob", "write"],
                "executed_tools": ["glob"]
            }),
            None,
        );

    assert_eq!(status, "needs_repair");
    assert_eq!(
        reason.as_deref(),
        Some("required output `outputs/generate-report.md` was not created in the current attempt")
    );
    assert_eq!(approved, None);
}

#[test]
fn generic_artifact_semantic_block_requests_repair_before_attempt_budget_is_exhausted() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "generate_report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Generate the final report".to_string(),
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
                "output_path": "report.md"
            }
        })),
    };
    let artifact_validation = json!({
        "semantic_block_reason": "editorial artifact is missing expected markdown structure",
        "unmet_requirements": ["markdown_structure_missing", "editorial_substance_missing"],
        "repair_exhausted": false,
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Done\n\n{\"status\":\"completed\"}",
            Some(&("report.md".to_string(), "# Draft\n\nTODO\n".to_string())),
            &json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"]
            }),
            Some(&artifact_validation),
        );

    assert_eq!(status, "needs_repair");
    assert_eq!(
        reason.as_deref(),
        Some("editorial artifact is missing expected markdown structure")
    );
    assert_eq!(approved, None);
}

#[test]
fn code_workflow_missing_verification_requests_repair() {
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
                "verification_command": "cargo test",
                "output_path": "handoff.md"
            }
        })),
    };
    let tool_telemetry = json!({
        "requested_tools": ["read", "apply_patch", "write", "bash"],
        "executed_tools": ["apply_patch", "write"],
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
fn code_workflow_without_structural_completion_signal_requests_repair() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "job-scout".to_string(),
        objective: "Operate the hourly job scout workflow".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "code_patch".to_string(),
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
                "task_kind": "code_change"
            }
        })),
    };
    let tool_telemetry = json!({
        "requested_tools": ["glob", "read", "websearch", "webfetch", "write", "bash"],
        "executed_tools": ["glob", "read", "websearch", "webfetch", "bash"],
        "verification_expected": false,
        "verification_ran": false,
        "verification_failed": false
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "I see the results from your previous tool calls:\n\nWhat would you like me to do next? For example:\n- Fetch more content from a specific URL\n- Search for Rust developer jobs\n- Explore the current workspace for related code/projects\n- Something else?",
            None,
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "needs_repair");
    assert_eq!(
        reason.as_deref(),
        Some("node did not return a final workflow result with an explicit status or validated output")
    );
    assert_eq!(approved, None);
}

#[test]
fn artifact_workflow_with_materialized_output_completes_without_explicit_status_json() {
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
                "output_path": "outputs/generate-report.md"
            }
        })),
    };
    let verified_output = (
        "outputs/generate-report.md".to_string(),
        "# Final Report\n\nCompleted synthesis.".to_string(),
    );

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Completed the requested tool actions and wrote the final report artifact.",
            Some(&verified_output),
            &json!({
                "requested_tools": ["read", "write"],
                "executed_tools": ["read", "write"]
            }),
            None,
        );

    assert_eq!(status, "completed");
    assert_eq!(reason, None);
    assert_eq!(approved, None);
}

#[test]
fn standup_synthesis_accepts_inline_completed_status_without_verified_output() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "standup_synthesis".to_string(),
        agent_id: "coordinator".to_string(),
        objective: "Write the daily standup report".to_string(),
        depends_on: vec!["standup_participants".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "standup_participants".to_string(),
            alias: "participants".to_string(),
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
                "workflow_template": "agent_standup",
                "output_path": "docs/standups/2026-04-14.md",
                "report_path_template": "docs/standups/{{date}}.md"
            }
        })),
    };
    let tool_telemetry = json!({
        "requested_tools": ["read", "write", "memory_search", "memory_store"],
        "executed_tools": ["write", "memory_store", "read", "memory_search"]
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Standup report written to `docs/standups/2026-04-14.md` for 3 participants.\n\n{\"status\":\"completed\",\"approved\":true,\"report_path\":\"docs/standups/2026-04-14.md\",\"participant_count\":3}",
            None,
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "completed");
    assert_eq!(reason, None);
    assert_eq!(approved, Some(true));
}

#[test]
fn code_workflow_accepts_status_json_when_it_appears_at_end_of_long_response() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "job-scout".to_string(),
        objective: "Operate the hourly job scout workflow".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "code_patch".to_string(),
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
                "task_kind": "code_change"
            }
        })),
    };
    let session_text = format!(
        "{}\n\n{{\"status\":\"completed\"}}",
        "Completed workspace analysis and validation steps. ".repeat(80)
    );

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            &session_text,
            None,
            &json!({
                "requested_tools": ["glob", "read", "bash"],
                "executed_tools": ["glob", "read", "bash"],
                "verification_expected": false,
                "verification_ran": false,
                "verification_failed": false
            }),
            None,
        );

    assert_eq!(status, "done");
    assert_eq!(reason, None);
    assert_eq!(approved, None);
}

#[test]
fn code_workflow_accepts_fenced_status_json_after_markdown_summary() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "job-scout".to_string(),
        objective: "Operate the hourly job scout workflow".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "code_patch".to_string(),
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
                "task_kind": "code_change"
            }
        })),
    };
    let tool_telemetry = json!({
        "requested_tools": ["read", "bash", "glob", "websearch", "write"],
        "executed_tools": ["read", "bash", "glob", "websearch", "write"],
        "verification_expected": false,
        "verification_ran": false,
        "verification_failed": false
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "## Summary\n\nExecution complete.\n\n```json\n{\"status\":\"completed\",\"summary\":\"all files updated\"}\n```",
            None,
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "done");
    assert_eq!(reason, None);
    assert_eq!(approved, None);
}

#[test]
fn report_markdown_validation_accepts_updated_verified_output_without_session_write_telemetry() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-updated-without-session-write-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let stale_preexisting = "# Strategic Summary\n\nOld report content.\n".to_string();
    let updated_report = r#"
<html>
  <body>
    <h1>Frumu AI Tandem: Strategic Summary</h1>
    <p>We synthesized the upstream research into one report.</p>
    <h3>Core Value Proposition</h3>
    <p>Tandem is an engine-backed workflow system for local execution and agentic operations.</p>
    <ul>
      <li>Local workspace reads and patch-based code execution.</li>
      <li>Current web research for externally grounded synthesis.</li>
      <li>Explicit delivery gating for email and other side effects.</li>
    </ul>
    <h3>Strategic Outlook</h3>
    <p>The positioning emphasizes deterministic execution, provenance, and operator control.</p>
    <p>Sources reviewed: <a href=".tandem/runs/run-456/artifacts/analyze-findings.md">analysis</a> and <a href=".tandem/runs/run-456/artifacts/research-sources.json">research</a>.</p>
  </body>
</html>
"#
    .trim()
    .to_string();
    std::fs::write(
        workspace_root.join("generate-report.md"),
        &stale_preexisting,
    )
    .expect("seed stale report");
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
        Some("generate-report-updated".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "read".to_string(),
            args: json!({"path":"analyze_findings.md"}),
            result: Some(json!({"ok": true})),
            error: None,
        }],
    ));
    std::fs::write(workspace_root.join("generate-report.md"), &updated_report)
        .expect("write updated report");
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
            &json!({}),
            Some(&stale_preexisting),
            Some(("generate-report.md".to_string(), updated_report.clone())),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_some(), "{artifact_validation:?}");
    assert!(rejected.is_none(), "{artifact_validation:?}");
    assert_eq!(
        artifact_validation
            .get("accepted_candidate_source")
            .and_then(Value::as_str),
        Some("verified_output")
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        None
    );
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        artifact_validation
            .get("validation_basis")
            .and_then(Value::as_object)
            .and_then(|value| value.get("authority"))
            .and_then(Value::as_str),
        Some("filesystem_and_receipts")
    );
    assert_eq!(
        artifact_validation
            .get("validation_basis")
            .and_then(Value::as_object)
            .and_then(|value| value.get("verified_output_materialized"))
            .and_then(Value::as_bool),
        Some(true)
    );

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn report_markdown_validation_rejects_bare_relative_artifact_hrefs() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-bare-href-block-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let report = r#"
<html>
  <body>
    <h1>Frumu AI Tandem: Strategic Summary</h1>
    <p>We synthesized the upstream research into one report.</p>
    <h3>Core Value Proposition</h3>
    <p>Tandem is an engine-backed workflow system for local execution and agentic operations.</p>
    <ul>
      <li>Local workspace reads and patch-based code execution.</li>
      <li>Current web research for externally grounded synthesis.</li>
      <li>Explicit delivery gating for email and other side effects.</li>
    </ul>
    <h3>Strategic Outlook</h3>
    <p>The positioning emphasizes deterministic execution, provenance, and operator control.</p>
    <p>Sources reviewed: <a href=".tandem/artifacts/analyze-findings.md">analysis</a> and <a href=".tandem/artifacts/research-sources.json">research</a>.</p>
  </body>
</html>
"#
    .trim()
    .to_string();
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
        Some("bare-href-block".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "generate-report.md",
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

    let (_accepted_output, artifact_validation, _rejected) =
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
            Some(("generate-report.md".to_string(), report.clone())),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some(
            "final artifact contains a bare relative artifact href; use a canonical run-scoped link or plain text instead"
        )
    );
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("bare_relative_artifact_href"))));
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("blocked")
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn research_validation_removes_blocked_handoff_artifact_without_preexisting_output() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-automation-blocked-handoff-remove-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let blocked_text = "# Marketing Brief\n\nStatus: blocked pending required source reads and web research in this run.\n\nThis file cannot be finalized from the current toolset available in this session because the required discovery and external research tools referenced by the task (`read`, `glob`, `websearch`) are not available to me here.\n".to_string();
    std::fs::write(workspace_root.join("marketing-brief.md"), &blocked_text)
        .expect("seed blocked handoff");
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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true
            }
        })),
    };
    let session = Session::new(
        Some("blocked handoff".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );

    let (accepted_output, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        &blocked_text,
        &json!({
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": ["glob", "websearch", "write"],
            "workspace_inspection_used": true,
            "web_research_used": true,
            "web_research_succeeded": false,
            "latest_web_research_failure": "web research authorization required"
        }),
        None,
        Some(("marketing-brief.md".to_string(), blocked_text.clone())),
        &snapshot,
    );

    assert!(accepted_output.is_none());
    assert_eq!(
        metadata
            .get("blocked_handoff_cleanup_action")
            .and_then(Value::as_str),
        Some("removed_blocked_output")
    );
    assert_eq!(
        rejected.as_deref(),
        Some("research completed without concrete file reads or required source coverage")
    );
    assert!(!workspace_root.join("marketing-brief.md").exists());

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn research_validation_restores_preexisting_output_without_accepting_blocked_handoff() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-automation-blocked-handoff-restore-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let previous = "# Marketing Brief\n\n## Workspace source audit\nPrepared from earlier sourced work.\n\n## Files reviewed\n- docs/source.md\n\n## Web sources reviewed\n- https://example.com\n".to_string();
    let blocked_text = "# Marketing Brief\n\nStatus: blocked pending required source reads and web research in this run.\n\nThis file cannot be finalized from the current toolset available in this session because the required discovery and external research tools referenced by the task (`read`, `glob`, `websearch`) are not available to me here.\n".to_string();
    std::fs::write(workspace_root.join("marketing-brief.md"), &blocked_text)
        .expect("seed blocked handoff");
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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true
            }
        })),
    };
    let session = Session::new(
        Some("blocked handoff restore".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );

    let (accepted_output, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        &blocked_text,
        &json!({
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": ["glob", "websearch", "write"],
            "workspace_inspection_used": true,
            "web_research_used": true,
            "web_research_succeeded": false,
            "latest_web_research_failure": "web research authorization required"
        }),
        Some(&previous),
        Some(("marketing-brief.md".to_string(), blocked_text.clone())),
        &snapshot,
    );

    assert!(accepted_output.is_none());
    assert_eq!(
        metadata
            .get("blocked_handoff_cleanup_action")
            .and_then(Value::as_str),
        Some("restored_preexisting_output")
    );
    assert_eq!(
        rejected.as_deref(),
        Some("research completed without concrete file reads or required source coverage")
    );
    let disk_text = std::fs::read_to_string(workspace_root.join("marketing-brief.md"))
        .expect("read restored artifact");
    assert_eq!(disk_text, previous);

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn artifact_validation_prefers_structurally_stronger_candidate_without_phrase_match() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-automation-stronger-candidate-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let substantive = format!(
        "# Marketing Brief\n\n## Workspace source audit\n{}\n\n## Files reviewed\n- docs/source.md\n\n## Files not reviewed\n- docs/extra.md (out of scope)\n",
        "Detailed sourced content. ".repeat(50)
    );
    let weak_final = "# Marketing Brief\n\nShort wrap-up.\n".to_string();
    std::fs::write(workspace_root.join("marketing-brief.md"), &weak_final)
        .expect("seed final weak artifact");
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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": false
            }
        })),
    };
    let mut session = Session::new(
        Some("stronger candidate".to_string()),
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
                tool: "read".to_string(),
                args: json!({"path":"docs/source.md"}),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": "marketing-brief.md",
                    "content": substantive
                }),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": "marketing-brief.md",
                    "content": weak_final
                }),
                result: Some(json!({"ok": true})),
                error: None,
            },
        ],
    ));

    let (accepted_output, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "Done",
        &json!({
            "requested_tools": ["glob", "read", "write"],
            "executed_tools": ["read", "write"]
        }),
        None,
        Some((
            "marketing-brief.md".to_string(),
            "# Marketing Brief\n\nShort wrap-up.\n".to_string(),
        )),
        &snapshot,
    );

    assert_eq!(
        rejected.as_deref(),
        Some("research completed without citation-backed claims")
    );
    assert_eq!(
        metadata
            .get("accepted_candidate_source")
            .and_then(Value::as_str),
        Some("session_write")
    );
    assert!(accepted_output
        .as_ref()
        .is_some_and(|(_, text)| text.contains("## Workspace source audit")));
    let disk_text = std::fs::read_to_string(workspace_root.join("marketing-brief.md"))
        .expect("read selected artifact");
    assert!(disk_text.contains("## Workspace source audit"));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn completed_brief_without_read_is_blocked_even_if_it_looks_confident() {
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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "marketing-brief.md",
                "web_research_expected": true
            }
        })),
    };
    let tool_telemetry = json!({
        "requested_tools": ["glob", "read", "websearch", "write"],
        "executed_tools": ["glob", "websearch", "write"],
        "workspace_inspection_used": true,
        "web_research_used": true
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Done — `marketing-brief.md` was written in the workspace.\n\n{\"status\":\"completed\",\"approved\":true}",
            Some(&(
                "marketing-brief.md".to_string(),
                "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n## Files reviewed\n- tandem-reference/readmes/repo-README.md\n- tandem-reference/readmes/engine-README.md\n".to_string(),
            )),
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "completed");
    assert_eq!(reason.as_deref(), None);
    assert_eq!(approved, Some(true));
}

