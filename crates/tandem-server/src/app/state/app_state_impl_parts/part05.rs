const DEFAULT_STALE_AUTO_RESUME_WINDOW_MS: u64 = 20 * 60 * 1000;
const DEFAULT_STALE_AUTO_RESUME_MAX_ATTEMPTS: usize = 2;

fn approval_gate_stale_after_ms() -> u64 {
    std::env::var("TANDEM_APPROVAL_GATE_STALE_AFTER_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(24 * 60 * 60 * 1000)
}

fn gate_policy_state_u64(gate: &crate::AutomationPendingGate, key: &str) -> Option<u64> {
    gate.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("gate_policy_state"))
        .and_then(|state| state.get(key))
        .and_then(Value::as_u64)
}

fn gate_policy_reminder_due(
    gate: &crate::AutomationPendingGate,
    policy: &crate::AutomationGateExpiryPolicy,
    now: u64,
    expires_at_ms: u64,
) -> bool {
    let action = policy
        .on_expiry
        .unwrap_or(crate::AutomationGateExpiryAction::Cancel);
    if now >= expires_at_ms && action != crate::AutomationGateExpiryAction::Remind {
        return false;
    }

    let Some(remind_every_ms) = policy.remind_every_ms.filter(|value| *value > 0) else {
        return now >= expires_at_ms
            && action == crate::AutomationGateExpiryAction::Remind
            && gate_policy_state_u64(gate, "last_reminded_at_ms").is_none();
    };

    let last_reminded_at_ms =
        gate_policy_state_u64(gate, "last_reminded_at_ms").unwrap_or(gate.requested_at_ms);
    now.saturating_sub(last_reminded_at_ms) >= remind_every_ms
}

fn gate_policy_state_has(gate: &crate::AutomationPendingGate, key: &str) -> bool {
    gate.metadata
        .as_ref()
        .and_then(|metadata| metadata.get("gate_policy_state"))
        .and_then(|state| state.get(key))
        .is_some()
}

fn update_gate_policy_state(
    gate: &mut crate::AutomationPendingGate,
    updates: impl IntoIterator<Item = (&'static str, Value)>,
) {
    let mut metadata = match gate.metadata.take() {
        Some(Value::Object(map)) => map,
        Some(other) => {
            let mut map = serde_json::Map::new();
            map.insert("legacy_metadata".to_string(), other);
            map
        }
        None => serde_json::Map::new(),
    };
    let mut state = match metadata.remove("gate_policy_state") {
        Some(Value::Object(map)) => map,
        _ => serde_json::Map::new(),
    };
    for (key, value) in updates {
        state.insert(key.to_string(), value);
    }
    metadata.insert("gate_policy_state".to_string(), Value::Object(state));
    gate.metadata = Some(Value::Object(metadata));
}

fn stale_auto_resume_window_ms() -> u64 {
    std::env::var("TANDEM_STALE_AUTO_RESUME_WINDOW_MS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_STALE_AUTO_RESUME_WINDOW_MS)
}

fn latest_stale_reap_recorded_at_ms(run: &AutomationV2RunRecord) -> Option<u64> {
    run.checkpoint
        .lifecycle_history
        .iter()
        .rev()
        .find(|record| {
            record.event == "run_paused_stale_no_provider_activity"
                || record.stop_kind == Some(AutomationStopKind::StaleReaped)
        })
        .map(|record| record.recorded_at_ms)
}

fn stale_reap_is_within_auto_resume_window(
    now: u64,
    stale_reaped_at_ms: u64,
    auto_resume_window_ms: u64,
) -> bool {
    now.saturating_sub(stale_reaped_at_ms) <= auto_resume_window_ms
}

fn stale_auto_resume_max_attempts() -> usize {
    std::env::var("TANDEM_STALE_AUTO_RESUME_MAX_ATTEMPTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_STALE_AUTO_RESUME_MAX_ATTEMPTS)
}

fn stale_auto_resume_count_exceeds_cap(auto_resume_count: usize, max_attempts: usize) -> bool {
    auto_resume_count >= max_attempts
}

fn detail_node_id(detail: &str) -> Option<&str> {
    let (_, tail) = detail.split_once("node `")?;
    let (node_id, _) = tail.split_once('`')?;
    (!node_id.trim().is_empty()).then_some(node_id)
}

fn refresh_stale_running_detail(run: &mut AutomationV2RunRecord) {
    if run.status != AutomationRunStatus::Running {
        return;
    }
    let Some(detail) = run.detail.as_deref() else {
        return;
    };
    let Some(stale_node_id) = detail_node_id(detail) else {
        return;
    };
    if !run
        .checkpoint
        .completed_nodes
        .iter()
        .any(|node_id| node_id == stale_node_id)
    {
        return;
    }

    if let Some((node_id, attempt)) = run.checkpoint.pending_nodes.iter().find_map(|node_id| {
        if run
            .checkpoint
            .completed_nodes
            .iter()
            .any(|id| id == node_id)
            || run.checkpoint.blocked_nodes.iter().any(|id| id == node_id)
        {
            return None;
        }
        let attempt = run
            .checkpoint
            .node_attempts
            .get(node_id)
            .copied()
            .unwrap_or(0);
        (attempt > 0).then_some((node_id, attempt))
    }) {
        run.detail = Some(format!("running node `{node_id}` attempt {attempt}"));
    } else {
        run.detail = Some(format!("completed node `{stale_node_id}`; continuing"));
    }
}

#[cfg(test)]
mod stale_auto_resume_window_tests {
    use super::{
        refresh_stale_running_detail, stale_auto_resume_count_exceeds_cap,
        stale_reap_is_within_auto_resume_window, AutomationRunCheckpoint, AutomationRunStatus,
        AutomationV2RunRecord, TenantContext,
    };

    #[test]
    fn stale_auto_resume_window_allows_fresh_reaped_runs() {
        assert!(stale_reap_is_within_auto_resume_window(
            10_000, 9_500, 1_000,
        ));
    }

    #[test]
    fn stale_auto_resume_window_rejects_old_reaped_runs() {
        assert!(!stale_reap_is_within_auto_resume_window(
            10_000, 7_000, 1_000,
        ));
    }

    #[test]
    fn stale_auto_resume_count_respects_configured_cap() {
        assert!(!stale_auto_resume_count_exceeds_cap(0, 2));
        assert!(!stale_auto_resume_count_exceeds_cap(1, 2));
        assert!(stale_auto_resume_count_exceeds_cap(2, 2));
        assert!(stale_auto_resume_count_exceeds_cap(3, 2));
        assert!(!stale_auto_resume_count_exceeds_cap(2, 3));
    }

    #[test]
    fn stale_running_detail_moves_to_active_attempt() {
        let mut run = AutomationV2RunRecord {
            run_id: "run-1".to_string(),
            automation_id: "automation-1".to_string(),
            tenant_context: TenantContext::local_implicit(),
            trigger_type: "manual".to_string(),
            status: AutomationRunStatus::Running,
            created_at_ms: 1,
            updated_at_ms: 1,
            started_at_ms: Some(1),
            finished_at_ms: None,
            active_session_ids: Vec::new(),
            latest_session_id: None,
            active_instance_ids: Vec::new(),
            runtime_context: None,
            automation_snapshot: None,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            execution_claim: None,
            execution_claim_epoch: 0,
            pause_reason: None,
            resume_reason: None,
            detail: Some("retrying node `collect` after transient provider failure".to_string()),
            stop_kind: None,
            stop_reason: None,
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: vec!["collect".to_string()],
                pending_nodes: vec!["draft".to_string()],
                node_outputs: std::collections::HashMap::new(),
                node_attempts: [("draft".to_string(), 1)].into_iter().collect(),
                node_attempt_verdicts: std::collections::HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            total_tokens: 0,
            prompt_tokens: 0,
            completion_tokens: 0,
            estimated_cost_usd: 0.0,
            scheduler: None,
            trigger_reason: None,
            consumed_handoff_id: None,
            learning_summary: None,
            effective_execution_profile:
                crate::automation_v2::execution_profile::ExecutionProfile::Strict,
            requested_execution_profile: None,
        };

        refresh_stale_running_detail(&mut run);

        assert_eq!(
            run.detail.as_deref(),
            Some("running node `draft` attempt 1")
        );
    }
}

impl AppState {
    pub async fn mark_stale_awaiting_approval_runs(&self) -> usize {
        let now = now_ms();
        let stale_after_ms = approval_gate_stale_after_ms();
        let candidate_runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|run| run.status == AutomationRunStatus::AwaitingApproval)
            .filter(|run| run.checkpoint.awaiting_gate.is_some())
            .cloned()
            .collect::<Vec<_>>();
        let mut marked = 0usize;
        for run in candidate_runs {
            let Some(gate) = run.checkpoint.awaiting_gate.as_ref() else {
                continue;
            };
            if now.saturating_sub(gate.requested_at_ms) < stale_after_ms {
                continue;
            }
            let already_marked = gate
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("stale"))
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if already_marked {
                continue;
            }
            let gate_node_id = gate.node_id.clone();
            let requested_at_ms = gate.requested_at_ms;
            let detail = format!(
                "awaiting manual approval for gate `{}` for at least {}s; no automatic expiry action is configured",
                gate_node_id,
                stale_after_ms / 1000
            );
            if let Some(updated_run) = self
                .update_automation_v2_run(&run.run_id, |row| {
                    row.detail = Some(detail.clone());
                    if let Some(gate) = row.checkpoint.awaiting_gate.as_mut() {
                        let mut metadata = gate
                            .metadata
                            .take()
                            .and_then(|value| value.as_object().cloned())
                            .unwrap_or_default();
                        metadata.insert("stale".to_string(), json!(true));
                        metadata.insert(
                            "stale_policy".to_string(),
                            json!("manual_only_visible_status"),
                        );
                        metadata.insert("stale_after_ms".to_string(), json!(stale_after_ms));
                        metadata.insert("stale_marked_at_ms".to_string(), json!(now));
                        metadata.insert("requested_at_ms".to_string(), json!(requested_at_ms));
                        gate.metadata = Some(Value::Object(metadata));
                    }
                    automation::record_automation_lifecycle_event_with_metadata(
                        row,
                        "approval_gate_marked_stale",
                        Some(detail.clone()),
                        None,
                        Some(json!({
                            "node_id": gate_node_id,
                            "requested_at_ms": requested_at_ms,
                            "stale_after_ms": stale_after_ms,
                            "policy": "manual_only_visible_status",
                        })),
                    );
                })
                .await
            {
                self.append_internal_sweep_protected_audit_event(
                    "automation_v2.internal_sweep.approval_gate_marked_stale",
                    &updated_run,
                    "mark_stale_awaiting_approval_runs",
                    "marked_stale",
                    Some(detail),
                    json!({
                        "node_id": gate_node_id,
                        "requested_at_ms": requested_at_ms,
                        "stale_after_ms": stale_after_ms,
                        "policy": "manual_only_visible_status",
                    }),
                )
                .await;
                marked += 1;
            }
        }
        marked
    }

    pub async fn process_awaiting_approval_gate_policies(&self) -> usize {
        let now = now_ms();
        let candidate_runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|run| run.status == AutomationRunStatus::AwaitingApproval)
            .filter(|run| run.checkpoint.awaiting_gate.is_some())
            .cloned()
            .collect::<Vec<_>>();

        let mut actions = 0usize;
        for run in candidate_runs {
            let Some(gate) = run.checkpoint.awaiting_gate.as_ref().cloned() else {
                continue;
            };
            let Some(policy) =
                automation::effective_automation_gate_expiry_policy(&gate)
            else {
                continue;
            };
            let Some(expires_at_ms) = automation::automation_gate_expires_at_ms(&gate) else {
                continue;
            };

            let action = policy
                .on_expiry
                .unwrap_or(crate::AutomationGateExpiryAction::Cancel);
            if now >= expires_at_ms {
                match action {
                    crate::AutomationGateExpiryAction::Cancel => {
                        if self
                            .expire_awaiting_approval_gate(&run, &gate, &policy, expires_at_ms)
                            .await
                        {
                            actions += 1;
                        }
                    }
                    crate::AutomationGateExpiryAction::Escalate => {
                        if !gate_policy_state_has(&gate, "escalated_at_ms")
                            && self
                                .escalate_awaiting_approval_gate(
                                    &run,
                                    &gate,
                                    &policy,
                                    expires_at_ms,
                                )
                                .await
                        {
                            actions += 1;
                        }
                    }
                    crate::AutomationGateExpiryAction::Remind => {
                        if gate_policy_reminder_due(&gate, &policy, now, expires_at_ms)
                            && self
                                .record_awaiting_approval_gate_reminder(
                                    &run,
                                    &gate,
                                    &policy,
                                    expires_at_ms,
                                    true,
                                )
                                .await
                        {
                            actions += 1;
                        }
                    }
                }
            } else if gate_policy_reminder_due(&gate, &policy, now, expires_at_ms)
                && self
                    .record_awaiting_approval_gate_reminder(
                        &run,
                        &gate,
                        &policy,
                        expires_at_ms,
                        false,
                    )
                    .await
            {
                actions += 1;
            }
        }
        actions
    }

    async fn expire_awaiting_approval_gate(
        &self,
        run: &AutomationV2RunRecord,
        gate: &crate::AutomationPendingGate,
        policy: &crate::AutomationGateExpiryPolicy,
        expires_at_ms: u64,
    ) -> bool {
        let reason = format!(
            "approval gate `{}` expired before a decision was recorded",
            gate.node_id
        );
        let mut applied = false;
        let updated = self
            .update_automation_v2_run(&run.run_id, |row| {
                match automation::apply_automation_gate_expiry(
                    row,
                    gate,
                    Some(reason.clone()),
                    expires_at_ms,
                    policy,
                ) {
                    automation::AutomationGateDecisionOutcome::Applied => {
                        applied = true;
                    }
                    automation::AutomationGateDecisionOutcome::AlreadyDecided(_) => {}
                }
            })
            .await;
        if !applied {
            return false;
        }
        if let Some(updated_run) = updated {
            self.append_internal_sweep_protected_audit_event(
                "automation_v2.internal_sweep.approval_gate_expired",
                &updated_run,
                "process_awaiting_approval_gate_policies",
                "expired_cancelled",
                Some(reason.clone()),
                json!({
                    "node_id": gate.node_id,
                    "expires_at_ms": expires_at_ms,
                    "expiry_policy": policy,
                }),
            )
            .await;
            self.event_bus.publish(tandem_types::EngineEvent::new(
                "approval.gate.expired",
                json!({
                    "run_id": updated_run.run_id,
                    "automation_id": updated_run.automation_id,
                    "node_id": gate.node_id,
                    "decision": "expired",
                    "expires_at_ms": expires_at_ms,
                    "tenantContext": updated_run.tenant_context,
                }),
            ));
        }
        true
    }

    async fn escalate_awaiting_approval_gate(
        &self,
        run: &AutomationV2RunRecord,
        gate: &crate::AutomationPendingGate,
        policy: &crate::AutomationGateExpiryPolicy,
        expires_at_ms: u64,
    ) -> bool {
        let now = now_ms();
        let escalate_to = policy
            .escalate_to
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or("unassigned_escalation_principal")
            .to_string();
        let detail = format!(
            "approval gate `{}` expired and was escalated to {}",
            gate.node_id, escalate_to
        );
        let mut applied = false;
        let updated = self
            .update_automation_v2_run(&run.run_id, |row| {
                if row.status != AutomationRunStatus::AwaitingApproval {
                    return;
                }
                let row_id = row.run_id.clone();
                let Some(row_gate) = row.checkpoint.awaiting_gate.as_mut() else {
                    return;
                };
                if row_gate.node_id != gate.node_id
                    || gate_policy_state_has(row_gate, "escalated_at_ms")
                {
                    return;
                }
                let reminder_count =
                    gate_policy_state_u64(row_gate, "reminder_count").unwrap_or(0) + 1;
                update_gate_policy_state(
                    row_gate,
                    [
                        ("escalated_at_ms", json!(now)),
                        ("escalated_to", json!(escalate_to.clone())),
                        ("expires_at_ms", json!(expires_at_ms)),
                        ("reminder_count", json!(reminder_count)),
                        (
                            "notification_key",
                            json!(format!(
                                "automation_v2:{}:{}:escalated:{}",
                                row_id, gate.node_id, reminder_count
                            )),
                        ),
                    ],
                );
                row.detail = Some(detail.clone());
                automation::record_automation_lifecycle_event_with_metadata(
                    row,
                    "approval_gate_escalated",
                    Some(detail.clone()),
                    None,
                    Some(json!({
                        "node_id": gate.node_id,
                        "expires_at_ms": expires_at_ms,
                        "escalated_to": escalate_to.clone(),
                        "expiry_policy": policy,
                    })),
                );
                applied = true;
            })
            .await;
        if !applied {
            return false;
        }
        if let Some(updated_run) = updated {
            self.append_internal_sweep_protected_audit_event(
                "automation_v2.internal_sweep.approval_gate_escalated",
                &updated_run,
                "process_awaiting_approval_gate_policies",
                "expired_escalated",
                Some(detail.clone()),
                json!({
                    "node_id": gate.node_id,
                    "expires_at_ms": expires_at_ms,
                    "escalated_to": escalate_to,
                    "expiry_policy": policy,
                }),
            )
            .await;
            self.event_bus.publish(tandem_types::EngineEvent::new(
                "approval.gate.escalated",
                json!({
                    "run_id": updated_run.run_id,
                    "automation_id": updated_run.automation_id,
                    "node_id": gate.node_id,
                    "expires_at_ms": expires_at_ms,
                    "escalated_to": escalate_to,
                    "tenantContext": updated_run.tenant_context,
                }),
            ));
        }
        true
    }

    async fn record_awaiting_approval_gate_reminder(
        &self,
        run: &AutomationV2RunRecord,
        gate: &crate::AutomationPendingGate,
        policy: &crate::AutomationGateExpiryPolicy,
        expires_at_ms: u64,
        expired: bool,
    ) -> bool {
        let now = now_ms();
        let detail = if expired {
            format!(
                "approval gate `{}` is expired and still awaiting a decision",
                gate.node_id
            )
        } else {
            format!(
                "approval gate `{}` is still awaiting a decision",
                gate.node_id
            )
        };
        let mut applied = false;
        let mut reminder_count = 0u64;
        let updated = self
            .update_automation_v2_run(&run.run_id, |row| {
                if row.status != AutomationRunStatus::AwaitingApproval {
                    return;
                }
                let row_id = row.run_id.clone();
                let Some(row_gate) = row.checkpoint.awaiting_gate.as_mut() else {
                    return;
                };
                if row_gate.node_id != gate.node_id {
                    return;
                }
                reminder_count = gate_policy_state_u64(row_gate, "reminder_count").unwrap_or(0) + 1;
                update_gate_policy_state(
                    row_gate,
                    [
                        ("last_reminded_at_ms", json!(now)),
                        ("reminder_count", json!(reminder_count)),
                        ("expires_at_ms", json!(expires_at_ms)),
                        ("expired_reminder", json!(expired)),
                        (
                            "notification_key",
                            json!(format!(
                                "automation_v2:{}:{}:reminder:{}",
                                row_id, gate.node_id, reminder_count
                            )),
                        ),
                    ],
                );
                row.detail = Some(detail.clone());
                automation::record_automation_lifecycle_event_with_metadata(
                    row,
                    "approval_gate_reminder_due",
                    Some(detail.clone()),
                    None,
                    Some(json!({
                        "node_id": gate.node_id,
                        "expires_at_ms": expires_at_ms,
                        "expired": expired,
                        "reminder_count": reminder_count,
                        "expiry_policy": policy,
                    })),
                );
                applied = true;
            })
            .await;
        if !applied {
            return false;
        }
        if let Some(updated_run) = updated {
            self.append_internal_sweep_protected_audit_event(
                "automation_v2.internal_sweep.approval_gate_reminder_due",
                &updated_run,
                "process_awaiting_approval_gate_policies",
                if expired {
                    "expired_reminder_due"
                } else {
                    "reminder_due"
                },
                Some(detail.clone()),
                json!({
                    "node_id": gate.node_id,
                    "expires_at_ms": expires_at_ms,
                    "expired": expired,
                    "reminder_count": reminder_count,
                    "expiry_policy": policy,
                }),
            )
            .await;
            self.event_bus.publish(tandem_types::EngineEvent::new(
                "approval.gate.reminder_due",
                json!({
                    "run_id": updated_run.run_id,
                    "automation_id": updated_run.automation_id,
                    "node_id": gate.node_id,
                    "expires_at_ms": expires_at_ms,
                    "expired": expired,
                    "reminder_count": reminder_count,
                    "tenantContext": updated_run.tenant_context,
                }),
            ));
        }
        true
    }

    pub async fn auto_resume_stale_reaped_runs(&self) -> usize {
        // Stale reaping is provider/session infrastructure failure, not proof
        // that the workflow contract failed. Keep the retry bounded so a truly
        // wedged provider cannot loop forever, but default to recovery while
        // the node still has attempt budget.
        if std::env::var_os("TANDEM_DISABLE_STALE_AUTO_RESUME").is_some() {
            return 0;
        }

        let candidate_runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|run| run.status == AutomationRunStatus::Paused)
            .filter(|run| run.stop_kind == Some(AutomationStopKind::StaleReaped))
            .cloned()
            .collect::<Vec<_>>();
        let mut resumed = 0usize;
        let now = now_ms();
        let auto_resume_window_ms = stale_auto_resume_window_ms();
        let auto_resume_max_attempts = stale_auto_resume_max_attempts();
        for run in candidate_runs {
            let Some(stale_reaped_at_ms) = latest_stale_reap_recorded_at_ms(&run) else {
                continue;
            };
            if !stale_reap_is_within_auto_resume_window(
                now,
                stale_reaped_at_ms,
                auto_resume_window_ms,
            ) {
                continue;
            }
            let auto_resume_count = run
                .checkpoint
                .lifecycle_history
                .iter()
                .filter(|event| event.event == "run_auto_resumed")
                .count();
            if stale_auto_resume_count_exceeds_cap(auto_resume_count, auto_resume_max_attempts) {
                continue;
            }
            let automation = self.get_automation_v2(&run.automation_id).await;
            let automation = match automation.or(run.automation_snapshot.clone()) {
                Some(a) => a,
                None => continue,
            };
            let has_repairable_nodes = automation.flow.nodes.iter().any(|node| {
                if run.checkpoint.completed_nodes.contains(&node.node_id) {
                    return false;
                }
                if run.checkpoint.node_outputs.contains_key(&node.node_id) {
                    let status = run.checkpoint.node_outputs[&node.node_id]
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_ascii_lowercase();
                    if status != "needs_repair" {
                        return false;
                    }
                } else {
                    return false;
                }
                let attempts = run
                    .checkpoint
                    .node_attempts
                    .get(&node.node_id)
                    .copied()
                    .unwrap_or(0);
                let max_attempts = automation_node_max_attempts(node);
                attempts < max_attempts
            });
            if !has_repairable_nodes {
                continue;
            }
            // GOV-B6a: do not resurrect a stale-reaped run whose agent is now
            // spend-paused without an approved override; leave it paused for the
            // guardrail-override resume path instead.
            if self.run_launch_blocked_by_spend_pause(&automation).await {
                continue;
            }
            if let Some(updated_run) = self
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Queued;
                    row.pause_reason = None;
                    row.detail = None;
                    row.stop_kind = None;
                    row.stop_reason = None;
                    automation::record_automation_lifecycle_event_with_metadata(
                        row,
                        "run_auto_resumed",
                        Some("auto_resume_after_stale_reap".to_string()),
                        None,
                        Some(json!({
                            "auto_resume_window_ms": auto_resume_window_ms,
                            "stale_reaped_at_ms": stale_reaped_at_ms,
                        })),
                    );
                })
                .await
            {
                self.append_internal_sweep_protected_audit_event(
                    "automation_v2.internal_sweep.auto_resumed_stale_reaped_run",
                    &updated_run,
                    "auto_resume_stale_reaped_runs",
                    "queued",
                    Some("auto_resume_after_stale_reap".to_string()),
                    json!({
                        "auto_resume_window_ms": auto_resume_window_ms,
                        "auto_resume_count_before": auto_resume_count,
                        "auto_resume_max_attempts": auto_resume_max_attempts,
                        "stale_reaped_at_ms": stale_reaped_at_ms,
                    }),
                )
                .await;
                resumed += 1;
            }
        }
        resumed += self.auto_resume_guardrail_stopped_runs().await;
        resumed
    }

    async fn auto_resume_guardrail_stopped_runs(&self) -> usize {
        let candidate_runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|run| run.status == AutomationRunStatus::Paused)
            .filter(|run| run.stop_kind == Some(AutomationStopKind::GuardrailStopped))
            .cloned()
            .collect::<Vec<_>>();
        let mut resumed = 0usize;
        for run in candidate_runs {
            let automation = self.get_automation_v2(&run.automation_id).await;
            let automation = match automation.or(run.automation_snapshot.clone()) {
                Some(a) => a,
                None => continue,
            };
            let agent_ids = std::iter::once(automation.creator_id.clone())
                .chain(automation.agents.iter().map(|agent| agent.agent_id.clone()))
                .filter(|agent_id| !agent_id.trim().is_empty())
                .collect::<std::collections::BTreeSet<_>>();
            if agent_ids.is_empty() {
                continue;
            }
            let tenant_context = automation.tenant_context();
            let mut has_approved_override = false;
            for agent_id in &agent_ids {
                if self
                    .tenant_agent_has_quota_override(&tenant_context, agent_id)
                    .await
                {
                    has_approved_override = true;
                    break;
                }
            }
            if !has_approved_override {
                continue;
            }
            if let Some(updated_run) = self
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Queued;
                    row.pause_reason = None;
                    row.detail = None;
                    row.stop_kind = None;
                    row.stop_reason = None;
                    automation::record_automation_lifecycle_event_with_metadata(
                        row,
                        "run_auto_resumed",
                        Some("auto_resume_after_guardrail_override".to_string()),
                        None,
                        Some(json!({
                            "agent_ids": agent_ids.iter().cloned().collect::<Vec<_>>(),
                            "stop_kind": "guardrail_stopped",
                        })),
                    );
                })
                .await
            {
                self.append_internal_sweep_protected_audit_event(
                    "automation_v2.internal_sweep.auto_resumed_guardrail_stopped_run",
                    &updated_run,
                    "auto_resume_guardrail_stopped_runs",
                    "queued",
                    Some("auto_resume_after_guardrail_override".to_string()),
                    json!({
                        "agent_ids": agent_ids.iter().cloned().collect::<Vec<_>>(),
                        "stop_kind": "guardrail_stopped",
                    }),
                )
                .await;
                resumed += 1;
            }
        }
        resumed
    }

    /// GOV-B6a: a queued or stale run must not transition into execution while any
    /// of its agents is spend-paused without an approved quota override *for that
    /// agent*. Quota overrides are agent-targeted, so the check is per-agent: a run
    /// is held if there exists a spend-paused agent that lacks its own override (an
    /// override on a different agent does not unblock a still-paused one). A held run
    /// is `Paused + GuardrailStopped` and is picked back up by
    /// `auto_resume_guardrail_stopped_runs` once the override lands. No-op in the
    /// OSS/local engine, where `spend_paused_agents` is always empty.
    async fn run_launch_blocked_by_spend_pause(
        &self,
        automation: &crate::automation_v2::types::AutomationV2Spec,
    ) -> bool {
        let agent_ids = std::iter::once(automation.creator_id.clone())
            .chain(automation.agents.iter().map(|agent| agent.agent_id.clone()))
            .filter(|agent_id| !agent_id.trim().is_empty())
            .collect::<std::collections::BTreeSet<_>>();
        if agent_ids.is_empty() {
            return false;
        }
        let tenant_context = automation.tenant_context();
        for agent_id in &agent_ids {
            if self
                .tenant_agent_spend_paused_without_quota_override(&tenant_context, agent_id)
                .await
            {
                return true;
            }
        }
        false
    }

    pub fn is_automation_scheduler_stopping(&self) -> bool {
        self.automation_scheduler_stopping.load(Ordering::Relaxed)
    }

    pub fn set_automation_scheduler_stopping(&self, stopping: bool) {
        self.automation_scheduler_stopping
            .store(stopping, Ordering::Relaxed);
    }

    pub async fn fail_running_automation_runs_for_shutdown(&self) -> usize {
        let run_ids = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|run| matches!(run.status, AutomationRunStatus::Running))
            .map(|run| run.run_id.clone())
            .collect::<Vec<_>>();
        let mut failed = 0usize;
        for run_id in run_ids {
            let detail = "automation run stopped during server shutdown".to_string();
            if let Some(updated_run) = self
                .update_automation_v2_run(&run_id, |row| {
                    row.status = AutomationRunStatus::Failed;
                    row.detail = Some(detail.clone());
                    row.stop_kind = Some(AutomationStopKind::Shutdown);
                    row.stop_reason = Some(detail.clone());
                    automation::record_automation_lifecycle_event(
                        row,
                        "run_failed_shutdown",
                        Some(detail.clone()),
                        Some(AutomationStopKind::Shutdown),
                    );
                })
                .await
            {
                self.append_internal_sweep_protected_audit_event(
                    "automation_v2.internal_sweep.shutdown_failed_run",
                    &updated_run,
                    "fail_running_automation_runs_for_shutdown",
                    "failed_running_run",
                    Some(detail),
                    json!({ "previous_status": "running" }),
                )
                .await;
                failed += 1;
            }
        }
        failed
    }

    pub async fn update_automation_v2_run(
        &self,
        run_id: &str,
        update: impl FnOnce(&mut AutomationV2RunRecord),
    ) -> Option<AutomationV2RunRecord> {
        let mut guard = self.automation_v2_runs.write().await;
        let check_time_ms = crate::now_ms();
        if !guard.contains_key(run_id) {
            drop(guard);
            let history =
                load_automation_v2_run_history_shard(&self.automation_v2_runs_path, run_id).await?;
            guard = self.automation_v2_runs.write().await;
            // TOCTOU fix: check if the entry was modified while we were loading from disk.
            // If another thread inserted and updated the run after we dropped the lock,
            // that thread's changes take precedence (or_insert won't overwrite).
            // Verify the entry wasn't updated by a concurrent thread during our load.
            if let Some(existing) = guard.get(run_id) {
                if existing.updated_at_ms > check_time_ms {
                    // Entry was updated by another thread while we were loading.
                    // Our loaded copy is stale. Skip insertion and let the caller
                    // see the concurrent modification via the updated in-memory version.
                    // The update closure below will apply to the concurrent version.
                }
            } else {
                guard.insert(run_id.to_string(), history);
            }
        }
        let run = guard.get_mut(run_id)?;
        let previous_status = run.status.clone();
        update(run);
        refresh_stale_running_detail(run);
        if run.status != AutomationRunStatus::Queued {
            run.scheduler = None;
        }
        if run.status != AutomationRunStatus::Running {
            run.execution_claim = None;
        }
        run.updated_at_ms = now_ms();
        if matches!(
            run.status,
            AutomationRunStatus::Completed
                | AutomationRunStatus::Blocked
                | AutomationRunStatus::Failed
                | AutomationRunStatus::Cancelled
        ) {
            run.finished_at_ms.get_or_insert_with(now_ms);
        }
        let out = run.clone();
        drop(guard);
        self.sync_automation_scheduler_for_run_transition(previous_status.clone(), &out)
            .await;
        let _ = self.persist_automation_v2_runs().await;
        let _ = self.persist_automation_v2_run_status_json(&out).await;
        self.project_automation_v2_stateful_boundaries_or_warn(&out)
            .await;
        if matches!(
            out.status,
            AutomationRunStatus::Completed
                | AutomationRunStatus::Blocked
                | AutomationRunStatus::Failed
                | AutomationRunStatus::Cancelled
        ) {
            let _ = self
                .finalize_terminal_automation_v2_run_learning(&out)
                .await;
            if !Self::automation_run_is_terminal(&previous_status) {
                let _ = self
                    .record_automation_review_progress(
                        &out.automation_id,
                        crate::automation_v2::governance::AutomationLifecycleReviewKind::RunDrift,
                        Some(out.run_id.clone()),
                        out.detail.clone().or_else(|| out.stop_reason.clone()),
                    )
                    .await;
            }
        }
        Some(out)
    }

    async fn persist_automation_v2_run_status_json(
        &self,
        run: &AutomationV2RunRecord,
    ) -> anyhow::Result<()> {
        let default_workspace = self.workspace_index.snapshot().await.root.clone();
        let automation = run.automation_snapshot.as_ref();
        let workspace_root = if let Some(ref a) = automation {
            if let Some(ref wr) = a.workspace_root {
                if !wr.trim().is_empty() {
                    wr.trim().to_string()
                } else {
                    a.metadata
                        .as_ref()
                        .and_then(|m| m.get("workspace_root"))
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| default_workspace.clone())
                }
            } else {
                a.metadata
                    .as_ref()
                    .and_then(|m| m.get("workspace_root"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
                    .unwrap_or_else(|| default_workspace.clone())
            }
        } else {
            default_workspace
        };
        let run_dir = PathBuf::from(&workspace_root)
            .join(".tandem")
            .join("runs")
            .join(&run.run_id);
        let status_path = run_dir.join("status.json");
        let status_json = json!({
            "run_id": run.run_id,
            "automation_id": run.automation_id,
            "status": run.status,
            "detail": run.detail,
            "completed_nodes": run.checkpoint.completed_nodes,
            "pending_nodes": run.checkpoint.pending_nodes,
            "blocked_nodes": run.checkpoint.blocked_nodes,
            "node_attempts": run.checkpoint.node_attempts,
            "last_failure": run.checkpoint.last_failure,
            "learning_summary": run.learning_summary,
            "updated_at_ms": run.updated_at_ms,
        });
        fs::create_dir_all(&run_dir).await?;
        fs::write(&status_path, serde_json::to_string_pretty(&status_json)?).await?;
        Ok(())
    }

    pub async fn set_automation_v2_run_scheduler_metadata(
        &self,
        run_id: &str,
        meta: automation::SchedulerMetadata,
    ) -> Option<AutomationV2RunRecord> {
        self.update_automation_v2_run(run_id, |row| {
            row.scheduler = Some(meta);
        })
        .await
    }

    pub async fn clear_automation_v2_run_scheduler_metadata(
        &self,
        run_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        self.update_automation_v2_run(run_id, |row| {
            row.scheduler = None;
        })
        .await
    }

    pub async fn add_automation_v2_session(
        &self,
        run_id: &str,
        session_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        let updated = self
            .update_automation_v2_run(run_id, |row| {
                if !row.active_session_ids.iter().any(|id| id == session_id) {
                    row.active_session_ids.push(session_id.to_string());
                }
                row.latest_session_id = Some(session_id.to_string());
            })
            .await;
        self.automation_v2_session_runs
            .write()
            .await
            .insert(session_id.to_string(), run_id.to_string());
        updated
    }

    pub async fn set_automation_v2_session_mcp_servers(
        &self,
        session_id: &str,
        servers: Vec<String>,
    ) {
        if servers.is_empty() {
            self.automation_v2_session_mcp_servers
                .write()
                .await
                .remove(session_id);
        } else {
            self.automation_v2_session_mcp_servers
                .write()
                .await
                .insert(session_id.to_string(), servers);
        }
    }

    pub async fn clear_automation_v2_session_mcp_servers(&self, session_id: &str) {
        self.automation_v2_session_mcp_servers
            .write()
            .await
            .remove(session_id);
    }

    pub async fn clear_automation_v2_session(
        &self,
        run_id: &str,
        session_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        self.automation_v2_session_runs
            .write()
            .await
            .remove(session_id);
        self.update_automation_v2_run(run_id, |row| {
            row.active_session_ids.retain(|id| id != session_id);
        })
        .await
    }

    pub async fn forget_automation_v2_sessions(&self, session_ids: &[String]) {
        let mut guard = self.automation_v2_session_runs.write().await;
        for session_id in session_ids {
            guard.remove(session_id);
        }
        let mut mcp_guard = self.automation_v2_session_mcp_servers.write().await;
        for session_id in session_ids {
            mcp_guard.remove(session_id);
        }
    }

    pub async fn add_automation_v2_instance(
        &self,
        run_id: &str,
        instance_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        self.update_automation_v2_run(run_id, |row| {
            if !row.active_instance_ids.iter().any(|id| id == instance_id) {
                row.active_instance_ids.push(instance_id.to_string());
            }
        })
        .await
    }

    pub async fn clear_automation_v2_instance(
        &self,
        run_id: &str,
        instance_id: &str,
    ) -> Option<AutomationV2RunRecord> {
        self.update_automation_v2_run(run_id, |row| {
            row.active_instance_ids.retain(|id| id != instance_id);
        })
        .await
    }
}
