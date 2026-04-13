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
fn research_workflow_status_is_needs_repair_before_repair_budget_is_exhausted() {
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
        "requested_tools": ["glob", "read", "websearch", "write"],
        "executed_tools": ["glob", "write"],
    });
    let artifact_validation = json!({
        "semantic_block_reason": "research completed without concrete file reads or required source coverage",
        "unmet_requirements": ["no_concrete_reads", "missing_successful_web_research"],
        "repair_exhausted": false,
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Done — `marketing-brief.md` was written.",
            Some(&(
                "marketing-brief.md".to_string(),
                "# Marketing Brief".to_string(),
            )),
            &tool_telemetry,
            Some(&artifact_validation),
        );

    assert_eq!(status, "needs_repair");
    assert!(matches!(
        reason.as_deref(),
        Some("research completed without concrete file reads or required source coverage")
            | Some("research completed without required current web research")
    ));
    assert_eq!(approved, None);
    let summary = build_automation_validator_summary(
        crate::AutomationOutputValidatorKind::ResearchBrief,
        &status,
        reason.as_deref(),
        Some(&artifact_validation),
    );
    assert_eq!(summary.outcome, "needs_repair");
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
fn research_workflow_status_blocks_after_repair_budget_is_exhausted() {
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
        "requested_tools": ["glob", "read", "websearch", "write"],
        "executed_tools": ["glob", "write"],
    });
    let artifact_validation = json!({
        "semantic_block_reason": "research completed without concrete file reads or required source coverage",
        "unmet_requirements": ["no_concrete_reads", "missing_successful_web_research"],
        "repair_exhausted": true,
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "Done — `marketing-brief.md` was written.",
            Some(&(
                "marketing-brief.md".to_string(),
                "# Marketing Brief".to_string(),
            )),
            &tool_telemetry,
            Some(&artifact_validation),
        );

    assert_eq!(status, "blocked");
    assert_eq!(
        detect_automation_node_failure_kind(
            &node,
            &status,
            approved,
            reason.as_deref(),
            Some(&artifact_validation),
        )
        .as_deref(),
        Some("research_retry_exhausted")
    );
}

#[test]
fn research_workflow_status_ignores_llm_blocked_when_validation_is_repairable() {
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
        "requested_tools": ["glob", "read", "websearch", "write"],
        "executed_tools": ["glob", "write"],
    });
    let artifact_validation = json!({
        "semantic_block_reason": "research completed without concrete file reads or required source coverage",
        "unmet_requirements": ["no_concrete_reads", "missing_successful_web_research"],
        "repair_exhausted": false,
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "The brief is blocked.\n\n{\"status\":\"blocked\",\"reason\":\"tools unavailable\"}",
            Some(&(
                "marketing-brief.md".to_string(),
                "# Marketing Brief".to_string(),
            )),
            &tool_telemetry,
            Some(&artifact_validation),
        );

    assert_eq!(status, "needs_repair");
    assert!(matches!(
        reason.as_deref(),
        Some("research completed without concrete file reads or required source coverage")
            | Some("research completed without required current web research")
    ));
    assert_eq!(approved, None);
}

#[test]
fn research_workflow_status_keeps_blocked_when_repair_is_exhausted_even_if_llm_declares_blocked() {
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
        "requested_tools": ["glob", "read", "websearch", "write"],
        "executed_tools": ["glob", "write"],
    });
    let artifact_validation = json!({
        "semantic_block_reason": "research completed without concrete file reads or required source coverage",
        "unmet_requirements": ["no_concrete_reads", "missing_successful_web_research"],
        "repair_exhausted": true,
    });

    let (status, reason, approved): (String, Option<String>, Option<bool>) =
        detect_automation_node_status(
            &node,
            "The brief is blocked.\n\n{\"status\":\"blocked\",\"reason\":\"tools unavailable\"}",
            Some(&(
                "marketing-brief.md".to_string(),
                "# Marketing Brief".to_string(),
            )),
            &tool_telemetry,
            Some(&artifact_validation),
        );

    assert_eq!(status, "blocked");
    assert_eq!(reason.as_deref(), Some("tools unavailable"));
    assert_eq!(approved, None);
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
