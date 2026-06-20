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
                    tool_policy: None,
                    mcp_policy: None,
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
                    tool_policy: None,
                    mcp_policy: None,
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
            profile: None,
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

#[tokio::test]
async fn automations_v2_run_cancel_records_operator_stop_kind_and_clears_active_ids() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-stop-kind").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    let _ = state
        .add_automation_v2_session(&run.run_id, "session-a")
        .await;
    let _ = state
        .add_automation_v2_session(&run.run_id, "session-b")
        .await;
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Running;
            row.active_session_ids = vec!["session-a".to_string(), "session-b".to_string()];
            row.active_instance_ids = vec!["instance-a".to_string()];
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/cancel", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "reason": "kill switch triggered by operator" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let context_run_id = payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        payload.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("cancelled")
    );
    assert!(payload
        .get("run")
        .and_then(|value| value.get("activeSessionIDs"))
        .and_then(Value::as_array)
        .map(|values| values.is_empty())
        .unwrap_or(true));
    assert!(payload
        .get("run")
        .and_then(|value| value.get("activeInstanceIDs"))
        .and_then(Value::as_array)
        .map(|values| values.is_empty())
        .unwrap_or(true));

    let cancelled = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("cancelled run");
    assert_eq!(cancelled.status, crate::AutomationRunStatus::Cancelled);
    assert_eq!(
        cancelled.stop_kind,
        Some(crate::AutomationStopKind::OperatorStopped)
    );
    assert_eq!(
        cancelled.stop_reason.as_deref(),
        Some("kill switch triggered by operator")
    );
    assert!(cancelled.active_session_ids.is_empty());
    assert!(cancelled.active_instance_ids.is_empty());
    state
        .apply_provider_usage_to_runs("session-a", 10, 20, 30)
        .await;
    let after_usage = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after late usage");
    assert_eq!(after_usage.total_tokens, 0);
    let stop_event = cancelled
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|entry| entry.event == "run_stopped")
        .expect("run stopped event");
    assert_eq!(
        stop_event.stop_kind,
        Some(crate::AutomationStopKind::OperatorStopped)
    );
    assert_eq!(
        stop_event.reason.as_deref(),
        Some("kill switch triggered by operator")
    );
}

#[tokio::test]
async fn automations_v2_run_cancel_is_idempotent_for_terminal_runs() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-terminal-cancel").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    let _ = state
        .add_automation_v2_session(&run.run_id, "session-terminal-a")
        .await;
    let _ = state
        .add_automation_v2_session(&run.run_id, "session-terminal-b")
        .await;
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Completed;
            row.detail = Some("completed before operator cleanup".to_string());
            row.active_session_ids = vec![
                "session-terminal-a".to_string(),
                "session-terminal-b".to_string(),
            ];
            row.latest_session_id = Some("session-terminal-b".to_string());
            row.active_instance_ids = vec!["instance-terminal-a".to_string()];
        })
        .await
        .expect("completed run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/cancel", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "reason": "cleanup old row" }).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("alreadyTerminal").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("completed")
    );
    assert!(payload
        .get("run")
        .and_then(|value| value.get("activeSessionIDs"))
        .and_then(Value::as_array)
        .map(|values| values.is_empty())
        .unwrap_or(true));
    assert!(payload
        .get("run")
        .and_then(|value| value.get("activeInstanceIDs"))
        .and_then(Value::as_array)
        .map(|values| values.is_empty())
        .unwrap_or(true));
    let context_run_id = payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        payload.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );

    let stored = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("stored run");
    assert_eq!(stored.status, crate::AutomationRunStatus::Completed);
    assert_eq!(
        stored.detail.as_deref(),
        Some("completed before operator cleanup")
    );
    assert!(stored.active_session_ids.is_empty());
    assert!(stored.active_instance_ids.is_empty());
    assert!(stored.latest_session_id.is_none());
    state
        .apply_provider_usage_to_runs("session-terminal-a", 10, 20, 30)
        .await;
    let after_usage = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after late usage");
    assert_eq!(after_usage.total_tokens, 0);
}

#[tokio::test]
async fn automations_v2_run_pause_clears_active_sessions_and_instances() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let automation = create_test_automation_v2(&state, "auto-v2-pause-active-cleanup").await;
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("run");
    let _ = state
        .add_automation_v2_session(&run.run_id, "session-a")
        .await;
    let _ = state
        .add_automation_v2_session(&run.run_id, "session-b")
        .await;
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = crate::AutomationRunStatus::Running;
            row.active_session_ids = vec!["session-a".to_string(), "session-b".to_string()];
            row.active_instance_ids = vec!["instance-a".to_string(), "instance-b".to_string()];
        })
        .await
        .expect("updated run");

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/automations/v2/runs/{}/pause", run.run_id))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "reason": "pause for operator checkpoint" }).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let context_run_id = payload
        .get("contextRunID")
        .and_then(Value::as_str)
        .expect("context run id");
    assert_eq!(
        payload.get("linked_context_run_id").and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(context_run_id)
    );
    assert_eq!(
        payload
            .get("run")
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str),
        Some("paused")
    );
    assert!(payload
        .get("run")
        .and_then(|value| value.get("activeSessionIDs"))
        .and_then(Value::as_array)
        .map(|values| values.is_empty())
        .unwrap_or(true));
    assert!(payload
        .get("run")
        .and_then(|value| value.get("activeInstanceIDs"))
        .and_then(Value::as_array)
        .map(|values| values.is_empty())
        .unwrap_or(true));

    let paused = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("paused run");
    assert_eq!(paused.status, crate::AutomationRunStatus::Paused);
    assert!(paused.active_session_ids.is_empty());
    assert!(paused.active_instance_ids.is_empty());
    let pause_event = paused
        .checkpoint
        .lifecycle_history
        .iter()
        .find(|entry| entry.event == "run_paused")
        .expect("run paused event");
    assert_eq!(
        pause_event.reason.as_deref(),
        Some("pause for operator checkpoint")
    );
    state
        .apply_provider_usage_to_runs("session-a", 10, 20, 30)
        .await;
    let after_usage = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("run after late usage");
    assert_eq!(after_usage.total_tokens, 0);
}
