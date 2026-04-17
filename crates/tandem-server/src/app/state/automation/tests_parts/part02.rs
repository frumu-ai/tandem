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
    assert!(must_write_files
        .iter()
        .any(|path| path == "daily-recaps/2026-04-09-job-search-recap.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "opportunities/ranked/2026-04-09-ranked-opportunities.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "opportunities/shortlisted/2026-04-09-shortlist.md"));
    assert!(must_write_files
        .iter()
        .any(|path| path == "tracker/pipeline.md"));
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
        retry_policy: None,
        timeout_ms: None,
        max_tool_calls: None,
        stage_kind: None,
        gate: None,
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
fn capability_ids_bare_node_empty() {
    let node = bare_node();
    let caps = automation_tool_capability_ids(&node, "research");
    assert!(
        caps.is_empty(),
        "bare node should yield no capabilities, got: {caps:?}"
    );
}

#[test]
fn capability_ids_node_with_input_ref_includes_workspace_read() {
    let node = node_with_input_ref();
    let caps = automation_tool_capability_ids(&node, "research");
    assert!(caps.contains(&"workspace_read".to_string()));
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

#[test]
fn normalize_upstream_paths_passthrough_when_no_content_key() {
    let output = json!({ "summary": "hello" });
    let result = normalize_upstream_research_output_paths("/workspace", None, &output);
    assert_eq!(
        result, output,
        "output with no 'content' key should be returned unchanged"
    );
}

#[test]
fn normalize_upstream_paths_survives_empty_handoff() {
    let output = json!({
        "content": {
            "text": "some text",
            "structured_handoff": {}
        }
    });
    let result = normalize_upstream_research_output_paths("/workspace", None, &output);
    assert!(result.is_object(), "result should still be a JSON object");
}

#[test]
fn normalize_upstream_paths_scopes_tandem_artifacts_for_run() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-upstream-run-scoped-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join(".tandem/runs/run-123/artifacts"))
        .expect("create artifacts");
    std::fs::write(
        workspace_root.join(".tandem/runs/run-123/artifacts/report.md"),
        "report",
    )
    .expect("write artifact");
    let output = json!({
        "content": {
            "structured_handoff": {
                "files_reviewed": [".tandem/artifacts/report.md"]
            }
        }
    });
    let result = normalize_upstream_research_output_paths(
        workspace_root.to_str().expect("workspace"),
        Some("run-123"),
        &output,
    );
    assert_eq!(
        result.pointer("/content/structured_handoff/files_reviewed/0"),
        Some(&json!(".tandem/runs/run-123/artifacts/report.md"))
    );
    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn required_output_path_scopes_shared_artifacts_for_run() {
    let mut node = bare_node();
    node.node_id = "generate_report".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "report_markdown".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/generate-report.md"
        }
    }));

    assert_eq!(
        automation_node_required_output_path_for_run(&node, Some("run-iso")),
        Some(".tandem/runs/run-iso/artifacts/generate-report.md".to_string())
    );
    assert_eq!(
        automation_node_required_output_path_for_run(&node, None),
        Some(".tandem/artifacts/generate-report.md".to_string())
    );
}

#[test]
fn required_output_path_with_runtime_resolves_legacy_timestamp_templates() {
    let mut node = bare_node();
    node.node_id = "finalize_outputs".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "report_markdown".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": "reports/agent_automation_painpoints_YYYY-MM-DD_HH-MM-SS.md"
        }
    }));

    assert_eq!(
        automation_node_required_output_path_with_runtime_for_run(
            &node,
            Some("run-ts"),
            Some(&runtime_values("2026-04-17", "1024", "2026-04-17 10:24")),
        ),
        Some("reports/agent_automation_painpoints_2026-04-17_10-24-00.md".to_string())
    );
}

#[test]
fn runtime_placeholder_replace_supports_legacy_timestamp_tokens() {
    let replaced = automation_runtime_placeholder_replace(
        "reports/run_YYYY-MM-DD_HH-MM-SS.md and logs/YYYYMMDD_HHMMSS.json",
        Some(&runtime_values("2026-04-17", "1024", "2026-04-17 10:24")),
    );

    assert_eq!(
        replaced,
        "reports/run_2026-04-17_10-24-00.md and logs/20260417_102400.json"
    );
}

#[test]
fn session_write_materialized_output_detects_run_scoped_artifact_files() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-current-attempt-output-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-123";
    let artifact_path = workspace_root.join(".tandem/runs/run-123/artifacts/report.md");
    std::fs::create_dir_all(
        artifact_path
            .parent()
            .expect("artifact path should have parent"),
    )
    .expect("create artifacts dir");
    std::fs::write(&artifact_path, "report body").expect("write artifact");

    let mut session = Session::new(Some("write evidence".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": ".tandem/artifacts/report.md",
                "content": "report body"
            }),
            result: Some(json!({"output": "written"})),
            error: None,
        }],
    ));

    assert!(session_write_materialized_output_for_output(
        &session,
        workspace_root.to_str().expect("workspace root"),
        ".tandem/artifacts/report.md",
        Some(run_id),
        None,
    ));

    std::fs::remove_file(&artifact_path).expect("remove artifact");

    assert!(!session_write_materialized_output_for_output(
        &session,
        workspace_root.to_str().expect("workspace root"),
        ".tandem/artifacts/report.md",
        Some(run_id),
        None,
    ));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_waits_for_late_file_visibility() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-verified-output-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-reconcile";
    let output_path = ".tandem/artifacts/report.md";
    let resolved_path = workspace_root.join(".tandem/runs/run-reconcile/artifacts/report.md");
    std::fs::create_dir_all(
        resolved_path
            .parent()
            .expect("artifact path should have parent"),
    )
    .expect("create artifacts dir");

    let mut session = Session::new(Some("reconcile visibility".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": output_path,
                "content": "report body"
            }),
            result: Some(json!({"output": "written"})),
            error: None,
        }],
    ));

    let writer_root = workspace_root.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(60));
        std::fs::write(
            writer_root.join(".tandem/runs/run-reconcile/artifacts/report.md"),
            "report body",
        )
        .expect("write delayed artifact");
    });

    let resolved = super::reconcile_automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace root"),
        run_id,
        &AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "generate_report".to_string(),
            agent_id: "writer".to_string(),
            objective: "Generate report".to_string(),
            depends_on: vec![],
            input_refs: vec![],
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
                    "output_path": output_path
                }
            })),
        },
        output_path,
        300,
        25,
    )
    .await
    .expect("resolve after delay");

    writer.join().expect("writer thread");
    assert_eq!(resolved.map(|value| value.path), Some(resolved_path));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_times_out_when_file_never_appears() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-verified-output-timeout-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-timeout";
    let output_path = ".tandem/artifacts/report.md";
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let session = Session::new(Some("reconcile timeout".to_string()), None);
    let resolved = super::reconcile_automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace root"),
        run_id,
        &AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "generate_report".to_string(),
            agent_id: "writer".to_string(),
            objective: "Generate report".to_string(),
            depends_on: vec![],
            input_refs: vec![],
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
                    "output_path": output_path
                }
            })),
        },
        output_path,
        50,
        10,
    )
    .await
    .expect("resolve timeout");

    assert!(resolved.is_none());

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_marks_stale_existing_run_output_as_not_current_attempt() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-stale-existing-output-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-stale-existing";
    let output_path = ".tandem/artifacts/report.md";
    let resolved_path = workspace_root.join(".tandem/runs/run-stale-existing/artifacts/report.md");
    std::fs::create_dir_all(
        resolved_path
            .parent()
            .expect("artifact path should have parent"),
    )
    .expect("create artifacts dir");
    std::fs::write(&resolved_path, "stale report").expect("write stale artifact");

    let session = Session::new(Some("no output write this attempt".to_string()), None);
    let resolved = super::reconcile_automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace root"),
        run_id,
        &AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "generate_report".to_string(),
            agent_id: "writer".to_string(),
            objective: "Generate report".to_string(),
            depends_on: vec![],
            input_refs: vec![],
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
                    "output_path": output_path
                }
            })),
        },
        output_path,
        50,
        10,
    )
    .await
    .expect("resolve stale output")
    .expect("stale output should still resolve");

    assert_eq!(resolved.path, resolved_path);
    assert!(!resolved.materialized_by_current_attempt);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_recovers_json_artifact_from_session_text() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-session-text-json-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-session-json";
    let output_path = ".tandem/artifacts/research-sources.json";
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let mut session = Session::new(Some("session text recovery".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::Text {
            text: "{\n  \"sources\": [\n    {\n      \"path\": \"README.md\",\n      \"reason\": \"project overview\"\n    }\n  ],\n  \"summary\": \"Primary local sources identified.\"\n}\n{\"status\":\"completed\"}".to_string(),
        }],
    ));

    let resolved = super::reconcile_automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace root"),
        run_id,
        &AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "research_sources".to_string(),
            agent_id: "researcher".to_string(),
            objective: "Find and record local sources".to_string(),
            depends_on: vec![],
            input_refs: vec![],
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
                    "output_path": output_path
                }
            })),
        },
        output_path,
        50,
        10,
    )
    .await
    .expect("recover from session text");

    let expected =
        workspace_root.join(".tandem/runs/run-session-json/artifacts/research-sources.json");
    assert_eq!(resolved.map(|value| value.path), Some(expected.clone()));
    let written = std::fs::read_to_string(expected).expect("read recovered artifact");
    let parsed: serde_json::Value = serde_json::from_str(&written).expect("parse recovered json");
    assert_eq!(parsed["sources"][0]["path"], "README.md");
    assert_eq!(parsed["summary"], "Primary local sources identified.");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

