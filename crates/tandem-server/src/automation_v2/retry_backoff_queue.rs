use serde_json::json;

use crate::app::state::automation::{QueueReason, SchedulerMetadata};
use crate::app::state::AppState;
use crate::automation_v2::types::{AutomationRunStatus, AutomationV2RunRecord};

pub(crate) struct RetryBackoffRequest {
    node_id: String,
    attempts: u32,
    max_attempts: u32,
    retry_decision: tandem_automation::RetryDecision,
    detail: String,
    backoff_ms: u64,
}

pub(crate) fn retry_backoff_is_due(run: &AutomationV2RunRecord, now_ms: u64) -> bool {
    let Some(scheduler) = run.scheduler.as_ref() else {
        return true;
    };
    if scheduler.queue_reason != Some(QueueReason::RetryBackoff) {
        return true;
    }
    scheduler
        .retry_after_ms
        .map(|retry_after_ms| retry_after_ms <= now_ms)
        .unwrap_or(true)
}

pub(crate) fn retry_backoff_remaining_ms(run: &AutomationV2RunRecord, now_ms: u64) -> Option<u64> {
    let scheduler = run.scheduler.as_ref()?;
    if scheduler.queue_reason != Some(QueueReason::RetryBackoff) {
        return None;
    }
    scheduler
        .retry_after_ms
        .map(|retry_after_ms| retry_after_ms.saturating_sub(now_ms))
}

pub(crate) fn retry_backoff_scheduler_metadata(
    run: &AutomationV2RunRecord,
    node_id: &str,
    attempts: u32,
    retry_decision: &tandem_automation::RetryDecision,
    detail: &str,
    now_ms: u64,
) -> Option<SchedulerMetadata> {
    let backoff_ms = retry_decision.backoff_ms?;
    let retry_after_ms = retry_decision
        .next_retry_at_ms
        .unwrap_or_else(|| now_ms.saturating_add(backoff_ms));
    Some(SchedulerMetadata {
        tenant_context: run.tenant_context.clone(),
        queue_reason: Some(QueueReason::RetryBackoff),
        resource_key: Some(format!(
            "automation://{}/runs/{}/nodes/{node_id}",
            run.automation_id, run.run_id
        )),
        rate_limited_provider: None,
        queued_at_ms: now_ms,
        retry_node_id: Some(node_id.to_string()),
        retry_attempt: Some(attempts.saturating_add(1)),
        retry_backoff_ms: Some(backoff_ms),
        retry_after_ms: Some(retry_after_ms),
        retry_reason: Some(crate::app::state::truncate_text(detail, 500)),
    })
}

pub(crate) fn record_pending_retry_backoff(
    pending: &mut Option<RetryBackoffRequest>,
    node_id: &str,
    attempts: u32,
    max_attempts: u32,
    retry_decision: &tandem_automation::RetryDecision,
    detail: &str,
    backoff_ms: u64,
) {
    if pending.is_some() {
        return;
    }
    *pending = Some(RetryBackoffRequest {
        node_id: node_id.to_string(),
        attempts,
        max_attempts,
        retry_decision: retry_decision.clone(),
        detail: detail.to_string(),
        backoff_ms,
    });
}

pub(crate) fn queue_run_for_retry_backoff(
    run: &mut AutomationV2RunRecord,
    node_id: &str,
    attempts: u32,
    max_attempts: u32,
    retry_decision: &tandem_automation::RetryDecision,
    detail: &str,
    expected_execution_claim_epoch: u64,
    now_ms: u64,
) -> Option<SchedulerMetadata> {
    if run.status != AutomationRunStatus::Running
        || run.execution_claim_epoch != expected_execution_claim_epoch
    {
        return None;
    }
    let metadata =
        retry_backoff_scheduler_metadata(run, node_id, attempts, retry_decision, detail, now_ms)?;
    let backoff_ms = metadata.retry_backoff_ms.unwrap_or_default();
    let retry_after_ms = metadata.retry_after_ms.unwrap_or(now_ms);
    let next_attempt = attempts.saturating_add(1);
    run.status = AutomationRunStatus::Queued;
    run.scheduler = Some(metadata.clone());
    run.finished_at_ms = None;
    run.stop_kind = None;
    run.stop_reason = None;
    run.pause_reason = None;
    run.resume_reason = Some("retry_backoff_scheduled".to_string());
    run.active_session_ids.clear();
    run.active_instance_ids.clear();
    run.detail = Some(format!(
        "retrying node `{node_id}` after transient provider failure; queued for retry in {backoff_ms} ms before attempt {next_attempt}/{max_attempts}: {detail}"
    ));
    crate::app::state::automation::lifecycle::record_automation_lifecycle_event_with_metadata(
        run,
        "node_retry_backoff_scheduled",
        Some(format!(
            "node `{node_id}` retry scheduled after {backoff_ms} ms"
        )),
        None,
        Some(json!({
            "node_id": node_id,
            "attempt": attempts,
            "next_attempt": next_attempt,
            "max_attempts": max_attempts,
            "backoff_ms": backoff_ms,
            "retry_after_ms": retry_after_ms,
            "reason": detail,
            "retry_decision": retry_decision,
        })),
    );
    Some(metadata)
}

pub(crate) async fn queue_pending_retry_backoff(
    state: &AppState,
    run_id: &str,
    expected_execution_claim_epoch: u64,
    pending: Option<RetryBackoffRequest>,
) -> bool {
    let Some(request) = pending else {
        return false;
    };
    let scheduled_at_ms = crate::util::time::now_ms();
    let scheduled = state
        .update_automation_v2_run(run_id, |row| {
            let _ = queue_run_for_retry_backoff(
                row,
                &request.node_id,
                request.attempts,
                request.max_attempts,
                &request.retry_decision,
                &request.detail,
                expected_execution_claim_epoch,
                scheduled_at_ms,
            );
        })
        .await;
    let queued = scheduled.as_ref().is_some_and(|row| {
        row.status == AutomationRunStatus::Queued
            && row.scheduler.as_ref().is_some_and(|metadata| {
                metadata.queue_reason == Some(QueueReason::RetryBackoff)
                    && metadata.retry_node_id.as_deref() == Some(request.node_id.as_str())
            })
    });
    if queued {
        tracing::info!(
            run_id = %run_id,
            node_id = %request.node_id,
            backoff_ms = request.backoff_ms,
            next_attempt = request.attempts.saturating_add(1),
            max_attempts = request.max_attempts,
            "automation node retry queued for durable backoff"
        );
    }
    queued
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn run_with_retry_after(retry_after_ms: u64) -> AutomationV2RunRecord {
        AutomationV2RunRecord {
            run_id: "run-retry".to_string(),
            automation_id: "automation-retry".to_string(),
            tenant_context: tandem_types::TenantContext::local_implicit(),
            trigger_type: "manual".to_string(),
            status: AutomationRunStatus::Queued,
            created_at_ms: 1,
            updated_at_ms: 1,
            started_at_ms: Some(1),
            finished_at_ms: None,
            active_session_ids: Vec::new(),
            latest_session_id: None,
            active_instance_ids: Vec::new(),
            checkpoint: tandem_automation::AutomationRunCheckpoint {
                completed_nodes: Vec::new(),
                pending_nodes: vec!["node-a".to_string()],
                node_outputs: Default::default(),
                node_attempts: Default::default(),
                node_attempt_verdicts: Default::default(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context: None,
            automation_snapshot: None,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            execution_claim: None,
            execution_claim_epoch: 0,
            pause_reason: None,
            resume_reason: None,
            detail: None,
            stop_kind: None,
            stop_reason: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
            scheduler: Some(SchedulerMetadata {
                tenant_context: tandem_types::TenantContext::local_implicit(),
                queue_reason: Some(QueueReason::RetryBackoff),
                resource_key: Some(
                    "automation://automation-retry/runs/run-retry/nodes/node-a".to_string(),
                ),
                rate_limited_provider: None,
                queued_at_ms: 100,
                retry_node_id: Some("node-a".to_string()),
                retry_attempt: Some(2),
                retry_backoff_ms: Some(500),
                retry_after_ms: Some(retry_after_ms),
                retry_reason: Some("provider timeout".to_string()),
            }),
            trigger_reason: None,
            consumed_handoff_id: None,
            learning_summary: None,
            effective_execution_profile: tandem_automation::ExecutionProfile::Strict,
            requested_execution_profile: None,
        }
    }

    #[test]
    fn retry_backoff_due_checks_scheduler_due_time() {
        let run = run_with_retry_after(1_500);

        assert!(!retry_backoff_is_due(&run, 1_499));
        assert_eq!(retry_backoff_remaining_ms(&run, 1_000), Some(500));
        assert!(retry_backoff_is_due(&run, 1_500));
        assert_eq!(retry_backoff_remaining_ms(&run, 1_600), Some(0));
    }

    #[test]
    fn retry_backoff_metadata_survives_json_roundtrip() {
        let run = run_with_retry_after(1_500);
        let encoded = serde_json::to_value(&run).expect("serialize");
        let decoded: AutomationV2RunRecord = serde_json::from_value(encoded).expect("deserialize");
        let scheduler = decoded.scheduler.expect("scheduler");

        assert_eq!(scheduler.queue_reason, Some(QueueReason::RetryBackoff));
        assert_eq!(scheduler.retry_node_id.as_deref(), Some("node-a"));
        assert_eq!(scheduler.retry_attempt, Some(2));
        assert_eq!(scheduler.retry_backoff_ms, Some(500));
        assert_eq!(scheduler.retry_after_ms, Some(1_500));
        assert_eq!(scheduler.retry_reason.as_deref(), Some("provider timeout"));
    }

    #[test]
    fn queue_run_for_retry_backoff_records_durable_retry_state() {
        let mut run = run_with_retry_after(100);
        run.status = AutomationRunStatus::Running;
        run.scheduler = None;
        run.active_session_ids.push("session-a".to_string());
        let decision = tandem_automation::RetryDecision {
            version: 1,
            policy_version_id: "policy".to_string(),
            decision: "retry_scheduled".to_string(),
            failure_class: "provider_transient".to_string(),
            reason: "provider timeout".to_string(),
            attempt: 1,
            max_attempts: 3,
            retryable: true,
            terminal: false,
            next_retry_at_ms: Some(1_500),
            backoff_ms: Some(500),
            terminal_behavior: tandem_automation::RetryTerminalBehavior::default(),
            manual_override_allowed: true,
        };
        let expected_epoch = run.execution_claim_epoch;

        let metadata = queue_run_for_retry_backoff(
            &mut run,
            "node-a",
            1,
            3,
            &decision,
            "provider timeout",
            expected_epoch,
            1_000,
        )
        .expect("metadata");

        assert_eq!(run.status, AutomationRunStatus::Queued);
        assert!(run.active_session_ids.is_empty());
        assert_eq!(metadata.retry_after_ms, Some(1_500));
        assert_eq!(metadata.retry_attempt, Some(2));
        assert_eq!(run.scheduler, Some(metadata));
        assert!(run
            .checkpoint
            .lifecycle_history
            .iter()
            .any(|event| event.event == "node_retry_backoff_scheduled"));
        assert_eq!(
            json!(run.scheduler.unwrap().queue_reason),
            json!("retry_backoff")
        );
    }

    #[test]
    fn queue_run_for_retry_backoff_does_not_resurrect_stopped_run() {
        let mut run = run_with_retry_after(100);
        run.status = AutomationRunStatus::Cancelled;
        run.scheduler = None;
        let decision = tandem_automation::RetryDecision {
            version: 1,
            policy_version_id: "policy".to_string(),
            decision: "retry_scheduled".to_string(),
            failure_class: "provider_transient".to_string(),
            reason: "provider timeout".to_string(),
            attempt: 1,
            max_attempts: 3,
            retryable: true,
            terminal: false,
            next_retry_at_ms: Some(1_500),
            backoff_ms: Some(500),
            terminal_behavior: tandem_automation::RetryTerminalBehavior::default(),
            manual_override_allowed: true,
        };
        let expected_epoch = run.execution_claim_epoch;

        assert_eq!(
            queue_run_for_retry_backoff(
                &mut run,
                "node-a",
                1,
                3,
                &decision,
                "provider timeout",
                expected_epoch,
                1_000,
            ),
            None
        );
        assert_eq!(run.status, AutomationRunStatus::Cancelled);
        assert_eq!(run.scheduler, None);
    }
}
