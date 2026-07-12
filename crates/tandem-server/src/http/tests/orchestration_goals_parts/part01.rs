#[tokio::test]
async fn draft_lifecycle_enforces_optimistic_concurrency() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let (planner_hash, executor_hash) = seed_workflows(&state).await;

    let (status, created) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations",
            Some(draft_payload(&planner_hash, &executor_hash)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let token = created["updated_at_ms"].as_u64().expect("draft token");

    let (status, validation) = dispatch(
        &app,
        local_request("POST", "/orchestrations/orch-goals/validate", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{validation}");
    assert_eq!(validation["report"]["valid"], json!(true));
    assert!(!validation["report"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["code"] == json!("invalid_version")));

    // Updating without the concurrency token is rejected, not silently applied.
    let mut update = draft_payload(&planner_hash, &executor_hash);
    update["name"] = json!("Renamed");
    let (status, body) = dispatch(
        &app,
        local_request("PUT", "/orchestrations/orch-goals", Some(update.clone())),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");

    // A stale token is rejected the same way.
    update["expected_updated_at_ms"] = json!(token.saturating_sub(1));
    let (status, body) = dispatch(
        &app,
        local_request("PUT", "/orchestrations/orch-goals", Some(update.clone())),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["error"], json!("draft_concurrency_conflict"));

    // The current token succeeds.
    update["expected_updated_at_ms"] = json!(token);
    let (status, updated) = dispatch(
        &app,
        local_request("PUT", "/orchestrations/orch-goals", Some(update)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(updated["orchestration"]["name"], json!("Renamed"));
    let updated_token = updated["updated_at_ms"].as_u64().expect("updated token");

    // List surfaces the draft; archive retires it.
    let (status, listed) = dispatch(&app, local_request("GET", "/orchestrations", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["count"], json!(1));
    let (status, conflict) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations/orch-goals/archive",
            Some(json!({"expected_updated_at_ms": token})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    let (status, archived) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations/orch-goals/archive",
            Some(json!({"expected_updated_at_ms": updated_token})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(archived["status"], json!("archived"));
}

#[tokio::test]
async fn draft_actions_accept_legacy_empty_and_null_json_bodies() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let (planner_hash, executor_hash) = seed_workflows(&state).await;
    let (status, _) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations",
            Some(draft_payload(&planner_hash, &executor_hash)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);

    let empty_json_request = Request::builder()
        .method("POST")
        .uri("/orchestrations/orch-goals/publish")
        .header("x-tandem-org-id", "local")
        .header("x-tandem-workspace-id", "local")
        .header("x-tandem-actor-id", "operator")
        .header("content-type", "application/json")
        .body(Body::empty())
        .expect("legacy empty JSON request");
    let (status, body) = dispatch(&app, empty_json_request).await;
    assert_eq!(status, StatusCode::CREATED, "{body}");

    let null_json_request = Request::builder()
        .method("POST")
        .uri("/orchestrations/orch-goals/archive")
        .header("x-tandem-org-id", "local")
        .header("x-tandem-workspace-id", "local")
        .header("x-tandem-actor-id", "operator")
        .header("content-type", "application/json")
        .body(Body::from("null"))
        .expect("legacy null JSON request");
    let (status, body) = dispatch(&app, null_json_request).await;
    assert_eq!(status, StatusCode::OK, "{body}");
}

#[tokio::test]
async fn goal_start_rejects_a_stale_root_workflow_definition() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    let app = app_router(state.clone());
    publish_orchestration(&app, &state).await;

    state
        .put_automation_v2(
            AutomationSpecBuilder::new("planner")
                .name("Planner changed after publish")
                .build(),
        )
        .await
        .unwrap();
    let (status, body) = dispatch(
        &app,
        local_request(
            "POST",
            "/goals",
            Some(json!({
                "orchestration_id": "orch-goals",
                "objective": "Must not use a stale root",
                "idempotency_key": "stale-root-start",
            })),
        ),
    )
    .await;

    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["detail"]
        .as_str()
        .unwrap_or_default()
        .contains("root workflow definition changed"));
}

#[tokio::test]
async fn stale_references_block_publish_until_refreshed() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let (_planner_hash, executor_hash) = seed_workflows(&state).await;

    // Pin the planner node to an outdated hash.
    let (status, created) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations",
            Some(draft_payload("sha256:outdated", &executor_hash)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let token = created["updated_at_ms"].as_u64().expect("draft token");

    // The stale reference is visible on the draft…
    let (status, stale) = dispatch(
        &app,
        local_request("GET", "/orchestrations/orch-goals/stale-references", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(stale["stale_count"], json!(1));

    // …and blocks publishing.
    let (status, blocked) = dispatch(
        &app,
        local_request("POST", "/orchestrations/orch-goals/publish", None),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(blocked["error"], json!("orchestration_invalid"));

    // Explicit refresh re-pins to the current hashes and unblocks publish.
    let (status, refreshed) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations/orch-goals/refresh-references",
            Some(json!({"expected_updated_at_ms": token})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{refreshed}");
    assert_eq!(refreshed["refreshed_node_ids"], json!(["plan"]));
    let refreshed_token = refreshed["orchestration"]["updated_at_ms"]
        .as_u64()
        .expect("refreshed draft token");
    let (status, conflict) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations/orch-goals/publish",
            Some(json!({"expected_updated_at_ms": token})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{conflict}");
    let (status, published) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations/orch-goals/publish",
            Some(json!({"expected_updated_at_ms": refreshed_token})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{published}");
    assert_eq!(published["version"], json!(1));
    // The published snapshot records actor + validation + referenced hashes.
    assert!(
        published["orchestration"]["metadata"]["publish"]["validation"]["valid"]
            .as_bool()
            .unwrap_or(false)
    );

    // Published versions are immutable and separately addressable.
    let (status, version) = dispatch(
        &app,
        local_request("GET", "/orchestrations/orch-goals/versions/1", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(version["status"], json!("published"));
}

#[tokio::test]
async fn cross_tenant_references_and_reads_fail_closed() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let (planner_hash, executor_hash) = seed_workflows(&state).await;

    // The workflows live in the local tenant; another tenant's draft that
    // references them must see them as missing (fail closed).
    let (status, _) = dispatch(
        &app,
        orchestration_request(
            "POST",
            "/orchestrations",
            "acme",
            "hq",
            Some(draft_payload(&planner_hash, &executor_hash)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let (status, validation) = dispatch(
        &app,
        orchestration_request(
            "POST",
            "/orchestrations/orch-goals/validate",
            "acme",
            "hq",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(validation["report"]["valid"], json!(false));
    assert!(validation["report"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| issue["code"] == json!("missing_workflow")));

    // Another tenant cannot read the acme draft at all.
    let (status, _) = dispatch(
        &app,
        local_request("GET", "/orchestrations/orch-goals", None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // Storage identity is tenant-scoped: the local tenant may use the same
    // orchestration ID/version without learning about or colliding with acme.
    let (status, local_created) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations",
            Some(draft_payload(&planner_hash, &executor_hash)),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{local_created}");
    let (status, local_draft) = dispatch(
        &app,
        local_request("GET", "/orchestrations/orch-goals", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{local_draft}");
}

#[tokio::test]
async fn normalized_workflow_deny_grant_wins_over_allow() {
    let state = test_state().await;
    let (definition_hash, allow) =
        seed_enterprise_workflow(&state, "governed-workflow", true).await;
    let deny = OrganizationUnitAccessGrant {
        grant_id: "  WORKFLOW-GRANT  ".to_string(),
        effect: AccessEffect::Deny,
        ..allow
    };
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .insert("deny-workflow-grant".to_string(), deny);
    let app = app_router(state);

    let (status, created) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations",
            Some(enterprise_draft_payload(
                "governed-workflow",
                &definition_hash,
            )),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let (status, validation) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations/enterprise-orchestration/validate",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{validation}");
    assert_eq!(validation["report"]["valid"], json!(false));
    assert!(validation["report"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| {
            issue["code"] == json!("workflow_authority_denied")
                && issue["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("explicitly denies"))
        }));
}

#[tokio::test]
async fn unauthorized_author_cannot_validate_or_publish_referenced_workflow() {
    let state = test_state().await;
    let (definition_hash, _) = seed_enterprise_workflow(&state, "restricted-workflow", false).await;
    let app = app_router(state);

    let (status, created) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations",
            Some(enterprise_draft_payload(
                "restricted-workflow",
                &definition_hash,
            )),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let (status, validation) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations/enterprise-orchestration/validate",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{validation}");
    assert_eq!(validation["report"]["valid"], json!(false));
    assert!(validation["report"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| {
            issue["code"] == json!("workflow_authority_denied")
                && issue["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("author `operator`"))
        }));

    let (status, publish) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations/enterprise-orchestration/publish",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{publish}");
    assert_eq!(publish["error"], json!("orchestration_invalid"));
}

#[tokio::test]
async fn missing_author_fails_closed_for_referenced_workflow_authority() {
    let state = test_state().await;
    let (definition_hash, _) = seed_enterprise_workflow(&state, "allowed-workflow", true).await;
    let app = app_router(state.clone());
    let (status, created) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations",
            Some(enterprise_draft_payload(
                "allowed-workflow",
                &definition_hash,
            )),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");

    let tenant = TenantContext::local_implicit();
    let store = crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(
        &state.automation_v2_runs_path,
    )
    .unwrap();
    let mut draft = store
        .get_orchestration_draft(&tenant, "enterprise-orchestration")
        .unwrap()
        .unwrap();
    let expected_updated_at_ms = draft.updated_at_ms;
    draft.metadata.as_mut().unwrap().as_object_mut().unwrap().remove("created_by");
    store
        .put_orchestration_draft(&draft, Some(expected_updated_at_ms))
        .unwrap();

    let (status, validation) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations/enterprise-orchestration/validate",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{validation}");
    assert_eq!(validation["report"]["valid"], json!(false));
    assert!(validation["report"]["issues"]
        .as_array()
        .unwrap()
        .iter()
        .any(|issue| {
            issue["code"] == json!("workflow_authority_denied")
                && issue["message"]
                    .as_str()
                    .is_some_and(|message| message.contains("author identity is required"))
        }));
}

#[tokio::test]
async fn dry_run_previews_transitions_without_mutating_state() {
    let state = test_state().await;
    let app = app_router(state.clone());
    publish_orchestration(&app, &state).await;

    let (status, allowed) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations/orch-goals/dry-run",
            Some(json!({
                "from_node_id": "plan",
                "transition_key": "continue",
                "artifact_type": "plan",
                "version": 1,
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(allowed["allowed"], json!(true));
    assert_eq!(allowed["target"]["node_id"], json!("execute"));

    let (status, rejected) = dispatch(
        &app,
        local_request(
            "POST",
            "/orchestrations/orch-goals/dry-run",
            Some(json!({
                "from_node_id": "plan",
                "transition_key": "abort",
                "version": 1,
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rejected["allowed"], json!(false));
}

#[tokio::test]
async fn goal_start_is_idempotent_and_lifecycle_is_governed() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    let app = app_router(state.clone());
    publish_orchestration(&app, &state).await;

    let start = json!({
        "orchestration_id": "orch-goals",
        "objective": "Ship the plan",
        "idempotency_key": "start-1",
    });
    let (status, first) =
        dispatch(&app, local_request("POST", "/goals", Some(start.clone()))).await;
    assert_eq!(status, StatusCode::CREATED, "{first}");
    assert_eq!(first["replayed"], json!(false));
    let goal_id = first["goal"]["goal_id"]
        .as_str()
        .expect("goal id")
        .to_string();
    let root_run_id = first["root_run_id"].as_str().expect("root run").to_string();
    assert_eq!(first["goal"]["current_node_id"], json!("plan"));

    // Replaying the same idempotency key returns the same goal and root run.
    let (status, replayed) = dispatch(&app, local_request("POST", "/goals", Some(start))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replayed["replayed"], json!(true));
    assert_eq!(replayed["goal"]["goal_id"], json!(goal_id));
    assert_eq!(replayed["root_run_id"], json!(root_run_id));

    // The goal is visible through list/get/graph/budgets read models.
    let (status, listed) = dispatch(&app, local_request("GET", "/goals", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(listed["count"], json!(1));
    let (status, graph) = dispatch(
        &app,
        local_request("GET", format!("/goals/{goal_id}/graph"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let plan_node = graph["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["node_id"] == json!("plan"))
        .expect("plan node");
    assert_eq!(plan_node["state"], json!("current"));
    assert_eq!(graph["current_workflow"]["run_id"], json!(root_run_id));
    let (status, budgets) = dispatch(
        &app,
        local_request("GET", format!("/goals/{goal_id}/budgets"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(budgets["budgets"]["remaining"]["hops"], json!(5));

    // Pause blocks; resume restores; both are durable events.
    let (status, paused) = dispatch(
        &app,
        local_request("POST", format!("/goals/{goal_id}/pause"), Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(paused["outcome"], json!("paused"));
    let (status, resumed) = dispatch(
        &app,
        local_request("POST", format!("/goals/{goal_id}/resume"), Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(resumed["outcome"], json!("resumed"));

    // The durable event read model pages by cursor with no gaps or repeats.
    let (status, all_events) = dispatch(
        &app,
        local_request("GET", format!("/goals/{goal_id}/events"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let events = all_events["events"].as_array().unwrap();
    let kinds = events
        .iter()
        .map(|row| row["event"]["event_type"].as_str().unwrap_or_default())
        .collect::<Vec<_>>();
    assert_eq!(
        kinds,
        vec![
            "stateful_runtime.goal.started",
            "stateful_runtime.goal.paused",
            "stateful_runtime.goal.resumed",
        ]
    );
    let first_cursor = events[0]["cursor"].as_i64().unwrap();
    let (status, after) = dispatch(
        &app,
        local_request(
            "GET",
            format!("/goals/{goal_id}/events?cursor={first_cursor}"),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        after["count"],
        json!(2),
        "cursor replay must skip delivered events"
    );

    // Cancellation is terminal; later mutations are rejected as conflicts.
    let (status, cancelled) = dispatch(
        &app,
        local_request("POST", format!("/goals/{goal_id}/cancel"), Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{cancelled}");
    let (status, blocked) = dispatch(
        &app,
        local_request("POST", format!("/goals/{goal_id}/pause"), Some(json!({}))),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{blocked}");
    assert_eq!(blocked["error"], json!("goal_terminal"));

    // Cross-tenant reads fail closed.
    let (status, _) = dispatch(
        &app,
        orchestration_request("GET", format!("/goals/{goal_id}"), "acme", "hq", None),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

/// TAN-705: artifact admission policy on the emit surface — traversal and
/// unresolvable content paths, symlink escapes, forged digests, and oversized
/// inline values are all rejected before a transition is attempted.
#[tokio::test]
async fn artifact_admission_policy_rejects_unsafe_content() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    let app = app_router(state.clone());
    publish_orchestration(&app, &state).await;

    let (status, started) = dispatch(
        &app,
        local_request(
            "POST",
            "/goals",
            Some(json!({
                "orchestration_id": "orch-goals",
                "objective": "Ship the plan",
                "idempotency_key": "start-artifact-policy",
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let goal_id = started["goal"]["goal_id"].as_str().unwrap().to_string();
    let root_run_id = started["root_run_id"].as_str().unwrap().to_string();
    complete_run(&state, &root_run_id).await;

    let emit = |artifact: Value, key: &str| {
        json!({
            "transition_key": "continue",
            "idempotency_key": key,
            "artifact": artifact,
        })
    };

    // Path traversal is rejected before any transition work happens.
    let (status, body) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/transitions"),
            Some(emit(
                json!({
                    "artifact_type": "plan",
                    "content_path": "../../etc/passwd",
                    "content_digest": format!("sha256:{}", "0".repeat(64)),
                }),
                "hop-traversal",
            )),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert_eq!(body["error"], json!("artifact_policy_violation"));

    // A content path that resolves to nothing is not provenance.
    let (status, body) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/transitions"),
            Some(emit(
                json!({
                    "artifact_type": "plan",
                    "content_path": "does/not/exist.md",
                    "content_digest": format!("sha256:{}", "0".repeat(64)),
                }),
                "hop-missing",
            )),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");

    // A real workspace file with a forged digest is rejected; the correct
    // digest commits.
    let workspace_root = state.workspace_index.snapshot().await.root;
    let relative_path = format!("target/tandem-artifact-policy-{}.md", uuid::Uuid::new_v4());
    let absolute_path = std::path::Path::new(&workspace_root).join(&relative_path);
    std::fs::create_dir_all(absolute_path.parent().unwrap()).unwrap();
    std::fs::write(&absolute_path, b"the plan").unwrap();
    let (status, body) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/transitions"),
            Some(emit(
                json!({
                    "artifact_type": "plan",
                    "content_path": relative_path,
                    "content_digest": format!("sha256:{}", "f".repeat(64)),
                }),
                "hop-forged",
            )),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
    assert!(body["detail"]
        .as_str()
        .unwrap_or_default()
        .contains("digest mismatch"));

    // Oversized inline values are rejected by the structural admission bound.
    let oversized = "x".repeat(300 * 1024);
    let (status, body) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/transitions"),
            Some(emit(
                json!({"artifact_type": "plan", "value": oversized}),
                "hop-oversized",
            )),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{body}");
    assert!(body["detail"]
        .as_str()
        .unwrap_or_default()
        .contains("admission bound"));

    let digest = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(b"the plan"))
    };
    let (status, committed) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/transitions"),
            Some(emit(
                json!({
                    "artifact_type": "plan",
                    "content_path": relative_path,
                    "content_digest": format!("sha256:{digest}"),
                }),
                "hop-verified",
            )),
        ),
    )
    .await;
    let _ = std::fs::remove_file(&absolute_path);
    assert_eq!(status, StatusCode::OK, "{committed}");
    assert_eq!(committed["outcome"], json!("committed"));
}

#[tokio::test]
async fn governed_transitions_flow_through_the_public_api() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    let app = app_router(state.clone());
    publish_orchestration(&app, &state).await;

    let (status, started) = dispatch(
        &app,
        local_request(
            "POST",
            "/goals",
            Some(json!({
                "orchestration_id": "orch-goals",
                "objective": "Ship the plan",
                "idempotency_key": "start-transitions",
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let goal_id = started["goal"]["goal_id"].as_str().unwrap().to_string();
    let root_run_id = started["root_run_id"].as_str().unwrap().to_string();

    // Simulate the planner workflow completing so the governed transition has
    // a completed source run to hand off from.
    complete_run(&state, &root_run_id).await;

    // Emit the governed plan -> execute transition.
    let emit = json!({
        "transition_key": "continue",
        "idempotency_key": "hop-1",
        "artifact": {"artifact_type": "plan", "value": {"steps": ["ship"]}},
    });
    let (status, committed) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/transitions"),
            Some(emit.clone()),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{committed}");
    assert_eq!(committed["outcome"], json!("committed"));
    assert_eq!(committed["commit"], json!("Committed"));
    let downstream_run_id = committed["downstream_run_id"].as_str().unwrap().to_string();

    // Replaying the same idempotency key is a no-op commit.
    let (status, replayed) = dispatch(
        &app,
        local_request("POST", format!("/goals/{goal_id}/transitions"), Some(emit)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(replayed["commit"], json!("AlreadyCommitted"));
    assert_eq!(replayed["downstream_run_id"], json!(downstream_run_id));

    // Lineage, handoffs, and artifacts are all served from the durable store.
    let (status, runs) = dispatch(
        &app,
        local_request("GET", format!("/goals/{goal_id}/runs"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(runs["count"], json!(2));
    let (status, handoffs) = dispatch(
        &app,
        local_request("GET", format!("/goals/{goal_id}/handoffs"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(handoffs["count"], json!(1));
    assert_eq!(handoffs["handoffs"][0]["status"], json!("consumed"));
    let (status, artifacts) = dispatch(
        &app,
        local_request("GET", format!("/goals/{goal_id}/artifacts"), None),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        artifacts["artifacts"][0]["artifact"]["artifact_type"],
        json!("plan")
    );

    // The executor workflow completes before settling into the terminal node.
    complete_run(&state, &downstream_run_id).await;

    // Settle the executor's completion into the terminal node.
    let (status, terminal) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/completion"),
            Some(json!({"transition_key": "complete"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{terminal}");
    assert_eq!(terminal["outcome"], json!("terminal"));
    assert_eq!(terminal["goal"]["status"], json!("completed"));

    // Terminal goals reject further transition emissions.
    let (status, rejected) = dispatch(
        &app,
        local_request(
            "POST",
            format!("/goals/{goal_id}/transitions"),
            Some(json!({
                "transition_key": "continue",
                "idempotency_key": "hop-2",
                "artifact": {"artifact_type": "plan"},
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT, "{rejected}");
}
