use super::*;

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

#[test]
fn research_evidence_validation_matrix_covers_local_web_mixed_and_mcp_grounding() {
    let local_brief = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n## Campaign goal\nClarify positioning.\n\n## Target audience\n- Operators.\n\n## Core pain points\n- Coordination overhead.\n\n## Positioning angle\nTandem centralizes orchestration.\n\n## Competitor context\nLocal-only comparison for this run.\n\n## Proof points with citations\n1. Supported from docs/source.md. Source note: https://example.com/reference\n\n## Likely objections\n- Proof depth.\n\n## Channel considerations\n- Landing page.\n\n## Recommended message hierarchy\n1. Problem\n2. Promise\n\n## Files reviewed\n- docs/source.md\n\n## Files not reviewed\n- docs/extra.md: not needed for this first pass.\n";
    let mixed_brief = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources and current external research.\n\n## Campaign goal\nClarify positioning.\n\n## Target audience\n- Operators.\n\n## Core pain points\n- Coordination overhead.\n\n## Positioning angle\nTandem centralizes orchestration.\n\n## Competitor context\nExternal validation confirmed the same positioning pressure points.\n\n## Proof points with citations\n1. Supported from docs/source.md. Source note: https://example.com/reference\n2. Supported by current market coverage. Source note: https://example.com/current-market\n\n## Likely objections\n- Proof depth.\n\n## Channel considerations\n- Landing page.\n\n## Recommended message hierarchy\n1. Problem\n2. Promise\n\n## Files reviewed\n- docs/source.md\n\n## Files not reviewed\n- docs/extra.md: not needed for this pass.\n\n## Web sources reviewed\n- https://example.com/current-market\n";
    let web_citations = "# Research Sources\n\n## Summary\nCurrent external research was gathered successfully.\n\n## Citations\n1. AI Agents in 2025: Expectations vs. Reality | IBM. Source note: https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality\n2. Agentic AI, explained | MIT Sloan. Source note: https://mitsloan.mit.edu/ideas-made-to-matter/agentic-ai-explained\n\n## Web sources reviewed\n- https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality\n- https://mitsloan.mit.edu/ideas-made-to-matter/agentic-ai-explained\n";
    let mcp_citations = "# Research Sources\n\n## Summary\nCollected current Tandem MCP documentation references.\n\n## Citations\n1. Tandem MCP Guide. Source note: tandem-mcp://docs/guide\n2. Tandem MCP API Reference. Source note: tandem-mcp://docs/api-reference\n";
    let cases = vec![
        ResearchEvidenceMatrixCase {
            name: "local-only",
            node: research_brief_matrix_node("marketing-brief.md", true),
            workspace_files: vec![("docs/source.md", "source")],
            tool_invocations: vec![
                ToolInvocationSpec {
                    tool: "read",
                    args: json!({"path":"docs/source.md"}),
                    result: json!({"output":"source"}),
                },
                ToolInvocationSpec {
                    tool: "write",
                    args: json!({"path":"marketing-brief.md","content":local_brief}),
                    result: json!({"ok": true}),
                },
            ],
            requested_tools: vec!["glob", "read", "write"],
            accepted_output_path: "marketing-brief.md",
            accepted_output_content: local_brief,
            session_text: "Done\n\n{\"status\":\"completed\"}",
            expected_validation_outcome: "passed",
            expected_external_research_mode: Some("waived_unavailable"),
            absent_unmet: vec!["no_concrete_reads", "missing_successful_web_research"],
            expected_read_paths: vec!["docs/source.md"],
        },
        ResearchEvidenceMatrixCase {
            name: "web-grounded",
            node: research_citations_matrix_node(
                "research_sources",
                ".tandem/artifacts/research-sources.json",
                true,
                &[],
            ),
            workspace_files: vec![("inputs/questions.md", "Question")],
            tool_invocations: vec![
                ToolInvocationSpec {
                    tool: "read",
                    args: json!({"path":"inputs/questions.md"}),
                    result: json!({"output":"Question"}),
                },
                ToolInvocationSpec {
                    tool: "websearch",
                    args: json!({"query":"autonomous AI agentic workflows 2024 2025"}),
                    result: json!({"output":"Search results found"}),
                },
                ToolInvocationSpec {
                    tool: "write",
                    args: json!({"path":".tandem/artifacts/research-sources.json","content":web_citations}),
                    result: json!({"output":"written"}),
                },
            ],
            requested_tools: vec!["read", "write", "websearch"],
            accepted_output_path: ".tandem/artifacts/research-sources.json",
            accepted_output_content: web_citations,
            session_text: "",
            expected_validation_outcome: "passed",
            expected_external_research_mode: None,
            absent_unmet: vec![
                "no_concrete_reads",
                "missing_successful_web_research",
                "files_reviewed_missing",
                "files_reviewed_not_backed_by_read",
            ],
            expected_read_paths: vec!["inputs/questions.md"],
        },
        ResearchEvidenceMatrixCase {
            name: "mixed-local-web",
            node: research_brief_matrix_node("marketing-brief.md", true),
            workspace_files: vec![("docs/source.md", "source")],
            tool_invocations: vec![
                ToolInvocationSpec {
                    tool: "read",
                    args: json!({"path":"docs/source.md"}),
                    result: json!({"output":"source"}),
                },
                ToolInvocationSpec {
                    tool: "websearch",
                    args: json!({"query":"workflow contract testing release safety"}),
                    result: json!({"output":"Search results found"}),
                },
                ToolInvocationSpec {
                    tool: "write",
                    args: json!({"path":"marketing-brief.md","content":mixed_brief}),
                    result: json!({"ok": true}),
                },
            ],
            requested_tools: vec!["glob", "read", "write", "websearch"],
            accepted_output_path: "marketing-brief.md",
            accepted_output_content: mixed_brief,
            session_text: "Done\n\n{\"status\":\"completed\"}",
            expected_validation_outcome: "passed",
            expected_external_research_mode: None,
            absent_unmet: vec!["no_concrete_reads", "missing_successful_web_research"],
            expected_read_paths: vec!["docs/source.md"],
        },
        ResearchEvidenceMatrixCase {
            name: "mcp-grounded",
            node: research_citations_matrix_node(
                "research_sources",
                ".tandem/runs/run-mcp-citations/artifacts/research-sources.json",
                false,
                &["tandem-mcp"],
            ),
            workspace_files: vec![],
            tool_invocations: vec![
                ToolInvocationSpec {
                    tool: "mcp.tandem_mcp.search_docs",
                    args: json!({"query":"research sources artifact contract"}),
                    result: json!({"output":"Matched Tandem MCP docs"}),
                },
                ToolInvocationSpec {
                    tool: "write",
                    args: json!({"path":".tandem/runs/run-mcp-citations/artifacts/research-sources.json","content":mcp_citations}),
                    result: json!({"output":"written"}),
                },
            ],
            requested_tools: vec!["mcp.tandem_mcp.search_docs", "write"],
            accepted_output_path: ".tandem/runs/run-mcp-citations/artifacts/research-sources.json",
            accepted_output_content: mcp_citations,
            session_text: "Done\n\n{\"status\":\"completed\"}",
            expected_validation_outcome: "passed",
            expected_external_research_mode: None,
            absent_unmet: vec![
                "current_attempt_output_missing",
                "no_concrete_reads",
                "missing_successful_web_research",
            ],
            expected_read_paths: vec![],
        },
    ];

    for case in cases {
        run_research_evidence_matrix_case(case);
    }
}

#[test]
fn research_retry_state_matrix_covers_repairable_and_exhausted_statuses() {
    let cases = vec![
        RepairStateMatrixCase {
            name: "repairable-completed-wrapper",
            session_text: "Done — `marketing-brief.md` was written.",
            repair_exhausted: false,
            expected_status: "needs_repair",
            expected_reason:
                "research completed without concrete file reads or required source coverage",
            expected_failure_kind: "research_missing_reads",
            expected_summary_outcome: "needs_repair",
        },
        RepairStateMatrixCase {
            name: "repair-exhausted-completed-wrapper",
            session_text: "Done — `marketing-brief.md` was written.",
            repair_exhausted: true,
            expected_status: "blocked",
            expected_reason:
                "research completed without concrete file reads or required source coverage",
            expected_failure_kind: "research_retry_exhausted",
            expected_summary_outcome: "blocked",
        },
        RepairStateMatrixCase {
            name: "repairable-overrides-llm-blocked",
            session_text:
                "The brief is blocked.\n\n{\"status\":\"blocked\",\"reason\":\"tools unavailable\"}",
            repair_exhausted: false,
            expected_status: "needs_repair",
            expected_reason:
                "research completed without concrete file reads or required source coverage",
            expected_failure_kind: "research_missing_reads",
            expected_summary_outcome: "needs_repair",
        },
        RepairStateMatrixCase {
            name: "repair-exhausted-keeps-llm-blocked",
            session_text:
                "The brief is blocked.\n\n{\"status\":\"blocked\",\"reason\":\"tools unavailable\"}",
            repair_exhausted: true,
            expected_status: "blocked",
            expected_reason: "tools unavailable",
            expected_failure_kind: "research_retry_exhausted",
            expected_summary_outcome: "blocked",
        },
    ];

    for case in cases {
        run_repair_state_matrix_case(case);
    }
}

#[test]
fn upstream_synthesis_validation_matrix_covers_markdown_and_html_evidence_preservation() {
    let thin_report = "# Strategic Summary\n\nTandem is an engineering agent for local execution.\n\n## Positioning\n\nIt connects human intent to repo changes.\n";
    let structured_report = "# Strategy Analysis Report\n\n## 1. Executive Summary\nThis analysis synthesizes Tandem's internal product definitions and external research to refine positioning. Tandem is positioned as a high-autonomy engineering engine rather than a generic code assistant.\n\n## 2. Product Positioning\n* **Core Identity:** Tandem by Frumu AI\n* **Key Positioning:** Workspace-aware AI collaboration embedded in the engineering workflow.\n\n## 3. Risks & Proof Gaps\n* Need stronger empirical time-saved metrics.\n\n---\nSource verification: based on `.tandem/artifacts/collect-inputs.json` and `.tandem/artifacts/research-sources.json`.\n";
    let generic_html_report = r#"
<html>
  <body>
    <h1>Strategic Summary</h1>
    <p>This report synthesizes the available upstream evidence into a concise outlook.</p>
    <p>Strategic positioning remains promising.</p>
  </body>
</html>
"#
    .trim();
    let anchored_html_report = r#"
<html>
  <body>
    <h1>Frumu AI Tandem: Strategic Summary</h1>
    <p>We synthesized the local Tandem docs and the external research into one report.</p>
    <h3>Core Value Proposition</h3>
    <p>Tandem is an engine-backed workflow system for local execution and agentic operations.</p>
    <ul>
      <li>Local workspace reads and patch-based code execution.</li>
      <li>Current web research for externally grounded synthesis.</li>
      <li>Explicit delivery gating for side effects.</li>
    </ul>
    <h3>Strategic Outlook</h3>
    <p>The positioning emphasizes deterministic execution, provenance, and operator control.</p>
    <p>Sources reviewed: <a href=".tandem/runs/run-123/artifacts/analyze-findings.md">analysis</a> and <a href=".tandem/runs/run-123/artifacts/research-sources.json">research</a>.</p>
  </body>
</html>
"#
    .trim();
    let rich_upstream = AutomationUpstreamEvidence {
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
    let cases = vec![
        UpstreamSynthesisMatrixCase {
            name: "markdown-generic-summary-blocked",
            node_id: "generate_report",
            output_path: "report.md",
            artifact_text: thin_report,
            session_text: "Completed the report.",
            write_path: "report.md",
            tool_telemetry: json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            upstream_evidence: rich_upstream.clone(),
            expected_validation_outcome: "blocked",
            expected_rejected: Some(
                "final artifact does not adequately synthesize the available upstream evidence",
            ),
            expect_upstream_unsynthesized: true,
        },
        UpstreamSynthesisMatrixCase {
            name: "markdown-structured-synthesis-passes",
            node_id: "analyze_findings",
            output_path: "analyze-findings.md",
            artifact_text: structured_report,
            session_text: "Completed the report.",
            write_path: "analyze-findings.md",
            tool_telemetry: json!({
                "requested_tools": ["read", "write"],
                "executed_tools": ["read", "write"],
                "tool_call_counts": {
                    "read": 2,
                    "write": 1
                }
            }),
            upstream_evidence: AutomationUpstreamEvidence {
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
            },
            expected_validation_outcome: "accepted_with_warnings",
            expected_rejected: None,
            expect_upstream_unsynthesized: false,
        },
        UpstreamSynthesisMatrixCase {
            name: "html-generic-summary-blocked",
            node_id: "generate_report",
            output_path: "generate-report.md",
            artifact_text: generic_html_report,
            session_text: "Completed the report.",
            write_path: "generate-report.md",
            tool_telemetry: json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            upstream_evidence: rich_upstream.clone(),
            expected_validation_outcome: "blocked",
            expected_rejected: Some(
                "final artifact does not adequately synthesize the available upstream evidence",
            ),
            expect_upstream_unsynthesized: true,
        },
        UpstreamSynthesisMatrixCase {
            name: "html-anchored-synthesis-passes",
            node_id: "generate_report",
            output_path: "generate-report.md",
            artifact_text: anchored_html_report,
            session_text: "Completed the report.",
            write_path: "generate-report.md",
            tool_telemetry: json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            upstream_evidence: rich_upstream,
            expected_validation_outcome: "passed",
            expected_rejected: None,
            expect_upstream_unsynthesized: false,
        },
    ];

    for case in cases {
        run_upstream_synthesis_matrix_case(case);
    }
}

#[test]
fn structured_json_node_passes_when_declared_workspace_files_are_written() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-must-write-present-{}",
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
        "{\"status\":\"completed\",\"summary\":\"Pain point synthesis completed.\"}".to_string();
    let markdown_text = "# Reddit pain points\n\n- Brittle automations.\n".to_string();
    std::fs::write(workspace_root.join("extract.json"), &artifact_text).expect("write artifact");
    std::fs::write(
        workspace_root.join("02_reddit_pain_points.md"),
        &markdown_text,
    )
    .expect("write markdown");
    let mut session = Session::new(
        Some("must write files".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"extract.json","content":artifact_text}),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"02_reddit_pain_points.md","content":markdown_text}),
                result: Some(json!({"ok": true})),
                error: None,
            },
        ],
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

    assert_eq!(rejected, None);
    assert_eq!(
        metadata.get("validation_outcome").and_then(Value::as_str),
        Some("passed")
    );
    assert!(metadata
        .get("validation_basis")
        .and_then(|value| value.get("must_write_file_statuses"))
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| {
            value.get("path").and_then(Value::as_str) == Some("02_reddit_pain_points.md")
                && value
                    .get("materialized_by_current_attempt")
                    .and_then(Value::as_bool)
                    == Some(true)
        })));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn explicit_output_files_override_legacy_must_write_files() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-explicit-output-files-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "draft_report".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Write report".to_string(),
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
                "must_write_files": ["legacy.md"],
                "output_files": ["reports/final.md"]
            }
        })),
    };
    let artifact_text =
        "{\"status\":\"completed\",\"summary\":\"Final report ready.\"}".to_string();
    let final_report = "# Final report\n\nDone.\n".to_string();
    std::fs::write(workspace_root.join("extract.json"), &artifact_text).expect("write artifact");
    std::fs::create_dir_all(workspace_root.join("reports")).expect("create reports directory");
    std::fs::write(workspace_root.join("reports/final.md"), &final_report)
        .expect("write final report");
    let mut session = Session::new(
        Some("explicit output files".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"extract.json","content":artifact_text}),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"reports/final.md","content":final_report}),
                result: Some(json!({"ok": true})),
                error: None,
            },
        ],
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

    assert_eq!(rejected, None);
    assert_eq!(
        metadata
            .get("validation_basis")
            .and_then(|value| value.get("explicit_output_files"))
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["reports/final.md"])
    );
    assert!(metadata
        .get("validation_basis")
        .and_then(|value| value.get("must_write_file_statuses"))
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| {
            value.get("path").and_then(Value::as_str) == Some("reports/final.md")
                && value
                    .get("materialized_by_current_attempt")
                    .and_then(Value::as_bool)
                    == Some(true)
        })));
    assert!(metadata
        .get("validation_basis")
        .and_then(|value| value.get("must_write_file_statuses"))
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .all(|value| { value.get("path").and_then(Value::as_str) != Some("legacy.md") })));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn code_verification_status_matrix_covers_missing_failed_and_satisfied_checks() {
    let cases = vec![
        CodeVerificationMatrixCase {
            name: "verification-failed",
            verification_command: Some("cargo test"),
            session_text: "Done\n\n{\"status\":\"completed\"}",
            tool_telemetry: json!({
                "requested_tools": ["glob", "read", "edit", "apply_patch", "write", "bash"],
                "executed_tools": ["read", "apply_patch", "bash"],
                "verification_expected": true,
                "verification_ran": true,
                "verification_failed": true,
                "latest_verification_failure": "verification command failed with exit code 101: cargo test"
            }),
            expected_status: "verify_failed",
            expected_reason: Some("verification command failed with exit code 101: cargo test"),
            expected_failure_kind: Some("verification_failed"),
        },
        CodeVerificationMatrixCase {
            name: "verification-missing",
            verification_command: Some("cargo test"),
            session_text: "Done\n\n{\"status\":\"completed\"}",
            tool_telemetry: json!({
                "requested_tools": ["glob", "read", "edit", "apply_patch", "write", "bash"],
                "executed_tools": ["read", "apply_patch"],
                "verification_expected": true,
                "verification_ran": false,
                "verification_failed": false
            }),
            expected_status: "needs_repair",
            expected_reason: Some(
                "coding task completed without running the declared verification command",
            ),
            expected_failure_kind: None,
        },
        CodeVerificationMatrixCase {
            name: "verification-satisfied",
            verification_command: Some("cargo test"),
            session_text: "Done\n\n{\"status\":\"completed\"}",
            tool_telemetry: json!({
                "requested_tools": ["glob", "read", "edit", "apply_patch", "write", "bash"],
                "executed_tools": ["read", "apply_patch", "bash"],
                "verification_expected": true,
                "verification_ran": true,
                "verification_failed": false
            }),
            expected_status: "done",
            expected_reason: None,
            expected_failure_kind: Some("verification_passed"),
        },
        CodeVerificationMatrixCase {
            name: "verification-not-required",
            verification_command: None,
            session_text: "Done\n\n{\"status\":\"completed\"}",
            tool_telemetry: json!({
                "requested_tools": ["glob", "read", "edit", "apply_patch", "write"],
                "executed_tools": ["read", "apply_patch", "write"],
                "verification_expected": false,
                "verification_ran": false,
                "verification_failed": false
            }),
            expected_status: "done",
            expected_reason: None,
            expected_failure_kind: Some("verification_passed"),
        },
    ];

    for case in cases {
        run_code_verification_matrix_case(case);
    }
}

#[test]
fn email_delivery_status_matrix_covers_repairable_unavailable_failed_and_succeeded_paths() {
    let cases = vec![
        DeliveryMatrixCase {
            name: "offered-tools-not-executed",
            session_text: "A Gmail draft has been created.\n\n{\"status\":\"completed\",\"approved\":true}",
            tool_telemetry: json!({
                "requested_tools": ["glob", "read", "mcp_list"],
                "executed_tools": ["read", "glob", "mcp_list"],
                "email_delivery_attempted": false,
                "email_delivery_succeeded": false,
                "latest_email_delivery_failure": null,
                "attempt_evidence": {
                    "delivery": {"status": "not_attempted"}
                },
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
            expected_status: "needs_repair",
            expected_reason:
                "email delivery to `test@example.com` was requested but no email draft/send tool executed",
            expected_blocker_category: "delivery_not_executed",
        },
        DeliveryMatrixCase {
            name: "no-email-tools-available",
            session_text: "{\"status\":\"completed\",\"approved\":true}",
            tool_telemetry: json!({
                "requested_tools": ["read", "mcp_list"],
                "executed_tools": ["read", "mcp_list"],
                "email_delivery_attempted": false,
                "email_delivery_succeeded": false,
                "latest_email_delivery_failure": null,
                "attempt_evidence": {
                    "delivery": {"status": "not_attempted"}
                },
                "capability_resolution": {
                    "mcp_tool_diagnostics": {
                        "selected_servers": ["gmail-main"],
                        "remote_tools": [],
                        "registered_tools": []
                    },
                    "email_tool_diagnostics": {
                        "available_tools": [],
                        "offered_tools": [],
                        "available_send_tools": [],
                        "offered_send_tools": [],
                        "available_draft_tools": [],
                        "offered_draft_tools": []
                    }
                }
            }),
            expected_status: "blocked",
            expected_reason:
                "email delivery to `test@example.com` was requested but no email-capable tools were available. Selected MCP servers: gmail-main. Remote MCP tools on selected servers: none. Registered tool-registry tools on selected servers: none. Discovered email-like tools: none. Offered email-like tools: none. This usually means the email connector is unavailable, MCP tools were not synced into the registry, or the tool names did not match email capability detection.",
            expected_blocker_category: "tool_unavailable",
        },
        DeliveryMatrixCase {
            name: "attempted-delivery-failed",
            session_text: "{\"status\":\"completed\",\"approved\":true}",
            tool_telemetry: json!({
                "requested_tools": ["mcp.composio_1.gmail_send_email"],
                "executed_tools": ["mcp.composio_1.gmail_send_email"],
                "email_delivery_attempted": true,
                "email_delivery_succeeded": false,
                "latest_email_delivery_failure": "smtp unauthorized",
                "attempt_evidence": {
                    "delivery": {"status": "attempted_failed"}
                },
                "capability_resolution": {
                    "email_tool_diagnostics": {
                        "available_tools": ["mcp.composio_1.gmail_send_email"],
                        "offered_tools": ["mcp.composio_1.gmail_send_email"],
                        "available_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "offered_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "available_draft_tools": [],
                        "offered_draft_tools": []
                    }
                }
            }),
            expected_status: "blocked",
            expected_reason: "smtp unauthorized",
            expected_blocker_category: "delivery_not_executed",
        },
        DeliveryMatrixCase {
            name: "delivery-succeeded",
            session_text: "{\"status\":\"completed\",\"approved\":true}",
            tool_telemetry: json!({
                "requested_tools": ["mcp.composio_1.gmail_send_email"],
                "executed_tools": ["mcp.composio_1.gmail_send_email"],
                "email_delivery_attempted": true,
                "email_delivery_succeeded": true,
                "latest_email_delivery_failure": null,
                "attempt_evidence": {
                    "delivery": {"status": "succeeded"}
                },
                "capability_resolution": {
                    "email_tool_diagnostics": {
                        "available_tools": ["mcp.composio_1.gmail_send_email"],
                        "offered_tools": ["mcp.composio_1.gmail_send_email"],
                        "available_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "offered_send_tools": ["mcp.composio_1.gmail_send_email"],
                        "available_draft_tools": [],
                        "offered_draft_tools": []
                    }
                }
            }),
            expected_status: "completed",
            expected_reason: "",
            expected_blocker_category: "",
        },
    ];

    for case in cases {
        if case.expected_blocker_category.is_empty() {
            let node = email_delivery_matrix_node();
            let (status, reason, approved): (String, Option<String>, Option<bool>) =
                detect_automation_node_status(
                    &node,
                    case.session_text,
                    None,
                    &case.tool_telemetry,
                    None,
                );
            assert_eq!(status, case.expected_status, "case={}", case.name);
            assert_eq!(reason.as_deref(), None, "case={}", case.name);
            assert_eq!(approved, Some(true), "case={}", case.name);
            assert_eq!(
                detect_automation_blocker_category(
                    &node,
                    &status,
                    reason.as_deref(),
                    &case.tool_telemetry,
                    None,
                ),
                None,
                "case={}",
                case.name
            );
        } else {
            run_delivery_matrix_case(case);
        }
    }
}

#[test]
fn upstream_shape_matrix_covers_none_strict_rich_and_legacy_rich_modes() {
    let generic_report = "# Summary\n\nPlaceholder update.\n";
    let no_upstream_report = "# Summary\n\nA concise report without upstream dependencies.\n";
    let rich_upstream = AutomationUpstreamEvidence {
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
    let cases = vec![
        UpstreamShapeMatrixCase {
            name: "no-upstream-generic-summary-passes",
            quality_mode: None,
            legacy_rollback_enabled: None,
            artifact_text: no_upstream_report,
            upstream_evidence: None,
            expected_validation_outcome: "accepted_with_warnings",
            expected_rejected: None,
            expected_warning_count: None,
            expect_upstream_unsynthesized: false,
        },
        UpstreamShapeMatrixCase {
            name: "strict-rich-upstream-blocks-generic-summary",
            quality_mode: None,
            legacy_rollback_enabled: None,
            artifact_text: generic_report,
            upstream_evidence: Some(rich_upstream.clone()),
            expected_validation_outcome: "blocked",
            expected_rejected: Some(
                "final artifact does not adequately synthesize the available upstream evidence",
            ),
            expected_warning_count: Some(2),
            expect_upstream_unsynthesized: true,
        },
        UpstreamShapeMatrixCase {
            name: "legacy-rich-upstream-allows-generic-summary",
            quality_mode: Some("legacy"),
            legacy_rollback_enabled: Some(true),
            artifact_text: generic_report,
            upstream_evidence: Some(rich_upstream),
            expected_validation_outcome: "passed",
            expected_rejected: None,
            expected_warning_count: Some(0),
            expect_upstream_unsynthesized: false,
        },
    ];

    for case in cases {
        run_upstream_shape_matrix_case(case);
    }
}

#[test]
fn report_with_blocked_content_and_completed_status_is_not_blocked() {
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
    let tool_telemetry = json!({
        "requested_tools": ["write"],
        "executed_tools": ["write"],
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "{\"status\":\"completed\"}",
            Some(&(
                "outputs/generate-report.md".to_string(),
                "# Report\n\nPipeline status: blocked by missing resume grounding artifacts.\n\nThe report is complete for the available evidence.".to_string(),
            )),
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "completed");
    assert_eq!(reason, None);
    assert_eq!(approved, None);
}

#[test]
fn report_describing_test_failures_with_completed_status_passes() {
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
    let tool_telemetry = json!({
        "requested_tools": ["write"],
        "executed_tools": ["write"],
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "{\"status\":\"completed\"}",
            Some(&(
                "outputs/generate-report.md".to_string(),
                "# CI Summary\n\nSeveral integration tests failed in the prior run, but this report artifact was generated successfully.".to_string(),
            )),
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "completed");
    assert_eq!(reason, None);
    assert_eq!(approved, None);
}

#[test]
fn explicit_blocked_status_still_detected() {
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
    let tool_telemetry = json!({
        "requested_tools": ["write"],
        "executed_tools": ["write"],
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "{\"status\":\"blocked\",\"reason\":\"waiting for more evidence\"}",
            Some(&(
                "outputs/generate-report.md".to_string(),
                "# Report\n\nPipeline status: blocked by missing resume grounding artifacts."
                    .to_string(),
            )),
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "blocked");
    assert_eq!(reason.as_deref(), Some("waiting for more evidence"));
    assert_eq!(approved, None);
}

#[test]
fn render_automation_repair_brief_summarizes_previous_research_miss() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-brief".to_string(),
        agent_id: "research".to_string(),
        objective: "Write marketing-brief.md".to_string(),
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
    let prior_output = json!({
        "status": "needs_repair",
        "validator_summary": {
            "reason": "research completed without required current web research",
            "unmet_requirements": [
                "missing_successful_web_research",
                "web_sources_reviewed_missing"
            ]
        },
        "tool_telemetry": {
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": ["glob", "write"]
        },
        "artifact_validation": {
            "blocking_classification": "tool_available_but_not_used",
            "unreviewed_relevant_paths": ["docs/pricing.md", "docs/customers.md"],
            "repair_attempt": 1,
            "repair_attempts_remaining": 4,
            "validation_basis": {
                "authority": "filesystem_and_receipts",
                "current_attempt_output_materialized": true,
                "current_attempt_has_recorded_activity": true,
                "current_attempt_has_read": false,
                "current_attempt_has_web_research": false,
                "workspace_inspection_satisfied": false
            },
            "required_next_tool_actions": [
                "Use `read` on the remaining relevant workspace files: docs/pricing.md, docs/customers.md.",
                "Use `websearch` successfully and include the resulting sources in `Web sources reviewed`."
            ]
        }
    });

    let brief = render_automation_repair_brief(&node, Some(&prior_output), 2, 5, Some("run-123"))
        .expect("repair brief");

    assert!(brief.contains("needs_repair"));
    assert!(brief.contains("missing_successful_web_research"));
    assert!(brief.contains("tool_available_but_not_used"));
    assert!(brief.contains("authority=filesystem_and_receipts"));
    assert!(brief.contains("output_materialized=true"));
    assert!(brief.contains("Required next tool actions"));
    assert!(brief.contains("Use `read` on the remaining relevant workspace files"));
    assert!(brief.contains("glob, read, websearch, write"));
    assert!(brief.contains("glob, write"));
    assert!(brief.contains("docs/pricing.md, docs/customers.md"));
    assert!(brief.contains("Remaining repair attempts after this run: 3"));
}

#[test]
fn code_patch_repair_brief_mentions_patch_apply_test_loop() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "code_patch".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Patch the code and verify the change.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "code_patch".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::CodePatch),
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
                "output_path": "src/lib.rs",
                "verification_command": "cargo test",
                "write_scope": "repo-scoped edits"
            }
        })),
    };
    let prior_output = json!({
        "status": "needs_repair",
        "validator_summary": {
            "reason": "verification did not run",
            "unmet_requirements": ["verification_missing"]
        },
        "tool_telemetry": {
            "requested_tools": ["glob", "read", "edit", "apply_patch", "write"],
            "executed_tools": ["glob", "read", "write"]
        },
        "artifact_validation": {
            "blocking_classification": "verification_required",
            "repair_attempt": 1,
            "repair_attempts_remaining": 4,
            "required_next_tool_actions": [
                "Patch the code with `edit` or `apply_patch` before any new `write`.",
                "Run `cargo test` after the patch and fix the smallest failing root cause."
            ]
        }
    });

    let brief = render_automation_repair_brief(&node, Some(&prior_output), 2, 5, Some("run-123"))
        .expect("repair brief");

    assert!(brief.contains("Code workflow repair path"));
    assert!(brief.contains("inspect the touched files"));
    assert!(brief.contains("edit` or `apply_patch"));
    assert!(brief.contains("cargo test"));
    assert!(brief.contains("repo-scoped edits"));
}

#[test]
fn render_automation_repair_brief_adds_final_attempt_escalation() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-brief".to_string(),
        agent_id: "research".to_string(),
        objective: "Write marketing-brief.md".to_string(),
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
                "output_path": ".tandem/artifacts/marketing-brief.md"
            }
        })),
    };
    let prior_output = json!({
        "status": "needs_repair",
        "validator_summary": {
            "reason": "research completed without required current web research",
            "unmet_requirements": ["missing_successful_web_research"]
        },
        "artifact_validation": {
            "blocking_classification": "tool_available_but_not_used",
            "repair_attempt": 2,
            "repair_attempts_remaining": 1
        }
    });

    let brief = render_automation_repair_brief(&node, Some(&prior_output), 3, 3, Some("run-123"))
        .expect("repair brief");

    assert!(brief.contains("FINAL ATTEMPT"));
    assert!(brief.contains(".tandem/runs/run-123/artifacts/marketing-brief.md"));
    assert!(!brief.contains("The engine will accept the output file at `.tandem/artifacts/"));
    assert!(brief.contains("{\"status\":\"completed\"}"));
    assert!(brief.contains("Do not ask follow-up questions."));
}

#[test]
fn repair_brief_detects_activity_despite_empty_telemetry() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "analyze-findings".to_string(),
        agent_id: "analyst".to_string(),
        objective: "Write analyze-findings.json".to_string(),
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
                "output_path": ".tandem/artifacts/analyze-findings.json"
            }
        })),
    };
    let prior_output = json!({
        "status": "needs_repair",
        "validator_summary": {
            "reason": "required output was not created",
            "unmet_requirements": []
        },
        "tool_telemetry": {
            "requested_tools": [],
            "executed_tools": []
        },
        "artifact_validation": {
            "blocking_classification": "execution_error",
            "repair_attempt": 2,
            "repair_attempts_remaining": 1,
            "required_next_tool_actions": [
                "Retry after provider connectivity recovers."
            ],
            "validation_basis": {
                "authority": "filesystem_and_receipts",
                "current_attempt_has_recorded_activity": true,
                "current_attempt_output_materialized": false,
                "current_attempt_has_read": true,
                "current_attempt_has_web_research": false,
                "workspace_inspection_satisfied": true
            }
        }
    });

    let brief = render_automation_repair_brief(&node, Some(&prior_output), 3, 3, Some("run-123"))
        .expect("repair brief");

    assert!(brief
        .contains("Tools offered last attempt: not recorded (but session activity was detected)."));
    assert!(brief.contains("Blocking classification: artifact_write_missing."));
    assert!(brief.contains(
        "Required next tool actions: write the required run artifact to the declared output path."
    ));
    assert!(brief.contains(".tandem/runs/run-123/artifacts/analyze-findings.json"));
}

#[test]
fn repair_attempt_with_concrete_read_and_changed_output_is_accepted() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-repair-read-changed-output-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("docs")).expect("create workspace");
    std::fs::write(
        workspace_root.join("docs/pricing.md"),
        "# Pricing\n\n- Teams plan starts at $49 per seat.\n",
    )
    .expect("write source file");
    let preexisting_output = "# Marketing Brief\n\nOld draft.\n".to_string();
    std::fs::write(
        workspace_root.join("marketing-brief.md"),
        &preexisting_output,
    )
    .expect("write previous output");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-brief".to_string(),
        agent_id: "research".to_string(),
        objective: "Write marketing-brief.md".to_string(),
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
    let final_output = "# Marketing Brief\n\n## Findings\nThe team plan starts at $49 per seat and the revised workflow now captures concrete pricing evidence from docs/pricing.md.\n\n## Files reviewed\n- docs/pricing.md\n".to_string();
    std::fs::write(workspace_root.join("marketing-brief.md"), &final_output)
        .expect("write repaired output");
    let mut session = Session::new(
        Some("repair attempt".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"file_path":"docs/pricing.md"}),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"marketing-brief.md","content":"# Marketing Brief\n\nWorking draft.\n"}),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"marketing-brief.md","content":final_output}),
                result: Some(json!({"ok": true})),
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
            "websearch".to_string(),
            "write".to_string(),
        ],
    );
    let (_accepted_output, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "I repaired the artifact and rewrote the output file.",
        &tool_telemetry,
        Some(&preexisting_output),
        Some(("marketing-brief.md".to_string(), final_output)),
        &snapshot,
    );

    assert_eq!(rejected, None);
    assert_eq!(
        metadata.get("repair_succeeded").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        metadata
            .get("validation_basis")
            .and_then(|value| value.get("repair_promoted_after_read_and_output_change"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        metadata
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .map(|values| values.len()),
        Some(0)
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn automation_output_enforcement_prefers_contract_over_legacy_builder_metadata() {
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
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: None,
                required_tools: vec!["read".to_string()],
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: vec!["files_reviewed".to_string()],
                prewrite_gates: vec!["workspace_inspection".to_string()],
                retry_on_missing: vec!["local_source_reads".to_string()],
                terminal_on: vec!["repair_budget_exhausted".to_string()],
                repair_budget: Some(2),
                session_text_recovery: Some("disabled".to_string()),
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
                "output_path": "marketing-brief.md",
                "required_tools": ["read", "websearch"],
                "web_research_expected": true
            }
        })),
    };

    let enforcement = automation_node_output_enforcement(&node);
    assert_eq!(enforcement.required_tools, vec!["read"]);
    assert_eq!(enforcement.required_evidence, vec!["local_source_reads"]);
    assert_eq!(
        enforcement.session_text_recovery.as_deref(),
        Some("disabled")
    );
}

#[test]
fn automation_output_enforcement_backfills_research_contract_from_legacy_builder_metadata() {
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
                "required_tools": ["read", "websearch"],
                "web_research_expected": true
            }
        })),
    };

    let enforcement = automation_node_output_enforcement(&node);
    assert!(enforcement.required_tools.iter().any(|tool| tool == "read"));
    assert!(enforcement
        .required_tools
        .iter()
        .any(|tool| tool == "websearch"));
    assert!(enforcement
        .required_sections
        .iter()
        .any(|item| item == "web_sources_reviewed"));
    assert_eq!(
        enforcement.session_text_recovery.as_deref(),
        Some("require_prewrite_satisfied")
    );
}

#[test]
fn structured_handoff_workspace_bootstrap_nodes_treat_reads_as_optional() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "workspace-operator".to_string(),
        objective: "Initialize any missing job-search workspace directories and files, read README.md if present, and update resume-overview.md, tracker/search-ledger/2026-04-07.json, tracker/seen-jobs.jsonl, and daily-recaps/2026-04-07-job-search-recap.md.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Return a structured handoff.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: None,
    };

    let enforcement = automation_node_output_enforcement(&node);
    assert!(enforcement.required_tools.iter().any(|tool| tool == "glob"));
    assert!(enforcement
        .required_tools
        .iter()
        .any(|tool| tool == "write"));
    assert!(!enforcement.required_tools.iter().any(|tool| tool == "read"));
    assert_eq!(
        enforcement.validation_profile.as_deref(),
        Some("artifact_only")
    );
    assert!(!enforcement
        .required_evidence
        .iter()
        .any(|evidence| evidence == "local_source_reads"));

    let capabilities = automation_tool_capability_ids(&node, "artifact_write");
    assert!(capabilities
        .iter()
        .any(|capability| capability == "workspace_discover"));
    assert!(capabilities
        .iter()
        .any(|capability| capability == "artifact_write"));
    assert!(!capabilities
        .iter()
        .any(|capability| capability == "workspace_read"));
}

#[test]
fn bootstrap_workspace_output_nodes_require_inspection_but_not_concrete_reads() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "workspace-operator".to_string(),
        objective: "Initialize any missing job-search workspace directories and files, read README.md if present, and update resume-overview.md, tracker/search-ledger/2026-04-07.json, tracker/seen-jobs.jsonl, and daily-recaps/2026-04-07-job-search-recap.md.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
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
                "output_path": "daily-recaps/2026-04-07-job-search-recap.md"
            }
        })),
    };

    let requirements = automation_node_prewrite_requirements(
        &node,
        &["glob".to_string(), "read".to_string(), "write".to_string()],
    )
    .expect("prewrite requirements");
    assert!(requirements.workspace_inspection_required);
    assert!(!requirements.concrete_read_required);
}

#[test]
fn bootstrap_required_files_are_inferred_from_objective_paths_without_filename_hardcoding() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-bootstrap-required-files-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "workspace-operator".to_string(),
        objective: "Initialize any missing workspace files, read notes/existing-context.md if present, and update guides/setup-guide.md and tracker/jobs.jsonl.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
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
                "output_path": "daily-recaps/2026-04-08-recap.md"
            }
        })),
    };
    let artifact_text =
        "{\"status\":\"completed\",\"summary\":\"Bootstrap completed.\"}".to_string();
    let setup_guide = "# Setup guide\n\nBootstrap complete.\n".to_string();
    let jobs_ledger = "{\"jobs\":[]}\n".to_string();
    std::fs::create_dir_all(workspace_root.join("daily-recaps")).expect("create recap dir");
    std::fs::create_dir_all(workspace_root.join("guides")).expect("create guides dir");
    std::fs::create_dir_all(workspace_root.join("tracker")).expect("create tracker dir");
    std::fs::write(
        workspace_root.join("daily-recaps/2026-04-08-recap.md"),
        &artifact_text,
    )
    .expect("write output");
    std::fs::write(workspace_root.join("guides/setup-guide.md"), &setup_guide)
        .expect("write setup guide");
    std::fs::write(workspace_root.join("tracker/jobs.jsonl"), &jobs_ledger)
        .expect("write jobs ledger");
    let mut session = Session::new(
        Some("bootstrap required files".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"daily-recaps/2026-04-08-recap.md","content":artifact_text}),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"guides/setup-guide.md","content":setup_guide}),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({"path":"tracker/jobs.jsonl","content":jobs_ledger}),
                result: Some(json!({"ok": true})),
                error: None,
            },
        ],
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
        Some((
            "daily-recaps/2026-04-08-recap.md".to_string(),
            artifact_text.clone(),
        )),
        &snapshot,
    );

    assert_eq!(rejected, None);
    assert_eq!(
        metadata.get("validation_outcome").and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        metadata
            .get("validation_basis")
            .and_then(|value| value.get("must_write_files"))
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        vec![
            Value::String("guides/setup-guide.md".to_string()),
            Value::String("tracker/jobs.jsonl".to_string()),
        ]
    );
    assert!(metadata
        .get("validation_basis")
        .and_then(|value| value.get("must_write_file_statuses"))
        .and_then(Value::as_array)
        .is_some_and(|values| {
            values.iter().any(|value| {
                value.get("path").and_then(Value::as_str) == Some("guides/setup-guide.md")
                    && value
                        .get("materialized_by_current_attempt")
                        .and_then(Value::as_bool)
                        == Some(true)
            }) && values.iter().any(|value| {
                value.get("path").and_then(Value::as_str) == Some("tracker/jobs.jsonl")
                    && value
                        .get("materialized_by_current_attempt")
                        .and_then(Value::as_bool)
                        == Some(true)
            })
        }));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn research_nodes_default_to_five_attempts() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-brief".to_string(),
        agent_id: "research".to_string(),
        objective: "Write marketing-brief.md".to_string(),
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
        stage_kind: None,
        gate: None,
        metadata: None,
    };

    assert_eq!(automation_node_max_attempts(&node), 5);
}
