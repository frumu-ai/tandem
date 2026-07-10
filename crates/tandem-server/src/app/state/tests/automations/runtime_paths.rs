use super::*;

fn test_flow_node(
    node_id: &str,
    kind: &str,
    validator: crate::AutomationOutputValidatorKind,
    metadata: Option<serde_json::Value>,
) -> AutomationFlowNode {
    AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: node_id.to_string(),
        agent_id: "test-agent".to_string(),
        objective: format!("Run {node_id}"),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: kind.to_string(),
            validator: Some(validator),
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
        metadata,
    }
}

#[test]
fn standard_workflow_nodes_receive_default_workspace_output_paths() {
    let node = test_flow_node(
        "research_sources",
        "citations",
        crate::AutomationOutputValidatorKind::ResearchBrief,
        None,
    );

    assert_eq!(
        automation_node_required_output_path(&node).as_deref(),
        Some(".tandem/artifacts/research-sources.json")
    );
}

#[test]
fn compare_results_nodes_receive_default_workspace_output_paths() {
    let node = test_flow_node(
        "compare_results",
        "report_markdown",
        crate::AutomationOutputValidatorKind::GenericArtifact,
        None,
    );

    assert_eq!(
        automation_node_required_output_path(&node).as_deref(),
        Some(".tandem/artifacts/compare-results.md")
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
    let node = test_flow_node(
        "collect_inputs",
        "brief",
        crate::AutomationOutputValidatorKind::StructuredJson,
        Some(json!({
            "inputs": {
                "topic": "autonomous AI agentic workflows",
                "delivery_email": "recipient@example.com",
                "email_format": "simple html",
                "attachments_allowed": false
            }
        })),
    );

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
    let node = test_flow_node(
        "collect_inputs",
        "structured_json",
        crate::AutomationOutputValidatorKind::StructuredJson,
        Some(json!({
            "builder": {
                "web_research_expected": false
            }
        })),
    );

    assert!(automation_node_required_output_path(&node).is_some());
    assert!(automation_node_inline_artifact_payload(&node).is_none());
}

#[test]
fn eval_nodes_use_explicit_inline_artifact_metadata() {
    let node = test_flow_node(
        "research_node",
        "report",
        crate::AutomationOutputValidatorKind::ResearchBrief,
        Some(json!({
            "eval": {
                "test_id": "ev_inline",
                "inline_artifact": {
                    "status": "completed",
                    "summary": "stubbed eval artifact",
                    "citations": ["https://example.com/source"]
                }
            }
        })),
    );

    let payload = automation_node_inline_artifact_payload(&node).expect("inline artifact");

    assert_eq!(
        payload.get("summary").and_then(serde_json::Value::as_str),
        Some("stubbed eval artifact")
    );
    assert_eq!(
        payload
            .get("citations")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(1)
    );
}

// WRC-03 (TAN-47): automation node runtime failure outcomes — exercise the
// blocked / retry / repair / artifact-recovery decision helpers that drive how a
// node reacts after a failed attempt.

#[test]
fn automation_node_outcome_status_classifiers_distinguish_blocked_and_repair() {
    let blocked = serde_json::json!({ "status": "blocked" });
    assert!(automation_output_is_blocked(&blocked));
    assert!(!automation_output_needs_repair(&blocked));

    let needs_repair = serde_json::json!({ "status": "needs_repair" });
    assert!(automation_output_needs_repair(&needs_repair));
    assert!(!automation_output_is_blocked(&needs_repair));

    let completed = serde_json::json!({ "status": "completed" });
    assert!(!automation_output_is_blocked(&completed));
    assert!(!automation_output_needs_repair(&completed));

    let exhausted = serde_json::json!({
        "status": "needs_repair",
        "artifact_validation": { "repair_exhausted": true }
    });
    assert!(automation_output_repair_exhausted(&exhausted));
    let not_exhausted = serde_json::json!({
        "status": "needs_repair",
        "artifact_validation": { "repair_exhausted": false }
    });
    assert!(!automation_output_repair_exhausted(&not_exhausted));
}

#[test]
fn infer_artifact_repair_state_reports_retry_budget_remaining() {
    // One repair attempt against a five-attempt budget: still retryable.
    let telemetry = serde_json::json!({
        "tool_call_counts": { "write": 2 }
    });
    let (attempt, remaining, exhausted) = infer_artifact_repair_state(
        None,
        true,
        false,
        Some("final artifact needs more upstream synthesis"),
        &telemetry,
        Some(5),
    );
    assert_eq!(attempt, 1);
    assert_eq!(remaining, 4);
    assert!(!exhausted);
}

#[test]
fn infer_artifact_repair_state_marks_exhausted_when_budget_spent() {
    // Repair attempts have consumed the whole budget and the block persists.
    let telemetry = serde_json::json!({
        "tool_call_counts": { "write": 9 }
    });
    let (attempt, remaining, exhausted) = infer_artifact_repair_state(
        None,
        true,
        false,
        Some("final artifact still does not synthesize upstream evidence"),
        &telemetry,
        Some(3),
    );
    assert_eq!(attempt, 3);
    assert_eq!(remaining, 0);
    assert!(exhausted);
}

#[test]
fn infer_artifact_repair_state_marks_exhausted_when_node_attempts_used_up() {
    // Node-attempt telemetry alone can exhaust repair even mid-budget.
    let telemetry = serde_json::json!({
        "node_attempt": 4u32,
        "node_max_attempts": 4u32,
        "tool_call_counts": { "write": 2 }
    });
    let (_attempt, _remaining, exhausted) = infer_artifact_repair_state(
        None,
        true,
        false,
        Some("still blocked"),
        &telemetry,
        Some(5),
    );
    assert!(exhausted);
}

#[test]
fn automation_repair_output_recovery_detects_changed_artifact() {
    // Recovery: a repaired artifact whose text differs from the prior attempt.
    let recovered = (
        ".tandem/runs/run-1/artifacts/report.md".to_string(),
        "# Report\n\nRepaired body grounded in upstream evidence.".to_string(),
    );
    assert!(automation_repair_output_differs_from_preexisting(
        Some("# Report\n\nOriginal blocked body."),
        Some(&recovered),
    ));

    // No prior artifact at all still counts as a recovery from nothing.
    assert!(automation_repair_output_differs_from_preexisting(
        None,
        Some(&recovered),
    ));

    // An unchanged artifact is not a recovery.
    let unchanged = ("p.md".to_string(), "identical body".to_string());
    assert!(!automation_repair_output_differs_from_preexisting(
        Some("identical body"),
        Some(&unchanged),
    ));

    // Nothing accepted means nothing recovered.
    assert!(!automation_repair_output_differs_from_preexisting(
        Some("anything"),
        None,
    ));
}
