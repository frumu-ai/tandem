use super::*;

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

#[test]
fn brief_with_timed_out_websearch_is_blocked_when_web_research_is_required() {
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
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-websearch-timeout-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace root");
    let _snapshot = std::collections::BTreeSet::new();

    let brief_text = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n## Files reviewed\n- tandem-reference/readmes/repo-README.md\n\n## Web sources reviewed\n- websearch attempt timed out.\n".to_string();
    std::fs::write(workspace_root.join("marketing-brief.md"), &brief_text).expect("seed artifact");

    let mut session = Session::new(
        Some("session-timeout".to_string()),
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
                args: json!({"path":"tandem-reference/readmes/repo-README.md"}),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "websearch".to_string(),
                args: json!({"query":"ai coding agents market"}),
                result: Some(json!({
                    "output": "Search timed out. No results received.",
                    "metadata": { "error": "timeout" }
                })),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": "marketing-brief.md",
                    "content": brief_text
                }),
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
    assert_eq!(
        tool_telemetry
            .get("web_research_used")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        tool_telemetry
            .get("web_research_succeeded")
            .and_then(Value::as_bool),
        Some(false)
    );

    let (accepted_output, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "Done — `marketing-brief.md` was written in the workspace.\n\n{\"status\":\"completed\",\"approved\":true}",
        &tool_telemetry,
        None,
        Some(("marketing-brief.md".to_string(), brief_text.clone())),
        &_snapshot,
    );

    assert!(accepted_output.is_some());
    assert_eq!(
        metadata
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("research completed without citation-backed claims")
    );
    assert_eq!(
        rejected.as_deref(),
        Some("research completed without citation-backed claims")
    );
    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Done — `marketing-brief.md` was written in the workspace.\n\n{\"status\":\"completed\",\"approved\":true}",
            accepted_output.as_ref(),
            &tool_telemetry,
            Some(&metadata),
        );
    assert_eq!(status, "needs_repair");
    assert_eq!(
        reason.as_deref(),
        Some("research completed without citation-backed claims")
    );
    assert_eq!(approved, Some(true));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn brief_prewrite_requirements_follow_external_research_defaults() {
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
    let requirements = automation_node_prewrite_requirements(
        &node,
        &[
            "glob".to_string(),
            "read".to_string(),
            "websearch".to_string(),
            "write".to_string(),
        ],
    )
    .expect("prewrite requirements");
    assert!(requirements.workspace_inspection_required);
    assert!(requirements.web_research_required);
    assert!(!requirements.concrete_read_required);
    assert!(requirements.successful_web_research_required);
    assert!(requirements.repair_on_unmet_requirements);
    assert_eq!(requirements.coverage_mode, PrewriteCoverageMode::None);
}
