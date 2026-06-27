#[test]
fn generic_artifact_output_schema_rejects_raw_connector_response() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-generic-artifact-schema-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "search_reddit_threads".to_string();
    node.objective = "Search Reddit through an MCP connector and write normalized posts."
        .to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "reddit_search_artifact".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: Some(json!({
            "type": "object",
            "required": ["status", "query", "raw_posts"],
            "properties": {
                "status": {"const": "completed"},
                "query": {"type": "string"},
                "raw_posts": {"type": "array"}
            }
        })),
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/search-reddit-threads.json"
        },
        "tool_allowlist": [
            "mcp.composio_gmail.composio_multi_execute_tool",
            "write"
        ]
    }));
    let session = Session::new(Some("raw connector response".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();
    let raw_connector_response = json!({
        "successful": true,
        "error": null,
        "log_id": "tool-log-123",
        "data": {
            "results": [{
                "response": {
                    "data_preview": {
                        "posts": [{"title": "enterprise MCP auth"}]
                    }
                }
            }]
        }
    })
    .to_string();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &json!({
            "executed_tools": [
                "mcp.composio_gmail.composio_multi_execute_tool",
                "write"
            ],
            "requested_tools": [
                "mcp.composio_gmail.composio_multi_execute_tool",
                "write"
            ],
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/search-reddit-threads.json".to_string(),
            raw_connector_response,
        )),
        &snapshot,
    );

    assert!(accepted.is_none());
    assert_eq!(validation["validation_outcome"], "needs_repair");
    assert!(validation["unmet_requirements"]
        .as_array()
        .expect("unmet requirements")
        .iter()
        .any(|value| value.as_str() == Some("output_schema_invalid")));
    assert!(rejected
        .as_deref()
        .expect("rejected reason")
        .contains("$.status is required"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_rejects_truncated_source_identity_values() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-truncated-identity-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "filter_reddit_threads".to_string();
    node.objective = "Filter source rows into database-ready leads.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "lead_filter_artifact".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: Some(json!({
            "type": "object",
            "required": ["status", "leads"],
            "properties": {
                "status": {"const": "completed"},
                "leads": {"type": "array"}
            }
        })),
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/filter-reddit-threads.json"
        },
        "tool_allowlist": ["read", "write"]
    }));
    let session = Session::new(Some("filtered leads".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();
    let artifact = json!({
        "status": "completed",
        "leads": [{
            "topic_thread_title": "I was backend lead a...",
            "thread_link": "https://www.reddit.com/r/LocalLLaMA/comments/1rrisqn/i_was_backend_lead_at_manus_after_building_agents/",
            "user_handle": "MorroHsu"
        }]
    })
    .to_string();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        &artifact,
        &json!({
            "executed_tools": ["read", "write"],
            "requested_tools": ["read", "write"],
            "node_attempt": 1,
            "node_max_attempts": 1,
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/filter-reddit-threads.json".to_string(),
            artifact.clone(),
        )),
        &snapshot,
    );

    assert!(accepted.is_none());
    assert!(validation["unmet_requirements"]
        .as_array()
        .expect("unmet requirements")
        .iter()
        .any(|value| value.as_str() == Some("truncated_source_identity_value")));
    assert_eq!(validation["validation_outcome"], "needs_repair");
    assert!(rejected
        .as_deref()
        .expect("rejected reason")
        .contains("topic_thread_title"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn connector_row_filter_materializer_builds_full_leads_from_upstream_source() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-connector-filter-{}",
        uuid::Uuid::new_v4()
    ));
    let source_path =
        ".tandem/runs/run-filter/artifacts/search-mcp-auth-enterprise.json".to_string();
    let resolved_source = workspace_root.join(&source_path);
    std::fs::create_dir_all(resolved_source.parent().expect("source parent"))
        .expect("create source parent");
    std::fs::write(
        &resolved_source,
        serde_json::to_string_pretty(&json!({
            "status": "completed",
            "raw_posts": [
                {
                    "title": "MCP security is the elephant in the room",
                    "subreddit_prefixed": "r/mcp",
                    "author": "security_architect",
                    "permalink": "/r/mcp/comments/1np6euu/mcp_security_is_the_elephant_in_the_room/",
                    "selftext": "Production teams need authentication, audit, and tool-call controls."
                },
                {
                    "title": "Minecraft local setup screenshots",
                    "subreddit_prefixed": "r/LocalLLaMA",
                    "author": "hobby_user",
                    "permalink": "/r/LocalLLaMA/comments/example/minecraft_setup/",
                    "selftext": "gaming toy project"
                }
            ]
        }))
        .expect("serialize source"),
    )
    .expect("write source artifact");

    let mut node = bare_node();
    node.node_id = "filter_mcp_auth_enterprise".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "lead_filter_artifact".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: Some(json!({
            "type": "object",
            "required": ["leads"],
            "properties": {
                "leads": {"type": "array"}
            }
        })),
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "source_node_id": "search_mcp_auth_enterprise",
        "source_alias": "mcp_auth_enterprise",
        "filter_keywords": ["production", "authentication", "security", "tool-call", "mcp", "audit"],
        "reject_keywords": ["gaming", "minecraft", "toy project"]
    }));

    let run = crate::automation_v2::types::AutomationV2RunRecord {
        run_id: "run-filter".to_string(),
        automation_id: "automation-test".to_string(),
        tenant_context: tandem_types::TenantContext::local_implicit(),
        trigger_type: "manual".to_string(),
        status: crate::automation_v2::types::AutomationRunStatus::Running,
        created_at_ms: 0,
        updated_at_ms: 0,
        started_at_ms: Some(0),
        finished_at_ms: None,
        active_session_ids: Vec::new(),
        latest_session_id: None,
        active_instance_ids: Vec::new(),
        checkpoint: crate::automation_v2::types::AutomationRunCheckpoint {
            completed_nodes: vec!["search_mcp_auth_enterprise".to_string()],
            pending_nodes: vec!["filter_mcp_auth_enterprise".to_string()],
            node_outputs: std::collections::HashMap::from([(
                "search_mcp_auth_enterprise".to_string(),
                json!({
                    "status": "completed",
                    "artifact_validation": {
                        "accepted_artifact_path": source_path
                    }
                }),
            )]),
            node_attempts: std::collections::HashMap::new(),
            node_attempt_verdicts: std::collections::HashMap::new(),
            blocked_nodes: Vec::new(),
            awaiting_gate: None,
            gate_history: Vec::new(),
            lifecycle_history: Vec::new(),
            last_failure: None,
        },
        runtime_context: None,
        automation_snapshot: None,
        pause_reason: None,
        resume_reason: None,
        detail: None,
        stop_kind: None,
        stop_reason: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        estimated_cost_usd: 0.0,
        scheduler: None,
        trigger_reason: None,
        consumed_handoff_id: None,
        learning_summary: None,
        effective_execution_profile:
            crate::automation_v2::execution_profile::ExecutionProfile::Strict,
        requested_execution_profile: None,
    };

    let artifact = super::automation_build_connector_row_filter_artifact(
        &run,
        &node,
        workspace_root.to_str().expect("workspace root"),
    )
    .expect("materializer succeeds")
    .expect("materialized leads");
    let leads = artifact
        .get("leads")
        .and_then(Value::as_array)
        .expect("leads array");

    assert_eq!(leads.len(), 1);
    assert_eq!(
        leads[0].get("topic_thread_title").and_then(Value::as_str),
        Some("MCP security is the elephant in the room")
    );
    assert_eq!(
        leads[0].get("thread_link").and_then(Value::as_str),
        Some("https://www.reddit.com/r/mcp/comments/1np6euu/mcp_security_is_the_elephant_in_the_room/")
    );
    assert_eq!(
        artifact.pointer("/source_counts/mcp_auth_enterprise"),
        Some(&json!(2))
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn connector_source_retry_accepts_materialized_same_run_artifact() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-connector-source-retry-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let run_id = "automation-v2-run-test";
    let output_path = format!(".tandem/runs/{run_id}/artifacts/search-reddit-threads.json");
    let mut node = bare_node();
    node.node_id = "search_reddit_threads".to_string();
    node.objective = "Search Reddit through an MCP connector and write normalized posts."
        .to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "reddit_search_artifact".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: Some(json!({
            "type": "object",
            "required": ["status", "query", "raw_posts"],
            "properties": {
                "status": {"const": "completed"},
                "query": {"type": "string"},
                "raw_posts": {"type": "array"}
            }
        })),
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "artifact_type": "connector_source_research_shard",
        "connector_capture": {"enabled": true},
        "builder": {
            "output_path": output_path
        },
        "tool_allowlist": [
            "mcp.composio_gmail.composio_multi_execute_tool",
            "write"
        ]
    }));
    let artifact = json!({
        "status": "completed",
        "query": "how to secure agent tool calls",
        "raw_posts": [{
            "title": "enterprise agent tool security",
            "url": "https://www.reddit.com/r/DevOps/comments/example/thread/"
        }]
    })
    .to_string();
    let session = Session::new(Some("repair inspected connector state".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output_with_upstream(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        Some(run_id),
        "{\"status\":\"completed\"}",
        &json!({
            "executed_tools": [
                "mcp.composio_gmail.composio_multi_execute_tool"
            ],
            "requested_tools": [
                "mcp.composio_gmail.composio_multi_execute_tool",
                "write"
            ],
            "verified_output_materialized_by_current_attempt": false,
            "connector_capture": {
                "extracted_item_count": 1,
                "extracted_items_truncated": false
            }
        }),
        Some(&artifact.clone()),
        Some((output_path, artifact)),
        &snapshot,
        None,
    );

    assert!(accepted.is_some(), "validation={validation:#}");
    assert_eq!(
        validation
            .pointer("/validation_basis/connector_source_output_satisfied_by_capture")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(!validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("current_attempt_output_missing")));
    assert!(rejected.is_none(), "rejected={rejected:?}");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn connector_remote_materializer_selects_available_bash_helper() {
    let requested_tools = vec![
        "mcp.composio_gmail.composio_multi_execute_tool".to_string(),
        "mcp.composio_gmail.composio_remote_bash_tool".to_string(),
    ];
    let capture = json!({
        "tools": ["mcp.composio_gmail.composio_multi_execute_tool"]
    });

    let helper = automation_connector_remote_python_helper_tool(&requested_tools, &capture)
        .expect("bash helper selected");
    assert_eq!(helper, "mcp.composio_gmail.composio_remote_bash_tool");

    let args = automation_connector_remote_python_args(
        &helper,
        "print('ok')".to_string(),
        "MATERIALIZE_CONNECTOR_REMOTE_RESULT",
        "session-1",
        "/tmp/workspace",
    );
    assert!(args
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| command.contains("python3 - <<'PY'")));
    assert!(args.get("code_to_execute").is_none());
}

#[test]
fn connector_remote_materializer_prefers_workbench_when_available() {
    let requested_tools = vec![
        "mcp.composio_gmail.composio_remote_bash_tool".to_string(),
        "mcp.composio_gmail.composio_remote_workbench".to_string(),
    ];
    let capture = json!({});

    let helper = automation_connector_remote_python_helper_tool(&requested_tools, &capture)
        .expect("workbench helper selected");
    assert_eq!(helper, "mcp.composio_gmail.composio_remote_workbench");

    let args = automation_connector_remote_python_args(
        &helper,
        "print('ok')".to_string(),
        "MATERIALIZE_CONNECTOR_REMOTE_RESULT",
        "session-1",
        "/tmp/workspace",
    );
    assert_eq!(
        args.get("code_to_execute").and_then(Value::as_str),
        Some("print('ok')")
    );
    assert!(args.get("command").is_none());
}

