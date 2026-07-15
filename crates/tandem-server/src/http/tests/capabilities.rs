// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[tokio::test]
async fn capabilities_resolve_prefers_arcade_when_requested() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
            .method("POST")
            .uri("/capabilities/resolve")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "workflow_id": "wf-pr",
                    "required_capabilities": ["github.create_pull_request"],
                    "provider_preference": ["arcade", "composio"],
                    "available_tools": [
                        {"provider":"composio","tool_name":"mcp.composio.github_create_pull_request","schema":{}},
                        {"provider":"arcade","tool_name":"mcp.arcade.github_create_pull_request","schema":{}}
                    ]
                })
                .to_string(),
            ))
            .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let provider = payload
        .get("resolution")
        .and_then(|v| v.get("resolved"))
        .and_then(|v| v.as_array())
        .and_then(|rows| rows.first())
        .and_then(|row| row.get("provider"))
        .and_then(|v| v.as_str());
    assert_eq!(provider, Some("arcade"));
}

#[tokio::test]
async fn capabilities_resolve_returns_missing_capability_error() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/capabilities/resolve")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_id": "wf-pr",
                "required_capabilities": ["github.create_pull_request"],
                "provider_preference": ["arcade", "composio"],
                "available_tools": [
                    {"provider":"mcp","tool_name":"mcp.github.list_issues","schema":{}}
                ]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.get("code").and_then(|v| v.as_str()),
        Some("missing_capability")
    );
    assert_eq!(
        payload.get("workflow_id").and_then(|v| v.as_str()),
        Some("wf-pr")
    );
}

#[tokio::test]
async fn capabilities_readiness_returns_blocking_issues_when_unbound() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/capabilities/readiness")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_id": "wf-readiness",
                "required_capabilities": ["github.create_pull_request"],
                "provider_preference": ["composio", "arcade", "mcp"],
                "available_tools": [
                    {"provider":"mcp","tool_name":"mcp.github.list_issues","schema":{}}
                ]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let readiness = payload.get("readiness").cloned().unwrap_or(Value::Null);
    assert_eq!(
        readiness.get("runnable").and_then(|v| v.as_bool()),
        Some(false)
    );
    assert!(readiness
        .get("unbound_capabilities")
        .and_then(|v| v.as_array())
        .is_some_and(|rows| !rows.is_empty()));
    assert!(readiness
        .get("blocking_issues")
        .and_then(|v| v.as_array())
        .is_some_and(|rows| !rows.is_empty()));
}

#[tokio::test]
async fn linear_readiness_reports_missing_connector() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/capabilities/readiness")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_id": "wf-linear-missing",
                "required_capabilities": ["linear.get_issue"],
                "provider_preference": ["mcp"]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let states = payload
        .pointer("/readiness/mcp_connector_states")
        .and_then(Value::as_array)
        .expect("connector states");
    assert!(states.iter().any(|state| {
        state.get("provider").and_then(Value::as_str) == Some("linear")
            && state.get("access").and_then(Value::as_str) == Some("missing_connector")
            && state.get("configured").and_then(Value::as_bool) == Some(false)
    }));
}

#[tokio::test]
async fn linear_readiness_distinguishes_read_only_available() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/capabilities/readiness")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_id": "wf-linear-read",
                "required_capabilities": ["linear.get_issue"],
                "provider_preference": ["mcp"],
                "available_tools": [
                    {"provider":"mcp","tool_name":"mcp.app_linear_linear.get_issue","schema":{}}
                ]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let state = payload
        .pointer("/readiness/mcp_connector_states/0")
        .expect("linear connector state");
    assert_eq!(
        state.get("provider").and_then(Value::as_str),
        Some("linear")
    );
    assert_eq!(
        state.get("access").and_then(Value::as_str),
        Some("read_only")
    );
    assert!(state
        .get("read_tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty()));
    assert!(state
        .get("write_tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| tools.is_empty()));
}

#[tokio::test]
async fn linear_readiness_distinguishes_write_capable() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/capabilities/readiness")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_id": "wf-linear-write",
                "required_capabilities": ["linear.update_issue"],
                "provider_preference": ["mcp"],
                "available_tools": [
                    {"provider":"mcp","tool_name":"mcp.app_linear_linear.update_issue","schema":{}}
                ]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let state = payload
        .pointer("/readiness/mcp_connector_states/0")
        .expect("linear connector state");
    assert_eq!(
        state.get("provider").and_then(Value::as_str),
        Some("linear")
    );
    assert_eq!(
        state.get("access").and_then(Value::as_str),
        Some("write_capable")
    );
    assert!(state
        .get("write_tools")
        .and_then(Value::as_array)
        .is_some_and(|tools| !tools.is_empty()));
}
