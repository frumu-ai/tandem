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

fn hosted_queue_app(
    state: &AppState,
    verified: tandem_types::VerifiedTenantContext,
) -> axum::Router {
    hosted_queue_app_as(state, verified, "admin")
}

fn hosted_queue_app_as(
    state: &AppState,
    verified: tandem_types::VerifiedTenantContext,
    tenant_actor: &str,
) -> axum::Router {
    super::super::routes_permissions_questions::apply(axum::Router::new())
        .layer(axum::extract::Extension(
            super::legacy_routine_authority::tenant(tenant_actor),
        ))
        .layer(axum::extract::Extension(
            tandem_types::RequestPrincipal::authenticated_user("admin", "tandem-web"),
        ))
        .layer(axum::extract::Extension(verified))
        .with_state(state.clone())
}

fn hosted_queue_identity(state: &AppState, version: u64) -> tandem_types::VerifiedTenantContext {
    let mut verified = super::legacy_routine_authority::verified("admin", "admin");
    verified.policy_version = Some(version);
    state
        .enterprise
        .hosted_policy
        .project(&mut verified)
        .expect("project current hosted admin");
    verified
}

async fn set_hosted_queue_reviewer_role(
    state: &AppState,
    policy_path: &std::path::Path,
    version: u64,
    role: Option<&str>,
) {
    let mut policy: Value = serde_json::from_slice(&std::fs::read(policy_path).unwrap()).unwrap();
    policy["policy_version"] = json!(version);
    policy["generated_at"] = json!(
        chrono::DateTime::from_timestamp_millis(crate::now_ms() as i64).expect("current time")
    );
    let users = policy["users"].as_array_mut().expect("policy users");
    if let Some(role) = role {
        let capabilities = tandem_enterprise_contract::hosted_policy::role_capabilities(role);
        if let Some(admin) = users.iter_mut().find(|user| user["id"] == "admin") {
            admin["role"] = json!(role);
            admin["capabilities"] = json!(capabilities);
        } else {
            users.push(json!({
                "id": "admin", "email": null, "username": null, "role": role,
                "capabilities": capabilities, "is_active": true, "email_verified": true
            }));
        }
    } else {
        users.retain(|user| user["id"] != "admin");
    }
    std::fs::write(policy_path, serde_json::to_vec(&policy).unwrap()).unwrap();
    state.reload_hosted_policy().await.expect("publish policy");
}

async fn wait_for_queue_audit(state: &AppState, event_type: &str, request_id: &str) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        let audit = tokio::fs::read_to_string(&state.protected_audit_path)
            .await
            .unwrap_or_default();
        if audit.contains(event_type) && audit.contains(request_id) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "queue audit not reached"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[tokio::test]
async fn hosted_permission_reply_rechecks_policy_after_queue_writer_wait() {
    for revoked_role in [Some("member"), None] {
        let (state, policy_dir) = super::legacy_routine_authority::hosted_state().await;
        let policy_path = policy_dir.path().join("policy.json");
        let stale_admin = hosted_queue_identity(&state, 1);
        let requester = super::legacy_routine_authority::tenant("alice");
        let request = state
            .permissions
            .ask_for_session_for_tenant(&requester, None, "glob", json!({"pattern":"*"}))
            .await;
        let mut events = state.event_bus.subscribe();

        let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let permissions = state.permissions.clone();
        let blocker = tokio::spawn(async move {
            permissions
                .reply_with_provenance_for_tenant_checked(
                    &requester,
                    None,
                    "missing-queue-request",
                    "once",
                    None,
                    None,
                    async {
                        locked_tx.send(()).unwrap();
                        release_rx.await.unwrap();
                        Ok(())
                    },
                )
                .await
        });
        locked_rx.await.expect("permission writer acquired");

        let app = hosted_queue_app(&state, stale_admin);
        let request_id = request.id.clone();
        let reply = tokio::spawn(async move {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/permission/{request_id}/reply"))
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"reply":"always"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
        });
        wait_for_queue_audit(&state, "permission.decision", &request.id).await;
        assert!(
            !reply.is_finished(),
            "decision must wait for the writer lock"
        );
        set_hosted_queue_reviewer_role(&state, &policy_path, 2, revoked_role).await;
        release_tx.send(()).unwrap();
        assert!(blocker.await.unwrap().unwrap().is_none());
        assert_eq!(reply.await.unwrap().status(), StatusCode::FORBIDDEN);
        assert_eq!(
            state
                .permissions
                .get_for_tenant(&request.id, &request.tenant_context)
                .await
                .unwrap()
                .status,
            "pending"
        );
        assert!(state
            .permissions
            .list_rules_for_tenant(&request.tenant_context)
            .await
            .is_empty());
        assert!(state
            .permissions
            .list_decisions_for_tenant(&request.tenant_context)
            .await
            .is_empty());
        assert!(matches!(
            events.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));
        let (waiter, _) = state
            .permissions
            .wait_for_reply_with_timeout(
                &request.id,
                tokio_util::sync::CancellationToken::new(),
                Some(Duration::from_millis(1)),
            )
            .await;
        assert!(waiter.is_none());

        set_hosted_queue_reviewer_role(&state, &policy_path, 3, Some("admin")).await;
        let fresh_admin = hosted_queue_identity(&state, 3);
        let response = hosted_queue_app(&state, fresh_admin)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/permission/{}/reply", request.id))
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"reply":"always"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn hosted_question_reply_rechecks_policy_after_queue_writer_wait() {
    for revoked_role in [Some("member"), None] {
        let (state, policy_dir) = super::legacy_routine_authority::hosted_state().await;
        let policy_path = policy_dir.path().join("policy.json");
        let stale_admin = hosted_queue_identity(&state, 1);
        let mut session = Session::new(Some("queue question".into()), Some(".".into()));
        session.tenant_context = super::legacy_routine_authority::tenant("alice");
        let session_id = session.id.clone();
        state.storage.save_session(session).await.unwrap();
        let question = state
            .storage
            .add_question_request(
                &session_id,
                "queue-message",
                vec![json!({"question":"approve?"})],
            )
            .await
            .unwrap();
        let mut events = state.event_bus.subscribe();

        let (locked_tx, locked_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let blocker_state = state.clone();
        let requester = question.tenant_context.clone();
        let blocker = tokio::spawn(async move {
            blocker_state
                .storage
                .decide_question_for_tenant_checked(
                    "missing-queue-question",
                    &requester,
                    None,
                    async {
                        locked_tx.send(()).unwrap();
                        release_rx.await.unwrap();
                        Ok(())
                    },
                )
                .await
        });
        locked_rx.await.expect("question writer acquired");

        let app = hosted_queue_app(&state, stale_admin);
        let question_id = question.id.clone();
        let reply = tokio::spawn(async move {
            app.oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/question/{question_id}/reply"))
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap()
        });
        wait_for_queue_audit(&state, "question.replied", &question.id).await;
        assert!(
            !reply.is_finished(),
            "decision must wait for the writer lock"
        );
        set_hosted_queue_reviewer_role(&state, &policy_path, 2, revoked_role).await;
        release_tx.send(()).unwrap();
        assert!(blocker.await.unwrap().unwrap().is_none());
        assert_eq!(reply.await.unwrap().status(), StatusCode::FORBIDDEN);
        assert!(state
            .storage
            .get_question_request_for_tenant(&question.id, &question.tenant_context, None)
            .await
            .unwrap()
            .is_some());
        assert!(matches!(
            events.try_recv(),
            Err(broadcast::error::TryRecvError::Empty)
        ));

        set_hosted_queue_reviewer_role(&state, &policy_path, 3, Some("admin")).await;
        let fresh_admin = hosted_queue_identity(&state, 3);
        let response = hosted_queue_app(&state, fresh_admin)
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/question/{}/reply", question.id))
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}

#[tokio::test]
async fn unversioned_legacy_reviewer_keeps_queue_decision_access() {
    let state = test_state().await;
    let requester = super::legacy_routine_authority::tenant("alice");
    let permission = state
        .permissions
        .ask_for_session_for_tenant(&requester, None, "glob", json!({"pattern":"*"}))
        .await;
    let mut session = Session::new(Some("legacy queue".into()), Some(".".into()));
    session.tenant_context = requester;
    let session_id = session.id.clone();
    state.storage.save_session(session).await.unwrap();
    let question = state
        .storage
        .add_question_request(
            &session_id,
            "legacy-question",
            vec![json!({"question":"ok?"})],
        )
        .await
        .unwrap();

    let mut reviewer = super::legacy_routine_authority::verified("admin", "admin");
    reviewer.policy_version = None;
    reviewer.roles = vec!["admin".into()];
    reviewer.capabilities = vec!["governance.review".into()];
    reviewer.strict_projection = None;
    let app = hosted_queue_app(&state, reviewer);
    let permission_response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/permission/{}/reply", permission.id))
                .header("content-type", "application/json")
                .body(Body::from(json!({"reply":"once"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(permission_response.status(), StatusCode::OK);
    let question_response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/question/{}/reply", question.id))
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(question_response.status(), StatusCode::OK);
}

#[tokio::test]
async fn hosted_queue_decision_requires_current_policy_and_bound_actor() {
    let (hosted_state, _policy_dir) = super::legacy_routine_authority::hosted_state().await;
    let admin = hosted_queue_identity(&hosted_state, 1);
    let requester = super::legacy_routine_authority::tenant("alice");
    let mismatched_request = hosted_state
        .permissions
        .ask_for_session_for_tenant(&requester, None, "glob", json!({"pattern":"*"}))
        .await;
    let response = hosted_queue_app_as(&hosted_state, admin.clone(), "bob")
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/permission/{}/reply", mismatched_request.id))
                .header("content-type", "application/json")
                .body(Body::from(json!({"reply":"once"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        hosted_state
            .permissions
            .get_for_tenant(&mismatched_request.id, &requester)
            .await
            .unwrap()
            .status,
        "pending"
    );

    let mut unversioned = admin.clone();
    unversioned.policy_version = None;
    unversioned.roles = vec!["admin".into()];
    unversioned.capabilities = vec!["governance.review".into()];
    unversioned.strict_projection = None;
    let unversioned_request = hosted_state
        .permissions
        .ask_for_session_for_tenant(&requester, None, "glob", json!({"pattern":"*"}))
        .await;
    let response = hosted_queue_app(&hosted_state, unversioned)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/permission/{}/reply", unversioned_request.id))
                .header("content-type", "application/json")
                .body(Body::from(json!({"reply":"once"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        hosted_state
            .permissions
            .get_for_tenant(&unversioned_request.id, &requester)
            .await
            .unwrap()
            .status,
        "pending"
    );

    let no_policy_state = test_state().await;
    let missing_policy_request = no_policy_state
        .permissions
        .ask_for_session_for_tenant(&requester, None, "glob", json!({"pattern":"*"}))
        .await;
    let response = hosted_queue_app(&no_policy_state, admin)
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/permission/{}/reply", missing_policy_request.id))
                .header("content-type", "application/json")
                .body(Body::from(json!({"reply":"once"}).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(
        no_policy_state
            .permissions
            .get_for_tenant(&missing_policy_request.id, &requester)
            .await
            .unwrap()
            .status,
        "pending"
    );
}
