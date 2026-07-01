use super::*;

const AUTOMATION_STALE_NODE_TIMEOUT_GRACE_MS: u64 = 60_000;

fn automation_run_effective_stale_after_ms(
    run: &AutomationV2RunRecord,
    default_stale_after_ms: u64,
) -> u64 {
    let Some(automation) = run.automation_snapshot.as_ref() else {
        return default_stale_after_ms;
    };
    let max_node_timeout_ms = automation::lifecycle::automation_in_progress_node_ids(run)
        .iter()
        .filter_map(|node_id| {
            automation
                .flow
                .nodes
                .iter()
                .find(|node| &node.node_id == node_id)
        })
        .map(automation::effective_automation_node_timeout_ms)
        .max()
        .unwrap_or(0);
    default_stale_after_ms
        .max(max_node_timeout_ms.saturating_add(AUTOMATION_STALE_NODE_TIMEOUT_GRACE_MS))
}

impl AppState {
    async fn automation_run_last_activity_at_ms(&self, run: &AutomationV2RunRecord) -> u64 {
        let mut last_activity_at_ms = automation::lifecycle::automation_last_activity_at_ms(run);
        for session_id in &run.active_session_ids {
            if let Some(session) = self.storage.get_session(session_id).await {
                last_activity_at_ms = last_activity_at_ms.max(
                    session
                        .time
                        .updated
                        .timestamp_millis()
                        .max(0)
                        .try_into()
                        .unwrap_or_default(),
                );
            }
            if let Some(active_run) = self.run_registry.get(session_id).await {
                if active_run.run_id == run.run_id {
                    last_activity_at_ms = last_activity_at_ms.max(active_run.last_activity_at_ms);
                }
            }
        }
        last_activity_at_ms
    }

    pub async fn reap_stale_running_automation_runs(&self, stale_after_ms: u64) -> usize {
        let _ = self.reclaim_abandoned_automation_v2_run_leases().await;
        let now = now_ms();
        let candidate_runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|run| run.status == AutomationRunStatus::Running)
            .filter(|run| {
                !automation_v2_run_claims::run_has_unexpired_launch_claim_without_progress(run, now)
            })
            .cloned()
            .collect::<Vec<_>>();
        let mut runs = Vec::new();
        for run in candidate_runs {
            let last_activity_at_ms = self.automation_run_last_activity_at_ms(&run).await;
            let effective_stale_after_ms =
                automation_run_effective_stale_after_ms(&run, stale_after_ms);
            if now.saturating_sub(last_activity_at_ms) >= effective_stale_after_ms {
                runs.push((run, effective_stale_after_ms));
            }
        }
        let mut reaped = 0usize;
        for (run, stale_after_ms) in runs {
            let run_id = run.run_id.clone();
            let session_ids = run.active_session_ids.clone();
            let instance_ids = run.active_instance_ids.clone();
            let stale_node_ids = automation::lifecycle::automation_in_progress_node_ids(&run);
            let detail = format!(
                "automation run paused after no provider activity for at least {}s",
                stale_after_ms / 1000
            );
            for session_id in &session_ids {
                let _ = self.cancellations.cancel(session_id).await;
                let _ = self.run_registry.finish_if_match(session_id, &run_id).await;
            }
            for instance_id in instance_ids {
                let _ = self
                    .agent_teams
                    .cancel_instance(self, &instance_id, "paused by stale-run reaper")
                    .await;
            }
            self.forget_automation_v2_sessions(&session_ids).await;
            let automation_name = run
                .automation_snapshot
                .as_ref()
                .map(|automation| automation.name.clone());
            let mut terminal_stale_node_ids = Vec::new();
            let updated_run = self
                .update_automation_v2_run(&run_id, |row| {
                    let stale_node_detail = format!(
                        "node execution stalled after no provider activity for at least {}s",
                        stale_after_ms / 1000
                    );
                    let automation_snapshot = row.automation_snapshot.clone();
                    let mut annotated_nodes = Vec::new();
                    let mut terminal_nodes = Vec::new();
                    if let Some(automation) = automation_snapshot.as_ref() {
                        for node_id in &stale_node_ids {
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
                            let attempts =
                                row.checkpoint.node_attempts.get(node_id).copied().unwrap_or(1);
                            let max_attempts = automation_node_max_attempts(node);
                            let terminal = attempts >= max_attempts;
                            if terminal {
                                terminal_nodes.push(node_id.clone());
                            }
                            row.checkpoint.node_outputs.insert(
                                node_id.clone(),
                                crate::automation_v2::executor::build_node_execution_error_output_with_category(
                                    node,
                                    &stale_node_detail,
                                    terminal,
                                    "stale_no_provider_activity",
                                ),
                            );
                            if row.checkpoint.last_failure.is_none() {
                                row.checkpoint.last_failure = Some(
                                    crate::automation_v2::types::AutomationFailureRecord {
                                        node_id: node_id.clone(),
                                        reason: stale_node_detail.clone(),
                                        failed_at_ms: now_ms(),
                                        failure_kind: Some(
                                            "stale_no_provider_activity".to_string(),
                                        ),
                                        metadata: None,
                                    },
                                );
                            }
                            annotated_nodes.push(node_id.clone());
                        }
                    }
                    terminal_stale_node_ids = terminal_nodes.clone();
                    let terminal = !terminal_nodes.is_empty();
                    row.status = if terminal {
                        AutomationRunStatus::Failed
                    } else {
                        AutomationRunStatus::Paused
                    };
                    row.pause_reason = if terminal {
                        None
                    } else {
                        Some("stale_no_provider_activity".to_string())
                    };
                    row.detail = Some(if terminal {
                        format!(
                            "automation run failed after no provider activity for at least {}s; terminal stale node(s): {}",
                            stale_after_ms / 1000,
                            terminal_nodes.join(", ")
                        )
                    } else if annotated_nodes.is_empty() {
                        detail.clone()
                    } else {
                        format!(
                            "{}; repairable node(s): {}",
                            detail,
                            annotated_nodes.join(", ")
                        )
                    });
                    row.stop_kind = Some(AutomationStopKind::StaleReaped);
                    row.stop_reason = row.detail.clone().or_else(|| Some(detail.clone()));
                    row.active_session_ids.clear();
                    row.latest_session_id = None;
                    row.active_instance_ids.clear();
                    automation::record_automation_lifecycle_event(
                        row,
                        if terminal {
                            "run_failed_stale_no_provider_activity"
                        } else {
                            "run_paused_stale_no_provider_activity"
                        },
                        row.detail.clone().or_else(|| Some(detail.clone())),
                        Some(AutomationStopKind::StaleReaped),
                    );
                    if let Some(automation) = automation_snapshot.as_ref() {
                        automation::refresh_automation_runtime_state(automation, row);
                    }
                })
                .await;
            if let Some(updated_run) = updated_run {
                let terminal = updated_run.status == AutomationRunStatus::Failed;
                if terminal {
                    if let Some(automation) = updated_run.automation_snapshot.as_ref() {
                        crate::automation_v2::executor::publish_automation_v2_failure_event(
                            self,
                            automation,
                            &updated_run,
                        );
                    }
                }
                self.append_internal_sweep_protected_audit_event(
                    if terminal {
                        "automation_v2.internal_sweep.failed_stale_run"
                    } else {
                        "automation_v2.internal_sweep.paused_stale_run"
                    },
                    &updated_run,
                    "reap_stale_running_automation_runs",
                    if terminal { "failed" } else { "paused" },
                    updated_run.detail.clone().or_else(|| Some(detail.clone())),
                    json!({
                        "stale_node_ids": stale_node_ids.clone(),
                        "terminal_stale_node_ids": terminal_stale_node_ids.clone(),
                        "stale_after_ms": stale_after_ms,
                    }),
                )
                .await;
                self.event_bus.publish(EngineEvent::new(
                    if terminal {
                        "automation_v2.run.failed_stale_no_provider_activity"
                    } else {
                        "automation_v2.run.paused_stale_no_provider_activity"
                    },
                    json!({
                        "automation_id": run.automation_id,
                        "automationID": run.automation_id,
                        "workflow_id": run.automation_id,
                        "workflowID": run.automation_id,
                        "workflow_name": automation_name,
                        "run_id": run_id,
                        "runID": run_id,
                        "source": "automation_v2",
                        "component": "automation_v2",
                        "status": if terminal { "failed" } else { "paused" },
                        "pause_reason": if terminal { Value::Null } else { json!("stale_no_provider_activity") },
                        "reason": updated_run.detail.clone().unwrap_or_else(|| detail.clone()),
                        "detail": updated_run.detail.clone().unwrap_or_else(|| detail.clone()),
                        "stale_node_ids": stale_node_ids,
                        "terminal_stale_node_ids": terminal_stale_node_ids,
                        "stale_after_ms": stale_after_ms,
                        "tenantContext": updated_run.tenant_context,
                    }),
                ));
                reaped += 1;
            }
        }
        reaped
    }
}
