// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;

use crate::stateful_runtime::{
    dead_letter_retry_dispatch_count, dead_letter_retry_dispatched_at_ms,
    dead_letter_superseded_by_success, load_stateful_reliability, mark_dead_letter_disposition,
    mark_dead_letter_retry_dispatched, operator_principal,
    stateful_reliability_path_from_runtime_events_path, StatefulDeadLetterRecord,
    StatefulDeadLetterStatus, StatefulRecoveryOption,
};

/// Cap on automatic re-drives of a single dead letter before it is parked for
/// operator review. Bounds runaway retry loops for a persistently failing
/// external effect.
const MAX_DEAD_LETTER_RETRY_ATTEMPTS: u32 = 5;

/// Base backoff between automatic dead-letter retries. Doubled per recorded
/// attempt and clamped to `MAX_DEAD_LETTER_RETRY_BACKOFF_MS`.
const DEAD_LETTER_RETRY_BASE_BACKOFF_MS: u64 = 1_000;
const MAX_DEAD_LETTER_RETRY_BACKOFF_MS: u64 = 300_000;

/// System principal recorded on dispatcher-driven dead-letter dispositions.
const DEAD_LETTER_DISPATCHER_ACTOR: &str = "tandem-server:dead-letter-dispatcher";

impl AppState {
    /// TAN-564: re-execute dead-lettered effects whose retry was requested.
    ///
    /// Historically, requesting a retry on a dead letter only recorded intent
    /// (`RetryRequested`) and appended an audit event — nothing re-executed the
    /// failed effect (this is the reopened TAN-515). This dispatcher closes that
    /// gap: for each retry-eligible tool-effect dead letter whose owning run is
    /// sitting in a recoverable failed state, it resets the failed node's
    /// checkpoint and re-queues the run so the effect re-executes through its
    /// normal **governed** tool-dispatch path (tenant assertion → tool authority
    /// → pre-send outbox gate → receipt). Re-driving the owning run — rather than
    /// re-invoking the external tool directly — is what keeps the retry inside
    /// the governance boundary and avoids a policy bypass.
    ///
    /// Outcome reconciliation rides the existing reliability bridge: a successful
    /// replay supersedes the dead letter (which this dispatcher then transitions
    /// to `Resolved`), while a repeat failure re-opens a fresh `Open` dead letter
    /// from `record_external_action_reliability_bridge`. Exponential backoff plus
    /// an attempt cap bound the loop; exhausted dead letters are parked
    /// (`Ignored`, disposition `retry_exhausted`) for operator review.
    ///
    /// Returns the number of dead letters acted on (dispatched, resolved, or
    /// exhausted). Invoked both at startup (crash safety, alongside
    /// `recover_in_flight_runs`) and on each executor tick.
    pub async fn dispatch_ready_stateful_dead_letter_retries(&self) -> usize {
        let path = stateful_reliability_path_from_runtime_events_path(&self.runtime_events_path);
        let dead_letters = load_stateful_reliability(&path).dead_letters;
        if dead_letters.is_empty() {
            return 0;
        }
        let now = now_ms();
        let mut acted = 0usize;
        // A single run can own several dead letters; one recovery re-drives them
        // all, so requeue any given run at most once per sweep.
        let mut requeued_runs: std::collections::HashSet<String> = std::collections::HashSet::new();
        for dead_letter in dead_letters {
            if !dead_letter_is_retry_candidate(&dead_letter) {
                continue;
            }
            let Some(run_id) = dead_letter.run_id.clone() else {
                continue;
            };
            let Some(run) = self.get_automation_v2_run(&run_id).await else {
                continue;
            };
            // Tenant guard: the reliability store is shared across tenants and a
            // run_id can collide across them, so a foreign-tenant dead letter
            // must never drive this run's recovery (mirrors the stateful wait
            // recovery path and the rest of the reliability API).
            if !dead_letter.visible_to_tenant(&run.tenant_context) {
                continue;
            }
            // A success already landed out-of-band via the reliability bridge —
            // record the terminal `Resolved` transition and move on.
            if dead_letter_superseded_by_success(&dead_letter) {
                if self
                    .resolve_dead_letter_after_success(
                        &path,
                        &run.tenant_context,
                        &dead_letter,
                        now,
                    )
                    .await
                {
                    acted += 1;
                }
                continue;
            }
            // Cap on *dispatcher* retries — counted separately from the
            // record's `attempts` (which is the node/tool execution attempt at
            // creation time, so a dead letter born on a high node attempt must
            // not look pre-exhausted). Park a permanently-failing dead letter
            // for operator review.
            let dispatch_count = dead_letter_retry_dispatch_count(&dead_letter);
            if dispatch_count >= MAX_DEAD_LETTER_RETRY_ATTEMPTS {
                if self
                    .exhaust_dead_letter_retries(&path, &run.tenant_context, &dead_letter, now)
                    .await
                {
                    acted += 1;
                }
                continue;
            }
            // Only re-drive a run that is actually sitting failed. Never touch a
            // run that is already active (Queued/Running) or parked on a live
            // durable wait (Paused) — the latter is TAN-566's territory.
            if !dead_letter_run_is_recoverable(&run.status) {
                continue;
            }
            // Honor exponential backoff between automatic re-drives.
            let backoff_ms = dead_letter_retry_backoff_ms(dispatch_count);
            if let Some(dispatched_at) = dead_letter_retry_dispatched_at_ms(&dead_letter) {
                if now < dispatched_at.saturating_add(backoff_ms) {
                    continue;
                }
            }
            let requeued = requeued_runs.contains(&run_id)
                || self
                    .requeue_run_for_dead_letter_retry(&run, &dead_letter)
                    .await;
            if !requeued {
                continue;
            }
            requeued_runs.insert(run_id.clone());
            let next_backoff = dead_letter_retry_backoff_ms(dispatch_count + 1);
            if matches!(
                mark_dead_letter_retry_dispatched(
                    &path,
                    &run.tenant_context,
                    &dead_letter.dead_letter_id,
                    next_backoff,
                    now,
                )
                .await,
                Ok(Some(_))
            ) {
                acted += 1;
            }
        }
        acted
    }

    /// Reset the dead-lettered effect's node(s) and re-queue the owning run so
    /// the effect re-executes through the governed dispatch path. Mirrors the
    /// checkpoint reset performed by `POST /automations/v2/runs/{id}/recover`,
    /// scoped to the run's recorded failure roots. Returns `true` when the run
    /// was actually re-queued.
    async fn requeue_run_for_dead_letter_retry(
        &self,
        run: &AutomationV2RunRecord,
        dead_letter: &StatefulDeadLetterRecord,
    ) -> bool {
        let automation = match self.automation_definition_for_restart_recovery(run).await {
            Ok(automation) => automation,
            Err(_) => return false,
        };
        let mut roots: std::collections::HashSet<String> =
            run.checkpoint.blocked_nodes.iter().cloned().collect();
        if let Some(failure) = run.checkpoint.last_failure.as_ref() {
            roots.insert(failure.node_id.clone());
        }
        roots.retain(|node_id| {
            automation
                .flow
                .nodes
                .iter()
                .any(|node| &node.node_id == node_id)
        });
        if roots.is_empty() {
            return false;
        }
        let reset_nodes = crate::collect_automation_descendants(&automation, &roots)
            .into_iter()
            .filter(|node_id| {
                automation
                    .flow
                    .nodes
                    .iter()
                    .any(|node| &node.node_id == node_id)
            })
            .collect::<std::collections::HashSet<_>>();
        if reset_nodes.is_empty() {
            return false;
        }
        let detail = format!(
            "dead letter `{}` retry dispatched; re-executing failed effect through the governed path",
            dead_letter.dead_letter_id
        );
        let mut applied = false;
        let updated = self
            .update_automation_v2_run(&run.run_id, |row| {
                // Re-check under the write lock in case the run advanced between
                // the read above and here. `Failed`/`Blocked` are exactly the
                // states we recover from, so — unlike the wait-wake requeue —
                // only bail if the run is already active or truly done.
                if matches!(
                    row.status,
                    AutomationRunStatus::Queued
                        | AutomationRunStatus::Running
                        | AutomationRunStatus::Completed
                        | AutomationRunStatus::Cancelled
                ) {
                    return;
                }
                row.status = AutomationRunStatus::Queued;
                row.finished_at_ms = None;
                row.detail = Some(detail.clone());
                row.resume_reason = Some("stateful_dead_letter_retry_dispatched".to_string());
                row.pause_reason = None;
                row.stop_kind = None;
                row.stop_reason = None;
                row.checkpoint.awaiting_gate = None;
                row.active_session_ids.clear();
                row.latest_session_id = None;
                row.active_instance_ids.clear();
                for node_id in &reset_nodes {
                    row.checkpoint.node_outputs.remove(node_id);
                    // Clearing node_attempts is essential: a node left at its
                    // exhausted attempt count re-fails immediately on resume
                    // (see the executor's attempts-exhausted gate), so the retry
                    // would be a no-op without this reset.
                    row.checkpoint.node_attempts.remove(node_id);
                }
                row.checkpoint
                    .blocked_nodes
                    .retain(|node_id| !reset_nodes.contains(node_id));
                row.checkpoint
                    .completed_nodes
                    .retain(|node_id| !reset_nodes.contains(node_id));
                let mut pending = row.checkpoint.pending_nodes.clone();
                for node_id in &reset_nodes {
                    if !pending.iter().any(|existing| existing == node_id) {
                        pending.push(node_id.clone());
                    }
                }
                pending.sort();
                pending.dedup();
                row.checkpoint.pending_nodes = pending;
                row.checkpoint.last_failure = None;
                automation::record_automation_lifecycle_event_with_metadata(
                    row,
                    "stateful_dead_letter_retry_requeued",
                    Some(detail.clone()),
                    None,
                    Some(json!({
                        "dead_letter_id": dead_letter.dead_letter_id,
                        "source_id": dead_letter.source_id,
                        "attempts": dead_letter.attempts,
                    })),
                );
                automation::refresh_automation_runtime_state(&automation, row);
                applied = true;
            })
            .await;
        if let Some(updated) = updated.filter(|_| applied) {
            self.append_internal_sweep_protected_audit_event(
                "automation_v2.internal_sweep.stateful_dead_letter_retry_dispatched",
                &updated,
                "dispatch_ready_stateful_dead_letter_retries",
                "requeued_for_dead_letter_retry",
                Some(detail),
                json!({
                    "dead_letter_id": dead_letter.dead_letter_id,
                    "source_id": dead_letter.source_id,
                    "attempts": dead_letter.attempts,
                }),
            )
            .await;
            return true;
        }
        false
    }

    /// Transition a dead letter that a successful replay superseded to the
    /// terminal `Resolved` status. Idempotent — no-op once already `Resolved`.
    async fn resolve_dead_letter_after_success(
        &self,
        path: &std::path::Path,
        tenant: &TenantContext,
        dead_letter: &StatefulDeadLetterRecord,
        now_ms: u64,
    ) -> bool {
        if dead_letter.status == StatefulDeadLetterStatus::Resolved {
            return false;
        }
        matches!(
            mark_dead_letter_disposition(
                path,
                tenant,
                &dead_letter.dead_letter_id,
                StatefulDeadLetterStatus::Resolved,
                "retry_succeeded",
                Some("dead letter superseded by a successful effect replay".to_string()),
                operator_principal(Some(DEAD_LETTER_DISPATCHER_ACTOR)),
                now_ms,
            )
            .await,
            Ok(Some(_))
        )
    }

    /// Park a dead letter whose automatic retries are exhausted for operator
    /// review (`Ignored`, disposition `retry_exhausted`). Idempotent.
    async fn exhaust_dead_letter_retries(
        &self,
        path: &std::path::Path,
        tenant: &TenantContext,
        dead_letter: &StatefulDeadLetterRecord,
        now_ms: u64,
    ) -> bool {
        if dead_letter.status == StatefulDeadLetterStatus::Ignored {
            return false;
        }
        matches!(
            mark_dead_letter_disposition(
                path,
                tenant,
                &dead_letter.dead_letter_id,
                StatefulDeadLetterStatus::Ignored,
                "retry_exhausted",
                Some(format!(
                    "automatic retries exhausted after {} dispatch attempts; parked for operator review",
                    dead_letter_retry_dispatch_count(dead_letter)
                )),
                operator_principal(Some(DEAD_LETTER_DISPATCHER_ACTOR)),
                now_ms,
            )
            .await,
            Ok(Some(_))
        )
    }
}

fn dead_letter_is_retry_candidate(dead_letter: &StatefulDeadLetterRecord) -> bool {
    matches!(
        dead_letter.status,
        StatefulDeadLetterStatus::RetryRequested | StatefulDeadLetterStatus::Retrying
    ) && dead_letter.source_type == "tool_effect"
        && dead_letter
            .recovery_options
            .contains(&StatefulRecoveryOption::Retry)
}

/// A run whose dead-lettered effect can be re-driven. Excludes active runs
/// (already executing), terminal-success/cancelled runs, and `Paused` runs
/// (which may legitimately be parked on a live durable wait — TAN-566's domain).
fn dead_letter_run_is_recoverable(status: &AutomationRunStatus) -> bool {
    matches!(
        status,
        AutomationRunStatus::Failed | AutomationRunStatus::Blocked
    )
}

fn dead_letter_retry_backoff_ms(attempts: u32) -> u64 {
    DEAD_LETTER_RETRY_BASE_BACKOFF_MS
        .saturating_mul(1u64 << attempts.min(20))
        .min(MAX_DEAD_LETTER_RETRY_BACKOFF_MS)
}
