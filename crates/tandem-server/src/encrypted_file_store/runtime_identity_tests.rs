// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use super::*;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tandem_memory::decrypt_broker::{MemoryDecryptBroker, MemoryDecryptBrokerConfig};
use tandem_memory::dek_cache::MemoryDekCache;
use tandem_memory::envelope_crypto::HostedMemoryEnvelopeCrypto;
use tandem_memory::kms_providers::{
    GoogleCloudKmsDecryptClient, GoogleCloudKmsDecryptRequest, GoogleCloudKmsDekUnwrapProvider,
    GoogleCloudKmsDekWrapProvider, GoogleCloudKmsEncryptClient, GoogleCloudKmsEncryptRequest,
};
use tandem_memory::types::{MemoryError, MemoryResult};

const PROVIDER: &str = "google_cloud_kms";
const RUNTIME: &str = "runtime-file-worker";
const SECRET: &str = "synthetic-private-file-record";

#[derive(Clone, Default)]
struct KmsCalls {
    wrap: Arc<AtomicUsize>,
    unwrap: Arc<AtomicUsize>,
}

#[derive(Clone)]
struct CountingKms(KmsCalls);

impl GoogleCloudKmsEncryptClient for CountingKms {
    fn encrypt(&self, request: &GoogleCloudKmsEncryptRequest) -> MemoryResult<Vec<u8>> {
        self.0.wrap.fetch_add(1, Ordering::SeqCst);
        let mut wrapped = Sha256::digest(&request.additional_authenticated_data).to_vec();
        wrapped.extend(request.plaintext.iter().map(|byte| byte ^ 0x5a));
        Ok(wrapped)
    }
}

impl GoogleCloudKmsDecryptClient for CountingKms {
    fn decrypt(&self, request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>> {
        self.0.unwrap.fetch_add(1, Ordering::SeqCst);
        let expected = Sha256::digest(&request.additional_authenticated_data);
        if request.ciphertext.len() < expected.len() {
            return Err(MemoryError::InvalidConfig(
                "fixture ciphertext truncated".into(),
            ));
        }
        let (actual, ciphertext) = request.ciphertext.split_at(expected.len());
        if actual != &expected[..] {
            return Err(MemoryError::InvalidConfig("fixture AAD mismatch".into()));
        }
        Ok(ciphertext.iter().map(|byte| byte ^ 0x5a).collect())
    }
}

fn tenant() -> MemoryTenantScope {
    MemoryTenantScope {
        org_id: "acme".into(),
        workspace_id: "hq".into(),
        deployment_id: Some("test".into()),
    }
}

fn context() -> ProtectedRecordContext {
    ProtectedRecordContext::new(
        MemoryKeyScope::new(
            &tenant(),
            DataClass::Restricted,
            Some("runtime-file-test".into()),
        ),
        "runtime-file-policy",
        "runtime-file-audit",
    )
}

fn hosted(
    broker_config: MemoryDecryptBrokerConfig,
    envelope_provider: &str,
    envelope_runtime: &str,
    calls: &KmsCalls,
) -> (MemoryCryptoProvider, MemoryDekCache) {
    let kms = CountingKms(calls.clone());
    let cache = MemoryDekCache::new(16);
    let provider = MemoryCryptoProvider::hosted(HostedMemoryEnvelopeCrypto::new(
        MemoryDecryptBroker::new(broker_config).expect("fixture broker"),
        Box::new(
            GoogleCloudKmsDekWrapProvider::new(kms.clone(), envelope_runtime)
                .expect("fixture wrap provider"),
        ),
        Box::new(
            GoogleCloudKmsDekUnwrapProvider::new(kms, envelope_runtime)
                .expect("fixture unwrap provider"),
        ),
        cache.clone(),
        envelope_provider,
        envelope_runtime,
        "projects/test/locations/global/keyRings/tandem/cryptoKeys/runtime-files",
        "1",
        0,
    ));
    (provider, cache)
}

fn healthy(calls: &KmsCalls) -> (MemoryCryptoProvider, MemoryDekCache) {
    hosted(
        MemoryDecryptBrokerConfig::hosted(PROVIDER, RUNTIME).expect("fixture config"),
        PROVIDER,
        RUNTIME,
        calls,
    )
}

fn file_crypto(provider: MemoryCryptoProvider, principal: Option<&str>) -> ProtectedFileCrypto {
    ProtectedFileCrypto {
        provider,
        principal_id: principal.map(ToOwned::to_owned),
    }
}

fn denied_principals() -> [Option<&'static str>; 6] {
    [
        None,
        Some("other-worker"),
        Some("RUNTIME-FILE-WORKER"),
        Some(" runtime-file-worker "),
        Some(""),
        Some("*"),
    ]
}

#[test]
fn hosted_file_seal_rejects_missing_or_wrong_runtime_before_kms_or_cache() {
    let calls = KmsCalls::default();
    let (provider, cache) = healthy(&calls);
    for principal in denied_principals() {
        let error = file_crypto(provider.clone(), principal)
            .encrypt_record(SECRET, &context())
            .expect_err("wrong runtime must not seal");
        assert!(error.to_string().contains("runtime principal"));
        assert_eq!(calls.wrap.load(Ordering::SeqCst), 0);
        assert_eq!(calls.unwrap.load(Ordering::SeqCst), 0);
        assert!(cache.is_empty());
    }
    let valid = file_crypto(provider, Some(RUNTIME));
    let stored = valid
        .encrypt_record(SECRET, &context())
        .expect("valid seal");
    assert!(!stored.contains(SECRET));
    assert_eq!(calls.wrap.load(Ordering::SeqCst), 1);
    assert_eq!(
        valid
            .decrypt_record(&stored, &context())
            .expect("valid read"),
        SECRET
    );
}

#[test]
fn hosted_file_cold_unseal_rejects_wrong_runtime_before_kms() {
    let writer_calls = KmsCalls::default();
    let (writer, _) = healthy(&writer_calls);
    let stored = file_crypto(writer, Some(RUNTIME))
        .encrypt_record(SECRET, &context())
        .expect("valid seal");
    let calls = KmsCalls::default();
    let (provider, cache) = healthy(&calls);
    for principal in denied_principals() {
        file_crypto(provider.clone(), principal)
            .decrypt_record(&stored, &context())
            .expect_err("wrong cold runtime must not unseal");
        assert_eq!(calls.unwrap.load(Ordering::SeqCst), 0);
        assert!(cache.is_empty());
    }
    assert_eq!(
        file_crypto(provider, Some(RUNTIME))
            .decrypt_record(&stored, &context())
            .expect("valid cold read"),
        SECRET
    );
    assert_eq!(calls.unwrap.load(Ordering::SeqCst), 1);
}

#[test]
fn hosted_pending_file_seal_and_open_preserve_unavailable_provider_error() {
    let calls = KmsCalls::default();
    let (writer, _) = healthy(&calls);
    let stored = file_crypto(writer, Some(RUNTIME))
        .encrypt_record(SECRET, &context())
        .expect("valid encrypted record");
    let pending = MemoryCryptoProvider::from_mode(tandem_memory::MemoryCryptoMode::HostedKms {
        provider: PROVIDER.into(),
    });
    assert!(pending.is_hosted());
    assert!(
        !pending.is_encrypted_ready(),
        "fixture KMS is unprovisioned"
    );
    for principal in [None, Some(RUNTIME), Some("other-worker")] {
        let crypto = file_crypto(pending.clone(), principal);
        for error in [
            crypto
                .encrypt_record(SECRET, &context())
                .expect_err("unprovisioned hosted writes deny"),
            crypto
                .decrypt_record(&stored, &context())
                .expect_err("unprovisioned hosted reads deny"),
        ] {
            assert!(
                format!("{error:?}").contains("refusing to store plaintext"),
                "unavailable-provider error must remain actionable: {error:?}"
            );
        }
    }
    assert_eq!(calls.wrap.load(Ordering::SeqCst), 1);
    assert_eq!(calls.unwrap.load(Ordering::SeqCst), 0);
}

#[test]
fn hosted_file_warm_unseal_rejects_wrong_runtime_without_cache_bypass() {
    let calls = KmsCalls::default();
    let (provider, cache) = healthy(&calls);
    let valid = file_crypto(provider.clone(), Some(RUNTIME));
    let stored = valid
        .encrypt_record(SECRET, &context())
        .expect("valid seal");
    assert_eq!(cache.len(), 1);
    for principal in denied_principals() {
        file_crypto(provider.clone(), principal)
            .decrypt_record(&stored, &context())
            .expect_err("wrong warm runtime must not unseal");
        assert_eq!(calls.unwrap.load(Ordering::SeqCst), 0);
        assert_eq!(cache.len(), 1);
    }
    assert_eq!(
        valid
            .decrypt_record(&stored, &context())
            .expect("valid warm read"),
        SECRET
    );
    assert_eq!(calls.unwrap.load(Ordering::SeqCst), 0);
}

#[test]
fn hosted_file_incoherent_runtime_handles_deny_seal_and_unseal() {
    let (writer, _) = healthy(&KmsCalls::default());
    let stored = file_crypto(writer, Some(RUNTIME))
        .encrypt_record(SECRET, &context())
        .expect("valid seal");
    for (config, envelope_runtime) in [
        (
            MemoryDecryptBrokerConfig::hosted(PROVIDER, "other-broker-worker").expect("config"),
            RUNTIME,
        ),
        (MemoryDecryptBrokerConfig::local_disabled(), RUNTIME),
        (
            MemoryDecryptBrokerConfig::hosted("other_kms", RUNTIME).expect("config"),
            RUNTIME,
        ),
        (
            MemoryDecryptBrokerConfig::hosted(PROVIDER, " runtime-file-worker ").expect("config"),
            " runtime-file-worker ",
        ),
    ] {
        let calls = KmsCalls::default();
        let (provider, cache) = hosted(config, PROVIDER, envelope_runtime, &calls);
        let crypto = file_crypto(provider, Some(envelope_runtime));
        crypto
            .encrypt_record(SECRET, &context())
            .expect_err("incoherent seal denied");
        crypto
            .decrypt_record(&stored, &context())
            .expect_err("incoherent unseal denied");
        assert_eq!(calls.wrap.load(Ordering::SeqCst), 0);
        assert_eq!(calls.unwrap.load(Ordering::SeqCst), 0);
        assert!(cache.is_empty());
    }
}

#[test]
fn hosted_file_provider_aliases_preserve_exact_runtime_binding() {
    for (broker_provider, envelope_provider) in [
        ("google_kms", PROVIDER),
        (PROVIDER, "gcp_kms"),
        ("Google.Cloud.Kms", "google-cloud-kms"),
    ] {
        let calls = KmsCalls::default();
        let (provider, _) = hosted(
            MemoryDecryptBrokerConfig::hosted(broker_provider, RUNTIME).expect("alias config"),
            envelope_provider,
            RUNTIME,
            &calls,
        );
        let crypto = file_crypto(provider, Some(RUNTIME));
        let stored = crypto
            .encrypt_record(SECRET, &context())
            .expect("alias seal");
        crypto.provider.clear_hosted_dek_cache();
        assert_eq!(
            crypto
                .decrypt_record(&stored, &context())
                .expect("alias cold read"),
            SECRET
        );
        assert_eq!(calls.wrap.load(Ordering::SeqCst), 1);
        assert_eq!(calls.unwrap.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn local_file_crypto_keeps_standalone_roundtrip_without_runtime_principal() {
    for provider in [
        MemoryCryptoProvider::plaintext(),
        MemoryCryptoProvider::local_key([0x5a; 32]),
    ] {
        let crypto = file_crypto(provider, None);
        let stored = crypto
            .encrypt_record(SECRET, &context())
            .expect("local seal");
        assert_eq!(
            crypto
                .decrypt_record(&stored, &context())
                .expect("local read"),
            SECRET
        );
    }
}

#[test]
fn generic_scoped_retrieval_keeps_distinct_caller_identity() {
    let calls = KmsCalls::default();
    let (provider, _) = healthy(&calls);
    let expected = context();
    let (stored, envelope) = provider
        .encrypt_field_scoped(
            SECRET,
            &expected.key_scope,
            &expected.policy_decision_id,
            &expected.audit_id,
        )
        .expect("generic seal");
    provider.clear_hosted_dek_cache();
    let principal = MemoryDecryptPrincipal::retrieval_gateway(
        "distinct-retrieval-gateway",
        tenant(),
        vec![DataClass::Restricted],
        vec!["runtime-file-test".into()],
    );
    assert_eq!(
        provider
            .decrypt_field_scoped_authorized(
                &stored,
                envelope.as_ref(),
                Some(&principal),
                &expected.authority(),
                None,
            )
            .expect("distinct generic caller remains authorized"),
        SECRET
    );
    assert_eq!(calls.unwrap.load(Ordering::SeqCst), 1);
}
