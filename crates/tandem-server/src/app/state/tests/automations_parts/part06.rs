fn approval_policy_test_automation(id: &str) -> AutomationV2Spec {
    AutomationV2Spec {
        automation_id: id.to_string(),
        name: "Approval Gate Policy Test".to_string(),
        description: None,
        status: AutomationV2Status::Active,
        schedule: AutomationV2Schedule {
            schedule_type: AutomationV2ScheduleType::Manual,
            cron_expression: None,
            interval_seconds: None,
            timezone: "UTC".to_string(),
            misfire_policy: RoutineMisfirePolicy::RunOnce,
        },
        knowledge: tandem_orchestrator::KnowledgeBinding::default(),
        agents: Vec::new(),
        flow: AutomationFlowSpec { nodes: Vec::new() },
        execution: AutomationExecutionPolicy {
            profile: None,
            max_parallel_agents: Some(1),
            max_total_runtime_ms: None,
            max_total_tool_calls: None,
            max_total_tokens: None,
            max_total_cost_usd: None,
        },
        output_targets: Vec::new(),
        created_at_ms: 1,
        updated_at_ms: 1,
        creator_id: "test".to_string(),
        workspace_root: Some(format!("/tmp/{id}-workspace")),
        metadata: None,
        next_fire_at_ms: None,
        last_fired_at_ms: None,
        scope_policy: None,
        watch_conditions: Vec::new(),
        handoff_config: None,
    }
}

async fn insert_awaiting_policy_gate_run(
    state: &crate::AppState,
    automation: &AutomationV2Spec,
    gate: AutomationPendingGate,
) -> String {
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("put automation");
    let mut run = state
        .create_automation_v2_run(automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();
    run.status = AutomationRunStatus::AwaitingApproval;
    run.detail = Some(format!("awaiting approval for gate `{}`", gate.node_id));
    run.checkpoint.pending_nodes = vec![gate.node_id.clone()];
    run.checkpoint.blocked_nodes = vec![gate.node_id.clone()];
    run.checkpoint.awaiting_gate = Some(gate);
    {
        let mut runs = state.automation_v2_runs.write().await;
        runs.insert(run_id.clone(), run);
    }
    run_id
}

#[tokio::test]
async fn approval_gate_transition_registers_and_completes_durable_wait() {
    let state = ready_test_state().await;
    let automation = approval_policy_test_automation("auto-gate-durable-wait");
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("put automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();
    let requested_at_ms = now_ms();
    let gate = AutomationPendingGate {
        node_id: "approval".to_string(),
        title: "Approval".to_string(),
        instructions: None,
        decisions: vec!["approve".to_string(), "cancel".to_string()],
        rework_targets: Vec::new(),
        requested_at_ms,
        upstream_node_ids: Vec::new(),
        metadata: None,
        expiry_policy: Some(AutomationGateExpiryPolicy {
            expires_after_ms: Some(60_000),
            on_expiry: Some(AutomationGateExpiryAction::Cancel),
            escalate_to: None,
            remind_every_ms: None,
        }),
    };
    state
        .update_automation_v2_run(&run_id, |row| {
            crate::app::state::automation::pause_automation_run_for_gate(
                row,
                gate.clone(),
                vec![gate.node_id.clone()],
            );
        })
        .await
        .expect("pause for approval");

    let paths = crate::stateful_runtime::StatefulRuntimeStoragePaths::from_runtime_events_path(
        &state.runtime_events_path,
    );
    let waits = crate::stateful_runtime::list_stateful_waits(
        &paths.waits_path,
        &run.tenant_context,
        crate::stateful_runtime::StatefulWaitQuery {
            run_id: Some(&run_id),
            wait_kind: Some(crate::stateful_runtime::StatefulWaitKind::Approval),
            status: None,
            limit: None,
        },
    );
    assert_eq!(waits.len(), 1);
    let wait = &waits[0];
    assert_eq!(
        wait.wait_id,
        format!("automation_v2:{run_id}:approval:wait")
    );
    assert_eq!(
        wait.status,
        crate::stateful_runtime::StatefulWaitStatus::Waiting
    );
    let timeout_policy = wait.timeout_policy.as_ref().expect("timeout policy");
    assert_eq!(
        timeout_policy.on_timeout,
        crate::stateful_runtime::StatefulWaitTimeoutAction::Cancel
    );
    assert_eq!(
        timeout_policy.timeout_at_ms,
        requested_at_ms.saturating_add(60_000)
    );

    let automation_for_decision = automation.clone();
    state
        .update_automation_v2_run(&run_id, |row| {
            let pending_gate = row.checkpoint.awaiting_gate.clone().expect("pending gate");
            let _ = crate::app::state::automation::apply_automation_gate_decision(
                row,
                &automation_for_decision,
                &pending_gate,
                "approve",
                None,
                Some(human_reviewer()),
            );
        })
        .await
        .expect("approve gate");

    let waits = crate::stateful_runtime::list_stateful_waits(
        &paths.waits_path,
        &run.tenant_context,
        crate::stateful_runtime::StatefulWaitQuery {
            run_id: Some(&run_id),
            wait_kind: Some(crate::stateful_runtime::StatefulWaitKind::Approval),
            status: None,
            limit: None,
        },
    );
    assert_eq!(waits.len(), 1);
    assert_eq!(
        waits[0].status,
        crate::stateful_runtime::StatefulWaitStatus::Woken
    );
    let expected_wake_key = format!("approval:{run_id}:approval:{requested_at_ms}:Woken");
    assert_eq!(
        waits[0].wake_idempotency_key.as_deref(),
        Some(expected_wake_key.as_str())
    );
    assert!(waits[0].event_seq.is_some());
}

#[tokio::test]
async fn approval_gate_decision_does_not_log_completion_while_wait_claimed() {
    let state = ready_test_state().await;
    let automation = approval_policy_test_automation("auto-gate-claimed-durable-wait");
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("put automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();
    let requested_at_ms = now_ms().saturating_sub(10_000);
    let gate = AutomationPendingGate {
        node_id: "approval".to_string(),
        title: "Approval".to_string(),
        instructions: None,
        decisions: vec!["approve".to_string(), "cancel".to_string()],
        rework_targets: Vec::new(),
        requested_at_ms,
        upstream_node_ids: Vec::new(),
        metadata: None,
        expiry_policy: Some(AutomationGateExpiryPolicy {
            expires_after_ms: Some(1),
            on_expiry: Some(AutomationGateExpiryAction::Cancel),
            escalate_to: None,
            remind_every_ms: None,
        }),
    };
    state
        .update_automation_v2_run(&run_id, |row| {
            crate::app::state::automation::pause_automation_run_for_gate(
                row,
                gate.clone(),
                vec![gate.node_id.clone()],
            );
        })
        .await
        .expect("pause for approval");

    let paths = crate::stateful_runtime::StatefulRuntimeStoragePaths::from_runtime_events_path(
        &state.runtime_events_path,
    );
    let wait_id = format!("automation_v2:{run_id}:approval:wait");
    let claimed = crate::stateful_runtime::claim_due_stateful_wait(
        &paths.waits_path,
        &run.tenant_context,
        &run_id,
        &wait_id,
        "scheduler-a",
        now_ms(),
        60_000,
    )
    .await
    .expect("claim due approval wait")
    .expect("claimed approval wait");
    assert_eq!(
        claimed.status,
        crate::stateful_runtime::StatefulWaitStatus::Claimed
    );

    let automation_for_decision = automation.clone();
    state
        .update_automation_v2_run(&run_id, |row| {
            let pending_gate = row.checkpoint.awaiting_gate.clone().expect("pending gate");
            let _ = crate::app::state::automation::apply_automation_gate_decision(
                row,
                &automation_for_decision,
                &pending_gate,
                "approve",
                None,
                Some(human_reviewer()),
            );
        })
        .await
        .expect("approve gate");

    let waits = crate::stateful_runtime::list_stateful_waits(
        &paths.waits_path,
        &run.tenant_context,
        crate::stateful_runtime::StatefulWaitQuery {
            run_id: Some(&run_id),
            wait_kind: Some(crate::stateful_runtime::StatefulWaitKind::Approval),
            status: None,
            limit: None,
        },
    );
    assert_eq!(waits.len(), 1);
    assert_eq!(
        waits[0].status,
        crate::stateful_runtime::StatefulWaitStatus::Claimed
    );
    assert_eq!(waits[0].event_seq, None);

    let events = crate::stateful_runtime::query_stateful_run_events(
        &paths.run_events_path,
        &run.tenant_context,
        crate::stateful_runtime::StatefulRunEventQuery {
            run_id: &run_id,
            after_seq: None,
            before_seq: None,
            limit: None,
            tail: false,
        },
    );
    assert!(!events.iter().any(|event| {
        matches!(
            event.event_type.as_str(),
            "stateful_runtime.wait.approval_woken"
                | "stateful_runtime.wait.approval_cancelled"
        )
    }));
}

#[tokio::test]
async fn approval_gate_decision_completes_escalated_durable_wait() {
    let state = ready_test_state().await;
    let automation = approval_policy_test_automation("auto-gate-escalated-durable-wait");
    state
        .put_automation_v2(automation.clone())
        .await
        .expect("put automation");
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let run_id = run.run_id.clone();
    let requested_at_ms = now_ms();
    let gate = AutomationPendingGate {
        node_id: "approval".to_string(),
        title: "Approval".to_string(),
        instructions: None,
        decisions: vec!["approve".to_string(), "cancel".to_string()],
        rework_targets: Vec::new(),
        requested_at_ms,
        upstream_node_ids: Vec::new(),
        metadata: None,
        expiry_policy: Some(AutomationGateExpiryPolicy {
            expires_after_ms: Some(1),
            on_expiry: Some(AutomationGateExpiryAction::Escalate),
            escalate_to: Some("risk-lead".to_string()),
            remind_every_ms: None,
        }),
    };
    state
        .update_automation_v2_run(&run_id, |row| {
            crate::app::state::automation::pause_automation_run_for_gate(
                row,
                gate.clone(),
                vec![gate.node_id.clone()],
            );
        })
        .await
        .expect("pause for approval");

    let paths = crate::stateful_runtime::StatefulRuntimeStoragePaths::from_runtime_events_path(
        &state.runtime_events_path,
    );
    let wait_id = format!("automation_v2:{run_id}:approval:wait");
    crate::stateful_runtime::mark_stateful_wait_timeout_result(
        &paths.waits_path,
        &run.tenant_context,
        &run_id,
        &wait_id,
        "timeout:escalate:test",
        7,
        crate::stateful_runtime::StatefulWaitStatus::Escalated,
        now_ms(),
    )
    .await
    .expect("mark wait escalated")
    .expect("escalated wait");

    let automation_for_decision = automation.clone();
    state
        .update_automation_v2_run(&run_id, |row| {
            let pending_gate = row.checkpoint.awaiting_gate.clone().expect("pending gate");
            let _ = crate::app::state::automation::apply_automation_gate_decision(
                row,
                &automation_for_decision,
                &pending_gate,
                "approve",
                None,
                Some(human_reviewer()),
            );
        })
        .await
        .expect("approve escalated gate");

    let waits = crate::stateful_runtime::list_stateful_waits(
        &paths.waits_path,
        &run.tenant_context,
        crate::stateful_runtime::StatefulWaitQuery {
            run_id: Some(&run_id),
            wait_kind: Some(crate::stateful_runtime::StatefulWaitKind::Approval),
            status: None,
            limit: None,
        },
    );
    assert_eq!(waits.len(), 1);
    assert_eq!(
        waits[0].status,
        crate::stateful_runtime::StatefulWaitStatus::Woken
    );
    let expected_wake_key = format!("approval:{run_id}:approval:{requested_at_ms}:Woken");
    assert_eq!(
        waits[0].wake_idempotency_key.as_deref(),
        Some(expected_wake_key.as_str())
    );
    assert_ne!(waits[0].event_seq, Some(7));
}

#[tokio::test]
async fn approval_gate_expiry_policy_auto_cancels_with_expired_record() {
    let state = ready_test_state().await;
    let automation = approval_policy_test_automation("auto-gate-expiry-cancel");
    let gate = AutomationPendingGate {
        node_id: "approval".to_string(),
        title: "Approval".to_string(),
        instructions: None,
        decisions: vec!["approve".to_string(), "cancel".to_string()],
        rework_targets: Vec::new(),
        requested_at_ms: now_ms().saturating_sub(10_000),
        upstream_node_ids: Vec::new(),
        metadata: None,
        expiry_policy: Some(AutomationGateExpiryPolicy {
            expires_after_ms: Some(1),
            on_expiry: Some(AutomationGateExpiryAction::Cancel),
            escalate_to: None,
            remind_every_ms: None,
        }),
    };
    let run_id = insert_awaiting_policy_gate_run(&state, &automation, gate).await;

    assert_eq!(state.process_awaiting_approval_gate_policies().await, 1);
    assert_eq!(state.process_awaiting_approval_gate_policies().await, 0);

    let updated = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("updated run");
    assert_eq!(updated.status, AutomationRunStatus::Cancelled);
    assert!(updated.checkpoint.awaiting_gate.is_none());
    assert_eq!(updated.checkpoint.gate_history.len(), 1);
    assert_eq!(updated.checkpoint.gate_history[0].decision, "expired");
    assert!(updated
        .checkpoint
        .lifecycle_history
        .iter()
        .any(|entry| entry.event == "approval_gate_expired"));
}

#[tokio::test]
async fn approval_gate_timeout_survives_reload_and_rejects_late_decision() {
    let state = ready_test_state().await;
    let automation = approval_policy_test_automation("auto-gate-timeout-reload");
    let gate = AutomationPendingGate {
        node_id: "approval".to_string(),
        title: "Approval".to_string(),
        instructions: None,
        decisions: vec!["approve".to_string(), "cancel".to_string()],
        rework_targets: Vec::new(),
        requested_at_ms: now_ms().saturating_sub(10_000),
        upstream_node_ids: Vec::new(),
        metadata: None,
        expiry_policy: Some(AutomationGateExpiryPolicy {
            expires_after_ms: Some(1),
            on_expiry: Some(AutomationGateExpiryAction::Cancel),
            escalate_to: None,
            remind_every_ms: None,
        }),
    };
    let run_id = insert_awaiting_policy_gate_run(&state, &automation, gate.clone()).await;
    state
        .persist_automation_v2_runs()
        .await
        .expect("persist awaiting approval run");

    let mut reloaded = ready_test_state().await;
    reloaded.automation_v2_runs_path = state.automation_v2_runs_path.clone();
    reloaded
        .load_automation_v2_runs()
        .await
        .expect("reload awaiting approval run");

    let before = reloaded
        .get_automation_v2_run(&run_id)
        .await
        .expect("reloaded run");
    assert_eq!(before.status, AutomationRunStatus::AwaitingApproval);
    assert_eq!(reloaded.process_awaiting_approval_gate_policies().await, 1);
    assert_eq!(reloaded.process_awaiting_approval_gate_policies().await, 0);

    let expired = reloaded
        .get_automation_v2_run(&run_id)
        .await
        .expect("expired run");
    assert_eq!(expired.status, AutomationRunStatus::Cancelled);
    assert!(expired.checkpoint.awaiting_gate.is_none());
    let winner = expired
        .checkpoint
        .gate_history
        .last()
        .expect("expired gate record");
    assert_eq!(winner.decision, "expired");
    let expected_request_id = format!("automation_v2:{run_id}:approval");
    assert_eq!(
        winner
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("approval_wait"))
            .and_then(|wait| wait.get("approval_request_id"))
            .and_then(Value::as_str),
        Some(expected_request_id.as_str())
    );

    let mut late = expired.clone();
    let expected_transition_id = format!("{expected_request_id}:decision");
    let late_outcome = crate::app::state::apply_automation_gate_decision_with_transition_guard(
        &mut late,
        &automation,
        &gate,
        "approve",
        Some("too late".to_string()),
        Some(human_reviewer()),
        Some(expected_request_id.as_str()),
        Some(expected_transition_id.as_str()),
    )
    .expect("late decision resolves to settled winner");
    match late_outcome {
        crate::app::state::AutomationGateDecisionOutcome::AlreadyDecided(Some(record)) => {
            assert_eq!(record.decision, "expired");
        }
        other => panic!("late decision must not change expired gate: {other:?}"),
    }
    assert_eq!(late.status, AutomationRunStatus::Cancelled);
    assert_eq!(
        late.checkpoint.gate_history.len(),
        expired.checkpoint.gate_history.len()
    );
}

#[tokio::test]
async fn approval_gate_reminder_policy_updates_notification_key() {
    let state = ready_test_state().await;
    let automation = approval_policy_test_automation("auto-gate-reminder");
    let requested_at_ms = now_ms().saturating_sub(120_000);
    let gate = AutomationPendingGate {
        node_id: "approval".to_string(),
        title: "Approval".to_string(),
        instructions: None,
        decisions: vec!["approve".to_string(), "cancel".to_string()],
        rework_targets: Vec::new(),
        requested_at_ms,
        upstream_node_ids: Vec::new(),
        metadata: None,
        expiry_policy: Some(AutomationGateExpiryPolicy {
            expires_after_ms: Some(3_600_000),
            on_expiry: Some(AutomationGateExpiryAction::Cancel),
            escalate_to: None,
            remind_every_ms: Some(60_000),
        }),
    };
    let run_id = insert_awaiting_policy_gate_run(&state, &automation, gate).await;

    assert_eq!(state.process_awaiting_approval_gate_policies().await, 1);
    assert_eq!(state.process_awaiting_approval_gate_policies().await, 0);

    let updated = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("updated run");
    assert_eq!(updated.status, AutomationRunStatus::AwaitingApproval);
    let state_metadata = updated
        .checkpoint
        .awaiting_gate
        .as_ref()
        .and_then(|gate| gate.metadata.as_ref())
        .and_then(|metadata| metadata.get("gate_policy_state"))
        .expect("gate policy state");
    assert_eq!(
        state_metadata.get("reminder_count").and_then(Value::as_u64),
        Some(1)
    );
    let notification_key = state_metadata
        .get("notification_key")
        .and_then(Value::as_str)
        .expect("notification key");
    assert!(notification_key.contains(":reminder:1"));

    let approvals = crate::http::approvals::list_pending_approvals(
        &state,
        &tandem_types::ApprovalListFilter::default(),
    )
    .await;
    let approval = approvals
        .iter()
        .find(|approval| approval.run_id == run_id)
        .expect("pending approval");
    assert_eq!(
        approval.expires_at_ms,
        Some(requested_at_ms.saturating_add(3_600_000))
    );
    assert_eq!(
        approval
            .surface_payload
            .as_ref()
            .and_then(|payload| payload.get("notification_key"))
            .and_then(Value::as_str),
        Some(notification_key)
    );
}

#[tokio::test]
async fn approval_gate_escalation_policy_updates_notification_key() {
    let state = ready_test_state().await;
    let automation = approval_policy_test_automation("auto-gate-escalation");
    let requested_at_ms = now_ms().saturating_sub(120_000);
    let gate = AutomationPendingGate {
        node_id: "approval".to_string(),
        title: "Approval".to_string(),
        instructions: None,
        decisions: vec!["approve".to_string(), "cancel".to_string()],
        rework_targets: Vec::new(),
        requested_at_ms,
        upstream_node_ids: Vec::new(),
        metadata: None,
        expiry_policy: Some(AutomationGateExpiryPolicy {
            expires_after_ms: Some(1),
            on_expiry: Some(AutomationGateExpiryAction::Escalate),
            escalate_to: Some("risk-lead".to_string()),
            remind_every_ms: None,
        }),
    };
    let run_id = insert_awaiting_policy_gate_run(&state, &automation, gate).await;

    assert_eq!(state.process_awaiting_approval_gate_policies().await, 1);
    assert_eq!(state.process_awaiting_approval_gate_policies().await, 0);

    let updated = state
        .get_automation_v2_run(&run_id)
        .await
        .expect("updated run");
    assert_eq!(updated.status, AutomationRunStatus::AwaitingApproval);
    assert!(updated
        .checkpoint
        .lifecycle_history
        .iter()
        .any(|entry| entry.event == "approval_gate_escalated"));
    let state_metadata = updated
        .checkpoint
        .awaiting_gate
        .as_ref()
        .and_then(|gate| gate.metadata.as_ref())
        .and_then(|metadata| metadata.get("gate_policy_state"))
        .expect("gate policy state");
    assert_eq!(
        state_metadata.get("escalated_to").and_then(Value::as_str),
        Some("risk-lead")
    );
    assert_eq!(
        state_metadata.get("reminder_count").and_then(Value::as_u64),
        Some(1)
    );
    let notification_key = state_metadata
        .get("notification_key")
        .and_then(Value::as_str)
        .expect("notification key");
    assert!(notification_key.contains(":escalated:1"));

    let approvals = crate::http::approvals::list_pending_approvals(
        &state,
        &tandem_types::ApprovalListFilter::default(),
    )
    .await;
    let approval = approvals
        .iter()
        .find(|approval| approval.run_id == run_id)
        .expect("pending approval");
    assert_eq!(
        approval
            .surface_payload
            .as_ref()
            .and_then(|payload| payload.get("notification_key"))
            .and_then(Value::as_str),
        Some(notification_key)
    );
}

#[test]
fn expired_cancel_policy_rejects_late_human_decision() {
    let mut gate = AutomationPendingGate {
        node_id: "approval".to_string(),
        title: "Approval".to_string(),
        instructions: None,
        decisions: vec!["approve".to_string(), "cancel".to_string()],
        rework_targets: Vec::new(),
        requested_at_ms: 10,
        upstream_node_ids: Vec::new(),
        metadata: None,
        expiry_policy: Some(AutomationGateExpiryPolicy {
            expires_after_ms: Some(5),
            on_expiry: Some(AutomationGateExpiryAction::Cancel),
            escalate_to: None,
            remind_every_ms: None,
        }),
    };

    assert!(crate::app::state::automation_gate_rejects_late_human_decision(&gate, 15));
    assert!(!crate::app::state::automation_gate_rejects_late_human_decision(&gate, 14));

    gate.expiry_policy = Some(AutomationGateExpiryPolicy {
        expires_after_ms: Some(5),
        on_expiry: Some(AutomationGateExpiryAction::Escalate),
        escalate_to: Some("risk-lead".to_string()),
        remind_every_ms: None,
    });
    assert!(!crate::app::state::automation_gate_rejects_late_human_decision(&gate, 15));

    gate.expiry_policy = Some(AutomationGateExpiryPolicy {
        expires_after_ms: Some(5),
        on_expiry: Some(AutomationGateExpiryAction::Remind),
        escalate_to: None,
        remind_every_ms: Some(60_000),
    });
    assert!(!crate::app::state::automation_gate_rejects_late_human_decision(&gate, 15));
}
