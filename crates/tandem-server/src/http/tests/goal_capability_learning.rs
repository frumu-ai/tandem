// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

fn request(method: &str, path: &str, actor: Option<&str>, body: Option<Value>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("x-tandem-org-id", "org-a")
        .header("x-tandem-workspace-id", "workspace-a");
    if let Some(actor) = actor {
        builder = builder.header("x-tandem-actor-id", actor);
    }
    let body = match body {
        Some(value) => {
            builder = builder.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let mut request = builder.body(body).expect("capability request");
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [127, 0, 0, 1],
            43123,
        ))));
    request
}

async fn response_json(response: axum::response::Response) -> Value {
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body");
    serde_json::from_slice(&body).expect("response JSON")
}

#[tokio::test]
async fn capability_decisions_are_hidden_from_other_actors_in_same_tenant() {
    let app = app_router(test_state().await);
    let mut ids = Vec::new();
    for actor in ["user-a", "user-b"] {
        let response = app
            .clone()
            .oneshot(request(
                "POST",
                "/goal-capability-learning/discover",
                Some(actor),
                Some(json!({
                    "goal": {
                        "goal_id": format!("goal-{actor}"),
                        "title": format!("Private goal for {actor}"),
                        "description": "Private capability discovery",
                        "input_parameters": [],
                        "expected_output_format": "JSON records",
                        "constraints": []
                    }
                })),
            ))
            .await
            .expect("discover response");
        assert_eq!(response.status(), StatusCode::OK);
        let payload = response_json(response).await;
        ids.push(
            payload["request_id"]
                .as_str()
                .expect("decision ID")
                .to_string(),
        );
    }

    let own = app
        .clone()
        .oneshot(request(
            "GET",
            &format!("/goal-capability-learning/decisions/{}", ids[0]),
            Some("user-a"),
            None,
        ))
        .await
        .expect("own decision");
    assert_eq!(own.status(), StatusCode::OK);
    let own = response_json(own).await;
    assert_eq!(own["goal_title"], "Private goal for user-a");

    for actor in [Some("user-b"), None] {
        let other = app
            .clone()
            .oneshot(request(
                "GET",
                &format!("/goal-capability-learning/decisions/{}", ids[0]),
                actor,
                None,
            ))
            .await
            .expect("hidden decision");
        assert_eq!(other.status(), StatusCode::NOT_FOUND);
    }

    let list = app
        .clone()
        .oneshot(request(
            "GET",
            "/goal-capability-learning/decisions",
            Some("user-b"),
            None,
        ))
        .await
        .expect("decision list");
    assert_eq!(list.status(), StatusCode::OK);
    let list = response_json(list).await;
    assert_eq!(list["total"], 1);
    assert_eq!(list["decisions"][0]["decision_id"], ids[1]);

    let missing_actor = app
        .oneshot(request(
            "GET",
            "/goal-capability-learning/decisions",
            None,
            None,
        ))
        .await
        .expect("missing actor list");
    assert_eq!(missing_actor.status(), StatusCode::OK);
    let missing_actor = response_json(missing_actor).await;
    assert_eq!(missing_actor["total"], 0);
}

#[tokio::test(flavor = "multi_thread")]
#[serial_test::serial]
async fn hosted_discovery_insertion_precedes_policy_revocation_publication() {
    use super::legacy_routine_authority::{hosted_state, tenant, verified};
    use std::sync::mpsc;

    let (state, policy_dir) = hosted_state().await;
    let tenant = tenant("alice");
    let mut identity = verified("alice", "member");
    state
        .enterprise
        .hosted_policy
        .project(&mut identity)
        .expect("project hosted member");
    let goal = tandem_types::GoalSpec {
        goal_id: "guarded-discovery".to_string(),
        title: "Guarded discovery".to_string(),
        description: "Verify authority stays locked through insertion".to_string(),
        input_parameters: vec![],
        expected_output_format: "JSON".to_string(),
        constraints: vec![],
    };
    let (authorized_tx, authorized_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let pending_state = state.clone();
    let pending = tokio::spawn(async move {
        let authority_state = pending_state.clone();
        let authority_tenant = tenant.clone();
        pending_state
            .discover_goal_capabilities(
                goal,
                format!("{}/{}", tenant.org_id, tenant.workspace_id),
                Some("alice".to_string()),
                move |commit| {
                    authority_state
                        .enterprise
                        .hosted_policy
                        .with_current_policy(|policy| {
                            super::super::require_hosted_permission_under_policy(
                                &authority_tenant,
                                Some(&identity),
                                tandem_types::AccessPermission::HostedUse,
                                policy,
                            )
                            .ok()?;
                            authorized_tx.send(()).expect("signal authorized discovery");
                            release_rx.recv().expect("release authorized discovery");
                            commit()
                        })
                        .ok()
                        .flatten()
                },
            )
            .await
    });
    authorized_rx
        .await
        .expect("discovery acquired policy read lock");
    assert!(
        state
            .enterprise
            .hosted_policy
            .publication_write_blocked_for_test(),
        "snapshot publication must be blocked between authorization and insertion"
    );

    let policy_path = policy_dir.path().join("policy.json");
    let mut policy: Value =
        serde_json::from_slice(&std::fs::read(&policy_path).expect("read hosted policy"))
            .expect("policy JSON");
    policy["policy_version"] = json!(2);
    policy["generated_at"] = json!(
        chrono::DateTime::from_timestamp_millis(crate::now_ms() as i64).expect("current time")
    );
    let alice = policy["users"]
        .as_array_mut()
        .expect("policy users")
        .iter_mut()
        .find(|user| user["id"] == "alice")
        .expect("alice policy entry");
    alice["is_active"] = json!(false);
    std::fs::write(
        &policy_path,
        serde_json::to_vec(&policy).expect("updated policy JSON"),
    )
    .expect("write revoked policy");
    let reload_state = state.clone();
    let (reload_started_tx, reload_started_rx) = tokio::sync::oneshot::channel();
    let reload = tokio::spawn(async move {
        reload_started_tx.send(()).expect("signal reload start");
        reload_state.reload_hosted_policy().await
    });
    reload_started_rx.await.expect("reload started");

    release_tx.send(()).expect("release insertion");
    let response = pending
        .await
        .expect("discovery task")
        .expect("authorized insertion");
    reload
        .await
        .expect("policy reload task")
        .expect("publish revoked policy");
    assert!(state
        .get_discovery_decision(&response.request_id)
        .await
        .is_some());
    assert!(state
        .enterprise
        .hosted_policy
        .authorize_permission(
            Some(&verified("alice", "member")),
            tandem_types::AccessPermission::HostedUse
        )
        .is_err());
}
