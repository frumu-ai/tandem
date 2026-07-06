#[tokio::test]
async fn same_tenant_actor_cannot_access_another_actor_session_routes() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let session_a = create_tenant_session(
        app.clone(),
        "org-a",
        "workspace-a",
        "user-a",
        "actor a session",
    )
    .await;
    let session_b = create_tenant_session(
        app.clone(),
        "org-a",
        "workspace-a",
        "user-b",
        "actor b session",
    )
    .await;
    let session_a_id = session_a["id"].as_str().expect("session a id").to_string();
    let session_b_id = session_b["id"].as_str().expect("session b id").to_string();

    state
        .storage
        .append_message(
            &session_b_id,
            Message::new(
                MessageRole::User,
                vec![MessagePart::Text {
                    text: "actor b secret".to_string(),
                }],
            ),
        )
        .await
        .expect("append actor b message");

    let list_resp = app
        .clone()
        .oneshot(tenant_request(
            "GET",
            "/session?scope=global&page_size=50",
            "org-a",
            "workspace-a",
            "user-a",
            None,
        ))
        .await
        .expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let listed: Vec<Value> = serde_json::from_slice(&list_body).expect("session list");
    let listed_ids = listed
        .iter()
        .filter_map(|session| session.get("id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(listed_ids.contains(&session_a_id.as_str()));
    assert!(!listed_ids.contains(&session_b_id.as_str()));

    let status_resp = app
        .clone()
        .oneshot(tenant_request(
            "GET",
            "/session/status",
            "org-a",
            "workspace-a",
            "user-a",
            None,
        ))
        .await
        .expect("status response");
    assert_eq!(status_resp.status(), StatusCode::OK);
    let status_body = to_bytes(status_resp.into_body(), usize::MAX)
        .await
        .expect("status body");
    let status_payload: Value = serde_json::from_slice(&status_body).expect("status json");
    assert!(status_payload.get(&session_a_id).is_some());
    assert!(status_payload.get(&session_b_id).is_none());

    for (method, uri, body) in [
        ("GET", format!("/session/{session_b_id}"), None),
        ("GET", format!("/session/{session_b_id}/message"), None),
        ("GET", format!("/session/{session_b_id}/todo"), None),
        ("GET", format!("/session/{session_b_id}/run"), None),
        ("GET", format!("/session/{session_b_id}/diff"), None),
        ("GET", format!("/session/{session_b_id}/children"), None),
        (
            "POST",
            format!("/session/{session_b_id}/message"),
            Some(json!({"parts":[{"type":"text","text":"nope"}]})),
        ),
        (
            "POST",
            format!("/session/{session_b_id}/prompt_async"),
            Some(json!({"parts":[{"type":"text","text":"nope"}]})),
        ),
        (
            "POST",
            format!("/session/{session_b_id}/prompt_sync"),
            Some(json!({"parts":[{"type":"text","text":"nope"}]})),
        ),
        (
            "POST",
            format!("/session/{session_b_id}/attach"),
            Some(json!({"target_workspace":"/tmp/actor-a"})),
        ),
        (
            "POST",
            format!("/session/{session_b_id}/workspace/override"),
            Some(json!({"ttl_seconds":60})),
        ),
        (
            "PATCH",
            format!("/session/{session_b_id}"),
            Some(json!({"title":"stolen"})),
        ),
        ("POST", format!("/session/{session_b_id}/abort"), None),
        (
            "POST",
            format!("/session/{session_b_id}/run/run-b/cancel"),
            None,
        ),
        ("POST", format!("/session/{session_b_id}/fork"), None),
        ("POST", format!("/session/{session_b_id}/revert"), None),
        ("POST", format!("/session/{session_b_id}/unrevert"), None),
        ("POST", format!("/session/{session_b_id}/share"), None),
        ("DELETE", format!("/session/{session_b_id}/share"), None),
        ("POST", format!("/session/{session_b_id}/summarize"), None),
        ("DELETE", format!("/session/{session_b_id}"), None),
    ] {
        let status = tenant_status(
            app.clone(),
            method,
            uri,
            "org-a",
            "workspace-a",
            "user-a",
            body,
        )
        .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} should be actor-hidden"
        );
    }

    let actor_b_session = state
        .storage
        .get_session(&session_b_id)
        .await
        .expect("actor b session still exists");
    assert_eq!(actor_b_session.title, "actor b session");
    assert_eq!(actor_b_session.messages.len(), 1);
}

#[tokio::test]
async fn explicit_tenant_session_request_without_actor_fails_closed() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let session = create_tenant_session(
        app.clone(),
        "org-a",
        "workspace-a",
        "user-a",
        "actor a session",
    )
    .await;
    let session_id = session["id"].as_str().expect("session id").to_string();

    let list_req = Request::builder()
        .method("GET")
        .uri("/session?scope=global&page_size=50")
        .header("x-tandem-org-id", "org-a")
        .header("x-tandem-workspace-id", "workspace-a")
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.clone().oneshot(list_req).await.expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let listed: Vec<Value> = serde_json::from_slice(&list_body).expect("session list");
    assert!(!listed.iter().any(|session| {
        session.get("id").and_then(Value::as_str) == Some(session_id.as_str())
    }));

    let get_req = Request::builder()
        .method("GET")
        .uri(format!("/session/{session_id}"))
        .header("x-tandem-org-id", "org-a")
        .header("x-tandem-workspace-id", "workspace-a")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::NOT_FOUND);
}

