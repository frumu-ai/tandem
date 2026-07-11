//! App-layer goal lifecycle: start from a published orchestration version,
//! operator pause/resume. The durable write happens in the stateful store's
//! single transaction; only afterwards is the root run mirrored into the
//! in-memory Automation V2 scheduler and announced on the event bus, so a
//! crash between the two leaves a consistent database the scheduler can
//! recover from.

use anyhow::{bail, Context};
use serde_json::json;
use tandem_automation::{
    GoalRunLink, LongRunningGoal, LongRunningGoalStatus, OrchestrationNodeKind, OrchestrationStatus,
};
use tandem_types::{PrincipalRef, TenantContext};

/// Operator-supplied goal metadata is bounded so it can never smuggle bulk
/// content (or secrets disguised as context) into prompts and graph records.
pub(crate) const MAX_GOAL_METADATA_BYTES: usize = 16 * 1024;

use super::AppState;
use crate::stateful_runtime::{
    automation_definition_snapshot_hash, stable_definition_snapshot_hash, GoalPauseOutcome,
    GoalResumeOutcome, OrchestrationStateStore, StartGoalOutcome,
};

#[derive(Debug, Clone)]
pub(crate) struct StartGoalRequest {
    pub orchestration_id: String,
    /// `None` starts from the latest published version.
    pub orchestration_version: Option<u64>,
    pub objective: String,
    pub idempotency_key: String,
    pub metadata: Option<serde_json::Value>,
    pub now_ms: u64,
}

impl StartGoalRequest {
    pub fn goal_id(&self, tenant: &TenantContext) -> String {
        stable_lifecycle_id(
            "goal",
            &format!(
                "{}:{}:{}:{}",
                tenant.org_id,
                tenant.workspace_id,
                tenant.deployment_id.as_deref().unwrap_or(""),
                self.orchestration_id
            ),
            &self.idempotency_key,
        )
    }

    pub fn root_run_id(&self, tenant: &TenantContext) -> String {
        stable_lifecycle_id(
            "automation-v2-run",
            &self.goal_id(tenant),
            &format!("root:{}", self.idempotency_key),
        )
    }
}

fn stable_lifecycle_id(prefix: &str, scope: &str, idempotency_key: &str) -> String {
    let hash = stable_definition_snapshot_hash(&json!({
        "scope": scope,
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

impl AppState {
    /// Start (or idempotently return) a long-running goal from a published
    /// orchestration version. The root run is created atomically with the
    /// goal and hop-0 lineage.
    pub(crate) async fn start_long_running_goal(
        &self,
        tenant: &TenantContext,
        request: &StartGoalRequest,
        actor: &PrincipalRef,
    ) -> anyhow::Result<StartGoalOutcome> {
        if request.idempotency_key.trim().is_empty() || request.idempotency_key.len() > 256 {
            bail!("goal start requires a bounded idempotency key");
        }
        if request.objective.trim().is_empty() {
            bail!("goal start requires an objective");
        }
        if let Some(metadata) = request.metadata.as_ref() {
            let bytes = serde_json::to_vec(metadata)?.len();
            if bytes > MAX_GOAL_METADATA_BYTES {
                bail!(
                    "goal metadata is {bytes} bytes; the admission bound is {MAX_GOAL_METADATA_BYTES} bytes"
                );
            }
        }
        let store =
            OrchestrationStateStore::from_automation_runs_path(&self.automation_v2_runs_path)?;
        let version = match request.orchestration_version {
            Some(version) => version,
            None => store
                .latest_published_orchestration_version(tenant, &request.orchestration_id)?
                .context("orchestration has no published version")?,
        };
        let orchestration = store
            .get_orchestration_for_tenant(tenant, &request.orchestration_id, version)?
            .context("published orchestration version not found")?;
        if orchestration.status != OrchestrationStatus::Published {
            bail!("goals can only start from a published orchestration version");
        }
        let same_scope = orchestration.tenant_context.org_id == tenant.org_id
            && orchestration.tenant_context.workspace_id == tenant.workspace_id
            && orchestration.tenant_context.deployment_id == tenant.deployment_id;
        if !same_scope {
            // Cross-tenant references fail closed with the same message as a
            // missing orchestration so nothing leaks across the boundary.
            bail!("published orchestration version not found");
        }
        let root_node = orchestration
            .nodes
            .iter()
            .find(|node| node.node_id == orchestration.root_node_id)
            .context("published orchestration is missing its root node")?;
        let OrchestrationNodeKind::Workflow {
            automation_id,
            pinned_definition_hash,
            ..
        } = &root_node.node
        else {
            bail!("orchestration root node must reference a workflow");
        };
        let automation = self
            .get_automation_v2(automation_id)
            .await
            .context("root workflow definition not found")?;
        let pinned_hash = pinned_definition_hash
            .as_deref()
            .context("published root workflow is missing its pinned definition hash")?;
        if automation_definition_snapshot_hash(&automation) != pinned_hash {
            bail!("root workflow definition changed after orchestration publication");
        }

        let goal_id = request.goal_id(tenant);
        let root_run_id = request.root_run_id(tenant);
        let goal = LongRunningGoal {
            schema_version: 1,
            goal_id: goal_id.clone(),
            orchestration_id: orchestration.orchestration_id.clone(),
            orchestration_version: orchestration.version,
            objective: request.objective.clone(),
            status: LongRunningGoalStatus::Active,
            // The goal inherits the orchestration's stored tenant so later
            // strict comparisons in the transition kernel stay consistent
            // regardless of which actor's request created it.
            tenant_context: orchestration.tenant_context.clone(),
            policy: orchestration.goal_policy.clone(),
            active_run_id: Some(root_run_id.clone()),
            current_node_id: Some(orchestration.root_node_id.clone()),
            hop_count: 0,
            total_tokens: 0,
            total_cost_usd: 0.0,
            created_at_ms: request.now_ms,
            updated_at_ms: request.now_ms,
            finished_at_ms: None,
            final_artifact: None,
            metadata: request.metadata.clone(),
        };
        let root_run = self
            .prepare_orchestration_node_run(
                &goal,
                &automation,
                root_run_id.clone(),
                request.now_ms,
                "orchestration_goal_start",
                format!("goal {goal_id} started"),
                None,
            )
            .await?;
        let link = GoalRunLink {
            goal_id: goal_id.clone(),
            run_id: root_run_id.clone(),
            orchestration_node_id: orchestration.root_node_id.clone(),
            orchestration_version: orchestration.version,
            hop_index: 0,
            parent_run_id: None,
            triggering_handoff_id: None,
            created_at_ms: request.now_ms,
        };
        let outcome = store.start_goal(&goal, &root_run, &link, actor)?;
        if let StartGoalOutcome::Created { root_run, goal } = &outcome {
            self.automation_v2_runs
                .write()
                .await
                .insert(root_run.run_id.clone(), root_run.clone());
            if let Err(error) = self.persist_automation_v2_runs().await {
                tracing::warn!(
                    goal_id = %goal.goal_id,
                    run_id = %root_run.run_id,
                    %error,
                    "durable goal start committed; compatibility run persistence will recover from SQLite"
                );
            }
            if let Err(status) = crate::http::context_runs::sync_automation_v2_run_blackboard(
                self,
                &automation,
                root_run,
            )
            .await
            {
                tracing::warn!(
                    goal_id = %goal.goal_id,
                    run_id = %root_run.run_id,
                    ?status,
                    "durable goal start committed; context blackboard synchronization will be retried"
                );
            }
            self.event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "orchestration.goal.started",
                    tenant,
                    json!({
                        "goalID": goal.goal_id,
                        "orchestrationID": goal.orchestration_id,
                        "orchestrationVersion": goal.orchestration_version,
                        "rootRunID": root_run.run_id,
                    }),
                ));
        }
        Ok(outcome)
    }

    pub(crate) async fn pause_long_running_goal(
        &self,
        goal_id: &str,
        tenant: &TenantContext,
        reason: &str,
        actor: &PrincipalRef,
    ) -> anyhow::Result<(GoalPauseOutcome, LongRunningGoal)> {
        let store =
            OrchestrationStateStore::from_automation_runs_path(&self.automation_v2_runs_path)?;
        let (outcome, goal) =
            store.pause_goal(goal_id, tenant, reason, actor, crate::util::time::now_ms())?;
        if outcome == GoalPauseOutcome::Applied {
            self.event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "orchestration.goal.paused",
                    tenant,
                    json!({"goalID": goal_id, "reason": reason}),
                ));
        }
        Ok((outcome, goal))
    }

    pub(crate) async fn resume_long_running_goal(
        &self,
        goal_id: &str,
        tenant: &TenantContext,
        reason: &str,
        actor: &PrincipalRef,
    ) -> anyhow::Result<(GoalResumeOutcome, LongRunningGoal)> {
        let store =
            OrchestrationStateStore::from_automation_runs_path(&self.automation_v2_runs_path)?;
        let (outcome, goal) =
            store.resume_goal(goal_id, tenant, reason, actor, crate::util::time::now_ms())?;
        if outcome == GoalResumeOutcome::Applied {
            self.event_bus
                .publish(crate::routines::types::tenant_scoped_engine_event(
                    "orchestration.goal.resumed",
                    tenant,
                    json!({"goalID": goal_id, "reason": reason}),
                ));
        }
        Ok((outcome, goal))
    }
}
