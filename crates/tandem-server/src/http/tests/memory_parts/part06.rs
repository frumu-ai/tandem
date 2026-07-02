#[tokio::test]
async fn memory_promote_blocks_rejected_source_outcome() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    let capability = json!({
        "run_id": "run-3-rejected",
        "subject": "reviewer-user",
        "org_id": "org-1",
        "workspace_id": "ws-1",
        "project_id": "proj-1",
        "memory": {
            "read_tiers": ["session", "project"],
            "write_tiers": ["session"],
            "promote_targets": ["project"],
            "require_review_for_promote": false,
            "allow_auto_use_tiers": ["curated"]
        },
        "expires_at": 9999999999999u64
    });

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-3-rejected",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "rejected outcome must not become reusable memory",
                "artifact_refs": ["artifact://run-3-rejected/output.json"],
                "classification": "internal",
                "metadata": {
                    "source_outcome": {
                        "status": "rejected",
                        "approved": false,
                        "source_run_id": "run-3-rejected",
                        "approval_id": "appr-denied"
                    }
                },
                "capability": capability
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);
    let put_body = to_bytes(put_resp.into_body(), usize::MAX)
        .await
        .expect("put body");
    let put_payload: Value = serde_json::from_slice(&put_body).expect("put json");
    let memory_id = put_payload
        .get("id")
        .and_then(Value::as_str)
        .expect("memory id")
        .to_string();
    let _ = next_event_of_type(&mut rx, "memory.put").await;
    let _ = next_event_of_type(&mut rx, "memory.updated").await;

    let promote_req = Request::builder()
        .method("POST")
        .uri("/memory/promote")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-3-rejected",
                "source_memory_id": memory_id,
                "from_tier": "session",
                "to_tier": "project",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "reason": "try rejected source",
                "review": {
                    "required": false
                },
                "source_outcome": {
                    "status": "approved",
                    "approved": true
                },
                "capability": capability
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
        Some(false)
    );
    let promote_policy_decision_id = promote_payload
        .get("policy_decision_id")
        .and_then(Value::as_str)
        .expect("blocked promote policy decision id")
        .to_string();
    let policy_record = state
        .get_policy_decision(&promote_policy_decision_id)
        .await
        .expect("blocked promote policy decision");
    assert_eq!(
        policy_record.decision,
        tandem_types::PolicyDecisionEffect::Deny
    );
    assert_eq!(policy_record.reason_code, "source_outcome_not_approved");

    let promote_event = next_event_of_type(&mut rx, "memory.promote").await;
    assert_eq!(
        promote_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        promote_event
            .properties
            .get("policyDecisionID")
            .and_then(Value::as_str),
        Some(promote_policy_decision_id.as_str())
    );
    assert!(promote_event
        .properties
        .get("detail")
        .and_then(Value::as_str)
        .is_some_and(|detail| detail.contains("source outcome not approved")));

    let search_req = Request::builder()
        .method("POST")
        .uri("/memory/search")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-3-rejected",
                "query": "rejected outcome reusable memory",
                "read_scopes": ["project"],
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "project"
                },
                "capability": capability,
                "limit": 5
            })
            .to_string(),
        ))
        .expect("search request");
    let search_resp = app
        .clone()
        .oneshot(search_req)
        .await
        .expect("search response");
    assert_eq!(search_resp.status(), StatusCode::OK);
    let search_body = to_bytes(search_resp.into_body(), usize::MAX)
        .await
        .expect("search body");
    let search_payload: Value = serde_json::from_slice(&search_body).expect("search json");
    assert_eq!(
        search_payload
            .get("results")
            .and_then(Value::as_array)
            .map(Vec::len),
        Some(0)
    );
}

#[tokio::test]
async fn memory_promote_enterprise_policy_override_blocks_local_allow() {
    let state = test_state().await;
    let app = app_router(state.clone());
    let mut rx = state.event_bus.subscribe();

    state.enterprise.policy_rules.write().await.insert(
        "enterprise-memory-promote-deny".to_string(),
        tandem_enterprise_contract::EnterprisePolicyRule::new(
            "enterprise-memory-promote-deny",
            "enterprise-memory-floor",
            tandem_enterprise_contract::EnterprisePolicyScopeLevel::Enterprise,
            tandem_enterprise_contract::EnterprisePolicyEffect::Deny,
        )
        .with_tool_patterns(vec!["memory.promote".to_string()])
        .with_reason(
            "enterprise_memory_promote_floor",
            "enterprise policy denies memory promotion",
        ),
    );

    let capability = json!({
        "run_id": "run-3-enterprise-deny",
        "subject": "reviewer-user",
        "org_id": "org-1",
        "workspace_id": "ws-1",
        "project_id": "proj-1",
        "memory": {
            "read_tiers": ["session", "project"],
            "write_tiers": ["session"],
            "promote_targets": ["project"],
            "require_review_for_promote": false,
            "allow_auto_use_tiers": ["curated"]
        },
        "expires_at": 9999999999999u64
    });

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-3-enterprise-deny",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "kind": "fact",
                "content": "enterprise denied memory promotion should stay private",
                "classification": "internal",
                "capability": capability
            })
            .to_string(),
        ))
        .expect("put request");
    let put_resp = app.clone().oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::OK);
    let put_body = to_bytes(put_resp.into_body(), usize::MAX)
        .await
        .expect("put body");
    let put_payload: Value = serde_json::from_slice(&put_body).expect("put json");
    let memory_id = put_payload
        .get("id")
        .and_then(Value::as_str)
        .expect("memory id")
        .to_string();
    let _ = next_event_of_type(&mut rx, "memory.put").await;
    let _ = next_event_of_type(&mut rx, "memory.updated").await;

    let promote_req = Request::builder()
        .method("POST")
        .uri("/memory/promote")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-3-enterprise-deny",
                "source_memory_id": memory_id,
                "from_tier": "session",
                "to_tier": "project",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "session"
                },
                "reason": "enterprise override test",
                "review": {
                    "required": false
                },
                "source_outcome": {
                    "status": "approved",
                    "approved": true
                },
                "capability": capability
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
        Some(false)
    );
    let promote_policy_decision_id = promote_payload
        .get("policy_decision_id")
        .and_then(Value::as_str)
        .expect("blocked promote policy decision id")
        .to_string();
    let policy_record = state
        .get_policy_decision(&promote_policy_decision_id)
        .await
        .expect("blocked promote policy decision");
    assert_eq!(
        policy_record.decision,
        tandem_types::PolicyDecisionEffect::Deny
    );
    assert_eq!(policy_record.reason_code, "enterprise_memory_promote_floor");

    let promote_event = next_event_of_type(&mut rx, "memory.promote").await;
    assert_eq!(
        promote_event
            .properties
            .get("status")
            .and_then(Value::as_str),
        Some("blocked")
    );
    assert_eq!(
        promote_event
            .properties
            .get("policyDecisionID")
            .and_then(Value::as_str),
        Some(promote_policy_decision_id.as_str())
    );
}

#[tokio::test]
async fn memory_put_blocks_project_write_denied_by_knowledge_scope() {
    let state = test_state().await;
    let app = app_router(state.clone());

    let capability = json!({
        "run_id": "run-knowledge-scope-write",
        "subject": "agent-user",
        "org_id": "org-1",
        "workspace_id": "ws-1",
        "project_id": "proj-1",
        "memory": {
            "read_tiers": ["session", "project"],
            "write_tiers": ["session", "project"],
            "promote_targets": ["project"],
            "require_review_for_promote": false,
            "allow_auto_use_tiers": ["curated"]
        },
        "expires_at": 9999999999999u64
    });

    let put_req = Request::builder()
        .method("POST")
        .uri("/memory/put")
        .header("content-type", "application/json")
        .body(Body::from(
            json!({
                "run_id": "run-knowledge-scope-write",
                "partition": {
                    "org_id": "org-1",
                    "workspace_id": "ws-1",
                    "project_id": "proj-1",
                    "tier": "project"
                },
                "kind": "fact",
                "content": "project write must carry explicit knowledge scope policy",
                "artifact_refs": ["artifact://run-knowledge-scope-write/output.json"],
                "classification": "internal",
                "metadata": {
                    "knowledge_scope_registry": {
                        "registry_id": "registry-session-only",
                        "resource_ref": {
                            "organization_id": "org-1",
                            "workspace_id": "ws-1",
                            "project_id": "proj-1",
                            "resource_kind": "knowledge_space",
                            "resource_id": "space-session"
                        },
                        "data_class": "confidential",
                        "collection_id": "collection-session",
                        "owner_org_unit_id": "ou-1",
                        "risk_tier": "confidential",
                        "allowed_workflow_phases": ["draft"],
                        "allowed_write_tiers": ["session"],
                        "allowed_promotion_tiers": ["project"],
                        "retention_expires_at_ms": 9999999999999u64,
                        "promotion_requires_approval": true
                    }
                },
                "capability": capability
            })
            .to_string(),
        ))
        .expect("put request");

    let put_resp = app.oneshot(put_req).await.expect("put response");
    assert_eq!(put_resp.status(), StatusCode::FORBIDDEN);
}
