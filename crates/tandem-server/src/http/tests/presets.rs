// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

#[tokio::test]
async fn presets_index_route_returns_layered_index_shape() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("GET")
        .uri("/presets/index")
        .body(Body::empty())
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let index = payload.get("index").cloned().unwrap_or(Value::Null);
    assert!(index
        .get("skill_modules")
        .and_then(|v| v.as_array())
        .is_some());
    assert!(index
        .get("agent_presets")
        .and_then(|v| v.as_array())
        .is_some());
    assert!(index
        .get("automation_presets")
        .and_then(|v| v.as_array())
        .is_some());
    assert!(index
        .get("pack_presets")
        .and_then(|v| v.as_array())
        .is_some());
    assert!(index
        .get("generated_at_ms")
        .and_then(|v| v.as_u64())
        .is_some());
}

#[tokio::test]
async fn presets_compose_preview_is_deterministic() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/presets/compose/preview")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "base_prompt": "Base",
                "fragments": [
                    {"id":"zeta","phase":"style","content":"Style Z"},
                    {"id":"alpha","phase":"core","content":"Core A"},
                    {"id":"safe","phase":"safety","content":"Do no harm"}
                ]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let ordered = payload
        .get("composition")
        .and_then(|v| v.get("ordered_fragment_ids"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(ordered.len(), 3);
    assert_eq!(ordered[0].as_str(), Some("alpha"));
    assert_eq!(ordered[1].as_str(), Some("zeta"));
    assert_eq!(ordered[2].as_str(), Some("safe"));
    assert!(payload
        .get("composition")
        .and_then(|v| v.get("composition_hash"))
        .and_then(|v| v.as_str())
        .map(|s| !s.is_empty())
        .unwrap_or(false));
}

#[tokio::test]
async fn presets_override_put_and_delete_roundtrip() {
    let state = test_state().await;
    let app = app_router(state);
    let put_req = Request::builder()
        .method("PUT")
        .uri("/presets/overrides/agent_preset/dev_agent")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "content": "id: dev_agent\nversion: 1.0.0\n"
            })
            .to_string(),
        ))
        .expect("request");
    let put_resp = app.clone().oneshot(put_req).await.expect("response");
    assert_eq!(put_resp.status(), StatusCode::OK);
    let put_body = to_bytes(put_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let put_payload: Value = serde_json::from_slice(&put_body).expect("json");
    assert_eq!(
        put_payload.get("saved").and_then(|v| v.as_bool()),
        Some(true)
    );

    let del_req = Request::builder()
        .method("DELETE")
        .uri("/presets/overrides/agent_preset/dev_agent")
        .body(Body::empty())
        .expect("request");
    let del_resp = app.clone().oneshot(del_req).await.expect("response");
    assert_eq!(del_resp.status(), StatusCode::OK);
    let del_body = to_bytes(del_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let del_payload: Value = serde_json::from_slice(&del_body).expect("json");
    assert_eq!(
        del_payload.get("removed").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[tokio::test]
async fn presets_fork_copies_source_into_overrides() {
    let state = test_state().await;
    let app = app_router(state);
    let seed_req = Request::builder()
        .method("PUT")
        .uri("/presets/overrides/skill_module/source_seed")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "content": "id: source_seed\nversion: 1.0.0\n"
            })
            .to_string(),
        ))
        .expect("request");
    let seed_resp = app.clone().oneshot(seed_req).await.expect("response");
    assert_eq!(seed_resp.status(), StatusCode::OK);
    let seed_body = to_bytes(seed_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let seed_payload: Value = serde_json::from_slice(&seed_body).expect("json");
    let source_path = seed_payload
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    assert!(!source_path.is_empty());

    let fork_req = Request::builder()
        .method("POST")
        .uri("/presets/fork")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "skill_module",
                "source_path": source_path,
                "target_id": "forked_skill"
            })
            .to_string(),
        ))
        .expect("request");
    let fork_resp = app.clone().oneshot(fork_req).await.expect("response");
    assert_eq!(fork_resp.status(), StatusCode::OK);
    let fork_body = to_bytes(fork_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let fork_payload: Value = serde_json::from_slice(&fork_body).expect("json");
    assert_eq!(
        fork_payload.get("forked").and_then(|v| v.as_bool()),
        Some(true)
    );
    let forked_path = fork_payload
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(forked_path.ends_with("forked_skill.yaml"));
}

#[tokio::test]
async fn presets_capability_summary_merges_agent_and_task_caps() {
    let state = test_state().await;
    let app = app_router(state);
    let req = Request::builder()
        .method("POST")
        .uri("/presets/capability_summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "agent": {
                    "required": ["github.create_pull_request"],
                    "optional": ["slack.post_message"]
                },
                "tasks": [
                    {"required": ["slack.post_message"], "optional": []},
                    {"required": [], "optional": ["jira.create_issue"]}
                ]
            })
            .to_string(),
        ))
        .expect("request");
    let resp = app.oneshot(req).await.expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    let required = payload
        .get("summary")
        .and_then(|v| v.get("automation"))
        .and_then(|v| v.get("required"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let optional = payload
        .get("summary")
        .and_then(|v| v.get("automation"))
        .and_then(|v| v.get("optional"))
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    assert_eq!(required.len(), 2);
    assert_eq!(optional.len(), 1);
    assert_eq!(
        payload
            .get("summary")
            .and_then(|v| v.get("totals"))
            .and_then(|v| v.get("task_count"))
            .and_then(|v| v.as_u64()),
        Some(2)
    );
}

#[tokio::test]
async fn presets_export_overrides_returns_zip_payload() {
    let state = test_state().await;
    let app = app_router(state);
    let seed_req = Request::builder()
        .method("PUT")
        .uri("/presets/overrides/skill_module/export_seed")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "content": "id: export_seed\nversion: 1.0.0\n"
            })
            .to_string(),
        ))
        .expect("request");
    let seed_resp = app.clone().oneshot(seed_req).await.expect("response");
    assert_eq!(seed_resp.status(), StatusCode::OK);

    let export_req = Request::builder()
        .method("POST")
        .uri("/presets/export_overrides")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "name": "exported-overrides",
                "version": "1.0.0"
            })
            .to_string(),
        ))
        .expect("request");
    let export_resp = app.clone().oneshot(export_req).await.expect("response");
    assert_eq!(export_resp.status(), StatusCode::OK);
    let export_body = to_bytes(export_resp.into_body(), usize::MAX)
        .await
        .expect("body");
    let export_payload: Value = serde_json::from_slice(&export_body).expect("json");
    assert!(export_payload
        .get("exported")
        .and_then(|v| v.get("path"))
        .and_then(|v| v.as_str())
        .map(|s| s.ends_with(".zip"))
        .unwrap_or(false));
    assert!(
        export_payload
            .get("exported")
            .and_then(|v| v.get("bytes"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0)
            > 0
    );
}
