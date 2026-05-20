use super::*;
use tandem_types::TenantContext;
use tokio_stream::StreamExt as _;

fn tenant_request(
    method: &str,
    uri: impl Into<String>,
    org_id: &str,
    workspace_id: &str,
    actor_id: &str,
    body: Option<Value>,
) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri.into())
        .header("x-tandem-org-id", org_id)
        .header("x-tandem-workspace-id", workspace_id)
        .header("x-tandem-actor-id", actor_id);
    let body = match body {
        Some(value) => {
            builder = builder.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    builder.body(body).expect("tenant request")
}

fn automation_v2_create_payload(automation_id: &str, name: &str) -> Value {
    json!({
        "automation_id": automation_id,
        "name": name,
        "status": "active",
        "schedule": {
            "type": "manual",
            "timezone": "UTC",
            "misfire_policy": { "type": "skip" }
        },
        "agents": [
            {
                "agent_id": "agent-a",
                "display_name": "Agent A",
                "skills": [],
                "tool_policy": { "allowlist": ["read"], "denylist": [] },
                "mcp_policy": { "allowed_servers": [] }
            }
        ],
        "flow": {
            "nodes": [
                {
                    "node_id": "node-1",
                    "agent_id": "agent-a",
                    "objective": "Check tenant routing",
                    "depends_on": []
                }
            ]
        },
        "execution": { "max_parallel_agents": 1 }
    })
}

fn explicit_tenant(org_id: &str, workspace_id: &str, actor_id: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(
        org_id.to_string(),
        workspace_id.to_string(),
        Some(actor_id.to_string()),
        "test-suite".to_string(),
    )
}

#[tokio::test]
async fn routines_create_run_now_and_history_roundtrip() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-1",
                "name": "Daily digest",
                "schedule": { "interval_seconds": { "seconds": 60 } },
                "entrypoint": "mission.default",
                "creator_type": "user",
                "creator_id": "u-1"
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/routines/routine-1/run_now")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_count": 2,
                "reason": "manual smoke check"
            })
            .to_string(),
        ))
        .expect("run_now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("run_now response");
    assert_eq!(run_now_resp.status(), StatusCode::OK);

    let history_req = Request::builder()
        .method("GET")
        .uri("/routines/routine-1/history?limit=10")
        .body(Body::empty())
        .expect("history request");
    let history_resp = app
        .clone()
        .oneshot(history_req)
        .await
        .expect("history response");
    assert_eq!(history_resp.status(), StatusCode::OK);
    let history_body = to_bytes(history_resp.into_body(), usize::MAX)
        .await
        .expect("history body");
    let history_payload: Value = serde_json::from_slice(&history_body).expect("history json");
    assert_eq!(
        history_payload.get("count").and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        history_payload
            .get("events")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("run_count"))
            .and_then(|v| v.as_u64()),
        Some(2)
    );
}

#[tokio::test]
async fn routines_patch_can_pause_routine() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-2",
                "name": "Research routine",
                "schedule": { "interval_seconds": { "seconds": 120 } },
                "entrypoint": "mission.default"
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/routines/routine-2")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "status": "paused"
            })
            .to_string(),
        ))
        .expect("patch request");
    let patch_resp = app
        .clone()
        .oneshot(patch_req)
        .await
        .expect("patch response");
    assert_eq!(patch_resp.status(), StatusCode::OK);
    let patch_body = to_bytes(patch_resp.into_body(), usize::MAX)
        .await
        .expect("patch body");
    let patch_payload: Value = serde_json::from_slice(&patch_body).expect("patch json");
    assert_eq!(
        patch_payload
            .get("routine")
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str()),
        Some("paused")
    );
}

#[tokio::test]
async fn routines_allowlist_is_persisted_and_copied_to_runs() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-tools",
                "name": "Tool-scoped routine",
                "schedule": { "interval_seconds": { "seconds": 90 } },
                "entrypoint": "mission.default",
                "allowed_tools": ["  mcp.arcade.search  ", "read", "read", ""],
                "output_targets": ["  s3://reports/daily.json  ", "s3://reports/daily.json", ""]
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    assert_eq!(
        create_payload
            .get("routine")
            .and_then(|v| v.get("allowed_tools"))
            .and_then(|v| v.as_array())
            .map(|rows| rows
                .iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()),
        Some(vec!["mcp.arcade.search".to_string(), "read".to_string()])
    );
    assert_eq!(
        create_payload
            .get("routine")
            .and_then(|v| v.get("output_targets"))
            .and_then(|v| v.as_array())
            .map(|rows| rows
                .iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()),
        Some(vec!["s3://reports/daily.json".to_string()])
    );

    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/routines/routine-tools")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "allowed_tools": ["mcp.arcade.send_email", "bash"],
                "output_targets": ["https://storage.example/run/output.md"]
            })
            .to_string(),
        ))
        .expect("patch request");
    let patch_resp = app
        .clone()
        .oneshot(patch_req)
        .await
        .expect("patch response");
    assert_eq!(patch_resp.status(), StatusCode::OK);
    let patch_body = to_bytes(patch_resp.into_body(), usize::MAX)
        .await
        .expect("patch body");
    let patch_payload: Value = serde_json::from_slice(&patch_body).expect("patch json");
    assert_eq!(
        patch_payload
            .get("routine")
            .and_then(|v| v.get("allowed_tools"))
            .and_then(|v| v.as_array())
            .map(|rows| rows
                .iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()),
        Some(vec![
            "mcp.arcade.send_email".to_string(),
            "bash".to_string()
        ])
    );
    assert_eq!(
        patch_payload
            .get("routine")
            .and_then(|v| v.get("output_targets"))
            .and_then(|v| v.as_array())
            .map(|rows| rows
                .iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()),
        Some(vec!["https://storage.example/run/output.md".to_string()])
    );

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/routines/routine-tools/run_now")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("run_now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("run_now response");
    assert_eq!(run_now_resp.status(), StatusCode::OK);
    let run_now_body = to_bytes(run_now_resp.into_body(), usize::MAX)
        .await
        .expect("run_now body");
    let run_now_payload: Value = serde_json::from_slice(&run_now_body).expect("run_now json");
    let run_id = run_now_payload
        .get("runID")
        .and_then(|v| v.as_str())
        .expect("runID");
    let context_run_id = run_now_payload
        .get("contextRunID")
        .and_then(|v| v.as_str())
        .expect("context run id");
    assert_eq!(
        run_now_payload
            .get("linked_context_run_id")
            .and_then(|v| v.as_str()),
        Some(context_run_id)
    );

    let run_get_req = Request::builder()
        .method("GET")
        .uri(format!("/routines/runs/{run_id}"))
        .body(Body::empty())
        .expect("run get request");
    let run_get_resp = app
        .clone()
        .oneshot(run_get_req)
        .await
        .expect("run get response");
    assert_eq!(run_get_resp.status(), StatusCode::OK);
    let run_get_body = to_bytes(run_get_resp.into_body(), usize::MAX)
        .await
        .expect("run get body");
    let run_get_payload: Value = serde_json::from_slice(&run_get_body).expect("run get json");
    assert_eq!(
        run_get_payload.get("contextRunID").and_then(|v| v.as_str()),
        Some(context_run_id)
    );
    assert_eq!(
        run_get_payload
            .get("run")
            .and_then(|v| v.get("contextRunID"))
            .and_then(|v| v.as_str()),
        Some(context_run_id)
    );
    assert_eq!(
        run_get_payload
            .get("run")
            .and_then(|v| v.get("allowed_tools"))
            .and_then(|v| v.as_array())
            .map(|rows| rows
                .iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()),
        Some(vec![
            "mcp.arcade.send_email".to_string(),
            "bash".to_string()
        ])
    );
    assert_eq!(
        run_get_payload
            .get("run")
            .and_then(|v| v.get("output_targets"))
            .and_then(|v| v.as_array())
            .map(|rows| rows
                .iter()
                .filter_map(|v| v.as_str().map(ToString::to_string))
                .collect::<Vec<_>>()),
        Some(vec!["https://storage.example/run/output.md".to_string()])
    );

    let context_run_req = Request::builder()
        .method("GET")
        .uri(format!("/context/runs/{context_run_id}"))
        .body(Body::empty())
        .expect("context run request");
    let context_run_resp = app
        .clone()
        .oneshot(context_run_req)
        .await
        .expect("context run response");
    assert_eq!(context_run_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn routines_runs_all_can_filter_by_routine() {
    let state = test_state().await;
    let app = app_router(state.clone());

    for routine_id in ["routine-run-a", "routine-run-b"] {
        let create_req = Request::builder()
            .method("POST")
            .uri("/routines")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "routine_id": routine_id,
                    "name": format!("Routine {routine_id}"),
                    "schedule": { "interval_seconds": { "seconds": 60 } },
                    "entrypoint": "mission.default",
                })
                .to_string(),
            ))
            .expect("create request");
        let create_resp = app
            .clone()
            .oneshot(create_req)
            .await
            .expect("create response");
        assert_eq!(create_resp.status(), StatusCode::OK);

        let run_now_req = Request::builder()
            .method("POST")
            .uri(format!("/routines/{routine_id}/run_now"))
            .header("content-type", "application/json")
            .body(Body::from(json!({}).to_string()))
            .expect("run_now request");
        let run_now_resp = app
            .clone()
            .oneshot(run_now_req)
            .await
            .expect("run_now response");
        assert_eq!(run_now_resp.status(), StatusCode::OK);
    }

    let all_req = Request::builder()
        .method("GET")
        .uri("/routines/runs?limit=10")
        .body(Body::empty())
        .expect("runs all request");
    let all_resp = app
        .clone()
        .oneshot(all_req)
        .await
        .expect("runs all response");
    assert_eq!(all_resp.status(), StatusCode::OK);
    let all_body = to_bytes(all_resp.into_body(), usize::MAX)
        .await
        .expect("runs all body");
    let all_payload: Value = serde_json::from_slice(&all_body).expect("runs all json");
    assert!(all_payload
        .get("count")
        .and_then(|v| v.as_u64())
        .is_some_and(|count| count >= 2));

    let filtered_req = Request::builder()
        .method("GET")
        .uri("/routines/runs?routine_id=routine-run-b&limit=10")
        .body(Body::empty())
        .expect("runs filtered request");
    let filtered_resp = app
        .clone()
        .oneshot(filtered_req)
        .await
        .expect("runs filtered response");
    assert_eq!(filtered_resp.status(), StatusCode::OK);
    let filtered_body = to_bytes(filtered_resp.into_body(), usize::MAX)
        .await
        .expect("runs filtered body");
    let filtered_payload: Value =
        serde_json::from_slice(&filtered_body).expect("runs filtered json");
    assert!(filtered_payload
        .get("count")
        .and_then(|v| v.as_u64())
        .is_some_and(|count| count >= 1));
    let all_match_routine = filtered_payload
        .get("runs")
        .and_then(|v| v.as_array())
        .map(|rows| {
            rows.iter().all(|row| {
                row.get("routine_id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| id == "routine-run-b")
            })
        })
        .unwrap_or(false);
    assert!(all_match_routine);
    assert!(filtered_payload
        .get("runs")
        .and_then(|v| v.as_array())
        .is_some_and(|rows| rows.iter().all(|row| {
            row.get("contextRunID")
                .and_then(|v| v.as_str())
                .is_some_and(|id| !id.is_empty())
                && row
                    .get("linked_context_run_id")
                    .and_then(|v| v.as_str())
                    .is_some_and(|id| !id.is_empty())
        })));
}

#[tokio::test]
async fn routine_run_operator_routes_expose_context_run_links() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-ops-links",
                "name": "Routine Operator Links",
                "schedule": { "interval_seconds": { "seconds": 60 } },
                "entrypoint": "mission.default",
                "requires_approval": true
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let routine = state
        .get_routine("routine-ops-links")
        .await
        .expect("stored routine");
    let approval_run = state
        .create_routine_run(
            &routine,
            "manual",
            1,
            crate::RoutineRunStatus::PendingApproval,
            None,
        )
        .await;
    crate::http::context_runs::sync_routine_run_blackboard(&state, &approval_run)
        .await
        .expect("sync approval context");
    let approval_run_id = approval_run.run_id.clone();
    let approval_context_run_id =
        crate::http::context_runs::routine_context_run_id(&approval_run_id);

    let approve_req = Request::builder()
        .method("POST")
        .uri(format!("/routines/runs/{approval_run_id}/approve"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "reason": "approved for execution" }).to_string(),
        ))
        .expect("approve request");
    let approve_resp = app
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approve response");
    assert_eq!(approve_resp.status(), StatusCode::OK);
    let approve_body = to_bytes(approve_resp.into_body(), usize::MAX)
        .await
        .expect("approve body");
    let approve_payload: Value = serde_json::from_slice(&approve_body).expect("approve json");
    assert_eq!(
        approve_payload.get("contextRunID").and_then(Value::as_str),
        Some(approval_context_run_id.as_str())
    );
    assert_eq!(
        approve_payload
            .get("linked_context_run_id")
            .and_then(Value::as_str),
        Some(approval_context_run_id.as_str())
    );
    assert_eq!(
        approve_payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(approval_context_run_id.as_str())
    );
    let approve_context_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{approval_context_run_id}"))
                .body(Body::empty())
                .expect("approve context request"),
        )
        .await
        .expect("approve context response");
    assert_eq!(approve_context_resp.status(), StatusCode::OK);

    let running = state
        .create_routine_run(
            &routine,
            "manual",
            2,
            crate::RoutineRunStatus::Running,
            None,
        )
        .await;
    crate::http::context_runs::sync_routine_run_blackboard(&state, &running)
        .await
        .expect("sync running context");
    let running_context_run_id = crate::http::context_runs::routine_context_run_id(&running.run_id);

    let pause_req = Request::builder()
        .method("POST")
        .uri(format!("/routines/runs/{}/pause", running.run_id))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "reason": "pause for inspection" }).to_string(),
        ))
        .expect("pause request");
    let pause_resp = app
        .clone()
        .oneshot(pause_req)
        .await
        .expect("pause response");
    assert_eq!(pause_resp.status(), StatusCode::OK);
    let pause_body = to_bytes(pause_resp.into_body(), usize::MAX)
        .await
        .expect("pause body");
    let pause_payload: Value = serde_json::from_slice(&pause_body).expect("pause json");
    assert_eq!(
        pause_payload.get("contextRunID").and_then(Value::as_str),
        Some(running_context_run_id.as_str())
    );
    assert_eq!(
        pause_payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(running_context_run_id.as_str())
    );
    let pause_context_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{running_context_run_id}"))
                .body(Body::empty())
                .expect("pause context request"),
        )
        .await
        .expect("pause context response");
    assert_eq!(pause_context_resp.status(), StatusCode::OK);

    let resume_req = Request::builder()
        .method("POST")
        .uri(format!("/routines/runs/{}/resume", running.run_id))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "reason": "resume after inspection" }).to_string(),
        ))
        .expect("resume request");
    let resume_resp = app
        .clone()
        .oneshot(resume_req)
        .await
        .expect("resume response");
    assert_eq!(resume_resp.status(), StatusCode::OK);
    let resume_body = to_bytes(resume_resp.into_body(), usize::MAX)
        .await
        .expect("resume body");
    let resume_payload: Value = serde_json::from_slice(&resume_body).expect("resume json");
    assert_eq!(
        resume_payload.get("contextRunID").and_then(Value::as_str),
        Some(running_context_run_id.as_str())
    );
    assert_eq!(
        resume_payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(running_context_run_id.as_str())
    );
    let resume_context_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{running_context_run_id}"))
                .body(Body::empty())
                .expect("resume context request"),
        )
        .await
        .expect("resume context response");
    assert_eq!(resume_context_resp.status(), StatusCode::OK);

    let add_artifact_req = Request::builder()
        .method("POST")
        .uri(format!("/routines/runs/{}/artifacts", running.run_id))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "uri": "file://reports/routine-ops-links.md",
                "kind": "report",
                "label": "Routine Report"
            })
            .to_string(),
        ))
        .expect("add artifact request");
    let add_artifact_resp = app
        .clone()
        .oneshot(add_artifact_req)
        .await
        .expect("add artifact response");
    assert_eq!(add_artifact_resp.status(), StatusCode::OK);
    let add_artifact_body = to_bytes(add_artifact_resp.into_body(), usize::MAX)
        .await
        .expect("add artifact body");
    let add_artifact_payload: Value =
        serde_json::from_slice(&add_artifact_body).expect("add artifact json");
    assert_eq!(
        add_artifact_payload
            .get("contextRunID")
            .and_then(Value::as_str),
        Some(running_context_run_id.as_str())
    );
    assert_eq!(
        add_artifact_payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(running_context_run_id.as_str())
    );
    let artifact_blackboard_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{running_context_run_id}/blackboard"))
                .body(Body::empty())
                .expect("artifact blackboard request"),
        )
        .await
        .expect("artifact blackboard response");
    assert_eq!(artifact_blackboard_resp.status(), StatusCode::OK);
    let artifact_blackboard_body = to_bytes(artifact_blackboard_resp.into_body(), usize::MAX)
        .await
        .expect("artifact blackboard body");
    let artifact_blackboard_payload: Value =
        serde_json::from_slice(&artifact_blackboard_body).expect("artifact blackboard json");
    assert!(artifact_blackboard_payload
        .get("blackboard")
        .and_then(|value| value.get("artifacts"))
        .and_then(Value::as_array)
        .is_some_and(|rows| rows.iter().any(|row| {
            row.get("path").and_then(Value::as_str) == Some("file://reports/routine-ops-links.md")
                && row.get("artifact_type").and_then(Value::as_str) == Some("report")
        })));

    let list_artifacts_req = Request::builder()
        .method("GET")
        .uri(format!("/routines/runs/{}/artifacts", running.run_id))
        .body(Body::empty())
        .expect("list artifacts request");
    let list_artifacts_resp = app
        .clone()
        .oneshot(list_artifacts_req)
        .await
        .expect("list artifacts response");
    assert_eq!(list_artifacts_resp.status(), StatusCode::OK);
    let list_artifacts_body = to_bytes(list_artifacts_resp.into_body(), usize::MAX)
        .await
        .expect("list artifacts body");
    let list_artifacts_payload: Value =
        serde_json::from_slice(&list_artifacts_body).expect("list artifacts json");
    assert_eq!(
        list_artifacts_payload
            .get("contextRunID")
            .and_then(Value::as_str),
        Some(running_context_run_id.as_str())
    );
    assert_eq!(
        list_artifacts_payload
            .get("linked_context_run_id")
            .and_then(Value::as_str),
        Some(running_context_run_id.as_str())
    );
    assert_eq!(
        list_artifacts_payload.get("count").and_then(Value::as_u64),
        Some(1)
    );
}

#[tokio::test]
async fn automations_create_requires_mission_objective() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-empty-objective",
                "name": "Automation without objective",
                "schedule": { "interval_seconds": { "seconds": 300 } },
                "mission": {
                    "objective": "   "
                }
            })
            .to_string(),
        ))
        .expect("automation create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("automation create response");
    assert_eq!(create_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn automations_create_rejects_invalid_mode() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-invalid-mode",
                "name": "Automation invalid mode",
                "schedule": { "interval_seconds": { "seconds": 300 } },
                "mode": "swarm-ish",
                "mission": {
                    "objective": "Execute a mission with invalid mode."
                }
            })
            .to_string(),
        ))
        .expect("automation create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("automation create response");
    assert_eq!(create_resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn automations_create_and_run_now_roundtrip() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-digest",
                "name": "Daily Digest Automation",
                "schedule": { "interval_seconds": { "seconds": 600 } },
                "mission": {
                    "objective": "Generate a daily digest with clear sources.",
                    "success_criteria": ["Contains source URLs", "Writes one artifact"]
                },
                "policy": {
                    "tool": {
                        "run_allowlist": ["read", "websearch", "webfetch", "write"],
                        "external_integrations_allowed": true
                    },
                    "approval": {
                        "requires_approval": true
                    }
                }
            })
            .to_string(),
        ))
        .expect("automation create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("automation create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/automations/auto-digest/run_now")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("automation run_now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("automation run_now response");
    assert_eq!(run_now_resp.status(), StatusCode::OK);
    let run_now_body = to_bytes(run_now_resp.into_body(), usize::MAX)
        .await
        .expect("automation run_now body");
    let run_now_payload: Value =
        serde_json::from_slice(&run_now_body).expect("automation run_now json");
    assert_eq!(
        run_now_payload
            .get("run")
            .and_then(|v| v.get("automation_id"))
            .and_then(|v| v.as_str()),
        Some("auto-digest")
    );
    let context_run_id = run_now_payload
        .get("contextRunID")
        .and_then(|v| v.as_str())
        .expect("automation context run id");
    assert_eq!(
        run_now_payload
            .get("linked_context_run_id")
            .and_then(|v| v.as_str()),
        Some(context_run_id)
    );
    assert_eq!(
        run_now_payload
            .get("run")
            .and_then(|v| v.get("contextRunID"))
            .and_then(|v| v.as_str()),
        Some(context_run_id)
    );
    assert_eq!(
        run_now_payload
            .get("run")
            .and_then(|v| v.get("mission_snapshot"))
            .and_then(|v| v.get("objective"))
            .and_then(|v| v.as_str()),
        Some("Generate a daily digest with clear sources.")
    );
    let run_id = run_now_payload
        .get("run")
        .and_then(|v| v.get("run_id"))
        .and_then(|v| v.as_str())
        .expect("automation run_id in run_now response")
        .to_string();

    let run_get_req = Request::builder()
        .method("GET")
        .uri(format!("/automations/runs/{run_id}"))
        .body(Body::empty())
        .expect("automation run get request");
    let run_get_resp = app
        .clone()
        .oneshot(run_get_req)
        .await
        .expect("automation run get response");
    assert_eq!(run_get_resp.status(), StatusCode::OK);
    let run_get_body = to_bytes(run_get_resp.into_body(), usize::MAX)
        .await
        .expect("automation run get body");
    let run_get_payload: Value =
        serde_json::from_slice(&run_get_body).expect("automation run get json");
    assert_eq!(
        run_get_payload.get("contextRunID").and_then(|v| v.as_str()),
        Some(context_run_id)
    );
    assert_eq!(
        run_get_payload
            .get("run")
            .and_then(|v| v.get("contextRunID"))
            .and_then(|v| v.as_str()),
        Some(context_run_id)
    );

    let context_run_req = Request::builder()
        .method("GET")
        .uri(format!("/context/runs/{context_run_id}"))
        .body(Body::empty())
        .expect("automation context run request");
    let context_run_resp = app
        .clone()
        .oneshot(context_run_req)
        .await
        .expect("automation context run response");
    assert_eq!(context_run_resp.status(), StatusCode::OK);

    let history_req = Request::builder()
        .method("GET")
        .uri("/automations/auto-digest/history?limit=5")
        .body(Body::empty())
        .expect("automation history request");
    let history_resp = app
        .clone()
        .oneshot(history_req)
        .await
        .expect("automation history response");
    assert_eq!(history_resp.status(), StatusCode::OK);
    let history_body = to_bytes(history_resp.into_body(), usize::MAX)
        .await
        .expect("automation history body");
    let history_payload: Value =
        serde_json::from_slice(&history_body).expect("automation history json");
    assert_eq!(
        history_payload.get("automationID").and_then(|v| v.as_str()),
        Some("auto-digest")
    );

    let add_artifact_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/runs/{run_id}/artifacts"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "uri": "file://reports/daily-digest.md",
                "kind": "report",
                "label": "Daily Digest",
            })
            .to_string(),
        ))
        .expect("automation add artifact request");
    let add_artifact_resp = app
        .clone()
        .oneshot(add_artifact_req)
        .await
        .expect("automation add artifact response");
    assert_eq!(add_artifact_resp.status(), StatusCode::OK);
    let add_artifact_body = to_bytes(add_artifact_resp.into_body(), usize::MAX)
        .await
        .expect("automation add artifact body");
    let add_artifact_payload: Value =
        serde_json::from_slice(&add_artifact_body).expect("automation add artifact json");
    assert_eq!(
        add_artifact_payload
            .get("contextRunID")
            .and_then(|v| v.as_str()),
        Some(context_run_id)
    );
    assert_eq!(
        add_artifact_payload
            .get("linked_context_run_id")
            .and_then(|v| v.as_str()),
        Some(context_run_id)
    );
    assert_eq!(
        add_artifact_payload
            .get("run")
            .and_then(|v| v.get("contextRunID"))
            .and_then(|v| v.as_str()),
        Some(context_run_id)
    );

    let list_artifacts_req = Request::builder()
        .method("GET")
        .uri(format!("/automations/runs/{run_id}/artifacts"))
        .body(Body::empty())
        .expect("automation list artifacts request");
    let list_artifacts_resp = app
        .clone()
        .oneshot(list_artifacts_req)
        .await
        .expect("automation list artifacts response");
    assert_eq!(list_artifacts_resp.status(), StatusCode::OK);
    let list_artifacts_body = to_bytes(list_artifacts_resp.into_body(), usize::MAX)
        .await
        .expect("automation list artifacts body");
    let list_artifacts_payload: Value =
        serde_json::from_slice(&list_artifacts_body).expect("automation list artifacts json");
    assert_eq!(
        list_artifacts_payload
            .get("automationRunID")
            .and_then(|v| v.as_str()),
        Some(run_id.as_str())
    );
    assert_eq!(
        list_artifacts_payload
            .get("contextRunID")
            .and_then(|v| v.as_str()),
        Some(context_run_id)
    );
    assert_eq!(
        list_artifacts_payload
            .get("linked_context_run_id")
            .and_then(|v| v.as_str()),
        Some(context_run_id)
    );
    assert!(list_artifacts_payload
        .get("count")
        .and_then(|v| v.as_u64())
        .is_some_and(|count| count >= 1));

    let patch_req = Request::builder()
        .method("PATCH")
        .uri("/automations/auto-digest")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "mode": "ORCHESTRATED"
            })
            .to_string(),
        ))
        .expect("automation patch request");
    let patch_resp = app
        .clone()
        .oneshot(patch_req)
        .await
        .expect("automation patch response");
    assert_eq!(patch_resp.status(), StatusCode::OK);
    let patch_body = to_bytes(patch_resp.into_body(), usize::MAX)
        .await
        .expect("automation patch body");
    let patch_payload: Value = serde_json::from_slice(&patch_body).expect("automation patch json");
    assert_eq!(
        patch_payload
            .get("automation")
            .and_then(|v| v.get("mode"))
            .and_then(|v| v.as_str()),
        Some("orchestrated")
    );
}

#[tokio::test]
async fn tenant_a_cannot_access_tenant_b_automation_v2_routes() {
    let state = test_state().await;
    let app = app_router(state.clone());

    for (automation_id, name, org, workspace, actor) in [
        (
            "tenant-a-auto",
            "Tenant A Automation",
            "org-a",
            "workspace-a",
            "user-a",
        ),
        (
            "tenant-b-auto",
            "Tenant B Automation",
            "org-b",
            "workspace-b",
            "user-b",
        ),
    ] {
        let create_resp = app
            .clone()
            .oneshot(tenant_request(
                "POST",
                "/automations/v2",
                org,
                workspace,
                actor,
                Some(automation_v2_create_payload(automation_id, name)),
            ))
            .await
            .expect("automation create response");
        assert_eq!(create_resp.status(), StatusCode::OK);
    }

    let list_resp = app
        .clone()
        .oneshot(tenant_request(
            "GET",
            "/automations/v2",
            "org-a",
            "workspace-a",
            "user-a",
            None,
        ))
        .await
        .expect("automation list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("list json");
    let automation_ids = list_payload
        .get("automations")
        .and_then(Value::as_array)
        .expect("automations array")
        .iter()
        .filter_map(|row| row.get("automation_id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(automation_ids.contains(&"tenant-a-auto"));
    assert!(!automation_ids.contains(&"tenant-b-auto"));

    let summary_resp = app
        .clone()
        .oneshot(tenant_request(
            "GET",
            "/automations/v2?view=summary",
            "org-a",
            "workspace-a",
            "user-a",
            None,
        ))
        .await
        .expect("automation summary list response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_body = to_bytes(summary_resp.into_body(), usize::MAX)
        .await
        .expect("summary list body");
    let summary_payload: Value = serde_json::from_slice(&summary_body).expect("summary list json");
    assert_eq!(
        summary_payload.get("view").and_then(Value::as_str),
        Some("summary")
    );
    let summary_row = summary_payload
        .get("automations")
        .and_then(Value::as_array)
        .expect("summary automations array")
        .iter()
        .find(|row| row.get("automation_id").and_then(Value::as_str) == Some("tenant-a-auto"))
        .expect("tenant-a summary row");
    assert_eq!(
        summary_row.get("node_count").and_then(Value::as_u64),
        Some(1)
    );
    assert!(summary_row.get("flow").is_none());
    assert!(summary_row.get("agents").is_none());

    for (method, uri, body) in [
        ("GET", "/automations/v2/tenant-b-auto", None),
        (
            "PATCH",
            "/automations/v2/tenant-b-auto",
            Some(json!({"name": "cross-tenant rename"})),
        ),
        ("DELETE", "/automations/v2/tenant-b-auto", None),
        (
            "POST",
            "/automations/v2/tenant-b-auto/run_now",
            Some(json!({})),
        ),
        (
            "POST",
            "/automations/v2/tenant-b-auto/pause",
            Some(json!({"reason": "cross tenant"})),
        ),
        ("POST", "/automations/v2/tenant-b-auto/resume", None),
        ("GET", "/automations/v2/tenant-b-auto/handoffs", None),
        ("GET", "/automations/v2/tenant-b-auto/runs", None),
    ] {
        let resp = app
            .clone()
            .oneshot(tenant_request(
                method,
                uri,
                "org-a",
                "workspace-a",
                "user-a",
                body,
            ))
            .await
            .expect("cross-tenant automation response");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{method} {uri}");
    }

    let run_now_resp = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/automations/v2/tenant-b-auto/run_now",
            "org-b",
            "workspace-b",
            "user-b",
            Some(json!({})),
        ))
        .await
        .expect("tenant b run_now response");
    assert_eq!(run_now_resp.status(), StatusCode::OK);
    let run_now_body = to_bytes(run_now_resp.into_body(), usize::MAX)
        .await
        .expect("run_now body");
    let run_now_payload: Value = serde_json::from_slice(&run_now_body).expect("run_now json");
    let run_id = run_now_payload
        .get("run")
        .and_then(|run| run.get("run_id"))
        .and_then(Value::as_str)
        .expect("run id")
        .to_string();
    assert_eq!(
        run_now_payload
            .get("run")
            .and_then(|run| run.get("tenant_context"))
            .and_then(|tenant| tenant.get("org_id"))
            .and_then(Value::as_str),
        Some("org-b")
    );

    let runs_all_resp = app
        .clone()
        .oneshot(tenant_request(
            "GET",
            "/automations/v2/runs?limit=10",
            "org-a",
            "workspace-a",
            "user-a",
            None,
        ))
        .await
        .expect("runs all response");
    assert_eq!(runs_all_resp.status(), StatusCode::OK);
    let runs_all_body = to_bytes(runs_all_resp.into_body(), usize::MAX)
        .await
        .expect("runs all body");
    let runs_all_payload: Value = serde_json::from_slice(&runs_all_body).expect("runs all json");
    let visible_run_ids = runs_all_payload
        .get("runs")
        .and_then(Value::as_array)
        .expect("runs array")
        .iter()
        .filter_map(|row| row.get("run_id").and_then(Value::as_str))
        .collect::<Vec<_>>();
    assert!(!visible_run_ids.contains(&run_id.as_str()));

    for (method, uri, body) in [
        ("GET", format!("/automations/v2/runs/{run_id}"), None),
        (
            "GET",
            format!("/automations/v2/runs/{run_id}/tasks/node-1/reset_preview"),
            None,
        ),
        (
            "POST",
            format!("/automations/v2/runs/{run_id}/tasks/node-1/retry"),
            Some(json!({"reason": "cross tenant"})),
        ),
        (
            "POST",
            format!("/automations/v2/runs/{run_id}/tasks/node-1/requeue"),
            Some(json!({"reason": "cross tenant"})),
        ),
        (
            "POST",
            format!("/automations/v2/runs/{run_id}/tasks/node-1/continue"),
            Some(json!({"reason": "cross tenant"})),
        ),
        (
            "PATCH",
            format!("/automations/v2/runs/{run_id}/tasks/node-1/disposition"),
            Some(json!({"disposition": "accepted"})),
        ),
        (
            "POST",
            format!("/automations/v2/runs/{run_id}/backlog/tasks/backlog-1/claim"),
            Some(json!({"agent_id": "agent-a"})),
        ),
        (
            "POST",
            format!("/automations/v2/runs/{run_id}/backlog/tasks/backlog-1/requeue"),
            Some(json!({"reason": "cross tenant"})),
        ),
        (
            "POST",
            format!("/automations/v2/runs/{run_id}/pause"),
            Some(json!({"reason": "cross tenant"})),
        ),
        (
            "POST",
            format!("/automations/v2/runs/{run_id}/resume"),
            Some(json!({"reason": "cross tenant"})),
        ),
        (
            "POST",
            format!("/automations/v2/runs/{run_id}/cancel"),
            Some(json!({"reason": "cross tenant"})),
        ),
        (
            "POST",
            format!("/automations/v2/runs/{run_id}/recover"),
            Some(json!({"reason": "cross tenant"})),
        ),
        (
            "POST",
            format!("/automations/v2/runs/{run_id}/repair"),
            Some(json!({"node_id": "node-1", "reason": "cross tenant"})),
        ),
        (
            "POST",
            format!("/automations/v2/runs/{run_id}/gate"),
            Some(json!({"decision": "approve"})),
        ),
    ] {
        let resp = app
            .clone()
            .oneshot(tenant_request(
                method,
                uri,
                "org-a",
                "workspace-a",
                "user-a",
                body,
            ))
            .await
            .expect("cross-tenant run response");
        assert_eq!(resp.status(), StatusCode::NOT_FOUND, "{method}");
    }

    let tenant_b_get_resp = app
        .clone()
        .oneshot(tenant_request(
            "GET",
            format!("/automations/v2/runs/{run_id}"),
            "org-b",
            "workspace-b",
            "user-b",
            None,
        ))
        .await
        .expect("tenant b run get response");
    assert_eq!(tenant_b_get_resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn automation_v2_payload_cannot_override_request_tenant() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut payload = automation_v2_create_payload("tenant-override-auto", "Tenant Override");
    payload["metadata"] = json!({
        "tenant_context": explicit_tenant("org-b", "workspace-b", "user-b")
    });

    let create_resp = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/automations/v2",
            "org-a",
            "workspace-a",
            "user-a",
            Some(payload),
        ))
        .await
        .expect("automation create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let stored = state
        .get_automation_v2("tenant-override-auto")
        .await
        .expect("stored automation");
    let tenant = stored.tenant_context();
    assert_eq!(tenant.org_id, "org-a");
    assert_eq!(tenant.workspace_id, "workspace-a");
    assert_eq!(tenant.actor_id.as_deref(), Some("user-a"));
}

#[tokio::test]
async fn automation_v2_background_runs_preserve_stored_tenant_context() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let create_resp = app
        .oneshot(tenant_request(
            "POST",
            "/automations/v2",
            "org-b",
            "workspace-b",
            "user-b",
            Some(automation_v2_create_payload(
                "tenant-b-background-auto",
                "Tenant B Background",
            )),
        ))
        .await
        .expect("automation create response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let automation = state
        .get_automation_v2("tenant-b-background-auto")
        .await
        .expect("stored automation");

    let scheduled = state
        .create_automation_v2_run(&automation, "scheduled")
        .await
        .expect("scheduled run");
    assert_eq!(scheduled.tenant_context.org_id, "org-b");
    let context_run_id = crate::http::context_runs::automation_v2_context_run_id(&scheduled.run_id);
    let context_run = crate::http::context_runs::load_context_run_state(&state, &context_run_id)
        .await
        .expect("scheduled context run");
    assert_eq!(context_run.tenant_context.org_id, "org-b");

    let claimed = state
        .claim_specific_automation_v2_run(&scheduled.run_id)
        .await
        .expect("claim scheduled run without request context");
    assert_eq!(claimed.tenant_context.org_id, "org-b");

    let watch = state
        .create_automation_v2_watch_run(&automation, "watch matched".to_string(), None)
        .await
        .expect("watch run");
    assert_eq!(watch.tenant_context.org_id, "org-b");
    let watch_context_run_id =
        crate::http::context_runs::automation_v2_context_run_id(&watch.run_id);
    let watch_context_run =
        crate::http::context_runs::load_context_run_state(&state, &watch_context_run_id)
            .await
            .expect("watch context run");
    assert_eq!(watch_context_run.tenant_context.org_id, "org-b");
}

#[test]
fn automation_v2_events_are_visible_only_to_matching_tenant() {
    let tenant_a = explicit_tenant("org-a", "workspace-a", "user-a");
    let tenant_b = explicit_tenant("org-b", "workspace-b", "user-b");
    let event = crate::EngineEvent::new(
        "automation.v2.run.created",
        json!({
            "automationID": "tenant-b-auto",
            "runID": "tenant-b-run",
            "tenantContext": tenant_b,
        }),
    );

    assert!(!super::super::global::event_visible_to_tenant(
        &event, &tenant_a
    ));
    assert!(super::super::global::event_visible_to_tenant(
        &event,
        &explicit_tenant("org-b", "workspace-b", "user-b")
    ));
}

#[tokio::test]
async fn automation_v2_sse_stream_filters_other_tenant_events_with_finite_body() {
    let state = test_state().await;
    let app = app_router(state.clone());
    for (automation_id, name, org, workspace, actor) in [
        (
            "tenant-a-sse-auto",
            "Tenant A SSE Automation",
            "org-a",
            "workspace-a",
            "user-a",
        ),
        (
            "tenant-b-sse-auto",
            "Tenant B SSE Automation",
            "org-b",
            "workspace-b",
            "user-b",
        ),
    ] {
        let create_resp = app
            .clone()
            .oneshot(tenant_request(
                "POST",
                "/automations/v2",
                org,
                workspace,
                actor,
                Some(automation_v2_create_payload(automation_id, name)),
            ))
            .await
            .expect("automation create response");
        assert_eq!(create_resp.status(), StatusCode::OK);
    }

    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let stream_app = app.clone();
    let reader = tokio::spawn(async move {
        let stream_resp = stream_app
            .oneshot(tenant_request(
                "GET",
                "/automations/v2/events",
                "org-a",
                "workspace-a",
                "user-a",
                None,
            ))
            .await
            .expect("automation v2 sse response");
        assert_eq!(stream_resp.status(), StatusCode::OK);

        let mut body = stream_resp.into_body().into_data_stream();
        let mut captured = String::new();
        let mut ready_tx = Some(ready_tx);
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            let Ok(Some(chunk)) = tokio::time::timeout(remaining, body.next()).await else {
                break;
            };
            let chunk = chunk.expect("sse chunk");
            captured.push_str(&String::from_utf8_lossy(&chunk));
            if captured.contains("\"stream\":\"automations_v2\"") {
                if let Some(ready_tx) = ready_tx.take() {
                    let _ = ready_tx.send(());
                }
            }
            if captured.contains("tenant-a-run") {
                break;
            }
        }
        captured
    });
    tokio::time::timeout(Duration::from_secs(2), ready_rx)
        .await
        .expect("timed out waiting for automation v2 sse ready frame")
        .expect("sse reader dropped before ready frame");

    let tenant_b_run_resp = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/automations/v2/tenant-b-sse-auto/run_now",
            "org-b",
            "workspace-b",
            "user-b",
            Some(json!({})),
        ))
        .await
        .expect("tenant b run_now response");
    assert_eq!(tenant_b_run_resp.status(), StatusCode::OK);
    let tenant_b_body = to_bytes(tenant_b_run_resp.into_body(), usize::MAX)
        .await
        .expect("tenant b run body");
    let tenant_b_payload: Value =
        serde_json::from_slice(&tenant_b_body).expect("tenant b run json");
    let tenant_b_run_id = tenant_b_payload
        .get("run")
        .and_then(|run| run.get("run_id"))
        .and_then(Value::as_str)
        .expect("tenant b run id")
        .to_string();

    let tenant_a_run_resp = app
        .clone()
        .oneshot(tenant_request(
            "POST",
            "/automations/v2/tenant-a-sse-auto/run_now",
            "org-a",
            "workspace-a",
            "user-a",
            Some(json!({})),
        ))
        .await
        .expect("tenant a run_now response");
    assert_eq!(tenant_a_run_resp.status(), StatusCode::OK);
    let tenant_a_body = to_bytes(tenant_a_run_resp.into_body(), usize::MAX)
        .await
        .expect("tenant a run body");
    let tenant_a_payload: Value =
        serde_json::from_slice(&tenant_a_body).expect("tenant a run json");
    let tenant_a_run_id = tenant_a_payload
        .get("run")
        .and_then(|run| run.get("run_id"))
        .and_then(Value::as_str)
        .expect("tenant a run id")
        .to_string();

    let captured = reader.await.expect("finite sse reader task");

    assert!(
        captured.contains(&tenant_a_run_id),
        "expected tenant A event in finite SSE body, got: {captured}"
    );
    assert!(
        !captured.contains(&tenant_b_run_id),
        "tenant B event leaked into tenant A SSE body: {captured}"
    );
}

#[tokio::test]
async fn automation_run_operator_wrappers_expose_context_run_links() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/automations")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "automation_id": "auto-ops-links",
                "name": "Automation Operator Links",
                "schedule": { "interval_seconds": { "seconds": 300 } },
                "mission": {
                    "objective": "Verify legacy automation operator linkage."
                },
                "policy": {
                    "approval": {
                        "requires_approval": true
                    }
                }
            })
            .to_string(),
        ))
        .expect("automation create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("automation create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let routine = state
        .get_routine("auto-ops-links")
        .await
        .expect("stored automation routine");

    let approval_run = state
        .create_routine_run(
            &routine,
            "manual",
            1,
            crate::RoutineRunStatus::PendingApproval,
            None,
        )
        .await;
    crate::http::context_runs::sync_routine_run_blackboard(&state, &approval_run)
        .await
        .expect("sync approval context");
    let approval_context_run_id =
        crate::http::context_runs::routine_context_run_id(&approval_run.run_id);

    let approve_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/runs/{}/approve", approval_run.run_id))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "reason": "approved from legacy wrapper" }).to_string(),
        ))
        .expect("approve request");
    let approve_resp = app
        .clone()
        .oneshot(approve_req)
        .await
        .expect("approve response");
    assert_eq!(approve_resp.status(), StatusCode::OK);
    let approve_body = to_bytes(approve_resp.into_body(), usize::MAX)
        .await
        .expect("approve body");
    let approve_payload: Value = serde_json::from_slice(&approve_body).expect("approve json");
    assert_eq!(
        approve_payload.get("contextRunID").and_then(Value::as_str),
        Some(approval_context_run_id.as_str())
    );
    assert_eq!(
        approve_payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(approval_context_run_id.as_str())
    );

    let running = state
        .create_routine_run(
            &routine,
            "manual",
            2,
            crate::RoutineRunStatus::Running,
            None,
        )
        .await;
    crate::http::context_runs::sync_routine_run_blackboard(&state, &running)
        .await
        .expect("sync running context");
    let running_context_run_id = crate::http::context_runs::routine_context_run_id(&running.run_id);

    let pause_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/runs/{}/pause", running.run_id))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "reason": "pause from legacy wrapper" }).to_string(),
        ))
        .expect("pause request");
    let pause_resp = app
        .clone()
        .oneshot(pause_req)
        .await
        .expect("pause response");
    assert_eq!(pause_resp.status(), StatusCode::OK);
    let pause_body = to_bytes(pause_resp.into_body(), usize::MAX)
        .await
        .expect("pause body");
    let pause_payload: Value = serde_json::from_slice(&pause_body).expect("pause json");
    assert_eq!(
        pause_payload.get("contextRunID").and_then(Value::as_str),
        Some(running_context_run_id.as_str())
    );
    assert_eq!(
        pause_payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(running_context_run_id.as_str())
    );

    let resume_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/runs/{}/resume", running.run_id))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "reason": "resume from legacy wrapper" }).to_string(),
        ))
        .expect("resume request");
    let resume_resp = app
        .clone()
        .oneshot(resume_req)
        .await
        .expect("resume response");
    assert_eq!(resume_resp.status(), StatusCode::OK);
    let resume_body = to_bytes(resume_resp.into_body(), usize::MAX)
        .await
        .expect("resume body");
    let resume_payload: Value = serde_json::from_slice(&resume_body).expect("resume json");
    assert_eq!(
        resume_payload.get("contextRunID").and_then(Value::as_str),
        Some(running_context_run_id.as_str())
    );
    assert_eq!(
        resume_payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(running_context_run_id.as_str())
    );

    let deny_run = state
        .create_routine_run(
            &routine,
            "manual",
            3,
            crate::RoutineRunStatus::PendingApproval,
            None,
        )
        .await;
    crate::http::context_runs::sync_routine_run_blackboard(&state, &deny_run)
        .await
        .expect("sync deny context");
    let deny_context_run_id = crate::http::context_runs::routine_context_run_id(&deny_run.run_id);

    let deny_req = Request::builder()
        .method("POST")
        .uri(format!("/automations/runs/{}/deny", deny_run.run_id))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "reason": "deny from legacy wrapper" }).to_string(),
        ))
        .expect("deny request");
    let deny_resp = app.clone().oneshot(deny_req).await.expect("deny response");
    assert_eq!(deny_resp.status(), StatusCode::OK);
    let deny_body = to_bytes(deny_resp.into_body(), usize::MAX)
        .await
        .expect("deny body");
    let deny_payload: Value = serde_json::from_slice(&deny_body).expect("deny json");
    assert_eq!(
        deny_payload.get("contextRunID").and_then(Value::as_str),
        Some(deny_context_run_id.as_str())
    );
    assert_eq!(
        deny_payload
            .get("run")
            .and_then(|value| value.get("contextRunID"))
            .and_then(Value::as_str),
        Some(deny_context_run_id.as_str())
    );
}

#[tokio::test]
async fn routines_run_now_blocks_external_side_effects_by_default() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-ext-blocked",
                "name": "External email sender",
                "schedule": { "interval_seconds": { "seconds": 300 } },
                "entrypoint": "connector.email.reply",
                "requires_approval": true,
                "external_integrations_allowed": false
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/routines/routine-ext-blocked/run_now")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("run_now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("run_now response");
    assert_eq!(run_now_resp.status(), StatusCode::FORBIDDEN);

    let history_req = Request::builder()
        .method("GET")
        .uri("/routines/routine-ext-blocked/history?limit=5")
        .body(Body::empty())
        .expect("history request");
    let history_resp = app
        .clone()
        .oneshot(history_req)
        .await
        .expect("history response");
    assert_eq!(history_resp.status(), StatusCode::OK);
    let history_body = to_bytes(history_resp.into_body(), usize::MAX)
        .await
        .expect("history body");
    let history_payload: Value = serde_json::from_slice(&history_body).expect("history json");
    assert_eq!(
        history_payload
            .get("events")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str()),
        Some("blocked_policy")
    );
}

#[tokio::test]
async fn routines_run_now_requires_approval_for_external_side_effects_when_enabled() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-ext-approval",
                "name": "External draft workflow",
                "schedule": { "interval_seconds": { "seconds": 300 } },
                "entrypoint": "connector.email.reply",
                "requires_approval": true,
                "external_integrations_allowed": true
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/routines/routine-ext-approval/run_now")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("run_now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("run_now response");
    assert_eq!(run_now_resp.status(), StatusCode::OK);
    let run_now_body = to_bytes(run_now_resp.into_body(), usize::MAX)
        .await
        .expect("run_now body");
    let run_now_payload: Value = serde_json::from_slice(&run_now_body).expect("run_now json");
    assert_eq!(
        run_now_payload.get("status").and_then(|v| v.as_str()),
        Some("pending_approval")
    );

    let history_req = Request::builder()
        .method("GET")
        .uri("/routines/routine-ext-approval/history?limit=5")
        .body(Body::empty())
        .expect("history request");
    let history_resp = app
        .clone()
        .oneshot(history_req)
        .await
        .expect("history response");
    assert_eq!(history_resp.status(), StatusCode::OK);
    let history_body = to_bytes(history_resp.into_body(), usize::MAX)
        .await
        .expect("history body");
    let history_payload: Value = serde_json::from_slice(&history_body).expect("history json");
    assert_eq!(
        history_payload
            .get("events")
            .and_then(|v| v.get(0))
            .and_then(|v| v.get("status"))
            .and_then(|v| v.as_str()),
        Some("pending_approval")
    );
}

#[tokio::test]
async fn routine_fired_event_contract_snapshot() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-fired-contract",
                "name": "Routine fired contract",
                "schedule": { "interval_seconds": { "seconds": 300 } },
                "entrypoint": "mission.default"
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/routines/routine-fired-contract/run_now")
        .header("content-type", "application/json")
        .body(Body::from(json!({ "run_count": 2 }).to_string()))
        .expect("run now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("run now response");
    assert_eq!(run_now_resp.status(), StatusCode::OK);

    let event = next_event_of_type(&mut rx, "routine.fired").await;
    let mut properties = event
        .properties
        .as_object()
        .cloned()
        .expect("properties object");
    let fired_at_ms = properties
        .remove("firedAtMs")
        .and_then(|v| v.as_u64())
        .expect("firedAtMs");
    assert!(fired_at_ms > 0);

    let snapshot = json!({
        "type": event.event_type,
        "properties": properties,
    });
    let expected = json!({
        "type": "routine.fired",
        "properties": {
            "routineID": "routine-fired-contract",
            "runCount": 2,
            "triggerType": "manual"
        }
    });
    assert_eq!(snapshot, expected);
}

#[tokio::test]
async fn routine_approval_required_event_contract_snapshot() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-approval-contract",
                "name": "Routine approval contract",
                "schedule": { "interval_seconds": { "seconds": 300 } },
                "entrypoint": "connector.email.reply",
                "requires_approval": true,
                "external_integrations_allowed": true
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/routines/routine-approval-contract/run_now")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("run now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("run now response");
    assert_eq!(run_now_resp.status(), StatusCode::OK);

    let event = next_event_of_type(&mut rx, "routine.approval_required").await;
    let snapshot = json!({
        "type": event.event_type,
        "properties": event.properties,
    });
    let expected = json!({
        "type": "routine.approval_required",
        "properties": {
            "routineID": "routine-approval-contract",
            "runCount": 1,
            "triggerType": "manual",
            "reason": "manual approval required before external side effects (manual)"
        }
    });
    assert_eq!(snapshot, expected);
}

#[tokio::test]
async fn routine_blocked_event_contract_snapshot() {
    let state = test_state().await;
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/routines")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "routine_id": "routine-blocked-contract",
                "name": "Routine blocked contract",
                "schedule": { "interval_seconds": { "seconds": 300 } },
                "entrypoint": "connector.email.reply",
                "requires_approval": true,
                "external_integrations_allowed": false
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::OK);

    let run_now_req = Request::builder()
        .method("POST")
        .uri("/routines/routine-blocked-contract/run_now")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("run now request");
    let run_now_resp = app
        .clone()
        .oneshot(run_now_req)
        .await
        .expect("run now response");
    assert_eq!(run_now_resp.status(), StatusCode::FORBIDDEN);

    let event = next_event_of_type(&mut rx, "routine.blocked").await;
    let snapshot = json!({
        "type": event.event_type,
        "properties": event.properties,
    });
    let expected = json!({
        "type": "routine.blocked",
        "properties": {
            "routineID": "routine-blocked-contract",
            "runCount": 1,
            "triggerType": "manual",
            "reason": "external integrations are disabled by policy"
        }
    });
    assert_eq!(snapshot, expected);
}

#[tokio::test]
async fn routine_tool_policy_hook_denies_disallowed_tool_for_session_scope() {
    let state = test_state().await;
    let session = Session::new(Some("routine-session".to_string()), Some(".".to_string()));
    let session_id = session.id.clone();
    state
        .storage
        .save_session(session)
        .await
        .expect("save session");

    state
        .set_routine_session_policy(
            session_id.clone(),
            "run-routine-hook-1".to_string(),
            "routine-hook-1".to_string(),
            vec!["read".to_string(), "mcp.arcade.search".to_string()],
        )
        .await;

    let hook = crate::agent_teams::ServerToolPolicyHook::new(state.clone());
    let decision = hook
        .evaluate_tool(ToolPolicyContext {
            session_id,
            message_id: "msg-1".to_string(),
            tenant_context: None,
            verified_tenant_context: None,
            tool: "bash".to_string(),
            args: json!({"command":"echo hi"}),
        })
        .await
        .expect("policy decision");

    assert!(!decision.allowed);
    assert!(decision
        .reason
        .as_deref()
        .unwrap_or_default()
        .contains("not allowed for routine"));
}

#[tokio::test]
async fn automation_tool_policy_hook_denies_writes_to_read_only_source_truth_files() {
    let state = test_state().await;
    let session = Session::new(
        Some("automation-session".to_string()),
        Some(".".to_string()),
    );
    let session_id = session.id.clone();
    state
        .storage
        .save_session(session)
        .await
        .expect("save session");

    let automation = crate::AutomationV2Spec {
        automation_id: "automation-read-only-guard".to_string(),
        name: "Read Only Guard".to_string(),
        description: Some(
            "Analyze RESUME.md and use it as the source of truth. Never edit, rewrite, rename, move, or delete RESUME.md."
                .to_string(),
        ),
        status: crate::AutomationV2Status::Active,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec { nodes: Vec::new() },
        execution: crate::AutomationExecutionPolicy {
            profile: None,
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 0,
        updated_at_ms: 0,
        creator_id: "test".to_string(),
        workspace_root: Some("/home/evan/job-hunt".to_string()),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("automation run");
    state
        .add_automation_v2_session(&run.run_id, &session_id)
        .await
        .expect("linked automation session");

    let hook = crate::agent_teams::ServerToolPolicyHook::new(state.clone());
    let decision = hook
        .evaluate_tool(ToolPolicyContext {
            session_id,
            message_id: "msg-automation-1".to_string(),
            tenant_context: None,
            verified_tenant_context: None,
            tool: "write".to_string(),
            args: json!({
                "path": "RESUME.md",
                "content": "bad overwrite",
                "__workspace_root": "/home/evan/job-hunt",
            }),
        })
        .await
        .expect("policy decision");

    assert!(!decision.allowed);
    assert!(decision
        .reason
        .as_deref()
        .unwrap_or_default()
        .contains("read-only source-of-truth"));
}
