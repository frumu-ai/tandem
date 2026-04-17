#[test]
fn email_delivery_nodes_without_email_tools_report_tool_unavailable_with_diagnostics() {
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

    let tool_telemetry = json!({
        "requested_tools": ["glob", "read"],
        "executed_tools": ["read"],
        "tool_call_counts": {"read": 1},
        "workspace_inspection_used": true,
        "email_delivery_attempted": false,
        "email_delivery_succeeded": false,
        "latest_email_delivery_failure": null,
        "capability_resolution": {
            "required_capabilities": ["workspace_read", "email_send", "email_draft"],
            "missing_capabilities": ["email_send", "email_draft"],
            "email_tool_diagnostics": {
                "available_tools": [],
                "offered_tools": [],
                "available_send_tools": [],
                "offered_send_tools": [],
                "available_draft_tools": [],
                "offered_draft_tools": [],
                "selected_servers": ["composio-1"],
                "remote_tools": ["mcp.composio_1.send_message"],
                "registered_tools": ["mcp.composio_1.send_message"]
            },
            "mcp_tool_diagnostics": {
                "selected_servers": ["composio-1"],
                "servers": [{
                    "name": "composio-1",
                    "connected": true,
                    "remote_tools": ["mcp.composio_1.send_message"],
                    "registered_tools": ["mcp.composio_1.send_message"]
                }],
                "remote_tools": ["mcp.composio_1.send_message"],
                "registered_tools": ["mcp.composio_1.send_message"],
                "remote_email_like_tools": [],
                "registered_email_like_tools": []
            }
        },
        "attempt_evidence": {
            "delivery": {
                "status": "not_attempted"
            }
        }
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "I could not verify that an email was sent in this run.",
            None,
            &tool_telemetry,
            None,
        );

    assert_eq!(status, "blocked");
    assert!(reason
        .as_deref()
        .is_some_and(|value| value.contains("Discovered email-like tools: none")));
    assert!(reason
        .as_deref()
        .is_some_and(|value| value.contains("Selected MCP servers: composio-1")));
    assert!(reason
        .as_deref()
        .is_some_and(|value| value
            .contains("Remote MCP tools on selected servers: mcp.composio_1.send_message")));
    assert!(reason.as_deref().is_some_and(|value| value.contains(
        "Registered tool-registry tools on selected servers: mcp.composio_1.send_message"
    )));
    assert_eq!(approved, None);
    assert_eq!(
        detect_automation_blocker_category(
            &node,
            &status,
            reason.as_deref(),
            &tool_telemetry,
            None,
        )
        .as_deref(),
        Some("tool_unavailable")
    );
}

#[test]
fn email_delivery_nodes_complete_after_email_tool_execution() {
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
            "Sent the report.\n\n{\"status\":\"completed\",\"approved\":true}",
            None,
            &json!({
                "requested_tools": ["*"],
                "executed_tools": ["read", "mcp.composio_1.gmail_send_email"],
                "tool_call_counts": {"read": 1, "mcp.composio_1.gmail_send_email": 1},
                "workspace_inspection_used": true,
                "email_delivery_attempted": true,
                "email_delivery_succeeded": true,
                "latest_email_delivery_failure": null
            }),
            None,
        );

    assert_eq!(status, "completed");
    assert_eq!(reason, None);
    assert_eq!(approved, Some(true));
}

#[test]
fn infer_selected_mcp_servers_does_not_select_any_servers_for_wildcard_allowlist() {
    let selected = crate::app::state::automation::automation_infer_selected_mcp_servers(
        &[],
        &["*".to_string()],
        &["gmail-main".to_string(), "slack-main".to_string()],
        false,
    );

    assert!(selected.is_empty());
}

#[test]
fn infer_selected_mcp_servers_uses_enabled_servers_for_email_delivery_fallback() {
    let selected = crate::app::state::automation::automation_infer_selected_mcp_servers(
        &[],
        &["glob".to_string(), "read".to_string()],
        &["gmail-main".to_string()],
        true,
    );

    assert_eq!(selected, vec!["gmail-main".to_string()]);
}

#[test]
fn infer_selected_mcp_servers_prefers_explicit_selection_when_present() {
    let selected = crate::app::state::automation::automation_infer_selected_mcp_servers(
        &["composio-1".to_string()],
        &["*".to_string()],
        &["gmail-main".to_string(), "composio-1".to_string()],
        true,
    );

    assert_eq!(selected, vec!["composio-1".to_string()]);
}

#[test]
fn session_read_paths_accepts_json_string_tool_args() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-session-read-paths-json-string-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("src")).expect("create workspace");
    std::fs::write(workspace_root.join("src/lib.rs"), "pub fn demo() {}\n").expect("seed file");

    let mut session = Session::new(
        Some("json string read args".to_string()),
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
            args: json!("{\"path\":\"src/lib.rs\"}"),
            result: Some(json!({"ok": true})),
            error: None,
        }],
    ));

    let paths = session_read_paths(
        &session,
        workspace_root.to_str().expect("workspace root string"),
    );

    assert_eq!(paths, vec!["src/lib.rs".to_string()]);
}

#[test]
fn session_write_candidates_accepts_json_string_tool_args() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-session-write-candidates-json-string-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let mut session = Session::new(
        Some("json string write args".to_string()),
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
            args: json!("{\"path\":\"brief.md\",\"content\":\"Draft body\"}"),
            result: Some(json!({"ok": true})),
            error: None,
        }],
    ));

    let candidates = session_write_candidates_for_output(
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "brief.md",
        None,
        None,
    );

    assert_eq!(candidates, vec!["Draft body".to_string()]);
}

#[test]
fn session_write_touched_output_detects_target_path_without_content() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-session-write-touched-output-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let mut session = Session::new(
        Some("write touched output".to_string()),
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
                "output_path": "brief.md"
            }),
            result: Some(json!({"ok": true})),
            error: None,
        }],
    ));

    let touched = session_write_touched_output_for_output(
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "brief.md",
        None,
        None,
    );

    assert!(
        touched,
        "write invocation should count as touching declared output path"
    );
}

#[test]
fn session_file_mutation_summary_accepts_json_string_tool_args() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-session-mutation-summary-json-string-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("src")).expect("create workspace");

    let mut session = Session::new(
        Some("json string mutation args".to_string()),
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
                    tool: "write".to_string(),
                    args: json!("{\"path\":\"src/lib.rs\",\"content\":\"pub fn demo() {}\\n\"}"),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
                MessagePart::ToolInvocation {
                    tool: "apply_patch".to_string(),
                    args: json!("{\"patchText\":\"*** Begin Patch\\n*** Update File: src/other.rs\\n@@\\n-old\\n+new\\n*** End Patch\\n\"}"),
                    result: Some(json!({"ok": true})),
                    error: None,
                },
            ],
        ));

    let summary = session_file_mutation_summary(
        &session,
        workspace_root.to_str().expect("workspace root string"),
    );

    assert_eq!(
        summary
            .get("touched_files")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        vec![json!("src/lib.rs"), json!("src/other.rs")]
    );
    assert_eq!(
        summary
            .get("mutation_tool_by_file")
            .and_then(|value| value.get("src/lib.rs"))
            .cloned(),
        Some(json!(["write"]))
    );
    assert_eq!(
        summary
            .get("mutation_tool_by_file")
            .and_then(|value| value.get("src/other.rs"))
            .cloned(),
        Some(json!(["apply_patch"]))
    );
}

#[test]
fn code_workflow_rejects_unsafe_raw_source_rewrites() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-automation-unsafe-write-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("src")).expect("create workspace");
    std::fs::write(workspace_root.join("src/lib.rs"), "pub fn before() {}\n").expect("seed source");
    let snapshot = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let long_handoff = format!(
        "# Handoff\n\n{}\n",
        "Detailed implementation summary. ".repeat(20)
    );
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
                "output_path": "handoff.md"
            }
        })),
    };
    let mut session = Session::new(
        Some("unsafe raw write".to_string()),
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
                tool: "write".to_string(),
                args: json!({
                    "path": "src/lib.rs",
                    "content": "pub fn after() {}\n"
                }),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": "handoff.md",
                    "content": long_handoff
                }),
                result: Some(json!({"ok": true})),
                error: None,
            },
        ],
    ));

    let (_, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "",
        &json!({
            "requested_tools": ["read", "write"],
            "executed_tools": ["write"]
        }),
        None,
        Some(("handoff.md".to_string(), long_handoff)),
        &snapshot,
    );

    assert_eq!(
        rejected.as_deref(),
        Some("unsafe raw source rewrite rejected: src/lib.rs")
    );
    assert_eq!(
        metadata
            .get("rejected_artifact_reason")
            .and_then(Value::as_str),
        Some("unsafe raw source rewrite rejected: src/lib.rs")
    );

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn research_finalize_prompt_includes_upstream_coverage_summary() {
    let automation = AutomationV2Spec {
        automation_id: "automation-research-summary".to_string(),
        name: "Research Summary".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
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
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research-brief".to_string(),
        agent_id: "research".to_string(),
        objective: "Write marketing brief".to_string(),
        depends_on: vec![
            "research-discover-sources".to_string(),
            "research-local-sources".to_string(),
            "research-external-research".to_string(),
        ],
        input_refs: vec![
            AutomationFlowInputRef {
                from_step_id: "research-discover-sources".to_string(),
                alias: "source_inventory".to_string(),
            },
            AutomationFlowInputRef {
                from_step_id: "research-local-sources".to_string(),
                alias: "local_source_notes".to_string(),
            },
            AutomationFlowInputRef {
                from_step_id: "research-external-research".to_string(),
                alias: "external_research".to_string(),
            },
        ],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "brief".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
            enforcement: None,
            schema: None,
            summary_guidance: Some("Write `marketing-brief.md`.".to_string()),
        }),
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "title": "Research Brief",
                "role": "watcher",
                "output_path": "marketing-brief.md",
                "research_stage": "research_finalize",
                "prompt": "Finalize the brief."
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "research".to_string(),
        template_id: None,
        display_name: "Research".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["glob".to_string(), "read".to_string(), "write".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
        },
        approval_policy: None,
    };
    let upstream_inputs = vec![
        json!({
            "alias": "source_inventory",
            "from_step_id": "research-discover-sources",
            "output": {
                "content": {
                    "structured_handoff": {
                        "discovered_paths": [
                            {"path": "tandem-reference/SOURCES.md", "type": "file"},
                            {"path": "tandem/implementation_plan.md", "type": "file"}
                        ],
                        "priority_paths": [
                            {"path": "tandem-reference/SOURCES.md", "priority": 1},
                            {"path": "tandem/implementation_plan.md", "priority": 2}
                        ]
                    }
                }
            }
        }),
        json!({
            "alias": "local_source_notes",
            "from_step_id": "research-local-sources",
            "output": {
                "content": {
                    "structured_handoff": {
                        "files_reviewed": ["tandem-reference/SOURCES.md"],
                        "files_not_reviewed": [
                            {"path": "tandem/implementation_plan.md", "reason": "deferred"}
                        ]
                    }
                }
            }
        }),
        json!({
            "alias": "external_research",
            "from_step_id": "research-external-research",
            "output": {
                "content": {
                    "structured_handoff": {
                        "sources_reviewed": [
                            {"url": "https://example.com/reference"}
                        ]
                    }
                }
            }
        }),
    ];

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-research-summary",
        &node,
        1,
        &agent,
        &upstream_inputs,
        &["glob".to_string(), "read".to_string(), "write".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Research Coverage Summary:"));
    assert!(prompt.contains("`tandem-reference/SOURCES.md`"));
    assert!(prompt.contains("`tandem/implementation_plan.md`"));
    assert!(prompt.contains("`Files reviewed` or `Files not reviewed`"));
    assert!(prompt.contains("citation-backed"));
}

#[test]
fn data_json_rewrite_is_not_treated_as_unsafe_source_rewrite() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-automation-json-ledger-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("tracker/search-ledger"))
        .expect("create workspace");
    std::fs::write(
        workspace_root.join("tracker/search-ledger/2026-04-07.json"),
        "{\n  \"searches\": []\n}\n",
    )
    .expect("seed ledger");
    let snapshot = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let handoff = "# Job scout summary\n\nUpdated tracker and recap.\n".to_string();
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "execute_goal".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Update job scout artifacts".to_string(),
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
    let mut session = Session::new(
        Some("json ledger rewrite".to_string()),
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
                tool: "write".to_string(),
                args: json!({
                    "path": "tracker/search-ledger/2026-04-07.json",
                    "content": "{\n  \"status\": \"completed\"\n}\n"
                }),
                result: Some(json!({"ok": true})),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": "handoff.md",
                    "content": handoff
                }),
                result: Some(json!({"ok": true})),
                error: None,
            },
        ],
    ));

    let (_, metadata, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "",
        &json!({
            "requested_tools": ["read", "write"],
            "executed_tools": ["write"]
        }),
        None,
        Some(("handoff.md".to_string(), handoff)),
        &snapshot,
    );

    assert_eq!(rejected, None);
    assert!(metadata
        .get("rejected_artifact_reason")
        .and_then(Value::as_str)
        .is_none());

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn artifact_validation_restores_substantive_session_write_over_short_completion_note() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-automation-restore-write-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let substantive = format!(
        "# Marketing Brief\n\n## Workspace source audit\n{}\n",
        "Real sourced marketing brief content. ".repeat(40)
    );
    std::fs::write(
        workspace_root.join("marketing-brief.md"),
        "Marketing brief completed and written to marketing-brief.md.\n",
    )
    .expect("seed placeholder");
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
    let mut session = Session::new(
        Some("restore substantive write".to_string()),
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
                    "content": "Marketing brief completed and written to marketing-brief.md."
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
        "Done — `marketing-brief.md` was written in the workspace.\n\n{\"status\":\"completed\",\"approved\":true}",
        &json!({
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": ["glob", "websearch", "write"],
            "workspace_inspection_used": true,
            "web_research_used": true
        }),
        None,
        Some((
            "marketing-brief.md".to_string(),
            "Marketing brief completed and written to marketing-brief.md.".to_string(),
        )),
        &snapshot,
    );

    assert!(matches!(
        rejected.as_deref(),
        Some("research completed without concrete file reads or required source coverage")
            | Some("research completed without required current web research")
    ));
    assert_eq!(
        metadata
            .get("recovered_from_session_write")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        metadata
            .get("validation_basis")
            .and_then(Value::as_object)
            .and_then(|value| value.get("authority"))
            .and_then(Value::as_str),
        Some("filesystem_and_receipts")
    );
    assert_eq!(
        metadata
            .get("validation_basis")
            .and_then(Value::as_object)
            .and_then(|value| value.get("current_attempt_output_materialized"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        accepted_output.as_ref().map(|(_, text)| text.as_str()),
        Some("Marketing brief completed and written to marketing-brief.md.")
    );
    let disk_text = std::fs::read_to_string(workspace_root.join("marketing-brief.md"))
        .expect("read restored file");
    assert_eq!(
        disk_text.trim(),
        "Marketing brief completed and written to marketing-brief.md."
    );
    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
        &node,
        "Done — `marketing-brief.md` was written in the workspace.\n\n{\"status\":\"completed\",\"approved\":true}",
        accepted_output.as_ref(),
        &json!({
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": ["glob", "websearch", "write"],
            "workspace_inspection_used": true,
            "web_research_used": true
        }),
        Some(&metadata),
    );
    assert_eq!(status, "needs_repair");
    assert!(matches!(
        reason.as_deref(),
        Some("research completed without concrete file reads or required source coverage")
            | Some("research completed without required current web research")
    ));
    assert_eq!(approved, Some(true));

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn artifact_validation_blocks_session_text_recovery_until_prewrite_is_satisfied() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-automation-block-session-recovery-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let placeholder = "Marketing brief completed and written to marketing-brief.md.\n";
    let substantive = format!(
        "# Marketing Brief\n\n## Workspace source audit\n{}\n\n## Files reviewed\n- docs/source.md\n\n## Web sources reviewed\n- https://example.com\n",
        "Unsafely recovered brief content. ".repeat(30)
    );
    std::fs::write(workspace_root.join("marketing-brief.md"), placeholder)
        .expect("seed placeholder");
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
        Some("blocked recovery".to_string()),
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
        &substantive,
        &json!({
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": [],
            "workspace_inspection_used": false,
            "web_research_used": false,
            "web_research_succeeded": false
        }),
        Some(&substantive),
        Some(("marketing-brief.md".to_string(), placeholder.to_string())),
        &snapshot,
    );

    assert_eq!(
        accepted_output.as_ref().map(|(_, text)| text.as_str()),
        None
    );
    assert_eq!(
        rejected.as_deref(),
        Some("research completed without concrete file reads or required source coverage")
    );
    assert_eq!(
        metadata
            .get("recovered_from_session_write")
            .and_then(Value::as_bool),
        Some(false)
    );
    let disk_text = std::fs::read_to_string(workspace_root.join("marketing-brief.md"))
        .expect("read placeholder");
    assert_eq!(disk_text, placeholder);

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn research_validation_does_not_accept_preexisting_output_without_current_attempt_activity() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-automation-preexisting-research-block-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let stale_preexisting = format!(
        "# Marketing Brief\n\n## Workspace source audit\n{}\n\n## Campaign Goal\nCarry over stale content.\n\n## Files Reviewed\nNone\n\n## Files Not Reviewed\nAll\n\n## Web Sources Reviewed\nNone\n",
        "Stale brief content from an earlier failed run. ".repeat(30)
    );
    let current_disk_output = "# Marketing Brief\n\nAttempt wrote nothing new.\n".to_string();
    std::fs::write(
        workspace_root.join("marketing-brief.md"),
        &current_disk_output,
    )
    .expect("seed output");
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
        Some("empty attempt".to_string()),
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
        "I completed project analysis steps using tools, but the model returned no final narrative text.",
        &json!({
            "requested_tools": ["glob", "read", "websearch", "write"],
            "executed_tools": [],
            "workspace_inspection_used": false,
            "web_research_used": false,
            "web_research_succeeded": false
        }),
        Some(&stale_preexisting),
        Some((
            "marketing-brief.md".to_string(),
            current_disk_output.clone(),
        )),
        &snapshot,
    );

    assert!(accepted_output.is_none());
    assert_eq!(
        metadata
            .get("accepted_candidate_source")
            .and_then(Value::as_str),
        Some("current_attempt_missing_output_write")
    );
    assert_eq!(
        rejected.as_deref(),
        Some("required output `marketing-brief.md` was not created in the current attempt")
    );
    assert_eq!(
        metadata
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("required output was not created in the current attempt")
    );

    let disk_text = std::fs::read_to_string(workspace_root.join("marketing-brief.md"))
        .expect("read unchanged output");
    assert_eq!(disk_text, current_disk_output);

    let _ = std::fs::remove_dir_all(workspace_root);
}

#[test]
fn generic_artifact_validation_rejects_stale_preexisting_output_without_current_session_write() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-stale-generic-artifact-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let stale_preexisting =
        "# Report\n\n## Summary\n\nOld generic content.\n\nParagraph two.\n".to_string();
    std::fs::write(workspace_root.join("report.md"), &stale_preexisting)
        .expect("seed stale output");
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
    let mut session = Session::new(
        Some("generate-report-stale".to_string()),
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
            args: json!({
                "path": "input.md"
            }),
            result: Some(json!("source material")),
            error: None,
        }],
    ));

    let (accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        "Completed the report.",
        &json!({
            "requested_tools": ["read", "write"],
            "executed_tools": ["read"],
            "tool_call_counts": {
                "read": 1
            }
        }),
        Some(&stale_preexisting),
        Some(("report.md".to_string(), stale_preexisting.clone())),
        &snapshot,
    );

    assert!(accepted_output.is_none());
    assert_eq!(
        artifact_validation
            .get("accepted_candidate_source")
            .and_then(Value::as_str),
        Some("current_attempt_missing_output_write")
    );
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        rejected.as_deref(),
        Some("required output `report.md` was not created in the current attempt")
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("required output was not created in the current attempt")
    );

    let disk_text =
        std::fs::read_to_string(workspace_root.join("report.md")).expect("read stale output");
    assert_eq!(disk_text, stale_preexisting);

    let _ = std::fs::remove_dir_all(workspace_root);
}

