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
