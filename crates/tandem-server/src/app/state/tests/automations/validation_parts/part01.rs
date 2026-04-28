fn bare_node() -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "n1".to_string(),
        agent_id: "a1".to_string(),
        objective: "do something".to_string(),
        depends_on: vec![],
        input_refs: vec![],
        output_contract: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: None,
    }
}

struct StructuredJsonWriteMatrixCase<'a> {
    name: &'a str,
    output_files: &'a [&'a str],
    writes: &'a [(&'a str, &'a str)],
    expected_validation_outcome: &'a str,
    expected_rejected: Option<&'a str>,
    expected_missing_workspace_files: &'a [&'a str],
}

struct ToolInvocationSpec {
    tool: &'static str,
    args: Value,
    result: Value,
}

struct ResearchEvidenceMatrixCase {
    name: &'static str,
    node: AutomationFlowNode,
    workspace_files: Vec<(&'static str, &'static str)>,
    tool_invocations: Vec<ToolInvocationSpec>,
    requested_tools: Vec<&'static str>,
    accepted_output_path: &'static str,
    accepted_output_content: &'static str,
    session_text: &'static str,
    expected_validation_outcome: &'static str,
    expected_external_research_mode: Option<&'static str>,
    absent_unmet: Vec<&'static str>,
    expected_read_paths: Vec<&'static str>,
}

struct RepairStateMatrixCase {
    name: &'static str,
    session_text: &'static str,
    repair_exhausted: bool,
    expected_status: &'static str,
    expected_reason: &'static str,
    expected_failure_kind: &'static str,
    expected_summary_outcome: &'static str,
}

struct UpstreamSynthesisMatrixCase {
    name: &'static str,
    node_id: &'static str,
    output_path: &'static str,
    artifact_text: &'static str,
    session_text: &'static str,
    write_path: &'static str,
    tool_telemetry: Value,
    upstream_evidence: AutomationUpstreamEvidence,
    expected_validation_outcome: &'static str,
    expected_rejected: Option<&'static str>,
    expect_upstream_unsynthesized: bool,
}

struct CodeVerificationMatrixCase {
    name: &'static str,
    verification_command: Option<&'static str>,
    session_text: &'static str,
    tool_telemetry: Value,
    expected_status: &'static str,
    expected_reason: Option<&'static str>,
    expected_failure_kind: Option<&'static str>,
}

struct DeliveryMatrixCase {
    name: &'static str,
    session_text: &'static str,
    tool_telemetry: Value,
    expected_status: &'static str,
    expected_reason: &'static str,
    expected_blocker_category: &'static str,
}

struct UpstreamShapeMatrixCase {
    name: &'static str,
    quality_mode: Option<&'static str>,
    legacy_rollback_enabled: Option<bool>,
    artifact_text: &'static str,
    upstream_evidence: Option<AutomationUpstreamEvidence>,
    expected_validation_outcome: &'static str,
    expected_rejected: Option<&'static str>,
    expected_warning_count: Option<usize>,
    expect_upstream_unsynthesized: bool,
}

fn structured_json_write_matrix_node(output_files: &[&str]) -> AutomationFlowNode {
    let mut builder = json!({
        "output_path": "extract.json"
    });
    if !output_files.is_empty() {
        builder["output_files"] = Value::Array(
            output_files
                .iter()
                .map(|path| json!(path))
                .collect::<Vec<_>>(),
        );
    }
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "extract_pain_points".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Write synthesis".to_string(),
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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": builder
        })),
    }
}

fn run_structured_json_write_matrix_case(case: StructuredJsonWriteMatrixCase<'_>) {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-structured-json-write-matrix-{}-{}",
        case.name,
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = structured_json_write_matrix_node(case.output_files);
    let artifact_text =
        "{\"status\":\"completed\",\"summary\":\"Synthesis artifact already written successfully.\"}"
            .to_string();
    std::fs::write(workspace_root.join("extract.json"), &artifact_text).expect("write artifact");

    let mut session = Session::new(
        Some(format!("structured-json-write-matrix-{}", case.name)),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let mut parts = Vec::new();
    parts.push(MessagePart::ToolInvocation {
        tool: "write".to_string(),
        args: json!({"path":"extract.json","content":artifact_text}),
        result: Some(json!({"ok": true})),
        error: None,
    });
    for (path, content) in case.writes {
        if let Some(parent) = std::path::Path::new(path)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(workspace_root.join(parent)).expect("create write parent");
        }
        std::fs::write(workspace_root.join(path), content).expect("write side file");
        parts.push(MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({"path":path,"content":content}),
            result: Some(json!({"ok": true})),
            error: None,
        });
    }
    session
        .messages
        .push(tandem_types::Message::new(MessageRole::Assistant, parts));

    let tool_telemetry =
        summarize_automation_tool_activity(&node, &session, &["write".to_string()]);
    let (_accepted_output, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        Some(("extract.json".to_string(), artifact_text)),
        &snapshot,
    );

    assert_eq!(
        metadata.get("validation_outcome").and_then(Value::as_str),
        Some(case.expected_validation_outcome),
        "case={}",
        case.name
    );
    assert_eq!(
        rejected.as_deref(),
        case.expected_rejected,
        "case={}",
        case.name
    );
    assert_eq!(
        metadata
            .get("validation_basis")
            .and_then(|value| value.get("must_write_files"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        case.output_files
            .iter()
            .map(|path| Value::String((*path).to_string()))
            .collect::<Vec<_>>(),
        "case={}",
        case.name
    );
    for expected_missing in case.expected_missing_workspace_files {
        assert!(
            metadata
                .get("validation_basis")
                .and_then(|value| value.get("must_write_file_statuses"))
                .and_then(Value::as_array)
                .is_some_and(|values| values.iter().any(|value| {
                    value.get("path").and_then(Value::as_str) == Some(*expected_missing)
                        && value
                            .get("materialized_by_current_attempt")
                            .and_then(Value::as_bool)
                            == Some(false)
                })),
            "case={}",
            case.name
        );
    }

    let _ = std::fs::remove_dir_all(workspace_root);
}

fn research_brief_matrix_node(
    output_path: &str,
    web_research_expected: bool,
) -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research_brief".to_string(),
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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": output_path,
                "web_research_expected": web_research_expected
            }
        })),
    }
}

fn research_citations_matrix_node(
    node_id: &str,
    output_path: &str,
    web_research_expected: bool,
    preferred_mcp_servers: &[&str],
) -> AutomationFlowNode {
    let mut builder = json!({
        "output_path": output_path,
        "web_research_expected": web_research_expected,
        "source_coverage_required": true
    });
    if !preferred_mcp_servers.is_empty() {
        builder["preferred_mcp_servers"] = Value::Array(
            preferred_mcp_servers
                .iter()
                .map(|server| json!(server))
                .collect::<Vec<_>>(),
        );
    }
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
        agent_id: "researcher".to_string(),
        objective: "Research sources for the current run".to_string(),
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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": builder
        })),
    }
}

fn run_research_evidence_matrix_case(case: ResearchEvidenceMatrixCase) {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-research-evidence-matrix-{}-{}",
        case.name,
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    for (path, content) in &case.workspace_files {
        if let Some(parent) = std::path::Path::new(path)
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(workspace_root.join(parent)).expect("create input parent");
        }
        std::fs::write(workspace_root.join(path), content).expect("write workspace file");
    }
    if let Some(parent) = std::path::Path::new(case.accepted_output_path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(workspace_root.join(parent)).expect("create output parent");
    }
    std::fs::write(
        workspace_root.join(case.accepted_output_path),
        case.accepted_output_content,
    )
    .expect("write accepted output");

    let mut session = Session::new(
        Some(format!("research-evidence-matrix-{}", case.name)),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let parts = case
        .tool_invocations
        .into_iter()
        .map(|spec| MessagePart::ToolInvocation {
            tool: spec.tool.to_string(),
            args: spec.args,
            result: Some(spec.result),
            error: None,
        })
        .collect::<Vec<_>>();
    session
        .messages
        .push(tandem_types::Message::new(MessageRole::Assistant, parts));

    let tool_telemetry = summarize_automation_tool_activity(
        &case.node,
        &session,
        &case
            .requested_tools
            .iter()
            .map(|tool| (*tool).to_string())
            .collect::<Vec<_>>(),
    );
    let (_accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &case.node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        case.session_text,
        &tool_telemetry,
        None,
        Some((
            case.accepted_output_path.to_string(),
            case.accepted_output_content.to_string(),
        )),
        &std::collections::BTreeSet::new(),
    );

    assert!(
        rejected.is_none(),
        "case={} rejected={rejected:?}",
        case.name
    );
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some(case.expected_validation_outcome),
        "case={}",
        case.name
    );
    if let Some(expected_mode) = case.expected_external_research_mode {
        assert_eq!(
            artifact_validation
                .get("external_research_mode")
                .and_then(Value::as_str),
            Some(expected_mode),
            "case={}",
            case.name
        );
    }
    for unmet in case.absent_unmet {
        assert!(
            !artifact_validation
                .get("unmet_requirements")
                .and_then(Value::as_array)
                .is_some_and(|values| values.iter().any(|value| value.as_str() == Some(unmet))),
            "case={}",
            case.name
        );
    }
    if !case.expected_read_paths.is_empty() {
        let expected = case
            .expected_read_paths
            .iter()
            .map(|path| json!(path))
            .collect::<Vec<_>>();
        assert_eq!(
            artifact_validation
                .get("read_paths")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default(),
            expected,
            "case={}",
            case.name
        );
    }

    let _ = std::fs::remove_dir_all(workspace_root);
}

fn research_retry_matrix_node() -> AutomationFlowNode {
    AutomationFlowNode {
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
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    }
}

fn run_repair_state_matrix_case(case: RepairStateMatrixCase) {
    let node = research_retry_matrix_node();
    let tool_telemetry = json!({
        "requested_tools": ["glob", "read", "websearch", "write"],
        "executed_tools": ["glob", "write"],
    });
    let artifact_validation = json!({
        "semantic_block_reason": "research completed without concrete file reads or required source coverage",
        "unmet_requirements": ["no_concrete_reads", "missing_successful_web_research"],
        "repair_exhausted": case.repair_exhausted,
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            case.session_text,
            Some(&(
                "marketing-brief.md".to_string(),
                "# Marketing Brief".to_string(),
            )),
            &tool_telemetry,
            Some(&artifact_validation),
        );

    assert_eq!(status, case.expected_status, "case={}", case.name);
    assert_eq!(
        reason.as_deref(),
        Some(case.expected_reason),
        "case={}",
        case.name
    );
    assert_eq!(approved, None, "case={}", case.name);
    assert_eq!(
        detect_automation_node_failure_kind(
            &node,
            &status,
            approved,
            reason.as_deref(),
            Some(&artifact_validation),
        )
        .as_deref(),
        Some(case.expected_failure_kind),
        "case={}",
        case.name
    );
    let summary = build_automation_validator_summary(
        crate::AutomationOutputValidatorKind::ResearchBrief,
        &status,
        reason.as_deref(),
        Some(&artifact_validation),
    );
    assert_eq!(
        summary.outcome, case.expected_summary_outcome,
        "case={}",
        case.name
    );
}

fn report_markdown_synthesis_matrix_node(node_id: &str, output_path: &str) -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
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
                "output_path": output_path
            }
        })),
    }
}

fn run_upstream_synthesis_matrix_case(case: UpstreamSynthesisMatrixCase) {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-upstream-synthesis-matrix-{}-{}",
        case.name,
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = report_markdown_synthesis_matrix_node(case.node_id, case.output_path);
    if let Some(parent) = std::path::Path::new(case.output_path)
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(workspace_root.join(parent)).expect("create output parent");
    }
    std::fs::write(workspace_root.join(case.output_path), case.artifact_text)
        .expect("write output");

    let mut session = Session::new(
        Some(format!("upstream-synthesis-matrix-{}", case.name)),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": case.write_path,
                "content": case.artifact_text
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            None,
            case.session_text,
            &case.tool_telemetry,
            None,
            Some((case.output_path.to_string(), case.artifact_text.to_string())),
            &snapshot,
            Some(&case.upstream_evidence),
        );

    assert!(accepted_output.is_some(), "case={}", case.name);
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some(case.expected_validation_outcome),
        "case={}",
        case.name
    );
    assert_eq!(
        rejected.as_deref(),
        case.expected_rejected,
        "case={}",
        case.name
    );
    assert_eq!(
        artifact_validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .is_some_and(|items| items
                .iter()
                .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))),
        case.expect_upstream_unsynthesized,
        "case={}",
        case.name
    );

    let _ = std::fs::remove_dir_all(workspace_root);
}

fn code_verification_matrix_node(verification_command: Option<&str>) -> AutomationFlowNode {
    let mut builder = json!({
        "task_kind": "code_change"
    });
    if let Some(command) = verification_command {
        builder["verification_command"] = json!(command);
    }
    AutomationFlowNode {
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
            "builder": builder
        })),
    }
}

fn run_code_verification_matrix_case(case: CodeVerificationMatrixCase) {
    let node = code_verification_matrix_node(case.verification_command);
    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(&node, case.session_text, None, &case.tool_telemetry, None);

    assert_eq!(status, case.expected_status, "case={}", case.name);
    assert_eq!(
        reason.as_deref(),
        case.expected_reason,
        "case={}",
        case.name
    );
    assert_eq!(approved, None, "case={}", case.name);
    assert_eq!(
        detect_automation_node_failure_kind(&node, &status, approved, reason.as_deref(), None)
            .as_deref(),
        case.expected_failure_kind,
        "case={}",
        case.name
    );
}

fn email_delivery_matrix_node() -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "operator".to_string(),
        objective:
            "Create a Gmail draft or send the final HTML summary email to test@example.com if mail tools are available."
                .to_string(),
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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "delivery": {
                "method": "email",
                "to": "test@example.com",
                "content_type": "text/html",
                "inline_body_only": true,
                "attachments": false
            }
        })),
    }
}

fn run_delivery_matrix_case(case: DeliveryMatrixCase) {
    let node = email_delivery_matrix_node();
    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(&node, case.session_text, None, &case.tool_telemetry, None);

    assert_eq!(status, case.expected_status, "case={}", case.name);
    assert_eq!(
        reason.as_deref(),
        Some(case.expected_reason),
        "case={}",
        case.name
    );
    assert_eq!(approved, Some(true), "case={}", case.name);
    assert_eq!(
        detect_automation_blocker_category(
            &node,
            &status,
            reason.as_deref(),
            &case.tool_telemetry,
            None,
        )
        .as_deref(),
        Some(case.expected_blocker_category),
        "case={}",
        case.name
    );
}

fn upstream_shape_matrix_node(quality_mode: Option<&str>) -> AutomationFlowNode {
    let mut metadata = json!({
        "builder": {
            "output_path": "generate-report.md"
        }
    });
    if let Some(mode) = quality_mode {
        metadata["quality_mode"] = json!(mode);
    }
    AutomationFlowNode {
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
        metadata: Some(metadata),
    }
}

fn run_upstream_shape_matrix_case(case: UpstreamShapeMatrixCase) {
    if let Some(enabled) = case.legacy_rollback_enabled {
        with_legacy_quality_rollback_enabled(enabled, || {
            run_upstream_shape_matrix_case_inner(&case);
        });
    } else {
        run_upstream_shape_matrix_case_inner(&case);
    }
}

fn run_upstream_shape_matrix_case_inner(case: &UpstreamShapeMatrixCase) {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-upstream-shape-matrix-{}-{}",
        case.name,
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = upstream_shape_matrix_node(case.quality_mode);
    std::fs::write(
        workspace_root.join("generate-report.md"),
        case.artifact_text,
    )
    .expect("write artifact");

    let mut session = Session::new(
        Some(format!("upstream-shape-matrix-{}", case.name)),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "generate-report.md",
                "content": case.artifact_text
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));

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
                    "read": 1,
                    "write": 1
                }
            }),
            None,
            Some((
                "generate-report.md".to_string(),
                case.artifact_text.to_string(),
            )),
            &snapshot,
            case.upstream_evidence.as_ref(),
        );

    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some(case.expected_validation_outcome),
        "case={}",
        case.name
    );
    assert_eq!(
        rejected.as_deref(),
        case.expected_rejected,
        "case={}",
        case.name
    );
    if let Some(expected_warning_count) = case.expected_warning_count {
        assert_eq!(
            artifact_validation
                .get("warning_count")
                .and_then(Value::as_u64)
                .unwrap_or_default() as usize,
            expected_warning_count,
            "case={}",
            case.name
        );
    }
    assert_eq!(
        artifact_validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .is_some_and(|items| items
                .iter()
                .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))),
        case.expect_upstream_unsynthesized,
        "case={}",
        case.name
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn output_validator_defaults_follow_existing_runtime_heuristics() {
    let code = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "code".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Implement fix".to_string(),
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
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "task_kind": "code_change",
                "output_path": "src/lib.rs"
            }
        })),
    };
    let brief = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "brief".to_string(),
        agent_id: "agent-b".to_string(),
        objective: "Draft research brief".to_string(),
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
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: None,
    };
    let review = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "review".to_string(),
        agent_id: "agent-c".to_string(),
        objective: "Approve draft".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "review".to_string(),
            validator: None,
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Review),
        gate: None,
        metadata: None,
    };

    assert_eq!(
        automation_output_validator_kind(&code),
        crate::AutomationOutputValidatorKind::CodePatch
    );
    assert_eq!(
        automation_output_validator_kind(&brief),
        crate::AutomationOutputValidatorKind::ResearchBrief
    );
    assert_eq!(
        automation_output_validator_kind(&review),
        crate::AutomationOutputValidatorKind::ReviewDecision
    );
}

#[test]
fn output_validator_explicit_override_wins() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "report".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Write report".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: None,
    };

    assert_eq!(
        automation_output_validator_kind(&node),
        crate::AutomationOutputValidatorKind::StructuredJson
    );
}

#[test]
fn enrich_automation_node_output_overwrites_stale_validator_metadata() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "brief".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Draft research brief".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
            enforcement: None,
            schema: None,
            summary_guidance: None,
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: None,
    };
    let output = json!({
        "node_id": "brief",
        "status": "blocked",
        "workflow_class": "artifact",
        "phase": "completed",
        "failure_kind": "verification_failed",
        "validator_kind": "generic_artifact",
        "validator_summary": {
            "kind": "generic_artifact",
            "outcome": "passed"
        },
        "artifact_validation": {
            "unmet_requirements": ["concrete_read_required"]
        }
    });

    let enriched = enrich_automation_node_output_for_contract(&node, &output);
    assert_eq!(
        enriched.get("validator_kind").and_then(Value::as_str),
        Some("research_brief")
    );
    assert_eq!(
        enriched.get("workflow_class").and_then(Value::as_str),
        Some("research")
    );
    assert_eq!(
        enriched.get("phase").and_then(Value::as_str),
        Some("research_validation")
    );
    assert_eq!(
        enriched.get("failure_kind").and_then(Value::as_str),
        Some("research_missing_reads")
    );
    assert_eq!(
        enriched
            .get("validator_summary")
            .and_then(|value| value.get("outcome"))
            .and_then(Value::as_str),
        Some("blocked")
    );
}

#[test]
fn placeholder_artifact_text_is_rejected() {
    assert!(placeholder_like_artifact_text(
        "Completed previously in this run; preserving file creation requirement."
    ));
    assert!(placeholder_like_artifact_text(
        "Created/updated to satisfy workflow artifact requirement. See existing workspace research already completed in this run."
    ));
    assert!(placeholder_like_artifact_text(
        "Marketing brief completed and written to marketing-brief.md."
    ));
    assert!(placeholder_like_artifact_text(
        "Marketing brief already written in prior step; no content change."
    ));
    assert!(placeholder_like_artifact_text(
        "# Status\n\nBlocked handoff"
    ));
    assert!(!placeholder_like_artifact_text(
        "# Marketing Brief\n\n## Audience\nReal sourced content with specific product details."
    ));
}

#[test]
fn artifact_validation_rejection_blocks_node_status() {
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
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let tool_telemetry = json!({
        "requested_tools": ["glob", "read", "write", "websearch"],
        "executed_tools": ["glob", "write"],
        "workspace_inspection_used": true,
        "web_research_used": false
    });
    let artifact_validation = json!({
        "accepted_artifact_path": Value::Null,
        "rejected_artifact_reason": "placeholder overwrite rejected",
        "undeclared_files_created": ["_automation_touch.txt"],
        "auto_cleaned": true,
        "execution_policy": {
            "mode": "filesystem_standard"
        }
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Done",
            None,
            &tool_telemetry,
            Some(&artifact_validation),
        );

    assert_eq!(status, "blocked");
    assert_eq!(reason.as_deref(), Some("placeholder overwrite rejected"));
    assert_eq!(approved, None);
}

#[test]
fn research_workflow_failure_kind_is_typed_from_unmet_requirements() {
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
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };
    let artifact_validation = json!({
        "semantic_block_reason": "research completed without concrete file reads or required source coverage",
        "unmet_requirements": ["no_concrete_reads", "files_reviewed_not_backed_by_read"],
        "verification": {
            "verification_failed": false
        }
    });

    assert_eq!(
        detect_automation_node_failure_kind(
            &node,
            "blocked",
            None,
            Some("research completed without concrete file reads or required source coverage"),
            Some(&artifact_validation),
        )
        .as_deref(),
        Some("research_missing_reads")
    );
    assert_eq!(
        detect_automation_node_phase(&node, "blocked", Some(&artifact_validation)),
        "research_validation"
    );
    let summary = build_automation_validator_summary(
        crate::AutomationOutputValidatorKind::ResearchBrief,
        "blocked",
        Some("research completed without concrete file reads or required source coverage"),
        Some(&artifact_validation),
    );
    assert_eq!(
        summary.kind,
        crate::AutomationOutputValidatorKind::ResearchBrief
    );
    assert_eq!(summary.outcome, "blocked");
    assert_eq!(
        summary.reason.as_deref(),
        Some("research completed without concrete file reads or required source coverage")
    );
    assert_eq!(
        summary.unmet_requirements,
        vec![
            "no_concrete_reads".to_string(),
            "files_reviewed_not_backed_by_read".to_string()
        ]
    );
}

#[test]
fn compare_results_retry_without_current_artifact_surfaces_write_and_synthesis_actions() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-compare-results-retry-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("content/blog-memory")).expect("create workspace");
    std::fs::write(
        workspace_root.join("content/blog-memory/01-facts.md"),
        "# Facts\n\nPersistent blog facts.\n",
    )
    .expect("write facts");
    std::fs::write(
        workspace_root.join("content/blog-memory/02-decisions.md"),
        "# Decisions\n\nPersistent blog decisions.\n",
    )
    .expect("write decisions");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "compare_results".to_string(),
        agent_id: "analyst".to_string(),
        objective: "Review existing persistent blog memory and recent Tandem blog history to produce recent-blog-review.md. First summarize themes, angle types, repeated phrasing, unexplored ideas, and stylistic notes from content/blog-memory/. Then call mcp_list for blog-mcp and use only discovered blog history inspection functions to inspect recent Tandem blog posts, identifying repeated themes, title/framing patterns, structures, openings, and what not to repeat. Record mcp_list usage, discovered functions, and exact functions used.".to_string(),
        depends_on: vec!["research_sources".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "research_sources".to_string(),
            alias: "blog_memory".to_string(),
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
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/compare-results.md"
            }
        })),
    };
    let mut session = Session::new(
        Some("compare-results-retry".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "mcp_list".to_string(),
                args: json!({}),
                result: Some(json!({
                    "servers": [
                        {
                            "name": "blog-mcp",
                            "tools": [
                                "create_blog_draft",
                                "get_blog_guidelines",
                                "list_blog_drafts",
                                "submit_blog_for_review",
                                "update_blog_draft"
                            ]
                        }
                    ]
                })),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "glob".to_string(),
                args: json!({"pattern":"content/blog-memory/*.md"}),
                result: Some(json!({
                    "output": [
                        workspace_root
                            .join("content/blog-memory/01-facts.md")
                            .display()
                            .to_string(),
                        workspace_root
                            .join("content/blog-memory/02-decisions.md")
                            .display()
                            .to_string()
                    ]
                })),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"path":"content/blog-memory/01-facts.md"}),
                result: Some(json!({"output":"Persistent blog facts."})),
                error: None,
            },
        ],
    ));

    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &[
            "mcp_list".to_string(),
            "glob".to_string(),
            "read".to_string(),
            "write".to_string(),
        ],
    );
    let upstream_evidence = AutomationUpstreamEvidence {
        read_paths: vec![
            "content/blog-memory/01-facts.md".to_string(),
            "content/blog-memory/02-decisions.md".to_string(),
        ],
        discovered_relevant_paths: vec![
            "content/blog-memory/01-facts.md".to_string(),
            "content/blog-memory/02-decisions.md".to_string(),
        ],
        web_research_attempted: false,
        web_research_succeeded: false,
        citation_count: 2,
        citations: vec![
            "mcp_list".to_string(),
            "mcp.blog_mcp.get_blog_guidelines".to_string(),
        ],
    };

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            Some("run-compare"),
            "Done\n\n{\"status\":\"completed\"}",
            &tool_telemetry,
            None,
            None,
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_none());
    assert_eq!(
        rejected.as_deref(),
        Some(
            "required output `.tandem/runs/run-compare/artifacts/compare-results.md` was not created in the current attempt"
        )
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
        Some("artifact_write_missing")
    );
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("current_attempt_output_missing"))));
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));
    assert!(artifact_validation
        .get("required_next_tool_actions")
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| {
            value.as_str().is_some_and(|text| {
                text.to_ascii_lowercase()
                    .contains("synthesize the upstream evidence")
            })
        })));
    assert!(artifact_validation
        .get("required_next_tool_actions")
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| {
            value
                .as_str()
                .is_some_and(|text| text.contains("Write the required run artifact"))
        })));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn analyze_findings_retry_without_artifact_or_required_workspace_file_surfaces_dual_write_actions()
{
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-analyze-findings-retry-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("inputs")).expect("create workspace");
    std::fs::write(
        workspace_root.join("inputs/clustered-findings.md"),
        "# Clustered findings\n\n- Repeated workflow repair failures.\n",
    )
    .expect("write clustered findings");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "analyze_findings".to_string(),
        agent_id: "analyst".to_string(),
        objective:
            "Synthesize the clustered findings into structured JSON and update the durable analysis file."
                .to_string(),
        depends_on: vec!["cluster_topics".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "cluster_topics".to_string(),
            alias: "clusters".to_string(),
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
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/analyze-findings.json",
                "output_files": ["reports/pain-points-analysis.md"]
            }
        })),
    };
    let session = Session::new(
        Some("analyze-findings-retry".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let tool_telemetry = json!({
        "requested_tools": ["glob", "read", "write"],
        "executed_tools": [],
        "tool_call_counts": {},
        "verified_output_materialized_by_current_attempt": false,
        "workspace_inspection_used": false,
    });
    let artifact_path =
        ".tandem/runs/run-analyze-findings/artifacts/analyze-findings.json".to_string();
    let artifact_text =
        "{\"status\":\"completed\",\"summary\":\"Structured analysis generated.\"}".to_string();

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            Some("run-analyze-findings"),
            "TOOL_MODE_REQUIRED_NOT_SATISFIED: WRITE_REQUIRED_NOT_SATISFIED: tool_mode=required but the model ended without executing a productive tool call.",
            &tool_telemetry,
            None,
            Some((artifact_path, artifact_text)),
            &snapshot,
            None,
        );

    assert!(accepted_output.is_none());
    assert_eq!(
        rejected.as_deref(),
        Some(
            "required output `.tandem/runs/run-analyze-findings/artifacts/analyze-findings.json` was not created in the current attempt"
        )
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("required output was not created in the current attempt")
    );
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("current_attempt_output_missing"))));
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("required_workspace_files_missing"))));
    assert_eq!(
        artifact_validation
            .get("validation_basis")
            .and_then(|value| value.get("workspace_inspection_satisfied"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        artifact_validation
            .get("validation_basis")
            .and_then(|value| value.get("must_write_files"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        vec![Value::String("reports/pain-points-analysis.md".to_string())]
    );
    assert!(artifact_validation
        .get("validation_basis")
        .and_then(|value| value.get("must_write_file_statuses"))
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| {
            value.get("path").and_then(Value::as_str) == Some("reports/pain-points-analysis.md")
                && value
                    .get("materialized_by_current_attempt")
                    .and_then(Value::as_bool)
                    == Some(false)
        })));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn node_with_bootstrap_intent_adds_workspace_inspection_prewrite_gate() {
    let mut node = bare_node();
    node.objective = "Initialize any missing directories or files if missing".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: Some(crate::AutomationOutputEnforcement {
            validation_profile: Some("artifact_only".to_string()),
            required_tools: Vec::new(),
            required_tool_calls: Vec::new(),
            required_evidence: Vec::new(),
            required_sections: Vec::new(),
            prewrite_gates: Vec::new(),
            retry_on_missing: Vec::new(),
            terminal_on: Vec::new(),
            repair_budget: None,
            session_text_recovery: None,
        }),
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": "extract.json"
        }
    }));

    let enforcement = super::super::automation::automation_node_output_enforcement(&node);
    assert!(enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "workspace_inspection"));
}

#[test]
fn structured_json_node_requires_declared_workspace_files_for_current_attempt() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-must-write-missing-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "extract_pain_points".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Write synthesis".to_string(),
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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": "extract.json",
                "must_write_files": ["02_reddit_pain_points.md"]
            }
        })),
    };
    let artifact_text =
        "{\"status\":\"completed\",\"summary\":\"Synthesis artifact already written successfully.\"}"
            .to_string();
    std::fs::write(workspace_root.join("extract.json"), &artifact_text).expect("write artifact");
    let mut session = Session::new(
        Some("must write files".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({"path":"extract.json","content":artifact_text}),
            result: Some(json!({"ok": true})),
            error: None,
        }],
    ));
    let tool_telemetry =
        summarize_automation_tool_activity(&node, &session, &["write".to_string()]);
    let (_accepted_output, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        Some(("extract.json".to_string(), artifact_text)),
        &snapshot,
    );

    assert_eq!(
        rejected.as_deref(),
        Some("required workspace files were not written for this run")
    );
    assert!(metadata
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("required_workspace_files_missing"))));
    assert!(metadata
        .get("validation_basis")
        .and_then(|value| value.get("must_write_file_statuses"))
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| {
            value.get("path").and_then(Value::as_str) == Some("02_reddit_pain_points.md")
                && value
                    .get("materialized_by_current_attempt")
                    .and_then(Value::as_bool)
                    == Some(false)
        })));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn validation_detects_and_reverts_read_only_source_of_truth_mutations() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-read-only-safety-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let workspace_root = workspace_root.to_str().expect("workspace root").to_string();

    let source_path = format!("{}/RESUME.md", workspace_root);
    std::fs::write(&source_path, "Original resume content\n").expect("write source file");

    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "assess".to_string(),
        agent_id: "agent-a".to_string(),
        objective:
            "Read RESUME.md as the source of truth. Never edit, rewrite, rename, move, or delete RESUME.md.".to_string(),
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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({"builder": {"output_path": "analyze-findings.json"}})),
    };
    let automation = AutomationSpecBuilder::new("auto-read-only")
        .workspace_root(workspace_root.clone())
        .build();

    let session_output_path = "analyze-findings.json".to_string();
    let session_output = "{\"status\":\"completed\",\"summary\":\"ok\"}".to_string();
    std::fs::write(
        format!("{}/{}", workspace_root, session_output_path),
        &session_output,
    )
    .expect("write verified output");

    let mut session = Session::new(
        Some("read-only test".to_string()),
        Some(workspace_root.clone()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({"path": session_output_path, "content": session_output}),
            result: Some(json!({"ok": true})),
            error: None,
        }],
    ));

    let snapshot_before = automation_workspace_root_file_snapshot(&workspace_root);
    let required_paths_for_node =
        enforcement::automation_node_required_source_read_paths_for_automation(
            &automation,
            &node,
            &workspace_root,
            None,
        );
    let snapshot =
        automation_read_only_file_snapshot_for_node(&workspace_root, &required_paths_for_node);
    // Simulate a mutation bug: a required read-only source file gets overwritten.
    // The snapshot must be taken before this overwrite to confirm mutation detection and restoration.
    std::fs::write(&source_path, "BAD resume content from workflow\n")
        .expect("overwrite source file");
    let tool_telemetry =
        summarize_automation_tool_activity(&node, &session, &["write".to_string()]);
    let (_accepted_output, metadata, rejected) = validate_automation_artifact_output_with_context(
        &automation,
        &node,
        &session,
        &workspace_root,
        Some("run-read-only"),
        None,
        "{\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        Some((session_output_path.clone(), session_output.clone())),
        &snapshot_before,
        None,
        Some(&snapshot),
    );

    assert!(
        rejected.is_some(),
        "expected validation rejection after source mutation"
    );
    assert!(
        metadata
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .is_some_and(|rows| rows
                .iter()
                .any(|value| value.as_str() == Some("read_only_source_mutations"))),
        "expected unmet requirement read_only_source_mutations"
    );
    assert_eq!(
        metadata
            .get("read_only_source_mutation_count")
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        metadata
            .get("read_only_source_mutation_events")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    let event = metadata
        .get("read_only_source_mutation_events")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .and_then(Value::as_object)
        .expect("mutation event");
    assert_eq!(
        event.get("path").and_then(Value::as_str),
        Some("RESUME.md"),
        "expected normalized path"
    );
    let restored = std::fs::read_to_string(&source_path).expect("restore source file");
    assert_eq!(restored, "Original resume content\n");

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn structured_json_validation_matrix_covers_artifact_only_and_workspace_side_writes() {
    let report_text = "# Reddit pain points\n\n- Brittle automations.\n";
    let no_output_files: [&str; 0] = [];
    let no_writes: [(&str, &str); 0] = [];
    let no_missing_files: [&str; 0] = [];
    let workspace_output_files = ["reports/pain-points.md"];
    let workspace_missing_files = ["reports/pain-points.md"];
    let workspace_writes = [("reports/pain-points.md", report_text)];
    let cases = vec![
        StructuredJsonWriteMatrixCase {
            name: "artifact-only-pass",
            output_files: &no_output_files,
            writes: &no_writes,
            expected_validation_outcome: "passed",
            expected_rejected: None,
            expected_missing_workspace_files: &no_missing_files,
        },
        StructuredJsonWriteMatrixCase {
            name: "workspace-side-write-missing",
            output_files: &workspace_output_files,
            writes: &no_writes,
            expected_validation_outcome: "blocked",
            expected_rejected: Some("required workspace files were not written for this run"),
            expected_missing_workspace_files: &workspace_missing_files,
        },
        StructuredJsonWriteMatrixCase {
            name: "workspace-side-write-present",
            output_files: &workspace_output_files,
            writes: &workspace_writes,
            expected_validation_outcome: "passed",
            expected_rejected: None,
            expected_missing_workspace_files: &no_missing_files,
        },
    ];

    for case in cases {
        run_structured_json_write_matrix_case(case);
    }
}
