#[tokio::test]
async fn memory_put_carries_explicit_trust_label_in_metadata_and_provenance() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let capability = memory_capability(
        "trust-label-run",
        "user-trust-label",
        "org-1",
        "ws-1",
        "proj-1",
    );

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "trust-label-run",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "external note should remain marked as untrusted evidence",
                "classification": "internal",
                "metadata": {
                    "memory_trust": {
                        "label": "external_user_supplied"
                    }
                },
                "capability": capability
            })
            .to_string(),
        ))
        .expect("trust label put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);

    let list_req = Request::builder()
        .method("GET")
        .uri("/memory?limit=20&user_id=user-trust-label&project_id=proj-1")
        .body(Body::empty())
        .expect("memory list request");
    let list_resp = app.oneshot(list_req).await.expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("list json");
    let item = list_payload
        .get("items")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .expect("stored memory item");
    assert_eq!(
        item.pointer("/metadata/memory_trust/label")
            .and_then(Value::as_str),
        Some("external_user_supplied")
    );
    assert_eq!(
        item.pointer("/metadata/memory_trust/trusted_for_promotion")
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        item.pointer("/provenance/memory_trust/label")
            .and_then(Value::as_str),
        Some("external_user_supplied")
    );
}

#[tokio::test]
async fn memory_promote_rejects_untrusted_source_without_review_evidence() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();
    let capability = memory_capability(
        "untrusted-promote-run",
        "user-untrusted-promote",
        "org-1",
        "ws-1",
        "proj-1",
    );

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "untrusted-promote-run",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "untrusted memory must not be promoted without review",
                "classification": "internal",
                "metadata": {
                    "memory_trust": {
                        "label": "external_user_supplied"
                    }
                },
                "capability": capability
            })
            .to_string(),
        ))
        .expect("untrusted put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);
    let put_body = to_bytes(put_resp.into_body(), usize::MAX)
        .await
        .expect("put body");
    let put_payload: Value = serde_json::from_slice(&put_body).expect("put json");
    let memory_id = put_payload
        .get("id")
        .and_then(Value::as_str)
        .expect("memory id")
        .to_string();
    let _put_event = next_event_of_type(&mut rx, "memory.put").await;

    let promote_req = Request::builder()
        .method("POST")
        .uri("/memory/promote")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "untrusted-promote-run",
                "source_memory_id": memory_id,
                "from_tier": "session",
                "to_tier": "project",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "reason": "attempt unreviewed promotion",
                "review": {
                    "required": false
                },
                "capability": capability
            })
            .to_string(),
        ))
        .expect("untrusted promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::FORBIDDEN);

    let blocked_event = next_event_of_type(&mut rx, "memory.promote").await;
    assert_eq!(
        blocked_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert!(blocked_event
        .properties
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("untrusted memory promotion requires review evidence")));
}
