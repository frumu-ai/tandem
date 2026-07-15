// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use anyhow::{bail, Context};
use serde_json::json;
use tandem_automation::{
    AutomationRunCheckpoint, AutomationRunStatus, AutomationV2RunRecord, AutomationV2Spec,
    LongRunningGoal, OrchestrationNodeKind,
};

use super::AppState;
use crate::stateful_runtime::{
    cancel_stateful_wait_after_phase_guard_denial, load_stateful_waits, GovernedTransitionRequest,
    GovernedTransitionResult, OrchestrationStateStore, OrchestrationTransitionAuthority,
    StatefulRuntimeStoragePaths, WorkflowCompletionResult,
};

impl AppState {
    pub async fn settle_orchestration_workflow_completion(
        &self,
        goal_id: &str,
        transition_key: Option<&str>,
        final_artifact: Option<tandem_automation::OrchestrationArtifactRef>,
        authority: &OrchestrationTransitionAuthority,
    ) -> anyhow::Result<WorkflowCompletionResult> {
        let store =
            OrchestrationStateStore::from_automation_runs_path(&self.automation_v2_runs_path)?;
        let goal = store
            .get_goal(goal_id)?
            .context("long-running goal not found")?;
        let orchestration = store
            .get_orchestration_for_tenant(
                &goal.tenant_context,
                &goal.orchestration_id,
                goal.orchestration_version,
            )?
            .context("published orchestration version not found")?;
        let run_id = goal
            .active_run_id
            .as_deref()
            .context("goal has no active workflow run")?;
        let source_run = self
            .get_automation_v2_run(run_id)
            .await
            .context("goal active workflow run not found")?;
        store.settle_workflow_completion(
            &orchestration,
            &goal,
            &source_run,
            transition_key,
            final_artifact,
            authority,
            crate::util::time::now_ms(),
        )
    }

    /// Production orchestration transition entrypoint used by the upcoming HTTP
    /// and MCP surfaces. Validation and the durable commit happen before the
    /// in-memory scheduler sees the downstream run.
    pub async fn emit_orchestration_transition(
        &self,
        goal_id: &str,
        request: &GovernedTransitionRequest,
        authority: &OrchestrationTransitionAuthority,
    ) -> anyhow::Result<GovernedTransitionResult> {
        let store =
            OrchestrationStateStore::from_automation_runs_path(&self.automation_v2_runs_path)?;
        let goal = store
            .get_goal(goal_id)?
            .context("long-running goal not found")?;
        let orchestration = store
            .get_orchestration_for_tenant(
                &goal.tenant_context,
                &goal.orchestration_id,
                goal.orchestration_version,
            )?
            .context("published orchestration version not found")?;
        if let Some(replayed) =
            store.replay_existing_governed_transition(&orchestration, &goal, request, authority)?
        {
            return Ok(replayed);
        }
        let source_run_id = goal
            .active_run_id
            .as_deref()
            .context("goal has no active workflow run")?;
        let source_run = self
            .get_automation_v2_run(source_run_id)
            .await
            .context("goal active workflow run not found")?;
        let target_automation_id = transition_target_automation_id(
            &orchestration,
            goal.current_node_id.as_deref(),
            &request.transition_key,
        )?;
        let target_automation = self
            .get_automation_v2(target_automation_id)
            .await
            .context("target Automation V2 definition not found")?;
        let downstream_run = self
            .prepare_orchestration_downstream_run(&goal, &target_automation, request)
            .await?;
        let result = store.commit_governed_transition(
            &orchestration,
            &goal,
            &source_run,
            &downstream_run,
            request,
            authority,
        )?;
        if let GovernedTransitionResult::Committed {
            downstream_run,
            commit,
            ..
        } = &result
        {
            self.automation_v2_runs
                .write()
                .await
                .insert(downstream_run.run_id.clone(), downstream_run.clone());
            if let Err(error) = self.persist_automation_v2_runs().await {
                tracing::warn!(
                    %goal_id,
                    run_id = %downstream_run.run_id,
                    %error,
                    "durable orchestration transition committed; compatibility run persistence will recover from SQLite"
                );
            }
            if let Err(status) = crate::http::context_runs::sync_automation_v2_run_blackboard(
                self,
                &target_automation,
                downstream_run,
            )
            .await
            {
                tracing::warn!(
                    %goal_id,
                    run_id = %downstream_run.run_id,
                    ?status,
                    "durable orchestration transition committed; context blackboard synchronization will be retried"
                );
            }
            self.event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "orchestration.goal.transitioned",
                    &downstream_run.tenant_context,
                    json!({
                        "goalID": goal_id,
                        "run": downstream_run,
                        "commit": format!("{commit:?}"),
                    }),
                ));
        }
        Ok(result)
    }

    /// Applies durable cancellation first, then mirrors the outcome into the
    /// compatibility wait store and in-memory Automation V2 scheduler.
    pub async fn cancel_long_running_goal(
        &self,
        goal_id: &str,
        tenant: &tandem_types::TenantContext,
        reason: &str,
        actor: &tandem_types::PrincipalRef,
    ) -> anyhow::Result<crate::stateful_runtime::GoalCancellationResult> {
        let now_ms = crate::util::time::now_ms();
        let store =
            OrchestrationStateStore::from_automation_runs_path(&self.automation_v2_runs_path)?;
        let result = store.cancel_goal(goal_id, tenant, reason, actor, now_ms)?;
        for session_id in &result.cancelled_session_ids {
            let _ = self.cancellations.cancel(session_id).await;
        }
        for instance_id in &result.cancelled_instance_ids {
            let _ = self
                .agent_teams
                .cancel_instance(self, instance_id, reason)
                .await;
        }
        self.forget_automation_v2_sessions(&result.cancelled_session_ids)
            .await;
        if let Some(cancelled_run) = result.cancelled_run.as_ref() {
            self.automation_v2_runs
                .write()
                .await
                .insert(cancelled_run.run_id.clone(), cancelled_run.clone());
            let paths =
                StatefulRuntimeStoragePaths::from_runtime_events_path(&self.runtime_events_path);
            for wait in load_stateful_waits(&paths.waits_path)
                .into_iter()
                .filter(|wait| wait.run_id == cancelled_run.run_id)
            {
                let _ = cancel_stateful_wait_after_phase_guard_denial(
                    &paths.waits_path,
                    tenant,
                    &wait,
                    reason,
                    now_ms,
                )
                .await?;
            }
            self.persist_automation_v2_runs().await?;
            self.event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "orchestration.goal.cancelled",
                    tenant,
                    json!({
                        "goalID": goal_id,
                        "runID": cancelled_run.run_id,
                        "reason": reason,
                        "cancelledSessionIDs": &result.cancelled_session_ids,
                        "cancelledInstanceIDs": &result.cancelled_instance_ids,
                    }),
                ));
        }
        Ok(result)
    }

    async fn prepare_orchestration_downstream_run(
        &self,
        goal: &LongRunningGoal,
        automation: &AutomationV2Spec,
        request: &GovernedTransitionRequest,
    ) -> anyhow::Result<AutomationV2RunRecord> {
        self.prepare_orchestration_node_run(
            goal,
            automation,
            request.downstream_run_id(&goal.goal_id),
            request.now_ms,
            "orchestration_handoff",
            format!(
                "goal {} transition {}",
                goal.goal_id, request.transition_key
            ),
            Some(request.handoff_id(&goal.goal_id)),
        )
        .await
    }

    /// Shared builder for every Automation V2 run an orchestration creates —
    /// the goal's root run and each governed-transition downstream run.
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn prepare_orchestration_node_run(
        &self,
        goal: &LongRunningGoal,
        automation: &AutomationV2Spec,
        run_id: String,
        now_ms: u64,
        trigger_type: &str,
        trigger_reason: String,
        consumed_handoff_id: Option<String>,
    ) -> anyhow::Result<AutomationV2RunRecord> {
        let automation_tenant = automation.tenant_context();
        let same_scope = automation_tenant.org_id == goal.tenant_context.org_id
            && automation_tenant.workspace_id == goal.tenant_context.workspace_id
            && automation_tenant.deployment_id == goal.tenant_context.deployment_id;
        if !same_scope {
            bail!("target automation is outside the goal tenant scope");
        }
        self.validate_automation_enterprise_delegation_grants(automation)
            .await?;
        let runtime_context = self
            .orchestration_effective_runtime_context(
                automation,
                automation
                    .runtime_context_materialization()
                    .or_else(|| automation.approved_plan_runtime_context_materialization()),
            )
            .await?;
        let pending_nodes = automation
            .flow
            .nodes
            .iter()
            .map(|node| node.node_id.clone())
            .collect();
        let effective_execution_profile =
            crate::automation_v2::types::resolve_effective_execution_profile(automation, None);
        let mut run = AutomationV2RunRecord {
            run_id,
            automation_id: automation.automation_id.clone(),
            tenant_context: goal.tenant_context.clone(),
            trigger_type: trigger_type.to_string(),
            status: AutomationRunStatus::Queued,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            started_at_ms: None,
            finished_at_ms: None,
            active_session_ids: Vec::new(),
            latest_session_id: None,
            active_instance_ids: Vec::new(),
            checkpoint: AutomationRunCheckpoint {
                completed_nodes: Vec::new(),
                pending_nodes,
                node_outputs: std::collections::HashMap::new(),
                node_attempts: std::collections::HashMap::new(),
                node_attempt_verdicts: std::collections::HashMap::new(),
                blocked_nodes: Vec::new(),
                awaiting_gate: None,
                gate_history: Vec::new(),
                lifecycle_history: Vec::new(),
                last_failure: None,
            },
            runtime_context,
            automation_snapshot: Some(automation.clone()),
            workflow_definition_version: None,
            workflow_definition_snapshot_hash: None,
            execution_claim: None,
            execution_claim_epoch: 0,
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
            trigger_reason: Some(trigger_reason),
            consumed_handoff_id,
            learning_summary: None,
            effective_execution_profile,
            requested_execution_profile: None,
        };
        crate::stateful_runtime::ensure_automation_run_definition_metadata(&mut run);
        Ok(run)
    }

    async fn orchestration_effective_runtime_context(
        &self,
        automation: &AutomationV2Spec,
        base: Option<crate::AutomationRuntimeContextMaterialization>,
    ) -> anyhow::Result<Option<crate::AutomationRuntimeContextMaterialization>> {
        let shared = self
            .orchestration_shared_context_runtime_context(automation)
            .await?;
        Ok(merge_runtime_contexts(base, shared))
    }

    async fn orchestration_shared_context_runtime_context(
        &self,
        automation: &AutomationV2Spec,
    ) -> anyhow::Result<Option<crate::AutomationRuntimeContextMaterialization>> {
        let pack_ids = crate::http::context_packs::shared_context_pack_ids_from_metadata(
            automation.metadata.as_ref(),
        );
        let mut merged = None;
        for pack_id in pack_ids {
            let pack = self
                .get_context_pack(&pack_id)
                .await
                .with_context(|| format!("shared workflow context not found: {pack_id}"))?;
            if pack.state != crate::http::context_packs::ContextPackState::Published {
                bail!("shared workflow context is not published: {pack_id}");
            }
            let context = pack
                .manifest
                .runtime_context
                .clone()
                .and_then(|value| serde_json::from_value(value).ok())
                .or_else(|| {
                    pack.manifest
                        .plan_package
                        .as_ref()
                        .and_then(|value| {
                            serde_json::from_value::<tandem_plan_compiler::api::PlanPackage>(
                                value.clone(),
                            )
                            .ok()
                        })
                        .map(|plan| {
                            tandem_plan_compiler::api::project_plan_context_materialization(&plan)
                        })
                })
                .with_context(|| {
                    format!("shared workflow context lacks runtime context: {pack_id}")
                })?;
            merged = merge_runtime_contexts(merged, Some(context));
        }
        Ok(merged)
    }
}

fn merge_runtime_contexts(
    base: Option<crate::AutomationRuntimeContextMaterialization>,
    extra: Option<crate::AutomationRuntimeContextMaterialization>,
) -> Option<crate::AutomationRuntimeContextMaterialization> {
    let mut partitions = std::collections::BTreeMap::<
        String,
        tandem_plan_compiler::api::ProjectedRoutineContextPartition,
    >::new();
    for partition in base
        .into_iter()
        .chain(extra)
        .flat_map(|context| context.routines)
    {
        let entry = partitions
            .entry(partition.routine_id.clone())
            .or_insert_with(
                || tandem_plan_compiler::api::ProjectedRoutineContextPartition {
                    routine_id: partition.routine_id.clone(),
                    visible_context_objects: Vec::new(),
                    step_context_bindings: Vec::new(),
                },
            );
        let mut context_ids = entry
            .visible_context_objects
            .iter()
            .map(|context| context.context_object_id.clone())
            .collect::<std::collections::HashSet<_>>();
        entry.visible_context_objects.extend(
            partition
                .visible_context_objects
                .into_iter()
                .filter(|context| context_ids.insert(context.context_object_id.clone())),
        );
        entry
            .visible_context_objects
            .sort_by(|left, right| left.context_object_id.cmp(&right.context_object_id));
        let mut step_ids = entry
            .step_context_bindings
            .iter()
            .map(|binding| binding.step_id.clone())
            .collect::<std::collections::HashSet<_>>();
        entry.step_context_bindings.extend(
            partition
                .step_context_bindings
                .into_iter()
                .filter(|binding| step_ids.insert(binding.step_id.clone())),
        );
        entry
            .step_context_bindings
            .sort_by(|left, right| left.step_id.cmp(&right.step_id));
    }
    (!partitions.is_empty()).then(|| crate::AutomationRuntimeContextMaterialization {
        routines: partitions.into_values().collect(),
    })
}

fn transition_target_automation_id<'a>(
    orchestration: &'a tandem_automation::OrchestrationSpec,
    source_node_id: Option<&str>,
    transition_key: &str,
) -> anyhow::Result<&'a str> {
    let source_node_id = source_node_id.context("goal has no current orchestration node")?;
    let edge = orchestration
        .edges
        .iter()
        .find(|edge| edge.from_node_id == source_node_id && edge.transition_key == transition_key)
        .context("transition is not allowed from the goal current node")?;
    let target = orchestration
        .nodes
        .iter()
        .find(|node| node.node_id == edge.to_node_id)
        .context("transition target node is missing")?;
    match &target.node {
        OrchestrationNodeKind::Workflow { automation_id, .. } => Ok(automation_id),
        OrchestrationNodeKind::Wait { .. } => {
            bail!("workflow transition targets a wait node, not an automation")
        }
        OrchestrationNodeKind::Terminal { .. } => {
            bail!("terminal transition must use the terminal settlement path")
        }
    }
}

#[cfg(test)]
mod tests {
    use tandem_automation::{GoalPolicy, LongRunningGoalStatus};
    use tandem_types::{PrincipalKind, PrincipalRef, TenantContext};

    use super::*;
    use crate::app::state::tests::AutomationRunBuilder;

    fn transition_authority() -> OrchestrationTransitionAuthority {
        OrchestrationTransitionAuthority {
            actor: PrincipalRef::new(PrincipalKind::HumanUser, "operator"),
            can_emit: true,
            can_approve: false,
        }
    }

    fn transition_request() -> GovernedTransitionRequest {
        GovernedTransitionRequest {
            transition_key: "continue".to_string(),
            idempotency_key: "goal-1:plan:continue:1".to_string(),
            artifact: tandem_automation::OrchestrationArtifactRef {
                artifact_type: "plan".to_string(),
                content_path: None,
                content_digest: None,
                value: Some(serde_json::json!({"steps": ["ship"]})),
            },
            now_ms: 20,
        }
    }

    fn transition_orchestration(
        executor: &AutomationV2Spec,
    ) -> tandem_automation::OrchestrationSpec {
        let executor_hash = crate::stateful_runtime::automation_definition_snapshot_hash(executor);
        serde_json::from_value(serde_json::json!({
            "orchestration_id": "orch-1",
            "name": "Plan and execute",
            "status": "published",
            "version": 1,
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
                    "accepts_artifact_types": ["plan"],
                    "allowed_transition_keys": ["complete"]
                },
                {
                    "node_id": "complete",
                    "name": "Complete",
                    "kind": "terminal",
                    "outcome": "complete"
                }
            ],
            "edges": [
                {
                    "edge_id": "plan-execute",
                    "from_node_id": "plan",
                    "to_node_id": "execute",
                    "transition_key": "continue",
                    "artifact_contract": {"artifact_type": "plan", "required": true}
                },
                {
                    "edge_id": "execute-complete",
                    "from_node_id": "execute",
                    "to_node_id": "complete",
                    "transition_key": "complete"
                }
            ],
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

    fn transition_goal(tenant: TenantContext) -> LongRunningGoal {
        LongRunningGoal {
            schema_version: 1,
            goal_id: "goal-1".to_string(),
            orchestration_id: "orch-1".to_string(),
            orchestration_version: 1,
            objective: "Plan then execute".to_string(),
            status: LongRunningGoalStatus::Active,
            tenant_context: tenant,
            policy: GoalPolicy::default(),
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

    #[tokio::test]
    async fn cancellation_signals_live_sessions_after_the_durable_goal_transition() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = crate::test_support::test_state().await;
        state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
        let tenant = TenantContext::local_implicit();
        let mut run = AutomationRunBuilder::new("run-1", "executor")
            .status(AutomationRunStatus::Running)
            .build();
        run.active_session_ids = vec!["session-1".to_string()];
        run.latest_session_id = Some("session-1".to_string());
        run.active_instance_ids = vec!["instance-1".to_string()];
        state
            .automation_v2_runs
            .write()
            .await
            .insert(run.run_id.clone(), run.clone());
        let store =
            OrchestrationStateStore::from_automation_runs_path(&state.automation_v2_runs_path)
                .unwrap();
        store.upsert_automation_runs([&run]).unwrap();
        store
            .put_goal(&LongRunningGoal {
                schema_version: 1,
                goal_id: "goal-1".to_string(),
                orchestration_id: "orch-1".to_string(),
                orchestration_version: 1,
                objective: "Stop work".to_string(),
                status: LongRunningGoalStatus::Active,
                tenant_context: tenant.clone(),
                policy: GoalPolicy::default(),
                active_run_id: Some(run.run_id.clone()),
                current_node_id: Some("execute".to_string()),
                hop_count: 1,
                total_tokens: 0,
                total_cost_usd: 0.0,
                created_at_ms: 1,
                updated_at_ms: 1,
                finished_at_ms: None,
                final_artifact: None,
                metadata: None,
            })
            .unwrap();
        let cancellation = state.cancellations.create("session-1").await;

        let result = state
            .cancel_long_running_goal(
                "goal-1",
                &tenant,
                "operator cancelled",
                &PrincipalRef::new(PrincipalKind::HumanUser, "operator"),
            )
            .await
            .unwrap();

        assert!(cancellation.is_cancelled());
        assert_eq!(result.cancelled_session_ids, vec!["session-1"]);
        assert_eq!(result.cancelled_instance_ids, vec!["instance-1"]);
        let persisted = state.get_automation_v2_run("run-1").await.unwrap();
        assert_eq!(persisted.status, AutomationRunStatus::Cancelled);
        assert!(persisted.active_session_ids.is_empty());
        assert!(persisted.active_instance_ids.is_empty());
    }

    #[tokio::test]
    async fn transition_retry_replays_before_reading_the_advanced_goal_edge() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = crate::test_support::test_state().await;
        state.automation_v2_runs_path = directory.path().join("automation_v2_runs.json");
        let executor = state
            .put_automation_v2(
                crate::app::state::tests::AutomationSpecBuilder::new("executor").build(),
            )
            .await
            .unwrap();
        let mut source_run = AutomationRunBuilder::new("run-plan", "planner")
            .status(AutomationRunStatus::Completed)
            .build();
        source_run.total_tokens = 10;
        state
            .automation_v2_runs
            .write()
            .await
            .insert(source_run.run_id.clone(), source_run);
        let store =
            OrchestrationStateStore::from_automation_runs_path(&state.automation_v2_runs_path)
                .unwrap();
        let orchestration = transition_orchestration(&executor);
        store.put_orchestration(&orchestration).unwrap();
        store
            .put_goal(&transition_goal(TenantContext::local_implicit()))
            .unwrap();
        let request = transition_request();
        let authority = transition_authority();

        let first = state
            .emit_orchestration_transition("goal-1", &request, &authority)
            .await
            .unwrap();
        assert!(matches!(
            first,
            GovernedTransitionResult::Committed {
                commit: crate::stateful_runtime::AtomicHandoffCommit::Committed,
                ..
            }
        ));
        let second = state
            .emit_orchestration_transition("goal-1", &request, &authority)
            .await
            .unwrap();
        assert!(matches!(
            second,
            GovernedTransitionResult::Committed {
                commit: crate::stateful_runtime::AtomicHandoffCommit::AlreadyCommitted,
                ..
            }
        ));
    }
}
