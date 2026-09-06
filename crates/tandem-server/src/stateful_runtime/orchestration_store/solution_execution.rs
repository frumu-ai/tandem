//! Resolve provider accounting roots from the existing protected goal/run
//! records. This validates execution lineage, not installation or user grants.

use anyhow::{ensure, Context};
use tandem_automation::{
    AutomationRunStatus, AutomationV2RunRecord, GoalRunLink, LongRunningGoal, LongRunningGoalStatus,
};
use tandem_types::TenantContext;

use super::{protected_records, OrchestrationStateStore};
use crate::stateful_runtime::backend::{params, Executor, TransactionBehavior};

/// Current executor identity supplied by the trusted runtime. This is not an
/// HTTP-deserializable permission or a caller-selected accounting root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SolutionRunExecution {
    pub run_id: String,
    pub claim_id: String,
    pub claimant_id: String,
    pub lease_epoch: u64,
}

fn same_tenant(left: &TenantContext, right: &TenantContext) -> bool {
    left.org_id == right.org_id
        && left.workspace_id == right.workspace_id
        && left.deployment_id == right.deployment_id
}

fn run(
    executor: &impl Executor,
    tenant: &TenantContext,
    run_id: &str,
) -> anyhow::Result<AutomationV2RunRecord> {
    let raw: String = executor.query_row(
        "SELECT run_json FROM automation_runs WHERE run_id=?1",
        [run_id],
        |row| row.get(0),
    )?;
    let value: AutomationV2RunRecord = protected_records::decode(tenant, "run", run_id, &raw)?;
    ensure!(
        value.run_id == run_id && same_tenant(&value.tenant_context, tenant),
        "solution execution run scope mismatch"
    );
    Ok(value)
}

fn link(
    executor: &impl Executor,
    tenant: &TenantContext,
    run_id: &str,
) -> anyhow::Result<GoalRunLink> {
    let count: u64 = executor.query_row(
        "SELECT COUNT(*) FROM goal_run_links WHERE run_id=?1",
        [run_id],
        |row| row.get(0),
    )?;
    ensure!(
        count == 1,
        "solution execution requires one durable goal lineage"
    );
    let (goal_id, hop, parent, raw): (String, u32, Option<String>, String) = executor.query_row(
        "SELECT goal_id,hop_index,parent_run_id,link_json FROM goal_run_links WHERE run_id=?1",
        [run_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    )?;
    let value: GoalRunLink = protected_records::decode(tenant, "link", run_id, &raw)?;
    ensure!(
        value.run_id == run_id
            && value.goal_id == goal_id
            && value.hop_index == hop
            && value.parent_run_id == parent,
        "solution execution lineage binding mismatch"
    );
    Ok(value)
}

pub(super) fn validate(
    executor: &impl Executor,
    tenant: &TenantContext,
    execution: &SolutionRunExecution,
    expected_root: &str,
    now_ms: u64,
) -> anyhow::Result<()> {
    ensure!(
        !tenant.is_local_implicit(),
        "solution execution requires explicit tenant scope"
    );
    let active = run(executor, tenant, &execution.run_id)?;
    ensure!(
        active.status == AutomationRunStatus::Running && active.finished_at_ms.is_none(),
        "solution execution run is not running"
    );
    let claim = active
        .execution_claim
        .as_ref()
        .context("solution execution claim missing")?;
    ensure!(
        !execution.claim_id.is_empty()
            && !execution.claimant_id.is_empty()
            && execution.lease_epoch > 0
            && claim.claim_id == execution.claim_id
            && claim.claimant_id == execution.claimant_id
            && claim.lease_epoch == execution.lease_epoch
            && active.execution_claim_epoch == execution.lease_epoch
            && claim.claimed_at_ms <= now_ms
            && !claim.is_expired(now_ms),
        "solution execution claim expired or changed"
    );
    let mut current = link(executor, tenant, &execution.run_id)?;
    let raw: String = executor.query_row(
        "SELECT goal_json FROM long_running_goals WHERE goal_id=?1",
        [&current.goal_id],
        |row| row.get(0),
    )?;
    let goal: LongRunningGoal = protected_records::decode(tenant, "goal", &current.goal_id, &raw)?;
    ensure!(
        goal.goal_id == current.goal_id
            && same_tenant(&goal.tenant_context, tenant)
            && goal.status == LongRunningGoalStatus::Active
            && goal.finished_at_ms.is_none()
            && goal.active_run_id.as_deref() == Some(execution.run_id.as_str())
            && goal.current_node_id.as_deref() == Some(current.orchestration_node_id.as_str())
            && goal.hop_count == current.hop_index
            && goal.hop_count <= goal.policy.max_hops
            && goal.created_at_ms <= now_ms
            && goal
                .policy
                .deadline_at_ms
                .is_none_or(|deadline| now_ms < deadline),
        "solution execution goal is not current and active"
    );
    // Bound traversal even when an operator configured an unreasonable hop cap.
    ensure!(
        current.hop_index <= 4096,
        "solution execution lineage exceeds traversal limit"
    );
    loop {
        ensure!(
            current.goal_id == goal.goal_id
                && current.orchestration_version == goal.orchestration_version,
            "solution execution crosses goal lineage"
        );
        run(executor, tenant, &current.run_id)?;
        if current.hop_index == 0 {
            ensure!(
                current.parent_run_id.is_none() && current.run_id == expected_root,
                "solution accounting root differs from durable lineage"
            );
            let roots: u64 = executor.query_row(
                "SELECT COUNT(*) FROM goal_run_links WHERE goal_id=?1 AND hop_index=0",
                params![goal.goal_id],
                |row| row.get(0),
            )?;
            ensure!(roots == 1, "solution goal root is ambiguous");
            break;
        }
        let parent_id = current
            .parent_run_id
            .as_deref()
            .context("solution execution parent missing")?;
        let parent = link(executor, tenant, parent_id)?;
        ensure!(
            parent.hop_index.checked_add(1) == Some(current.hop_index),
            "solution execution parent hop mismatch"
        );
        current = parent;
    }
    Ok(())
}

impl OrchestrationStateStore {
    /// Recheck the same persisted run/claim/root after reservation and before
    /// dispatch. A later settlement remains permitted after a goal stops.
    pub(super) fn validate_solution_execution(
        &self,
        tenant: &TenantContext,
        execution: &SolutionRunExecution,
        expected_root: &str,
        clock: impl Fn() -> u64,
    ) -> anyhow::Result<()> {
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            validate(&transaction, tenant, execution, expected_root, clock())?;
            transaction.commit()?;
            Ok(())
        })
    }
}
