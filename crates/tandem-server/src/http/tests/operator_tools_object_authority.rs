// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use tandem_types::{AuthorityChain, HumanActor, RequestPrincipal, Session, VerifiedTenantContext};

fn tenant(actor: &str, org: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(org, "workspace-a", None, actor)
}

fn verified(tenant: TenantContext, actor: &str) -> VerifiedTenantContext {
    VerifiedTenantContext {
        tenant_context: tenant,
        human_actor: HumanActor::tandem_user(actor),
        authority_chain: AuthorityChain::from_request(RequestPrincipal::authenticated_user(
            actor,
            "operator-test",
        )),
        roles: vec!["operator".to_string()],
        org_units: Vec::new(),
        capabilities: Vec::new(),
        policy_version: None,
        strict_projection: None,
        issuer: "operator-test".to_string(),
        audience: "tandem".to_string(),
        issued_at_ms: crate::now_ms(),
        expires_at_ms: crate::now_ms() + 60_000,
        assertion_id: format!("operator-object-{actor}"),
        assertion_key_id: None,
    }
}

async fn chat(state: &AppState, tenant: TenantContext, actor: &str) -> Session {
    let mut session = Session::new(Some("Operator object test".to_string()), None);
    session.tenant_context = tenant.clone();
    session.verified_tenant_context = Some(verified(tenant, actor));
    state.storage.save_session(session.clone()).await.unwrap();
    session
}

fn tool(state: AppState, name: &str) -> std::sync::Arc<dyn tandem_tools::Tool> {
    crate::http::operator_tools::operator_tools(state)
        .into_iter()
        .find(|tool| tool.schema().name == name)
        .unwrap()
}

fn automation_http(
    state: &AppState,
    tenant: TenantContext,
    actor: &str,
    admin: bool,
    deployment_writer: bool,
) -> axum::Router {
    let mut identity = verified(tenant.clone(), actor);
    if admin {
        identity.roles.push("admin".to_string());
    }
    if deployment_writer {
        identity.capabilities.extend([
            "automation.write".to_string(),
            "automation.share".to_string(),
        ]);
    }
    crate::http::routes_routines_automations::apply(axum::Router::new())
        .layer(axum::Extension(tenant))
        .layer(axum::Extension(identity))
        .layer(axum::Extension(RequestPrincipal::authenticated_user(
            actor,
            "operator-test",
        )))
        .with_state(state.clone())
}

fn automation(id: &str, tenant: &TenantContext, actor: &str) -> crate::AutomationV2Spec {
    let mut automation = crate::AutomationV2Spec {
        automation_id: id.to_string(),
        name: format!("Private workflow for {actor}"),
        description: None,
        status: crate::AutomationV2Status::Draft,
        schedule: crate::AutomationV2Schedule {
            schedule_type: crate::AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: crate::AutomationFlowSpec {
            nodes: vec![serde_json::from_value(json!({
                "node_id": "step", "agent_id": "agent", "objective": "Write a brief"
            }))
            .unwrap()],
        },
        execution: crate::AutomationExecutionPolicy::default(),
        output_targets: Vec::new(),
        created_at_ms: crate::now_ms(),
        updated_at_ms: crate::now_ms(),
        creator_id: actor.to_string(),
        workspace_root: None,
        metadata: Some(json!({
            "resource_access": {
                "owner_principal": { "kind": "human_user", "id": actor },
                "visibility": "private", "audience_principals": [],
                "created_by": actor, "updated_by": actor
            }
        })),
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    };
    automation.set_tenant_context(tenant);
    automation
}

#[tokio::test]
async fn operator_tools_enforce_private_automation_object_authority() {
    let state = test_state().await;
    let alice_tenant = tenant("alice", "org-a");
    let bob_tenant = tenant("bob", "org-a");
    state
        .put_automation_v2(automation("alice-private", &alice_tenant, "alice"))
        .await
        .unwrap();
    let alice_chat = chat(&state, alice_tenant.clone(), "alice").await;
    let bob_chat = chat(&state, bob_tenant.clone(), "bob").await;

    let inspect = tool(state.clone(), "automation_inspect");
    let denied = inspect
        .execute_for_tenant(
            json!({ "__dispatch_session_id": bob_chat.id, "automation_id": "alice-private" }),
            bob_tenant.clone(),
        )
        .await
        .expect_err("same-tenant private object must not be inspected");
    assert!(denied.to_string().contains("automation not found"));
    let list = inspect
        .execute_for_tenant(
            json!({ "__dispatch_session_id": bob_chat.id }),
            bob_tenant.clone(),
        )
        .await
        .unwrap()
        .metadata;
    assert_eq!(list["count"], json!(0));

    let draft = tool(state.clone(), "automation_manage_draft");
    for action in ["validate", "duplicate", "revise"] {
        let denied = draft
            .execute_for_tenant(
                json!({
                    "__dispatch_session_id": bob_chat.id, "action": action,
                    "automation_id": "alice-private", "new_automation_id": "bob-copy",
                    "idempotency_key": format!("bob-{action}")
                }),
                bob_tenant.clone(),
            )
            .await
            .expect_err("same-tenant private draft must not be read or changed");
        assert!(denied.to_string().contains("automation not found"));
    }
    let denied = tool(state.clone(), "automation_control")
        .execute_for_tenant(
            json!({
                "__dispatch_session_id": bob_chat.id, "action": "archive",
                "automation_id": "alice-private", "idempotency_key": "bob-archive"
            }),
            bob_tenant.clone(),
        )
        .await
        .expect_err("same-tenant private control must not change another actor's object");
    assert!(denied.to_string().contains("automation not found"));
    assert!(state.get_automation_v2("bob-copy").await.is_none());
    assert_eq!(
        state
            .get_automation_v2("alice-private")
            .await
            .unwrap()
            .creator_id,
        "alice"
    );

    let own = inspect
        .execute_for_tenant(
            json!({ "__dispatch_session_id": alice_chat.id, "automation_id": "alice-private" }),
            alice_tenant.clone(),
        )
        .await
        .expect("owner still reads own automation");
    assert_eq!(
        own.metadata["automation"]["automation_id"],
        json!("alice-private")
    );

    let mut shared = state.get_automation_v2("alice-private").await.unwrap();
    shared.metadata.as_mut().unwrap()["resource_access"]["visibility"] = json!("org");
    state.put_automation_v2(shared).await.unwrap();
    let shared_read = inspect
        .execute_for_tenant(
            json!({ "__dispatch_session_id": bob_chat.id, "automation_id": "alice-private" }),
            bob_tenant.clone(),
        )
        .await
        .expect("org-visible automation remains readable");
    assert_eq!(
        shared_read.metadata["automation"]["automation_id"],
        json!("alice-private")
    );
    let denied = tool(state.clone(), "automation_control")
        .execute_for_tenant(
            json!({
                "__dispatch_session_id": bob_chat.id, "action": "disable",
                "automation_id": "alice-private", "idempotency_key": "bob-disable-shared"
            }),
            bob_tenant.clone(),
        )
        .await
        .expect_err("org visibility is not write authority");
    assert!(denied.to_string().contains("automation not found"));
    let copied = draft
        .execute_for_tenant(
            json!({
                "__dispatch_session_id": bob_chat.id, "action": "duplicate",
                "automation_id": "alice-private", "new_automation_id": "bob-copy",
                "idempotency_key": "bob-copy-shared"
            }),
            bob_tenant.clone(),
        )
        .await
        .expect("shared read plus creation authority permits a private copy");
    assert_eq!(copied.metadata["status"], json!("draft_saved"));
    let copy = state.get_automation_v2("bob-copy").await.unwrap();
    assert_eq!(
        copy.metadata.as_ref().unwrap()["resource_access"]["owner_principal"]["id"],
        json!("bob")
    );
    assert_eq!(
        copy.metadata.as_ref().unwrap()["resource_access"]["visibility"],
        json!("private")
    );
    assert_eq!(
        copy.metadata.as_ref().unwrap()["enterprise_scope"]["owner_principal"]["id"],
        json!("bob")
    );
    let mut private_again = state.get_automation_v2("alice-private").await.unwrap();
    private_again.metadata.as_mut().unwrap()["resource_access"]["visibility"] = json!("private");
    state.put_automation_v2(private_again).await.unwrap();
    let denied_replay = draft
        .execute_for_tenant(
            json!({
                "__dispatch_session_id": bob_chat.id, "action": "duplicate",
                "automation_id": "alice-private", "new_automation_id": "bob-copy",
                "idempotency_key": "bob-copy-shared"
            }),
            bob_tenant.clone(),
        )
        .await
        .expect_err("revoked source access must not replay a cached automation result");
    assert!(denied_replay.to_string().contains("automation not found"));
    let own_control = tool(state.clone(), "automation_control")
        .execute_for_tenant(
            json!({
                "__dispatch_session_id": alice_chat.id, "action": "disable",
                "automation_id": "alice-private", "idempotency_key": "alice-disable"
            }),
            alice_tenant,
        )
        .await
        .expect("owner retains automation control");
    assert_eq!(own_control.metadata["status"], json!("paused"));
}

#[tokio::test]
async fn legacy_id_only_owner_and_named_admin_roles_retain_object_access() {
    let state = test_state().await;
    let alice_tenant = tenant("alice", "org-a");
    let bob_tenant = tenant("bob", "org-a");
    let mut legacy = automation("legacy-id-only-owner", &alice_tenant, "alice");
    legacy.metadata.as_mut().unwrap()["resource_access"]["owner_principal"] =
        json!({ "id": "alice" });
    state.put_automation_v2(legacy).await.unwrap();
    let inspect = tool(state.clone(), "automation_inspect");
    let alice_chat = chat(&state, alice_tenant.clone(), "alice").await;
    let bob_chat = chat(&state, bob_tenant.clone(), "bob").await;

    inspect
        .execute_for_tenant(
            json!({ "__dispatch_session_id": alice_chat.id, "automation_id": "legacy-id-only-owner" }),
            alice_tenant.clone(),
        )
        .await
        .expect("legacy owner IDs without a kind remain owned by the named human");
    assert!(inspect
        .execute_for_tenant(
            json!({ "__dispatch_session_id": bob_chat.id, "automation_id": "legacy-id-only-owner" }),
            bob_tenant.clone(),
        )
        .await
        .is_err());

    let mut admin = verified(bob_tenant.clone(), "bob");
    admin.roles.push("organization:admin".to_string());
    let mut admin_chat = Session::new(Some("Named admin access".to_string()), None);
    admin_chat.tenant_context = bob_tenant.clone();
    admin_chat.verified_tenant_context = Some(admin);
    state
        .storage
        .save_session(admin_chat.clone())
        .await
        .unwrap();
    inspect
        .execute_for_tenant(
            json!({ "__dispatch_session_id": admin_chat.id, "automation_id": "legacy-id-only-owner" }),
            bob_tenant,
        )
        .await
        .expect("signed organization admins retain same-tenant private-object access");

    let mut non_human = state
        .get_automation_v2("legacy-id-only-owner")
        .await
        .unwrap();
    non_human.metadata.as_mut().unwrap()["resource_access"]["owner_principal"]["kind"] =
        json!("service_account");
    state.put_automation_v2(non_human).await.unwrap();
    assert!(inspect
        .execute_for_tenant(
            json!({ "__dispatch_session_id": alice_chat.id, "automation_id": "legacy-id-only-owner" }),
            alice_tenant.clone(),
        )
        .await
        .is_err());

    let mut malformed = state
        .get_automation_v2("legacy-id-only-owner")
        .await
        .unwrap();
    malformed.metadata.as_mut().unwrap()["resource_access"]["owner_principal"]["kind"] =
        serde_json::Value::Null;
    state.put_automation_v2(malformed).await.unwrap();
    assert!(inspect
        .execute_for_tenant(
            json!({ "__dispatch_session_id": alice_chat.id, "automation_id": "legacy-id-only-owner" }),
            alice_tenant,
        )
        .await
        .is_err());
}

#[tokio::test]
async fn direct_automation_http_patch_cannot_transfer_legacy_owner() {
    use tower::ServiceExt;

    let state = test_state().await;
    let alice_tenant = tenant("alice", "org-a");
    let bob_tenant = tenant("bob", "org-a");
    let mut legacy = automation("legacy-http-owner", &alice_tenant, "alice");
    legacy
        .metadata
        .as_mut()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("resource_access");
    let legacy = state.put_automation_v2(legacy).await.unwrap();
    state.get_or_bootstrap_automation_governance(&legacy).await;
    state
        .grant_automation_modify_access(
            "legacy-http-owner",
            crate::automation_v2::governance::GovernanceActorRef::human(
                Some("bob".to_string()),
                "test",
            ),
            crate::automation_v2::governance::GovernanceActorRef::human(
                Some("alice".to_string()),
                "test",
            ),
            None,
            &alice_tenant,
            || Ok(()),
        )
        .await
        .unwrap();

    let bob = automation_http(&state, bob_tenant.clone(), "bob", false, true);
    let read = bob
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .uri("/automations/v2/legacy-http-owner")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read.status(), axum::http::StatusCode::NOT_FOUND);
    let denied = bob
        .oneshot(
            axum::http::Request::builder()
                .method("PATCH")
                .uri("/automations/v2/legacy-http-owner")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "name": "Bob takeover" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), axum::http::StatusCode::FORBIDDEN);

    let admin = automation_http(&state, bob_tenant.clone(), "bob", true, true);
    let takeover = admin
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("PATCH")
                .uri("/automations/v2/legacy-http-owner")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({
                        "metadata": {
                            "resource_access": {
                                "owner_principal": { "kind": "human_user", "id": "bob" },
                                "visibility": "private"
                            }
                        }
                    })
                    .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(takeover.status(), axum::http::StatusCode::BAD_REQUEST);
    let edited = admin
        .clone()
        .oneshot(
            axum::http::Request::builder()
                .method("PATCH")
                .uri("/automations/v2/legacy-http-owner")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "name": "Admin edit" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(edited.status(), axum::http::StatusCode::OK);
    let stored = state.get_automation_v2("legacy-http-owner").await.unwrap();
    assert_eq!(stored.name, "Admin edit");
    assert_eq!(stored.tenant_context().actor_id.as_deref(), Some("alice"));
    assert_eq!(stored.creator_id, "alice");

    let shared = admin
        .oneshot(
            axum::http::Request::builder()
                .method("POST")
                .uri("/automations/v2/legacy-http-owner/share")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(
                    json!({ "visibility": "private" }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(shared.status(), axum::http::StatusCode::OK);
    let stored = state.get_automation_v2("legacy-http-owner").await.unwrap();
    assert_eq!(
        stored.metadata.as_ref().unwrap()["resource_access"]["owner_principal"]["id"],
        json!("alice")
    );

    let bob = automation_http(&state, bob_tenant, "bob", false, true);
    let read = bob
        .oneshot(
            axum::http::Request::builder()
                .uri("/automations/v2/legacy-http-owner")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read.status(), axum::http::StatusCode::NOT_FOUND);
    let alice = automation_http(&state, alice_tenant, "alice", false, false);
    let read = alice
        .oneshot(
            axum::http::Request::builder()
                .uri("/automations/v2/legacy-http-owner")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(read.status(), axum::http::StatusCode::OK);
}

#[tokio::test]
async fn operator_revision_preserves_legacy_automation_owner() {
    let state = test_state().await;
    let alice_tenant = tenant("alice", "org-a");
    let bob_tenant = tenant("bob", "org-a");
    let mut legacy = automation("legacy-owned", &alice_tenant, "alice");
    legacy
        .metadata
        .as_mut()
        .unwrap()
        .as_object_mut()
        .unwrap()
        .remove("resource_access");
    state.put_automation_v2(legacy).await.unwrap();
    let mut admin = verified(bob_tenant.clone(), "bob");
    admin.roles = vec!["admin".to_string()];
    let mut session = Session::new(Some("Admin revision".to_string()), None);
    session.tenant_context = bob_tenant.clone();
    session.verified_tenant_context = Some(admin);
    state.storage.save_session(session.clone()).await.unwrap();

    tool(state.clone(), "automation_manage_draft")
        .execute_for_tenant(
            json!({
                "__dispatch_session_id": session.id,
                "action": "revise",
                "automation_id": "legacy-owned",
                "idempotency_key": "legacy-admin-revision"
            }),
            bob_tenant.clone(),
        )
        .await
        .expect("an administrator may revise the legacy automation");
    let stored = state.get_automation_v2("legacy-owned").await.unwrap();
    assert_eq!(stored.tenant_context().actor_id.as_deref(), Some("alice"));
    assert_eq!(stored.creator_id, "alice");
    let bob_operator = chat(&state, bob_tenant.clone(), "bob").await;
    let denied = tool(state.clone(), "automation_inspect")
        .execute_for_tenant(
            json!({ "__dispatch_session_id": bob_operator.id, "automation_id": "legacy-owned" }),
            bob_tenant,
        )
        .await
        .expect_err("an administrator's edit must not make them a legacy object's owner");
    assert!(denied.to_string().contains("automation not found"));
    let alice_chat = chat(&state, alice_tenant.clone(), "alice").await;
    tool(state, "automation_inspect")
        .execute_for_tenant(
            json!({ "__dispatch_session_id": alice_chat.id, "automation_id": "legacy-owned" }),
            alice_tenant,
        )
        .await
        .expect("the original owner retains access");
}

#[tokio::test]
async fn deleted_automation_id_cannot_expose_private_retained_runs() {
    let state = test_state().await;
    let alice_tenant = tenant("alice", "org-a");
    let bob_tenant = tenant("bob", "org-a");
    let mut private = automation("retired-id", &alice_tenant, "alice");
    private.description = Some("alice-private-run-marker".to_string());
    private.status = crate::AutomationV2Status::Active;
    let private = state.put_automation_v2(private).await.unwrap();
    state
        .create_automation_v2_run(&private, "manual")
        .await
        .unwrap();
    let alice_chat = chat(&state, alice_tenant.clone(), "alice").await;
    let own_runs = tool(state.clone(), "automation_inspect")
        .execute_for_tenant(
            json!({ "__dispatch_session_id": alice_chat.id, "automation_id": "retired-id" }),
            alice_tenant,
        )
        .await
        .expect("the owner can inspect runs from the same definition incarnation");
    assert_eq!(own_runs.metadata["runs"].as_array().unwrap().len(), 1);
    state
        .delete_automation_v2_with_governance(
            "retired-id",
            crate::automation_v2::governance::GovernanceActorRef::system("test-delete"),
        )
        .await
        .unwrap();
    // Simulate a crash after the tombstone reached disk but before the old
    // definition shard was removed. Startup must let governance win.
    state
        .automations_v2
        .write()
        .await
        .insert("retired-id".to_string(), private.clone());
    state.persist_automations_v2().await.unwrap();
    state.automations_v2.write().await.clear();
    state.load_automations_v2().await.unwrap();
    assert!(state.get_automation_v2("retired-id").await.is_some());
    state.load_automation_governance().await.unwrap();
    state.bootstrap_automation_governance().await.unwrap();
    assert!(state.get_automation_v2("retired-id").await.is_none());
    state.load_automations_v2().await.unwrap();
    assert!(
        state.get_automation_v2("retired-id").await.is_none(),
        "startup reconciliation must remove the stale definition shard"
    );
    state.load_automation_v2_runs().await.unwrap();
    assert!(
        state.get_automation_v2("retired-id").await.is_none(),
        "reloading retained runs must not resurrect a deleted definition"
    );
    let retained = state
        .list_automation_v2_runs_scoped(Some("retired-id"), Some("org-a"), Some("workspace-a"), 10)
        .await;
    assert_eq!(retained.len(), 1);
    assert_eq!(
        retained[0]
            .automation_snapshot
            .as_ref()
            .unwrap()
            .description
            .as_deref(),
        Some("alice-private-run-marker")
    );

    state
        .put_automation_v2(automation("bob-source", &bob_tenant, "bob"))
        .await
        .unwrap();
    let bob_chat = chat(&state, bob_tenant.clone(), "bob").await;
    let denied = tool(state.clone(), "automation_manage_draft")
        .execute_for_tenant(
            json!({
                "__dispatch_session_id": bob_chat.id,
                "action": "duplicate",
                "automation_id": "bob-source",
                "new_automation_id": "retired-id",
                "idempotency_key": "claim-retired-id"
            }),
            bob_tenant.clone(),
        )
        .await
        .expect_err("a retained deleted id cannot be claimed by another owner");
    assert!(denied.to_string().contains("automation id already exists"));

    let restored = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        state.restore_deleted_automation_v2(
            "retired-id",
            crate::automation_v2::governance::GovernanceActorRef::system("test-restore"),
            None,
            &tenant("alice", "org-a"),
            || Ok(()),
        ),
    )
    .await
    .expect("restore must not deadlock with the creation lock")
    .unwrap()
    .expect("the original deleted automation remains restorable");
    assert_eq!(restored.creator_id, "alice");
    state
        .delete_automation_v2_with_governance(
            "retired-id",
            crate::automation_v2::governance::GovernanceActorRef::system("test-delete-again"),
        )
        .await
        .unwrap();

    // Simulate an older deployment that reused the id in the same millisecond.
    // The timestamp alone cannot decide access to Alice's private run.
    let mut replacement = automation("retired-id", &bob_tenant, "bob");
    replacement.created_at_ms = private.created_at_ms;
    state
        .automations_v2
        .write()
        .await
        .insert("retired-id".to_string(), replacement);
    let inspected = tool(state.clone(), "automation_inspect")
        .execute_for_tenant(
            json!({ "__dispatch_session_id": bob_chat.id, "automation_id": "retired-id" }),
            bob_tenant,
        )
        .await
        .expect("the replacement owner may inspect the replacement definition");
    assert_eq!(inspected.metadata["runs"], json!([]));
    let alice_scope = tenant("alice", "org-a");
    let restore_audit_count = |events: &[crate::audit::ProtectedAuditEnvelope]| {
        events
            .iter()
            .filter(|event| event.event_type == "automation.governance.restored")
            .count()
    };
    let before = crate::audit::try_load_protected_audit_events_for_tenant(&state, &alice_scope)
        .await
        .unwrap();
    let error = state
        .restore_deleted_automation_v2(
            "retired-id",
            crate::automation_v2::governance::GovernanceActorRef::system("test-colliding-restore"),
            None,
            &alice_scope,
            || Ok(()),
        )
        .await
        .expect_err("restore must not overwrite a live definition");
    assert!(error.to_string().contains("automation id already exists"));
    let after = crate::audit::try_load_protected_audit_events_for_tenant(&state, &alice_scope)
        .await
        .unwrap();
    assert_eq!(restore_audit_count(&before), restore_audit_count(&after));
}

#[tokio::test]
async fn failed_restore_does_not_write_a_success_audit_event() {
    use std::sync::atomic::{AtomicUsize, Ordering};

    let state = test_state().await;
    let alice_tenant = tenant("alice", "org-a");
    state
        .put_automation_v2(automation("failed-restore", &alice_tenant, "alice"))
        .await
        .unwrap();
    state
        .delete_automation_v2_with_governance(
            "failed-restore",
            crate::automation_v2::governance::GovernanceActorRef::system("test-delete"),
        )
        .await
        .unwrap();
    let restored_count = |events: &[crate::audit::ProtectedAuditEnvelope]| {
        events
            .iter()
            .filter(|event| event.event_type == "automation.governance.restored")
            .count()
    };
    let before = crate::audit::try_load_protected_audit_events_for_tenant(&state, &alice_tenant)
        .await
        .unwrap();
    let checks = AtomicUsize::new(0);
    let error = state
        .restore_deleted_automation_v2(
            "failed-restore",
            crate::automation_v2::governance::GovernanceActorRef::system("test-restore"),
            None,
            &alice_tenant,
            || {
                if checks.fetch_add(1, Ordering::SeqCst) == 0 {
                    Ok(())
                } else {
                    anyhow::bail!("restore authority was revoked")
                }
            },
        )
        .await
        .expect_err("a revoked authorization check must stop restore");
    assert!(error.to_string().contains("restore authority was revoked"));
    assert!(state.get_automation_v2("failed-restore").await.is_none());
    assert!(state
        .get_deleted_automation_v2("failed-restore")
        .await
        .is_some());
    let after = crate::audit::try_load_protected_audit_events_for_tenant(&state, &alice_tenant)
        .await
        .unwrap();
    assert_eq!(restored_count(&before), restored_count(&after));
}

#[tokio::test]
async fn pending_restore_replays_once_after_audit_crash_boundary() {
    let mut state = test_state().await;
    let alice_tenant = tenant("alice", "org-a");
    let mut original = automation("audit-failed-restore", &alice_tenant, "alice");
    original.status = crate::AutomationV2Status::Active;
    state.put_automation_v2(original).await.unwrap();
    state
        .delete_automation_v2_with_governance(
            "audit-failed-restore",
            crate::automation_v2::governance::GovernanceActorRef::system("test-delete"),
        )
        .await
        .unwrap();

    let real_audit_path = state.protected_audit_path.clone();
    let failed_audit_path = real_audit_path.with_file_name("restore-audit-is-a-directory");
    tokio::fs::create_dir_all(&failed_audit_path).await.unwrap();
    state.protected_audit_path = failed_audit_path;
    let actor = crate::automation_v2::governance::GovernanceActorRef::system("test-restore");
    state
        .restore_deleted_automation_v2(
            "audit-failed-restore",
            actor.clone(),
            None,
            &alice_tenant,
            || Ok(()),
        )
        .await
        .expect_err("the required audit must fail");
    state.protected_audit_path = real_audit_path;

    let pending = state
        .automation_governance
        .read()
        .await
        .deleted_automations
        .get("audit-failed-restore")
        .and_then(|deleted| deleted.pending_restore.clone())
        .expect("durable pending intent retains the tombstone");
    assert!(!pending.operation_id.is_empty());
    assert!(state
        .get_automation_v2("audit-failed-restore")
        .await
        .is_none());
    let shard_path = state
        .automations_v2_path
        .parent()
        .unwrap()
        .join("automations-v2/audit-failed-restore.json");
    assert!(
        shard_path.exists(),
        "the shard was staged before audit failure"
    );

    let wrong_actor = state
        .restore_deleted_automation_v2(
            "audit-failed-restore",
            crate::automation_v2::governance::GovernanceActorRef::system("different-actor"),
            None,
            &alice_tenant,
            || Ok(()),
        )
        .await
        .expect_err("a different actor cannot take over the pending restore");
    assert!(wrong_actor.to_string().contains("different request"));

    // Simulate a crash after the protected append returned but before the
    // governance snapshot cleared the tombstone. Retry must find this exact
    // operation in the verified ledger and must not append a duplicate.
    crate::audit::append_protected_audit_event_once(
        &state,
        &pending.operation_id,
        "automation.governance.restored",
        &pending.tenant_context,
        pending
            .restored_by
            .actor_id
            .clone()
            .or_else(|| pending.restored_by.source.clone()),
        json!({
            "automationID": "audit-failed-restore",
            "restoredBy": pending.restored_by,
            "approvalID": pending.approval_id,
            "operationID": pending.operation_id,
        }),
    )
    .await
    .unwrap();
    let collision = crate::audit::append_protected_audit_event_once(
        &state,
        &pending.operation_id,
        "automation.governance.restored",
        &pending.tenant_context,
        None,
        json!({"automationID": "another-object"}),
    )
    .await
    .expect_err("a reused audit event id with changed content must fail closed");
    assert!(collision.to_string().contains("different event"));

    state.automations_v2.write().await.clear();
    state.load_automation_governance().await.unwrap();
    state.load_automations_v2().await.unwrap();
    assert!(
        state
            .get_automation_v2("audit-failed-restore")
            .await
            .is_none(),
        "a staged shard must never enter the startup live map"
    );
    state.bootstrap_automation_governance().await.unwrap();
    assert!(state
        .get_deleted_automation_v2("audit-failed-restore")
        .await
        .is_none());
    assert_eq!(
        state
            .get_automation_v2("audit-failed-restore")
            .await
            .unwrap()
            .status,
        crate::AutomationV2Status::Active
    );
    let rows = crate::audit::try_load_protected_audit_events_for_tenant(&state, &alice_tenant)
        .await
        .unwrap();
    assert_eq!(
        rows.iter()
            .filter(|row| row.event_type == "automation.governance.restored")
            .count(),
        1
    );
}

#[tokio::test]
async fn pending_restore_survives_shard_failure_without_success_audit() {
    let state = test_state().await;
    let alice_tenant = tenant("alice", "org-a");
    let mut original = automation("restore-shard-failure", &alice_tenant, "alice");
    original.status = crate::AutomationV2Status::Active;
    state.put_automation_v2(original).await.unwrap();
    state
        .delete_automation_v2_with_governance(
            "restore-shard-failure",
            crate::automation_v2::governance::GovernanceActorRef::system("test-delete"),
        )
        .await
        .unwrap();
    let shard_path = state
        .automations_v2_path
        .parent()
        .unwrap()
        .join("automations-v2/restore-shard-failure.json");
    assert!(!shard_path.exists());
    tokio::fs::create_dir_all(&shard_path).await.unwrap();
    let actor = crate::automation_v2::governance::GovernanceActorRef::system("test-restore");
    state
        .restore_deleted_automation_v2(
            "restore-shard-failure",
            actor.clone(),
            None,
            &alice_tenant,
            || Ok(()),
        )
        .await
        .expect_err("a shard write failure must stop before success audit");
    assert!(state
        .get_automation_v2("restore-shard-failure")
        .await
        .is_none());
    assert!(state
        .automation_governance
        .read()
        .await
        .deleted_automations
        .get("restore-shard-failure")
        .and_then(|deleted| deleted.pending_restore.as_ref())
        .is_some());
    let before = crate::audit::try_load_protected_audit_events_for_tenant(&state, &alice_tenant)
        .await
        .unwrap();
    assert!(!before
        .iter()
        .any(|row| row.event_type == "automation.governance.restored"));

    tokio::fs::remove_dir(&shard_path).await.unwrap();
    state
        .restore_deleted_automation_v2("restore-shard-failure", actor, None, &alice_tenant, || {
            Ok(())
        })
        .await
        .unwrap()
        .expect("the same actor can resume after storage recovery");
    let after = crate::audit::try_load_protected_audit_events_for_tenant(&state, &alice_tenant)
        .await
        .unwrap();
    assert_eq!(
        after
            .iter()
            .filter(|row| row.event_type == "automation.governance.restored")
            .count(),
        1
    );
}

#[tokio::test]
async fn operator_duplicate_cannot_overwrite_a_concurrent_destination() {
    let state = test_state().await;
    let alice_tenant = tenant("alice", "org-a");
    let bob_tenant = tenant("bob", "org-b");
    state
        .put_automation_v2(automation("source-a", &alice_tenant, "alice"))
        .await
        .unwrap();
    state
        .put_automation_v2(automation("source-b", &bob_tenant, "bob"))
        .await
        .unwrap();
    let alice_chat = chat(&state, alice_tenant.clone(), "alice").await;
    let bob_chat = chat(&state, bob_tenant.clone(), "bob").await;
    let guard = state.automations_v2_persistence.lock().await;
    let first_tool = tool(state.clone(), "automation_manage_draft");
    let second_tool = tool(state.clone(), "automation_manage_draft");
    let first_tenant = alice_tenant.clone();
    let second_tenant = bob_tenant.clone();
    let first = tokio::spawn(async move {
        first_tool
            .execute_for_tenant(
                json!({
                    "__dispatch_session_id": alice_chat.id, "action": "duplicate",
                    "automation_id": "source-a", "new_automation_id": "contended-copy",
                    "idempotency_key": "collision-a"
                }),
                first_tenant,
            )
            .await
    });
    let second = tokio::spawn(async move {
        second_tool
            .execute_for_tenant(
                json!({
                    "__dispatch_session_id": bob_chat.id, "action": "duplicate",
                    "automation_id": "source-b", "new_automation_id": "contended-copy",
                    "idempotency_key": "collision-b"
                }),
                second_tenant,
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let a = state
                .get_idempotency_key(&alice_tenant, "operator.automation_draft", "collision-a")
                .await;
            let b = state
                .get_idempotency_key(&bob_tenant, "operator.automation_draft", "collision-b")
                .await;
            if a.is_some() && b.is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("both calls passed destination precheck before either write");
    drop(guard);
    let first = first.await.unwrap();
    let second = second.await.unwrap();
    assert_ne!(
        first.is_ok(),
        second.is_ok(),
        "exactly one duplicate may claim the global id"
    );
    let stored = state.get_automation_v2("contended-copy").await.unwrap();
    let winner = if first.is_ok() {
        &alice_tenant
    } else {
        &bob_tenant
    };
    assert_eq!(stored.tenant_context().org_id, winner.org_id);
}
