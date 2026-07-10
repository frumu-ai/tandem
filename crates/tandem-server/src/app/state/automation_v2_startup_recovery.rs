use super::*;

impl AppState {
    pub(super) async fn append_internal_sweep_protected_audit_event(
        &self,
        event_type: &str,
        run: &AutomationV2RunRecord,
        sweep: &str,
        outcome: &str,
        detail: Option<String>,
        metadata: Value,
    ) {
        let _ = crate::audit::append_protected_audit_event(
            self,
            event_type,
            &run.tenant_context,
            Some("tandem-server:internal-sweep".to_string()),
            json!({
                "source": "automation_v2_internal_sweep",
                "sweep": sweep,
                "actor": {
                    "type": "system",
                    "id": "tandem-server",
                    "component": "automation_v2_sweeper",
                },
                "run_id": run.run_id,
                "runID": run.run_id,
                "automation_id": run.automation_id,
                "automationID": run.automation_id,
                "status": run.status,
                "stop_kind": run.stop_kind,
                "reason": detail,
                "tenantContext": run.tenant_context,
                "outcome": outcome,
                "metadata": metadata,
            }),
        )
        .await;
    }

    pub(super) async fn automation_definition_for_restart_recovery(
        &self,
        run: &AutomationV2RunRecord,
    ) -> Result<AutomationV2Spec, Value> {
        if let Some((recorded, actual)) =
            crate::stateful_runtime::automation_run_definition_snapshot_hash_mismatch(run)
        {
            tracing::warn!(
                run_id = %run.run_id,
                recorded_snapshot_hash = %recorded,
                actual_snapshot_hash = %actual,
                "automation run definition snapshot hash mismatch; using persisted snapshot for restart recovery"
            );
        }
        match run.automation_snapshot.clone() {
            Some(snapshot) => Ok(snapshot),
            None => {
                let Some(automation) = self.get_automation_v2(&run.automation_id).await else {
                    return Err(json!({ "reason": "missing_automation_snapshot" }));
                };
                if let Some(recorded) = run.workflow_definition_snapshot_hash.as_ref() {
                    let actual =
                        crate::stateful_runtime::automation_definition_snapshot_hash(&automation);
                    if recorded != &actual {
                        return Err(json!({
                            "reason": "definition_snapshot_hash_mismatch",
                            "recorded_snapshot_hash": recorded,
                            "actual_snapshot_hash": actual,
                            "definition_source": "current_automation_definition",
                        }));
                    }
                }
                Ok(automation)
            }
        }
    }

    pub async fn recover_in_flight_runs(&self) -> usize {
        let runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .cloned()
            .collect::<Vec<_>>();
        let mut recovered = 0usize;
        for run in runs {
            match run.status {
                AutomationRunStatus::Running => {
                    if self.recover_running_run_after_restart(&run).await {
                        recovered += 1;
                    }
                }
                AutomationRunStatus::Pausing => {
                    // `Pausing` is a transient state: the executor task that
                    // was about to finish pausing is gone after a restart and
                    // will never complete the transition. Settle the run to
                    // `Paused` so it (a) releases its workspace lock (Pausing
                    // holds it, Paused does not) and (b) becomes eligible for
                    // `/recover` via the API. Without this, the Pausing lock
                    // perpetuates across every restart and blocks every new
                    // run on the same workspace.
                    let detail =
                        "automation run settled to paused after server restart".to_string();
                    if let Some(updated_run) = self
                        .update_automation_v2_run(&run.run_id, |row| {
                            row.status = AutomationRunStatus::Paused;
                            if row.pause_reason.is_none() {
                                row.pause_reason = Some(detail.clone());
                            }
                            automation::record_automation_lifecycle_event(
                                row,
                                "run_pausing_settled_on_restart",
                                Some(detail.clone()),
                                None,
                            );
                        })
                        .await
                    {
                        self.append_internal_sweep_protected_audit_event(
                            "automation_v2.internal_sweep.server_restart_settled_pausing_run",
                            &updated_run,
                            "recover_in_flight_runs",
                            "settled_pausing_run",
                            Some(detail),
                            json!({ "previous_status": "pausing" }),
                        )
                        .await;
                        recovered += 1;
                    }
                }
                AutomationRunStatus::Paused | AutomationRunStatus::AwaitingApproval => {
                    if run.status == AutomationRunStatus::AwaitingApproval {
                        let has_settled_gate_decision = run
                            .checkpoint
                            .awaiting_gate
                            .as_ref()
                            .and_then(|gate| {
                                run.checkpoint
                                    .gate_history
                                    .iter()
                                    .rev()
                                    .find(|record| record.node_id == gate.node_id)
                            })
                            .is_some_and(|record| {
                                crate::app::state::automation_gate_decision_settles_wait(
                                    &record.decision,
                                )
                            });
                        if has_settled_gate_decision {
                            let automation =
                                self.automation_definition_for_restart_recovery(&run).await;
                            if let Ok(automation) = automation {
                                if let Some(updated_run) = self
                                    .update_automation_v2_run(&run.run_id, |row| {
                                        crate::app::state::recover_settled_automation_gate_decision(
                                            row,
                                            &automation,
                                        );
                                    })
                                    .await
                                    .filter(|updated| {
                                        updated.status != AutomationRunStatus::AwaitingApproval
                                    })
                                {
                                    self.append_internal_sweep_protected_audit_event(
                                        "automation_v2.internal_sweep.approval_gate_decision_recovered",
                                        &updated_run,
                                        "recover_in_flight_runs",
                                        "recovered_settled_gate_decision",
                                        updated_run.detail.clone(),
                                        json!({ "previous_status": "awaiting_approval" }),
                                    )
                                    .await;
                                    recovered += 1;
                                    continue;
                                }
                            }
                        }
                    }
                    let workspace_root = if automation_status_holds_workspace_lock(&run.status) {
                        self.automation_v2_run_workspace_root(&run).await
                    } else {
                        None
                    };
                    let mut scheduler = self.automation_scheduler.write().await;
                    if automation_status_holds_workspace_lock(&run.status) {
                        scheduler.reserve_workspace(&run.run_id, workspace_root.as_deref());
                    }
                    for (node_id, output) in &run.checkpoint.node_outputs {
                        if let Some((path, content_digest)) =
                            automation::node_output::automation_output_validated_artifact(output)
                        {
                            scheduler.preexisting_registry.register_validated(
                                &run.run_id,
                                node_id,
                                automation::scheduler::ValidatedArtifact {
                                    path,
                                    content_digest,
                                },
                            );
                        }
                    }
                }
                _ => {}
            }
        }
        recovered += self
            .recover_missing_automation_v2_wait_registrations()
            .await;
        recovered += self.recover_lost_stateful_wait_wakes().await;
        // TAN-564: re-drive any dead letters whose retry was requested before a
        // crash so the failed effect actually re-executes on restart.
        recovered += self.dispatch_ready_stateful_dead_letter_retries().await;
        recovered
    }

    /// TAN-566: re-enqueue runs whose durable wait already fired but whose
    /// in-memory requeue was lost to a crash.
    ///
    /// The scheduler finalizes a due wait to a terminal `Woken` state (durably)
    /// and only *then* requeues the run from in-memory tick state
    /// (`apply_stateful_wait_scheduler_outcome`). A crash in that window strands
    /// the run in `Paused` forever: the wait is terminal so the live scheduler
    /// never revisits it, and nothing else resumes the run. On restart we detect
    /// that signature and drive the same idempotent requeue the live path uses.
    ///
    /// A run is only recovered when it is `Paused`, has **no** active
    /// (`Waiting`/`Claimed`) wait — so it is not legitimately parked on a newer
    /// wait — and its most recent `Woken` wait fired at or after the run's last
    /// state change (`wait.updated_at_ms >= run.updated_at_ms`), which is the
    /// lost-wake signature and excludes runs that were re-paused or manually
    /// paused after the wake. Timeout actions (`TimedOut`/`Escalated`) are
    /// intentionally out of scope — their policy-specific replay is a follow-up.
    async fn recover_lost_stateful_wait_wakes(&self) -> usize {
        use crate::stateful_runtime::{
            load_stateful_waits, StatefulRuntimeStoragePaths, StatefulWaitRecord,
            StatefulWaitStatus,
        };

        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&self.runtime_events_path);
        let waits = load_stateful_waits(&paths.waits_path);
        if waits.is_empty() {
            return 0;
        }

        let paused_runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|run| run.status == AutomationRunStatus::Paused)
            .cloned()
            .collect::<Vec<_>>();

        let mut recovered = 0usize;
        for run in paused_runs {
            // Match by run id AND tenant visibility: the waits store is shared
            // across tenants/deployments and the same run_id can appear in more
            // than one, so a foreign tenant's wait must never influence this
            // run's recovery (mirrors the rest of the stateful wait API).
            let run_waits = waits
                .iter()
                .filter(|wait| {
                    wait.run_id == run.run_id && wait.visible_to_tenant(&run.tenant_context)
                })
                .collect::<Vec<&StatefulWaitRecord>>();
            if run_waits.is_empty() {
                continue;
            }
            // Legitimately parked on a live wait — leave it for the scheduler.
            if run_waits.iter().any(|wait| {
                matches!(
                    wait.status,
                    StatefulWaitStatus::Waiting | StatefulWaitStatus::Claimed
                )
            }) {
                continue;
            }
            let Some(wait) = run_waits
                .iter()
                .filter(|wait| {
                    wait.status == StatefulWaitStatus::Woken
                        && wait.event_seq.is_some()
                        && wait.updated_at_ms >= run.updated_at_ms
                })
                .max_by_key(|wait| wait.updated_at_ms)
            else {
                continue;
            };

            let event_seq = wait.event_seq.unwrap_or_default();
            let detail = format!(
                "stateful wait `{}` woke while the run was paused; requeued after server restart",
                wait.wait_id
            );
            if let Some(updated) = self
                .requeue_automation_v2_run_from_stateful_wait_wake(
                    &run.run_id,
                    &wait.wait_id,
                    "stateful_wait_wake_recovered_on_restart",
                    event_seq,
                    detail.clone(),
                    json!({
                        "wait_id": wait.wait_id,
                        "wait_kind": wait.wait_kind,
                        "recovered_on_restart": true,
                    }),
                )
                .await
            {
                self.append_internal_sweep_protected_audit_event(
                    "automation_v2.internal_sweep.stateful_wait_wake_recovered",
                    &updated,
                    "recover_lost_stateful_wait_wakes",
                    "requeued_lost_wake",
                    Some(detail),
                    json!({ "wait_id": wait.wait_id, "event_seq": event_seq }),
                )
                .await;
                recovered += 1;
            }
        }
        recovered
    }

    async fn recover_running_run_after_restart(&self, run: &AutomationV2RunRecord) -> bool {
        self.forget_interrupted_run_handles(run).await;
        let automation = self.automation_definition_for_restart_recovery(run).await;
        let automation = match automation {
            Ok(automation) => automation,
            Err(metadata) => {
                let detail = if metadata.get("reason").and_then(Value::as_str)
                    == Some("definition_snapshot_hash_mismatch")
                {
                    "automation run interrupted by server restart; definition snapshot hash mismatch"
                        .to_string()
                } else {
                    "automation run interrupted by server restart".to_string()
                };
                return self
                    .fail_running_run_after_restart(run, detail, metadata)
                    .await;
            }
        };

        let in_progress_node_ids = automation::lifecycle::automation_in_progress_node_ids(run);
        let detail = if in_progress_node_ids.is_empty() {
            "automation run queued for resume after server restart".to_string()
        } else {
            format!(
                "automation run queued for resume after server restart; repairable node(s): {}",
                in_progress_node_ids.join(", ")
            )
        };
        let mut missing_node_ids = Vec::new();
        let mut exhausted_node_ids = Vec::new();
        for node_id in &in_progress_node_ids {
            if run.checkpoint.node_outputs.contains_key(node_id) {
                continue;
            }
            let Some(node) = automation
                .flow
                .nodes
                .iter()
                .find(|candidate| &candidate.node_id == node_id)
            else {
                missing_node_ids.push(node_id.clone());
                continue;
            };
            let attempts = run
                .checkpoint
                .node_attempts
                .get(node_id)
                .copied()
                .unwrap_or(1);
            if attempts >= automation_node_max_attempts(node) {
                exhausted_node_ids.push(node_id.clone());
            }
        }
        if !missing_node_ids.is_empty() || !exhausted_node_ids.is_empty() {
            return self
                .fail_running_run_after_restart(
                    run,
                    "automation run interrupted by server restart".to_string(),
                    json!({
                        "reason": "unrecoverable_in_progress_nodes",
                        "missing_node_ids": missing_node_ids,
                        "exhausted_node_ids": exhausted_node_ids,
                    }),
                )
                .await;
        }

        let updated_run = self
            .update_automation_v2_run(&run.run_id, |row| {
                for node_id in &in_progress_node_ids {
                    if row.checkpoint.node_outputs.contains_key(node_id) {
                        continue;
                    }
                    let Some(node) = automation
                        .flow
                        .nodes
                        .iter()
                        .find(|candidate| &candidate.node_id == node_id)
                    else {
                        continue;
                    };
                    row.checkpoint.node_outputs.insert(
                        node_id.clone(),
                        crate::automation_v2::executor::build_node_execution_error_output_with_category(
                            node,
                            "node execution interrupted by server restart before an outcome was recorded",
                            false,
                            "server_restart_interrupted",
                        ),
                    );
                    if row.checkpoint.last_failure.is_none() {
                        row.checkpoint.last_failure = Some(crate::AutomationFailureRecord {
                            node_id: node_id.clone(),
                            reason:
                                "node execution interrupted by server restart before an outcome was recorded"
                                    .to_string(),
                            failed_at_ms: now_ms(),
                            failure_kind: Some("server_restart_interrupted".to_string()),
                            metadata: None,
                        });
                    }
                }
                row.status = AutomationRunStatus::Queued;
                row.detail = Some(detail.clone());
                row.resume_reason = Some("server_restart_rehydration".to_string());
                row.stop_kind = None;
                row.stop_reason = None;
                row.active_session_ids.clear();
                row.latest_session_id = None;
                row.active_instance_ids.clear();
                automation::record_automation_lifecycle_event_with_metadata(
                    row,
                    "run_queued_for_resume_after_restart",
                    Some(detail.clone()),
                    None,
                    Some(json!({
                        "previous_status": "running",
                        "in_progress_node_ids": &in_progress_node_ids,
                    })),
                );
                automation::refresh_automation_runtime_state(&automation, row);
            })
            .await;

        if let Some(updated_run) =
            updated_run.filter(|row| row.status == AutomationRunStatus::Queued)
        {
            self.append_internal_sweep_protected_audit_event(
                "automation_v2.internal_sweep.server_restart_queued_run_for_resume",
                &updated_run,
                "recover_in_flight_runs",
                "queued_for_resume",
                Some(detail),
                json!({
                    "previous_status": "running",
                    "in_progress_node_ids": in_progress_node_ids,
                }),
            )
            .await;
            return true;
        }
        false
    }

    async fn fail_running_run_after_restart(
        &self,
        run: &AutomationV2RunRecord,
        detail: String,
        metadata: Value,
    ) -> bool {
        if let Some(updated_run) = self
            .update_automation_v2_run(&run.run_id, |row| {
                row.status = AutomationRunStatus::Failed;
                row.detail = Some(detail.clone());
                row.stop_kind = Some(AutomationStopKind::ServerRestart);
                row.stop_reason = Some(detail.clone());
                row.active_session_ids.clear();
                row.latest_session_id = None;
                row.active_instance_ids.clear();
                automation::record_automation_lifecycle_event(
                    row,
                    "run_failed_server_restart",
                    Some(detail.clone()),
                    Some(AutomationStopKind::ServerRestart),
                );
            })
            .await
        {
            self.append_internal_sweep_protected_audit_event(
                "automation_v2.internal_sweep.server_restart_failed_run",
                &updated_run,
                "recover_in_flight_runs",
                "failed_running_run",
                Some(detail),
                json!({
                    "previous_status": "running",
                    "metadata": metadata,
                }),
            )
            .await;
            return true;
        }
        false
    }

    async fn forget_interrupted_run_handles(&self, run: &AutomationV2RunRecord) {
        for session_id in &run.active_session_ids {
            let _ = self.cancellations.cancel(session_id).await;
            let _ = self
                .run_registry
                .finish_if_match(session_id, &run.run_id)
                .await;
        }
        for instance_id in &run.active_instance_ids {
            let _ = self
                .agent_teams
                .cancel_instance(self, instance_id, "interrupted by server restart")
                .await;
        }
        self.forget_automation_v2_sessions(&run.active_session_ids)
            .await;
    }
}
