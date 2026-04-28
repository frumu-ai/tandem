use super::*;

#[test]
fn structured_handoff_nodes_fail_when_only_fallback_tool_summary_is_returned() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-structured-handoff-fallback-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-discover-sources".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Discover source corpus".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("local_research".to_string()),
                required_tools: vec!["read".to_string()],
                required_tool_calls: Vec::new(),
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: vec![
                    "workspace_inspection".to_string(),
                    "concrete_reads".to_string(),
                ],
                retry_on_missing: vec![
                    "local_source_reads".to_string(),
                    "workspace_inspection".to_string(),
                    "concrete_reads".to_string(),
                ],
                terminal_on: Vec::new(),
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
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
    let mut session = Session::new(
        Some("structured-handoff-fallback".to_string()),
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
            args: json!({"path":"tandem-reference/SOURCES.md"}),
            result: Some(json!({"output":"# Sources"})),
            error: None,
        }],
    ));
    let requested_tools = vec!["glob".to_string(), "read".to_string()];
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    let (_accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "I completed project analysis steps using tools, but the model returned no final narrative text.\n\nTool result summary:\nTool `read` result:\n# Sources",
        &tool_telemetry,
        None,
        None,
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(
        rejected.as_deref(),
        Some("structured handoff was not returned in the final response")
    );
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("needs_repair")
    );
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("structured_handoff_missing"))));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn structured_handoff_missing_is_repairable_even_without_enforcement_metadata() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-structured-handoff-defaults-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-discover-sources".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Discover source corpus".to_string(),
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
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: None,
    };
    let mut session = Session::new(
        Some("structured-handoff-defaults".to_string()),
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
                tool: "glob".to_string(),
                args: json!({"pattern":"**/*.md"}),
                result: Some(json!({"output":"README.md"})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"path":"tandem-reference/SOURCES.md"}),
                result: Some(json!({"output":"# Sources"})),
                error: None,
            },
        ],
    ));
    let requested_tools = vec!["glob".to_string(), "read".to_string()];
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    let (_accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "I completed project analysis steps using tools, but the model returned no final narrative text.\n\nTool result summary:\nTool `read` result:\n# Sources",
        &tool_telemetry,
        None,
        None,
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(
        rejected.as_deref(),
        Some("structured handoff was not returned in the final response")
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
        Some("handoff_missing")
    );
    assert!(artifact_validation
        .get("required_next_tool_actions")
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| value
            .as_str()
            .is_some_and(|text| text.contains("structured JSON handoff")))));

    let output: Value = wrap_automation_node_output(
        &node,
        &session,
        &requested_tools,
        "sess-structured-handoff-defaults",
        Some("run-structured-handoff-defaults"),
        "I completed project analysis steps using tools, but the model returned no final narrative text.\n\nTool result summary:\nTool `read` result:\n# Sources",
        None,
        Some(artifact_validation),
    );
    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("needs_repair")
    );
    assert_eq!(
        output.get("failure_kind").and_then(Value::as_str),
        Some("structured_handoff_missing")
    );

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn structured_handoff_nodes_require_concrete_reads_without_output_path() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-structured-handoff-reads-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-local-sources".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Read prioritized sources".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("local_research".to_string()),
                required_tools: vec!["read".to_string()],
                required_tool_calls: Vec::new(),
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: vec!["concrete_reads".to_string()],
                retry_on_missing: vec![
                    "local_source_reads".to_string(),
                    "concrete_reads".to_string(),
                ],
                terminal_on: Vec::new(),
                repair_budget: Some(5),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
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
    let session = Session::new(
        Some("structured-handoff-missing-read".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );
    let requested_tools = vec!["read".to_string()];
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    let (_accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "{\"read_paths\":[\"tandem-reference/readmes/repo-README.md\"],\"reviewed_facts\":[\"Tandem is an engine-owned workflow runtime.\"],\"files_reviewed\":[\"tandem-reference/readmes/repo-README.md\"],\"files_not_reviewed\":[],\"citations_local\":[\"tandem-reference/readmes/repo-README.md\"]}\n\n{\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        None,
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(
        rejected.as_deref(),
        Some("structured handoff completed without required concrete file reads")
    );
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("needs_repair")
    );
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("no_concrete_reads"))));
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("concrete_read_required"))));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn structured_handoff_blocks_when_exact_named_source_read_fails_but_copy_is_discovered() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-structured-handoff-exact-source-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    std::fs::write(workspace_root.join("RESUME copy 2.md"), "# Copy\n").expect("write copy");
    let resume_path = workspace_root.join("RESUME.md");
    let copy_path = workspace_root.join("RESUME copy 2.md");
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "assess".to_string(),
        agent_id: "triage".to_string(),
        objective: format!(
            "Analyze the local `{}` file and use it as the source of truth for skills. Never edit, rewrite, rename, move, or delete `{}`. If `resume_overview.md` is missing, create it.",
            resume_path.display(),
            resume_path.display()
        ),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("local_research".to_string()),
                required_tools: vec!["glob".to_string(), "read".to_string()],
                required_tool_calls: Vec::new(),
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: Vec::new(),
                prewrite_gates: vec!["concrete_reads".to_string()],
                retry_on_missing: vec![
                    "local_source_reads".to_string(),
                    "concrete_reads".to_string(),
                ],
                terminal_on: Vec::new(),
                repair_budget: Some(3),
                session_text_recovery: Some("require_prewrite_satisfied".to_string()),
            }),
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
    let mut session = Session::new(
        Some("structured-handoff-exact-source".to_string()),
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
                tool: "glob".to_string(),
                args: json!({"pattern":"*.md"}),
                result: Some(json!({"output": copy_path.display().to_string()})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"path": resume_path.display().to_string()}),
                result: None,
                error: Some("read failed: No such file or directory (os error 2)".to_string()),
            },
        ],
    ));
    let requested_tools = vec!["glob".to_string(), "read".to_string()];
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    let (_accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "{\"structured_handoff\":{\"assessment\":{\"has_resume_md\":false,\"proceed_downstream\":false}},\"status\":\"completed\"}",
        &tool_telemetry,
        None,
        None,
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(
        rejected.as_deref(),
        Some("research completed without reading the exact required source files")
    );
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| value.as_str() == Some("required_source_paths_not_read"))));
    assert_eq!(
        artifact_validation
            .pointer("/validation_basis/missing_required_source_read_paths/0")
            .and_then(Value::as_str),
        Some("RESUME.md")
    );

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn wrap_automation_node_output_includes_parsed_structured_handoff() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-discover-sources".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Discover source corpus".to_string(),
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
    let mut session = Session::new(Some("structured-handoff-wrap".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "read".to_string(),
            args: json!({"path":"tandem-reference/SOURCES.md"}),
            result: Some(json!({"output":"# Sources"})),
            error: None,
        }],
    ));

    let output: Value = wrap_automation_node_output(
        &node,
        &session,
        &["read".to_string()],
        "sess-structured-handoff-wrap",
        Some("run-structured-handoff-wrap"),
        "Structured handoff ready.\n\n```json\n{\"workspace_inventory_summary\":\"Marketing source bundle found\",\"priority_paths\":[\"tandem-reference/SOURCES.md\"],\"discovered_paths\":[\"tandem-reference/SOURCES.md\"],\"skipped_paths_initial\":[]}\n```\n\n{\"status\":\"completed\"}",
        None,
        Some(json!({})),
    );

    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        output
            .get("content")
            .and_then(|value| value.get("structured_handoff"))
            .and_then(|value| value.get("workspace_inventory_summary"))
            .and_then(Value::as_str),
        Some("Marketing source bundle found")
    );
    assert_eq!(
        output
            .get("provenance")
            .and_then(|value| value.get("run_id"))
            .and_then(Value::as_str),
        Some("run-structured-handoff-wrap")
    );
    assert!(output
        .get("content")
        .and_then(|value| value.get("text"))
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("\"priority_paths\"")));
}

#[test]
fn wrap_automation_node_output_attaches_source_material_from_reads() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-structured-handoff-source-material-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    std::fs::write(
        workspace_root.join("RESUME.md"),
        "# Resume\n\nKeep the source text intact.\n",
    )
    .expect("write resume");

    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-discover-sources".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Discover source corpus".to_string(),
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
    let mut session = Session::new(
        Some("structured-handoff-source-material".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );
    session.workspace_root = Some(
        workspace_root
            .to_str()
            .expect("workspace root string")
            .to_string(),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "read".to_string(),
            args: json!({"path":"RESUME.md"}),
            result: Some(json!({"output":"# Resume\n\nKeep the source text intact.\n"})),
            error: None,
        }],
    ));

    let output: Value = wrap_automation_node_output(
        &node,
        &session,
        &["read".to_string()],
        "sess-structured-handoff-source-material",
        Some("run-structured-handoff-source-material"),
        "{\n  \"status\": \"completed\",\n  \"source_summary\": \"resume reviewed\"\n}\n",
        None,
        Some(json!({})),
    );

    let structured_handoff = output
        .get("content")
        .and_then(|value| value.get("structured_handoff"))
        .and_then(Value::as_object)
        .expect("structured handoff");
    let source_material = structured_handoff
        .get("source_material")
        .and_then(Value::as_array)
        .expect("source material");
    assert_eq!(source_material.len(), 1);
    assert_eq!(
        source_material[0].get("path").and_then(Value::as_str),
        Some("RESUME.md")
    );
    assert!(source_material[0]
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("Keep the source text intact.")));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn wrap_automation_node_output_strips_read_only_files_from_write_targets() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-structured-handoff-write-targets-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    std::fs::write(
        workspace_root.join("RESUME.md"),
        "# Resume\n\nKeep the source text intact.\n",
    )
    .expect("write resume");

    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "assess".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Read RESUME.md as the source of truth and keep RESUME.md untouched while creating resume_overview.md and daily_results_2026-04-15.md.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Return a structured triage handoff.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: None,
    };
    let mut session = Session::new(
        Some("structured-handoff-write-targets".to_string()),
        Some(
            workspace_root
                .to_str()
                .expect("workspace root string")
                .to_string(),
        ),
    );
    session.workspace_root = Some(
        workspace_root
            .to_str()
            .expect("workspace root string")
            .to_string(),
    );
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "read".to_string(),
            args: json!({"path":"RESUME.md"}),
            result: Some(json!({"output":"# Resume\n\nKeep the source text intact.\n"})),
            error: None,
        }],
    ));

    let output: Value = wrap_automation_node_output(
        &node,
        &session,
        &["read".to_string()],
        "sess-structured-handoff-write-targets",
        Some("run-structured-handoff-write-targets"),
        "{\n  \"status\": \"completed\",\n  \"workspace_writes_needed\": [\"RESUME.md\", \"resume_overview.md\"],\n  \"actions\": {\n    \"workspace_writes_needed\": [\"RESUME.md\", \"daily_results_2026-04-15.md\"]\n  }\n}\n",
        None,
        Some(json!({})),
    );

    let structured_handoff = output
        .get("content")
        .and_then(|value| value.get("structured_handoff"))
        .and_then(Value::as_object)
        .expect("structured handoff");
    let top_level_writes = structured_handoff
        .get("workspace_writes_needed")
        .and_then(Value::as_array)
        .expect("workspace_writes_needed");
    assert!(!top_level_writes
        .iter()
        .any(|value| value.as_str() == Some("RESUME.md")));
    assert!(top_level_writes
        .iter()
        .any(|value| value.as_str() == Some("resume_overview.md")));

    let nested_writes = structured_handoff
        .get("actions")
        .and_then(Value::as_object)
        .and_then(|actions| actions.get("workspace_writes_needed"))
        .and_then(Value::as_array)
        .expect("nested workspace_writes_needed");
    assert!(!nested_writes
        .iter()
        .any(|value| value.as_str() == Some("RESUME.md")));
    assert!(nested_writes
        .iter()
        .any(|value| value.as_str() == Some("daily_results_2026-04-15.md")));

    let _ = std::fs::remove_dir_all(workspace_root);
}
