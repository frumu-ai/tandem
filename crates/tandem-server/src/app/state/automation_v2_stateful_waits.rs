use serde_json::{json, Value};
use tandem_types::{ApprovalSourceKind, ApprovalWaitRef};

use crate::app::state::automation;
use crate::automation_v2::types::{
    AutomationGateExpiryAction, AutomationRunStatus, AutomationStopKind, AutomationV2RunRecord,
};
use crate::stateful_runtime::{
    append_stateful_run_event_once_with_next_seq, list_stateful_waits,
    mark_stateful_wait_timeout_result, mark_stateful_wait_woken, stateful_run_from_automation_v2,
    upsert_stateful_wait, StatefulRunEventRecord, StatefulRuntimeStoragePaths, StatefulWaitKind,
    StatefulWaitQuery, StatefulWaitRecord, StatefulWaitSchedulerOutcome, StatefulWaitStatus,
    StatefulWaitTimeoutAction, StatefulWaitTimeoutPolicy, StatefulWorkflowRunStatus,
};
use crate::util::time::now_ms;

use super::AppState;

impl AppState {
    pub(crate) async fn sync_automation_v2_stateful_waits_for_run_or_warn(
        &self,
        run: &AutomationV2RunRecord,
    ) {
        if let Err(error) = self.sync_automation_v2_stateful_waits_for_run(run).await {
            tracing::warn!(
                run_id = %run.run_id,
                error = %error,
                "failed to register active automation v2 stateful wait"
            );
        }
    }

    async fn sync_automation_v2_stateful_waits_for_run(
        &self,
        run: &AutomationV2RunRecord,
    ) -> anyhow::Result<usize> {
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&self.runtime_events_path);
        let mut registered = 0usize;
        for wait in active_stateful_wait_records_for_run(run) {
            if self.register_stateful_wait_if_active(&paths, wait).await? {
                registered += 1;
            }
        }
        Ok(registered)
    }

    async fn register_stateful_wait_if_active(
        &self,
        paths: &StatefulRuntimeStoragePaths,
        wait: StatefulWaitRecord,
    ) -> anyhow::Result<bool> {
        if let Some(existing) = find_stateful_wait(
            paths,
            &wait.scope.tenant_context,
            &wait.run_id,
            &wait.wait_id,
        ) {
            if existing.status.is_terminal() || existing.claim_is_active_at(now_ms()) {
                return Ok(false);
            }
        }

        upsert_stateful_wait(&paths.waits_path, wait).await?;
        Ok(true)
    }

    pub(crate) async fn apply_stateful_wait_scheduler_outcome(
        &self,
        outcome: &StatefulWaitSchedulerOutcome,
    ) -> anyhow::Result<Option<AutomationV2RunRecord>> {
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&self.runtime_events_path);
        let wait = find_stateful_wait(
            &paths,
            &outcome.tenant_context,
            &outcome.run_id,
            &outcome.wait_id,
        );
        match outcome.wait_status {
            StatefulWaitStatus::Woken => {
                if outcome.run_status == StatefulWorkflowRunStatus::Running {
                    return self
                        .queue_automation_v2_run_after_stateful_wait_woken(
                            wait.as_ref(),
                            &outcome.run_id,
                            &outcome.wait_id,
                            outcome.event_type.as_str(),
                            json!({
                                "source": "stateful_wait_scheduler",
                                "event_seq": outcome.event_seq,
                                "lag_ms": outcome.lag_ms,
                            }),
                        )
                        .await;
                }
            }
            StatefulWaitStatus::Cancelled => {
                return self
                    .cancel_automation_v2_run_after_stateful_wait_timeout(wait.as_ref(), outcome)
                    .await;
            }
            StatefulWaitStatus::Escalated => {
                return self
                    .escalate_automation_v2_approval_after_stateful_wait_timeout(
                        wait.as_ref(),
                        outcome,
                    )
                    .await;
            }
            StatefulWaitStatus::TimedOut => {
                return self
                    .remind_automation_v2_approval_after_stateful_wait_timeout(
                        wait.as_ref(),
                        outcome,
                    )
                    .await;
            }
            StatefulWaitStatus::Waiting | StatefulWaitStatus::Claimed => {}
        }
        Ok(None)
    }

    pub(crate) async fn queue_automation_v2_run_after_stateful_wait_woken(
        &self,
        wait: Option<&StatefulWaitRecord>,
        run_id: &str,
        wait_id: &str,
        source_event: &str,
        metadata: Value,
    ) -> anyhow::Result<Option<AutomationV2RunRecord>> {
        let wait_kind = wait
            .map(|wait| wait.wait_kind.clone())
            .unwrap_or(StatefulWaitKind::Timer);
        let detail = format!(
            "stateful {:?} wait `{wait_id}` woke; automation run queued to resume",
            wait_kind
        );
        let updated = self
            .update_automation_v2_run(run_id, |row| {
                if automation_run_is_terminal(&row.status)
                    || row.status == AutomationRunStatus::Running
                {
                    return;
                }
                row.status = AutomationRunStatus::Queued;
                row.detail = Some(detail.clone());
                row.pause_reason = None;
                row.resume_reason = Some(format!("stateful_wait_woken:{wait_id}"));
                row.stop_kind = None;
                row.stop_reason = None;
                row.scheduler = None;
                row.active_session_ids.clear();
                row.latest_session_id = None;
                row.active_instance_ids.clear();
                automation::record_automation_lifecycle_event_with_metadata(
                    row,
                    "stateful_wait_woken_requeued",
                    Some(detail.clone()),
                    None,
                    Some(json!({
                        "wait_id": wait_id,
                        "wait_kind": wait_kind,
                        "source_event": source_event,
                        "stateful_wait": metadata,
                    })),
                );
            })
            .await;
        Ok(updated.filter(|run| run.status == AutomationRunStatus::Queued))
    }

    pub(crate) async fn complete_automation_v2_approval_wait_decision(
        &self,
        run: &AutomationV2RunRecord,
        gate: &crate::AutomationPendingGate,
        decision: &str,
        reason: Option<String>,
    ) -> anyhow::Result<Option<StatefulWaitRecord>> {
        if !automation::automation_gate_decision_settles_wait(decision) {
            return Ok(None);
        }

        let wait_ref =
            ApprovalWaitRef::for_gate(ApprovalSourceKind::AutomationV2, &run.run_id, &gate.node_id);
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&self.runtime_events_path);
        let tenant = &run.tenant_context;
        let wait = find_stateful_wait(&paths, tenant, &run.run_id, &wait_ref.wait_id);
        let Some(wait) = wait else {
            return Ok(None);
        };
        let now = now_ms();
        let completion_key = format!(
            "approval:{}:{}:{}:{}",
            decision, run.run_id, wait_ref.wait_id, gate.requested_at_ms
        );
        let event_type = if decision == "cancel" {
            "stateful_runtime.wait.approval_cancelled"
        } else {
            "stateful_runtime.wait.approval_woken"
        };
        let event = StatefulRunEventRecord {
            schema_version: 1,
            event_id: format!("stateful-approval-decision-{completion_key}"),
            run_id: run.run_id.clone(),
            seq: 0,
            event_type: event_type.to_string(),
            occurred_at_ms: now,
            scope: wait.scope.clone(),
            actor: None,
            phase_id: Some(gate.node_id.clone()),
            phase_transition: None,
            wait_kind: Some(StatefulWaitKind::Approval),
            causation_id: Some(wait_ref.approval_request_id.clone()),
            correlation_id: Some(completion_key.clone()),
            payload: json!({
                "wait_id": wait_ref.wait_id,
                "approval_request_id": wait_ref.approval_request_id,
                "transition_id": wait_ref.transition_id,
                "node_id": gate.node_id,
                "decision": decision,
                "reason": reason,
                "source": "automation_v2_approval_gate",
            }),
        };
        let (_appended, seq) =
            append_stateful_run_event_once_with_next_seq(&paths.run_events_path, tenant, &event)
                .await?;

        if decision == "cancel" {
            mark_stateful_wait_timeout_result(
                &paths.waits_path,
                tenant,
                &run.run_id,
                &wait.wait_id,
                &completion_key,
                seq,
                StatefulWaitStatus::Cancelled,
                now,
            )
            .await
        } else {
            mark_stateful_wait_woken(
                &paths.waits_path,
                tenant,
                &run.run_id,
                &wait.wait_id,
                &completion_key,
                seq,
                now,
            )
            .await
        }
    }

    pub(crate) async fn complete_automation_v2_approval_wait_expired(
        &self,
        run: &AutomationV2RunRecord,
        gate: &crate::AutomationPendingGate,
        expires_at_ms: u64,
    ) -> anyhow::Result<Option<StatefulWaitRecord>> {
        let wait_ref =
            ApprovalWaitRef::for_gate(ApprovalSourceKind::AutomationV2, &run.run_id, &gate.node_id);
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&self.runtime_events_path);
        let tenant = &run.tenant_context;
        let Some(wait) = find_stateful_wait(&paths, tenant, &run.run_id, &wait_ref.wait_id) else {
            return Ok(None);
        };
        let now = now_ms();
        let completion_key = format!(
            "approval:expired:{}:{}:{expires_at_ms}",
            run.run_id, wait_ref.wait_id
        );
        let event = StatefulRunEventRecord {
            schema_version: 1,
            event_id: format!("stateful-approval-expired-{completion_key}"),
            run_id: run.run_id.clone(),
            seq: 0,
            event_type: "stateful_runtime.wait.approval_cancelled".to_string(),
            occurred_at_ms: now,
            scope: wait.scope.clone(),
            actor: None,
            phase_id: Some(gate.node_id.clone()),
            phase_transition: None,
            wait_kind: Some(StatefulWaitKind::Approval),
            causation_id: Some(wait_ref.approval_request_id),
            correlation_id: Some(completion_key.clone()),
            payload: json!({
                "wait_id": wait_ref.wait_id,
                "node_id": gate.node_id,
                "expires_at_ms": expires_at_ms,
                "source": "automation_v2_approval_gate_expiry",
            }),
        };
        let (_appended, seq) =
            append_stateful_run_event_once_with_next_seq(&paths.run_events_path, tenant, &event)
                .await?;

        mark_stateful_wait_timeout_result(
            &paths.waits_path,
            tenant,
            &run.run_id,
            &wait.wait_id,
            &completion_key,
            seq,
            StatefulWaitStatus::Cancelled,
            now,
        )
        .await
    }

    async fn cancel_automation_v2_run_after_stateful_wait_timeout(
        &self,
        wait: Option<&StatefulWaitRecord>,
        outcome: &StatefulWaitSchedulerOutcome,
    ) -> anyhow::Result<Option<AutomationV2RunRecord>> {
        if wait.is_some_and(|wait| wait.wait_kind == StatefulWaitKind::Approval) {
            let Some(run) = self.get_automation_v2_run(&outcome.run_id).await else {
                return Ok(None);
            };
            let Some(gate) = run.checkpoint.awaiting_gate.clone() else {
                return Ok(None);
            };
            let Some(policy) = automation::effective_automation_gate_expiry_policy(&gate) else {
                return Ok(None);
            };
            let expires_at_ms =
                automation::automation_gate_expires_at_ms(&gate).unwrap_or_else(now_ms);
            if self
                .expire_awaiting_approval_gate(&run, &gate, &policy, expires_at_ms)
                .await
            {
                return Ok(self.get_automation_v2_run(&outcome.run_id).await);
            }
            return Ok(None);
        }

        let detail = format!(
            "stateful wait `{}` timed out and cancelled the automation run",
            outcome.wait_id
        );
        let updated = self
            .update_automation_v2_run(&outcome.run_id, |row| {
                if automation_run_is_terminal(&row.status) {
                    return;
                }
                row.status = AutomationRunStatus::Cancelled;
                row.detail = Some(detail.clone());
                row.stop_kind = Some(AutomationStopKind::Cancelled);
                row.stop_reason = Some(detail.clone());
                automation::record_automation_lifecycle_event_with_metadata(
                    row,
                    "stateful_wait_timeout_cancelled",
                    Some(detail.clone()),
                    Some(AutomationStopKind::Cancelled),
                    Some(json!({
                        "wait_id": outcome.wait_id,
                        "event_type": outcome.event_type,
                        "event_seq": outcome.event_seq,
                        "lag_ms": outcome.lag_ms,
                    })),
                );
            })
            .await;
        Ok(updated)
    }

    async fn escalate_automation_v2_approval_after_stateful_wait_timeout(
        &self,
        wait: Option<&StatefulWaitRecord>,
        outcome: &StatefulWaitSchedulerOutcome,
    ) -> anyhow::Result<Option<AutomationV2RunRecord>> {
        if !wait.is_some_and(|wait| wait.wait_kind == StatefulWaitKind::Approval) {
            return Ok(None);
        }
        let Some(run) = self.get_automation_v2_run(&outcome.run_id).await else {
            return Ok(None);
        };
        let Some(gate) = run.checkpoint.awaiting_gate.clone() else {
            return Ok(None);
        };
        let Some(policy) = automation::effective_automation_gate_expiry_policy(&gate) else {
            return Ok(None);
        };
        let expires_at_ms = automation::automation_gate_expires_at_ms(&gate).unwrap_or_else(now_ms);
        if self
            .escalate_awaiting_approval_gate(&run, &gate, &policy, expires_at_ms)
            .await
        {
            return Ok(self.get_automation_v2_run(&outcome.run_id).await);
        }
        Ok(None)
    }

    async fn remind_automation_v2_approval_after_stateful_wait_timeout(
        &self,
        wait: Option<&StatefulWaitRecord>,
        outcome: &StatefulWaitSchedulerOutcome,
    ) -> anyhow::Result<Option<AutomationV2RunRecord>> {
        if !wait.is_some_and(|wait| wait.wait_kind == StatefulWaitKind::Approval) {
            return Ok(None);
        }
        let Some(run) = self.get_automation_v2_run(&outcome.run_id).await else {
            return Ok(None);
        };
        let Some(gate) = run.checkpoint.awaiting_gate.clone() else {
            return Ok(None);
        };
        let Some(policy) = automation::effective_automation_gate_expiry_policy(&gate) else {
            return Ok(None);
        };
        let expires_at_ms = automation::automation_gate_expires_at_ms(&gate).unwrap_or_else(now_ms);
        if self
            .record_awaiting_approval_gate_reminder(&run, &gate, &policy, expires_at_ms, true)
            .await
        {
            return Ok(self.get_automation_v2_run(&outcome.run_id).await);
        }
        Ok(None)
    }
}

fn active_stateful_wait_records_for_run(run: &AutomationV2RunRecord) -> Vec<StatefulWaitRecord> {
    let mut waits = Vec::new();
    if let Some(wait) = approval_stateful_wait_record(run) {
        waits.push(wait);
    }
    waits
}

fn approval_stateful_wait_record(run: &AutomationV2RunRecord) -> Option<StatefulWaitRecord> {
    if run.status != AutomationRunStatus::AwaitingApproval {
        return None;
    }
    let gate = run.checkpoint.awaiting_gate.as_ref()?;
    let wait_ref =
        ApprovalWaitRef::for_gate(ApprovalSourceKind::AutomationV2, &run.run_id, &gate.node_id);
    let stateful_run = stateful_run_from_automation_v2(run);
    let timeout_policy = approval_gate_timeout_policy(gate);
    Some(StatefulWaitRecord {
        schema_version: 1,
        wait_id: wait_ref.wait_id.clone(),
        run_id: run.run_id.clone(),
        wait_kind: StatefulWaitKind::Approval,
        status: StatefulWaitStatus::Waiting,
        scope: stateful_run.scope,
        phase_id: Some(gate.node_id.clone()),
        reason: Some(format!("awaiting approval for gate `{}`", gate.node_id)),
        created_at_ms: gate.requested_at_ms,
        updated_at_ms: run.updated_at_ms,
        wake_at_ms: None,
        timeout_policy,
        event_seq: None,
        wake_idempotency_key: None,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        completed_at_ms: None,
        metadata: Some(approval_wait_metadata(run, gate, &wait_ref)),
    })
}

fn approval_gate_timeout_policy(
    gate: &crate::AutomationPendingGate,
) -> Option<StatefulWaitTimeoutPolicy> {
    let policy = automation::effective_automation_gate_expiry_policy(gate)?;
    let timeout_at_ms = automation::automation_gate_expires_at_ms(gate)?;
    Some(StatefulWaitTimeoutPolicy {
        timeout_at_ms,
        on_timeout: match policy
            .on_expiry
            .unwrap_or(AutomationGateExpiryAction::Cancel)
        {
            AutomationGateExpiryAction::Cancel => StatefulWaitTimeoutAction::Cancel,
            AutomationGateExpiryAction::Escalate => StatefulWaitTimeoutAction::Escalate,
            AutomationGateExpiryAction::Remind => StatefulWaitTimeoutAction::Remind,
        },
        escalate_to: policy.escalate_to.clone(),
        remind_every_ms: policy.remind_every_ms,
        metadata: Some(json!({
            "source": "automation_v2_approval_gate",
            "expiry_policy": policy,
        })),
    })
}

fn approval_wait_metadata(
    run: &AutomationV2RunRecord,
    gate: &crate::AutomationPendingGate,
    wait_ref: &ApprovalWaitRef,
) -> Value {
    let mut object = match gate.metadata.clone() {
        Some(Value::Object(object)) => object,
        Some(value) => {
            let mut object = serde_json::Map::new();
            object.insert("gate_metadata".to_string(), value);
            object
        }
        None => serde_json::Map::new(),
    };
    object.insert("source".to_string(), json!("automation_v2_approval_gate"));
    object.insert("automation_id".to_string(), json!(run.automation_id));
    object.insert("node_id".to_string(), json!(gate.node_id));
    object.insert("approval_wait".to_string(), json!(wait_ref));
    object.insert(
        "stateful_wait_registered_at_ms".to_string(),
        json!(now_ms()),
    );
    Value::Object(object)
}

fn find_stateful_wait(
    paths: &StatefulRuntimeStoragePaths,
    tenant: &tandem_types::TenantContext,
    run_id: &str,
    wait_id: &str,
) -> Option<StatefulWaitRecord> {
    list_stateful_waits(
        &paths.waits_path,
        tenant,
        StatefulWaitQuery {
            run_id: Some(run_id),
            ..StatefulWaitQuery::default()
        },
    )
    .into_iter()
    .find(|wait| wait.wait_id == wait_id)
}

fn automation_run_is_terminal(status: &AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Completed
            | AutomationRunStatus::Blocked
            | AutomationRunStatus::Failed
            | AutomationRunStatus::Cancelled
    )
}
