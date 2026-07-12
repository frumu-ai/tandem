use crate::stateful_runtime::backend::{params, Executor, OptionalExtension, TransactionBehavior};
use anyhow::{bail, Context};
use serde_json::{json, Value};
use tandem_automation::{
    AutomationRunStatus, AutomationV2RunRecord, GoalRunLink, LongRunningGoal,
    LongRunningGoalStatus, OrchestrationArtifactRef, OrchestrationNodeKind, OrchestrationSpec,
    OrchestrationStatus, WorkflowHandoff, WorkflowHandoffStatus,
};
use tandem_types::{PrincipalRef, TenantContext};

use super::{protected_records, AtomicHandoffCommit, OrchestrationStateStore};
use crate::stateful_runtime::{
    automation_definition_snapshot_hash, stable_definition_snapshot_hash, StatefulRunEventRecord,
    StatefulRuntimeScope,
};

#[derive(Debug, Clone)]
pub struct OrchestrationTransitionAuthority {
    pub actor: PrincipalRef,
    pub can_emit: bool,
    pub can_approve: bool,
}

#[derive(Debug, Clone)]
pub struct GovernedTransitionRequest {
    pub transition_key: String,
    pub idempotency_key: String,
    pub artifact: OrchestrationArtifactRef,
    pub now_ms: u64,
}

impl GovernedTransitionRequest {
    pub fn handoff_id(&self, goal_id: &str) -> String {
        stable_transition_id("handoff", goal_id, &self.idempotency_key)
    }

    pub fn downstream_run_id(&self, goal_id: &str) -> String {
        stable_transition_id("automation-v2-run", goal_id, &self.idempotency_key)
    }
}

#[derive(Debug, Clone)]
pub enum GovernedTransitionResult {
    PendingApproval {
        handoff: WorkflowHandoff,
        goal: LongRunningGoal,
    },
    Committed {
        commit: AtomicHandoffCommit,
        handoff: WorkflowHandoff,
        downstream_run: AutomationV2RunRecord,
        link: GoalRunLink,
        goal: LongRunningGoal,
    },
}

#[derive(Debug, Clone)]
pub enum WorkflowCompletionResult {
    Waiting { goal: LongRunningGoal },
    Terminal { goal: LongRunningGoal },
}

impl OrchestrationStateStore {
    /// Returns the canonical result for a previously accepted idempotency key
    /// before callers derive a new source or target from a goal that may have
    /// advanced since the original request.
    pub fn replay_existing_governed_transition(
        &self,
        orchestration: &OrchestrationSpec,
        goal: &LongRunningGoal,
        request: &GovernedTransitionRequest,
        authority: &OrchestrationTransitionAuthority,
    ) -> anyhow::Result<Option<GovernedTransitionResult>> {
        if !authority.can_emit {
            bail!("actor is not authorized to emit orchestration transitions");
        }
        let Some(existing) =
            self.handoff_for_idempotency_key(&goal.goal_id, &request.idempotency_key)?
        else {
            return Ok(None);
        };
        validate_replayed_handoff(orchestration, goal, request, &existing)?;
        match existing.status {
            WorkflowHandoffStatus::Consumed => {
                self.replay_committed_transition(&existing).map(Some)
            }
            WorkflowHandoffStatus::PendingApproval => {
                let waiting_goal = self
                    .get_goal(&goal.goal_id)?
                    .context("pending handoff is missing its goal")?;
                Ok(Some(GovernedTransitionResult::PendingApproval {
                    handoff: existing,
                    goal: waiting_goal,
                }))
            }
            WorkflowHandoffStatus::Rejected | WorkflowHandoffStatus::DeadLettered => {
                bail!("handoff is no longer eligible for consumption")
            }
            WorkflowHandoffStatus::Claimed => {
                bail!("handoff is being claimed by another worker")
            }
            WorkflowHandoffStatus::Approved => Ok(None),
        }
    }

    pub fn settle_workflow_completion(
        &self,
        orchestration: &OrchestrationSpec,
        goal: &LongRunningGoal,
        source_run: &AutomationV2RunRecord,
        transition_key: Option<&str>,
        final_artifact: Option<OrchestrationArtifactRef>,
        authority: &OrchestrationTransitionAuthority,
        now_ms: u64,
    ) -> anyhow::Result<WorkflowCompletionResult> {
        if !authority.can_emit {
            bail!("actor is not authorized to settle orchestration workflow completion");
        }
        validate_goal_source(orchestration, goal, source_run)?;
        let source_node_id = goal
            .current_node_id
            .as_deref()
            .context("goal has no current orchestration node")?;
        let Some(transition_key) = transition_key else {
            let mut waiting = goal.clone();
            waiting.status = LongRunningGoalStatus::Waiting;
            waiting.total_tokens = waiting.total_tokens.saturating_add(source_run.total_tokens);
            waiting.total_cost_usd += source_run.estimated_cost_usd.max(0.0);
            waiting.updated_at_ms = now_ms;
            let event = completion_event(
                &waiting,
                authority,
                now_ms,
                &source_run.run_id,
                "stateful_runtime.goal.awaiting_transition",
                json!({"source_run_id": source_run.run_id}),
            );
            self.put_goal_with_event(&waiting, &event)?;
            return Ok(WorkflowCompletionResult::Waiting { goal: waiting });
        };
        let edge = orchestration
            .edges
            .iter()
            .find(|edge| {
                edge.from_node_id == source_node_id && edge.transition_key == transition_key
            })
            .context("transition key is not allowed from the completed workflow")?;
        let target = orchestration
            .nodes
            .iter()
            .find(|node| node.node_id == edge.to_node_id)
            .context("terminal transition target is missing")?;
        let OrchestrationNodeKind::Terminal {
            outcome,
            final_artifact_type,
        } = &target.node
        else {
            bail!("nonterminal workflow transitions require a downstream-run commit");
        };
        if let Some(artifact) = final_artifact.as_ref() {
            validate_artifact(edge.artifact_contract.as_ref(), artifact)?;
            if final_artifact_type
                .as_deref()
                .is_some_and(|expected| expected != artifact.artifact_type)
            {
                bail!("final artifact type does not match the published terminal node");
            }
        } else if edge
            .artifact_contract
            .as_ref()
            .is_some_and(|contract| contract.required)
        {
            bail!("terminal transition requires a final artifact");
        }
        let mut terminal = goal.clone();
        terminal.total_tokens = terminal
            .total_tokens
            .saturating_add(source_run.total_tokens);
        terminal.total_cost_usd += source_run.estimated_cost_usd.max(0.0);
        terminal.current_node_id = Some(target.node_id.clone());
        terminal.final_artifact = final_artifact;
        terminal.apply_terminal_outcome(outcome.clone(), now_ms);
        let event = completion_event(
            &terminal,
            authority,
            now_ms,
            &source_run.run_id,
            "stateful_runtime.goal.terminal",
            json!({
                "source_run_id": source_run.run_id,
                "transition_key": edge.transition_key,
                "target_node_id": target.node_id,
                "outcome": outcome,
            }),
        );
        self.put_goal_with_event(&terminal, &event)?;
        Ok(WorkflowCompletionResult::Terminal { goal: terminal })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn commit_governed_transition(
        &self,
        orchestration: &OrchestrationSpec,
        goal: &LongRunningGoal,
        source_run: &AutomationV2RunRecord,
        downstream_run: &AutomationV2RunRecord,
        request: &GovernedTransitionRequest,
        authority: &OrchestrationTransitionAuthority,
    ) -> anyhow::Result<GovernedTransitionResult> {
        if let Some(replayed) =
            self.replay_existing_governed_transition(orchestration, goal, request, authority)?
        {
            return Ok(replayed);
        }
        let existing_handoff =
            self.handoff_for_idempotency_key(&goal.goal_id, &request.idempotency_key)?;
        if let Some(existing) = existing_handoff.as_ref() {
            validate_replayed_handoff(orchestration, goal, request, existing)?;
            match existing.status {
                WorkflowHandoffStatus::Consumed => {
                    return self.replay_committed_transition(existing);
                }
                WorkflowHandoffStatus::PendingApproval => {
                    let waiting_goal = self
                        .get_goal(&goal.goal_id)?
                        .context("pending handoff is missing its goal")?;
                    return Ok(GovernedTransitionResult::PendingApproval {
                        handoff: existing.clone(),
                        goal: waiting_goal,
                    });
                }
                WorkflowHandoffStatus::Rejected | WorkflowHandoffStatus::DeadLettered => {
                    bail!("handoff is no longer eligible for consumption")
                }
                WorkflowHandoffStatus::Claimed => {
                    bail!("handoff is being claimed by another worker")
                }
                WorkflowHandoffStatus::Approved => {}
            }
        }
        validate_goal_source(orchestration, goal, source_run)?;
        let source_node_id = goal
            .current_node_id
            .as_deref()
            .context("goal has no current orchestration node")?;
        let source_node = orchestration
            .nodes
            .iter()
            .find(|node| node.node_id == source_node_id)
            .context("goal current node is missing from the published orchestration")?;
        let edge = orchestration
            .edges
            .iter()
            .find(|edge| {
                edge.from_node_id == source_node_id && edge.transition_key == request.transition_key
            })
            .context("transition key is not allowed from the current workflow")?;
        let target_node = orchestration
            .nodes
            .iter()
            .find(|node| node.node_id == edge.to_node_id)
            .context("transition target is missing from the published orchestration")?;
        let (target_automation_id, pinned_definition_hash) = match &target_node.node {
            OrchestrationNodeKind::Workflow {
                automation_id,
                pinned_definition_hash,
                accepts_artifact_types,
                ..
            } => {
                if !accepts_artifact_types.is_empty()
                    && !accepts_artifact_types.contains(&request.artifact.artifact_type)
                {
                    bail!("target workflow does not accept the handoff artifact type");
                }
                (
                    automation_id.as_str(),
                    pinned_definition_hash.as_deref().context(
                        "published workflow references require a pinned definition hash",
                    )?,
                )
            }
            OrchestrationNodeKind::Wait { .. } => {
                bail!("workflow completion cannot create a run for a wait node")
            }
            OrchestrationNodeKind::Terminal { .. } => {
                bail!("terminal transitions are settled without a downstream workflow run")
            }
        };
        validate_artifact(edge.artifact_contract.as_ref(), &request.artifact)?;
        validate_downstream_run(
            goal,
            downstream_run,
            target_automation_id,
            pinned_definition_hash,
            &request.downstream_run_id(&goal.goal_id),
        )?;

        let handoff = existing_handoff.unwrap_or_else(|| WorkflowHandoff {
            schema_version: 1,
            handoff_id: request.handoff_id(&goal.goal_id),
            idempotency_key: request.idempotency_key.clone(),
            goal_id: goal.goal_id.clone(),
            orchestration_id: goal.orchestration_id.clone(),
            orchestration_version: goal.orchestration_version,
            tenant_context: goal.tenant_context.clone(),
            edge_id: edge.edge_id.clone(),
            transition_key: edge.transition_key.clone(),
            source_automation_id: source_run.automation_id.clone(),
            source_run_id: source_run.run_id.clone(),
            source_node_id: source_node_id.to_string(),
            target_automation_id: target_automation_id.to_string(),
            target_node_id: target_node.node_id.clone(),
            artifact: request.artifact.clone(),
            status: if edge.approval.as_ref().is_some_and(|policy| policy.required)
                && !authority.can_approve
            {
                WorkflowHandoffStatus::PendingApproval
            } else {
                WorkflowHandoffStatus::Approved
            },
            created_at_ms: request.now_ms,
            updated_at_ms: request.now_ms,
            consumed_by_run_id: None,
            metadata: Some(json!({
                "actor": authority.actor,
                "pinned_definition_hash": pinned_definition_hash,
            })),
        });

        if handoff.status == WorkflowHandoffStatus::PendingApproval {
            let mut waiting_goal = goal.clone();
            waiting_goal.status = LongRunningGoalStatus::Waiting;
            waiting_goal.updated_at_ms = request.now_ms;
            self.put_pending_handoff(&handoff, &waiting_goal)?;
            return Ok(GovernedTransitionResult::PendingApproval {
                handoff,
                goal: waiting_goal,
            });
        }

        let admission = goal.admit_transition(
            request.now_ms,
            source_run.total_tokens,
            source_run.estimated_cost_usd,
        );
        if !admission.allowed {
            let mut limited_goal = goal.clone();
            if let Some(status) = admission.resulting_status {
                limited_goal.status = status;
            }
            limited_goal.updated_at_ms = request.now_ms;
            if limited_goal.status.is_terminal() {
                limited_goal.finished_at_ms = Some(request.now_ms);
                limited_goal.active_run_id = None;
            }
            self.put_goal(&limited_goal)?;
            bail!("goal policy rejected the transition: {:?}", admission.limit);
        }

        let mut updated_goal = goal.clone();
        updated_goal.status = LongRunningGoalStatus::Active;
        updated_goal.active_run_id = Some(downstream_run.run_id.clone());
        updated_goal.current_node_id = Some(target_node.node_id.clone());
        updated_goal.hop_count = updated_goal.hop_count.saturating_add(1);
        updated_goal.total_tokens = updated_goal
            .total_tokens
            .saturating_add(source_run.total_tokens);
        updated_goal.total_cost_usd += source_run.estimated_cost_usd.max(0.0);
        updated_goal.updated_at_ms = request.now_ms;
        let link = GoalRunLink {
            goal_id: goal.goal_id.clone(),
            run_id: downstream_run.run_id.clone(),
            orchestration_node_id: target_node.node_id.clone(),
            orchestration_version: goal.orchestration_version,
            hop_index: updated_goal.hop_count,
            parent_run_id: Some(source_run.run_id.clone()),
            triggering_handoff_id: Some(handoff.handoff_id.clone()),
            created_at_ms: request.now_ms,
        };
        let event = transition_event(&handoff, &updated_goal, authority, request.now_ms);
        let commit = self.commit_handoff_transition_with_event(
            &handoff,
            downstream_run,
            &link,
            &updated_goal,
            Some(&event),
        )?;
        Ok(GovernedTransitionResult::Committed {
            commit,
            handoff,
            downstream_run: downstream_run.clone(),
            link,
            goal: updated_goal,
        })
    }

    pub fn decide_pending_handoff(
        &self,
        handoff_id: &str,
        tenant: &TenantContext,
        approve: bool,
        authority: &OrchestrationTransitionAuthority,
        now_ms: u64,
    ) -> anyhow::Result<WorkflowHandoff> {
        if !authority.can_approve {
            bail!("actor is not authorized to decide orchestration handoffs");
        }
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let payload = transaction
                .query_row(
                    "SELECT handoff_json FROM workflow_handoffs WHERE handoff_id = ?1",
                    [handoff_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .context("handoff not found")?;
            let mut handoff: WorkflowHandoff =
                protected_records::decode(tenant, "handoff", handoff_id, &payload)?;
            if &handoff.tenant_context != tenant {
                bail!("handoff is outside the caller tenant scope");
            }
            let requested_status = if approve {
                WorkflowHandoffStatus::Approved
            } else {
                WorkflowHandoffStatus::Rejected
            };
            if handoff.status == requested_status {
                transaction.commit()?;
                return Ok(handoff);
            }
            if handoff.status != WorkflowHandoffStatus::PendingApproval {
                bail!("only pending handoffs can be approved or rejected");
            }
            handoff.status = requested_status;
            handoff.updated_at_ms = now_ms;
            handoff.metadata = Some(merge_metadata(
                handoff.metadata.take(),
                json!({"decision_actor": authority.actor, "approved": approve}),
            ));
            transaction.execute(
                "UPDATE workflow_handoffs SET status = ?2, handoff_json = ?3,
                    updated_at_ms = ?4 WHERE handoff_id = ?1",
                params![
                    handoff.handoff_id,
                    status_name(&handoff.status)?,
                    protected_records::encode(tenant, "handoff", &handoff.handoff_id, &handoff,)?,
                    now_ms,
                ],
            )?;
            let goal_payload = transaction.query_row(
                "SELECT goal_json FROM long_running_goals WHERE goal_id = ?1",
                [&handoff.goal_id],
                |row| row.get::<_, String>(0),
            )?;
            let goal: LongRunningGoal =
                protected_records::decode(tenant, "goal", &handoff.goal_id, &goal_payload)?;
            let run_id = format!("goal:{}", handoff.goal_id);
            let mut event = StatefulRunEventRecord {
                schema_version: 1,
                event_id: format!(
                    "{}:decision:{}",
                    handoff.handoff_id,
                    if approve { "approved" } else { "rejected" }
                ),
                run_id: run_id.clone(),
                seq: 0,
                event_type: "stateful_runtime.goal.handoff_decided".to_string(),
                occurred_at_ms: now_ms,
                scope: StatefulRuntimeScope::from_tenant_context(handoff.tenant_context.clone()),
                actor: Some(authority.actor.clone()),
                phase_id: None,
                phase_transition: None,
                wait_kind: None,
                causation_id: Some(handoff.handoff_id.clone()),
                correlation_id: Some(handoff.goal_id.clone()),
                payload: json!({
                    "goal_id": handoff.goal_id,
                    "handoff_id": handoff.handoff_id,
                    "approved": approve,
                    "goal_snapshot": goal,
                }),
            };
            event.seq = super::next_event_seq(&transaction, &run_id)?;
            let event = super::runtime_records::event_with_projection_snapshot(
                &transaction,
                &event,
                &handoff.goal_id,
            )?;
            transaction.execute(
                "INSERT INTO stateful_events
                    (event_id, goal_id, run_id, seq, event_json, created_at_ms,
                     org_id, workspace_id, deployment_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(event_id) DO NOTHING",
                params![
                    event.event_id,
                    handoff.goal_id,
                    event.run_id,
                    event.seq,
                    protected_records::encode(tenant, "event", &event.event_id, &event)?,
                    now_ms,
                    handoff.tenant_context.org_id,
                    handoff.tenant_context.workspace_id,
                    handoff.tenant_context.deployment_id,
                ],
            )?;
            transaction.commit()?;
            Ok(handoff)
        })
    }

    fn put_pending_handoff(
        &self,
        handoff: &WorkflowHandoff,
        waiting_goal: &LongRunningGoal,
    ) -> anyhow::Result<()> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let existing = transaction
                .query_row(
                    "SELECT handoff_id, handoff_json FROM workflow_handoffs
                     WHERE goal_id = ?1 AND idempotency_key = ?2",
                    params![handoff.goal_id, handoff.idempotency_key],
                    |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?;
            if let Some((existing_id, payload)) = existing {
                let current: WorkflowHandoff = protected_records::decode(
                    &handoff.tenant_context,
                    "handoff",
                    &existing_id,
                    &payload,
                )?;
                if current.handoff_id != handoff.handoff_id {
                    bail!("idempotency key is already bound to another handoff");
                }
                return Ok(());
            }
            transaction.execute(
                "INSERT INTO workflow_handoffs
                    (handoff_id, goal_id, idempotency_key, org_id, workspace_id,
                     deployment_id, source_run_id, target_automation_id, status,
                     consumed_by_run_id, handoff_json, created_at_ms, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'pending_approval',
                         NULL, ?9, ?10, ?11)",
                params![
                    handoff.handoff_id,
                    handoff.goal_id,
                    handoff.idempotency_key,
                    handoff.tenant_context.org_id,
                    handoff.tenant_context.workspace_id,
                    handoff.tenant_context.deployment_id,
                    handoff.source_run_id,
                    handoff.target_automation_id,
                    protected_records::encode(
                        &handoff.tenant_context,
                        "handoff",
                        &handoff.handoff_id,
                        handoff,
                    )?,
                    handoff.created_at_ms,
                    handoff.updated_at_ms,
                ],
            )?;
            super::upsert_goal(&transaction, waiting_goal)?;
            let run_id = format!("goal:{}", handoff.goal_id);
            let mut event = StatefulRunEventRecord {
                schema_version: 1,
                event_id: format!("{}:pending_approval", handoff.handoff_id),
                run_id: run_id.clone(),
                seq: 0,
                event_type: "stateful_runtime.goal.handoff_pending_approval".to_string(),
                occurred_at_ms: handoff.updated_at_ms,
                scope: StatefulRuntimeScope::from_tenant_context(handoff.tenant_context.clone()),
                actor: None,
                phase_id: None,
                phase_transition: None,
                wait_kind: Some(crate::stateful_runtime::StatefulWaitKind::Approval),
                causation_id: Some(handoff.source_run_id.clone()),
                correlation_id: Some(handoff.goal_id.clone()),
                payload: json!({
                    "goal_id": handoff.goal_id,
                    "handoff_id": handoff.handoff_id,
                    "goal_snapshot": waiting_goal,
                }),
            };
            event.seq = super::next_event_seq(&transaction, &run_id)?;
            let event = super::runtime_records::event_with_projection_snapshot(
                &transaction,
                &event,
                &handoff.goal_id,
            )?;
            transaction.execute(
                "INSERT INTO stateful_events
                    (event_id, goal_id, run_id, seq, event_json, created_at_ms,
                     org_id, workspace_id, deployment_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(event_id) DO NOTHING",
                params![
                    event.event_id,
                    handoff.goal_id,
                    event.run_id,
                    event.seq,
                    protected_records::encode(
                        &event.scope.tenant_context,
                        "event",
                        &event.event_id,
                        &event,
                    )?,
                    handoff.updated_at_ms,
                    handoff.tenant_context.org_id,
                    handoff.tenant_context.workspace_id,
                    handoff.tenant_context.deployment_id,
                ],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    fn put_goal_with_event(
        &self,
        goal: &LongRunningGoal,
        event: &StatefulRunEventRecord,
    ) -> anyhow::Result<()> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            super::upsert_goal(&transaction, goal)?;
            let mut event = event.clone();
            event.seq = super::next_event_seq(&transaction, &event.run_id)?;
            let event = super::runtime_records::event_with_projection_snapshot(
                &transaction,
                &event,
                &goal.goal_id,
            )?;
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
                    protected_records::encode(
                        &event.scope.tenant_context,
                        "event",
                        &event.event_id,
                        &event,
                    )?,
                    event.occurred_at_ms,
                    event.scope.tenant_context.org_id,
                    event.scope.tenant_context.workspace_id,
                    event.scope.tenant_context.deployment_id,
                ],
            )?;
            transaction.commit()?;
            Ok(())
        })
    }

    fn handoff_for_idempotency_key(
        &self,
        goal_id: &str,
        idempotency_key: &str,
    ) -> anyhow::Result<Option<WorkflowHandoff>> {
        self.with_connection(|connection| {
            let payload = connection
                .query_row(
                    "SELECT handoff_id, org_id, workspace_id, deployment_id, handoff_json
                     FROM workflow_handoffs
                     WHERE goal_id = ?1 AND idempotency_key = ?2",
                    params![goal_id, idempotency_key],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, Option<String>>(3)?,
                            row.get::<_, String>(4)?,
                        ))
                    },
                )
                .optional()?;
            payload
                .map(|(id, org, workspace, deployment, payload)| {
                    protected_records::decode_scoped(
                        &org,
                        &workspace,
                        deployment.as_deref(),
                        "handoff",
                        &id,
                        &payload,
                    )
                })
                .transpose()
        })
    }

    fn replay_committed_transition(
        &self,
        handoff: &WorkflowHandoff,
    ) -> anyhow::Result<GovernedTransitionResult> {
        let downstream_run_id = handoff
            .consumed_by_run_id
            .as_deref()
            .context("consumed handoff is missing its downstream run ID")?;
        self.with_connection(|connection| {
            let run_payload = connection
                .query_row(
                    "SELECT run_json FROM automation_runs WHERE run_id = ?1",
                    [downstream_run_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .context("consumed handoff downstream run is missing")?;
            let link_payload = connection
                .query_row(
                    "SELECT link_json FROM goal_run_links
                     WHERE goal_id = ?1 AND triggering_handoff_id = ?2",
                    params![handoff.goal_id, handoff.handoff_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .context("consumed handoff lineage link is missing")?;
            let goal_payload = connection
                .query_row(
                    "SELECT goal_json FROM long_running_goals WHERE goal_id = ?1",
                    [&handoff.goal_id],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .context("consumed handoff goal is missing")?;
            Ok(GovernedTransitionResult::Committed {
                commit: AtomicHandoffCommit::AlreadyCommitted,
                handoff: handoff.clone(),
                downstream_run: protected_records::decode(
                    &handoff.tenant_context,
                    "run",
                    downstream_run_id,
                    &run_payload,
                )?,
                link: protected_records::decode(
                    &handoff.tenant_context,
                    "link",
                    downstream_run_id,
                    &link_payload,
                )?,
                goal: protected_records::decode(
                    &handoff.tenant_context,
                    "goal",
                    &handoff.goal_id,
                    &goal_payload,
                )?,
            })
        })
    }
}

fn validate_goal_source(
    orchestration: &OrchestrationSpec,
    goal: &LongRunningGoal,
    source_run: &AutomationV2RunRecord,
) -> anyhow::Result<()> {
    if orchestration.status != OrchestrationStatus::Published {
        bail!("transitions require a published orchestration version");
    }
    if orchestration.orchestration_id != goal.orchestration_id
        || orchestration.version != goal.orchestration_version
    {
        bail!("goal is pinned to a different orchestration version");
    }
    if orchestration.tenant_context != goal.tenant_context
        || source_run.tenant_context != goal.tenant_context
    {
        bail!("orchestration transition crosses tenant scope");
    }
    if goal.status.is_terminal() || goal.status == LongRunningGoalStatus::Paused {
        bail!("terminal or paused goals reject new handoffs");
    }
    if goal.active_run_id.as_deref() != Some(source_run.run_id.as_str()) {
        bail!("source run is not the goal's active workflow run");
    }
    if source_run.status != AutomationRunStatus::Completed {
        bail!("workflow transitions require a completed source run");
    }
    let source_node = orchestration
        .nodes
        .iter()
        .find(|node| Some(node.node_id.as_str()) == goal.current_node_id.as_deref())
        .context("goal current node is missing")?;
    match &source_node.node {
        OrchestrationNodeKind::Workflow {
            automation_id,
            allowed_transition_keys,
            ..
        } if automation_id == &source_run.automation_id => {
            if allowed_transition_keys.is_empty() {
                bail!("source workflow declares no transition outcomes");
            }
        }
        _ => bail!("source run does not match the goal's current workflow node"),
    }
    Ok(())
}

fn validate_replayed_handoff(
    orchestration: &OrchestrationSpec,
    goal: &LongRunningGoal,
    request: &GovernedTransitionRequest,
    handoff: &WorkflowHandoff,
) -> anyhow::Result<()> {
    if handoff.handoff_id != request.handoff_id(&goal.goal_id)
        || handoff.goal_id != goal.goal_id
        || handoff.orchestration_id != goal.orchestration_id
        || handoff.orchestration_version != goal.orchestration_version
        || handoff.tenant_context != goal.tenant_context
        || orchestration.orchestration_id != goal.orchestration_id
        || orchestration.version != goal.orchestration_version
        || orchestration.tenant_context != goal.tenant_context
    {
        bail!("idempotency key is bound to another orchestration transition");
    }
    if handoff.transition_key != request.transition_key || handoff.artifact != request.artifact {
        bail!("idempotency key is bound to a different transition payload");
    }
    Ok(())
}

fn validate_downstream_run(
    goal: &LongRunningGoal,
    run: &AutomationV2RunRecord,
    target_automation_id: &str,
    pinned_hash: &str,
    expected_run_id: &str,
) -> anyhow::Result<()> {
    if run.run_id != expected_run_id {
        bail!("downstream run ID must be derived from the transition idempotency key");
    }
    if run.status != AutomationRunStatus::Queued || run.automation_id != target_automation_id {
        bail!("downstream run must be a queued run for the published target workflow");
    }
    if run.tenant_context != goal.tenant_context {
        bail!("downstream run crosses the goal tenant scope");
    }
    let snapshot = run
        .automation_snapshot
        .as_ref()
        .context("downstream run requires an immutable automation snapshot")?;
    let actual_hash = automation_definition_snapshot_hash(snapshot);
    if actual_hash != pinned_hash {
        bail!("target workflow definition is stale; revalidate and republish the orchestration");
    }
    Ok(())
}

/// Inline artifact values (and their serialized handoffs) are bounded so a
/// single transition cannot balloon the durable store or downstream prompts.
pub const MAX_ARTIFACT_VALUE_BYTES: usize = 256 * 1024;

/// Structural admission policy applied to every emitted artifact, contract or
/// not: bounded inline payloads, workspace-relative content paths, and digest
/// provenance for file-backed content. Filesystem resolution (symlink escape,
/// digest-vs-content) happens at the API surfaces where a workspace root is
/// known; this layer rejects everything that is invalid on shape alone.
pub(super) fn validate_artifact_shape(artifact: &OrchestrationArtifactRef) -> anyhow::Result<()> {
    if let Some(value) = artifact.value.as_ref() {
        let bytes = serde_json::to_vec(value)?.len();
        if bytes > MAX_ARTIFACT_VALUE_BYTES {
            bail!(
                "handoff artifact value is {bytes} bytes; the admission bound is {MAX_ARTIFACT_VALUE_BYTES} bytes — store large content in the workspace and reference it by content_path + digest"
            );
        }
    }
    if let Some(content_path) = artifact.content_path.as_deref() {
        let path = std::path::Path::new(content_path);
        if path.is_absolute()
            || path
                .components()
                .any(|component| matches!(component, std::path::Component::ParentDir))
        {
            bail!("handoff artifact content_path `{content_path}` escapes the workspace root");
        }
        if artifact.content_digest.is_none() {
            bail!("file-backed handoff artifacts require a content_digest for provenance");
        }
    }
    if let Some(digest) = artifact.content_digest.as_deref() {
        let hex = digest.strip_prefix("sha256:").unwrap_or(digest);
        if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            bail!("handoff artifact content_digest must be a sha256 hex digest");
        }
    }
    Ok(())
}

fn validate_artifact(
    contract: Option<&tandem_automation::OrchestrationArtifactContract>,
    artifact: &OrchestrationArtifactRef,
) -> anyhow::Result<()> {
    validate_artifact_shape(artifact)?;
    let Some(contract) = contract else {
        return Ok(());
    };
    if artifact.artifact_type != contract.artifact_type {
        bail!("handoff artifact type does not match the published edge contract");
    }
    if contract.required
        && artifact.value.is_none()
        && artifact.content_path.is_none()
        && artifact.content_digest.is_none()
    {
        bail!("required handoff artifact has no value or durable content reference");
    }
    if let (Some(schema), Some(value)) = (contract.schema.as_ref(), artifact.value.as_ref()) {
        validate_schema_value(schema, value, "$")?;
    }
    Ok(())
}

fn validate_schema_value(schema: &Value, value: &Value, path: &str) -> anyhow::Result<()> {
    if let Some(expected) = schema.get("type").and_then(Value::as_str) {
        let matches = match expected {
            "object" => value.is_object(),
            "array" => value.is_array(),
            "string" => value.is_string(),
            "number" => value.is_number(),
            "integer" => value.as_i64().is_some() || value.as_u64().is_some(),
            "boolean" => value.is_boolean(),
            "null" => value.is_null(),
            _ => false,
        };
        if !matches {
            bail!("artifact schema mismatch at {path}: expected {expected}");
        }
    }
    if let (Some(object), Some(required)) = (
        value.as_object(),
        schema.get("required").and_then(Value::as_array),
    ) {
        for key in required.iter().filter_map(Value::as_str) {
            if !object.contains_key(key) {
                bail!("artifact schema mismatch at {path}: missing required property {key}");
            }
        }
    }
    if let (Some(object), Some(properties)) = (
        value.as_object(),
        schema.get("properties").and_then(Value::as_object),
    ) {
        for (key, child_schema) in properties {
            if let Some(child) = object.get(key) {
                validate_schema_value(child_schema, child, &format!("{path}.{key}"))?;
            }
        }
    }
    Ok(())
}

fn transition_event(
    handoff: &WorkflowHandoff,
    goal: &LongRunningGoal,
    authority: &OrchestrationTransitionAuthority,
    now_ms: u64,
) -> StatefulRunEventRecord {
    StatefulRunEventRecord {
        schema_version: 1,
        event_id: format!("{}:transition", handoff.handoff_id),
        run_id: format!("goal:{}", goal.goal_id),
        seq: 0,
        event_type: "stateful_runtime.goal.transitioned".to_string(),
        occurred_at_ms: now_ms,
        scope: StatefulRuntimeScope::from_tenant_context(goal.tenant_context.clone()),
        actor: Some(authority.actor.clone()),
        phase_id: None,
        phase_transition: None,
        wait_kind: None,
        causation_id: Some(handoff.source_run_id.clone()),
        correlation_id: Some(goal.goal_id.clone()),
        payload: json!({
            "goal_id": goal.goal_id,
            "handoff_id": handoff.handoff_id,
            "edge_id": handoff.edge_id,
            "transition_key": handoff.transition_key,
            "target_node_id": handoff.target_node_id,
            "goal_snapshot": goal,
        }),
    }
}

fn completion_event(
    goal: &LongRunningGoal,
    authority: &OrchestrationTransitionAuthority,
    now_ms: u64,
    source_run_id: &str,
    event_type: &str,
    payload: Value,
) -> StatefulRunEventRecord {
    StatefulRunEventRecord {
        schema_version: 1,
        event_id: format!(
            "goal:{}:{}:{}",
            goal.goal_id,
            source_run_id,
            event_type.rsplit('.').next().unwrap_or("completion")
        ),
        run_id: format!("goal:{}", goal.goal_id),
        seq: 0,
        event_type: event_type.to_string(),
        occurred_at_ms: now_ms,
        scope: StatefulRuntimeScope::from_tenant_context(goal.tenant_context.clone()),
        actor: Some(authority.actor.clone()),
        phase_id: None,
        phase_transition: None,
        wait_kind: None,
        causation_id: None,
        correlation_id: Some(goal.goal_id.clone()),
        payload: payload_with_goal_snapshot(payload, goal),
    }
}

fn payload_with_goal_snapshot(payload: Value, goal: &LongRunningGoal) -> Value {
    let mut object = payload.as_object().cloned().unwrap_or_default();
    object.insert("goal_snapshot".to_string(), json!(goal));
    Value::Object(object)
}

fn stable_transition_id(prefix: &str, goal_id: &str, idempotency_key: &str) -> String {
    let hash = stable_definition_snapshot_hash(&json!({
        "goal_id": goal_id,
        "idempotency_key": idempotency_key,
        "kind": prefix,
    }));
    format!(
        "{prefix}-{}",
        hash.trim_start_matches("sha256:")
            .chars()
            .take(32)
            .collect::<String>()
    )
}

fn status_name(status: &WorkflowHandoffStatus) -> anyhow::Result<String> {
    serde_json::to_value(status)?
        .as_str()
        .map(str::to_string)
        .context("handoff status must serialize as a string")
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
    use tandem_automation::{AutomationV2Spec, GoalLimitAction, GoalPolicy, LongRunningGoalStatus};
    use tandem_types::{PrincipalKind, PrincipalRef};

    use super::*;
    use crate::stateful_runtime::OrchestrationStorePaths;

    fn automation(id: &str) -> AutomationV2Spec {
        serde_json::from_value(json!({
            "automation_id": id,
            "name": id,
            "status": "active",
            "schedule": {
                "type": "manual",
                "timezone": "UTC",
                "misfire_policy": {"type": "skip"}
            },
            "flow": {"nodes": []},
            "execution": {},
            "created_at_ms": 1,
            "updated_at_ms": 1,
            "creator_id": "test"
        }))
        .unwrap()
    }

    fn source_run() -> AutomationV2RunRecord {
        let mut run: AutomationV2RunRecord = serde_json::from_value(json!({
            "run_id": "run-plan",
            "automation_id": "planner",
            "trigger_type": "goal_start",
            "status": "completed",
            "created_at_ms": 1,
            "updated_at_ms": 10,
            "checkpoint": {}
        }))
        .unwrap();
        run.total_tokens = 25;
        run.estimated_cost_usd = 0.5;
        run
    }

    fn goal() -> LongRunningGoal {
        LongRunningGoal {
            schema_version: 1,
            goal_id: "goal-1".to_string(),
            orchestration_id: "orch-1".to_string(),
            orchestration_version: 3,
            objective: "Plan then execute".to_string(),
            status: LongRunningGoalStatus::Active,
            tenant_context: TenantContext::local_implicit(),
            policy: GoalPolicy {
                max_hops: 5,
                deadline_at_ms: None,
                max_total_tokens: Some(100),
                max_total_cost_usd: Some(10.0),
                on_limit: GoalLimitAction::PauseForReview,
            },
            active_run_id: Some("run-plan".to_string()),
            current_node_id: Some("plan".to_string()),
            hop_count: 0,
            total_tokens: 0,
            total_cost_usd: 0.0,
            created_at_ms: 1,
            updated_at_ms: 1,
            finished_at_ms: None,
            final_artifact: None,
            metadata: None,
        }
    }

    fn orchestration(approval_required: bool) -> OrchestrationSpec {
        let executor = automation("executor");
        let executor_hash = automation_definition_snapshot_hash(&executor);
        serde_json::from_value(json!({
            "orchestration_id": "orch-1",
            "name": "Plan and execute",
            "status": "published",
            "version": 3,
            "root_node_id": "plan",
            "nodes": [
                {
                    "node_id": "plan",
                    "name": "Plan",
                    "kind": "workflow",
                    "automation_id": "planner",
                    "pinned_definition_hash": "sha256:planner",
                    "allowed_transition_keys": ["continue"],
                    "emits_artifact_types": ["plan"]
                },
                {
                    "node_id": "execute",
                    "name": "Execute",
                    "kind": "workflow",
                    "automation_id": "executor",
                    "pinned_definition_hash": executor_hash,
                    "allowed_transition_keys": ["complete"],
                    "accepts_artifact_types": ["plan"]
                }
            ],
            "edges": [{
                "edge_id": "plan-execute",
                "from_node_id": "plan",
                "to_node_id": "execute",
                "transition_key": "continue",
                "artifact_contract": {
                    "artifact_type": "plan",
                    "required": true,
                    "schema": {
                        "type": "object",
                        "required": ["steps"],
                        "properties": {"steps": {"type": "array"}}
                    }
                },
                "approval": {"required": approval_required}
            }],
            "goal_policy": {"max_hops": 5},
            "tenant_context": {
                "org_id": "local",
                "workspace_id": "local",
                "source": "local_implicit"
            },
            "created_at_ms": 1,
            "updated_at_ms": 1,
            "published_at_ms": 1
        }))
        .unwrap()
    }

    fn terminal_orchestration() -> OrchestrationSpec {
        serde_json::from_value(json!({
            "orchestration_id": "orch-1",
            "name": "Plan and finish",
            "status": "published",
            "version": 3,
            "root_node_id": "plan",
            "nodes": [
                {
                    "node_id": "plan",
                    "name": "Plan",
                    "kind": "workflow",
                    "automation_id": "planner",
                    "pinned_definition_hash": "sha256:planner",
                    "allowed_transition_keys": ["complete"],
                    "emits_artifact_types": ["plan"]
                },
                {
                    "node_id": "complete",
                    "name": "Complete",
                    "kind": "terminal",
                    "outcome": "complete",
                    "final_artifact_type": "plan"
                }
            ],
            "edges": [{
                "edge_id": "plan-complete",
                "from_node_id": "plan",
                "to_node_id": "complete",
                "transition_key": "complete",
                "artifact_contract": {"artifact_type": "plan", "required": true}
            }],
            "goal_policy": {"max_hops": 5},
            "tenant_context": {
                "org_id": "local",
                "workspace_id": "local",
                "source": "local_implicit"
            },
            "created_at_ms": 1,
            "updated_at_ms": 1,
            "published_at_ms": 1
        }))
        .unwrap()
    }

    fn request() -> GovernedTransitionRequest {
        GovernedTransitionRequest {
            transition_key: "continue".to_string(),
            idempotency_key: "goal-1:plan:continue:1".to_string(),
            artifact: OrchestrationArtifactRef {
                artifact_type: "plan".to_string(),
                content_path: None,
                content_digest: None,
                value: Some(json!({"steps": ["ship"]})),
            },
            now_ms: 20,
        }
    }

    fn downstream(request: &GovernedTransitionRequest) -> AutomationV2RunRecord {
        let executor = automation("executor");
        let mut run: AutomationV2RunRecord = serde_json::from_value(json!({
            "run_id": request.downstream_run_id("goal-1"),
            "automation_id": "executor",
            "trigger_type": "orchestration_handoff",
            "status": "queued",
            "created_at_ms": 20,
            "updated_at_ms": 20,
            "checkpoint": {}
        }))
        .unwrap();
        run.automation_snapshot = Some(executor);
        run
    }

    fn authority(can_approve: bool) -> OrchestrationTransitionAuthority {
        OrchestrationTransitionAuthority {
            actor: PrincipalRef::new(PrincipalKind::HumanUser, "reviewer"),
            can_emit: true,
            can_approve,
        }
    }

    fn store() -> (tempfile::TempDir, OrchestrationStateStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = OrchestrationStateStore::open(OrchestrationStorePaths {
            database_path: directory.path().join("runtime.sqlite3"),
            engine_lock_path: directory.path().join("engine.lock"),
        })
        .unwrap();
        (directory, store)
    }

    #[test]
    fn governed_transition_is_atomic_idempotent_and_accounts_usage() {
        let (_directory, store) = store();
        let goal = goal();
        store.put_goal(&goal).unwrap();
        let request = request();
        let downstream = downstream(&request);
        let first = store
            .commit_governed_transition(
                &orchestration(false),
                &goal,
                &source_run(),
                &downstream,
                &request,
                &authority(false),
            )
            .unwrap();
        assert!(matches!(
            first,
            GovernedTransitionResult::Committed {
                commit: AtomicHandoffCommit::Committed,
                ..
            }
        ));
        let replayed_goal = store.get_goal("goal-1").unwrap().unwrap();
        let second = store
            .commit_governed_transition(
                &orchestration(false),
                &replayed_goal,
                &source_run(),
                &downstream,
                &request,
                &authority(false),
            )
            .unwrap();
        assert!(matches!(
            second,
            GovernedTransitionResult::Committed {
                commit: AtomicHandoffCommit::AlreadyCommitted,
                ..
            }
        ));
        let mut conflicting_request = request.clone();
        conflicting_request.artifact.value = Some(json!({"steps": ["rewrite"]}));
        assert!(store
            .commit_governed_transition(
                &orchestration(false),
                &replayed_goal,
                &source_run(),
                &downstream,
                &conflicting_request,
                &authority(false),
            )
            .is_err());
        let updated = store.get_goal("goal-1").unwrap().unwrap();
        assert_eq!(updated.hop_count, 1);
        assert_eq!(updated.total_tokens, 25);
        assert_eq!(updated.total_cost_usd, 0.5);
        assert_eq!(
            updated.active_run_id.as_deref(),
            Some(downstream.run_id.as_str())
        );
        store
            .with_connection(|connection| {
                let events: u64 = connection.query_row(
                    "SELECT COUNT(*) FROM stateful_events WHERE goal_id = 'goal-1'",
                    [],
                    |row| row.get(0),
                )?;
                assert_eq!(events, 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn approval_handoff_must_be_decided_before_consumption() {
        let (_directory, store) = store();
        let goal = goal();
        store.put_goal(&goal).unwrap();
        let request = request();
        let downstream = downstream(&request);
        let pending = store
            .commit_governed_transition(
                &orchestration(true),
                &goal,
                &source_run(),
                &downstream,
                &request,
                &authority(false),
            )
            .unwrap();
        assert!(matches!(
            pending,
            GovernedTransitionResult::PendingApproval { .. }
        ));
        let waiting_goal = store.get_goal("goal-1").unwrap().unwrap();
        assert_eq!(waiting_goal.status, LongRunningGoalStatus::Waiting);
        let approved = store
            .decide_pending_handoff(
                &request.handoff_id("goal-1"),
                &TenantContext::local_implicit(),
                true,
                &authority(true),
                21,
            )
            .unwrap();
        assert_eq!(approved.status, WorkflowHandoffStatus::Approved);
        let committed = store
            .commit_governed_transition(
                &orchestration(true),
                &waiting_goal,
                &source_run(),
                &downstream,
                &request,
                &authority(false),
            )
            .unwrap();
        assert!(matches!(
            committed,
            GovernedTransitionResult::Committed {
                commit: AtomicHandoffCommit::Committed,
                ..
            }
        ));
    }

    #[test]
    fn stale_definition_and_cross_tenant_runs_fail_closed() {
        let (_directory, store) = store();
        let goal = goal();
        let request = request();
        let mut stale = downstream(&request);
        stale.automation_snapshot = Some(automation("changed-executor"));
        assert!(store
            .commit_governed_transition(
                &orchestration(false),
                &goal,
                &source_run(),
                &stale,
                &request,
                &authority(false),
            )
            .is_err());

        let mut cross_tenant = downstream(&request);
        cross_tenant.tenant_context = TenantContext::explicit("other", "other", None);
        assert!(store
            .commit_governed_transition(
                &orchestration(false),
                &goal,
                &source_run(),
                &cross_tenant,
                &request,
                &authority(false),
            )
            .is_err());
    }

    #[test]
    fn stale_transition_cannot_revive_a_cancelled_goal() {
        let (_directory, store) = store();
        let goal = goal();
        store.put_goal(&goal).unwrap();
        let mut cancelled = goal.clone();
        cancelled.status = LongRunningGoalStatus::Cancelled;
        cancelled.active_run_id = None;
        cancelled.finished_at_ms = Some(21);
        store.put_goal(&cancelled).unwrap();
        let request = request();

        assert!(store
            .commit_governed_transition(
                &orchestration(false),
                &goal,
                &source_run(),
                &downstream(&request),
                &request,
                &authority(false),
            )
            .is_err());
        assert_eq!(
            store.get_goal("goal-1").unwrap().unwrap().status,
            LongRunningGoalStatus::Cancelled
        );
    }

    #[test]
    fn workflow_completion_waits_without_an_outcome_and_settles_explicit_terminal() {
        let (_directory, waiting_store) = store();
        assert!(waiting_store
            .settle_workflow_completion(
                &terminal_orchestration(),
                &goal(),
                &source_run(),
                Some("invented"),
                None,
                &authority(false),
                19,
            )
            .is_err());
        let waiting = waiting_store
            .settle_workflow_completion(
                &terminal_orchestration(),
                &goal(),
                &source_run(),
                None,
                None,
                &authority(false),
                20,
            )
            .unwrap();
        assert!(matches!(
            waiting,
            WorkflowCompletionResult::Waiting { ref goal }
                if goal.status == LongRunningGoalStatus::Waiting
        ));
        assert_eq!(
            waiting_store
                .cancel_goal(
                    "goal-1",
                    &TenantContext::local_implicit(),
                    "operator cancelled waiting goal",
                    &authority(false).actor,
                    21,
                )
                .unwrap()
                .outcome,
            crate::stateful_runtime::GoalControlOutcome::Applied
        );

        let (_directory, terminal_store) = store();
        let terminal = terminal_store
            .settle_workflow_completion(
                &terminal_orchestration(),
                &goal(),
                &source_run(),
                Some("complete"),
                Some(OrchestrationArtifactRef {
                    artifact_type: "plan".to_string(),
                    content_path: None,
                    content_digest: None,
                    value: Some(json!({"steps": ["ship"]})),
                }),
                &authority(false),
                20,
            )
            .unwrap();
        assert!(matches!(
            terminal,
            WorkflowCompletionResult::Terminal { ref goal }
                if goal.status == LongRunningGoalStatus::Completed
                    && goal.active_run_id.is_none()
                    && goal.final_artifact.is_some()
        ));
    }

    #[test]
    fn fake_clock_policy_limits_survive_restart_without_creating_a_handoff() {
        let cases = [
            (
                "hop",
                GoalPolicy {
                    max_hops: 5,
                    deadline_at_ms: None,
                    max_total_tokens: None,
                    max_total_cost_usd: None,
                    on_limit: GoalLimitAction::PauseForReview,
                },
                5,
                0,
                0.0,
                20,
                LongRunningGoalStatus::Paused,
            ),
            (
                "deadline",
                GoalPolicy {
                    max_hops: 5,
                    deadline_at_ms: Some(20),
                    max_total_tokens: None,
                    max_total_cost_usd: None,
                    on_limit: GoalLimitAction::PauseForReview,
                },
                0,
                0,
                0.0,
                20,
                LongRunningGoalStatus::Expired,
            ),
            (
                "tokens",
                GoalPolicy {
                    max_hops: 5,
                    deadline_at_ms: None,
                    max_total_tokens: Some(24),
                    max_total_cost_usd: None,
                    on_limit: GoalLimitAction::Fail,
                },
                0,
                0,
                0.0,
                20,
                LongRunningGoalStatus::Failed,
            ),
            (
                "cost",
                GoalPolicy {
                    max_hops: 5,
                    deadline_at_ms: None,
                    max_total_tokens: None,
                    max_total_cost_usd: Some(0.49),
                    on_limit: GoalLimitAction::Fail,
                },
                0,
                0,
                0.0,
                20,
                LongRunningGoalStatus::Failed,
            ),
        ];
        for (name, policy, hop_count, total_tokens, total_cost_usd, now_ms, expected_status) in
            cases
        {
            let directory = tempfile::tempdir().unwrap();
            let paths = OrchestrationStorePaths {
                database_path: directory.path().join(format!("{name}.sqlite3")),
                engine_lock_path: directory.path().join(format!("{name}.lock")),
            };
            let mut persisted_goal = goal();
            persisted_goal.policy = policy;
            persisted_goal.hop_count = hop_count;
            persisted_goal.total_tokens = total_tokens;
            persisted_goal.total_cost_usd = total_cost_usd;
            OrchestrationStateStore::open(paths.clone())
                .unwrap()
                .put_goal(&persisted_goal)
                .unwrap();

            let restarted_store = OrchestrationStateStore::open(paths).unwrap();
            let restored_goal = restarted_store.get_goal("goal-1").unwrap().unwrap();
            let mut request = request();
            request.now_ms = now_ms;
            assert!(
                restarted_store
                    .commit_governed_transition(
                        &orchestration(false),
                        &restored_goal,
                        &source_run(),
                        &downstream(&request),
                        &request,
                        &authority(false),
                    )
                    .is_err(),
                "{name} limit should reject the transition"
            );
            let limited_goal = restarted_store.get_goal("goal-1").unwrap().unwrap();
            assert_eq!(limited_goal.status, expected_status, "{name} status");
            assert!(restarted_store
                .get_automation_run(&request.downstream_run_id("goal-1"))
                .unwrap()
                .is_none());
            if limited_goal.status.is_terminal() {
                assert_eq!(limited_goal.finished_at_ms, Some(now_ms));
                assert!(limited_goal.active_run_id.is_none());
            }
        }
    }
}
