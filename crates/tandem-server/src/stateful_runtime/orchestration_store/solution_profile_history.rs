//! Durable per-logical-operation route history. A selection is an observation,
//! never a provider send permit or a substitute for current account authority.

use std::collections::BTreeMap;

use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use tandem_solutions::{
    canonical_json, resolve_model_profile, sha256, ModelProfileDecision, ModelProfileLimits,
    ModelProfileResolutionInput,
};

use super::{
    solution_budget_records as records, solution_execution, solution_goals, solution_installations,
    CustomerConfigVersion, OrchestrationStateStore, SolutionGoalStart, SolutionRunExecution,
};
use crate::stateful_runtime::backend::TransactionBehavior;

/// The host supplies current route, capability and data facts. The store
/// replaces caller-supplied history, time, bindings and global constraints with
/// protected current state before resolving. No HTTP route accepts this type.
pub struct RuntimeProfileRequest<'a> {
    pub goal: SolutionGoalStart<'a>,
    pub execution: &'a SolutionRunExecution,
    pub root_run_id: &'a str,
    /// Stable logical work unit, distinct for concurrent workers under a root.
    pub operation_id: &'a str,
    /// Stable selection attempt ID. A replay never returns a sendable decision.
    pub selection_id: &'a str,
    pub profile: ModelProfileResolutionInput<'a>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RuntimeProfileSelection {
    Selected(ModelProfileDecision),
    Replayed,
    Blocked(String),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    schema_version: u32,
    operation_id: String,
    root_run_id: String,
    actor_id: String,
    configuration: CustomerConfigVersion,
    installation_generation: u64,
    composition_sha256: String,
    started_at_ms: u64,
    catalog_sha256: Option<String>,
    selected_path: Vec<String>,
    route_evaluations: u32,
    effective_limits: ModelProfileLimits,
    selection_ids: Vec<String>,
    last_decision: Option<ModelProfileDecision>,
    terminal_code: Option<String>,
}

fn reference(value: &str) -> anyhow::Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 256
            && value.trim() == value
            && !value.chars().any(char::is_control),
        "invalid model profile operation identity"
    );
    Ok(())
}

impl OrchestrationStateStore {
    /// Resolve and persist one selection under the same writer lock as the
    /// protected goal and installation checks. A failed resolution terminates
    /// this logical operation, so retrying it cannot reset a finite route cap.
    pub fn select_runtime_model_profile(
        &self,
        request: RuntimeProfileRequest<'_>,
        clock: impl Fn() -> u64,
    ) -> anyhow::Result<RuntimeProfileSelection> {
        reference(request.root_run_id)?;
        reference(request.operation_id)?;
        reference(request.selection_id)?;
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let now_ms = clock();
            let tenant = &request.goal.verified.tenant_context;
            let goal = solution_execution::validate(
                &transaction,
                tenant,
                request.execution,
                request.root_run_id,
                now_ms,
            )?;
            let current_goal = SolutionGoalStart {
                now_ms,
                ..request.goal
            };
            solution_goals::validate(&transaction, &goal, &current_goal, request.root_run_id)?;
            let installation = solution_installations::load(
                &transaction,
                request.goal.verified,
                request.goal.scope,
            )?
            .context("solution installation missing")?;
            let customer_bindings: BTreeMap<String, String> = installation
                .plan
                .models
                .iter()
                .map(|(class, binding)| (class.clone(), binding.binding_id.clone()))
                .collect();
            let key = format!(
                "profile:{}",
                sha256(&canonical_json(&(
                    request.root_run_id,
                    request.operation_id
                ))?)
            );
            let loaded: Option<(u64, History)> =
                records::load(&transaction, tenant, request.goal.scope, &key)?;
            let generation = loaded.as_ref().map_or(0, |(generation, _)| *generation);
            let mut history = loaded.map_or_else(
                || History {
                    schema_version: 1,
                    operation_id: request.operation_id.into(),
                    root_run_id: request.root_run_id.into(),
                    actor_id: request.goal.verified.human_actor.actor_id.clone(),
                    configuration: request.goal.configuration.clone(),
                    installation_generation: request.goal.installation_generation,
                    composition_sha256: request.goal.composition_sha256.into(),
                    started_at_ms: now_ms,
                    catalog_sha256: None,
                    selected_path: Vec::new(),
                    route_evaluations: 0,
                    effective_limits: request.profile.inherited_limits.clone(),
                    selection_ids: Vec::new(),
                    last_decision: None,
                    terminal_code: None,
                },
                |(_, value)| value,
            );
            ensure!(
                history.schema_version == 1
                    && history.operation_id == request.operation_id
                    && history.root_run_id == request.root_run_id
                    && history.actor_id == request.goal.verified.human_actor.actor_id
                    && history.configuration == *request.goal.configuration
                    && history.installation_generation == request.goal.installation_generation
                    && history.composition_sha256 == request.goal.composition_sha256
                    && history.selection_ids.len() <= 16
                    && history.route_evaluations <= 16,
                "model profile history belongs to another authority or is invalid"
            );
            if history
                .selection_ids
                .iter()
                .any(|id| id == request.selection_id)
            {
                return Ok(RuntimeProfileSelection::Replayed);
            }
            if let Some(code) = history.terminal_code {
                return Ok(RuntimeProfileSelection::Blocked(code));
            }
            ensure!(
                history.selection_ids.len() < 16,
                "model profile selection limit reached"
            );
            // The caller's previous path, evaluation count, start time, digest,
            // customer bindings and global policy never establish authority.
            let profile = ModelProfileResolutionInput {
                catalog: request.profile.catalog,
                requested_class: request.profile.requested_class,
                escalation_from: request.profile.escalation_from,
                escalation_reason: request.profile.escalation_reason,
                customer_bindings: &customer_bindings,
                current_routes: request.profile.current_routes,
                constraints: &installation.plan.constraints,
                data_policies: request.profile.data_policies,
                data_classes: request.profile.data_classes,
                modalities: request.profile.modalities,
                uses_tools: request.profile.uses_tools,
                maximum_input_tokens: request.profile.maximum_input_tokens,
                maximum_output_tokens: request.profile.maximum_output_tokens,
                inherited_limits: &history.effective_limits,
                previously_selected: &history.selected_path,
                previous_evaluations: history.route_evaluations,
                expected_catalog_sha256: history.catalog_sha256.as_deref(),
                started_at_ms: history.started_at_ms,
                now_ms,
            };
            let resolution = resolve_model_profile(profile).and_then(|decision| {
                if installation.plan.models.get(&decision.selected_class) != Some(&decision.binding)
                {
                    return Err(tandem_solutions::SolutionError {
                        code: "model_installation_binding_changed".into(),
                        path: "model.binding".into(),
                        message: "Selected route differs from the current installation".into(),
                    });
                }
                Ok(decision)
            });
            history.selection_ids.push(request.selection_id.into());
            let outcome = match resolution {
                Ok(decision) => {
                    history.catalog_sha256 = Some(decision.catalog_sha256.clone());
                    history.selected_path = decision.selected_path.clone();
                    history.route_evaluations = decision.route_evaluations;
                    history.effective_limits = decision.effective_limits.clone();
                    history.last_decision = Some(decision.clone());
                    RuntimeProfileSelection::Selected(decision)
                }
                Err(error) => {
                    // The pure resolver does not expose evaluations on failure.
                    // Terminate instead of guessing a refundable count.
                    history.terminal_code = Some(error.code.clone());
                    RuntimeProfileSelection::Blocked(error.code)
                }
            };
            records::save(
                &transaction,
                tenant,
                request.goal.scope,
                &key,
                generation,
                history,
            )?;
            transaction.commit()?;
            Ok(outcome)
        })
    }
}
