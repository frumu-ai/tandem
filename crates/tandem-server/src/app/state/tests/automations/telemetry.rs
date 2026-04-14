use super::*;

#[test]
fn research_required_next_tool_actions_summarize_missing_reads_and_websearch() {
    let requested_tools = vec![
        json!("glob"),
        json!("read"),
        json!("websearch"),
        json!("write"),
    ];
    let executed_tools = vec![json!("glob"), json!("write")];
    let unmet_requirements = vec![
        "no_concrete_reads".to_string(),
        "missing_successful_web_research".to_string(),
        "web_sources_reviewed_missing".to_string(),
        "relevant_files_not_reviewed_or_skipped".to_string(),
    ];
    let unreviewed_relevant_paths = vec![
        "docs/pricing.md".to_string(),
        "docs/customers.md".to_string(),
    ];

    let actions = research_required_next_tool_actions(
        &requested_tools,
        &executed_tools,
        true,
        &unmet_requirements,
        &unreviewed_relevant_paths,
        None,
    );

    assert!(actions
        .iter()
        .any(|value| value.contains("docs/pricing.md, docs/customers.md")));
    assert!(actions
        .iter()
        .any(|value| value.contains("Use `websearch` successfully")));
    assert!(actions
        .iter()
        .any(|value| value.contains("Files not reviewed")));
}

#[test]
fn research_required_next_tool_actions_surface_websearch_authorization() {
    let requested_tools = vec![
        json!("glob"),
        json!("read"),
        json!("websearch"),
        json!("write"),
    ];
    let executed_tools = vec![json!("glob"), json!("websearch")];
    let unmet_requirements = vec![
        "no_concrete_reads".to_string(),
        "missing_successful_web_research".to_string(),
        "web_sources_reviewed_missing".to_string(),
    ];

    let actions = research_required_next_tool_actions(
        &requested_tools,
        &executed_tools,
        true,
        &unmet_requirements,
        &Vec::new(),
        Some("web research authorization required"),
    );

    assert!(actions
        .iter()
        .any(|value| value.contains("Skip `websearch` for this run")));
}

#[test]
fn research_required_next_tool_actions_surface_generic_websearch_unavailability() {
    let requested_tools = vec![
        json!("glob"),
        json!("read"),
        json!("websearch"),
        json!("write"),
    ];
    let executed_tools = vec![json!("glob"), json!("websearch")];
    let unmet_requirements = vec![
        "no_concrete_reads".to_string(),
        "missing_successful_web_research".to_string(),
        "web_sources_reviewed_missing".to_string(),
    ];

    let actions = research_required_next_tool_actions(
        &requested_tools,
        &executed_tools,
        true,
        &unmet_requirements,
        &Vec::new(),
        Some("web research unavailable"),
    );

    assert!(actions
        .iter()
        .any(|value| value.contains("external research is unavailable")));
}

#[test]
fn research_required_next_tool_actions_include_workspace_file_write_guidance() {
    let requested_tools = vec![json!("glob"), json!("read"), json!("write")];
    let executed_tools = vec![];
    let unmet_requirements = vec![
        "current_attempt_output_missing".to_string(),
        "required_workspace_files_missing".to_string(),
    ];

    let actions = research_required_next_tool_actions(
        &requested_tools,
        &executed_tools,
        false,
        &unmet_requirements,
        &Vec::new(),
        None,
    );

    assert!(actions
        .iter()
        .any(|value| value.contains("Write the required run artifact")));
    assert!(actions.iter().any(|value| value.contains(
        "Write the required workspace files approved for this node before ending this attempt."
    )));
}

#[test]
fn summarize_automation_tool_activity_recovers_tools_from_synthetic_summary() {
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
        metadata: None,
    };
    let mut session = Session::new(Some("synthetic summary".to_string()), None);
    session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::Text {
                text: "I completed project analysis steps using tools, but the model returned no final narrative text.\n\nTool result summary:\nTool `glob` result:\n/home/user123/marketing-tandem/marketing-brief.md\nTool `websearch` result:\nAuthorization required for `websearch`.\nThis integration requires authorization before this action can run.\n\nAuthorize here: https://dashboard.exa.ai/api-keys".to_string(),
            }],
        ));

    let telemetry = summarize_automation_tool_activity(
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
        telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["glob", "websearch"])
    );
    assert_eq!(
        telemetry
            .get("workspace_inspection_used")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        telemetry.get("web_research_used").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        telemetry
            .get("latest_web_research_failure")
            .and_then(Value::as_str),
        Some("web research authorization required")
    );
}

#[test]
fn summarize_automation_tool_activity_counts_auth_failed_websearch_as_attempted() {
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
        metadata: None,
    };
    let mut session = Session::new(Some("auth failed websearch".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "websearch".to_string(),
            args: json!({"query":"tandem competitors"}),
            result: None,
            error: Some("Authorization required for `websearch`.".to_string()),
        }],
    ));

    let telemetry = summarize_automation_tool_activity(
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
        telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["websearch"])
    );
    assert_eq!(
        telemetry.get("web_research_used").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        telemetry
            .get("latest_web_research_failure")
            .and_then(Value::as_str),
        Some("web research authorization required")
    );
}

#[test]
fn summarize_automation_tool_activity_treats_backend_unavailable_websearch_as_unavailable() {
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
        metadata: None,
    };
    let mut session = Session::new(Some("backend unavailable websearch".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "websearch".to_string(),
            args: json!({"query":"tandem competitors"}),
            result: Some(json!({
                "output": "Web search is currently unavailable for `websearch`.",
                "metadata": { "error": "backend_unavailable" }
            })),
            error: None,
        }],
    ));

    let telemetry = summarize_automation_tool_activity(
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
        telemetry
            .get("latest_web_research_failure")
            .and_then(Value::as_str),
        Some("web research unavailable")
    );
    assert_eq!(
        telemetry
            .get("web_research_succeeded")
            .and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
fn summarize_automation_tool_activity_treats_partial_websearch_with_results_as_success() {
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
        metadata: None,
    };
    let mut session = Session::new(Some("partial websearch".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "websearch".to_string(),
            args: json!({"query":"autonomous AI agentic workflows 2025"}),
            result: Some(json!({
                "output": serde_json::to_string(&json!({
                    "query": "autonomous AI agentic workflows 2025",
                    "result_count": 2,
                    "partial": true,
                    "results": [
                        {"title": "One", "url": "https://example.com/1"},
                        {"title": "Two", "url": "https://example.com/2"}
                    ]
                })).expect("json output"),
                "metadata": {
                    "count": 2,
                    "error": "rate_limited",
                    "partial": true
                }
            })),
            error: None,
        }],
    ));

    let telemetry = summarize_automation_tool_activity(
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
        telemetry
            .get("web_research_succeeded")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        telemetry
            .get("latest_web_research_failure")
            .and_then(Value::as_str),
        None
    );
}

#[test]
fn summarize_automation_tool_activity_treats_runtime_websearch_string_result_as_success() {
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
        metadata: None,
    };
    let mut session = Session::new(Some("runtime websearch".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "websearch".to_string(),
            args: json!({"query":"autonomous AI agentic workflows 2024 2025"}),
            result: Some(json!(serde_json::to_string_pretty(&json!({
                "attempted_backends": ["brave"],
                "backend": "brave",
                "configured_backend": "brave",
                "partial": false,
                "query": "autonomous AI agentic workflows 2024 2025",
                "result_count": 2,
                "results": [
                    {
                        "title": "AI Agents in 2025: Expectations vs. Reality | IBM",
                        "url": "https://www.ibm.com/think/insights/ai-agents-2025-expectations-vs-reality"
                    },
                    {
                        "title": "Agentic AI strategy | Deloitte Insights",
                        "url": "https://www.deloitte.com/us/en/insights/topics/technology-management/tech-trends/2026/agentic-ai-strategy.html"
                    }
                ]
            }))
            .expect("json output"))),
            error: None,
        }],
    ));

    let telemetry = summarize_automation_tool_activity(
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
        telemetry.get("web_research_used").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        telemetry
            .get("web_research_succeeded")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        telemetry
            .get("latest_web_research_failure")
            .and_then(Value::as_str),
        None
    );
}

#[test]
fn automation_prompt_preflight_marks_warning_for_large_prompt() {
    let prompt = "x".repeat(20_000);
    let preflight = crate::app::state::automation::build_automation_prompt_preflight(
        &prompt,
        &["glob".to_string(), "read".to_string(), "write".to_string()],
        &[json!({
            "name": "write",
            "description": "write a file",
            "input_schema": {"type": "object"}
        })],
        "artifact_write",
        &json!({
            "required_capabilities": ["artifact_write"],
            "resolved": {
                "artifact_write": {
                    "status": "resolved",
                    "offered_tools": ["write"],
                    "available_tools": ["write"]
                }
            },
            "missing_capabilities": []
        }),
        "standard",
        false,
    );

    assert_eq!(
        preflight.get("budget_status").and_then(Value::as_str),
        Some("high")
    );
    assert_eq!(
        preflight.get("degraded_prompt").and_then(Value::as_bool),
        Some(false)
    );
}

#[test]
fn build_automation_attempt_evidence_captures_runtime_websearch_success() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research_sources".to_string(),
        agent_id: "agent-a".to_string(),
        objective: "Research".to_string(),
        depends_on: Vec::new(),
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "collect_inputs".to_string(),
            alias: "research_brief".to_string(),
        }],
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
                "output_path": ".tandem/artifacts/research-sources.json"
            }
        })),
    };
    let mut session = Session::new(Some("research".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![
            MessagePart::ToolInvocation {
                tool: "read".to_string(),
                args: json!({"file_path": ".tandem/artifacts/collect-inputs.json"}),
                result: Some(json!("{\"topic\":\"autonomous AI agentic workflows\"}")),
                error: None,
            },
            MessagePart::ToolInvocation {
                tool: "websearch".to_string(),
                args: json!({"query":"autonomous AI agentic workflows 2024 2025"}),
                result: Some(json!(serde_json::to_string_pretty(&json!({
                    "backend": "brave",
                    "result_count": 2,
                    "partial": false,
                    "results": [
                        {"title": "IBM", "url": "https://example.com/ibm"},
                        {"title": "Deloitte", "url": "https://example.com/deloitte"}
                    ]
                }))
                .expect("json output"))),
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
    let preflight = crate::app::state::automation::build_automation_prompt_preflight(
        "Research prompt",
        &[
            "glob".to_string(),
            "read".to_string(),
            "websearch".to_string(),
            "write".to_string(),
        ],
        &[
            json!({"name":"websearch"}),
            json!({"name":"read"}),
            json!({"name":"write"}),
        ],
        "artifact_write",
        &json!({
            "required_capabilities": ["workspace_read", "workspace_discover", "artifact_write", "web_research"],
            "resolved": {},
            "missing_capabilities": []
        }),
        "standard",
        false,
    );
    let attempt_evidence = build_automation_attempt_evidence(
        &node,
        1,
        &session,
        &session.id,
        ".",
        &tool_telemetry,
        &preflight,
        &json!({
            "required_capabilities": ["web_research"],
            "resolved": {},
            "missing_capabilities": []
        }),
        Some(".tandem/artifacts/research-sources.json"),
        None,
        None,
    );

    let web_status = attempt_evidence
        .get("evidence")
        .and_then(Value::as_object)
        .and_then(|value| value.get("web_research"))
        .and_then(Value::as_object)
        .and_then(|value| value.get("status"))
        .and_then(Value::as_str);
    assert_eq!(web_status, Some("succeeded"));

    let succeeded_tools = attempt_evidence
        .get("tool_execution")
        .and_then(Value::as_object)
        .and_then(|value| value.get("succeeded_tools"))
        .and_then(Value::as_array);
    assert_eq!(
        succeeded_tools
            .is_some_and(|rows| rows.iter().any(|value| value.as_str() == Some("websearch"))),
        true
    );
    assert_eq!(
        attempt_evidence
            .get("requested_output_path")
            .and_then(Value::as_str),
        Some(".tandem/artifacts/research-sources.json")
    );
    assert_eq!(
        attempt_evidence
            .get("transcript_recovery_result")
            .and_then(Value::as_str),
        Some("not_recoverable")
    );
    assert_eq!(
        attempt_evidence
            .get("offered_tools")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["glob", "read", "websearch", "write"])
    );
}

#[test]
fn detect_automation_blocker_category_prefers_delivery_category_from_canonical_evidence() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "notify_user".to_string(),
        agent_id: "agent-committer".to_string(),
        objective: "Send the report by email.".to_string(),
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
                "to": "recipient@example.com",
                "content_type": "text/html",
                "inline_body_only": true,
                "attachments": false
            }
        })),
    };
    let tool_telemetry = json!({
        "executed_tools": ["read"],
        "preflight": {"budget_status": "ok"},
        "attempt_evidence": {
            "capability_resolution": {
                "missing_capabilities": []
            },
            "delivery": {
                "status": "not_attempted"
            }
        }
    });

    assert_eq!(
        detect_automation_blocker_category(
            &node,
            "blocked",
            Some("email delivery to `recipient@example.com` was requested but no email draft/send tool executed"),
            &tool_telemetry,
            None,
        )
        .as_deref(),
        Some("delivery_not_executed")
    );
}

#[test]
fn detect_automation_node_failure_kind_marks_provider_transport_failures() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research_brief".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Research the market and produce a brief.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
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
        detect_automation_node_failure_kind(
            &node,
            "blocked",
            None,
            Some("provider stream connect timeout after 90000 ms"),
            Some(&json!({
                "semantic_block_reason": "provider stream connect timeout after 90000 ms",
                "unmet_requirements": [],
                "verification": {
                    "verification_failed": false
                }
            })),
        )
        .as_deref(),
        Some("provider_transport_failure")
    );
}
