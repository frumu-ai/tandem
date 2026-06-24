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
    ] {
        assert!(
            prompt.contains(expected_path),
            "{expected_path} missing from:\n{prompt}"
        );
    }
}
