//! Connect actual adapter attempts to the existing protected budget store.
//! The runtime must supply current authorized binding/root facts; this module
//! never treats a customer document or a provider callback as that authority.

use std::{future::Future, sync::Arc};

use anyhow::{ensure, Context};
use tandem_providers::{
    ProviderAttempt, ProviderAttemptOutcome, ProviderAttemptPolicy, ProviderAttemptReceipt,
    ProviderProtocol,
};
use tandem_solutions::CustomerScope;
use tandem_types::VerifiedTenantContext;

use super::{
    OrchestrationStateStore, SolutionBudgetInput, SolutionChargeCostBasis, SolutionChargeIntent,
    SolutionChargeKind, SolutionRunBudget,
};

pub use tandem_solutions::ModelProfilePrice as ApprovedModelPrice;

/// Produced by a trusted current model/root authorization callback, never HTTP
/// deserialization. The caller must validate activation/current account binding,
/// source/data-class policy and real root lineage before returning this value.
#[derive(Clone, PartialEq, Eq)]
pub struct ApprovedSolutionProviderCharge {
    pub verified: VerifiedTenantContext,
    pub scope: CustomerScope,
    pub composition_sha256: String,
    pub configuration: super::CustomerConfigVersion,
    pub installation_generation: u64,
    pub model_class: String,
    pub binding: tandem_solutions::LockedModelBinding,
    pub root_run_id: String,
    pub kind: SolutionChargeKind,
    /// Reviewed model/account/price revision, not an arbitrary request ID.
    pub route_revision: String,
    pub provider_id: String,
    pub model_id: String,
    pub protocol: ProviderProtocol,
    pub endpoint_sha256: String,
    pub credential_sha256: String,
    /// Must be a supported host bound, never guessed from prompt byte length.
    pub maximum_input_tokens: u64,
    pub maximum_output_tokens: u32,
    pub run_budget: SolutionRunBudget,
    /// Missing prices block; zero must be an explicit approved value.
    pub price: Option<ApprovedModelPrice>,
}

impl OrchestrationStateStore {
    /// Build an optional scope around the existing provider registry dispatch.
    /// The callback runs again for each physical adapter attempt. It must verify
    /// current facts independently; the scope is not an installation capability.
    pub fn solution_provider_attempt_policy<F, Fut, Clock>(
        &self,
        maximum_output_tokens: u32,
        maximum_request_bytes: usize,
        authorize: F,
        clock: Clock,
    ) -> anyhow::Result<ProviderAttemptPolicy>
    where
        F: Fn(ProviderAttempt) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = anyhow::Result<ApprovedSolutionProviderCharge>> + Send + 'static,
        Clock: Fn() -> u64 + Send + Sync + 'static,
    {
        let store = self.clone();
        let authorize = Arc::new(authorize);
        let clock = Arc::new(clock);
        ProviderAttemptPolicy::new(
            maximum_output_tokens,
            maximum_request_bytes,
            move |attempt| {
                let store = store.clone();
                let authorize = authorize.clone();
                let clock = clock.clone();
                async move {
                    let approval = authorize(attempt.clone()).await?;
                    ensure!(
                        attempt.provider_id == approval.provider_id
                            && attempt.model_id == approval.model_id
                            && attempt.protocol == approval.protocol
                            && attempt.endpoint_sha256 == approval.endpoint_sha256
                            && attempt.credential_sha256 == approval.credential_sha256,
                        "provider attempt differs from current approved model/account route"
                    );
                    ensure!(
                        approval.binding.binding.provider == approval.provider_id
                            && approval.binding.binding.model == approval.model_id,
                        "provider route does not match the reviewed solution model binding"
                    );
                    ensure!(
                        attempt.maximum_output_tokens <= approval.maximum_output_tokens,
                        "provider attempt exceeds current output policy"
                    );
                    let now_ms = clock();
                    let price = approval
                        .price
                        .clone()
                        .context("model price unknown; dispatch blocked")?;
                    ensure!(
                        now_ms <= price.valid_until_ms,
                        "model price expired; review current binding"
                    );
                    let maximum_tokens = approval
                        .maximum_input_tokens
                        .checked_add(u64::from(attempt.maximum_output_tokens))
                        .context("model token bound overflow")?;
                    let maximum_cost = price.cost(
                        approval.maximum_input_tokens,
                        u64::from(attempt.maximum_output_tokens),
                    )?;
                    let intent = SolutionChargeIntent {
                        reservation_id: uuid::Uuid::new_v4().to_string(),
                        root_run_id: approval.root_run_id.clone(),
                        kind: approval.kind.clone(),
                        route_revision: approval.route_revision.clone(),
                        maximum_tokens,
                        maximum_cost_microusd: Some(maximum_cost),
                        run_budget: approval.run_budget.clone(),
                    };
                    let admission_store = store.clone();
                    let reviewed = approval.clone();
                    let ticket = crate::encrypted_file_store::spawn_protected_blocking(move || {
                        let (result, ticket) = admission_store.reserve_runtime_solution_charge(
                            SolutionBudgetInput {
                                verified: &approval.verified,
                                scope: &approval.scope,
                                composition_sha256: &approval.composition_sha256,
                                now_ms,
                            },
                            intent,
                            super::solution_budgets::RuntimeSolutionModel {
                                configuration: approval.configuration,
                                installation_generation: approval.installation_generation,
                                model_class: approval.model_class,
                                binding: approval.binding,
                            },
                        )?;
                        ensure!(
                            result.newly_reserved,
                            "provider attempt already exists; reconcile instead of sending"
                        );
                        Ok::<_, anyhow::Error>(ticket)
                    })
                    .await??;
                    let ticket = Arc::new(ticket);
                    // Reservation can wait for another database writer. Recheck
                    // current model/account/root authorization after it completes.
                    // No send has happened, so this denial may reconcile at zero.
                    let rechecked = async {
                        let current = authorize(attempt).await?;
                        ensure!(
                            current == reviewed,
                            "model or runtime authority changed during budget admission"
                        );
                        tandem_solutions::validate_customer_config_scope(
                            &current.verified,
                            &current.scope,
                            clock(),
                        )?;
                        ensure!(
                            clock() <= price.valid_until_ms,
                            "model price expired during budget admission"
                        );
                        Ok::<_, anyhow::Error>(())
                    }
                    .await;
                    if let Err(error) = rechecked {
                        let refund_store = store.clone();
                        let refund_ticket = ticket.clone();
                        let now_ms = clock();
                        crate::encrypted_file_store::spawn_protected_blocking(move || {
                            refund_store.settle_runtime_solution_charge(
                                &refund_ticket,
                                now_ms,
                                0,
                                0,
                                SolutionChargeCostBasis::Confirmed,
                            )
                        })
                        .await??;
                        return Err(error);
                    }
                    Ok(ProviderAttemptReceipt::new(move |outcome| {
                        let store = store.clone();
                        let ticket = ticket.clone();
                        let price = price.clone();
                        let now_ms = clock();
                        async move {
                            let (tokens, cost, basis) = match outcome {
                                ProviderAttemptOutcome::NotDispatched => {
                                    (0, 0, SolutionChargeCostBasis::Confirmed)
                                }
                                ProviderAttemptOutcome::Usage(usage) => (
                                    usage.total_tokens,
                                    price.cost(usage.input_tokens, usage.output_tokens)?,
                                    SolutionChargeCostBasis::ApprovedUpperBound,
                                ),
                            };
                            crate::encrypted_file_store::spawn_protected_blocking(move || {
                                store.settle_runtime_solution_charge(
                                    &ticket, now_ms, tokens, cost, basis,
                                )
                            })
                            .await??;
                            Ok(())
                        }
                    }))
                }
            },
        )
    }
}
