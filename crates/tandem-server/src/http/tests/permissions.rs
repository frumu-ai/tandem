// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&body).expect("response json")
}

#[tokio::test]
async fn approve_tool_by_call_route_replies_permission() {
    let state = test_state().await;
    let request = state
        .permissions
        .ask_for_session(Some("s1"), "bash", json!({"command":"echo hi"}))
        .await;
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri(format!("/sessions/s1/tools/{}/approve", request.id))
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("ok").and_then(|v| v.as_bool()), Some(true));
}

#[tokio::test]
async fn permission_reply_route_rejects_invalid_reply() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/permission/some-request/reply")
        .header("content-type", "application/json")
        .body(Body::from(json!({"reply":"invalid"}).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(|v| v.as_str()),
        Some("APPROVAL_REPLY_INVALID")
    );
    assert_eq!(
        payload.get("retryable").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[tokio::test]
async fn permission_reply_route_returns_not_found_for_unknown_request() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/permission/missing-request/reply")
        .header("content-type", "application/json")
        .body(Body::from(json!({"reply":"always"}).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(|v| v.as_str()),
        Some("APPROVAL_REQUEST_NOT_FOUND")
    );
    assert_eq!(
        payload.get("retryable").and_then(|v| v.as_bool()),
        Some(false)
    );
}

#[tokio::test]
async fn permission_reply_route_applies_and_persists_allow_rule() {
    let state = test_state().await;
    let request = state
        .permissions
        .ask_for_session(Some("s1"), "glob", json!({"pattern":"*"}))
        .await;
    let app = app_router(state.clone());
    let req = Request::builder()
        .method("POST")
        .uri(format!("/permission/{}/reply", request.id))
        .header("content-type", "application/json")
        .body(Body::from(json!({"reply":"always"}).to_string()))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(payload.get("ok").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(
        payload.get("reply").and_then(|v| v.as_str()),
        Some("always")
    );
    assert_eq!(
        payload.get("persistedRule").and_then(|v| v.as_bool()),
        Some(true)
    );
    let audit = tokio::fs::read_to_string(&state.protected_audit_path)
        .await
        .expect("protected audit file");
    assert!(audit.contains("\"event_type\":\"permission.decision\""));
    assert!(audit.contains("\"permission\":\"glob\""));
    assert!(audit.contains("\"actionDigest\""));
    assert!(audit.contains("\"reason\":\"http_permission_reply\""));
}

#[tokio::test]
async fn hosted_queue_lists_hide_sibling_actor_records() {
    let state = test_state().await;
    let tenant = TenantContext::explicit("queue-org", "queue-workspace", Some("alice".to_string()));
    let mut session = Session::new(Some("alice queue".to_string()), Some(".".to_string()));
    session.tenant_context = tenant.clone();
    let session_id = session.id.clone();
    state
        .storage
        .save_session(session)
        .await
        .expect("save session");
    let permission = state
        .permissions
        .ask_for_session_for_tenant(
            &tenant,
            Some(&session_id),
            "bash",
            json!({"command":"echo secret"}),
        )
        .await;
    let question = state
        .storage
        .add_question_request(
            &session_id,
            "message-1",
            vec![json!({"question":"secret question"})],
        )
        .await
        .expect("add question");
    let reviewer_tenant = TenantContext::explicit(
        "queue-org",
        "queue-workspace",
        Some("reviewer-a".to_string()),
    );
    let reviewer_principal =
        tandem_types::RequestPrincipal::authenticated_user("reviewer-a", "tandem-test");
    let verified = tandem_types::VerifiedTenantContext {
        tenant_context: reviewer_tenant,
        human_actor: tandem_types::HumanActor::tandem_user("reviewer-a"),
        authority_chain: tandem_types::AuthorityChain::from_request(reviewer_principal),
        roles: vec!["admin".to_string()],
        org_units: Vec::new(),
        capabilities: vec!["governance.review".to_string()],
        policy_version: None,
        strict_projection: None,
        issuer: "tandem-test".to_string(),
        audience: "tandem-runtime".to_string(),
        issued_at_ms: 1,
        expires_at_ms: 9_999_999_999_999,
        assertion_id: "queue-agent-reviewer".to_string(),
        assertion_key_id: None,
    };
    let app = app_router(state.clone());
    let agent_app = app_router(state).layer(axum::Extension(verified));

    for (uri, record_key) in [("/permission", "requests"), ("/question", "")] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .header("x-tandem-org-id", "queue-org")
                    .header("x-tandem-workspace-id", "queue-workspace")
                    .header("x-tandem-actor-id", "bob")
                    .body(Body::empty())
                    .expect("sibling list request"),
            )
            .await
            .expect("sibling list response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let records = if record_key.is_empty() {
            body.as_array().expect("question list")
        } else {
            body[record_key].as_array().expect("permission list")
        };
        assert!(records.is_empty(), "sibling actor must not enumerate {uri}");
    }

    let permission_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/permission")
                .header("x-tandem-org-id", "queue-org")
                .header("x-tandem-workspace-id", "queue-workspace")
                .header("x-tandem-actor-id", "alice")
                .body(Body::empty())
                .expect("owner permission list request"),
        )
        .await
        .expect("owner permission list response");
    let permission_body = response_json(permission_response).await;
    assert_eq!(permission_body["requests"][0]["id"], permission.id);

    let question_response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/question")
                .header("x-tandem-org-id", "queue-org")
                .header("x-tandem-workspace-id", "queue-workspace")
                .header("x-tandem-actor-id", "alice")
                .body(Body::empty())
                .expect("owner question list request"),
        )
        .await
        .expect("owner question list response");
    let question_body = response_json(question_response).await;
    assert_eq!(question_body[0]["id"], question.id);

    for (uri, record_key) in [("/permission", "requests"), ("/question", "")] {
        let response = agent_app
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(uri)
                    .header("x-tandem-org-id", "queue-org")
                    .header("x-tandem-workspace-id", "queue-workspace")
                    .header("x-tandem-actor-id", "reviewer-a")
                    .header("x-tandem-agent-id", "agent-reviewer")
                    .body(Body::empty())
                    .expect("agent reviewer list request"),
            )
            .await
            .expect("agent reviewer list response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let records = if record_key.is_empty() {
            body.as_array().expect("question list")
        } else {
            body[record_key].as_array().expect("permission list")
        };
        assert!(
            records.is_empty(),
            "authoritative agent must not inherit reviewer-wide access to {uri}"
        );
    }
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn question_reply_rejects_requester_self_review() {
    let state = test_state().await;
    let tenant = TenantContext::explicit("queue-org", "queue-workspace", Some("alice".to_string()));
    let mut session = Session::new(Some("self review".to_string()), Some(".".to_string()));
    session.tenant_context = tenant.clone();
    let session_id = session.id.clone();
    state
        .storage
        .save_session(session)
        .await
        .expect("save session");
    let question = state
        .storage
        .add_question_request(
            &session_id,
            "message-1",
            vec![json!({"question":"approve my request"})],
        )
        .await
        .expect("add question");
    let principal = tandem_types::RequestPrincipal::authenticated_user("alice", "tandem-test");
    let verified = tandem_types::VerifiedTenantContext {
        tenant_context: tenant,
        human_actor: tandem_types::HumanActor::tandem_user("alice"),
        authority_chain: tandem_types::AuthorityChain::from_request(principal),
        roles: vec!["admin".to_string()],
        org_units: Vec::new(),
        capabilities: vec!["governance.review".to_string()],
        policy_version: None,
        strict_projection: None,
        issuer: "tandem-test".to_string(),
        audience: "tandem-runtime".to_string(),
        issued_at_ms: 1,
        expires_at_ms: 9_999_999_999_999,
        assertion_id: "queue-self-review".to_string(),
        assertion_key_id: None,
    };
    let app = app_router(state.clone()).layer(axum::Extension(verified));
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/question/{}/reply", question.id))
                .header("content-type", "application/json")
                .header("x-tandem-org-id", "queue-org")
                .header("x-tandem-workspace-id", "queue-workspace")
                .header("x-tandem-actor-id", "alice")
                .body(Body::from("{}"))
                .expect("self-review request"),
        )
        .await
        .expect("self-review response");
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert!(state
        .storage
        .get_question_request_for_tenant(&question.id, &question.tenant_context, None)
        .await
        .expect("load question")
        .is_some());
}
