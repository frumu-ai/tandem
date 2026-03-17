use super::*;

use async_trait::async_trait;
use serde_json::json;
use std::path::Path;
use tokio::sync::Mutex;

struct RecordingTool {
    name: String,
    calls: Arc<Mutex<Vec<Value>>>,
}

#[async_trait]
impl tandem_tools::Tool for RecordingTool {
    fn schema(&self) -> tandem_types::ToolSchema {
        tandem_types::ToolSchema {
            name: self.name.clone(),
            description: format!("Recording tool for {}", self.name),
            input_schema: json!({
                "type": "object",
                "additionalProperties": true,
            }),
        }
    }

    async fn execute(&self, args: Value) -> anyhow::Result<tandem_types::ToolResult> {
        self.calls.lock().await.push(args.clone());
        Ok(tandem_types::ToolResult {
            output: format!("executed {}", self.name),
            metadata: json!({ "name": self.name, "args": args }),
        })
    }
}

fn write_demo_workflows(root: &Path) {
    let workflows_dir = root.join("workflows");
    let hooks_dir = root.join("hooks");
    std::fs::create_dir_all(&workflows_dir).expect("create workflows dir");
    std::fs::create_dir_all(&hooks_dir).expect("create hooks dir");
    std::fs::write(
        workflows_dir.join("build_feature.yaml"),
        r#"
workflow:
  id: build_feature
  name: Build Feature
  steps:
    - action: tool:workflow_test.executor
      with:
        stage: executor
  hooks:
    task_created:
      - action: tool:workflow_test.kanban
        with:
          board: roadmap
"#,
    )
    .expect("write workflow");
    std::fs::write(
        hooks_dir.join("notify.yaml"),
        r#"
hooks:
  - id: build_feature.task_completed.notify
    workflow_id: build_feature
    event: task_completed
    actions:
      - action: tool:workflow_test.slack
        with:
          channel: engineering
"#,
    )
    .expect("write hooks");
}

async fn register_recording_tool(state: &AppState, name: &str) -> Arc<Mutex<Vec<Value>>> {
    let calls = Arc::new(Mutex::new(Vec::new()));
    state
        .tools
        .register_tool(
            name.to_string(),
            Arc::new(RecordingTool {
                name: name.to_string(),
                calls: calls.clone(),
            }),
        )
        .await;
    calls
}

async fn seed_workflow_test_slack_binding(state: &AppState) {
    let mut bindings = state
        .capability_resolver
        .list_bindings()
        .await
        .expect("list bindings");
    bindings
        .bindings
        .push(crate::capability_resolver::CapabilityBinding {
            capability_id: "slack.post_message".to_string(),
            provider: "custom".to_string(),
            tool_name: "workflow_test.slack".to_string(),
            tool_name_aliases: Vec::new(),
            request_transform: None,
            response_transform: None,
            metadata: json!({
                "source": "workflow_test",
            }),
        });
    state
        .capability_resolver
        .set_bindings(bindings)
        .await
        .expect("set bindings");
}

async fn workflow_test_state() -> AppState {
    let state = test_state().await;
    let state_dir = state
        .workflow_runs_path
        .parent()
        .expect("state dir")
        .to_path_buf();
    write_demo_workflows(&state_dir.join("builtin_workflows"));
    state.reload_workflows().await.expect("reload workflows");
    state
}

async fn wait_for_call_count(calls: &Arc<Mutex<Vec<Value>>>, expected: usize) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if calls.lock().await.len() >= expected {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("timed out waiting for workflow tool call");
}

#[tokio::test]
async fn workflows_list_validate_and_manual_run() {
    let state = workflow_test_state().await;
    let executor_calls = register_recording_tool(&state, "workflow_test.executor").await;
    let app = app_router(state.clone());

    let list_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/workflows")
                .body(Body::empty())
                .expect("list request"),
        )
        .await
        .expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("list json");
    assert_eq!(list_payload.get("count").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(
        list_payload["workflows"][0]["workflow_id"].as_str(),
        Some("build_feature")
    );
    assert_eq!(
        list_payload["automation_previews"]["build_feature"]["creator_id"].as_str(),
        Some("workflow_registry")
    );
    assert_eq!(
        list_payload["automation_previews"]["build_feature"]["metadata"]["workflow_id"].as_str(),
        Some("build_feature")
    );

    let get_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/workflows/build_feature")
                .body(Body::empty())
                .expect("get request"),
        )
        .await
        .expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    assert_eq!(
        get_payload["automation_preview"]["metadata"]["workflow_id"].as_str(),
        Some("build_feature")
    );
    assert_eq!(
        get_payload["automation_preview"]["flow"]["nodes"][0]["objective"]
            .as_str()
            .map(|value| value.contains("tool:workflow_test.executor")),
        Some(true)
    );

    let validate_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflows/validate")
                .header("content-type", "application/json")
                .body(Body::from(json!({ "reload": true }).to_string()))
                .expect("validate request"),
        )
        .await
        .expect("validate response");
    assert_eq!(validate_resp.status(), StatusCode::OK);

    let run_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflows/build_feature/run")
                .body(Body::empty())
                .expect("run request"),
        )
        .await
        .expect("run response");
    assert_eq!(run_resp.status(), StatusCode::OK);
    let run_body = to_bytes(run_resp.into_body(), usize::MAX)
        .await
        .expect("run body");
    let run_payload: Value = serde_json::from_slice(&run_body).expect("run json");
    assert_eq!(run_payload["run"]["status"].as_str(), Some("completed"));
    let workflow_context_run_id =
        crate::http::workflow_context_run_id(run_payload["run"]["run_id"].as_str().unwrap_or(""));
    let blackboard_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/context/runs/{workflow_context_run_id}/blackboard"
                ))
                .body(Body::empty())
                .expect("blackboard request"),
        )
        .await
        .expect("blackboard response");
    assert_eq!(blackboard_resp.status(), StatusCode::OK);
    let blackboard_body = to_bytes(blackboard_resp.into_body(), usize::MAX)
        .await
        .expect("blackboard body");
    let blackboard_payload: Value =
        serde_json::from_slice(&blackboard_body).expect("blackboard json");
    let blackboard = blackboard_payload
        .get("blackboard")
        .cloned()
        .unwrap_or_else(|| json!({}));
    assert!(blackboard["tasks"]
        .as_array()
        .map(|rows| rows
            .iter()
            .any(|row| row["task_type"].as_str() == Some("workflow_action")))
        .unwrap_or(false));
    assert!(blackboard["artifacts"]
        .as_array()
        .map(|rows| rows
            .iter()
            .any(|row| row["artifact_type"].as_str() == Some("workflow_action_output")))
        .unwrap_or(false));
    wait_for_call_count(&executor_calls, 1).await;
    assert_eq!(
        executor_calls.lock().await[0]["stage"].as_str(),
        Some("executor")
    );
}

#[tokio::test]
async fn workflow_dispatch_executes_hooks_and_dedupes() {
    let state = workflow_test_state().await;
    let app = app_router(state.clone());
    let kanban_calls = register_recording_tool(&state, "workflow_test.kanban").await;
    let slack_calls = register_recording_tool(&state, "workflow_test.slack").await;
    seed_workflow_test_slack_binding(&state).await;
    let mut rx = state.event_bus.subscribe();

    crate::dispatch_workflow_event(
        &state,
        &EngineEvent::new(
            "context.task.created",
            json!({
                "event_id": "evt-task-created-1",
                "task_id": "task-1"
            }),
        ),
    )
    .await;
    crate::dispatch_workflow_event(
        &state,
        &EngineEvent::new(
            "context.task.created",
            json!({
                "event_id": "evt-task-created-1",
                "task_id": "task-1"
            }),
        ),
    )
    .await;
    crate::dispatch_workflow_event(
        &state,
        &EngineEvent::new(
            "context.task.completed",
            json!({
                "event_id": "evt-task-completed-1",
                "task_id": "task-1"
            }),
        ),
    )
    .await;

    wait_for_call_count(&kanban_calls, 1).await;
    wait_for_call_count(&slack_calls, 1).await;
    assert_eq!(kanban_calls.lock().await.len(), 1);
    assert_eq!(slack_calls.lock().await.len(), 1);
    assert_eq!(
        kanban_calls.lock().await[0]["board"].as_str(),
        Some("roadmap")
    );
    assert_eq!(
        slack_calls.lock().await[0]["channel"].as_str(),
        Some("engineering")
    );

    let runs = state.list_workflow_runs(Some("build_feature"), 10).await;
    assert_eq!(runs.len(), 2);
    assert!(runs
        .iter()
        .all(|run| run.status == crate::WorkflowRunStatus::Completed));
    assert!(runs
        .iter()
        .all(|run| run.task_id.as_deref() == Some("task-1")));
    assert!(runs.iter().all(|run| {
        run.actions
            .iter()
            .all(|action| action.task_id.as_deref() == Some("task-1"))
    }));
    let slack_run = runs
        .iter()
        .find(|run| {
            run.actions
                .iter()
                .any(|action| action.action == "tool:workflow_test.slack")
        })
        .expect("slack workflow run");
    let slack_action_output = slack_run.actions[0]
        .output
        .clone()
        .expect("slack workflow output");
    assert_eq!(
        slack_action_output["external_action"]["capability_id"].as_str(),
        Some("slack.post_message")
    );
    assert_eq!(
        slack_action_output["external_action"]["source_kind"].as_str(),
        Some("workflow")
    );
    let external_actions_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/external-actions?limit=10")
                .body(Body::empty())
                .expect("external actions request"),
        )
        .await
        .expect("external actions response");
    assert_eq!(external_actions_resp.status(), StatusCode::OK);
    let external_actions_body = to_bytes(external_actions_resp.into_body(), usize::MAX)
        .await
        .expect("external actions body");
    let external_actions_payload: Value =
        serde_json::from_slice(&external_actions_body).expect("external actions json");
    assert!(external_actions_payload["actions"]
        .as_array()
        .map(|rows| rows.iter().any(|row| {
            row["source_kind"].as_str() == Some("workflow")
                && row["capability_id"].as_str() == Some("slack.post_message")
        }))
        .unwrap_or(false));
    let slack_context_run_id = crate::http::workflow_context_run_id(&slack_run.run_id);
    let slack_blackboard_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/context/runs/{slack_context_run_id}/blackboard"))
                .body(Body::empty())
                .expect("slack workflow blackboard request"),
        )
        .await
        .expect("slack workflow blackboard response");
    assert_eq!(slack_blackboard_resp.status(), StatusCode::OK);
    let slack_blackboard_body = to_bytes(slack_blackboard_resp.into_body(), usize::MAX)
        .await
        .expect("slack workflow blackboard body");
    let slack_blackboard_payload: Value =
        serde_json::from_slice(&slack_blackboard_body).expect("slack workflow blackboard json");
    assert!(slack_blackboard_payload["blackboard"]["artifacts"]
        .as_array()
        .map(|rows| rows
            .iter()
            .any(|row| { row["artifact_type"].as_str() == Some("external_action_receipt") }))
        .unwrap_or(false));
    let workflow_context_run_id = crate::http::workflow_context_run_id(&runs[0].run_id);
    let blackboard_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!(
                    "/context/runs/{workflow_context_run_id}/blackboard"
                ))
                .body(Body::empty())
                .expect("workflow blackboard request"),
        )
        .await
        .expect("workflow blackboard response");
    assert_eq!(blackboard_resp.status(), StatusCode::OK);
    let blackboard_body = to_bytes(blackboard_resp.into_body(), usize::MAX)
        .await
        .expect("workflow blackboard body");
    let blackboard_payload: Value =
        serde_json::from_slice(&blackboard_body).expect("workflow blackboard json");
    let blackboard = blackboard_payload
        .get("blackboard")
        .cloned()
        .unwrap_or_else(|| json!({}));
    assert!(blackboard["tasks"]
        .as_array()
        .map(|rows| rows
            .iter()
            .any(|row| row["workflow_id"].as_str() == Some("build_feature")))
        .unwrap_or(false));
    assert!(blackboard["artifacts"]
        .as_array()
        .map(|rows| rows
            .iter()
            .any(|row| row["artifact_type"].as_str() == Some("workflow_action_output")))
        .unwrap_or(false));

    let mut saw_action_started = false;
    let mut saw_run_completed = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    while tokio::time::Instant::now() < deadline && (!saw_action_started || !saw_run_completed) {
        if let Ok(event) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            if let Ok(event) = event {
                if event.event_type == "workflow.action.started"
                    && event.properties.get("taskID").and_then(|v| v.as_str()) == Some("task-1")
                {
                    saw_action_started = true;
                }
                if event.event_type == "workflow.run.completed"
                    && event.properties.get("taskID").and_then(|v| v.as_str()) == Some("task-1")
                {
                    saw_run_completed = true;
                }
            }
        }
    }
    assert!(
        saw_action_started,
        "expected workflow.action.started with taskID"
    );
    assert!(
        saw_run_completed,
        "expected workflow.run.completed with taskID"
    );
}

#[tokio::test]
async fn workflow_hook_patch_disables_binding() {
    let state = workflow_test_state().await;
    let app = app_router(state.clone());

    let hooks_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/workflow-hooks")
                .body(Body::empty())
                .expect("hooks request"),
        )
        .await
        .expect("hooks response");
    assert_eq!(hooks_resp.status(), StatusCode::OK);
    let hooks_body = to_bytes(hooks_resp.into_body(), usize::MAX)
        .await
        .expect("hooks body");
    let hooks_payload: Value = serde_json::from_slice(&hooks_body).expect("hooks json");
    let binding_id = hooks_payload["hooks"]
        .as_array()
        .and_then(|rows| {
            rows.iter()
                .find(|row| row["event"].as_str() == Some("task_completed"))
        })
        .and_then(|row| row["binding_id"].as_str())
        .expect("task_completed binding")
        .to_string();

    let patch_resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/workflow-hooks/{binding_id}"))
                .header("content-type", "application/json")
                .body(Body::from(json!({ "enabled": false }).to_string()))
                .expect("patch request"),
        )
        .await
        .expect("patch response");
    assert_eq!(patch_resp.status(), StatusCode::OK);

    let simulate_resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/workflows/simulate")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({
                        "event_type": "context.task.completed",
                        "properties": { "event_id": "evt-sim-1" }
                    })
                    .to_string(),
                ))
                .expect("simulate request"),
        )
        .await
        .expect("simulate response");
    assert_eq!(simulate_resp.status(), StatusCode::OK);
    let simulate_body = to_bytes(simulate_resp.into_body(), usize::MAX)
        .await
        .expect("simulate body");
    let simulate_payload: Value = serde_json::from_slice(&simulate_body).expect("simulate json");
    assert_eq!(
        simulate_payload["simulation"]["matched_bindings"]
            .as_array()
            .map(|rows| rows.len()),
        Some(0)
    );
}
