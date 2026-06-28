use super::*;

const DEFAULT_AUTOMATION_RUN_CLAIM_LEASE_MS: u64 = 30 * 60 * 1000;
const MIN_AUTOMATION_RUN_CLAIM_LEASE_MS: u64 = 5 * 1000;
const MAX_AUTOMATION_RUN_CLAIM_LEASE_MS: u64 = 6 * 60 * 60 * 1000;

fn automation_run_claim_lease_ms() -> u64 {
    std::env::var("TANDEM_AUTOMATION_RUN_CLAIM_LEASE_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_AUTOMATION_RUN_CLAIM_LEASE_MS)
        .clamp(
            MIN_AUTOMATION_RUN_CLAIM_LEASE_MS,
            MAX_AUTOMATION_RUN_CLAIM_LEASE_MS,
        )
}

fn automation_run_claimant_id() -> String {
    std::env::var("TANDEM_AUTOMATION_RUN_CLAIMANT_ID")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            format!(
                "tandem-server:automation-v2-executor:{}",
                std::process::id()
            )
        })
}

fn claimable_queued_run_id(
    runs: &std::collections::HashMap<String, AutomationV2RunRecord>,
) -> Option<String> {
    runs.values()
        .filter(|row| row.status == AutomationRunStatus::Queued)
        .min_by(|a, b| a.created_at_ms.cmp(&b.created_at_ms))
        .map(|row| row.run_id.clone())
}

fn run_has_expired_launch_claim(run: &AutomationV2RunRecord, now: u64) -> bool {
    run.status == AutomationRunStatus::Running
        && run.active_session_ids.is_empty()
        && run.active_instance_ids.is_empty()
        && run
            .execution_claim
            .as_ref()
            .is_some_and(|claim| claim.is_expired(now))
}

impl AppState {
    pub async fn reclaim_abandoned_automation_v2_run_leases(&self) -> usize {
        let now = now_ms();
        let candidates = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|run| run_has_expired_launch_claim(run, now))
            .cloned()
            .collect::<Vec<_>>();

        let mut reclaimed = 0usize;
        for run in candidates {
            let mut reclaimed_claim: Option<AutomationRunExecutionClaim> = None;
            let detail =
                "automation run requeued after execution claim lease expired before work began"
                    .to_string();
            if let Some(updated_run) = self
                .update_automation_v2_run(&run.run_id, |row| {
                    if !run_has_expired_launch_claim(row, now) {
                        return;
                    }
                    reclaimed_claim = row.execution_claim.clone();
                    row.status = AutomationRunStatus::Queued;
                    row.detail = Some(detail.clone());
                    row.resume_reason = Some("abandoned_execution_claim".to_string());
                    row.stop_kind = None;
                    row.stop_reason = None;
                    row.active_session_ids.clear();
                    row.latest_session_id = None;
                    row.active_instance_ids.clear();
                    automation::record_automation_lifecycle_event_with_metadata(
                        row,
                        "run_execution_claim_expired_requeued",
                        Some(detail.clone()),
                        None,
                        Some(json!({
                            "previous_status": "running",
                            "claim": reclaimed_claim,
                        })),
                    );
                })
                .await
            {
                if let Some(claim) = reclaimed_claim {
                    reclaimed += 1;
                    self.append_internal_sweep_protected_audit_event(
                        "automation_v2.internal_sweep.execution_claim_expired_requeued",
                        &updated_run,
                        "reclaim_abandoned_automation_v2_run_leases",
                        "requeued_expired_claim",
                        Some(detail.clone()),
                        json!({
                            "previous_status": "running",
                            "claim": claim,
                        }),
                    )
                    .await;
                }
            }
        }

        reclaimed
    }

    pub async fn claim_next_queued_automation_v2_run(&self) -> Option<AutomationV2RunRecord> {
        let run_id = {
            let guard = self.automation_v2_runs.read().await;
            claimable_queued_run_id(&guard)
        };
        let run_id = match run_id {
            Some(run_id) => run_id,
            None => {
                if self.reclaim_abandoned_automation_v2_run_leases().await == 0 {
                    return None;
                }
                let guard = self.automation_v2_runs.read().await;
                claimable_queued_run_id(&guard)?
            }
        };
        self.claim_specific_automation_v2_run(&run_id).await
    }

    pub async fn claim_specific_automation_v2_run(
        &self,
        run_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        const STARTUP_RUNTIME_CONTEXT_MISSING: &str =
            "runtime context partition missing for automation run";
        const STARTUP_RUNTIME_CONTEXT_FAILURE_NODE: &str = "runtime_context";

        let (automation_snapshot, previous_status, automation_id, stored_runtime_context) = {
            let mut guard = self.automation_v2_runs.write().await;
            let run = guard.get_mut(run_id)?;
            if run.status != AutomationRunStatus::Queued {
                return None;
            }
            (
                run.automation_snapshot.clone(),
                run.status.clone(),
                run.automation_id.clone(),
                run.runtime_context.clone(),
            )
        };
        let automation_for_context = if let Some(automation) = automation_snapshot {
            Some(automation)
        } else {
            self.get_automation_v2(&automation_id).await
        };
        let runtime_context_required = automation_for_context
            .as_ref()
            .map(crate::automation_v2::types::AutomationV2Spec::requires_runtime_context)
            .unwrap_or(false);
        let computed_runtime_context = match automation_for_context.as_ref() {
            Some(automation) => self
                .automation_v2_effective_runtime_context(
                    automation,
                    automation
                        .runtime_context_materialization()
                        .or_else(|| automation.approved_plan_runtime_context_materialization()),
                )
                .await
                .ok()
                .flatten(),
            None => None,
        };
        let runtime_context = computed_runtime_context.or(stored_runtime_context);
        if runtime_context_required && runtime_context.is_none() {
            let mut guard = self.automation_v2_runs.write().await;
            let run = guard.get_mut(run_id)?;
            if run.status != AutomationRunStatus::Queued {
                return None;
            }
            let previous_status = run.status.clone();
            let now = now_ms();
            run.status = AutomationRunStatus::Failed;
            run.updated_at_ms = now;
            run.finished_at_ms.get_or_insert(now);
            run.scheduler = None;
            run.detail = Some(STARTUP_RUNTIME_CONTEXT_MISSING.to_string());
            if run.checkpoint.last_failure.is_none() {
                run.checkpoint.last_failure = Some(crate::AutomationFailureRecord {
                    node_id: STARTUP_RUNTIME_CONTEXT_FAILURE_NODE.to_string(),
                    reason: STARTUP_RUNTIME_CONTEXT_MISSING.to_string(),
                    failed_at_ms: now,
                });
            }
            let claimed = run.clone();
            drop(guard);
            self.sync_automation_scheduler_for_run_transition(previous_status, &claimed)
                .await;
            let _ = self.persist_automation_v2_runs().await;
            return None;
        }

        // GOV-B6a: re-check governance at the moment of launch. A run queued before
        // its agent hit the weekly spend cap must not transition into execution and
        // burn more budget; hold it as `Paused + GuardrailStopped` so the existing
        // `auto_resume_guardrail_stopped_runs` sweep resumes it once a quota override
        // is approved.
        if let Some(automation) = automation_for_context.as_ref() {
            if self.run_launch_blocked_by_spend_pause(automation).await {
                let mut guard = self.automation_v2_runs.write().await;
                let run = guard.get_mut(run_id)?;
                if run.status != AutomationRunStatus::Queued {
                    return None;
                }
                let previous_status = run.status.clone();
                let now = now_ms();
                let reason =
                    "automation run held at launch: agent weekly spend cap reached".to_string();
                run.status = AutomationRunStatus::Paused;
                run.updated_at_ms = now;
                run.scheduler = None;
                run.stop_kind = Some(AutomationStopKind::GuardrailStopped);
                run.pause_reason = Some(reason.clone());
                run.detail = Some(reason.clone());
                run.stop_reason = Some(reason.clone());
                automation::record_automation_lifecycle_event_with_metadata(
                    run,
                    "run_launch_held",
                    Some(reason.clone()),
                    Some(AutomationStopKind::GuardrailStopped),
                    Some(json!({ "reason": "agent_spend_paused" })),
                );
                let held = run.clone();
                drop(guard);
                self.sync_automation_scheduler_for_run_transition(previous_status, &held)
                    .await;
                let _ = self.persist_automation_v2_runs().await;
                return None;
            }
        }

        let mut guard = self.automation_v2_runs.write().await;
        let run = guard.get_mut(run_id)?;
        if run.status != AutomationRunStatus::Queued {
            return None;
        }
        let now = now_ms();
        if run.automation_snapshot.is_none() {
            run.automation_snapshot = automation_for_context.clone();
        }
        run.runtime_context = runtime_context;
        run.status = AutomationRunStatus::Running;
        run.updated_at_ms = now;
        run.started_at_ms.get_or_insert(now);
        run.scheduler = None;
        let lease_epoch = run.execution_claim_epoch.saturating_add(1).max(1);
        let lease_ms = automation_run_claim_lease_ms();
        let claim = AutomationRunExecutionClaim {
            claim_id: format!("run-claim-{}", uuid::Uuid::new_v4()),
            claimant_id: automation_run_claimant_id(),
            claimed_at_ms: now,
            lease_expires_at_ms: now.saturating_add(lease_ms),
            lease_epoch,
        };
        run.execution_claim_epoch = lease_epoch;
        run.execution_claim = Some(claim.clone());
        automation::record_automation_lifecycle_event_with_metadata(
            run,
            "run_execution_claimed",
            Some(format!("automation run claimed by {}", claim.claimant_id)),
            None,
            Some(json!({
                "claim": claim,
            })),
        );
        let claimed = run.clone();
        drop(guard);
        self.sync_automation_scheduler_for_run_transition(previous_status, &claimed)
            .await;
        let _ = self.persist_automation_v2_runs().await;
        Some(claimed)
    }
}
