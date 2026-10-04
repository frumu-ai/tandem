//! Join current registry transport facts to the selected persisted credential.
//! A matching revision is necessary for model approval, never sufficient authority.
use std::path::Path;

use anyhow::{ensure, Context};
use serde::{Deserialize, Serialize};
use tandem_types::TenantContext;

use crate::{
    ProviderCredentialKind, ProviderCredentialRevision, ProviderCredentialSource, ProviderRegistry,
    ProviderRuntimeBinding,
};

/// Explicit storage scope. Tenant service credentials are scoped to the existing
/// organization/workspace/deployment key, not to an individual human actor.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCredentialLocation {
    HostService,
    TenantService,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct VersionedProviderRuntimeBinding {
    pub runtime: ProviderRuntimeBinding,
    pub credential_kind: ProviderCredentialKind,
    pub credential_location: ProviderCredentialLocation,
    pub revision: ProviderCredentialRevision,
}

impl ProviderRegistry {
    /// Resolve a model, verify that its loaded credential is the selected stored
    /// material at the recorded revision, then recheck registry facts. An unrelated
    /// record with a fresh revision cannot approve a stale loaded bearer token.
    ///
    /// This is an optimistic snapshot: callers must separately authorize account
    /// sharing/current user/data policy and revalidate at every physical attempt.
    /// No secret, endpoint, account identifier or credential refresh is returned.
    pub async fn versioned_runtime_binding_for_tenant_in_dir(
        &self,
        security_dir: &Path,
        tenant: &TenantContext,
        provider_id: &str,
        model_id: &str,
        credential_kind: ProviderCredentialKind,
        credential_location: ProviderCredentialLocation,
    ) -> anyhow::Result<VersionedProviderRuntimeBinding> {
        let runtime = self
            .runtime_binding_for_tenant(tenant, provider_id, model_id)
            .await?;
        let expected_source = match credential_location {
            ProviderCredentialLocation::HostService => ProviderCredentialSource::HostService,
            ProviderCredentialLocation::TenantService if tenant.is_local_implicit() => {
                anyhow::bail!("explicit tenant required for tenant service credential")
            }
            ProviderCredentialLocation::TenantService => ProviderCredentialSource::TenantBearer,
        };
        ensure!(
            runtime.credential_source == expected_source,
            "runtime credential source differs from selected account scope"
        );
        let directory = security_dir.to_path_buf();
        let observed = runtime.clone();
        // Filesystem/keychain locking must not block the asynchronous worker.
        let revision = tokio::task::spawn_blocking(move || {
            crate::provider_auth_store::match_runtime_credential_revision(
                &directory,
                &observed,
                credential_kind,
                credential_location,
            )
        })
        .await
        .context("join credential binding lookup")??;
        let current = self
            .runtime_binding_for_tenant(tenant, provider_id, model_id)
            .await?;
        ensure!(
            current == runtime,
            "runtime binding changed while reading credential revision"
        );
        Ok(VersionedProviderRuntimeBinding {
            runtime,
            credential_kind,
            credential_location,
            revision,
        })
    }
}
