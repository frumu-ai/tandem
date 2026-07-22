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
    let _ = std::fs::remove_dir_all(blocked_path);
}
