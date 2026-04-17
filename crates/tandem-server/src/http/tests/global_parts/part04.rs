
#[tokio::test]
async fn automation_v2_publish_block_smoke_skips_external_action_receipts() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let automation = crate::AutomationV2Spec {
        automation_id: "auto-v2-smoke-editorial-publish".to_string(),
        name: "Editorial Publish Smoke".to_string(),
        description: Some("Publish is blocked until editorial issues are resolved".to_string()),
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: vec![crate::AutomationAgentProfile {
            agent_id: "publisher".to_string(),
            template_id: None,
            display_name: "Publisher".to_string(),
            avatar_url: None,
            model_policy: None,
            skills: Vec::new(),
            tool_policy: crate::AutomationAgentToolPolicy {
                allowlist: vec!["workflow_test.slack".to_string()],
                denylist: Vec::new(),
            },
            mcp_policy: crate::AutomationAgentMcpPolicy {
                allowed_servers: Vec::new(),
                allowed_tools: None,
            },
            approval_policy: None,
        }],
        flow: crate::AutomationFlowSpec {
            nodes: vec![
                crate::AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "draft-report".to_string(),
                    agent_id: "publisher".to_string(),
                    objective: "Draft the final markdown report".to_string(),
                    depends_on: Vec::new(),
                    input_refs: Vec::new(),
                    output_contract: Some(crate::AutomationFlowOutputContract {
                        kind: "report_markdown".to_string(),
                        validator: Some(crate::AutomationOutputValidatorKind::GenericArtifact),
                        enforcement: None,
                        schema: None,
                        summary_guidance: None,
                    }),
                    retry_policy: None,
                    timeout_ms: None,
                    max_tool_calls: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "output_path": "final-report.md",
                            "role": "writer"
                        }
                    })),
                },
                crate::AutomationFlowNode {
                    knowledge: tandem_orchestrator::KnowledgeBinding::default(),
                    node_id: "publish-report".to_string(),
                    agent_id: "publisher".to_string(),
                    objective: "Publish the final report to Slack".to_string(),
                    depends_on: vec!["draft-report".to_string()],
                    input_refs: vec![crate::AutomationFlowInputRef {
                        from_step_id: "draft-report".to_string(),
                        alias: "draft".to_string(),
                    }],
                    output_contract: None,
                    retry_policy: None,
                    timeout_ms: None,
                    max_tool_calls: None,
                    stage_kind: Some(crate::AutomationNodeStageKind::Workstream),
                    gate: None,
                    metadata: Some(json!({
                        "builder": {
                            "role": "publisher"
                        }
                    })),
                },
            ],
        },
        execution: crate::AutomationExecutionPolicy {
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: vec!["final-report.md".to_string()],
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/tmp".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("store automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Blocked;
            row.detail = Some("publish is blocked pending editorial fixes".to_string());
            row.checkpoint.pending_nodes = vec!["publish-report".to_string()];
            row.checkpoint.blocked_nodes =
                vec!["draft-report".to_string(), "publish-report".to_string()];
            row.checkpoint.node_outputs.insert(
                "draft-report".to_string(),
                json!({
                    "node_id": "draft-report",
                    "status": "blocked",
                    "workflow_class": "artifact",
                    "phase": "editorial_validation",
                    "failure_kind": "editorial_quality_failed",
                    "summary": "Blocked editorial draft is too weak to publish.",
                    "validator_kind": "generic_artifact",
                    "validator_summary": {
                        "kind": "generic_artifact",
                        "outcome": "blocked",
                        "reason": "editorial artifact is missing expected markdown structure",
                        "unmet_requirements": ["editorial_substance_missing", "markdown_structure_missing"]
                    },
                    "artifact_validation": {
                        "accepted_artifact_path": "final-report.md",
                        "heading_count": 1,
                        "paragraph_count": 1,
                        "repair_attempted": false,
                        "repair_succeeded": false,
                        "unmet_requirements": ["editorial_substance_missing", "markdown_structure_missing"]
                    }
                }),
            );
            row.checkpoint.node_outputs.insert(
                "publish-report".to_string(),
                json!({
                    "node_id": "publish-report",
                    "status": "blocked",
                    "workflow_class": "artifact",
                    "phase": "editorial_validation",
                    "failure_kind": "editorial_quality_failed",
                    "summary": "Publish blocked until editorial issues are resolved.",
                    "validator_summary": {
                        "outcome": "blocked",
                        "reason": "publish step blocked until upstream editorial issues are resolved: draft-report",
                        "unmet_requirements": ["editorial_clearance_required"]
                    },
                    "artifact_validation": {
                        "unmet_requirements": ["editorial_clearance_required"],
                        "semantic_block_reason": "publish step blocked until upstream editorial issues are resolved: draft-report"
                    }
                }),
            );
        })
        .await
        .expect("update run");

    let run_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/automations/v2/runs/{}", run.run_id))
                .body(Body::empty())
                .expect("run request"),
        )
        .await
        .expect("run response");
    assert_eq!(run_resp.status(), StatusCode::OK);
    let run_body = to_bytes(run_resp.into_body(), usize::MAX)
        .await
        .expect("run body");
    let run_payload: Value = serde_json::from_slice(&run_body).expect("run json");
    let publish_output = run_payload
        .get("run")
        .and_then(|value| value.get("checkpoint"))
        .and_then(|value| value.get("node_outputs"))
        .and_then(|value| value.get("publish-report"))
        .expect("publish output");
    assert_eq!(
        publish_output.get("failure_kind").and_then(Value::as_str),
        Some("editorial_quality_failed")
    );
    assert_eq!(
        publish_output.get("phase").and_then(Value::as_str),
        Some("editorial_validation")
    );
    assert_eq!(
        publish_output
            .get("validator_summary")
            .and_then(|value| value.get("unmet_requirements"))
            .and_then(Value::as_array)
            .map(|rows| rows.clone()),
        Some(vec![json!("editorial_clearance_required")])
    );

    let external_actions_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/external-actions?limit=10")
                .body(Body::empty())
                .expect("external actions request"),
        )
        .await
        .expect("external actions response");
    assert_eq!(external_actions_resp.status(), StatusCode::OK);
    let external_actions_body = to_bytes(external_actions_resp.into_body(), usize::MAX)
        .await
        .expect("external actions body");
    let external_actions_payload: Value =
        serde_json::from_slice(&external_actions_body).expect("external actions json");
    assert_eq!(
        external_actions_payload
            .get("actions")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
}
