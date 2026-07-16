// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[test]
fn automation_prompt_includes_sanitized_webhook_trigger_context() {
    let mut automation = automation_with_output_targets(Vec::new(), Vec::new());
    automation.metadata = Some(json!({
        "automation_webhook": {
            "provider": "generic",
            "provider_event_kind": "customer.incident_reported",
            "provider_event_id": "incident-demo-1",
            "trust": "untrusted_external_webhook",
            "preview": {
                "event": "customer.incident_reported",
                "event_id": "incident-demo-1",
                "customer_name": "ACME",
                "service": "Payments API",
                "summary": "Elevated 5xx errors affecting checkout"
            }
        }
    }));
    let node = bare_node();
    let agent = AutomationAgentProfile {
        agent_id: "agent-demo".to_string(),
        template_id: None,
        display_name: "Demo Agent".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: AutomationAgentToolPolicy {
            allowlist: Vec::new(),
            denylist: Vec::new(),
        },
        mcp_policy: AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
            allowed_connections: Vec::new(),
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp/workspace",
        "run-demo",
        &node,
        1,
        &agent,
        &[],
        &[],
        None,
        None,
        None,
    );

    assert!(prompt.contains("Run Trigger Context:"));
    assert!(prompt.contains("\"customer_name\": \"ACME\""));
    assert!(prompt.contains("\"service\": \"Payments API\""));
    assert!(prompt.contains("untrusted external data"));
}

#[test]
fn intermediate_nodes_cannot_treat_live_output_targets_as_input_files() {
    let mut gather = bare_node();
    gather.node_id = "gather_fintech_candidates".to_string();
    gather.objective = "Research fintech sponsor candidates.".to_string();
    gather.metadata = Some(json!({
        "builder": {
            "input_files": ["/tmp/workspace/sales/genz-sponsor-research/2026-04-16_1530_genz_sponsor_targets.md"]
        }
    }));

    let mut finalize = bare_node();
    finalize.node_id = "draft_markdown_report".to_string();
    finalize.objective = "Write the final sponsor targets report to sales/genz-sponsor-research/2026-04-16_1530_genz_sponsor_targets.md.".to_string();
    finalize.depends_on = vec!["gather_fintech_candidates".to_string()];

    let automation = automation_with_live_output_target(vec![gather.clone(), finalize]);
    let runtime_values = runtime_values("2026-04-16", "1530", "2026-04-16 15:30");

    let input_files = automation_node_effective_input_files_for_automation(
        &automation,
        &gather,
        Some(&runtime_values),
    );

    assert!(input_files.is_empty(), "expected live output targets to be stripped from intermediate input files, got {input_files:?}");
}

#[test]
fn terminal_report_node_may_access_live_output_target() {
    let gather = {
        let mut node = bare_node();
        node.node_id = "gather_candidates".to_string();
        node.objective = "Research sponsor candidates.".to_string();
        node
    };
    let mut finalize = bare_node();
    finalize.node_id = "draft_markdown_report".to_string();
    finalize.objective = "Append the final sponsor targets report to sales/genz-sponsor-research/2026-04-16_1530_genz_sponsor_targets.md.".to_string();
    finalize.depends_on = vec!["gather_candidates".to_string()];
    finalize.metadata = Some(json!({
        "builder": {
            "input_files": ["sales/genz-sponsor-research/2026-04-16_1530_genz_sponsor_targets.md"]
        }
    }));

    let automation = automation_with_live_output_target(vec![gather, finalize.clone()]);
    let runtime_values = runtime_values("2026-04-16", "1530", "2026-04-16 15:30");

    let input_files = automation_node_effective_input_files_for_automation(
        &automation,
        &finalize,
        Some(&runtime_values),
    );

    assert_eq!(
        input_files,
        vec!["sales/genz-sponsor-research/2026-04-16_1530_genz_sponsor_targets.md".to_string()]
    );
}

#[test]
fn report_markdown_nodes_do_not_infer_template_filenames_as_workspace_writes() {
    let automation = AutomationV2Spec {
        automation_id: "automation-report-markdown".to_string(),
        name: "Report Markdown".to_string(),
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
        output_targets: vec![
            "daily-recaps/{current_date}-job-search-recap.md".to_string(),
            "opportunities/ranked/{current_date}-ranked-opportunities.md".to_string(),
            "opportunities/shortlisted/{current_date}-shortlist.md".to_string(),
            "tracker/pipeline.md".to_string(),
        ],
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
    let mut node = bare_node();
    node.node_id = "analyze_findings".to_string();
    node.objective = "Normalize only worthwhile jobs into per-role folders with `source.md`, `normalized-job.md`, `fit-analysis.md`, `apply-details.md`, and `status.json`; score fit honestly using `RESUME.md`, `resume-overview.md`, and `resume-positioning.md`; update daily ranked opportunities, shortlist, and pipeline views; then merge the daily recap so ratings, links, company names, role titles, and concise next steps are present.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "report_markdown".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });

    let must_write_files = automation_node_must_write_files_for_automation(
        &automation,
        &node,
        Some(&runtime_values("2026-04-09", "2138", "2026-04-09 21:38")),
    );

    assert!(!must_write_files.iter().any(|path| {
        matches!(
            path.as_str(),
            "source.md"
                | "normalized-job.md"
                | "fit-analysis.md"
                | "apply-details.md"
                | "status.json"
                | "RESUME.md"
                | "resume-overview.md"
                | "resume-positioning.md"
        )
    }));
    assert!(must_write_files.is_empty());
}

#[test]
fn automation_wide_read_only_rules_filter_later_node_write_targets() {
    let protect_node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "assess".to_string(),
        agent_id: "a1".to_string(),
        objective: "Read RESUME.md as the source of truth. Never edit, rewrite, rename, move, or delete RESUME.md.".to_string(),
        depends_on: vec![],
        input_refs: vec![],
        output_contract: None,
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
    let mut write_node = bare_node();
    write_node.node_id = "generate_report".to_string();
    write_node.objective =
        "Create the daily results file and return the append-safe report summary.".to_string();
    write_node.metadata = Some(json!({
        "builder": {
            "output_files": ["RESUME.md", "daily_results_{current_date}.md"]
        }
    }));
    let automation = AutomationV2Spec {
        automation_id: "automation-read-only-invariant".to_string(),
        name: "Read Only Invariant".to_string(),
        description: Some(
            "Only read from RESUME.md. Keep RESUME.md untouched throughout the workflow."
                .to_string(),
        ),
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
        flow: crate::AutomationFlowSpec {
            nodes: vec![protect_node, write_node.clone()],
        },
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
        workspace_root: Some("/home/evan/job-hunt".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };

    let must_write_files = automation_node_must_write_files_for_automation(
        &automation,
        &write_node,
        Some(&runtime_values("2026-04-15", "1049", "2026-04-15 10:49")),
    );

    assert!(!must_write_files.iter().any(|path| path == "RESUME.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "daily_results_2026-04-15.md"));
}

#[test]
fn wildcard_tool_allowlist_does_not_select_mcp_servers() {
    let selected = automation_infer_selected_mcp_servers(
        &Vec::new(),
        &vec!["*".to_string()],
        &vec!["github".to_string(), "slack".to_string()],
        false,
    );
    assert!(selected.is_empty());
}

#[test]
fn automation_quality_mode_defaults_to_strict_and_requires_rollback_for_legacy_metadata() {
    let strict_mode =
        super::enforcement::automation_quality_mode_resolution_from_metadata(None, true, false);
    assert_eq!(
        strict_mode.effective,
        super::enforcement::AutomationQualityMode::StrictResearchV1
    );
    assert_eq!(strict_mode.requested, None);
    assert!(!strict_mode.legacy_rollback_enabled);

    let legacy_metadata = serde_json::json!({
        "quality_mode": "legacy"
    });
    let legacy_object = legacy_metadata.as_object().cloned().expect("object");
    let forced_strict = super::enforcement::automation_quality_mode_resolution_from_metadata(
        Some(&legacy_object),
        true,
        false,
    );
    assert_eq!(
        forced_strict.requested,
        Some(super::enforcement::AutomationQualityMode::Legacy)
    );
    assert_eq!(
        forced_strict.effective,
        super::enforcement::AutomationQualityMode::StrictResearchV1
    );

    let legacy_mode = super::enforcement::automation_quality_mode_resolution_from_metadata(
        Some(&legacy_object),
        true,
        true,
    );
    assert_eq!(
        legacy_mode.requested,
        Some(super::enforcement::AutomationQualityMode::Legacy)
    );
    assert_eq!(
        legacy_mode.effective,
        super::enforcement::AutomationQualityMode::Legacy
    );
}

// -----------------------------------------------------------------------
// automation_infer_selected_mcp_servers
// -----------------------------------------------------------------------

#[test]
fn mcp_servers_empty_inputs_returns_empty() {
    let result = automation_infer_selected_mcp_servers(&[], &[], &[], false);
    assert!(result.is_empty());
}

#[test]
fn mcp_servers_explicit_allowed_list_returned_directly() {
    let result = automation_infer_selected_mcp_servers(
        &["gmail".to_string()],
        &[],
        &["gmail".to_string(), "slack".to_string()],
        false,
    );
    assert_eq!(result, vec!["gmail"]);
}

#[test]
fn mcp_servers_allowlist_wildcard_does_not_select_any_servers() {
    let enabled = vec!["gmail".to_string(), "slack".to_string()];
    let result = automation_infer_selected_mcp_servers(&[], &["*".to_string()], &enabled, false);
    assert!(result.is_empty());
}

#[test]
fn mcp_servers_requires_email_delivery_returns_all_enabled() {
    let enabled = vec!["gmail".to_string(), "hubspot".to_string()];
    let result = automation_infer_selected_mcp_servers(&[], &[], &enabled, true);
    assert_eq!(result, enabled);
}

#[test]
fn report_markdown_preserves_full_upstream_inputs() {
    let node = report_markdown_node();
    assert!(automation_node_preserves_full_upstream_inputs(&node));

    let mut email_delivery = email_delivery_node();
    email_delivery.depends_on = vec!["generate_report".to_string()];
    email_delivery.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "generate_report".to_string(),
        alias: "report_body".to_string(),
    }];
    assert!(automation_node_preserves_full_upstream_inputs(
        &email_delivery
    ));

    let mut execute_goal = bare_node();
    execute_goal.node_id = "execute_goal".to_string();
    execute_goal.objective =
        "Create a Gmail draft or send the final HTML summary email to recipient@example.com if mail tools are available.".to_string();
    execute_goal.output_contract = Some(AutomationFlowOutputContract {
        kind: "approval_gate".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ReviewDecision),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    execute_goal.depends_on = vec!["generate_report".to_string()];
    execute_goal.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "generate_report".to_string(),
        alias: "report_body".to_string(),
    }];
    execute_goal.metadata = Some(json!({
        "delivery": {
            "method": "email",
            "to": "recipient@example.com",
            "content_type": "text/html",
            "inline_body_only": true,
            "attachments": false
        }
    }));
    assert!(automation_node_preserves_full_upstream_inputs(
        &execute_goal
    ));

    let mut text_summary = bare_node();
    text_summary.output_contract = Some(AutomationFlowOutputContract {
        kind: "text_summary".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    text_summary.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "prev".to_string(),
        alias: "input".to_string(),
    }];
    assert!(automation_node_preserves_full_upstream_inputs(
        &text_summary
    ));
}


#[test]
fn connector_writer_metadata_preserves_full_upstream_inputs() {
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "filter_leads".to_string(),
        alias: "filtered_leads".to_string(),
    }];
    node.metadata = Some(json!({
        "connector_writer": true
    }));

    assert!(automation_node_uses_upstream_validation_evidence(&node));
    assert!(automation_node_preserves_full_upstream_inputs(&node));
}

#[test]
fn blog_draft_objective_with_negative_gmail_mentions_does_not_require_email_delivery() {
    let mut node = report_markdown_node();
    node.node_id = "generate_report".to_string();
    node.objective = "The blog post is NOT about Gmail/Reddit/blog integrations as product marketing. Before drafting the article, write article-thesis.md, then produce blog-draft.md and blog-package.md with a publish-ready article.".to_string();

    assert!(!automation_node_requires_email_delivery(&node));
}

#[test]
fn explicit_gmail_draft_objective_requires_email_delivery() {
    let mut node = bare_node();
    node.node_id = "execute_goal".to_string();
    node.objective =
        "Create a Gmail draft or send the final HTML summary email to recipient@example.com."
            .to_string();

    assert!(automation_node_requires_email_delivery(&node));
}

#[test]
fn gmail_draft_objective_with_no_send_constraint_requires_email_delivery() {
    let mut node = bare_node();
    node.node_id = "create-gmail-draft".to_string();
    node.objective = "Create a Gmail draft from compose-email output. Do not send the email. Return draft_id, recipient, subject, and draft_url.".to_string();

    assert!(automation_node_requires_email_delivery(&node));
}

#[test]
fn generic_synthesis_nodes_get_default_artifact_paths_without_legacy_ids() {
    let node = generic_research_artifact_node();

    assert_eq!(
        super::node_runtime_impl::automation_node_default_output_path(&node).as_deref(),
        Some(".tandem/artifacts/summarize-resume-signals.json")
    );
}

#[test]
fn delivery_nodes_do_not_get_default_artifact_paths() {
    let node = email_delivery_node();

    assert_eq!(
        super::node_runtime_impl::automation_node_default_output_path(&node),
        None
    );
}

#[test]
fn approval_gates_are_not_outbound_actions() {
    let mut node = bare_node();
    node.objective = "Approve this draft before publishing it.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "approval_gate".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ReviewDecision),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });

    assert!(!automation_node_is_outbound_action(&node));
}

#[test]
fn gated_local_publications_are_not_outbound_actions() {
    let mut node = bare_node();
    node.node_id = "publish_approved_update".to_string();
    node.objective = "Publish the approved update as a local artifact.".to_string();
    node.metadata = Some(json!({
        "artifact": {
            "path": "demo-output/approved/incident-update.md",
            "visible_in_run": true
        },
        "builder": {
            "role": "publisher",
            "task_kind": "delivery",
            "task_class": "gated_local_publish"
        }
    }));

    assert!(!automation_node_is_outbound_action(&node));
}

#[test]
fn connector_triage_nodes_are_not_outbound_actions() {
    let mut node = bare_node();
    node.node_id = "assess_issue".to_string();
    node.objective = "Classify the issue initial urgency and delivery/security risk.".to_string();
    node.metadata = Some(json!({
        "triage_gate": true,
        "builder": {
            "task_kind": "delivery",
            "task_class": "connector_triage",
            "triage_model": true
        }
    }));

    assert!(!automation_node_is_outbound_action(&node));
}

#[test]
fn delivery_risk_language_is_not_an_outbound_action() {
    let mut node = bare_node();
    node.objective = "Assess delivery/security/operational risk before planning.".to_string();

    assert!(!automation_node_is_outbound_action(&node));
}

#[test]
fn explicit_deliver_action_remains_outbound() {
    let mut node = bare_node();
    node.objective = "Deliver the approved report to the customer.".to_string();

    assert!(automation_node_is_outbound_action(&node));
}

#[test]
fn planner_delivery_publication_gets_a_run_artifact_path() {
    let mut node = bare_node();
    node.node_id = "publish_approved_update".to_string();
    node.objective = "Publish the approved local artifact.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "report_markdown".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "artifact": {
            "path": "demo-output/approved/incident-update.md",
            "visible_in_run": true
        },
        "builder": {
            "task_kind": "delivery",
            "task_class": "gated_local_publish"
        }
    }));

    assert_eq!(
        super::node_runtime_impl::automation_node_default_output_path(&node).as_deref(),
        Some(".tandem/artifacts/publish-approved-update.md")
    );
    assert_eq!(
        super::automation_node_required_output_path_for_run(&node, Some("run-approved"))
            .as_deref(),
        Some(".tandem/runs/run-approved/artifacts/publish-approved-update.md")
    );
}

#[test]
fn metadata_can_disable_default_artifact_paths_for_wrapper_nodes() {
    let mut node = generic_research_artifact_node();
    node.metadata = Some(json!({
        "disable_default_output_path": true
    }));

    assert_eq!(
        super::node_runtime_impl::automation_node_default_output_path(&node),
        None
    );

    let mut builder_node = generic_research_artifact_node();
    builder_node.metadata = Some(json!({
        "builder": {
            "output_path_mode": "none"
        }
    }));

    assert_eq!(
        super::node_runtime_impl::automation_node_default_output_path(&builder_node),
        None
    );
}

#[test]
fn mcp_servers_allowlist_namespace_pattern_matches_server() {
    // "mcp.my_server.*" should match server named "my-server" (dashes → underscores)
    let enabled = vec!["my-server".to_string(), "other".to_string()];
    let result = automation_infer_selected_mcp_servers(
        &[],
        &["mcp.my_server.*".to_string()],
        &enabled,
        false,
    );
    assert_eq!(result, vec!["my-server"]);
}

#[test]
fn mcp_servers_deduplicates_when_allowed_and_allowlist_overlap() {
    let enabled = vec!["gmail".to_string()];
    let result = automation_infer_selected_mcp_servers(
        &["gmail".to_string()],
        &["mcp.gmail.*".to_string()],
        &enabled,
        false,
    );
    assert_eq!(result, vec!["gmail"]);
}

#[test]
fn missing_capabilities_from_collapsed_tool_resolution_are_detected() {
    let node = email_delivery_node();
    let available_tool_names = std::collections::HashSet::from(["mcp_list".to_string()]);
    let resolution = automation_resolve_capabilities_with_schemas(
        &node,
        "structured_json",
        &["mcp_list".to_string()],
        &available_tool_names,
        &[],
    );

    assert_eq!(
        automation_capability_resolution_missing_capabilities(&resolution),
        vec!["email_draft".to_string(), "email_send".to_string()]
    );
}

#[test]
fn retry_attempt_tool_failure_labels_are_cleared_before_reuse() {
    let mut tool_telemetry = json!({
        "latest_web_research_failure": "web research timed out",
        "latest_email_delivery_failure": "smtp unauthorized",
        "attempt_evidence": {
            "evidence": {
                "web_research": {
                    "latest_failure": "dns error"
                }
            },
            "delivery": {
                "latest_failure": "unauthorized"
            }
        }
    });

    automation_reset_attempt_tool_failure_labels(&mut tool_telemetry);

    assert!(tool_telemetry
        .get("latest_web_research_failure")
        .is_some_and(Value::is_null));
    assert!(tool_telemetry
        .get("latest_email_delivery_failure")
        .is_some_and(Value::is_null));
    assert!(tool_telemetry
        .pointer("/attempt_evidence/evidence/web_research/latest_failure")
        .is_some_and(Value::is_null));
    assert!(tool_telemetry
        .pointer("/attempt_evidence/delivery/latest_failure")
        .is_some_and(Value::is_null));
}

#[test]
fn email_send_detection_recognizes_compact_sendemail_names() {
    assert!(automation_tool_name_is_email_send(
        "mcp.composio_1.gmail_sendemail"
    ));
    assert!(automation_tool_name_is_email_send("Gmail_SendEmail"));
    assert!(automation_tool_name_is_email_draft(
        "mcp.composio_1.gmail_draftemail"
    ));
}

#[test]
fn step_cost_provenance_marks_budget_limit_and_cost_deltas() {
    let provenance = automation_step_cost_provenance(
        "step_1",
        Some("gpt-5.1".to_string()),
        120,
        80,
        2.75,
        9.50,
        true,
    );

    assert_eq!(
        provenance.get("step_id").and_then(Value::as_str),
        Some("step_1")
    );
    assert_eq!(
        provenance.get("model_id").and_then(Value::as_str),
        Some("gpt-5.1")
    );
    assert_eq!(
        provenance.get("tokens_in").and_then(Value::as_u64),
        Some(120)
    );
    assert_eq!(
        provenance.get("tokens_out").and_then(Value::as_u64),
        Some(80)
    );
    assert_eq!(
        provenance.get("computed_cost_usd").and_then(Value::as_f64),
        Some(2.75)
    );
    assert_eq!(
        provenance
            .get("cumulative_run_cost_usd_at_step_end")
            .and_then(Value::as_f64),
        Some(9.50)
    );
    assert_eq!(
        provenance
            .get("budget_limit_reached")
            .and_then(Value::as_bool),
        Some(true)
    );
}

// -----------------------------------------------------------------------
// automation_tool_capability_ids
// -----------------------------------------------------------------------

#[test]
fn capability_ids_default_artifact_node_includes_write_and_discover() {
    let node = bare_node();
    let caps = automation_tool_capability_ids(&node, "research");
    assert_eq!(
        caps,
        vec![
            "artifact_write".to_string(),
            "workspace_discover".to_string()
        ]
    );
}

#[test]
fn capability_ids_node_with_input_ref_includes_workspace_read() {
    let node = node_with_input_ref();
    let caps = automation_tool_capability_ids(&node, "research");
    assert!(caps.contains(&"workspace_read".to_string()));
}

#[test]
fn capability_ids_connector_source_output_excludes_workspace_discover() {
    let mut node = bare_node();
    node.objective = "Use Reddit MCP to check for fresh AI productivity discussions.".to_string();
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/assess-reddit-activity.json"
        },
        "tool_allowlist": [
            "mcp.reddit_gmail.reddit_search_across_subreddits"
        ]
    }));

    let caps = automation_tool_capability_ids(&node, "artifact_write");

    assert!(caps.contains(&"artifact_write".to_string()));
    assert!(
        !caps.contains(&"workspace_discover".to_string()),
        "connector-source artifact writes should not advertise workspace discovery"
    );
}

#[test]
fn capability_ids_exact_path_read_write_node_excludes_workspace_discover() {
    let mut node = bare_node();
    node.node_id = "draft_incident_update".to_string();
    node.objective = "Use read on demo-output/context/incident-context.md, then write demo-output/drafts/incident-update.md.".to_string();
    node.metadata = Some(json!({
        "artifact": {
            "path": "demo-output/drafts/incident-update.md",
            "visible_in_run": true
        },
        "filesystem_policy": {
            "read_paths": ["demo-output/context/incident-context.md"],
            "write_paths": ["demo-output/drafts/incident-update.md"]
        },
        "required_tools": ["read", "write"],
        "tool_allowlist": ["read", "write"]
    }));

    let caps = automation_tool_capability_ids(&node, "artifact_write");

    assert!(caps.contains(&"workspace_read".to_string()));
    assert!(caps.contains(&"artifact_write".to_string()));
    assert!(
        !caps.contains(&"workspace_discover".to_string()),
        "an exact-path read/write node must not require an unrequested discovery tool: {caps:?}"
    );
}

#[test]
fn capability_ids_human_approval_excludes_artifact_write() {
    let mut node = bare_node();
    node.node_id = "human_approval".to_string();
    node.objective = "Pause for approval, use no tools, and do not create, modify, or delete files.".to_string();
    node.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "draft_incident_update".to_string(),
        alias: "draft_for_review".to_string(),
    }];
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "approval_gate".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ReviewDecision),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "stage_kind": "Approval",
        "approval": {
            "allowed_decisions": ["Approve", "Reject"],
            "require_explicit_decision": true
        },
        "builder": {
            "task_class": "human_decision_gate"
        },
        "tool_allowlist": []
    }));

    let caps = automation_tool_capability_ids(&node, "artifact_write");

    assert!(
        !caps.contains(&"artifact_write".to_string()),
        "human approval must not infer a write capability from negated file language: {caps:?}"
    );
}

#[test]
fn capability_ids_connector_source_input_ref_excludes_implicit_workspace_read() {
    let mut node = node_with_input_ref();
    node.objective =
        "Use Reddit MCP to collect follow-up source evidence from upstream triage.".to_string();
    node.metadata = Some(json!({
        "tool_allowlist": [
            "mcp.reddit_gmail.reddit_search_across_subreddits",
            "write"
        ]
    }));

    let caps = automation_tool_capability_ids(&node, "artifact_write");

    assert!(
        !caps.contains(&"workspace_read".to_string()),
        "connector-source nodes receive upstream context without requiring local read tools"
    );
    assert!(caps.contains(&"artifact_write".to_string()));
}

#[test]
fn capability_ids_code_workflow_git_patch_includes_verify_command() {
    let caps = automation_tool_capability_ids(&code_workflow_node(), "git_patch");
    assert!(
        caps.contains(&"verify_command".to_string()),
        "git_patch code node should require verify_command, got: {caps:?}"
    );
}

#[test]
fn capability_ids_code_workflow_research_mode_excludes_verify_command() {
    let caps = automation_tool_capability_ids(&code_workflow_node(), "research");
    assert!(
        !caps.contains(&"verify_command".to_string()),
        "research mode should not include verify_command, got: {caps:?}"
    );
}

#[test]
fn code_patch_contract_is_treated_as_a_code_workflow() {
    let node = code_patch_contract_node();
    assert_eq!(
        automation_output_validator_kind(&node),
        crate::AutomationOutputValidatorKind::CodePatch
    );
    assert!(automation_node_is_code_workflow(&node));
    assert_eq!(
        automation_node_execution_policy(&node, ".")
            .get("workflow_class")
            .and_then(Value::as_str),
        Some("code")
    );
}

#[test]
fn code_patch_contract_includes_verification_command_capability() {
    let caps = automation_tool_capability_ids(&code_patch_contract_node(), "git_patch");
    assert!(
        caps.contains(&"verify_command".to_string()),
        "code_patch contract should require verify_command in patch mode, got: {caps:?}"
    );
}

#[test]
fn code_patch_contract_enforcement_defaults_require_reads_and_prewrite_gates() {
    let enforcement = automation_node_output_enforcement(&code_patch_contract_node());
    assert_eq!(
        enforcement.validation_profile.as_deref(),
        Some("code_change")
    );
    assert!(enforcement.required_tools.iter().any(|tool| tool == "read"));
    assert!(enforcement
        .required_evidence
        .iter()
        .any(|value| value == "local_source_reads"));
    assert!(enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "workspace_inspection"));
    assert!(enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "concrete_reads"));
}

#[test]
fn code_patch_contract_requires_verification_before_completion() {
    let node = code_patch_contract_node();
    let tool_telemetry = json!({
        "verification_expected": true,
        "verification_ran": false
    });
    assert_eq!(
        detect_automation_node_failure_kind(&node, "blocked", None, None, None).as_deref(),
        None
    );
    assert_eq!(
        detect_automation_node_failure_kind(
            &node,
            "blocked",
            Some(false),
            None,
            Some(&json!({"verification_expected": true, "verification_ran": false}))
        )
        .as_deref(),
        Some("verification_missing")
    );
    assert_eq!(
        detect_automation_blocker_category(&node, "blocked", None, &tool_telemetry, None,),
        Some("verification_required".to_string())
    );
}

#[test]
fn local_citations_contract_defaults_to_local_research_not_external_research() {
    let enforcement = automation_node_output_enforcement(&local_citations_contract_node());
    assert_eq!(
        enforcement.validation_profile.as_deref(),
        Some("local_research")
    );
    assert!(enforcement.required_tools.iter().any(|tool| tool == "glob"));
    assert!(enforcement.required_tools.iter().any(|tool| tool == "read"));
    assert!(enforcement
        .required_evidence
        .iter()
        .any(|value| value == "local_source_reads"));
    assert!(enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "workspace_inspection"));
}

#[test]
fn external_research_prewrite_does_not_require_workspace_inspection_from_offered_glob() {
    let mut node = bare_node();
    node.node_id = "gather_market_sources".to_string();
    node.objective = "Use web_research and web_fetch to gather current market sources.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "citations".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: Some(crate::AutomationOutputEnforcement {
            validation_profile: Some("external_research".to_string()),
            required_tools: vec!["websearch".to_string()],
            required_tool_calls: Vec::new(),
            required_evidence: Vec::new(),
            required_sections: vec!["web_sources_reviewed".to_string()],
            prewrite_gates: vec!["successful_web_research".to_string()],
            retry_on_missing: vec!["missing_successful_web_research".to_string()],
            terminal_on: vec!["completed".to_string()],
            repair_budget: Some(2),
            session_text_recovery: None,
        }),
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/gather-market-sources.json"
        }
    }));

    let requirements = automation_node_prewrite_requirements_impl(
        &node,
        &[
            "glob".to_string(),
            "websearch".to_string(),
            "write".to_string(),
        ],
    )
    .expect("prewrite requirements");

    assert!(!requirements.workspace_inspection_required);
    assert!(requirements.web_research_required);
    assert!(requirements.successful_web_research_required);
}

#[test]
fn connector_scoped_nodes_keep_required_web_research_tools() {
    let mut node = bare_node();
    node.node_id = "draft_productivity_signals_brief".to_string();
    node.objective =
        "Synthesize Reddit findings and supporting web citations into one concise brief."
            .to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "brief".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
        enforcement: Some(crate::AutomationOutputEnforcement {
            validation_profile: Some("external_research".to_string()),
            required_tools: vec!["websearch".to_string()],
            required_tool_calls: Vec::new(),
            required_evidence: vec!["external_sources".to_string()],
            required_sections: vec!["citations".to_string()],
            prewrite_gates: vec!["successful_web_research".to_string()],
            retry_on_missing: Vec::new(),
            terminal_on: Vec::new(),
            repair_budget: Some(2),
            session_text_recovery: None,
        }),
        schema: None,
        summary_guidance: None,
    });
    let available_tool_names = std::collections::HashSet::from([
        "mcp_list".to_string(),
        "mcp.reddit_gmail.reddit_search_across_subreddits".to_string(),
        "websearch".to_string(),
        "webfetch".to_string(),
        "write".to_string(),
    ]);

    let requested = automation_requested_tools_for_node(
        &node,
        "/tmp",
        vec!["mcp.reddit_gmail.*".to_string(), "write".to_string()],
        &available_tool_names,
    );

    assert!(
        requested.iter().any(|tool| tool == "websearch"),
        "required websearch must not be dropped by connector-source policy: {requested:?}"
    );
    assert!(
        requested.iter().any(|tool| tool == "webfetch"),
        "required webfetch must not be dropped by connector-source policy: {requested:?}"
    );
}

#[test]
fn connector_scoped_nodes_keep_required_email_send_tools() {
    let mut node = email_delivery_node();
    node.node_id = "send_productivity_signals_brief".to_string();
    node.objective =
        "Use Reddit context and send the finalized brief to recipient@example.com.".to_string();
    let available_tool_names = std::collections::HashSet::from([
        "mcp_list".to_string(),
        "mcp.reddit_gmail.reddit_search_across_subreddits".to_string(),
        "send_email".to_string(),
        "write".to_string(),
    ]);

    let requested = automation_requested_tools_for_node(
        &node,
        "/tmp",
        vec!["mcp.reddit_gmail.*".to_string(), "write".to_string()],
        &available_tool_names,
    );

    assert!(
        requested.iter().any(|tool| tool == "send_email"),
        "required email send tool must not be dropped by connector-source policy: {requested:?}"
    );
    assert!(
        !requested.iter().any(|tool| tool == "create_email_draft"),
        "send-only email node should not offer draft tools by default: {requested:?}"
    );
}

#[test]
fn gmail_draft_only_nodes_do_not_offer_send_tools() {
    let mut node = email_delivery_node();
    node.node_id = "create-gmail-draft".to_string();
    node.objective =
        "Create a Gmail draft from the composed test email and prepare to send to evan@example.com"
            .to_string();
    let available_tool_names = std::collections::HashSet::from([
        "mcp.poop.gmail_create_email_draft".to_string(),
        "mcp.poop.gmail_update_draft".to_string(),
        "mcp.poop.gmail_send_draft".to_string(),
        "mcp.poop.gmail_send_email".to_string(),
        "mcp_list".to_string(),
        "write".to_string(),
    ]);

    let caps = automation_tool_capability_ids(&node, "artifact_write");
    assert!(caps.contains(&"email_draft".to_string()));
    assert!(!caps.contains(&"email_send".to_string()));

    let requested = automation_requested_tools_for_node(
        &node,
        "/tmp",
        vec!["mcp.poop.*".to_string(), "write".to_string()],
        &available_tool_names,
    );

    assert!(
        requested
            .iter()
            .any(|tool| tool == "mcp.poop.gmail_create_email_draft"),
        "draft-only node should keep concrete draft creation tools: {requested:?}"
    );
    assert!(
        requested
            .iter()
            .any(|tool| tool == "mcp.poop.gmail_update_draft"),
        "draft-only node should keep concrete draft update tools: {requested:?}"
    );
    assert!(
        !requested
            .iter()
            .any(|tool| tool == "mcp.poop.gmail_send_email" || tool == "mcp.poop.gmail_send_draft"),
        "draft-only node must not offer send tools before approval: {requested:?}"
    );
}

#[test]
fn explicit_gmail_draft_node_allowlist_blocks_inferred_send_tools() {
    let mut node = email_delivery_node();
    let namespace = "user_named_mailbox";
    let draft_tool = format!("mcp.{namespace}.gmail_create_email_draft");
    let get_draft_tool = format!("mcp.{namespace}.gmail_get_draft");
    let list_drafts_tool = format!("mcp.{namespace}.gmail_list_drafts");
    let send_draft_tool = format!("mcp.{namespace}.gmail_send_draft");
    let send_email_tool = format!("mcp.{namespace}.gmail_send_email");
    node.node_id = "create-gmail-draft".to_string();
    node.objective = format!("Create a Gmail draft from compose-email output using the concrete Gmail draft capability discovered by action name, preferably {draft_tool}. Do not send the email. If send-draft is unavailable, block with a clear reason listing available Gmail tools.");
    node.metadata = Some(json!({
        "tool_allowlist": [
            "read",
            "write",
            "mcp_list",
            draft_tool,
            get_draft_tool,
            list_drafts_tool
        ],
        "builder": {
            "output_path": ".tandem/artifacts/gmail-draft-approval/create-gmail-draft.json"
        }
    }));
    let available_tool_names = std::collections::HashSet::from([
        draft_tool.clone(),
        get_draft_tool.clone(),
        list_drafts_tool.clone(),
        send_draft_tool.clone(),
        send_email_tool.clone(),
        "mcp_list".to_string(),
        "read".to_string(),
        "write".to_string(),
    ]);

    let caps = automation_tool_capability_ids(&node, "artifact_write");
    assert!(caps.contains(&"email_draft".to_string()));
    assert!(!caps.contains(&"email_send".to_string()));

    let requested = automation_requested_tools_for_node(
        &node,
        "/tmp",
        vec![
            "read".to_string(),
            "write".to_string(),
            "mcp_list".to_string(),
            draft_tool.clone(),
            get_draft_tool.clone(),
            list_drafts_tool.clone(),
            send_draft_tool.clone(),
            send_email_tool.clone(),
        ],
        &available_tool_names,
    );

    assert!(requested.iter().any(|tool| tool == &draft_tool));
    assert!(!requested
        .iter()
        .any(|tool| tool == &send_draft_tool || tool == &send_email_tool));
}

#[test]
fn explicit_draft_tool_policy_overrides_approval_send_wording() {
    let mut node = email_delivery_node();
    node.node_id = "create-gmail-draft".to_string();
    node.objective =
        "Create the Gmail draft now. It may be sent only after human approval.".to_string();
    node.metadata = Some(json!({
        "delivery": {
            "method": "email",
            "to": "recipient@example.com"
        },
        "tool_allowlist": [
            "write",
            "mcp.reddit_gmail.gmail_create_email_draft"
        ]
    }));
    let available_tool_names = std::collections::HashSet::from([
        "mcp.reddit_gmail.gmail_create_email_draft".to_string(),
        "mcp.reddit_gmail.gmail_send_draft".to_string(),
        "mcp.reddit_gmail.gmail_send_email".to_string(),
        "mcp_list".to_string(),
        "write".to_string(),
    ]);

    let caps = automation_tool_capability_ids(&node, "artifact_write");
    assert!(caps.contains(&"email_draft".to_string()));
    assert!(!caps.contains(&"email_send".to_string()));

    let requested = automation_requested_tools_for_node(
        &node,
        "/tmp",
        vec![
            "mcp.reddit_gmail.*".to_string(),
            "mcp.reddit_gmail.gmail_send_draft".to_string(),
            "write".to_string(),
        ],
        &available_tool_names,
    );

    assert!(requested.contains(&"mcp.reddit_gmail.gmail_create_email_draft".to_string()));
    assert!(!requested.contains(&"mcp.reddit_gmail.gmail_send_draft".to_string()));
    assert!(!requested.contains(&"mcp.reddit_gmail.gmail_send_email".to_string()));
}

#[test]
fn node_tool_allowlist_overrides_broader_agent_policy() {
    let mut node = bare_node();
    node.node_id = "compose-email".to_string();
    node.metadata = Some(json!({
        "tool_allowlist": ["read", "write"]
    }));
    let available_tool_names = std::collections::HashSet::from([
        "mcp.reddit_gmail.gmail_create_email_draft".to_string(),
        "mcp.reddit_gmail.gmail_send_draft".to_string(),
        "mcp_list".to_string(),
        "read".to_string(),
        "write".to_string(),
    ]);

    let requested = automation_requested_tools_for_node(
        &node,
        "/tmp",
        vec![
            "read".to_string(),
            "write".to_string(),
            "mcp_list".to_string(),
            "mcp.reddit_gmail.gmail_create_email_draft".to_string(),
            "mcp.reddit_gmail.gmail_send_draft".to_string(),
        ],
        &available_tool_names,
    );

    assert_eq!(requested, vec!["read".to_string(), "write".to_string()]);
}

#[test]
fn node_first_class_mcp_policy_is_hard_tool_scope() {
    let mut node = bare_node();
    node.node_id = "send-approved-draft".to_string();
    node.tool_policy = Some(crate::AutomationAgentToolPolicy {
        allowlist: vec!["read".to_string()],
        denylist: Vec::new(),
    });
    node.mcp_policy = Some(crate::AutomationAgentMcpPolicy {
        allowed_servers: vec!["reddit-gmail".to_string()],
        allowed_tools: Some(vec!["mcp.reddit_gmail.gmail_send_draft".to_string()]),
        allowed_connections: Vec::new(),
    });
    let available_tool_names = std::collections::HashSet::from([
        "mcp.reddit_gmail.gmail_create_email_draft".to_string(),
        "mcp.reddit_gmail.gmail_send_draft".to_string(),
        "mcp.reddit_gmail.gmail_send_email".to_string(),
        "mcp_list".to_string(),
        "read".to_string(),
        "write".to_string(),
    ]);

    let requested = automation_requested_tools_for_node(
        &node,
        "/tmp",
        vec![
            "read".to_string(),
            "write".to_string(),
            "mcp_list".to_string(),
            "mcp.reddit_gmail.gmail_create_email_draft".to_string(),
            "mcp.reddit_gmail.gmail_send_draft".to_string(),
            "mcp.reddit_gmail.gmail_send_email".to_string(),
        ],
        &available_tool_names,
    );

    assert!(requested.contains(&"mcp.reddit_gmail.gmail_send_draft".to_string()));
    assert!(!requested.contains(&"mcp.reddit_gmail.gmail_create_email_draft".to_string()));
    assert!(!requested.contains(&"mcp.reddit_gmail.gmail_send_email".to_string()));
}

#[test]
fn node_empty_mcp_policy_suppresses_agent_mcp_preflight_scope() {
    let mut node = node_with_input_ref();
    node.node_id = "filter_agent_tool_security".to_string();
    node.objective = "Read the upstream Reddit artifact, filter leads, write JSON, and do not call external tools.".to_string();
    node.tool_policy = Some(crate::AutomationAgentToolPolicy {
        allowlist: vec!["read".to_string(), "write".to_string()],
        denylist: Vec::new(),
    });
    node.mcp_policy = Some(crate::AutomationAgentMcpPolicy {
        allowed_servers: Vec::new(),
        allowed_tools: None,
        allowed_connections: Vec::new(),
    });

    let agent = crate::AutomationAgentProfile {
        agent_id: "lead_analyst".to_string(),
        template_id: None,
        display_name: "Lead Analyst".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec![
                "read".to_string(),
                "write".to_string(),
                "mcp.composio_gmail.composio_multi_execute_tool".to_string(),
                "mcp.notion.notion_create_pages".to_string(),
            ],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: vec!["composio-gmail".to_string(), "notion".to_string()],
            allowed_tools: Some(vec![
                "mcp.composio_gmail.composio_multi_execute_tool".to_string(),
                "mcp.notion.notion_create_pages".to_string(),
            ]),
            allowed_connections: Vec::new(),
        },
        approval_policy: None,
    };
    let mut agent_allowlist = merge_automation_agent_allowlist(&agent, None);
    if let Some(mcp_tools) = agent.mcp_policy.allowed_tools.as_ref() {
        agent_allowlist.extend(mcp_tools.clone());
    }
    let available_tool_names = std::collections::HashSet::from([
        "mcp.composio_gmail.composio_multi_execute_tool".to_string(),
        "mcp.notion.notion_create_pages".to_string(),
        "mcp_list".to_string(),
        "read".to_string(),
        "write".to_string(),
    ]);

    let scope =
        node_runtime_impl::automation_node_mcp_preflight_scope(&node, &agent, &agent_allowlist);
    let requested =
        automation_requested_tools_for_node(&node, "/tmp", agent_allowlist, &available_tool_names);
    let offered =
        automation_add_mcp_list_when_scoped(requested, !scope.allowed_servers.is_empty());

    assert!(scope.allowed_servers.is_empty());
    assert_eq!(scope.allowlist, vec!["read".to_string(), "write".to_string()]);
    assert_eq!(offered, vec!["read".to_string(), "write".to_string()]);
}

#[test]
fn exact_node_mcp_policy_does_not_offer_discovery_tool() {
    let mut node = node_with_input_ref();
    node.node_id = "notion_agent_tool_security".to_string();
    node.objective =
        "Use filtered leads, call the exact Notion tools, then write the insertion artifact."
            .to_string();
    node.tool_policy = Some(crate::AutomationAgentToolPolicy {
        allowlist: vec![
            "write".to_string(),
            "mcp.notion.notion_fetch".to_string(),
            "mcp.notion.notion_search".to_string(),
            "mcp.notion.notion_create_pages".to_string(),
            "mcp.notion.notion_update_page".to_string(),
        ],
        denylist: Vec::new(),
    });
    node.mcp_policy = Some(crate::AutomationAgentMcpPolicy {
        allowed_servers: vec!["notion".to_string()],
        allowed_tools: Some(vec![
            "mcp.notion.notion_fetch".to_string(),
            "mcp.notion.notion_search".to_string(),
            "mcp.notion.notion_create_pages".to_string(),
            "mcp.notion.notion_update_page".to_string(),
        ]),
        allowed_connections: Vec::new(),
    });

    let agent = crate::AutomationAgentProfile {
        agent_id: "notion_writer".to_string(),
        template_id: None,
        display_name: "Notion Writer".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: vec![
                "write".to_string(),
                "mcp_list".to_string(),
                "mcp.composio_gmail.composio_multi_execute_tool".to_string(),
                "mcp.notion.notion_fetch".to_string(),
                "mcp.notion.notion_search".to_string(),
                "mcp.notion.notion_create_pages".to_string(),
                "mcp.notion.notion_update_page".to_string(),
            ],
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: vec!["composio-gmail".to_string(), "notion".to_string()],
            allowed_tools: Some(vec![
                "mcp.composio_gmail.composio_multi_execute_tool".to_string(),
                "mcp.notion.notion_fetch".to_string(),
                "mcp.notion.notion_search".to_string(),
                "mcp.notion.notion_create_pages".to_string(),
                "mcp.notion.notion_update_page".to_string(),
            ]),
            allowed_connections: Vec::new(),
        },
        approval_policy: None,
    };
    let mut agent_allowlist = merge_automation_agent_allowlist(&agent, None);
    if let Some(mcp_tools) = agent.mcp_policy.allowed_tools.as_ref() {
        agent_allowlist.extend(mcp_tools.clone());
    }
    let available_tool_names = std::collections::HashSet::from([
        "mcp.composio_gmail.composio_multi_execute_tool".to_string(),
        "mcp.notion.notion_fetch".to_string(),
        "mcp.notion.notion_search".to_string(),
        "mcp.notion.notion_create_pages".to_string(),
        "mcp.notion.notion_update_page".to_string(),
        "mcp_list".to_string(),
        "read".to_string(),
        "write".to_string(),
    ]);

    let scope =
        node_runtime_impl::automation_node_mcp_preflight_scope(&node, &agent, &agent_allowlist);
    let requested =
        automation_requested_tools_for_node(&node, "/tmp", agent_allowlist, &available_tool_names);
    let offered =
        automation_add_mcp_list_when_scoped(requested, !scope.allowed_servers.is_empty());

    assert_eq!(scope.allowed_servers, vec!["notion".to_string()]);
    assert!(offered.contains(&"mcp.notion.notion_fetch".to_string()));
    assert!(offered.contains(&"mcp.notion.notion_create_pages".to_string()));
    assert!(!offered.contains(&"mcp_list".to_string()));
    assert!(!offered.contains(&"mcp.composio_gmail.composio_multi_execute_tool".to_string()));
}

#[test]
fn incident_monitor_downstream_structured_json_nodes_reuse_upstream_source_evidence() {
    let mut inspection = bare_node();
    inspection.node_id = "inspect_failure_report".to_string();
    inspection.metadata = Some(json!({
        "incident_monitor": {
            "artifact_type": "incident_monitor_inspection"
        }
    }));

    let mut research = bare_node();
    research.node_id = "research_likely_root_cause".to_string();
    research.depends_on = vec!["inspect_failure_report".to_string()];
    research.metadata = Some(json!({
        "incident_monitor": {
            "artifact_type": "incident_monitor_research"
        }
    }));

    let mut validation = bare_node();
    validation.node_id = "validate_failure_scope".to_string();
    validation.depends_on = vec!["research_likely_root_cause".to_string()];
    validation.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    validation.metadata = Some(json!({
        "incident_monitor": {
            "artifact_type": "incident_monitor_validation"
        }
    }));

    assert!(!automation_node_uses_upstream_validation_evidence(
        &inspection
    ));
    assert!(!automation_node_uses_upstream_validation_evidence(
        &research
    ));
    assert!(automation_node_uses_upstream_validation_evidence(
        &validation
    ));
}

#[test]
fn mcp_citations_contract_defaults_to_artifact_only_without_local_read_gates() {
    let enforcement = automation_node_output_enforcement(&mcp_citations_contract_node());
    assert_eq!(
        enforcement.validation_profile.as_deref(),
        Some("artifact_only")
    );
    assert!(!enforcement.required_tools.iter().any(|tool| tool == "glob"));
    assert!(!enforcement.required_tools.iter().any(|tool| tool == "read"));
    assert!(!enforcement
        .required_evidence
        .iter()
        .any(|value| value == "local_source_reads"));
    assert!(!enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "workspace_inspection"));
    assert_eq!(enforcement.session_text_recovery.as_deref(), Some("allow"));
}

#[test]
fn concrete_mcp_row_inspection_does_not_infer_workspace_inspection_gate() {
    let mut node = bare_node();
    node.node_id = "inspect_notion_row".to_string();
    node.objective = "Fetch and inspect only the existing Notion database row at https://www.notion.so/f3975ce71d8d45318bea2812c65f209b inside Operational Workflow Results collection://892d3e9b-2bf8-4b3e-a541-dc725f77295d, confirming the target page/row identity and current editable fields. Do not create a database, top-level page, workspace page, or new database row.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/inspect-notion-row.json",
            "required_tools": ["mcp.notion.notion_fetch"],
            "optional_tools": ["mcp.notion.notion_search"],
            "task_class": "connector_read",
            "retry_class": "notion_read"
        }
    }));

    let enforcement = automation_node_output_enforcement(&node);

    assert_eq!(
        enforcement.validation_profile.as_deref(),
        Some("artifact_only")
    );
    assert!(
        enforcement
            .required_tools
            .iter()
            .any(|tool| tool == "mcp.notion.notion_fetch"),
        "the concrete MCP source tool should remain required"
    );
    assert!(
        !enforcement
            .required_tools
            .iter()
            .any(|tool| matches!(tool.as_str(), "glob" | "read" | "write")),
        "Notion connector row inspection must not infer local workspace file tools: {enforcement:#?}"
    );
    assert!(
        !enforcement
            .required_evidence
            .iter()
            .any(|evidence| evidence == "local_source_reads"),
        "connector evidence should not be treated as local filesystem evidence"
    );
    assert!(
        !enforcement
            .prewrite_gates
            .iter()
            .any(|gate| matches!(gate.as_str(), "workspace_inspection" | "concrete_reads")),
        "Notion connector row inspection must not require local workspace inspection: {enforcement:#?}"
    );
    assert_eq!(enforcement.session_text_recovery.as_deref(), Some("allow"));
}

#[test]
fn tandem_mcp_reference_node_does_not_require_web_research() {
    let mut node = bare_node();
    node.node_id = "gather_tandem_reference".to_string();
    node.objective = "Use Tandem MCP docs as reference if needed via mcp.tandem_mcp.search_docs, mcp.tandem_mcp.get_doc, mcp.tandem_mcp.get_tandem_guide, or mcp.tandem_mcp.answer_how_to to collect relevant Tandem guidance for reliable automation runs, workflow validation, approvals, connector use, and Tandem Run details. Return only relevant excerpts and citations; do not invent undocumented Tandem behavior.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "task_class": "connector_research",
            "web_research_expected": false
        },
        "tool_allowlist": [
            "mcp.tandem_mcp.search_docs",
            "mcp.tandem_mcp.get_doc",
            "mcp.tandem_mcp.get_tandem_guide",
            "mcp.tandem_mcp.answer_how_to"
        ]
    }));

    let caps = automation_tool_capability_ids(&node, "artifact_write");

    assert!(caps.contains(&"artifact_write".to_string()));
    assert!(
        !caps.contains(&"web_research".to_string()),
        "Tandem MCP docs are connector-backed source tools, not general web research: {caps:?}"
    );

    let available_tool_names = [
        "mcp_list".to_string(),
        "mcp.tandem_mcp.search_docs".to_string(),
        "mcp.tandem_mcp.get_doc".to_string(),
        "mcp.tandem_mcp.get_tandem_guide".to_string(),
        "mcp.tandem_mcp.answer_how_to".to_string(),
        "write".to_string(),
    ]
    .into_iter()
    .collect::<std::collections::HashSet<_>>();
    let resolution = automation_resolve_capabilities(
        &node,
        "artifact_write",
        &[
            "mcp.tandem_mcp.search_docs".to_string(),
            "mcp.tandem_mcp.get_doc".to_string(),
            "mcp.tandem_mcp.get_tandem_guide".to_string(),
            "mcp.tandem_mcp.answer_how_to".to_string(),
            "mcp_list".to_string(),
            "write".to_string(),
        ],
        &available_tool_names,
    );

    assert_eq!(
        automation_capability_resolution_missing_capabilities(&resolution),
        Vec::<String>::new(),
        "Tandem MCP source tools plus write should satisfy preflight: {resolution:#}"
    );
}

#[test]
fn tandem_mcp_citations_node_does_not_require_workspace_read_capability() {
    let mut node = bare_node();
    node.node_id = "gather_tandem_reference".to_string();
    node.objective = "Use Tandem MCP docs as reference material for reliability patterns in automated business workflows, including docs or guides about workflow design, validation, approvals, connector-backed work, retries, observability, and MCP/tool usage. Produce cited notes that can support the final report's Tandem Run details and reliability framing.".to_string();
    node.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "runtime_context".to_string(),
        alias: "runtime_context_partition".to_string(),
    }];
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "citations".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/gather-tandem-reference.json"
        }
    }));

    let caps = automation_tool_capability_ids(&node, "artifact_write");

    assert!(caps.contains(&"artifact_write".to_string()));
    assert!(
        !caps.contains(&"workspace_read".to_string()),
        "Tandem MCP citations nodes should use connector docs instead of requiring local read tools: {caps:?}"
    );
    assert!(
        !caps.contains(&"workspace_discover".to_string()),
        "Tandem MCP citations nodes should not require local workspace discovery: {caps:?}"
    );

    let available_tool_names = [
        "mcp_list".to_string(),
        "mcp.tandem_mcp.answer_how_to".to_string(),
        "mcp.tandem_mcp.compare_doc_page_refresh".to_string(),
        "mcp.tandem_mcp.compare_docs_index_refresh".to_string(),
        "mcp.tandem_mcp.get_doc".to_string(),
        "mcp.tandem_mcp.get_docs_cache_status".to_string(),
        "mcp.tandem_mcp.get_start_path".to_string(),
        "mcp.tandem_mcp.get_tandem_guide".to_string(),
        "mcp.tandem_mcp.invalidate_docs_cache".to_string(),
        "mcp.tandem_mcp.recommend_next_docs".to_string(),
        "mcp.tandem_mcp.refresh_doc_page".to_string(),
        "mcp.tandem_mcp.refresh_docs_index".to_string(),
        "mcp.tandem_mcp.search_docs".to_string(),
        "mcp.tandem_mcp.warmup_docs_cache".to_string(),
        "write".to_string(),
    ]
    .into_iter()
    .collect::<std::collections::HashSet<_>>();
    let offered_tools = available_tool_names.iter().cloned().collect::<Vec<_>>();
    let resolution = automation_resolve_capabilities(
        &node,
        "artifact_write",
        &offered_tools,
        &available_tool_names,
    );

    assert_eq!(
        automation_capability_resolution_missing_capabilities(&resolution),
        Vec::<String>::new(),
        "Tandem MCP tools plus write should satisfy capability preflight without workspace_read: {resolution:#}"
    );
}

#[test]
fn optional_tandem_mcp_reference_does_not_prompt_for_required_connector_source() {
    let mut node = bare_node();
    node.node_id = "gather_tandem_reference".to_string();
    node.objective = "Use Tandem MCP docs as reference if needed via mcp.tandem_mcp.search_docs, mcp.tandem_mcp.get_doc, mcp.tandem_mcp.get_tandem_guide, or mcp.tandem_mcp.answer_how_to to collect relevant Tandem guidance for reliable automation runs, workflow validation, approvals, connector use, and Tandem Run details. Return only relevant excerpts and citations; do not invent undocumented Tandem behavior.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/gather-tandem-reference.json"
        },
        "tool_allowlist": [
            "mcp.tandem_mcp.search_docs",
            "mcp.tandem_mcp.get_doc",
            "mcp.tandem_mcp.get_tandem_guide",
            "mcp.tandem_mcp.answer_how_to"
        ]
    }));
    let automation = automation_with_output_targets(vec![node.clone()], Vec::new());
    let agent = crate::AutomationAgentProfile {
        agent_id: "a1".to_string(),
        template_id: None,
        display_name: "Docs Researcher".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: Vec::new(),
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: vec!["tandem-mcp".to_string()],
            allowed_tools: None,
            allowed_connections: Vec::new(),
        },
        approval_policy: None,
    };
    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp/workspace",
        "run-optional-mcp",
        &node,
        1,
        &agent,
        &[],
        &[
            "mcp_list".to_string(),
            "mcp.tandem_mcp.search_docs".to_string(),
            "mcp.tandem_mcp.get_doc".to_string(),
            "mcp.tandem_mcp.get_tandem_guide".to_string(),
            "mcp.tandem_mcp.answer_how_to".to_string(),
            "write".to_string(),
        ],
        None,
        None,
        None,
    );

    assert!(
        prompt.contains("These connector tools are optional for this objective"),
        "{prompt}"
    );
    assert!(
        !prompt.contains("Call at least one concrete source tool before writing"),
        "{prompt}"
    );
    assert!(
        !prompt.contains("call at least one concrete `mcp.*` source tool"),
        "{prompt}"
    );
}

#[test]
fn explicit_input_files_ignore_mcp_tool_identifiers() {
    let mut node = bare_node();
    node.metadata = Some(json!({
        "builder": {
            "input_files": [
                "mcp.notion.notion_create_pages",
                "data/filtered-leads.json"
            ]
        }
    }));
    let automation = automation_with_output_targets(vec![node.clone()], Vec::new());

    let input_files = automation_node_effective_input_files_for_automation(
        &automation,
        &node,
        None,
    );

    assert_eq!(input_files, vec!["data/filtered-leads.json".to_string()]);
}

#[test]
fn prompt_does_not_treat_mcp_tool_ids_as_concrete_source_files() {
    let mut node = bare_node();
    node.node_id = "notion_local_llm_privacy".to_string();
    node.objective = "Read the single filtered leads artifact before doing anything else. If leads is empty or missing, do not call mcp.notion.notion_search and do not call mcp.notion.notion_create_pages; write exactly one JSON artifact and stop. If leads is non-empty, insert those leads into the Notion data source.".to_string();
    node.metadata = Some(json!({
        "required_tools": [
            "mcp.notion.notion_search",
            "mcp.notion.notion_create_pages"
        ]
    }));
    let automation = automation_with_output_targets(vec![node.clone()], Vec::new());
    let upstream_inputs = vec![
        json!({
            "alias": "filtered_leads",
            "from_step_id": "filter_agent_tool_security",
            "output": {
                "content": {
                    "path": ".tandem/runs/run-notion/artifacts/filter-agent-tool-security.json"
                },
                "artifact_validation": {
                    "accepted_artifact_path": ".tandem/runs/run-notion/artifacts/filter-agent-tool-security.json"
                }
            }
        }),
        json!({
            "alias": "filtered_leads_data_path",
            "from_step_id": "filter_agent_tool_security_data_path",
            "output": {
                "content": {
                    "data": {
                        "path": ".tandem/runs/run-notion/artifacts/filter-agent-tool-security-data.json"
                    }
                }
            }
        }),
        json!({
            "alias": "filtered_leads_root_path",
            "from_step_id": "filter_agent_tool_security_root_path",
            "output": {
                "path": ".tandem/runs/run-notion/artifacts/filter-agent-tool-security-root.json"
            }
        }),
    ];
    let agent = crate::AutomationAgentProfile {
        agent_id: "a1".to_string(),
        template_id: None,
        display_name: "Notion Writer".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: Vec::new(),
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: vec!["notion".to_string()],
            allowed_tools: None,
            allowed_connections: Vec::new(),
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp/workspace",
        "run-notion",
        &node,
        1,
        &agent,
        &upstream_inputs,
        &[
            "mcp_list".to_string(),
            "mcp.notion.notion_search".to_string(),
            "mcp.notion.notion_create_pages".to_string(),
            "read".to_string(),
            "write".to_string(),
        ],
        None,
        None,
        None,
    );

    assert!(
        prompt.contains("- `.tandem/runs/run-notion/artifacts/filter-agent-tool-security.json`"),
        "{prompt}"
    );
    assert!(
        prompt.contains("- `.tandem/runs/run-notion/artifacts/filter-agent-tool-security-data.json`"),
        "{prompt}"
    );
    assert!(
        prompt.contains("- `.tandem/runs/run-notion/artifacts/filter-agent-tool-security-root.json`"),
        "{prompt}"
    );
    assert!(
        !prompt.contains("Concrete files for this node:\n- `mcp.notion.notion_create_pages`"),
        "{prompt}"
    );
    assert!(
        prompt.contains("These are action tools, not source evidence"),
        "{prompt}"
    );
    assert!(
        prompt.contains("Do not call connector action tools with empty payloads"),
        "{prompt}"
    );
    assert!(
        !prompt.contains("Call at least one concrete source tool before writing"),
        "{prompt}"
    );
    assert!(
        !prompt.contains("Read-only files for this node:\n- `mcp.notion.notion_create_pages`"),
        "{prompt}"
    );
}

#[test]
fn capability_ids_optional_web_context_offers_web_without_requiring_research_gate() {
    let mut node = bare_node();
    node.node_id = "gather_supporting_context".to_string();
    node.objective = "Use web research and web_fetch only when useful to add supporting context for tools, market references, or claims that emerged from collect_reddit_signals. Do not replace Reddit as the primary evidence source. Return concise citations; if no web context is needed, return an empty citations list with rationale.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "citations".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/gather-supporting-context.json"
        }
    }));

    let caps = automation_tool_capability_ids(&node, "artifact_write");
    let enforcement = automation_node_output_enforcement(&node);

    assert!(
        !caps.contains(&"web_research".to_string()),
        "optional web context should offer web tools without making web research a hard preflight capability"
    );
    assert!(
        !enforcement
            .required_tools
            .iter()
            .any(|tool| tool == "websearch"),
        "optional web context should not make websearch a required tool"
    );
    assert!(
        !enforcement
            .required_evidence
            .iter()
            .any(|evidence| evidence == "external_sources"),
        "optional web context should not require external source evidence"
    );
    assert!(
        !enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "successful_web_research"),
        "optional web context should not install a successful-web-research gate"
    );

    let requested = normalize_automation_requested_tools(
        &node,
        ".",
        vec!["web_research".to_string(), "web_fetch".to_string()],
    );
    assert!(requested.iter().any(|tool| tool == "websearch"));
    assert!(requested.iter().any(|tool| tool == "webfetch"));

    let no_web_tools_available = ["read".to_string(), "write".to_string()]
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    let resolution = automation_resolve_capabilities(
        &node,
        "artifact_write",
        &["read".to_string(), "write".to_string()],
        &no_web_tools_available,
    );
    assert_eq!(
        automation_capability_resolution_missing_capabilities(&resolution),
        Vec::<String>::new(),
        "optional web context must not fail preflight when web tools are not offered: {resolution:#}"
    );
}

#[test]
fn report_writer_from_upstream_artifacts_does_not_require_local_reads() {
    let mut node = bare_node();
    node.node_id = "draft_final_report".to_string();
    node.objective = "Draft the final report body for the existing Notion row. Use the synthesized findings, cite sources clearly, and include Tandem Run details.".to_string();
    node.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "synthesize_reliability_approaches".to_string(),
        alias: "synthesis".to_string(),
    }];
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "brief".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
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
            terminal_on: vec![
                "tool_unavailable".to_string(),
                "repair_budget_exhausted".to_string(),
            ],
            repair_budget: Some(5),
            session_text_recovery: Some("require_prewrite_satisfied".to_string()),
        }),
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "task_class": "report_writing",
            "task_kind": "delivery",
            "retry_class": "artifact_revision"
        }
    }));

    let enforcement = automation_node_output_enforcement(&node);

    assert_eq!(
        enforcement.validation_profile.as_deref(),
        Some("research_synthesis")
    );
    assert!(!enforcement.required_tools.iter().any(|tool| tool == "read"));
    assert!(!enforcement
        .required_evidence
        .iter()
        .any(|item| item == "local_source_reads"));
    assert!(!enforcement
        .prewrite_gates
        .iter()
        .any(|gate| gate == "concrete_reads"));
}

#[test]
fn synthesis_from_upstream_web_artifact_does_not_require_web_research_capability() {
    let mut node = bare_node();
    node.node_id = "synthesize_report".to_string();
    node.objective = "Synthesize the Tandem MCP reference notes, Reddit MCP findings, and current web research into a final report body for the existing Notion row. The report must include exactly these major sections: Summary, Key Findings, Market Notes, Reddit Signals, Sources, and Tandem Run details. The Sources section must consolidate web, Reddit, and Tandem documentation references; Tandem Run details must describe the tools/connectors used and the update target constraints.".to_string();
    node.depends_on = vec![
        "gather_tandem_reference".to_string(),
        "gather_reddit_signals".to_string(),
        "gather_web_sources".to_string(),
        "inspect_notion_row".to_string(),
    ];
    node.input_refs = vec![
        AutomationFlowInputRef {
            from_step_id: "gather_tandem_reference".to_string(),
            alias: "tandem_reference".to_string(),
        },
        AutomationFlowInputRef {
            from_step_id: "gather_reddit_signals".to_string(),
            alias: "reddit_signals".to_string(),
        },
        AutomationFlowInputRef {
            from_step_id: "gather_web_sources".to_string(),
            alias: "web_sources".to_string(),
        },
        AutomationFlowInputRef {
            from_step_id: "inspect_notion_row".to_string(),
            alias: "notion_target".to_string(),
        },
    ];
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "brief".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/synthesize-report.md"
        }
    }));

    let enforcement = automation_node_output_enforcement(&node);
    assert_eq!(
        enforcement.validation_profile.as_deref(),
        Some("research_synthesis")
    );
    assert!(
        !enforcement
            .required_tools
            .iter()
            .any(|tool| tool == "websearch"),
        "synthesis should consume upstream web artifacts instead of requiring fresh websearch: {enforcement:#?}"
    );
    assert!(
        !enforcement
            .required_evidence
            .iter()
            .any(|item| item == "external_sources"),
        "synthesis should not require fresh external source collection: {enforcement:#?}"
    );
    assert!(
        !enforcement
            .prewrite_gates
            .iter()
            .any(|gate| gate == "successful_web_research"),
        "synthesis should not require fresh successful web research gate: {enforcement:#?}"
    );

    let available_tool_names = [
        "mcp_list".to_string(),
        "mcp.notion.notion_fetch".to_string(),
        "mcp.reddit_gmail.reddit_search_across_subreddits".to_string(),
        "mcp.tandem_mcp.answer_how_to".to_string(),
        "write".to_string(),
    ]
    .into_iter()
    .collect::<std::collections::HashSet<_>>();
    let offered_tools = available_tool_names.iter().cloned().collect::<Vec<_>>();
    let resolution = automation_resolve_capabilities(
        &node,
        "artifact_write",
        &offered_tools,
        &available_tool_names,
    );

    assert_eq!(
        automation_capability_resolution_missing_capabilities(&resolution),
        Vec::<String>::new(),
        "synthesis from upstream artifacts should not fail when web tools are not offered: {resolution:#}"
    );
}

#[test]
fn auto_cleaned_marker_file_rejection_is_downgraded_when_output_is_valid() {
    assert!(super::should_downgrade_auto_cleaned_marker_rejection(
        Some("undeclared marker files created: .tandem_ack"),
        true,
        None,
        true
    ));
    assert!(!super::should_downgrade_auto_cleaned_marker_rejection(
        Some("undeclared marker files created: .tandem_ack"),
        false,
        None,
        true
    ));
    assert!(!super::should_downgrade_auto_cleaned_marker_rejection(
        Some("undeclared marker files created: .tandem_ack"),
        true,
        Some("no_concrete_reads"),
        true
    ));
    assert!(!super::should_downgrade_auto_cleaned_marker_rejection(
        Some("other rejection"),
        true,
        None,
        true
    ));
}

#[test]
fn capability_ids_output_is_sorted_and_deduplicated() {
    let node = node_with_input_ref();
    let caps = automation_tool_capability_ids(&node, "research");
    let mut sorted = caps.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(
        caps, sorted,
        "capability ids must be sorted and deduplicated"
    );
}

#[test]
fn capability_resolution_expands_wildcard_offered_email_tools() {
    let node = email_delivery_node();
    let available_tool_names = [
        "read".to_string(),
        "glob".to_string(),
        "mcp.composio_1.gmail_send_email".to_string(),
        "mcp.composio_1.gmail_create_email_draft".to_string(),
    ]
    .into_iter()
    .collect::<std::collections::HashSet<_>>();
    let resolution = automation_resolve_capabilities(
        &node,
        "artifact_write",
        &["mcp.composio_1.*".to_string()],
        &available_tool_names,
    );

    let offered_send_tools = resolution
        .get("email_tool_diagnostics")
        .and_then(|value| value.get("offered_send_tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let offered_draft_tools = resolution
        .get("email_tool_diagnostics")
        .and_then(|value| value.get("offered_draft_tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();

    assert!(offered_send_tools
        .iter()
        .any(|value| { value.as_str() == Some("mcp.composio_1.gmail_send_email") }));
    assert!(offered_draft_tools
        .iter()
        .any(|value| { value.as_str() == Some("mcp.composio_1.gmail_create_email_draft") }));
}

#[test]
fn capability_resolution_uses_metadata_for_unknown_tool_names() {
    let node = code_patch_contract_node();
    let available_tool_schemas = vec![
        ToolSchema::new("workspace_inspector", "", json!({})).with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Read)
                .domain(ToolDomain::Workspace)
                .reads_workspace(),
        ),
        ToolSchema::new("workspace_searcher", "", json!({})).with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Search)
                .domain(ToolDomain::Workspace)
                .reads_workspace()
                .preferred_for_discovery(),
        ),
        ToolSchema::new("workspace_writer", "", json!({})).with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Write)
                .domain(ToolDomain::Workspace)
                .writes_workspace()
                .requires_verification(),
        ),
        ToolSchema::new("run_local_checks", "", json!({})).with_capabilities(
            ToolCapabilities::new()
                .effect(ToolEffect::Execute)
                .domain(ToolDomain::Shell),
        ),
    ];
    let available_tool_names = available_tool_schemas
        .iter()
        .map(|schema| schema.name.clone())
        .collect::<std::collections::HashSet<_>>();
    let resolution = automation_resolve_capabilities_with_schemas(
        &node,
        "git_patch",
        &available_tool_names.iter().cloned().collect::<Vec<_>>(),
        &available_tool_names,
        &available_tool_schemas,
    );

    assert_eq!(
        resolution["resolved"]["workspace_read"]["status"].as_str(),
        Some("resolved")
    );
    assert_eq!(
        resolution["resolved"]["workspace_discover"]["status"].as_str(),
        Some("resolved")
    );
    assert_eq!(
        resolution["resolved"]["artifact_write"]["status"].as_str(),
        Some("resolved")
    );
    assert_eq!(
        resolution["resolved"]["verify_command"]["status"].as_str(),
        Some("resolved")
    );
}

// -----------------------------------------------------------------------
// normalize_upstream_research_output_paths
// -----------------------------------------------------------------------
