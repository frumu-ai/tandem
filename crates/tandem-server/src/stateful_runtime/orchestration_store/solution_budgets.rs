//! Atomic accounting, not dispatch authority. The existing authorized runtime
//! must resolve current model/price limits and an authoritative root run before
//! calling this store. A duplicate reservation never authorizes a second send.

use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use tandem_solutions::{sha256, validate_customer_config_scope, CustomerScope};
use tandem_types::VerifiedTenantContext;

use super::{
    customer_configs, solution_budget_records as records, solution_installations,
    OrchestrationStateStore,
};
use crate::stateful_runtime::backend::TransactionBehavior;

const DAY_MS: u64 = 86_400_000;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolutionChargeKind {
    Model,
    Retry,
    Escalation,
    Embedding,
    Transcription,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionRunBudget {
    pub max_tokens: u64,
    pub max_cost_microusd: u64,
    pub max_requests: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionChargeIntent {
    /// One immutable physical attempt ID. Every retry uses another ID.
    pub reservation_id: String,
    /// Children inherit their runtime-owned root, never choose a fresh budget.
    pub root_run_id: String,
    pub kind: SolutionChargeKind,
    pub route_revision: String,
    pub maximum_tokens: u64,
    /// None means unknown pricing; it is never interpreted as free.
    pub maximum_cost_microusd: Option<u64>,
    pub run_budget: SolutionRunBudget,
}

pub struct SolutionBudgetInput<'a> {
    pub verified: &'a VerifiedTenantContext,
    pub scope: &'a CustomerScope,
    pub composition_sha256: &'a str,
    pub now_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum SolutionChargeStatus {
    Reserved,
    Settled {
        tokens: u64,
        cost_microusd: u64,
        overrun: bool,
        #[serde(default)]
        cost_basis: SolutionChargeCostBasis,
    },
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolutionChargeCostBasis {
    #[default]
    Confirmed,
    /// Confirmed token usage valued at the approved conservative input/output
    /// rates. This is not a provider invoice or a hard billing guarantee.
    ApprovedUpperBound,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolutionChargeReservation {
    pub intent: SolutionChargeIntent,
    pub composition_sha256: String,
    pub period_start_ms: u64,
    pub created_at_ms: u64,
    pub status: SolutionChargeStatus,
}

#[derive(Clone, Debug, Serialize)]
pub struct SolutionBudgetReservationResult {
    pub reservation: SolutionChargeReservation,
    /// False means observe/reconcile the existing attempt; do not dispatch.
    pub newly_reserved: bool,
}

/// A nonserializable settlement capability minted only after a current, scoped
/// reservation succeeds. It authorizes accounting for that attempt, never a new
/// dispatch, and remains usable when the initiating user assertion expires.
pub(crate) struct RuntimeSolutionCharge {
    verified: VerifiedTenantContext,
    scope: CustomerScope,
    composition_sha256: String,
    intent: SolutionChargeIntent,
}

pub(crate) struct RuntimeSolutionModel {
    pub execution: super::SolutionRunExecution,
    pub configuration: super::CustomerConfigVersion,
    pub installation_generation: u64,
    pub model_class: String,
    pub binding: tandem_solutions::LockedModelBinding,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Account {
    committed_tokens: u64,
    reserved_tokens: u64,
    committed_cost: u64,
    reserved_cost: u64,
    outstanding: u64,
    requests: u64,
    last_observed_ms: u64,
    overrun: bool,
    root_limits: Option<SolutionRunBudget>,
}

fn sum(left: u64, right: u64) -> anyhow::Result<u64> {
    left.checked_add(right)
        .context("solution budget amount overflow")
}

fn reference(value: &str) -> anyhow::Result<()> {
    ensure!(
        !value.is_empty()
            && value.len() <= 256
            && value.trim() == value
            && !value.chars().any(char::is_control),
        "invalid solution charge identity"
    );
    Ok(())
}

fn key(prefix: &str, value: &str) -> String {
    format!("{prefix}:{}", sha256(value.as_bytes()))
}

impl OrchestrationStateStore {
    pub(crate) fn reserve_runtime_solution_charge(
        &self,
        input: SolutionBudgetInput<'_>,
        intent: SolutionChargeIntent,
        model: RuntimeSolutionModel,
    ) -> anyhow::Result<(SolutionBudgetReservationResult, RuntimeSolutionCharge)> {
        let ticket = RuntimeSolutionCharge {
            verified: input.verified.clone(),
            scope: input.scope.clone(),
            composition_sha256: input.composition_sha256.into(),
            intent: intent.clone(),
        };
        let reservation = self.reserve_solution_charge_inner(input, intent, Some(&model))?;
        Ok((reservation, ticket))
    }

    pub(crate) fn settle_runtime_solution_charge(
        &self,
        ticket: &RuntimeSolutionCharge,
        now_ms: u64,
        actual_tokens: u64,
        actual_cost_microusd: u64,
        cost_basis: SolutionChargeCostBasis,
    ) -> anyhow::Result<SolutionChargeReservation> {
        self.settle_admitted_solution_charge(
            SolutionBudgetInput {
                verified: &ticket.verified,
                scope: &ticket.scope,
                composition_sha256: &ticket.composition_sha256,
                now_ms,
            },
            &ticket.intent,
            actual_tokens,
            actual_cost_microusd,
            cost_basis,
        )
    }

    pub fn reserve_solution_charge(
        &self,
        input: SolutionBudgetInput<'_>,
        intent: SolutionChargeIntent,
    ) -> anyhow::Result<SolutionBudgetReservationResult> {
        self.reserve_solution_charge_inner(input, intent, None)
    }

    fn reserve_solution_charge_inner(
        &self,
        input: SolutionBudgetInput<'_>,
        intent: SolutionChargeIntent,
        model: Option<&RuntimeSolutionModel>,
    ) -> anyhow::Result<SolutionBudgetReservationResult> {
        validate_customer_config_scope(input.verified, input.scope, input.now_ms)?;
        reference(&intent.reservation_id)?;
        reference(&intent.root_run_id)?;
        ensure!(
            intent.route_revision.len() == 64
                && intent
                    .route_revision
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
            "invalid reviewed charge route"
        );
        let cost = intent
            .maximum_cost_microusd
            .context("solution budget price unknown; dispatch blocked")?;
        ensure!(
            intent.run_budget.max_requests > 0,
            "solution run request budget exhausted"
        );
        let tenant = &input.verified.tenant_context;
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let installation =
                solution_installations::load(&transaction, input.verified, input.scope)?
                    .context("solution installation missing")?;
            if let Some(model) = model {
                super::solution_execution::validate(
                    &transaction,
                    tenant,
                    &model.execution,
                    &intent.root_run_id,
                    input.now_ms,
                )?;
                ensure!(
                    installation.generation == model.installation_generation
                        && installation.config_version == model.configuration
                        && installation.plan.models.get(&model.model_class) == Some(&model.binding),
                    "reviewed runtime model or installation generation changed"
                );
            }
            ensure!(
                installation.composition_sha256 == input.composition_sha256,
                "solution composition changed before budget reservation"
            );
            let configuration = customer_configs::load(&transaction, tenant, input.scope)?
                .context("solution customer configuration missing")?;
            ensure!(
                configuration.version == installation.config_version
                    && configuration.blueprint_sha256 == installation.plan.blueprint_sha256,
                "solution configuration changed before budget reservation"
            );
            let policy = &installation.plan.constraints;
            let reservation_key = key("attempt", &intent.reservation_id);
            let existing = records::load::<SolutionChargeReservation>(
                &transaction,
                tenant,
                input.scope,
                &reservation_key,
            )?;
            let (global_generation, mut global) =
                records::load::<Account>(&transaction, tenant, input.scope, "global")?
                    .unwrap_or_default();
            ensure!(
                !global.overrun,
                "solution budget overrun requires reconciliation"
            );
            ensure!(
                input.now_ms >= global.last_observed_ms,
                "solution budget clock moved backwards"
            );
            if let Some((_, reservation)) = existing {
                ensure!(
                    reservation.intent == intent
                        && reservation.composition_sha256 == input.composition_sha256,
                    "solution charge ID already belongs to another intent"
                );
                transaction.commit()?;
                return Ok(SolutionBudgetReservationResult {
                    reservation,
                    newly_reserved: false,
                });
            }
            let day = input.now_ms / DAY_MS * DAY_MS;
            let day_key = format!("day:{day}");
            let root_key = key("root", &intent.root_run_id);
            let (day_generation, mut daily) =
                records::load::<Account>(&transaction, tenant, input.scope, &day_key)?
                    .unwrap_or_default();
            let (root_generation, mut root) =
                records::load::<Account>(&transaction, tenant, input.scope, &root_key)?
                    .unwrap_or_default();
            let mut limits = intent.run_budget.clone();
            limits.max_tokens = limits.max_tokens.min(policy.max_tokens_per_run);
            limits.max_cost_microusd = limits.max_cost_microusd.min(policy.max_daily_cost_microusd);
            if let Some(previous) = &root.root_limits {
                limits.max_tokens = limits.max_tokens.min(previous.max_tokens);
                limits.max_cost_microusd = limits.max_cost_microusd.min(previous.max_cost_microusd);
                limits.max_requests = limits.max_requests.min(previous.max_requests);
            }
            ensure!(
                global.outstanding < u64::from(policy.max_concurrent_runs),
                "solution concurrent request budget exhausted"
            );
            ensure!(
                sum(sum(daily.committed_cost, daily.reserved_cost)?, cost)?
                    <= policy.max_daily_cost_microusd,
                "solution daily cost budget exhausted"
            );
            ensure!(
                sum(sum(root.committed_cost, root.reserved_cost)?, cost)?
                    <= limits.max_cost_microusd,
                "solution root run cost budget exhausted"
            );
            ensure!(
                sum(
                    sum(root.committed_tokens, root.reserved_tokens)?,
                    intent.maximum_tokens
                )? <= limits.max_tokens,
                "solution root run token budget exhausted"
            );
            ensure!(
                root.requests < u64::from(limits.max_requests),
                "solution root run request budget exhausted"
            );
            root.root_limits = Some(limits);
            for account in [&mut daily, &mut root] {
                account.reserved_tokens = sum(account.reserved_tokens, intent.maximum_tokens)?;
                account.reserved_cost = sum(account.reserved_cost, cost)?;
                account.outstanding = sum(account.outstanding, 1)?;
                account.requests = sum(account.requests, 1)?;
            }
            global.outstanding = sum(global.outstanding, 1)?;
            global.last_observed_ms = input.now_ms;
            let reservation = SolutionChargeReservation {
                intent: intent.clone(),
                composition_sha256: input.composition_sha256.into(),
                period_start_ms: day,
                created_at_ms: input.now_ms,
                status: SolutionChargeStatus::Reserved,
            };
            records::save(
                &transaction,
                tenant,
                input.scope,
                "global",
                global_generation,
                global,
            )?;
            records::save(
                &transaction,
                tenant,
                input.scope,
                &day_key,
                day_generation,
                daily,
            )?;
            records::save(
                &transaction,
                tenant,
                input.scope,
                &root_key,
                root_generation,
                root,
            )?;
            records::save(
                &transaction,
                tenant,
                input.scope,
                &reservation_key,
                0,
                &reservation,
            )?;
            transaction.commit()?;
            Ok(SolutionBudgetReservationResult {
                reservation,
                newly_reserved: true,
            })
        })
    }

    /// Only a trusted adapter with a confirmed result may settle an attempt.
    /// An unknown outcome remains reserved, including across midnight/restart.
    /// Reconciliation can record an actual overrun; it must not erase it to
    /// pretend that an estimated price was a provider-side billing guarantee.
    pub fn settle_solution_charge(
        &self,
        input: SolutionBudgetInput<'_>,
        intent: &SolutionChargeIntent,
        actual_tokens: u64,
        actual_cost_microusd: u64,
    ) -> anyhow::Result<SolutionChargeReservation> {
        validate_customer_config_scope(input.verified, input.scope, input.now_ms)?;
        self.settle_admitted_solution_charge(
            input,
            intent,
            actual_tokens,
            actual_cost_microusd,
            SolutionChargeCostBasis::Confirmed,
        )
    }

    fn settle_admitted_solution_charge(
        &self,
        input: SolutionBudgetInput<'_>,
        intent: &SolutionChargeIntent,
        actual_tokens: u64,
        actual_cost_microusd: u64,
        cost_basis: SolutionChargeCostBasis,
    ) -> anyhow::Result<SolutionChargeReservation> {
        let tenant = &input.verified.tenant_context;
        self.with_connection(|connection| {
            let transaction =
                connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let reservation_key = key("attempt", &intent.reservation_id);
            let (generation, mut reservation) = records::load::<SolutionChargeReservation>(
                &transaction,
                tenant,
                input.scope,
                &reservation_key,
            )?
            .context("solution charge reservation missing")?;
            ensure!(
                reservation.intent == *intent
                    && reservation.composition_sha256 == input.composition_sha256,
                "solution charge settlement intent mismatch"
            );
            let maximum_cost = intent
                .maximum_cost_microusd
                .context("reservation maximum cost missing")?;
            let overrun =
                actual_tokens > intent.maximum_tokens || actual_cost_microusd > maximum_cost;
            let settled = SolutionChargeStatus::Settled {
                tokens: actual_tokens,
                cost_microusd: actual_cost_microusd,
                overrun,
                cost_basis,
            };
            if reservation.status != SolutionChargeStatus::Reserved {
                ensure!(
                    reservation.status == settled,
                    "solution charge already settled differently"
                );
                transaction.commit()?;
                return Ok(reservation);
            }
            let (global_generation, mut global) =
                records::load::<Account>(&transaction, tenant, input.scope, "global")?
                    .context("solution global budget missing")?;
            global.outstanding = global
                .outstanding
                .checked_sub(1)
                .context("solution budget outstanding underflow")?;
            global.overrun |= overrun;
            global.last_observed_ms = global.last_observed_ms.max(input.now_ms);
            for account_key in [
                format!("day:{}", reservation.period_start_ms),
                key("root", &intent.root_run_id),
            ] {
                let (account_generation, mut account) =
                    records::load::<Account>(&transaction, tenant, input.scope, &account_key)?
                        .context("solution charge account missing")?;
                account.reserved_tokens = account
                    .reserved_tokens
                    .checked_sub(intent.maximum_tokens)
                    .context("solution token reservation underflow")?;
                account.reserved_cost = account
                    .reserved_cost
                    .checked_sub(maximum_cost)
                    .context("solution cost reservation underflow")?;
                account.outstanding = account
                    .outstanding
                    .checked_sub(1)
                    .context("solution outstanding reservation underflow")?;
                account.committed_tokens = sum(account.committed_tokens, actual_tokens)?;
                account.committed_cost = sum(account.committed_cost, actual_cost_microusd)?;
                account.overrun |= overrun;
                records::save(
                    &transaction,
                    tenant,
                    input.scope,
                    &account_key,
                    account_generation,
                    account,
                )?;
            }
            reservation.status = settled;
            records::save(
                &transaction,
                tenant,
                input.scope,
                "global",
                global_generation,
                global,
            )?;
            records::save(
                &transaction,
                tenant,
                input.scope,
                &reservation_key,
                generation,
                &reservation,
            )?;
            transaction.commit()?;
            Ok(reservation)
        })
    }
}
