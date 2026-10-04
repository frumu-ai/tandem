// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

async fn goal_actor_context(
    mut request: Request<Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let actor = request
        .headers()
        .get("x-test-goal-actor")
        .and_then(|value| value.to_str().ok())
        .expect("test actor")
        .to_string();
    let tenant =
        TenantContext::explicit_user_workspace("org-a", "dep-a", Some("dep-a".to_string()), &actor);
    let mut verified = verified_context(&actor);
    verified.tenant_context = tenant.clone();
    if request
        .headers()
        .contains_key("x-test-hosted-policy-version")
    {
        verified.policy_version = Some(1);
        if actor == "other" {
            verified.org_units.push("executors".to_string());
        }
    }
    if actor == "administrator" {
        verified.roles.push(
            if verified.policy_version.is_some() {
                "hosted:admin"
            } else {
                "admin"
            }
            .to_string(),
        );
    }
    if actor == "viewer" {
        verified.roles.push("hosted:role:viewer".to_string());
        verified.capabilities =
            tandem_enterprise_contract::hosted_policy::role_capabilities("viewer")
                .into_iter()
                .map(str::to_string)
                .collect();
        verified.policy_version = Some(1);
    }
    if actor == "grantee" {
        let goal_id = request
            .headers()
            .get("x-test-goal-grant")
            .and_then(|value| value.to_str().ok())
            .expect("goal grant");
        let resource = ResourceRef::new("org-a", "dep-a", ResourceKind::Run, goal_id);
        let principal = PrincipalRef::human_user(&actor);
        verified.strict_projection = Some(
            tandem_types::StrictTenantContext::new(
                tenant.clone(),
                principal.clone(),
                verified.authority_chain.clone(),
                ResourceScope::root(resource.clone()),
                tandem_types::AssertionMetadata::new(
                    "tandem-web",
                    "tandem-runtime",
                    1_000,
                    9_999_999_999_999,
                    "goal-grantee",
                ),
            )
            .with_grants(vec![tandem_types::ScopedGrant::new(
                "goal-read",
                principal,
                resource,
                tandem_types::GrantSource::Direct,
            )
            .with_permissions(vec![AccessPermission::Read])
            .with_data_classes(vec![tandem_types::DataClass::Internal])])
            .with_data_boundary(tandem_types::DataBoundary::allow(vec![
                tandem_types::DataClass::Internal,
            ])),
        );
    }
    if let Some(orchestration_id) = request
        .headers()
        .get("x-test-goal-orchestration-execute")
        .and_then(|value| value.to_str().ok())
    {
        let resource = ResourceRef::new(
            "org-a",
            "dep-a",
            ResourceKind::Orchestration,
            orchestration_id,
        );
        let principal = PrincipalRef::human_user(&actor);
        verified.strict_projection = Some(
            tandem_types::StrictTenantContext::new(
                tenant.clone(),
                principal.clone(),
                verified.authority_chain.clone(),
                ResourceScope::root(resource.clone()),
                tandem_types::AssertionMetadata::new(
                    "tandem-web",
                    "tandem-runtime",
                    1_000,
                    9_999_999_999_999,
                    "orchestration-executor",
                ),
            )
            .with_grants(vec![tandem_types::ScopedGrant::new(
                "orchestration-execute",
                principal,
                resource,
                tandem_types::GrantSource::Direct,
            )
            .with_permissions(vec![AccessPermission::Execute])
            .with_data_classes(vec![tandem_types::DataClass::Internal])])
            .with_data_boundary(tandem_types::DataBoundary::allow(vec![
                tandem_types::DataClass::Internal,
            ])),
        );
    }
    request.extensions_mut().insert(tenant);
    request
        .extensions_mut()
        .insert(tandem_types::RequestPrincipal::authenticated_user(
            &actor,
            "tandem-web",
        ));
    request.extensions_mut().insert(verified);
    next.run(request).await
}

fn goal_actor_request(actor: &str, path: impl Into<String>, grant: Option<&str>) -> Request<Body> {
    let mut builder = Request::builder()
        .method("GET")
        .uri(path.into())
        .header("x-test-goal-actor", actor);
    if let Some(goal_id) = grant {
        builder = builder.header("x-test-goal-grant", goal_id);
    }
    builder.body(Body::empty()).unwrap()
}

fn goal_actor_post(actor: &str, path: impl Into<String>, body: Value) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(path.into())
        .header("x-test-goal-actor", actor)
        .header("content-type", "application/json")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn goal_actor_post_with_execute(
    actor: &str,
    path: impl Into<String>,
    body: Value,
    orchestration_id: &str,
) -> Request<Body> {
    let mut request = goal_actor_post(actor, path, body);
    request.headers_mut().insert(
        "x-test-goal-orchestration-execute",
        orchestration_id.parse().unwrap(),
    );
    request
}

fn goal_actor_post_hosted(actor: &str, body: Value) -> Request<Body> {
    let mut request = goal_actor_post(actor, "/goals", body);
    request
        .headers_mut()
        .insert("x-test-hosted-policy-version", "1".parse().unwrap());
    request
}

#[tokio::test]
async fn hosted_goal_reads_are_actor_scoped_across_models_and_stream() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    let resource_tenant = TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".to_string()),
        "operator",
    );
    let mut planner = AutomationSpecBuilder::new("planner").build();
    planner.set_tenant_context(&resource_tenant);
    let planner = state.put_automation_v2(planner).await.unwrap();
    let mut executor = AutomationSpecBuilder::new("executor").build();
    executor.set_tenant_context(&resource_tenant);
    let executor = state.put_automation_v2(executor).await.unwrap();
    assert_eq!(planner.tenant_context(), resource_tenant);
    assert_eq!(executor.tenant_context(), resource_tenant);
    assert_eq!(
        state
            .get_automation_v2("planner")
            .await
            .unwrap()
            .tenant_context(),
        resource_tenant,
    );
    let planner_hash = automation_definition_snapshot_hash(&planner);
    let executor_hash = automation_definition_snapshot_hash(&executor);

    let hosted_app = Router::new()
        .route(
            "/orchestrations",
            axum::routing::post(crate::http::orchestrations_api::create_orchestration_draft),
        )
        .route(
            "/orchestrations/{id}/publish",
            axum::routing::post(crate::http::orchestrations_api::publish_orchestration),
        )
        .route(
            "/goals",
            axum::routing::get(crate::http::goals_api::list_goals)
                .post(crate::http::goals_api::start_goal),
        )
        .route(
            "/goals/{goal_id}",
            axum::routing::get(crate::http::goals_api::get_goal),
        )
        .route(
            "/goals/{goal_id}/projection",
            axum::routing::get(crate::http::goals_projection::get_goal_projection),
        )
        .route(
            "/goals/{goal_id}/graph",
            axum::routing::get(crate::http::goals_api::get_goal_graph),
        )
        .route(
            "/goals/{goal_id}/runs",
            axum::routing::get(crate::http::goals_api::list_goal_runs),
        )
        .route(
            "/goals/{goal_id}/events",
            axum::routing::get(crate::http::goals_api::list_goal_events),
        )
        .route(
            "/goals/{goal_id}/events/stream",
            axum::routing::get(crate::http::goals_api::stream_goal_events),
        )
        .route(
            "/goals/{goal_id}/artifacts",
            axum::routing::get(crate::http::goals_api::list_goal_artifacts),
        )
        .route(
            "/goals/{goal_id}/budgets",
            axum::routing::get(crate::http::goals_api::get_goal_budgets),
        )
        .route(
            "/goals/{goal_id}/handoffs",
            axum::routing::get(crate::http::goals_api::list_goal_handoffs),
        )
        .route(
            "/goals/{goal_id}/waits",
            axum::routing::get(crate::http::goals_api::list_goal_waits),
        )
        .route(
            "/goals/{goal_id}/waits/{wait_id}",
            axum::routing::get(crate::http::goals_api::get_goal_wait),
        )
        .layer(axum::middleware::from_fn(goal_actor_context))
        .with_state(state.clone());

    let (status, draft) = dispatch(
        &hosted_app,
        goal_actor_post(
            "operator",
            "/orchestrations",
            draft_payload(&planner_hash, &executor_hash),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    let (status, published) = dispatch(
        &hosted_app,
        goal_actor_post(
            "operator",
            "/orchestrations/orch-goals/publish",
            Value::Null,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{published}");
    let (status, denied_start) = dispatch(
        &hosted_app,
        goal_actor_post(
            "outsider",
            "/goals",
            json!({
                "orchestration_id": "orch-goals",
                "objective": "must not start a private orchestration",
                "idempotency_key": "outsider-private-goal",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{denied_start}");
    let (status, started) = dispatch(
        &hosted_app,
        goal_actor_post(
            "operator",
            "/goals",
            json!({
                "orchestration_id": "orch-goals",
                "objective": "private objective",
                "idempotency_key": "actor-private-goal",
            }),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{started}");
    let goal_id = started["goal"]["goal_id"].as_str().unwrap().to_string();

    let (status, owner_list) =
        dispatch(&hosted_app, goal_actor_request("operator", "/goals", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_list["count"], 1);
    let (status, other_list) =
        dispatch(&hosted_app, goal_actor_request("other", "/goals", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        other_list["count"], 0,
        "another actor listed private goals: {other_list}"
    );
    for suffix in [
        "",
        "/projection",
        "/graph",
        "/runs",
        "/events",
        "/events/stream",
        "/artifacts",
        "/budgets",
        "/handoffs",
        "/waits",
        "/waits/missing",
    ] {
        let path = format!("/goals/{goal_id}{suffix}");
        let response = hosted_app
            .clone()
            .oneshot(goal_actor_request("other", &path, None))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::NOT_FOUND,
            "other actor read {path}"
        );
    }
    for actor in ["operator", "administrator", "grantee"] {
        let grant = (actor == "grantee").then_some(goal_id.as_str());
        let response = hosted_app
            .clone()
            .oneshot(goal_actor_request(
                actor,
                format!("/goals/{goal_id}"),
                grant,
            ))
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{actor} should see the goal"
        );
    }
    let replay = hosted_app
        .clone()
        .oneshot(goal_actor_post(
            "other",
            "/goals",
            json!({
                "orchestration_id": "orch-goals",
                "objective": "private objective",
                "idempotency_key": "actor-private-goal",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(
        replay.status(),
        StatusCode::NOT_FOUND,
        "another actor replayed the goal start"
    );

    // The newest row belongs to another actor. Visibility must be applied
    // before limit=1, including for a scoped reader of the older goal.
    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    let (status, other_started) = dispatch(
        &hosted_app,
        goal_actor_post_with_execute(
            "other",
            "/goals",
            json!({
                "orchestration_id": "orch-goals",
                "objective": "other actor objective",
                "idempotency_key": "other-private-goal",
            }),
            "orch-goals",
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{other_started}");
    let other_goal_id = other_started["goal"]["goal_id"].as_str().unwrap();
    let (status, replay_after_source_grant_removed) = dispatch(
        &hosted_app,
        goal_actor_post(
            "other",
            "/goals",
            json!({
                "orchestration_id": "orch-goals",
                "objective": "other actor objective",
                "idempotency_key": "other-private-goal",
            }),
        ),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "{replay_after_source_grant_removed}"
    );
    assert_eq!(replay_after_source_grant_removed["replayed"], true);
    let (status, scoped_page) = dispatch(
        &hosted_app,
        goal_actor_request("grantee", "/goals?limit=1", Some(&goal_id)),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{scoped_page}");
    assert_eq!(scoped_page["count"], 1, "{scoped_page}");
    assert_eq!(scoped_page["goals"][0]["goal_id"], goal_id);
    let (status, other_page) = dispatch(
        &hosted_app,
        goal_actor_request("other", "/goals?limit=1", None),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{other_page}");
    assert_eq!(other_page["goals"][0]["goal_id"], other_goal_id);

    // Local test ingress may deliberately trust explicit tenant headers
    // without a signed assertion. A configured hosted source must never use
    // that fallback, even while its policy snapshot is unavailable.
    let stored_goal: tandem_automation::LongRunningGoal =
        serde_json::from_value(started["goal"].clone()).unwrap();
    let local_context = crate::http::goals_authority::current_goal_context(
        &state,
        &resource_tenant,
        None,
        AccessPermission::HostedAutomationRead,
    )
    .await;
    assert!(matches!(local_context, Ok(None)));
    assert!(crate::http::goals_authority::can_inspect_goal(
        &state,
        &resource_tenant,
        None,
        &stored_goal,
    ));
    let actor = PrincipalRef::human_user("operator");
    assert!(crate::http::goals_authority::require_goal_owner(
        &state,
        &resource_tenant,
        None,
        &stored_goal,
        &actor,
    )
    .is_ok());
    state.enterprise.hosted_policy.configure_test_source(
        "org-a",
        "dep-a",
        directory.path().join("unsynchronized-policy.json"),
    );
    assert!(crate::http::goals_authority::current_goal_context(
        &state,
        &resource_tenant,
        None,
        AccessPermission::HostedAutomationRead,
    )
    .await
    .is_err());
    assert!(!crate::http::goals_authority::can_inspect_goal(
        &state,
        &resource_tenant,
        None,
        &stored_goal,
    ));
    assert!(crate::http::goals_authority::require_goal_owner(
        &state,
        &resource_tenant,
        None,
        &stored_goal,
        &actor,
    )
    .is_err());
}

#[tokio::test]
async fn hosted_goal_start_requires_live_orchestration_execute_or_owner() {
    let directory = tempfile::tempdir().unwrap();
    let mut state = test_state().await;
    state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
    let tenant = TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".to_string()),
        "operator",
    );
    let mut planner = AutomationSpecBuilder::new("planner").build();
    planner.set_tenant_context(&tenant);
    let planner = state.put_automation_v2(planner).await.unwrap();
    let mut executor = AutomationSpecBuilder::new("executor").build();
    executor.set_tenant_context(&tenant);
    let executor = state.put_automation_v2(executor).await.unwrap();
    let app = Router::new()
        .route(
            "/orchestrations",
            axum::routing::post(crate::http::orchestrations_api::create_orchestration_draft),
        )
        .route(
            "/orchestrations/{id}/publish",
            axum::routing::post(crate::http::orchestrations_api::publish_orchestration),
        )
        .route(
            "/goals",
            axum::routing::post(crate::http::goals_api::start_goal),
        )
        .layer(axum::middleware::from_fn(goal_actor_context))
        .with_state(state.clone());
    let (status, draft) = dispatch(
        &app,
        goal_actor_post(
            "operator",
            "/orchestrations",
            draft_payload(
                &automation_definition_snapshot_hash(&planner),
                &automation_definition_snapshot_hash(&executor),
            ),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{draft}");
    let (status, published) = dispatch(
        &app,
        goal_actor_post(
            "operator",
            "/orchestrations/orch-goals/publish",
            Value::Null,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{published}");

    let now = crate::now_ms();
    let member_capabilities =
        tandem_enterprise_contract::hosted_policy::role_capabilities("member");
    let bundle = tandem_enterprise_contract::hosted_policy::HostedPolicyBundle::from_json(
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "policy_version": 1,
            "organization_id": "org-a",
            "deployment_id": "dep-a",
            "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
            "users": [
                {"id":"operator", "email":null, "username":null, "role":"member", "is_active":true, "email_verified":true, "capabilities":member_capabilities},
                {"id":"other", "email":null, "username":null, "role":"member", "is_active":true, "email_verified":true, "capabilities":member_capabilities},
                {"id":"outsider", "email":null, "username":null, "role":"member", "is_active":true, "email_verified":true, "capabilities":member_capabilities},
                {"id":"administrator", "email":null, "username":null, "role":"admin", "is_active":true, "email_verified":true, "capabilities":tandem_enterprise_contract::hosted_policy::role_capabilities("admin")},
            ],
            "org_units": [{"id":"executors", "slug":"executors", "display_name":"Executors", "kind":"team", "state":"active"}],
            "org_unit_memberships": [{"unit_id":"executors", "user_id":"other"}],
            "deployment_grants": [
                {"id":"operator-use", "deployment_id":"dep-a", "principal_kind":"member", "principal_id":"operator", "resource_kind":"deployment", "resource_id":"dep-a", "permissions":["hosted.use"]},
                {"id":"other-use", "deployment_id":"dep-a", "principal_kind":"member", "principal_id":"other", "resource_kind":"deployment", "resource_id":"dep-a", "permissions":["hosted.use"]},
                {"id":"outsider-use", "deployment_id":"dep-a", "principal_kind":"member", "principal_id":"outsider", "resource_kind":"deployment", "resource_id":"dep-a", "permissions":["hosted.use"]},
                {"id":"admin-use", "deployment_id":"dep-a", "principal_kind":"member", "principal_id":"administrator", "resource_kind":"deployment", "resource_id":"dep-a", "permissions":["hosted.use", "hosted.admin"]},
            ],
        }))
        .unwrap()
        .as_slice(),
    )
    .unwrap();
    state
        .enterprise
        .hosted_policy
        .install_test_bundle(bundle)
        .unwrap();

    let start_body = |key: &str| {
        json!({
            "orchestration_id": "orch-goals",
            "objective": "private hosted objective",
            "idempotency_key": key,
        })
    };
    let mut outsider_absent_response = Value::Null;
    for actor in ["other", "outsider"] {
        let (status, denied) = dispatch(
            &app,
            goal_actor_post_hosted(actor, start_body(&format!("{actor}-denied"))),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{actor}: {denied}");
        if actor == "outsider" {
            outsider_absent_response = denied;
        }
    }
    let (status, owner) = dispatch(
        &app,
        goal_actor_post_hosted("operator", start_body("hosted-owner")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{owner}");
    let (status, admin) = dispatch(
        &app,
        goal_actor_post_hosted("administrator", start_body("hosted-admin")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{admin}");

    let unit = tandem_enterprise_contract::hosted_policy::hosted_unit_principal("executors");
    let grant_for = |id: &str, kind: ResourceKind, resource_id: &str| {
        OrganizationUnitAccessGrant::active(
            id,
            tenant.clone(),
            unit.clone(),
            ResourceRef::new("org-a", "dep-a", kind, resource_id),
            now,
        )
        .with_permissions(vec![AccessPermission::Execute])
        .with_data_classes(vec![tandem_types::DataClass::Internal])
    };
    let grants = &state.enterprise.org_unit_access_grants;
    grants.write().await.insert(
        "goal-source-grant".to_string(),
        grant_for("alias-project", ResourceKind::Project, "orch-goals"),
    );
    let (status, alias_denied) = dispatch(
        &app,
        goal_actor_post_hosted("other", start_body("alias-denied")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{alias_denied}");
    grants.write().await.insert(
        "goal-source-grant".to_string(),
        grant_for(
            "exact-orchestration",
            ResourceKind::Orchestration,
            "orch-goals",
        ),
    );
    let (status, granted) = dispatch(
        &app,
        goal_actor_post_hosted("other", start_body("hosted-granted")),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{granted}");

    let other_tenant = TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".to_string()),
        "other",
    );
    let mut other_verified = verified_context("other");
    other_verified.tenant_context = other_tenant.clone();
    other_verified.policy_version = Some(1);
    other_verified.org_units.push("executors".to_string());
    let store = crate::stateful_runtime::OrchestrationStateStore::from_automation_runs_path(
        &state.automation_v2_runs_path,
    )
    .unwrap();
    let source = store
        .get_orchestration_for_tenant(&other_tenant, "orch-goals", 1)
        .unwrap()
        .unwrap();
    let mut owner_verified = verified_context("operator");
    owner_verified.tenant_context = tenant.clone();
    owner_verified.policy_version = Some(1);
    let admin_tenant = TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".to_string()),
        "administrator",
    );
    let mut admin_verified = verified_context("administrator");
    admin_verified.tenant_context = admin_tenant.clone();
    admin_verified.policy_version = Some(1);
    let unrelated_grant_writer = grants.write().await;
    for (actor_tenant, actor_verified) in
        [(&tenant, &owner_verified), (&admin_tenant, &admin_verified)]
    {
        state
            .with_goal_start_commit_authority(
                actor_tenant,
                Some(actor_verified),
                &source,
                || Ok(()),
            )
            .expect("owner and current admin do not depend on unrelated grant writers");
    }
    drop(unrelated_grant_writer);
    let stale = crate::http::goals_authority::current_goal_context(
        &state,
        &other_tenant,
        Some(&other_verified),
        AccessPermission::HostedUse,
    )
    .await
    .unwrap();
    assert!(state.can_start_goal_from_orchestration(&other_tenant, stale.as_ref(), &source,));
    state
        .with_goal_start_commit_authority(&other_tenant, Some(&other_verified), &source, || {
            assert!(
                grants.try_write().is_err(),
                "the live object grant must stay locked through commit"
            );
            Ok(())
        })
        .expect("current Execute grant permits a guarded start");
    grants.write().await.insert(
        "goal-source-deny".to_string(),
        grant_for("exact-orchestration-deny", ResourceKind::Orchestration, "orch-goals")
            .with_effect(AccessEffect::Deny),
    );
    let mut committed_with_live_deny = false;
    assert!(state
        .with_goal_start_commit_authority(&other_tenant, Some(&other_verified), &source, || {
            committed_with_live_deny = true;
            Ok(())
        })
        .is_err());
    assert!(!committed_with_live_deny, "a live Deny must override Execute");
    grants.write().await.remove("goal-source-deny");
    grants.write().await.remove("goal-source-grant");
    // This old async projection still contains Execute; the final synchronous
    // projection must not admit a new start after the live grant is removed.
    assert!(state.can_start_goal_from_orchestration(&other_tenant, stale.as_ref(), &source,));
    let fresh = state
        .current_goal_start_context_before_commit(&other_tenant, Some(&other_verified))
        .unwrap();
    assert!(!state.can_start_goal_from_orchestration(&other_tenant, fresh.as_ref(), &source,));
    let mut committed_after_revocation = false;
    assert!(state
        .with_goal_start_commit_authority(&other_tenant, Some(&other_verified), &source, || {
            committed_after_revocation = true;
            Ok(())
        })
        .is_err());
    assert!(!committed_after_revocation);
    let (status, new_start_denied) = dispatch(
        &app,
        goal_actor_post_hosted("other", start_body("grant-revoked-new")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{new_start_denied}");
    let (status, replay) = dispatch(
        &app,
        goal_actor_post_hosted("other", start_body("hosted-granted")),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{replay}");
    assert_eq!(replay["replayed"], true);
    let (status, private_replay) = dispatch(
        &app,
        goal_actor_post_hosted("outsider", start_body("hosted-owner")),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND, "{private_replay}");
    assert_eq!(private_replay, outsider_absent_response);
    for version in [None, Some(1)] {
        let (status, absent_source) = dispatch(
            &app,
            goal_actor_post_hosted(
                "outsider",
                json!({
                    "orchestration_id": "missing-goal-source",
                    "orchestration_version": version,
                    "objective": "private hosted objective",
                    "idempotency_key": "missing-source-key",
                }),
            ),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{absent_source}");
        assert_eq!(absent_source, outsider_absent_response);
    }
}

#[tokio::test]
async fn hosted_goal_viewer_read_grant_does_not_allow_start() {
    let state = test_state().await;
    let now = crate::now_ms();
    let bundle = tandem_enterprise_contract::hosted_policy::HostedPolicyBundle::from_json(
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "policy_version": 1,
            "organization_id": "org-a",
            "deployment_id": "dep-a",
            "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
            "users": [{
                "id": "viewer", "email": null, "username": null,
                "role": "viewer", "is_active": true, "email_verified": true,
                "capabilities": tandem_enterprise_contract::hosted_policy::role_capabilities("viewer"),
            }],
            "org_units": [],
            "org_unit_memberships": [],
            "deployment_grants": [{
                "id": "viewer-read", "deployment_id": "dep-a",
                "principal_kind": "member", "principal_id": "viewer",
                "resource_kind": "deployment", "resource_id": "dep-a",
                "permissions": ["automation.read"],
            }],
        }))
        .unwrap()
        .as_slice(),
    )
    .unwrap();
    state
        .enterprise
        .hosted_policy
        .install_test_bundle(bundle)
        .unwrap();
    let app = Router::new()
        .route(
            "/goals",
            axum::routing::get(crate::http::goals_api::list_goals)
                .post(crate::http::goals_api::start_goal),
        )
        .layer(axum::middleware::from_fn(goal_actor_context))
        .with_state(state);

    let (status, listing) = dispatch(&app, goal_actor_request("viewer", "/goals", None)).await;
    assert_eq!(status, StatusCode::OK, "viewer read grant: {listing}");
    let response = app
        .oneshot(goal_actor_post(
            "viewer",
            "/goals",
            json!({
                "orchestration_id": "not-present",
                "objective": "must not start",
                "idempotency_key": "viewer-mutation",
            }),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn hosted_goal_routes_require_live_read_or_use_grants() {
    use axum::routing::any;
    for pattern in [
        "/goals",
        "/goals/{goal_id}",
        "/goals/{goal_id}/projection",
        "/goals/{goal_id}/graph",
        "/goals/{goal_id}/runs",
        "/goals/{goal_id}/events",
        "/goals/{goal_id}/events/stream",
        "/goals/{goal_id}/artifacts",
        "/goals/{goal_id}/budgets",
        "/goals/{goal_id}/handoffs",
        "/goals/{goal_id}/waits",
        "/goals/{goal_id}/waits/{wait_id}",
    ] {
        for method in ["GET", "HEAD"] {
            let app = Router::new().route(
                pattern,
                any(move |request: Request<Body>| async move {
                    assert_eq!(
                        crate::http::hosted_route_authority::required_permission(&request),
                        Some(AccessPermission::HostedAutomationRead),
                        "{method} {}",
                        request.uri(),
                    );
                    StatusCode::NO_CONTENT
                }),
            );
            let response = app
                .oneshot(
                    Request::builder()
                        .method(method)
                        .uri(
                            pattern
                                .replace("{goal_id}", "goal-a")
                                .replace("{wait_id}", "wait-a"),
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NO_CONTENT);
        }
    }
    for pattern in [
        "/goals",
        "/goals/{goal_id}/actions/{action_id}",
        "/goals/{goal_id}/pause",
        "/goals/{goal_id}/resume",
        "/goals/{goal_id}/cancel",
        "/goals/{goal_id}/transitions",
        "/goals/{goal_id}/completion",
        "/goals/{goal_id}/handoffs/{handoff_id}/decision",
        "/goals/{goal_id}/waits/{wait_id}/resolve",
    ] {
        let app = Router::new().route(
            pattern,
            any(|request: Request<Body>| async move {
                assert_eq!(
                    crate::http::hosted_route_authority::required_permission(&request),
                    Some(AccessPermission::HostedUse),
                    "POST {}",
                    request.uri(),
                );
                StatusCode::NO_CONTENT
            }),
        );
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(
                        pattern
                            .replace("{goal_id}", "goal-a")
                            .replace("{action_id}", "pause")
                            .replace("{handoff_id}", "handoff-a")
                            .replace("{wait_id}", "wait-a"),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }
}
