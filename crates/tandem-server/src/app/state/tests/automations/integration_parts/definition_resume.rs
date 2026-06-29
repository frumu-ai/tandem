#[tokio::test]
async fn restart_recovery_projects_resume_boundary_with_definition_metadata() {
    let workspace_root = restart_test_workspace("tandem-restart-definition-projection");
    let state = ready_test_state().await;
    let automation = consequential_restart_automation(
        "automation-restart-definition-projection",
        &workspace_root,
    );
    let run = create_persisted_restart_run(&state, &automation).await;
    let snapshot = run.automation_snapshot.as_ref().expect("run snapshot");
    let expected_hash = crate::stateful_runtime::automation_definition_snapshot_hash(snapshot);
    let expected_version =
        crate::stateful_runtime::automation_definition_version(snapshot, &expected_hash);

    assert_eq!(
        run.workflow_definition_version.as_deref(),
        Some(expected_version.as_str())
    );
    assert_eq!(
        run.workflow_definition_snapshot_hash.as_deref(),
        Some(expected_hash.as_str())
    );

    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = AutomationRunStatus::Running;
            row.started_at_ms = Some(crate::now_ms());
            row.active_session_ids = vec!["session-in-flight-before-restart".to_string()];
            row.latest_session_id = Some("session-in-flight-before-restart".to_string());
        })
        .await
        .expect("mark running restart run");

    let (reloaded, recovered) = reload_automation_state_after_restart(&state).await;
    assert_eq!(recovered, 1);
    let recovered_run = reloaded
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("recovered running run");
    assert_eq!(recovered_run.status, AutomationRunStatus::Queued);

    let paths = crate::stateful_runtime::StatefulRuntimeStoragePaths::from_runtime_events_path(
        &reloaded.runtime_events_path,
    );
    let events = crate::stateful_runtime::query_stateful_run_events(
        &paths.run_events_path,
        &recovered_run.tenant_context,
        crate::stateful_runtime::StatefulRunEventQuery {
            run_id: &run.run_id,
            after_seq: None,
            before_seq: None,
            limit: None,
            tail: false,
        },
    );
    let resume_event = events
        .iter()
        .find(|event| {
            event.event_type
                == "stateful_runtime.automation_v2.run_queued_for_resume_after_restart"
        })
        .expect("resume lifecycle projected to stateful event log");
    let snapshots = crate::stateful_runtime::list_stateful_run_snapshots(
        &paths.snapshots_root,
        &recovered_run.tenant_context,
        &run.run_id,
        None,
    );
    let resume_snapshot = snapshots
        .iter()
        .find(|snapshot| snapshot.seq == resume_event.seq)
        .expect("resume lifecycle projected to stateful snapshot");

    assert_eq!(
        resume_snapshot.workflow_definition_version,
        recovered_run.workflow_definition_version
    );
    assert_eq!(
        resume_snapshot.workflow_definition_snapshot_hash,
        recovered_run.workflow_definition_snapshot_hash
    );
    let _ = std::fs::remove_dir_all(&workspace_root);
}

#[tokio::test]
async fn restart_recovery_fails_definition_snapshot_hash_mismatch() {
    let workspace_root = restart_test_workspace("tandem-restart-definition-mismatch");
    let state = ready_test_state().await;
    let automation = consequential_restart_automation(
        "automation-restart-definition-mismatch",
        &workspace_root,
    );
    let run = create_persisted_restart_run(&state, &automation).await;
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = AutomationRunStatus::Running;
            row.started_at_ms = Some(crate::now_ms());
            row.active_session_ids = vec!["session-in-flight-before-restart".to_string()];
            row.latest_session_id = Some("session-in-flight-before-restart".to_string());
            row.workflow_definition_snapshot_hash = Some("sha256:not-the-run-snapshot".to_string());
            row.checkpoint
                .node_attempts
                .insert("send_customer_update".to_string(), 1);
            row.checkpoint.lifecycle_history.push(
                crate::automation_v2::types::AutomationLifecycleRecord {
                    event: "node_started".to_string(),
                    recorded_at_ms: crate::now_ms(),
                    reason: Some("node `send_customer_update` started".to_string()),
                    stop_kind: None,
                    metadata: Some(json!({
                        "node_id": "send_customer_update",
                        "attempt": 1,
                    })),
                },
            );
        })
        .await
        .expect("mark mismatched running restart run");

    let (reloaded, recovered) = reload_automation_state_after_restart(&state).await;
    assert_eq!(recovered, 1);
    assert!(reloaded
        .claim_specific_automation_v2_run(&run.run_id)
        .await
        .is_none());
    let recovered_run = reloaded
        .get_automation_v2_run(&run.run_id)
        .await
        .expect("mismatched recovered run");
    let golden = restart_resume_golden(&recovered_run);

    assert_eq!(golden.status, AutomationRunStatus::Failed);
    assert_eq!(golden.stop_kind, Some(AutomationStopKind::ServerRestart));
    assert_eq!(
        golden.detail.as_deref(),
        Some("automation run interrupted by server restart; definition snapshot hash mismatch")
    );
    assert!(golden.node_outputs.is_empty());
    let _ = std::fs::remove_dir_all(&workspace_root);
}
