//! Current concrete transport facts for trusted model policy. These are a
//! read-only snapshot, not an authorization or a durable account generation.

use anyhow::{ensure, Context};
use serde::Serialize;
use tandem_types::TenantContext;

use crate::{ProviderAttempt, ProviderAuthOverride, ProviderProtocol, ProviderRegistry};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderCredentialSource {
    HostService,
    TenantBearer,
    Unauthenticated,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ProviderRuntimeBinding {
    pub tenant_context: TenantContext,
    pub provider_id: String,
    pub model_id: String,
    pub protocol: ProviderProtocol,
    pub endpoint_sha256: String,
    pub credential_sha256: String,
    pub credential_source: ProviderCredentialSource,
}

impl ProviderRuntimeBinding {
    pub fn matches_attempt(&self, attempt: &ProviderAttempt) -> bool {
        self.provider_id == attempt.provider_id
            && self.model_id == attempt.model_id
            && self.protocol == attempt.protocol
            && self.endpoint_sha256 == attempt.endpoint_sha256
            && self.credential_sha256 == attempt.credential_sha256
    }
}

/// Adapter-owned request description. It never contains raw keys or endpoints.
/// Unknown custom adapters cannot construct this through browser configuration.
#[derive(Clone)]
pub struct ProviderTransportBinding {
    pub(crate) protocol: ProviderProtocol,
    pub(crate) endpoint_sha256: String,
    pub(crate) credential_sha256: String,
    pub(crate) credential_source: ProviderCredentialSource,
}

pub(crate) fn transport(
    endpoint: &str,
    protocol: ProviderProtocol,
    bearer: Option<&str>,
    api_key: Option<&str>,
    source: ProviderCredentialSource,
) -> anyhow::Result<ProviderTransportBinding> {
    let mut request = reqwest::Request::new(reqwest::Method::POST, reqwest::Url::parse(endpoint)?);
    ensure!(
        matches!(request.url().scheme(), "http" | "https")
            && request.url().host_str().is_some()
            && request.url().username().is_empty()
            && request.url().password().is_none(),
        "invalid provider transport endpoint"
    );
    if let Some(token) = bearer {
        ensure!(!token.trim().is_empty(), "empty provider credential");
        request.headers_mut().insert(
            reqwest::header::AUTHORIZATION,
            reqwest::header::HeaderValue::from_str(&format!("Bearer {token}"))?,
        );
    }
    if let Some(token) = api_key {
        ensure!(!token.trim().is_empty(), "empty provider credential");
        request
            .headers_mut()
            .insert("x-api-key", reqwest::header::HeaderValue::from_str(token)?);
    }
    let fingerprints = crate::attempt_accounting::request_fingerprints(&request);
    Ok(ProviderTransportBinding {
        protocol,
        endpoint_sha256: fingerprints.endpoint_sha256,
        credential_sha256: fingerprints.credential_sha256,
        credential_source: source,
    })
}

pub(crate) fn inherited_source(key: Option<&str>) -> ProviderCredentialSource {
    if key.is_some() {
        ProviderCredentialSource::HostService
    } else {
        ProviderCredentialSource::Unauthenticated
    }
}

impl ProviderRegistry {
    /// Resolve an explicit model from the actual configured adapter and current
    /// tenant authentication. No network, credential load/refresh or mutation.
    /// Callers must independently authorize the tenant and approve the account,
    /// model capabilities, prices, data classes and durable lifecycle revision.
    pub async fn runtime_binding_for_tenant(
        &self,
        tenant: &TenantContext,
        provider_id: &str,
        model_id: &str,
    ) -> anyhow::Result<ProviderRuntimeBinding> {
        ensure!(
            !provider_id.is_empty()
                && !model_id.is_empty()
                && provider_id == provider_id.trim()
                && model_id == model_id.trim(),
            "explicit provider and model IDs required"
        );
        // Keep the adapter read lock while inspecting tenant auth. A reload
        // cannot splice a new adapter into this snapshot. No mutation holds the
        // token lock while acquiring the adapter lock.
        let providers = self.providers.read().await;
        let mut matching = providers
            .iter()
            .filter(|provider| provider.info().id == provider_id);
        let provider = matching.next().context("runtime provider unavailable")?;
        ensure!(matching.next().is_none(), "ambiguous runtime provider ID");
        ensure!(
            provider.supports_attempt_accounting(),
            "provider lacks bounded attempt support"
        );
        let info = provider.info();
        ensure!(
            info.models
                .iter()
                .filter(|model| model.id == model_id && model.provider_id == provider_id)
                .count()
                == 1,
            "runtime model unavailable or ambiguous"
        );
        let auth = self
            .auth_override_for_tenant(provider_id, Some(tenant))
            .await;
        ensure!(
            !matches!(auth, ProviderAuthOverride::Suppress),
            "tenant provider credential missing"
        );
        let binding = provider.runtime_transport_binding(&auth)?;
        Ok(ProviderRuntimeBinding {
            tenant_context: tenant.clone(),
            provider_id: provider_id.into(),
            model_id: model_id.into(),
            protocol: binding.protocol,
            endpoint_sha256: binding.endpoint_sha256,
            credential_sha256: binding.credential_sha256,
            credential_source: binding.credential_source,
        })
    }
}
