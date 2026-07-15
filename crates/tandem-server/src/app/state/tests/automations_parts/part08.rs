// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

// Continuation split for transition-guard approval gate tests.

#[tokio::test]
async fn approval_gate_pause_binds_transition_guard_metadata() {
    let state = ready_test_state().await;
    let automation = email_approval_automation("auto-email-transition-guard-bind");
    let run = paused_email_run(&state, &automation).await;
    let gate = run
        .checkpoint
        .awaiting_gate
        .as_ref()
        .expect("pending gate");
    let expected = tandem_types::ApprovalWaitRef::for_gate(
        tandem_types::ApprovalSourceKind::AutomationV2,
        &run.run_id,
        &gate.node_id,
    );
    let metadata = gate.metadata.as_ref().expect("gate metadata");

    assert_eq!(
        metadata
            .get("approval_wait")
            .and_then(|wait| wait.get("approval_request_id"))
            .and_then(serde_json::Value::as_str),
        Some(expected.approval_request_id.as_str())
    );
    assert_eq!(
        metadata
            .get("transition_guard")
            .and_then(|guard| guard.get("transition_id"))
            .and_then(serde_json::Value::as_str),
        expected.transition_id.as_deref()
    );
}

#[tokio::test]
async fn approval_gate_transition_guard_denial_does_not_consume_gate() {
    let state = ready_test_state().await;
    let automation = email_approval_automation("auto-email-transition-guard-denial");
    let mut run = paused_email_run(&state, &automation).await;
    let gate = run
        .checkpoint
        .awaiting_gate
        .clone()
        .expect("pending gate");
    let expected = tandem_types::ApprovalWaitRef::for_gate(
        tandem_types::ApprovalSourceKind::AutomationV2,
        &run.run_id,
        &gate.node_id,
    );

    let denial = crate::app::state::apply_automation_gate_decision_with_transition_guard(
        &mut run,
        &automation,
        &gate,
        "approve",
        Some("wrong approval card".to_string()),
        Some(human_reviewer()),
        Some("automation_v2:other-run:approval_gate"),
        None,
    )
    .expect_err("cross-run approval request rejected");

    assert_eq!(denial.code, "AUTOMATION_V2_GATE_TRANSITION_GUARD_DENIED");
    assert_eq!(run.status, AutomationRunStatus::AwaitingApproval);
    assert!(run.checkpoint.awaiting_gate.is_some());
    assert_eq!(run.checkpoint.gate_history.len(), 1);
    assert_eq!(run.checkpoint.gate_history[0].decision, "guard_denied");
    assert!(run
        .checkpoint
        .lifecycle_history
        .iter()
        .any(|entry| entry.event == "approval_gate_transition_guard_denied"));

    let outcome = crate::app::state::apply_automation_gate_decision_with_transition_guard(
        &mut run,
        &automation,
        &gate,
        "approve",
        Some("correct approval card".to_string()),
        Some(human_reviewer()),
        Some(expected.approval_request_id.as_str()),
        expected.transition_id.as_deref(),
    )
    .expect("matching approval request accepted");

    assert!(matches!(
        outcome,
        crate::app::state::AutomationGateDecisionOutcome::Applied
    ));
    assert_eq!(run.status, AutomationRunStatus::Queued);
    assert_eq!(run.checkpoint.gate_history.len(), 2);
    assert_eq!(run.checkpoint.gate_history[1].decision, "approve");
}
