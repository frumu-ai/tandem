// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use axum::{extract::Extension, http::StatusCode, Router};
use serde_json::{json, Value};
use tandem_types::{PrincipalRef, TenantContext};

use super::legacy_routine_authority::{hosted_state, tenant, verified};

fn hosted_app(state: &AppState, actor: &str, role: &str) -> Router {
    let mut identity = verified(actor, role);
    state
        .enterprise
        .hosted_policy
        .project(&mut identity)
        .expect("project hosted actor");
    crate::http::routes_external_actions::apply(Router::new())
        .layer(Extension(tenant(actor)))
        .layer(Extension(identity))
        .with_state(state.clone())
}

fn receipt(
    id: &str,
    scope: Option<TenantContext>,
    owner: Option<&str>,
    at: u64,
) -> crate::ExternalActionRecord {
    crate::ExternalActionRecord {
        action_id: id.to_string(),
        provenance: scope.map(|tenant_context| crate::ExternalActionProvenance {
            tenant_context,
            owner_principal: owner.map(PrincipalRef::human_user),
        }),
        operation: "send".to_string(),
        status: "posted".to_string(),
        receipt: Some(json!({ "secret": id })),
        created_at_ms: at,
        updated_at_ms: at,
        ..Default::default()
    }
}

async fn publish_policy_v2(
    state: &AppState,
    policy_dir: &tempfile::TempDir,
    edit: impl FnOnce(&mut Value),
) {
    let path = policy_dir.path().join("policy.json");
    let mut policy: Value =
        serde_json::from_slice(&std::fs::read(&path).expect("read policy")).expect("policy JSON");
    policy["policy_version"] = json!(2);
    policy["generated_at"] = json!(
        chrono::DateTime::from_timestamp_millis(crate::now_ms() as i64).expect("current time")
    );
    edit(&mut policy);
    std::fs::write(path, serde_json::to_vec(&policy).expect("policy bytes")).expect("write policy");
    state
        .reload_hosted_policy()
        .await
        .expect("publish updated policy");
}

async fn get_json(app: Router, path: &str) -> (StatusCode, Value) {
    let response = app
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .expect("external action response");
    let status = response.status();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("external action body");
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    (status, value)
}

#[tokio::test]
async fn hosted_receipts_are_filtered_by_exact_tenant_and_owner_before_limit() {
    let (state, _policy) = hosted_state().await;
    let mut other_tenant = tenant("alice");
    other_tenant.org_id = "another-org".to_string();
    let rows = [
        receipt("alice", Some(tenant("alice")), Some("alice"), 1),
        receipt("bob", Some(tenant("bob")), Some("bob"), 2),
        receipt("system", Some(tenant("alice")), None, 3),
        receipt("other-tenant", Some(other_tenant), Some("alice"), 4),
        receipt("legacy", None, None, 5),
    ];
    {
        let mut guard = state.external_actions.write().await;
        for row in rows {
            guard.insert(row.action_id.clone(), row);
        }
    }

    let (status, list) = get_json(
        hosted_app(&state, "alice", "member"),
        "/external-actions?limit=1",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(list["count"], json!(1));
    assert_eq!(list["actions"][0]["action_id"], json!("alice"));
    for hidden in ["bob", "system", "other-tenant", "legacy"] {
        let (status, _) = get_json(
            hosted_app(&state, "alice", "member"),
            &format!("/external-actions/{hidden}"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{hidden}");
    }
    let (status, _) = get_json(hosted_app(&state, "bob", "member"), "/external-actions/bob").await;
    assert_eq!(status, StatusCode::OK);
    let (status, admin_list) =
        get_json(hosted_app(&state, "admin", "admin"), "/external-actions").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(admin_list["count"], json!(3));
    let (status, _) = get_json(
        hosted_app(&state, "admin", "admin"),
        "/external-actions/system",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    for hidden in ["other-tenant", "legacy"] {
        let (status, _) = get_json(
            hosted_app(&state, "admin", "admin"),
            &format!("/external-actions/{hidden}"),
        )
        .await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{hidden}");
    }
}

#[tokio::test]
async fn legacy_receipts_remain_local_only() {
    let state = test_state().await;
    let legacy: crate::ExternalActionRecord = serde_json::from_value(json!({
        "action_id": "legacy", "operation": "send", "status": "posted",
        "receipt": { "secret": "legacy" }, "created_at_ms": 1, "updated_at_ms": 1
    }))
    .expect("deserialize pre-provenance receipt");
    assert!(legacy.provenance.is_none());
    state
        .external_actions
        .write()
        .await
        .insert(legacy.action_id.clone(), legacy);
    let app = crate::http::routes_external_actions::apply(Router::new())
        .layer(Extension(TenantContext::local_implicit()))
        .with_state(state);
    let (status, body) = get_json(app, "/external-actions/legacy").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["action"]["receipt"]["secret"], json!("legacy"));
}

#[tokio::test]
async fn canonical_run_provenance_prevents_cross_owner_idempotent_reuse() {
    let state = test_state().await;
    for (run_id, scope) in [
        ("run-alice", tenant("alice")),
        ("run-bob", tenant("bob")),
        (
            "run-other",
            TenantContext::explicit_user_workspace(
                "other-org",
                "dep-routine",
                Some("dep-routine".into()),
                "alice",
            ),
        ),
    ] {
        let run: tandem_workflows::WorkflowRunRecord = serde_json::from_value(json!({
            "run_id": run_id, "workflow_id": "workflow", "tenant_context": scope,
            "status": "running", "created_at_ms": 1, "updated_at_ms": 1
        }))
        .expect("workflow run fixture");
        state
            .workflow_runs
            .write()
            .await
            .insert(run_id.to_string(), run);
        let action = crate::ExternalActionRecord {
            action_id: format!("action-{run_id}"),
            source_kind: Some("workflow".to_string()),
            source_id: Some(format!("{run_id}:send")),
            idempotency_key: Some("shared-key".to_string()),
            operation: "send".to_string(),
            status: "posted".to_string(),
            ..Default::default()
        };
        let recorded = state
            .record_external_action(action)
            .await
            .expect("record receipt");
        assert_eq!(recorded.action_id, format!("action-{run_id}"));
        assert!(recorded.provenance.is_some());
    }
    assert_eq!(state.list_external_actions(10).await.len(), 3);
}

#[tokio::test]
async fn automation_receipt_belongs_to_object_owner_not_run_trigger_actor() {
    let (state, _policy) = hosted_state().await;
    let mut automation = crate::http::tests::global::create_test_automation_v2_for_tenant(
        &state,
        "owner-receipt-automation",
        &tenant("alice"),
    )
    .await;
    automation
        .metadata
        .as_mut()
        .and_then(Value::as_object_mut)
        .expect("automation metadata")
        .insert(
            "resource_access".to_string(),
            json!({
                "visibility": "private",
                "owner_principal": { "kind": "human_user", "id": "alice" }
            }),
        );
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("persist canonical automation owner");

    let run: crate::automation_v2::types::AutomationV2RunRecord = serde_json::from_value(json!({
        "run_id": "run-triggered-by-bob",
        "automation_id": automation.automation_id,
        "tenant_context": tenant("bob"),
        "trigger_type": "manual",
        "status": "queued",
        "created_at_ms": 1,
        "updated_at_ms": 1,
        "checkpoint": {}
    }))
    .expect("automation run fixture");
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run.run_id.clone(), run);

    let recorded = state
        .record_external_action(crate::ExternalActionRecord {
            action_id: "owner-receipt".to_string(),
            source_kind: Some("automation_v2".to_string()),
            source_id: Some("run-triggered-by-bob:send".to_string()),
            operation: "send".to_string(),
            status: "posted".to_string(),
            ..Default::default()
        })
        .await
        .expect("record automation receipt");
    assert_eq!(
        recorded.provenance.unwrap().owner_principal,
        Some(PrincipalRef::human_user("alice"))
    );
    for (actor, role, expected) in [
        ("alice", "member", StatusCode::OK),
        ("bob", "member", StatusCode::NOT_FOUND),
        ("admin", "admin", StatusCode::OK),
    ] {
        let (status, _) = get_json(
            hosted_app(&state, actor, role),
            "/external-actions/owner-receipt",
        )
        .await;
        assert_eq!(status, expected, "actor {actor}");
    }
}

#[tokio::test]
async fn automation_receipt_uses_run_snapshot_before_transferred_live_owner() {
    let (state, _policy) = hosted_state().await;
    let mut snapshot = crate::http::tests::global::create_test_automation_v2_for_tenant(
        &state,
        "transferred-receipt-automation",
        &tenant("alice"),
    )
    .await;
    snapshot
        .metadata
        .as_mut()
        .and_then(Value::as_object_mut)
        .expect("automation metadata")
        .insert(
            "resource_access".to_string(),
            json!({
                "visibility": "private",
                "owner_principal": { "kind": "human_user", "id": "alice" }
            }),
        );
    state
        .put_automation_v2(snapshot.clone())
        .await
        .expect("persist original automation owner");
    let mut transferred = snapshot.clone();
    transferred.metadata.as_mut().unwrap()["resource_access"]["owner_principal"]["id"] =
        json!("bob");
    state
        .put_automation_v2(transferred)
        .await
        .expect("persist transferred automation owner");

    let run: crate::automation_v2::types::AutomationV2RunRecord = serde_json::from_value(json!({
        "run_id": "run-before-transfer",
        "automation_id": snapshot.automation_id,
        "tenant_context": tenant("alice"),
        "trigger_type": "manual",
        "status": "queued",
        "created_at_ms": 1,
        "updated_at_ms": 1,
        "checkpoint": {},
        "automation_snapshot": snapshot
    }))
    .expect("run snapshot fixture");
    state
        .automation_v2_runs
        .write()
        .await
        .insert(run.run_id.clone(), run.clone());
    let recorded = state
        .record_external_action(crate::ExternalActionRecord {
            action_id: "before-transfer-receipt".to_string(),
            source_kind: Some("automation_v2".to_string()),
            source_id: Some("run-before-transfer:send".to_string()),
            operation: "send".to_string(),
            status: "posted".to_string(),
            ..Default::default()
        })
        .await
        .expect("record historical receipt");
    assert_eq!(
        recorded.provenance.unwrap().owner_principal,
        Some(PrincipalRef::human_user("alice"))
    );
    for (actor, expected) in [("alice", StatusCode::OK), ("bob", StatusCode::NOT_FOUND)] {
        let (status, _) = get_json(
            hosted_app(&state, actor, "member"),
            "/external-actions/before-transfer-receipt",
        )
        .await;
        assert_eq!(status, expected, "actor {actor}");
    }

    // An invalid snapshot cannot be replaced by a convenient live object.
    let mut mismatched = run.clone();
    mismatched.run_id = "run-mismatched-snapshot".to_string();
    mismatched
        .automation_snapshot
        .as_mut()
        .expect("snapshot")
        .automation_id = "different-automation".to_string();
    state
        .automation_v2_runs
        .write()
        .await
        .insert(mismatched.run_id.clone(), mismatched);
    let invalid = state
        .record_external_action(crate::ExternalActionRecord {
            action_id: "mismatched-snapshot-receipt".to_string(),
            source_kind: Some("automation_v2".to_string()),
            source_id: Some("run-mismatched-snapshot:send".to_string()),
            operation: "send".to_string(),
            status: "posted".to_string(),
            ..Default::default()
        })
        .await
        .expect("record unattributed receipt");
    assert!(invalid.provenance.is_none());

    // Snapshotless legacy fallback must pass the same exact tenant check.
    let mut legacy_mismatched = run;
    legacy_mismatched.run_id = "run-legacy-other-tenant".to_string();
    legacy_mismatched.tenant_context.org_id = "other-org".to_string();
    legacy_mismatched.automation_snapshot = None;
    state
        .automation_v2_runs
        .write()
        .await
        .insert(legacy_mismatched.run_id.clone(), legacy_mismatched);
    let invalid_fallback = state
        .record_external_action(crate::ExternalActionRecord {
            action_id: "mismatched-legacy-receipt".to_string(),
            source_kind: Some("automation_v2".to_string()),
            source_id: Some("run-legacy-other-tenant:send".to_string()),
            operation: "send".to_string(),
            status: "posted".to_string(),
            ..Default::default()
        })
        .await
        .expect("record unattributed fallback receipt");
    assert!(invalid_fallback.provenance.is_none());
}

#[tokio::test]
async fn stale_hosted_admin_projection_cannot_read_system_receipt() {
    let (state, policy_dir) = hosted_state().await;
    let mut stale_admin = verified("admin", "admin");
    state
        .enterprise
        .hosted_policy
        .project(&mut stale_admin)
        .expect("project original admin");
    let system = receipt("system", Some(tenant("alice")), None, 1);
    assert!(crate::http::external_actions::external_action_visible(
        &state,
        &system,
        &tenant("admin"),
        Some(&stale_admin),
        false,
    ));
    publish_policy_v2(&state, &policy_dir, |policy| {
        let admin = policy["users"]
            .as_array_mut()
            .expect("policy users")
            .iter_mut()
            .find(|user| user["id"] == "admin")
            .expect("admin user");
        admin["role"] = json!("member");
        admin["capabilities"] =
            json!(tandem_enterprise_contract::hosted_policy::role_capabilities("member"));
    })
    .await;
    assert!(!crate::http::external_actions::external_action_visible(
        &state,
        &system,
        &tenant("admin"),
        Some(&stale_admin),
        false,
    ));
}

#[tokio::test]
async fn hosted_receipt_read_rechecks_policy_after_waiting_on_receipt_map() {
    for path in ["/external-actions", "/external-actions/alice"] {
        let (state, policy_dir) = hosted_state().await;
        state.external_actions.write().await.insert(
            "alice".to_string(),
            receipt("alice", Some(tenant("alice")), Some("alice"), 1),
        );
        let mut stale_member = verified("alice", "member");
        state
            .enterprise
            .hosted_policy
            .project(&mut stale_member)
            .expect("project original reader");
        let app = crate::http::routes_external_actions::apply(Router::new())
            .layer(Extension(tenant("alice")))
            .layer(Extension(stale_member))
            .with_state(state.clone());

        let guard = state.external_actions.write().await;
        let mut read = Box::pin(get_json(app, path));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(20), read.as_mut())
                .await
                .is_err(),
            "{path} must wait on the held receipt map"
        );
        publish_policy_v2(&state, &policy_dir, |policy| {
            policy["users"]
                .as_array_mut()
                .expect("policy users")
                .iter_mut()
                .find(|user| user["id"] == "alice")
                .expect("alice user")["is_active"] = json!(false);
        })
        .await;
        drop(guard);

        let (status, _) = read.await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }
}

#[tokio::test]
async fn revoked_hosted_read_and_source_loss_fail_closed() {
    let (state, policy_dir) = hosted_state().await;
    state.external_actions.write().await.insert(
        "alice".to_string(),
        receipt("alice", Some(tenant("alice")), Some("alice"), 1),
    );
    let mut stale_member = verified("alice", "member");
    state
        .enterprise
        .hosted_policy
        .project(&mut stale_member)
        .expect("project original member");
    publish_policy_v2(&state, &policy_dir, |policy| {
        policy["users"]
            .as_array_mut()
            .expect("policy users")
            .iter_mut()
            .find(|user| user["id"] == "alice")
            .expect("alice user")["is_active"] = json!(false);
    })
    .await;
    let app = crate::http::routes_external_actions::apply(Router::new())
        .layer(Extension(tenant("alice")))
        .layer(Extension(stale_member))
        .with_state(state);
    for path in ["/external-actions", "/external-actions/alice"] {
        let (status, _) = get_json(app.clone(), path).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
    }

    let local = test_state().await;
    let versioned = crate::http::routes_external_actions::apply(Router::new())
        .layer(Extension(tenant("alice")))
        .layer(Extension(verified("alice", "member")))
        .with_state(local);
    for path in ["/external-actions", "/external-actions/alice"] {
        let (status, _) = get_json(versioned.clone(), path).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "source loss: {path}");
    }
}
