//! Replay-on-registration for webhook waits (TAN-571, reopen of TAN-524).
//!
//! Split out of `automation_webhook_store.rs` to keep that module under the
//! repository's per-file line-count ceiling. See
//! `register_stateful_webhook_wait_and_replay_pending` for the fix's full
//! rationale.

use anyhow::Context;
use serde_json::json;
use tandem_types::TenantContext;

use crate::stateful_runtime::{
    append_stateful_run_event_once_with_next_seq, begin_claimed_stateful_wait_wake_completion,
    claim_matching_stateful_webhook_wait, finish_claimed_stateful_wait_completion,
    stateful_webhook_wait_match_from_metadata, upsert_stateful_wait, wait_matches_webhook_event,
    write_stateful_run_snapshot, StatefulRunEventRecord, StatefulRunSnapshotRecord,
    StatefulRuntimeStoragePaths, StatefulWaitKind, StatefulWaitRecord, StatefulWaitStatus,
    StatefulWebhookWaitEvent, StatefulWorkflowRunKind, StatefulWorkflowRunStatus,
};
use crate::util::time::now_ms;

use super::{
    automation_webhook_delivery_correlation, cancel_webhook_wait_after_phase_guard_denial,
    guarded_phase_state_for_webhook_wait, stateful_webhook_wake_key, AppState,
    AutomationWebhookDeliveryRecord, AutomationWebhookDeliveryStatus,
    AutomationWebhookRawEventRecord, AUTOMATION_WEBHOOK_STATEFUL_WAIT_CLAIMANT,
    AUTOMATION_WEBHOOK_STATEFUL_WAIT_LEASE_MS,
};

/// Outcome of `register_stateful_webhook_wait_and_replay_pending` (TAN-571).
pub(crate) enum AutomationWebhookWaitReplayOutcome {
    /// The wait was registered; no already-recorded delivery matched it.
    Registered(StatefulWaitRecord),
    /// The wait was registered and immediately woken by an already-recorded
    /// delivery that arrived before the wait existed.
    Woken {
        wait: StatefulWaitRecord,
        delivery: AutomationWebhookDeliveryRecord,
    },
}

/// Same shape as `automation_webhook_stateful_wait_event`, but derived from
/// an already-recorded `AutomationWebhookRawEventRecord` instead of a live
/// trigger + incoming request (TAN-571's replay-on-registration reads
/// history, not a live delivery).
fn stateful_wait_event_from_raw_event(
    event: &AutomationWebhookRawEventRecord,
) -> StatefulWebhookWaitEvent {
    let idempotency_key = event
        .provider_event_id
        .as_deref()
        .map(|provider_event_id| format!("{}:{provider_event_id}", event.provider))
        .unwrap_or_else(|| event.body_digest.clone());
    StatefulWebhookWaitEvent {
        trigger_id: event.trigger_id.clone(),
        provider: event.provider.clone(),
        provider_event_kind: event.provider_event_kind.clone(),
        provider_event_id: event.provider_event_id.clone(),
        body_digest: event.body_digest.clone(),
        idempotency_key,
    }
}

impl AppState {
    /// Register a `StatefulWaitKind::Webhook` wait and immediately replay it
    /// against already-recorded webhook deliveries (TAN-571, reopen of
    /// TAN-524).
    ///
    /// The live drain path (`wake_matching_stateful_webhook_wait_locked`)
    /// already matches an *incoming* delivery against *existing* waits — but
    /// nothing previously matched a *newly registered* wait against
    /// deliveries that were already recorded. If a correlated webhook
    /// arrived before this wait was registered, the drain found no match,
    /// created a new (orphan) run, and the real run's wait was left
    /// depending on a provider redelivery that may never come — a
    /// redelivery only hits idempotency dedupe and returns `Duplicate`
    /// without waking anything (see `webhook_retry_after_orphaned_...`-style
    /// tests). Recorded raw events (`AutomationWebhookRawEventRecord`) are
    /// already durably retained for the configured retention window, so they
    /// serve as the "parked" delivery history this replay needs — no new
    /// parking store is required.
    ///
    /// Only `Accepted` (i.e. signature-verified) deliveries that have not
    /// already woken a wait are eligible; this must never wake a run from a
    /// payload that failed verification. Uses the exact same
    /// claim/phase-guard/wake sequence as the live path so a genuinely
    /// concurrent live delivery racing this replay is resolved correctly by
    /// the shared optimistic-claim mechanism — whichever wins, the other
    /// safely no-ops.
    ///
    /// This does not retroactively cancel an orphan run a too-early delivery
    /// may already have created — whether a given trigger is "always spawn a
    /// new run" or "correlated to an awaited run" is a separate, larger
    /// policy question that is out of scope here. It ensures the
    /// *correlated* run's wait resolves immediately instead of hanging to
    /// timeout waiting on a redelivery.
    pub(crate) async fn register_stateful_webhook_wait_and_replay_pending(
        &self,
        wait: StatefulWaitRecord,
    ) -> anyhow::Result<AutomationWebhookWaitReplayOutcome> {
        debug_assert_eq!(wait.wait_kind, StatefulWaitKind::Webhook);
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&self.runtime_events_path);
        let registered = upsert_stateful_wait(&paths.waits_path, wait).await?;

        let Some(match_rules) =
            stateful_webhook_wait_match_from_metadata(registered.metadata.as_ref())
        else {
            return Ok(AutomationWebhookWaitReplayOutcome::Registered(registered));
        };
        let Some(trigger_id) = match_rules
            .trigger_id
            .as_deref()
            .filter(|id| !id.trim().is_empty())
        else {
            return Ok(AutomationWebhookWaitReplayOutcome::Registered(registered));
        };

        let tenant = registered.scope.tenant_context.clone();
        let now = now_ms();
        // Hold the same lock the live delivery path holds for its critical
        // section, so the candidate scan and the resulting wake bookkeeping
        // happen atomically with respect to a concurrently arriving delivery.
        let _guard = self.automation_webhook_persistence.lock().await;
        let candidates = self
            .list_automation_webhook_raw_events_for_trigger(&tenant, trigger_id)
            .await;
        let Some(matching_event) = candidates.into_iter().find(|event| {
            event.status == AutomationWebhookDeliveryStatus::Accepted
                && event.woken_wait_id.is_none()
                && wait_matches_webhook_event(
                    &registered,
                    &stateful_wait_event_from_raw_event(event),
                )
        }) else {
            return Ok(AutomationWebhookWaitReplayOutcome::Registered(registered));
        };

        let wait_event = stateful_wait_event_from_raw_event(&matching_event);
        let Some(claimed_wait) = claim_matching_stateful_webhook_wait(
            &paths.waits_path,
            &tenant,
            &wait_event,
            AUTOMATION_WEBHOOK_STATEFUL_WAIT_CLAIMANT,
            now,
            AUTOMATION_WEBHOOK_STATEFUL_WAIT_LEASE_MS,
        )
        .await?
        else {
            return Ok(AutomationWebhookWaitReplayOutcome::Registered(registered));
        };
        if claimed_wait.wait_id != registered.wait_id {
            // A different (older) wait matched this event first — leave it
            // to its own resolution; this call only replays for the wait it
            // just registered.
            return Ok(AutomationWebhookWaitReplayOutcome::Registered(registered));
        }

        if let Err(error) = guarded_phase_state_for_webhook_wait(&paths, &claimed_wait, now) {
            cancel_webhook_wait_after_phase_guard_denial(
                &paths,
                &claimed_wait,
                &error.to_string(),
                now,
            )
            .await;
            return Ok(AutomationWebhookWaitReplayOutcome::Registered(registered));
        }
        let wake_key = stateful_webhook_wake_key(&claimed_wait, &wait_event);
        let reserved_wait = begin_claimed_stateful_wait_wake_completion(
            &paths.waits_path,
            &tenant,
            &claimed_wait,
            &wake_key,
            now,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("stateful webhook wait replay wake conflict"))?;
        let phase_state = match guarded_phase_state_for_webhook_wait(&paths, &reserved_wait, now) {
            Ok(phase_state) => phase_state,
            Err(error) => {
                cancel_webhook_wait_after_phase_guard_denial(
                    &paths,
                    &reserved_wait,
                    &error.to_string(),
                    now,
                )
                .await;
                return Ok(AutomationWebhookWaitReplayOutcome::Registered(registered));
            }
        };

        let event_type = "stateful_runtime.wait.webhook_woken_replay";
        let run_event = StatefulRunEventRecord {
            schema_version: 1,
            event_id: format!("stateful-webhook-wake-replay-{}", matching_event.event_id),
            run_id: reserved_wait.run_id.clone(),
            seq: 0,
            event_type: event_type.to_string(),
            occurred_at_ms: now,
            scope: reserved_wait.scope.clone(),
            actor: None,
            phase_id: reserved_wait.phase_id.clone(),
            phase_transition: None,
            wait_kind: Some(StatefulWaitKind::Webhook),
            causation_id: Some(matching_event.event_id.clone()),
            correlation_id: matching_event
                .provider_event_id
                .clone()
                .or_else(|| Some(matching_event.body_digest.clone())),
            payload: json!({
                "raw_event_id": &matching_event.event_id,
                "delivery_id": &matching_event.delivery_id,
                "trigger_id": &matching_event.trigger_id,
                "provider": &matching_event.provider,
                "provider_event_kind": &matching_event.provider_event_kind,
                "provider_event_id": &matching_event.provider_event_id,
                "body_digest": &matching_event.body_digest,
                "wait_id": &reserved_wait.wait_id,
                "wake_idempotency_key": &wake_key,
                "replay": true,
            }),
        };
        let (_appended, seq) = append_stateful_run_event_once_with_next_seq(
            &paths.run_events_path,
            &tenant,
            &run_event,
        )
        .await?;
        let _ = self
            .requeue_automation_v2_run_from_stateful_wait_wake(
                &reserved_wait.run_id,
                &reserved_wait.wait_id,
                event_type,
                seq,
                format!(
                    "stateful webhook wait `{}` woke on registration by replaying an earlier-arriving delivery `{}`",
                    reserved_wait.wait_id, matching_event.event_id
                ),
                json!({
                    "raw_event_id": &matching_event.event_id,
                    "delivery_id": &matching_event.delivery_id,
                    "trigger_id": &matching_event.trigger_id,
                    "provider": &matching_event.provider,
                    "provider_event_id": &matching_event.provider_event_id,
                    "body_digest": &matching_event.body_digest,
                    "replay": true,
                }),
            )
            .await;
        let snapshot = StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: format!("stateful-webhook-wake-replay-{}", matching_event.event_id),
            run_id: reserved_wait.run_id.clone(),
            seq,
            created_at_ms: now,
            scope: reserved_wait.scope.clone(),
            status: StatefulWorkflowRunStatus::Running,
            phase: phase_state.phase,
            phase_history: phase_state.phase_history,
            allowed_next_phases: phase_state.allowed_next_phases,
            phase_id: reserved_wait.phase_id.clone(),
            source_record_kind: Some(StatefulWorkflowRunKind::AutomationV2),
            checkpoint: None,
            payload_digest: Some(matching_event.body_digest.clone()),
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: Some(json!({
                "source": "automation_webhook_replay",
                "raw_event_id": &matching_event.event_id,
                "delivery_id": &matching_event.delivery_id,
                "trigger_id": &matching_event.trigger_id,
                "provider": &matching_event.provider,
                "provider_event_id": &matching_event.provider_event_id,
                "body_digest": &matching_event.body_digest,
                "wait_id": &reserved_wait.wait_id,
            })),
        };
        write_stateful_run_snapshot(&paths.snapshots_root, &snapshot).await?;

        let delivery = self
            .mark_automation_webhook_delivery_woken_by_replay_locked(
                &tenant,
                &matching_event,
                &reserved_wait,
                now,
            )
            .await?;

        let woken_wait = finish_claimed_stateful_wait_completion(
            &paths.waits_path,
            &tenant,
            &reserved_wait,
            &wake_key,
            seq,
            StatefulWaitStatus::Woken,
            now,
        )
        .await?
        .ok_or_else(|| anyhow::anyhow!("stateful webhook wait replay wake conflict"))?;
        self.event_bus.publish(crate::EngineEvent::new(
            "stateful_runtime.wait.webhook_woken",
            json!({
                "runID": &woken_wait.run_id,
                "waitID": &woken_wait.wait_id,
                "deliveryID": &delivery.delivery_id,
                "triggerID": &matching_event.trigger_id,
                "provider": &matching_event.provider,
                "tenantContext": &tenant,
                "replay": true,
            }),
        ));
        Ok(AutomationWebhookWaitReplayOutcome::Woken {
            wait: woken_wait,
            delivery,
        })
    }

    /// Mark the delivery/raw-event pair backing an already-recorded webhook
    /// as having woken `wait`, retroactively (TAN-571). Assumes the caller
    /// already holds `automation_webhook_persistence`.
    async fn mark_automation_webhook_delivery_woken_by_replay_locked(
        &self,
        tenant: &TenantContext,
        event: &AutomationWebhookRawEventRecord,
        wait: &StatefulWaitRecord,
        now_ms: u64,
    ) -> anyhow::Result<AutomationWebhookDeliveryRecord> {
        let delivery_id = event.delivery_id.as_ref().ok_or_else(|| {
            anyhow::anyhow!("accepted raw event `{}` has no delivery id", event.event_id)
        })?;
        let delivery = {
            let mut deliveries = self.automation_webhook_deliveries.write().await;
            let delivery = deliveries
                .get_mut(delivery_id)
                .with_context(|| format!("webhook delivery `{delivery_id}` not found"))?;
            if !delivery.tenant_matches(tenant) {
                anyhow::bail!("webhook delivery tenant mismatch");
            }
            delivery.woken_run_id = Some(wait.run_id.clone());
            delivery.woken_wait_id = Some(wait.wait_id.clone());
            delivery.correlation = Some(automation_webhook_delivery_correlation(
                delivery,
                Some(event.event_id.clone()),
            ));
            delivery.clone()
        };
        self.persist_automation_webhook_deliveries_locked().await?;
        self.update_automation_webhook_raw_event_outcome_locked(
            tenant,
            &event.event_id,
            &delivery,
            now_ms,
        )
        .await?;
        Ok(delivery)
    }
}
