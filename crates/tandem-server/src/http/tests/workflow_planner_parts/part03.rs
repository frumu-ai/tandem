#[tokio::test]
#[serial_test::serial]
async fn workflow_plan_apply_can_materialize_a_disabled_draft_with_planner_metadata() {
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
                "Comparison Workflow",
                "Collect inputs, compare them, and produce a report.",
                manual_schedule_json(),
                "/tmp/ignored",
                vec![
                    step_json("collect_inputs", "collect", "Gather inputs.", &[], "researcher", json!([]), "structured_json"),
                    step_json("compare_results", "compare", "Compare them.", &["collect_inputs"], "analyst", json!([
                        {"from_step_id":"collect_inputs","alias":"comparison_inputs"}
                    ]), "structured_json"),
                    step_json("generate_report", "report", "Generate the report.", &["compare_results"], "writer", json!([
                        {"from_step_id":"compare_results","alias":"comparison_findings"}
                    ]), "report_markdown")
                ],
                Some(json!({
                    "execution_mode": "swarm",
                    "max_parallel_agents": 6,
                    "model_provider": "openai",
                    "model_id": "gpt-5.1",
                    "role_models": {
                        "planner": {
                            "provider_id": "openai",
                            "model_id": "gpt-5.1"
                        }
                    }
                }))
            )
        })
        .to_string(),
    );

    let preview_resp = app
        .clone()
        .oneshot(preview_request(json!({
            "prompt": "Compare two competitor summaries and generate a report",
            "plan_source": "automations_page",
            "allowed_mcp_servers": ["slack", "github", "github"],
            "workspace_root": "/tmp/custom-workspace",
            "operator_preferences": {
                "execution_mode": "swarm",
                "max_parallel_agents": 6,
                "model_provider": "openai",
                "model_id": "gpt-5.1",
                "role_models": {
                    "planner": {
                        "provider_id": "openai",
                        "model_id": "gpt-5.1"
                    }
                }
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

    let mut apply_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("x-tandem-org-id", "org-a")
        .header("x-tandem-workspace-id", "workspace-a")
        .header("x-tandem-actor-id", "user-a")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel",
                "materialize_as_draft": true,
                "idempotency_key": "workflow-apply-draft-1"
            })
            .to_string(),
        ))
        .expect("apply request");
    apply_req.extensions_mut().insert(verified_workflow_plan_context(
        tandem_types::TenantContext::explicit("org-a", "workspace-a", None),
        "user-a",
    ));
    let apply_resp = app
        .clone()
        .oneshot(apply_req)
        .await
        .expect("apply response");
    let apply_status = apply_resp.status();
    let apply_body = to_bytes(apply_resp.into_body(), usize::MAX)
        .await
        .expect("apply body");
    assert_eq!(apply_status, StatusCode::OK);
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
    assert_eq!(stored.status, crate::AutomationV2Status::Draft);
    assert_eq!(stored.next_fire_at_ms, None);
    let tenant = stored.tenant_context();
    assert_eq!(tenant.org_id, "org-a");
    assert_eq!(tenant.workspace_id, "workspace-a");
    assert_eq!(tenant.actor_id.as_deref(), Some("user-a"));
    assert_eq!(stored.creator_id, "user-a");
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("authoring_actor_id"))
            .and_then(Value::as_str),
        Some("user-a")
    );
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("requested_creator_id"))
            .and_then(Value::as_str),
        Some("control-panel")
    );
    assert_eq!(
        stored.workspace_root.as_deref(),
        Some("/tmp/custom-workspace")
    );
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("plan_source"))
            .and_then(Value::as_str),
        Some("automations_page")
    );
    assert!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("plan_package_bundle"))
            .is_some(),
        "plan package bundle should be stored on the automation snapshot"
    );
    assert!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("plan_package"))
            .is_some(),
        "plan package should be stored on the automation snapshot"
    );
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("plan_package"))
            .and_then(|row| row.get("plan_revision"))
            .and_then(Value::as_u64),
        Some(1)
    );
    assert!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("plan_package_validation"))
            .is_some(),
        "plan package validation should be stored on the automation snapshot"
    );
    assert!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("approved_plan_materialization"))
            .is_some(),
        "approved plan materialization should be stored on the automation snapshot"
    );
    assert!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("planner_diagnostics"))
            .is_some(),
        "planner diagnostics should be present on the automation snapshot"
    );
    assert!(apply_payload.get("plan_package_bundle").is_some());
    assert!(apply_payload.get("approved_plan_materialization").is_some());
    let stored_draft = state.get_workflow_plan_draft(plan_id).await.expect("draft");
    assert!(stored_draft.last_success_materialization.is_some());
    assert_eq!(
        stored_draft
            .last_success_materialization
            .as_ref()
            .and_then(|value| value.get("plan_id"))
            .and_then(Value::as_str),
        Some(plan_id)
    );
    assert_eq!(
        stored
            .metadata
            .as_ref()
            .and_then(|row| row.get("approved_plan_materialization"))
            .and_then(|row| row.get("plan_id"))
            .and_then(Value::as_str),
        Some(plan_id)
    );
    let mut replay_req = Request::builder()
        .method("POST")
        .uri("/workflow-plans/apply")
        .header("x-tandem-org-id", "org-a")
        .header("x-tandem-workspace-id", "workspace-a")
        .header("x-tandem-actor-id", "user-a")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "plan_id": plan_id,
                "creator_id": "control-panel",
                "materialize_as_draft": true,
                "idempotency_key": "workflow-apply-draft-1"
            })
            .to_string(),
        ))
        .expect("replay apply request");
    replay_req
        .extensions_mut()
        .insert(verified_workflow_plan_context(
            tandem_types::TenantContext::explicit("org-a", "workspace-a", None),
            "user-a",
        ));
    let replay_resp = app
        .clone()
        .oneshot(replay_req)
        .await
        .expect("replay apply response");
    assert_eq!(replay_resp.status(), StatusCode::OK);
    let replay_body = to_bytes(replay_resp.into_body(), usize::MAX)
        .await
        .expect("replay apply body");
    let replay_payload: Value = serde_json::from_slice(&replay_body).expect("replay apply json");
    assert_eq!(
        replay_payload
            .pointer("/automation/automation_id")
            .and_then(Value::as_str),
        Some(automation_id)
    );
    assert_eq!(
        state
            .list_automations_v2()
            .await
            .into_iter()
            .filter(|automation| automation.automation_id == automation_id)
            .count(),
        1
    );
    let dry_run_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/v2/{automation_id}/run_now"))
        .header("x-tandem-org-id", "org-a")
        .header("x-tandem-workspace-id", "workspace-a")
        .header("x-tandem-actor-id", "user-a")
        .header("content-type", "application/json")
        .body(Body::from(json!({"dry_run": true}).to_string()))
        .expect("dry run request");
    let dry_run_resp = app
        .clone()
        .oneshot(dry_run_req)
        .await
        .expect("dry run response");
    assert_eq!(dry_run_resp.status(), StatusCode::OK);
    let dry_run_body = to_bytes(dry_run_resp.into_body(), usize::MAX)
        .await
        .expect("dry run body");
    let dry_run_payload: Value = serde_json::from_slice(&dry_run_body).expect("dry run json");
    let dry_run_run_id = dry_run_payload
        .get("run")
        .and_then(|row| row.get("run_id"))
        .and_then(Value::as_str)
        .expect("dry run id");
    assert_eq!(
        dry_run_payload
            .get("run")
            .and_then(|row| row.get("trigger_type"))
            .and_then(Value::as_str),
        Some("manual_dry_run")
    );
    let stored_after_run_now = state
        .get_automation_v2(automation_id)
        .await
        .expect("stored automation after manual run");
    let expected_trigger_id = format!("manual-trigger-{dry_run_run_id}");
    let manual_trigger_record = stored_after_run_now
        .metadata
        .as_ref()
        .and_then(|row| row.get("plan_package"))
        .and_then(|row| row.get("manual_trigger_record"))
        .expect("manual trigger record");
    assert_eq!(
        manual_trigger_record
            .get("trigger_id")
            .and_then(Value::as_str),
        Some(expected_trigger_id.as_str())
    );
    assert_eq!(
        manual_trigger_record
            .get("triggered_by")
            .and_then(Value::as_str),
        Some("user-a")
    );
    assert_eq!(
        manual_trigger_record
            .get("trigger_source")
            .and_then(Value::as_str),
        Some("dry_run")
    );
    assert_eq!(
        manual_trigger_record
            .get("dry_run")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        dry_run_payload
            .get("run")
            .and_then(|row| row.get("automation_snapshot"))
            .and_then(|row| row.get("metadata"))
            .and_then(|row| row.get("plan_package"))
            .and_then(|row| row.get("manual_trigger_record"))
            .and_then(|row| row.get("run_id"))
            .and_then(Value::as_str),
        Some(dry_run_run_id)
    );
    let operator_agent = stored
        .agents
        .iter()
        .find(|agent| agent.agent_id == "agent_writer")
        .expect("writer agent");
    assert!(operator_agent
        .tool_policy
        .allowlist
        .contains(&"mcp.github.*".to_string()));
    assert!(operator_agent
        .tool_policy
        .allowlist
        .contains(&"mcp.slack.*".to_string()));
    assert!(stored
        .flow
        .nodes
        .iter()
        .any(|node| !node.input_refs.is_empty()));
}

#[tokio::test]
async fn workflow_plan_apply_rolls_back_when_protected_audit_persistence_fails() {
    let state = test_state().await;
    tokio::fs::create_dir_all(&state.protected_audit_path)
        .await
        .expect("make protected audit path unwritable as a file");
    crate::audit::reset_protected_audit_tail_for_test(&state.protected_audit_path).await;
    let app = app_router(state.clone());
    let plan = llm_plan_json(
        "Audited workflow",
        "Create a report only when the required audit can be persisted.",
        manual_schedule_json(),
        "/tmp/workspace",
        vec![step_json(
            "generate_report",
            "report",
            "Generate the report.",
            &[],
            "writer",
            json!([]),
            "report_markdown",
        )],
        None,
    );

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/apply")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "plan": plan,
                        "creator_id": "control-panel",
                        "idempotency_key": "audit-rollback-test",
                    })
                    .to_string(),
                ))
                .expect("apply request"),
        )
        .await
        .expect("apply response");

    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("error body");
    let payload: Value = serde_json::from_slice(&body).expect("error json");
    assert_eq!(
        payload.get("code").and_then(Value::as_str),
        Some("PROTECTED_AUDIT_PERSISTENCE_FAILED")
    );
    assert_eq!(
        payload.get("operationApplied").and_then(Value::as_bool),
        Some(false)
    );
    assert!(
        state.list_automations_v2().await.is_empty(),
        "an unaudited workflow materialization must be rolled back"
    );
    assert!(
        state
            .get_idempotency_key(
                &tandem_types::TenantContext::local_implicit(),
                "workflow_plan.apply",
                "audit-rollback-test",
            )
            .await
            .is_none(),
        "a rolled-back apply must release its idempotency reservation"
    );
}

#[tokio::test]
async fn workflow_plan_apply_preserves_recovered_automation_when_audit_retry_fails() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let request_body = json!({
        "plan": llm_plan_json(
            "Recovered audited workflow",
            "Preserve an existing materialization while retrying its audit.",
            manual_schedule_json(),
            "/tmp/workspace",
            vec![step_json(
                "generate_report",
                "report",
                "Generate the report.",
                &[],
                "writer",
                json!([]),
                "report_markdown",
            )],
            None,
        ),
        "creator_id": "control-panel",
        "idempotency_key": "audit-recovery-test",
    })
    .to_string();

    let initial_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/apply")
                .header("content-type", "application/json")
                .body(Body::from(request_body.clone()))
                .expect("initial apply request"),
        )
        .await
        .expect("initial apply response");
    assert_eq!(initial_response.status(), StatusCode::OK);
    let initial_body = to_bytes(initial_response.into_body(), usize::MAX)
        .await
        .expect("initial apply body");
    let initial_payload: Value = serde_json::from_slice(&initial_body).expect("initial apply json");
    let automation_id = initial_payload
        .pointer("/automation/automation_id")
        .and_then(Value::as_str)
        .expect("automation id")
        .to_string();
    assert!(state.get_automation_governance(&automation_id).await.is_some());

    state.idempotency_keys.write().await.clear();
    tokio::fs::remove_file(&state.protected_audit_path)
        .await
        .expect("remove working protected audit file");
    tokio::fs::create_dir_all(&state.protected_audit_path)
        .await
        .expect("make protected audit path unwritable as a file");
    crate::audit::reset_protected_audit_tail_for_test(&state.protected_audit_path).await;

    let retry_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflow-plans/apply")
                .header("content-type", "application/json")
                .body(Body::from(request_body))
                .expect("retry apply request"),
        )
        .await
        .expect("retry apply response");
    assert_eq!(retry_response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let retry_body = to_bytes(retry_response.into_body(), usize::MAX)
        .await
        .expect("retry error body");
    let retry_payload: Value = serde_json::from_slice(&retry_body).expect("retry error json");
    assert_eq!(
        retry_payload.get("code").and_then(Value::as_str),
        Some("PROTECTED_AUDIT_PERSISTENCE_FAILED")
    );
    assert_eq!(
        retry_payload
            .get("operationApplied")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        retry_payload.get("retryable").and_then(Value::as_bool),
        Some(true)
    );
    assert!(
        state.get_automation_v2(&automation_id).await.is_some(),
        "an existing recovered automation must not be rolled back by a retry"
    );
    assert!(
        state.get_automation_governance(&automation_id).await.is_some(),
        "an existing recovered automation must retain its governance record"
    );
    assert!(
        state
            .get_idempotency_key(
                &tandem_types::TenantContext::local_implicit(),
                "workflow_plan.apply",
                "audit-recovery-test",
            )
            .await
            .is_none(),
        "the failed audit retry must release its idempotency reservation"
    );
}
