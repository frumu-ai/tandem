use super::*;

use crate::app::state::automation::lifecycle::automation_lifecycle_event_counts_as_activity;
use crate::stateful_runtime::{
    append_stateful_run_event_once_with_next_seq, guarded_phase_state_from_status,
    list_stateful_run_snapshots, stable_definition_snapshot_hash, stateful_run_from_automation_v2,
    write_stateful_run_snapshot, StatefulRunEventRecord, StatefulRunSnapshotRecord,
    StatefulRuntimeStoragePaths, StatefulWaitKind, StatefulWorkflowRunKind,
};

impl AppState {
    pub(crate) async fn project_automation_v2_stateful_boundaries(
        &self,
        run: &AutomationV2RunRecord,
    ) -> anyhow::Result<usize> {
        let paths =
            StatefulRuntimeStoragePaths::from_runtime_events_path(&self.runtime_events_path);
        project_automation_v2_stateful_boundaries_to_paths(&paths, run).await
    }

    pub(crate) async fn project_automation_v2_stateful_boundaries_or_warn(
        &self,
        run: &AutomationV2RunRecord,
    ) {
        if let Err(error) = self.project_automation_v2_stateful_boundaries(run).await {
            tracing::warn!(
                run_id = %run.run_id,
                error = %error,
                "failed to project automation v2 lifecycle boundary to stateful runtime log"
            );
        }
    }
}

pub(crate) async fn project_automation_v2_stateful_boundaries_to_paths(
    paths: &StatefulRuntimeStoragePaths,
    run: &AutomationV2RunRecord,
) -> anyhow::Result<usize> {
    if run.checkpoint.lifecycle_history.is_empty() {
        return Ok(0);
    }

    let stateful_run = stateful_run_from_automation_v2(run);
    let scope = stateful_run.scope.clone();
    let mut latest_snapshot = list_stateful_run_snapshots(
        &paths.snapshots_root,
        &run.tenant_context,
        &run.run_id,
        Some(1),
    )
    .pop();
    let mut projected = 0usize;
    for (index, lifecycle) in run.checkpoint.lifecycle_history.iter().enumerate() {
        let event_id = automation_lifecycle_stateful_event_id(run, index, lifecycle);
        let phase_id = lifecycle_phase_id(run, lifecycle);
        let wait_kind = lifecycle_wait_kind(run, lifecycle);
        let previous_history = latest_snapshot
            .as_ref()
            .map(|snapshot| snapshot.phase_history.as_slice())
            .unwrap_or(&[]);
        let phase_state = guarded_phase_state_from_status(
            &run.run_id,
            &stateful_run.status,
            lifecycle.recorded_at_ms,
            phase_id.as_deref(),
            latest_snapshot.as_ref().map(|snapshot| snapshot.phase),
            previous_history,
            Some(format!("automation_v2_lifecycle:{}", lifecycle.event)),
        )?;
        let event = StatefulRunEventRecord {
            schema_version: 1,
            event_id: event_id.clone(),
            run_id: run.run_id.clone(),
            seq: 0,
            event_type: format!("stateful_runtime.automation_v2.{}", lifecycle.event),
            occurred_at_ms: lifecycle.recorded_at_ms,
            scope: scope.clone(),
            actor: None,
            phase_id: phase_id.clone(),
            phase_transition: None,
            wait_kind,
            causation_id: lifecycle_causation_id(lifecycle),
            correlation_id: Some(run.automation_id.clone()),
            payload: automation_lifecycle_event_payload(run, index, lifecycle),
        };
        let (_appended, seq) = append_stateful_run_event_once_with_next_seq(
            &paths.run_events_path,
            &run.tenant_context,
            &event,
        )
        .await?;

        let snapshot = automation_lifecycle_snapshot(
            run,
            &stateful_run,
            lifecycle,
            index,
            seq,
            event_id,
            phase_id,
            phase_state,
        );
        write_stateful_run_snapshot(&paths.snapshots_root, &snapshot).await?;
        latest_snapshot = Some(snapshot);
        projected += 1;
    }

    Ok(projected)
}

fn automation_lifecycle_stateful_event_id(
    run: &AutomationV2RunRecord,
    index: usize,
    lifecycle: &AutomationLifecycleRecord,
) -> String {
    format!(
        "automation-v2-lifecycle-{}-{}-{}",
        run.run_id,
        index,
        safe_event_component(&lifecycle.event)
    )
}

fn safe_event_component(value: &str) -> String {
    let component = value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    if component.is_empty() {
        "event".to_string()
    } else {
        component
    }
}

fn lifecycle_phase_id(
    run: &AutomationV2RunRecord,
    lifecycle: &AutomationLifecycleRecord,
) -> Option<String> {
    lifecycle
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("node_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| {
            run.checkpoint
                .awaiting_gate
                .as_ref()
                .map(|gate| gate.node_id.clone())
        })
}

fn lifecycle_causation_id(lifecycle: &AutomationLifecycleRecord) -> Option<String> {
    let metadata = lifecycle.metadata.as_ref()?;
    ["session_id", "wait_id", "delivery_id", "handoff_id"]
        .into_iter()
        .find_map(|key| metadata_string(metadata, key))
        .or_else(|| {
            metadata
                .get("claim")
                .and_then(|claim| metadata_string(claim, "claim_id"))
        })
}

fn lifecycle_wait_kind(
    run: &AutomationV2RunRecord,
    lifecycle: &AutomationLifecycleRecord,
) -> Option<StatefulWaitKind> {
    if run.status == AutomationRunStatus::AwaitingApproval || run.checkpoint.awaiting_gate.is_some()
    {
        return Some(StatefulWaitKind::Approval);
    }

    lifecycle
        .metadata
        .as_ref()
        .and_then(|metadata| metadata_string(metadata, "wait_kind"))
        .and_then(|wait_kind| match wait_kind.as_str() {
            "timer" => Some(StatefulWaitKind::Timer),
            "webhook" => Some(StatefulWaitKind::Webhook),
            "approval" => Some(StatefulWaitKind::Approval),
            "external_condition" => Some(StatefulWaitKind::ExternalCondition),
            "retry_backoff" => Some(StatefulWaitKind::RetryBackoff),
            _ => None,
        })
}

fn metadata_string(metadata: &Value, key: &str) -> Option<String> {
    metadata
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn automation_lifecycle_event_payload(
    run: &AutomationV2RunRecord,
    index: usize,
    lifecycle: &AutomationLifecycleRecord,
) -> Value {
    json!({
        "source": "automation_v2_lifecycle",
        "automation_id": &run.automation_id,
        "trigger_type": &run.trigger_type,
        "status": &run.status,
        "event": &lifecycle.event,
        "lifecycle_index": index,
        "reason": &lifecycle.reason,
        "stop_kind": &lifecycle.stop_kind,
        "metadata": &lifecycle.metadata,
        "counts_as_activity": automation_lifecycle_event_counts_as_activity(&lifecycle.event),
    })
}

fn automation_lifecycle_snapshot(
    run: &AutomationV2RunRecord,
    stateful_run: &crate::stateful_runtime::StatefulWorkflowRunRecord,
    lifecycle: &AutomationLifecycleRecord,
    index: usize,
    seq: u64,
    event_id: String,
    phase_id: Option<String>,
    phase_state: crate::stateful_runtime::StatefulWorkflowPhaseState,
) -> StatefulRunSnapshotRecord {
    let status = stateful_run.status.clone();
    let checkpoint = checkpoint_summary(run);
    let payload_digest = Some(stable_definition_snapshot_hash(&checkpoint));
    StatefulRunSnapshotRecord {
        schema_version: 1,
        snapshot_id: event_id.clone(),
        run_id: run.run_id.clone(),
        seq,
        created_at_ms: lifecycle.recorded_at_ms,
        scope: stateful_run.scope.clone(),
        status,
        phase: phase_state.phase,
        phase_history: phase_state.phase_history,
        allowed_next_phases: phase_state.allowed_next_phases,
        phase_id,
        source_record_kind: Some(StatefulWorkflowRunKind::AutomationV2),
        checkpoint: Some(checkpoint),
        payload_digest,
        workflow_definition_version: stateful_run.workflow_definition_version.clone(),
        workflow_definition_snapshot_hash: stateful_run.workflow_definition_snapshot_hash.clone(),
        metadata: Some(json!({
            "source": "automation_v2_lifecycle",
            "event_id": event_id,
            "event": &lifecycle.event,
            "lifecycle_index": index,
            "redaction_tier": "summary",
            "activity_boundary": automation_lifecycle_event_counts_as_activity(&lifecycle.event),
        })),
    }
}

fn checkpoint_summary(run: &AutomationV2RunRecord) -> Value {
    json!({
        "redaction_tier": "summary",
        "completed_nodes": &run.checkpoint.completed_nodes,
        "pending_nodes": &run.checkpoint.pending_nodes,
        "blocked_nodes": &run.checkpoint.blocked_nodes,
        "node_attempts": &run.checkpoint.node_attempts,
        "node_attempt_verdict_counts": node_attempt_verdict_counts(run),
        "node_output_ids": node_output_ids(run),
        "awaiting_gate": awaiting_gate_summary(run),
        "gate_history_count": run.checkpoint.gate_history.len(),
        "lifecycle_event_count": run.checkpoint.lifecycle_history.len(),
        "last_failure": &run.checkpoint.last_failure,
        "execution_claim_epoch": run.execution_claim_epoch,
        "execution_claim": &run.execution_claim,
        "pause_reason": &run.pause_reason,
        "resume_reason": &run.resume_reason,
        "stop_kind": &run.stop_kind,
        "stop_reason": &run.stop_reason,
        "active_session_ids": &run.active_session_ids,
        "active_instance_ids": &run.active_instance_ids,
    })
}

fn node_output_ids(run: &AutomationV2RunRecord) -> Vec<String> {
    let mut ids = run
        .checkpoint
        .node_outputs
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    ids.sort();
    ids
}

fn node_attempt_verdict_counts(
    run: &AutomationV2RunRecord,
) -> std::collections::BTreeMap<String, usize> {
    run.checkpoint
        .node_attempt_verdicts
        .iter()
        .map(|(node_id, verdicts)| (node_id.clone(), verdicts.len()))
        .collect()
}

fn awaiting_gate_summary(run: &AutomationV2RunRecord) -> Option<Value> {
    run.checkpoint.awaiting_gate.as_ref().map(|gate| {
        json!({
            "node_id": &gate.node_id,
            "title": &gate.title,
            "decisions": &gate.decisions,
            "rework_targets": &gate.rework_targets,
            "requested_at_ms": gate.requested_at_ms,
            "upstream_node_ids": &gate.upstream_node_ids,
            "expiry_policy": &gate.expiry_policy,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::automation_v2::types::{
        AutomationExecutionPolicy, AutomationFlowSpec, AutomationRunCheckpoint,
        AutomationV2Schedule, AutomationV2ScheduleType, AutomationV2Spec, AutomationV2Status,
    };
    use crate::stateful_runtime::{
        list_stateful_run_snapshots, phase_state_from_status, query_stateful_run_events,
        write_stateful_run_snapshot, StatefulRunSnapshotRecord, StatefulRuntimeScope,
        StatefulWorkflowRunStatus,
    };
    use tandem_types::TenantContext;
    use uuid::Uuid;

    fn tenant() -> TenantContext {
        TenantContext::explicit_user_workspace("org-a", "workspace-a", None, "user-a")
    }

    fn automation_snapshot(automation_id: &str) -> AutomationV2Spec {
        AutomationV2Spec {
            automation_id: automation_id.to_string(),
            name: "Snapshot test".to_string(),
            description: None,
            status: AutomationV2Status::Active,
            schedule: AutomationV2Schedule {
                schedule_type: AutomationV2ScheduleType::Manual,
                cron_expression: None,
                interval_seconds: None,
                timezone: "UTC".to_string(),
                misfire_policy: crate::RoutineMisfirePolicy::RunOnce,
            },
            knowledge: Default::default(),
            agents: Vec::new(),
            flow: AutomationFlowSpec { nodes: Vec::new() },
            execution: AutomationExecutionPolicy::default(),
            output_targets: Vec::new(),
            created_at_ms: 1,
            updated_at_ms: 2,
            creator_id: "creator-a".to_string(),
            workspace_root: None,
            metadata: Some(json!({ "definition_version": "release-9" })),
            next_fire_at_ms: None,
            last_fired_at_ms: None,
            scope_policy: None,
            watch_conditions: Vec::new(),
            handoff_config: None,
        }
    }

    fn run_with_lifecycle() -> AutomationV2RunRecord {
        AutomationV2RunRecord {
            run_id: "run-stateful-projection".to_string(),
            automation_id: "automation-a".to_string(),
            tenant_context: tenant(),
            trigger_type: "manual".to_string(),
            status: AutomationRunStatus::Running,
            created_at_ms: 100,
            updated_at_ms: 200,
            started_at_ms: Some(120),
            finished_at_ms: None,
            active_session_ids: vec!["session-a".to_string()],
            latest_session_id: Some("session-a".to_string()),
            active_instance_ids: Vec::new(),
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: vec!["node-a".to_string()],
                pending_nodes: vec!["node-b".to_string()],
                node_outputs: [("node-a".to_string(), json!({ "sensitive": "not copied" }))]
                    .into_iter()
                    .collect(),
                node_attempts: [("node-a".to_string(), 1)].into_iter().collect(),
                node_attempt_verdicts: [("node-a".to_string(), vec![json!({ "ok": true })])]
                    .into_iter()
                    .collect(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: vec![
                    AutomationLifecycleRecord {
                        event: "run_execution_claimed".to_string(),
                        recorded_at_ms: 130,
                        reason: Some("claimed".to_string()),
                        stop_kind: None,
                        metadata: Some(json!({
                            "claim": { "claim_id": "claim-a" },
                        })),
                    },
                    AutomationLifecycleRecord {
                        event: "node_completed".to_string(),
                        recorded_at_ms: 180,
                        reason: Some("done".to_string()),
                        stop_kind: None,
                        metadata: Some(json!({
                            "node_id": "node-a",
                            "session_id": "session-a",
                        })),
                    },
                ],
                last_failure: None,
            },
            runtime_context: None,
            automation_snapshot: Some(automation_snapshot("automation-a")),
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            execution_claim: None,
            execution_claim_epoch: 1,
            pause_reason: None,
            resume_reason: None,
            detail: None,
            stop_kind: None,
            stop_reason: None,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            estimated_cost_usd: 0.0,
            scheduler: None,
            trigger_reason: None,
            consumed_handoff_id: None,
            learning_summary: None,
            effective_execution_profile: Default::default(),
            requested_execution_profile: None,
        }
    }

    #[tokio::test]
    async fn projects_new_lifecycle_records_to_stateful_events_and_snapshots() {
        let root =
            std::env::temp_dir().join(format!("stateful-lifecycle-projection-{}", Uuid::new_v4()));
        let paths = StatefulRuntimeStoragePaths::new(
            root.join("events.jsonl"),
            root.join("snapshots"),
            root.join("waits.json"),
        );
        let run = run_with_lifecycle();

        let projected = project_automation_v2_stateful_boundaries_to_paths(&paths, &run)
            .await
            .expect("project lifecycle");

        assert_eq!(projected, 2);
        let events = query_stateful_run_events(
            &paths.run_events_path,
            &run.tenant_context,
            crate::stateful_runtime::StatefulRunEventQuery {
                run_id: &run.run_id,
                after_seq: None,
                before_seq: None,
                limit: None,
                tail: false,
            },
        );
        assert_eq!(events.len(), 2);
        assert_eq!(
            events[0].event_type,
            "stateful_runtime.automation_v2.run_execution_claimed"
        );
        assert_eq!(
            events[1].event_type,
            "stateful_runtime.automation_v2.node_completed"
        );
        assert_eq!(events[1].phase_id.as_deref(), Some("node-a"));
        assert_eq!(events[1].causation_id.as_deref(), Some("session-a"));

        let snapshots = list_stateful_run_snapshots(
            &paths.snapshots_root,
            &run.tenant_context,
            &run.run_id,
            None,
        );
        assert_eq!(snapshots.len(), 2);
        assert_eq!(
            snapshots[1].workflow_definition_version.as_deref(),
            Some("release-9")
        );
        assert!(snapshots[1]
            .workflow_definition_snapshot_hash
            .as_deref()
            .is_some_and(|hash| hash.starts_with("sha256:")));
        let checkpoint = snapshots[1].checkpoint.as_ref().expect("checkpoint");
        assert_eq!(checkpoint["node_output_ids"], json!(["node-a"]));
        assert!(checkpoint["node_outputs"].is_null());

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn projection_is_idempotent_for_same_lifecycle_index() {
        let root = std::env::temp_dir().join(format!(
            "stateful-lifecycle-projection-once-{}",
            Uuid::new_v4()
        ));
        let paths = StatefulRuntimeStoragePaths::new(
            root.join("events.jsonl"),
            root.join("snapshots"),
            root.join("waits.json"),
        );
        let run = run_with_lifecycle();

        project_automation_v2_stateful_boundaries_to_paths(&paths, &run)
            .await
            .expect("first projection");
        project_automation_v2_stateful_boundaries_to_paths(&paths, &run)
            .await
            .expect("second projection");

        let events = query_stateful_run_events(
            &paths.run_events_path,
            &run.tenant_context,
            crate::stateful_runtime::StatefulRunEventQuery {
                run_id: &run.run_id,
                after_seq: None,
                before_seq: None,
                limit: None,
                tail: false,
            },
        );
        assert_eq!(events.len(), 2);
        let snapshots = list_stateful_run_snapshots(
            &paths.snapshots_root,
            &run.tenant_context,
            &run.run_id,
            None,
        );
        assert_eq!(snapshots.len(), 2);

        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn projection_accumulates_guarded_phase_transition_from_previous_snapshot() {
        let root = std::env::temp_dir().join(format!(
            "stateful-lifecycle-projection-phase-{}",
            Uuid::new_v4()
        ));
        let paths = StatefulRuntimeStoragePaths::new(
            root.join("events.jsonl"),
            root.join("snapshots"),
            root.join("waits.json"),
        );
        let run = run_with_lifecycle();
        let queued_phase =
            phase_state_from_status(&run.run_id, &StatefulWorkflowRunStatus::Queued, 100, None);
        let seed_snapshot = StatefulRunSnapshotRecord {
            schema_version: 1,
            snapshot_id: "queued-seed".to_string(),
            run_id: run.run_id.clone(),
            seq: 1,
            created_at_ms: 100,
            scope: StatefulRuntimeScope::from_tenant_context(run.tenant_context.clone()),
            status: StatefulWorkflowRunStatus::Queued,
            phase: queued_phase.phase,
            phase_history: queued_phase.phase_history,
            allowed_next_phases: queued_phase.allowed_next_phases,
            phase_id: None,
            source_record_kind: Some(StatefulWorkflowRunKind::AutomationV2),
            checkpoint: None,
            payload_digest: None,
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            metadata: None,
        };
        write_stateful_run_snapshot(&paths.snapshots_root, &seed_snapshot)
            .await
            .expect("write queued seed snapshot");

        project_automation_v2_stateful_boundaries_to_paths(&paths, &run)
            .await
            .expect("project lifecycle");

        let snapshots = list_stateful_run_snapshots(
            &paths.snapshots_root,
            &run.tenant_context,
            &run.run_id,
            None,
        );
        let latest = snapshots.last().expect("latest snapshot");
        assert_eq!(
            latest.phase,
            crate::stateful_runtime::StatefulWorkflowPhase::RunningPhase
        );
        assert_eq!(latest.phase_history.len(), 2);
        assert_eq!(
            latest.phase_history[1].from_phase,
            Some(crate::stateful_runtime::StatefulWorkflowPhase::Queued)
        );
        assert_eq!(
            latest.phase_history[1].to_phase,
            crate::stateful_runtime::StatefulWorkflowPhase::RunningPhase
        );

        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
