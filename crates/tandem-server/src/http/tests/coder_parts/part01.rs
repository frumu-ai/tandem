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
                        },
                        {
                            "name": "mcp.github.merge_pull_request",
                            "description": "Merge a GitHub pull request",
                            "inputSchema": {"type":"object"}
                        },
                        {
                            "name": "mcp.github.get_project",
                            "description": "Get a GitHub project",
                            "inputSchema": {"type":"object"}
                        },
                        {
                            "name": "mcp.github.list_project_items",
                            "description": "List GitHub project items",
                            "inputSchema": {"type":"object"}
                        },
                        {
                            "name": "mcp.github.update_project_item_field",
                            "description": "Update a GitHub project item field",
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
                                "html_url": "https://github.com/user123/tandem/pull/314",
                                "head": {"ref": "coder/issue-313-fix"},
                                "base": {"ref": "main"}
                            }
                        }),
                        "mcp.github.merge_pull_request" => json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": "merged pull request #314"
                                }
                            ],
                            "merged": true,
                            "sha": "abc123def456",
                            "message": "Pull request successfully merged",
                            "pull_request": {
                                "number": 314,
                                "state": "merged",
                                "html_url": "https://github.com/user123/tandem/pull/314"
                            }
                        }),
                        "mcp.github.get_project" => json!({
                            "id": "proj_42",
                            "owner": "user123",
                            "number": 42,
                            "title": "Coder Intake",
                            "fields": [
                                {
                                    "id": "status_field_1",
                                    "name": "Status",
                                    "options": [
                                        {"id": "opt_todo", "name": "TODO"},
                                        {"id": "opt_progress", "name": "In Progress"},
                                        {"id": "opt_review", "name": "In Review"},
                                        {"id": "opt_blocked", "name": "Blocked"},
                                        {"id": "opt_done", "name": "Done"}
                                    ]
                                }
                            ]
                        }),
                        "mcp.github.list_project_items" => json!({
                            "items": [
                                {
                                    "id": "PVT_item_1",
                                    "title": "Guard startup recovery config loading",
                                    "status": {"id": "opt_todo", "name": "TODO"},
                                    "content": {
                                        "type": "Issue",
                                        "number": 313,
                                        "title": "Guard startup recovery config loading",
                                        "url": "https://github.com/user123/tandem/issues/313"
                                    }
                                },
                                {
                                    "id": "PVT_item_2",
                                    "title": "Draft note",
                                    "status": {"id": "opt_todo", "name": "TODO"},
                                    "content": {
                                        "type": "DraftIssue",
                                        "title": "Draft note"
                                    }
                                }
                            ]
                        }),
                        "mcp.github.update_project_item_field" => json!({
                            "ok": true,
                            "item_id": request
                                .get("params")
                                .and_then(|row| row.get("arguments"))
                                .and_then(|row| row.get("project_item_id"))
                                .cloned()
                                .unwrap_or(Value::Null)
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

fn init_coder_git_repo() -> std::path::PathBuf {
    let repo_root =
        std::env::temp_dir().join(format!("tandem-coder-worktree-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&repo_root).expect("create repo dir");
    let status = std::process::Command::new("git")
        .args(["init"])
        .current_dir(&repo_root)
        .status()
        .expect("git init");
    assert!(status.success());
    let status = std::process::Command::new("git")
        .args(["config", "user.email", "tests@tandem.local"])
        .current_dir(&repo_root)
        .status()
        .expect("git config email");
    assert!(status.success());
    let status = std::process::Command::new("git")
        .args(["config", "user.name", "Tandem Tests"])
        .current_dir(&repo_root)
        .status()
        .expect("git config name");
    assert!(status.success());
    std::fs::write(repo_root.join("README.md"), "# coder test\n").expect("seed readme");
    let status = std::process::Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&repo_root)
        .status()
        .expect("git add");
    assert!(status.success());
    let status = std::process::Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&repo_root)
        .status()
        .expect("git commit");
    assert!(status.success());
    repo_root
}

async fn create_coder_run_for_replay(app: axum::Router, body: Value) -> (Value, String) {
    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
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
    (create_payload, linked_context_run_id)
}

async fn checkpoint_and_replay_coder_run(app: axum::Router, linked_context_run_id: &str) -> Value {
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
        Some(linked_context_run_id)
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
    serde_json::from_slice(&replay_body).expect("replay json")
}

#[tokio::test]
async fn coder_issue_triage_run_create_get_and_list() {
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
                "coder_run_id": "coder-run-1",
                "workflow_mode": "issue_triage",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 1234,
                    "url": "https://github.com/user123/tandem/issues/1234"
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
        Some("user123/tandem issue #1234")
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
                    "repo_slug": "user123/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 88,
                    "url": "https://github.com/user123/tandem/pull/88"
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
    assert!(get_payload
        .get("coder_artifacts")
        .and_then(Value::as_array)
        .map(|rows| rows.iter().any(|row| {
            row.get("artifact_type").and_then(Value::as_str) == Some("coder_memory_hits")
                && row.get("exists").and_then(Value::as_bool) == Some(true)
                && row.get("payload_format").and_then(Value::as_str) == Some("json")
                && row.get("payload").is_some()
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
        Some("user123/tandem pull request #88 review regressions blockers requested changes")
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
                    "repo_slug": "user123/tandem"
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
        Some("user123/tandem issue #77")
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
                    "repo_slug": "user123/tandem"
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
async fn coder_issue_fix_failed_validation_writes_regression_signal() {
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
                "coder_run_id": "coder-issue-fix-validation-failed",
                "workflow_mode": "issue_fix",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 80
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

    let validation_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-validation-failed/issue-fix-validation-report")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "summary": "Guarded the startup recovery path, but one regression still failed.",
                "root_cause": "Startup recovery skipped the config fallback branch.",
                "fix_strategy": "guard fallback branch",
                "changed_files": ["crates/tandem-server/src/http/coder.rs"],
                "validation_steps": ["cargo test -p tandem-server coder_issue_fix_failed_validation_writes_regression_signal -- --test-threads=1"],
                "validation_results": [{
                    "kind": "test",
                    "status": "failed",
                    "summary": "targeted startup recovery regression still fails"
                }],
                "memory_hits_used": ["memory-hit-fix-validation-failure-1"]
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
            .get("generated_candidates")
            .and_then(Value::as_array)
            .map(|rows| rows.iter().any(|row| {
                row.get("kind").and_then(Value::as_str) == Some("regression_signal")
            })),
        Some(true)
    );

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-issue-fix-validation-failed/memory-candidates")
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
    let regression_signal = candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("regression_signal"))
        })
        .and_then(|row| row.get("payload"))
        .cloned()
        .expect("regression signal payload");
    assert_eq!(
        regression_signal
            .get("regression_signals")
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("status"))
            .and_then(Value::as_str),
        Some("failed")
    );
    assert_eq!(
        regression_signal
            .get("validation_artifact_path")
            .and_then(Value::as_str)
            .is_some(),
        true
    );
}

#[tokio::test]
async fn coder_issue_fix_worker_failure_writes_run_outcome() {
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
                "coder_run_id": "coder-issue-fix-worker-failure",
                "workflow_mode": "issue_fix",
                "model_provider": "missing-provider",
                "model_id": "missing-model",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
                    "number": 81
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

    let first_step_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-worker-failure/execute-next")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("first step request");
    let first_step_resp = app
        .clone()
        .oneshot(first_step_req)
        .await
        .expect("first step response");
    assert_eq!(first_step_resp.status(), StatusCode::OK);

    let second_step_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-issue-fix-worker-failure/execute-next")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("second step request");
    let second_step_resp = app
        .clone()
        .oneshot(second_step_req)
        .await
        .expect("second step response");
    assert_eq!(second_step_resp.status(), StatusCode::OK);
    let second_step_payload: Value = serde_json::from_slice(
        &to_bytes(second_step_resp.into_body(), usize::MAX)
            .await
            .expect("second step body"),
    )
    .expect("second step json");
    assert_eq!(
        second_step_payload
            .get("dispatch_result")
            .and_then(|row| row.get("code"))
            .and_then(Value::as_str),
        Some("CODER_WORKER_SESSION_FAILED")
    );
    assert_eq!(
        second_step_payload
            .get("dispatch_result")
            .and_then(|row| row.get("generated_candidates"))
            .and_then(Value::as_array)
            .and_then(|rows| rows.first())
            .and_then(|row| row.get("kind"))
            .and_then(Value::as_str),
        Some("run_outcome")
    );

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-issue-fix-worker-failure/memory-candidates")
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
    let run_outcome_payload = candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("run_outcome"))
        })
        .and_then(|row| row.get("payload"))
        .cloned()
        .expect("run outcome payload");
    assert_eq!(
        run_outcome_payload.get("result").and_then(Value::as_str),
        Some("issue_fix_prepare_failed")
    );
    assert_eq!(
        run_outcome_payload
            .get("worker_artifact_type")
            .and_then(Value::as_str),
        Some("coder_issue_fix_worker_session")
    );
    assert_eq!(
        run_outcome_payload
            .get("worker_session_context_run_id")
            .and_then(Value::as_str)
            .map(|value| value.starts_with("session-")),
        Some(true)
    );
    assert_eq!(
        run_outcome_payload
            .get("worker_run_reference")
            .and_then(Value::as_str),
        run_outcome_payload
            .get("worker_session_context_run_id")
            .and_then(Value::as_str)
    );
}

#[tokio::test]
async fn coder_pr_review_worker_failure_writes_run_outcome() {
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
                "coder_run_id": "coder-pr-review-worker-failure",
                "workflow_mode": "pr_review",
                "model_provider": "missing-provider",
                "model_id": "missing-model",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 82
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

    let first_step_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-pr-review-worker-failure/execute-next")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("first step request");
    let first_step_resp = app
        .clone()
        .oneshot(first_step_req)
        .await
        .expect("first step response");
    assert_eq!(first_step_resp.status(), StatusCode::OK);

    let second_step_req = Request::builder()
        .method("POST")
        .uri("/coder/runs/coder-pr-review-worker-failure/execute-next")
        .header("content-type", "application/json")
        .body(Body::from(json!({}).to_string()))
        .expect("second step request");
    let second_step_resp = app
        .clone()
        .oneshot(second_step_req)
        .await
        .expect("second step response");
    assert_eq!(second_step_resp.status(), StatusCode::OK);
    let second_step_payload: Value = serde_json::from_slice(
        &to_bytes(second_step_resp.into_body(), usize::MAX)
            .await
            .expect("second step body"),
    )
    .expect("second step json");
    assert_eq!(
        second_step_payload
            .get("dispatch_result")
            .and_then(|row| row.get("code"))
            .and_then(Value::as_str),
        Some("CODER_WORKER_SESSION_FAILED")
    );

    let candidates_req = Request::builder()
        .method("GET")
        .uri("/coder/runs/coder-pr-review-worker-failure/memory-candidates")
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
    let run_outcome_payload = candidates_payload
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|rows| {
            rows.iter()
                .find(|row| row.get("kind").and_then(Value::as_str) == Some("run_outcome"))
        })
        .and_then(|row| row.get("payload"))
        .cloned()
        .expect("run outcome payload");
    assert_eq!(
        run_outcome_payload.get("result").and_then(Value::as_str),
        Some("pr_review_failed")
    );
    assert_eq!(
        run_outcome_payload
            .get("worker_artifact_type")
            .and_then(Value::as_str),
        Some("coder_pr_review_worker_session")
    );
    assert_eq!(
        run_outcome_payload
            .get("worker_run_reference")
            .and_then(Value::as_str),
        run_outcome_payload
            .get("worker_session_context_run_id")
            .and_then(Value::as_str)
            .or_else(|| {
                run_outcome_payload
                    .get("worker_session_id")
                    .and_then(Value::as_str)
            })
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
                    "repo_slug": "user123/tandem"
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
                    .and_then(|row| row.get("worker_run_reference"))
                    .and_then(Value::as_str),
                execute_payload
                    .get("dispatch_result")
                    .and_then(|row| row.get("worker_session"))
                    .and_then(|row| row.get("session_context_run_id"))
                    .and_then(Value::as_str)
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
        assert_eq!(
            changed_file_payload
                .get("worker_run_reference")
                .and_then(Value::as_str),
            changed_file_payload
                .get("worker_session_context_run_id")
                .and_then(Value::as_str)
                .or_else(|| {
                    changed_file_payload
                        .get("worker_session_id")
                        .and_then(Value::as_str)
                })
        );
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
        assert_eq!(
            patch_summary_payload
                .get("worker_run_reference")
                .and_then(Value::as_str),
            patch_summary_payload
                .get("worker_session_context_run_id")
                .and_then(Value::as_str)
                .or_else(|| {
                    patch_summary_payload
                        .get("worker_session_id")
                        .and_then(Value::as_str)
                })
        );
        assert_eq!(
            patch_summary_payload
                .get("validation_run_reference")
                .and_then(Value::as_str),
            patch_summary_payload
                .get("validation_session_context_run_id")
                .and_then(Value::as_str)
                .or_else(|| {
                    patch_summary_payload
                        .get("validation_session_id")
                        .and_then(Value::as_str)
                })
        );
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
async fn coder_issue_fix_worker_uses_managed_worktree_for_git_repo() {
    let state = test_state().await;
    state
        .capability_resolver
        .refresh_builtin_bindings()
        .await
        .expect("refresh builtin bindings");
    let app = app_router(state.clone());
    let repo_root = init_coder_git_repo();
    let nested_workspace_root = repo_root.join("nested").join("workspace");
    std::fs::create_dir_all(&nested_workspace_root).expect("create nested workspace");

    let create_req = Request::builder()
        .method("POST")
        .uri("/coder/runs")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "coder_run_id": "coder-issue-fix-managed-worktree",
                "workflow_mode": "issue_fix",
                "model_provider": "local",
                "model_id": "echo-1",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": nested_workspace_root.to_string_lossy(),
                    "repo_slug": "user123/tandem"
                },
                "github_ref": {
                    "kind": "issue",
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

    let mut saw_prepare_fix = false;
    for _ in 0..2 {
        let execute_req = Request::builder()
            .method("POST")
            .uri("/coder/runs/coder-issue-fix-managed-worktree/execute-next")
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
        if execute_payload
            .get("task")
            .and_then(|row| row.get("workflow_node_id"))
            .and_then(Value::as_str)
            == Some("prepare_fix")
        {
            saw_prepare_fix = true;
            let worker_session = execute_payload
                .get("dispatch_result")
                .and_then(|row| row.get("worker_session"))
                .cloned()
                .expect("worker session");
            let worker_workspace_root = worker_session
                .get("worker_workspace_root")
                .and_then(Value::as_str)
                .expect("worker workspace root")
                .to_string();
            assert!(worker_workspace_root.contains("/.tandem/worktrees/"));
            assert_eq!(
                worker_session
                    .get("worker_workspace_repo_root")
                    .and_then(Value::as_str),
                Some(repo_root.to_string_lossy().as_ref())
            );
            assert_eq!(
                worker_session.get("task_id").and_then(Value::as_str),
                execute_payload
                    .get("task")
                    .and_then(|row| row.get("id"))
                    .and_then(Value::as_str)
            );
            assert!(!std::path::Path::new(&worker_workspace_root).exists());

            let managed_root = repo_root.join(".tandem").join("worktrees");
            if managed_root.exists() {
                let entries = std::fs::read_dir(&managed_root)
                    .expect("list managed root")
                    .filter_map(Result::ok)
                    .collect::<Vec<_>>();
                assert!(entries.is_empty());
            }
            break;
        }
    }
    assert!(saw_prepare_fix, "expected prepare_fix task to run");

    let _ = std::fs::remove_dir_all(repo_root);
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
                    "repo_slug": "user123/tandem"
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
async fn coder_pr_review_execute_all_runs_to_completion() {
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
                "coder_run_id": "coder-pr-review-execute-all",
                "workflow_mode": "pr_review",
                "repo_binding": {
                    "project_id": "proj-engine",
                    "workspace_id": "ws-tandem",
                    "workspace_root": "/tmp/tandem-repo",
                    "repo_slug": "user123/tandem",
                    "default_branch": "main"
                },
                "github_ref": {
                    "kind": "pull_request",
                    "number": 300
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
        .uri("/coder/runs/coder-pr-review-execute-all/execute-all")
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
        .is_some_and(|count| count >= 3));
}
