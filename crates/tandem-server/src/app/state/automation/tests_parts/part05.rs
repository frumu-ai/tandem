#[test]
fn validation_accepts_unknown_mcp_server_artifact_from_concrete_tool_receipt() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-dynamic-mcp-artifact-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "confirm_acme_target".to_string();
    node.objective = "Use the Acme MCP connector to confirm the external destination.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "text_summary".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/confirm-acme-target.md"
        },
        "tool_allowlist": [
            "mcp.acme_connector.fetch_destination",
            "write"
        ]
    }));
    let artifact = "Confirmed the external target using `mcp.acme_connector.fetch_destination`.\n\nThe connector returned a concrete destination record for `destination://primary` with a display name, writable status, and no connector limitation. This is enough for downstream publishing to proceed without relying on connector inventory.";
    let session = Session::new(Some("dynamic mcp confirmation".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &json!({
            "executed_tools": [
                "mcp_list",
                "mcp.acme_connector.fetch_destination",
                "write"
            ],
            "requested_tools": [
                "mcp_list",
                "mcp.acme_connector.fetch_destination",
                "write"
            ],
            "capability_resolution": {
                "mcp_tool_diagnostics": {
                    "selected_servers": ["acme-connector"]
                }
            },
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/confirm-acme-target.md".to_string(),
            artifact.to_string(),
        )),
        &snapshot,
    );

    assert!(accepted.is_some());
    assert_eq!(validation["validation_outcome"], "passed");
    assert!(!validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("mcp_connector_source_artifact_missing")));
    assert!(rejected.is_none());

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn outbound_connector_mutation_failure_requires_retry_even_with_failure_artifact() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-external-mutation-failure-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "save_notion_report".to_string();
    node.objective = "Save the completed report into the existing Notion database.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "report_markdown".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/save-notion-report.md",
            "task_kind": "publish"
        },
        "tool_allowlist": [
            "mcp.any_user_name.notion_create_pages",
            "write"
        ]
    }));
    let artifact = "# Notion Publication Report\n\nThe Notion create failed with schema validation; no row was created.";
    let session = Session::new(Some("failed notion publication".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &json!({
            "executed_tools": [
                "mcp_list",
                "mcp.any_user_name.notion_create_pages",
                "write"
            ],
            "failed_tools": [
                "mcp.any_user_name.notion_create_pages"
            ],
            "external_mutation_attempted": true,
            "external_mutation_succeeded": false,
            "latest_external_mutation_failure": "Property \"Report Date\" not found in the data source",
            "requested_tools": [
                "mcp_list",
                "mcp.any_user_name.notion_create_pages",
                "write"
            ],
            "capability_resolution": {
                "mcp_tool_diagnostics": {
                    "selected_servers": ["any-user-name"]
                }
            },
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/save-notion-report.md".to_string(),
            artifact.to_string(),
        )),
        &snapshot,
    );

    assert!(accepted.is_none(), "{validation}");
    assert_eq!(validation["validation_outcome"], "needs_repair");
    assert!(validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("external_mutation_failed")));
    assert!(rejected
        .as_deref()
        .unwrap_or_default()
        .contains("external delivery mutation failed"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_rejects_connector_source_inventory_only_artifact() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-connector-inventory-only-artifact-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "collect_reddit_signals".to_string();
    node.objective = "Use reddit-gmail MCP to collect Reddit posts and comments.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/collect-reddit-signals.json"
        },
        "tool_allowlist": [
            "mcp.reddit_gmail.reddit_search_across_subreddits"
        ]
    }));
    let artifact = serde_json::to_string_pretty(&json!({
        "connected_server_names": ["reddit-gmail"],
        "enabled_server_names": ["reddit-gmail"],
        "inventory_version": 1,
        "registered_tools": ["mcp.reddit_gmail.reddit_search_across_subreddits"],
        "remote_tools": [],
        "servers": [{
            "name": "reddit-gmail",
            "connected": true,
            "registered_tools": ["mcp.reddit_gmail.reddit_search_across_subreddits"]
        }]
    }))
    .expect("serialize inventory");
    let session = Session::new(Some("inventory only connector artifact".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "",
        &json!({
            "executed_tools": [
                "mcp_list",
                "mcp.reddit_gmail.reddit_search_across_subreddits",
                "write"
            ],
            "requested_tools": [
                "mcp_list",
                "mcp.reddit_gmail.reddit_search_across_subreddits",
                "write"
            ],
            "capability_resolution": {
                "mcp_tool_diagnostics": {
                    "selected_servers": ["reddit-gmail"]
                }
            },
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/collect-reddit-signals.json".to_string(),
            artifact,
        )),
        &snapshot,
    );

    assert!(accepted.is_none());
    assert!(validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("mcp_connector_source_artifact_missing")));
    assert!(rejected
        .as_deref()
        .unwrap_or_default()
        .contains("connector inventory"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_requires_declared_concrete_mcp_tools() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-required-mcp-tool-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.objective =
        "Use mcp.githubcopilot.get_me and mcp.githubcopilot.search_repositories.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "allowed_tools": [
            "mcp.githubcopilot.get_me",
            "mcp.githubcopilot.search_repositories"
        ],
        "builder": {
            "task_class": "connector_preflight",
            "output_path": ".tandem/artifacts/establish-github-context.json"
        }
    }));
    let artifact = serde_json::to_string_pretty(&json!({
        "status": "completed",
        "confirmed_authenticated_user": false,
        "confirmed_target_repository": false
    }))
    .expect("serialize artifact");
    let session = Session::new(Some("mcp required tool validation".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, _rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "",
        &json!({
            "executed_tools": ["mcp_list", "write"],
            "requested_tools": [
                "mcp.githubcopilot.get_me",
                "mcp.githubcopilot.search_repositories",
                "write"
            ],
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/establish-github-context.json".to_string(),
            artifact,
        )),
        &snapshot,
    );

    assert!(accepted.is_none());
    assert_eq!(validation["validation_outcome"], "blocked");
    assert!(validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("mcp_required_tool_missing")));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_blocks_read_only_source_mutations_without_retry() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-read-only-source-mutation-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let source_rel_path = "packages/tandem-control-panel/src/app/store.js";
    let source_path = workspace_root.join(source_rel_path);
    std::fs::create_dir_all(source_path.parent().expect("source parent"))
        .expect("create source parent");
    std::fs::write(&source_path, "export const routes = [];\n").expect("write source file");

    let mut snapshot = std::collections::BTreeMap::new();
    snapshot.insert(
        source_path.to_string_lossy().to_string(),
        std::fs::read(&source_path).expect("snapshot source file"),
    );

    std::fs::write(&source_path, "export const routes = ['bug-monitor'];\n")
        .expect("mutate source file");

    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/read-control-panel-store.json"
        }
    }));
    let artifact = serde_json::to_string_pretty(&json!({
        "status": "completed",
        "summary": "Read control panel store"
    }))
    .expect("serialize artifact");
    let session = Session::new(Some("read only source mutation".to_string()), None);
    let workspace_snapshot_before = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) =
        super::logic::validate_automation_artifact_output_with_context(
            &AutomationV2Spec {
                automation_id: "validation".to_string(),
                name: "validation".to_string(),
                description: None,
                status: crate::AutomationV2Status::Draft,
                schedule: AutomationV2Schedule {
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
                    max_parallel_agents: None,
                    max_total_runtime_ms: None,
                    max_total_tool_calls: None,
                    max_total_tokens: None,
                    max_total_cost_usd: None,
                },
                output_targets: Vec::new(),
                created_at_ms: 0,
                updated_at_ms: 0,
                creator_id: "test".to_string(),
                workspace_root: Some(workspace_root.to_string_lossy().to_string()),
                metadata: None,
                next_fire_at_ms: None,
                last_fired_at_ms: None,
                scope_policy: None,
                watch_conditions: Vec::new(),
                handoff_config: None,
            },
            &node,
            &session,
            workspace_root.to_str().expect("workspace root"),
            None,
            None,
            "",
            &json!({
                "executed_tools": ["read"],
                "requested_tools": ["read"],
                "verified_output_materialized_by_current_attempt": true
            }),
            None,
            Some((
                ".tandem/artifacts/read-control-panel-store.json".to_string(),
                artifact,
            )),
            &workspace_snapshot_before,
            None,
            Some(&snapshot),
        );

    assert!(accepted.is_none());
    assert_eq!(validation["validation_outcome"], "blocked");
    assert!(validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("read_only_source_mutations")));
    assert!(rejected
        .as_deref()
        .unwrap_or_default()
        .contains("read-only source-of-truth mutation"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_repair_state_uses_node_attempt_budget() {
    let tool_telemetry = json!({
        "node_attempt": 3,
        "node_max_attempts": 3,
        "tool_call_counts": {}
    });

    let (repair_attempt, repair_attempts_remaining, repair_exhausted) =
        super::logic::infer_artifact_repair_state(
            None,
            false,
            false,
            Some("required output was not created in the current attempt"),
            &tool_telemetry,
            Some(5),
        );

    assert_eq!(repair_attempt, 2);
    assert_eq!(repair_attempts_remaining, 0);
    assert!(repair_exhausted);
}

#[test]
fn assess_evidence_anchors_count_upstream_path_and_url_mentions() {
    let assessment = assess_artifact_candidate(
        &bare_node(),
        "/workspace",
        "tool",
        "See /workspace/docs/product-capabilities.md and https://example.com/source-1 for details.",
        &[],
        &[],
        &[
            "/workspace/docs/product-capabilities.md".to_string(),
            "/workspace/README.md".to_string(),
        ],
        &["https://example.com/source-1".to_string()],
    );
    assert!(
        assessment.evidence_anchor_count >= 2,
        "expected to match at least two upstream evidence anchors, got {}",
        assessment.evidence_anchor_count
    );
}

// -----------------------------------------------------------------------
// Standup gap fill — T1: filler detection consolidation (item E)
// -----------------------------------------------------------------------

// Converts raw standup JSON into the upstream input shape that
// extract_standup_participant_update() and the filler detectors consume.
fn standup_participant_input(node_id: &str, yesterday: &str, today: &str) -> Value {
    json!({
        "alias": node_id,
        "from_step_id": node_id,
        "output": {
            "status": "completed",
            "content": {
                "text": serde_json::to_string(&json!({
                    "yesterday": yesterday,
                    "today": today,
                    "status": "completed"
                })).unwrap()
            }
        }
    })
}

#[test]
fn standup_filler_detection_catches_standup_specific_phrases() {
    use super::node_output::detect_automation_node_status;
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "standup_update".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StandupUpdate),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    // Both fields contain standup-specific filler phrases
    let session_text = serde_json::to_string(&json!({
        "yesterday": "reviewed workspace artifacts and tandem memory; identified relevant context",
        "today": "prepare the daily standup report from available context",
        "status": "completed"
    }))
    .unwrap();
    let (status, reason, _) =
        detect_automation_node_status(&node, &session_text, None, &json!({}), None);
    assert_eq!(
        status, "needs_repair",
        "standup-specific filler phrases should trigger needs_repair"
    );
    assert!(
        reason.is_some(),
        "filler rejection should include a repair reason"
    );
}

#[test]
fn standup_filler_detection_catches_generic_placeholder_phrases() {
    use super::node_output::detect_automation_node_status;
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "standup_update".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StandupUpdate),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    // Generic status-only markers that placeholder_like_artifact_text() catches:
    // short text containing "completed", "confirmed", "write completion", etc.
    // These represent agents that respond with status echo strings instead of content.
    let session_text = serde_json::to_string(&json!({
        "yesterday": "completed",
        "today": "write completion",
        "status": "completed"
    }))
    .unwrap();
    let (status, _reason, _) =
        detect_automation_node_status(&node, &session_text, None, &json!({}), None);
    assert_eq!(
        status, "needs_repair",
        "generic placeholder phrases should also trigger needs_repair via consolidated detection"
    );
}

#[test]
fn standup_filler_detection_accepts_concrete_updates() {
    use super::node_output::detect_automation_node_status;
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "standup_update".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StandupUpdate),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    // Concrete update with real file references
    let session_text = serde_json::to_string(&json!({
        "yesterday": "Drafted homepage headline copy in outputs/homepage-copy.md and refined the H1 variant list.",
        "today": "Update the campaign brief with the new audience segment based on outputs/research-brief.md.",
        "status": "completed"
    }))
    .unwrap();
    let (status, _reason, _) =
        detect_automation_node_status(&node, &session_text, None, &json!({}), None);
    assert_eq!(
        status, "completed",
        "concrete standup update with file references should be accepted"
    );
}

// -----------------------------------------------------------------------
// Standup gap fill — T2: enriched repair reason (item D)
// -----------------------------------------------------------------------

#[test]
fn standup_filler_repair_reason_includes_tool_telemetry_context() {
    use super::node_output::detect_automation_node_status;
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "standup_update".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StandupUpdate),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    let session_text = serde_json::to_string(&json!({
        "yesterday": "reviewed workspace artifacts and tandem memory",
        "today": "prepare the daily standup report from available context",
        "status": "completed"
    }))
    .unwrap();
    let tool_telemetry = json!({
        "executed_tools": ["glob", "read", "memory_search"],
        "glob_directories": ["outputs/", "content/"],
        "read_paths": ["outputs/homepage-copy.md", "content/article-draft.md"]
    });
    let (status, reason, _) =
        detect_automation_node_status(&node, &session_text, None, &tool_telemetry, None);
    assert_eq!(status, "needs_repair");
    let reason = reason.expect("filler rejection should include a reason");
    assert!(
        reason.contains("glob") || reason.contains("read"),
        "repair reason should mention tools used, got: {reason}"
    );
    assert!(
        reason.contains("outputs/") || reason.contains("content/"),
        "repair reason should mention directories searched, got: {reason}"
    );
    assert!(
        reason.contains("homepage-copy") || reason.contains("article-draft"),
        "repair reason should mention files read, got: {reason}"
    );
}

#[test]
fn standup_filler_repair_reason_handles_missing_telemetry_gracefully() {
    use super::node_output::detect_automation_node_status;
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "standup_update".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StandupUpdate),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    let session_text = serde_json::to_string(&json!({
        "yesterday": "reviewed workspace",
        "today": "workspace context",
        "status": "completed"
    }))
    .unwrap();
    let (status, reason, _) =
        detect_automation_node_status(&node, &session_text, None, &json!({}), None);
    assert_eq!(status, "needs_repair");
    let reason = reason.expect("filler rejection should always include a reason");
    assert!(
        reason.contains("none recorded"),
        "missing telemetry should not cause panic; got: {reason}"
    );
}

// -----------------------------------------------------------------------
// Standup gap fill — T3: receipt path derivation (item B)
// -----------------------------------------------------------------------

#[test]
fn standup_receipt_path_derived_from_report_path() {
    // Test the standup_receipt_path_for_report helper directly
    // The function is private, so we test it indirectly through compile-time
    // inclusion. We verify the expected pattern holds for our documented example.
    let report = "docs/standups/2026-04-05.md";
    let receipt = super::standup_receipt_path_for_report(report);
    assert_eq!(receipt, "docs/standups/receipt-2026-04-05.json");
}

#[test]
fn standup_receipt_path_handles_root_level_report() {
    let report = "standup.md";
    let receipt = super::standup_receipt_path_for_report(report);
    assert_eq!(receipt, "docs/standups/receipt-standup.json");
}

#[test]
fn standup_receipt_path_handles_nested_report() {
    let report = "team/standups/weekly/2026-04-05.md";
    let receipt = super::standup_receipt_path_for_report(report);
    assert_eq!(receipt, "team/standups/weekly/receipt-2026-04-05.json");
}

#[test]
fn standup_synthesis_effective_required_output_path_uses_report_template() {
    let automation = AutomationV2Spec {
        automation_id: "automation-standup".to_string(),
        name: "Daily Standup".to_string(),
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
        output_targets: vec!["docs/standups/{{date}}.md".to_string()],
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: Some(json!({
            "feature": "agent_standup",
            "standup": {
                "report_path_template": "docs/standups/{{date}}.md"
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "standup_synthesis".to_string(),
        agent_id: "coordinator".to_string(),
        objective: "Write the standup report".to_string(),
        depends_on: vec!["participant_0".to_string()],
        input_refs: vec![AutomationFlowInputRef {
            from_step_id: "participant_0".to_string(),
            alias: "participant_0".to_string(),
        }],
        output_contract: Some(AutomationFlowOutputContract {
            kind: "report_markdown".to_string(),
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
        stage_kind: Some(crate::AutomationNodeStageKind::Orchestrator),
        gate: None,
        metadata: None,
    };
    let started_at_ms = chrono::DateTime::parse_from_rfc3339("2026-04-14T09:00:00Z")
        .expect("timestamp")
        .timestamp_millis() as u64;

    let output_path = super::automation_effective_required_output_path_for_run(
        &automation,
        &node,
        "automation-v2-run-standup",
        started_at_ms,
    );

    assert_eq!(output_path.as_deref(), Some("docs/standups/2026-04-14.md"));
}

#[test]
fn parse_status_json_accepts_standup_completion_metadata() {
    let raw = "Standup report written to `docs/standups/2026-04-14.md` for 3 participants.\n\n{\"status\":\"completed\",\"approved\":true,\"report_path\":\"docs/standups/2026-04-14.md\",\"participant_count\":3}";

    let parsed = super::parse_status_json(raw).expect("standup status payload should parse");

    assert_eq!(
        parsed.get("status").and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        parsed.get("report_path").and_then(Value::as_str),
        Some("docs/standups/2026-04-14.md")
    );
    assert_eq!(
        parsed.get("participant_count").and_then(Value::as_u64),
        Some(3)
    );
}

#[test]
fn bug_monitor_context_artifacts_do_not_require_workspace_output_paths() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "research_likely_root_cause".to_string(),
        agent_id: "bug_monitor_triage_agent".to_string(),
        objective: "Research the failure".to_string(),
        depends_on: Vec::new(),
        input_refs: Vec::new(),
        output_contract: Some(AutomationFlowOutputContract {
            kind: "structured_json".to_string(),
            validator: Some(AutomationOutputValidatorKind::StructuredJson),
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
        metadata: Some(json!({
            "builder": {
                "output_path": ".tandem/artifacts/bug_monitor.research.json"
            },
            "bug_monitor": {
                "artifact_type": "bug_monitor_research",
                "context_artifact_path": "artifacts/bug_monitor.research.json"
            }
        })),
    };

    assert_eq!(super::automation_node_required_output_path(&node), None);
    assert_eq!(
        super::automation_node_required_output_path_for_run(&node, Some("automation-v2-run-test")),
        None
    );
}

#[test]
fn bug_monitor_recovery_rejects_mcp_inventory_json() {
    let payload = json!({
        "connected_server_names": ["githubcopilot"],
        "registered_tools": ["mcp.githubcopilot.get_me"],
        "servers": [{"name": "githubcopilot", "connected": true}]
    });

    assert!(!super::recoverable_json_matches_required_output(
        &payload,
        ".tandem/artifacts/bug_monitor.research.json"
    ));
}

#[test]
fn bug_monitor_recovery_accepts_matching_research_artifact_json() {
    let payload = json!({
        "status": "completed",
        "research_summary": {
            "likely_root_cause": "The required artifact was recovered from unrelated tool output."
        },
        "file_references": [{
            "path": "crates/tandem-server/src/app/state/automation/extraction.rs",
            "line": 289
        }]
    });

    assert!(super::recoverable_json_matches_required_output(
        &payload,
        ".tandem/artifacts/bug_monitor.research.json"
    ));
}

// -----------------------------------------------------------------------
// Standup gap fill — T5: coordinator input formatting (item C)
// -----------------------------------------------------------------------

#[test]
fn extract_standup_participant_update_finds_nested_json_in_content_text() {
    let input = standup_participant_input(
        "participant_0_copywriter",
        "Drafted homepage headline copy in outputs/homepage-copy.md",
        "Refine the H1 variants based on the new positioning brief",
    );
    let update = super::prompting_impl::extract_standup_participant_update_pub(&input);
    assert!(
        update.is_some(),
        "should extract standup update from content.text JSON"
    );
    let update = update.unwrap();
    assert!(
        update.get("yesterday").is_some(),
        "extracted update should have yesterday field"
    );
    assert!(
        update.get("today").is_some(),
        "extracted update should have today field"
    );
}

#[test]
fn extract_standup_participant_update_returns_none_for_non_standup_output() {
    let input = json!({
        "alias": "research_brief",
        "from_step_id": "research_brief",
        "output": {
            "status": "completed",
            "content": {
                "text": "The research findings indicate three key market opportunities..."
            }
        }
    });
    let update = super::prompting_impl::extract_standup_participant_update_pub(&input);
    assert!(
        update.is_none(),
        "non-standup output text should not be mistaken for a participant update"
    );
}
