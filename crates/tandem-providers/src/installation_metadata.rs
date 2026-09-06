//! Installation snapshots describe the concrete provider object, never infer
//! locality from a user-chosen provider ID. They do not authorize dispatch.

use serde::Serialize;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderInstallationMetadata {
    pub uses_network: bool,
    pub routing_sha256: String,
    pub is_test_provider: bool,
}

impl ProviderInstallationMetadata {
    pub(crate) fn network(provider_id: &str, endpoint: &str) -> Self {
        Self::new(provider_id, endpoint, true, false)
    }

    pub(crate) fn local_echo() -> Self {
        Self::new("local", "builtin-echo-v1", false, true)
    }

    fn new(provider_id: &str, endpoint: &str, uses_network: bool, is_test_provider: bool) -> Self {
        // Length-delimited input; only an opaque hash leaves the provider. No
        // credential values or raw endpoint URLs enter a customer plan.
        let mut digest = Sha256::new();
        digest.update(b"tandem-provider-route-v1");
        for value in [provider_id, endpoint] {
            digest.update((value.len() as u64).to_be_bytes());
            digest.update(value.as_bytes());
        }
        Self {
            uses_network,
            routing_sha256: format!("{:x}", digest.finalize()),
            is_test_provider,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{AppConfig, ProviderConfig, ProviderRegistry};
    use std::collections::HashMap;

    #[tokio::test]
    async fn installation_metadata_does_not_trust_a_network_provider_named_local() {
        let local = ProviderRegistry::new(AppConfig::default());
        let rows = local.installation_models().await;
        let (_, metadata) = rows.iter().find(|(info, _)| info.id == "local").unwrap();
        assert_eq!(
            metadata.as_ref(),
            Some(&ProviderInstallationMetadata::local_echo())
        );
        let config = |url: &str| AppConfig {
            providers: HashMap::from([(
                "local".into(),
                ProviderConfig {
                    url: Some(url.into()),
                    api_key: None,
                    default_model: Some("text-model".into()),
                },
            )]),
            default_provider: Some("local".into()),
        };
        let network = ProviderRegistry::new(config("https://provider.example/v1"));
        let rows = network.installation_models().await;
        let (_, metadata) = rows.iter().find(|(info, _)| info.id == "local").unwrap();
        let before = metadata.as_ref().unwrap();
        assert!(before.uses_network);
        assert!(!before.is_test_provider);
        network.reload(config("https://changed.example/v1")).await;
        let changed = network.installation_models().await;
        assert_ne!(
            before.routing_sha256,
            changed[0].1.as_ref().unwrap().routing_sha256
        );
        let serialized = serde_json::to_string(before).unwrap();
        assert!(!serialized.contains("provider.example"));
    }
}
