
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
