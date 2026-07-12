#[tokio::test]
async fn goal_event_stream_replays_from_last_event_id() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    let app = app_router(state.clone());
    publish_orchestration(&app, &state).await;

    let (status, started) = dispatch(
        &app,
        local_request(
            "POST",
            "/goals",
            Some(json!({
                "orchestration_id": "orch-goals",
                "objective": "Stream me",
                "idempotency_key": "start-stream",
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let goal_id = started["goal"]["goal_id"].as_str().unwrap().to_string();
    let (_, paused) = dispatch(
        &app,
        local_request("POST", format!("/goals/{goal_id}/pause"), Some(json!({}))),
    )
    .await;
    assert_eq!(paused["outcome"], json!("paused"));

    // Find the durable cursor of the first event, then reconnect "after" it
    // via the Last-Event-ID header: the stream must replay only the pause.
    let (_, all_events) = dispatch(
        &app,
        local_request("GET", format!("/goals/{goal_id}/events"), None),
    )
    .await;
    let first_cursor = all_events["events"][0]["cursor"].as_i64().unwrap();

    let request = Request::builder()
        .method("GET")
        .uri(format!("/goals/{goal_id}/events/stream"))
        .header("x-tandem-org-id", "local")
        .header("x-tandem-workspace-id", "local")
        .header("x-tandem-actor-id", "operator")
        .header("last-event-id", first_cursor.to_string())
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.expect("sse response");
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    let mut collected = String::new();
    // Read frames until the replayed pause event arrives (bounded wait).
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let chunk = tokio::time::timeout_at(deadline, futures::StreamExt::next(&mut body))
            .await
            .expect("SSE frame before deadline");
        let Some(Ok(bytes)) = chunk else {
            panic!("SSE stream ended before replaying events: {collected}");
        };
        collected.push_str(&String::from_utf8_lossy(&bytes));
        if collected.contains("stateful_runtime.goal.paused") {
            break;
        }
    }
    // The started event was before the Last-Event-ID cursor: no duplicate.
    assert!(
        !collected.contains("stateful_runtime.goal.started"),
        "reconnect must not replay events at or before Last-Event-ID: {collected}"
    );
    assert!(collected.contains("event: ready"), "{collected}");
    // Durable ids ride along for the next reconnect.
    assert!(
        collected.contains(&format!("id: {}", first_cursor + 1)),
        "{collected}"
    );
}

#[tokio::test]
async fn canonical_goal_projection_is_bounded_isolated_and_replayable() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    let app = app_router(state.clone());
    publish_orchestration(&app, &state).await;

    let (status, started) = dispatch(
        &app,
        local_request(
            "POST",
            "/goals",
            Some(json!({
                "orchestration_id": "orch-goals",
                "objective": "Project this goal",
                "idempotency_key": "projection-contract",
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let goal_id = started["goal"]["goal_id"].as_str().unwrap().to_string();
    let updated_at_ms = started["goal"]["updated_at_ms"].as_u64().unwrap();

    // A corrupt/mis-attributed row sharing the local goal ID must remain
    // invisible to the canonical projection's live handoff read.
    let foreign = foreign_handoff(&goal_id);
    let database_path = directory.path().join("stateful_runtime.sqlite3");
    let connection = rusqlite::Connection::open(&database_path).unwrap();
    connection
        .execute(
            "INSERT INTO workflow_handoffs
                (handoff_id, goal_id, idempotency_key, org_id, workspace_id,
                 deployment_id, source_run_id, target_automation_id, status,
                 consumed_by_run_id, handoff_json, created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, ?8, NULL, ?9, ?10, ?11)",
            rusqlite::params![
                foreign.handoff_id,
                foreign.goal_id,
                foreign.idempotency_key,
                foreign.tenant_context.org_id,
                foreign.tenant_context.workspace_id,
                foreign.source_run_id,
                foreign.target_automation_id,
                "pending_approval",
                serde_json::to_string(&foreign).unwrap(),
                foreign.created_at_ms,
                foreign.updated_at_ms,
            ],
        )
        .unwrap();
    drop(connection);

    let (status, projection) = dispatch(
        &app,
        local_request(
            "GET",
            format!("/goals/{goal_id}/projection?limit=99999"),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{projection}");
    assert_eq!(projection["mode"], json!("live"));
    assert_eq!(projection["timeline"]["limit"], json!(250));
    assert_eq!(
        projection["orchestration_source"],
        json!("goal_metadata_snapshot")
    );
    assert_eq!(projection["graph"]["available"], json!(true));
    assert_eq!(projection["workflow"]["automation_id"], json!("planner"));
    assert_eq!(projection["handoffs"], json!([]));
    assert!(projection["actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["id"] == json!("pause") && action["enabled"] == json!(true)));
    let start_cursor = projection["cursor"].as_i64().unwrap();

    let (status, _) = dispatch(
        &app,
        orchestration_request(
            "GET",
            format!("/goals/{goal_id}/projection"),
            "other",
            "tenant",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let action = json!({
        "expected_updated_at_ms": updated_at_ms,
        "idempotency_key": "pause-projection",
        "reason": "operator review",
    });
    let (status, denied) = dispatch(
        &app,
        unauthenticated_local_request(
            "POST",
            format!("/goals/{goal_id}/actions/pause"),
            Some(action.clone()),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{denied}");

    let mut stale = action.clone();
    stale["expected_updated_at_ms"] = json!(updated_at_ms.saturating_sub(1));
    let (status, conflict) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/actions/pause"),
            Some(stale),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    assert_eq!(conflict["error"], json!("stale_goal_action"));

    let (status, paused) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/actions/pause"),
            Some(action.clone()),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{paused}");
    assert_eq!(paused["goal"]["status"], json!("paused"));
    assert!(paused["projection_cursor"].as_i64().unwrap() > start_cursor);

    let (status, duplicate_pause) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/actions/pause"),
            Some(action),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{duplicate_pause}");
    assert_eq!(
        duplicate_pause["action"]["result"]["outcome"],
        json!("paused")
    );

    let (status, replay) = dispatch(
        &app,
        local_request(
            "GET",
            format!("/goals/{goal_id}/projection?cursor={start_cursor}"),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["mode"], json!("replay"));
    assert_eq!(replay["goal"]["status"], json!("active"));
    assert_eq!(replay["historical_state"]["exact"], json!(true));

    // Simulate a legacy event without a full projection snapshot. The server
    // must fail closed instead of presenting today's mutable state as history.
    let connection = rusqlite::Connection::open(database_path).unwrap();
    let (event_id, event_json): (String, String) = connection
        .query_row(
            "SELECT event_id, event_json FROM stateful_events WHERE goal_id = ?1 ORDER BY rowid LIMIT 1",
            [&goal_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let tenant = TenantContext::local_implicit();
    let mut event: crate::stateful_runtime::StatefulRunEventRecord =
        crate::stateful_runtime::orchestration_store::protected_records::decode(
            &tenant,
            "event",
            &event_id,
            &event_json,
        )
        .unwrap();
    event
        .payload
        .as_object_mut()
        .unwrap()
        .remove("projection_snapshot_ref");
    let event_json = crate::stateful_runtime::orchestration_store::protected_records::encode(
        &tenant, "event", &event_id, &event,
    )
    .unwrap();
    connection
        .execute(
            "UPDATE stateful_events SET event_json = ?1 WHERE goal_id = ?2 AND rowid = ?3",
            rusqlite::params![event_json, goal_id, start_cursor],
        )
        .unwrap();
    drop(connection);
    let (status, fallback) = dispatch(
        &app,
        local_request(
            "GET",
            format!("/goals/{goal_id}/projection?cursor={start_cursor}"),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{fallback}");
    assert_eq!(
        fallback["error"],
        json!("historical_projection_snapshot_unavailable")
    );
}

#[tokio::test]
async fn canonical_handoff_decision_is_authoritatively_idempotent() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    let app = app_router(state.clone());
    let (planner_hash, executor_hash) = seed_workflows(&state).await;
    let mut draft = draft_payload(&planner_hash, &executor_hash);
    draft["edges"][0]["approval"] = json!({"required": true});
    let (status, _) = dispatch(&app, local_request("POST", "/orchestrations", Some(draft))).await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, published) = dispatch(
        &app,
        local_request("POST", "/orchestrations/orch-goals/publish", None),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{published}");
    let (status, started) = dispatch(
        &app,
        local_request(
            "POST",
            "/goals",
            Some(json!({
                "orchestration_id": "orch-goals",
                "objective": "Approve once",
                "idempotency_key": "decision-idempotency",
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let goal_id = started["goal"]["goal_id"].as_str().unwrap().to_string();
    complete_run(&state, started["root_run_id"].as_str().unwrap()).await;
    let (status, pending) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/transitions"),
            Some(json!({
                "transition_key": "continue",
                "idempotency_key": "approval-hop",
                "artifact": {"artifact_type": "plan", "value": {"ready": true}},
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED, "{pending}");
    let updated_at_ms = pending["goal"]["updated_at_ms"].as_u64().unwrap();
    let handoff_id = pending["handoff"]["handoff_id"].as_str().unwrap();
    let action_id = format!("handoff:{handoff_id}:decision");
    let decision = json!({
        "expected_updated_at_ms": updated_at_ms,
        "idempotency_key": "approve-once",
        "decision": "approve",
        "reason": "reviewed",
    });
    let (status, first) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/actions/{action_id}"),
            Some(decision.clone()),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(
        first["action"]["result"]["handoff"]["status"],
        json!("approved")
    );

    let (status, replayed) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/actions/{action_id}"),
            Some(decision),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replayed}");
    assert_eq!(replayed["action"]["result"]["outcome"], json!("decided"));
    assert_eq!(
        replayed["action"]["result"]["handoff"]["handoff_id"],
        json!(handoff_id)
    );
}

#[tokio::test]
async fn hosted_goal_start_stamps_verified_owner_and_owner_can_pause() {
    let state = test_state().await;
    let mut events = state.event_bus.subscribe();
    let app = app_router(state.clone());
    publish_orchestration(&app, &state).await;

    let tenant = TenantContext::local_implicit();
    let principal = tandem_types::RequestPrincipal::authenticated_user("transport-user", "test");
    let verified = verified_context("goal-owner");
    let payload = serde_json::from_value(json!({
        "orchestration_id": "orch-goals",
        "objective": "Own the hosted goal",
        "idempotency_key": "hosted-owner-start",
        "metadata": {"started_by": "forged-owner", "source": "test"}
    }))
    .unwrap();
    let response = crate::http::goals_api::start_goal(
        State(state.clone()),
        Extension(tenant.clone()),
        Extension(principal.clone()),
        Some(Extension(verified.clone())),
        Json(payload),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let started = json_body(response).await;
    assert_eq!(
        started["goal"]["metadata"]["started_by"]["id"],
        "goal-owner"
    );
    assert_eq!(started["goal"]["metadata"]["source"], "test");

    let goal_id = started["goal"]["goal_id"].as_str().unwrap().to_string();
    let response = crate::http::goals_api::pause_goal(
        State(state.clone()),
        Extension(tenant),
        Extension(principal),
        Some(Extension(verified)),
        Path(goal_id.clone()),
        Json(Default::default()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let paused = json_body(response).await;
    assert_eq!(paused["outcome"], "paused");
    let receipt =
        crate::test_support::next_event_of_type(&mut events, "orchestration.goal.action_receipt")
            .await;
    assert_eq!(receipt.properties["goalID"], json!(goal_id));
    assert_eq!(receipt.properties["action"], json!("pause"));
    assert_eq!(
        receipt.properties["effective_actor"]["id"],
        json!("goal-owner")
    );
    assert_eq!(
        receipt.properties["run_as"]["assertion_id"],
        json!("assertion-goal-owner")
    );

    let response = crate::http::goals_api::settle_goal_completion(
        State(state),
        Extension(TenantContext::local_implicit()),
        Extension(tandem_types::RequestPrincipal::authenticated_user(
            "intruder", "test",
        )),
        Some(Extension(verified_context("intruder"))),
        Path(started["goal"]["goal_id"].as_str().unwrap().to_string()),
        Json(Default::default()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn denied_wait_resolution_emits_no_success_receipt() {
    let state = test_state().await;
    let mut events = state.event_bus.subscribe();
    let response = crate::http::goals_api::resolve_goal_wait(
        State(state),
        Extension(TenantContext::local_implicit()),
        Extension(tandem_types::RequestPrincipal::authenticated_user(
            "transport-user",
            "test",
        )),
        Some(Extension(verified_context("unprivileged-user"))),
        Path(("missing-goal".to_string(), "missing-wait".to_string())),
        Json(
            serde_json::from_value(json!({
                "idempotency_key": "denied-resolution"
            }))
            .unwrap(),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), events.recv())
            .await
            .is_err(),
        "denied wait resolution must not emit a success receipt"
    );
}

#[tokio::test]
async fn wait_resolution_receipt_records_verified_actor_and_run_as() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    let app = app_router(state.clone());
    publish_orchestration(&app, &state).await;
    let (status, started) = dispatch(
        &app,
        local_request(
            "POST",
            "/goals",
            Some(json!({
                "orchestration_id": "orch-goals",
                "objective": "Resolve an external wait",
                "idempotency_key": "wait-receipt-goal",
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let goal_id = started["goal"]["goal_id"].as_str().unwrap().to_string();
    let run_id = started["root_run_id"].as_str().unwrap().to_string();
    let wait_id = "receipt-external-wait";
    let paths = crate::stateful_runtime::StatefulRuntimeStoragePaths::from_runtime_events_path(
        &state.runtime_events_path,
    );
    crate::stateful_runtime::upsert_stateful_wait(
        &paths.waits_path,
        crate::stateful_runtime::StatefulWaitRecord {
            schema_version: crate::stateful_runtime::STATEFUL_RUNTIME_SCHEMA_VERSION,
            wait_id: wait_id.to_string(),
            run_id,
            wait_kind: crate::stateful_runtime::StatefulWaitKind::ExternalCondition,
            status: crate::stateful_runtime::StatefulWaitStatus::Waiting,
            scope: crate::stateful_runtime::StatefulRuntimeScope::local_implicit(),
            phase_id: None,
            reason: Some("receipt test".to_string()),
            created_at_ms: crate::now_ms(),
            updated_at_ms: crate::now_ms(),
            wake_at_ms: None,
            timeout_policy: None,
            event_seq: None,
            wake_idempotency_key: None,
            claimed_by: None,
            claimed_at_ms: None,
            claim_expires_at_ms: None,
            completed_at_ms: None,
            metadata: None,
        },
    )
    .await
    .unwrap();

    let mut verified = verified_context("wait-operator");
    verified
        .capabilities
        .push("orchestration.resolve_wait".to_string());
    let mut events = state.event_bus.subscribe();
    let response = crate::http::goals_api::resolve_goal_wait(
        State(state),
        Extension(TenantContext::local_implicit()),
        Extension(tandem_types::RequestPrincipal::authenticated_user(
            "transport-user",
            "test",
        )),
        Some(Extension(verified)),
        Path((goal_id.clone(), wait_id.to_string())),
        Json(
            serde_json::from_value(json!({
                "idempotency_key": "receipt-resolution",
                "payload": {"ready": true}
            }))
            .unwrap(),
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let receipt = crate::test_support::next_event_of_type(
        &mut events,
        "orchestration.goal.wait_resolution_receipt",
    )
    .await;
    assert_eq!(receipt.properties["goalID"], json!(goal_id));
    assert_eq!(receipt.properties["waitID"], json!(wait_id));
    assert_eq!(
        receipt.properties["effective_actor"]["id"],
        json!("wait-operator")
    );
    assert_eq!(
        receipt.properties["run_as"]["assertion_id"],
        json!("assertion-wait-operator")
    );
}

#[tokio::test]
async fn hosted_publish_receipt_records_verified_run_as_context() {
    let state = test_state().await;
    let mut events = state.event_bus.subscribe();
    let app = app_router(state.clone());
    let (planner_hash, executor_hash) = seed_workflows(&state).await;
    let (status, created) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations",
            Some(draft_payload(&planner_hash, &executor_hash)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let response = crate::http::orchestrations_api::publish_orchestration(
        State(state),
        Extension(TenantContext::local_implicit()),
        Extension(tandem_types::RequestPrincipal::authenticated_user(
            "transport-user",
            "test",
        )),
        Some(Extension(verified_context("operator"))),
        Path("orch-goals".to_string()),
        axum::body::Bytes::new(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let published = json_body(response).await;
    assert_eq!(
        published["orchestration"]["metadata"]["publish"]["run_as"]["issuer"],
        json!("tandem-web")
    );
    assert_eq!(
        published["orchestration"]["metadata"]["publish"]["run_as"]["assertion_id"],
        json!("assertion-operator")
    );

    let receipt =
        crate::test_support::next_event_of_type(&mut events, "orchestration.publish_receipt").await;
    assert_eq!(receipt.properties["effective_actor"]["id"], "operator");
    assert_eq!(receipt.properties["run_as"]["issuer"], "tandem-web");
    assert_eq!(
        receipt.properties["run_as"]["assertion_id"],
        "assertion-operator"
    );
}

#[tokio::test]
async fn hosted_draft_update_accepts_principal_ref_creator_metadata() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let (planner_hash, executor_hash) = seed_workflows(&state).await;
    let (status, created) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations",
            Some(draft_payload(&planner_hash, &executor_hash)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let tenant = TenantContext::local_implicit();
    let store = crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(
        &state.automation_v2_runs_path,
    )
    .unwrap();
    let mut draft = store
        .get_orchestration_draft(&tenant, "orch-goals")
        .unwrap()
        .unwrap();
    let expected_updated_at_ms = draft.updated_at_ms;
    draft.metadata.as_mut().unwrap()["created_by"] =
        json!(tandem_types::PrincipalRef::human_user("operator"));
    store
        .put_orchestration_draft(&draft, Some(expected_updated_at_ms))
        .unwrap();

    let mut update = draft_payload(&planner_hash, &executor_hash);
    update["name"] = json!("Updated through hosted HTTP");
    update["expected_updated_at_ms"] = json!(expected_updated_at_ms);
    let payload = serde_json::from_value(update).unwrap();
    let response = crate::http::orchestrations_api::update_orchestration_draft(
        State(state.clone()),
        Extension(tenant.clone()),
        Extension(tandem_types::RequestPrincipal::authenticated_user(
            "transport-user",
            "test",
        )),
        Some(Extension(verified_context("operator"))),
        Path("orch-goals".to_string()),
        Json(payload),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let updated = json_body(response).await;
    assert_eq!(
        updated["orchestration"]["name"],
        "Updated through hosted HTTP"
    );
    assert_eq!(
        updated["orchestration"]["metadata"]["created_by"],
        "operator"
    );

    let intruder = tandem_types::RequestPrincipal::authenticated_user("intruder", "test");
    let verified_intruder = Some(Extension(verified_context("intruder")));
    let refresh = serde_json::from_value(json!({
        "expected_updated_at_ms": updated["orchestration"]["updated_at_ms"]
    }))
    .unwrap();
    let response = crate::http::orchestrations_api::refresh_orchestration_references(
        State(state.clone()),
        Extension(tenant.clone()),
        Extension(intruder.clone()),
        verified_intruder.clone(),
        Path("orch-goals".to_string()),
        Json(refresh),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = crate::http::orchestrations_api::publish_orchestration(
        State(state.clone()),
        Extension(tenant.clone()),
        Extension(intruder.clone()),
        verified_intruder.clone(),
        Path("orch-goals".to_string()),
        axum::body::Bytes::new(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = crate::http::orchestrations_api::archive_orchestration_draft(
        State(state),
        Extension(tenant),
        Extension(intruder),
        verified_intruder,
        Path("orch-goals".to_string()),
        axum::body::Bytes::new(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}
