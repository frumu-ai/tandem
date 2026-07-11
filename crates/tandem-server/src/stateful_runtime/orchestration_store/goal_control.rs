use anyhow::{bail, Context};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::{json, Value};
use tandem_automation::{
    AutomationRunStatus, AutomationStopKind, AutomationV2RunRecord, LongRunningGoal,
    WorkflowHandoff, WorkflowHandoffStatus,
};
use tandem_types::{PrincipalRef, TenantContext};

use super::{upsert_automation_run, upsert_goal, OrchestrationStateStore};
use crate::stateful_runtime::{
    StatefulOutboxRecord, StatefulOutboxStatus, StatefulRunEventRecord, StatefulRuntimeScope,
    StatefulWaitRecord, StatefulWaitStatus,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalControlOutcome {
    Applied,
    AlreadyTerminal,
}

#[derive(Debug, Clone)]
pub struct GoalCancellationResult {
    pub outcome: GoalControlOutcome,
    pub goal: LongRunningGoal,
    pub cancelled_run: Option<AutomationV2RunRecord>,
    /// Session and instance handles captured before the durable run record was
    /// cleared. The app layer uses them to stop in-flight execution after the
    /// cancellation transaction commits.
    pub cancelled_session_ids: Vec<String>,
    pub cancelled_instance_ids: Vec<String>,
    pub cancelled_wait_ids: Vec<String>,
    pub dead_lettered_handoff_ids: Vec<String>,
    pub outbox_id: Option<String>,
}

impl OrchestrationStateStore {
    /// Cancels the goal, active run, waits, and unconsumed handoffs in one
    /// transaction. The durable outbox row lets the engine stop any in-memory
    /// execution after a restart without weakening the database outcome.
    pub fn cancel_goal(
        &self,
        goal_id: &str,
        tenant: &TenantContext,
        reason: &str,
        actor: &PrincipalRef,
        now_ms: u64,
    ) -> anyhow::Result<GoalCancellationResult> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let payload = transaction
                .query_row(
                    "SELECT goal_json FROM long_running_goals WHERE goal_id = ?1",
                    [goal_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .context("long-running goal not found")?;
            let mut goal: LongRunningGoal = serde_json::from_str(&payload)?;
            if &goal.tenant_context != tenant {
                bail!("goal is outside the caller tenant scope");
            }
            if goal.status.is_terminal() {
                transaction.commit()?;
                return Ok(GoalCancellationResult {
                    outcome: GoalControlOutcome::AlreadyTerminal,
                    goal,
                    cancelled_run: None,
                    cancelled_session_ids: Vec::new(),
                    cancelled_instance_ids: Vec::new(),
                    cancelled_wait_ids: Vec::new(),
                    dead_lettered_handoff_ids: Vec::new(),
                    outbox_id: None,
                });
            }

            let active_run_id = goal.cancel(now_ms);
            let cancelled = active_run_id
                .as_deref()
                .map(|run_id| cancel_active_run(&transaction, run_id, reason, now_ms))
                .transpose()?
                .flatten();
            let cancelled_session_ids = cancelled
                .as_ref()
                .map(|cancelled| cancelled.session_ids.clone())
                .unwrap_or_default();
            let cancelled_instance_ids = cancelled
                .as_ref()
                .map(|cancelled| cancelled.instance_ids.clone())
                .unwrap_or_default();
            let cancelled_run = cancelled.map(|cancelled| cancelled.run);
            let cancelled_wait_ids = active_run_id
                .as_deref()
                .map(|run_id| {
                    cancel_waits(&transaction, run_id, &goal.tenant_context, reason, now_ms)
                })
                .transpose()?
                .unwrap_or_default();
            let dead_lettered_handoff_ids =
                dead_letter_handoffs(&transaction, goal_id, reason, now_ms)?;
            upsert_goal(&transaction, &goal)?;

            let outbox_id = active_run_id
                .as_ref()
                .map(|run_id| format!("goal-cancel:{goal_id}:{run_id}"));
            if let (Some(run_id), Some(outbox_id)) =
                (active_run_id.as_deref(), outbox_id.as_deref())
            {
                let outbox =
                    cancellation_outbox(outbox_id, goal_id, run_id, tenant, reason, actor, now_ms);
                transaction.execute(
                    "INSERT INTO outbox_effects
                        (effect_id, goal_id, run_id, status, effect_json, updated_at_ms,
                         org_id, workspace_id, deployment_id)
                     VALUES (?1, ?2, ?3, 'pending', ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(effect_id) DO NOTHING",
                    params![
                        outbox_id,
                        goal_id,
                        run_id,
                        serde_json::to_string(&outbox)?,
                        now_ms,
                        tenant.org_id,
                        tenant.workspace_id,
                        tenant.deployment_id,
                    ],
                )?;
            }
            let mut event = cancellation_event(&goal, reason, actor, now_ms);
            event.seq = super::next_event_seq(&transaction, &event.run_id)?;
            transaction.execute(
                "INSERT INTO stateful_events
                    (event_id, goal_id, run_id, seq, event_json, created_at_ms,
                     org_id, workspace_id, deployment_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(event_id) DO NOTHING",
                params![
                    event.event_id,
                    goal_id,
                    event.run_id,
                    event.seq,
                    serde_json::to_string(&event)?,
                    now_ms,
                    tenant.org_id,
                    tenant.workspace_id,
                    tenant.deployment_id,
                ],
            )?;
            transaction.commit()?;
            Ok(GoalCancellationResult {
                outcome: GoalControlOutcome::Applied,
                goal,
                cancelled_run,
                cancelled_session_ids,
                cancelled_instance_ids,
                cancelled_wait_ids,
                dead_lettered_handoff_ids,
                outbox_id,
            })
        })
    }
}

struct CancelledActiveRun {
    run: AutomationV2RunRecord,
    session_ids: Vec<String>,
    instance_ids: Vec<String>,
}

fn cancel_active_run(
    transaction: &rusqlite::Transaction<'_>,
    run_id: &str,
    reason: &str,
    now_ms: u64,
) -> anyhow::Result<Option<CancelledActiveRun>> {
    let payload = transaction
        .query_row(
            "SELECT run_json FROM automation_runs WHERE run_id = ?1",
            [run_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    let Some(payload) = payload else {
        return Ok(None);
    };
    let mut run: AutomationV2RunRecord = serde_json::from_str(&payload)?;
    if matches!(
        run.status,
        AutomationRunStatus::Completed
            | AutomationRunStatus::Failed
            | AutomationRunStatus::Cancelled
    ) {
        return Ok(Some(CancelledActiveRun {
            session_ids: run.active_session_ids.clone(),
            instance_ids: run.active_instance_ids.clone(),
            run,
        }));
    }
    let session_ids = run.active_session_ids.clone();
    let instance_ids = run.active_instance_ids.clone();
    run.status = AutomationRunStatus::Cancelled;
    run.updated_at_ms = now_ms;
    run.finished_at_ms = Some(now_ms);
    run.detail = Some(reason.to_string());
    run.stop_kind = Some(AutomationStopKind::Cancelled);
    run.stop_reason = Some(reason.to_string());
    run.active_session_ids.clear();
    run.latest_session_id = None;
    run.active_instance_ids.clear();
    run.execution_claim = None;
    upsert_automation_run(transaction, &run)?;
    Ok(Some(CancelledActiveRun {
        run,
        session_ids,
        instance_ids,
    }))
}

fn cancel_waits(
    transaction: &rusqlite::Transaction<'_>,
    run_id: &str,
    tenant: &TenantContext,
    reason: &str,
    now_ms: u64,
) -> anyhow::Result<Vec<String>> {
    let mut statement = transaction.prepare(
        "SELECT wait_id, wait_json FROM automation_waits
         WHERE run_id = ?1 AND org_id = ?2 AND workspace_id = ?3 AND deployment_id = ?4
           AND status IN ('waiting', 'claimed')",
    )?;
    let rows = statement
        .query_map(
            params![
                run_id,
                tenant.org_id,
                tenant.workspace_id,
                tenant.deployment_id.as_deref().unwrap_or(""),
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    let mut ids = Vec::new();
    for (wait_id, payload) in rows {
        let mut wait: StatefulWaitRecord = serde_json::from_str(&payload)?;
        wait.status = StatefulWaitStatus::Cancelled;
        wait.updated_at_ms = now_ms;
        wait.completed_at_ms = Some(now_ms);
        wait.claimed_by = None;
        wait.claimed_at_ms = None;
        wait.claim_expires_at_ms = None;
        wait.wake_idempotency_key = Some(format!("goal-cancel:{wait_id}"));
        wait.metadata = Some(merge_metadata(
            wait.metadata.take(),
            json!({"goal_cancellation_reason": reason}),
        ));
        transaction.execute(
            "UPDATE automation_waits SET status = 'cancelled', wait_json = ?2,
                updated_at_ms = ?3 WHERE wait_id = ?1 AND run_id = ?4
                AND org_id = ?5 AND workspace_id = ?6 AND deployment_id = ?7",
            params![
                wait_id,
                serde_json::to_string(&wait)?,
                now_ms,
                wait.run_id,
                tenant.org_id,
                tenant.workspace_id,
                tenant.deployment_id.as_deref().unwrap_or(""),
            ],
        )?;
        ids.push(wait.wait_id);
    }
    Ok(ids)
}

fn dead_letter_handoffs(
    transaction: &rusqlite::Transaction<'_>,
    goal_id: &str,
    reason: &str,
    now_ms: u64,
) -> anyhow::Result<Vec<String>> {
    let mut statement = transaction.prepare(
        "SELECT handoff_id, handoff_json FROM workflow_handoffs
         WHERE goal_id = ?1 AND status IN ('pending_approval', 'approved', 'claimed')",
    )?;
    let rows = statement
        .query_map([goal_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(statement);
    let mut ids = Vec::new();
    for (handoff_id, payload) in rows {
        let mut handoff: WorkflowHandoff = serde_json::from_str(&payload)?;
        handoff.status = WorkflowHandoffStatus::DeadLettered;
        handoff.updated_at_ms = now_ms;
        handoff.metadata = Some(merge_metadata(
            handoff.metadata.take(),
            json!({"goal_cancellation_reason": reason}),
        ));
        transaction.execute(
            "UPDATE workflow_handoffs SET status = 'dead_lettered', handoff_json = ?2,
                updated_at_ms = ?3 WHERE handoff_id = ?1",
            params![handoff_id, serde_json::to_string(&handoff)?, now_ms],
        )?;
        ids.push(handoff.handoff_id);
    }
    Ok(ids)
}

fn cancellation_outbox(
    outbox_id: &str,
    goal_id: &str,
    run_id: &str,
    tenant: &TenantContext,
    reason: &str,
    actor: &PrincipalRef,
    now_ms: u64,
) -> StatefulOutboxRecord {
    StatefulOutboxRecord {
        schema_version: 1,
        outbox_id: outbox_id.to_string(),
        run_id: Some(run_id.to_string()),
        scope: StatefulRuntimeScope::from_tenant_context(tenant.clone()),
        operation: "orchestration.goal.cancel_active_run".to_string(),
        status: StatefulOutboxStatus::Pending,
        source_kind: Some("long_running_goal".to_string()),
        source_id: Some(goal_id.to_string()),
        node_id: None,
        provider: None,
        tool: None,
        target: Some(run_id.to_string()),
        idempotency_key: Some(outbox_id.to_string()),
        payload_digest: None,
        policy_decision_id: None,
        context_assertion_id: None,
        effect_id: None,
        receipt_id: None,
        compensation_id: None,
        dead_letter_id: None,
        attempts: 0,
        created_at_ms: now_ms,
        updated_at_ms: now_ms,
        claimed_by: None,
        claimed_at_ms: None,
        claim_expires_at_ms: None,
        metadata: Some(json!({"reason": reason, "actor": actor})),
    }
}

fn cancellation_event(
    goal: &LongRunningGoal,
    reason: &str,
    actor: &PrincipalRef,
    now_ms: u64,
) -> StatefulRunEventRecord {
    StatefulRunEventRecord {
        schema_version: 1,
        event_id: format!("goal:{}:cancelled", goal.goal_id),
        run_id: format!("goal:{}", goal.goal_id),
        seq: 0,
        event_type: "stateful_runtime.goal.cancelled".to_string(),
        occurred_at_ms: now_ms,
        scope: StatefulRuntimeScope::from_tenant_context(goal.tenant_context.clone()),
        actor: Some(actor.clone()),
        phase_id: None,
        phase_transition: None,
        wait_kind: None,
        causation_id: None,
        correlation_id: Some(goal.goal_id.clone()),
        payload: json!({"goal_id": goal.goal_id, "reason": reason}),
    }
}

fn merge_metadata(existing: Option<Value>, additional: Value) -> Value {
    let mut object = existing
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    if let Some(additional) = additional.as_object() {
        object.extend(additional.clone());
    }
    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use tandem_automation::{
        GoalLimitAction, GoalPolicy, LongRunningGoalStatus, OrchestrationArtifactRef,
    };
    use tandem_types::{PrincipalKind, PrincipalRef};

    use super::*;
    use crate::stateful_runtime::{OrchestrationStorePaths, StatefulWaitKind, StatefulWaitRecord};

    fn store() -> (tempfile::TempDir, OrchestrationStateStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = OrchestrationStateStore::open(OrchestrationStorePaths {
            database_path: directory.path().join("runtime.sqlite3"),
            engine_lock_path: directory.path().join("engine.lock"),
        })
        .unwrap();
        (directory, store)
    }

    fn goal() -> LongRunningGoal {
        LongRunningGoal {
            schema_version: 1,
            goal_id: "goal-1".to_string(),
            orchestration_id: "orch-1".to_string(),
            orchestration_version: 1,
            objective: "Long-running objective".to_string(),
            status: LongRunningGoalStatus::Waiting,
            tenant_context: TenantContext::local_implicit(),
            policy: GoalPolicy {
                max_hops: 5,
                deadline_at_ms: None,
                max_total_tokens: None,
                max_total_cost_usd: None,
                on_limit: GoalLimitAction::PauseForReview,
            },
            active_run_id: Some("run-1".to_string()),
            current_node_id: Some("execute".to_string()),
            hop_count: 2,
            total_tokens: 50,
            total_cost_usd: 1.0,
            created_at_ms: 1,
            updated_at_ms: 10,
            finished_at_ms: None,
            final_artifact: None,
            metadata: None,
        }
    }

    fn run() -> AutomationV2RunRecord {
        serde_json::from_value(json!({
            "run_id": "run-1",
            "automation_id": "executor",
            "trigger_type": "orchestration_handoff",
            "status": "running",
            "created_at_ms": 2,
            "updated_at_ms": 10,
            "active_session_ids": ["session-1"],
            "latest_session_id": "session-1",
            "active_instance_ids": ["instance-1"],
            "checkpoint": {}
        }))
        .unwrap()
    }

    fn wait() -> StatefulWaitRecord {
        StatefulWaitRecord {
            schema_version: 1,
            wait_id: "wait-1".to_string(),
            run_id: "run-1".to_string(),
            wait_kind: StatefulWaitKind::ExternalCondition,
            status: StatefulWaitStatus::Claimed,
            scope: StatefulRuntimeScope::from_tenant_context(TenantContext::local_implicit()),
            phase_id: None,
            reason: None,
            created_at_ms: 3,
            updated_at_ms: 10,
            wake_at_ms: None,
            timeout_policy: None,
            event_seq: None,
            wake_idempotency_key: None,
            claimed_by: Some("engine-1".to_string()),
            claimed_at_ms: Some(9),
            claim_expires_at_ms: Some(100),
            completed_at_ms: None,
            metadata: None,
        }
    }

    fn handoff() -> WorkflowHandoff {
        WorkflowHandoff {
            schema_version: 1,
            handoff_id: "handoff-1".to_string(),
            idempotency_key: "goal-1:execute:complete".to_string(),
            goal_id: "goal-1".to_string(),
            orchestration_id: "orch-1".to_string(),
            orchestration_version: 1,
            tenant_context: TenantContext::local_implicit(),
            edge_id: "execute-verify".to_string(),
            transition_key: "continue".to_string(),
            source_automation_id: "executor".to_string(),
            source_run_id: "run-1".to_string(),
            source_node_id: "execute".to_string(),
            target_automation_id: "verifier".to_string(),
            target_node_id: "verify".to_string(),
            artifact: OrchestrationArtifactRef {
                artifact_type: "result".to_string(),
                content_path: None,
                content_digest: None,
                value: Some(json!({"ok": true})),
            },
            status: WorkflowHandoffStatus::PendingApproval,
            created_at_ms: 10,
            updated_at_ms: 10,
            consumed_by_run_id: None,
            metadata: None,
        }
    }

    fn seed(store: &OrchestrationStateStore) {
        store.put_goal(&goal()).unwrap();
        store.upsert_automation_runs([&run()]).unwrap();
        let wait = wait();
        let handoff = handoff();
        store
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO automation_waits
                        (wait_id, goal_id, run_id, org_id, workspace_id, deployment_id,
                         status, wait_json, updated_at_ms)
                     VALUES (?1, 'goal-1', ?2, 'local', 'local', '', 'claimed', ?3, ?4)",
                    params![
                        wait.wait_id,
                        wait.run_id,
                        serde_json::to_string(&wait)?,
                        wait.updated_at_ms,
                    ],
                )?;
                connection.execute(
                    "INSERT INTO workflow_handoffs
                        (handoff_id, goal_id, idempotency_key, org_id, workspace_id,
                         deployment_id, source_run_id, target_automation_id, status,
                         consumed_by_run_id, handoff_json, created_at_ms, updated_at_ms)
                     VALUES (?1, ?2, ?3, 'local', 'local', NULL, ?4, ?5,
                             'pending_approval', NULL, ?6, ?7, ?8)",
                    params![
                        handoff.handoff_id,
                        handoff.goal_id,
                        handoff.idempotency_key,
                        handoff.source_run_id,
                        handoff.target_automation_id,
                        serde_json::to_string(&handoff)?,
                        handoff.created_at_ms,
                        handoff.updated_at_ms,
                    ],
                )?;
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn cancellation_atomically_settles_active_records_and_is_idempotent() {
        let (_directory, store) = store();
        seed(&store);
        let actor = PrincipalRef::new(PrincipalKind::HumanUser, "operator");
        let first = store
            .cancel_goal(
                "goal-1",
                &TenantContext::local_implicit(),
                "operator cancelled",
                &actor,
                20,
            )
            .unwrap();
        assert_eq!(first.outcome, GoalControlOutcome::Applied);
        assert_eq!(first.goal.status, LongRunningGoalStatus::Cancelled);
        assert_eq!(first.cancelled_wait_ids, vec!["wait-1"]);
        assert_eq!(first.dead_lettered_handoff_ids, vec!["handoff-1"]);
        assert_eq!(first.cancelled_session_ids, vec!["session-1"]);
        assert_eq!(first.cancelled_instance_ids, vec!["instance-1"]);
        assert_eq!(
            first.cancelled_run.as_ref().map(|run| &run.status),
            Some(&AutomationRunStatus::Cancelled)
        );
        assert!(first.outbox_id.is_some());

        let second = store
            .cancel_goal(
                "goal-1",
                &TenantContext::local_implicit(),
                "duplicate",
                &actor,
                30,
            )
            .unwrap();
        assert_eq!(second.outcome, GoalControlOutcome::AlreadyTerminal);
        store
            .with_connection(|connection| {
                let outbox: u64 = connection.query_row(
                    "SELECT COUNT(*) FROM outbox_effects WHERE goal_id = 'goal-1'",
                    [],
                    |row| row.get(0),
                )?;
                let wait_status: String = connection.query_row(
                    "SELECT status FROM automation_waits WHERE wait_id = 'wait-1'",
                    [],
                    |row| row.get(0),
                )?;
                let handoff_status: String = connection.query_row(
                    "SELECT status FROM workflow_handoffs WHERE handoff_id = 'handoff-1'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(outbox, 1);
                assert_eq!(wait_status, "cancelled");
                assert_eq!(handoff_status, "dead_lettered");
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn cancellation_only_settles_waits_in_the_goal_tenant() {
        let (_directory, store) = store();
        seed(&store);
        let mut foreign_wait = wait();
        foreign_wait.wait_id = "wait-foreign".to_string();
        foreign_wait.scope = StatefulRuntimeScope::from_tenant_context(
            TenantContext::explicit_user_workspace("org-b", "workspace-b", None, "user-b"),
        );
        store
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO automation_waits
                        (wait_id, goal_id, run_id, org_id, workspace_id, deployment_id,
                         status, wait_json, updated_at_ms)
                     VALUES (?1, NULL, ?2, ?3, ?4, '', 'claimed', ?5, ?6)",
                    params![
                        foreign_wait.wait_id,
                        foreign_wait.run_id,
                        foreign_wait.scope.tenant_context.org_id,
                        foreign_wait.scope.tenant_context.workspace_id,
                        serde_json::to_string(&foreign_wait)?,
                        foreign_wait.updated_at_ms,
                    ],
                )?;
                Ok(())
            })
            .unwrap();

        let actor = PrincipalRef::new(PrincipalKind::HumanUser, "operator");
        let result = store
            .cancel_goal(
                "goal-1",
                &TenantContext::local_implicit(),
                "operator cancelled",
                &actor,
                20,
            )
            .unwrap();

        assert_eq!(result.cancelled_wait_ids, vec!["wait-1"]);
        store
            .with_connection(|connection| {
                let foreign_status: String = connection.query_row(
                    "SELECT status FROM automation_waits
                     WHERE wait_id = 'wait-foreign' AND org_id = 'org-b'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(foreign_status, "claimed");
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn cancellation_fails_closed_across_tenants() {
        let (_directory, store) = store();
        seed(&store);
        let error = store
            .cancel_goal(
                "goal-1",
                &TenantContext::explicit("other", "other", None),
                "malicious",
                &PrincipalRef::new(PrincipalKind::HumanUser, "other"),
                20,
            )
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("outside the caller tenant scope"));
        assert_eq!(
            store.get_goal("goal-1").unwrap().unwrap().status,
            LongRunningGoalStatus::Waiting
        );
    }
}
