// Continuation of routines tests split from routines.rs for the file-size gate
// (same module via include!).


#[tokio::test]
async fn routines_run_now_requires_approval_for_external_side_effects_when_enabled() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-ext-approval",
                "name": "External draft workflow",
                "schedule": { "interval_seconds": { "seconds": 300 } },
                "entrypoint": "connector.email.reply",
                "requires_approval": true,
                "external_integrations_allowed": true
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/routines/routine-ext-approval/run_now")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("run_now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("run_now response");
    assert_eq!(run_now_resp.status(), StatusCode::OK);
    let run_now_body = to_bytes(run_now_resp.into_body(), usize::MAX)
        .await
        .expect("run_now body");
    let run_now_payload: Value = serde_json::from_slice(&run_now_body).expect("run_now json");
    assert_eq!(
        run_now_payload.get("status").and_then(|v| v.as_str()),
        Some("pending_approval")
    );

    let history_req = Request::builder()
        .method("GET")
        .uri("/routines/routine-ext-approval/history?limit=5")
        .body(Body::empty())
        .expect("history request");
    let history_resp = app
        .clone()
        .oneshot(history_req)
        .await
        .expect("history response");
    assert_eq!(history_resp.status(), StatusCode::OK);
    let history_body = to_bytes(history_resp.into_body(), usize::MAX)
        .await
        .expect("history body");
    let history_payload: Value = serde_json::from_slice(&history_body).expect("history json");
    assert_eq!(
        history_payload
            .get("events")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str()),
        Some("pending_approval")
    );
}

#[tokio::test]
async fn routine_fired_event_contract_snapshot() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-fired-contract",
                "name": "Routine fired contract",
                "schedule": { "interval_seconds": { "seconds": 300 } },
                "entrypoint": "mission.default"
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/routines/routine-fired-contract/run_now")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "run_count": 2 }).to_string()))
        .expect("run now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("run now response");
    assert_eq!(run_now_resp.status(), StatusCode::OK);

    let event = next_event_of_type(&mut rx, "routine.fired").await;
    let mut properties = event
        .properties
        .as_object()
        .cloned()
        .expect("properties object");
    let fired_at_ms = properties
        .remove("firedAtMs")
        .and_then(|v| v.as_u64())
        .expect("firedAtMs");
    assert!(fired_at_ms > 0);

    let snapshot = json!({
        "type": event.event_type,
        "properties": properties,
    });
    let run_id = snapshot
        .pointer("/properties/runID")
        .and_then(Value::as_str)
        .expect("runID");
    assert!(run_id.starts_with("routine-run-"));
    let expected = json!({
        "type": "routine.fired",
        "properties": {
            "routineID": "routine-fired-contract",
            "runCount": 2,
            "runID": run_id,
            "triggerType": "manual"
        }
    });
    assert_eq!(snapshot, expected);
}

#[tokio::test]
async fn routine_approval_required_event_contract_snapshot() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-approval-contract",
                "name": "Routine approval contract",
                "schedule": { "interval_seconds": { "seconds": 300 } },
                "entrypoint": "connector.email.reply",
                "requires_approval": true,
                "external_integrations_allowed": true
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/routines/routine-approval-contract/run_now")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("run now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("run now response");
    assert_eq!(run_now_resp.status(), StatusCode::OK);

    let event = next_event_of_type(&mut rx, "routine.approval_required").await;
    let snapshot = json!({
        "type": event.event_type,
        "properties": event.properties,
    });
    let run_id = snapshot
        .pointer("/properties/runID")
        .and_then(Value::as_str)
        .expect("runID");
    assert!(run_id.starts_with("routine-run-"));
    let expected = json!({
        "type": "routine.approval_required",
        "properties": {
            "routineID": "routine-approval-contract",
            "runCount": 1,
            "runID": run_id,
            "triggerType": "manual",
            "reason": "manual approval required before external side effects (manual)"
        }
    });
    assert_eq!(snapshot, expected);
}

#[tokio::test]
async fn routine_blocked_event_contract_snapshot() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-blocked-contract",
                "name": "Routine blocked contract",
                "schedule": { "interval_seconds": { "seconds": 300 } },
                "entrypoint": "connector.email.reply",
                "requires_approval": true,
                "external_integrations_allowed": false
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/routines/routine-blocked-contract/run_now")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("run now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("run now response");
    assert_eq!(run_now_resp.status(), StatusCode::FORBIDDEN);

    let event = next_event_of_type(&mut rx, "routine.blocked").await;
    let snapshot = json!({
        "type": event.event_type,
        "properties": event.properties,
    });
    let run_id = snapshot
        .pointer("/properties/runID")
        .and_then(Value::as_str)
        .expect("runID");
    assert!(run_id.starts_with("routine-run-"));
    let expected = json!({
        "type": "routine.blocked",
        "properties": {
            "routineID": "routine-blocked-contract",
            "runCount": 1,
            "runID": run_id,
            "triggerType": "manual",
            "reason": "external integrations are disabled by policy"
        }
    });
    assert_eq!(snapshot, expected);
}

#[tokio::test]
async fn routine_tool_policy_hook_denies_disallowed_tool_for_session_scope() {
    let state = test_state().await;
    let session = Session::new(Some("routine-session".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state
        .storage
        .save_session(session)
        .await
        .expect("save session");

    state
        .set_routine_session_policy(
            session_id.clone(),
            "run-routine-hook-1".to_string(),
            "routine-hook-1".to_string(),
            vec!["read".to_string(), "mcp.arcade.search".to_string()],
        )
        .await;

    let hook = crate::agent_teams::ServerToolPolicyHook::new(state.clone());
    let decision = hook
        .evaluate_tool(ToolPolicyContext {
            session_id,
            message_id: "msg-1".to_string(),
            tenant_context: None,
            verified_tenant_context: None,
            tool: "bash".to_string(),
            args: json!({"command":"echo hi"}),
        })
        .await
        .expect("policy decision");

    assert!(!decision.allowed);
    assert!(decision
        .reason
        .as_deref()
        .unwrap_or_default()
        .contains("not allowed for routine"));
}

#[tokio::test]
async fn automation_tool_policy_hook_denies_writes_to_read_only_source_truth_files() {
    let state = test_state().await;
    let session = Session::new(
        Some("automation-session".to_string()),
        Some(".".to_string()),
    );
    let session_id = session.id.clone();
    state
        .storage
        .save_session(session)
        .await
        .expect("save session");

    let automation = crate::AutomationV2Spec {
        automation_id: "automation-read-only-guard".to_string(),
        name: "Read Only Guard".to_string(),
        description: Some(
            "Analyze RESUME.md and use it as the source of truth. Never edit, rewrite, rename, move, or delete RESUME.md."
                .to_string(),
        ),
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
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/home/evan/job-hunt".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("automation run");
    state
        .add_automation_v2_session(&run.run_id, &session_id)
        .await
        .expect("linked automation session");
    state
        .engine_loop
        .set_session_allowed_tools(&session_id, vec!["write".to_string()])
        .await;

    let hook = crate::agent_teams::ServerToolPolicyHook::new(state.clone());
    let decision = hook
        .evaluate_tool(ToolPolicyContext {
            session_id,
            message_id: "msg-automation-1".to_string(),
            tenant_context: None,
            verified_tenant_context: None,
            tool: "write".to_string(),
            args: json!({
                "path": "RESUME.md",
                "content": "bad overwrite",
                "__workspace_root": "/home/evan/job-hunt",
            }),
        })
        .await
        .expect("policy decision");

    assert!(!decision.allowed);
    assert!(decision
        .reason
        .as_deref()
        .unwrap_or_default()
        .contains("read-only source-of-truth"));
}

/// GOV-B2b: routine create and run_now from an agent context are rejected.
/// Routines have no per-routine governance/approval record, so agent-authored
/// routine work is refused — agents must use Automations V2.
#[tokio::test]
async fn routines_reject_agent_context_create_and_run() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .header("x-tandem-request-source", "agent")
        .header("x-tandem-agent-id", "agent-routine")
        .body(Body::from(
            json!({
                "routine_id": "routine-agent",
                "name": "Agent routine",
                "schedule": { "interval_seconds": { "seconds": 60 } },
                "entrypoint": "mission.default"
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app.clone().oneshot(create_req).await.expect("create response");
    assert_eq!(create_resp.status(), StatusCode::FORBIDDEN);
    let create_body: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX).await.expect("body"),
    )
    .expect("json");
    assert_eq!(
        create_body.get("code").and_then(Value::as_str),
        Some("ROUTINE_REQUIRES_HUMAN")
    );

    // The routine must not have been created.
    let list_req = Request::builder()
        .method("GET")
        .uri("/routines")
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.clone().oneshot(list_req).await.expect("list response");
    let list_body: Value =
        serde_json::from_slice(&to_bytes(list_resp.into_body(), usize::MAX).await.expect("body"))
            .expect("json");
    assert_eq!(list_body.get("count").and_then(Value::as_u64), Some(0));

    // A human creates a routine; an agent-context run_now is then refused.
    let human_create = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-human",
                "name": "Human routine",
                "schedule": { "interval_seconds": { "seconds": 60 } },
                "entrypoint": "mission.default"
            })
            .to_string(),
        ))
        .expect("human create request");
    assert_eq!(
        app.clone().oneshot(human_create).await.expect("resp").status(),
        StatusCode::OK
    );

    let run_req = Request::builder()
        .method("POST")
        .uri("/routines/routine-human/run_now")
        .header("content-type", "application/json")
        .header("x-tandem-request-source", "agent")
        .header("x-tandem-agent-id", "agent-routine")
        .body(Body::from(json!({ "run_count": 1 }).to_string()))
        .expect("run request");
    let run_resp = app.clone().oneshot(run_req).await.expect("run response");
    assert_eq!(run_resp.status(), StatusCode::FORBIDDEN);
}
