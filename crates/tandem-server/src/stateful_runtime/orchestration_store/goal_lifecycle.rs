//! Goal start, pause/resume, and the read models behind the public goal APIs.
//!
//! `start_goal` is the only way a goal comes into existence: one transaction
//! writes the goal seeded at the orchestration root, the root Automation V2
//! run, the hop-0 lineage link, and the goal-started event. Goal identity is
//! derived from the caller's idempotency key, so replaying a start request
//! returns the already-created goal and root run instead of a duplicate.

use anyhow::{bail, Context};
use rusqlite::{params, OptionalExtension, TransactionBehavior};
use serde_json::json;
use tandem_automation::{
    AutomationV2RunRecord, GoalRunLink, LongRunningGoal, LongRunningGoalStatus, WorkflowHandoff,
};
use tandem_types::{PrincipalRef, TenantContext};

use super::{upsert_automation_run, upsert_goal, OrchestrationStateStore};
use crate::stateful_runtime::{StatefulRunEventRecord, StatefulRuntimeScope};

#[derive(Debug, Clone)]
pub enum StartGoalOutcome {
    Created {
        goal: LongRunningGoal,
        root_run: AutomationV2RunRecord,
    },
    /// The idempotency key already produced this goal; the stored goal and
    /// root run are returned unchanged.
    AlreadyStarted {
        goal: LongRunningGoal,
        root_run: AutomationV2RunRecord,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalPauseOutcome {
    Applied,
    AlreadyPaused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GoalResumeOutcome {
    Applied,
    NotPaused,
}

/// One durable goal event together with its store-wide cursor. The cursor is
/// the SQLite rowid of the event, which increases monotonically per insert;
/// SSE uses it as the `Last-Event-ID`, so replay after reconnect is exact.
#[derive(Debug, Clone)]
pub struct GoalEventRow {
    pub cursor: i64,
    pub event: StatefulRunEventRecord,
}

impl OrchestrationStateStore {
    /// Atomically create the goal, root run, hop-0 lineage, and start event —
    /// or return the previously created records for a replayed idempotency key.
    pub fn start_goal(
        &self,
        goal: &LongRunningGoal,
        root_run: &AutomationV2RunRecord,
        link: &GoalRunLink,
        actor: &PrincipalRef,
    ) -> anyhow::Result<StartGoalOutcome> {
        if link.goal_id != goal.goal_id || link.run_id != root_run.run_id {
            bail!("goal start lineage must bind the goal to its root run");
        }
        if link.hop_index != 0 || link.parent_run_id.is_some() {
            bail!("goal start lineage must be hop 0 with no parent run");
        }
        if root_run.tenant_context != goal.tenant_context {
            bail!("root run must remain in the goal tenant scope");
        }
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = transaction
                .query_row(
                    "SELECT goal_json FROM long_running_goals WHERE goal_id = ?1",
                    [&goal.goal_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            if let Some(payload) = existing {
                let stored: LongRunningGoal = serde_json::from_str(&payload)?;
                let same_scope = stored.tenant_context.org_id == goal.tenant_context.org_id
                    && stored.tenant_context.workspace_id == goal.tenant_context.workspace_id
                    && stored.tenant_context.deployment_id == goal.tenant_context.deployment_id;
                if !same_scope {
                    bail!("goal is outside the caller tenant scope");
                }
                if stored.orchestration_id != goal.orchestration_id
                    || stored.orchestration_version != goal.orchestration_version
                {
                    bail!(
                        "goal start idempotency key is already bound to orchestration {} version {}",
                        stored.orchestration_id,
                        stored.orchestration_version
                    );
                }
                let root_payload = transaction
                    .query_row(
                        "SELECT run_json FROM automation_runs WHERE run_id = ?1",
                        [&root_run.run_id],
                        |row| row.get::<_, String>(0),
                    )
                    .optional()?
                    .context("started goal is missing its root run")?;
                transaction.commit()?;
                return Ok(StartGoalOutcome::AlreadyStarted {
                    goal: stored,
                    root_run: serde_json::from_str(&root_payload)?,
                });
            }

            upsert_goal(&transaction, goal)?;
            upsert_automation_run(&transaction, root_run)?;
            transaction.execute(
                "INSERT INTO goal_run_links
                    (goal_id, run_id, orchestration_node_id, orchestration_version, hop_index,
                     parent_run_id, triggering_handoff_id, link_json, created_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    link.goal_id,
                    link.run_id,
                    link.orchestration_node_id,
                    link.orchestration_version,
                    link.hop_index,
                    link.parent_run_id,
                    link.triggering_handoff_id,
                    serde_json::to_string(link)?,
                    link.created_at_ms,
                ],
            )?;
            insert_goal_event(
                &transaction,
                goal,
                actor,
                goal.created_at_ms,
                "stateful_runtime.goal.started",
                json!({
                    "goal_id": goal.goal_id,
                    "orchestration_id": goal.orchestration_id,
                    "orchestration_version": goal.orchestration_version,
                    "root_run_id": root_run.run_id,
                }),
            )?;
            transaction.commit()?;
            Ok(StartGoalOutcome::Created {
                goal: goal.clone(),
                root_run: root_run.clone(),
            })
        })
    }

    /// Operator pause: no new transitions are admitted until resume. Terminal
    /// goals reject the mutation; an already-paused goal is a no-op.
    pub fn pause_goal(
        &self,
        goal_id: &str,
        tenant: &TenantContext,
        reason: &str,
        actor: &PrincipalRef,
        now_ms: u64,
    ) -> anyhow::Result<(GoalPauseOutcome, LongRunningGoal)> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut goal = load_goal_for_update(&transaction, goal_id, tenant)?;
            if goal.status.is_terminal() {
                bail!("terminal goals cannot be paused");
            }
            if goal.status == LongRunningGoalStatus::Paused {
                transaction.commit()?;
                return Ok((GoalPauseOutcome::AlreadyPaused, goal));
            }
            goal.status = LongRunningGoalStatus::Paused;
            goal.updated_at_ms = now_ms;
            upsert_goal(&transaction, &goal)?;
            insert_goal_event(
                &transaction,
                &goal,
                actor,
                now_ms,
                "stateful_runtime.goal.paused",
                json!({"goal_id": goal_id, "reason": reason}),
            )?;
            transaction.commit()?;
            Ok((GoalPauseOutcome::Applied, goal))
        })
    }

    /// Resume a paused goal: back to `Active` when it still has an active run,
    /// otherwise `Waiting` for the next transition or wake.
    pub fn resume_goal(
        &self,
        goal_id: &str,
        tenant: &TenantContext,
        reason: &str,
        actor: &PrincipalRef,
        now_ms: u64,
    ) -> anyhow::Result<(GoalResumeOutcome, LongRunningGoal)> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let mut goal = load_goal_for_update(&transaction, goal_id, tenant)?;
            if goal.status.is_terminal() {
                bail!("terminal goals cannot be resumed");
            }
            if goal.status != LongRunningGoalStatus::Paused {
                transaction.commit()?;
                return Ok((GoalResumeOutcome::NotPaused, goal));
            }
            goal.status = if goal.active_run_id.is_some() {
                LongRunningGoalStatus::Active
            } else {
                LongRunningGoalStatus::Waiting
            };
            goal.updated_at_ms = now_ms;
            upsert_goal(&transaction, &goal)?;
            insert_goal_event(
                &transaction,
                &goal,
                actor,
                now_ms,
                "stateful_runtime.goal.resumed",
                json!({"goal_id": goal_id, "reason": reason}),
            )?;
            transaction.commit()?;
            Ok((GoalResumeOutcome::Applied, goal))
        })
    }

    pub fn list_goals(
        &self,
        tenant: &TenantContext,
        status: Option<&str>,
        orchestration_id: Option<&str>,
        limit: usize,
    ) -> anyhow::Result<Vec<LongRunningGoal>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT goal_json FROM long_running_goals
                 WHERE org_id = ?1 AND workspace_id = ?2
                   AND (deployment_id IS ?3 OR deployment_id = ?3)
                 ORDER BY updated_at_ms DESC, goal_id",
            )?;
            let rows = statement.query_map(
                params![tenant.org_id, tenant.workspace_id, tenant.deployment_id],
                |row| row.get::<_, String>(0),
            )?;
            let mut goals = Vec::new();
            for row in rows {
                let goal: LongRunningGoal = serde_json::from_str(&row?)?;
                if let Some(status) = status {
                    let stored = serde_json::to_value(&goal.status)?;
                    if stored.as_str() != Some(status) {
                        continue;
                    }
                }
                if orchestration_id.is_some_and(|id| id != goal.orchestration_id) {
                    continue;
                }
                goals.push(goal);
                if goals.len() >= limit {
                    break;
                }
            }
            Ok(goals)
        })
    }

    /// Tenant-scoped goal read. Cross-tenant access is indistinguishable from
    /// absence at the store layer, so callers cannot leak goals by skipping
    /// an entrypoint-level scope check.
    pub fn get_goal_for_tenant(
        &self,
        tenant: &TenantContext,
        goal_id: &str,
    ) -> anyhow::Result<Option<LongRunningGoal>> {
        self.with_connection(|connection| {
            let payload = connection
                .query_row(
                    "SELECT goal_json FROM long_running_goals
                     WHERE goal_id = ?1 AND org_id = ?2 AND workspace_id = ?3
                       AND (deployment_id IS ?4 OR deployment_id = ?4)",
                    params![
                        goal_id,
                        tenant.org_id,
                        tenant.workspace_id,
                        tenant.deployment_id
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            payload
                .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
                .transpose()
        })
    }

    /// Tenant-scoped lineage read: run links join through their goal's scope
    /// columns so a goal ID alone can never cross a tenant boundary.
    pub fn list_goal_run_links_for_tenant(
        &self,
        tenant: &TenantContext,
        goal_id: &str,
    ) -> anyhow::Result<Vec<GoalRunLink>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT links.link_json FROM goal_run_links links
                 JOIN long_running_goals goals ON goals.goal_id = links.goal_id
                 WHERE links.goal_id = ?1 AND goals.org_id = ?2 AND goals.workspace_id = ?3
                   AND (goals.deployment_id IS ?4 OR goals.deployment_id = ?4)
                 ORDER BY links.hop_index",
            )?;
            let rows = statement.query_map(
                params![
                    goal_id,
                    tenant.org_id,
                    tenant.workspace_id,
                    tenant.deployment_id
                ],
                |row| row.get::<_, String>(0),
            )?;
            let mut links = Vec::new();
            for row in rows {
                links.push(serde_json::from_str(&row?)?);
            }
            Ok(links)
        })
    }

    pub fn list_goal_run_links(&self, goal_id: &str) -> anyhow::Result<Vec<GoalRunLink>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT link_json FROM goal_run_links WHERE goal_id = ?1 ORDER BY hop_index",
            )?;
            let rows = statement.query_map([goal_id], |row| row.get::<_, String>(0))?;
            let mut links = Vec::new();
            for row in rows {
                links.push(serde_json::from_str(&row?)?);
            }
            Ok(links)
        })
    }

    pub fn list_goal_handoffs(&self, goal_id: &str) -> anyhow::Result<Vec<WorkflowHandoff>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT handoff_json FROM workflow_handoffs
                 WHERE goal_id = ?1 ORDER BY created_at_ms, handoff_id",
            )?;
            let rows = statement.query_map([goal_id], |row| row.get::<_, String>(0))?;
            let mut handoffs = Vec::new();
            for row in rows {
                handoffs.push(serde_json::from_str(&row?)?);
            }
            Ok(handoffs)
        })
    }

    /// Tenant-scoped handoff listing for one goal.
    pub fn list_goal_handoffs_for_tenant(
        &self,
        tenant: &TenantContext,
        goal_id: &str,
    ) -> anyhow::Result<Vec<WorkflowHandoff>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT handoff_json FROM workflow_handoffs
                 WHERE goal_id = ?1 AND org_id = ?2 AND workspace_id = ?3
                   AND (deployment_id IS ?4 OR deployment_id = ?4)
                 ORDER BY created_at_ms, handoff_id",
            )?;
            let rows = statement.query_map(
                params![
                    goal_id,
                    tenant.org_id,
                    tenant.workspace_id,
                    tenant.deployment_id
                ],
                |row| row.get::<_, String>(0),
            )?;
            let mut handoffs = Vec::new();
            for row in rows {
                handoffs.push(serde_json::from_str(&row?)?);
            }
            Ok(handoffs)
        })
    }

    /// Tenant-scoped handoff read: a handoff ID from another tenant resolves
    /// to absence.
    pub fn get_workflow_handoff_for_tenant(
        &self,
        tenant: &TenantContext,
        handoff_id: &str,
    ) -> anyhow::Result<Option<WorkflowHandoff>> {
        self.with_connection(|connection| {
            let payload = connection
                .query_row(
                    "SELECT handoff_json FROM workflow_handoffs
                     WHERE handoff_id = ?1 AND org_id = ?2 AND workspace_id = ?3
                       AND (deployment_id IS ?4 OR deployment_id = ?4)",
                    params![
                        handoff_id,
                        tenant.org_id,
                        tenant.workspace_id,
                        tenant.deployment_id
                    ],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            payload
                .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
                .transpose()
        })
    }

    pub fn get_workflow_handoff(
        &self,
        handoff_id: &str,
    ) -> anyhow::Result<Option<WorkflowHandoff>> {
        self.with_connection(|connection| {
            let payload = connection
                .query_row(
                    "SELECT handoff_json FROM workflow_handoffs WHERE handoff_id = ?1",
                    [handoff_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?;
            payload
                .map(|payload| serde_json::from_str(&payload).map_err(Into::into))
                .transpose()
        })
    }

    /// Ordered durable events for one goal, strictly after `after_cursor`
    /// (the SQLite rowid). Bounded by `limit` so reconnect replay stays cheap;
    /// callers page until they receive fewer rows than the limit.
    pub fn query_goal_events(
        &self,
        goal_id: &str,
        after_cursor: Option<i64>,
        limit: usize,
    ) -> anyhow::Result<Vec<GoalEventRow>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT rowid, event_json FROM stateful_events
                 WHERE goal_id = ?1 AND rowid > ?2
                 ORDER BY rowid
                 LIMIT ?3",
            )?;
            let rows = statement.query_map(
                params![goal_id, after_cursor.unwrap_or(0), limit as i64],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?;
            let mut events = Vec::new();
            for row in rows {
                let (cursor, payload) = row?;
                events.push(GoalEventRow {
                    cursor,
                    event: serde_json::from_str(&payload)?,
                });
            }
            Ok(events)
        })
    }

    /// Tenant-scoped variant of [`Self::query_goal_events`]: the scope
    /// predicate runs in SQL, so replay reads fail closed at the store even
    /// if a caller forgets its own goal ownership check.
    pub fn query_goal_events_for_tenant(
        &self,
        tenant: &TenantContext,
        goal_id: &str,
        after_cursor: Option<i64>,
        limit: usize,
    ) -> anyhow::Result<Vec<GoalEventRow>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT rowid, event_json FROM stateful_events
                 WHERE goal_id = ?1 AND rowid > ?2
                   AND org_id = ?4 AND workspace_id = ?5
                   AND (deployment_id IS ?6 OR deployment_id = ?6)
                 ORDER BY rowid
                 LIMIT ?3",
            )?;
            let rows = statement.query_map(
                params![
                    goal_id,
                    after_cursor.unwrap_or(0),
                    limit as i64,
                    tenant.org_id,
                    tenant.workspace_id,
                    tenant.deployment_id
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?;
            let mut events = Vec::new();
            for row in rows {
                let (cursor, payload) = row?;
                events.push(GoalEventRow {
                    cursor,
                    event: serde_json::from_str(&payload)?,
                });
            }
            Ok(events)
        })
    }

    /// A bounded chronological window ending at `through_cursor`. This is the
    /// canonical projection replay read; the descending inner query avoids
    /// scanning an entire long-lived goal just to return its recent timeline.
    pub fn query_goal_event_window(
        &self,
        goal_id: &str,
        through_cursor: Option<i64>,
        limit: usize,
    ) -> anyhow::Result<Vec<GoalEventRow>> {
        self.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT rowid, event_json FROM (
                    SELECT rowid, event_json FROM stateful_events
                    WHERE goal_id = ?1 AND rowid <= ?2
                    ORDER BY rowid DESC LIMIT ?3
                 ) ORDER BY rowid",
            )?;
            let rows = statement.query_map(
                params![goal_id, through_cursor.unwrap_or(i64::MAX), limit as i64],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?;
            let mut events = Vec::new();
            for row in rows {
                let (cursor, payload) = row?;
                events.push(GoalEventRow {
                    cursor,
                    event: serde_json::from_str(&payload)?,
                });
            }
            Ok(events)
        })
    }

    pub fn goal_event_cursor_bounds(&self, goal_id: &str) -> anyhow::Result<Option<(i64, i64)>> {
        self.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT MIN(rowid), MAX(rowid) FROM stateful_events WHERE goal_id = ?1",
                    [goal_id],
                    |row| Ok((row.get::<_, Option<i64>>(0)?, row.get::<_, Option<i64>>(1)?)),
                )
                .map(|(minimum, maximum)| minimum.zip(maximum))
                .map_err(Into::into)
        })
    }
}

fn load_goal_for_update(
    transaction: &rusqlite::Transaction<'_>,
    goal_id: &str,
    tenant: &TenantContext,
) -> anyhow::Result<LongRunningGoal> {
    let payload = transaction
        .query_row(
            "SELECT goal_json FROM long_running_goals WHERE goal_id = ?1",
            [goal_id],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .context("long-running goal not found")?;
    let goal: LongRunningGoal = serde_json::from_str(&payload)?;
    let same_scope = goal.tenant_context.org_id == tenant.org_id
        && goal.tenant_context.workspace_id == tenant.workspace_id
        && goal.tenant_context.deployment_id == tenant.deployment_id;
    if !same_scope {
        bail!("goal is outside the caller tenant scope");
    }
    Ok(goal)
}

fn insert_goal_event(
    transaction: &rusqlite::Transaction<'_>,
    goal: &LongRunningGoal,
    actor: &PrincipalRef,
    now_ms: u64,
    event_type: &str,
    payload: serde_json::Value,
) -> anyhow::Result<()> {
    let run_id = format!("goal:{}", goal.goal_id);
    let mut event = StatefulRunEventRecord {
        schema_version: 1,
        event_id: format!("goal:{}:{}:{}", goal.goal_id, event_type, now_ms),
        run_id: run_id.clone(),
        seq: 0,
        event_type: event_type.to_string(),
        occurred_at_ms: now_ms,
        scope: StatefulRuntimeScope::from_tenant_context(goal.tenant_context.clone()),
        actor: Some(actor.clone()),
        phase_id: None,
        phase_transition: None,
        wait_kind: None,
        causation_id: None,
        correlation_id: Some(goal.goal_id.clone()),
        payload: payload_with_goal_snapshot(payload, goal),
    };
    event.seq = super::next_event_seq(transaction, &run_id)?;
    let event =
        super::runtime_records::event_with_projection_snapshot(transaction, &event, &goal.goal_id)?;
    transaction.execute(
        "INSERT INTO stateful_events
            (event_id, goal_id, run_id, seq, event_json, created_at_ms,
             org_id, workspace_id, deployment_id)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
         ON CONFLICT(event_id) DO NOTHING",
        params![
            event.event_id,
            goal.goal_id,
            event.run_id,
            event.seq,
            serde_json::to_string(&event)?,
            now_ms,
            goal.tenant_context.org_id,
            goal.tenant_context.workspace_id,
            goal.tenant_context.deployment_id,
        ],
    )?;
    Ok(())
}

fn payload_with_goal_snapshot(
    payload: serde_json::Value,
    goal: &LongRunningGoal,
) -> serde_json::Value {
    let mut object = payload.as_object().cloned().unwrap_or_default();
    object.insert("goal_snapshot".to_string(), json!(goal));
    serde_json::Value::Object(object)
}
