//! Read-only host loading of a signed catalog for one protected installation.
//! A returned catalog is data, never model availability or provider authority.

use anyhow::{ensure, Context};
use tandem_enterprise_contract::{AccessPermission, VerifiedTenantContext};
use tandem_solutions::{
    blueprint_hash, canonical_json, parse_model_profiles, sha256, validate_customer_config_scope,
    ComponentKind, CustomerScope, ModelProfileCatalog, SolutionIdentity,
};

use crate::pack_manager::SolutionPackArtifacts;
use crate::stateful_runtime::orchestration_store::{
    CustomerConfigVersion, OrchestrationStateStore, SolutionComponentProgress, SolutionInstallation,
};
use crate::AppState;

/// A verified catalog observation tied to a current staged generation. A
/// future runtime caller must recheck this binding with its goal and current
/// route/account facts before each selection or physical provider request.
#[derive(Debug)]
#[allow(dead_code)]
pub(crate) struct StagedModelProfileCatalog {
    pub catalog: ModelProfileCatalog,
    pub catalog_sha256: String,
    pub artifact_sha256: String,
    pub component_id: String,
    pub solution: SolutionIdentity,
    pub blueprint_sha256: String,
    pub config_version: CustomerConfigVersion,
    pub installation_generation: u64,
    pub composition_sha256: String,
}

pub(crate) fn catalog_from_installation(
    installation: &SolutionInstallation,
    pack: &SolutionPackArtifacts,
) -> anyhow::Result<StagedModelProfileCatalog> {
    ensure!(
        installation.all_components_staged()
            && installation.plan.composition_hash()? == installation.composition_sha256,
        "model-profile catalog requires a fully staged protected composition"
    );
    ensure!(
        pack.blueprint.solution == installation.plan.solution
            && blueprint_hash(&pack.blueprint)? == installation.plan.blueprint_sha256,
        "signed solution differs from the current staged plan"
    );
    let mut profiles = installation
        .plan
        .components
        .iter()
        .filter(|(_, component)| component.kind == ComponentKind::ModelProfile);
    let (component_id, locked) = profiles
        .next()
        .context("current installation has no model-profile component")?;
    ensure!(
        profiles.next().is_none(),
        "ambiguous staged model-profile component"
    );
    let selected = pack
        .blueprint
        .components
        .get(component_id)
        .context("signed model-profile component missing")?;
    ensure!(
        selected.kind == ComponentKind::ModelProfile
            && selected.artifact == locked.artifact
            && locked.artifact.pack_id == installation.plan.solution.id
            && locked.artifact.version == installation.plan.solution.version,
        "model-profile artifact differs from the current staged plan"
    );
    let artifact = pack
        .artifacts
        .get(component_id)
        .context("signed model-profile artifact missing")?;
    let artifact_sha256 = sha256(artifact);
    let receipt = installation
        .components
        .get(component_id)
        .context("model-profile staging receipt missing")?;
    ensure!(
        matches!(receipt, SolutionComponentProgress::Staged { resource_sha256, .. }
            if resource_sha256 == &artifact_sha256)
            && artifact_sha256 == locked.artifact.sha256,
        "model-profile staged receipt differs from signed artifact"
    );
    let catalog = parse_model_profiles(std::str::from_utf8(artifact)?)?;
    ensure!(
        selected.model_classes == catalog.profiles.keys().cloned().collect(),
        "signed model-profile classes differ from declared slots"
    );
    Ok(StagedModelProfileCatalog {
        catalog_sha256: sha256(&canonical_json(&catalog)?),
        catalog,
        artifact_sha256,
        component_id: component_id.clone(),
        solution: installation.plan.solution.clone(),
        blueprint_sha256: installation.plan.blueprint_sha256.clone(),
        config_version: installation.config_version.clone(),
        installation_generation: installation.generation,
        composition_sha256: installation.composition_sha256.clone(),
    })
}

impl AppState {
    pub(super) fn authorized_profile_reader(
        &self,
        verified: &VerifiedTenantContext,
        scope: &CustomerScope,
    ) -> anyhow::Result<VerifiedTenantContext> {
        let mut current = verified.clone();
        self.enterprise
            .hosted_policy
            .project(&mut current)
            .map_err(anyhow::Error::msg)?
            .context("synchronized hosted identity required")?;
        self.enterprise
            .hosted_policy
            .authorize_permission(Some(&current), AccessPermission::HostedUse)
            .map_err(anyhow::Error::msg)?;
        validate_customer_config_scope(&current, scope, crate::now_ms())?;
        Ok(current)
    }

    pub(super) async fn read_current_staged_installation(
        &self,
        context: VerifiedTenantContext,
        scope: CustomerScope,
        generation: u64,
        composition: String,
    ) -> anyhow::Result<SolutionInstallation> {
        let path = self.automation_v2_runs_path.clone();
        crate::encrypted_file_store::spawn_protected_blocking(move || {
            OrchestrationStateStore::from_automation_runs_path(&path)?
                .current_staged_solution_installation(
                    &context,
                    &scope,
                    generation,
                    &composition,
                    crate::now_ms(),
                )
        })
        .await?
    }

    /// Host-only read of the signed catalog in the current fully staged plan.
    /// The caller supplies an expected generation/composition from its protected
    /// goal binding, not a catalog, pack selector, or artifact path.
    #[allow(dead_code)]
    pub(crate) async fn load_current_staged_model_profile_catalog(
        &self,
        verified: &VerifiedTenantContext,
        scope: &CustomerScope,
        expected_generation: u64,
        expected_composition: &str,
    ) -> anyhow::Result<StagedModelProfileCatalog> {
        let context = self.authorized_profile_reader(verified, scope)?;
        let installation = self
            .read_current_staged_installation(
                context,
                scope.clone(),
                expected_generation,
                expected_composition.into(),
            )
            .await?;
        let pack = self
            .pack_manager
            .solution_artifacts_exact(
                &installation.plan.solution.id,
                &installation.plan.solution.version,
            )
            .await?;
        let catalog = catalog_from_installation(&installation, &pack)?;
        // Pack loading awaits filesystem locks. A second protected read and
        // policy projection reject a generation or membership change during it.
        let context = self.authorized_profile_reader(verified, scope)?;
        let current = self
            .read_current_staged_installation(
                context,
                scope.clone(),
                expected_generation,
                expected_composition.into(),
            )
            .await?;
        ensure!(
            current == installation,
            "solution installation changed while loading signed catalog"
        );
        Ok(catalog)
    }
}
