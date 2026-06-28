#[tokio::test]
async fn run_claim_records_durable_execution_claim() {
    let workspace_root = restart_test_workspace("tandem-run-claim-record");
    let state = ready_test_state().await;
    let automation = empty_restart_automation("automation-run-claim-record", &workspace_root);
    let run = create_persisted_restart_run(&state, &automation).await;

    let claimed = state
        .claim_specific_automation_v2_run(&run.run_id)
        .await
        .expect("claim run");
    let claim = claimed
        .execution_claim
        .as_ref()
        .expect("durable execution claim");
    assert_eq!(claimed.status, AutomationRunStatus::Running);
    assert_eq!(claim.lease_epoch, 1);
    assert!(claim.claim_id.starts_with("run-claim-"));
    assert!(claim.claimant_id.starts_with("tandem-server:automation-v2-executor:"));
    assert!(claim.lease_expires_at_ms > claim.claimed_at_ms);
    assert!(claimed
        .checkpoint
        .lifecycle_history
        .iter()
        .any(|event| event.event == "run_execution_claimed"));

    let persisted = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("persisted run");
    assert_eq!(persisted.execution_claim, claimed.execution_claim);
    assert_eq!(persisted.execution_claim_epoch, 1);
    assert!(state
        .claim_specific_automation_v2_run(&run.run_id)
        .await
        .is_none());

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn run_claim_allows_single_claimant() {
    let workspace_root = restart_test_workspace("tandem-run-claim-single");
    let state = ready_test_state().await;
    let automation = empty_restart_automation("automation-run-claim-single", &workspace_root);
    let run = create_persisted_restart_run(&state, &automation).await;
    let run_id_a = run.run_id.clone();
    let run_id_b = run.run_id.clone();
    let state_a = state.clone();
    let state_b = state.clone();

    let (first, second) = tokio::join!(
        async move { state_a.claim_specific_automation_v2_run(&run_id_a).await },
        async move { state_b.claim_specific_automation_v2_run(&run_id_b).await },
    );
    let claimed = [first, second].into_iter().flatten().collect::<Vec<_>>();
    assert_eq!(claimed.len(), 1);
    assert_eq!(
        state
            .get_automation_v2_run(&run.run_id)
            .await
            .expect("persisted run")
            .execution_claim_epoch,
        1
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn abandoned_run_claim_requeues_for_resume() {
    let workspace_root = restart_test_workspace("tandem-run-claim-requeue");
    let state = ready_test_state().await;
    let automation = empty_restart_automation("automation-run-claim-requeue", &workspace_root);
    let run = create_persisted_restart_run(&state, &automation).await;

    let claimed = state
        .claim_specific_automation_v2_run(&run.run_id)
        .await
        .expect("claim run");
    assert!(claimed.execution_claim.is_some());
    state
        .update_automation_v2_run(&run.run_id, |row| {
            let claim = row.execution_claim.as_mut().expect("execution claim");
            claim.lease_expires_at_ms = crate::now_ms().saturating_sub(1);
        })
        .await
        .expect("expire claim");

    assert_eq!(state.reclaim_abandoned_automation_v2_run_leases().await, 1);
    let requeued = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("requeued run");
    assert_eq!(requeued.status, AutomationRunStatus::Queued);
    assert!(requeued.execution_claim.is_none());
    assert_eq!(requeued.execution_claim_epoch, 1);
    assert_eq!(
        requeued.resume_reason.as_deref(),
        Some("abandoned_execution_claim")
    );
    assert!(requeued
        .checkpoint
        .lifecycle_history
        .iter()
        .any(|event| event.event == "run_execution_claim_expired_requeued"));

    let reclaimed = state
        .claim_specific_automation_v2_run(&run.run_id)
        .await
        .expect("reclaim requeued run");
    assert_eq!(reclaimed.execution_claim_epoch, 2);
    assert_eq!(
        reclaimed
            .execution_claim
            .as_ref()
            .expect("second claim")
            .lease_epoch,
        2
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn abandoned_run_claim_with_active_handles_is_not_requeued() {
    let workspace_root = restart_test_workspace("tandem-run-claim-active");
    let state = ready_test_state().await;
    let automation = empty_restart_automation("automation-run-claim-active", &workspace_root);
    let run = create_persisted_restart_run(&state, &automation).await;

    state
        .claim_specific_automation_v2_run(&run.run_id)
        .await
        .expect("claim run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            let claim = row.execution_claim.as_mut().expect("execution claim");
            claim.lease_expires_at_ms = crate::now_ms().saturating_sub(1);
            row.active_session_ids = vec!["session-still-running".to_string()];
            row.latest_session_id = Some("session-still-running".to_string());
        })
        .await
        .expect("expire active claim");

    assert_eq!(state.reclaim_abandoned_automation_v2_run_leases().await, 0);
    let still_running = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("running run");
    assert_eq!(still_running.status, AutomationRunStatus::Running);
    assert!(still_running.execution_claim.is_some());

    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn stale_reaper_defers_to_unexpired_launch_claim_and_reclaims_expired_one() {
    let workspace_root = restart_test_workspace("tandem-run-claim-stale-reaper");
    let state = ready_test_state().await;
    let automation = empty_restart_automation("automation-run-claim-stale-reaper", &workspace_root);
    let run = create_persisted_restart_run(&state, &automation).await;

    state
        .claim_specific_automation_v2_run(&run.run_id)
        .await
        .expect("claim run");
    assert_eq!(state.reap_stale_running_automation_runs(0).await, 0);
    let still_running = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("running run");
    assert_eq!(still_running.status, AutomationRunStatus::Running);

    state
        .update_automation_v2_run(&run.run_id, |row| {
            let claim = row.execution_claim.as_mut().expect("execution claim");
            claim.lease_expires_at_ms = crate::now_ms().saturating_sub(1);
        })
        .await
        .expect("expire claim");
    assert_eq!(state.reap_stale_running_automation_runs(0).await, 0);
    let requeued = state
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("requeued run");
    assert_eq!(requeued.status, AutomationRunStatus::Queued);
    assert_eq!(
        requeued.resume_reason.as_deref(),
        Some("abandoned_execution_claim")
    );

    let _ = std::fs::remove_dir_all(&workspace_root);
}
