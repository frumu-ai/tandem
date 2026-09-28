// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

fn hosted_revision_router(state: AppState, actor: &str) -> axum::Router {
    let tenant = hosted_learning_tenant(actor);
    let verified = hosted_learning_verified(&state, actor);
    axum::Router::<AppState>::new()
        .route(
            "/workflow-learning/candidates/{candidate_id}/spawn-revision",
            axum::routing::post(skills_memory::workflow_learning_candidate_spawn_revision),
        )
        .route(
            "/workflow-plans/sessions",
            axum::routing::get(crate::http::workflow_planner::workflow_planner_session_list)
                .post(crate::http::workflow_planner::workflow_planner_session_create),
        )
        .route(
            "/workflow-plans/sessions/{session_id}",
            axum::routing::get(crate::http::workflow_planner::workflow_planner_session_get)
                .patch(crate::http::workflow_planner::workflow_planner_session_patch)
                .delete(crate::http::workflow_planner::workflow_planner_session_delete),
        )
        .route(
            "/workflow-plans/sessions/{session_id}/duplicate",
            axum::routing::post(crate::http::workflow_planner::workflow_planner_session_duplicate),
        )
        .route(
            "/workflow-plans/sessions/{session_id}/start",
            axum::routing::post(crate::http::workflow_planner::workflow_planner_session_start),
        )
        .route(
            "/workflow-plans/sessions/{session_id}/start-async",
            axum::routing::post(
                crate::http::workflow_planner::workflow_planner_session_start_async,
            ),
        )
        .route(
            "/workflow-plans/sessions/{session_id}/message",
            axum::routing::post(crate::http::workflow_planner::workflow_planner_session_message),
        )
        .route(
            "/workflow-plans/sessions/{session_id}/message-async",
            axum::routing::post(
                crate::http::workflow_planner::workflow_planner_session_message_async,
            ),
        )
        .route(
            "/workflow-plans/sessions/{session_id}/reset",
            axum::routing::post(crate::http::workflow_planner::workflow_planner_session_reset),
        )
        .route(
            "/workflow-plans/{plan_id}",
            axum::routing::get(crate::http::workflow_planner::workflow_plan_get),
        )
        .route(
            "/workflow-plans/chat/message",
            axum::routing::post(crate::http::workflow_planner::workflow_plan_chat_message),
        )
        .route(
            "/workflow-plans/chat/reset",
            axum::routing::post(crate::http::workflow_planner::workflow_plan_chat_reset),
        )
        .layer(axum::Extension(tenant))
        .layer(axum::Extension(verified))
        .with_state(state)
}

#[tokio::test]
async fn hosted_spawned_revision_session_hides_private_evidence_from_another_actor() {
    let (state, _policy) = hosted_learning_state().await;
    let root = state.workspace_index.snapshot().await.root;
    let mut source = hosted_learning_automation(&root, "alice-revision-source", "alice");
    source.metadata.as_mut().unwrap()["plan_package_bundle"] = json!(sample_plan_package_bundle());
    let source = state
        .put_automation_v2(source)
        .await
        .expect("Alice workflow");
    state
        .put_workflow_learning_candidate(candidate_for_workflow(
            sample_candidate(
                "alice-revision-evidence",
                &source.automation_id,
                crate::WorkflowLearningCandidateKind::PromptPatch,
                crate::WorkflowLearningCandidateStatus::Approved,
            ),
            &source,
        ))
        .await
        .expect("Alice candidate");

    let alice = hosted_revision_router(state.clone(), "alice");
    let (status, payload) = hosted_learning_request(
        alice.clone(),
        "POST",
        "/workflow-learning/candidates/alice-revision-evidence/spawn-revision",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{payload}");
    let session_id = payload["session"]["session_id"]
        .as_str()
        .expect("spawned session id");
    let plan_id = payload["session"]["current_plan_id"]
        .as_str()
        .expect("spawned plan id");
    let spawned_session = payload["session"].clone();
    let session_uri = format!("/workflow-plans/sessions/{session_id}");
    let plan_uri = format!("/workflow-plans/{plan_id}");

    let (status, payload) = hosted_learning_request(alice.clone(), "GET", &session_uri, None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(payload["session"]["notes"]
        .as_str()
        .unwrap()
        .contains("alice-revision-evidence"));
    let (status, payload) =
        hosted_learning_request(alice, "GET", "/workflow-plans/sessions", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(payload["count"], 1);
    let (status, _) = hosted_learning_request(
        hosted_revision_router(state.clone(), "alice"),
        "GET",
        &plan_uri,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, second) = hosted_learning_request(
        hosted_revision_router(state.clone(), "alice"),
        "POST",
        "/workflow-learning/candidates/alice-revision-evidence/spawn-revision",
        Some(json!({})),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "second revision failed: {second}");
    assert_ne!(second["session"]["session_id"].as_str(), Some(session_id));
    assert_ne!(second["session"]["current_plan_id"].as_str(), Some(plan_id));
    let (status, alice_list) = hosted_learning_request(
        hosted_revision_router(state.clone(), "alice"),
        "GET",
        "/workflow-plans/sessions",
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(alice_list["count"], 2);

    let bob = hosted_revision_router(state.clone(), "bob");
    let (status, payload) =
        hosted_learning_request(bob.clone(), "GET", "/workflow-plans/sessions", None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        payload["count"], 0,
        "private session metadata leaked: {payload}"
    );
    let (status, payload) = hosted_learning_request(bob, "GET", &session_uri, None).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "private evidence leaked: {payload}"
    );

    let bob = hosted_revision_router(state.clone(), "bob");
    let (status, payload) = hosted_learning_request(bob.clone(), "GET", &plan_uri, None).await;
    assert_eq!(status, StatusCode::NOT_FOUND, "raw plan leaked: {payload}");
    for (uri, body) in [
        (
            "/workflow-plans/chat/message",
            json!({"plan_id": plan_id, "message": "stolen"}),
        ),
        ("/workflow-plans/chat/reset", json!({"plan_id": plan_id})),
    ] {
        let (status, payload) = hosted_learning_request(bob.clone(), "POST", uri, Some(body)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{uri}: {payload}");
    }

    let (status, payload) = hosted_learning_request(
        bob.clone(),
        "POST",
        "/workflow-plans/sessions",
        Some(json!({
            "project_slug": "bob-project",
            "plan": spawned_session["draft"]["current_plan"],
            "conversation": spawned_session["draft"]["conversation"],
        })),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "duplicate plan ID accepted: {payload}"
    );
    let (status, created) = hosted_learning_request(
        bob.clone(),
        "POST",
        "/workflow-plans/sessions",
        Some(json!({
            "project_slug": "bob-project",
            "goal": "Bob's own plan",
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{created}");
    let bob_session_id = created["session"]["session_id"].as_str().unwrap();
    let (status, payload) = hosted_learning_request(
        bob.clone(),
        "PATCH",
        &format!("/workflow-plans/sessions/{bob_session_id}"),
        Some(json!({"current_plan_id": plan_id})),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "foreign plan alias accepted: {payload}"
    );

    let bob = hosted_revision_router(state.clone(), "bob");
    for (method, suffix, body) in [
        ("PATCH", "", Some(json!({"notes": "stolen"}))),
        ("DELETE", "", None),
        ("POST", "/duplicate", Some(json!({}))),
        ("POST", "/start", Some(json!({"prompt": "stolen"}))),
        ("POST", "/start-async", Some(json!({"prompt": "stolen"}))),
        ("POST", "/message", Some(json!({"message": "stolen"}))),
        ("POST", "/message-async", Some(json!({"message": "stolen"}))),
        ("POST", "/reset", None),
    ] {
        let (status, payload) =
            hosted_learning_request(bob.clone(), method, &format!("{session_uri}{suffix}"), body)
                .await;
        assert_eq!(
            status,
            StatusCode::NOT_FOUND,
            "{method} {suffix}: {payload}"
        );
    }
    let retained = state
        .get_workflow_planner_session(session_id)
        .await
        .expect("denied mutations preserve Alice's session");
    assert!(retained.notes.contains("alice-revision-evidence"));
    assert!(retained.operation.is_none());
}
