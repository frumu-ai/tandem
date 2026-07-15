// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use tandem_automation::{
    AutomationWaitSpec, OrchestrationValueBinding, WaitTimeoutAction, WaitTimeoutPolicy,
};

fn timeout_policy() -> WaitTimeoutPolicy {
    WaitTimeoutPolicy {
        expires_after_ms: 60_000,
        on_timeout: WaitTimeoutAction::Cancel,
        escalate_to: None,
        remind_every_ms: None,
    }
}

#[test]
fn approval_wait_projects_to_the_existing_governed_gate_path() {
    let node = AutomationNodeBuilder::new("approve-release")
        .wait(AutomationWaitSpec::Approval {
            decisions: vec!["approve".to_string(), "deny".to_string()],
            expires_after_ms: None,
            timeout: Some(WaitTimeoutPolicy {
                expires_after_ms: 60_000,
                on_timeout: WaitTimeoutAction::Resume,
                escalate_to: None,
                remind_every_ms: None,
            }),
        })
        .build();

    assert!(crate::app::state::is_automation_approval_node(&node));
    let gate = crate::app::state::build_automation_pending_gate(&node).expect("pending gate");
    assert_eq!(gate.decisions, vec!["approve", "deny"]);
    assert_eq!(
        gate.expiry_policy.unwrap().on_expiry,
        Some(crate::AutomationGateExpiryAction::Resume)
    );
}

#[tokio::test]
async fn approval_resume_scheduler_outcome_settles_the_gate_before_requeue() {
    let state = ready_test_state().await;
    let node = AutomationNodeBuilder::new("approve-release")
        .wait(AutomationWaitSpec::Approval {
            decisions: vec!["approve".to_string(), "deny".to_string()],
            expires_after_ms: None,
            timeout: Some(WaitTimeoutPolicy {
                expires_after_ms: 1,
                on_timeout: WaitTimeoutAction::Resume,
                escalate_to: None,
                remind_every_ms: None,
            }),
        })
        .build();
    let automation = AutomationSpecBuilder::new("auto-approval-resume")
        .nodes(vec![node.clone()])
        .build();
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let gate = crate::app::state::build_automation_pending_gate(&node).expect("pending gate");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            crate::app::state::automation::pause_automation_run_for_gate(
                row,
                gate.clone(),
                Vec::new(),
            );
        })
        .await
        .expect("pause for approval");

    let paths = crate::stateful_runtime::StatefulRuntimeStoragePaths::from_runtime_events_path(
        &state.runtime_events_path,
    );
    let tick = crate::stateful_runtime::process_due_stateful_waits(
        &paths,
        gate.requested_at_ms.saturating_add(2),
        Default::default(),
    )
    .await;
    assert_eq!(tick.completed, 1);
    assert_eq!(
        tick.outcomes[0].event_type,
        "stateful_runtime.wait.timeout_resumed"
    );
    state
        .apply_stateful_wait_scheduler_outcome(&tick.outcomes[0])
        .await
        .expect("settle approval resume");

    let resumed = state.get_automation_v2_run(&run.run_id).await.unwrap();
    assert_eq!(resumed.status, AutomationRunStatus::Queued);
    assert!(resumed.checkpoint.awaiting_gate.is_none());
    assert!(resumed.checkpoint.completed_nodes.contains(&node.node_id));
    assert_eq!(
        resumed
            .checkpoint
            .gate_history
            .last()
            .map(|row| row.decision.as_str()),
        Some("timeout_resume")
    );
}

#[tokio::test]
async fn webhook_wait_registers_a_public_correlation_constraint() {
    let state = ready_test_state().await;
    let node = AutomationNodeBuilder::new("wait-for-callback")
        .wait(AutomationWaitSpec::Webhook {
            trigger_id: "trigger-callback".to_string(),
            provider: Some("custom".to_string()),
            provider_event_kind: Some("job.completed".to_string()),
            correlation: tandem_automation::WebhookCorrelationBinding {
                field: tandem_automation::WebhookCorrelationField::ProviderEventId,
                value: OrchestrationValueBinding::Literal {
                    value: json!("event-42"),
                },
            },
            timeout: timeout_policy(),
        })
        .build();
    let automation = AutomationSpecBuilder::new("auto-webhook-wait")
        .nodes(vec![node.clone()])
        .build();
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");

    let wait = state
        .register_automation_v2_wait_node(&run, &node)
        .await
        .expect("register webhook wait");
    assert_eq!(
        wait.wait_kind,
        crate::stateful_runtime::StatefulWaitKind::Webhook
    );
    let match_rules =
        crate::stateful_runtime::stateful_webhook_wait_match_from_metadata(wait.metadata.as_ref())
            .expect("webhook match rules");
    assert_eq!(match_rules.trigger_id.as_deref(), Some("trigger-callback"));
    assert_eq!(match_rules.provider_event_id.as_deref(), Some("event-42"));
}

#[tokio::test]
async fn timer_wait_parks_and_resumes_the_same_run_with_bounded_output() {
    let state = ready_test_state().await;
    let node = AutomationNodeBuilder::new("wait-for-window")
        .wait(AutomationWaitSpec::Timer {
            delay_ms: Some(1),
            wake_at: None,
            timeout: None,
        })
        .build();
    let automation = AutomationSpecBuilder::new("auto-timer-wait")
        .nodes(vec![node.clone()])
        .build();
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");

    let wait = state
        .register_automation_v2_wait_node(&run, &node)
        .await
        .expect("register timer wait");
    assert_eq!(
        wait.wait_kind,
        crate::stateful_runtime::StatefulWaitKind::Timer
    );
    assert_eq!(
        state
            .get_automation_v2_run(&run.run_id)
            .await
            .unwrap()
            .status,
        AutomationRunStatus::Paused
    );

    let paths = crate::stateful_runtime::StatefulRuntimeStoragePaths::from_runtime_events_path(
        &state.runtime_events_path,
    );
    let tick = crate::stateful_runtime::process_due_stateful_waits(
        &paths,
        wait.wake_at_ms.unwrap().saturating_add(1),
        Default::default(),
    )
    .await;
    assert_eq!(tick.completed, 1);
    state
        .apply_stateful_wait_scheduler_outcome(&tick.outcomes[0])
        .await
        .expect("requeue run");

    let resumed = state.get_automation_v2_run(&run.run_id).await.unwrap();
    assert_eq!(resumed.status, AutomationRunStatus::Queued);
    assert!(resumed.checkpoint.completed_nodes.contains(&node.node_id));
    assert!(!resumed.checkpoint.pending_nodes.contains(&node.node_id));
    let output = resumed
        .checkpoint
        .node_outputs
        .get(&node.node_id)
        .expect("wait output");
    assert_eq!(output["contract_kind"], "stateful_wait");
    assert_eq!(output["content"]["wait_id"], wait.wait_id);
}

#[tokio::test]
async fn external_wait_resolution_is_tenant_scoped_and_exactly_once() {
    let state = ready_test_state().await;
    let node = AutomationNodeBuilder::new("wait-for-review")
        .wait(AutomationWaitSpec::ExternalCondition {
            condition_key: OrchestrationValueBinding::Literal {
                value: json!("review-42"),
            },
            timeout: timeout_policy(),
            payload_schema: Some(json!({
                "type": "object",
                "required": ["accepted"]
            })),
        })
        .build();
    let automation = AutomationSpecBuilder::new("auto-external-wait")
        .nodes(vec![node.clone()])
        .build();
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    let wait = state
        .register_automation_v2_wait_node(&run, &node)
        .await
        .expect("register external wait");

    let invalid = state
        .resolve_automation_v2_external_wait(
            &run.tenant_context,
            &wait.wait_id,
            "invalid-event",
            json!({ "wrong": true }),
        )
        .await
        .expect_err("reject invalid resolution payload");
    assert!(invalid.to_string().contains("$.accepted is required"));

    let resolved = state
        .resolve_automation_v2_external_wait(
            &run.tenant_context,
            &wait.wait_id,
            "review-event-42",
            json!({ "accepted": true }),
        )
        .await
        .expect("resolve wait")
        .expect("claimed wait");
    assert_eq!(
        resolved.status,
        crate::stateful_runtime::StatefulWaitStatus::Woken
    );
    let duplicate = state
        .resolve_automation_v2_external_wait(
            &run.tenant_context,
            &wait.wait_id,
            "review-event-42",
            json!({ "accepted": true }),
        )
        .await
        .expect("duplicate resolution");
    assert!(duplicate.is_some());
    let conflict = state
        .resolve_automation_v2_external_wait(
            &run.tenant_context,
            &wait.wait_id,
            "different-event",
            json!({ "accepted": true }),
        )
        .await
        .expect("conflicting resolution");
    assert!(conflict.is_none());

    let resumed = state.get_automation_v2_run(&run.run_id).await.unwrap();
    assert_eq!(resumed.status, AutomationRunStatus::Queued);
    assert_eq!(resumed.checkpoint.completed_nodes, vec![node.node_id]);
}

#[tokio::test]
async fn restart_recovers_the_parked_before_wait_insert_window() {
    let state = ready_test_state().await;
    let node = AutomationNodeBuilder::new("wait-after-restart")
        .wait(AutomationWaitSpec::Timer {
            delay_ms: Some(30_000),
            wake_at: None,
            timeout: None,
        })
        .build();
    let automation = AutomationSpecBuilder::new("auto-restart-wait")
        .nodes(vec![node.clone()])
        .build();
    let run = state
        .create_automation_v2_run(&automation, "manual")
        .await
        .expect("create run");
    state
        .update_automation_v2_run(&run.run_id, |row| {
            row.status = AutomationRunStatus::Paused;
            crate::app::state::automation::record_automation_lifecycle_event_with_metadata(
                row,
                "wait_node_registration_started",
                Some("simulated crash before wait insert".to_string()),
                None,
                Some(json!({ "node_id": &node.node_id })),
            );
        })
        .await
        .expect("park run");

    assert_eq!(
        state
            .recover_missing_automation_v2_wait_registrations()
            .await,
        1
    );
    assert_eq!(
        state
            .recover_missing_automation_v2_wait_registrations()
            .await,
        0
    );
    let paths = crate::stateful_runtime::StatefulRuntimeStoragePaths::from_runtime_events_path(
        &state.runtime_events_path,
    );
    let waits = crate::stateful_runtime::list_stateful_waits(
        &paths.waits_path,
        &run.tenant_context,
        crate::stateful_runtime::StatefulWaitQuery {
            run_id: Some(&run.run_id),
            wait_kind: Some(crate::stateful_runtime::StatefulWaitKind::Timer),
            status: None,
            limit: None,
        },
    );
    assert_eq!(waits.len(), 1);
    assert_eq!(waits[0].phase_id.as_deref(), Some(node.node_id.as_str()));
}
