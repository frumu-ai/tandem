//! Authenticated customer configuration and installation orchestration over
//! existing PackManager, host registries and protected state. No new identity,
//! pack, credential or memory registry is introduced here.

mod host_facts;
mod staging;
pub use staging::SolutionStagingRequest;

use anyhow::ensure;
use serde::{Deserialize, Serialize};
use tandem_enterprise_contract::VerifiedTenantContext;
use tandem_solutions::{
    prepare_customer_config, resolve, CustomerConfig, ResolutionInput, ResolvedPlan,
};

use crate::stateful_runtime::orchestration_store::{CustomerConfigVersion, StoredCustomerConfig};
use crate::{AppState, OrchestrationStateStore};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SolutionConfigurationRequest {
    pub pack_selector: String,
    pub configuration: CustomerConfig,
}

#[derive(Serialize)]
pub struct SolutionConfigurationPreview {
    pub plan: ResolvedPlan,
    pub composition_sha256: String,
    pub activation_required: bool,
    pub solution_ready: bool,
}

impl AppState {
    /// Nonmutating draft preview: does not open/create the state database and
    /// cannot install resources. The caller cannot supply approved host facts.
    pub async fn preview_solution_configuration(
        &self,
        verified: &VerifiedTenantContext,
        request: &SolutionConfigurationRequest,
    ) -> anyhow::Result<SolutionConfigurationPreview> {
        ensure!(request.pack_selector.len() <= 512, "invalid pack selector");
        ensure!(
            serde_json::to_vec(&request.configuration)?.len()
                <= tandem_solutions::MAX_BLUEPRINT_BYTES,
            "customer configuration exceeds the document size limit"
        );
        let facts = self
            .solution_host_facts(verified, &request.configuration.scope)
            .await?;
        let pack = self
            .pack_manager
            .solution_artifacts(&request.pack_selector)
            .await?;
        let prepared = prepare_customer_config(
            &pack.blueprint,
            &request.configuration,
            facts.configuration(&request.configuration.scope),
        )?;
        let plan = resolve(
            &pack.blueprint,
            ResolutionInput {
                host_facts_sha256: Some(&facts.digest),
                request: &prepared.request,
                verified_context: &facts.context,
                now_ms: crate::now_ms(),
                engine_version: env!("CARGO_PKG_VERSION"),
                deployment_policy: &prepared.deployment_policy,
                available_deployment_requirements: &facts.readiness,
                approved_models: &facts.models,
                artifacts: &pack.artifacts,
            },
        )?;
        Ok(SolutionConfigurationPreview {
            composition_sha256: plan.composition_hash()?,
            plan,
            activation_required: true,
            solution_ready: false,
        })
    }

    /// Save in the existing encrypted customer store with generation+digest
    /// compare-and-swap. A saved document is not approval to install or run.
    pub async fn save_solution_configuration(
        &self,
        verified: &VerifiedTenantContext,
        request: SolutionConfigurationRequest,
        expected: Option<CustomerConfigVersion>,
    ) -> anyhow::Result<StoredCustomerConfig> {
        ensure!(request.pack_selector.len() <= 512, "invalid pack selector");
        let facts = self
            .solution_host_facts(verified, &request.configuration.scope)
            .await?;
        let pack = self
            .pack_manager
            .solution_artifacts(&request.pack_selector)
            .await?;
        let path = self.automation_v2_runs_path.clone();
        let state = self.clone();
        crate::encrypted_file_store::spawn_protected_blocking(move || {
            state
                .enterprise
                .hosted_policy
                .authorize_permission(
                    Some(&facts.context),
                    tandem_enterprise_contract::AccessPermission::HostedAdmin,
                )
                .map_err(anyhow::Error::msg)?;
            let store = OrchestrationStateStore::from_automation_runs_path(&path)?;
            let mut input = facts.configuration(&request.configuration.scope);
            input.expected_revision = expected.as_ref().map(|version| version.sha256.as_str());
            store.save_customer_configuration(
                &pack.blueprint,
                &request.configuration,
                input,
                expected.as_ref(),
            )
        })
        .await?
    }
}
