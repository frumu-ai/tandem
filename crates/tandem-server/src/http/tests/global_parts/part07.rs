// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn init_git_repo() -> std::path::PathBuf {
    let repo_root = std::env::temp_dir().join(format!("tandem-worktree-test-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&repo_root).expect("create repo dir");
    let status = Command::new("git")
        .args(["init"])
        .current_dir(&repo_root)
        .status()
        .expect("git init");
    assert!(status.success());
    let status = Command::new("git")
        .args(["config", "user.email", "tests@tandem.local"])
        .current_dir(&repo_root)
        .status()
        .expect("git config email");
    assert!(status.success());
    let status = Command::new("git")
        .args(["config", "user.name", "Tandem Tests"])
        .current_dir(&repo_root)
        .status()
        .expect("git config name");
    assert!(status.success());
    std::fs::write(repo_root.join("README.md"), "# test\n").expect("seed readme");
    let status = Command::new("git")
        .args(["add", "README.md"])
        .current_dir(&repo_root)
        .status()
        .expect("git add");
    assert!(status.success());
    let status = Command::new("git")
        .args(["commit", "-m", "init"])
        .current_dir(&repo_root)
        .status()
        .expect("git commit");
    assert!(status.success());
    repo_root
}

async fn insert_test_lease(state: &AppState, lease_id: &str) {
    let now = crate::now_ms();
    state.engine_leases.write().await.insert(
        lease_id.to_string(),
        crate::EngineLease {
            lease_id: lease_id.to_string(),
            client_id: "tests".to_string(),
            client_type: "http-test".to_string(),
            acquired_at_ms: now,
            last_renewed_at_ms: now,
            ttl_ms: 60_000,
            tenant_context: tandem_types::TenantContext::local_implicit(),
        },
    );
}

#[tokio::test]
async fn managed_worktree_endpoints_are_idempotent_and_cleanup_branch() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let repo_root = init_git_repo();
    let repo_root_str = repo_root.to_string_lossy().to_string();
    insert_test_lease(&state, "lease-1").await;

    let create_req = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root_str,
                "task_id": "task-a",
                "owner_run_id": "run-1",
                "lease_id": "lease-1",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create worktree body"),
    )
    .expect("create worktree json");
    assert_eq!(
        create_payload.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        create_payload.get("managed").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        create_payload.get("reused").and_then(Value::as_bool),
        Some(false)
    );
    let worktree_path = create_payload
        .get("path")
        .and_then(Value::as_str)
        .expect("worktree path")
        .to_string();
    let branch = create_payload
        .get("branch")
        .and_then(Value::as_str)
        .expect("branch")
        .to_string();
    assert!(worktree_path.contains("/.tandem/worktrees/"));
    assert!(std::path::Path::new(&worktree_path).exists());

    let create_again_req = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "task_id": "task-a",
                "owner_run_id": "run-1",
                "lease_id": "lease-1",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree again request");
    let create_again_resp = app
        .clone()
        .oneshot(create_again_req)
        .await
        .expect("create worktree again response");
    assert_eq!(create_again_resp.status(), StatusCode::OK);
    let create_again_payload: Value = serde_json::from_slice(
        &to_bytes(create_again_resp.into_body(), usize::MAX)
            .await
            .expect("create worktree again body"),
    )
    .expect("create worktree again json");
    assert_eq!(
        create_again_payload.get("reused").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        create_again_payload.get("path").and_then(Value::as_str),
        Some(worktree_path.as_str())
    );
    assert_eq!(
        create_again_payload.get("branch").and_then(Value::as_str),
        Some(branch.as_str())
    );

    let list_req = Request::builder()
        .method("GET")
        .extension(direct_loopback_peer())
        .uri(format!(
            "/worktree?repo_root={}&managed_only=true",
            repo_root.to_string_lossy()
        ))
        .body(Body::empty())
        .expect("list worktrees request");
    let list_resp = app
        .clone()
        .oneshot(list_req)
        .await
        .expect("list worktrees response");
    assert_eq!(list_resp.status(), StatusCode::OK);
    let list_payload: Value = serde_json::from_slice(
        &to_bytes(list_resp.into_body(), usize::MAX)
            .await
            .expect("list worktrees body"),
    )
    .expect("list worktrees json");
    assert!(list_payload
        .as_array()
        .is_some_and(|rows| rows.iter().any(|row| {
            row.get("path").and_then(Value::as_str) == Some(worktree_path.as_str())
                && row.get("task_id").and_then(Value::as_str) == Some("task-a")
                && row.get("owner_run_id").and_then(Value::as_str) == Some("run-1")
                && row.get("lease_id").and_then(Value::as_str) == Some("lease-1")
                && row.get("managed").and_then(Value::as_bool) == Some(true)
        })));

    let delete_req = Request::builder()
        .method("DELETE")
        .extension(direct_loopback_peer())
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "path": worktree_path,
                "lease_id": "lease-1"
            })
            .to_string(),
        ))
        .expect("delete worktree request");
    let delete_resp = app
        .clone()
        .oneshot(delete_req)
        .await
        .expect("delete worktree response");
    assert_eq!(delete_resp.status(), StatusCode::OK);
    let delete_payload: Value = serde_json::from_slice(
        &to_bytes(delete_resp.into_body(), usize::MAX)
            .await
            .expect("delete worktree body"),
    )
    .expect("delete worktree json");
    assert_eq!(
        delete_payload.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert_eq!(
        delete_payload
            .get("branch_deleted")
            .and_then(Value::as_bool),
        Some(true)
    );
    assert!(!std::path::Path::new(
        delete_payload
            .get("path")
            .and_then(Value::as_str)
            .expect("deleted path")
    )
    .exists());
    let branch_output = Command::new("git")
        .args(["branch", "--list", &branch])
        .current_dir(&repo_root)
        .output()
        .expect("git branch list");
    assert!(String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .is_empty());

    let _ = std::fs::remove_dir_all(repo_root);
}

#[tokio::test]
async fn managed_worktree_create_rejects_unknown_lease() {
    let state = test_state().await;
    let app = app_router(state);
    let repo_root = init_git_repo();

    let create_req = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "task_id": "task-b",
                "owner_run_id": "run-2",
                "lease_id": "missing-lease",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    assert_eq!(create_resp.status(), StatusCode::CONFLICT);

    let _ = std::fs::remove_dir_all(repo_root);
}

#[tokio::test]
async fn stale_worktree_cleanup_preserves_unknown_restart_worktrees() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let repo_root = init_git_repo();
    let repo_root_str = repo_root.to_string_lossy().to_string();
    insert_test_lease(&state, "lease-cleanup").await;

    let create_req = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root_str,
                "task_id": "task-cleanup",
                "owner_run_id": "run-cleanup",
                "lease_id": "lease-cleanup",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    assert_eq!(create_resp.status(), StatusCode::OK);
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create worktree body"),
    )
    .expect("create worktree json");
    let worktree_path = create_payload
        .get("path")
        .and_then(Value::as_str)
        .expect("worktree path")
        .to_string();
    let branch = create_payload
        .get("branch")
        .and_then(Value::as_str)
        .expect("branch")
        .to_string();
    assert!(std::path::Path::new(&worktree_path).exists());

    // Simulate a restarted process that lost the in-memory managed_worktrees map.
    state.managed_worktrees.write().await.clear();

    let cleanup_req = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
        .uri("/worktree/cleanup")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
            })
            .to_string(),
        ))
        .expect("cleanup worktree request");
    let cleanup_resp = app
        .clone()
        .oneshot(cleanup_req)
        .await
        .expect("cleanup worktree response");
    assert_eq!(cleanup_resp.status(), StatusCode::OK);
    assert!(std::path::Path::new(&worktree_path).exists());

    let branch_output = Command::new("git")
        .args(["branch", "--list", &branch])
        .current_dir(&repo_root)
        .output()
        .expect("git branch list");
    assert!(!String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .is_empty());

    let remove = Command::new("git")
        .args([
            "-C",
            &repo_root_str,
            "worktree",
            "remove",
            "--force",
            &worktree_path,
        ])
        .output()
        .expect("remove preserved test worktree");
    assert!(remove.status.success());
    let delete_branch = Command::new("git")
        .args(["-C", &repo_root_str, "branch", "-D", &branch])
        .output()
        .expect("delete preserved test worktree branch");
    assert!(delete_branch.status.success());
    let _ = std::fs::remove_dir_all(repo_root);
}

#[tokio::test]
async fn managed_worktree_create_rejects_external_path_override() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let repo_root = init_git_repo();
    insert_test_lease(&state, "lease-path-boundary").await;
    let external_path = std::env::temp_dir().join(format!("tandem-external-{}", Uuid::new_v4()));

    let create_req = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "path": external_path.to_string_lossy(),
                "task_id": "task-path",
                "owner_run_id": "run-path",
                "lease_id": "lease-path-boundary",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    assert_eq!(create_resp.status(), StatusCode::BAD_REQUEST);
    assert!(!external_path.exists());

    let _ = std::fs::remove_dir_all(repo_root);
}

#[tokio::test]
async fn managed_worktree_mutations_require_matching_active_lease() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let repo_root = init_git_repo();
    insert_test_lease(&state, "lease-1").await;

    let create_req = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "task_id": "task-c",
                "owner_run_id": "run-3",
                "lease_id": "lease-1",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create worktree body"),
    )
    .expect("create worktree json");
    let worktree_path = create_payload
        .get("path")
        .and_then(Value::as_str)
        .expect("worktree path")
        .to_string();

    let reset_without_lease = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
        .uri("/worktree/reset")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "path": worktree_path
            })
            .to_string(),
        ))
        .expect("reset worktree request");
    let reset_resp = app
        .clone()
        .oneshot(reset_without_lease)
        .await
        .expect("reset worktree response");
    assert_eq!(reset_resp.status(), StatusCode::CONFLICT);

    let delete_wrong_lease = Request::builder()
        .method("DELETE")
        .extension(direct_loopback_peer())
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "path": worktree_path,
                "lease_id": "lease-other"
            })
            .to_string(),
        ))
        .expect("delete wrong lease request");
    let delete_wrong_resp = app
        .clone()
        .oneshot(delete_wrong_lease)
        .await
        .expect("delete wrong lease response");
    assert_eq!(delete_wrong_resp.status(), StatusCode::CONFLICT);

    let delete_req = Request::builder()
        .method("DELETE")
        .extension(direct_loopback_peer())
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "path": worktree_path,
                "lease_id": "lease-1"
            })
            .to_string(),
        ))
        .expect("delete worktree request");
    let delete_resp = app
        .clone()
        .oneshot(delete_req)
        .await
        .expect("delete worktree response");
    assert_eq!(delete_resp.status(), StatusCode::OK);

    let _ = std::fs::remove_dir_all(repo_root);
}

#[tokio::test]
async fn releasing_lease_cleans_up_managed_worktrees() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let repo_root = init_git_repo();
    insert_test_lease(&state, "lease-cleanup").await;

    let create_req = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "task_id": "task-d",
                "owner_run_id": "run-4",
                "lease_id": "lease-cleanup",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create worktree body"),
    )
    .expect("create worktree json");
    let worktree_path = create_payload
        .get("path")
        .and_then(Value::as_str)
        .expect("worktree path")
        .to_string();
    let branch = create_payload
        .get("branch")
        .and_then(Value::as_str)
        .expect("branch")
        .to_string();

    let release_req = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
        .uri("/global/lease/release")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({ "lease_id": "lease-cleanup" }).to_string(),
        ))
        .expect("release request");
    let release_resp = app
        .clone()
        .oneshot(release_req)
        .await
        .expect("release response");
    assert_eq!(release_resp.status(), StatusCode::OK);
    let release_payload: Value = serde_json::from_slice(
        &to_bytes(release_resp.into_body(), usize::MAX)
            .await
            .expect("release body"),
    )
    .expect("release json");
    assert_eq!(
        release_payload.get("ok").and_then(Value::as_bool),
        Some(true)
    );
    assert!(release_payload
        .get("released_worktrees")
        .and_then(Value::as_array)
        .is_some_and(|rows| rows
            .iter()
            .any(|row| row.as_str() == Some(worktree_path.as_str()))));
    assert!(!std::path::Path::new(&worktree_path).exists());

    let branch_output = Command::new("git")
        .args(["branch", "--list", &branch])
        .current_dir(&repo_root)
        .output()
        .expect("git branch list");
    assert!(String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .is_empty());

    let _ = std::fs::remove_dir_all(repo_root);
}

#[tokio::test]
async fn expired_leases_are_pruned_and_cleanup_managed_worktrees() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let repo_root = init_git_repo();
    let now = crate::now_ms();
    state.engine_leases.write().await.insert(
        "lease-expired".to_string(),
        crate::EngineLease {
            lease_id: "lease-expired".to_string(),
            client_id: "tests".to_string(),
            client_type: "http-test".to_string(),
            acquired_at_ms: now.saturating_sub(120_000),
            last_renewed_at_ms: now.saturating_sub(120_000),
            ttl_ms: 5_000,
            tenant_context: tandem_types::TenantContext::local_implicit(),
        },
    );

    let create_req = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "task_id": "task-e",
                "owner_run_id": "run-5",
                "lease_id": "lease-expired",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    assert_eq!(create_resp.status(), StatusCode::CONFLICT);

    insert_test_lease(&state, "lease-fresh").await;
    let create_req = Request::builder()
        .method("POST")
        .extension(direct_loopback_peer())
        .uri("/worktree")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "repo_root": repo_root.to_string_lossy(),
                "task_id": "task-f",
                "owner_run_id": "run-6",
                "lease_id": "lease-fresh",
                "managed": true
            })
            .to_string(),
        ))
        .expect("create worktree request");
    let create_resp = app
        .clone()
        .oneshot(create_req)
        .await
        .expect("create worktree response");
    let create_payload: Value = serde_json::from_slice(
        &to_bytes(create_resp.into_body(), usize::MAX)
            .await
            .expect("create worktree body"),
    )
    .expect("create worktree json");
    let worktree_path = create_payload
        .get("path")
        .and_then(Value::as_str)
        .expect("worktree path")
        .to_string();
    let branch = create_payload
        .get("branch")
        .and_then(Value::as_str)
        .expect("branch")
        .to_string();

    {
        let mut leases = state.engine_leases.write().await;
        let lease = leases.get_mut("lease-fresh").expect("fresh lease present");
        lease.last_renewed_at_ms = now.saturating_sub(120_000);
        lease.ttl_ms = 5_000;
    }

    let health_req = Request::builder()
        .method("GET")
        .uri("/global/health")
        .body(Body::empty())
        .expect("health request");
    let health_resp = app
        .clone()
        .oneshot(health_req)
        .await
        .expect("health response");
    assert_eq!(health_resp.status(), StatusCode::OK);
    assert!(!std::path::Path::new(&worktree_path).exists());
    let branch_output = Command::new("git")
        .args(["branch", "--list", &branch])
        .current_dir(&repo_root)
        .output()
        .expect("git branch list");
    assert!(String::from_utf8_lossy(&branch_output.stdout)
        .trim()
        .is_empty());

    let _ = std::fs::remove_dir_all(repo_root);
}

