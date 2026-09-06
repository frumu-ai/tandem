//! Immutable installation association in the existing protected goal record.
//! This does not grant activation or replace current per-attempt user policy.

use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use tandem_automation::LongRunningGoal;
use tandem_solutions::{validate_customer_config_scope, CustomerScope};
use tandem_types::{PrincipalRef, VerifiedTenantContext};

use super::{customer_configs, protected_records, solution_installations, CustomerConfigVersion};
use crate::stateful_runtime::backend::{Executor, OptionalExtension};

const KEY: &str = "tandem_solution_binding";
const RECORD_KIND: &str = "solution-goal-binding";

/// Trusted host inputs, never an HTTP-deserializable start permission. The
/// calling service must authorize activation and the current execution actor.
#[derive(Clone, Copy)]
pub struct SolutionGoalStart<'a> {
    pub verified: &'a VerifiedTenantContext,
    pub scope: &'a CustomerScope,
    pub configuration: &'a CustomerConfigVersion,
    pub installation_generation: u64,
    pub composition_sha256: &'a str,
    pub now_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    schema_version: u32,
    scope: CustomerScope,
    configuration: CustomerConfigVersion,
    installation_generation: u64,
    composition_sha256: String,
    actor_id: String,
    root_run_id: String,
}

pub(super) fn value(goal: &LongRunningGoal) -> Option<&serde_json::Value> {
    goal.metadata
        .as_ref()
        .and_then(|metadata| metadata.get(KEY))
}

pub(super) fn reject_caller_binding(goal: &LongRunningGoal) -> anyhow::Result<()> {
    ensure!(
        value(goal).is_none(),
        "solution goal binding is reserved host authority"
    );
    Ok(())
}

fn current_binding(
    executor: &impl Executor,
    input: &SolutionGoalStart<'_>,
    root: &str,
) -> anyhow::Result<Binding> {
    validate_customer_config_scope(input.verified, input.scope, input.now_ms)?;
    ensure!(!root.is_empty(), "solution root run is required");
    let installation = solution_installations::load(executor, input.verified, input.scope)?
        .context("solution installation missing")?;
    ensure!(
        installation.all_components_staged()
            && installation.generation == input.installation_generation
            && &installation.config_version == input.configuration
            && installation.composition_sha256 == input.composition_sha256,
        "solution goal requires the current fully staged installation"
    );
    let configuration =
        customer_configs::load(executor, &input.verified.tenant_context, input.scope)?
            .context("solution customer configuration missing")?;
    ensure!(
        configuration.version == installation.config_version
            && configuration.blueprint_sha256 == installation.plan.blueprint_sha256,
        "solution goal configuration changed"
    );
    Ok(Binding {
        schema_version: 1,
        scope: input.scope.clone(),
        configuration: input.configuration.clone(),
        installation_generation: input.installation_generation,
        composition_sha256: input.composition_sha256.into(),
        actor_id: input.verified.human_actor.actor_id.clone(),
        root_run_id: root.into(),
    })
}

pub(super) fn bind(
    executor: &impl Executor,
    goal: &LongRunningGoal,
    input: &SolutionGoalStart<'_>,
    root: &str,
    actor: &PrincipalRef,
) -> anyhow::Result<LongRunningGoal> {
    reject_caller_binding(goal)?;
    let tenant = &input.verified.tenant_context;
    ensure!(
        goal.tenant_context.org_id == tenant.org_id
            && goal.tenant_context.workspace_id == tenant.workspace_id
            && goal.tenant_context.deployment_id == tenant.deployment_id
            && actor == &PrincipalRef::human_user(&input.verified.human_actor.actor_id),
        "solution goal actor or tenant differs from current authorization"
    );
    let binding = current_binding(executor, input, root)?;
    let mut bound = goal.clone();
    let mut metadata = match bound.metadata.take() {
        Some(serde_json::Value::Object(metadata)) => metadata,
        Some(other) => serde_json::Map::from_iter([("value".into(), other)]),
        None => serde_json::Map::new(),
    };
    // A separately authenticated record kind distinguishes host issuance from
    // arbitrary metadata persisted by older servers before this key was reserved.
    metadata.insert(
        KEY.into(),
        serde_json::Value::String(protected_records::encode(
            &goal.tenant_context,
            RECORD_KIND,
            &goal.goal_id,
            &binding,
        )?),
    );
    metadata.insert("started_by".into(), serde_json::to_value(actor)?);
    bound.metadata = Some(serde_json::Value::Object(metadata));
    read(&bound)?;
    Ok(bound)
}

pub(super) fn validate(
    executor: &impl Executor,
    goal: &LongRunningGoal,
    input: &SolutionGoalStart<'_>,
    root: &str,
) -> anyhow::Result<()> {
    let binding = read(goal)?.context("native goal has no approved solution association")?;
    ensure!(
        binding == current_binding(executor, input, root)?,
        "native goal belongs to another solution, configuration, actor or root"
    );
    Ok(())
}

fn read(goal: &LongRunningGoal) -> anyhow::Result<Option<Binding>> {
    value(goal)
        .map(|value| {
            let raw = value
                .as_str()
                .context("solution goal association lacks host authentication")?;
            ensure!(
                crate::encrypted_file_store::is_encrypted_payload(raw),
                "solution goal association requires an authenticated envelope"
            );
            protected_records::decode(&goal.tenant_context, RECORD_KIND, &goal.goal_id, raw)
        })
        .transpose()
}

pub(super) fn same(left: &LongRunningGoal, right: &LongRunningGoal) -> anyhow::Result<bool> {
    Ok(read(left)? == read(right)?)
}

/// All generic goal writes preserve the association, including its absence.
/// The writer transaction prevents a concurrent insert from turning an ordinary
/// upsert into an overwrite of a newly bound solution goal.
pub(super) fn preserve(
    executor: &impl Executor,
    goal: &LongRunningGoal,
    allow_initial: bool,
) -> anyhow::Result<()> {
    let raw: Option<String> = executor
        .query_row(
            "SELECT goal_json FROM long_running_goals WHERE goal_id=?1",
            [&goal.goal_id],
            |row| row.get(0),
        )
        .optional()?;
    if let Some(raw) = raw {
        let stored: LongRunningGoal =
            protected_records::decode(&goal.tenant_context, "goal", &goal.goal_id, &raw)?;
        ensure!(
            value(&stored) == value(goal),
            "solution goal association is immutable"
        );
    } else if !allow_initial {
        reject_caller_binding(goal)?;
    }
    Ok(())
}
