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
async fn dependency_revocation_persistence_failure_keeps_admission_closed() {
    let mut state = test_state().await;
    let tenant = TenantContext::explicit(
        "org-dependency-persist-failure",
        "workspace-dependency-persist-failure",
        Some("owner".to_string()),
    );
    let automation = super::global::create_test_automation_v2_for_tenant(
        &state,
        "auto-governance-dependency-persist-failure",
        &tenant,
    )
    .await;
    let queued = state
        .create_automation_v2_run(&automation, "scheduler")
        .await
        .expect("queue run before dependency pause");

    let revoked_by =
        crate::automation_v2::governance::GovernanceActorRef::human(
            Some("owner".to_string()),
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
    let revoked = state
        .revoke_automation_modify_access(
            &automation.automation_id,
            &grant.grant_id,
            revoked_by.clone(),
            Some("connected capability revoked".to_string()),
            &tenant,
        )
        .await
        .expect("persist grant revocation")
        .expect("active grant");
    assert!(revoked.revoked_at_ms.is_some());

    let durable_governance_path = state.automation_governance_path.clone();

    let blocked_path = std::env::temp_dir().join(format!(
        "tandem-governance-write-blocked-{}",
        uuid::Uuid::new_v4()
    ));
    std::fs::create_dir_all(&blocked_path).expect("create blocking directory");
    state.automation_governance_path = blocked_path.clone();

    let result = state
        .pause_automation_for_dependency_revocation(
            &automation.automation_id,
            "connected capability revoked".to_string(),
            json!({"capability": "mcp:revoked"}),
            &tenant,
        )
        .await;
    assert!(result.is_err(), "the governance snapshot write must fail");
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
        "failed persistence must not restore admissible in-memory governance"
    );

    // Simulate a restart from the last durable snapshot. It contains the
    // revoked grant but not the failed lifecycle-pause write. An exact DELETE
    // retry must recover the revoked record and finish the pause.
    let _ = std::fs::remove_dir_all(blocked_path);
    state.automation_governance_path = durable_governance_path;
    state
        .load_automation_governance()
        .await
        .expect("reload durable governance snapshot");
    let reloaded = state
        .get_automation_governance(&automation.automation_id)
        .await
        .expect("reloaded governance record");
    assert!(!reloaded.paused_for_lifecycle);
    let retried = state
        .revoke_automation_modify_access(
            &automation.automation_id,
            &grant.grant_id,
            revoked_by,
            Some("connected capability revoked".to_string()),
            &tenant,
        )
        .await
        .expect("retry revoked grant lookup")
        .expect("revoked grant remains addressable for exact retry");
    assert_eq!(retried.grant_id, grant.grant_id);
    assert!(retried.revoked_at_ms.is_some());
    state
        .pause_automation_for_dependency_revocation(
            &automation.automation_id,
            "connected capability revoked".to_string(),
            json!({"capability": "mcp:revoked", "retry": true}),
            &tenant,
        )
        .await
        .expect("retry dependency pause");
    assert!(
        state
            .get_automation_governance(&automation.automation_id)
            .await
            .expect("retried governance record")
            .paused_for_lifecycle
    );
}
