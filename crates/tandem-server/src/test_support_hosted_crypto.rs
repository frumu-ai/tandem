// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

//! Deterministic in-process KMS for dependent-crate hosted HTTP tests.
use tandem_memory::decrypt_broker::{MemoryDecryptBroker, MemoryDecryptBrokerConfig};
use tandem_memory::dek_cache::MemoryDekCache;
use tandem_memory::envelope_crypto::HostedMemoryEnvelopeCrypto;
use tandem_memory::kms_providers::{
    GoogleCloudKmsDecryptClient, GoogleCloudKmsDecryptRequest, GoogleCloudKmsDekUnwrapProvider,
    GoogleCloudKmsDekWrapProvider, GoogleCloudKmsEncryptClient, GoogleCloudKmsEncryptRequest,
};
use tandem_memory::types::MemoryResult;
use tandem_memory::MemoryCryptoProvider;

const PROVIDER_ID: &str = "google_cloud_kms";
const RUNTIME_PRINCIPAL: &str = "runtime-tandem";
const KEK_ID: &str = "projects/test/locations/global/keyRings/tandem/cryptoKeys/governance";

#[derive(Clone)]
struct TestKms;

impl GoogleCloudKmsEncryptClient for TestKms {
    fn encrypt(&self, request: &GoogleCloudKmsEncryptRequest) -> MemoryResult<Vec<u8>> {
        assert!(!request.additional_authenticated_data.is_empty());
        Ok(request.plaintext.iter().map(|byte| byte ^ 0x5a).collect())
    }
}

impl GoogleCloudKmsDecryptClient for TestKms {
    fn decrypt(&self, request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>> {
        assert!(!request.additional_authenticated_data.is_empty());
        Ok(request.ciphertext.iter().map(|byte| byte ^ 0x5a).collect())
    }
}

/// Scope a hosted-memory provider to one async test task. No environment or
/// external KMS process is changed, and the production crypto path is used.
pub async fn with_hosted_crypto_for_test<F, T>(future: F) -> T
where
    F: std::future::Future<Output = T>,
{
    let config = MemoryDecryptBrokerConfig::hosted(PROVIDER_ID, RUNTIME_PRINCIPAL)
        .expect("hosted fixture config");
    let broker = MemoryDecryptBroker::new(config).expect("hosted fixture broker");
    let wrap = GoogleCloudKmsDekWrapProvider::new(TestKms, RUNTIME_PRINCIPAL)
        .expect("hosted fixture wrap");
    let unwrap = GoogleCloudKmsDekUnwrapProvider::new(TestKms, RUNTIME_PRINCIPAL)
        .expect("hosted fixture unwrap");
    let provider = MemoryCryptoProvider::hosted(HostedMemoryEnvelopeCrypto::new(
        broker,
        Box::new(wrap),
        Box::new(unwrap),
        MemoryDekCache::new(64),
        PROVIDER_ID,
        RUNTIME_PRINCIPAL,
        KEK_ID,
        "1",
        0,
    ));
    crate::encrypted_file_store::with_test_crypto_provider(
        provider,
        Some(RUNTIME_PRINCIPAL),
        future,
    )
    .await
}
