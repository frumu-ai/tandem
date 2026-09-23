//! Host-only construction of current model route facts. This is an observation,
//! not a model selection, provider request permit, or activation path.

use anyhow::{ensure, Context};
use tandem_enterprise_contract::VerifiedTenantContext;
use tandem_providers::ProviderDispatchAuthority;
use tandem_solutions::{canonical_json, sha256, CustomerScope, ModelRouteFacts};

use super::host_facts::{HostModel, HostRouteReview};
use crate::AppState;

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn current_review(
    model: &HostModel,
    review: &HostRouteReview,
    endpoint_sha256: &str,
    now_ms: u64,
) -> anyhow::Result<()> {
    let account = model
        .account
        .as_ref()
        .context("runtime model route requires a reviewed account")?;
    ensure!(
        review.schema_version == 1
            && !review.revision.is_empty()
            && review.revision.len() <= 256
            && review.provider_id == model.provider_id
            && review.model_id == model.model_id
            && review.credential_ref == model.credential_ref
            && review.authorization_revision == account.authorization_revision
            && valid_digest(&review.endpoint_sha256)
            && review.endpoint_sha256 == endpoint_sha256
            && now_ms <= review.reviewed_until_ms
            && now_ms <= review.price.valid_until_ms
            && !review.modalities.is_empty()
            && !review.processing_regions.is_empty()
            && review
                .processing_regions
                .iter()
                .all(|region| !region.trim().is_empty() && region.len() <= 128),
        "model route review is absent, stale or differs from the current account and transport"
    );
    Ok(())
}

impl AppState {
    /// Requires the exact protected installation and signed profile class,
    /// current human account-use grant, operator review, and a live adapter
    /// model-list observation. Rechecks all mutable facts after the network wait.
    #[allow(dead_code)]
    pub(crate) async fn observe_current_model_route(
        &self,
        verified: &VerifiedTenantContext,
        scope: &CustomerScope,
        expected_generation: u64,
        expected_composition: &str,
        class: &str,
    ) -> anyhow::Result<ModelRouteFacts> {
        let catalog = self
            .load_current_staged_model_profile_catalog(
                verified,
                scope,
                expected_generation,
                expected_composition,
            )
            .await?;
        ensure!(
            catalog.catalog.profiles.contains_key(class),
            "model class is absent from the current signed catalog"
        );
        let context = self.authorized_profile_reader(verified, scope)?;
        let installation = self
            .read_current_staged_installation(
                context,
                scope.clone(),
                expected_generation,
                expected_composition.into(),
            )
            .await?;
        // Until each reviewed route has its own protected plan receipt, only
        // the current installer/admin can reproduce the full approved host
        // facts. This prevents a review added after staging from widening an
        // older composition.
        let host_facts = self.solution_host_facts(verified, scope).await?;
        ensure!(
            installation.plan.host_facts_sha256.as_deref() == Some(host_facts.digest.as_str()),
            "operator route review differs from the staged host facts"
        );
        let locked = installation
            .plan
            .models
            .get(class)
            .context("current installation has no binding for model class")?
            .clone();
        let model = self.operator_model_account(&locked.binding_id).await?;
        ensure!(
            model.provider_id == locked.binding.provider
                && model.model_id == locked.binding.model
                && model.credential_ref == locked.binding.credential_ref,
            "operator model route differs from the staged installation"
        );
        let review = model
            .review
            .as_ref()
            .context("operator model route review required")?
            .clone();
        let account = self
            .authorize_solution_model_account_binding(
                verified,
                scope,
                &locked.binding_id,
                model.clone(),
            )
            .await?;
        current_review(
            &model,
            &review,
            &account.binding.runtime.endpoint_sha256,
            crate::now_ms(),
        )?;
        // DNS and endpoint resolution can wait. Repeat the human/account
        // authorization immediately before the adapter sends its model-list
        // request, even though this request carries no customer prompt.
        let state = self.clone();
        let guard_verified = verified.clone();
        let guard_scope = scope.clone();
        let guard_binding_id = locked.binding_id.clone();
        let guard_model = model.clone();
        let expected_authority = account.authority_sha256.clone();
        let authority = ProviderDispatchAuthority::new(move || {
            let state = state.clone();
            let verified = guard_verified.clone();
            let scope = guard_scope.clone();
            let binding_id = guard_binding_id.clone();
            let model = guard_model.clone();
            let expected_authority = expected_authority.clone();
            async move {
                let current = state
                    .authorize_solution_model_account_binding(&verified, &scope, &binding_id, model)
                    .await?;
                ensure!(
                    current.authority_sha256 == expected_authority,
                    "model account authority changed before availability request"
                );
                Ok(())
            }
        });
        let observed = authority
            .scope(self.providers.probe_model_availability_for_tenant(
                &account.verified.tenant_context,
                &model.provider_id,
                &model.model_id,
            ))
            .await?;
        ensure!(
            observed.binding == account.binding.runtime,
            "availability observation belongs to a different provider route"
        );
        let rechecked = self
            .authorize_solution_model_account_binding(
                verified,
                scope,
                &locked.binding_id,
                model.clone(),
            )
            .await?;
        ensure!(
            rechecked.authority_sha256 == account.authority_sha256
                && rechecked.binding == account.binding,
            "model account authority changed during availability observation"
        );
        let current_catalog = self
            .load_current_staged_model_profile_catalog(
                verified,
                scope,
                expected_generation,
                expected_composition,
            )
            .await?;
        ensure!(
            current_catalog.catalog_sha256 == catalog.catalog_sha256
                && current_catalog.artifact_sha256 == catalog.artifact_sha256
                && current_catalog.solution == catalog.solution,
            "signed model profile changed during availability observation"
        );
        let context = self.authorized_profile_reader(verified, scope)?;
        let current_installation = self
            .read_current_staged_installation(
                context,
                scope.clone(),
                expected_generation,
                expected_composition.into(),
            )
            .await?;
        ensure!(
            current_installation == installation,
            "model installation changed during availability observation"
        );
        let host_facts = self.solution_host_facts(verified, scope).await?;
        ensure!(
            current_installation.plan.host_facts_sha256.as_deref()
                == Some(host_facts.digest.as_str()),
            "operator route review changed during availability observation"
        );
        let now_ms = crate::now_ms();
        current_review(
            &model,
            &review,
            &rechecked.binding.runtime.endpoint_sha256,
            now_ms,
        )?;
        ensure!(
            observed.observed_at_ms <= now_ms && now_ms <= observed.available_until_ms,
            "model availability observation expired during authorization"
        );
        let binding_revision = sha256(&canonical_json(&serde_json::json!({
            "schema_version": 1,
            "scope": scope,
            "class": class,
            "host_facts_sha256": &host_facts.digest,
            "binding": &locked,
            "review": &review,
            "account_authority_sha256": &rechecked.authority_sha256,
        }))?);
        Ok(ModelRouteFacts {
            binding: locked,
            binding_revision,
            modalities: review.modalities,
            supports_tool_use: review.supports_tool_use,
            processing_regions: review.processing_regions,
            retention_hours: Some(review.retention_hours),
            available_until_ms: Some(
                observed
                    .available_until_ms
                    .min(review.reviewed_until_ms)
                    .min(review.price.valid_until_ms),
            ),
            price: Some(review.price),
        })
    }
}
