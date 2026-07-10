use serde_json::{json, Value};
use tandem_automation::{
    AutomationFlowNode, AutomationV2RunRecord, AutomationWaitSpec, OrchestrationValueBinding,
    WaitTimeoutAction, WaitTimeoutPolicy, WebhookCorrelationField,
};

use super::{
    automation, AppState, AutomationRunStatus, AutomationStopKind,
    AutomationWebhookWaitReplayOutcome,
};
use crate::stateful_runtime::{
    append_stateful_run_event_once_with_next_seq, begin_claimed_stateful_wait_wake_completion,
    claim_stateful_wait_for_resolution, finish_claimed_stateful_wait_completion,
    list_stateful_waits, stateful_run_from_automation_v2, stateful_webhook_wait_metadata,
    upsert_stateful_wait, StatefulRunEventRecord, StatefulRuntimeStoragePaths, StatefulWaitKind,
    StatefulWaitQuery, StatefulWaitRecord, StatefulWaitStatus, StatefulWaitTimeoutAction,
    StatefulWaitTimeoutPolicy, StatefulWebhookWaitMatch,
};
use crate::util::time::now_ms;

const AUTOMATION_WAIT_NODE_METADATA_KEY: &str = "automation_v2_wait_node";
const MAX_WAIT_BINDING_STRING_BYTES: usize = 512;
const MAX_EXTERNAL_WAIT_RESOLUTION_BYTES: usize = 16 * 1024;
const EXTERNAL_WAIT_RESOLUTION_LEASE_MS: u64 = 60_000;

impl AppState {
    pub(crate) async fn park_first_runnable_automation_v2_wait(
        &self,
        run: &AutomationV2RunRecord,
        runnable: &[AutomationFlowNode],
    ) -> bool {
        let Some(wait_node) = runnable
            .iter()
            .find(|node| node.is_explicit_wait_node() && !super::is_automation_approval_node(node))
        else {
            return false;
        };
        if let Err(error) = self.register_automation_v2_wait_node(run, wait_node).await {
            let detail = format!(
                "failed to register wait node `{}`: {error}",
                wait_node.node_id
            );
            let _ = self
                .update_automation_v2_run(&run.run_id, |row| {
                    row.status = AutomationRunStatus::Failed;
                    row.detail = Some(detail.clone());
                    row.stop_kind = Some(AutomationStopKind::GuardrailStopped);
                    row.stop_reason = Some(detail.clone());
                    automation::record_automation_lifecycle_event_with_metadata(
                        row,
                        "wait_node_registration_failed",
                        Some(detail.clone()),
                        Some(AutomationStopKind::GuardrailStopped),
                        Some(json!({ "node_id": &wait_node.node_id })),
                    );
                })
                .await;
        }
        true
    }

    /// Repair the crash window between durably parking a run and durably
    /// inserting its wait record. Only runs carrying the registration-started
    /// lifecycle marker are eligible, so an operator-paused run is never
    /// converted into a wait implicitly.
    pub(super) async fn recover_missing_automation_v2_wait_registrations(&self) -> usize {
        let runs = self
            .automation_v2_runs
            .read()
            .await
            .values()
            .filter(|run| run.status == AutomationRunStatus::Paused)
            .cloned()
            .collect::<Vec<_>>();
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&self.runtime_events_path);
        let mut recovered = 0usize;

        for run in runs {
            let Some(node_id) = run
                .checkpoint
                .lifecycle_history
                .iter()
                .rev()
                .find(|event| event.event == "wait_node_registration_started")
                .and_then(|event| event.metadata.as_ref())
                .and_then(|metadata| metadata.get("node_id"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            else {
                continue;
            };
            if run
                .checkpoint
                .completed_nodes
                .iter()
                .any(|value| value == node_id)
            {
                continue;
            }
            let wait_id = format!("automation-v2:{}:{node_id}:wait", run.run_id);
            if list_stateful_waits(
                &paths.waits_path,
                &run.tenant_context,
                StatefulWaitQuery {
                    run_id: Some(&run.run_id),
                    wait_kind: None,
                    status: None,
                    limit: None,
                },
            )
            .iter()
            .any(|wait| wait.wait_id == wait_id)
            {
                continue;
            }

            let Ok(automation) = self.automation_definition_for_restart_recovery(&run).await else {
                continue;
            };
            let Some(node) = automation
                .flow
                .nodes
                .iter()
                .find(|node| node.node_id == node_id)
            else {
                continue;
            };
            if node
                .wait
                .as_ref()
                .is_none_or(|wait| matches!(wait, AutomationWaitSpec::Approval { .. }))
            {
                continue;
            }
            match self.register_automation_v2_wait_node(&run, node).await {
                Ok(_) => recovered += 1,
                Err(error) => tracing::warn!(
                    run_id = %run.run_id,
                    node_id,
                    error = %error,
                    "failed to recover missing Automation V2 wait registration"
                ),
            }
        }
        recovered
    }

    /// Persist an eligible Automation V2 wait node and park the same run.
    /// Webhook registration replays an accepted early delivery through the
    /// existing exactly-once claim path before returning.
    pub(crate) async fn register_automation_v2_wait_node(
        &self,
        run: &AutomationV2RunRecord,
        node: &AutomationFlowNode,
    ) -> anyhow::Result<StatefulWaitRecord> {
        let wait = node.wait.as_ref().ok_or_else(|| {
            anyhow::anyhow!("node `{}` is not an explicit wait node", node.node_id)
        })?;
        if matches!(wait, AutomationWaitSpec::Approval { .. }) {
            anyhow::bail!("approval waits must use the approval-gate projection path");
        }

        let now = now_ms();
        let record = automation_v2_wait_record(run, node, wait, now)?;
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&self.runtime_events_path);
        let existing = list_stateful_waits(
            &paths.waits_path,
            &run.tenant_context,
            StatefulWaitQuery {
                run_id: Some(&run.run_id),
                wait_kind: Some(record.wait_kind.clone()),
                status: None,
                limit: None,
            },
        )
        .into_iter()
        .find(|candidate| candidate.wait_id == record.wait_id);

        let wait_id = record.wait_id.clone();
        let wait_kind = record.wait_kind.clone();
        let detail = format!(
            "waiting at Automation V2 node `{}` ({wait_kind:?})",
            node.node_id
        );
        self.update_automation_v2_run(&run.run_id, |row| {
            if row.checkpoint.completed_nodes.contains(&node.node_id) {
                return;
            }
            row.status = AutomationRunStatus::Paused;
            row.detail = Some(detail.clone());
            row.pause_reason = Some(detail.clone());
            row.resume_reason = None;
            row.stop_kind = None;
            row.stop_reason = None;
            row.active_session_ids.clear();
            row.latest_session_id = None;
            row.active_instance_ids.clear();
            automation::record_automation_lifecycle_event_with_metadata(
                row,
                "wait_node_registration_started",
                Some(detail.clone()),
                None,
                Some(json!({
                    "node_id": &node.node_id,
                    "wait_id": &wait_id,
                    "wait_kind": wait_kind_name(&wait_kind),
                })),
            );
        })
        .await
        .ok_or_else(|| anyhow::anyhow!("automation run `{}` not found", run.run_id))?;

        if let Some(existing) = existing {
            if matches!(
                existing.status,
                StatefulWaitStatus::Waiting | StatefulWaitStatus::Claimed
            ) {
                return Ok(existing);
            }
            if existing.status == StatefulWaitStatus::Woken {
                let _ = self
                    .requeue_automation_v2_run_from_stateful_wait_wake(
                        &existing.run_id,
                        &existing.wait_id,
                        "stateful_wait_already_woken_on_registration",
                        existing.event_seq.unwrap_or_default(),
                        format!(
                            "stateful wait `{}` was already complete; run queued for resume",
                            existing.wait_id
                        ),
                        json!({ "recovered_existing_wait": true }),
                    )
                    .await;
            }
            return Ok(existing);
        }

        let registered = if record.wait_kind == StatefulWaitKind::Webhook {
            match self
                .register_stateful_webhook_wait_and_replay_pending(record)
                .await?
            {
                AutomationWebhookWaitReplayOutcome::Registered(wait) => wait,
                AutomationWebhookWaitReplayOutcome::Woken { wait, .. } => wait,
            }
        } else {
            upsert_stateful_wait(&paths.waits_path, record).await?
        };

        let registered_wait_id = registered.wait_id.clone();
        let registered_wait_kind = registered.wait_kind.clone();
        let _ = self
            .update_automation_v2_run(&run.run_id, |row| {
                automation::record_automation_lifecycle_event_with_metadata(
                    row,
                    "wait_node_registered",
                    Some(format!(
                        "wait `{registered_wait_id}` registered for node `{}`",
                        node.node_id
                    )),
                    None,
                    Some(json!({
                        "node_id": &node.node_id,
                        "wait_id": &registered_wait_id,
                        "wait_kind": wait_kind_name(&registered_wait_kind),
                        "wait_status": &registered.status,
                    })),
                );
            })
            .await;
        Ok(registered)
    }

    pub(crate) async fn resolve_automation_v2_external_wait(
        &self,
        tenant: &tandem_types::TenantContext,
        wait_id: &str,
        idempotency_key: &str,
        resolution: Value,
    ) -> anyhow::Result<Option<StatefulWaitRecord>> {
        let idempotency_key = idempotency_key.trim();
        if idempotency_key.is_empty() || idempotency_key.len() > MAX_WAIT_BINDING_STRING_BYTES {
            anyhow::bail!("external wait resolution requires a bounded idempotency key");
        }
        if serde_json::to_vec(&resolution)?.len() > MAX_EXTERNAL_WAIT_RESOLUTION_BYTES {
            anyhow::bail!(
                "external wait resolution exceeds {MAX_EXTERNAL_WAIT_RESOLUTION_BYTES} bytes"
            );
        }

        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&self.runtime_events_path);
        let completion_key = format!("external:{wait_id}:{idempotency_key}");
        let existing = list_stateful_waits(
            &paths.waits_path,
            tenant,
            StatefulWaitQuery {
                run_id: None,
                wait_kind: Some(StatefulWaitKind::ExternalCondition),
                status: None,
                limit: None,
            },
        )
        .into_iter()
        .find(|wait| wait.wait_id == wait_id);
        if let Some(existing) = existing.as_ref() {
            if existing.status == StatefulWaitStatus::Woken {
                return Ok(
                    (existing.wake_idempotency_key.as_deref() == Some(&completion_key))
                        .then_some(existing.clone()),
                );
            }
            if let Some(schema) = existing
                .metadata
                .as_ref()
                .and_then(|metadata| metadata.get("details"))
                .and_then(|details| details.get("payload_schema"))
                .filter(|schema| !schema.is_null())
            {
                if let Some(issue) =
                    automation::automation_wait_payload_schema_validation_issue(schema, &resolution)
                {
                    anyhow::bail!("external wait resolution payload is invalid: {issue}");
                }
            }
        }

        let Some(claimed) = claim_stateful_wait_for_resolution(
            &paths.waits_path,
            tenant,
            wait_id,
            StatefulWaitKind::ExternalCondition,
            "automation-v2-external-wait-resolver",
            now_ms(),
            EXTERNAL_WAIT_RESOLUTION_LEASE_MS,
        )
        .await?
        else {
            return Ok(None);
        };
        let now = now_ms();
        let Some(reserved) = begin_claimed_stateful_wait_wake_completion(
            &paths.waits_path,
            tenant,
            &claimed,
            &completion_key,
            now,
        )
        .await?
        else {
            return Ok(None);
        };
        let event_type = "stateful_runtime.wait.external_condition_resolved";
        let event = StatefulRunEventRecord {
            schema_version: 1,
            event_id: format!("stateful-external-wait-{completion_key}"),
            run_id: reserved.run_id.clone(),
            seq: 0,
            event_type: event_type.to_string(),
            occurred_at_ms: now,
            scope: reserved.scope.clone(),
            actor: None,
            phase_id: reserved.phase_id.clone(),
            phase_transition: None,
            wait_kind: Some(StatefulWaitKind::ExternalCondition),
            causation_id: Some(idempotency_key.to_string()),
            correlation_id: Some(completion_key.clone()),
            payload: json!({
                "wait_id": &reserved.wait_id,
                "resolution": &resolution,
            }),
        };
        let (_appended, seq) =
            append_stateful_run_event_once_with_next_seq(&paths.run_events_path, tenant, &event)
                .await?;
        let _ = self
            .requeue_automation_v2_run_from_stateful_wait_wake(
                &reserved.run_id,
                &reserved.wait_id,
                event_type,
                seq,
                format!("external condition wait `{}` resolved", reserved.wait_id),
                json!({ "resolution": &resolution }),
            )
            .await;
        finish_claimed_stateful_wait_completion(
            &paths.waits_path,
            tenant,
            &reserved,
            &completion_key,
            seq,
            StatefulWaitStatus::Woken,
            now,
        )
        .await
    }
}

pub(crate) fn explicit_automation_wait_gate_parts(
    node: &AutomationFlowNode,
) -> Option<(
    Option<String>,
    Vec<String>,
    Vec<String>,
    Option<crate::AutomationGateExpiryPolicy>,
)> {
    let AutomationWaitSpec::Approval {
        decisions,
        expires_after_ms,
        timeout,
    } = node.wait.as_ref()?
    else {
        return None;
    };
    let expiry_policy = timeout
        .as_ref()
        .map(|timeout| crate::AutomationGateExpiryPolicy {
            expires_after_ms: Some(timeout.expires_after_ms),
            on_expiry: Some(match timeout.on_timeout {
                WaitTimeoutAction::Cancel => crate::AutomationGateExpiryAction::Cancel,
                WaitTimeoutAction::Escalate => crate::AutomationGateExpiryAction::Escalate,
                WaitTimeoutAction::Remind => crate::AutomationGateExpiryAction::Remind,
                WaitTimeoutAction::Resume => crate::AutomationGateExpiryAction::Resume,
            }),
            escalate_to: timeout.escalate_to.clone(),
            remind_every_ms: timeout.remind_every_ms,
        })
        .or_else(|| {
            expires_after_ms.map(|expires_after_ms| crate::AutomationGateExpiryPolicy {
                expires_after_ms: Some(expires_after_ms),
                on_expiry: Some(crate::AutomationGateExpiryAction::Cancel),
                escalate_to: None,
                remind_every_ms: None,
            })
        });
    Some((
        Some(node.objective.clone()),
        decisions.clone(),
        Vec::new(),
        expiry_policy,
    ))
}

fn automation_v2_wait_record(
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
    wait: &AutomationWaitSpec,
    now: u64,
) -> anyhow::Result<StatefulWaitRecord> {
    let wait_id = format!("automation-v2:{}:{}:wait", run.run_id, node.node_id);
    let (wait_kind, wake_at_ms, timeout_policy, metadata) = match wait {
        AutomationWaitSpec::Timer {
            delay_ms,
            wake_at,
            timeout,
        } => {
            let wake_at_ms = match (delay_ms, wake_at) {
                (Some(delay_ms), None) => now.checked_add(*delay_ms),
                (None, Some(binding)) => Some(resolve_wait_binding_u64(run, binding)?),
                _ => None,
            }
            .ok_or_else(|| anyhow::anyhow!("timer wait wake timestamp overflowed or is missing"))?;
            (
                StatefulWaitKind::Timer,
                Some(wake_at_ms),
                timeout
                    .as_ref()
                    .map(|policy| stateful_timeout_policy(policy, now)),
                automation_wait_node_metadata(
                    run,
                    node,
                    wait,
                    json!({
                        "wake_at_ms": wake_at_ms,
                    }),
                ),
            )
        }
        AutomationWaitSpec::Webhook {
            trigger_id,
            provider,
            provider_event_kind,
            correlation,
            timeout,
        } => {
            let expected = resolve_wait_binding_string(run, &correlation.value)?;
            let mut match_rules = StatefulWebhookWaitMatch {
                trigger_id: Some(trigger_id.clone()),
                provider: provider.clone(),
                provider_event_kind: provider_event_kind.clone(),
                provider_event_id: None,
                body_digest: None,
                idempotency_key: None,
            };
            match correlation.field {
                WebhookCorrelationField::ProviderEventId => {
                    match_rules.provider_event_id = Some(expected)
                }
                WebhookCorrelationField::BodyDigest => match_rules.body_digest = Some(expected),
                WebhookCorrelationField::IdempotencyKey => {
                    match_rules.idempotency_key = Some(expected)
                }
            }
            let extra = automation_wait_node_metadata(
                run,
                node,
                wait,
                json!({
                    "trigger_id": trigger_id,
                    "provider": provider,
                    "provider_event_kind": provider_event_kind,
                    "correlation_field": &correlation.field,
                }),
            );
            (
                StatefulWaitKind::Webhook,
                None,
                Some(stateful_timeout_policy(timeout, now)),
                stateful_webhook_wait_metadata(match_rules, Some(extra)),
            )
        }
        AutomationWaitSpec::ExternalCondition {
            condition_key,
            timeout,
            payload_schema,
        } => {
            let condition_key = resolve_wait_binding_string(run, condition_key)?;
            (
                StatefulWaitKind::ExternalCondition,
                None,
                Some(stateful_timeout_policy(timeout, now)),
                automation_wait_node_metadata(
                    run,
                    node,
                    wait,
                    json!({
                        "condition_key": condition_key,
                        "payload_schema": payload_schema,
                    }),
                ),
            )
        }
        AutomationWaitSpec::Approval { .. } => {
            anyhow::bail!("approval waits use the approval-gate projection path")
        }
    };

    Ok(StatefulWaitRecord {
        schema_version: 1,
        wait_id,
        run_id: run.run_id.clone(),
        wait_kind,
        status: StatefulWaitStatus::Waiting,
        scope: stateful_run_from_automation_v2(run).scope,
        phase_id: Some(node.node_id.clone()),
        reason: Some(format!("Automation V2 wait node `{}`", node.node_id)),
        created_at_ms: now,
        updated_at_ms: now,
        wake_at_ms,
        timeout_policy,
        event_seq: None,
        wake_idempotency_key: None,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        completed_at_ms: None,
        metadata: Some(metadata),
    })
}

fn stateful_timeout_policy(policy: &WaitTimeoutPolicy, now: u64) -> StatefulWaitTimeoutPolicy {
    StatefulWaitTimeoutPolicy {
        timeout_at_ms: now.saturating_add(policy.expires_after_ms),
        on_timeout: match policy.on_timeout {
            WaitTimeoutAction::Cancel => StatefulWaitTimeoutAction::Cancel,
            WaitTimeoutAction::Escalate => StatefulWaitTimeoutAction::Escalate,
            WaitTimeoutAction::Remind => StatefulWaitTimeoutAction::Remind,
            WaitTimeoutAction::Resume => StatefulWaitTimeoutAction::Resume,
        },
        escalate_to: policy.escalate_to.clone(),
        remind_every_ms: policy.remind_every_ms,
        metadata: Some(json!({ "source": "automation_v2.wait_node" })),
    }
}

fn resolve_wait_binding<'a>(
    run: &'a AutomationV2RunRecord,
    binding: &'a OrchestrationValueBinding,
) -> anyhow::Result<Value> {
    match binding {
        OrchestrationValueBinding::Literal { value } => Ok(value.clone()),
        OrchestrationValueBinding::NodeOutput {
            node_id,
            json_pointer,
        } => {
            let output = run.checkpoint.node_outputs.get(node_id).ok_or_else(|| {
                anyhow::anyhow!("wait binding source node `{node_id}` has no output")
            })?;
            match json_pointer.as_deref() {
                Some(pointer) if !pointer.is_empty() => {
                    output.pointer(pointer).cloned().ok_or_else(|| {
                        anyhow::anyhow!("wait binding pointer `{pointer}` was not found")
                    })
                }
                _ => Ok(output.clone()),
            }
        }
    }
}

fn resolve_wait_binding_u64(
    run: &AutomationV2RunRecord,
    binding: &OrchestrationValueBinding,
) -> anyhow::Result<u64> {
    resolve_wait_binding(run, binding)?
        .as_u64()
        .filter(|value| *value > 0)
        .ok_or_else(|| anyhow::anyhow!("wait binding must resolve to a positive integer"))
}

fn resolve_wait_binding_string(
    run: &AutomationV2RunRecord,
    binding: &OrchestrationValueBinding,
) -> anyhow::Result<String> {
    let value = resolve_wait_binding(run, binding)?;
    let value = value
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("wait binding must resolve to a non-empty string"))?;
    if value.len() > MAX_WAIT_BINDING_STRING_BYTES {
        anyhow::bail!("wait binding exceeds {MAX_WAIT_BINDING_STRING_BYTES} bytes");
    }
    Ok(value.to_string())
}

fn automation_wait_node_metadata(
    run: &AutomationV2RunRecord,
    node: &AutomationFlowNode,
    wait: &AutomationWaitSpec,
    details: Value,
) -> Value {
    json!({
        AUTOMATION_WAIT_NODE_METADATA_KEY: {
            "automation_id": &run.automation_id,
            "node_id": &node.node_id,
            "wait_kind": wait_kind_name_from_spec(wait),
        },
        "details": details,
    })
}

pub(crate) fn automation_wait_node_id_from_record(wait: &StatefulWaitRecord) -> Option<&str> {
    wait.metadata
        .as_ref()?
        .get(AUTOMATION_WAIT_NODE_METADATA_KEY)?
        .get("node_id")?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn wait_kind_name_from_spec(wait: &AutomationWaitSpec) -> &'static str {
    match wait {
        AutomationWaitSpec::Timer { .. } => "timer",
        AutomationWaitSpec::Approval { .. } => "approval",
        AutomationWaitSpec::Webhook { .. } => "webhook",
        AutomationWaitSpec::ExternalCondition { .. } => "external_condition",
    }
}

fn wait_kind_name(wait_kind: &StatefulWaitKind) -> &'static str {
    match wait_kind {
        StatefulWaitKind::Timer => "timer",
        StatefulWaitKind::Webhook => "webhook",
        StatefulWaitKind::Approval => "approval",
        StatefulWaitKind::ExternalCondition => "external_condition",
        StatefulWaitKind::RetryBackoff => "retry_backoff",
    }
}
