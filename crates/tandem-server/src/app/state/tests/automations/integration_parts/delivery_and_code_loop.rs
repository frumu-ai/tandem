#[tokio::test]
async fn analyze_findings_dual_write_flow_completes_with_artifact_and_workspace_file() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-analyze-findings-integration-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("inputs")).expect("create workspace");
    std::fs::write(
        workspace_root.join("inputs/clustered-findings.md"),
        "# Clustered findings\n\n- Repair loops block release confidence.\n- Missing artifacts break downstream synthesis.\n",
    )
    .expect("seed clustered findings");

    let state = ready_test_state().await;
    let workspace_file = "reports/pain-points-analysis.md";
    let node = analyze_findings_node(
        "analyze_findings",
        ".tandem/artifacts/analyze-findings.json",
        workspace_file,
    );
    let automation = automation_with_single_node(
        "automation-analyze-findings",
        node.clone(),
        &workspace_root,
        vec!["glob".to_string(), "read".to_string(), "write".to_string()],
    );
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let output_path = automation_node_required_output_path_for_run(&node, Some(&run.run_id))
        .expect("required output path");
    let workspace_snapshot_before = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let artifact_text = serde_json::to_string_pretty(&json!({
        "status": "completed",
        "pain_points": [
            "Repair loops reduce operator confidence.",
            "Artifact contract misses block downstream steps."
        ],
        "recommended_actions": [
            "Add replay regressions for escaped workflow bugs.",
            "Tighten required output enforcement for synthesis nodes."
        ],
        "summary": "Structured analysis generated from clustered workflow findings."
    }))
    .expect("artifact json");
    let workspace_file_text = "# Pain Points Analysis\n\n## Key Patterns\n- Repair loops reduce operator confidence.\n- Missing artifacts block downstream synthesis.\n\n## Recommended Actions\n1. Add replay regressions for escaped workflow bugs.\n2. Tighten required output enforcement for synthesis nodes.\n";

    let artifact_dir = workspace_root
        .join(".tandem/runs")
        .join(&run.run_id)
        .join("artifacts");
    std::fs::create_dir_all(&artifact_dir).expect("create artifact dir");
    std::fs::create_dir_all(workspace_root.join("reports")).expect("create reports dir");
    std::fs::write(artifact_dir.join("analyze-findings.json"), &artifact_text)
        .expect("write artifact");
    std::fs::write(workspace_root.join(workspace_file), workspace_file_text)
        .expect("write workspace file");

    let session = assistant_session_with_tool_invocations(
        "analyze-findings-validation",
        &workspace_root,
        vec![
            (
                "glob",
                json!({"pattern":"inputs/**/*.md"}),
                json!({
                    "output": workspace_root
                        .join("inputs/clustered-findings.md")
                        .display()
                        .to_string()
                }),
                None,
            ),
            (
                "read",
                json!({"path":"inputs/clustered-findings.md"}),
                json!({"output":"Repair loops block release confidence."}),
                None,
            ),
            (
                "write",
                json!({"path":workspace_file,"content":workspace_file_text}),
                json!({"ok": true}),
                None,
            ),
            (
                "write",
                json!({"path":output_path,"content":artifact_text}),
                json!({"ok": true}),
                None,
            ),
        ],
    );
    let requested_tools = vec!["glob".to_string(), "read".to_string(), "write".to_string()];
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    assert_eq!(
        tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["glob", "read", "write"])
    );
    assert_eq!(
        tool_telemetry
            .pointer("/tool_call_counts/write")
            .and_then(Value::as_u64),
        Some(2)
    );

    let session_text = format!("{artifact_text}\n\n{{\"status\":\"completed\"}}");
    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root string"),
            Some(&run.run_id),
            &session_text,
            &tool_telemetry,
            None,
            Some((output_path.clone(), artifact_text.clone())),
            &workspace_snapshot_before,
            None,
        );
    assert!(rejected.is_none());
    let validation_outcome = artifact_validation
        .get("validation_outcome")
        .and_then(Value::as_str);
    assert!(
        validation_outcome == Some("passed"),
        "artifact_validation={}",
        serde_json::to_string_pretty(&artifact_validation).expect("artifact validation json")
    );
    assert!(artifact_validation
        .pointer("/validation_basis/must_write_file_statuses")
        .and_then(Value::as_array)
        .is_some_and(|values| values.iter().any(|value| {
            value.get("path").and_then(Value::as_str) == Some(workspace_file)
                && value
                    .get("touched_by_current_attempt")
                    .and_then(Value::as_bool)
                    == Some(true)
                && value
                    .get("materialized_by_current_attempt")
                    .and_then(Value::as_bool)
                    == Some(true)
        })));

    let status = detect_automation_node_status(
        &node,
        &session_text,
        accepted_output.as_ref(),
        &tool_telemetry,
        Some(&artifact_validation),
    );
    assert_eq!(status.0, "completed");

    let output = wrap_automation_node_output(
        &node,
        &session,
        &requested_tools,
        &session.id,
        Some(&run.run_id),
        &session_text,
        accepted_output.clone(),
        Some(artifact_validation.clone()),
    );
    persist_validated_output(
        &state,
        &run.run_id,
        &node.node_id,
        output,
        AutomationRunStatus::Completed,
        1,
    )
    .await;

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    let output = persisted
        .checkpoint
        .node_outputs
        .get("analyze_findings")
        .expect("node output");
    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        output
            .pointer("/artifact_validation/validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        output
            .pointer("/artifact_validation/validation_basis/must_write_files")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        vec![Value::String(workspace_file.to_string())]
    );

    let written_artifact = std::fs::read_to_string(artifact_dir.join("analyze-findings.json"))
        .expect("written artifact");
    assert_eq!(written_artifact, artifact_text);
    let written_workspace_file =
        std::fs::read_to_string(workspace_root.join(workspace_file)).expect("workspace file");
    assert_eq!(written_workspace_file, workspace_file_text);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn compare_results_synthesis_flow_completes_with_upstream_evidence() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-compare-results-integration-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("content/blog-memory")).expect("create workspace");
    std::fs::write(
        workspace_root.join("content/blog-memory/used-themes.md"),
        "# Used Themes\n\n- workflow repair loops\n- release confidence\n",
    )
    .expect("seed memory file");

    let state = ready_test_state().await;
    let node = compare_results_node("compare_results", ".tandem/artifacts/compare-results.md");
    let automation = automation_with_single_node(
        "automation-compare-results",
        node.clone(),
        &workspace_root,
        vec![
            "glob".to_string(),
            "read".to_string(),
            "mcp_list".to_string(),
            "mcp.blog_mcp.list_blog_drafts".to_string(),
            "write".to_string(),
        ],
    );
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let output_path = automation_node_required_output_path_for_run(&node, Some(&run.run_id))
        .expect("required output path");
    let workspace_snapshot_before = automation_workspace_root_file_snapshot(
        workspace_root.to_str().expect("workspace root string"),
    );
    let artifact_text = "# Recent Blog Review\n\n## Memory Themes\n\nPersistent memory in `content/blog-memory/used-themes.md` shows that workflow repair loops and release confidence are already well-trodden editorial territory. That means a fresh post should treat those ideas as context, not as the entire hook.\n\n## Upstream Grounding\n\nThe upstream handoffs in `.tandem/runs/run-compare/artifacts/collect-inputs.json` and `.tandem/runs/run-compare/artifacts/research-sources.json` already establish the run context, the approved Tandem terminology, and the tool inventory around tandem-mcp. A successful follow-on piece should preserve that terminology and use it as proof, rather than resetting to vague \"AI workflow\" language.\n\n## Recent Blog History\n\nThe `mcp.blog_mcp.list_blog_drafts` inspection shows that recent Tandem drafts emphasize orchestration reliability, faster recovery loops, and operator trust. Those drafts tend to open from a concrete operator pain point and then connect that pain to product truth, which is working well but is now close to becoming repetitive.\n\n## Repeated Framing To Avoid\n\nWe should avoid another opener that says repair loops are frustrating without adding new evidence. We should also avoid generic workflow-quality language that does not tie back to concrete artifacts, because the upstream evidence already gives us stronger anchors than that.\n\n## Unexplored Angles\n\nA stronger next angle is release-safety testing as a differentiator: how deterministic workflow contracts, replay coverage, and repair guidance reduce the operational cost of running agent systems in production. Another viable angle is contrasting structured workflow contracts with ad hoc orchestration, using the upstream Tandem grounding as the product-proof section instead of as a vague capabilities summary.\n\n## Recommended Direction\n\nThe best follow-up post should combine the memory evidence from `content/blog-memory/used-themes.md`, the Tandem grounding from `.tandem/runs/run-compare/artifacts/research-sources.json`, and the recent blog-pattern scan from `mcp.blog_mcp.list_blog_drafts`. That gives us a post with a new claim, grounded proof points, and a clear explanation of what not to repeat.\n";

    let artifact_dir = workspace_root
        .join(".tandem/runs")
        .join(&run.run_id)
        .join("artifacts");
    std::fs::create_dir_all(&artifact_dir).expect("create artifact dir");
    std::fs::write(artifact_dir.join("compare-results.md"), artifact_text).expect("write artifact");

    let session = assistant_session_with_tool_invocations(
        "compare-results-validation",
        &workspace_root,
        vec![
            (
                "mcp_list",
                json!({}),
                json!({
                    "output": {
                        "connected_server_names": ["blog-mcp"],
                        "registered_tools": ["mcp.blog_mcp.list_blog_drafts"]
                    }
                }),
                None,
            ),
            (
                "glob",
                json!({"pattern":"content/blog-memory/*.md"}),
                json!({
                    "output": workspace_root
                        .join("content/blog-memory/used-themes.md")
                        .display()
                        .to_string()
                }),
                None,
            ),
            (
                "read",
                json!({"path":"content/blog-memory/used-themes.md"}),
                json!({"output":"workflow repair loops\nrelease confidence"}),
                None,
            ),
            (
                "mcp.blog_mcp.list_blog_drafts",
                json!({"limit": 3}),
                json!({
                    "output": "Recent drafts emphasize orchestration reliability and recovery loops."
                }),
                None,
            ),
            (
                "write",
                json!({"path":output_path,"content":artifact_text}),
                json!({"ok": true}),
                None,
            ),
        ],
    );
    let requested_tools = vec![
        "mcp_list".to_string(),
        "glob".to_string(),
        "read".to_string(),
        "mcp.blog_mcp.list_blog_drafts".to_string(),
        "write".to_string(),
    ];
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    let upstream_evidence = AutomationUpstreamEvidence {
        read_paths: vec![
            ".tandem/runs/run-compare/artifacts/collect-inputs.json".to_string(),
            ".tandem/runs/run-compare/artifacts/research-sources.json".to_string(),
        ],
        discovered_relevant_paths: vec![
            ".tandem/runs/run-compare/artifacts/research-sources.json".to_string()
        ],
        web_research_attempted: false,
        web_research_succeeded: false,
        citation_count: 2,
        citations: vec![
            "Tandem MCP Guide".to_string(),
            "Blog history inspection from blog-mcp".to_string(),
        ],
    };

    let session_text = "Standup-like synthesis complete.\n\n{\"status\":\"completed\"}";
    let (accepted_output, artifact_validation, rejected) =
        validate_automation_artifact_output_with_upstream(
            &node,
            &session,
            workspace_root.to_str().expect("workspace root string"),
            Some(&run.run_id),
            session_text,
            &tool_telemetry,
            None,
            Some((output_path.clone(), artifact_text.to_string())),
            &workspace_snapshot_before,
            Some(&upstream_evidence),
        );
    assert!(rejected.is_none());
    let validation_outcome = artifact_validation
        .get("validation_outcome")
        .and_then(Value::as_str);
    assert!(
        validation_outcome == Some("passed"),
        "artifact_validation={}",
        serde_json::to_string_pretty(&artifact_validation).expect("artifact validation json")
    );
    assert_eq!(
        artifact_validation
            .pointer("/validation_basis/upstream_evidence_used")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(artifact_validation
        .get("read_paths")
        .and_then(Value::as_array)
        .is_some_and(|values| values
            .iter()
            .any(|value| { value.as_str() == Some("content/blog-memory/used-themes.md") })));

    let status = detect_automation_node_status(
        &node,
        session_text,
        accepted_output.as_ref(),
        &tool_telemetry,
        Some(&artifact_validation),
    );
    assert_eq!(status.0, "completed");

    let output = wrap_automation_node_output(
        &node,
        &session,
        &requested_tools,
        &session.id,
        Some(&run.run_id),
        session_text,
        accepted_output.clone(),
        Some(artifact_validation.clone()),
    );
    persist_validated_output(
        &state,
        &run.run_id,
        &node.node_id,
        output,
        AutomationRunStatus::Completed,
        1,
    )
    .await;

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    let output = persisted
        .checkpoint
        .node_outputs
        .get("compare_results")
        .expect("node output");
    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        output
            .pointer("/artifact_validation/validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        output
            .pointer("/artifact_validation/validation_basis/upstream_evidence_used")
            .and_then(Value::as_bool),
        Some(true)
    );

    let written =
        std::fs::read_to_string(artifact_dir.join("compare-results.md")).expect("written artifact");
    assert_eq!(written, artifact_text);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn delivery_flow_completes_with_validated_artifact_body_and_email_send() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-delivery-integration-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("reports")).expect("create workspace");
    let report_path = "reports/final-report.md";
    let report_text = "# Final Report\n\n## Highlights\n- Deterministic workflow contracts reduced repair churn.\n- Replay coverage caught escaped bugs before release.\n\n## Recommendation\nShip the gated workflow coverage bundle.\n";
    std::fs::write(workspace_root.join(report_path), report_text).expect("seed report");

    let state = ready_test_state().await;
    let node = delivery_node("notify_release_owner", "release-owner@example.com");
    let automation = automation_with_single_node(
        "automation-delivery",
        node.clone(),
        &workspace_root,
        vec![
            "read".to_string(),
            "mcp.composio_1.gmail_send_email".to_string(),
        ],
    );
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");

    let session = assistant_session_with_tool_invocations(
        "delivery-validation",
        &workspace_root,
        vec![
            (
                "read",
                json!({"path": report_path}),
                json!({"output": report_text}),
                None,
            ),
            (
                "mcp.composio_1.gmail_send_email",
                json!({
                    "to": "release-owner@example.com",
                    "subject": "Workflow release candidate",
                    "html_body": "<h1>Final Report</h1><p>Deterministic workflow contracts reduced repair churn.</p>"
                }),
                json!({
                    "output": "Email sent",
                    "metadata": {
                        "delivery_status": "sent",
                        "message_id": "msg_123"
                    }
                }),
                None,
            ),
        ],
    );
    let requested_tools = vec![
        "read".to_string(),
        "mcp.composio_1.gmail_send_email".to_string(),
    ];
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    assert_eq!(
        tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["read", "mcp.composio_1.gmail_send_email"])
    );
    assert_eq!(
        tool_telemetry
            .get("workspace_inspection_used")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        tool_telemetry
            .get("email_delivery_attempted")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        tool_telemetry
            .get("email_delivery_succeeded")
            .and_then(Value::as_bool),
        Some(true)
    );

    let session_text = format!(
        "Sent the validated report to release-owner@example.com.\n\n{}",
        serde_json::to_string(&json!({
            "status": "completed",
            "approved": true,
            "report_path": report_path
        }))
        .expect("status json")
    );
    let status = detect_automation_node_status(&node, &session_text, None, &tool_telemetry, None);
    assert_eq!(status.0, "completed");
    assert_eq!(status.1, None);
    assert_eq!(status.2, Some(true));

    let output = wrap_automation_node_output(
        &node,
        &session,
        &requested_tools,
        &session.id,
        Some(&run.run_id),
        &session_text,
        None,
        None,
    );
    persist_validated_output(
        &state,
        &run.run_id,
        &node.node_id,
        output,
        AutomationRunStatus::Completed,
        1,
    )
    .await;

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    let output = persisted
        .checkpoint
        .node_outputs
        .get("notify_release_owner")
        .expect("node output");
    assert_eq!(
        output.get("status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(output.get("approved").and_then(Value::as_bool), Some(true));
    assert_eq!(
        output
            .pointer("/tool_telemetry/email_delivery_attempted")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        output
            .pointer("/tool_telemetry/email_delivery_succeeded")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        output
            .pointer("/tool_telemetry/executed_tools")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["read", "mcp.composio_1.gmail_send_email"])
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}
