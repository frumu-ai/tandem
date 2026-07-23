// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn lifecycle_pause_blocks_run_creation_outside_tenant_quarantine() {
    let state = test_state().await;
    let tenant = TenantContext::explicit(
        "org-lifecycle-pause",
        "workspace-lifecycle-pause",
        Some("owner".to_string()),
    );
    let automation = super::global::create_test_automation_v2_for_tenant(
        &state,
        "auto-governance-lifecycle-pause",
        &tenant,
    )
    .await;
    {
        let mut governance = state.automation_governance.write().await;
        let record = governance
            .records
            .get_mut(&automation.automation_id)
            .expect("governance record");
        record.review_required = true;
        record.review_kind = Some(
            crate::automation_v2::governance::AutomationLifecycleReviewKind::DependencyRevoked,
        );
        record.paused_for_lifecycle = true;
    }
    assert!(state
        .create_automation_v2_run(&automation, "scheduler")
        .await
        .is_err());
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn dependency_revocation_supersedes_unrelated_lifecycle_review() {
    let state = test_state().await;
    let tenant = TenantContext::explicit(
        "org-dependency-review",
        "workspace-dependency-review",
        Some("owner".to_string()),
    );
    let automation = super::global::create_test_automation_v2_for_tenant(
        &state,
        "auto-governance-dependency-review",
        &tenant,
    )
    .await;
    {
        let mut governance = state.automation_governance.write().await;
        let record = governance
            .records
            .get_mut(&automation.automation_id)
            .expect("governance record");
        record.review_required = true;
        record.review_kind = Some(
            crate::automation_v2::governance::AutomationLifecycleReviewKind::HealthDrift,
        );
        record.paused_for_lifecycle = true;
        record.review_requested_at_ms = Some(crate::now_ms());
    }
    let unrelated = state
        .request_approval(
            crate::automation_v2::governance::GovernanceApprovalRequestType::LifecycleReview,
            crate::automation_v2::governance::GovernanceActorRef::system("health_review"),
            crate::automation_v2::governance::GovernanceResourceRef {
                resource_type: "automation".to_string(),
                id: automation.automation_id.clone(),
            },
            "review health drift".to_string(),
            json!({
                "trigger": "health_drift",
                "automationID": automation.automation_id,
                "evidence": {"emptyOutputCount": 3}
            }),
            None,
            &tenant,
        )
        .await
        .expect("request unrelated lifecycle review");
    let timed_out_approval_id = format!("{}-timed-out", unrelated.approval_id);
    {
        let mut governance = state.automation_governance.write().await;
        let mut timed_out = unrelated.clone();
        timed_out.approval_id = timed_out_approval_id.clone();
        timed_out.expires_at_ms = crate::now_ms().saturating_sub(1);
        timed_out.updated_at_ms = crate::now_ms();
        governance
            .approvals
            .insert(timed_out.approval_id.clone(), timed_out);
    }

    state
        .pause_automation_for_dependency_revocation(
            &automation.automation_id,
            "connected capability revoked".to_string(),
            json!({"capability": "mcp:revoked", "server": "notion"}),
            &tenant,
        )
        .await
        .expect("pause for dependency revocation");

    let superseded = state
        .get_governance_approval_request(&unrelated.approval_id)
        .await
        .expect("superseded lifecycle review");
    assert_eq!(
        superseded.status,
        crate::automation_v2::governance::GovernanceApprovalStatus::Expired
    );
    let timed_out = state
        .get_governance_approval_request(&timed_out_approval_id)
        .await
        .expect("timed-out lifecycle review");
    assert_eq!(
        timed_out.status,
        crate::automation_v2::governance::GovernanceApprovalStatus::Expired,
        "a timed-out Pending receipt must be materialized as Expired"
    );
    let record = state
        .get_automation_governance(&automation.automation_id)
        .await
        .expect("dependency-paused governance record");
    let dependency_review_id = record
        .review_request_id
        .clone()
        .expect("dependency-specific review id");
    assert_ne!(dependency_review_id, unrelated.approval_id);
    assert_eq!(
        record.review_kind,
        Some(
            crate::automation_v2::governance::AutomationLifecycleReviewKind::DependencyRevoked
        )
    );
    let dependency_review = state
        .get_governance_approval_request(&dependency_review_id)
        .await
        .expect("dependency-specific approval");
    assert_eq!(
        dependency_review.status,
        crate::automation_v2::governance::GovernanceApprovalStatus::Pending
    );
    assert_eq!(
        dependency_review
            .context
            .get("trigger")
            .and_then(Value::as_str),
        Some("dependency_revoked")
    );
    assert_eq!(
        dependency_review
            .context
            .pointer("/evidence/evidence/capability"),
        Some(&json!("mcp:revoked"))
    );

    let old_retry = state
        .decide_approval_request(
            &unrelated.approval_id,
            crate::automation_v2::governance::GovernanceActorRef::human(
                Some("reviewer".to_string()),
                "dependency_review_test",
            ),
            true,
            Some("attempt to approve superseded review".to_string()),
            &tenant,
        )
        .await
        .expect("superseded receipt remains idempotent")
        .expect("superseded approval remains addressable");
    assert_eq!(
        old_retry.status,
        crate::automation_v2::governance::GovernanceApprovalStatus::Expired
    );
    let after_old_retry = state
        .get_automation_governance(&automation.automation_id)
        .await
        .expect("dependency review remains active");
    assert!(after_old_retry.review_required);
    assert_eq!(
        after_old_retry.review_request_id.as_deref(),
        Some(dependency_review_id.as_str())
    );
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn dependency_revocation_writer_serializes_with_queued_run_claim() {
    let state = test_state().await;
    let tenant = TenantContext::explicit(
        "org-dependency-claim-race",
        "workspace-dependency-claim-race",
        Some("owner".to_string()),
    );
    let automation = super::global::create_test_automation_v2_for_tenant(
        &state,
        "auto-governance-dependency-claim-race",
        &tenant,
    )
    .await;
    let run = state
        .create_automation_v2_run(&automation, "scheduler")
        .await
        .expect("queue run before dependency pause");

    // Hold the lifecycle writer before starting the claimant. The claim must
    // wait for this writer rather than acquiring the hot-run map and changing
    // Queued -> Running behind it.
    let mut governance = state.automation_governance.write().await;
    let claimant_state = state.clone();
    let run_id = run.run_id.clone();
    let mut claimant = tokio::spawn(async move {
        claimant_state
            .claim_specific_automation_v2_run(&run_id)
            .await
    });
    tokio::task::yield_now().await;
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(50), &mut claimant)
            .await
            .is_err(),
        "claim must wait while the governance writer is active"
    );
    assert_eq!(
        state
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("queued run")
            .status,
        crate::AutomationRunStatus::Queued
    );

    let record = governance
        .records
        .get_mut(&automation.automation_id)
        .expect("governance record");
    record.creation_paused = true;
    record.paused_for_lifecycle = true;
    record.review_required = true;
    drop(governance);

    assert!(
        claimant.await.expect("claim task").is_none(),
        "claim must recheck the lifecycle gate after the writer"
    );
    assert_ne!(
        state
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("unclaimed run")
            .status,
        crate::AutomationRunStatus::Running
    );
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn dependency_revocation_pauses_already_queued_run_and_blocks_the_next_one() {
    let state = test_state().await;
    let tenant = TenantContext::explicit(
        "org-dependency-pause",
        "workspace-dependency-pause",
        Some("owner".to_string()),
    );
    let automation = super::global::create_test_automation_v2_for_tenant(
        &state,
        "auto-governance-dependency-pause",
        &tenant,
    )
    .await;
    let first = state
        .create_automation_v2_run(&automation, "scheduler")
        .await
        .expect("queue run before dependency pause");
    assert_eq!(first.status, crate::AutomationRunStatus::Queued);
    let mut queued = vec![first.clone()];
    {
        let mut runs = state.automation_v2_runs.write().await;
        for index in 1..101 {
            let mut run = first.clone();
            run.run_id = format!("{}-{index}", first.run_id);
            runs.insert(run.run_id.clone(), run.clone());
            queued.push(run);
        }
    }

    state
        .pause_automation_for_dependency_revocation(
            &automation.automation_id,
            "connected capability revoked".to_string(),
            json!({"capability": "mcp:revoked"}),
            &tenant,
        )
        .await
        .expect("pause for dependency revocation");

    for queued in queued {
        let paused = state
            .get_automation_v2_run(&queued.run_id)
            .await
            .expect("queued run remains recorded");
        assert_eq!(paused.status, crate::AutomationRunStatus::Paused);
    }
    assert!(state
        .create_automation_v2_run(&automation, "scheduler")
        .await
        .is_err());
}

#[cfg(feature = "premium-governance")]
#[tokio::test]
async fn dependency_revocation_failure_allows_only_exact_route_retry() {
    let mut state = test_state().await;
    let tenant = TenantContext::explicit(
        "org-dependency-retry",
        "workspace-dependency-retry",
        Some("operator-a".to_string()),
    );
    let automation = super::global::create_test_automation_v2_for_tenant(
        &state,
        "auto-governance-dependency-retry",
        &tenant,
    )
    .await;
    let queued = state
        .create_automation_v2_run(&automation, "scheduler")
        .await
        .expect("queue run before dependency pause");

    let revoked_by = crate::automation_v2::governance::GovernanceActorRef::human(
        Some("operator-a".to_string()),
        "dependency_retry_test",
    );
    let grant = state
        .grant_automation_modify_access(
            &automation.automation_id,
            crate::automation_v2::governance::GovernanceActorRef::agent(
                Some("agent-grantee".to_string()),
                "dependency_retry_test",
            ),
            revoked_by.clone(),
            Some("temporary modify access".to_string()),
            &tenant,
        )
        .await
        .expect("grant modify access");

    let requester_app = verified_governance_app(
        state.clone(),
        "org-dependency-retry",
        "workspace-dependency-retry",
        "operator-a",
    );
    let reviewer_app = verified_governance_app(
        state.clone(),
        "org-dependency-retry",
        "workspace-dependency-retry",
        "reviewer-a",
    );
    let approval_create = Request::builder()
        .method("POST")
        .uri("/governance/approvals")
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "org-dependency-retry")
        .header("x-tandem-workspace-id", "workspace-dependency-retry")
        .header("x-tandem-actor-id", "operator-a")
        .body(Body::from(
            json!({
                "request_type": "capability_request",
                "target_resource": {
                    "type": "automation",
                    "id": automation.automation_id
                },
                "rationale": "approve exact dependency revoke retry",
                "context": {
                    "action": "revoke_modify_access",
                    "parameters": {
                        "grantID": grant.grant_id,
                        "reason": "connected capability revoked"
                    }
                }
            })
            .to_string(),
        ))
        .expect("create revoke approval request");
    let approval_response = requester_app
        .clone()
        .oneshot(approval_create)
        .await
        .expect("create revoke approval response");
    assert_eq!(approval_response.status(), StatusCode::OK);
    let approval_id = response_json(approval_response).await["approval"]["approval_id"]
        .as_str()
        .expect("revoke approval id")
        .to_string();
    let approve = Request::builder()
        .method("POST")
        .uri(format!("/governance/approvals/{approval_id}/approve"))
        .header("content-type", "application/json")
        .header("x-tandem-org-id", "org-dependency-retry")
        .header("x-tandem-workspace-id", "workspace-dependency-retry")
        .header("x-tandem-actor-id", "reviewer-a")
        .body(Body::from(
            json!({"notes": "independent revoke review"}).to_string(),
        ))
        .expect("approve revoke request");
    assert_eq!(
        reviewer_app
            .oneshot(approve)
            .await
            .expect("approve revoke response")
            .status(),
        StatusCode::OK
    );

    let durable_runs_path = state.automation_v2_runs_path.clone();
    let blocked_path = std::env::temp_dir().join(format!(
        "tandem-runs-write-blocked-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&blocked_path).expect("create blocking directory");
    state.automation_v2_runs_path = blocked_path.clone();

    let revoke_request = |org: &str, workspace: &str, actor: &str, reason: &str| {
        Request::builder()
            .method("DELETE")
            .uri(format!(
                "/automations/v2/{}/grants/{}",
                automation.automation_id, grant.grant_id
            ))
            .header("content-type", "application/json")
            .header("x-tandem-org-id", org)
            .header("x-tandem-workspace-id", workspace)
            .header("x-tandem-actor-id", actor)
            .body(Body::from(
                json!({
                    "approval_id": approval_id,
                    "reason": reason,
                })
                .to_string(),
            ))
            .expect("grant revoke request")
    };

    let failing_app = verified_governance_app(
        state.clone(),
        "org-dependency-retry",
        "workspace-dependency-retry",
        "operator-a",
    );
    let failed = failing_app
        .oneshot(revoke_request(
            "org-dependency-retry",
            "workspace-dependency-retry",
            "operator-a",
            "connected capability revoked",
        ))
        .await;
    let failed = failed.expect("failed dependency revoke response");
    assert_eq!(failed.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert_eq!(
        response_json(failed).await["code"].as_str(),
        Some("AUTOMATION_GOVERNANCE_DEPENDENCY_PAUSE_FAILED")
    );
    let paused = state
        .get_automation_v2_run(&queued.run_id)
        .await
        .expect("queued run remains recorded");
    assert_eq!(paused.status, crate::AutomationRunStatus::Paused);
    assert!(
        state
            .create_automation_v2_run(&automation, "scheduler")
            .await
            .is_err(),
        "failed run persistence must retain fail-closed admission"
    );
    let durable_pause = state
        .get_automation_governance(&automation.automation_id)
        .await
        .expect("governance after failed run persistence");
    assert!(durable_pause.creation_paused);
    assert!(durable_pause.paused_for_lifecycle);
    assert!(durable_pause.review_required);

    // Reload the durable governance snapshot as startup does. Admission must
    // remain closed even though the final paused-run snapshot failed.
    state
        .load_automation_governance()
        .await
        .expect("reload durable fail-closed governance snapshot");
    assert!(
        state
            .get_automation_governance(&automation.automation_id)
            .await
            .expect("reloaded governance record")
            .paused_for_lifecycle
    );
    let _ = std::fs::remove_dir_all(blocked_path);
    state.automation_v2_runs_path = durable_runs_path;

    let wrong_actor_app = verified_governance_app(
        state.clone(),
        "org-dependency-retry",
        "workspace-dependency-retry",
        "operator-b",
    );
    let wrong_actor = wrong_actor_app
        .oneshot(revoke_request(
            "org-dependency-retry",
            "workspace-dependency-retry",
            "operator-b",
            "connected capability revoked",
        ))
        .await
        .expect("wrong actor retry response");
    assert_eq!(wrong_actor.status(), StatusCode::FORBIDDEN);

    let wrong_tenant_app = verified_governance_app(
        state.clone(),
        "org-other",
        "workspace-other",
        "operator-a",
    );
    let wrong_tenant = wrong_tenant_app
        .oneshot(revoke_request(
            "org-other",
            "workspace-other",
            "operator-a",
            "connected capability revoked",
        ))
        .await
        .expect("wrong tenant retry response");
    assert_eq!(wrong_tenant.status(), StatusCode::NOT_FOUND);

    let retry_app = verified_governance_app(
        state.clone(),
        "org-dependency-retry",
        "workspace-dependency-retry",
        "operator-a",
    );
    let altered_payload = retry_app
        .clone()
        .oneshot(revoke_request(
            "org-dependency-retry",
            "workspace-dependency-retry",
            "operator-a",
            "different revoke reason",
        ))
        .await
        .expect("altered payload retry response");
    assert_eq!(altered_payload.status(), StatusCode::FORBIDDEN);

    let retried = retry_app
        .clone()
        .oneshot(revoke_request(
            "org-dependency-retry",
            "workspace-dependency-retry",
            "operator-a",
            "connected capability revoked",
        ))
        .await
        .expect("exact retry response");
    assert_eq!(retried.status(), StatusCode::OK);
    assert_eq!(
        response_json(retried).await["grant"]["grant_id"].as_str(),
        Some(grant.grant_id.as_str())
    );
    state
        .load_automation_v2_runs()
        .await
        .expect("reload run snapshot persisted by exact retry");
    assert_eq!(
        state
            .get_automation_v2_run(&queued.run_id)
            .await
            .expect("reloaded governance-paused run")
            .status,
        crate::AutomationRunStatus::Paused,
        "exact retry must replace the stale pre-revocation queued snapshot"
    );
    let consumed = state
        .get_governance_approval_request(&approval_id)
        .await
        .expect("consumed revoke approval");
    assert!(consumed.context.get("_mutation_reservation").is_none());
    assert!(consumed.context.get("_mutation_consumption").is_some());

    let replay = retry_app
        .oneshot(revoke_request(
            "org-dependency-retry",
            "workspace-dependency-retry",
            "operator-a",
            "connected capability revoked",
        ))
        .await
        .expect("consumed replay response");
    assert_eq!(replay.status(), StatusCode::FORBIDDEN);
    assert!(
        state
            .get_automation_governance(&automation.automation_id)
            .await
            .expect("retried governance record")
            .paused_for_lifecycle
    );
}
