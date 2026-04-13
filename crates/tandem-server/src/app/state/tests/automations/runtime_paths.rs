use super::*;

#[test]
fn standard_workflow_nodes_receive_default_workspace_output_paths() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research_sources".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Research sources".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "citations".to_string(),
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

    assert_eq!(
        automation_node_required_output_path(&node).as_deref(),
        Some(".tandem/artifacts/research-sources.json")
    );
}

#[test]
fn report_markdown_retries_accept_html_sibling_outputs() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-html-sibling-{}",
        uuid::Uuid::new_v4()
    ));
    let artifact_dir = workspace_root.join(".tandem/runs/run-research/artifacts");
    std::fs::create_dir_all(&artifact_dir).expect("create artifact dir");
    std::fs::write(
        artifact_dir.join("generate-report.html"),
        "<!doctype html><html><body>Report</body></html>",
    )
    .expect("write html artifact");

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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/generate-report.md"
            }
        })),
    };
    let mut session = Session::new(
        Some("generate-report-retry".to_string()),
        Some(workspace_root.to_str().expect("workspace utf8").to_string()),
    );
    let expected_output_path = crate::app::state::automation::automation_run_scoped_output_path(
        "run-research",
        ".tandem/artifacts/generate-report.md",
    )
    .expect("scoped output path");
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": expected_output_path.replace("generate-report.md", "generate-report.html"),
                "content": "<!doctype html><html><body>Report</body></html>"
            }),
            result: Some(json!({"output":"written"})),
            error: None,
        }],
    ));

    let resolved = automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace utf8"),
        "run-research",
        &node,
        ".tandem/artifacts/generate-report.md",
    )
    .expect("resolve verified output")
    .expect("accepted sibling output");

    assert_eq!(
        resolved
            .file_name()
            .and_then(|value| value.to_str())
            .expect("file name"),
        "generate-report.html"
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn automation_resolve_verified_output_path_accepts_file_path_schema_with_dot_segments() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-html-sibling-file-path-{}",
        uuid::Uuid::new_v4()
    ));
    let artifact_path = workspace_root.join(".tandem/runs/run-research/artifacts/report.md");
    std::fs::create_dir_all(
        artifact_path
            .parent()
            .expect("artifact path should have parent"),
    )
    .expect("create artifact dir");
    std::fs::write(&artifact_path, "report body").expect("write artifact");

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
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/report.md"
            }
        })),
    };
    let mut session = Session::new(
        Some("generate-report-file-path".to_string()),
        Some(workspace_root.to_str().expect("workspace utf8").to_string()),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "filePath": artifact_path
                    .parent()
                    .expect("artifact path should have parent")
                    .join("./report.md")
                    .to_string_lossy(),
                "content": "report body"
            }),
            result: Some(json!({"output":"written"})),
            error: None,
        }],
    ));

    let resolved = automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace utf8"),
        "run-research",
        &node,
        ".tandem/artifacts/report.md",
    )
    .expect("resolve verified output")
    .expect("accepted normalized output");

    assert_eq!(resolved, artifact_path);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn citations_nodes_do_not_require_files_reviewed_sections_by_default() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research_sources".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Research sources".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "citations".to_string(),
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
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/research-sources.json",
                "web_research_expected": true,
                "source_coverage_required": true
            }
        })),
    };

    let enforcement = automation_node_output_enforcement(&node);

    assert!(enforcement
        .required_sections
        .iter()
        .any(|item| item == "citations"));
    assert!(enforcement
        .validation_profile
        .as_deref()
        .is_some_and(|value| value == "external_research"));
    assert!(!enforcement
        .required_sections
        .iter()
        .any(|item| item == "files_reviewed"));
    assert!(!enforcement
        .required_sections
        .iter()
        .any(|item| item == "files_not_reviewed"));
}

#[test]
fn collect_inputs_nodes_write_deterministic_inline_artifacts() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "collect_inputs".to_string(),
        agent_id: "planner".to_string(),
        objective: "Gather workflow inputs".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
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
            "inputs": {
                "topic": "autonomous AI agentic workflows",
                "delivery_email": "recipient@example.com",
                "email_format": "simple html",
                "attachments_allowed": false
            }
        })),
    };

    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-inline-artifact-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_nanos()
    ));
    std::fs::create_dir_all(&workspace_root).expect("temp workspace");

    let output_path =
        automation_node_required_output_path(&node).expect("collect_inputs output path");
    let payload = automation_node_inline_artifact_payload(&node).expect("inline payload");
    let (written_path, file_text) = write_automation_inline_artifact(
        workspace_root.to_str().expect("workspace utf8"),
        "run-inline-collect",
        &output_path,
        &payload,
    )
    .expect("inline artifact write");

    assert_eq!(
        written_path,
        ".tandem/runs/run-inline-collect/artifacts/collect-inputs.json"
    );
    assert!(file_text.contains("autonomous AI agentic workflows"));

    let resolved =
        workspace_root.join(".tandem/runs/run-inline-collect/artifacts/collect-inputs.json");
    assert!(resolved.exists());
    let persisted = std::fs::read_to_string(&resolved).expect("read artifact");
    assert!(persisted.contains("\"delivery_email\": \"recipient@example.com\""));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn collect_inputs_without_explicit_inputs_do_not_use_deterministic_inline_artifacts() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "collect_inputs".to_string(),
        agent_id: "planner".to_string(),
        objective: "Inspect the workspace and resolve runtime values.".to_string(),
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
                "web_research_expected": false
            }
        })),
    };

    assert!(automation_node_required_output_path(&node).is_some());
    assert!(automation_node_inline_artifact_payload(&node).is_none());
}
