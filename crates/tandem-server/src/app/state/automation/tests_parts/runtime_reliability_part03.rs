// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[test]
fn publish_verified_output_limits_planner_node_to_owned_artifact_target() {
    let workspace_root = std::env::temp_dir().join(format!(
        "tandem-publish-planner-target-{}",
        uuid::Uuid::new_v4()
    ));
    let run_artifact = workspace_root.join(
        ".tandem/runs/run-planner-target/artifacts/validate-incident-event.md",
    );
    std::fs::create_dir_all(run_artifact.parent().expect("run artifact parent"))
        .expect("create run artifact parent");
    std::fs::write(&run_artifact, "# Validated Incident Context\n")
        .expect("write run artifact");

    let mut source_node = bare_node();
    source_node.node_id = "validate_incident_event".to_string();
    source_node.objective = "Validate the incident event.".to_string();
    source_node.metadata = Some(json!({
        "artifact": {
            "path": "demo-output/context/incident-context.md",
            "visible_in_run": true
        }
    }));
    let mut final_node = bare_node();
    final_node.node_id = "publish_approved_update".to_string();
    final_node.depends_on = vec![source_node.node_id.clone()];

    let automation = AutomationV2Spec {
        automation_id: "automation-planner-targets".to_string(),
        name: "Planner targets".to_string(),
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
        output_targets: vec![
            "demo-output/context/incident-context.md".to_string(),
            "demo-output/drafts/incident-update.md".to_string(),
            "demo-output/approved/incident-update.md".to_string(),
        ],
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
        "run-planner-target",
        &source_node,
        &(
            ".tandem/runs/run-planner-target/artifacts/validate-incident-event.md".to_string(),
            "# Validated Incident Context\n".to_string(),
        ),
    )
    .expect("publish owned planner target");

    assert_eq!(result["targets"].as_array().map(Vec::len), Some(1));
    assert_eq!(result["targets"][0]["path"], "demo-output/context/incident-context.md");
    assert!(workspace_root.join("demo-output/context/incident-context.md").exists());
    assert!(!workspace_root.join("demo-output/drafts/incident-update.md").exists());
    assert!(!workspace_root.join("demo-output/approved/incident-update.md").exists());

    let _ = std::fs::remove_dir_all(&workspace_root);
}
