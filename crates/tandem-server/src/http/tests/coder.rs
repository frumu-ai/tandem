use super::*;
use tandem_memory::types::GlobalMemoryRecord;
use tokio::net::TcpListener;

async fn spawn_fake_github_mcp_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fake github mcp listener");
    let addr = listener.local_addr().expect("fake github mcp addr");
    let app = axum::Router::new().route(
        "/",
        axum::routing::post(|axum::Json(request): axum::Json<Value>| async move {
            let id = request.get("id").cloned().unwrap_or(Value::Null);
            let method = request
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let result = match method {
                "initialize" => json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {},
                    "serverInfo": {
                        "name": "github",
                        "version": "test"
                    }
                }),
                "tools/list" => json!({
                    "tools": [
                        {
                            "name": "list_repository_issues",
                            "description": "List repository issues",
                            "inputSchema": {"type":"object"}
                        },
                        {
                            "name": "get_issue",
                            "description": "Get a GitHub issue",
                            "inputSchema": {"type":"object"}
                        },
                        {
                            "name": "mcp.github.list_pull_requests",
                            "description": "List repository pull requests",
                            "inputSchema": {"type":"object"}
                        },
                        {
                            "name": "mcp.github.get_pull_request",
                            "description": "Get a GitHub pull request",
                            "inputSchema": {"type":"object"}
                        },
                        {
                            "name": "mcp.github.create_pull_request",
                            "description": "Create a GitHub pull request",
                            "inputSchema": {"type":"object"}
                        }
                    ]
                }),
                "tools/call" => {
                    let name = request
                        .get("params")
                        .and_then(|row| row.get("name"))
                        .and_then(Value::as_str)
                        .unwrap_or_default();
                    match name {
                        "mcp.github.create_pull_request" => json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": "created pull request #314"
                                }
                            ],
                            "pull_request": {
                                "number": 314,
                                "title": "Guard startup recovery config loading.",
                                "state": "open",
                                "html_url": "https://github.com/evan/tandem/pull/314",
                                "head": {"ref": "coder/issue-313-fix"},
                                "base": {"ref": "main"}
                            }
                        }),
                        _ => json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": format!("handled {name}")
                                }
                            ]
                        }),
                    }
                }
                other => json!({
                    "content": [
                        {
                            "type": "text",
                            "text": format!("unsupported method {other}")
                        }
                    ]
                }),
            };
            axum::Json(json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result,
            }))
        }),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .await
            .expect("serve fake github mcp");
    });
    (format!("http://{addr}"), server)
}

#[tokio::test]
async fn coder_issue_triage_run_create_get_and_list() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-1",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 1234,
                    "url": "https://github.com/evan/tandem/issues/1234"
                },
                "source_client": "desktop_developer_mode"
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
            .get("coder_run")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("issue_triage")
    );
    assert_eq!(
        create_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("repo_inspection")
    );
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("run_type"))
            .and_then(Value::as_str),
        Some("coder_issue_triage")
    );
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("tasks"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(5)
    );
    let tasks = get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .cloned()
        .expect("tasks");
    assert_eq!(
        tasks
            .iter()
            .find(|row| row.get("workflow_node_id").and_then(Value::as_str)
                == Some("ingest_reference"))
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("done")
    );
    assert_eq!(
        tasks
            .iter()
            .find(|row| row.get("workflow_node_id").and_then(Value::as_str)
                == Some("retrieve_memory"))
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("done")
    );
    assert_eq!(
        tasks
            .iter()
            .find(|row| row.get("workflow_node_id").and_then(Value::as_str) == Some("inspect_repo"))
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("runnable")
    );
    assert!(get_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
        }))
        .unwrap_or(false));
    assert_eq!(
        get_payload
            .get("memory_hits")
            .and_then(|row| row.get("query"))
            .and_then(Value::as_str),
        Some("evan/tandem issue #1234")
    );
    assert_eq!(
        get_payload
            .get("memory_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );

    let list_req = Request::builder()
        .method("GET")
        .uri("/coder/runs?workflow_mode=issue_triage")
        .body(Body::empty())
        .expect("list request");
    let list_resp = app.clone().oneshot(list_req).await.expect("list response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_body = to_bytes(list_resp.into_body(), usize::MAX)
        .await
        .expect("list body");
    let list_payload: Value = serde_json::from_slice(&list_body).expect("list json");
    assert_eq!(
        list_payload
            .get("runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        list_payload
            .get("runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("linked_context_run_id"))
            .and_then(Value::as_str),
        Some(linked_context_run_id.as_str())
    );

    let artifact_event = next_event_of_type(&mut rx, "coder.artifact.added").await;
    assert_eq!(
        artifact_event
            .properties
            .get("workflow_mode")
            .and_then(Value::as_str),
        Some("issue_triage")
    );
    assert_eq!(
        artifact_event
            .properties
            .get("kind")
            .and_then(Value::as_str),
        Some("memory_hits")
    );
    assert_eq!(
        artifact_event
            .properties
            .get("phase")
            .and_then(Value::as_str),
        Some("memory_retrieval")
    );

    let created_event = next_event_of_type(&mut rx, "coder.run.created").await;
    assert_eq!(
        created_event
            .properties
            .get("workflow_mode")
            .and_then(Value::as_str),
        Some("issue_triage")
    );
    assert_eq!(
        created_event
            .properties
            .get("repo_binding")
            .and_then(|row| row.get("repo_slug"))
            .and_then(Value::as_str),
        Some("evan/tandem")
    );
    assert_eq!(
        created_event
            .properties
            .get("phase")
            .and_then(Value::as_str),
        Some("repo_inspection")
    );
}

#[tokio::test]
async fn coder_pr_review_run_create_gets_seeded_review_tasks() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-pr-review-1",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 88,
                    "url": "https://github.com/evan/tandem/pull/88"
                },
                "source_client": "desktop_developer_mode"
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
            .get("coder_run")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("run_type"))
            .and_then(Value::as_str),
        Some("coder_pr_review")
    );
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    let tasks = get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .cloned()
        .expect("tasks");
    assert_eq!(
        tasks
            .iter()
            .find(|row| row.get("workflow_node_id").and_then(Value::as_str)
                == Some("retrieve_memory"))
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("done")
    );
    assert_eq!(
        tasks
            .iter()
            .find(|row| row.get("workflow_node_id").and_then(Value::as_str)
                == Some("inspect_pull_request"))
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("runnable")
    );
    assert!(get_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
        }))
        .unwrap_or(false));
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("tasks"))
            .and_then(Value::as_array)
            .map(|rows| rows.len())
            .filter(|count| *count >= 3),
        Some(
            get_payload
                .get("run")
                .and_then(|row| row.get("tasks"))
                .and_then(Value::as_array)
                .map(|rows| rows.len())
                .unwrap_or_default()
        )
    );
    assert!(get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("workflow_node_id").and_then(Value::as_str) == Some("inspect_pull_request")
        }))
        .unwrap_or(false));
    assert!(get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("workflow_node_id").and_then(Value::as_str) == Some("review_pull_request")
        }))
        .unwrap_or(false));
    assert_eq!(
        get_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("repo_inspection")
    );
    assert_eq!(
        get_payload
            .get("coder_run")
            .and_then(|row| row.get("linked_context_run_id"))
            .and_then(Value::as_str),
        Some(linked_context_run_id.as_str())
    );

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-1/memory-hits")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    assert_eq!(
        hits_payload.get("query").and_then(Value::as_str),
        Some("evan/tandem pull request #88")
    );
}

#[tokio::test]
async fn coder_issue_fix_run_create_gets_seeded_fix_tasks() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-1",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 77
                }
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

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-issue-fix-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_payload: Value = serde_json::from_slice(
        &to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body"),
    )
    .expect("get json");
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("run_type"))
            .and_then(Value::as_str),
        Some("coder_issue_fix")
    );
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    let tasks = get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .cloned()
        .expect("tasks");
    assert_eq!(
        tasks
            .iter()
            .find(|row| row.get("workflow_node_id").and_then(Value::as_str)
                == Some("retrieve_memory"))
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("done")
    );
    assert_eq!(
        tasks
            .iter()
            .find(|row| row.get("workflow_node_id").and_then(Value::as_str)
                == Some("inspect_issue_context"))
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("runnable")
    );
    assert!(get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("workflow_node_id").and_then(Value::as_str) == Some("prepare_fix")
        }))
        .unwrap_or(false));
    assert!(get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("workflow_node_id").and_then(Value::as_str) == Some("validate_fix")
        }))
        .unwrap_or(false));
    assert!(get_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
        }))
        .unwrap_or(false));
    assert_eq!(
        get_payload
            .get("memory_hits")
            .and_then(|row| row.get("query"))
            .and_then(Value::as_str),
        Some("evan/tandem issue #77")
    );
}

#[tokio::test]
async fn coder_issue_fix_validation_report_advances_fix_run() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-validate",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 79
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let validation_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-validate/issue-fix-validation-report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Added a guard around the startup recovery path.",
                "root_cause": "Startup recovery skipped the config fallback branch.",
                "fix_strategy": "guard fallback branch",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "validation_steps": ["cargo test -p tandem-server coder_issue_fix_validation_report_advances_fix_run -- --test-threads=1"],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "targeted validation regression passed"
                }],
                "memory_hits_used": ["memory-hit-fix-validation-1"]
            })
            .to_string(),
        ))
        .expect("validation request");
    let validation_resp = app
        .clone()
        .oneshot(validation_req)
        .await
        .expect("validation response");
    assert_eq!(validation_resp.status(), StatusCode::OK);
    let validation_payload: Value = serde_json::from_slice(
        &to_bytes(validation_resp.into_body(), usize::MAX)
            .await
            .expect("validation body"),
    )
    .expect("validation json");
    assert_eq!(
        validation_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_validation_report")
    );
    assert_eq!(
        validation_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().any(|row| {
                row.get("kind").and_then(Value::as_str) == Some("validation_memory")
            })),
        Some(true)
    );
    assert_eq!(
        validation_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    assert_eq!(
        validation_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("artifact_write")
    );

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Running);
    for workflow_node_id in [
        "inspect_issue_context",
        "retrieve_memory",
        "prepare_fix",
        "validate_fix",
    ] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done"
        );
    }
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("write_fix_artifact"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Runnable)
    );
}

#[tokio::test]
async fn coder_issue_fix_execute_next_drives_task_runtime_to_completion() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-execute-next",
                "workflow_mode": "issue_fix",
                "model_provider": "local",
                "model_id": "echo-1",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 199
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();
    let mut changed_file_artifact_path: Option<String> = None;

    for expected in [
        "inspect_issue_context",
        "prepare_fix",
        "validate_fix",
        "write_fix_artifact",
    ] {
        let execute_req = Request::builder()
            .method("POST")
            .uri("/coder/runs/coder-issue-fix-execute-next/execute-next")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "agent_id": "coder_engine_worker_test"
                })
                .to_string(),
            ))
            .expect("execute request");
        let execute_resp = app
            .clone()
            .oneshot(execute_req)
            .await
            .expect("execute response");
        assert_eq!(execute_resp.status(), StatusCode::OK);
        let execute_payload: Value = serde_json::from_slice(
            &to_bytes(execute_resp.into_body(), usize::MAX)
                .await
                .expect("execute body"),
        )
        .expect("execute json");
        assert_eq!(
            execute_payload
                .get("task")
                .and_then(|row| row.get("workflow_node_id"))
                .and_then(Value::as_str),
            Some(expected)
        );
        if expected == "prepare_fix" {
            assert_eq!(
                execute_payload
                    .get("dispatch_result")
                    .and_then(|row| row.get("worker_artifact"))
                    .and_then(|row| row.get("artifact_type"))
                    .and_then(Value::as_str),
                Some("coder_issue_fix_worker_session")
            );
            assert_eq!(
                execute_payload
                    .get("dispatch_result")
                    .and_then(|row| row.get("plan_artifact"))
                    .and_then(|row| row.get("artifact_type"))
                    .and_then(Value::as_str),
                Some("coder_issue_fix_plan")
            );
            assert_eq!(
                execute_payload
                    .get("dispatch_result")
                    .and_then(|row| row.get("worker_session"))
                    .and_then(|row| row.get("status"))
                    .and_then(Value::as_str),
                Some("completed")
            );
            assert_eq!(
                execute_payload
                    .get("dispatch_result")
                    .and_then(|row| row.get("worker_session"))
                    .and_then(|row| row.get("model"))
                    .and_then(|row| row.get("provider_id"))
                    .and_then(Value::as_str),
                Some("local")
            );
            assert!(execute_payload
                .get("dispatch_result")
                .and_then(|row| row.get("worker_session"))
                .and_then(|row| row.get("assistant_text"))
                .and_then(Value::as_str)
                .is_some_and(|text| text.contains("Echo:")));
            let changed_file_entries = execute_payload
                .get("dispatch_result")
                .and_then(|row| row.get("worker_session"))
                .and_then(|row| row.get("changed_file_entries"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if !changed_file_entries.is_empty() {
                assert_eq!(
                    execute_payload
                        .get("dispatch_result")
                        .and_then(|row| row.get("changed_file_artifact"))
                        .and_then(|row| row.get("artifact_type"))
                        .and_then(Value::as_str),
                    Some("coder_changed_file_evidence")
                );
                changed_file_artifact_path = execute_payload
                    .get("dispatch_result")
                    .and_then(|row| row.get("changed_file_artifact"))
                    .and_then(|row| row.get("path"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string);
            }
        } else if expected == "validate_fix" {
            assert_eq!(
                execute_payload
                    .get("dispatch_result")
                    .and_then(|row| row.get("artifact"))
                    .and_then(|row| row.get("artifact_type"))
                    .and_then(Value::as_str),
                Some("coder_validation_report")
            );
        }
    }

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Completed);
    for workflow_node_id in [
        "inspect_issue_context",
        "retrieve_memory",
        "prepare_fix",
        "validate_fix",
        "write_fix_artifact",
    ] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done"
        );
    }
    let blackboard = load_context_blackboard(&state, &linked_context_run_id);
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| { artifact.artifact_type == "coder_issue_fix_worker_session" }));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| { artifact.artifact_type == "coder_issue_fix_plan" }));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| { artifact.artifact_type == "coder_issue_fix_validation_session" }));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| { artifact.artifact_type == "coder_patch_summary" }));
    if let Some(changed_file_artifact_path) = changed_file_artifact_path {
        let changed_file_payload: Value = serde_json::from_str(
            &tokio::fs::read_to_string(&changed_file_artifact_path)
                .await
                .expect("read changed file artifact"),
        )
        .expect("parse changed file artifact");
        assert!(changed_file_payload
            .get("entries")
            .and_then(Value::as_array)
            .is_some_and(|rows| rows.iter().any(|row| {
                row.get("path").and_then(Value::as_str)
                    == Some("crates/tandem-server/src/http/coder.rs")
                    && row
                        .get("preview")
                        .and_then(Value::as_str)
                        .is_some_and(|preview| preview.contains("Summary:"))
            })));
        let patch_summary_path = blackboard
            .artifacts
            .iter()
            .find(|artifact| artifact.artifact_type == "coder_patch_summary")
            .map(|artifact| artifact.path.clone())
            .expect("patch summary path");
        let patch_summary_payload: Value = serde_json::from_str(
            &tokio::fs::read_to_string(&patch_summary_path)
                .await
                .expect("read patch summary artifact"),
        )
        .expect("parse patch summary artifact");
        assert!(patch_summary_payload
            .get("changed_file_entries")
            .and_then(Value::as_array)
            .is_some_and(|rows| rows.iter().any(|row| {
                row.get("path").and_then(Value::as_str)
                    == Some("crates/tandem-server/src/http/coder.rs")
            })));
    }
}

#[tokio::test]
async fn coder_issue_fix_execute_all_runs_to_completion() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-execute-all",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 299
                }
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

    let execute_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-execute-all/execute-all")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "agent_id": "coder_engine_worker_test",
                "max_steps": 8
            })
            .to_string(),
        ))
        .expect("execute-all request");
    let execute_resp = app
        .clone()
        .oneshot(execute_req)
        .await
        .expect("execute-all response");
    assert_eq!(execute_resp.status(), StatusCode::OK);
    let execute_payload: Value = serde_json::from_slice(
        &to_bytes(execute_resp.into_body(), usize::MAX)
            .await
            .expect("execute-all body"),
    )
    .expect("execute-all json");
    assert_eq!(
        execute_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        execute_payload
            .get("stopped_reason")
            .and_then(Value::as_str),
        Some("run_completed")
    );
    assert!(execute_payload
        .get("executed_steps")
        .and_then(Value::as_u64)
        .is_some_and(|count| count >= 4));
}

#[tokio::test]
async fn coder_issue_fix_summary_create_writes_artifact() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-summary",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 78
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-summary/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Guard the missing config branch and add a regression test for startup recovery.",
                "root_cause": "Nil config fallback was skipped during startup recovery.",
                "fix_strategy": "add startup fallback guard",
                "changed_files": [
                    "crates/tandem-server/src/http/coder.rs",
                    "crates/tandem-server/src/http/tests/coder.rs"
                ],
                "validation_steps": ["cargo test -p tandem-server coder_issue_fix_summary_create_writes_artifact -- --test-threads=1"],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "targeted coder issue-fix regression passed"
                }],
                "memory_hits_used": ["memory-hit-fix-1"],
                "notes": "Prior triage memory pointed to startup recovery flow."
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    assert_eq!(
        summary_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_issue_fix_summary")
    );
    assert_eq!(
        summary_payload
            .get("validation_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_validation_report")
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| { row.get("kind").and_then(Value::as_str) == Some("fix_pattern") })),
        Some(true)
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().any(|row| {
                row.get("kind").and_then(Value::as_str) == Some("validation_memory")
            })),
        Some(true)
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| { row.get("kind").and_then(Value::as_str) == Some("run_outcome") })),
        Some(true)
    );
    assert_eq!(
        summary_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("completed")
    );
    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Completed);
    for workflow_node_id in [
        "inspect_issue_context",
        "retrieve_memory",
        "prepare_fix",
        "validate_fix",
        "write_fix_artifact",
    ] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done"
        );
    }

    let artifacts_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-issue-fix-summary/artifacts")
        .body(Body::empty())
        .expect("artifacts request");
    let artifacts_resp = app
        .clone()
        .oneshot(artifacts_req)
        .await
        .expect("artifacts response");
    assert_eq!(artifacts_resp.status(), StatusCode::OK);
    let artifacts_payload: Value = serde_json::from_slice(
        &to_bytes(artifacts_resp.into_body(), usize::MAX)
            .await
            .expect("artifacts body"),
    )
    .expect("artifacts json");
    assert!(artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_issue_fix_summary")
        }))
        .unwrap_or(false));
    assert!(artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_validation_report")
        }))
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_issue_fix_pr_draft_create_writes_artifact() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-pr-draft",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 312
                }
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-draft/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Guard startup recovery config loading.",
                "root_cause": "Recovery skipped the nil-config fallback branch.",
                "fix_strategy": "restore the fallback guard and add a regression test",
                "changed_files": [
                    "crates/tandem-server/src/http/coder.rs",
                    "crates/tandem-server/src/http/tests/coder.rs"
                ],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "targeted issue-fix regression passed"
                }]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);

    let draft_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-draft/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "base_branch": "main"
            })
            .to_string(),
        ))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);
    let draft_payload: Value = serde_json::from_slice(
        &to_bytes(draft_resp.into_body(), usize::MAX)
            .await
            .expect("draft body"),
    )
    .expect("draft json");
    assert_eq!(
        draft_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_pr_draft")
    );
    assert_eq!(
        draft_payload
            .get("approval_required")
            .and_then(Value::as_bool),
        Some(true)
    );

    let artifact_path = draft_payload
        .get("artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .expect("draft artifact path");
    let artifact_payload: Value = serde_json::from_str(
        &tokio::fs::read_to_string(artifact_path)
            .await
            .expect("read draft artifact"),
    )
    .expect("parse draft artifact");
    assert_eq!(
        artifact_payload.get("title").and_then(Value::as_str),
        Some("Guard startup recovery config loading.")
    );
    assert!(artifact_payload
        .get("body")
        .and_then(Value::as_str)
        .is_some_and(|body| body.contains("Closes #312")));
    assert!(artifact_payload
        .get("body")
        .and_then(Value::as_str)
        .is_some_and(|body| body.contains("coder.rs")));
}

#[tokio::test]
async fn coder_issue_fix_pr_submit_dry_run_writes_submission_artifact() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-pr-submit",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 313
                }
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add missing fallback to startup recovery.",
                "root_cause": "Recovery skipped the nil-config guard.",
                "fix_strategy": "restore startup fallback and add a targeted regression",
                "changed_files": [
                    "crates/tandem-server/src/http/coder.rs"
                ],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup recovery regression passed"
                }]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);

    let draft_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit/pr-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "evan",
                "reason": "Looks good for a draft PR",
                "dry_run": true
            })
            .to_string(),
        ))
        .expect("submit request");
    let submit_resp = app
        .clone()
        .oneshot(submit_req)
        .await
        .expect("submit response");
    assert_eq!(submit_resp.status(), StatusCode::OK);
    let submit_payload: Value = serde_json::from_slice(
        &to_bytes(submit_resp.into_body(), usize::MAX)
            .await
            .expect("submit body"),
    )
    .expect("submit json");
    assert_eq!(
        submit_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_pr_submission")
    );
    assert_eq!(
        submit_payload.get("submitted").and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submit_payload.get("dry_run").and_then(Value::as_bool),
        Some(true)
    );
    assert!(submit_payload
        .get("submitted_github_ref")
        .is_some_and(Value::is_null));
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        submit_payload
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        submit_payload
            .get("artifact")
            .and_then(|row| row.get("path"))
            .and_then(Value::as_str)
            .and_then(|path| std::fs::read_to_string(path).ok())
            .and_then(|body| serde_json::from_str::<Value>(&body).ok())
            .and_then(|payload| payload.get("follow_on_runs").cloned())
            .and_then(|rows| rows.as_array().cloned())
            .map(|rows| rows.len()),
        Some(0)
    );
}

#[tokio::test]
async fn coder_issue_fix_pr_submit_real_submit_writes_canonical_pr_identity() {
    let (endpoint, server) = spawn_fake_github_mcp_server().await;

    let state = test_state().await;
    state
        .mcp
        .add_or_update(
            "github".to_string(),
            endpoint,
            std::collections::HashMap::new(),
            true,
        )
        .await;
    assert!(state.mcp.connect("github").await);
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-pr-submit-real",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 313
                },
                "mcp_servers": ["github"]
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-real/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add missing fallback to startup recovery.",
                "root_cause": "Recovery skipped the nil-config guard.",
                "fix_strategy": "restore startup fallback and add a targeted regression",
                "changed_files": [
                    "crates/tandem-server/src/http/coder.rs"
                ],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup recovery regression passed"
                }]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);

    let draft_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-real/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-real/pr-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "evan",
                "reason": "Ready to open the draft PR",
                "dry_run": false,
                "mcp_server": "github",
                "spawn_follow_on_runs": ["pr_review"]
            })
            .to_string(),
        ))
        .expect("submit request");
    let submit_resp = app
        .clone()
        .oneshot(submit_req)
        .await
        .expect("submit response");
    server.abort();

    assert_eq!(submit_resp.status(), StatusCode::OK);
    let submit_payload: Value = serde_json::from_slice(
        &to_bytes(submit_resp.into_body(), usize::MAX)
            .await
            .expect("submit body"),
    )
    .expect("submit json");
    assert_eq!(
        submit_payload.get("submitted").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        submit_payload
            .get("submitted_github_ref")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("pull_request")
    );
    assert_eq!(
        submit_payload
            .get("pull_request")
            .and_then(|row| row.get("number"))
            .and_then(Value::as_u64),
        Some(314)
    );
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
    );
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("parent_coder_run_id"))
            .and_then(Value::as_str),
        Some("coder-issue-fix-pr-submit-real")
    );
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("spawn_mode"))
            .and_then(Value::as_str),
        Some("template")
    );
    assert_eq!(
        submit_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.get(1))
            .and_then(|row| row.get("required_completed_workflow_modes"))
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["pr_review"])
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("parent_coder_run_id"))
            .and_then(Value::as_str),
        Some("coder-issue-fix-pr-submit-real")
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("origin"))
            .and_then(Value::as_str),
        Some("issue_fix_pr_submit_auto")
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("spawn_mode"))
            .and_then(Value::as_str),
        Some("auto")
    );
    assert_eq!(
        submit_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_pr_submission")
    );

    let artifact_path = submit_payload
        .get("artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .expect("submit artifact path");
    let artifact_payload: Value = serde_json::from_str(
        &tokio::fs::read_to_string(artifact_path)
            .await
            .expect("read submit artifact"),
    )
    .expect("parse submit artifact");
    assert_eq!(
        artifact_payload
            .get("submitted_github_ref")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("pull_request")
    );
    assert_eq!(
        artifact_payload
            .get("submitted_github_ref")
            .and_then(|row| row.get("number"))
            .and_then(Value::as_u64),
        Some(314)
    );
    assert_eq!(
        artifact_payload
            .get("pull_request")
            .and_then(|row| row.get("number"))
            .and_then(Value::as_u64),
        Some(314)
    );
    assert_eq!(
        artifact_payload.get("owner").and_then(Value::as_str),
        Some("evan")
    );
    assert_eq!(
        artifact_payload.get("repo").and_then(Value::as_str),
        Some("tandem")
    );
    assert_eq!(
        artifact_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
    );
    assert_eq!(
        artifact_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("spawn_mode"))
            .and_then(Value::as_str),
        Some("template")
    );
    assert_eq!(
        artifact_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        artifact_payload
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        artifact_payload
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );

    let follow_on_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-real/follow-on-run")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "pr_review",
                "coder_run_id": "coder-follow-on-pr-review"
            })
            .to_string(),
        ))
        .expect("follow-on request");
    let follow_on_resp = app
        .clone()
        .oneshot(follow_on_req)
        .await
        .expect("follow-on response");
    assert_eq!(follow_on_resp.status(), StatusCode::OK);
    let follow_on_payload: Value = serde_json::from_slice(
        &to_bytes(follow_on_resp.into_body(), usize::MAX)
            .await
            .expect("follow-on body"),
    )
    .expect("follow-on json");
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("github_ref"))
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("pull_request")
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("github_ref"))
            .and_then(|row| row.get("number"))
            .and_then(Value::as_u64),
        Some(314)
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("parent_coder_run_id"))
            .and_then(Value::as_str),
        Some("coder-issue-fix-pr-submit-real")
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("origin"))
            .and_then(Value::as_str),
        Some("issue_fix_pr_submit_manual_follow_on")
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("origin_artifact_type"))
            .and_then(Value::as_str),
        Some("coder_pr_submission")
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("spawn_mode"))
            .and_then(Value::as_str),
        Some("manual")
    );
    assert_eq!(
        follow_on_payload
            .get("coder_run")
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("required_completed_workflow_modes"))
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(Vec::<&str>::new())
    );

    let submitted_event = next_event_of_type(&mut rx, "coder.pr.submitted").await;
    assert_eq!(
        submitted_event
            .properties
            .get("submitted_github_ref")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("pull_request")
    );
    assert_eq!(
        submitted_event
            .properties
            .get("pull_request_number")
            .and_then(Value::as_u64),
        Some(314)
    );
    assert_eq!(
        submitted_event
            .properties
            .get("follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(2)
    );
    assert_eq!(
        submitted_event
            .properties
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        submitted_event
            .properties
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
}

#[tokio::test]
async fn coder_issue_fix_pr_submit_merge_auto_spawn_requires_opt_in() {
    let (endpoint, server) = spawn_fake_github_mcp_server().await;

    let state = test_state().await;
    state
        .mcp
        .add_or_update(
            "github".to_string(),
            endpoint,
            std::collections::HashMap::new(),
            true,
        )
        .await;
    assert!(state.mcp.connect("github").await);
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-pr-submit-merge-policy",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 313
                },
                "mcp_servers": ["github"]
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-merge-policy/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add missing fallback to startup recovery.",
                "root_cause": "Recovery skipped the nil-config guard.",
                "fix_strategy": "restore startup fallback and add a targeted regression",
                "changed_files": [
                    "crates/tandem-server/src/http/coder.rs"
                ],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup recovery regression passed"
                }]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);

    let draft_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-merge-policy/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-pr-submit-merge-policy/pr-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "evan",
                "reason": "Open the draft PR and queue review",
                "dry_run": false,
                "mcp_server": "github",
                "spawn_follow_on_runs": ["merge_recommendation"]
            })
            .to_string(),
        ))
        .expect("submit request");
    let submit_resp = app
        .clone()
        .oneshot(submit_req)
        .await
        .expect("submit response");
    server.abort();

    assert_eq!(submit_resp.status(), StatusCode::OK);
    let submit_payload: Value = serde_json::from_slice(
        &to_bytes(submit_resp.into_body(), usize::MAX)
            .await
            .expect("submit body"),
    )
    .expect("submit json");
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("origin"))
            .and_then(Value::as_str),
        Some("issue_fix_pr_submit_auto")
    );
    assert_eq!(
        submit_payload
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("coder_run"))
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("merge_auto_spawn_opted_in"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        submit_payload
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        submit_payload
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("merge_recommendation")
    );
    assert_eq!(
        submit_payload
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_explicit_auto_merge_recommendation_opt_in")
    );

    let submitted_event = next_event_of_type(&mut rx, "coder.pr.submitted").await;
    assert_eq!(
        submitted_event
            .properties
            .get("spawned_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
    assert_eq!(
        submitted_event
            .properties
            .get("skipped_follow_on_runs")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
}

#[tokio::test]
async fn coder_merge_follow_on_execution_waits_for_completed_review() {
    let (endpoint, server) = spawn_fake_github_mcp_server().await;

    let state = test_state().await;
    state
        .mcp
        .add_or_update(
            "github".to_string(),
            endpoint,
            std::collections::HashMap::new(),
            true,
        )
        .await;
    assert!(state.mcp.connect("github").await);
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-follow-on-policy-parent",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 313
                },
                "mcp_servers": ["github"]
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-policy-parent/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add missing fallback to startup recovery.",
                "root_cause": "Recovery skipped the nil-config guard.",
                "fix_strategy": "restore startup fallback and add a targeted regression",
                "changed_files": [
                    "crates/tandem-server/src/http/coder.rs"
                ],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup recovery regression passed"
                }]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);

    let draft_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-policy-parent/pr-draft")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("draft request");
    let draft_resp = app
        .clone()
        .oneshot(draft_req)
        .await
        .expect("draft response");
    assert_eq!(draft_resp.status(), StatusCode::OK);

    let submit_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-policy-parent/pr-submit")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "approved_by": "evan",
                "reason": "Open the draft PR",
                "dry_run": false,
                "mcp_server": "github"
            })
            .to_string(),
        ))
        .expect("submit request");
    let submit_resp = app
        .clone()
        .oneshot(submit_req)
        .await
        .expect("submit response");
    assert_eq!(submit_resp.status(), StatusCode::OK);

    let merge_follow_on_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-policy-parent/follow-on-run")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "merge_recommendation",
                "coder_run_id": "coder-follow-on-merge"
            })
            .to_string(),
        ))
        .expect("merge follow-on request");
    let merge_follow_on_resp = app
        .clone()
        .oneshot(merge_follow_on_req)
        .await
        .expect("merge follow-on response");
    assert_eq!(merge_follow_on_resp.status(), StatusCode::OK);

    let blocked_execute_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-merge/execute-next")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("blocked execute request");
    let blocked_execute_resp = app
        .clone()
        .oneshot(blocked_execute_req)
        .await
        .expect("blocked execute response");
    assert_eq!(blocked_execute_resp.status(), StatusCode::OK);
    let blocked_execute_payload: Value = serde_json::from_slice(
        &to_bytes(blocked_execute_resp.into_body(), usize::MAX)
            .await
            .expect("blocked execute body"),
    )
    .expect("blocked execute json");
    assert_eq!(
        blocked_execute_payload.get("code").and_then(Value::as_str),
        Some("CODER_EXECUTION_POLICY_BLOCKED")
    );
    assert_eq!(
        blocked_execute_payload
            .get("policy")
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_completed_pr_review_follow_on")
    );

    let merge_run_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-follow-on-merge")
        .body(Body::empty())
        .expect("merge get request");
    let merge_run_resp = app
        .clone()
        .oneshot(merge_run_req)
        .await
        .expect("merge get response");
    assert_eq!(merge_run_resp.status(), StatusCode::OK);
    let merge_run_payload: Value = serde_json::from_slice(
        &to_bytes(merge_run_resp.into_body(), usize::MAX)
            .await
            .expect("merge get body"),
    )
    .expect("merge get json");
    assert_eq!(
        merge_run_payload
            .get("coder_run")
            .and_then(|row| row.get("origin_policy"))
            .and_then(|row| row.get("required_completed_workflow_modes"))
            .and_then(Value::as_array)
            .map(|rows| rows.iter().filter_map(Value::as_str).collect::<Vec<_>>()),
        Some(vec!["pr_review"])
    );
    assert_eq!(
        merge_run_payload
            .get("execution_policy")
            .and_then(|row| row.get("blocked"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        merge_run_payload
            .get("execution_policy")
            .and_then(|row| row.get("policy"))
            .and_then(|row| row.get("reason"))
            .and_then(Value::as_str),
        Some("requires_completed_pr_review_follow_on")
    );

    let review_follow_on_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-policy-parent/follow-on-run")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "pr_review",
                "coder_run_id": "coder-follow-on-review"
            })
            .to_string(),
        ))
        .expect("review follow-on request");
    let review_follow_on_resp = app
        .clone()
        .oneshot(review_follow_on_req)
        .await
        .expect("review follow-on response");
    assert_eq!(review_follow_on_resp.status(), StatusCode::OK);

    let review_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-review/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "approve",
                "summary": "Looks good after targeted review.",
                "risk_level": "low",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "blockers": [],
                "requested_changes": []
            })
            .to_string(),
        ))
        .expect("review summary request");
    let review_summary_resp = app
        .clone()
        .oneshot(review_summary_req)
        .await
        .expect("review summary response");
    assert_eq!(review_summary_resp.status(), StatusCode::OK);

    let allowed_execute_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-follow-on-merge/execute-next")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("allowed execute request");
    let allowed_execute_resp = app
        .clone()
        .oneshot(allowed_execute_req)
        .await
        .expect("allowed execute response");
    server.abort();

    assert_eq!(allowed_execute_resp.status(), StatusCode::OK);
    let allowed_execute_payload: Value = serde_json::from_slice(
        &to_bytes(allowed_execute_resp.into_body(), usize::MAX)
            .await
            .expect("allowed execute body"),
    )
    .expect("allowed execute json");
    assert_eq!(
        allowed_execute_payload.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        allowed_execute_payload
            .get("dispatched")
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn coder_issue_fix_summary_writes_patch_summary_without_changed_files() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-diagnostic-summary",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "acme/platform"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 132
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-diagnostic-summary/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "root_cause": "The startup fallback branch was intentionally not patched because the incident was configuration-only.",
                "validation_steps": ["cargo test -p tandem-server coder_issue_fix_summary_writes_patch_summary_without_changed_files -- --test-threads=1"],
                "validation_results": [{
                    "kind": "diagnostic",
                    "status": "passed",
                    "summary": "Configuration-only recovery path validated without code changes"
                }],
                "memory_hits_used": ["memory-hit-fix-diagnostic-1"],
                "notes": "No-op fix summary for operator follow-up."
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    assert_eq!(
        summary_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_issue_fix_summary")
    );
    assert_eq!(
        summary_payload
            .get("validation_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_validation_report")
    );

    let blackboard = load_context_blackboard(&state, &linked_context_run_id);
    let patch_summary_path = blackboard
        .artifacts
        .iter()
        .find(|artifact| artifact.artifact_type == "coder_patch_summary")
        .map(|artifact| artifact.path.clone())
        .expect("patch summary path");
    let patch_summary_payload: Value = serde_json::from_str(
        &tokio::fs::read_to_string(&patch_summary_path)
            .await
            .expect("read patch summary artifact"),
    )
    .expect("parse patch summary artifact");
    assert_eq!(
        patch_summary_payload
            .get("root_cause")
            .and_then(Value::as_str),
        Some(
            "The startup fallback branch was intentionally not patched because the incident was configuration-only."
        )
    );
    assert_eq!(
        patch_summary_payload
            .get("changed_files")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(0)
    );
    assert_eq!(
        patch_summary_payload
            .get("validation_results")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(1)
    );
}

#[tokio::test]
async fn coder_issue_triage_prefers_failure_patterns_in_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-triage-a",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 65
                }
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let triage_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-triage-a/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Crash loop traces point at startup recovery.",
                "confidence": "medium"
            })
            .to_string(),
        ))
        .expect("triage summary request");
    let triage_summary_resp = app
        .clone()
        .oneshot(triage_summary_req)
        .await
        .expect("triage summary response");
    assert_eq!(triage_summary_resp.status(), StatusCode::OK);

    let failure_pattern_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-triage-a/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "task_id": "attempt_reproduction",
                "summary": "Crash loop consistently starts in startup recovery.",
                "payload": {
                    "workflow_mode": "issue_triage",
                    "summary": "Crash loop consistently starts in startup recovery.",
                    "fingerprint": "triage-startup-recovery-loop",
                    "canonical_markers": ["startup recovery", "crash loop"]
                }
            })
            .to_string(),
        ))
        .expect("failure pattern request");
    let failure_pattern_resp = app
        .clone()
        .oneshot(failure_pattern_req)
        .await
        .expect("failure pattern response");
    assert_eq!(failure_pattern_resp.status(), StatusCode::OK);

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-triage-b",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 65
                }
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-issue-triage-b/memory-hits")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("failure_pattern")
    );
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("triage_memory")
                && (row.get("source_coder_run_id").and_then(Value::as_str)
                    == Some("coder-issue-triage-a")
                    || row.get("run_id").and_then(Value::as_str) == Some("coder-issue-triage-a"))
        }))
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_issue_fix_reuses_prior_fix_pattern_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-a",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 79
                }
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-a/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add the missing startup fallback guard and cover it with a targeted regression test.",
                "root_cause": "Startup recovery skipped the nil-config fallback path.",
                "fix_strategy": "add startup fallback guard",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup recovery regression is now covered"
                }]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-b",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 79
                }
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-issue-fix-b/memory-hits")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    assert_eq!(
        hits_payload.get("query").and_then(Value::as_str),
        Some("evan/tandem issue #79")
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("fix_pattern")
    );
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("validation_memory")
                && (row.get("source_coder_run_id").and_then(Value::as_str)
                    == Some("coder-issue-fix-a")
                    || row.get("run_id").and_then(Value::as_str) == Some("coder-issue-fix-a"))
        }))
        .unwrap_or(false));
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("fix_pattern")
                && (row.get("source_coder_run_id").and_then(Value::as_str)
                    == Some("coder-issue-fix-a")
                    || row.get("run_id").and_then(Value::as_str) == Some("coder-issue-fix-a"))
        }))
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_pr_review_evidence_advances_review_run() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-pr-review-evidence",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 87
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let evidence_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-pr-review-evidence/pr-review-evidence")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "Inspection found a risky migration path and missing rollback test.",
                "risk_level": "high",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "blockers": ["Rollback test missing"],
                "requested_changes": ["Add rollback coverage"],
                "regression_signals": [{
                    "kind": "historical_failure_pattern",
                    "summary": "Migrations without rollback have failed before"
                }],
                "memory_hits_used": ["memory-hit-pr-evidence-1"],
                "notes": "Evidence recorded before final verdict summary."
            })
            .to_string(),
        ))
        .expect("evidence request");
    let evidence_resp = app
        .clone()
        .oneshot(evidence_req)
        .await
        .expect("evidence response");
    assert_eq!(evidence_resp.status(), StatusCode::OK);
    let evidence_payload: Value = serde_json::from_slice(
        &to_bytes(evidence_resp.into_body(), usize::MAX)
            .await
            .expect("evidence body"),
    )
    .expect("evidence json");
    assert_eq!(
        evidence_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_review_evidence")
    );
    assert_eq!(
        evidence_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    assert_eq!(
        evidence_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("artifact_write")
    );

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Running);
    for workflow_node_id in [
        "inspect_pull_request",
        "retrieve_memory",
        "review_pull_request",
    ] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done"
        );
    }
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("write_review_artifact"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Runnable)
    );
}

#[tokio::test]
async fn coder_pr_review_execute_next_drives_task_runtime_to_completion() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-pr-review-execute-next",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 200
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    for expected in [
        "inspect_pull_request",
        "review_pull_request",
        "write_review_artifact",
    ] {
        let execute_req = Request::builder()
            .method("POST")
            .uri("/coder/runs/coder-pr-review-execute-next/execute-next")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "agent_id": "coder_engine_worker_test"
                })
                .to_string(),
            ))
            .expect("execute request");
        let execute_resp = app
            .clone()
            .oneshot(execute_req)
            .await
            .expect("execute response");
        assert_eq!(execute_resp.status(), StatusCode::OK);
        let execute_payload: Value = serde_json::from_slice(
            &to_bytes(execute_resp.into_body(), usize::MAX)
                .await
                .expect("execute body"),
        )
        .expect("execute json");
        assert_eq!(
            execute_payload
                .get("task")
                .and_then(|row| row.get("workflow_node_id"))
                .and_then(Value::as_str),
            Some(expected)
        );
    }

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Completed);
    let blackboard = load_context_blackboard(&state, &linked_context_run_id);
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_pr_review_worker_session"));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_review_evidence"));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_pr_review_summary"));
    for workflow_node_id in [
        "inspect_pull_request",
        "retrieve_memory",
        "review_pull_request",
        "write_review_artifact",
    ] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done"
        );
    }
}

#[tokio::test]
async fn coder_pr_review_summary_create_writes_artifact_and_outcome() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-pr-review-summary",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 89,
                    "url": "https://github.com/evan/tandem/pull/89"
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-pr-review-summary/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "The PR introduces a migration risk and is missing rollback coverage.",
                "risk_level": "high",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "blockers": ["Missing rollback test"],
                "requested_changes": ["Add rollback coverage for the migration path"],
                "regression_signals": [{
                    "kind": "historical_failure_pattern",
                    "summary": "Similar rollout failed without rollback coverage"
                }],
                "memory_hits_used": ["memory-hit-1"],
                "notes": "Review memory suggests prior migration regressions."
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    assert_eq!(
        summary_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_pr_review_summary")
    );
    assert_eq!(
        summary_payload
            .get("review_evidence_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_review_evidence")
    );
    assert!(summary_payload
        .get("review_evidence_artifact")
        .and_then(|row| row.get("path"))
        .and_then(Value::as_str)
        .is_some_and(|path| path.ends_with(
            "/context_runs/ctx-coder-pr-review-summary/artifacts/pr_review.evidence.json"
        )));
    let summary_artifact_id = summary_payload
        .get("artifact")
        .and_then(|row| row.get("id"))
        .and_then(Value::as_str)
        .expect("summary artifact id")
        .to_string();
    let review_evidence_artifact_id = summary_payload
        .get("review_evidence_artifact")
        .and_then(|row| row.get("id"))
        .and_then(Value::as_str)
        .expect("review evidence artifact id")
        .to_string();
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| { row.get("kind").and_then(Value::as_str) == Some("review_memory") })),
        Some(true)
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().any(|row| {
                row.get("kind").and_then(Value::as_str) == Some("regression_signal")
            })),
        Some(true)
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| { row.get("kind").and_then(Value::as_str) == Some("run_outcome") })),
        Some(true)
    );

    let artifacts_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-summary/artifacts")
        .body(Body::empty())
        .expect("artifacts request");
    let artifacts_resp = app
        .clone()
        .oneshot(artifacts_req)
        .await
        .expect("artifacts response");
    assert_eq!(artifacts_resp.status(), StatusCode::OK);
    let artifacts_payload: Value = serde_json::from_slice(
        &to_bytes(artifacts_resp.into_body(), usize::MAX)
            .await
            .expect("artifacts body"),
    )
    .expect("artifacts json");
    assert!(artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("id").and_then(Value::as_str) == Some(summary_artifact_id.as_str())
                && row.get("artifact_type").and_then(Value::as_str)
                    == Some("coder_pr_review_summary")
        }))
        .unwrap_or(false));
    assert!(artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("id").and_then(Value::as_str) == Some(review_evidence_artifact_id.as_str())
                && row.get("artifact_type").and_then(Value::as_str) == Some("coder_review_evidence")
        }))
        .unwrap_or(false));

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-summary/memory-candidates")
        .body(Body::empty())
        .expect("candidates request");
    let candidates_resp = app
        .clone()
        .oneshot(candidates_req)
        .await
        .expect("candidates response");
    assert_eq!(candidates_resp.status(), StatusCode::OK);
    let candidates_payload: Value = serde_json::from_slice(
        &to_bytes(candidates_resp.into_body(), usize::MAX)
            .await
            .expect("candidates body"),
    )
    .expect("candidates json");
    assert!(candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("review_memory")
                && row
                    .get("payload")
                    .and_then(|payload| payload.get("review_evidence_artifact_path"))
                    .and_then(Value::as_str)
                    .is_some_and(|path| path.ends_with("/artifacts/pr_review.evidence.json"))
        }))
        .unwrap_or(false));

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.run_type, "coder_pr_review");
    assert_eq!(run.status, ContextRunStatus::Completed);
    let workflow_nodes = run
        .tasks
        .iter()
        .filter_map(|task| task.workflow_node_id.clone())
        .collect::<Vec<_>>();
    for workflow_node_id in [
        "inspect_pull_request",
        "retrieve_memory",
        "review_pull_request",
        "write_review_artifact",
    ] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done; saw workflow nodes: {workflow_nodes:?}"
        );
    }
}

#[tokio::test]
async fn coder_pr_review_reuses_prior_review_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_baseline_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-pr-review-baseline",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 90
                }
            })
            .to_string(),
        ))
        .expect("baseline create request");
    let create_baseline_resp = app
        .clone()
        .oneshot(create_baseline_req)
        .await
        .expect("baseline create response");
    assert_eq!(create_baseline_resp.status(), StatusCode::OK);

    let baseline_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-pr-review-baseline/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "comment",
                "summary": "Initial review requested one more pass before merge."
            })
            .to_string(),
        ))
        .expect("baseline summary request");
    let baseline_summary_resp = app
        .clone()
        .oneshot(baseline_summary_req)
        .await
        .expect("baseline summary response");
    assert_eq!(baseline_summary_resp.status(), StatusCode::OK);

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-pr-review-a",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 90
                }
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-pr-review-a/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "Previous review flagged missing rollback coverage.",
                "risk_level": "high",
                "requested_changes": ["Add rollback coverage"],
                "regression_signals": [{
                    "kind": "historical_failure_pattern",
                    "summary": "Rollback-free migrations regressed previously"
                }]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-pr-review-b",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 90
                }
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);
    let _create_second_payload: Value = serde_json::from_slice(
        &to_bytes(create_second_resp.into_body(), usize::MAX)
            .await
            .expect("second create body"),
    )
    .expect("second create json");

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-b")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_payload: Value = serde_json::from_slice(
        &to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body"),
    )
    .expect("get json");
    assert!(get_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
        }))
        .unwrap_or(false));

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-b/memory-hits")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    assert_eq!(
        hits_payload.get("query").and_then(Value::as_str),
        Some("evan/tandem pull request #90")
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("review_memory")
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("source_coder_run_id"))
            .and_then(Value::as_str)
            .or_else(|| {
                hits_payload
                    .get("hits")
                    .and_then(Value::as_array)
                    .and_then(|rows| rows.first())
                    .and_then(|row| row.get("run_id"))
                    .and_then(Value::as_str)
            }),
        Some("coder-pr-review-a")
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("same_ref"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(get_payload
        .get("memory_hits")
        .and_then(|row| row.get("hits"))
        .and_then(Value::as_array)
        .map(|rows| !rows.is_empty())
        .unwrap_or(false));
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("regression_signal")
                && (row.get("source_coder_run_id").and_then(Value::as_str)
                    == Some("coder-pr-review-a")
                    || row.get("run_id").and_then(Value::as_str) == Some("coder-pr-review-a"))
        }))
        .unwrap_or(false));
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("review_memory")
                && (row.get("source_coder_run_id").and_then(Value::as_str)
                    == Some("coder-pr-review-a")
                    || row.get("run_id").and_then(Value::as_str) == Some("coder-pr-review-a"))
        }))
        .unwrap_or(false));
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("source_coder_run_id").and_then(Value::as_str) == Some("coder-pr-review-a")
                || row.get("run_id").and_then(Value::as_str) == Some("coder-pr-review-a")
        }))
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_merge_recommendation_run_create_gets_seeded_tasks() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-recommendation-1",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 91
                }
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

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-merge-recommendation-1")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_payload: Value = serde_json::from_slice(
        &to_bytes(get_resp.into_body(), usize::MAX)
            .await
            .expect("get body"),
    )
    .expect("get json");
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("run_type"))
            .and_then(Value::as_str),
        Some("coder_merge_recommendation")
    );
    assert_eq!(
        get_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    let tasks = get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .cloned()
        .expect("tasks");
    assert_eq!(
        tasks
            .iter()
            .find(|row| row.get("workflow_node_id").and_then(Value::as_str)
                == Some("retrieve_memory"))
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("done")
    );
    assert_eq!(
        tasks
            .iter()
            .find(|row| row.get("workflow_node_id").and_then(Value::as_str)
                == Some("inspect_pull_request"))
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("runnable")
    );
    assert!(get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("workflow_node_id").and_then(Value::as_str) == Some("assess_merge_readiness")
        }))
        .unwrap_or(false));
    assert!(get_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
        }))
        .unwrap_or(false));
    assert_eq!(
        get_payload
            .get("memory_hits")
            .and_then(|row| row.get("query"))
            .and_then(Value::as_str),
        Some("evan/tandem pull request #91")
    );
}

#[tokio::test]
async fn coder_merge_readiness_report_advances_merge_run() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-readiness",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 93
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let readiness_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-readiness/merge-readiness-report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "The PR is close, but CODEOWNERS approval is still required.",
                "risk_level": "medium",
                "blockers": ["Required CODEOWNERS approval missing"],
                "required_checks": ["ci / test", "ci / lint"],
                "required_approvals": ["codeowners"],
                "memory_hits_used": ["memory-hit-merge-readiness-1"],
                "notes": "Readiness captured before final merge summary."
            })
            .to_string(),
        ))
        .expect("readiness request");
    let readiness_resp = app
        .clone()
        .oneshot(readiness_req)
        .await
        .expect("readiness response");
    assert_eq!(readiness_resp.status(), StatusCode::OK);
    let readiness_payload: Value = serde_json::from_slice(
        &to_bytes(readiness_resp.into_body(), usize::MAX)
            .await
            .expect("readiness body"),
    )
    .expect("readiness json");
    assert_eq!(
        readiness_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_merge_readiness_report")
    );
    assert_eq!(
        readiness_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    assert_eq!(
        readiness_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("artifact_write")
    );

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Running);
    for workflow_node_id in [
        "inspect_pull_request",
        "retrieve_memory",
        "assess_merge_readiness",
    ] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done"
        );
    }
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("write_merge_artifact"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Runnable)
    );
}

#[tokio::test]
async fn coder_merge_recommendation_execute_next_drives_task_runtime_to_completion() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-execute-next",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 201
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    for expected in [
        "inspect_pull_request",
        "assess_merge_readiness",
        "write_merge_artifact",
    ] {
        let execute_req = Request::builder()
            .method("POST")
            .uri("/coder/runs/coder-merge-execute-next/execute-next")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "agent_id": "coder_engine_worker_test"
                })
                .to_string(),
            ))
            .expect("execute request");
        let execute_resp = app
            .clone()
            .oneshot(execute_req)
            .await
            .expect("execute response");
        assert_eq!(execute_resp.status(), StatusCode::OK);
        let execute_payload: Value = serde_json::from_slice(
            &to_bytes(execute_resp.into_body(), usize::MAX)
                .await
                .expect("execute body"),
        )
        .expect("execute json");
        assert_eq!(
            execute_payload
                .get("task")
                .and_then(|row| row.get("workflow_node_id"))
                .and_then(Value::as_str),
            Some(expected)
        );
    }

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Completed);
    let blackboard = load_context_blackboard(&state, &linked_context_run_id);
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_merge_recommendation_worker_session"));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_merge_readiness_report"));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_merge_recommendation_summary"));
    for workflow_node_id in [
        "inspect_pull_request",
        "retrieve_memory",
        "assess_merge_readiness",
        "write_merge_artifact",
    ] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done"
        );
    }
}

#[tokio::test]
async fn coder_merge_recommendation_summary_create_writes_artifact() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-recommendation-summary",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 92
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-recommendation-summary/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "Checks are mostly green but one required approval is still missing.",
                "risk_level": "medium",
                "blockers": ["Required reviewer approval missing"],
                "required_checks": ["ci / test", "ci / lint"],
                "required_approvals": ["codeowners"],
                "memory_hits_used": ["memory-hit-merge-1"],
                "notes": "Wait for CODEOWNERS approval before merge."
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    assert_eq!(
        summary_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_merge_recommendation_summary")
    );
    assert_eq!(
        summary_payload
            .get("readiness_artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_merge_readiness_report")
    );
    assert_eq!(
        summary_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("completed")
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().any(|row| {
                row.get("kind").and_then(Value::as_str) == Some("merge_recommendation_memory")
            })),
        Some(true)
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows
                .iter()
                .any(|row| { row.get("kind").and_then(Value::as_str) == Some("run_outcome") })),
        Some(true)
    );
    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Completed);
    for workflow_node_id in [
        "inspect_pull_request",
        "retrieve_memory",
        "assess_merge_readiness",
        "write_merge_artifact",
    ] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done"
        );
    }
    let readiness_artifact_id = summary_payload
        .get("readiness_artifact")
        .and_then(|row| row.get("id"))
        .and_then(Value::as_str)
        .expect("readiness artifact id")
        .to_string();

    let artifacts_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-merge-recommendation-summary/artifacts")
        .body(Body::empty())
        .expect("artifacts request");
    let artifacts_resp = app
        .clone()
        .oneshot(artifacts_req)
        .await
        .expect("artifacts response");
    assert_eq!(artifacts_resp.status(), StatusCode::OK);
    let artifacts_payload: Value = serde_json::from_slice(
        &to_bytes(artifacts_resp.into_body(), usize::MAX)
            .await
            .expect("artifacts body"),
    )
    .expect("artifacts json");
    assert!(artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str)
                == Some("coder_merge_recommendation_summary")
        }))
        .unwrap_or(false));
    assert!(artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("id").and_then(Value::as_str) == Some(readiness_artifact_id.as_str())
                && row.get("artifact_type").and_then(Value::as_str)
                    == Some("coder_merge_readiness_report")
        }))
        .unwrap_or(false));

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-merge-recommendation-summary/memory-candidates")
        .body(Body::empty())
        .expect("candidates request");
    let candidates_resp = app
        .clone()
        .oneshot(candidates_req)
        .await
        .expect("candidates response");
    assert_eq!(candidates_resp.status(), StatusCode::OK);
    let candidates_payload: Value = serde_json::from_slice(
        &to_bytes(candidates_resp.into_body(), usize::MAX)
            .await
            .expect("candidates body"),
    )
    .expect("candidates json");
    assert!(candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("merge_recommendation_memory")
                && row
                    .get("payload")
                    .and_then(|payload| payload.get("readiness_artifact_path"))
                    .and_then(Value::as_str)
                    .is_some_and(|path| {
                        path.ends_with("/artifacts/merge_recommendation.readiness.json")
                    })
        }))
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_merge_recommendation_reuses_prior_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_baseline_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-recommendation-baseline",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 93
                }
            })
            .to_string(),
        ))
        .expect("baseline create request");
    let create_baseline_resp = app
        .clone()
        .oneshot(create_baseline_req)
        .await
        .expect("baseline create response");
    assert_eq!(create_baseline_resp.status(), StatusCode::OK);

    let baseline_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-recommendation-baseline/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "Hold merge pending final manual verification."
            })
            .to_string(),
        ))
        .expect("baseline summary request");
    let baseline_summary_resp = app
        .clone()
        .oneshot(baseline_summary_req)
        .await
        .expect("baseline summary response");
    assert_eq!(baseline_summary_resp.status(), StatusCode::OK);

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-recommendation-a",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 93
                }
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-recommendation-a/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "Hold merge until the final approval lands and the rollout note is attached.",
                "risk_level": "medium",
                "blockers": ["Required reviewer approval missing"],
                "required_checks": ["ci / test"],
                "required_approvals": ["codeowners"],
                "memory_hits_used": ["memory-hit-merge-a"]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-recommendation-b",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 93
                }
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-merge-recommendation-b/memory-hits")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    assert_eq!(
        hits_payload.get("query").and_then(Value::as_str),
        Some("evan/tandem pull request #93")
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("merge_recommendation_memory")
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("source_coder_run_id"))
            .and_then(Value::as_str)
            .or_else(|| {
                hits_payload
                    .get("hits")
                    .and_then(Value::as_array)
                    .and_then(|rows| rows.first())
                    .and_then(|row| row.get("run_id"))
                    .and_then(Value::as_str)
            }),
        Some("coder-merge-recommendation-a")
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("same_ref"))
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("kind").and_then(Value::as_str) == Some("merge_recommendation_memory")
                && (row.get("source_coder_run_id").and_then(Value::as_str)
                    == Some("coder-merge-recommendation-a")
                    || row.get("run_id").and_then(Value::as_str)
                        == Some("coder-merge-recommendation-a"))
        }))
        .unwrap_or(false));
}

#[tokio::test]
async fn coder_run_approve_and_cancel_project_context_run_controls() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-controls",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 15
                }
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
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run")
        .to_string();

    let plan_req = Request::builder()
        .method("POST")
        .uri(format!("/context/runs/{linked_context_run_id}/events"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "type": "planning_started",
                "status": "awaiting_approval",
                "payload": {}
            })
            .to_string(),
        ))
        .expect("plan request");
    let plan_resp = app.clone().oneshot(plan_req).await.expect("plan response");
    assert_eq!(plan_resp.status(), StatusCode::OK);

    let approve_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-controls/approve")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "approve coder plan"
            })
            .to_string(),
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
        approve_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("bootstrapping")
    );
    assert_eq!(
        approve_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );

    let cancel_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-controls/cancel")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "stop this coder run"
            })
            .to_string(),
        ))
        .expect("cancel request");
    let cancel_resp = app
        .clone()
        .oneshot(cancel_req)
        .await
        .expect("cancel response");
    assert_eq!(cancel_resp.status(), StatusCode::OK);
    let cancel_body = to_bytes(cancel_resp.into_body(), usize::MAX)
        .await
        .expect("cancel body");
    let cancel_payload: Value = serde_json::from_slice(&cancel_body).expect("cancel json");
    assert_eq!(
        cancel_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("cancelled")
    );
    assert_eq!(
        cancel_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("cancelled")
    );
}

#[tokio::test]
async fn coder_issue_triage_run_replay_matches_persisted_state_and_checkpoint() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-replay",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 404,
                    "url": "https://github.com/evan/tandem/issues/404"
                }
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
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let checkpoint_req = Request::builder()
        .method("POST")
        .uri(format!("/context/runs/{linked_context_run_id}/checkpoints"))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "reason": "coder_replay_regression"
            })
            .to_string(),
        ))
        .expect("checkpoint request");
    let checkpoint_resp = app
        .clone()
        .oneshot(checkpoint_req)
        .await
        .expect("checkpoint response");
    assert_eq!(checkpoint_resp.status(), StatusCode::OK);
    let checkpoint_body = to_bytes(checkpoint_resp.into_body(), usize::MAX)
        .await
        .expect("checkpoint body");
    let checkpoint_payload: Value =
        serde_json::from_slice(&checkpoint_body).expect("checkpoint json");
    assert_eq!(
        checkpoint_payload
            .get("checkpoint")
            .and_then(|row| row.get("run_id"))
            .and_then(Value::as_str),
        Some(linked_context_run_id.as_str())
    );

    let replay_req = Request::builder()
        .method("GET")
        .uri(format!("/context/runs/{linked_context_run_id}/replay"))
        .body(Body::empty())
        .expect("replay request");
    let replay_resp = app
        .clone()
        .oneshot(replay_req)
        .await
        .expect("replay response");
    assert_eq!(replay_resp.status(), StatusCode::OK);
    let replay_body = to_bytes(replay_resp.into_body(), usize::MAX)
        .await
        .expect("replay body");
    let replay_payload: Value = serde_json::from_slice(&replay_body).expect("replay json");

    assert_eq!(
        replay_payload
            .get("drift")
            .and_then(|row| row.get("mismatch"))
            .and_then(Value::as_bool),
        Some(false)
    );
    assert_eq!(
        replay_payload
            .get("from_checkpoint")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        replay_payload
            .get("replay")
            .and_then(|row| row.get("run_type"))
            .and_then(Value::as_str),
        Some("coder_issue_triage")
    );
    assert_eq!(
        replay_payload
            .get("replay_blackboard")
            .and_then(|row| row.get("artifacts"))
            .and_then(Value::as_array)
            .map(|rows| rows.iter().any(|row| {
                row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
            })),
        Some(true)
    );
    assert_eq!(
        replay_payload
            .get("replay_blackboard")
            .and_then(|row| row.get("tasks"))
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(5)
    );
}

#[tokio::test]
async fn coder_artifacts_endpoint_projects_context_blackboard_artifacts() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-2",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 9
                }
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

    let artifacts_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-2/artifacts")
        .body(Body::empty())
        .expect("artifacts request");
    let artifacts_resp = app
        .clone()
        .oneshot(artifacts_req)
        .await
        .expect("artifacts response");
    assert_eq!(artifacts_resp.status(), StatusCode::OK);
    let artifacts_body = to_bytes(artifacts_resp.into_body(), usize::MAX)
        .await
        .expect("artifacts body");
    let artifacts_payload: Value = serde_json::from_slice(&artifacts_body).expect("artifacts json");
    let contains_memory_hits = artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
            })
        })
        .unwrap_or(false);
    assert!(contains_memory_hits);
}

#[tokio::test]
async fn coder_issue_triage_blocks_when_preferred_mcp_server_is_missing() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state);

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 42
                },
                "mcp_servers": ["missing-github"]
            })
            .to_string(),
        ))
        .expect("create request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create response");
    assert_eq!(create_resp.status(), StatusCode::CONFLICT);
    let create_body = to_bytes(create_resp.into_body(), usize::MAX)
        .await
        .expect("create body");
    let create_payload: Value = serde_json::from_slice(&create_body).expect("create json");
    assert_eq!(
        create_payload.get("code").and_then(Value::as_str),
        Some("CODER_READINESS_BLOCKED")
    );
}

#[tokio::test]
async fn coder_memory_candidate_create_persists_artifact() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-3",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 77
                }
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

    let candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-3/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "triage_memory",
                "summary": "Likely duplicate failure",
                "payload": {
                    "confidence": "medium"
                }
            })
            .to_string(),
        ))
        .expect("candidate request");
    let candidate_resp = app
        .clone()
        .oneshot(candidate_req)
        .await
        .expect("candidate response");
    assert_eq!(candidate_resp.status(), StatusCode::OK);

    let artifacts_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-3/artifacts")
        .body(Body::empty())
        .expect("artifacts request");
    let artifacts_resp = app
        .clone()
        .oneshot(artifacts_req)
        .await
        .expect("artifacts response");
    assert_eq!(artifacts_resp.status(), StatusCode::OK);
    let artifacts_body = to_bytes(artifacts_resp.into_body(), usize::MAX)
        .await
        .expect("artifacts body");
    let artifacts_payload: Value = serde_json::from_slice(&artifacts_body).expect("artifacts json");
    let contains_candidate = artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_candidate")
            })
        })
        .unwrap_or(false);
    assert!(contains_candidate);
}

#[tokio::test]
async fn coder_issue_triage_seeds_ranked_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let first_run_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-seed-a",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 88
                }
            })
            .to_string(),
        ))
        .expect("first run request");
    let first_run_resp = app
        .clone()
        .oneshot(first_run_req)
        .await
        .expect("first run response");
    assert_eq!(first_run_resp.status(), StatusCode::OK);

    let candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-seed-a/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "summary": "Known duplicate failure",
                "payload": {
                    "label": "duplicate"
                }
            })
            .to_string(),
        ))
        .expect("candidate request");
    let candidate_resp = app
        .clone()
        .oneshot(candidate_req)
        .await
        .expect("candidate response");
    assert_eq!(candidate_resp.status(), StatusCode::OK);

    let second_run_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-seed-b",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 88
                }
            })
            .to_string(),
        ))
        .expect("second run request");
    let second_run_resp = app
        .clone()
        .oneshot(second_run_req)
        .await
        .expect("second run response");
    assert_eq!(second_run_resp.status(), StatusCode::OK);

    let get_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-seed-b")
        .body(Body::empty())
        .expect("get request");
    let get_resp = app.clone().oneshot(get_req).await.expect("get response");
    assert_eq!(get_resp.status(), StatusCode::OK);
    let get_body = to_bytes(get_resp.into_body(), usize::MAX)
        .await
        .expect("get body");
    let get_payload: Value = serde_json::from_slice(&get_body).expect("get json");
    let retrieve_task = get_payload
        .get("run")
        .and_then(|row| row.get("tasks"))
        .and_then(Value::as_array)
        .and_then(|tasks| {
            tasks.iter().find(|task| {
                task.get("workflow_node_id").and_then(Value::as_str) == Some("retrieve_memory")
            })
        })
        .cloned()
        .expect("retrieve task");
    let hint_count = retrieve_task
        .get("payload")
        .and_then(|row| row.get("memory_hits"))
        .and_then(Value::as_array)
        .map(|rows| rows.len())
        .unwrap_or(0);
    assert!(hint_count >= 1);
}

#[tokio::test]
async fn coder_triage_reproduction_report_advances_triage_run() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-repro",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 96
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let repro_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-triage-repro/triage-reproduction-report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Reproduced the capability-readiness issue when bindings are missing.",
                "outcome": "reproduced",
                "steps": [
                    "Disconnect GitHub MCP bindings",
                    "Create issue_triage coder run"
                ],
                "observed_logs": [
                    "capabilities readiness failed closed"
                ],
                "affected_files": ["crates/tandem-server/src/http/coder.rs"],
                "memory_hits_used": ["memory-hit-triage-repro-1"]
            })
            .to_string(),
        ))
        .expect("repro request");
    let repro_resp = app
        .clone()
        .oneshot(repro_req)
        .await
        .expect("repro response");
    assert_eq!(repro_resp.status(), StatusCode::OK);
    let repro_payload: Value = serde_json::from_slice(
        &to_bytes(repro_resp.into_body(), usize::MAX)
            .await
            .expect("repro body"),
    )
    .expect("repro json");
    assert_eq!(
        repro_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_reproduction_report")
    );
    assert_eq!(
        repro_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    assert_eq!(
        repro_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("artifact_write")
    );

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Running);
    for workflow_node_id in ["inspect_repo", "attempt_reproduction"] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done"
        );
    }
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("write_triage_artifact"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Runnable)
    );
}

#[tokio::test]
async fn coder_triage_inspection_report_advances_to_reproduction() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-inspection",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 97
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let inspection_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-triage-inspection/triage-inspection-report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "The repo inspection points at capability readiness and MCP binding setup.",
                "likely_areas": ["capability resolver", "github readiness"],
                "affected_files": ["crates/tandem-server/src/http/coder.rs"],
                "memory_hits_used": ["memory-hit-triage-inspection-1"],
                "notes": "Inspection completed before reproduction."
            })
            .to_string(),
        ))
        .expect("inspection request");
    let inspection_resp = app
        .clone()
        .oneshot(inspection_req)
        .await
        .expect("inspection response");
    assert_eq!(inspection_resp.status(), StatusCode::OK);
    let inspection_payload: Value = serde_json::from_slice(
        &to_bytes(inspection_resp.into_body(), usize::MAX)
            .await
            .expect("inspection body"),
    )
    .expect("inspection json");
    assert_eq!(
        inspection_payload
            .get("artifact")
            .and_then(|row| row.get("artifact_type"))
            .and_then(Value::as_str),
        Some("coder_repo_inspection_report")
    );
    assert_eq!(
        inspection_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("running")
    );
    assert_eq!(
        inspection_payload
            .get("coder_run")
            .and_then(|row| row.get("phase"))
            .and_then(Value::as_str),
        Some("reproduction")
    );

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Running);
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("inspect_repo"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Done)
    );
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("attempt_reproduction"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Runnable)
    );
}

#[tokio::test]
async fn coder_issue_triage_execute_next_drives_task_runtime_to_completion() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-execute-next",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 198
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    for expected in [
        "inspect_repo",
        "attempt_reproduction",
        "write_triage_artifact",
    ] {
        let execute_req = Request::builder()
            .method("POST")
            .uri("/coder/runs/coder-triage-execute-next/execute-next")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "agent_id": "coder_engine_worker_test"
                })
                .to_string(),
            ))
            .expect("execute request");
        let execute_resp = app
            .clone()
            .oneshot(execute_req)
            .await
            .expect("execute response");
        assert_eq!(execute_resp.status(), StatusCode::OK);
        let execute_payload: Value = serde_json::from_slice(
            &to_bytes(execute_resp.into_body(), usize::MAX)
                .await
                .expect("execute body"),
        )
        .expect("execute json");
        assert_eq!(
            execute_payload
                .get("task")
                .and_then(|row| row.get("workflow_node_id"))
                .and_then(Value::as_str),
            Some(expected)
        );
    }

    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Completed);
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("inspect_repo"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Done)
    );
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("attempt_reproduction"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Done)
    );
    assert_eq!(
        run.tasks
            .iter()
            .find(|task| task.workflow_node_id.as_deref() == Some("write_triage_artifact"))
            .map(|task| &task.status),
        Some(&ContextBlackboardTaskStatus::Done)
    );
}

#[tokio::test]
async fn coder_memory_hits_endpoint_returns_ranked_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-hits-a",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 95
                }
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

    let candidate_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-hits-a/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "triage_memory",
                "summary": "Repeated issue near capability readiness",
                "payload": {
                    "tag": "known"
                }
            })
            .to_string(),
        ))
        .expect("candidate request");
    let candidate_resp = app
        .clone()
        .oneshot(candidate_req)
        .await
        .expect("candidate response");
    assert_eq!(candidate_resp.status(), StatusCode::OK);

    let failure_pattern_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-hits-a/memory-candidates")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "kind": "failure_pattern",
                "summary": "Capability readiness drift repeatedly blocks issue triage startup.",
                "payload": {
                    "type": "historical_failure_pattern",
                    "root_cause": "GitHub capability bindings were missing during run bootstrap."
                }
            })
            .to_string(),
        ))
        .expect("failure pattern request");
    let failure_pattern_resp = app
        .clone()
        .oneshot(failure_pattern_req)
        .await
        .expect("failure pattern response");
    assert_eq!(failure_pattern_resp.status(), StatusCode::OK);

    let second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-hits-b",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 95
                }
            })
            .to_string(),
        ))
        .expect("second request");
    let second_resp = app
        .clone()
        .oneshot(second_req)
        .await
        .expect("second response");
    assert_eq!(second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-hits-b/memory-hits")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_body = to_bytes(hits_resp.into_body(), usize::MAX)
        .await
        .expect("hits body");
    let hits_payload: Value = serde_json::from_slice(&hits_body).expect("hits json");
    assert!(hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| !rows.is_empty())
        .unwrap_or(false));
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("failure_pattern")
    );
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("same_ref"))
            .and_then(Value::as_bool),
        Some(true)
    );
}

#[tokio::test]
async fn coder_issue_triage_retrieves_governed_memory_hits() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let db = super::super::skills_memory::open_global_memory_db()
        .await
        .expect("global memory db");
    db.put_global_memory_record(&GlobalMemoryRecord {
        id: "memory-governed-1".to_string(),
        user_id: "desktop_developer_mode".to_string(),
        source_type: "solution_capsule".to_string(),
        content: "Past triage found capability readiness drift in coder issue triage setup"
            .to_string(),
        content_hash: String::new(),
        run_id: "memory-run-1".to_string(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: Some("proj-engine".to_string()),
        channel_tag: None,
        host_tag: None,
        metadata: Some(json!({
            "kind": "triage_memory"
        })),
        provenance: Some(json!({
            "origin_event_type": "memory.put"
        })),
        redaction_status: "passed".to_string(),
        redaction_count: 0,
        visibility: "private".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        expires_at_ms: None,
    })
    .await
    .expect("seed governed memory");

    let app = app_router(state.clone());
    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-governed-hits",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 202
                },
                "source_client": "desktop_developer_mode"
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

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-governed-hits/memory-hits?q=capability%20readiness")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_body = to_bytes(hits_resp.into_body(), usize::MAX)
        .await
        .expect("hits body");
    let hits_payload: Value = serde_json::from_slice(&hits_body).expect("hits json");
    let has_governed_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("source").and_then(Value::as_str) == Some("governed_memory")
                    && row.get("memory_id").and_then(Value::as_str) == Some("memory-governed-1")
            })
        })
        .unwrap_or(false);
    assert!(has_governed_hit);
}

#[tokio::test]
async fn coder_triage_summary_write_adds_summary_artifact() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-summary",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 91
                }
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
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create body"),
    )
    .expect("create json");
    let linked_context_run_id = create_payload
        .get("coder_run")
        .and_then(|row| row.get("linked_context_run_id"))
        .and_then(Value::as_str)
        .expect("linked context run id")
        .to_string();

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-summary/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Likely duplicate in capabilities flow",
                "confidence": "medium",
                "affected_files": ["crates/tandem-server/src/http/coder.rs"],
                "memory_hits_used": ["memcand-1"]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_body = to_bytes(summary_resp.into_body(), usize::MAX)
        .await
        .expect("summary body");
    let summary_payload: Value = serde_json::from_slice(&summary_body).expect("summary json");
    let generated_candidates = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert!(generated_candidates
        .iter()
        .any(|row| { row.get("kind").and_then(Value::as_str) == Some("triage_memory") }));
    assert!(generated_candidates
        .iter()
        .any(|row| { row.get("kind").and_then(Value::as_str) == Some("failure_pattern") }));
    assert!(generated_candidates
        .iter()
        .any(|row| { row.get("kind").and_then(Value::as_str) == Some("run_outcome") }));

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-summary/memory-candidates")
        .body(Body::empty())
        .expect("candidates request");
    let candidates_resp = app
        .clone()
        .oneshot(candidates_req)
        .await
        .expect("candidates response");
    assert_eq!(candidates_resp.status(), StatusCode::OK);
    let candidates_body = to_bytes(candidates_resp.into_body(), usize::MAX)
        .await
        .expect("candidates body");
    let candidates_payload: Value =
        serde_json::from_slice(&candidates_body).expect("candidates json");
    let failure_pattern_payload = candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("failure_pattern"))
        })
        .and_then(|row| row.get("payload"))
        .cloned()
        .expect("failure pattern payload");
    assert_eq!(
        failure_pattern_payload.get("type").and_then(Value::as_str),
        Some("failure.pattern")
    );
    assert_eq!(
        failure_pattern_payload
            .get("linked_issue_numbers")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(Value::as_u64),
        Some(91)
    );
    assert_eq!(
        summary_payload
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.len()),
        Some(3)
    );
    assert_eq!(
        summary_payload
            .get("run")
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("completed")
    );

    let artifacts_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-summary/artifacts")
        .body(Body::empty())
        .expect("artifacts request");
    let artifacts_resp = app
        .clone()
        .oneshot(artifacts_req)
        .await
        .expect("artifacts response");
    assert_eq!(artifacts_resp.status(), StatusCode::OK);
    let artifacts_body = to_bytes(artifacts_resp.into_body(), usize::MAX)
        .await
        .expect("artifacts body");
    let artifacts_payload: Value = serde_json::from_slice(&artifacts_body).expect("artifacts json");
    let contains_summary = artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("artifact_type").and_then(Value::as_str) == Some("coder_triage_summary")
            })
        })
        .unwrap_or(false);
    assert!(contains_summary);

    let contains_memory_hits = artifacts_payload
        .get("artifacts")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
            })
        })
        .unwrap_or(false);
    assert!(contains_memory_hits);

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-summary/memory-candidates")
        .body(Body::empty())
        .expect("candidates request");
    let candidates_resp = app
        .clone()
        .oneshot(candidates_req)
        .await
        .expect("candidates response");
    assert_eq!(candidates_resp.status(), StatusCode::OK);
    let candidates_body = to_bytes(candidates_resp.into_body(), usize::MAX)
        .await
        .expect("candidates body");
    let candidates_payload: Value =
        serde_json::from_slice(&candidates_body).expect("candidates json");
    let kinds = candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row.get("kind").and_then(Value::as_str))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    assert!(kinds.contains(&"triage_memory"));
    assert!(kinds.contains(&"run_outcome"));
    let run = load_context_run_state(&state, &linked_context_run_id)
        .await
        .expect("context run state");
    assert_eq!(run.status, ContextRunStatus::Completed);
    let blackboard = load_context_blackboard(&state, &linked_context_run_id);
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_issue_triage_worker_session"));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_repo_inspection_report"));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_reproduction_report"));
    assert!(blackboard
        .artifacts
        .iter()
        .any(|artifact| artifact.artifact_type == "coder_triage_summary"));
    for workflow_node_id in [
        "ingest_reference",
        "retrieve_memory",
        "inspect_repo",
        "attempt_reproduction",
        "write_triage_artifact",
    ] {
        assert_eq!(
            run.tasks
                .iter()
                .find(|task| task.workflow_node_id.as_deref() == Some(workflow_node_id))
                .map(|task| &task.status),
            Some(&ContextBlackboardTaskStatus::Done),
            "expected {workflow_node_id} to be done"
        );
    }
}

#[tokio::test]
async fn coder_memory_candidate_promote_stores_governed_memory() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-promote",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 333
                },
                "source_client": "desktop_developer_mode"
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-promote/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Capability readiness drift already explained this failure",
                "confidence": "high"
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_body = to_bytes(summary_resp.into_body(), usize::MAX)
        .await
        .expect("summary body");
    let summary_payload: Value = serde_json::from_slice(&summary_body).expect("summary json");
    let triage_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("triage_memory")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("triage candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-run-promote/memory-candidates/{triage_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable triage memory"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_body = to_bytes(promote_resp.into_body(), usize::MAX)
        .await
        .expect("promote body");
    let promote_payload: Value = serde_json::from_slice(&promote_body).expect("promote json");
    assert_eq!(
        promote_payload.get("promoted").and_then(Value::as_bool),
        Some(true)
    );

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-promote/memory-hits?q=capability%20readiness")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_body = to_bytes(hits_resp.into_body(), usize::MAX)
        .await
        .expect("hits body");
    let hits_payload: Value = serde_json::from_slice(&hits_body).expect("hits json");
    let has_promoted_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("source").and_then(Value::as_str) == Some("governed_memory")
                    && row.get("memory_id").and_then(Value::as_str)
                        == promote_payload.get("memory_id").and_then(Value::as_str)
            })
        })
        .unwrap_or(false);
    assert!(has_promoted_hit);

    let create_fix_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-promote-fix",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 334
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("create fix request");
    let create_fix_resp = app
        .clone()
        .oneshot(create_fix_req)
        .await
        .expect("create fix response");
    assert_eq!(create_fix_resp.status(), StatusCode::OK);

    let fix_summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-promote-fix/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add the missing startup fallback guard and validate recovery behavior.",
                "root_cause": "Startup recovery skipped the nil-config fallback path.",
                "fix_strategy": "add startup fallback guard",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"]
            })
            .to_string(),
        ))
        .expect("fix summary request");
    let fix_summary_resp = app
        .clone()
        .oneshot(fix_summary_req)
        .await
        .expect("fix summary response");
    assert_eq!(fix_summary_resp.status(), StatusCode::OK);
    let fix_summary_payload: Value = serde_json::from_slice(
        &to_bytes(fix_summary_resp.into_body(), usize::MAX)
            .await
            .expect("fix summary body"),
    )
    .expect("fix summary json");
    let fix_pattern_candidate_id = fix_summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("fix_pattern")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("fix pattern candidate id");

    let promote_fix_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-run-promote-fix/memory-candidates/{fix_pattern_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable fix pattern"
            })
            .to_string(),
        ))
        .expect("promote fix request");
    let promote_fix_resp = app
        .clone()
        .oneshot(promote_fix_req)
        .await
        .expect("promote fix response");
    assert_eq!(promote_fix_resp.status(), StatusCode::OK);
    let promote_fix_payload: Value = serde_json::from_slice(
        &to_bytes(promote_fix_resp.into_body(), usize::MAX)
            .await
            .expect("promote fix body"),
    )
    .expect("promote fix json");
    assert_eq!(
        promote_fix_payload.get("promoted").and_then(Value::as_bool),
        Some(true)
    );

    let fix_hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-run-promote-fix/memory-hits?q=startup%20fallback%20guard")
        .body(Body::empty())
        .expect("fix hits request");
    let fix_hits_resp = app
        .clone()
        .oneshot(fix_hits_req)
        .await
        .expect("fix hits response");
    assert_eq!(fix_hits_resp.status(), StatusCode::OK);
    let fix_hits_payload: Value = serde_json::from_slice(
        &to_bytes(fix_hits_resp.into_body(), usize::MAX)
            .await
            .expect("fix hits body"),
    )
    .expect("fix hits json");
    let has_promoted_fix_hit = fix_hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter().any(|row| {
                row.get("source").and_then(Value::as_str) == Some("governed_memory")
                    && row.get("memory_id").and_then(Value::as_str)
                        == promote_fix_payload.get("memory_id").and_then(Value::as_str)
                    && row
                        .get("metadata")
                        .and_then(|metadata| metadata.get("kind"))
                        .and_then(Value::as_str)
                        == Some("fix_pattern")
            })
        })
        .unwrap_or(false);
    assert!(has_promoted_fix_hit);

    let db = super::super::skills_memory::open_global_memory_db()
        .await
        .expect("global memory db");
    let promoted_fix_record = db
        .get_global_memory(
            promote_fix_payload
                .get("memory_id")
                .and_then(Value::as_str)
                .expect("fix memory id"),
        )
        .await
        .expect("load fix governed memory")
        .expect("fix governed memory record");
    assert_eq!(promoted_fix_record.source_type, "solution_capsule");
    assert_eq!(
        promoted_fix_record.project_tag.as_deref(),
        Some("proj-engine")
    );
    assert!(promoted_fix_record.content.contains("workflow: issue_fix"));
    assert!(promoted_fix_record
        .content
        .contains("fix_strategy: add startup fallback guard"));
    assert!(promoted_fix_record
        .content
        .contains("root_cause: Startup recovery skipped the nil-config fallback path."));
    assert_eq!(
        promoted_fix_record
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("kind"))
            .and_then(Value::as_str),
        Some("fix_pattern")
    );
    assert_eq!(
        promoted_fix_record
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("issue_fix")
    );
    assert_eq!(
        promoted_fix_record
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("candidate_id"))
            .and_then(Value::as_str),
        Some(fix_pattern_candidate_id.as_str())
    );
}

#[tokio::test]
async fn coder_promoted_merge_memory_reuses_policy_history_across_pull_requests() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-promote-a",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 101
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-promote-a/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "Hold merge until ci / test passes and codeowners approval lands.",
                "risk_level": "medium",
                "blockers": ["Required reviewer approval missing"],
                "required_checks": ["ci / test"],
                "required_approvals": ["codeowners"]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let merge_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("merge_recommendation_memory"))
                    .then(|| {
                        row.get("candidate_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    })?
            })
        })
        .expect("merge candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-merge-promote-a/memory-candidates/{merge_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable merge policy memory"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-promote-b",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 102
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-merge-promote-b/memory-hits?q=codeowners%20ci%20%2F%20test%20approval")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    let promoted_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("source").and_then(Value::as_str) == Some("governed_memory")
                    && row.get("memory_id").and_then(Value::as_str)
                        == promote_payload.get("memory_id").and_then(Value::as_str)
            })
        })
        .cloned()
        .expect("promoted merge hit");
    assert_eq!(
        hits_payload
            .get("hits")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("memory_id"))
            .and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(promoted_hit.get("same_ref").and_then(Value::as_bool), None);
    assert_eq!(
        promoted_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("merge_recommendation_memory")
    );
    assert!(promoted_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("required_checks: ci / test")));
    assert!(promoted_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("required_approvals: codeowners")));
    assert!(promoted_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("blockers: Required reviewer approval missing")));
}

#[tokio::test]
async fn coder_promoted_merge_outcome_reuses_across_pull_requests() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-outcome-promote-a",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 111
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-merge-outcome-promote-a/merge-recommendation-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "recommendation": "hold",
                "summary": "Merge should wait until rollout notes are attached and post-deploy verification is ready.",
                "risk_level": "medium"
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let run_outcome_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("run_outcome")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("run outcome candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-merge-outcome-promote-a/memory-candidates/{run_outcome_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable merge outcome"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-merge-outcome-promote-b",
                "workflow_mode": "merge_recommendation",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 112
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-merge-outcome-promote-b/memory-hits?q=merge%20should%20wait%20until%20rollout%20notes%20are%20attached")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    let first_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first hit");
    assert_eq!(
        first_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("run_outcome")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("merge_recommendation")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
}

#[tokio::test]
async fn coder_promoted_review_memory_reuses_requested_changes_across_pull_requests() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-review-promote-a",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 111
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-review-promote-a/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "Require rollback coverage before approval.",
                "risk_level": "high",
                "blockers": ["Rollback scenario coverage missing"],
                "requested_changes": ["Add rollback coverage"],
                "changed_files": ["crates/tandem-server/src/http/coder.rs"]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let review_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("review_memory")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("review candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-review-promote-a/memory-candidates/{review_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable review guidance"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-review-promote-b",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 112
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-review-promote-b/memory-hits?q=rollback%20coverage")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    let promoted_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find(|row| {
                row.get("source").and_then(Value::as_str) == Some("governed_memory")
                    && row.get("memory_id").and_then(Value::as_str)
                        == promote_payload.get("memory_id").and_then(Value::as_str)
            })
        })
        .cloned()
        .expect("promoted review hit");
    assert_eq!(
        promoted_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("review_memory")
    );
    assert!(promoted_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("requested_changes: Add rollback coverage")));
    assert!(promoted_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("blockers: Rollback scenario coverage missing")));
}

#[tokio::test]
async fn coder_promoted_regression_signal_reuses_across_pull_requests() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-regression-promote-a",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 501
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-regression-promote-a/pr-review-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "verdict": "changes_requested",
                "summary": "This change repeats the rollback-free migration pattern that regressed previously.",
                "regression_signals": [{
                    "kind": "historical_failure_pattern",
                    "summary": "Rollback-free migrations regressed previously during deploy."
                }]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let regression_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("regression_signal")).then(
                    || {
                        row.get("candidate_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    },
                )?
            })
        })
        .expect("regression candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-regression-promote-a/memory-candidates/{regression_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable regression signal"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-regression-promote-b",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 502
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-regression-promote-b/memory-hits?q=rollback-free%20migrations%20regressed%20during%20deploy")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    let first_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first hit");
    assert_eq!(
        first_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("regression_signal")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("pr_review")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
}

#[tokio::test]
async fn coder_promoted_fix_memory_reuses_strategy_across_issues() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-fix-promote-a",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 201
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-fix-promote-a/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add the startup fallback guard and cover the nil-config recovery path.",
                "root_cause": "Startup recovery skipped the nil-config fallback path.",
                "fix_strategy": "add startup fallback guard",
                "validation_steps": ["cargo test -p tandem-server coder_promoted_fix_memory_reuses_strategy_across_issues -- --test-threads=1"],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup fallback recovery regression passed"
                }]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let fix_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("fix_pattern")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("fix candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-fix-promote-a/memory-candidates/{fix_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable fix pattern"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-fix-promote-b",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 202
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-fix-promote-b/memory-hits?q=startup%20fallback%20guard%20nil-config%20recovery")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    let first_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first hit");
    assert_eq!(
        first_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("fix_pattern")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
    assert!(first_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("fix_strategy: add startup fallback guard")));
    assert!(first_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| {
            content.contains("root_cause: Startup recovery skipped the nil-config fallback path.")
        }));
}

#[tokio::test]
async fn coder_promoted_validation_memory_reuses_across_issues() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-validation-promote-a",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 211
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-validation-promote-a/issue-fix-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Add the startup fallback guard and verify recovery with a targeted regression.",
                "root_cause": "Startup recovery skipped the nil-config fallback path.",
                "fix_strategy": "add startup fallback guard",
                "validation_steps": ["cargo test -p tandem-server coder_promoted_validation_memory_reuses_across_issues -- --test-threads=1"],
                "validation_results": [{
                    "kind": "test",
                    "status": "passed",
                    "summary": "startup recovery regression passed"
                }]
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let validation_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("validation_memory")).then(
                    || {
                        row.get("candidate_id")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    },
                )?
            })
        })
        .expect("validation candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-validation-promote-a/memory-candidates/{validation_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable validation evidence"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-validation-promote-b",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 212
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-validation-promote-b/memory-hits?q=startup%20recovery%20regression%20passed")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    let first_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first hit");
    assert_eq!(
        first_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("validation_memory")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
    assert!(first_hit.get("content").and_then(Value::as_str).is_some());
}

#[tokio::test]
async fn coder_promoted_failure_pattern_reuses_across_issues() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-failure-pattern-promote-a",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 301
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-failure-pattern-promote-a/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "GitHub capability bindings drifted, so issue triage failed before reproduction.",
                "confidence": "high",
                "likely_root_cause": "Capability readiness drift in GitHub issue bindings.",
                "reproduction": "Run creation halted before reproduction because GitHub issue capabilities were missing."
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let failure_pattern_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("failure_pattern")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("failure pattern candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-failure-pattern-promote-a/memory-candidates/{failure_pattern_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable failure pattern"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-failure-pattern-promote-b",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 302
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-failure-pattern-promote-b/memory-hits?q=github%20capability%20bindings%20drift%20issue%20triage%20reproduction%20missing")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    let first_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first hit");
    assert_eq!(
        first_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("failure_pattern")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
    assert!(first_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("GitHub capability bindings drifted")));
    assert!(first_hit
        .get("content")
        .and_then(Value::as_str)
        .is_some_and(|content| content.contains("issue triage failed before reproduction")));
}

#[tokio::test]
async fn coder_promoted_triage_outcome_reuses_across_issues() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());

    let create_first_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-outcome-promote-a",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 401
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("first create request");
    let create_first_resp = app
        .clone()
        .oneshot(create_first_req)
        .await
        .expect("first create response");
    assert_eq!(create_first_resp.status(), StatusCode::OK);

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-triage-outcome-promote-a/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Capability readiness drift in GitHub issue bindings is the likely root cause.",
                "confidence": "high",
                "likely_root_cause": "GitHub issue bindings were not connected when triage started."
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let run_outcome_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("run_outcome")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("run outcome candidate id");

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-triage-outcome-promote-a/memory-candidates/{run_outcome_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable triage outcome"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);
    let promote_payload: Value = serde_json::from_slice(
        &to_bytes(promote_resp.into_body(), usize::MAX)
            .await
            .expect("promote body"),
    )
    .expect("promote json");

    let create_second_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-triage-outcome-promote-b",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 402
                },
                "source_client": "desktop_developer_mode"
            })
            .to_string(),
        ))
        .expect("second create request");
    let create_second_resp = app
        .clone()
        .oneshot(create_second_req)
        .await
        .expect("second create response");
    assert_eq!(create_second_resp.status(), StatusCode::OK);

    let hits_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-triage-outcome-promote-b/memory-hits?q=issue%20triage%20completed%20high%20confidence")
        .body(Body::empty())
        .expect("hits request");
    let hits_resp = app.clone().oneshot(hits_req).await.expect("hits response");
    assert_eq!(hits_resp.status(), StatusCode::OK);
    let hits_payload: Value = serde_json::from_slice(
        &to_bytes(hits_resp.into_body(), usize::MAX)
            .await
            .expect("hits body"),
    )
    .expect("hits json");
    let first_hit = hits_payload
        .get("hits")
        .and_then(Value::as_array)
        .and_then(|rows| rows.first())
        .cloned()
        .expect("first hit");
    assert_eq!(
        first_hit.get("source").and_then(Value::as_str),
        Some("governed_memory")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("run_outcome")
    );
    assert_eq!(
        first_hit
            .get("metadata")
            .and_then(|row| row.get("workflow_mode"))
            .and_then(Value::as_str),
        Some("issue_triage")
    );
    assert_eq!(
        first_hit.get("memory_id").and_then(Value::as_str),
        promote_payload.get("memory_id").and_then(Value::as_str)
    );
    assert_eq!(first_hit.get("same_ref").and_then(Value::as_bool), None);
    assert!(first_hit.get("content").and_then(Value::as_str).is_some());
}

#[tokio::test]
async fn coder_memory_events_include_normalized_artifact_fields() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let mut rx = state.event_bus.subscribe();
    let app = app_router(state.clone());

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-run-memory-events",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "evan/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 335
                },
                "source_client": "desktop_developer_mode"
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

    let summary_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-run-memory-events/triage-summary")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Capability readiness drift already explained this failure",
                "confidence": "high"
            })
            .to_string(),
        ))
        .expect("summary request");
    let summary_resp = app
        .clone()
        .oneshot(summary_req)
        .await
        .expect("summary response");
    assert_eq!(summary_resp.status(), StatusCode::OK);
    let summary_payload: Value = serde_json::from_slice(
        &to_bytes(summary_resp.into_body(), usize::MAX)
            .await
            .expect("summary body"),
    )
    .expect("summary json");
    let triage_candidate_id = summary_payload
        .get("generated_candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter().find_map(|row| {
                (row.get("kind").and_then(Value::as_str) == Some("triage_memory")).then(|| {
                    row.get("candidate_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string)
                })?
            })
        })
        .expect("triage candidate id");

    let candidate_event = next_event_of_type(&mut rx, "coder.memory.candidate_added").await;
    assert_eq!(
        candidate_event
            .properties
            .get("kind")
            .and_then(Value::as_str),
        Some("memory_candidate")
    );
    assert_eq!(
        candidate_event
            .properties
            .get("artifact_type")
            .and_then(Value::as_str),
        Some("coder_memory_candidate")
    );
    assert!(candidate_event
        .properties
        .get("artifact_id")
        .and_then(Value::as_str)
        .is_some());
    assert!(candidate_event
        .properties
        .get("artifact_path")
        .and_then(Value::as_str)
        .is_some());

    let promote_req = Request::builder()
        .method("POST")
        .uri(format!(
            "/coder/runs/coder-run-memory-events/memory-candidates/{triage_candidate_id}/promote"
        ))
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "to_tier": "project",
                "reviewer_id": "reviewer-1",
                "approval_id": "approval-1",
                "reason": "approved reusable triage memory"
            })
            .to_string(),
        ))
        .expect("promote request");
    let promote_resp = app
        .clone()
        .oneshot(promote_req)
        .await
        .expect("promote response");
    assert_eq!(promote_resp.status(), StatusCode::OK);

    let promoted_event = next_event_of_type(&mut rx, "coder.memory.promoted").await;
    assert_eq!(
        promoted_event
            .properties
            .get("kind")
            .and_then(Value::as_str),
        Some("memory_promotion")
    );
    assert_eq!(
        promoted_event
            .properties
            .get("artifact_type")
            .and_then(Value::as_str),
        Some("coder_memory_promotion")
    );
    assert!(promoted_event
        .properties
        .get("artifact_id")
        .and_then(Value::as_str)
        .is_some());
    assert!(promoted_event
        .properties
        .get("artifact_path")
        .and_then(Value::as_str)
        .is_some());
}
