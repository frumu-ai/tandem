use super::*;

#[test]
fn research_brief_passes_local_only_when_websearch_is_not_offered() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-research-local-only-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let brief_text = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n## Campaign goal\nClarify positioning.\n\n## Target audience\n- Operators.\n\n## Core pain points\n- Coordination overhead.\n\n## Positioning angle\nTandem centralizes orchestration.\n\n## Competitor context\nLocal-only comparison for this run.\n\n## Proof points with citations\n1. Supported from docs/source.md. Source note: https://example.com/reference\n\n## Likely objections\n- Proof depth.\n\n## Channel considerations\n- Landing page.\n\n## Recommended message hierarchy\n1. Problem\n2. Promise\n\n## Files reviewed\n- docs/source.md\n\n## Files not reviewed\n- docs/extra.md: not needed for this first pass.\n".to_string();
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
    let mut session = Session::new(
        Some("research-local-only".to_string()),
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
                result: Some(json!({"output":"source"})),
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
    let requested_tools = vec!["glob".to_string(), "read".to_string(), "write".to_string()];
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    let (_accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "Done\n\n{\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        Some(("marketing-brief.md".to_string(), brief_text.clone())),
        &std::collections::BTreeSet::new(),
    );

    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        artifact_validation
            .get("external_research_mode")
            .and_then(Value::as_str),
        Some("waived_unavailable")
    );

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn research_brief_passes_when_source_audit_uses_markdown_tables() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-research-table-audit-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("docs")).expect("create workspace");
    let brief_text = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n### Files Reviewed\n| Local Path | Evidence Summary |\n|---|---|\n| `docs/source.md` | Core source reviewed |\n\n### Files Not Reviewed\n| Local Path | Reason |\n|---|---|\n| `docs/extra.md` | Out of scope for this run |\n\n### Web Sources Reviewed\n| URL | Status | Notes |\n|---|---|---|\n| https://example.com | Fetched | Confirmed live |\n\n## Campaign goal\nClarify positioning.\n\n## Target audience\n- Operators.\n\n## Core pain points\n- Coordination overhead.\n\n## Positioning angle\nTandem centralizes orchestration.\n\n## Competitor context\nLocal-only comparison for this run.\n\n## Proof points with citations\n1. Supported from docs/source.md. Source note: https://example.com/reference\n\n## Likely objections\n- Proof depth.\n\n## Channel considerations\n- Landing page.\n\n## Recommended message hierarchy\n1. Problem\n2. Promise\n".to_string();
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
    let mut session = Session::new(
        Some("research-table-audit".to_string()),
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
                result: Some(json!({"output":"source"})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "websearch".to_string(),
                args: json!({"query":"tandem competitor landscape"}),
                result: Some(json!({
                    "output": "Authorization required for `websearch`.",
                    "metadata": { "error": "authorization required" }
                })),
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
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    let (_accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "Done\n\n{\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        Some(("marketing-brief.md".to_string(), brief_text.clone())),
        &std::collections::BTreeSet::new(),
    );

    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        None
    );
    assert_eq!(
        artifact_validation
            .get("web_sources_reviewed_present")
            .and_then(Value::as_bool),
        Some(true)
    );

    let _ = std::fs::remove_dir_all(workspace_root);
}
