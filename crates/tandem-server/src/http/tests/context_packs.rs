// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[tokio::test]
async fn context_packs_publish_list_bind_revoke_and_supersede_roundtrip() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!("tandem-context-pack-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&workspace_root).expect("workspace root");

    let publish_req = Request::builder()
        .method("POST")
        .uri("/context/packs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "title": "Shared release context",
                "summary": "Approved plan materialization plus artifacts.",
                "workspace_root": workspace_root,
                "project_key": "project-a",
                "source_plan_id": "plan-a",
                "plan_package": {
                    "plan_id": "plan-a",
                    "title": "Plan A",
                    "context_objects": [
                        { "context_object_id": "ctx:1" },
                        { "context_object_id": "ctx:2" }
                    ]
                },
                "approved_plan_materialization": {
                    "plan_id": "plan-a",
                    "plan_revision": 3
                },
                "runtime_context": {
                    "routines": []
                },
                "artifact_refs": ["artifact://one"],
                "governed_memory_refs": ["memory://one"],
                "freshness_window_hours": 24
            })
            .to_string(),
        ))
        .expect("publish request");
    let publish_resp = app.clone().oneshot(publish_req).await.expect("response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_body = to_bytes(publish_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let publish_payload: Value = serde_json::from_slice(&publish_body).expect("json");
    let pack_id = publish_payload
        .get("context_pack")
        .and_then(|value| value.get("pack_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("pack id");
    let publish_hook = next_event_of_type(&mut rx, "context.pack.policy_hook").await;
    assert_eq!(
        publish_hook
            .properties
            .get("action")
            .and_then(Value::as_str),
        Some("publish")
    );
    assert_eq!(
        publish_hook
            .properties
            .get("pack_id")
            .and_then(Value::as_str),
        Some(pack_id.as_str())
    );

    let list_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/context/packs?workspace_root={}",
                    workspace_root.replace('/', "%2F")
                ))
                .body(Body::empty())
                .expect("list request"),
        )
        .await
        .expect("response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
    let packs = list_payload
        .get("context_packs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(packs.len(), 1);
    assert_eq!(
        packs[0].get("pack_id").and_then(Value::as_str),
        Some(pack_id.as_str())
    );

    let bind_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/context/packs/{pack_id}/bind"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "consumer_plan_id": "consumer-plan-1",
                        "consumer_project_key": "project-a",
                        "consumer_workspace_root": workspace_root,
                        "alias": "release-context",
                        "required": true
                    })
                    .to_string(),
                ))
                .expect("bind request"),
        )
        .await
        .expect("response");
    assert_eq!(bind_resp.status(), StatusCode::OK);
    let bind_body = to_bytes(bind_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let bind_payload: Value = serde_json::from_slice(&bind_body).expect("json");
    let bind_hook = next_event_of_type(&mut rx, "context.pack.policy_hook").await;
    assert_eq!(
        bind_hook.properties.get("action").and_then(Value::as_str),
        Some("bind")
    );
    assert_eq!(
        bind_hook.properties.get("pack_id").and_then(Value::as_str),
        Some(pack_id.as_str())
    );
    assert_eq!(
        bind_payload
            .get("context_pack")
            .and_then(|value| value.get("bindings"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );

    let revoke_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/context/packs/{pack_id}/revoke"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({ "actor_metadata": { "source": "test" } }).to_string(),
                ))
                .expect("revoke request"),
        )
        .await
        .expect("response");
    assert_eq!(revoke_resp.status(), StatusCode::OK);
    let revoke_body = to_bytes(revoke_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let revoke_payload: Value = serde_json::from_slice(&revoke_body).expect("json");
    let revoke_hook = next_event_of_type(&mut rx, "context.pack.policy_hook").await;
    assert_eq!(
        revoke_hook.properties.get("action").and_then(Value::as_str),
        Some("revoke")
    );
    assert_eq!(
        revoke_hook
            .properties
            .get("pack_id")
            .and_then(Value::as_str),
        Some(pack_id.as_str())
    );
    assert_eq!(
        revoke_hook
            .properties
            .get("actor_present")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        revoke_payload
            .get("context_pack")
            .and_then(|value| value.get("state"))
            .and_then(Value::as_str),
        Some("revoked")
    );

    let second_publish_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/context/packs")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "title": "Shared release context v2",
                        "workspace_root": workspace_root,
                        "project_key": "project-a",
                        "source_plan_id": "plan-b",
                        "plan_package": { "plan_id": "plan-b", "title": "Plan B" },
                        "approved_plan_materialization": { "plan_id": "plan-b" }
                    })
                    .to_string(),
                ))
                .expect("publish request 2"),
        )
        .await
        .expect("response");
    assert_eq!(second_publish_resp.status(), StatusCode::OK);
    let second_body = to_bytes(second_publish_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let second_payload: Value = serde_json::from_slice(&second_body).expect("json");
    let second_pack_id = second_payload
        .get("context_pack")
        .and_then(|value| value.get("pack_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("pack id");
    let second_publish_hook = next_event_of_type(&mut rx, "context.pack.policy_hook").await;
    assert_eq!(
        second_publish_hook
            .properties
            .get("action")
            .and_then(Value::as_str),
        Some("publish")
    );
    assert_eq!(
        second_publish_hook
            .properties
            .get("pack_id")
            .and_then(Value::as_str),
        Some(second_pack_id.as_str())
    );

    let supersede_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/context/packs/{pack_id}/supersede"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "superseded_by_pack_id": second_pack_id,
                        "actor_metadata": { "source": "test" }
                    })
                    .to_string(),
                ))
                .expect("supersede request"),
        )
        .await
        .expect("response");
    assert_eq!(supersede_resp.status(), StatusCode::OK);
    let supersede_hook = next_event_of_type(&mut rx, "context.pack.policy_hook").await;
    assert_eq!(
        supersede_hook
            .properties
            .get("action")
            .and_then(Value::as_str),
        Some("supersede")
    );
    assert_eq!(
        supersede_hook
            .properties
            .get("pack_id")
            .and_then(Value::as_str),
        Some(pack_id.as_str())
    );
    assert_eq!(
        supersede_hook
            .properties
            .get("related_pack_id")
            .and_then(Value::as_str),
        Some(second_pack_id.as_str())
    );

    let get_resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/packs/{pack_id}"))
                .body(Body::empty())
                .expect("get request"),
        )
        .await
        .expect("response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("json");
    assert_eq!(
        get_payload
            .get("context_pack")
            .and_then(|value| value.get("state"))
            .and_then(Value::as_str),
        Some("superseded")
    );
    assert_eq!(
        get_payload
            .get("context_pack")
            .and_then(|value| value.get("superseded_by_pack_id"))
            .and_then(Value::as_str),
        Some(second_pack_id.as_str())
    );
}

#[tokio::test]
async fn context_packs_list_filters_by_workspace_root() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_a = std::env::temp_dir()
        .join(format!("tandem-context-pack-a-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let workspace_b = std::env::temp_dir()
        .join(format!("tandem-context-pack-b-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&workspace_a).expect("workspace a");
    std::fs::create_dir_all(&workspace_b).expect("workspace b");

    for (title, workspace_root) in [
        ("Pack A", workspace_a.clone()),
        ("Pack B", workspace_b.clone()),
    ] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/context/packs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "title": title,
                            "workspace_root": workspace_root,
                            "plan_package": { "plan_id": title },
                            "approved_plan_materialization": { "plan_id": title }
                        })
                        .to_string(),
                    ))
                    .expect("publish request"),
            )
            .await
            .expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/context/packs?workspace_root={}",
                    workspace_a.replace('/', "%2F")
                ))
                .body(Body::empty())
                .expect("list request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let packs = payload
        .get("context_packs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(packs.len(), 1);
    assert_eq!(
        packs[0].get("title").and_then(Value::as_str),
        Some("Pack A")
    );
}

#[tokio::test]
async fn context_packs_list_filters_by_project_key() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!("tandem-context-pack-project-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&workspace_root).expect("workspace root");

    for (title, project_key) in [("Pack A", "project-a"), ("Pack B", "project-b")] {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/context/packs")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        json!({
                            "title": title,
                            "workspace_root": workspace_root,
                            "project_key": project_key,
                            "plan_package": { "plan_id": title },
                            "approved_plan_materialization": { "plan_id": title }
                        })
                        .to_string(),
                    ))
                    .expect("publish request"),
            )
            .await
            .expect("response");
        assert_eq!(resp.status(), StatusCode::OK);
    }

    let resp = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/context/packs?project_key=project-a")
                .body(Body::empty())
                .expect("list request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let packs = payload
        .get("context_packs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(packs.len(), 1);
    assert_eq!(
        packs[0].get("title").and_then(Value::as_str),
        Some("Pack A")
    );
    assert_eq!(
        packs[0].get("project_key").and_then(Value::as_str),
        Some("project-a")
    );
}

#[tokio::test]
async fn context_packs_allowlist_grants_explicit_cross_project_access() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!("tandem-context-pack-allowlist-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&workspace_root).expect("workspace root");

    let publish_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/context/packs")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "title": "Allowlisted pack",
                        "workspace_root": workspace_root,
                        "project_key": "project-a",
                        "allowed_project_keys": ["project-b"],
                        "plan_package": { "plan_id": "plan-allowlisted" },
                        "approved_plan_materialization": { "plan_id": "plan-allowlisted" }
                    })
                    .to_string(),
                ))
                .expect("publish request"),
        )
        .await
        .expect("response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_body = to_bytes(publish_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let publish_payload: Value = serde_json::from_slice(&publish_body).expect("json");
    let pack_id = publish_payload
        .get("context_pack")
        .and_then(|value| value.get("pack_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("pack id");
    assert_eq!(
        publish_payload
            .get("context_pack")
            .and_then(|value| value.get("visibility_scope"))
            .and_then(Value::as_str),
        Some("project_allowlist")
    );
    assert_eq!(
        publish_payload
            .get("context_pack")
            .and_then(|value| value.get("allowed_project_keys"))
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_str),
        Some("project-b")
    );

    let list_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/context/packs?workspace_root={}&project_key=project-b",
                    workspace_root.replace('/', "%2F")
                ))
                .body(Body::empty())
                .expect("list request"),
        )
        .await
        .expect("response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("json");
    let packs = list_payload
        .get("context_packs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(packs.len(), 1);
    assert_eq!(
        packs[0].get("pack_id").and_then(Value::as_str),
        Some(pack_id.as_str())
    );

    let get_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/packs/{pack_id}?project_key=project-b"))
                .body(Body::empty())
                .expect("get request"),
        )
        .await
        .expect("response");
    assert_eq!(get_resp.status(), StatusCode::OK);

    let bind_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/context/packs/{pack_id}/bind"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "consumer_plan_id": "consumer-plan-allowlisted",
                        "consumer_project_key": "project-b",
                        "consumer_workspace_root": workspace_root,
                        "required": true
                    })
                    .to_string(),
                ))
                .expect("bind request"),
        )
        .await
        .expect("response");
    assert_eq!(bind_resp.status(), StatusCode::OK);

    let forbidden_list_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/context/packs?workspace_root={}&project_key=project-c",
                    workspace_root.replace('/', "%2F")
                ))
                .body(Body::empty())
                .expect("list request"),
        )
        .await
        .expect("response");
    assert_eq!(forbidden_list_resp.status(), StatusCode::OK);
    let forbidden_list_body = to_bytes(forbidden_list_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let forbidden_list_payload: Value = serde_json::from_slice(&forbidden_list_body).expect("json");
    let forbidden_packs = forbidden_list_payload
        .get("context_packs")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(forbidden_packs.is_empty());
}

#[tokio::test]
async fn context_packs_bind_rejects_workspace_mismatch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!("tandem-context-pack-bind-a-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let other_workspace_root = std::env::temp_dir()
        .join(format!("tandem-context-pack-bind-b-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&workspace_root).expect("workspace root");
    std::fs::create_dir_all(&other_workspace_root).expect("other workspace root");

    let publish_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/context/packs")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "title": "Bind workspace pack",
                        "workspace_root": workspace_root,
                        "project_key": "project-a",
                        "plan_package": { "plan_id": "plan-bind-workspace" },
                        "approved_plan_materialization": { "plan_id": "plan-bind-workspace" }
                    })
                    .to_string(),
                ))
                .expect("publish request"),
        )
        .await
        .expect("response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_body = to_bytes(publish_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let publish_payload: Value = serde_json::from_slice(&publish_body).expect("json");
    let pack_id = publish_payload
        .get("context_pack")
        .and_then(|value| value.get("pack_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("pack id");

    let bind_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/context/packs/{pack_id}/bind"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "consumer_plan_id": "consumer-plan-bind-workspace",
                        "consumer_project_key": "project-a",
                        "consumer_workspace_root": other_workspace_root,
                        "required": true
                    })
                    .to_string(),
                ))
                .expect("bind request"),
        )
        .await
        .expect("response");
    assert_eq!(bind_resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn context_packs_bind_rejects_project_mismatch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!(
            "tandem-context-pack-bind-project-{}",
            Uuid::new_v4()
        ))
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&workspace_root).expect("workspace root");

    let publish_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/context/packs")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "title": "Bind project pack",
                        "workspace_root": workspace_root,
                        "project_key": "project-a",
                        "plan_package": { "plan_id": "plan-bind-project" },
                        "approved_plan_materialization": { "plan_id": "plan-bind-project" }
                    })
                    .to_string(),
                ))
                .expect("publish request"),
        )
        .await
        .expect("response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_body = to_bytes(publish_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let publish_payload: Value = serde_json::from_slice(&publish_body).expect("json");
    let pack_id = publish_payload
        .get("context_pack")
        .and_then(|value| value.get("pack_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("pack id");

    let bind_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/context/packs/{pack_id}/bind"))
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "consumer_plan_id": "consumer-plan-bind-project",
                        "consumer_project_key": "project-b",
                        "consumer_workspace_root": workspace_root,
                        "required": true
                    })
                    .to_string(),
                ))
                .expect("bind request"),
        )
        .await
        .expect("response");
    assert_eq!(bind_resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn context_packs_get_rejects_workspace_mismatch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!("tandem-context-pack-get-a-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    let other_workspace_root = std::env::temp_dir()
        .join(format!("tandem-context-pack-get-b-{}", Uuid::new_v4()))
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&workspace_root).expect("workspace root");
    std::fs::create_dir_all(&other_workspace_root).expect("other workspace root");

    let publish_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/context/packs")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "title": "Get workspace pack",
                        "workspace_root": workspace_root,
                        "project_key": "project-a",
                        "plan_package": { "plan_id": "plan-get-workspace" },
                        "approved_plan_materialization": { "plan_id": "plan-get-workspace" }
                    })
                    .to_string(),
                ))
                .expect("publish request"),
        )
        .await
        .expect("response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_body = to_bytes(publish_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let publish_payload: Value = serde_json::from_slice(&publish_body).expect("json");
    let pack_id = publish_payload
        .get("context_pack")
        .and_then(|value| value.get("pack_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("pack id");

    let get_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/context/packs/{pack_id}?workspace_root={}",
                    other_workspace_root.replace('/', "%2F")
                ))
                .body(Body::empty())
                .expect("get request"),
        )
        .await
        .expect("response");
    assert_eq!(get_resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn context_packs_get_rejects_project_mismatch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let workspace_root = std::env::temp_dir()
        .join(format!(
            "tandem-context-pack-get-project-{}",
            Uuid::new_v4()
        ))
        .to_string_lossy()
        .to_string();
    std::fs::create_dir_all(&workspace_root).expect("workspace root");

    let publish_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/context/packs")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "title": "Get project pack",
                        "workspace_root": workspace_root,
                        "project_key": "project-a",
                        "plan_package": { "plan_id": "plan-get-project" },
                        "approved_plan_materialization": { "plan_id": "plan-get-project" }
                    })
                    .to_string(),
                ))
                .expect("publish request"),
        )
        .await
        .expect("response");
    assert_eq!(publish_resp.status(), StatusCode::OK);
    let publish_body = to_bytes(publish_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let publish_payload: Value = serde_json::from_slice(&publish_body).expect("json");
    let pack_id = publish_payload
        .get("context_pack")
        .and_then(|value| value.get("pack_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .expect("pack id");

    let get_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/packs/{pack_id}?project_key=project-b"))
                .body(Body::empty())
                .expect("get request"),
        )
        .await
        .expect("response");
    assert_eq!(get_resp.status(), StatusCode::FORBIDDEN);
}
