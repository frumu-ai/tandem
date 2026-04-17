#[tokio::test]
async fn reconcile_verified_output_path_unwraps_json_handoff_wrapper_from_session_text() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-session-text-json-wrapper-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-session-json-wrapper";
    let output_path = ".tandem/artifacts/research-sources.json";
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let mut session = Session::new(Some("session text wrapper recovery".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::Text {
            text: "{\n  \"structured_handoff\": {\n    \"sources\": [\n      {\n        \"path\": \"README.md\",\n        \"reason\": \"project overview\"\n      }\n    ],\n    \"summary\": \"Primary local sources identified.\"\n  }\n}\n{\"status\":\"completed\"}".to_string(),
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
    .expect("recover wrapped session text");

    let expected = workspace_root
        .join(".tandem/runs/run-session-json-wrapper/artifacts/research-sources.json");
    assert_eq!(resolved.map(|value| value.path), Some(expected.clone()));
    let written = std::fs::read_to_string(expected).expect("read recovered artifact");
    let parsed: serde_json::Value = serde_json::from_str(&written).expect("parse recovered json");
    assert_eq!(parsed["sources"][0]["path"], "README.md");
    assert_eq!(parsed["summary"], "Primary local sources identified.");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_promotes_legacy_workspace_artifact_into_run_scope() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-legacy-promotion-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-legacy-promotion";
    let output_path = ".tandem/artifacts/research-sources.json";
    let legacy_path = workspace_root.join(output_path);
    std::fs::create_dir_all(legacy_path.parent().expect("legacy parent"))
        .expect("create legacy parent");
    std::fs::write(&legacy_path, "{\n  \"status\": \"completed\"\n}")
        .expect("write legacy artifact");

    let mut session = Session::new(Some("legacy promotion".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": output_path,
                "content": "{\n  \"status\": \"completed\"\n}"
            }),
            result: Some(json!({"output": "written"})),
            error: None,
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
            objective: "Find and record sources".to_string(),
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
    .expect("promote legacy artifact")
    .expect("resolution");

    let expected =
        workspace_root.join(".tandem/runs/run-legacy-promotion/artifacts/research-sources.json");
    assert_eq!(resolved.path, expected);
    assert_eq!(
        resolved.legacy_workspace_artifact_promoted_from,
        Some(legacy_path.clone())
    );
    let promoted = std::fs::read_to_string(&resolved.path).expect("read promoted artifact");
    assert!(promoted.contains("\"status\": \"completed\""));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn reconcile_verified_output_path_does_not_promote_unrelated_workspace_file() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-reconcile-no-unrelated-promotion-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-no-promotion";
    let output_path = ".tandem/artifacts/research-sources.json";
    let unrelated_path = workspace_root.join(".tandem/knowledge/research-sources.json");
    std::fs::create_dir_all(unrelated_path.parent().expect("unrelated parent"))
        .expect("create unrelated parent");
    std::fs::write(&unrelated_path, "{\n  \"status\": \"completed\"\n}")
        .expect("write unrelated file");

    let mut session = Session::new(Some("unrelated write".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": ".tandem/knowledge/research-sources.json",
                "content": "{\n  \"status\": \"completed\"\n}"
            }),
            result: Some(json!({"output": "written"})),
            error: None,
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
            objective: "Find and record sources".to_string(),
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
    .expect("resolve unrelated file");

    assert!(resolved.is_none());
    assert!(!workspace_root
        .join(".tandem/runs/run-no-promotion/artifacts/research-sources.json")
        .exists());

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn publish_verified_output_snapshot_replace_copies_into_workspace_target() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-publish-workspace-{}", uuid::Uuid::new_v4()));
    let run_artifact = workspace_root.join(".tandem/runs/run-publish/artifacts/report.md");
    std::fs::create_dir_all(run_artifact.parent().expect("run artifact parent"))
        .expect("create run artifact parent");
    std::fs::write(&run_artifact, "# Report\n").expect("write run artifact");

    let automation = AutomationV2Spec {
        automation_id: "automation-publish".to_string(),
        name: "Publish".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: Default::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
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
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let mut node = bare_node();
    node.node_id = "generate_report".to_string();

    let result = super::publish_automation_verified_output(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-publish",
        &node,
        &(
            ".tandem/runs/run-publish/artifacts/report.md".to_string(),
            "# Report\n".to_string(),
        ),
        &super::AutomationArtifactPublishSpec {
            scope: super::AutomationArtifactPublishScope::Workspace,
            path: ".tandem/knowledge/report-latest.md".to_string(),
            mode: super::AutomationArtifactPublishMode::SnapshotReplace,
        },
    )
    .expect("publish to workspace");

    let published = workspace_root.join(".tandem/knowledge/report-latest.md");
    assert_eq!(
        std::fs::read_to_string(&published).expect("read published"),
        "# Report\n"
    );
    assert_eq!(result["scope"], "workspace");
    assert_eq!(result["mode"], "snapshot_replace");
    assert_eq!(result["path"], ".tandem/knowledge/report-latest.md");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn publish_verified_output_snapshot_replace_copies_into_global_target() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-publish-global-workspace-{}",
        uuid::Uuid::new_v4()
    ));
    let run_artifact = workspace_root.join(".tandem/runs/run-publish-global/artifacts/report.md");
    std::fs::create_dir_all(run_artifact.parent().expect("run artifact parent"))
        .expect("create run artifact parent");
    std::fs::write(&run_artifact, "# Global Report\n").expect("write run artifact");

    let automation = AutomationV2Spec {
        automation_id: "automation-global-publish".to_string(),
        name: "Publish Global".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: Default::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
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
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let mut node = bare_node();
    node.node_id = "generate_report".to_string();
    let relative_global_path = format!("test-{}/report.md", uuid::Uuid::new_v4());

    let result = super::publish_automation_verified_output(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-publish-global",
        &node,
        &(
            ".tandem/runs/run-publish-global/artifacts/report.md".to_string(),
            "# Global Report\n".to_string(),
        ),
        &super::AutomationArtifactPublishSpec {
            scope: super::AutomationArtifactPublishScope::Global,
            path: relative_global_path.clone(),
            mode: super::AutomationArtifactPublishMode::SnapshotReplace,
        },
    )
    .expect("publish to global");

    let published_root = crate::config::paths::resolve_automation_published_artifacts_dir();
    let published = published_root.join(&relative_global_path);
    assert_eq!(
        std::fs::read_to_string(&published).expect("read published"),
        "# Global Report\n"
    );
    assert_eq!(result["scope"], "global");
    assert_eq!(result["mode"], "snapshot_replace");
    assert_eq!(
        result["path"],
        json!(published.to_string_lossy().to_string())
    );

    let _ = std::fs::remove_file(&published);
    if let Some(parent) = published.parent() {
        let _ = std::fs::remove_dir(parent);
    }
    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn publish_verified_output_append_jsonl_appends_records() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-publish-append-jsonl-{}",
        uuid::Uuid::new_v4()
    ));
    let run_artifact = workspace_root.join(".tandem/runs/run-append/artifacts/research.json");
    std::fs::create_dir_all(run_artifact.parent().expect("run artifact parent"))
        .expect("create run artifact parent");
    std::fs::write(&run_artifact, "{\n  \"sources\": [\"README.md\"]\n}")
        .expect("write run artifact");

    let automation = AutomationV2Spec {
        automation_id: "automation-append".to_string(),
        name: "Append".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: Default::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
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
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let mut node = bare_node();
    node.node_id = "research_sources".to_string();
    let publish_path = ".tandem/knowledge/research-history.jsonl";

    super::publish_automation_verified_output(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-append",
        &node,
        &(
            ".tandem/runs/run-append/artifacts/research.json".to_string(),
            "{\n  \"sources\": [\"README.md\"]\n}".to_string(),
        ),
        &super::AutomationArtifactPublishSpec {
            scope: super::AutomationArtifactPublishScope::Workspace,
            path: publish_path.to_string(),
            mode: super::AutomationArtifactPublishMode::AppendJsonl,
        },
    )
    .expect("first append");
    super::publish_automation_verified_output(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-append-2",
        &node,
        &(
            ".tandem/runs/run-append/artifacts/research.json".to_string(),
            "{\n  \"sources\": [\"README.md\"]\n}".to_string(),
        ),
        &super::AutomationArtifactPublishSpec {
            scope: super::AutomationArtifactPublishScope::Workspace,
            path: publish_path.to_string(),
            mode: super::AutomationArtifactPublishMode::AppendJsonl,
        },
    )
    .expect("second append");

    let published = workspace_root.join(publish_path);
    let lines = std::fs::read_to_string(&published)
        .expect("read appended file")
        .lines()
        .map(str::to_string)
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 2);
    let first: Value = serde_json::from_str(&lines[0]).expect("parse first");
    let second: Value = serde_json::from_str(&lines[1]).expect("parse second");
    assert_eq!(first["run_id"], "run-append");
    assert_eq!(second["run_id"], "run-append-2");
    assert_eq!(first["content"]["sources"][0], "README.md");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn publish_verified_output_falls_back_to_automation_output_targets() {
    let workspace_root =
        std::env::temp_dir().join(format!("tandem-publish-targets-{}", uuid::Uuid::new_v4()));
    let run_artifact = workspace_root.join(".tandem/runs/run-targets/artifacts/report.md");
    std::fs::create_dir_all(run_artifact.parent().expect("run artifact parent"))
        .expect("create run artifact parent");
    std::fs::write(&run_artifact, "# Targeted Report\n").expect("write run artifact");

    let automation = AutomationV2Spec {
        automation_id: "automation-targets".to_string(),
        name: "Targets".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: Default::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: vec!["notes/final-report.md".to_string()],
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
    };
    let node = bare_node();

    let result = super::publish_automation_verified_outputs(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-targets",
        &node,
        &(
            ".tandem/runs/run-targets/artifacts/report.md".to_string(),
            "# Targeted Report\n".to_string(),
        ),
    )
    .expect("publish to output targets");

    let published = workspace_root.join("notes/final-report.md");
    assert_eq!(
        std::fs::read_to_string(&published).expect("read published"),
        "# Targeted Report\n"
    );
    assert_eq!(result["targets"][0]["scope"], "workspace");
    assert_eq!(result["targets"][0]["mode"], "snapshot_replace");
    assert_eq!(result["targets"][0]["path"], "notes/final-report.md");
    assert_eq!(result["targets"][0]["copied"], true);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn publish_verified_output_rejects_workspace_target_outside_workspace() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-publish-invalid-workspace-{}",
        uuid::Uuid::new_v4()
    ));
    let run_artifact = workspace_root.join(".tandem/runs/run-invalid/artifacts/report.md");
    std::fs::create_dir_all(run_artifact.parent().expect("run artifact parent"))
        .expect("create run artifact parent");
    std::fs::write(&run_artifact, "# Report\n").expect("write run artifact");

    let automation = AutomationV2Spec {
        automation_id: "automation-invalid-publish".to_string(),
        name: "Invalid Publish".to_string(),
        description: None,
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: Default::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
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
        workspace_root: Some(workspace_root.to_string_lossy().to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let node = bare_node();

    let error = super::publish_automation_verified_output(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-invalid",
        &node,
        &(
            ".tandem/runs/run-invalid/artifacts/report.md".to_string(),
            "# Report\n".to_string(),
        ),
        &super::AutomationArtifactPublishSpec {
            scope: super::AutomationArtifactPublishScope::Workspace,
            path: "../outside/report.md".to_string(),
            mode: super::AutomationArtifactPublishMode::SnapshotReplace,
        },
    )
    .expect_err("workspace publish should fail");

    assert!(error.to_string().contains("must stay inside workspace"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn session_write_candidates_accepts_file_path_schema_with_normalized_run_scoped_paths() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-write-candidate-file-path-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let run_id = "run-123";
    let artifact_path_with_dot_segments = workspace_root
        .join(".tandem/runs/run-123/artifacts")
        .join("./report.md");

    let mut session = Session::new(Some("file path candidate".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "filePath": artifact_path_with_dot_segments.to_string_lossy(),
                "body": "report body"
            }),
            result: Some(json!({"output":"written"})),
            error: None,
        }],
    ));

    let candidates = session_write_candidates_for_output(
        &session,
        workspace_root.to_str().expect("workspace root"),
        ".tandem/artifacts/report.md",
        Some(run_id),
        None,
    );

    assert_eq!(candidates, vec!["report body".to_string()]);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn session_write_materialized_output_accepts_absolute_legacy_artifact_paths() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-current-attempt-output-abs-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-abs";
    let legacy_abs_path = workspace_root
        .join(".tandem/artifacts/report.md")
        .to_string_lossy()
        .to_string();
    let run_scoped_path = workspace_root.join(".tandem/runs/run-abs/artifacts/report.md");
    std::fs::create_dir_all(
        run_scoped_path
            .parent()
            .expect("artifact path should have parent"),
    )
    .expect("create run artifacts dir");
    std::fs::write(&run_scoped_path, "report body").expect("write run-scoped artifact");

    let mut session = Session::new(Some("absolute write evidence".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "path": legacy_abs_path,
                "content": "report body"
            }),
            result: Some(json!({"output":"ok"})),
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

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn session_write_materialized_output_accepts_file_path_schema_with_normalized_run_scoped_paths() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-current-attempt-output-file-path-{}",
        uuid::Uuid::new_v4()
    ));
    let run_id = "run-file-path";
    let artifact_path = workspace_root.join(".tandem/runs/run-file-path/artifacts/report.md");
    let artifact_path_with_dot_segments = workspace_root
        .join(".tandem/runs/run-file-path/artifacts")
        .join("./report.md");
    std::fs::create_dir_all(
        artifact_path
            .parent()
            .expect("artifact path should have parent"),
    )
    .expect("create artifacts dir");
    std::fs::write(&artifact_path, "report body").expect("write artifact");

    let mut session = Session::new(Some("file path write evidence".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "filePath": artifact_path_with_dot_segments.to_string_lossy(),
                "content": "report body"
            }),
            result: Some(json!({"output":"written"})),
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

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn session_write_candidates_supports_variant_path_and_content_keys() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-write-candidate-variants-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let mut session = Session::new(Some("candidate variants".to_string()), None);
    session.messages.push(tandem_types::Message::new(
        tandem_types::MessageRole::Assistant,
        vec![tandem_types::MessagePart::ToolInvocation {
            tool: "write".to_string(),
            args: json!({
                "output_path": ".tandem/artifacts/report.md",
                "contents": "variant payload"
            }),
            result: Some(json!({"output":"ok"})),
            error: None,
        }],
    ));

    let candidates = session_write_candidates_for_output(
        &session,
        workspace_root.to_str().expect("workspace root"),
        ".tandem/artifacts/report.md",
        Some("run-variants"),
        None,
    );
    assert_eq!(candidates, vec!["variant payload".to_string()]);

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn resolve_automation_output_path_rejects_parent_escape_segments() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-output-path-escape-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");

    let resolved = resolve_automation_output_path(
        workspace_root.to_str().expect("workspace root"),
        "../outside.md",
    );
    assert!(
        resolved.is_err(),
        "expected parent escape path to be rejected, got {resolved:?}"
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn resolve_automation_output_path_normalizes_dot_segments_inside_workspace() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-output-path-normalize-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(workspace_root.join("nested")).expect("create workspace");

    let resolved = resolve_automation_output_path(
        workspace_root.to_str().expect("workspace root"),
        "nested/../report.md",
    )
    .expect("resolve normalized path");

    assert_eq!(resolved, workspace_root.join("report.md"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

// -----------------------------------------------------------------------
// assess_artifact_candidate — score ordering invariants
// -----------------------------------------------------------------------

#[test]
fn assess_empty_text_has_negative_score() {
    let assessment =
        assess_artifact_candidate(&bare_node(), "/workspace", "tool", "", &[], &[], &[], &[]);
    assert!(
        assessment.score < 0,
        "empty text should produce a negative score, got {}",
        assessment.score
    );
}

#[test]
fn assess_substantive_text_scores_higher_than_empty() {
    let rich = "## Summary\n\nDetailed analysis.\n\n## Files reviewed\n\n- /workspace/foo.rs\n\n## Approved\n\nYes.";
    let rich_score =
        assess_artifact_candidate(&bare_node(), "/workspace", "tool", rich, &[], &[], &[], &[])
            .score;
    let empty_score =
        assess_artifact_candidate(&bare_node(), "/workspace", "tool", "", &[], &[], &[], &[]).score;
    assert!(
        rich_score > empty_score,
        "substantive text ({rich_score}) should score higher than empty ({empty_score})"
    );
}

#[test]
fn assess_source_field_preserved() {
    let assessment = assess_artifact_candidate(
        &bare_node(),
        "/workspace",
        "my_source",
        "hello",
        &[],
        &[],
        &[],
        &[],
    );
    assert_eq!(assessment.source, "my_source");
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
