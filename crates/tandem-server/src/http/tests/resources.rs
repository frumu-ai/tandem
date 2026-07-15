// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

fn tenant_request(
    method: &str,
    uri: &str,
    org_id: &str,
    workspace_id: &str,
    actor_id: &str,
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("x-tandem-org-id", org_id)
        .header("x-tandem-workspace-id", workspace_id)
        .header("x-tandem-actor-id", actor_id);
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    builder
        .body(
            body.map(|value| Body::from(value.to_string()))
                .unwrap_or_else(Body::empty),
        )
        .expect("tenant request")
}

#[tokio::test]
async fn resource_put_patch_get_and_list_roundtrip() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let put_req = Request::builder()
        .method("PUT")
        .uri("/resource/project/demo/board")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "value": {"status":"todo","count":1},
                "updated_by": "agent-1"
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/resource/project/demo/board")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "value": {"count":2},
                "if_match_rev": 1,
                "updated_by": "agent-2"
            })
            .to_string(),
        ))
        .expect("patch request");
    let patch_resp = app
        .clone()
        .oneshot(patch_req)
        .await
        .expect("patch response");
    assert_eq!(patch_resp.status(), StatusCode::OK);

    let get_req = Request::builder()
        .method("GET")
        .uri("/resource/project/demo/board")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let payload: Value = serde_json::from_slice(&get_body).expect("json");
    assert_eq!(
        payload
            .get("resource")
            .and_then(|r| r.get("rev"))
            .and_then(|v| v.as_u64()),
        Some(2)
    );
    assert_eq!(
        payload
            .get("resource")
            .and_then(|r| r.get("value"))
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str()),
        Some("todo")
    );
    assert_eq!(
        payload
            .get("resource")
            .and_then(|r| r.get("value"))
            .and_then(|v| v.get("count"))
            .and_then(|v| v.as_i64()),
        Some(2)
    );

    let list_req = Request::builder()
        .method("GET")
        .uri("/resource?prefix=project/demo")
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.clone().oneshot(list_req).await.expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
    assert_eq!(list_payload.get("count").and_then(|v| v.as_u64()), Some(1));
}

#[tokio::test]
async fn tenant_a_cannot_access_tenant_b_resource_records() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let put_b = tenant_request(
        "PUT",
        "/resource/project/shared/board",
        "org-b",
        "workspace-b",
        "user-b",
        Some(json!({
            "value": {"owner":"tenant-b"},
            "updated_by": "user-b"
        })),
    );
    let put_b_resp = app.clone().oneshot(put_b).await.expect("put b response");
    assert_eq!(put_b_resp.status(), StatusCode::OK);

    let list_a = tenant_request(
        "GET",
        "/resource?prefix=project/shared",
        "org-a",
        "workspace-a",
        "user-a",
        None,
    );
    let list_a_resp = app.clone().oneshot(list_a).await.expect("list a response");
    assert_eq!(list_a_resp.status(), StatusCode::OK);
    let list_a_body = to_bytes(list_a_resp.into_body(), usize::MAX)
        .await
        .expect("list a body");
    let list_a_payload: Value = serde_json::from_slice(&list_a_body).expect("list a json");
    assert_eq!(
        list_a_payload.get("count").and_then(|v| v.as_u64()),
        Some(0)
    );

    let get_a = tenant_request(
        "GET",
        "/resource/project/shared/board",
        "org-a",
        "workspace-a",
        "user-a",
        None,
    );
    let get_a_resp = app.clone().oneshot(get_a).await.expect("get a response");
    assert_eq!(get_a_resp.status(), StatusCode::NOT_FOUND);

    let delete_a = tenant_request(
        "DELETE",
        "/resource/project/shared/board",
        "org-a",
        "workspace-a",
        "user-a",
        Some(json!({ "updated_by": "user-a" })),
    );
    let delete_a_resp = app
        .clone()
        .oneshot(delete_a)
        .await
        .expect("delete a response");
    assert_eq!(delete_a_resp.status(), StatusCode::NOT_FOUND);

    let put_a = tenant_request(
        "PUT",
        "/resource/project/shared/board",
        "org-a",
        "workspace-a",
        "user-a",
        Some(json!({
            "value": {"owner":"tenant-a"},
            "updated_by": "user-a"
        })),
    );
    let put_a_resp = app.clone().oneshot(put_a).await.expect("put a response");
    assert_eq!(put_a_resp.status(), StatusCode::OK);

    for (org, workspace, actor, expected_owner) in [
        ("org-a", "workspace-a", "user-a", "tenant-a"),
        ("org-b", "workspace-b", "user-b", "tenant-b"),
    ] {
        let get = tenant_request(
            "GET",
            "/resource/project/shared/board",
            org,
            workspace,
            actor,
            None,
        );
        let get_resp = app.clone().oneshot(get).await.expect("get response");
        assert_eq!(get_resp.status(), StatusCode::OK);
        let get_body = to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body");
        let payload: Value = serde_json::from_slice(&get_body).expect("get json");
        assert_eq!(
            payload
                .get("resource")
                .and_then(|r| r.get("value"))
                .and_then(|v| v.get("owner"))
                .and_then(|v| v.as_str()),
            Some(expected_owner)
        );
    }
}

#[tokio::test]
async fn tenant_resource_event_visibility_filters_other_tenant_events() {
    let tenant_a = tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "workspace-a",
        None,
        "user-a",
    );
    let tenant_a_event = EngineEvent::new(
        "resource.updated",
        json!({
            "key": "project/shared/board",
            "tenantContext": tenant_a.clone(),
        }),
    );
    let tenant_b_event = EngineEvent::new(
        "resource.updated",
        json!({
            "key": "project/shared/board",
            "tenantContext": tandem_types::TenantContext::explicit_user_workspace(
                "org-b",
                "workspace-b",
                None,
                "user-b",
            ),
        }),
    );

    assert!(super::super::resources::resource_event_visible_to_tenant(
        &tenant_a_event,
        &tenant_a
    ));
    assert!(!super::super::resources::resource_event_visible_to_tenant(
        &tenant_b_event,
        &tenant_a
    ));
}

#[tokio::test]
async fn resource_put_conflict_returns_409() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let first_req = Request::builder()
        .method("PUT")
        .uri("/resource/mission/demo/card-1")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "value": {"title":"Card 1"},
                "updated_by": "agent-1"
            })
            .to_string(),
        ))
        .expect("first request");
    let first_resp = app
        .clone()
        .oneshot(first_req)
        .await
        .expect("first response");
    assert_eq!(first_resp.status(), StatusCode::OK);

    let conflict_req = Request::builder()
        .method("PUT")
        .uri("/resource/mission/demo/card-1")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "value": {"title":"Card 1 updated"},
                "if_match_rev": 99,
                "updated_by": "agent-2"
            })
            .to_string(),
        ))
        .expect("conflict request");
    let conflict_resp = app
        .clone()
        .oneshot(conflict_req)
        .await
        .expect("conflict response");
    assert_eq!(conflict_resp.status(), StatusCode::CONFLICT);
}

#[tokio::test]
async fn resource_updated_event_contract_snapshot() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let put_req = Request::builder()
        .method("PUT")
        .uri("/resource/project/demo/board")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "value": {"status":"todo"},
                "updated_by": "agent-1"
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "resource.updated" {
                return event;
            }
        }
    })
    .await
    .expect("resource.updated timeout");

    let mut properties = event
        .properties
        .as_object()
        .cloned()
        .expect("resource.updated properties object");
    let updated_at_ms = properties
        .remove("updatedAtMs")
        .and_then(|v| v.as_u64())
        .expect("updatedAtMs");
    assert!(updated_at_ms > 0);

    let snapshot = json!({
        "type": event.event_type,
        "properties": properties,
    });
    let expected = json!({
        "type": "resource.updated",
        "properties": {
            "key": "project/demo/board",
            "rev": 1,
            "updatedBy": "agent-1"
        }
    });
    assert_eq!(snapshot, expected);
}

#[tokio::test]
async fn resource_deleted_event_contract_snapshot() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let put_req = Request::builder()
        .method("PUT")
        .uri("/resource/project/demo/board")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "value": {"status":"todo"},
                "updated_by": "agent-1"
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let delete_req = Request::builder()
        .method("DELETE")
        .uri("/resource/project/demo/board")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "if_match_rev": 1,
                "updated_by": "reviewer-1"
            })
            .to_string(),
        ))
        .expect("delete request");
    let delete_resp = app
        .clone()
        .oneshot(delete_req)
        .await
        .expect("delete response");
    assert_eq!(delete_resp.status(), StatusCode::OK);

    let event = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let event = rx.recv().await.expect("event");
            if event.event_type == "resource.deleted" {
                return event;
            }
        }
    })
    .await
    .expect("resource.deleted timeout");

    let mut properties = event
        .properties
        .as_object()
        .cloned()
        .expect("resource.deleted properties object");
    let updated_at_ms = properties
        .remove("updatedAtMs")
        .and_then(|v| v.as_u64())
        .expect("updatedAtMs");
    assert!(updated_at_ms > 0);

    let snapshot = json!({
        "type": event.event_type,
        "properties": properties,
    });
    let expected = json!({
        "type": "resource.deleted",
        "properties": {
            "key": "project/demo/board",
            "rev": 1,
            "updatedBy": "reviewer-1"
        }
    });
    assert_eq!(snapshot, expected);
}
