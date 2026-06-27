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
            tool_policy: None,
            mcp_policy: None,
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
            tool_policy: None,
            mcp_policy: None,
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

#[test]
fn automation_node_prompt_timeout_error_matches_same_node_timeout_only() {
    let node = AutomationFlowNode {
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        node_id: "collect_reddit_signals".to_string(),
        agent_id: "reddit".to_string(),
        objective: "Collect Reddit signals".to_string(),
        depends_on: vec![],
        input_refs: vec![],
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
        metadata: None,
    };

    assert!(super::automation_node_prompt_timeout_error(
        &anyhow::anyhow!("automation node `collect_reddit_signals` timed out after 180000 ms"),
        &node,
    ));
    assert!(!super::automation_node_prompt_timeout_error(
        &anyhow::anyhow!("automation node `other_node` timed out after 180000 ms"),
        &node,
    ));
    assert!(!super::automation_node_prompt_timeout_error(
        &anyhow::anyhow!("provider stream idle timeout after 60000 ms"),
        &node,
    ));
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
            tool_policy: None,
            mcp_policy: None,
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
            tool_policy: None,
            mcp_policy: None,
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

#[tokio::test]
async fn reconcile_verified_output_path_recovers_schema_matching_remote_artifact() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-remote-artifact-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-remote-artifact";
    let output_path = ".tandem/artifacts/search-reddit.json";
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let remote_stdout = serde_json::to_string(&json!({
        "path": ".tandem/runs/run-remote-artifact/artifacts/search-reddit.json",
        "returncode": 0,
        "stdout": "VERIFIED\n",
        "stderr": "",
        "artifact": {
            "status": "completed",
            "query": "mcp server authentication enterprise",
            "raw_posts": [
                {
                    "title": "Better Auth MCP Server",
                    "url": "https://www.reddit.com/r/mcp/comments/example",
                    "subreddit": "r/mcp"
                }
            ],
            "tool_calls": ["mcp.composio_gmail.composio_multi_execute_tool"]
        }
    }))
    .expect("remote stdout json");

    let mut session = Session::new(Some("remote artifact recovery".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "mcp.composio_gmail.composio_remote_workbench".to_string(),
            args: json!({
                "cmd": "materialize connector result"
            }),
            result: Some(json!({
                "output": {
                    "data": {
                        "stdout": remote_stdout
                    },
                    "error": null,
                    "successful": true
                }
            })),
            error: None,
        }],
    ));

    let resolved = super::reconcile_automation_resolve_verified_output_path(
        &session,
        workspace_root.to_str().expect("workspace root"),
        run_id,
        &AutomationFlowNode {
            knowledge: tandem_orchestrator::KnowledgeBinding::default(),
            node_id: "search_reddit".to_string(),
            agent_id: "reddit".to_string(),
            objective: "Search Reddit for infrastructure leads".to_string(),
            depends_on: vec![],
            input_refs: vec![],
            output_contract: Some(AutomationFlowOutputContract {
                kind: "generic_artifact".to_string(),
                validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                enforcement: None,
                schema: Some(json!({
                    "type": "object",
                    "required": ["status", "query", "raw_posts"],
                    "properties": {
                        "status": { "type": "string" },
                        "query": { "type": "string" },
                        "raw_posts": { "type": "array" }
                    }
                })),
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
                    "output_path": output_path
                }
            })),
        },
        output_path,
        50,
        10,
    )
    .await
    .expect("recover remote artifact")
    .expect("schema-matching artifact should be recovered");

    let expected =
        workspace_root.join(".tandem/runs/run-remote-artifact/artifacts/search-reddit.json");
    assert_eq!(resolved.path, expected.clone());
    assert!(resolved.materialized_by_current_attempt);

    let written = std::fs::read_to_string(expected).expect("read recovered artifact");
    let parsed: serde_json::Value = serde_json::from_str(&written).expect("parse recovered json");
    assert_eq!(parsed["raw_posts"][0]["title"], "Better Auth MCP Server");
    assert!(parsed.get("data").is_none(), "wrapper payload should not win");
    assert!(
        parsed.get("successful").is_none(),
        "connector wrapper should not be recovered as artifact"
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}
