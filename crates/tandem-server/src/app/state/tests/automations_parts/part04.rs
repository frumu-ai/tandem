// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[tokio::test]
async fn automation_v2_misfires_skip_queued_or_running_runs_for_same_automation() {
    let state = test_state_with_path(tmp_resource_file("automation-misfire-dedup"));
    let mut automation = AutomationSpecBuilder::new("auto-misfire-dedup")
        .nodes(vec![AutomationNodeBuilder::new("work").build()])
        .build();
    automation.schedule = AutomationV2Schedule {
        schedule_type: AutomationV2ScheduleType::Interval,
        cron_expression: None,
        interval_seconds: Some(60),
        timezone: "UTC".to_string(),
        misfire_policy: RoutineMisfirePolicy::RunOnce,
    };
    automation.next_fire_at_ms = Some(1_000);
    automation.status = AutomationV2Status::Active;
    {
        let mut automations = state.automations_v2.write().await;
        automations.insert(automation.automation_id.clone(), automation.clone());
    }
    {
        let mut runs = state.automation_v2_runs.write().await;
        runs.insert(
            "run-existing".to_string(),
            AutomationRunBuilder::new("run-existing", "auto-misfire-dedup")
                .status(AutomationRunStatus::Queued)
                .build(),
        );
    }

    let due = state.evaluate_automation_v2_misfires(2_000).await;

    assert!(due.is_empty(), "queued automation run should dedupe timer fire");
    let stored = state
        .automations_v2
        .read()
        .await
        .get("auto-misfire-dedup")
        .cloned()
        .expect("automation stored");
    assert_eq!(stored.next_fire_at_ms, Some(1_000));
    assert_eq!(stored.last_fired_at_ms, None);
}

#[test]
fn prompt_orders_required_workspace_writes_before_run_artifact() {
    let automation = AutomationV2Spec {
        automation_id: "automation-review".to_string(),
        name: "Review".to_string(),
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
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            profile: None,
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
        node_id: "research_sources".to_string(),
        agent_id: "researcher".to_string(),
        objective: "Write a review and durable workspace report.".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
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
                "must_write_files": ["tandem-review.md"]
            }
        })),
    };
    let agent = AutomationAgentProfile {
        agent_id: "researcher".to_string(),
        template_id: None,
        display_name: "Researcher".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["read".to_string(), "write".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
            allowed_connections: Vec::new(),
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-review",
        &node,
        1,
        &agent,
        &[],
        &["read".to_string(), "write".to_string()],
        None,
        None,
        None,
    );

    let workspace_idx = prompt
        .find("Required Workspace Writes:")
        .expect("workspace writes section");
    let artifact_idx = prompt
        .find("Required Run Artifact:")
        .expect("run artifact section");
    assert!(workspace_idx < artifact_idx);
    assert!(prompt.contains("Write the required workspace file(s) first: `tandem-review.md`"));
    assert!(prompt.contains("Do not rely on, auto-copy, or mirror the run artifact"));
}

#[test]
fn prompt_includes_email_delivery_metadata_for_notify_user() {
    let automation = AutomationV2Spec {
        automation_id: "automation-email-delivery".to_string(),
        name: "Email Delivery Automation".to_string(),
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
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            profile: None,
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
        node_id: "notify_user".to_string(),
        agent_id: "committer".to_string(),
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
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
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
    let agent = AutomationAgentProfile {
        agent_id: "committer".to_string(),
        template_id: None,
        display_name: "Committer".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["*".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: vec!["composio-1".to_string()],
            allowed_tools: None,
            allowed_connections: Vec::new(),
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-email",
        &node,
        1,
        &agent,
        &[json!({
            "alias": "report_body",
            "from_step_id": "generate_report",
            "output": {
                "content": {
                    "path": ".tandem/artifacts/generate-report.html",
                    "text": "<h1>Tandem Strategic Analysis</h1><p>Rich upstream report body.</p>"
                }
            }
        })],
        &["*".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Delivery target:"));
    assert!(prompt.contains("`recipient@example.com`"));
    assert!(prompt.contains("Inline body only: `true`"));
    assert!(prompt.contains("Upstream synthesis rules:"));
    assert!(
        prompt.contains("use the compiled upstream report/body as the email body source of truth")
    );
    assert!(prompt.contains("For email delivery, use the compiled upstream report/body as the email body source of truth."));
    assert!(prompt.contains("Deterministic Delivery Body:"));
    assert!(prompt.contains("Source artifact:"));
    assert!(prompt.contains("generate-report.html"));
    assert!(prompt.contains("<h1>Tandem Strategic Analysis</h1>"));
    assert!(prompt.contains(
        "Do not mark the node completed unless you actually execute an email draft or send tool."
    ));
}

#[test]
fn prompt_compacts_upstream_outputs_for_downstream_nodes() {
    let automation = AutomationV2Spec {
        automation_id: "automation-compact-upstream".to_string(),
        name: "Compact Upstream Automation".to_string(),
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
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            profile: None,
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
        node_id: "analyze_findings".to_string(),
        agent_id: "analyst".to_string(),
        objective: "Synthesize the clustered themes into a concise analysis and recommendations."
            .to_string(),
        depends_on: vec!["cluster_topics".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "cluster_topics".to_string(),
            alias: "topic_clusters".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
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
        metadata: None,
    };
    let agent = AutomationAgentProfile {
        agent_id: "analyst".to_string(),
        template_id: None,
        display_name: "Analyst".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["*".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
            allowed_connections: Vec::new(),
        },
        approval_policy: None,
    };
    let upstream_inputs = vec![json!({
        "alias": "topic_clusters",
        "from_step_id": "cluster_topics",
        "output": {
            "status": "completed",
            "phase": "completed",
            "summary": "Clustered pain points into report themes.",
            "contract_kind": "structured_json",
            "artifact_validation": {
                "accepted_artifact_path": ".tandem/artifacts/cluster-topics.json",
                "artifact_candidates": [{"source": "verified_output", "score": 999}],
                "validation_outcome": "passed",
                "warning_count": 0
            },
            "validator_summary": {
                "kind": "structured_json",
                "outcome": "passed",
                "warning_count": 0
            },
            "tool_telemetry": {
                "executed_tools": ["read", "write"],
                "tool_call_counts": {"read": 1, "write": 1}
            },
            "content": {
                "path": ".tandem/artifacts/cluster-topics.json",
                "raw_assistant_text": "very verbose narrative",
                "text": "{\"themes\":[{\"id\":\"T1\",\"summary\":\"alpha\"}],\"cross_cutting_observation\":\"beta\"}"
            }
        }
    })];

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-compact",
        &node,
        1,
        &agent,
        &upstream_inputs,
        &["*".to_string()],
        None,
        None,
        None,
    );

    assert!(prompt.contains("\"themes\""));
    assert!(prompt.contains("\"cross_cutting_observation\""));
    assert!(!prompt.contains("artifact_candidates"));
    assert!(!prompt.contains("raw_assistant_text"));
    assert!(!prompt.contains("tool_call_counts"));
}

#[tokio::test]
async fn execute_collect_inputs_node_uses_deterministic_shortcut() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-collect-inputs-exec-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("workspace");

    let automation = AutomationV2Spec {
        automation_id: "automation-inline-collect-inputs".to_string(),
        name: "Collect Inputs Shortcut".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![AutomationAgentProfile {
            agent_id: "agent_planner".to_string(),
            template_id: None,
            display_name: "Planner".to_string(),
            avatar_url: None,
            model_policy: Some(json!({
                "default_model": "openrouter/not-a-real-model"
            })),
            skills: Vec::new(),
            tool_policy: AutomationAgentToolPolicy {
                allowlist: vec!["*".to_string()],
                denylist: Vec::new(),
            },
            mcp_policy: AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
                allowed_connections: Vec::new(),
            },
            approval_policy: None,
        }],
        flow: AutomationFlowSpec {
            nodes: vec![AutomationFlowNode {
                knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                node_id: "collect_inputs".to_string(),
                agent_id: "agent_planner".to_string(),
                objective: "Capture the report topic, delivery target, and formatting constraints."
                    .to_string(),
                depends_on: Vec::new(),
                input_refs: Vec::new(),
                output_contract: Some(AutomationFlowOutputContract {
                    kind: "brief".to_string(),
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
                    "inputs": {
                        "topic": "autonomous AI agentic workflows",
                        "delivery_email": "recipient@example.com",
                        "email_format": "simple html",
                        "attachments_allowed": false
                    }
                })),
            }],
        },
        execution: AutomationExecutionPolicy {
            profile: None,
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        creator_id: "test".to_string(),
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: Some(json!({
            "context_materialization": {
                "routines": [
                    {
                        "routine_id": "collect_inputs",
                        "visible_context_objects": [],
                        "step_context_bindings": [
                            {
                                "step_id": "collect_inputs",
                                "context_reads": ["ctx:collect_inputs:mission.goal"],
                                "context_writes": []
                            }
                        ]
                    }
                ]
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };

    let state = ready_test_state().await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    assert_eq!(
        run.runtime_context
            .as_ref()
            .map(|context| context.routines.len()),
        Some(1)
    );
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.runtime_context = None;
        })
        .await
        .expect("clear runtime context");
    let claimed = state
        .claim_specific_automation_v2_run(&run.run_id)
        .await
        .expect("claim run");
    assert_eq!(
        claimed
            .runtime_context
            .as_ref()
            .map(|context| context.routines.len()),
        Some(1)
    );
    let node = automation.flow.nodes.first().expect("collect_inputs node");
    let agent = automation.agents.first().expect("planner agent");

    let output = execute_automation_v2_node(&state, &claimed.run_id, &automation, node, agent)
        .await
        .expect("execute collect_inputs");

    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        output
            .get("artifact_validation")
            .and_then(|value| value.get("deterministic_artifact"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        output
            .get("artifact_validation")
            .and_then(|value| value.get("deterministic_source"))
            .and_then(Value::as_str),
        Some("node_metadata_inputs")
    );

    let artifact_path = workspace_root
        .join(".tandem/runs")
        .join(&claimed.run_id)
        .join("artifacts")
        .join("collect-inputs.json");
    assert!(artifact_path.exists());
    let artifact_text = std::fs::read_to_string(&artifact_path).expect("artifact text");
    assert!(artifact_text.contains("autonomous AI agentic workflows"));

    let session_id = output
        .get("content")
        .and_then(|value| value.get("session_id"))
        .and_then(Value::as_str)
        .expect("session id");
    let session = state
        .storage
        .get_session(session_id)
        .await
        .expect("deterministic session");
    assert!(session.messages.iter().all(|message| {
        message
            .parts
            .iter()
            .all(|part| !matches!(part, tandem_types::MessagePart::ToolInvocation { .. }))
    }));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

// TAN-600: the workflow-artifact data-boundary guard must fire on exactly the
// artifact the email-delivery prompt fold embeds, with the audit-safe event
// shape shared with the other source guards. Env-touching, so it joins the
// DEFAULT serial group used by the other data-boundary tests.
#[test]
#[serial_test::serial]
fn workflow_artifact_guard_covers_the_folded_delivery_artifact() {
    let previous_mode = std::env::var("TANDEM_DATA_BOUNDARY_MODE").ok();
    std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", "audit");

    let automation = AutomationV2Spec {
        automation_id: "automation-artifact-guard".to_string(),
        name: "Artifact Guard Automation".to_string(),
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
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            profile: None,
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
        node_id: "notify_user".to_string(),
        agent_id: "committer".to_string(),
        objective: "Send the finalized report by email.".to_string(),
        depends_on: vec!["generate_report".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "generate_report".to_string(),
            alias: "report_body".to_string(),
        }],
        output_contract: None,
        tool_policy: None,
        mcp_policy: None,
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
        wait: None,
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
    let secret = "sk-live-abcdef1234567890";
    // A clean decoy that out-lengths the sensitive artifact ONLY before
    // normalization: its text is JSON whose bulk is a `context_writes` blob
    // the prompt path strips. If the guard selected from pre-normalized
    // inputs it would scan this decoy, find nothing, and emit no event while
    // the prompt embeds the sensitive artifact (Codex P2 on PR #1789).
    let decoy_text = serde_json::to_string(&json!({
        "summary": "clean upstream summary",
        "context_writes": (0..40)
            .map(|index| format!("ctx:wfplan-decoy-{index}:node:artifact"))
            .collect::<Vec<_>>(),
    }))
    .expect("decoy json");
    let upstream_inputs = vec![
        json!({
            "alias": "report_body",
            "from_step_id": "generate_report",
            "output": {
                "content": {
                    "path": ".tandem/artifacts/generate-report.html",
                    "text": format!(
                        "<h1>Quarterly Report</h1><p>api_key={} must be rotated.</p>{}",
                        secret,
                        "<p>Body paragraph with enough length to win post-normalization.</p>"
                    )
                }
            }
        }),
        json!({
            "alias": "decoy_summary",
            "from_step_id": "collect_context",
            "output": {
                "content": {
                    "path": ".tandem/artifacts/decoy-summary.json",
                    "text": decoy_text
                }
            }
        }),
    ];
    let mut tenant = tandem_types::TenantContext::local_implicit();
    tenant.org_id = "org-artifact".to_string();
    tenant.workspace_id = "workspace-artifact".to_string();

    let event = crate::app::state::automation::prompting_impl::workflow_artifact_boundary_event(
        "/tmp",
        "run-artifact-guard",
        &node,
        &upstream_inputs,
        None,
        &tenant,
    )
    .expect("artifact with findings must produce a boundary event");
    assert_eq!(event.properties["sourceKind"], "workflow_artifact");
    assert_eq!(event.properties["runID"], "run-artifact-guard");
    assert_eq!(event.properties["auditOnly"], true);
    assert_eq!(event.properties["enforced"], false);
    assert_eq!(event.properties["tenant"]["organization_id"], "org-artifact");
    let serialized = serde_json::to_string(&event.properties).expect("json");
    assert!(
        !serialized.contains(secret),
        "guard event must carry safe evidence only: {serialized}"
    );

    // Anti-drift: the guard scanned the same artifact the fold embeds. The
    // selector is the single selection rule, and the rendered prompt labels
    // the same source artifact.
    let normalized =
        crate::app::state::automation::prompting_impl::automation_prompt_normalize_upstream_inputs(
            "/tmp",
            "run-artifact-guard",
            &upstream_inputs,
            None,
        );
    let (selected_text, selected_path) =
        crate::app::state::automation::prompting_impl::automation_prompt_select_delivery_artifact(
            &normalized,
        )
        .expect("selector must pick the delivery artifact");
    assert!(
        selected_text.contains(secret),
        "post-normalization selection must pick the sensitive artifact, not the decoy"
    );
    assert!(selected_path.contains("generate-report.html"));
    let agent = AutomationAgentProfile {
        agent_id: "committer".to_string(),
        template_id: None,
        display_name: "Committer".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec!["*".to_string()],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
            allowed_connections: Vec::new(),
        },
        approval_policy: None,
    };
    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp",
        "run-artifact-guard",
        &node,
        1,
        &agent,
        &upstream_inputs,
        &["*".to_string()],
        None,
        None,
        None,
    );
    assert!(prompt.contains("Deterministic Delivery Body:"));
    assert!(
        prompt.contains(&selected_path),
        "fold must label the same artifact the guard scanned"
    );

    // Nodes without email delivery fold no artifact, so no event fires.
    let mut plain_node = node.clone();
    plain_node.metadata = None;
    assert!(
        crate::app::state::automation::prompting_impl::workflow_artifact_boundary_event(
            "/tmp",
            "run-artifact-guard",
            &plain_node,
            &upstream_inputs,
            None,
            &tenant,
        )
        .is_none()
    );

    // Boundary off: guard is silent even for email-delivery nodes.
    std::env::remove_var("TANDEM_DATA_BOUNDARY_MODE");
    assert!(
        crate::app::state::automation::prompting_impl::workflow_artifact_boundary_event(
            "/tmp",
            "run-artifact-guard",
            &node,
            &upstream_inputs,
            None,
            &tenant,
        )
        .is_none()
    );
    if let Some(previous) = previous_mode {
        std::env::set_var("TANDEM_DATA_BOUNDARY_MODE", previous);
    }
}
