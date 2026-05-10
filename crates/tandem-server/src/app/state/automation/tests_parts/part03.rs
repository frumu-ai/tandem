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
            profile: None,
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
            profile: None,
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
            profile: None,
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
            profile: None,
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
    let mut node = bare_node();
    node.objective = "write final report".to_string();

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
fn publish_verified_output_rejects_intermediate_node_for_automation_output_targets() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-publish-intermediate-reject-{}",
        uuid::Uuid::new_v4()
    ));
    let run_artifact = workspace_root.join(".tandem/runs/run-source/artifacts/scope.md");
    std::fs::create_dir_all(run_artifact.parent().expect("run artifact parent"))
        .expect("create run artifact parent");
    std::fs::write(&run_artifact, "# Repository Scope Assessment\n").expect("write run artifact");

    let mut source_node = bare_node();
    source_node.node_id = "assess_repository_scope".to_string();
    source_node.objective = "inspect source files".to_string();
    let mut final_node = bare_node();
    final_node.node_id = "write_feature_report".to_string();
    final_node.objective = "write final report".to_string();
    final_node.depends_on = vec![source_node.node_id.clone()];

    let automation = AutomationV2Spec {
        automation_id: "automation-source-targets".to_string(),
        name: "Source targets".to_string(),
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
        flow: crate::AutomationFlowSpec {
            nodes: vec![source_node.clone(), final_node],
        },
        execution: crate::AutomationExecutionPolicy {
            profile: None,
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: vec!["packages/tandem-client-ts/src/client.ts".to_string()],
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

    let result = super::publish_automation_verified_outputs(
        workspace_root.to_str().expect("workspace root"),
        &automation,
        "run-source",
        &source_node,
        &(
            ".tandem/runs/run-source/artifacts/scope.md".to_string(),
            "# Repository Scope Assessment\n".to_string(),
        ),
    );

    assert!(result.is_err());
    assert!(!workspace_root
        .join("packages/tandem-client-ts/src/client.ts")
        .exists());

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
            profile: None,
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
fn assess_nonterminal_json_status_as_placeholder() {
    let artifact = serde_json::to_string_pretty(&json!({
        "status": "in_progress",
        "node_id": "read_contracts",
        "note": "Initial artifact materialized before local contract inspection.",
        "contracts": []
    }))
    .expect("serialize artifact");

    let assessment = assess_artifact_candidate(
        &bare_node(),
        "/workspace",
        "verified_output",
        &artifact,
        &[],
        &[],
        &[],
        &[],
    );

    assert!(assessment.placeholder_like);
}

#[test]
fn validation_rejects_nonterminal_json_status_artifact() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-nonterminal-artifact-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
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
            "output_path": ".tandem/artifacts/read-contracts.json"
        }
    }));
    let artifact = serde_json::to_string_pretty(&json!({
        "status": "in_progress",
        "node_id": "read_contracts",
        "contracts": [],
        "limitations": []
    }))
    .expect("serialize artifact");
    let session = Session::new(Some("nonterminal artifact".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "",
        &json!({
            "executed_tools": ["write"],
            "requested_tools": ["write"],
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/read-contracts.json".to_string(),
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
        .any(|value| value.as_str() == Some("artifact_status_not_terminal")));
    assert!(rejected
        .as_deref()
        .unwrap_or_default()
        .contains("non-terminal status"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_rejects_placeholder_markdown_artifact() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-placeholder-markdown-artifact-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "report_markdown".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/assess-repository-scope.md"
        }
    }));
    let artifact = "# Repository Scope Assessment\n\nInitial artifact created for this final retry. The required workspace output path exists. This file will be updated in-place after inspection.\n";
    let session = Session::new(Some("placeholder markdown artifact".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "",
        &json!({
            "executed_tools": ["write"],
            "requested_tools": ["write"],
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/assess-repository-scope.md".to_string(),
            artifact.to_string(),
        )),
        &snapshot,
    );

    assert!(accepted.is_none());
    assert_eq!(validation["validation_outcome"], "blocked");
    assert!(validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("placeholder_artifact")));
    assert!(rejected
        .as_deref()
        .unwrap_or_default()
        .contains("placeholder"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_rejects_required_tool_mode_failure_artifact() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-required-tool-marker-artifact-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
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
            "output_path": ".tandem/artifacts/collect-reddit-signals.json"
        }
    }));
    let artifact = "TOOL_MODE_REQUIRED_NOT_SATISFIED: WRITE_REQUIRED_NOT_SATISFIED: tool_mode=required but the model ended without executing a productive tool call.";
    let session = Session::new(Some("required tool failure marker".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "",
        &json!({
            "executed_tools": ["mcp_list"],
            "requested_tools": ["mcp_list", "write"],
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/collect-reddit-signals.json".to_string(),
            artifact.to_string(),
        )),
        &snapshot,
    );

    assert!(accepted.is_none());
    assert!(validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("provider_required_tool_mode_unsatisfied")));
    assert!(rejected
        .as_deref()
        .unwrap_or_default()
        .contains("required-tool"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_rejects_web_unavailable_artifact_after_successful_websearch() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-web-research-contradiction-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "gather_market_sources".to_string();
    node.objective = "Use web_research and web_fetch to gather current market sources.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "citations".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: Some(crate::AutomationOutputEnforcement {
            validation_profile: Some("external_research".to_string()),
            required_tools: vec!["websearch".to_string()],
            required_tool_calls: Vec::new(),
            required_evidence: Vec::new(),
            required_sections: vec!["web_sources_reviewed".to_string()],
            prewrite_gates: vec!["successful_web_research".to_string()],
            retry_on_missing: vec!["missing_successful_web_research".to_string()],
            terminal_on: vec!["completed".to_string()],
            repair_budget: Some(2),
            session_text_recovery: None,
        }),
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/gather-market-sources.json"
        }
    }));
    let artifact = serde_json::to_string_pretty(&json!({
        "status": "completed",
        "node_id": "gather_market_sources",
        "web_research": {
            "status": "unavailable_in_current_tooling",
            "limitations": ["No websearch/webfetch tools were available in this workspace session."]
        },
        "sources_reviewed": [],
        "citations_external": [],
        "evidence_notes": ["No dated market or technical sources were captured in this attempt."]
    }))
    .expect("serialize artifact");
    let session = Session::new(Some("web contradiction".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "",
        &json!({
            "executed_tools": ["websearch", "write"],
            "requested_tools": ["websearch", "write"],
            "web_research_used": true,
            "web_research_succeeded": true,
            "web_research_citations": ["https://example.com/source"],
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/gather-market-sources.json".to_string(),
            artifact,
        )),
        &snapshot,
    );

    assert!(accepted.is_none());
    assert!(validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| {
            value.as_str() == Some("web_research_artifact_contradicts_tool_receipts")
        }));
    assert!(rejected
        .as_deref()
        .unwrap_or_default()
        .contains("web research was unavailable"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_accepts_structured_json_handoff_from_verified_artifact() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-structured-handoff-artifact-{}",
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
        "artifact_type": "collect_reddit_signals",
        "status": "completed",
        "findings": [{
            "title": "Agent cost anxiety",
            "source_url": "https://www.reddit.com/r/vibecoding/comments/1t6bys1/",
            "subreddit": "r/vibecoding",
            "permalink": "https://www.reddit.com/r/vibecoding/comments/1t6bys1/",
            "relevance_rationale": "Discusses token costs and AI agent workflow friction."
        }],
        "tool_evidence": [{
            "tool": "mcp.reddit_gmail.reddit_search_across_subreddits",
            "result_excerpt": "Returned Reddit posts about AI agent costs and reliability."
        }]
    }))
    .expect("serialize artifact");
    let session = Session::new(Some("compact final status".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
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

    assert!(accepted.is_some());
    assert_eq!(validation["validation_outcome"], "passed");
    assert!(!validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("structured_handoff_missing")));
    assert!(rejected.is_none());

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_accepts_concrete_mcp_source_from_top_level_diagnostics() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-top-level-mcp-diagnostics-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "extract_pain_points".to_string();
    node.objective = "Use Reddit MCP to extract pain points from connector-backed source research about agent reliability. Retrieve representative Reddit posts or comments, summarize recurring pain points, and write a structured JSON artifact with source identifiers.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/extract-pain-points.json"
        }
    }));
    let artifact = serde_json::to_string_pretty(&json!({
        "status": "completed",
        "pain_points": [{
            "theme": "Tool-calling failures",
            "source": "https://www.reddit.com/r/LLMDevs/comments/example/",
            "evidence": "A Reddit MCP search returned discussion about agent runtime behavior."
        }],
        "source_evidence": [{
            "tool": "mcp.reddit_gmail.reddit_search_across_subreddits",
            "result": "success"
        }]
    }))
    .expect("serialize artifact");
    let session = Session::new(Some("{\"status\":\"completed\"}".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &json!({
            "executed_tools": [
                "mcp_list",
                "mcp.reddit_gmail.reddit_search_across_subreddits",
                "write"
            ],
            "requested_tools": [
                "mcp_list",
                "mcp.reddit_gmail.*",
                "write"
            ],
            "capability_resolution": {},
            "mcp_tool_diagnostics": {
                "selected_servers": ["reddit-gmail"]
            },
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/extract-pain-points.json".to_string(),
            artifact,
        )),
        &snapshot,
    );

    assert!(accepted.is_some(), "{validation:#}");
    assert_eq!(validation["validation_outcome"], "passed");
    assert!(!validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("mcp_connector_source_missing")));
    assert!(rejected.is_none(), "{rejected:?}");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_accepts_required_connector_limitation_when_source_tool_unavailable() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-required-mcp-limitation-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "gather_reddit_signals".to_string();
    node.objective = "Use Reddit MCP to research current community discussions about making AI agents reliable for business workflows. Retrieve representative posts or comments and summarize recurring signals with Reddit source links or identifiers.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/gather-reddit-signals.json"
        },
        "tool_allowlist": [
            "mcp_list",
            "mcp.reddit_gmail.*",
            "write"
        ]
    }));
    let artifact = serde_json::to_string_pretty(&json!({
        "status": "completed",
        "node_id": "gather_reddit_signals",
        "recurring_signals": [],
        "connector_limitations": [{
            "connector": "reddit-gmail",
            "limitation": "No concrete mcp.reddit_gmail.* source tool was available in this attempt after discovery."
        }],
        "source_limitations": [
            "The Reddit connector was unavailable, so this run records the limitation instead of fabricating Reddit evidence."
        ]
    }))
    .expect("serialize artifact");
    let session = Session::new(Some("{\"status\":\"completed\"}".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &json!({
            "executed_tools": ["mcp_list", "write"],
            "requested_tools": ["mcp_list", "write"],
            "capability_resolution": {
                "mcp_tool_diagnostics": {
                    "selected_servers": ["reddit-gmail"],
                    "missing_selected_servers": ["reddit-gmail"],
                    "servers": [{
                        "name": "reddit-gmail",
                        "exists": false,
                        "enabled": false,
                        "connected": false,
                        "sync_error": "server_not_found"
                    }]
                }
            },
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/gather-reddit-signals.json".to_string(),
            artifact,
        )),
        &snapshot,
    );

    assert!(accepted.is_some(), "{validation:#}");
    assert_eq!(validation["validation_outcome"], "passed");
    assert!(!validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("mcp_connector_source_missing")));
    assert!(rejected.is_none(), "{rejected:?}");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn research_synthesis_rejects_unconfirmed_notion_identity_overstatement() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-synthesis-notion-overstatement-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "synthesize_report".to_string();
    node.objective = "Synthesize upstream source artifacts into a final report.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "brief".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/synthesize-report.json"
        }
    }));
    let artifact = serde_json::to_string_pretty(&json!({
        "status": "completed",
        "report_body": "## Summary\nThe report is ready.\n\n## Tandem Run details\nUpstream Notion inspection artifact recorded that the target was the existing page f3975ce7-1d8d-4531-8bea-2812c65f209b.",
        "citations": ["https://docs.tandem.ac/start-here/"]
    }))
    .expect("serialize artifact");
    let session = Session::new(Some("{\"status\":\"completed\"}".to_string()), None);
    let upstream = AutomationUpstreamEvidence {
        notion_identity_unconfirmed: true,
        citation_count: 1,
        citations: vec!["https://docs.tandem.ac/start-here/".to_string()],
        ..Default::default()
    };
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output_with_upstream(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        Some("automation-v2-run-test"),
        "{\"status\":\"completed\"}",
        &json!({
            "executed_tools": ["write"],
            "requested_tools": ["write"],
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/synthesize-report.json".to_string(),
            artifact,
        )),
        &snapshot,
        Some(&upstream),
    );

    assert!(accepted.is_some(), "{validation:#}");
    assert_eq!(validation["validation_outcome"], "blocked");
    assert!(validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("upstream_notion_identity_overstated")));
    assert!(rejected
        .as_deref()
        .unwrap_or_default()
        .contains("Notion inspection"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn research_synthesis_rejects_market_claims_when_external_citations_missing() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-synthesis-uncited-market-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "synthesize_report".to_string();
    node.objective = "Synthesize upstream source artifacts into a final report.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "brief".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::ResearchBrief),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/synthesize-report.json"
        }
    }));
    let artifact = serde_json::to_string_pretty(&json!({
        "status": "completed",
        "report_body": "## Summary\nReliable agents are a systems problem.\n\n## Market Notes\nThe safest market read is that vendors are converging around orchestration and governance.",
        "citations": ["https://docs.tandem.ac/start-here/"]
    }))
    .expect("serialize artifact");
    let session = Session::new(Some("{\"status\":\"completed\"}".to_string()), None);
    let upstream = AutomationUpstreamEvidence {
        external_citations_missing: true,
        citation_count: 1,
        citations: vec!["https://docs.tandem.ac/start-here/".to_string()],
        ..Default::default()
    };
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output_with_upstream(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        Some("automation-v2-run-test"),
        "{\"status\":\"completed\"}",
        &json!({
            "executed_tools": ["write"],
            "requested_tools": ["write"],
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/synthesize-report.json".to_string(),
            artifact,
        )),
        &snapshot,
        Some(&upstream),
    );

    assert!(accepted.is_some(), "{validation:#}");
    assert_eq!(validation["validation_outcome"], "blocked");
    assert!(validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| {
            value.as_str() == Some("uncited_market_claims_from_limited_web_artifact")
        }));
    assert!(rejected
        .as_deref()
        .unwrap_or_default()
        .contains("market/web-backed claims"));

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_accepts_optional_tandem_mcp_reference_without_connector_call() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-optional-mcp-reference-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "gather_tandem_reference".to_string();
    node.objective = "Use Tandem MCP docs as reference if needed via mcp.tandem_mcp.search_docs, mcp.tandem_mcp.get_doc, mcp.tandem_mcp.get_tandem_guide, or mcp.tandem_mcp.answer_how_to to collect relevant Tandem guidance for reliable automation runs, workflow validation, approvals, connector use, and Tandem Run details. Return only relevant excerpts and citations; do not invent undocumented Tandem behavior.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/gather-tandem-reference.json"
        },
        "tool_allowlist": [
            "mcp.tandem_mcp.search_docs",
            "mcp.tandem_mcp.get_doc",
            "mcp.tandem_mcp.get_tandem_guide",
            "mcp.tandem_mcp.answer_how_to"
        ]
    }));
    let artifact = serde_json::to_string_pretty(&json!({
        "status": "completed",
        "citations": [],
        "rationale": "No Tandem docs context was needed for the upstream findings in this run."
    }))
    .expect("serialize artifact");
    let session = Session::new(Some("{\"status\":\"completed\"}".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &json!({
            "executed_tools": ["write"],
            "requested_tools": [
                "mcp_list",
                "mcp.tandem_mcp.search_docs",
                "mcp.tandem_mcp.get_doc",
                "mcp.tandem_mcp.get_tandem_guide",
                "mcp.tandem_mcp.answer_how_to",
                "write"
            ],
            "capability_resolution": {
                "mcp_tool_diagnostics": {
                    "selected_servers": ["tandem-mcp"]
                }
            },
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/gather-tandem-reference.json".to_string(),
            artifact,
        )),
        &snapshot,
    );

    assert!(accepted.is_some(), "{validation:#}");
    assert_eq!(validation["validation_outcome"], "passed");
    assert!(!validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("mcp_connector_source_missing")));
    assert!(rejected.is_none(), "{rejected:?}");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[test]
fn validation_accepts_notion_fetch_markdown_as_connector_source_evidence() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-notion-fetch-artifact-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "confirm_notion_target".to_string();
    node.objective =
        "Use mcp.notion.notion_fetch to confirm the Notion target database.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "text_summary".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
        enforcement: None,
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/confirm-notion-target.md"
        },
        "tool_allowlist": [
            "mcp.notion.notion_fetch",
            "mcp.notion.notion_search",
            "write"
        ]
    }));
    let artifact = "Confirmed the Notion target using `mcp.notion.notion_fetch`.\n\nSource evidence: `collection://database-id` returned a Notion database target named `AI productivity signals`; no connector limitation was observed.\n";
    let session = Session::new(Some("notion target confirmation".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &json!({
            "executed_tools": [
                "mcp_list",
                "mcp.notion.notion_fetch",
                "write"
            ],
            "requested_tools": [
                "mcp_list",
                "mcp.notion.notion_fetch",
                "mcp.notion.notion_search",
                "write"
            ],
            "capability_resolution": {
                "mcp_tool_diagnostics": {
                    "selected_servers": ["notion"]
                }
            },
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/confirm-notion-target.md".to_string(),
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
fn structured_json_connector_fetch_does_not_require_workspace_inspection() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-notion-row-inspection-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&workspace_root).expect("create workspace");
    let mut node = bare_node();
    node.node_id = "inspect_notion_row".to_string();
    node.objective = "Fetch and inspect only the existing Notion database row at https://www.notion.so/f3975ce71d8d45318bea2812c65f209b inside Operational Workflow Results collection://892d3e9b-2bf8-4b3e-a541-dc725f77295d, confirming the target page/row identity and current editable fields. Do not create a database, top-level page, workspace page, or new database row.".to_string();
    node.output_contract = Some(AutomationFlowOutputContract {
        kind: "structured_json".to_string(),
        validator: Some(crate::AutomationOutputValidatorKind::StructuredJson),
        enforcement: Some(crate::AutomationOutputEnforcement {
            validation_profile: Some("artifact_only".to_string()),
            required_tools: vec!["mcp.notion.notion_fetch".to_string()],
            required_tool_calls: Vec::new(),
            required_evidence: Vec::new(),
            required_sections: Vec::new(),
            prewrite_gates: vec!["workspace_inspection".to_string()],
            retry_on_missing: vec!["workspace_inspection".to_string()],
            terminal_on: vec!["completed".to_string()],
            repair_budget: Some(2),
            session_text_recovery: Some("require_prewrite_satisfied".to_string()),
        }),
        schema: None,
        summary_guidance: None,
    });
    node.metadata = Some(json!({
        "builder": {
            "output_path": ".tandem/artifacts/inspect-notion-row.json"
        },
        "tool_allowlist": [
            "mcp.notion.*",
            "mcp.notion.notion_fetch",
            "write"
        ]
    }));
    let artifact = json!({
        "status": "completed",
        "target": {
            "url": "https://www.notion.so/f3975ce71d8d45318bea2812c65f209b",
            "page_id": "f3975ce71d8d45318bea2812c65f209b",
            "collection": "collection://892d3e9b-2bf8-4b3e-a541-dc725f77295d"
        },
        "source_evidence": {
            "tool": "mcp.notion.notion_fetch",
            "result": "Fetched existing Notion page in Operational Workflow Results with editable properties."
        },
        "editable_fields": ["Name", "Status", "Summary", "Evidence", "Sources", "Run ID"]
    })
    .to_string();
    let session = Session::new(Some("notion row inspection".to_string()), None);
    let snapshot = std::collections::BTreeSet::new();

    let (accepted, validation, rejected) = validate_automation_artifact_output(
        &node,
        &session,
        workspace_root.to_str().expect("workspace root"),
        "{\"status\":\"completed\"}",
        &json!({
            "executed_tools": [
                "mcp_list",
                "mcp.notion.notion_fetch",
                "write"
            ],
            "requested_tools": [
                "mcp_list",
                "mcp.notion.*",
                "mcp.notion.notion_fetch",
                "write"
            ],
            "capability_resolution": {
                "mcp_tool_diagnostics": {
                    "selected_servers": ["notion"]
                }
            },
            "verified_output_materialized_by_current_attempt": true
        }),
        None,
        Some((
            ".tandem/artifacts/inspect-notion-row.json".to_string(),
            artifact,
        )),
        &snapshot,
    );

    assert!(accepted.is_some());
    assert_eq!(validation["validation_outcome"], "passed");
    assert!(!validation["unmet_requirements"]
        .as_array()
        .expect("unmet array")
        .iter()
        .any(|value| value.as_str() == Some("workspace_inspection_required")));
    assert!(rejected.is_none(), "{rejected:?}");

    let _ = std::fs::remove_dir_all(&workspace_root);
}

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
