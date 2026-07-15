// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[tokio::test]
async fn legacy_routine_and_automation_routes_isolate_tenants() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/routines",
            "org-b",
            "workspace-b",
            "user-b",
            Some(json!({
                "routine_id": "tenant-b-routine",
                "name": "Tenant B routine",
                "schedule": { "interval_seconds": { "seconds": 60 } },
                "entrypoint": "mission.default"
            })),
        ))
        .await
        .expect("tenant B create response");
    assert_eq!(create.status(), StatusCode::OK);

    for (org, workspace, actor, name) in [
        ("org-a", "workspace-a", "user-a", "Tenant A shared routine"),
        ("org-b", "workspace-b", "user-b", "Tenant B shared routine"),
    ] {
        let same_id = app
            .clone()
            .oneshot(tenant_request(
                "POST",
                "/routines",
                org,
                workspace,
                actor,
                Some(json!({
                    "routine_id": "shared-routine-id",
                    "name": name,
                    "schedule": { "interval_seconds": { "seconds": 60 } },
                    "entrypoint": "mission.default"
                })),
            ))
            .await
            .expect("same-id create response");
        assert_eq!(same_id.status(), StatusCode::OK);
    }

    let tenant_a_shared = state
        .get_routine_for_tenant(
            "shared-routine-id",
            &TenantContext::explicit("org-a", "workspace-a", Some("user-a".to_string())),
        )
        .await
        .expect("tenant A shared routine");
    let tenant_b_shared = state
        .get_routine_for_tenant(
            "shared-routine-id",
            &TenantContext::explicit("org-b", "workspace-b", Some("user-b".to_string())),
        )
        .await
        .expect("tenant B shared routine");
    assert_eq!(tenant_a_shared.name, "Tenant A shared routine");
    assert_eq!(tenant_b_shared.name, "Tenant B shared routine");

    let tenant_a_patch = app
        .clone()
        .oneshot(tenant_request(
            "PATCH",
            "/routines/shared-routine-id",
            "org-a",
            "workspace-a",
            "user-a",
            Some(json!({"name": "Tenant A renamed"})),
        ))
        .await
        .expect("tenant A same-id patch response");
    assert_eq!(tenant_a_patch.status(), StatusCode::OK);
    assert_eq!(
        state
            .get_routine_for_tenant(
                "shared-routine-id",
                &TenantContext::explicit("org-b", "workspace-b", None),
            )
            .await
            .expect("tenant B shared routine after A patch")
            .name,
        "Tenant B shared routine"
    );

    for uri in ["/routines", "/automations"] {
        let response = app
            .clone()
            .oneshot(tenant_request(
                "GET",
                uri,
                "org-a",
                "workspace-a",
                "user-a",
                None,
            ))
            .await
            .expect("tenant A list response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("list body");
        assert!(!String::from_utf8_lossy(&body).contains("tenant-b-routine"));
    }

    for (method, uri, body) in [
        (
            "PATCH",
            "/routines/tenant-b-routine",
            Some(json!({"name":"cross-tenant rename"})),
        ),
        ("DELETE", "/routines/tenant-b-routine", None),
        ("GET", "/routines/tenant-b-routine/history", None),
        ("GET", "/routines/tenant-b-routine/runs", None),
        (
            "POST",
            "/routines/tenant-b-routine/run_now",
            Some(json!({})),
        ),
        (
            "PATCH",
            "/automations/tenant-b-routine",
            Some(json!({"name":"cross-tenant rename"})),
        ),
        ("DELETE", "/automations/tenant-b-routine", None),
        ("GET", "/automations/tenant-b-routine/history", None),
        ("GET", "/automations/tenant-b-routine/runs", None),
    ] {
        let response = app
            .clone()
            .oneshot(tenant_request(
                method,
                uri,
                "org-a",
                "workspace-a",
                "user-a",
                body,
            ))
            .await
            .expect("cross-tenant routine response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method} {uri}");
    }

    let run_response = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/routines/tenant-b-routine/run_now",
            "org-b",
            "workspace-b",
            "user-b",
            Some(json!({})),
        ))
        .await
        .expect("tenant B run response");
    assert_eq!(run_response.status(), StatusCode::OK);
    let run_body = to_bytes(run_response.into_body(), usize::MAX)
        .await
        .expect("run body");
    let run_payload: Value = serde_json::from_slice(&run_body).expect("run json");
    let run_id = run_payload
        .get("runID")
        .and_then(Value::as_str)
        .expect("run id");

    for (method, uri, body) in [
        ("GET", format!("/routines/runs/{run_id}"), None),
        ("GET", format!("/routines/runs/{run_id}/artifacts"), None),
        (
            "POST",
            format!("/routines/runs/{run_id}/artifacts"),
            Some(json!({"uri":"file:///secret","kind":"report"})),
        ),
        (
            "POST",
            format!("/routines/runs/{run_id}/approve"),
            Some(json!({"reason":"cross tenant"})),
        ),
        ("GET", format!("/automations/runs/{run_id}"), None),
        ("GET", format!("/automations/runs/{run_id}/artifacts"), None),
    ] {
        let response = app
            .clone()
            .oneshot(tenant_request(
                method,
                uri,
                "org-a",
                "workspace-a",
                "user-a",
                body,
            ))
            .await
            .expect("cross-tenant run response");
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{method}");
    }

    let owner_get = app
        .oneshot(tenant_request(
            "GET",
            format!("/routines/runs/{run_id}"),
            "org-b",
            "workspace-b",
            "user-b",
            None,
        ))
        .await
        .expect("tenant B run get");
    assert_eq!(owner_get.status(), StatusCode::OK);
}

#[tokio::test]
async fn concurrent_tenant_delete_and_recreate_same_public_id_do_not_cross() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let initial = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/routines",
            "org-a",
            "workspace-a",
            "user-a",
            Some(json!({
                "routine_id": "transferred-routine",
                "name": "Tenant A original",
                "schedule": { "interval_seconds": { "seconds": 60 } },
                "entrypoint": "mission.default"
            })),
        ))
        .await
        .expect("initial routine create");
    assert_eq!(initial.status(), StatusCode::OK);

    let delete = app.clone().oneshot(tenant_request(
        "DELETE",
        "/routines/transferred-routine",
        "org-a",
        "workspace-a",
        "user-a",
        None,
    ));
    let recreate = app.clone().oneshot(tenant_request(
        "POST",
        "/routines",
        "org-b",
        "workspace-b",
        "user-b",
        Some(json!({
            "routine_id": "transferred-routine",
            "name": "Tenant B recreation",
            "schedule": { "interval_seconds": { "seconds": 60 } },
            "entrypoint": "mission.default"
        })),
    ));
    let (deleted, recreated) = tokio::join!(delete, recreate);
    assert_eq!(deleted.expect("delete response").status(), StatusCode::OK);
    assert_eq!(
        recreated.expect("recreate response").status(),
        StatusCode::OK
    );

    assert!(state
        .get_routine_for_tenant(
            "transferred-routine",
            &TenantContext::explicit("org-a", "workspace-a", None),
        )
        .await
        .is_none());
    assert_eq!(
        state
            .get_routine_for_tenant(
                "transferred-routine",
                &TenantContext::explicit("org-b", "workspace-b", None),
            )
            .await
            .expect("tenant B recreation survives")
            .name,
        "Tenant B recreation"
    );
}

async fn capture_legacy_tenant_stream(
    app: axum::Router,
    uri: &'static str,
    ready_marker: &'static str,
    stop_marker: &'static str,
    ready_tx: tokio::sync::oneshot::Sender<()>,
) -> String {
    let response = app
        .oneshot(tenant_request(
            "GET",
            uri,
            "org-a",
            "workspace-a",
            "user-a",
            None,
        ))
        .await
        .expect("legacy SSE response");
    assert_eq!(response.status(), StatusCode::OK);
    let mut body = response.into_body().into_data_stream();
    let mut captured = String::new();
    let mut ready_tx = Some(ready_tx);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while tokio::time::Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        let Ok(Some(chunk)) = tokio::time::timeout(remaining, body.next()).await else {
            break;
        };
        let chunk = chunk.expect("legacy SSE chunk");
        captured.push_str(&String::from_utf8_lossy(&chunk));
        if captured.contains(ready_marker) {
            if let Some(ready_tx) = ready_tx.take() {
                let _ = ready_tx.send(());
            }
        }
        if captured.contains(stop_marker) {
            break;
        }
    }
    captured
}

#[tokio::test]
async fn legacy_sse_filters_live_event_tenant_after_id_ownership_changes() {
    let state = test_state().await;
    let app = app_router(state);
    let original = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/routines",
            "org-a",
            "workspace-a",
            "user-a",
            Some(json!({
                "routine_id": "ownership-transfer-id",
                "name": "Tenant A original owner",
                "schedule": { "interval_seconds": { "seconds": 60 } },
                "entrypoint": "mission.default"
            })),
        ))
        .await
        .expect("original owner create");
    assert_eq!(original.status(), StatusCode::OK);

    let (routines_ready_tx, routines_ready_rx) = tokio::sync::oneshot::channel();
    let (automations_ready_tx, automations_ready_rx) = tokio::sync::oneshot::channel();
    let routines_reader = tokio::spawn(capture_legacy_tenant_stream(
        app.clone(),
        "/routines/events",
        "\"stream\":\"routines\"",
        "tenant-a-new-after-connect",
        routines_ready_tx,
    ));
    let automations_reader = tokio::spawn(capture_legacy_tenant_stream(
        app.clone(),
        "/automations/events",
        "\"stream\":\"automations\"",
        "tenant-a-new-after-connect",
        automations_ready_tx,
    ));
    tokio::time::timeout(Duration::from_secs(2), routines_ready_rx)
        .await
        .expect("routines SSE ready timeout")
        .expect("routines SSE reader dropped");
    tokio::time::timeout(Duration::from_secs(2), automations_ready_rx)
        .await
        .expect("automations SSE ready timeout")
        .expect("automations SSE reader dropped");

    let deleted = app
        .clone()
        .oneshot(tenant_request(
            "DELETE",
            "/routines/ownership-transfer-id",
            "org-a",
            "workspace-a",
            "user-a",
            None,
        ))
        .await
        .expect("original owner delete");
    assert_eq!(deleted.status(), StatusCode::OK);

    let tenant_b_create = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/routines",
            "org-b",
            "workspace-b",
            "user-b",
            Some(json!({
                "routine_id": "ownership-transfer-id",
                "name": "Tenant B transferred owner",
                "schedule": { "interval_seconds": { "seconds": 60 } },
                "entrypoint": "mission.default"
            })),
        ))
        .await
        .expect("tenant B transferred create");
    assert_eq!(tenant_b_create.status(), StatusCode::OK);
    let tenant_b_run = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/routines/ownership-transfer-id/run_now",
            "org-b",
            "workspace-b",
            "user-b",
            Some(json!({})),
        ))
        .await
        .expect("tenant B run");
    assert_eq!(tenant_b_run.status(), StatusCode::OK);
    let tenant_b_body = to_bytes(tenant_b_run.into_body(), usize::MAX)
        .await
        .expect("tenant B run body");
    let tenant_b_payload: Value =
        serde_json::from_slice(&tenant_b_body).expect("tenant B run json");
    let tenant_b_run_id = tenant_b_payload
        .get("runID")
        .and_then(Value::as_str)
        .expect("tenant B run id")
        .to_string();

    let tenant_a_new = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/routines",
            "org-a",
            "workspace-a",
            "user-a",
            Some(json!({
                "routine_id": "tenant-a-new-after-connect",
                "name": "Tenant A live routine",
                "schedule": { "interval_seconds": { "seconds": 60 } },
                "entrypoint": "mission.default"
            })),
        ))
        .await
        .expect("tenant A live create");
    assert_eq!(tenant_a_new.status(), StatusCode::OK);
    let tenant_a_run = app
        .oneshot(tenant_request(
            "POST",
            "/routines/tenant-a-new-after-connect/run_now",
            "org-a",
            "workspace-a",
            "user-a",
            Some(json!({})),
        ))
        .await
        .expect("tenant A live run");
    assert_eq!(tenant_a_run.status(), StatusCode::OK);

    let routines_captured = routines_reader.await.expect("routines SSE reader");
    let automations_captured = automations_reader.await.expect("automations SSE reader");
    for captured in [&routines_captured, &automations_captured] {
        assert!(
            captured.contains("tenant-a-new-after-connect"),
            "new tenant A routine was absent from live SSE: {captured}"
        );
        assert!(
            !captured.contains(&tenant_b_run_id),
            "tenant B ownership-transfer event leaked into tenant A SSE: {captured}"
        );
    }
}
