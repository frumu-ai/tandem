#[test]
fn repair_brief_names_exact_connector_remote_file_path() {
    let node = bare_node();
    let prior_output = json!({
        "status": "needs_repair",
        "attempt_verdict": {
            "failure_class": "contract_miss",
            "validation_reason": "connector remote result file was not materialized",
            "expected": {
                "required_output_path": ".tandem/artifacts/search.json"
            },
            "observed": {
                "executed_tools": ["mcp.example.search"],
                "artifact": {"status": "preview_only"}
            },
            "attempt_review": {
                "progress_label": "partial",
                "progress_score": 40,
                "completed_correctly": ["Called the connector search tool."],
                "still_needed": ["Read the connector remote result file before writing the artifact."],
                "why_it_matters": ["Preview rows do not prove full source coverage."],
                "next_moves": ["Use the available remote helper to read `/mnt/files/mex/full.json` before writing the artifact."]
            },
            "unmet_requirements": ["connector_remote_result_not_materialized"],
            "required_next_actions": ["Use the available remote helper to read `/mnt/files/mex/full.json` before writing the artifact."]
        }
    });

    let brief = render_automation_repair_brief(&node, Some(&prior_output), 2, 3, Some("run-1"))
        .expect("repair brief");

    assert!(brief.contains("connector remote result must be materialized"));
    assert!(brief.contains("/mnt/files/mex/full.json"));
    assert!(brief.contains("Do not use `data_preview`"));
}

#[test]
fn connector_capture_prompt_does_not_treat_remote_metadata_as_source_files() {
    let mut node = bare_node();
    node.node_id = "search_connector_records".to_string();
    node.objective = "Call mcp.composio_gmail.composio_multi_execute_tool once. If the connector returns remote_file_info.file_path, saved file instructions, a data_preview with omitted rows like `...N more items`, or total_results greater than visible rows, materialize the referenced remote result file through the connector helper before writing the artifact.".to_string();
    node.metadata = Some(json!({
        "artifact_type": "connector_source_research_shard",
        "connector_capture": {"enabled": true},
        "source_query": "enterprise agent governance",
        "search_query": "enterprise agent governance",
        "required_tools": [
            "mcp.composio_gmail.composio_multi_execute_tool"
        ]
    }));
    let automation = automation_with_output_targets(vec![node.clone()], Vec::new());
    let agent = crate::AutomationAgentProfile {
        agent_id: "collector".to_string(),
        template_id: None,
        display_name: "Collector".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: Vec::new(),
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: vec!["composio-gmail".to_string()],
            allowed_tools: None,
            allowed_connections: Vec::new(),
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt(
        &automation,
        "/tmp/workspace",
        "run-connector",
        &node,
        1,
        &agent,
        &[],
        &[
            "mcp.composio_gmail.composio_multi_execute_tool".to_string(),
            "mcp.composio_gmail.composio_remote_bash_tool".to_string(),
            "mcp.composio_gmail.composio_remote_workbench".to_string(),
            "write".to_string(),
        ],
        None,
        None,
        None,
    );

    assert!(
        !prompt.contains("Concrete files for this node:\n- `...N`"),
        "{prompt}"
    );
    assert!(
        !prompt.contains("Read-only files for this node:\n- `remote_file_info.file_path`"),
        "{prompt}"
    );
    assert!(
        !prompt.contains("These connector tools are optional for this objective"),
        "{prompt}"
    );
    assert!(
        prompt.contains("Call at least one concrete source tool before writing the artifact"),
        "{prompt}"
    );
}
