//! Current account-use authorization over existing hosted/native grants and the
//! existing provider credential store. This snapshot does not authorize a run.

use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use tandem_enterprise_contract::{
    AccessDecision, AccessPermission, DataClass, ResourceKind, ResourceRef, VerifiedTenantContext,
};
use tandem_providers::{
    ProviderCredentialKind, ProviderCredentialLocation, VersionedProviderRuntimeBinding,
};
use tandem_solutions::{canonical_json, sha256, validate_customer_config_scope, CustomerScope};

use super::host_facts::{HostModel, HostSettings};
use crate::AppState;

#[cfg(test)]
tokio::task_local! {
    static MODEL_ACCOUNT_CHECKED: std::sync::Arc<tokio::sync::Notify>;
}

#[cfg(test)]
pub(crate) async fn scope_model_account_observation<F: std::future::Future>(
    observed: std::sync::Arc<tokio::sync::Notify>,
    future: F,
) -> F::Output {
    MODEL_ACCOUNT_CHECKED.scope(observed, future).await
}

/// Operator-managed configuration, not an HTTP account grant or secret value.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct HostAccountBinding {
    pub credential_kind: ProviderCredentialKind,
    pub credential_location: ProviderCredentialLocation,
    /// Explicit reconnect requires operator review of a new account revision.
    pub authorization_revision: String,
    pub resource: ResourceRef,
}

/// Non-deserializable host snapshot. Model capability/data/price policy,
/// installation activation and current run authority are separate prerequisites.
#[derive(Debug)]
pub struct SolutionModelAccount {
    pub verified: VerifiedTenantContext,
    pub binding_id: String,
    pub binding: VersionedProviderRuntimeBinding,
    pub authority_sha256: String,
}

impl AppState {
    async fn operator_model_account(&self, binding_id: &str) -> anyhow::Result<HostModel> {
        ensure!(
            !binding_id.is_empty() && binding_id.len() <= 256,
            "invalid model binding"
        );
        let layers = self.config.get_layers_value().await;
        let settings: HostSettings = serde_json::from_value(
            layers
                .get("cli")
                .and_then(|value| value.get("solution_installation"))
                .or_else(|| {
                    layers
                        .get("managed")
                        .and_then(|value| value.get("solution_installation"))
                })
                .cloned()
                .context("operator solution installation policy required")?,
        )?;
        ensure!(
            settings.schema_version == 1,
            "unsupported host installation settings version"
        );
        settings
            .models
            .get(binding_id)
            .cloned()
            .context("operator model account binding unavailable")
    }

    async fn current_model_account_context(
        &self,
        verified: &VerifiedTenantContext,
        scope: &CustomerScope,
        model: &HostModel,
    ) -> anyhow::Result<VerifiedTenantContext> {
        let mut context = verified.clone();
        let memberships = self
            .enterprise
            .hosted_policy
            .project(&mut context)
            .map_err(anyhow::Error::msg)?
            .context("synchronized hosted identity required")?;
        self.enterprise
            .hosted_policy
            .authorize_execution(Some(&context))
            .map_err(anyhow::Error::msg)?;
        crate::http::enrich_verified_context_with_org_unit_grants(
            self,
            &mut context,
            Some(memberships),
        )
        .await;
        validate_customer_config_scope(&context, scope, crate::now_ms())?;
        let account = model
            .account
            .as_ref()
            .context("explicit model account approval required")?;
        ensure!(
            account.resource.organization_id == scope.org_id
                && account.resource.workspace_id == scope.workspace_id
                && account.resource.resource_kind == ResourceKind::SecretProviderCredential
                && account.resource.resource_id == model.credential_ref
                && !account.authorization_revision.is_empty()
                && account.authorization_revision.len() <= 256,
            "model account resource or revision scope mismatch"
        );
        let strict = context
            .strict_projection
            .as_ref()
            .context("strict model account context required")?;
        ensure!(
            strict
                .evaluate_access(
                    &account.resource,
                    AccessPermission::Execute,
                    DataClass::Credential,
                    crate::now_ms()
                )
                .decision
                == AccessDecision::Allow,
            "current user is not permitted to use the model credential"
        );
        #[cfg(test)]
        let _ = MODEL_ACCOUNT_CHECKED.try_with(|observed| observed.notify_one());
        Ok(context)
    }

    /// Reuse current hosted identity, native credential resource grants and the
    /// persisted-to-loaded credential matcher. No refresh, request or secret
    /// value is returned, and tenant service scope never implies personal access.
    pub async fn authorize_solution_model_account(
        &self,
        verified: &VerifiedTenantContext,
        scope: &CustomerScope,
        binding_id: &str,
    ) -> anyhow::Result<SolutionModelAccount> {
        let model = self.operator_model_account(binding_id).await?;
        self.authorize_solution_model_account_binding(verified, scope, binding_id, model)
            .await
    }

    /// Installation passes its exact operator snapshot so authorization cannot
    /// silently approve a different model binding read after a configuration change.
    pub(super) async fn authorize_solution_model_account_binding(
        &self,
        verified: &VerifiedTenantContext,
        scope: &CustomerScope,
        binding_id: &str,
        model: HostModel,
    ) -> anyhow::Result<SolutionModelAccount> {
        let context = self
            .current_model_account_context(verified, scope, &model)
            .await?;
        let account = model
            .account
            .as_ref()
            .context("explicit model account approval required")?;
        let directory = crate::http::config_providers::provider_auth_security_dir_for_state(self);
        let binding = self
            .providers
            .versioned_runtime_binding_for_tenant_in_dir(
                &directory,
                &context.tenant_context,
                &model.provider_id,
                &model.model_id,
                account.credential_kind,
                account.credential_location,
            )
            .await?;
        ensure!(
            binding.revision.authorization_revision == account.authorization_revision,
            "model account was reconnected; review current authorization revision"
        );
        let current_model = self.operator_model_account(binding_id).await?;
        ensure!(
            canonical_json(&current_model)? == canonical_json(&model)?,
            "operator model account binding changed during resolution"
        );
        // Filesystem/keychain reads can wait. Reproject current grants after
        // that wait instead of reusing the initiating user's old projection.
        let context = self
            .current_model_account_context(verified, scope, &current_model)
            .await?;
        let authority_sha256 = sha256(&canonical_json(&serde_json::json!({
            "schema_version": 1, "scope": scope, "binding_id": binding_id,
            "model": current_model, "runtime": binding,
            "verified": context,
            "hosted_revision": self.enterprise.hosted_policy.revision().map_err(anyhow::Error::msg)?,
        }))?);
        Ok(SolutionModelAccount {
            verified: context,
            binding_id: binding_id.into(),
            binding,
            authority_sha256,
        })
    }
}
