//! Short-lived evidence from an adapter-owned, live model-list request.
//! A configured model catalog or customer assertion never establishes health.

use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{ensure, Context};
use tandem_types::TenantContext;

use crate::{ProviderAuthOverride, ProviderRegistry, ProviderRuntimeBinding};

const AVAILABILITY_TTL_MS: u64 = 5_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProviderAvailabilityObservation {
    pub binding: ProviderRuntimeBinding,
    pub observed_at_ms: u64,
    pub available_until_ms: u64,
}

fn clock_ms() -> anyhow::Result<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

impl ProviderRegistry {
    /// Probe the concrete adapter/model using adapter-owned transport and auth.
    /// The caller must authorize the human, account and policy before invoking
    /// this. Unsupported adapters and failed model-list requests remain denied.
    pub async fn probe_model_availability_for_tenant(
        &self,
        tenant: &TenantContext,
        provider_id: &str,
        model_id: &str,
    ) -> anyhow::Result<ProviderAvailabilityObservation> {
        let before = self
            .runtime_binding_for_tenant(tenant, provider_id, model_id)
            .await?;
        let provider = {
            let providers = self.providers.read().await;
            let mut matches = providers
                .iter()
                .filter(|provider| provider.info().id == provider_id);
            let provider = matches
                .next()
                .context("availability provider unavailable")?
                .clone();
            ensure!(
                matches.next().is_none(),
                "ambiguous availability provider ID"
            );
            provider
        };
        let auth = self
            .auth_override_for_tenant(provider_id, Some(tenant))
            .await;
        ensure!(
            !matches!(auth, ProviderAuthOverride::Suppress),
            "availability credential unavailable"
        );
        provider.probe_model_availability(&auth, model_id).await?;
        let after = self
            .runtime_binding_for_tenant(tenant, provider_id, model_id)
            .await?;
        ensure!(
            after == before,
            "provider route changed during availability probe"
        );
        let observed_at_ms = clock_ms()?;
        let available_until_ms = observed_at_ms
            .checked_add(AVAILABILITY_TTL_MS)
            .context("availability observation timestamp overflow")?;
        Ok(ProviderAvailabilityObservation {
            binding: after,
            observed_at_ms,
            available_until_ms,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppConfig, ProviderConfig, PROVIDER_PRIVATE_ENDPOINTS_ALLOWED};
    use std::collections::HashMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn probe_with_list(
        body: &'static str,
    ) -> anyhow::Result<(anyhow::Result<ProviderAvailabilityObservation>, String)> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 4096];
            let size = socket.read(&mut request).await.unwrap();
            let received = String::from_utf8_lossy(&request[..size]).into_owned();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            socket.write_all(response.as_bytes()).await.unwrap();
            received
        });
        let registry = ProviderRegistry::new(AppConfig {
            providers: HashMap::from([(
                "llama_cpp".into(),
                ProviderConfig {
                    url: Some(format!("http://{address}/v1")),
                    api_key: Some("synthetic-probe-key".into()),
                    default_model: Some("synthetic-model".into()),
                },
            )]),
            default_provider: Some("llama_cpp".into()),
        });
        let result = PROVIDER_PRIVATE_ENDPOINTS_ALLOWED
            .scope(
                true,
                registry.probe_model_availability_for_tenant(
                    &TenantContext::local_implicit(),
                    "llama_cpp",
                    "synthetic-model",
                ),
            )
            .await;
        Ok((result, server.await?))
    }

    #[tokio::test]
    async fn live_model_list_proves_only_the_selected_route_without_completion_send() {
        let (result, request) = probe_with_list(r#"{"data":[{"id":"synthetic-model"}]}"#)
            .await
            .unwrap();
        let observation = result.unwrap();
        assert!(observation.available_until_ms > observation.observed_at_ms);
        assert!(observation.available_until_ms - observation.observed_at_ms <= AVAILABILITY_TTL_MS);
        assert!(request.starts_with("GET /v1/models HTTP/1.1"));
        assert!(!request.starts_with("POST "));

        let (denied, request) = probe_with_list(r#"{"data":[{"id":"different-model"}]}"#)
            .await
            .unwrap();
        assert!(denied.is_err());
        assert!(request.starts_with("GET /v1/models HTTP/1.1"));
    }
}
