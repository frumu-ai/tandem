
#[test]
fn marketing_template_automation_migrates_to_split_research_flow() {
    let mut automation = AutomationV2Spec {
        automation_id: "automation-v2-test".to_string(),
        name: "Marketing Content Pipeline".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![
            AutomationAgentProfile {
                agent_id: "research".to_string(),
                template_id: None,
                display_name: "Research".to_string(),
                avatar_url: None,
                model_policy: None,
                skills: vec!["analysis".to_string()],
                tool_policy: AutomationAgentToolPolicy {
                    allowlist: vec![
                        "read".to_string(),
                        "write".to_string(),
                        "websearch".to_string(),
                        "glob".to_string(),
                    ],
                    denylist: Vec::new(),
                },
                mcp_policy: AutomationAgentMcpPolicy {
                    allowed_servers: Vec::new(),
                    allowed_tools: None,
                },
                approval_policy: None,
            },
            AutomationAgentProfile {
                agent_id: "copywriter".to_string(),
                template_id: None,
                display_name: "Copywriter".to_string(),
                avatar_url: None,
                model_policy: None,
                skills: Vec::new(),
                tool_policy: AutomationAgentToolPolicy {
                    allowlist: vec!["read".to_string(), "write".to_string()],
                    denylist: Vec::new(),
                },
                mcp_policy: AutomationAgentMcpPolicy {
                    allowed_servers: Vec::new(),
                    allowed_tools: None,
                },
                approval_policy: None,
            },
        ],
        flow: AutomationFlowSpec {
            nodes: vec![
                AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "research-brief".to_string(),
                    agent_id: "research".to_string(),
                    objective: "Legacy research".to_string(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: Some(AutomationFlowOutputContract {
                        kind: "brief".to_string(),
                        validator: None,
                        enforcement: None,
                        schema: None,
                        summary_guidance: Some("Write `marketing-brief.md`.".to_string()),
                    }),
                    retry_policy: None,
                    timeout_ms: None,
                    max_tool_calls: None,
                    stage_kind: None,
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "title": "Research Brief",
                            "role": "watcher",
                            "output_path": "marketing-brief.md",
                            "prompt": "Legacy one-shot research prompt"
                        },
                        "studio": {
                            "output_path": "marketing-brief.md"
                        }
                    })),
                },
                AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "draft-copy".to_string(),
                    agent_id: "copywriter".to_string(),
                    objective: "Draft copy".to_string(),
                    depends_on: vec!["research-brief".to_string()],
                    input_refs: vec![AutomationFlowInputRef {
                        from_step_id: "research-brief".to_string(),
                        alias: "marketing_brief".to_string(),
                    }],
                    output_contract: Some(AutomationFlowOutputContract {
                        kind: "draft".to_string(),
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
                },
            ],
        },
        execution: AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp/workspace".to_string()),
        metadata: Some(json!({
            "studio": {
                "template_id": "marketing-content-pipeline",
                "version": 1,
                "agent_drafts": [{"agentId":"research"}],
                "node_drafts": [{"nodeId":"research-brief"}]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };

    assert!(migrate_bundled_studio_research_split_automation(
        &mut automation
    ));
    assert!(automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == "research-discover-sources"));
    assert!(automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == "research-local-sources"));
    assert!(automation
        .flow
        .nodes
        .iter()
        .any(|node| node.node_id == "research-external-research"));
    let discover_node = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "research-discover-sources")
        .expect("discover node present");
    let discover_enforcement = discover_node
        .output_contract
        .as_ref()
        .and_then(|contract| contract.enforcement.as_ref())
        .expect("discover enforcement");
    assert!(discover_enforcement
        .required_tools
        .iter()
        .any(|tool| tool == "read"));
    assert!(discover_enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "workspace_inspection"));
    assert!(discover_enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "concrete_reads"));
    let final_node = automation
        .flow
        .nodes
        .iter()
        .find(|node| node.node_id == "research-brief")
        .expect("final node preserved");
    assert_eq!(
        automation_node_research_stage(final_node).as_deref(),
        Some("research_finalize")
    );
    assert_eq!(final_node.depends_on.len(), 3);
    assert!(automation
        .agents
        .iter()
        .any(|agent| agent.agent_id == "research-discover"));
    assert!(automation
        .agents
        .iter()
        .any(|agent| agent.agent_id == "research-local-sources"));
    assert!(automation
        .agents
        .iter()
        .any(|agent| agent.agent_id == "research-external"));
    let studio = automation
        .metadata
        .as_ref()
        .and_then(|value| value.get("studio"))
        .and_then(Value::as_object)
        .expect("studio metadata");
    assert_eq!(studio.get("version").and_then(Value::as_u64), Some(2));
    assert_eq!(
        studio
            .get("workflow_structure_version")
            .and_then(Value::as_u64),
        Some(2)
    );
    assert!(!studio.contains_key("agent_drafts"));
    assert!(!studio.contains_key("node_drafts"));
}

#[test]
fn research_finalize_validation_accepts_upstream_read_evidence() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-research-finalize-test-{}", now_ms()));
    std::fs::create_dir_all(workspace_root.join("inputs")).expect("create workspace");
    std::fs::write(workspace_root.join("inputs/questions.md"), "Question")
        .expect("seed input file");

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
            enforcement: Some(crate::AutomationOutputEnforcement {
                validation_profile: Some("research_synthesis".to_string()),
                required_tools: Vec::new(),
                required_evidence: vec!["local_source_reads".to_string()],
                required_sections: vec![
                    "files_reviewed".to_string(),
                    "files_not_reviewed".to_string(),
                    "citations".to_string(),
                ],
                prewrite_gates: Vec::new(),
                retry_on_missing: vec![
                    "local_source_reads".to_string(),
                    "files_reviewed".to_string(),
                    "files_not_reviewed".to_string(),
                    "citations".to_string(),
                ],
                terminal_on: Vec::new(),
                repair_budget: Some(5),
                session_text_recovery: None,
            }),
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
                "output_path": "marketing-brief.md",
                "research_stage": "research_finalize",
                "title": "Research Brief",
                "role": "watcher"
            }
        })),
    };

    let mut session = Session::new(Some("research finalize".to_string()), None);
    session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path":"marketing-brief.md",
                    "content":"# Marketing Brief\n\n## Files reviewed\n- inputs/questions.md\n\n## Files not reviewed\n- inputs/extra.md: not needed for this test.\n\n## Proof Points With Citations\n1. Supported claim. Source note: https://example.com/reference\n"
                }),
                result: Some(json!({"output":"written"})),
                error: None,
            }],
        ));

    let tool_telemetry = summarize_automation_tool_activity(
        &node,
        &session,
        &["read".to_string(), "write".to_string()],
    );
    let upstream_evidence = AutomationUpstreamEvidence {
        read_paths: vec!["inputs/questions.md".to_string()],
        discovered_relevant_paths: vec!["inputs/questions.md".to_string()],
        web_research_attempted: false,
        web_research_succeeded: false,
        citation_count: 0,
        citations: vec![],
    };
    let (accepted_output, artifact_validation, rejected) =
            validate_automation_artifact_output_with_upstream(
                &node,
                &session,
                workspace_root.to_str().expect("workspace root"),
                None,
                "",
                &tool_telemetry,
                None,
                Some((
                    "marketing-brief.md".to_string(),
                    "# Marketing Brief\n\n## Files reviewed\n- inputs/questions.md\n\n## Files not reviewed\n- inputs/extra.md: not needed for this test.\n\n## Proof Points With Citations\n1. Supported claim. Source note: https://example.com/reference\n".to_string(),
                )),
                &std::collections::BTreeSet::new(),
                Some(&upstream_evidence),
            );

    assert!(accepted_output.is_some(), "{artifact_validation:?}");
    assert!(
        rejected.is_none(),
        "rejected={rejected:?} metadata={artifact_validation:?}"
    );
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        artifact_validation
            .get("upstream_evidence_applied")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        artifact_validation
            .get("read_paths")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        vec![json!("inputs/questions.md")]
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validator_summary_tracks_verification_and_repair_state() {
    let artifact_validation = json!({
        "accepted_candidate_source": "session_write",
        "repair_attempted": true,
        "repair_succeeded": true,
        "validation_basis": {
            "authority": "filesystem_and_receipts",
            "current_attempt_output_materialized": true
        },
        "verification": {
            "verification_outcome": "passed"
        }
    });

    let summary = build_automation_validator_summary(
        crate::AutomationOutputValidatorKind::CodePatch,
        "done",
        None,
        Some(&artifact_validation),
    );

    assert_eq!(
        summary.kind,
        crate::AutomationOutputValidatorKind::CodePatch
    );
    assert_eq!(summary.outcome, "passed");
    assert_eq!(
        summary.accepted_candidate_source.as_deref(),
        Some("session_write")
    );
    assert_eq!(summary.verification_outcome.as_deref(), Some("passed"));
    assert_eq!(
        summary
            .validation_basis
            .as_ref()
            .and_then(|value| value.get("authority"))
            .and_then(Value::as_str),
        Some("filesystem_and_receipts")
    );
    assert!(summary.repair_attempted);
    assert!(summary.repair_succeeded);
}

#[test]
fn generic_artifact_validation_rejects_stale_verified_output_on_retry() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-stale-verified-output-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("workspace dir");
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research_sources".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Collect sources".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "citations".to_string(),
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
                "output_path": ".tandem/artifacts/research-sources.json"
            }
        })),
    };

    let session = Session::new(Some("retry without touching output".to_string()), None);
    let mut tool_telemetry =
        summarize_automation_tool_activity(&node, &session, &["write".to_string()]);
    tool_telemetry
        .as_object_mut()
        .expect("tool telemetry object")
        .insert(
            "verified_output_materialized_by_current_attempt".to_string(),
            json!(false),
        );

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            Some("run-stale"),
            "",
            &tool_telemetry,
            None,
            Some((
                ".tandem/runs/run-stale/artifacts/research-sources.json".to_string(),
                "{\"status\":\"completed\"}".to_string(),
            )),
            &std::collections::BTreeSet::new(),
            None,
        );

    assert!(accepted_output.is_none(), "{artifact_validation:?}");
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("needs_repair")
    );
    assert_eq!(
        artifact_validation
            .get("accepted_candidate_source")
            .and_then(Value::as_str),
        Some("current_attempt_missing_output_write")
    );
    assert_eq!(
        artifact_validation
            .get("validation_basis")
            .and_then(|value| value.get("current_attempt_output_materialized"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        rejected.as_deref(),
        Some(
            "required output `.tandem/runs/run-stale/artifacts/research-sources.json` was not created in the current attempt"
        )
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn mcp_grounding_citations_accept_verified_output_without_local_read_gates() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-mcp-grounding-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("workspace dir");
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research_sources".to_string(),
        agent_id: "researcher".to_string(),
        objective:
            "Use tandem-mcp first to study Tandem's supported product truths and save grounded notes."
                .to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "citations".to_string(),
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
                "output_path": ".tandem/artifacts/research-sources.json",
                "preferred_mcp_servers": ["tandem-mcp"],
                "web_research_expected": false
            }
        })),
    };

    let mut session = Session::new(Some("mcp grounding".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "mcp_list".to_string(),
            args: json!({}),
            result: Some(json!({
                "servers": [
                    {
                        "name": "tandem-mcp"
                    }
                ]
            })),
            error: None,
        }],
    ));
    let mut tool_telemetry =
        summarize_automation_tool_activity(&node, &session, &["write".to_string()]);
    tool_telemetry
        .as_object_mut()
        .expect("tool telemetry object")
        .insert(
            "verified_output_materialized_by_current_attempt".to_string(),
            json!(true),
        );

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            Some("run-mcp-grounding"),
            "{\"status\":\"completed\"}",
            &tool_telemetry,
            None,
            Some((
                ".tandem/runs/run-mcp-grounding/artifacts/research-sources.json".to_string(),
                "{\n  \"status\": \"completed\",\n  \"approved\": true,\n  \"notes\": \"Grounded notes captured from tandem-mcp.\"\n}".to_string(),
            )),
            &std::collections::BTreeSet::new(),
            None,
        );

    assert!(accepted_output.is_some(), "{artifact_validation:?}");
    assert!(rejected.is_none(), "{artifact_validation:?}");
    assert_eq!(
        artifact_validation
            .get("validation_profile")
            .and_then(Value::as_str),
        Some("artifact_only")
    );
    assert!(!artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("no_concrete_reads"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn generic_artifact_validation_warns_on_weak_report_markdown() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-editorial-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("workspace dir");
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "draft-report".to_string(),
        agent_id: "writer".to_string(),
        objective: "Draft the final report".to_string(),
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
    let session = Session::new(
        Some("editorial".to_string()),
        Some(workspace_root.to_string_lossy().to_string()),
    );
    let tool_telemetry = json!({
        "requested_tools": ["write"],
        "executed_tools": ["write"],
        "tool_call_counts": {
            "write": 1
        }
    });
    let (_, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "",
        &tool_telemetry,
        None,
        Some(("report.md".to_string(), "# Draft\n\nTODO\n".to_string())),
        &std::collections::BTreeSet::new(),
    );

    assert_eq!(rejected, None);
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("accepted_with_warnings")
    );
    assert_eq!(
        artifact_validation
            .get("warning_requirements")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        vec![
            json!("editorial_substance_missing"),
            json!("markdown_structure_missing")
        ]
    );
    assert_eq!(
        artifact_validation
            .get("heading_count")
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        artifact_validation
            .get("paragraph_count")
            .and_then(Value::as_u64),
        Some(1)
    );
    assert_eq!(
        detect_automation_node_failure_kind(
            &node,
            "completed",
            None,
            None,
            Some(&artifact_validation),
        ),
        None
    );
    assert_eq!(
        detect_automation_node_phase(&node, "completed", Some(&artifact_validation)),
        "completed"
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn publish_node_blocks_when_upstream_editorial_validation_failed() {
    let publish = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "publish".to_string(),
        agent_id: "publisher".to_string(),
        objective: "Publish final output".to_string(),
        depends_on: vec!["draft".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "draft".to_string(),
            alias: "draft".to_string(),
        }],
        output_contract: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: Some(AutomationNodeStageKind::Workstream),
        gate: None,
        metadata: Some(json!({
            "builder": {
                "role": "publisher"
            }
        })),
    };
    let mut run = test_phase_run(vec!["publish"], vec!["draft"]);
    run.checkpoint.node_outputs.insert(
        "draft".to_string(),
        json!({
            "node_id": "draft",
            "failure_kind": "editorial_quality_failed",
            "phase": "editorial_validation",
            "validator_summary": {
                "unmet_requirements": ["editorial_substance_missing", "markdown_structure_missing"]
            }
        }),
    );

    let reason = automation_publish_editorial_block_reason(&run, &publish).expect("publish block");
    assert!(reason.contains("draft"));
    assert!(reason.contains("editorial"));
}

#[test]
fn report_markdown_blocks_when_rich_upstream_evidence_is_reduced_to_generic_summary() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-upstream-synthesis-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
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
        metadata: Some(json!({
            "builder": {
                "output_path": "report.md"
            }
        })),
    };
    let session = Session::new(
        Some("thin-final-summary".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let mut session = session;
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "report.md",
                "content": "# Strategic Summary\n\nTandem is an engineering agent for local execution.\n\n## Positioning\n\nIt connects human intent to repo changes.\n"
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));
    let thin_report = "# Strategic Summary\n\nTandem is an engineering agent for local execution.\n\n## Positioning\n\nIt connects human intent to repo changes.\n".to_string();
    let upstream_evidence = AutomationUpstreamEvidence {
        read_paths: vec![
            "README.md".to_string(),
            "docs/product-capabilities.md".to_string(),
        ],
        discovered_relevant_paths: vec![
            "README.md".to_string(),
            "docs/product-capabilities.md".to_string(),
        ],
        web_research_attempted: true,
        web_research_succeeded: true,
        citation_count: 3,
        citations: vec![
            "https://example.com/source-1".to_string(),
            "https://example.com/source-2".to_string(),
            "https://example.com/source-3".to_string(),
        ],
    };

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            None,
            "Completed the report.",
            &json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            None,
            Some(("report.md".to_string(), thin_report)),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_some());
    assert_eq!(
        rejected.as_deref(),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn report_markdown_accepts_structured_synthesis_without_inline_citations_when_upstream_is_rich() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-upstream-synthesis-pass-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "analyze_findings".to_string(),
        agent_id: "analyst".to_string(),
        objective: "Synthesize findings into a strategy report".to_string(),
        depends_on: vec!["collect_inputs".to_string(), "research_sources".to_string()],
        input_refs: vec![
            AutomationFlowInputRef {
                from_step_id: "collect_inputs".to_string(),
                alias: "local_grounding".to_string(),
            },
            AutomationFlowInputRef {
                from_step_id: "research_sources".to_string(),
                alias: "external_research".to_string(),
            },
        ],
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
                "output_path": "analyze-findings.md"
            }
        })),
    };
    let mut session = Session::new(
        Some("structured-synthesis".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let report = "# Strategy Analysis Report\n\n## 1. Executive Summary\nThis analysis synthesizes Tandem's core internal product definitions and external research to refine positioning and strategy. Tandem is positioned as a high-autonomy, agentic engineering engine that solves cognitive load and cross-functional orchestration issues, positioning itself firmly against generic code assistants.\n\n## 2. Product Positioning\n*   **Core Identity:** Tandem by Frumu AI\n*   **Market Category:** Agentic Software Development / IDE-Integrated Engineering Tool\n*   **Key Positioning:** Empowering engineers with high-autonomy, context-accurate AI collaboration embedded directly within their development workflow.\n\n## 3. Target Users & Use-Case Wedges\n*   **Primary Users:** Professional engineers and development teams struggling with high cognitive load.\n*   **Use-Case Wedge:** Utilizing workspace-aware code analysis and automated task execution to bridge the gap between documentation and implementation.\n\n## 4. Investor Narrative & Competitive Outlook\n*   **Competitive Standing:** Tandem differentiates itself by being a full-context engineering engine rather than a simple chatbot.\n*   **Narrative Hook:** Stop context-switching and let Tandem handle tooling and documentation synthesis overhead.\n\n## 5. Risks & Proof Gaps\n*   **Market Risk:** Strong competition from well-capitalized code-assistant vendors.\n*   **Proof Gaps:** Need stronger empirical time-saved and throughput metrics.\n\n## 6. Execution Summary\nThe immediate priority is to prove the agentic value proposition with high-utility automation flows such as multi-file updates and refactors.\n\n---\n*Source Verification: Based on `.tandem/artifacts/collect-inputs.json` and `.tandem/artifacts/research-sources.json`.*\n".to_string();
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "analyze-findings.md",
                "content": report
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));
    let upstream_evidence = AutomationUpstreamEvidence {
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
    };

    let (accepted_output, artifact_validation, rejected) =
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
                    "read": 2,
                    "write": 1
                }
            }),
            None,
            Some(("analyze-findings.md".to_string(), report)),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_some());
    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        None
    );
    assert!(!artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn report_markdown_legacy_metadata_is_forced_to_strict_without_emergency_rollback() {
    with_legacy_quality_rollback_enabled(false, || {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-report-forced-strict-quality-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace_root).expect("create workspace");
        let snapshot = automation_workspace_root_file_snapshot(
            workspace_root.to_str().expect("workspace root"),
        );
        let node = AutomationFlowNode {
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
            metadata: Some(json!({
                "quality_mode": "legacy",
                "builder": {
                    "output_path": "generate-report.md"
                }
            })),
        };
        let mut session = Session::new(
            Some("legacy-quality-mode".to_string()),
            Some(workspace_root.to_str().expect("workspace root").to_string()),
        );
        let generic_report = "# Summary\n\nPlaceholder update.\n".to_string();
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": "generate-report.md",
                    "content": generic_report
                }),
                result: Some(json!("ok")),
                error: None,
            }],
        ));
        let upstream_evidence = AutomationUpstreamEvidence {
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
                        "read": 2,
                        "write": 1
                    }
                }),
                None,
                Some(("generate-report.md".to_string(), generic_report)),
                &snapshot,
                Some(&upstream_evidence),
            );

        assert!(rejected.is_some());
        assert!(artifact_validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .is_some_and(|items| items
                .iter()
                .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));
        assert_eq!(
            artifact_validation
                .get("validation_basis")
                .and_then(|value| value.get("quality_mode"))
                .and_then(Value::as_str),
            Some("strict_research_v1")
        );
        assert_eq!(
            artifact_validation
                .get("validation_basis")
                .and_then(|value| value.get("requested_quality_mode"))
                .and_then(Value::as_str),
            Some("legacy")
        );
        assert_eq!(
            artifact_validation
                .get("validation_basis")
                .and_then(|value| value.get("legacy_quality_rollback_enabled"))
                .and_then(Value::as_bool),
            Some(false)
        );

        let _ = std::fs::remove_dir_all(&workspace_root);
    });
}

#[test]
fn report_markdown_legacy_quality_mode_allows_generic_synthesis_with_emergency_rollback() {
    with_legacy_quality_rollback_enabled(true, || {
        let workspace_root = std::env::temp_dir().join(format!(
            "tandem-report-legacy-quality-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace_root).expect("create workspace");
        let snapshot = automation_workspace_root_file_snapshot(
            workspace_root.to_str().expect("workspace root"),
        );
        let node = AutomationFlowNode {
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
            metadata: Some(json!({
                "quality_mode": "legacy",
                "builder": {
                    "output_path": "generate-report.md"
                }
            })),
        };
        let mut session = Session::new(
            Some("legacy-quality-mode".to_string()),
            Some(workspace_root.to_str().expect("workspace root").to_string()),
        );
        let generic_report = "# Summary\n\nPlaceholder update.\n".to_string();
        session.messages.push(tandem_types::Message::new(
            MessageRole::Assistant,
            vec![MessagePart::ToolInvocation {
                tool: "write".to_string(),
                args: json!({
                    "path": "generate-report.md",
                    "content": generic_report
                }),
                result: Some(json!("ok")),
                error: None,
            }],
        ));
        let upstream_evidence = AutomationUpstreamEvidence {
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

        let (accepted_output, artifact_validation, rejected) =
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
                        "read": 2,
                        "write": 1
                    }
                }),
                None,
                Some(("generate-report.md".to_string(), generic_report)),
                &snapshot,
                Some(&upstream_evidence),
            );

        assert!(accepted_output.is_some());
        assert!(rejected.is_none());
        assert!(artifact_validation
            .get("unmet_requirements")
            .and_then(Value::as_array)
            .is_none_or(|items| !items
                .iter()
                .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));
        assert_eq!(
            artifact_validation
                .get("validation_basis")
                .and_then(|value| value.get("quality_mode"))
                .and_then(Value::as_str),
            Some("legacy")
        );
        assert_eq!(
            artifact_validation
                .get("validation_basis")
                .and_then(|value| value.get("requested_quality_mode"))
                .and_then(Value::as_str),
            Some("legacy")
        );
        assert_eq!(
            artifact_validation
                .get("validation_basis")
                .and_then(|value| value.get("legacy_quality_rollback_enabled"))
                .and_then(Value::as_bool),
            Some(true)
        );

        let _ = std::fs::remove_dir_all(&workspace_root);
    });
}

#[test]
fn report_markdown_rejects_generic_synthesis_without_evidence_anchors_when_upstream_is_rich() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-anchor-synthesis-block-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
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
        metadata: Some(json!({
            "builder": {
                "output_path": "generate-report.md"
            }
        })),
    };
    let mut session = Session::new(
        Some("anchor-block-report".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let generic_report = "# Strategic Summary\n\n## Executive Summary\nThis report synthesizes the available upstream evidence into a concise outlook.\n\n## Key Findings\n* Growth vectors were identified across the workflow.\n* Strategic positioning remains promising.\n\n## Critical Risks\n* Competitive pressure remains a factor.\n\n## Recommendations\n* Continue iterating on the workflow.\n\n## Evidence/Sources\n* Internal documentation and external research informed this summary.\n\n## Next Steps\n* Refine the messaging and validate the next cycle.\n"
        .to_string();
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "generate-report.md",
                "content": generic_report
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));
    let upstream_evidence = AutomationUpstreamEvidence {
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

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            None,
            "Completed the report.",
            &json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            None,
            Some(("generate-report.md".to_string(), generic_report)),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_some());
    assert_eq!(
        rejected.as_deref(),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn report_markdown_accepts_rich_html_synthesis_when_upstream_is_rich() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-html-synthesis-pass-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
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
        metadata: Some(json!({
            "builder": {
                "output_path": "generate-report.md"
            }
        })),
    };
    let mut session = Session::new(
        Some("html-report".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let html_report = r#"
<html>
  <body>
    <h1>Frumu AI Tandem: Strategic Summary</h1>
    <p>We synthesized the local Tandem docs and the external research into one report.</p>
    <h3>Core Value Proposition</h3>
    <p>Tandem is an engine-backed workflow system for local execution and agentic operations.</p>
    <ul>
      <li>Local workspace reads and patch-based code execution.</li>
      <li>Current web research for externally grounded synthesis.</li>
      <li>Explicit delivery gating for email and other side effects.</li>
    </ul>
    <h3>Strategic Outlook</h3>
    <p>The positioning emphasizes deterministic execution, provenance, and operator control.</p>
    <p>Sources reviewed: <a href=\".tandem/runs/run-123/artifacts/analyze-findings.md\">analysis</a> and <a href=\".tandem/runs/run-123/artifacts/research-sources.json\">research</a>.</p>
  </body>
</html>
"#
    .trim()
    .to_string();
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "generate-report.md",
                "content": html_report
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));
    let upstream_evidence = AutomationUpstreamEvidence {
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

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            None,
            "Completed the report.",
            &json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            None,
            Some(("generate-report.md".to_string(), html_report.clone())),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_some());
    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        None
    );
    assert!(!artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn report_markdown_rejects_generic_html_synthesis_without_evidence_anchors_when_upstream_is_rich() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-report-html-anchor-synthesis-block-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let snapshot =
        automation_workspace_root_file_snapshot(workspace_root.to_str().expect("workspace root"));
    let node = AutomationFlowNode {
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
        metadata: Some(json!({
            "builder": {
                "output_path": "generate-report.md"
            }
        })),
    };
    let mut session = Session::new(
        Some("html-anchor-block-report".to_string()),
        Some(workspace_root.to_str().expect("workspace root").to_string()),
    );
    let generic_html_report = r#"
<html>
  <body>
    <h1>Investor Summary: Strategic Analysis Report</h1>
    <p>We synthesized refined market data and findings from our research cycles into key growth vectors and strategic positioning for the target project.</p>
    <h3>Key Findings</h3>
    <ul>
      <li>Market growth vectors are present.</li>
      <li>Strategic positioning is available.</li>
    </ul>
    <h3>Critical Risks &amp; Considerations</h3>
    <p>Competitive pressure and entry barriers remain relevant.</p>
    <p>Operational mitigation follows the updated strategy.</p>
  </body>
</html>
"#
    .trim()
    .to_string();
    session.messages.push(tandem_types::Message::new(
        MessageRole::Assistant,
        vec![MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": "generate-report.md",
                "content": generic_html_report
            }),
            result: Some(json!("ok")),
            error: None,
        }],
    ));
    let upstream_evidence = AutomationUpstreamEvidence {
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

    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            None,
            "Completed the report.",
            &json!({
                "requested_tools": ["write"],
                "executed_tools": ["write"],
                "tool_call_counts": {
                    "write": 1
                }
            }),
            None,
            Some(("generate-report.md".to_string(), generic_html_report)),
            &snapshot,
            Some(&upstream_evidence),
        );

    assert!(accepted_output.is_some());
    assert_eq!(
        rejected.as_deref(),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("final artifact does not adequately synthesize the available upstream evidence")
    );
    assert!(artifact_validation
        .get("unmet_requirements")
        .and_then(Value::as_array)
        .is_some_and(|items| items
            .iter()
            .any(|value| value.as_str() == Some("upstream_evidence_not_synthesized"))));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn research_brief_full_pipeline_overrides_llm_blocked_to_needs_repair_without_source_coverage_flag()
{
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-research-full-pipeline-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let brief_text =
        "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n"
            .to_string();
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
        Some("research-full-pipeline".to_string()),
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
                args: json!({"pattern":"docs/**/*.md"}),
                result: Some(json!({"output": ""})),
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
    let session_text =
        "The brief is blocked.\n\n{\"status\":\"blocked\",\"reason\":\"tools unavailable\"}";
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    let (accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        session_text,
        &tool_telemetry,
        None,
        Some(("marketing-brief.md".to_string(), brief_text.clone())),
        &std::collections::BTreeSet::new(),
    );
    assert_eq!(
        rejected.as_deref(),
        Some("research completed without concrete file reads or required source coverage")
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        Some("research completed without concrete file reads or required source coverage")
    );

    let output: Value = wrap_automation_node_output(
        &node,
        &session,
        &requested_tools,
        "sess-research-full-pipeline",
        Some("run-research-full-pipeline"),
        session_text,
        accepted_output,
        Some(artifact_validation),
    );

    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("needs_repair")
    );
    assert_eq!(
        output.get("blocked_reason").and_then(Value::as_str),
        Some("research completed without concrete file reads or required source coverage")
    );
    assert!(!automation_output_is_blocked(&output));
    assert!(automation_output_needs_repair(&output));
    assert!(!automation_output_repair_exhausted(&output));

    let _ = std::fs::remove_dir_all(workspace_root);
}
