#[test]
fn connector_prompt_includes_supported_upstream_artifact_path_shapes() {
    let mut node = bare_node();
    node.node_id = "notion_writer".to_string();
    node.objective =
        "Insert filtered leads into Notion after reading the upstream artifact.".to_string();
    node.metadata = Some(json!({
        "connector_writer": true,
        "required_tools": ["mcp.notion.notion_create_pages"]
    }));
    let automation = automation_with_output_targets(vec![node.clone()], Vec::new());
    let upstream_inputs = vec![
        json!({
            "alias": "top_level_path",
            "path": ".tandem/runs/run-path/artifacts/top-level.json",
            "output": {}
        }),
        json!({
            "alias": "content_path",
            "output": {
                "content": {
                    "path": ".tandem/runs/run-path/artifacts/content-path.json"
                }
            }
        }),
        json!({
            "alias": "content_data_path",
            "output": {
                "content": {
                    "data": {
                        "path": ".tandem/runs/run-path/artifacts/content-data-path.json"
                    }
                }
            }
        }),
        json!({
            "alias": "root_output_path",
            "output": {
                "path": ".tandem/runs/run-path/artifacts/root-output-path.json"
            }
        }),
        json!({
            "alias": "validated_artifact_path",
            "output": {
                "artifact_validation": {
                    "accepted_artifact_path": ".tandem/runs/run-path/artifacts/accepted-artifact.json"
                }
            }
        }),
        json!({
            "alias": "connector_capture_path",
            "output": {
                "connector_capture": {
                    "artifact_path": ".tandem/runs/run-path/artifacts/connector-results.json"
                }
            }
        }),
        json!({
            "alias": "connector_capture_validation_path",
            "output": {
                "artifact_validation": {
                    "connector_capture_artifact_path": ".tandem/runs/run-path/artifacts/connector-validation-results.json"
                }
            }
        }),
    ];
    let agent = crate::AutomationAgentProfile {
        agent_id: "notion_writer".to_string(),
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
        "run-path",
        &node,
        1,
        &agent,
        &upstream_inputs,
        &[
            "mcp.notion.notion_create_pages".to_string(),
            "read".to_string(),
            "write".to_string(),
        ],
        None,
        None,
        None,
    );

    for expected_path in [
        ".tandem/runs/run-path/artifacts/top-level.json",
        ".tandem/runs/run-path/artifacts/content-path.json",
        ".tandem/runs/run-path/artifacts/content-data-path.json",
        ".tandem/runs/run-path/artifacts/root-output-path.json",
        ".tandem/runs/run-path/artifacts/accepted-artifact.json",
        ".tandem/runs/run-path/artifacts/connector-results.json",
        ".tandem/runs/run-path/artifacts/connector-validation-results.json",
    ] {
        assert!(
            prompt.contains(expected_path),
            "{expected_path} missing from:\n{prompt}"
        );
    }
}

#[test]
fn compacted_upstream_prompt_preserves_connector_capture_paths() {
    let mut node = bare_node();
    node.node_id = "filter_leads".to_string();
    node.objective = "Read the upstream connector capture artifact and filter leads.".to_string();
    node.input_refs = vec![AutomationFlowInputRef {
        from_step_id: "search_reddit".to_string(),
        alias: "raw_reddit".to_string(),
    }];
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    let automation = automation_with_output_targets(vec![node.clone()], Vec::new());
    let upstream_inputs = vec![json!({
        "alias": "raw_reddit",
        "from_step_id": "search_reddit",
        "output": {
            "status": "completed",
            "content": {
                "path": ".tandem/runs/run-path/artifacts/search-reddit.json"
            },
            "connector_capture": {
                "artifact_path": ".tandem/runs/run-path/artifacts/search-reddit-connector-results.json",
                "remote_hydration_required": false
            }
        }
    })];
    let agent = crate::AutomationAgentProfile {
        agent_id: "lead_filter".to_string(),
        template_id: None,
        display_name: "Lead Filter".to_string(),
        avatar_url: None,
        model_policy: None,
        skills: Vec::new(),
        tool_policy: crate::AutomationAgentToolPolicy {
            allowlist: Vec::new(),
            denylist: Vec::new(),
        },
        mcp_policy: crate::AutomationAgentMcpPolicy {
            allowed_servers: Vec::new(),
            allowed_tools: None,
            allowed_connections: Vec::new(),
        },
        approval_policy: None,
    };

    let prompt = render_automation_v2_prompt_with_options(
        &automation,
        "/tmp/workspace",
        "run-path",
        &node,
        1,
        &agent,
        &upstream_inputs,
        &["read".to_string(), "write".to_string()],
        None,
        None,
        None,
        AutomationPromptRenderOptions {
            summary_only_upstream: true,
            knowledge_context: None,
            runtime_values: None,
            mcp_contract_guidance: None,
        },
    );

    assert!(prompt.contains(".tandem/runs/run-path/artifacts/search-reddit.json"));
    assert!(prompt.contains(
        ".tandem/runs/run-path/artifacts/search-reddit-connector-results.json"
    ));
    assert!(prompt.contains("connector_capture"));
}

#[test]
fn composio_source_nodes_keep_large_result_remote_helpers() {
    let mut node = bare_node();
    node.node_id = "search_reddit".to_string();
    node.objective =
        "Use Composio Reddit to search and collect connector-backed lead candidates.".to_string();
    node.tool_policy = Some(crate::AutomationAgentToolPolicy {
        allowlist: vec![
            "write".to_string(),
            "mcp.composio_gmail.composio_search_tools".to_string(),
            "mcp.composio_gmail.composio_multi_execute_tool".to_string(),
        ],
        denylist: Vec::new(),
    });
    node.mcp_policy = Some(crate::AutomationAgentMcpPolicy {
        allowed_servers: vec!["composio-gmail".to_string()],
        allowed_tools: Some(vec![
            "mcp.composio_gmail.composio_search_tools".to_string(),
            "mcp.composio_gmail.composio_multi_execute_tool".to_string(),
        ]),
        allowed_connections: Vec::new(),
    });
    let available_tool_names = std::collections::HashSet::from([
        "mcp.composio_gmail.composio_get_tool_schemas".to_string(),
        "mcp.composio_gmail.composio_multi_execute_tool".to_string(),
        "mcp.composio_gmail.composio_remote_bash_tool".to_string(),
        "mcp.composio_gmail.composio_remote_workbench".to_string(),
        "mcp.composio_gmail.composio_search_tools".to_string(),
        "write".to_string(),
    ]);

    let requested =
        automation_requested_tools_for_node(&node, "/tmp", Vec::new(), &available_tool_names);

    for expected in [
        "mcp.composio_gmail.composio_multi_execute_tool",
        "mcp.composio_gmail.composio_remote_bash_tool",
        "mcp.composio_gmail.composio_remote_workbench",
        "mcp.composio_gmail.composio_get_tool_schemas",
    ] {
        assert!(requested.contains(&expected.to_string()));
    }
}
