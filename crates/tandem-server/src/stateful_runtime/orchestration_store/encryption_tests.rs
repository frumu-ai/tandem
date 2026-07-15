// Copyright (c) 2026 Frumu LTD
// Licensed under the Business Source License 1.1

use serial_test::serial;
use sha2::{Digest, Sha256};
use tandem_memory::decrypt_broker::{MemoryDecryptBroker, MemoryDecryptBrokerConfig};
use tandem_memory::dek_cache::MemoryDekCache;
use tandem_memory::envelope_crypto::HostedMemoryEnvelopeCrypto;
use tandem_memory::kms_providers::{
    GoogleCloudKmsDecryptClient, GoogleCloudKmsDecryptRequest, GoogleCloudKmsDekUnwrapProvider,
    GoogleCloudKmsDekWrapProvider, GoogleCloudKmsEncryptClient, GoogleCloudKmsEncryptRequest,
};
use tandem_memory::types::{MemoryError, MemoryResult};
use tandem_memory::MemoryCryptoProvider;
use tandem_types::TenantContext;

use super::{protected_records, OrchestrationStateStore, OrchestrationStorePaths};
use crate::stateful_runtime::backend::Executor as _;

const PROVIDER_ID: &str = "google_cloud_kms";
const RUNTIME_PRINCIPAL: &str = "runtime-tandem";
const KEK_ID: &str = "projects/test/locations/global/keyRings/tandem/cryptoKeys/orchestration";

#[derive(Clone)]
struct FixtureKms(u8);

impl GoogleCloudKmsEncryptClient for FixtureKms {
    fn encrypt(&self, request: &GoogleCloudKmsEncryptRequest) -> MemoryResult<Vec<u8>> {
        let mut wrapped = Sha256::digest(&request.additional_authenticated_data).to_vec();
        wrapped.extend(request.plaintext.iter().map(|byte| byte ^ self.0));
        Ok(wrapped)
    }
}

impl GoogleCloudKmsDecryptClient for FixtureKms {
    fn decrypt(&self, request: &GoogleCloudKmsDecryptRequest) -> MemoryResult<Vec<u8>> {
        let expected = Sha256::digest(&request.additional_authenticated_data);
        if request.ciphertext.len() < expected.len() {
            return Err(MemoryError::InvalidConfig(
                "fixture KMS ciphertext is truncated".to_string(),
            ));
        }
        let (actual, ciphertext) = request.ciphertext.split_at(expected.len());
        if actual != &expected[..] {
            return Err(MemoryError::InvalidConfig(
                "fixture KMS authenticated data mismatch".to_string(),
            ));
        }
        Ok(ciphertext.iter().map(|byte| byte ^ self.0).collect())
    }
}

fn hosted_provider(fingerprint: u8) -> MemoryCryptoProvider {
    let config = MemoryDecryptBrokerConfig::hosted(PROVIDER_ID, RUNTIME_PRINCIPAL).unwrap();
    let kms = FixtureKms(fingerprint);
    MemoryCryptoProvider::hosted(HostedMemoryEnvelopeCrypto::new(
        MemoryDecryptBroker::new(config).unwrap(),
        Box::new(GoogleCloudKmsDekWrapProvider::new(kms.clone(), RUNTIME_PRINCIPAL).unwrap()),
        Box::new(GoogleCloudKmsDekUnwrapProvider::new(kms, RUNTIME_PRINCIPAL).unwrap()),
        MemoryDekCache::new(64),
        PROVIDER_ID,
        RUNTIME_PRINCIPAL,
        KEK_ID,
        "1",
        0,
    ))
}

fn tenant(org: &str, actor: &str) -> TenantContext {
    TenantContext::explicit_user_workspace(org, "workspace-a", Some("prod".to_string()), actor)
}

#[tokio::test]
async fn local_plaintext_round_trips_and_reads_legacy_json() {
    crate::encrypted_file_store::with_test_crypto_provider(
        MemoryCryptoProvider::plaintext(),
        None,
        async {
            let tenant = tenant("org-a", "user-a");
            let value = serde_json::json!({"secret": "value"});
            let stored = protected_records::encode(&tenant, "goal", "goal-1", &value).unwrap();
            assert!(!crate::encrypted_file_store::is_encrypted_payload(&stored));
            assert_eq!(
                protected_records::decode::<serde_json::Value>(&tenant, "goal", "goal-1", &stored,)
                    .unwrap(),
                value
            );
            assert_eq!(
                protected_records::decode::<serde_json::Value>(
                    &tenant,
                    "goal",
                    "goal-1",
                    r#"{"secret":"legacy"}"#,
                )
                .unwrap(),
                serde_json::json!({"secret": "legacy"})
            );
        },
    )
    .await;
}

#[tokio::test]
async fn protected_records_bind_tenant_scope_kind_and_id() {
    crate::encrypted_file_store::with_test_crypto_provider(
        MemoryCryptoProvider::local_key([0x5a; 32]),
        None,
        async {
            let tenant_a = tenant("org-a", "user-a");
            let value = serde_json::json!({"secret": "value"});
            let stored = protected_records::encode(&tenant_a, "run", "run-1", &value).unwrap();
            assert_eq!(
                protected_records::decode::<serde_json::Value>(
                    &tenant("org-a", "user-b"),
                    "run",
                    "run-1",
                    &stored,
                )
                .unwrap(),
                value
            );
            assert!(protected_records::decode::<serde_json::Value>(
                &tenant("org-b", "user-a"),
                "run",
                "run-1",
                &stored,
            )
            .is_err());
            assert!(protected_records::decode::<serde_json::Value>(
                &tenant_a, "goal", "run-1", &stored,
            )
            .is_err());
            assert!(protected_records::decode::<serde_json::Value>(
                &tenant_a, "run", "run-2", &stored,
            )
            .is_err());
        },
    )
    .await;
}

#[tokio::test]
async fn randomized_ciphertext_uses_stable_tenant_scoped_digest() {
    crate::encrypted_file_store::with_test_crypto_provider(
        MemoryCryptoProvider::local_key([0x33; 32]),
        None,
        async {
            let tenant_a = tenant("org-a", "user-a");
            let tenant_b = tenant("org-b", "user-a");
            let value = serde_json::json!({"status": "settled"});
            let first = protected_records::encode(&tenant_a, "wait", "wait-1", &value).unwrap();
            let second = protected_records::encode(&tenant_a, "wait", "wait-1", &value).unwrap();
            assert_ne!(first, second);
            assert_eq!(
                protected_records::digest(&tenant_a, "wait", &value).unwrap(),
                protected_records::digest(&tenant_a, "wait", &value).unwrap()
            );
            assert_ne!(
                protected_records::digest(&tenant_a, "projection", &value).unwrap(),
                protected_records::digest(&tenant_b, "projection", &value).unwrap()
            );
            assert_eq!(
                protected_records::digest(&tenant_a, "projection", &value).unwrap(),
                protected_records::digest(&tenant("org-a", "user-b"), "projection", &value,)
                    .unwrap()
            );
        },
    )
    .await;
}

#[tokio::test]
#[serial]
async fn hosted_required_without_kms_fails_closed() {
    let names = [
        "TANDEM_MEMORY_ENCRYPTION_REQUIRED",
        "TANDEM_MEMORY_DECRYPT_PROVIDER",
        "TANDEM_MEMORY_LOCAL_KEY_FILE",
        "TANDEM_MEMORY_GOOGLE_KMS_ENCRYPT_COMMAND",
        "TANDEM_MEMORY_GOOGLE_KMS_DECRYPT_COMMAND",
        "TANDEM_MEMORY_KEK_ID",
        "TANDEM_MEMORY_KEK_VERSION",
    ];
    let previous = names.map(|name| std::env::var(name).ok());
    std::env::set_var("TANDEM_MEMORY_ENCRYPTION_REQUIRED", "true");
    std::env::set_var("TANDEM_MEMORY_DECRYPT_PROVIDER", "google_cloud_kms");
    for name in &names[2..] {
        std::env::remove_var(name);
    }

    let provider = MemoryCryptoProvider::from_mode(tandem_memory::MemoryCryptoMode::HostedKms {
        provider: "google_cloud_kms".to_string(),
    });
    let error = crate::encrypted_file_store::with_test_crypto_provider(provider, None, async {
        protected_records::encode(
            &tenant("org-a", "user-a"),
            "goal",
            "goal-1",
            &serde_json::json!({"secret": "must-not-land"}),
        )
        .expect_err("hosted mode must not fall back to plaintext")
    })
    .await;

    for (name, value) in names.into_iter().zip(previous) {
        match value {
            Some(value) => std::env::set_var(name, value),
            None => std::env::remove_var(name),
        }
    }
    assert!(format!("{error:?}").contains("refusing to store plaintext"));
}

#[tokio::test]
async fn hosted_kms_store_row_round_trips_and_rejects_wrong_key_or_scope() {
    let directory = tempfile::tempdir().unwrap();
    let store = OrchestrationStateStore::open(OrchestrationStorePaths {
        database_path: directory.path().join("runtime.sqlite3"),
        engine_lock_path: directory.path().join("engine.lock"),
    })
    .unwrap();
    let authorized_tenant = tenant("org-a", "user-a");
    let foreign = tenant("org-b", "user-a");
    let response = serde_json::json!({"result": "hosted-secret"});

    crate::encrypted_file_store::with_test_crypto_provider(
        hosted_provider(0x5a),
        Some(RUNTIME_PRINCIPAL),
        async {
            assert!(store
                .begin_orchestration_tool_request(
                    &authorized_tenant,
                    "publish",
                    "key-1",
                    "digest-1",
                    1,
                )
                .unwrap()
                .is_none());
            store
                .complete_orchestration_tool_request(
                    &authorized_tenant,
                    "publish",
                    "key-1",
                    "digest-1",
                    &response,
                    2,
                )
                .unwrap();
            let stored = store
                .with_connection(|connection| {
                    Ok(connection.query_row(
                        "SELECT response_json FROM orchestration_tool_requests",
                        [],
                        |row| row.get::<_, String>(0),
                    )?)
                })
                .unwrap();
            assert!(stored.starts_with(crate::encrypted_file_store::SCOPED_RECORD_PREFIX));
            assert!(!stored.contains("hosted-secret"));
            assert_eq!(
                store
                    .completed_orchestration_tool_request(
                        &authorized_tenant,
                        "publish",
                        "key-1",
                        "digest-1",
                    )
                    .unwrap(),
                Some(response.clone())
            );
            assert!(store
                .completed_orchestration_tool_request(&foreign, "publish", "key-1", "digest-1",)
                .unwrap()
                .is_none());
        },
    )
    .await;

    crate::encrypted_file_store::with_test_crypto_provider(
        hosted_provider(0x33),
        Some(RUNTIME_PRINCIPAL),
        async {
            assert!(store
                .completed_orchestration_tool_request(
                    &authorized_tenant,
                    "publish",
                    "key-1",
                    "digest-1",
                )
                .is_err());
        },
    )
    .await;
}
