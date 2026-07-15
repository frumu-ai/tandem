// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[tokio::test]
async fn context_tree_endpoints_are_tenant_scoped() {
    let state = test_state().await;
    let app = app_router(state.clone());

    // Seed a context node + L2 layer for tenant org-a directly through the
    // memory manager (nodes have no HTTP creation surface).
    if let Some(parent) = state.memory_db_path.parent() {
        tokio::fs::create_dir_all(parent).await.expect("memory dir");
    }
    let manager = tandem_memory::MemoryManager::new(&state.memory_db_path)
        .await
        .expect("memory manager");
    let scope_a = tandem_memory::types::MemoryTenantScope {
        org_id: "org-a".to_string(),
        workspace_id: "workspace-a".to_string(),
        deployment_id: None,
    };
    let node_id = manager
        .store_content_with_layers(
            "tandem://resources/proj-a/secret.md",
            "tenant A secret context",
            None,
            &scope_a,
        )
        .await
        .expect("seed node");

    let resolve_request = |org: &str, workspace: &str| {
        Request::builder()
            .method("POST")
            .uri("/memory/context/resolve")
            .header("content-type", "application/json")
            .header("x-tandem-org-id", org)
            .header("x-tandem-workspace-id", workspace)
            .header("x-tandem-actor-id", "actor-a")
            .body(Body::from(
                json!({ "uri": "tandem://resources/proj-a/secret.md" }).to_string(),
            ))
            .expect("request")
    };

    // The owning tenant resolves the node.
    let resp = app
        .clone()
        .oneshot(resolve_request("org-a", "workspace-a"))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload.pointer("/node/id").and_then(Value::as_str),
        Some(node_id.as_str())
    );

    // A different tenant gets the same shape as a nonexistent URI: node null.
    let resp = app
        .clone()
        .oneshot(resolve_request("org-b", "workspace-b"))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert!(payload.get("node").is_some_and(Value::is_null));

    // Tree traversal: owner sees the child; foreign tenant sees an empty tree.
    let tree_request = |org: &str, workspace: &str| {
        Request::builder()
            .method("GET")
            .uri("/memory/context/tree?uri=tandem%3A%2F%2Fresources%2Fproj-a&max_depth=3")
            .header("x-tandem-org-id", org)
            .header("x-tandem-workspace-id", workspace)
            .header("x-tandem-actor-id", "actor-a")
            .body(Body::empty())
            .expect("request")
    };
    let resp = app
        .clone()
        .oneshot(tree_request("org-a", "workspace-a"))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .pointer("/tree")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );
    let resp = app
        .clone()
        .oneshot(tree_request("org-b", "workspace-b"))
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let body = to_bytes(resp.into_body(), usize::MAX).await.expect("body");
    let payload: Value = serde_json::from_slice(&body).expect("json");
    assert_eq!(
        payload
            .pointer("/tree")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );

    // Layer generation on a foreign node id behaves exactly like a missing
    // node id: 200 no-op, and the owner's layer set is untouched.
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/memory/context/layers/generate")
                .header("content-type", "application/json")
                .header("x-tandem-org-id", "org-b")
                .header("x-tandem-workspace-id", "workspace-b")
                .header("x-tandem-actor-id", "actor-a")
                .body(Body::from(json!({ "node_id": node_id }).to_string()))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(resp.status(), StatusCode::OK);
    let scope_b = tandem_memory::types::MemoryTenantScope {
        org_id: "org-b".to_string(),
        workspace_id: "workspace-b".to_string(),
        deployment_id: None,
    };
    assert!(manager
        .get_context_layer(&node_id, tandem_memory::types::LayerType::L0, &scope_b)
        .await
        .expect("layer lookup")
        .is_none());
    assert!(manager
        .get_context_layer(&node_id, tandem_memory::types::LayerType::L0, &scope_a)
        .await
        .expect("layer lookup")
        .is_none());
    assert!(manager
        .get_context_layer(&node_id, tandem_memory::types::LayerType::L2, &scope_a)
        .await
        .expect("layer lookup")
        .is_some());
}

fn verified_org_unit_context(
    tenant_context: tandem_types::TenantContext,
    actor: &str,
    org_units: Vec<String>,
) -> tandem_types::VerifiedTenantContext {
    let principal = tandem_types::RequestPrincipal::authenticated_user(actor, "tandem-web");
    let authority_chain = tandem_types::AuthorityChain::from_request(principal);
    let strict_projection = tandem_types::StrictTenantContext::new(
        tenant_context.clone(),
        tandem_types::PrincipalRef::human_user(actor),
        authority_chain.clone(),
        tandem_types::ResourceScope::root(tandem_types::ResourceRef::new(
            tenant_context.org_id.clone(),
            tenant_context.workspace_id.clone(),
            tandem_types::ResourceKind::Workspace,
            tenant_context.workspace_id.clone(),
        )),
        tandem_types::AssertionMetadata::new(
            "tandem-web",
            "tandem-runtime",
            1_000,
            9_999_999_999_999,
            format!("org-unit-assertion-{actor}"),
        ),
    );
    tandem_types::VerifiedTenantContext {
        tenant_context,
        human_actor: tandem_types::HumanActor::tandem_user(actor),
        authority_chain,
        roles: Vec::new(),
        org_units,
        capabilities: Vec::new(),
        policy_version: None,
        strict_projection: Some(strict_projection),
        issuer: "tandem-web".to_string(),
        audience: "tandem-runtime".to_string(),
        issued_at_ms: 1_000,
        expires_at_ms: 9_999_999_999_999,
        assertion_id: format!("org-unit-assertion-{actor}"),
        assertion_key_id: None,
    }
}

fn org_unit_put_request(run_id: &str, owner_org_unit_id: &str) -> tandem_memory::MemoryPutRequest {
    tandem_memory::MemoryPutRequest {
        private: false,
        run_id: run_id.to_string(),
        partition: tandem_memory::MemoryPartition {
            org_id: "acme".to_string(),
            workspace_id: "north".to_string(),
            project_id: "proj-a".to_string(),
            tier: tandem_memory::GovernedMemoryTier::Session,
        },
        kind: tandem_memory::MemoryContentKind::Fact,
        content: "engineering department runbook detail".to_string(),
        artifact_refs: Vec::new(),
        classification: tandem_memory::MemoryClassification::Internal,
        authority_job_context: None,
        metadata: Some(json!({ "owner_org_unit_id": owner_org_unit_id })),
    }
}

fn org_unit_capability(run_id: &str, subject: &str) -> tandem_memory::MemoryCapabilityToken {
    tandem_memory::MemoryCapabilityToken {
        run_id: run_id.to_string(),
        subject: subject.to_string(),
        org_id: "acme".to_string(),
        workspace_id: "north".to_string(),
        project_id: "proj-a".to_string(),
        memory: tandem_memory::MemoryCapabilities::default(),
        expires_at: 9_999_999_999_999,
    }
}

#[tokio::test]
async fn memory_put_org_unit_stamp_requires_membership() {
    let state = test_state().await;
    let tenant_context =
        tandem_types::TenantContext::explicit_user_workspace("acme", "north", None, "user-a");

    // A verified writer who is not a member of the stamped unit is rejected.
    let outsider = verified_org_unit_context(
        tenant_context.clone(),
        "user-a",
        vec!["ou-sales".to_string()],
    );
    let denied = super::super::skills_memory::memory_put_impl_with_verified(
        &state,
        &tenant_context,
        Some(&outsider),
        org_unit_put_request("org-unit-put-denied", "ou-eng"),
        Some(org_unit_capability("org-unit-put-denied", "user-a")),
    )
    .await;
    assert_eq!(denied.expect_err("non-member stamp"), StatusCode::FORBIDDEN);

    // A member of the unit stores the record.
    let member = verified_org_unit_context(
        tenant_context.clone(),
        "user-a",
        vec!["ou-eng".to_string(), "ou-sales".to_string()],
    );
    let stored = super::super::skills_memory::memory_put_impl_with_verified(
        &state,
        &tenant_context,
        Some(&member),
        org_unit_put_request("org-unit-put-allowed", "ou-eng"),
        Some(org_unit_capability("org-unit-put-allowed", "user-a")),
    )
    .await
    .expect("member stamp stores");
    assert!(stored.stored);
}

#[tokio::test]
async fn governed_read_filter_threads_org_units_from_verified_context() {
    let tenant_context =
        tandem_types::TenantContext::explicit_user_workspace("acme", "north", None, "user-a");
    let member = verified_org_unit_context(
        tenant_context.clone(),
        "user-a",
        vec!["ou-eng".to_string()],
    );
    let filter = crate::memory::read_policy::governed_memory_read_filter(
        tandem_types::RuntimeAuthMode::EnterpriseRequired,
        Some(&member),
        false,
        2_000,
    )
    .expect("governed filter");
    assert_eq!(
        filter.caller_org_units,
        Some(std::collections::BTreeSet::from(["ou-eng".to_string()]))
    );

    // An org-unit-restricted record is readable by the member and hidden from a
    // same-tenant principal in a different unit.
    let restricted_metadata = json!({ "owner_org_unit_id": "ou-eng" });
    let record = tandem_memory::types::GlobalMemoryRecord {
        id: "record-ou".to_string(),
        user_id: "user-a".to_string(),
        source_type: "fact".to_string(),
        content: "department fact".to_string(),
        content_hash: "hash".to_string(),
        run_id: "run-ou".to_string(),
        session_id: None,
        message_id: None,
        tool_name: None,
        project_tag: Some("proj-a".to_string()),
        channel_tag: None,
        host_tag: None,
        metadata: Some(restricted_metadata),
        provenance: None,
        redaction_status: "passed".to_string(),
        redaction_count: 0,
        visibility: "private".to_string(),
        demoted: false,
        score_boost: 0.0,
        created_at_ms: 1_000,
        updated_at_ms: 1_000,
        expires_at_ms: None,
    };
    assert!(filter.allows_global_record(&record));

    let other_unit = verified_org_unit_context(
        tenant_context.clone(),
        "user-b",
        vec!["ou-sales".to_string()],
    );
    let other_filter = crate::memory::read_policy::governed_memory_read_filter(
        tandem_types::RuntimeAuthMode::EnterpriseRequired,
        Some(&other_unit),
        false,
        2_000,
    )
    .expect("governed filter");
    let decision = other_filter.decision_for_global_record(&record);
    assert!(!decision.allowed);
    assert_eq!(decision.reason.as_deref(), Some("org_unit_scope_mismatch"));
}

#[tokio::test]
async fn memory_put_rejects_non_storage_backed_tiers() {
    let state = test_state().await;
    let app = app_router(state.clone());

    // Team/Curated exist only in the governance contract; writes fail closed
    // until a backing store lands (TAN-607).
    for tier in ["team", "curated"] {
        let req = Request::builder()
            .method("POST")
            .uri("/memory/put")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "run_id": format!("run-{tier}"),
                    "partition": {
                        "org_id": "org-1",
                        "workspace_id": "ws-1",
                        "project_id": "proj-1",
                        "tier": tier
                    },
                    "kind": "note",
                    "content": "should fail: tier has no backing store",
                    "classification": "internal",
                    "capability": {
                        "run_id": format!("run-{tier}"),
                        "subject": "user-1",
                        "org_id": "org-1",
                        "workspace_id": "ws-1",
                        "project_id": "proj-1",
                        "memory": {
                            "read_tiers": ["session", "project"],
                            "write_tiers": ["session", "project", tier],
                            "promote_targets": [],
                            "require_review_for_promote": false,
                            "allow_auto_use_tiers": []
                        },
                        "expires_at": 9999999999999u64
                    }
                })
                .to_string(),
            ))
            .expect("request");
        let resp = app.clone().oneshot(req).await.expect("response");
        assert_eq!(
            resp.status(),
            StatusCode::FORBIDDEN,
            "tier {tier} must be rejected"
        );
    }
}

#[tokio::test]
async fn memory_promote_rejects_non_storage_backed_tiers() {
    let state = test_state().await;
    let tenant_context =
        tandem_types::TenantContext::explicit_user_workspace("org-1", "ws-1", None, "user-1");

    for tier in [
        tandem_memory::GovernedMemoryTier::Team,
        tandem_memory::GovernedMemoryTier::Curated,
    ] {
        let capability = tandem_memory::MemoryCapabilityToken {
            run_id: "run-promote-tier".to_string(),
            subject: "user-1".to_string(),
            org_id: "org-1".to_string(),
            workspace_id: "ws-1".to_string(),
            project_id: "proj-1".to_string(),
            memory: tandem_memory::MemoryCapabilities {
                promote_targets: vec![tier],
                ..Default::default()
            },
            expires_at: 9_999_999_999_999,
        };
        let denied = super::super::skills_memory::memory_promote_impl(
            &state,
            &tenant_context,
            tandem_memory::MemoryPromoteRequest {
                run_id: "run-promote-tier".to_string(),
                source_memory_id: "missing-source".to_string(),
                from_tier: tandem_memory::GovernedMemoryTier::Session,
                to_tier: tier,
                partition: tandem_memory::MemoryPartition {
                    org_id: "org-1".to_string(),
                    workspace_id: "ws-1".to_string(),
                    project_id: "proj-1".to_string(),
                    tier,
                },
                reason: "promote into unbacked tier".to_string(),
                review: tandem_memory::PromotionReview {
                    required: false,
                    reviewer_id: None,
                    approval_id: None,
                },
                authority_job_context: None,
                source_outcome: None,
            },
            Some(capability),
        )
        .await;
        assert_eq!(
            denied.expect_err("unbacked tier promotion is rejected"),
            StatusCode::FORBIDDEN
        );
    }
}

fn org_unit_http_request(
    method: &str,
    uri: &str,
    actor: &str,
    org_unit: &str,
    body: Option<Value>,
) -> Request<Body> {
    let tenant_context = tandem_types::TenantContext::explicit_user_workspace(
        "acme", "north", None, actor,
    );
    let verified = verified_org_unit_context(
        tenant_context,
        actor,
        vec![org_unit.to_string()],
    );
    let mut request = tenant_memory_request(
        method,
        uri,
        "acme",
        "north",
        actor,
        body,
    );
    request.extensions_mut().insert(verified);
    request
}

async fn memory_http_json(app: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("memory HTTP response");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("memory HTTP body");
    let payload = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, payload)
}

#[tokio::test]
async fn memory_http_enforces_trusted_subject_and_active_org_unit_scopes() {
    let state = test_state().await;
    let app = app_router(state);

    let put = |run_id: &str, content: &str, private: bool, metadata: Option<Value>| {
        org_unit_http_request(
            "POST",
            "/memory/put",
            "user-owner",
            "ou-eng",
            Some(json!({
                "run_id": run_id,
                "partition": {
                    "org_id": "acme",
                    "workspace_id": "north",
                    "project_id": "proj-a",
                    "tier": "session"
                },
                "kind": "fact",
                "content": content,
                "classification": "internal",
                "private": private,
                "metadata": metadata,
                "capability": memory_capability(
                    run_id,
                    "user-owner",
                    "acme",
                    "north",
                    "proj-a"
                )
            })),
        )
    };

    let (status, shared_put) = memory_http_json(
        &app,
        put(
            "trusted-scope-shared",
            "trusted scope sentinel shared engineering detail",
            false,
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let shared_id = shared_put
        .get("id")
        .and_then(Value::as_str)
        .expect("shared memory id")
        .to_string();

    let (status, private_put) = memory_http_json(
        &app,
        put(
            "trusted-scope-private",
            "trusted scope sentinel private owner detail",
            true,
            Some(json!({"owner_subject": "user-peer"})),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let private_id = private_put
        .get("id")
        .and_then(Value::as_str)
        .expect("private memory id")
        .to_string();

    let search = |actor: &str, org_unit: &str, run_id: &str| {
        org_unit_http_request(
            "POST",
            "/memory/search",
            actor,
            org_unit,
            Some(json!({
                "run_id": run_id,
                "query": "trusted scope sentinel",
                "read_scopes": ["session"],
                "partition": {
                    "org_id": "acme",
                    "workspace_id": "north",
                    "project_id": "proj-a",
                    "tier": "session"
                },
                "limit": 20,
                "capability": memory_capability(
                    run_id,
                    actor,
                    "acme",
                    "north",
                    "proj-a"
                )
            })),
        )
    };

    let (status, owner_search) =
        memory_http_json(&app, search("user-owner", "ou-eng", "scope-owner-search")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        owner_search
            .get("results")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(2)
    );

    let (status, peer_search) =
        memory_http_json(&app, search("user-peer", "ou-eng", "scope-peer-search")).await;
    assert_eq!(status, StatusCode::OK);
    let peer_results = peer_search
        .get("results")
        .and_then(Value::as_array)
        .expect("peer search results");
    assert_eq!(peer_results.len(), 1);
    assert_eq!(
        peer_results[0].get("id").and_then(Value::as_str),
        Some(shared_id.as_str())
    );

    let (status, other_department_search) = memory_http_json(
        &app,
        search("user-sales", "ou-sales", "scope-sales-search"),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(other_department_search
        .get("results")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty));

    let list = |actor: &str, org_unit: &str| {
        org_unit_http_request(
            "GET",
            "/memory?q=trusted%20scope%20sentinel&project_id=proj-a&limit=20",
            actor,
            org_unit,
            None,
        )
    };
    let (status, owner_list) = memory_http_json(&app, list("user-owner", "ou-eng")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(owner_list.get("count").and_then(Value::as_u64), Some(2));

    let (status, peer_list) = memory_http_json(&app, list("user-peer", "ou-eng")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(peer_list.get("count").and_then(Value::as_u64), Some(1));

    let (status, other_department_list) =
        memory_http_json(&app, list("user-sales", "ou-sales")).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        other_department_list.get("count").and_then(Value::as_u64),
        Some(0)
    );

    // The forged owner_subject was replaced by the validated owner subject, so
    // the named peer cannot point-read it through a delete operation.
    let (status, _) = memory_http_json(
        &app,
        org_unit_http_request(
            "DELETE",
            &format!("/memory/{private_id}?project_id=proj-a"),
            "user-peer",
            "ou-eng",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A different department cannot point-read even a department-shared row.
    let (status, _) = memory_http_json(
        &app,
        org_unit_http_request(
            "DELETE",
            &format!("/memory/{shared_id}?project_id=proj-a"),
            "user-sales",
            "ou-sales",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let (status, _) = memory_http_json(
        &app,
        org_unit_http_request(
            "POST",
            "/memory/demote",
            "user-owner",
            "ou-eng",
            Some(json!({
                "id": shared_id,
                "run_id": "scope-owner-demote"
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, _) = memory_http_json(
        &app,
        org_unit_http_request(
            "DELETE",
            &format!("/memory/{private_id}?project_id=proj-a"),
            "user-owner",
            "ou-eng",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
}

#[tokio::test]
async fn revoked_org_unit_membership_denies_reads_and_mutations() {
    let state = test_state().await;
    let app = app_router(state);
    let run_id = "revoked-org-unit-membership";

    let (status, put_payload) = memory_http_json(
        &app,
        org_unit_http_request(
            "POST",
            "/memory/put",
            "user-owner",
            "ou-eng",
            Some(json!({
                "run_id": run_id,
                "partition": {
                    "org_id": "acme",
                    "workspace_id": "north",
                    "project_id": "proj-a",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "revoked department membership sentinel",
                "classification": "internal",
                "capability": memory_capability(
                    run_id,
                    "user-owner",
                    "acme",
                    "north",
                    "proj-a"
                )
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let memory_id = put_payload
        .get("id")
        .and_then(Value::as_str)
        .expect("stored memory id")
        .to_string();

    let revoked_request = |method: &str, uri: &str, body: Option<Value>| {
        let tenant_context = tandem_types::TenantContext::explicit_user_workspace(
            "acme",
            "north",
            None,
            "user-owner",
        );
        let verified = verified_org_unit_context(tenant_context, "user-owner", Vec::new());
        let mut request = tenant_memory_request(
            method,
            uri,
            "acme",
            "north",
            "user-owner",
            body,
        );
        request.extensions_mut().insert(verified);
        request
    };

    let (status, _) = memory_http_json(
        &app,
        revoked_request(
            "GET",
            "/memory?q=revoked%20department%20membership&project_id=proj-a&limit=10",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = memory_http_json(
        &app,
        revoked_request(
            "POST",
            "/memory/search",
            Some(json!({
                "run_id": run_id,
                "query": "revoked department membership sentinel",
                "read_scopes": ["session"],
                "partition": {
                    "org_id": "acme",
                    "workspace_id": "north",
                    "project_id": "proj-a",
                    "tier": "session"
                },
                "limit": 10,
                "capability": memory_capability(
                    run_id,
                    "user-owner",
                    "acme",
                    "north",
                    "proj-a"
                )
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = memory_http_json(
        &app,
        revoked_request(
            "POST",
            "/memory/demote",
            Some(json!({
                "id": memory_id,
                "run_id": "revoked-org-unit-demote"
            })),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, _) = memory_http_json(
        &app,
        revoked_request(
            "DELETE",
            &format!("/memory/{memory_id}?project_id=proj-a"),
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);

    let (status, member_list) = memory_http_json(
        &app,
        org_unit_http_request(
            "GET",
            "/memory?q=revoked%20department%20membership&project_id=proj-a&limit=10",
            "user-owner",
            "ou-eng",
            None,
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(member_list.get("count").and_then(Value::as_u64), Some(1));
}

#[tokio::test]
async fn local_http_keeps_private_subject_reads_and_unrestricted_point_mutation() {
    let state = test_state().await;
    let app = app_router(state);
    let run_id = "local-private-scope-run";
    let capability = memory_capability(run_id, "local-owner", "local", "local", "proj-local");
    let put = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": run_id,
                "partition": {
                    "org_id": "local",
                    "workspace_id": "local",
                    "project_id": "proj-local",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "local unrestricted private scope sentinel",
                "classification": "internal",
                "private": true,
                "capability": capability
            })
            .to_string(),
        ))
        .expect("local private put request");
    let (status, put_payload) = memory_http_json(&app, put).await;
    assert_eq!(status, StatusCode::OK);
    let memory_id = put_payload
        .get("id")
        .and_then(Value::as_str)
        .expect("local private memory id")
        .to_string();

    let list = Request::builder()
        .method("GET")
        .uri("/memory?user_id=local-owner&q=local%20unrestricted%20private&limit=20")
        .body(Body::empty())
        .expect("local private list request");
    let (status, list_payload) = memory_http_json(&app, list).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list_payload.get("count").and_then(Value::as_u64), Some(1));

    let search = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": run_id,
                "query": "local unrestricted private scope sentinel",
                "read_scopes": ["session"],
                "partition": {
                    "org_id": "local",
                    "workspace_id": "local",
                    "project_id": "proj-local",
                    "tier": "session"
                },
                "limit": 20,
                "capability": memory_capability(
                    run_id,
                    "local-owner",
                    "local",
                    "local",
                    "proj-local"
                )
            })
            .to_string(),
        ))
        .expect("local private search request");
    let (status, search_payload) = memory_http_json(&app, search).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        search_payload
            .get("results")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(1)
    );

    // Local implicit mode has no trusted caller identity for point operations;
    // it intentionally retains the legacy unrestricted tenant behavior.
    let delete = Request::builder()
        .method("DELETE")
        .uri(format!("/memory/{memory_id}?project_id=proj-local"))
        .body(Body::empty())
        .expect("local private delete request");
    let (status, _) = memory_http_json(&app, delete).await;
    assert_eq!(status, StatusCode::OK);
}
