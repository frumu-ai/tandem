// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

fn optimization_verified_actor(actor: &str) -> tandem_types::VerifiedTenantContext {
    let now = crate::now_ms();
    let tenant = tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".to_string()),
        actor,
    );
    let claims = tandem_types::TenantContextAssertionClaims::new_v1(
        "tandem-web",
        "tandem-runtime",
        now,
        now + 60_000,
        format!("optimization-{actor}"),
        tenant,
        tandem_types::HumanActor::tandem_user(actor),
        tandem_types::AuthorityChain::from_request(
            tandem_types::RequestPrincipal::authenticated_user(actor, "tandem-web"),
        ),
        Vec::new(),
    );
    tandem_types::VerifiedTenantContext::from(claims)
}

#[tokio::test]
async fn hosted_group_audience_uses_current_unit_membership() {
    let state = test_state().await;
    let now = crate::now_ms();
    let bundle = tandem_enterprise_contract::hosted_policy::HostedPolicyBundle::from_json(
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "policy_version": 1,
            "organization_id": "org-a",
            "deployment_id": "dep-a",
            "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
            "users": [
                {"id": "alice", "email": null, "username": null, "role": "member", "capabilities": [], "is_active": true, "email_verified": true},
                {"id": "bob", "email": null, "username": null, "role": "member", "capabilities": ["automation.read"], "is_active": true, "email_verified": true}
            ],
            "org_units": [{"id": "eng", "slug": "eng", "display_name": "Engineering", "kind": "department", "state": "active"}],
            "org_unit_memberships": [{"unit_id": "eng", "user_id": "bob"}],
            "deployment_grants": []
        }))
        .unwrap()
        .as_slice(),
    )
    .unwrap();
    state
        .enterprise
        .hosted_policy
        .install_test_bundle(bundle.clone())
        .unwrap();

    let workspace = tempfile::tempdir().unwrap();
    let mut source = sample_automation(workspace.path().to_str().unwrap());
    source.creator_id = "alice".into();
    source.metadata = Some(json!({
        "resource_access": {
            "visibility": "group",
            "owner_principal": {"kind": "human_user", "id": "alice"},
            "audience_principals": ["eng"]
        }
    }));
    source.set_tenant_context(&tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".into()),
        "alice",
    ));
    let mut verified = optimization_verified_actor("bob");
    verified.policy_version = Some(1);
    verified.org_units.push("eng".into());
    verified.capabilities.push("automation.read".into());
    let tenant = verified.tenant_context.clone();

    assert!(crate::http::automation_object_authority::can_read(
        &state,
        &tenant,
        Some(&verified),
        &source,
    ));
    assert!(!crate::http::automation_object_authority::can_write(
        &state,
        &tenant,
        Some(&verified),
        &source,
    ));

    source.metadata.as_mut().unwrap()["resource_access"]["audience_principals"] = json!(["ops"]);
    assert!(!crate::http::automation_object_authority::can_read(
        &state,
        &tenant,
        Some(&verified),
        &source,
    ));
    source.metadata.as_mut().unwrap()["resource_access"]["audience_principals"] = json!(["eng"]);

    let mut removed = bundle.clone();
    removed.policy_version = 2;
    removed.org_unit_memberships.clear();
    state
        .enterprise
        .hosted_policy
        .install_test_bundle(removed)
        .unwrap();
    verified.policy_version = Some(2);
    assert!(!crate::http::automation_object_authority::can_read(
        &state,
        &tenant,
        Some(&verified),
        &source,
    ));

    let mut archived = bundle;
    archived.policy_version = 3;
    archived.org_units[0].state = "archived".into();
    state
        .enterprise
        .hosted_policy
        .install_test_bundle(archived)
        .unwrap();
    verified.policy_version = Some(3);
    assert!(!crate::http::automation_object_authority::can_read(
        &state,
        &tenant,
        Some(&verified),
        &source,
    ));
}

#[tokio::test]
async fn private_optimization_source_accepts_current_scoped_org_unit_grant() {
    use tandem_types::{
        AccessPermission, DataClass, OrganizationUnitAccessGrant, ResourceKind, ResourceRef,
    };

    let state = test_state().await;
    let now = crate::now_ms();
    let bundle = tandem_enterprise_contract::hosted_policy::HostedPolicyBundle::from_json(
        serde_json::to_vec(&json!({
            "schema_version": 1,
            "policy_version": 1,
            "organization_id": "org-a",
            "deployment_id": "dep-a",
            "generated_at": chrono::DateTime::from_timestamp_millis(now as i64).unwrap(),
            "users": [
                {"id": "alice", "email": null, "username": null, "role": "member", "capabilities": ["hosted.use"], "is_active": true, "email_verified": true},
                {"id": "bob", "email": null, "username": null, "role": "member", "capabilities": ["hosted.use"], "is_active": true, "email_verified": true}
            ],
            "org_units": [{"id": "eng", "slug": "eng", "display_name": "Engineering", "kind": "department", "state": "active"}],
            "org_unit_memberships": [{"unit_id": "eng", "user_id": "bob"}],
            "deployment_grants": [{
                "id": "bob-automation", "deployment_id": "dep-a",
                "principal_kind": "member", "principal_id": "bob",
                "resource_kind": "deployment", "resource_id": "dep-a",
                "permissions": ["automation.read", "automation.write", "automation.execute"]
            }]
        }))
        .unwrap()
        .as_slice(),
    )
    .unwrap();
    let mut without_membership = bundle.clone();
    without_membership.policy_version = 2;
    without_membership.org_unit_memberships.clear();
    state
        .enterprise
        .hosted_policy
        .install_test_bundle(bundle)
        .unwrap();

    let workspace = tempfile::tempdir().unwrap();
    let mut source = sample_automation(workspace.path().to_str().unwrap());
    source.creator_id = "alice".into();
    source.metadata = Some(json!({
        "resource_access": {
            "visibility": "private",
            "owner_principal": {"kind": "human_user", "id": "alice"}
        }
    }));
    source.set_tenant_context(&tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".into()),
        "alice",
    ));
    let mut verified = optimization_verified_actor("bob");
    verified.policy_version = Some(1);
    verified.roles.push("hosted:role:member".into());
    verified.org_units.push("eng".into());
    let tenant = verified.tenant_context.clone();
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .insert(
            "eng-workflow".into(),
            OrganizationUnitAccessGrant::active(
                "eng-workflow",
                tenant.clone(),
                tandem_enterprise_contract::hosted_policy::hosted_unit_principal("eng"),
                ResourceRef::new(
                    "org-a",
                    "dep-a",
                    ResourceKind::Automation,
                    &source.automation_id,
                ),
                now,
            )
            .with_permissions(vec![AccessPermission::Read, AccessPermission::Edit])
            .with_data_classes(vec![DataClass::Internal]),
        );

    let memberships = state
        .enterprise
        .hosted_policy
        .project(&mut verified)
        .unwrap();
    crate::http::middleware::enrich_verified_context_with_org_unit_grants(
        &state,
        &mut verified,
        memberships,
    )
    .await;

    assert!(crate::http::automation_object_authority::can_read(
        &state,
        &tenant,
        Some(&verified),
        &source
    ));
    assert!(crate::http::automation_object_authority::can_write(
        &state,
        &tenant,
        Some(&verified),
        &source
    ));
    assert!(
        !crate::http::automation_object_authority::can_execute(
            &state,
            &tenant,
            Some(&verified),
            &source
        ),
        "an Edit-only object grant must not become Execute through deployment authority"
    );
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .remove("eng-workflow");
    assert!(!crate::http::automation_object_authority::can_read(
        &state,
        &tenant,
        Some(&verified),
        &source
    ));
    assert!(!crate::http::automation_object_authority::can_write(
        &state,
        &tenant,
        Some(&verified),
        &source
    ));

    let grant = OrganizationUnitAccessGrant::active(
        "eng-workflow",
        tenant.clone(),
        tandem_enterprise_contract::hosted_policy::hosted_unit_principal("eng"),
        ResourceRef::new(
            "org-a",
            "dep-a",
            ResourceKind::Automation,
            &source.automation_id,
        ),
        now,
    )
    .with_permissions(vec![
        AccessPermission::Read,
        AccessPermission::Edit,
        AccessPermission::Execute,
    ])
    .with_data_classes(vec![DataClass::Internal]);
    let mut grants = state.enterprise.org_unit_access_grants.write().await;
    grants.insert("eng-workflow".into(), grant);
    assert!(
        !crate::http::automation_object_authority::can_read(
            &state,
            &tenant,
            Some(&verified),
            &source
        ),
        "a busy grant store must fail closed without blocking"
    );
    drop(grants);
    assert!(crate::http::automation_object_authority::can_read(
        &state,
        &tenant,
        Some(&verified),
        &source
    ));
    assert!(crate::http::automation_object_authority::can_execute(
        &state,
        &tenant,
        Some(&verified),
        &source
    ));
    state
        .enterprise
        .hosted_policy
        .install_test_bundle(without_membership)
        .unwrap();
    verified.policy_version = Some(2);
    verified.org_units.clear();
    assert!(
        !crate::http::automation_object_authority::can_read(
            &state,
            &tenant,
            Some(&verified),
            &source
        ),
        "a removed hosted membership must not return through the local grant store"
    );
}

#[tokio::test]
async fn local_explicit_optimization_grant_revocation_discards_ingress_projection() {
    use tandem_types::{
        AccessPermission, DataClass, OrganizationUnitAccessGrant, OrganizationUnitMembership,
        OrganizationUnitMembershipSource, PrincipalRef, ResourceKind, ResourceRef, ScopedGrant,
    };

    let state = test_state().await;
    let now = crate::now_ms();
    let workspace = tempfile::tempdir().unwrap();
    let mut source = sample_automation(workspace.path().to_str().unwrap());
    source.creator_id = "alice".into();
    source.metadata = Some(json!({
        "resource_access": {
            "visibility": "private",
            "owner_principal": {"kind": "human_user", "id": "alice"}
        }
    }));
    source.set_tenant_context(&tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".into()),
        "alice",
    ));
    let mut verified = optimization_verified_actor("bob");
    let tenant = verified.tenant_context.clone();
    verified.strict_projection = Some(tandem_types::StrictTenantContext::new(
        tenant.clone(),
        PrincipalRef::human_user("bob"),
        verified.authority_chain.clone(),
        tandem_types::ResourceScope::root(ResourceRef::new(
            "org-a",
            "dep-a",
            ResourceKind::Workspace,
            "dep-a",
        )),
        tandem_types::AssertionMetadata::new(
            "tandem-web",
            "tandem-runtime",
            now,
            now + 60_000,
            "local-bob",
        ),
    ));
    let unit = tandem_enterprise_contract::hosted_policy::hosted_unit_principal("eng");
    state.enterprise.org_unit_memberships.write().await.insert(
        "local-eng-bob".into(),
        OrganizationUnitMembership::active(
            "local-eng-bob",
            tenant.clone(),
            unit.clone(),
            PrincipalRef::human_user("bob"),
            OrganizationUnitMembershipSource::Direct,
            now,
        ),
    );
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .insert(
            "local-eng-workflow".into(),
            OrganizationUnitAccessGrant::active(
                "local-eng-workflow",
                tenant.clone(),
                unit,
                ResourceRef::new(
                    "org-a",
                    "dep-a",
                    ResourceKind::Automation,
                    &source.automation_id,
                ),
                now,
            )
            .with_permissions(vec![AccessPermission::Read, AccessPermission::Edit])
            .with_data_classes(vec![DataClass::Internal]),
        );
    crate::http::middleware::enrich_verified_context_with_org_unit_grants(
        &state,
        &mut verified,
        None,
    )
    .await;
    assert!(crate::http::automation_object_authority::can_read(
        &state,
        &tenant,
        Some(&verified),
        &source,
    ));
    assert!(crate::http::automation_object_authority::can_write(
        &state,
        &tenant,
        Some(&verified),
        &source,
    ));
    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .remove("local-eng-workflow");
    assert!(!crate::http::automation_object_authority::can_read(
        &state,
        &tenant,
        Some(&verified),
        &source,
    ));
    assert!(!crate::http::automation_object_authority::can_write(
        &state,
        &tenant,
        Some(&verified),
        &source,
    ));

    state
        .enterprise
        .org_unit_access_grants
        .write()
        .await
        .insert(
            "local-eng-workflow".into(),
            OrganizationUnitAccessGrant::active(
                "local-eng-workflow",
                tenant.clone(),
                tandem_enterprise_contract::hosted_policy::hosted_unit_principal("eng"),
                ResourceRef::new(
                    "org-a",
                    "dep-a",
                    ResourceKind::Automation,
                    &source.automation_id,
                ),
                now,
            )
            .with_permissions(vec![AccessPermission::Read, AccessPermission::Edit])
            .with_data_classes(vec![DataClass::Internal]),
        );
    state
        .enterprise
        .org_unit_memberships
        .write()
        .await
        .remove("local-eng-bob");
    assert!(
        !crate::http::automation_object_authority::can_read(
            &state,
            &tenant,
            Some(&verified),
            &source,
        ),
        "removed membership must not survive in the ingress projection"
    );

    verified.strict_projection.as_mut().unwrap().grants.push(
        ScopedGrant::new(
            "signed-direct",
            PrincipalRef::human_user("bob"),
            ResourceRef::new(
                "org-a",
                "dep-a",
                ResourceKind::Automation,
                &source.automation_id,
            ),
            tandem_types::GrantSource::Direct,
        )
        .with_permissions(vec![AccessPermission::Read, AccessPermission::Edit])
        .with_data_classes(vec![DataClass::Internal]),
    );
    assert!(
        crate::http::automation_object_authority::can_read(
            &state,
            &tenant,
            Some(&verified),
            &source,
        ),
        "a direct assertion grant remains valid independently of the revoked local grant"
    );
}

#[tokio::test]
async fn optimization_campaigns_are_private_to_the_source_workflow_owner() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("workspace");
    let mut source = sample_automation(workspace.path().to_str().expect("workspace path"));
    source.creator_id = "alice".to_string();
    source.metadata = Some(json!({
        "resource_access": {
            "visibility": "private",
            "owner_principal": {"kind": "human_user", "id": "alice"}
        }
    }));
    source.set_tenant_context(&tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".to_string()),
        "alice",
    ));
    let source = state
        .put_automation_v2(source.clone())
        .await
        .expect("source");

    let artifact = crate::OptimizationFrozenArtifact {
        artifact_ref: "fixture".to_string(),
        resolved_path: String::new(),
        sha256: String::new(),
        size_bytes: 0,
    };
    state
        .put_optimization_campaign(crate::OptimizationCampaignRecord {
            optimization_id: "opt-private".to_string(),
            name: "Private optimization".to_string(),
            target_kind: crate::OptimizationTargetKind::WorkflowV2PromptObjectiveOptimization,
            status: crate::OptimizationCampaignStatus::Draft,
            source_workflow_id: source.automation_id.clone(),
            source_workflow_name: source.name.clone(),
            source_workflow_snapshot: source.clone(),
            source_workflow_snapshot_hash: crate::optimization_snapshot_hash(&source),
            baseline_snapshot: source.clone(),
            baseline_snapshot_hash: crate::optimization_snapshot_hash(&source),
            execution_override: None,
            artifacts: crate::OptimizationArtifactRefs::default(),
            frozen_artifacts: crate::OptimizationFrozenArtifacts {
                objective: artifact.clone(),
                eval: artifact.clone(),
                mutation_policy: artifact.clone(),
                scope: artifact.clone(),
                budget: artifact,
            },
            phase1: None,
            baseline_metrics: None,
            baseline_replays: Vec::new(),
            pending_baseline_run_ids: Vec::new(),
            pending_promotion_experiment_id: None,
            last_pause_reason: None,
            created_at_ms: 1,
            updated_at_ms: 1,
            metadata: None,
        })
        .await
        .expect("campaign");

    for (actor, expected_count, expected_get, expected_action) in [
        ("bob", 0, StatusCode::NOT_FOUND, StatusCode::NOT_FOUND),
        ("alice", 1, StatusCode::OK, StatusCode::OK),
    ] {
        let verified = optimization_verified_actor(actor);
        let tenant = verified.tenant_context.clone();
        let app = crate::http::routes_optimizations::apply(axum::Router::new())
            .layer(axum::Extension(verified))
            .layer(axum::Extension(tenant))
            .with_state(state.clone());
        let list = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/optimizations")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(list.status(), StatusCode::OK);
        let list: Value =
            serde_json::from_slice(&to_bytes(list.into_body(), usize::MAX).await.unwrap()).unwrap();
        assert_eq!(
            list["count"].as_u64(),
            Some(expected_count),
            "actor={actor}"
        );
        let get = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/optimizations/opt-private")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(get.status(), expected_get, "actor={actor}");
        let experiments = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/optimizations/opt-private/experiments")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(experiments.status(), expected_get, "actor={actor}");
        if actor == "bob" {
            let create = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/optimizations")
                        .header("content-type", "application/json")
                        .body(Body::from(
                            json!({
                                "source_workflow_id": source.automation_id.clone(),
                                "artifacts": {
                                    "objective_ref": "missing",
                                    "eval_ref": "missing",
                                    "mutation_policy_ref": "missing",
                                    "scope_ref": "missing",
                                    "budget_ref": "missing"
                                }
                            })
                            .to_string(),
                        ))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(create.status(), StatusCode::NOT_FOUND);
        }
        let action = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/optimizations/opt-private/actions")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"action":"pause"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(action.status(), expected_action, "actor={actor}");
        if actor == "bob" {
            assert_eq!(
                state
                    .get_optimization_campaign("opt-private")
                    .await
                    .unwrap()
                    .status,
                crate::OptimizationCampaignStatus::Draft
            );
        }
    }

    let mut colliding = state
        .get_optimization_campaign("opt-private")
        .await
        .unwrap();
    colliding.name = "forged replacement".to_string();
    assert!(state.create_optimization_campaign(colliding).await.is_err());
    assert_eq!(
        state
            .get_optimization_campaign("opt-private")
            .await
            .unwrap()
            .name,
        "Private optimization"
    );

    let mut changed = state
        .get_automation_v2(&source.automation_id)
        .await
        .unwrap();
    changed.name = "unauthorized winner".to_string();
    assert!(state
        .put_automation_v2_checked(changed, |_| anyhow::bail!("revoked"))
        .await
        .is_err());
    assert_eq!(
        state
            .get_automation_v2(&source.automation_id)
            .await
            .unwrap()
            .name,
        source.name
    );

    // A retained campaign must never follow a workflow ID into a new owner's
    // record, even when the new actor can read the replacement workflow.
    let mut replacement = source.clone();
    replacement.creator_id = "bob".to_string();
    replacement.created_at_ms += 1;
    replacement.metadata = Some(json!({
        "resource_access": {
            "visibility": "private",
            "owner_principal": {"kind": "human_user", "id": "bob"}
        }
    }));
    replacement.set_tenant_context(&tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".to_string()),
        "bob",
    ));
    state
        .put_automation_v2(replacement)
        .await
        .expect("replacement workflow");
    let bob = optimization_verified_actor("bob");
    let app = crate::http::routes_optimizations::apply(axum::Router::new())
        .layer(axum::Extension(bob.tenant_context.clone()))
        .layer(axum::Extension(bob))
        .with_state(state.clone());
    let list = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/optimizations")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let list: Value =
        serde_json::from_slice(&to_bytes(list.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(list["count"], 0, "replacement owner inherited campaign");
    let get = app
        .oneshot(
            Request::builder()
                .uri("/optimizations/opt-private")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn creating_an_automation_cannot_replace_an_existing_global_id() {
    let state = test_state().await;
    let workspace = tempfile::tempdir().expect("workspace");
    let mut source = sample_automation(workspace.path().to_str().expect("workspace path"));
    source.creator_id = "alice".to_string();
    source.set_tenant_context(&tandem_types::TenantContext::explicit_user_workspace(
        "org-a",
        "dep-a",
        Some("dep-a".to_string()),
        "alice",
    ));
    let source = state.put_automation_v2(source).await.expect("source");
    let app = app_router(state.clone());
    let create = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/automations/v2")
                .header("content-type", "application/json")
                .header("x-tandem-org-id", "org-a")
                .header("x-tandem-workspace-id", "dep-a")
                .header("x-tandem-actor-id", "bob")
                .body(Body::from(
                    json!({
                        "automation_id": source.automation_id,
                        "name": "Bob's replacement",
                        "status": "draft",
                        "schedule": {
                            "type": "manual",
                            "timezone": "UTC",
                            "misfire_policy": { "type": "skip" }
                        },
                        "agents": [{
                            "agent_id": "agent-b",
                            "display_name": "Agent B",
                            "skills": [],
                            "tool_policy": { "allowlist": ["read"], "denylist": [] },
                            "mcp_policy": { "allowed_servers": [] }
                        }],
                        "flow": {"nodes": [{
                            "node_id": "node-1",
                            "agent_id": "agent-b",
                            "objective": "Replace source",
                            "depends_on": []
                        }]},
                        "execution": { "max_parallel_agents": 1 }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::CONFLICT);
    assert_eq!(
        state
            .get_automation_v2(&source.automation_id)
            .await
            .expect("original still exists")
            .creator_id,
        "alice"
    );
}
