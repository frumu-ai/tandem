
#[tokio::test]
async fn workflow_plan_import_rejects_runnable_lifecycle_state() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state.clone());

    let import_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/import")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "bundle": {
                            "bundle_version": "1",
                            "plan": {
                                "plan_id": "plan_runnable_state",
                                "plan_revision": 1,
                                "lifecycle_state": "applied",
                                "owner": {
                                    "owner_id": "control-panel",
                                    "scope": "workspace",
                                    "audience": "internal"
                                },
                                "mission": {
                                    "goal": "Import with runnable lifecycle",
                                    "summary": null,
                                    "domain": "workflow"
                                },
                                "success_criteria": {
                                    "required_artifacts": [],
                                    "minimum_viable_completion": null,
                                    "minimum_output": null,
                                    "freshness_window_hours": null
                                },
                                "routine_graph": [],
                                "connector_intents": [],
                                "connector_bindings": [],
                                "credential_envelopes": [],
                                "context_objects": [],
                                "metadata": null
                            },
                            "scope_snapshot": {
                                "plan_id": "plan_runnable_state",
                                "plan_revision": 1,
                                "output_roots": null,
                                "inter_routine_policy": null,
                                "credential_envelopes": [],
                                "context_objects": [],
                                "routine_scopes": []
                            }
                        }
                    })
                    .to_string(),
                ))
                .expect("import request"),
        )
        .await
        .expect("import response");
    assert_eq!(import_resp.status(), StatusCode::BAD_REQUEST);
    let import_body = to_bytes(import_resp.into_body(), usize::MAX)
        .await
        .expect("import body");
    let import_payload: Value = serde_json::from_slice(&import_body).expect("import json");
    assert_eq!(
        import_payload
            .get("import_validation")
            .and_then(|value| value.get("compatible"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert!(import_payload
        .get("import_validation")
        .and_then(|value| value.get("issues"))
        .and_then(Value::as_array)
        .map(|issues| {
            issues.iter().any(|issue| {
                issue.get("code").and_then(Value::as_str)
                    == Some("import_requires_preview_lifecycle")
            })
        })
        .unwrap_or(false));
}

#[tokio::test]
async fn workflow_plan_apply_normalizes_mcp_server_prefixes_into_tool_allowlist() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state.clone());
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Delivery Workflow",
                "Write a report and email it.",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![
                    step_json("generate_report", "report", "Generate the report.", &[], "writer", json!([]), "report_markdown"),
                    step_json("notify_user", "notify", "Send the report by email.", &["generate_report"], "operator", json!([
                        {"from_step_id":"generate_report","alias":"final_report"}
                    ]), "text_summary")
                ],
                Some(json!({
                    "model_provider": "openai",
                    "model_id": "gpt-5.1"
                }))
            )
        })
        .to_string(),
    );

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Generate a report and send it by email",
            "plan_source": "automations_page",
            "allowed_mcp_servers": ["composio-1"],
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": {
                "model_provider": "openai",
                "model_id": "gpt-5.1"
            }
        })))
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let plan_id = preview_payload
        .get("plan")
        .and_then(|plan| plan.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel"
            })
            .to_string(),
        ))
        .expect("apply request");
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automation_id = apply_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");
    let stored = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation");
    let operator_agent = stored
        .agents
        .iter()
        .find(|agent| agent.agent_id == "agent_operator")
        .expect("operator agent");
    assert!(operator_agent
        .tool_policy
        .allowlist
        .contains(&"mcp.composio_1.*".to_string()));
}

#[tokio::test]
async fn workflow_plan_apply_succeeds_when_legacy_automations_file_is_stale() {
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_STATE_DIR",
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    let state_root =
        std::env::temp_dir().join(format!("tandem-workflow-plan-legacy-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&state_root).expect("state root");
    std::fs::write(
        state_root.join("automations_v2.json"),
        r#"{
  "legacy-automation": {
    "automation_id": "legacy-automation",
    "name": "Legacy Automation",
    "description": "stale legacy file",
    "enabled": true,
    "trigger": {
      "type": "manual"
    },
    "schedule": {
      "type": "manual",
      "timezone": "UTC",
      "misfire_policy": {
        "type": "run_once"
      }
    },
    "agents": [],
    "flow": {
      "nodes": [],
      "edges": []
    },
    "created_at_ms": 1,
    "updated_at_ms": 1
  }
}"#,
    )
    .expect("write stale legacy automations file");
    _guard.set("TANDEM_STATE_DIR", state_root.display().to_string());

    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state.clone());
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Delivery Workflow",
                "Write a report and notify the operator.",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![
                    step_json("generate_report", "report", "Generate the report.", &[], "writer", json!([]), "report_markdown"),
                    step_json("notify_user", "notify", "Notify the operator.", &["generate_report"], "operator", json!([
                        {"from_step_id":"generate_report","alias":"final_report"}
                    ]), "text_summary")
                ],
                Some(json!({
                    "model_provider": "openai",
                    "model_id": "gpt-5.1"
                }))
            )
        })
        .to_string(),
    );

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Generate a report and notify the operator",
            "plan_source": "automations_page",
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": {
                "model_provider": "openai",
                "model_id": "gpt-5.1"
            }
        })))
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");
    let plan_id = preview_payload
        .get("plan")
        .and_then(|plan| plan.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let apply_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/apply")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "plan_id": plan_id,
                        "creator_id": "control-panel"
                    })
                    .to_string(),
                ))
                .expect("apply request"),
        )
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    let automation_id = apply_payload
        .get("automation")
        .and_then(|row| row.get("automation_id"))
        .and_then(Value::as_str)
        .expect("automation id");

    let canonical_path = state_root.join("data").join("automations_v2.json");
    let canonical_raw =
        std::fs::read_to_string(&canonical_path).expect("read canonical automations file");
    let canonical_json: Value = serde_json::from_str(&canonical_raw).expect("canonical json");
    assert!(canonical_json.get(automation_id).is_some());

    assert!(
        !state_root.join("automations_v2.json").exists(),
        "stale legacy automations file should be removed once canonical persistence succeeds"
    );
}

#[tokio::test]
async fn load_automations_v2_prefers_canonical_file_over_stale_legacy_entries() {
    let _guard = PlannerEnvGuard::new(&["TANDEM_STATE_DIR"]);
    let state_root =
        std::env::temp_dir().join(format!("tandem-automation-v2-load-{}", Uuid::new_v4()));
    let canonical_dir = state_root.join("data");
    std::fs::create_dir_all(&canonical_dir).expect("canonical dir");
    std::fs::write(
        canonical_dir.join("automations_v2.json"),
        r#"{
  "automation-v2-current": {
    "automation_id": "automation-v2-current",
    "name": "Current Automation",
    "description": "canonical definition",
    "status": "active",
    "schedule": {
      "type": "manual",
      "timezone": "UTC",
      "misfire_policy": { "type": "run_once" }
    },
    "agents": [],
    "flow": { "nodes": [] },
    "execution": { "max_parallel_agents": 1 },
    "output_targets": [],
    "created_at_ms": 20,
    "updated_at_ms": 20,
    "creator_id": "test"
  }
}"#,
    )
    .expect("write canonical automations file");
    std::fs::write(
        state_root.join("automations_v2.json"),
        r#"{
  "automation-v2-stale": {
    "automation_id": "automation-v2-stale",
    "name": "Stale Legacy Automation",
    "description": "legacy definition",
    "status": "paused",
    "schedule": {
      "type": "manual",
      "timezone": "UTC",
      "misfire_policy": { "type": "run_once" }
    },
    "agents": [],
    "flow": { "nodes": [] },
    "execution": { "max_parallel_agents": 1 },
    "output_targets": [],
    "created_at_ms": 1,
    "updated_at_ms": 1,
    "creator_id": "legacy"
  }
}"#,
    )
    .expect("write legacy automations file");
    _guard.set("TANDEM_STATE_DIR", state_root.display().to_string());

    let state = test_state().await;
    let automations = state.list_automations_v2().await;
    let ids = automations
        .iter()
        .map(|row| row.automation_id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, vec!["automation-v2-current"]);
    assert!(
        !state_root.join("automations_v2.json").exists(),
        "stale legacy automations file should be removed when canonical definitions load"
    );
}

#[tokio::test]
async fn workflow_plan_chat_message_returns_planner_model_hint_without_model() {
    let state = test_state().await;
    let app = app_router(state);

    let start_resp = app
        .clone()
        .oneshot(chat_start_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/initial-workspace"
        })))
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let message_resp = app
        .clone()
        .oneshot(chat_message_request(
            plan_id,
            "Make this weekly and add analysis.",
        ))
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert!(message_payload
        .get("clarifier")
        .and_then(|row| row.get("question"))
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("planner model settings")));
}

#[tokio::test]
async fn workflow_plan_chat_message_uses_llm_revision_when_planner_model_is_configured() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state);
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Initial Workflow",
                "Initial workflow",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![step_json("execute_goal", "execute", "Execute the goal.", &[], "worker", json!([]), "structured_json")],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        json!({
            "action": "revise",
            "assistant_text": "Updated the workflow to research, analyze, report, and notify.",
            "change_summary": ["updated workflow plan"],
            "plan": llm_plan_json(
                "Market Workflow",
                "Research, analyze, report, and notify.",
                cron_schedule_json("0 9 * * 1"),
                "/tmp/initial-workspace",
                vec![
                    step_json("research_sources", "research", "Research the market.", &[], "researcher", json!([]), "structured_json"),
                    step_json("analyze_findings", "analyze", "Analyze the research.", &["research_sources"], "analyst", json!([
                        {"from_step_id":"research_sources","alias":"source_findings"}
                    ]), "structured_json"),
                    step_json("generate_report", "report", "Generate the report.", &["analyze_findings"], "writer", json!([
                        {"from_step_id":"analyze_findings","alias":"analysis"}
                    ]), "report_markdown"),
                    step_json("notify_user", "notify", "Email the report.", &["generate_report"], "writer", json!([
                        {"from_step_id":"generate_report","alias":"report"}
                    ]), "text_summary")
                ],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );

    let start_resp = app
        .clone()
        .oneshot(chat_start_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/initial-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let message_resp = app
        .clone()
        .oneshot(chat_message_request(
            plan_id,
            "Add a delivery step and make this weekly.",
        ))
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("steps"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(4)
    );
    assert_eq!(
        message_payload
            .get("plan")
            .and_then(|row| row.get("schedule"))
            .and_then(|row| row.get("cron_expression"))
            .and_then(Value::as_str),
        Some("0 9 * * 1")
    );
    assert!(message_payload.get("plan_package_bundle").is_some());
    assert!(message_payload.get("plan_package_replay").is_some());
    assert_eq!(
        message_payload
            .get("plan_package")
            .and_then(|row| row.get("plan_revision"))
            .and_then(Value::as_u64),
        Some(2)
    );
    let steps = message_payload
        .get("plan")
        .and_then(|row| row.get("steps"))
        .and_then(Value::as_array)
        .expect("steps");
    assert_eq!(
        steps[0]
            .get("output_contract")
            .and_then(|row| row.get("validator"))
            .and_then(Value::as_str),
        Some("structured_json")
    );
    assert_eq!(
        steps[2]
            .get("output_contract")
            .and_then(|row| row.get("validator"))
            .and_then(Value::as_str),
        Some("generic_artifact")
    );
    assert_eq!(
        steps[3]
            .get("output_contract")
            .and_then(|row| row.get("validator"))
            .and_then(Value::as_str),
        Some("generic_artifact")
    );

    let apply_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/apply")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "plan_id": plan_id,
                        "creator_id": "control-panel"
                    })
                    .to_string(),
                ))
                .expect("apply request"),
        )
        .await
        .expect("apply response");
    assert_eq!(apply_resp.status(), StatusCode::OK);
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    let apply_payload: Value = serde_json::from_slice(&apply_body).expect("apply json");
    assert_eq!(
        apply_payload
            .get("plan_package")
            .and_then(|row| row.get("plan_revision"))
            .and_then(Value::as_u64),
        Some(2)
    );
}

#[tokio::test]
async fn workflow_plan_chat_message_returns_clarify_from_llm() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state);
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Initial Workflow",
                "Initial workflow",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![step_json("execute_goal", "execute", "Execute the goal.", &[], "worker", json!([]), "structured_json")],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        json!({
            "action": "clarify",
            "assistant_text": "Do you want email delivery or a saved report only?",
            "clarifier": {
                "field": "general",
                "question": "Do you want email delivery or a saved report only?"
            }
        })
        .to_string(),
    );

    let start_resp = app
        .clone()
        .oneshot(chat_start_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/initial-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("start response");
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let message_resp = app
        .clone()
        .oneshot(chat_message_request(
            plan_id,
            "Make sure it gets delivered.",
        ))
        .await
        .expect("message response");
    assert_eq!(message_resp.status(), StatusCode::OK);
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("clarifier")
            .and_then(|row| row.get("question"))
            .and_then(Value::as_str),
        Some("Do you want email delivery or a saved report only?")
    );
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn workflow_plan_chat_message_returns_keep_from_llm() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state);
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Initial Workflow",
                "Initial workflow",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![step_json("execute_goal", "execute", "Execute the goal.", &[], "worker", json!([]), "structured_json")],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        json!({
            "action": "keep",
            "assistant_text": "The current workflow already matches the request."
        })
        .to_string(),
    );

    let start_resp = app
        .clone()
        .oneshot(chat_start_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/initial-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("start response");
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let message_resp = app
        .clone()
        .oneshot(chat_message_request(plan_id, "Keep it as-is."))
        .await
        .expect("message response");
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert_eq!(
        message_payload
            .get("assistant_message")
            .and_then(|row| row.get("text"))
            .and_then(Value::as_str),
        Some("The current workflow already matches the request.")
    );
    assert_eq!(
        message_payload
            .get("change_summary")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn workflow_plan_chat_message_falls_back_when_llm_revision_is_invalid() {
    let state = test_state().await;
    configure_openai_provider(&state).await;
    let app = app_router(state);
    let _guard = PlannerEnvGuard::new(&[
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        "TANDEM_WORKFLOW_PLANNER_TEST_RESPONSE",
    ]);
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_BUILD_RESPONSE",
        json!({
            "action": "build",
            "plan": llm_plan_json(
                "Initial Workflow",
                "Initial workflow",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![step_json("execute_goal", "execute", "Execute the goal.", &[], "worker", json!([]), "structured_json")],
                Some(planner_preferences())
            )
        })
        .to_string(),
    );
    _guard.set(
        "TANDEM_WORKFLOW_PLANNER_TEST_REVISION_RESPONSE",
        r#"{"action":"revise","plan":{"steps":[{"step_id":"custom_step"}]}}"#,
    );

    let start_resp = app
        .clone()
        .oneshot(chat_start_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/initial-workspace",
            "operator_preferences": planner_preferences()
        })))
        .await
        .expect("start response");
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let message_resp = app
        .clone()
        .oneshot(chat_message_request(plan_id, "Rewrite the workflow."))
        .await
        .expect("message response");
    let message_body = to_bytes(message_resp.into_body(), usize::MAX)
        .await
        .expect("message body");
    let message_payload: Value = serde_json::from_slice(&message_body).expect("message json");
    assert!(message_payload
        .get("clarifier")
        .and_then(|row| row.get("question"))
        .and_then(Value::as_str)
        .is_some_and(|text| text.contains("could not produce a valid workflow revision")));
}

#[tokio::test]
async fn workflow_plan_chat_reset_restores_initial_plan() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let start_resp = app
        .clone()
        .oneshot(chat_start_request(json!({
            "prompt": "Research the market and generate a report",
            "workspace_root": "/tmp/initial-workspace"
        })))
        .await
        .expect("start response");
    assert_eq!(start_resp.status(), StatusCode::OK);
    let start_body = to_bytes(start_resp.into_body(), usize::MAX)
        .await
        .expect("start body");
    let start_payload: Value = serde_json::from_slice(&start_body).expect("start json");
    let plan_id = start_payload
        .get("plan")
        .and_then(|row| row.get("plan_id"))
        .and_then(Value::as_str)
        .expect("plan id");

    let reset_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/chat/reset")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id
            })
            .to_string(),
        ))
        .expect("reset request");
    let reset_resp = app
        .clone()
        .oneshot(reset_req)
        .await
        .expect("reset response");
    assert_eq!(reset_resp.status(), StatusCode::OK);
    let draft = state.get_workflow_plan_draft(plan_id).await.expect("draft");
    assert_eq!(
        serde_json::to_value(&draft.initial_plan.steps).expect("initial steps"),
        serde_json::to_value(&draft.current_plan.steps).expect("current steps")
    );
}

#[tokio::test]
async fn workflow_planner_sessions_support_crud_and_duplication() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Write a small workflow plan for testing session CRUD",
            "workspace_root": "/tmp/custom-workspace"
        })))
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");

    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/sessions")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "project_slug": "planner-crud",
                        "title": "Planner CRUD",
                        "workspace_root": "/tmp/custom-workspace",
                        "goal": "Write a small workflow plan for testing session CRUD",
                        "notes": "seeded from preview payload",
                        "plan_source": "coding_task_planning",
                        "plan": preview_payload.get("plan").cloned(),
                        "conversation": preview_payload.get("conversation").cloned(),
                        "planner_diagnostics": preview_payload.get("planner_diagnostics").cloned(),
                    })
                    .to_string(),
                ))
                .expect("create request"),
        )
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    assert_eq!(
        create_payload
            .get("session")
            .and_then(|row| row.get("source_kind"))
            .and_then(Value::as_str),
        Some("planner")
    );
    let session_id = create_payload
        .get("session")
        .and_then(|row| row.get("session_id"))
        .and_then(Value::as_str)
        .expect("session id")
        .to_string();

    let list_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/workflow-plans/sessions?project_slug=planner-crud")
                .body(Body::empty())
                .expect("list request"),
        )
        .await
        .expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("list json");
    assert!(list_payload
        .get("sessions")
        .and_then(Value::as_array)
        .is_some_and(|rows| rows.iter().any(|row| {
            row.get("session_id").and_then(Value::as_str) == Some(session_id.as_str())
        })));
    assert!(list_payload
        .get("sessions")
        .and_then(Value::as_array)
        .is_some_and(|rows| rows
            .iter()
            .any(|row| { row.get("source_kind").and_then(Value::as_str) == Some("planner") })));

    let patch_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/workflow-plans/sessions/{session_id}"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "title": "Planner CRUD Renamed" }).to_string(),
                ))
                .expect("patch request"),
        )
        .await
        .expect("patch response");
    assert_eq!(patch_resp.status(), StatusCode::OK);
    let patch_body = to_bytes(patch_resp.into_body(), usize::MAX)
        .await
        .expect("patch body");
    let patch_payload: Value = serde_json::from_slice(&patch_body).expect("patch json");
    assert_eq!(
        patch_payload
            .get("session")
            .and_then(|row| row.get("title"))
            .and_then(Value::as_str),
        Some("Planner CRUD Renamed")
    );

    let duplicate_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/workflow-plans/sessions/{session_id}/duplicate"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "title": "Planner CRUD Copy" }).to_string(),
                ))
                .expect("duplicate request"),
        )
        .await
        .expect("duplicate response");
    assert_eq!(duplicate_resp.status(), StatusCode::OK);
    let duplicate_body = to_bytes(duplicate_resp.into_body(), usize::MAX)
        .await
        .expect("duplicate body");
    let duplicate_payload: Value = serde_json::from_slice(&duplicate_body).expect("duplicate json");
    let duplicate_session_id = duplicate_payload
        .get("session")
        .and_then(|row| row.get("session_id"))
        .and_then(Value::as_str)
        .expect("duplicate session id");
    assert_ne!(duplicate_session_id, session_id);
    assert_eq!(
        duplicate_payload
            .get("session")
            .and_then(|row| row.get("source_kind"))
            .and_then(Value::as_str),
        Some("forked_planner")
    );
}

#[tokio::test]
async fn workflow_planner_session_reset_recovers_missing_current_plan_id() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Write a small workflow plan for reset recovery",
            "workspace_root": "/tmp/custom-workspace"
        })))
        .await
        .expect("preview response");
    assert_eq!(preview_resp.status(), StatusCode::OK);
    let preview_body = to_bytes(preview_resp.into_body(), usize::MAX)
        .await
        .expect("preview body");
    let preview_payload: Value = serde_json::from_slice(&preview_body).expect("preview json");

    let create_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/sessions")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "project_slug": "planner-reset",
                        "title": "Planner Reset",
                        "workspace_root": "/tmp/custom-workspace",
                        "goal": "Write a small workflow plan for reset recovery",
                        "notes": "seeded from preview payload",
                        "plan_source": "coding_task_planning",
                        "plan": preview_payload.get("plan").cloned(),
                        "conversation": preview_payload.get("conversation").cloned(),
                        "planner_diagnostics": preview_payload.get("planner_diagnostics").cloned(),
                    })
                    .to_string(),
                ))
                .expect("create request"),
        )
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    let session_id = create_payload
        .get("session")
        .and_then(|row| row.get("session_id"))
        .and_then(Value::as_str)
        .expect("session id")
        .to_string();

    let patch_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/workflow-plans/sessions/{session_id}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "current_plan_id": "" }).to_string()))
                .expect("patch request"),
        )
        .await
        .expect("patch response");
    assert_eq!(patch_resp.status(), StatusCode::OK);

    let reset_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/workflow-plans/sessions/{session_id}/reset"))
                .body(Body::empty())
                .expect("reset request"),
        )
        .await
        .expect("reset response");
    assert_eq!(reset_resp.status(), StatusCode::OK);
    let reset_body = to_bytes(reset_resp.into_body(), usize::MAX)
        .await
        .expect("reset body");
    let reset_payload: Value = serde_json::from_slice(&reset_body).expect("reset json");
    assert!(reset_payload
        .get("session")
        .and_then(|row| row.get("current_plan_id"))
        .and_then(Value::as_str)
        .is_some());
    assert!(reset_payload.get("plan").is_some());
}

#[tokio::test]
async fn workflow_plan_planner_model_spec_prefers_planner_role_model() {
    let spec = crate::http::workflow_planner::planner_model_spec(Some(&json!({
        "model_provider": "openai",
        "model_id": "gpt-5.1",
        "role_models": {
            "planner": {
                "provider_id": "anthropic",
                "model_id": "claude-sonnet-4"
            }
        }
    })))
    .expect("planner spec");
    assert_eq!(spec.provider_id, "anthropic");
    assert_eq!(spec.model_id, "claude-sonnet-4");
}
