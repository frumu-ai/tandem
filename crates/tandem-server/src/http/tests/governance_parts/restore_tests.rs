// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Included into governance.rs; exercises the hosted HTTP continuation rather
// than a direct state-only retry that could bypass current admin authority.

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn hosted_pending_restore_needs_current_admin_and_exact_expired_receipt() {
    use crate::automation_v2::governance::{
        GovernanceActorRef, GovernanceApprovalRequest, GovernanceApprovalRequestType,
        GovernanceApprovalStatus, GovernanceResourceRef,
    };

    let mut state = test_state().await;
    let automation_id = "hosted-pending-restore";
    let approval_id = "approval-hosted-pending-restore";
    let tenant = TenantContext::explicit("org-a", "workspace-a", Some("operator-a".to_string()));
    let actor = GovernanceActorRef::human(Some("operator-a".to_string()), "tandem-test");
    let automation =
        super::global::create_test_automation_v2_for_tenant(&state, automation_id, &tenant).await;
    state
        .delete_automation_v2_with_governance(
            automation_id,
            GovernanceActorRef::system("test-delete"),
        )
        .await
        .unwrap();
    assert_eq!(automation.automation_id, automation_id);

    let now = crate::now_ms();
    let approval = GovernanceApprovalRequest {
        approval_id: approval_id.to_string(),
        request_type: GovernanceApprovalRequestType::RetirementAction,
        requested_by: actor.clone(),
        target_resource: GovernanceResourceRef {
            resource_type: "automation".to_string(),
            id: automation_id.to_string(),
        },
        rationale: "independent restore review".to_string(),
        context: json!({"action": "restore_automation", "parameters": {}}),
        status: GovernanceApprovalStatus::Approved,
        expires_at_ms: now + 60_000,
        tenant_context: Some(tenant.clone()),
        reviewed_by: Some(GovernanceActorRef::human(
            Some("reviewer-a".to_string()),
            "tandem-test",
        )),
        reviewed_at_ms: Some(now),
        review_notes: None,
        created_at_ms: now,
        updated_at_ms: now,
    };
    state
        .automation_governance
        .write()
        .await
        .approvals
        .insert(approval_id.to_string(), approval);
    state.persist_automation_governance().await.unwrap();
    let reservation_id = state
        .reserve_governance_mutation_approval(
            approval_id,
            &actor,
            automation_id,
            "restore_automation",
            &json!({}),
            &[
                GovernanceApprovalRequestType::RetirementAction,
                GovernanceApprovalRequestType::LifecycleReview,
            ],
            &tenant,
        )
        .await
        .unwrap();

    let real_audit_path = state.protected_audit_path.clone();
    let failed_audit_path = real_audit_path.with_file_name("hosted-restore-audit-directory");
    tokio::fs::create_dir_all(&failed_audit_path).await.unwrap();
    state.protected_audit_path = failed_audit_path;
    state
        .restore_deleted_automation_v2_with_reservation(
            automation_id,
            actor.clone(),
            Some(approval_id.to_string()),
            Some(reservation_id.clone()),
            &tenant,
            || Ok(()),
        )
        .await
        .expect_err("audit outage must retain hosted pending restore");
    state.protected_audit_path = real_audit_path;
    assert!(state.get_automation_v2(automation_id).await.is_none());

    // On restart, governance loads before definitions. The staged shard must
    // never enter the live map, and bootstrap must not autonomously finish a
    // hosted operation under a potentially revoked admin.
    state.automations_v2.write().await.clear();
    state.load_automation_governance().await.unwrap();
    state.load_automations_v2().await.unwrap();
    assert!(state.get_automation_v2(automation_id).await.is_none());
    state.bootstrap_automation_governance().await.unwrap();
    assert!(state.get_automation_v2(automation_id).await.is_none());
    assert!(state
        .get_deleted_automation_v2(automation_id)
        .await
        .is_some());
    state.automations_v2.write().await.clear();
    state.load_automations_v2().await.unwrap();
    assert!(state.get_automation_v2(automation_id).await.is_none());
    assert!(
        !state
            .pending_restore_matches_approval(
                automation_id,
                &actor,
                approval_id,
                "wrong-reservation",
                &tenant,
            )
            .await
    );
    assert!(
        !state
            .pending_restore_matches_approval(
                automation_id,
                &GovernanceActorRef::human(Some("another-admin".to_string()), "tandem-test"),
                approval_id,
                &reservation_id,
                &tenant,
            )
            .await
    );
    assert!(
        !state
            .pending_restore_matches_approval(
                automation_id,
                &actor,
                "another-approval",
                &reservation_id,
                &tenant,
            )
            .await
    );
    assert!(
        !state
            .pending_restore_matches_approval(
                automation_id,
                &actor,
                approval_id,
                &reservation_id,
                &TenantContext::explicit("org-b", "workspace-a", Some("operator-a".to_string()),),
            )
            .await
    );

    // An expired receipt may continue only its already-prepared exact intent.
    // Without current admin authority, even that exact continuation is denied.
    {
        let mut governance = state.automation_governance.write().await;
        governance
            .approvals
            .get_mut(approval_id)
            .unwrap()
            .expires_at_ms = crate::now_ms() - 1;
    }
    state.persist_automation_governance().await.unwrap();
    let restore_request = || {
        Request::builder()
            .method("POST")
            .uri(format!("/automations/v2/{automation_id}/restore"))
            .header("x-tandem-org-id", "org-a")
            .header("x-tandem-workspace-id", "workspace-a")
            .header("x-tandem-actor-id", "operator-a")
            .header("x-tandem-approval-id", approval_id)
            .body(Body::empty())
            .unwrap()
    };
    let principal = tandem_types::RequestPrincipal::authenticated_user("operator-a", "tandem-test");
    let revoked_projection = tandem_types::VerifiedTenantContext {
        tenant_context: tenant.clone(),
        human_actor: tandem_types::HumanActor::tandem_user("operator-a"),
        authority_chain: tandem_types::AuthorityChain::from_request(principal),
        roles: Vec::new(),
        org_units: Vec::new(),
        capabilities: Vec::new(),
        policy_version: None,
        strict_projection: None,
        issuer: "tandem-test".to_string(),
        audience: "tandem-runtime".to_string(),
        issued_at_ms: crate::now_ms(),
        expires_at_ms: crate::now_ms() + 60_000,
        assertion_id: "revoked-restore-admin".to_string(),
        assertion_key_id: None,
    };
    let denied = app_router(state.clone())
        .layer(axum::Extension(revoked_projection))
        .oneshot(restore_request())
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::FORBIDDEN);
    assert!(state.get_automation_v2(automation_id).await.is_none());

    let app = verified_governance_app(state.clone(), "org-a", "workspace-a", "operator-a");
    let restored = app.clone().oneshot(restore_request()).await.unwrap();
    let restored_status = restored.status();
    let restored_body = response_json(restored).await;
    assert_eq!(restored_status, StatusCode::OK, "{restored_body:?}");
    assert!(state.get_automation_v2(automation_id).await.is_some());
    assert!(state
        .get_deleted_automation_v2(automation_id)
        .await
        .is_none());
    let consumed = state
        .get_governance_approval_request(approval_id)
        .await
        .unwrap();
    assert_eq!(
        consumed.context["_mutation_consumption"]["reservationID"],
        reservation_id
    );
    let rows = crate::audit::try_load_protected_audit_events_for_tenant(&state, &tenant)
        .await
        .unwrap();
    assert_eq!(
        rows.iter()
            .filter(|row| row.event_type == "automation.governance.restored")
            .count(),
        1
    );
    assert_eq!(
        rows.iter()
            .filter(|row| row.event_type == "automation.governance.approval.mutation_consumed")
            .count(),
        1
    );
    let duplicate = app.oneshot(restore_request()).await.unwrap();
    assert_eq!(duplicate.status(), StatusCode::NOT_FOUND);
}
