#[tokio::test]
async fn local_research_flow_completes_with_read_and_write_artifact() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-local-research-integration-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("docs")).expect("create workspace");
    std::fs::write(
        workspace_root.join("docs/source.md"),
        "# Source\n\nWorkspace evidence for the local brief.\n",
    )
    .expect("seed source file");

    let state = ready_test_state().await;
    let node = brief_research_node("research_local", ".tandem/artifacts/local-brief.md", false);
    let automation = automation_with_single_node(
        "automation-local-research",
        node.clone(),
        &workspace_root,
        vec!["read".to_string()],
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
    let artifact_text = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n## Campaign goal\nClarify positioning.\n\n## Target audience\n- Operators.\n\n## Core pain points\n- Coordination overhead.\n\n## Positioning angle\nTandem centralizes orchestration.\n\n## Competitor context\nLocal-only comparison for this run.\n\n## Proof points with citations\n1. Supported from docs/source.md. Source note: https://example.com/reference\n\n## Likely objections\n- Proof depth.\n\n## Channel considerations\n- Landing page.\n\n## Recommended message hierarchy\n1. Problem\n2. Promise\n\n## Files reviewed\n- docs/source.md\n\n## Files not reviewed\n- docs/extra.md: not needed for this first pass.\n"
        .to_string();

    let artifact_dir = workspace_root
        .join(".tandem/runs")
        .join(&run.run_id)
        .join("artifacts");
    std::fs::create_dir_all(&artifact_dir).expect("create artifact dir");
    std::fs::write(artifact_dir.join("local-brief.md"), &artifact_text).expect("write artifact");

    let session = assistant_session_with_tool_invocations(
        "local-research-validation",
        &workspace_root,
        vec![
            (
                "glob",
                json!({"pattern":"docs/**/*.md"}),
                json!({
                    "output": workspace_root
                        .join("docs/source.md")
                        .display()
                        .to_string()
                }),
                None,
            ),
            (
                "read",
                json!({"path":"docs/source.md"}),
                json!({"output":"Workspace evidence for the local brief."}),
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
            .get("workspace_inspection_used")
            .and_then(Value::as_bool),
        Some(true)
    );

    let session_text = "Done\n\n{\"status\":\"completed\"}";
    let (accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        session_text,
        &tool_telemetry,
        None,
        Some((output_path.clone(), artifact_text.clone())),
        &workspace_snapshot_before,
    );
    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        artifact_validation
            .get("semantic_block_reason")
            .and_then(Value::as_str),
        None
    );

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
        output.clone(),
        AutomationRunStatus::Completed,
        1,
    )
    .await;

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Completed);
    assert_eq!(
        persisted.checkpoint.node_attempts.get("research_local"),
        Some(&1)
    );

    let output = persisted
        .checkpoint
        .node_outputs
        .get("research_local")
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
            .pointer("/tool_telemetry/executed_tools")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["glob", "read", "write"])
    );
    assert_eq!(
        output
            .pointer("/tool_telemetry/workspace_inspection_used")
            .and_then(Value::as_bool),
        Some(true)
    );

    let written = std::fs::read_to_string(
        workspace_root
            .join(".tandem/runs")
            .join(&run.run_id)
            .join("artifacts")
            .join("local-brief.md"),
    )
    .expect("written artifact");
    assert_eq!(written, artifact_text);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn mcp_grounded_research_flow_completes_with_mcp_tool_usage() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-mcp-research-integration-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let state = ready_test_state().await;
    let node = citations_research_node("research_mcp", ".tandem/artifacts/research-sources.json");
    let automation = automation_with_single_node(
        "automation-mcp-research",
        node.clone(),
        &workspace_root,
        vec!["mcp.tandem_mcp.search_docs".to_string()],
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
    let artifact_text = "# Research Sources\n\n## Summary\nCollected current Tandem MCP documentation references.\n\n## Citations\n1. Tandem MCP Guide. Source note: tandem-mcp://docs/guide\n2. Tandem MCP API Reference. Source note: tandem-mcp://docs/api-reference\n"
        .to_string();

    let artifact_dir = workspace_root
        .join(".tandem/runs")
        .join(&run.run_id)
        .join("artifacts");
    std::fs::create_dir_all(&artifact_dir).expect("create artifact dir");
    std::fs::write(artifact_dir.join("research-sources.json"), &artifact_text)
        .expect("write artifact");

    let session = assistant_session_with_tool_invocations(
        "mcp-research-validation",
        &workspace_root,
        vec![
            (
                "mcp.tandem_mcp.search_docs",
                json!({
                    "query": "research sources artifact contract"
                }),
                json!({
                    "output": "Matched Tandem MCP docs",
                    "metadata": {"count": 2}
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
        "mcp.tandem_mcp.search_docs".to_string(),
        "write".to_string(),
    ];
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    assert_eq!(
        tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["mcp.tandem_mcp.search_docs", "write"])
    );
    assert_eq!(
        tool_telemetry
            .get("web_research_used")
            .and_then(Value::as_bool),
        Some(false)
    );

    let session_text = "Done\n\n{\"status\":\"completed\"}";
    let (accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        session_text,
        &tool_telemetry,
        None,
        Some((output_path.clone(), artifact_text.clone())),
        &workspace_snapshot_before,
    );
    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );

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
        output.clone(),
        AutomationRunStatus::Completed,
        1,
    )
    .await;

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Completed);
    assert_eq!(
        persisted.checkpoint.node_attempts.get("research_mcp"),
        Some(&1)
    );

    let output = persisted
        .checkpoint
        .node_outputs
        .get("research_mcp")
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
            .pointer("/tool_telemetry/executed_tools")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["mcp.tandem_mcp.search_docs", "write"])
    );

    let written = std::fs::read_to_string(
        workspace_root
            .join(".tandem/runs")
            .join(&run.run_id)
            .join("artifacts")
            .join("research-sources.json"),
    )
    .expect("written artifact");
    assert_eq!(written, artifact_text);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn external_web_research_flow_completes_with_websearch_and_write() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-web-research-integration-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("docs")).expect("create workspace");
    std::fs::write(
        workspace_root.join("docs/source.md"),
        "# Source\n\nWorkspace evidence for the web-backed brief.\n",
    )
    .expect("seed source file");

    let state = ready_test_state().await;

    let node = brief_research_node("research_web", ".tandem/artifacts/web-brief.md", true);
    let automation = automation_with_single_node(
        "automation-web-research",
        node.clone(),
        &workspace_root,
        vec!["read".to_string()],
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
    let artifact_text = "# Marketing Brief\n\n## Workspace source audit\nPrepared from workspace sources.\n\n### Files Reviewed\n| Local Path | Evidence Summary |\n|---|---|\n| `docs/source.md` | Core source reviewed |\n\n### Files Not Reviewed\n| Local Path | Reason |\n|---|---|\n| `docs/extra.md` | Out of scope for this run |\n\n### Web Sources Reviewed\n| URL | Status | Notes |\n|---|---|---|\n| https://example.com | Fetched | Confirmed live |\n\n## Campaign goal\nClarify positioning.\n\n## Target audience\n- Operators.\n\n## Core pain points\n- Coordination overhead.\n\n## Positioning angle\nTandem centralizes orchestration.\n\n## Competitor context\nExternal web comparison for this run.\n\n## Proof points with citations\n1. Supported from docs/source.md. Source note: https://example.com/reference\n\n## Likely objections\n- Proof depth.\n\n## Channel considerations\n- Landing page.\n\n## Recommended message hierarchy\n1. Problem\n2. Promise\n"
        .to_string();

    let artifact_dir = workspace_root
        .join(".tandem/runs")
        .join(&run.run_id)
        .join("artifacts");
    std::fs::create_dir_all(&artifact_dir).expect("create artifact dir");
    std::fs::write(artifact_dir.join("web-brief.md"), &artifact_text).expect("write artifact");

    let session = assistant_session_with_tool_invocations(
        "web-research-validation",
        &workspace_root,
        vec![
            (
                "glob",
                json!({"pattern":"docs/**/*.md"}),
                json!({
                    "output": workspace_root
                        .join("docs/source.md")
                        .display()
                        .to_string()
                }),
                None,
            ),
            (
                "read",
                json!({"path":"docs/source.md"}),
                json!({"output":"Workspace evidence for the web-backed brief."}),
                None,
            ),
            (
                "websearch",
                json!({"query":"tandem competitor landscape"}),
                json!({
                    "output": "Matched Tandem web research",
                    "metadata": {"count": 2}
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
        "glob".to_string(),
        "read".to_string(),
        "websearch".to_string(),
        "write".to_string(),
    ];
    let tool_telemetry = summarize_automation_tool_activity(&node, &session, &requested_tools);
    assert_eq!(
        tool_telemetry
            .get("executed_tools")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["glob", "read", "websearch", "write"])
    );
    assert_eq!(
        tool_telemetry
            .get("web_research_used")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        tool_telemetry
            .get("web_research_succeeded")
            .and_then(Value::as_bool),
        Some(true)
    );

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Queued);
    assert_eq!(persisted.checkpoint.node_attempts.get("research_web"), None);

    let session_text = "Done\n\n{\"status\":\"completed\"}";
    let (accepted_output, artifact_validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root string"),
        session_text,
        &tool_telemetry,
        None,
        Some((output_path.clone(), artifact_text.clone())),
        &workspace_snapshot_before,
    );
    assert!(rejected.is_none());
    assert_eq!(
        artifact_validation
            .get("validation_outcome")
            .and_then(Value::as_str),
        Some("passed")
    );
    assert_eq!(
        artifact_validation
            .get("web_sources_reviewed_present")
            .and_then(Value::as_bool),
        Some(true)
    );

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
        output.clone(),
        AutomationRunStatus::Completed,
        1,
    )
    .await;

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.status, AutomationRunStatus::Completed);
    assert_eq!(
        persisted.checkpoint.node_attempts.get("research_web"),
        Some(&1)
    );

    let output = persisted
        .checkpoint
        .node_outputs
        .get("research_web")
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
            .pointer("/tool_telemetry/web_research_used")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        output
            .pointer("/tool_telemetry/web_research_succeeded")
            .and_then(Value::as_bool),
        Some(true)
    );
    let output_tools = output
        .pointer("/tool_telemetry/executed_tools")
        .and_then(Value::as_array)
        .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>())
        .expect("output tools");
    assert!(output_tools.iter().any(|tool| *tool == "glob"));
    assert!(output_tools.iter().any(|tool| *tool == "read"));
    assert!(output_tools.iter().any(|tool| *tool == "websearch"));
    assert!(output_tools.iter().any(|tool| *tool == "write"));
    assert_eq!(
        output
            .pointer("/artifact_validation/web_sources_reviewed_present")
            .and_then(Value::as_bool),
        Some(true)
    );

    let written = std::fs::read_to_string(
        workspace_root
            .join(".tandem/runs")
            .join(&run.run_id)
            .join("artifacts")
            .join("web-brief.md"),
    )
    .expect("written artifact");
    assert_eq!(written, artifact_text);

    let _ = std::fs::remove_dir_all(&workspace_root);
}
